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
        let document_version = params
            .get("textDocument")
            .and_then(|td| td.get("version"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0);

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

        // Snapshot text
        let text = {
            let documents = self.documents_guard();
            match self.get_document(&documents, uri) {
                Some(doc) => doc.text_arc.to_string(),
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

        // Prepare context
        let provider =
            perl_lsp_rs_core::providers::inline_completion::InlineCompletionProvider::new();
        let context = match provider.prepare_context(&text, line, character) {
            Some(ctx) => ctx,
            None => return Ok(Some(json!(null))),
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

            // Evaluate the terminal cumulative text through the same shared
            // seam. A filtered final is a typed decision: with fallback
            // configured, the deterministic route owns the final content;
            // without it, the final list is empty.
            let outcome = if cumulative_text.is_empty() {
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
            let final_decision = outcome.or(final_outcome.take());

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
