use super::*;

impl LspServer {
    /// Handle didClose notification
    ///
    /// Deterministic state transition: notify coordinator of document close
    /// so it can update pending change tracking if needed.
    pub(crate) fn handle_did_close(&self, params: Option<Value>) -> Result<(), JsonRpcError> {
        if let Some(params) = params {
            let uri = params
                .pointer("/textDocument/uri")
                .and_then(|v| v.as_str())
                .ok_or_else(|| invalid_params("Missing required parameter: textDocument.uri"))?;

            tracing::debug!("Document closed: {}", uri);

            // Notify coordinator of pending change to track cleanup work
            #[cfg(feature = "workspace")]
            if let Some(coordinator) = self.coordinator() {
                coordinator.notify_change(uri);
            }

            self.evict_open_document_session_state(uri);

            // If the closed document has no backing file on disk it existed only in
            // the editor buffer (e.g. a new unsaved file or a test virtual document).
            // Remove it from the workspace index so `workspace/symbol` does not
            // return stale entries after close.
            //
            // For files that do exist on disk, closing the editor buffer leaves the
            // workspace index intact: the file is still part of the project and a
            // workspace scan would re-discover it.
            #[cfg(feature = "workspace")]
            {
                let file_on_disk = source_path_from_uri(uri).map(|p| p.exists()).unwrap_or(false);
                if !file_on_disk {
                    if let Some(coordinator) = self.coordinator() {
                        for key in self.uri_key_variants(uri) {
                            coordinator.index().remove_file(&key);
                        }
                    }
                }
            }

            // Notify coordinator that cleanup is complete
            #[cfg(feature = "workspace")]
            if let Some(coordinator) = self.coordinator() {
                coordinator.notify_parse_complete(uri);
            }

            // Clear diagnostics for this file using centralized notify
            if let Err(e) = self.notify(
                "textDocument/publishDiagnostics",
                json!({
                    "uri": uri,
                    "diagnostics": []
                }),
            ) {
                tracing::warn!("Failed to clear diagnostics for {}: {}", uri, e);
            }
        }

        Ok(())
    }

    /// Handle didSave notification
    pub(crate) fn handle_did_save(&self, params: Option<Value>) -> Result<(), JsonRpcError> {
        if let Some(params) = params {
            let uri = params
                .pointer("/textDocument/uri")
                .and_then(|v| v.as_str())
                .ok_or_else(|| invalid_params("Missing required parameter: textDocument.uri"))?;
            let normalized_uri = self.normalize_uri_key(uri);
            let _version = params
                .pointer("/textDocument/version")
                .and_then(|v| v.as_i64())
                .and_then(|v| i32::try_from(v).ok());

            tracing::debug!("Document saved: {}", uri);

            // Re-run diagnostics on save to catch any changes
            let documents = self.documents.lock();
            if let Some(doc) = self.get_document(&documents, &normalized_uri) {
                let parsed = doc.current_parsed();
                if let Some(ast) = parsed.as_ref().and_then(|p| p.ast()) {
                    // `parsed` is guaranteed `Some` here since `ast` was
                    // derived from it.
                    let empty_errors: Arc<[perl_parser::error::ParseError]> = Arc::from([]);
                    let parse_errors = parsed
                        .as_ref()
                        .map_or_else(|| empty_errors.clone(), |p| p.parse_errors_arc());
                    // Run diagnostics, threading workspace semantic queries when available.
                    let provider = DiagnosticsProvider::new(ast, doc.text.clone());
                    let source_path = source_path_from_uri(uri);

                    #[cfg(all(feature = "workspace", not(target_arch = "wasm32")))]
                    let diagnostics = {
                        // Attempt semantic-aware path; fall back to legacy when URI not indexed.
                        let semantic_diags = self.workspace_index().and_then(|workspace_index| {
                            workspace_index.with_semantic_queries_for_uri(
                                uri,
                                |file_id, queries| {
                                    provider.get_diagnostics_with_path_and_semantics(
                                        ast,
                                        &parse_errors,
                                        &doc.text,
                                        None,
                                        &[],
                                        source_path.as_deref(),
                                        file_id,
                                        &queries,
                                    )
                                },
                            )
                        });
                        semantic_diags.unwrap_or_else(|| {
                            provider.get_diagnostics_with_path(
                                ast,
                                &parse_errors,
                                &doc.text,
                                None,
                                &[],
                                source_path.as_deref(),
                            )
                        })
                    };
                    #[cfg(not(all(feature = "workspace", not(target_arch = "wasm32"))))]
                    let diagnostics = provider.get_diagnostics_with_path(
                        ast,
                        &parse_errors,
                        &doc.text,
                        None,
                        &[],
                        source_path.as_deref(),
                    );

                    // Convert diagnostics
                    let lsp_diagnostics: Vec<Value> = diagnostics
                        .iter()
                        .map(|diag| {
                            let (start_line, start_char) = self.offset_to_pos16(doc, diag.range.0);
                            let (end_line, end_char) = self.offset_to_pos16(doc, diag.range.1);

                            json!({
                                "range": {
                                    "start": { "line": start_line, "character": start_char },
                                    "end": { "line": end_line, "character": end_char }
                                },
                                "severity": match diag.severity {
                                    InternalDiagnosticSeverity::Error => 1,
                                    InternalDiagnosticSeverity::Warning => 2,
                                    InternalDiagnosticSeverity::Information => 3,
                                    InternalDiagnosticSeverity::Hint => 4,
                                },
                                "message": diag.message,
                                "source": "perl"
                            })
                        })
                        .collect();

                    // Send diagnostics notification
                    if let Err(e) = self.notify(
                        "textDocument/publishDiagnostics",
                        json!({
                            "uri": uri,
                            "diagnostics": lsp_diagnostics
                        }),
                    ) {
                        tracing::warn!("Failed to publish diagnostics for {}: {}", uri, e);
                    }
                }
            }

            // Optionally, trigger any post-save hooks here
            // For example: format on save, run tests, etc.

            // Reconcile: if the saved document's index is stale (generation
            // lag from coalesced parse jobs), re-index it now from the
            // in-memory text. This prevents permanent index lag where no
            // async parse ticket ever catches the index up. (#5111)
            #[cfg(all(feature = "workspace", not(target_arch = "wasm32")))]
            {
                // Read both generation and text in a SINGLE lock to avoid TOCTOU.
                let doc_info = {
                    let documents = self.documents.lock();
                    self.get_document(&documents, &normalized_uri)
                        .map(|d| (d.current_generation(), d.text.clone()))
                };
                if let Some((doc_gen_val, text)) = doc_info
                    && let Some(coordinator) = self.coordinator()
                {
                    let index = coordinator.index();
                    if index.is_index_generation_stale(&normalized_uri, doc_gen_val) {
                        if let Ok(url) = url::Url::parse(&normalized_uri) {
                            tracing::debug!(
                                "Reconciling stale index for {} (doc gen {} > indexed gen)",
                                normalized_uri,
                                doc_gen_val
                            );
                            let _ = index.index_file(url, text);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Handle willSave notification
    pub(crate) fn handle_will_save(&self, params: Option<Value>) -> Result<(), JsonRpcError> {
        if let Some(params) = params {
            let uri = params["textDocument"]["uri"].as_str().unwrap_or("");
            let reason = params["reason"].as_u64().unwrap_or(1); // 1 = Manual, 2 = AfterDelay, 3 = FocusOut

            tracing::debug!("Document will save: {} (reason: {})", uri, reason);

            // Pre-save validation or cleanup can be done here
            // For example: remove trailing whitespace, fix imports, etc.
        }

        Ok(())
    }

    /// Handle willSaveWaitUntil request
    pub(crate) fn handle_will_save_wait_until(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        if let Some(params) = params {
            let uri = params["textDocument"]["uri"].as_str().unwrap_or("");

            tracing::debug!("Document will save wait until: {}", uri);

            // Phase 1: snapshot text under brief lock, then drop.
            // Formatting can shell out to perltidy, so we must NOT hold locks
            // during the format call (#4643 off-lock pattern).
            if !self.is_formatting_enabled() {
                return Ok(Some(json!([])));
            }
            let text = {
                let documents = self.documents.lock();
                match self.get_document(&documents, uri) {
                    Some(doc) => doc.text.clone(),
                    None => return Ok(Some(json!([]))),
                }
            };
            // locks dropped here

            // Phase 2: format off-lock using the user's actual perltidy config.
            let config = self.build_perltidy_config();
            let formatter = CodeFormatter::with_config_and_mode(config, self.formatter_mode());
            let format_options = FormattingOptions {
                tab_size: 4,
                insert_spaces: true,
                trim_trailing_whitespace: Some(true),
                insert_final_newline: Some(true),
                trim_final_newlines: Some(true),
            };

            match formatter.format_document(&text, &format_options) {
                Ok(edits) if !edits.is_empty() => {
                    let lsp_edits: Vec<Value> = edits
                        .iter()
                        .map(|edit| {
                            json!({
                                "range": {
                                    "start": {
                                        "line": edit.range.start.line,
                                        "character": edit.range.start.character
                                    },
                                    "end": {
                                        "line": edit.range.end.line,
                                        "character": edit.range.end.character
                                    }
                                },
                                "newText": edit.new_text
                            })
                        })
                        .collect();
                    Ok(Some(json!(lsp_edits)))
                }
                _ => Ok(Some(json!([]))),
            }
        } else {
            Ok(Some(json!([])))
        }
    }

    /// Get the end position of a document
    pub(crate) fn get_document_end_position(&self, content: &str) -> Value {
        let lines: Vec<&str> = content.split('\n').collect();
        let last_line = lines.len().saturating_sub(1);
        let last_char = lines.last().map(|l| l.len()).unwrap_or(0);

        json!({
            "line": last_line,
            "character": last_char
        })
    }
}
