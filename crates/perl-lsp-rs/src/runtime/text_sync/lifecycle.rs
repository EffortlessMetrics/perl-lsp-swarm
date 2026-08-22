use super::{
    CodeFormatter, FormattingOptions, JsonRpcError, LspServer, Value, invalid_params, json,
    source_path_from_uri,
};

/// One held didSave stale-index reconciliation candidate.
///
/// Captured under a single `documents` lock: the EXACT document instance
/// Arc, its accepted generation, and the buffer text the save would commit.
/// The commit step re-validates both identity components at its own
/// boundary (#11305).
#[cfg(all(feature = "workspace", not(target_arch = "wasm32")))]
pub(crate) struct DidSaveIndexReconcile {
    url: url::Url,
    instance: std::sync::Arc<std::sync::atomic::AtomicU32>,
    generation: u32,
    text: String,
}

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
                } else {
                    // File is on disk: retain the index entry (symbols are still
                    // valid) but reset the generation counters so the reopened
                    // document — which starts at generation 0 — is not blocked
                    // by the stale high-water mark from the previous session
                    // (#5438).
                    if let Some(coordinator) = self.coordinator() {
                        for key in self.uri_key_variants(uri) {
                            coordinator.index().reset_generation_for_close(&key);
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
            //
            // Capture and commit are separate steps so deterministic tests
            // can hold a save's reconciliation across a newer edit; the
            // commit re-validates the exact captured instance/generation at
            // its own boundary (#11305).
            #[cfg(all(feature = "workspace", not(target_arch = "wasm32")))]
            if let Some(candidate) = self.capture_did_save_index_reconcile(&normalized_uri) {
                self.commit_did_save_index_reconcile(candidate);
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

    /// One held didSave stale-index reconciliation candidate.
    ///
    /// Captured under a single `documents` lock: the EXACT document instance
    /// Arc, its accepted generation, and the buffer text the save would
    /// commit. The commit step re-validates both identity components at its
    /// own boundary (#11305).
    #[cfg(all(feature = "workspace", not(target_arch = "wasm32")))]
    pub(crate) fn capture_did_save_index_reconcile(
        &self,
        normalized_uri: &str,
    ) -> Option<DidSaveIndexReconcile> {
        let captured = {
            let documents = self.documents.lock();
            self.get_document(&documents, normalized_uri).map(|d| {
                (
                    std::sync::Arc::clone(&d.generation),
                    d.current_generation(),
                    d.text_str().to_string(),
                )
            })
        };
        let (instance, generation, text) = captured?;
        let coordinator = self.coordinator()?;
        let index = coordinator.index();
        if !index.is_index_generation_stale(normalized_uri, generation) {
            return None;
        }
        let url = url::Url::parse(normalized_uri).ok()?;
        Some(DidSaveIndexReconcile { url, instance, generation, text })
    }

    /// Commit one held didSave reconciliation as a typed live source
    /// commit behind the sanctioned final-currentness oracle (#11305).
    ///
    /// Returns `None` when the document moved on between capture and
    /// commit -- a newer edit bumped the generation, or close/reopen
    /// swapped the instance Arc -- leaving accepted workspace source and
    /// facts untouched. `Some(outcome)` reports the live API's typed
    /// accepted/no-op/stale/failed disposition.
    #[cfg(all(feature = "workspace", not(target_arch = "wasm32")))]
    pub(crate) fn commit_did_save_index_reconcile(
        &self,
        candidate: DidSaveIndexReconcile,
    ) -> Option<perl_parser::workspace_index::SourceCommitOutcome> {
        use perl_parser::workspace_index::{SourceCommit, SourceCommitOutcome};

        let normalized_uri = self.normalize_uri_key(candidate.url.as_str());
        tracing::debug!(
            uri = %normalized_uri,
            generation = candidate.generation,
            "Committing held didSave stale-index reconciliation"
        );
        let index = self.coordinator()?.index();
        // Zero identity cannot occur for a parseable open document (didOpen
        // mints generation 1 and edits only bump); refuse loudly rather than
        // touch the index through any compatibility surface.
        let Some(commit_gen) = std::num::NonZeroU32::new(candidate.generation) else {
            tracing::error!(
                uri = %normalized_uri,
                "didSave reconciliation carried a zero document generation; \
                 refusing the generation-less compatibility surface"
            );
            return None;
        };
        let outcome = super::commit_parse_effect_if_current(
            &self.documents,
            &normalized_uri,
            candidate.generation,
            &candidate.instance,
            || index.index_live_file(candidate.url, candidate.text, SourceCommit::new(commit_gen)),
        );
        match &outcome {
            Some(SourceCommitOutcome::Failed(e)) => {
                tracing::warn!(
                    uri = %normalized_uri,
                    "didSave stale-index reconciliation failed: {}",
                    e
                );
            }
            Some(outcome) => {
                tracing::debug!(
                    uri = %normalized_uri,
                    outcome = ?outcome,
                    "didSave stale-index reconciliation settled"
                );
            }
            None => {
                tracing::debug!(
                    uri = %normalized_uri,
                    "didSave stale-index reconciliation skipped: \
                     document changed before the live commit"
                );
            }
        }
        outcome
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
