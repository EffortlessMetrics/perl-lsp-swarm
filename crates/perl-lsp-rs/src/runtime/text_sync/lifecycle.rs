use super::{
    CodeFormatter, FormattingOptions, JsonRpcError, LspServer, Value, invalid_params, json,
    source_path_from_uri,
};
#[cfg(feature = "workspace")]
use perl_parser_core::source_file::is_perl_source_uri;

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

            // Complete the open→closed authority handoff (#8041):
            //
            // - No backing file: the source subject is gone; remove its
            //   disk-backed facts so `workspace/symbol` does not return stale
            //   entries after close.
            //
            // - Backing file present: closed-file authority is stable current
            //   disk source. The index entry may still hold facts derived from
            //   the open buffer (which may have diverged from disk), so
            //   stable-reread the backing file and re-index it. This runs after
            //   `evict_open_document_session_state`, so any late buffer-derived
            //   index task fails its generation oracle and cannot resurrect
            //   pre-divergence bytes.
            #[cfg(feature = "workspace")]
            {
                let file_on_disk = source_path_from_uri(uri).map(|p| p.exists()).unwrap_or(false);
                if !file_on_disk {
                    if let Some(coordinator) = self.coordinator() {
                        for key in self.uri_key_variants(uri) {
                            coordinator.index().remove_file(&key);
                        }
                    }
                } else {
                    // Reset the generation counters first so the reopened
                    // document — which starts at generation 0 — is not blocked
                    // by the stale high-water mark from the previous session
                    // (#5438).
                    if let Some(coordinator) = self.coordinator() {
                        for key in self.uri_key_variants(uri) {
                            coordinator.index().reset_generation_for_close(&key);
                        }

                        let disk_content = is_perl_source_uri(uri).then(|| {
                            crate::runtime::workspace::read_watched_file_content(
                                uri,
                                "didClose closed-authority reload",
                            )
                        });

                        match disk_content
                            .flatten()
                            .and_then(|content| url::Url::parse(uri).ok().map(|url| (url, content)))
                        {
                            Some((url, content)) => {
                                if let Err(e) = coordinator.index().index_file(url, content) {
                                    tracing::warn!(
                                        "Failed to reload closed file {} from disk: {}",
                                        uri,
                                        e
                                    );
                                    for key in self.uri_key_variants(uri) {
                                        coordinator.index().remove_file(&key);
                                    }
                                } else {
                                    tracing::debug!(
                                        "Reloaded closed file under disk authority: {}",
                                        uri
                                    );
                                }
                            }
                            None => {
                                // A rejected disk read must not leave the old
                                // buffer-derived facts looking current after
                                // the authority handoff. Fail closed until a
                                // valid disk read or a fresh didOpen rebuilds
                                // the source subject.
                                tracing::debug!(
                                    "Clearing closed file {} because no safe disk source was available",
                                    uri
                                );
                                for key in self.uri_key_variants(uri) {
                                    coordinator.index().remove_file(&key);
                                }
                            }
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
            tracing::debug!("Document saved: {}", uri);

            // When the client sends the full saved text in params.text,
            // reconcile the document's content through the normal full
            // replacement lifecycle (#4963/#5679). This ensures diagnostics
            // reflect the saved content without pairing new text with the
            // previous generation's parse snapshot.
            if let Some(saved_text) = params.pointer("/text").and_then(|v| v.as_str()) {
                let replacement = {
                    let documents = self.documents.lock();
                    self.get_document(&documents, &normalized_uri).and_then(|doc| {
                        (doc.text.as_str() != saved_text)
                            .then(|| (saved_text.to_owned(), doc.version))
                    })
                };
                if let Some((saved_text, version)) = replacement {
                    tracing::debug!(uri, "didSave text differs from in-memory buffer; reconciling");
                    return self.handle_did_save_text_replacement(uri, &saved_text, version);
                }
            }

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
                        .map(|d| (d.current_generation(), d.text_str().to_string()))
                };
                if let Some((doc_gen_val, text)) = doc_info
                    && let Some(coordinator) = self.coordinator()
                {
                    let index = coordinator.index();
                    if index.is_index_generation_stale(&normalized_uri, doc_gen_val)
                        && let Ok(url) = url::Url::parse(&normalized_uri)
                    {
                        tracing::debug!(
                            "Reconciling stale index for {} (doc gen {} > indexed gen)",
                            normalized_uri,
                            doc_gen_val
                        );
                        let _ = index.index_file(url, text);
                    }
                }
            }

            // Refresh through the generation-aware, canonical diagnostics path
            // after the synchronous stale-index reconciliation above. This
            // ordering matters when immediate diagnostics are enabled: semantic
            // projections must observe the same current index generation as the
            // saved document rather than publishing once from stale workspace
            // state and waiting for a later edit/save to correct it.
            //
            // The publisher snapshots the document under a brief lock, computes
            // off-lock, preserves push/pull policy, and rejects stale document
            // generations.
            self.publish_diagnostics_debounced(uri);

            // Optionally, trigger any post-save hooks here
            // For example: format on save, run tests, etc.
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

            // Reject stale requests: if the document version in the request is
            // older than the current version, the edit would apply to outdated
            // content (#5054). The non-save handle_formatting handler does the
            // same check (formatting.rs:129-131).
            let req_version =
                params["textDocument"]["version"].as_i64().and_then(|n| i32::try_from(n).ok());
            self.ensure_latest(uri, req_version)?;

            // Phase 1: snapshot text under brief lock, then drop.
            // Formatting can shell out to perltidy, so we must NOT hold locks
            // during the format call (#4643 off-lock pattern).
            if !self.is_formatting_enabled() || !self.config.lock().format_on_save {
                return Ok(Some(json!([])));
            }
            let text = {
                let documents = self.documents.lock();
                match self.get_document(&documents, uri) {
                    Some(doc) => doc.text_str().to_string(),
                    None => return Ok(Some(json!([]))),
                }
            };
            // locks dropped here

            // Phase 2: format off-lock using the user's actual perltidy config.
            let config = self.build_perltidy_config();
            let tab_size = config.indent_columns.unwrap_or(4);
            let insert_spaces = !config.tabs.unwrap_or(false);
            let formatter = CodeFormatter::with_config_and_mode(config, self.formatter_mode());
            let format_options = FormattingOptions {
                tab_size,
                insert_spaces,
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
