//! Formatting handlers for code formatting features
//!
//! Handles textDocument/formatting, textDocument/rangeFormatting,
//! and textDocument/onTypeFormatting requests.

use super::super::{
    GLOBAL_CANCELLATION_REGISTRY, INVALID_REQUEST, JsonRpcError, JsonRpcId, LspServer,
    PerlLspCancellationToken, Value, json,
};
use crate::cancellation::RequestCleanupGuard;
use crate::convert::{WirePosition, WireRange};
use crate::features::formatting::{
    CodeFormatter, FormattingError, FormattingOptions, PerlTidyConfig,
};
use crate::protocol::{REQUEST_CANCELLED, invalid_params, req_position, req_range, req_uri};
use perl_lsp_rs_core::config::FormatterMode;

/// Build a `JsonRpcError` from a `FormattingError`, populating the `data` field
/// with a structured object so that VSCode / LSP clients can surface targeted
/// remediation actions (e.g. "install perltidy" vs "check Perl syntax").
fn formatting_error_to_rpc(context: &str, e: FormattingError) -> JsonRpcError {
    let error_kind = e.error_kind();
    JsonRpcError {
        code: -32603,
        message: format!("{}: {}", context, e),
        data: Some(json!({
            "error_kind": error_kind,
        })),
    }
}

fn document_not_open_error(uri: &str) -> JsonRpcError {
    JsonRpcError {
        code: INVALID_REQUEST,
        message: format!("Document not open: {}", uri),
        data: None,
    }
}

impl LspServer {
    /// Build a `PerlTidyConfig` from the current server configuration.
    ///
    /// The native scalar fields are read directly from the server config, which
    /// already reflects the correct precedence: built-in defaults, then the
    /// discovered `.perltidyrc` options applied at initialize (see
    /// `set_root_uri`), then user `.perl-lsp.toml` / `didChangeConfiguration`.
    /// The profile *path* used for the external adapter is the explicitly
    /// configured `perltidy_profile` when set, else the discovered one.
    pub(crate) fn build_perltidy_config(&self) -> PerlTidyConfig {
        let config = self.config.lock();
        let profile = config
            .perltidy_profile
            .clone()
            .or_else(|| self.discovered_perltidy_profile.lock().clone());
        PerlTidyConfig {
            maximum_line_length: config.perltidy_maximum_line_length,
            indent_columns: config.perltidy_indent_columns,
            tabs: config.perltidy_tabs,
            opening_brace_on_new_line: config.perltidy_opening_brace_on_new_line,
            cuddled_else: config.perltidy_cuddled_else,
            space_after_keyword: config.perltidy_space_after_keyword,
            add_trailing_commas: config.perltidy_add_trailing_commas,
            vertical_alignment: config.perltidy_vertical_alignment,
            block_comment_indentation: config.perltidy_block_comment_indentation,
            profile,
            extra_args: config.perltidy_extra_args.clone(),
            timeout_secs: config.perltidy_timeout_secs,
        }
    }
}

impl LspServer {
    pub(crate) fn is_formatting_enabled(&self) -> bool {
        let config = self.config.lock();
        config.perltidy_enabled && config.formatting_engine != FormatterMode::Off
    }

    pub(crate) fn formatter_mode(&self) -> FormatterMode {
        self.config.lock().formatting_engine
    }

    /// Handle textDocument/onTypeFormatting request
    pub(crate) fn handle_on_type_formatting(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        if let Some(p) = params {
            let uri = req_uri(&p)?;
            let ch = p["ch"].as_str().and_then(|s| s.chars().next()).unwrap_or('\n');
            let (line, col) = req_position(&p)?;

            let documents = self.documents_guard();
            let doc = self.get_document(&documents, uri).ok_or_else(|| JsonRpcError {
                code: INVALID_REQUEST,
                message: format!("Document not open: {}", uri),
                data: None,
            })?;

            let indent_step = p["options"]["tabSize"].as_u64().unwrap_or(4) as usize;

            if let Some(edits) = crate::on_type_formatting::compute_on_type_edit(
                &doc.text,
                line,
                col,
                ch,
                indent_step,
            ) {
                return Ok(Some(json!(edits)));
            }
        }
        Ok(Some(json!([])))
    }

    /// Handle textDocument/formatting request
    pub(crate) fn handle_formatting(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        // Gate unadvertised feature
        if !self.advertised_features.lock().formatting {
            return Err(crate::protocol::method_not_advertised());
        }

        if !self.is_formatting_enabled() {
            return Ok(Some(json!([])));
        }

        if let Some(params) = params {
            let uri = req_uri(&params)?;

            // Reject stale requests
            let req_version =
                params["textDocument"]["version"].as_i64().and_then(|n| i32::try_from(n).ok());
            self.ensure_latest(uri, req_version)?;

            let options: FormattingOptions = serde_json::from_value(params["options"].clone())
                .unwrap_or(FormattingOptions {
                    tab_size: 4,
                    insert_spaces: true,
                    trim_trailing_whitespace: None,
                    insert_final_newline: None,
                    trim_final_newlines: None,
                });

            tracing::debug!(uri, "Formatting document");

            // Snapshot the document text under the lock, then release the
            // guard before running the perltidy subprocess. Holding the
            // documents lock across the entire format would block every
            // other concurrent handler (hover, completion, didChange, …)
            // for the full subprocess duration (#4643).
            //
            // Clone from text_arc rather than text to avoid the double-store
            // overhead — text_arc is the canonical copy (#4999).
            let text = {
                let documents = self.documents_guard();
                let doc = self
                    .get_document(&documents, uri)
                    .ok_or_else(|| document_not_open_error(uri))?;
                doc.text_arc.to_string()
            };
            let config = self.build_perltidy_config();
            let formatter = CodeFormatter::with_config_and_mode(config, self.formatter_mode());
            match formatter.format_document(&text, &options) {
                Ok(edits) => {
                    let lsp_edits: Vec<Value> = edits
                        .into_iter()
                        .map(|edit| {
                            json!({
                                "range": {
                                    "start": {
                                        "line": edit.range.start.line,
                                        "character": edit.range.start.character,
                                    },
                                    "end": {
                                        "line": edit.range.end.line,
                                        "character": edit.range.end.character,
                                    },
                                },
                                "newText": edit.new_text,
                            })
                        })
                        .collect();

                    return Ok(Some(json!(lsp_edits)));
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Formatting error");
                    return Err(formatting_error_to_rpc("Formatting failed", e));
                }
            }
        }

        Ok(Some(json!([])))
    }

    /// Cancellation-aware wrapper for `textDocument/formatting`.
    ///
    /// Polls the cancellation token before invoking perltidy so that a
    /// `$/cancelRequest` issued while the handler is waiting on the documents
    /// lock is observed promptly, returning `REQUEST_CANCELLED` (code -32800)
    /// instead of running the formatter to completion.
    pub(crate) fn handle_formatting_cancellable(
        &self,
        params: Option<Value>,
        request_id: Option<&Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        let typed_id = request_id.and_then(JsonRpcId::try_from_value);
        let _cleanup_guard = RequestCleanupGuard::from_ref(typed_id.as_ref());

        if let Some(ref tid) = typed_id {
            let token = GLOBAL_CANCELLATION_REGISTRY.get_token(tid).unwrap_or_else(|| {
                let token =
                    PerlLspCancellationToken::new(tid.clone(), "textDocument/formatting".into());
                let _ = GLOBAL_CANCELLATION_REGISTRY.register_token(token.clone());
                token
            });
            if token.is_cancelled_relaxed() {
                return Err(JsonRpcError {
                    code: REQUEST_CANCELLED,
                    message: "Request cancelled - formatting provider".to_string(),
                    data: None,
                });
            }
        }

        self.handle_formatting(params)
    }

    /// Handle textDocument/rangeFormatting request
    pub(crate) fn handle_range_formatting(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        // Gate unadvertised feature
        if !self.advertised_features.lock().range_formatting {
            return Err(crate::protocol::method_not_advertised());
        }

        if !self.is_formatting_enabled() {
            return Ok(Some(json!([])));
        }

        if let Some(params) = params {
            let uri = req_uri(&params)?;
            let ((start_line, start_char), (end_line, end_char)) = req_range(&params)?;
            let options: FormattingOptions = serde_json::from_value(params["options"].clone())
                .unwrap_or(FormattingOptions {
                    tab_size: 4,
                    insert_spaces: true,
                    trim_trailing_whitespace: None,
                    insert_final_newline: None,
                    trim_final_newlines: None,
                });

            let range = WireRange::new(
                WirePosition::new(start_line, start_char),
                WirePosition::new(end_line, end_char),
            );

            tracing::debug!(uri, "Formatting range in document");

            // Snapshot the document text under the lock, then release the
            // guard before running the perltidy subprocess so other LSP
            // requests are not blocked for the full subprocess duration
            // (#4643).
            let text = {
                let documents = self.documents_guard();
                let doc = self
                    .get_document(&documents, uri)
                    .ok_or_else(|| document_not_open_error(uri))?;
                doc.text_arc.to_string()
            };
            let config = self.build_perltidy_config();
            let formatter = CodeFormatter::with_config_and_mode(config, self.formatter_mode());
            match formatter.format_range(&text, &range, &options) {
                Ok(edits) => {
                    let lsp_edits: Vec<Value> = edits
                        .into_iter()
                        .map(|edit| {
                            json!({
                                "range": {
                                    "start": {
                                        "line": edit.range.start.line,
                                        "character": edit.range.start.character,
                                    },
                                    "end": {
                                        "line": edit.range.end.line,
                                        "character": edit.range.end.character,
                                    },
                                },
                                "newText": edit.new_text,
                            })
                        })
                        .collect();

                    return Ok(Some(json!(lsp_edits)));
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Range formatting error");
                    return Err(formatting_error_to_rpc("Range formatting failed", e));
                }
            }
        }

        Ok(Some(json!([])))
    }

    /// Handle textDocument/rangesFormatting request (LSP 3.18)
    pub(crate) fn handle_ranges_formatting(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        if !self.is_formatting_enabled() {
            return Ok(Some(json!([])));
        }

        if let Some(params) = params {
            let uri = req_uri(&params)?;
            let options: FormattingOptions = serde_json::from_value(params["options"].clone())
                .unwrap_or(FormattingOptions {
                    tab_size: 4,
                    insert_spaces: true,
                    trim_trailing_whitespace: None,
                    insert_final_newline: None,
                    trim_final_newlines: None,
                });

            // Parse ranges array
            let ranges_array = params
                .get("ranges")
                .and_then(|r| r.as_array())
                .ok_or_else(|| invalid_params("Missing required parameter: ranges"))?;

            if ranges_array.is_empty() {
                return Ok(Some(json!([])));
            }

            tracing::debug!(count = ranges_array.len(), uri, "Formatting ranges in document");

            // Snapshot the document text under the lock, then release the
            // guard before running the perltidy subprocess so other LSP
            // requests are not blocked for the full subprocess duration
            // (#4643).
            let text = {
                let documents = self.documents_guard();
                let doc = self
                    .get_document(&documents, uri)
                    .ok_or_else(|| document_not_open_error(uri))?;
                doc.text_arc.to_string()
            };
            let config = self.build_perltidy_config();
            let formatter = CodeFormatter::with_config_and_mode(config, self.formatter_mode());
            let mut all_edits = Vec::new();

            // Process each range
            for (idx, range_val) in ranges_array.iter().enumerate() {
                let start_line_u64 =
                    range_val.pointer("/start/line").and_then(|v| v.as_u64()).ok_or_else(|| {
                        invalid_params(&format!("Missing ranges[{}].start.line", idx))
                    })?;
                let start_line = u32::try_from(start_line_u64).map_err(|_| {
                    invalid_params(&format!("ranges[{}].start.line exceeds u32::MAX", idx))
                })?;

                let start_char_u64 =
                    range_val.pointer("/start/character").and_then(|v| v.as_u64()).ok_or_else(
                        || invalid_params(&format!("Missing ranges[{}].start.character", idx)),
                    )?;
                let start_char = u32::try_from(start_char_u64).map_err(|_| {
                    invalid_params(&format!("ranges[{}].start.character exceeds u32::MAX", idx))
                })?;

                let end_line_u64 = range_val
                    .pointer("/end/line")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| invalid_params(&format!("Missing ranges[{}].end.line", idx)))?;
                let end_line = u32::try_from(end_line_u64).map_err(|_| {
                    invalid_params(&format!("ranges[{}].end.line exceeds u32::MAX", idx))
                })?;

                let end_char_u64 =
                    range_val.pointer("/end/character").and_then(|v| v.as_u64()).ok_or_else(
                        || invalid_params(&format!("Missing ranges[{}].end.character", idx)),
                    )?;
                let end_char = u32::try_from(end_char_u64).map_err(|_| {
                    invalid_params(&format!("ranges[{}].end.character exceeds u32::MAX", idx))
                })?;

                let range = WireRange::new(
                    WirePosition::new(start_line, start_char),
                    WirePosition::new(end_line, end_char),
                );

                match formatter.format_range(&text, &range, &options) {
                    Ok(edits) => {
                        all_edits.extend(edits);
                    }
                    Err(e) => {
                        tracing::warn!(idx, error = %e, "Range formatting error");
                        return Err(formatting_error_to_rpc(
                            &format!("Range formatting failed for range {}", idx),
                            e,
                        ));
                    }
                }
            }

            let lsp_edits: Vec<Value> = all_edits
                .into_iter()
                .map(|edit| {
                    json!({
                        "range": {
                            "start": {
                                "line": edit.range.start.line,
                                "character": edit.range.start.character,
                            },
                            "end": {
                                "line": edit.range.end.line,
                                "character": edit.range.end.character,
                            },
                        },
                        "newText": edit.new_text,
                    })
                })
                .collect();

            return Ok(Some(json!(lsp_edits)));
        }

        Ok(Some(json!([])))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::formatting::FormattingError;
    use perl_tdd_support::must_some;

    #[test]
    fn formatting_error_to_rpc_not_found_has_data_field() {
        let err = FormattingError::PerltidyNotFound("command not found".to_string());
        let rpc = formatting_error_to_rpc("Formatting failed", err);
        assert_eq!(rpc.code, -32603);
        assert!(rpc.message.contains("Formatting failed"), "message should contain context prefix");
        assert!(
            rpc.message.contains("perltidy not found"),
            "message should contain the error description"
        );
        assert!(
            rpc.message.contains("cpanm Perl::Tidy"),
            "message should contain cpanm install recommendation"
        );
        let data = must_some(rpc.data);
        assert_eq!(
            data["error_kind"].as_str(),
            Some("perltidy_not_found"),
            "data.error_kind should be 'perltidy_not_found'"
        );
    }

    #[test]
    fn formatting_error_to_rpc_execution_error_has_data_field() {
        let err = FormattingError::PerltidyError("syntax error at line 3".to_string());
        let rpc = formatting_error_to_rpc("Range formatting failed", err);
        assert_eq!(rpc.code, -32603);
        assert!(rpc.message.contains("Range formatting failed"));
        assert!(
            rpc.message.contains("check Perl syntax") || rpc.message.contains("perltidy error")
        );
        let data = must_some(rpc.data);
        assert_eq!(
            data["error_kind"].as_str(),
            Some("perltidy_error"),
            "data.error_kind should be 'perltidy_error'"
        );
    }

    #[test]
    fn formatting_error_to_rpc_io_error_has_data_field() {
        let err = FormattingError::IoError("disk full".to_string());
        let rpc = formatting_error_to_rpc("Formatting failed", err);
        let data = must_some(rpc.data);
        assert_eq!(
            data["error_kind"].as_str(),
            Some("io_error"),
            "data.error_kind should be 'io_error'"
        );
    }

    #[test]
    pub(crate) fn build_perltidy_config_uses_discovered_profile_when_unset() {
        let server = LspServer::new();
        server.config.lock().perltidy_profile = None;
        *server.discovered_perltidy_profile.lock() = Some("/ws/.perltidyrc".to_string());

        let config = server.build_perltidy_config();

        assert_eq!(
            config.profile.as_deref(),
            Some("/ws/.perltidyrc"),
            "discovered profile should be used when none is explicitly configured"
        );
    }

    #[test]
    pub(crate) fn build_perltidy_config_prefers_explicit_profile_over_discovered() {
        let server = LspServer::new();
        server.config.lock().perltidy_profile = Some("/explicit/.perltidyrc".to_string());
        *server.discovered_perltidy_profile.lock() = Some("/ws/.perltidyrc".to_string());

        let config = server.build_perltidy_config();

        assert_eq!(
            config.profile.as_deref(),
            Some("/explicit/.perltidyrc"),
            "explicit configuration must take precedence over discovery"
        );
    }

    #[test]
    pub(crate) fn build_perltidy_config_profile_none_when_unset_and_undiscovered() {
        let server = LspServer::new();
        server.config.lock().perltidy_profile = None;
        *server.discovered_perltidy_profile.lock() = None;

        let config = server.build_perltidy_config();

        assert!(
            config.profile.is_none(),
            "profile should be None when neither configured nor discovered"
        );
    }

    #[test]
    pub(crate) fn build_perltidy_config_reads_native_scalars_from_server_config() {
        // The native scalar fields are read straight from the server config,
        // which already reflects defaults + discovered-profile + user config.
        let server = LspServer::new();
        {
            let mut config = server.config.lock();
            config.perltidy_maximum_line_length = Some(123);
            config.perltidy_indent_columns = Some(3);
            config.perltidy_tabs = Some(true);
        }

        let config = server.build_perltidy_config();

        assert_eq!(config.maximum_line_length, Some(123));
        assert_eq!(config.indent_columns, Some(3));
        assert_eq!(config.tabs, Some(true));
    }

    #[test]
    fn document_not_open_error_uses_invalid_request_code() {
        let uri = "file:///tmp/missing.pl";
        let rpc = document_not_open_error(uri);
        assert_eq!(rpc.code, INVALID_REQUEST);
        assert_eq!(rpc.message, format!("Document not open: {uri}"));
        assert!(rpc.data.is_none(), "document-not-open error should not include data");
    }

    #[test]
    fn handle_formatting_returns_document_not_open_error() -> Result<(), Box<dyn std::error::Error>>
    {
        // The snapshot lookup must still produce the correct error when the
        // document is not open — the lock is acquired only for the lookup,
        // then released.
        let server = LspServer::new();

        let params = Some(json!({
            "textDocument": { "uri": "file:///nonexistent.pl", "version": 1 },
            "options": { "tabSize": 4, "insertSpaces": true },
        }));

        let result = server.handle_formatting(params);
        let err = result.err().ok_or("expected an error for a missing document")?;
        assert_eq!(err.code, INVALID_REQUEST);
        assert!(err.message.contains("Document not open"));
        Ok(())
    }

    #[test]
    fn handle_formatting_produces_edits_after_lock_scope_refactor()
    -> Result<(), Box<dyn std::error::Error>> {
        // Functional regression: the snapshot-and-release refactor must still
        // produce correct formatting edits. The document text is cloned under
        // the lock, the lock is released, and the formatter runs off-lock.
        let server = LspServer::new();
        let uri = "file:///test_lock_scope.pl";
        server.test_apply_did_open(uri, "sub hello{my $x=1;return $x;}\n", 1)?;

        let params = Some(json!({
            "textDocument": { "uri": uri, "version": 1 },
            "options": { "tabSize": 4, "insertSpaces": true },
        }));

        let result = server.handle_formatting(params)?;
        let edits = result
            .and_then(|v| v.as_array().map(|a| a.to_vec()))
            .ok_or("expected an array of edits")?;
        assert!(!edits.is_empty(), "native formatter should produce edits for unformatted Perl");

        // The documents lock must be immediately acquirable after formatting
        // returns, proving it was released.
        assert!(
            server.documents.try_lock().is_some(),
            "documents lock must be released after handle_formatting returns"
        );
        Ok(())
    }

    #[test]
    fn handle_formatting_lock_not_held_during_formatting() -> Result<(), Box<dyn std::error::Error>>
    {
        // Concurrency test: prove the documents lock is released before the
        // formatting operation runs (#4643).
        //
        // Design:
        // 1. The main thread holds the documents lock, then spawns the
        //    formatting thread — which blocks on lock acquisition.
        // 2. After the formatting thread is blocked, the poller thread starts.
        //    The poller only records a success while `formatting_active` is
        //    true, preventing false positives after formatting completes.
        // 3. The main thread releases the lock. parking_lot hands the lock to
        //    the waiting formatting thread (fairness), not the poller's
        //    try_lock, so the poller cannot sneak in during the handoff.
        // 4. With the fix: the formatting thread clones text, releases the
        //    lock, then formats off-lock. The poller's try_lock succeeds
        //    while formatting is still active.
        // 5. Without the fix: the formatting thread holds the lock through
        //    the entire formatting call. The poller's try_lock fails while
        //    formatting is active, and `formatting_active` is cleared before
        //    the poller can try again after the lock is released.
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::thread;
        use std::time::Duration;

        let server = Arc::new(LspServer::new());
        let uri = "file:///test_concurrent_lock.pl";

        // Generate a large document so the native formatter has enough work
        // to create a measurable window where the lock is released but
        // formatting has not yet completed.
        let mut text = String::with_capacity(200_000);
        for i in 0..2000 {
            text.push_str(&format!("sub func_{i}{{my $x={i};return $x;}}\n"));
        }
        server.test_apply_did_open(uri, &text, 1)?;

        let formatting_active = Arc::new(AtomicBool::new(false));
        let lock_acquired_during_format = Arc::new(AtomicBool::new(false));
        let stop_polling = Arc::new(AtomicBool::new(false));

        // --- Hold the lock so the formatting thread blocks on acquisition ---
        let lock_guard = server.documents.lock();

        // --- Spawn the formatting thread (blocks on the lock) ---
        let server_fmt = Arc::clone(&server);
        let fmt_active = Arc::clone(&formatting_active);
        let fmt_thread = thread::spawn(move || {
            let params = Some(json!({
                "textDocument": { "uri": uri, "version": 1 },
                "options": { "tabSize": 4, "insertSpaces": true },
            }));
            fmt_active.store(true, Ordering::SeqCst);
            let _ = server_fmt.handle_formatting(params);
            fmt_active.store(false, Ordering::SeqCst);
        });

        // Wait for the formatting thread to start and block on the lock.
        thread::sleep(Duration::from_millis(50));

        // --- Spawn the poller thread ---
        let poll_lock = Arc::clone(&lock_acquired_during_format);
        let poll_stop = Arc::clone(&stop_polling);
        let poll_active = Arc::clone(&formatting_active);
        let server_clone = Arc::clone(&server);
        let poller = thread::spawn(move || {
            while !poll_stop.load(Ordering::SeqCst) {
                // Only try to acquire the lock while formatting is in flight.
                if poll_active.load(Ordering::SeqCst) {
                    if server_clone.documents.try_lock().is_some() {
                        poll_lock.store(true, Ordering::SeqCst);
                    }
                }
                thread::sleep(Duration::from_micros(50));
            }
        });

        // --- Release the lock so the formatting thread can proceed ---
        drop(lock_guard);

        // Wait for formatting to complete.
        fmt_thread.join().ok();

        stop_polling.store(true, Ordering::SeqCst);
        poller.join().ok();

        assert!(
            lock_acquired_during_format.load(Ordering::SeqCst),
            "documents lock should be acquirable while handle_formatting is still \
             running, proving the lock is not held across the formatting operation \
             (#4643)"
        );
        Ok(())
    }
}
