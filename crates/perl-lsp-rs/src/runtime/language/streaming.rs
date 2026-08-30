//! Streaming inline completion handler.
//!
//! Implements the custom `textDocument/perlInlineCompletionStream` request.
//! This handler starts a streaming session that emits cumulative inline
//! completion candidates via `$/progress` notifications. The final JSON-RPC
//! response is `null` -- all data is delivered through progress tokens.

use super::super::{JsonRpcError, LspServer, Value, json};
use crate::protocol::{invalid_params, req_position, req_uri};
use crate::runtime::language::misc::{
    ExternalCompletionOutcome, evaluate_external_candidates, external_completion_permitted,
    inline_completion_trigger_kind, selected_inline_completion_info,
};
use crate::runtime::stream_session::{SessionKey, StreamTerminalOutcome};
use perl_lsp_rs_core::providers::inline_completion::{BackendError, InlineCompletionItem};
use std::time::{Duration, Instant};

/// Build one `$/progress` payload for the streaming inline-completion feature.
///
/// Items without an explicit replacement range collapse to a zero-length range
/// at the request cursor, matching the wire contract clients already consume.
fn stream_progress_payload(
    token: &str,
    session_id: &str,
    sequence: u64,
    is_final: bool,
    items: Vec<InlineCompletionItem>,
    line: u32,
    character: u32,
) -> Value {
    let items = items
        .into_iter()
        .map(|item| {
            let range = item.range.unwrap_or(lsp_types::Range {
                start: lsp_types::Position { line, character },
                end: lsp_types::Position { line, character },
            });
            json!({
                "insertText": item.insert_text,
                "range": {
                    "start": { "line": range.start.line, "character": range.start.character },
                    "end": { "line": range.end.line, "character": range.end.character }
                }
            })
        })
        .collect::<Vec<_>>();

    json!({
        "token": token,
        "value": {
            "kind": "perlInlineCompletionStream",
            "sessionId": session_id,
            "sequence": sequence,
            "isFinal": is_final,
            "items": items
        }
    })
}

impl LspServer {
    /// Handle `textDocument/perlInlineCompletionStream` custom request.
    ///
    /// Starts a streaming session that emits cumulative candidates via `$/progress`.
    /// The final JSON-RPC response is `null` (all data sent via progress).
    ///
    /// If the client does not supply a `partialResultToken`, falls back to the
    /// standard one-shot `textDocument/inlineCompletion` handler.
    pub(crate) fn handle_streaming_inline_completion(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        let params = params.ok_or_else(|| invalid_params("missing params"))?;

        let uri = req_uri(&params)?;
        let (line, character) = req_position(&params)?;
        // Parse the actual request context the same way the standard route
        // does, so the stream applies the identical trigger and
        // selected-completion policy.
        let trigger_kind = inline_completion_trigger_kind(&params)?;
        let selected_completion = selected_inline_completion_info(&params)?;
        let partial_result_token =
            params.get("partialResultToken").and_then(|v| v.as_str()).map(|s| s.to_string());
        // The request's document version, when the client supplies one. An
        // absent version is "unknown", not zero: it cannot prove staleness.
        let request_document_version =
            params.get("textDocument").and_then(|td| td.get("version")).and_then(|v| v.as_i64());
        let document_version = request_document_version.unwrap_or(0);

        // An automatic custom-stream request can never display external output
        // unbidden: delegate to the standard deterministic-only route before
        // any session or backend work.
        if !external_completion_permitted(trigger_kind) {
            return self.handle_inline_completion(Some(params));
        }

        // Must have a partial result token for streaming
        let token = match partial_result_token {
            Some(t) => t,
            None => {
                // Fall back to one-shot inline completion
                return self.handle_inline_completion(Some(params));
            }
        };

        // Snapshot text plus its version/generation identity under the
        // document lock. The identity binds the prepared AI context to one
        // immutable snapshot, matching the buffered route.
        let (text, snapshot_identity) = {
            let documents = self.documents_guard();
            match self.get_document(&documents, uri) {
                Some(doc) => (
                    doc.text_arc.to_string(),
                    perl_lsp_rs_core::providers::inline_completion::InlineCompletionSnapshotIdentity {
                        document_version: Some(i64::from(doc.version)),
                        source_generation: Some(u64::from(
                            doc.generation.load(std::sync::atomic::Ordering::Acquire),
                        )),
                    },
                ),
                None => return Ok(Some(json!(null))),
            }
        };

        // Check AI config
        let (
            ai_enabled,
            streaming_enabled,
            ai_fallback,
            ai_max_output_tokens,
            ai_timeout_ms,
            streaming_debounce_ms,
        ) = {
            let cfg = self.config.lock();
            let a = &cfg.ai_completion;
            (
                a.enabled,
                a.streaming.enabled,
                a.fallback,
                a.max_output_tokens,
                a.timeout_ms,
                a.streaming.update_debounce_ms,
            )
        };
        if !ai_enabled || !streaming_enabled {
            // Fall back to one-shot
            return self.handle_inline_completion(Some(params));
        }

        // Start session. This supersedes every older stream for the same
        // document, whatever cursor it was started at.
        let session_key = SessionKey {
            uri: uri.to_string(),
            document_version,
            line: u64::from(line),
            character: u64::from(character),
        };
        let session = self.stream_sessions().start_session(session_key.clone());
        let session_id = session.session_id.clone();

        // Every path below this point owns a manager entry and must release it
        // exactly once. `finish_if_current` settles the session and evicts the
        // entry only while it is still the exact session this request started,
        // so a stale task cannot remove its replacement.
        let release = |outcome: StreamTerminalOutcome| {
            self.stream_sessions().finish_if_current(&session_key, &session_id, outcome);
        };

        // Prepare context. Invoked AI preparation fails closed here: a stale
        // request version or a hard-reject cursor makes zero backend calls,
        // exactly as in the buffered route.
        let provider =
            perl_lsp_rs_core::providers::inline_completion::InlineCompletionProvider::new();
        let context = match provider.prepare_invoked_context(
            &text,
            line,
            character,
            snapshot_identity,
            request_document_version,
        ) {
            perl_lsp_rs_core::providers::inline_completion::PreparedInvocationContext::Ready(
                ctx,
            ) => *ctx,
            _ => {
                // A stale request version or a hard-reject cursor ends the
                // stream before any backend work. No progress value is emitted,
                // so the client settles fail-closed on the null response.
                release(StreamTerminalOutcome::ProtocolEndedWithoutFinal);
                return Ok(Some(json!(null)));
            }
        };

        // Build request
        let req = perl_lsp_rs_core::providers::inline_completion::BackendRequest {
            context: context.clone(),
            max_output_tokens: ai_max_output_tokens,
            timeout_ms: ai_timeout_ms,
        };

        let token_clone = token.clone();

        // Get the AI backend; fall back to one-shot if unavailable
        let backend = match self.ai_backend() {
            Some(b) => b,
            None => {
                if ai_fallback {
                    // The buffered route answers directly; this stream never
                    // reaches a progress value.
                    release(StreamTerminalOutcome::ProtocolEndedWithoutFinal);
                    return self.handle_inline_completion(Some(params));
                }
                // No backend and no fallback -- emit the one empty final.
                let progress = stream_progress_payload(
                    &token_clone,
                    &session_id,
                    session.pending_sequence(),
                    true,
                    Vec::new(),
                    line,
                    character,
                );
                match self.notify("$/progress", progress) {
                    Ok(()) => {
                        session.commit_sequence();
                        session.settle(StreamTerminalOutcome::CompletedEmptyOrFiltered);
                    }
                    Err(e) => {
                        tracing::debug!(
                            "streaming inline completion: failed to send empty final: {}",
                            e
                        );
                    }
                }
                release(StreamTerminalOutcome::ProtocolEndedWithoutFinal);
                return Ok(Some(json!(null)));
            }
        };

        // Capture values needed inside the streaming closure.
        // No document locks are held at this point -- notify() only
        // touches the outbound channel, so it is safe to call during
        // the (potentially slow) network streaming call.
        // Track whether the one terminal frame has been emitted from inside the
        // stream, so the tail below knows whether it still owns the terminal.
        let mut sent_final = false;
        let debounce = Duration::from_millis(streaming_debounce_ms);
        let mut last_emitted_at: Option<Instant> = None;

        // Stream from the backend -- each chunk carries cumulative text
        let stream_result = backend.stream(
            &req,
            &mut |chunk: perl_lsp_rs_core::providers::inline_completion::StreamChunk| {
                // A cancelled or already-settled stream emits nothing further.
                if session.is_cancelled() || session.is_settled() {
                    return perl_lsp_rs_core::providers::inline_completion::StreamControl::Stop;
                }

                // Update session cumulative text
                if let Ok(mut text) = session.current_text.lock() {
                    *text = chunk.text.clone();
                }

                let is_final = chunk.is_final;

                // One external candidate per cumulative chunk, evaluated
                // through the same shared finalization seam the buffered route
                // uses: exact range, parse-safety, selected-completion
                // constraint, and trigger policy — never a stream-local verdict.
                let outcome = evaluate_external_candidates(
                    &provider,
                    vec![perl_lsp_rs_core::providers::inline_completion::InlineCompletionItem {
                        insert_text: chunk.text,
                        filter_text: None,
                        range: None,
                        command: None,
                    }],
                    &text,
                    &context,
                    selected_completion.as_ref(),
                    trigger_kind,
                    line,
                    character,
                    ai_fallback,
                );
                let (safe_items, terminal) = match outcome {
                    ExternalCompletionOutcome::Accepted(list) => {
                        (list.items, StreamTerminalOutcome::CompletedWithCandidate)
                    }
                    ExternalCompletionOutcome::FallbackRequired if is_final => {
                        // A filtered final with fallback configured hands the
                        // final content to the deterministic route.
                        (
                            self.deterministic_inline_items(
                                &provider,
                                uri,
                                &text,
                                line,
                                character,
                                selected_completion.as_ref(),
                                trigger_kind,
                            ),
                            StreamTerminalOutcome::CompletedWithDeterministicFallback,
                        )
                    }
                    ExternalCompletionOutcome::FallbackRequired
                    | ExternalCompletionOutcome::FinalEmpty => {
                        (Vec::new(), StreamTerminalOutcome::CompletedEmptyOrFiltered)
                    }
                };
                if safe_items.is_empty() && !is_final {
                    // Unsafe or filtered intermediate cumulative text is
                    // skipped without ending the backend stream, and consumes
                    // no sequence number.
                    return perl_lsp_rs_core::providers::inline_completion::StreamControl::Continue;
                }

                // Keep the first update responsive, then suppress intermediate
                // chunks until the configured interval has elapsed. A final
                // chunk always goes through so the client receives the complete
                // cumulative result even when the provider emits rapidly.
                let should_emit = is_final
                    || last_emitted_at.map(|last| last.elapsed() >= debounce).unwrap_or(true);
                if !should_emit {
                    // A coalesced frame is never observed by the client, so it
                    // must not consume a sequence value: the sequence stream the
                    // client sees stays contiguous.
                    return perl_lsp_rs_core::providers::inline_completion::StreamControl::Continue;
                }

                // Read, do not consume. The outbound channel is bounded, so this
                // send can fail transiently under backpressure; a value consumed
                // for a frame the client never received would be a permanent gap.
                let seq = session.pending_sequence();
                let progress = stream_progress_payload(
                    &token_clone,
                    &session_id,
                    seq,
                    is_final,
                    safe_items,
                    line,
                    character,
                );

                if let Err(e) = self.notify("$/progress", progress) {
                    tracing::debug!("streaming inline completion: failed to send progress: {}", e);
                    // Nothing reached the client: the sequence value stays
                    // available and, for a final chunk, the stream has *not*
                    // reached its terminal. Stop pulling from the backend and
                    // let the tail owner below attempt the terminal once more.
                    return perl_lsp_rs_core::providers::inline_completion::StreamControl::Stop;
                }
                session.commit_sequence();
                last_emitted_at = Some(Instant::now());

                if is_final {
                    // Settle only now that the terminal value is actually on the
                    // wire. Recording it before the send would let a dropped
                    // frame be remembered as a delivered candidate, and would
                    // block the retry below.
                    session.settle(terminal);
                    sent_final = true;
                    perl_lsp_rs_core::providers::inline_completion::StreamControl::Stop
                } else {
                    perl_lsp_rs_core::providers::inline_completion::StreamControl::Continue
                }
            },
        );

        // Backend errors are not propagated to the JSON-RPC result -- the
        // protocol contract carries the outcome in the terminal progress value.
        let backend_failed = match stream_result {
            Ok(()) => false,
            Err(e) => {
                if matches!(e, BackendError::Auth(_)) {
                    self.notify_ai_auth_failure();
                }
                tracing::debug!("streaming inline completion backend error: {}", e);
                true
            }
        };

        // The stream did not settle itself. This scope is the remaining
        // terminal owner: it selects one outcome, emits at most one final
        // progress value, and releases the session.
        if !sent_final && !session.is_cancelled() {
            let (terminal, final_items) = if backend_failed {
                // A backend failure must never present its partial cumulative
                // text as a successful completion. The candidate is discarded
                // and the configured fallback policy -- the same one the
                // buffered route applies to a failed AI call -- owns the final
                // content.
                let items = if ai_fallback {
                    self.deterministic_inline_items(
                        &provider,
                        uri,
                        &text,
                        line,
                        character,
                        selected_completion.as_ref(),
                        trigger_kind,
                    )
                } else {
                    Vec::new()
                };
                (StreamTerminalOutcome::BackendFailed, items)
            } else {
                // A clean end-of-stream without an explicit final chunk:
                // evaluate the terminal cumulative text through the same shared
                // seam. A filtered final is a typed decision, never an implicit
                // empty list.
                let cumulative_text =
                    session.current_text.lock().map(|t| t.clone()).unwrap_or_default();
                let outcome = if cumulative_text.is_empty() {
                    None
                } else {
                    Some(evaluate_external_candidates(
                        &provider,
                        vec![InlineCompletionItem {
                            insert_text: cumulative_text,
                            filter_text: None,
                            range: None,
                            command: None,
                        }],
                        &text,
                        &context,
                        selected_completion.as_ref(),
                        trigger_kind,
                        line,
                        character,
                        ai_fallback,
                    ))
                };

                match outcome {
                    Some(ExternalCompletionOutcome::Accepted(list)) => {
                        (StreamTerminalOutcome::CompletedWithCandidate, list.items)
                    }
                    Some(ExternalCompletionOutcome::FallbackRequired) => (
                        StreamTerminalOutcome::CompletedWithDeterministicFallback,
                        self.deterministic_inline_items(
                            &provider,
                            uri,
                            &text,
                            line,
                            character,
                            selected_completion.as_ref(),
                            trigger_kind,
                        ),
                    ),
                    Some(ExternalCompletionOutcome::FinalEmpty) | None => {
                        (StreamTerminalOutcome::CompletedEmptyOrFiltered, Vec::new())
                    }
                }
            };

            if !session.is_settled() {
                let progress = stream_progress_payload(
                    &token_clone,
                    &session_id,
                    session.pending_sequence(),
                    true,
                    final_items,
                    line,
                    character,
                );
                match self.notify("$/progress", progress) {
                    Ok(()) => {
                        session.commit_sequence();
                        session.settle(terminal);
                    }
                    Err(e) => {
                        // The terminal value never reached the client. Leaving
                        // the session unsettled lets `release` below record the
                        // honest `ProtocolEndedWithoutFinal` rather than a
                        // success this stream did not achieve.
                        tracing::debug!(
                            "streaming inline completion: failed to send final progress: {}",
                            e
                        );
                    }
                }
            }
        }

        // Release the manager entry on every terminal path, including a
        // cancelled one. `finish_if_current` preserves an outcome already
        // recorded above and refuses to evict a newer session that reused this
        // display key.
        release(StreamTerminalOutcome::ProtocolEndedWithoutFinal);

        // Final response is null -- all data was sent via $/progress
        Ok(Some(json!(null)))
    }
}
