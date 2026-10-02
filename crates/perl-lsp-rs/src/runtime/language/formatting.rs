//! Formatting handler for the proven manual whole-document formatting route.
//!
//! Secondary edit-producing routes are withdrawn (#11955); see the shared
//! policy interceptor in `runtime::dispatch::formatting_policy`.

use super::super::{
    GLOBAL_CANCELLATION_REGISTRY, INVALID_REQUEST, JsonRpcError, JsonRpcId, LspServer,
    PerlLspCancellationToken, Value, json,
};
use crate::cancellation::RequestCleanupGuard;
use crate::features::formatting::{
    CodeFormatter, FormattingError, FormattingOptions, PerlTidyConfig,
};
use crate::protocol::{CONTENT_MODIFIED, REQUEST_CANCELLED, req_uri};
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
        let config = self.client_session.config.lock();
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
        let config = self.client_session.config.lock();
        config.perltidy_enabled && config.formatting_engine != FormatterMode::Off
    }

    pub(crate) fn formatter_mode(&self) -> FormatterMode {
        self.client_session.config.lock().formatting_engine
    }

    // Secondary edit-producing handlers (`textDocument/rangeFormatting`,
    // `textDocument/rangesFormatting`, `textDocument/onTypeFormatting`) were
    // removed here (#11955): their geometry, currentness, and outcome policy
    // are unproven, and duplicate authorities behind the shared policy route
    // could re-arm withdrawn routes. Restoration is owned by #9317, #7089,
    // and #9320; until then the shared policy interceptor refuses these
    // methods before any parameter validation.

    /// Handle textDocument/formatting request
    pub(crate) fn handle_formatting(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        // Gate unadvertised feature
        if !self.client_session.advertised_features.lock().formatting {
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
            let (text, captured_generation) = {
                let documents = self.documents_guard();
                let doc = self
                    .get_document(&documents, uri)
                    .ok_or_else(|| document_not_open_error(uri))?;
                match doc.text_for_user_answers() {
                    Some(text) => (text.to_string(), doc.current_generation()),
                    None => {
                        return Err(JsonRpcError {
                            code: CONTENT_MODIFIED,
                            message: "Document requires a full-document resync before formatting"
                                .to_string(),
                            data: None,
                        });
                    }
                }
            };
            let config = self.build_perltidy_config();
            let formatter = CodeFormatter::with_config_and_mode(config, self.formatter_mode());
            match formatter.format_document(&text, &options) {
                Ok(edits) => {
                    if !self.user_answer_text_is_current(uri, captured_generation) {
                        return Err(JsonRpcError {
                            code: CONTENT_MODIFIED,
                            message: "Document changed while formatting was running; no edits were returned."
                                .to_string(),
                            data: None,
                        });
                    }
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::formatting::FormattingError;
    use crate::protocol::CONTENT_MODIFIED;
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
        server.client_session.config.lock().perltidy_profile = None;
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
        server.client_session.config.lock().perltidy_profile =
            Some("/explicit/.perltidyrc".to_string());
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
        server.client_session.config.lock().perltidy_profile = None;
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
            let mut config = server.client_session.config.lock();
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
    fn handle_formatting_fails_closed_while_full_sync_required()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        let uri = "file:///desync_formatting.pl";
        server.test_apply_did_open(uri, "sub hello{my $x=1;return $x;}\n", 1)?;

        let params = json!({
            "textDocument": { "uri": uri, "version": 1 },
            "options": { "tabSize": 4, "insertSpaces": true },
        });
        let fresh = server.handle_formatting(Some(params.clone()))?;
        let fresh_edits = fresh
            .and_then(|value| value.as_array().map(ToOwned::to_owned))
            .ok_or("expected formatting edits before desync")?;
        assert!(!fresh_edits.is_empty(), "native formatter should edit unformatted Perl");

        server.handle_did_change(Some(json!({
            "textDocument": { "uri": uri, "version": 2 },
            "contentChanges": [{
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 0, "character": 3 }
                },
                "text": "foo"
            }]
        })))?;

        let error = server
            .handle_formatting(Some(json!({
                "textDocument": { "uri": uri, "version": 2 },
                "options": { "tabSize": 4, "insertSpaces": true },
            })))
            .err()
            .ok_or("desynchronized formatting must fail closed")?;
        assert_eq!(error.code, CONTENT_MODIFIED);
        assert!(
            error.message.contains("full-document resync"),
            "desync formatting error should name resync: {}",
            error.message
        );

        server.handle_did_change(Some(json!({
            "textDocument": { "uri": uri, "version": 3 },
            "contentChanges": [{ "text": "sub recovered{my $y=2;return $y;}\n" }]
        })))?;
        let recovered = server.handle_formatting(Some(json!({
            "textDocument": { "uri": uri, "version": 3 },
            "options": { "tabSize": 4, "insertSpaces": true },
        })))?;
        let recovered_edits = recovered
            .and_then(|value| value.as_array().map(ToOwned::to_owned))
            .ok_or("expected formatting edits after full replacement")?;
        assert!(!recovered_edits.is_empty(), "accepted full replacement must restore formatting");
        Ok(())
    }

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

    #[test]
    fn handle_formatting_does_not_publish_in_flight_predecessor_after_violation()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        let uri = "file:///inflight_formatting.pl";
        server.test_apply_did_open(uri, "sub hello{my $x=1;return $x;}\n", 1)?;

        let snapshot = server
            .snapshot_user_answer_text(uri)
            .ok_or("open document must have a usable user-answer snapshot")?;
        let computed = server.handle_formatting(Some(json!({
            "textDocument": { "uri": uri, "version": 1 },
            "options": { "tabSize": 4, "insertSpaces": true },
        })))?;
        let computed_edits = computed
            .and_then(|value| value.as_array().map(ToOwned::to_owned))
            .ok_or("expected formatting edits before desync")?;
        assert!(
            !computed_edits.is_empty(),
            "in-flight formatting must see the predecessor document"
        );

        server.handle_did_change(Some(ranged_violation(uri, 2)))?;
        assert!(
            !server.user_answer_text_is_current(uri, snapshot.generation),
            "ranged violation must invalidate the captured formatting generation"
        );

        let error = server
            .handle_formatting(Some(json!({
                "textDocument": { "uri": uri, "version": 2 },
                "options": { "tabSize": 4, "insertSpaces": true },
            })))
            .err()
            .ok_or("live formatting after Full-sync violation must fail closed")?;
        assert_eq!(error.code, CONTENT_MODIFIED);
        assert!(
            error.message.contains("full-document resync")
                || error.message.contains("Document changed while formatting"),
            "post-violation formatting must not publish predecessor edits: {}",
            error.message
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
        server.client_session.advertised_features.lock().formatting = true;

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
                if poll_active.load(Ordering::SeqCst) && server_clone.documents.try_lock().is_some()
                {
                    poll_lock.store(true, Ordering::SeqCst);
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

    /// End-to-end regression for #16659: the ~2.1 MiB sync-ceiling stale
    /// snapshot. `didOpen` is admitted, formatting returns a whole-document
    /// edit, the client's post-format `didChange` exceeds the text-sync
    /// ceiling and is silently dropped at pre-dispatch admission — and the
    /// second formatting request MUST fail closed with `ContentModified`
    /// instead of returning stale-snapshot edits the client would splice over
    /// its actual (larger) buffer, duplicating the tail.
    ///
    /// Runs through the real dispatcher (`handle_request`), so the drop lands
    /// in `prepare_request`, exactly where production loses the notification.
    #[test]
    fn handle_formatting_refuses_after_over_ceiling_did_change_drop()
    -> Result<(), Box<dyn std::error::Error>> {
        use perl_lsp_rs_core::runtime::input_validation::text_sync_params_ceiling;
        use std::io::Cursor;
        use std::sync::Arc;

        // Captured out here so the request constructor below stays terse.
        use crate::runtime::JsonRpcRequest;

        // The default configuration's ceiling is 2,101,248 bytes
        // (maxFileSizeBytes 1,048,576 × 2 + 4,096 envelope headroom), so the
        // fixture is sized at the reported scale — deliberately with no
        // global-limit mutation, which would race non-serial sibling tests.
        let ceiling = text_sync_params_ceiling();

        let output = Arc::new(parking_lot::Mutex::new(Vec::<u8>::new()));
        struct CaptureWriter(Arc<parking_lot::Mutex<Vec<u8>>>);
        impl std::io::Write for CaptureWriter {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.lock().extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let server = LspServer::with_io(
            Box::new(Cursor::new(Vec::<u8>::new())),
            Box::new(CaptureWriter(Arc::clone(&output))),
        );

        let request = |id: Option<i64>, method: &str, params: Value| JsonRpcRequest {
            _jsonrpc: "2.0".to_string(),
            id: id.map(JsonRpcId::Integer),
            method: method.to_string(),
            params: Some(params),
        };

        // Initialize so textDocument/formatting passes the lifecycle gate.
        let init =
            server.handle_request(request(Some(1), "initialize", json!({ "capabilities": {} })));
        assert!(
            init.as_ref().is_some_and(|response| response.error.is_none()),
            "initialize must succeed for the dispatcher route"
        );

        // Unformatted one-line subs (~28 bytes each): the native formatter
        // expands this shape ~1.5x, which is what pushes the client's
        // post-format didChange frame over the ceiling while the didOpen
        // frame stays under it. 56,000 subs ⇒ didOpen frame ≈ 1.63 MB (78% of
        // the ceiling), formatted didChange frame ≈ 2.35 MB (112%).
        const N_SUBS: usize = 56_000;
        let mut text = String::with_capacity(N_SUBS * 32);
        for i in 0..N_SUBS {
            text.push_str(&format!("sub f_{i}{{my $x=7;return $x;}}\n"));
        }
        let uri = "file:///fmt_desync_16659.pl";

        // 1. didOpen — admitted (frame under the ceiling, stored as an
        //    oversize no-parse document exactly like the reported session).
        let open_params = json!({
            "textDocument": { "uri": uri, "languageId": "perl", "version": 1, "text": text },
        });
        let open_frame = serde_json::to_string(&open_params)?.len();
        assert!(
            open_frame < ceiling,
            "precondition: didOpen frame {open_frame} must be under ceiling {ceiling}"
        );
        assert!(
            server.handle_request(request(None, "textDocument/didOpen", open_params)).is_none(),
            "didOpen is a notification and must not respond"
        );

        // 2. formatting #1 — returns the whole-document edit.
        let fmt1 = server
            .handle_request(request(
                Some(2),
                "textDocument/formatting",
                json!({
                    "textDocument": { "uri": uri },
                    "options": { "tabSize": 4, "insertSpaces": true },
                }),
            ))
            .ok_or("formatting #1 must respond")?;
        let fmt1_error = fmt1.error.as_ref().map(|error| (error.code, error.message.clone()));
        assert!(fmt1_error.is_none(), "formatting #1 must succeed: {fmt1_error:?}");
        let edits1 = fmt1
            .result
            .and_then(|value| value.as_array().map(ToOwned::to_owned))
            .ok_or("formatting #1 must return an edits array")?;
        assert!(!edits1.is_empty(), "formatting #1 must return edits");
        let formatted = edits1[0]["newText"].as_str().ok_or("edit must carry newText")?.to_string();

        // 3. The client applies the edit and pushes the formatted buffer back.
        //    That didChange frame exceeds the ceiling and is silently dropped.
        let change_params = json!({
            "textDocument": { "uri": uri, "version": 2 },
            "contentChanges": [{ "text": formatted }],
        });
        let change_frame = serde_json::to_string(&change_params)?.len();
        assert!(
            change_frame > ceiling,
            "precondition: formatted didChange frame {change_frame} must exceed ceiling \
             {ceiling}; the fixture no longer reproduces the corruption shape"
        );
        assert!(
            server.handle_request(request(None, "textDocument/didChange", change_params)).is_none(),
            "didChange is a notification and must not respond"
        );

        // 4. formatting #2 — the stale-snapshot corruption case: it must fail
        //    closed, never return edits computed against the dropped buffer.
        let fmt2 = server
            .handle_request(request(
                Some(3),
                "textDocument/formatting",
                json!({
                    "textDocument": { "uri": uri },
                    "options": { "tabSize": 4, "insertSpaces": true },
                }),
            ))
            .ok_or("formatting #2 must respond")?;
        let error = fmt2
            .error
            .ok_or("formatting #2 after a dropped oversize sync must fail closed (#16659)")?;
        assert_eq!(
            error.code, CONTENT_MODIFIED,
            "stale-snapshot formatting must be ContentModified, not Applied"
        );
        assert!(
            error.message.contains("full-document resync"),
            "the refusal must be the full-sync fail-close, got: {}",
            error.message
        );
        assert!(fmt2.result.is_none(), "a refused formatting request must not carry edits");

        // 5. Recovery: an admitted full replacement (under the ceiling, newer
        //    version) restores currentness; formatting works again.
        assert!(
            server
                .handle_request(request(
                    None,
                    "textDocument/didChange",
                    json!({
                        "textDocument": { "uri": uri, "version": 3 },
                        "contentChanges": [{ "text": "sub recovered{my $y=2;return $y;}\n" }],
                    }),
                ))
                .is_none(),
            "didChange is a notification and must not respond"
        );
        let fmt3 = server
            .handle_request(request(
                Some(4),
                "textDocument/formatting",
                json!({
                    "textDocument": { "uri": uri },
                    "options": { "tabSize": 4, "insertSpaces": true },
                }),
            ))
            .ok_or("formatting #3 must respond")?;
        let recovered_edits = if fmt3.error.is_some() {
            0
        } else {
            fmt3.result.as_ref().and_then(|value| value.as_array()).map(Vec::len).unwrap_or(0)
        };
        assert!(
            recovered_edits > 0,
            "an admitted full replacement must restore formatting, got error {:?}",
            fmt3.error.as_ref().map(|error| (error.code, error.message.clone()))
        );

        // 6. The silent drop must have been surfaced exactly once.
        drop(server);
        let outbound = String::from_utf8(output.lock().clone())
            .map_err(|error| format!("outbound stream not valid UTF-8: {error}"))?;
        assert_eq!(
            outbound.matches("window/showMessage").count(),
            1,
            "exactly one desync warning per episode: {outbound}"
        );
        assert!(
            outbound.contains("\"type\":2"),
            "desync warning must be MessageType::Warning: {outbound}"
        );
        assert!(
            outbound.contains("text-sync limit"),
            "desync warning must name the sync limit: {outbound}"
        );
        Ok(())
    }
}
