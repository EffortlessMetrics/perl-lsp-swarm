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
use crate::runtime::stream_session::SessionKey;
use perl_lsp_rs_core::providers::inline_completion::BackendError;
use std::time::{Duration, Instant};

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
                Some(doc) => match doc.text_for_user_answers() {
                    Some(text) => (
                        text.to_string(),
                        perl_lsp_rs_core::providers::inline_completion::InlineCompletionSnapshotIdentity {
                            document_version: Some(i64::from(doc.version)),
                            source_generation: Some(u64::from(
                                doc.generation.load(std::sync::atomic::Ordering::Acquire),
                            )),
                        },
                    ),
                    None => return Ok(Some(json!(null))),
                },
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

        // Start session (cancels any previous for same position)
        let session_key = SessionKey {
            uri: uri.to_string(),
            document_version,
            line: u64::from(line),
            character: u64::from(character),
        };
        let session = self.stream_sessions().start_session(session_key);

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
            _ => return Ok(Some(json!(null))),
        };

        // Build request
        let req = perl_lsp_rs_core::providers::inline_completion::BackendRequest {
            context: context.clone(),
            max_output_tokens: ai_max_output_tokens,
            timeout_ms: ai_timeout_ms,
        };

        let session_id = session.session_id.clone();
        let token_clone = token.clone();

        // Get the AI backend; fall back to one-shot if unavailable
        let backend = match self.ai_backend() {
            Some(b) => b,
            None => {
                if ai_fallback {
                    return self.handle_inline_completion(Some(params));
                }
                // No backend and no fallback -- emit empty final and return
                let progress = json!({
                    "token": token_clone,
                    "value": {
                        "kind": "perlInlineCompletionStream",
                        "sessionId": session_id,
                        "sequence": session.next_sequence(),
                        "isFinal": true,
                        "items": []
                    }
                });
                if let Err(e) = self.notify("$/progress", progress) {
                    tracing::debug!(
                        "streaming inline completion: failed to send empty final: {}",
                        e
                    );
                }
                self.stream_sessions().cleanup();
                return Ok(Some(json!(null)));
            }
        };

        // Capture values needed inside the streaming closure.
        // No document locks are held at this point -- notify() only
        // touches the outbound channel, so it is safe to call during
        // the (potentially slow) network streaming call.
        // Track whether we sent any chunk so we know if a final is needed
        let mut sent_final = false;
        let debounce = Duration::from_millis(streaming_debounce_ms);
        let mut last_emitted_at: Option<Instant> = None;
        // The typed final decision for the stream's last candidate, retained
        // for #10005's terminal owner: filtered output is a decision, never an
        // implicit empty list.
        let mut final_outcome: Option<ExternalCompletionOutcome> = None;

        // Stream from the backend -- each chunk carries cumulative text
        let stream_result = backend.stream(
            &req,
            &mut |chunk: perl_lsp_rs_core::providers::inline_completion::StreamChunk| {
                // Check cancellation before emitting
                if session.is_cancelled() {
                    return perl_lsp_rs_core::providers::inline_completion::StreamControl::Stop;
                }

                // Update session cumulative text
                if let Ok(mut text) = session.current_text.lock() {
                    *text = chunk.text.clone();
                }

                let is_final = chunk.is_final;
                if is_final {
                    sent_final = true;
                }

                // One external candidate per cumulative chunk, evaluated
                // through the same shared finalization seam the buffered route
                // uses: exact range, parse-safety, selected-completion
                // constraint, and trigger policy — never a stream-local verdict.
                let outcome = evaluate_external_candidates(
                    &provider,
                    vec![
                        perl_lsp_rs_core::providers::inline_completion::InlineCompletionItem {
                            insert_text: chunk.text,
                            filter_text: None,
                            range: None,
                            command: None,
                        },
                    ],
                    &text,
                    &context,
                    selected_completion.as_ref(),
                    trigger_kind,
                    line,
                    character,
                    ai_fallback,
                );
                let safe_items = match outcome {
                    ExternalCompletionOutcome::Accepted(list) => list.items,
                    ExternalCompletionOutcome::FallbackRequired if is_final => {
                        // A filtered final with fallback configured hands the
                        // final content to the deterministic route.
                        final_outcome = Some(ExternalCompletionOutcome::FallbackRequired);
                        self.deterministic_inline_items(
                            &provider,
                            uri,
                            &text,
                            line,
                            character,
                            selected_completion.as_ref(),
                            trigger_kind,
                        )
                    }
                    filtered @ (ExternalCompletionOutcome::FallbackRequired
                    | ExternalCompletionOutcome::FinalEmpty) => {
                        if is_final {
                            final_outcome = Some(filtered);
                        }
                        Vec::new()
                    }
                };
                if safe_items.is_empty() && !is_final {
                    // Unsafe or filtered intermediate cumulative text is
                    // skipped without ending the backend stream.
                    return perl_lsp_rs_core::providers::inline_completion::StreamControl::Continue;
                }

                let seq = session.next_sequence();

                // Keep the first update responsive, then suppress intermediate
                // chunks until the configured interval has elapsed. A final
                // chunk always goes through so the client receives the complete
                // cumulative result even when the provider emits rapidly.
                let should_emit = is_final
                    || last_emitted_at.map(|last| last.elapsed() >= debounce).unwrap_or(true);
                if !should_emit {
                    return perl_lsp_rs_core::providers::inline_completion::StreamControl::Continue;
                }

                let progress = json!({
                    "token": token_clone,
                    "value": {
                        "kind": "perlInlineCompletionStream",
                        "sessionId": session_id,
                        "sequence": seq,
                        "isFinal": is_final,
                        "items": safe_items.into_iter().map(|item| {
                            let range = item.range.unwrap_or(lsp_types::Range {
                                start: lsp_types::Position {
                                    line,
                                    character,
                                },
                                end: lsp_types::Position {
                                    line,
                                    character,
                                },
                            });
                            json!({
                                "insertText": item.insert_text,
                                "range": {
                                    "start": { "line": range.start.line, "character": range.start.character },
                                    "end": { "line": range.end.line, "character": range.end.character }
                                }
                            })
                        }).collect::<Vec<_>>()
                    }
                });

                if let Err(e) = self.notify("$/progress", progress) {
                    tracing::debug!("streaming inline completion: failed to send progress: {}", e);
                    return perl_lsp_rs_core::providers::inline_completion::StreamControl::Stop;
                }
                last_emitted_at = Some(Instant::now());

                if is_final {
                    perl_lsp_rs_core::providers::inline_completion::StreamControl::Stop
                } else {
                    perl_lsp_rs_core::providers::inline_completion::StreamControl::Continue
                }
            },
        );

        // If the stream ended without sending a final chunk, send one now
        if !sent_final && !session.is_cancelled() {
            let cumulative_text =
                session.current_text.lock().map(|t| t.clone()).unwrap_or_default();

            // A typed provider failure (response.failed / non-token-limited
            // response.incomplete) means the accumulated text is NOT a usable
            // candidate: the recovery path must never promote it to the final
            // completion. Mirror the no-backend branch — with fallback
            // configured the deterministic route owns the final content,
            // otherwise the stream ends with an empty final. The terminal
            // isFinal notification below is still always sent.
            let provider_failed = matches!(&stream_result, Err(BackendError::Provider(_)));
            let outcome = if provider_failed || cumulative_text.is_empty() {
                None
            } else {
                Some(evaluate_external_candidates(
                    &provider,
                    vec![perl_lsp_rs_core::providers::inline_completion::InlineCompletionItem {
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
            let final_decision = if provider_failed {
                // Failed provider output never becomes the candidate: the
                // deterministic route owns the final when configured.
                if ai_fallback {
                    Some(ExternalCompletionOutcome::FallbackRequired)
                } else {
                    Some(ExternalCompletionOutcome::FinalEmpty)
                }
            } else {
                outcome.or(final_outcome.take())
            };

            let final_items = match final_decision {
                Some(ExternalCompletionOutcome::Accepted(list)) => list.items,
                Some(ExternalCompletionOutcome::FallbackRequired) => self
                    .deterministic_inline_items(
                        &provider,
                        uri,
                        &text,
                        line,
                        character,
                        selected_completion.as_ref(),
                        trigger_kind,
                    ),
                Some(ExternalCompletionOutcome::FinalEmpty) | None => Vec::new(),
            };

            let items = json!(final_items
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
                .collect::<Vec<_>>());

            let progress = json!({
                "token": token_clone,
                "value": {
                    "kind": "perlInlineCompletionStream",
                    "sessionId": session_id,
                    "sequence": session.next_sequence(),
                    "isFinal": true,
                    "items": items
                }
            });

            if let Err(e) = self.notify("$/progress", progress) {
                tracing::debug!(
                    "streaming inline completion: failed to send final progress: {}",
                    e
                );
            }
        }

        // Log backend errors but don't propagate -- the protocol contract
        // only needs the final isFinal:true notification to be sent.
        if let Err(e) = stream_result {
            if matches!(e, BackendError::Auth(_)) {
                self.notify_ai_auth_failure();
            }
            tracing::debug!("streaming inline completion backend error: {}", e);
        }

        // Cleanup completed/cancelled sessions
        self.stream_sessions().cleanup();

        // Final response is null -- all data was sent via $/progress
        Ok(Some(json!(null)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_lsp_rs_core::providers::inline_completion::{
        BackendError, BackendRequest, InlineCompletionBackend, StreamChunk, StreamControl,
    };
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn ranged_violation(uri: &str, version: i32) -> Value {
        json!({
            "textDocument": { "uri": uri, "version": version },
            "contentChanges": [{
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 0, "character": 1 }
                },
                "text": "x"
            }]
        })
    }

    struct CountingBackend {
        calls: Arc<AtomicUsize>,
    }

    impl InlineCompletionBackend for CountingBackend {
        fn stream(
            &self,
            _req: &BackendRequest,
            _sink: &mut dyn FnMut(StreamChunk) -> StreamControl,
        ) -> Result<(), BackendError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[test]
    fn streaming_inline_completion_fails_closed_after_ranged_did_change()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        let uri = "file:///desync-streaming.pl";
        server.test_apply_did_open(uri, "my $value = ", 1)?;
        server.test_configure_ai_completion(true, false);
        let calls = Arc::new(AtomicUsize::new(0));
        server
            .test_install_ai_backend(Some(Arc::new(CountingBackend { calls: Arc::clone(&calls) })));

        server.handle_did_change(Some(ranged_violation(uri, 2)))?;

        let result = server.handle_streaming_inline_completion(Some(json!({
            "textDocument": { "uri": uri, "version": 2 },
            "position": { "line": 0, "character": 12 },
            "partialResultToken": "stream-desync",
            "context": { "triggerKind": 1 }
        })))?;
        assert_eq!(
            result,
            Some(json!(null)),
            "Full-sync unavailability must terminate the stream empty rather than copy predecessor text"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "streaming backend must not run on predecessor text after a Full-sync violation"
        );
        assert_eq!(
            server.stream_sessions().len(),
            0,
            "fail-closed streaming must not start a session on unavailable user-answer text"
        );
        Ok(())
    }
}
