use super::{
    Arc, AtomicU32, CodeFormatter, FormattingOptions, JsonRpcError, LspServer, NonZeroU32, Value,
    invalid_params, json, source_path_from_uri,
};
use crate::runtime::BackingFileTransition;
#[cfg(feature = "workspace")]
use crate::runtime::workspace::read_watched_file_content;
#[cfg(feature = "workspace")]
use perl_workspace::workspace_index::{SourceCommit, SourceCommitOutcome};

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

            // Open-buffer authority (#8041): consume any pending backing-file
            // transition BEFORE eviction so this handoff resolves exactly once
            // and no stale marker leaks into a successor session.
            let backing_transition = self.take_backing_file_transition(uri);

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
            //
            // Exception (#8041): when the open session diverged from disk
            // (external change observed, or external delete recorded), the
            // retained snapshot would be pre-divergence bytes. Closed-file
            // authority is stable CURRENT disk source, so drop the old entry
            // and re-index from a fresh disk read.
            #[cfg(feature = "workspace")]
            {
                let file_on_disk = source_path_from_uri(uri).map(|p| p.exists()).unwrap_or(false);
                let session_diverged = matches!(
                    backing_transition,
                    Some(BackingFileTransition::Changed | BackingFileTransition::Deleted)
                );
                if !file_on_disk {
                    if let Some(coordinator) = self.coordinator() {
                        for key in self.uri_key_variants(uri) {
                            coordinator.index().remove_file(&key);
                        }
                    }
                } else {
                    if session_diverged && let Some(coordinator) = self.coordinator() {
                        for key in self.uri_key_variants(uri) {
                            coordinator.index().remove_file(&key);
                        }
                        if let Some(content) =
                            read_watched_file_content(uri, "closed-file authority reload")
                            && let Ok(url) = url::Url::parse(uri)
                        {
                            // #8041 owns this close-handoff semantic (a fresh
                            // disk read after divergence). The index entry was
                            // just removed, so this is an explicit
                            // initial-import role, not a live document
                            // generation commit.
                            match coordinator.index().index_initial_file(url, content) {
                                Ok(()) => {
                                    tracing::debug!(
                                        "Re-indexed {} from current disk on close (#8041)",
                                        uri
                                    );
                                }
                                Err(e) => {
                                    tracing::warn!("Failed to re-index {} on close: {}", uri, e);
                                }
                            }
                        }
                    }
                    // File is on disk: retain the index entry (symbols are still
                    // valid) but reset the generation counters so the reopened
                    // document — whose first accepted generation restarts at 1
                    // (#11305) — is not blocked by the stale high-water mark
                    // from the previous session (#5438).
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
            // Sink-owned admission (#8895): the save path resolves the URI for
            // diagnostics and index refresh, so it owns URI policy, judged on
            // the normalized key it will actually use.
            if let Err(err) = crate::security::validate_document_uri(&normalized_uri) {
                return Err(invalid_params(&err.to_string()));
            };
            tracing::debug!("Document saved: {}", uri);

            // Open-buffer authority (#8041): a save writes the editor buffer
            // to disk, re-cohering any recorded divergence (deleted or
            // externally changed backing file). Consume the marker exactly
            // once so it cannot leak into a later handoff.
            let backing_transition = self.take_backing_file_transition(uri);

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
            // #8041: a save resolves ANY recorded backing-file transition by
            // committing the authoritative buffer snapshot at its current
            // generation. `Changed` means disk moved on while open;
            // `Deleted` removed the index entry entirely so the generation
            // check alone would never fire; `RenamedOrMoved` recreates the
            // original path on save, which must regain its own facts.
            #[cfg(all(feature = "workspace", not(target_arch = "wasm32")))]
            {
                // Read generation, document-instance handle, and text in a
                // SINGLE lock to avoid TOCTOU. The non-zero requirement is
                // applied here so generation zero (minimal/no-parse documents)
                // can never cross a live source commit.
                let doc_info = {
                    let documents = self.documents.lock();
                    self.get_document(&documents, &normalized_uri).and_then(|d| {
                        let doc_gen_val = NonZeroU32::new(d.current_generation())?;
                        Some((doc_gen_val, d.generation.clone(), d.text_str().to_string()))
                    })
                };
                if let Some((doc_gen_val, instance, text)) = doc_info
                    && let Some(coordinator) = self.coordinator()
                {
                    let index = coordinator.index();
                    if backing_transition.is_some()
                        && let Ok(url) = url::Url::parse(&normalized_uri)
                    {
                        tracing::debug!(
                            "Re-cohering workspace index from saved buffer for {} (#8041)",
                            normalized_uri
                        );
                        self.commit_live_save_reconciliation(
                            index,
                            &normalized_uri,
                            url,
                            doc_gen_val,
                            &instance,
                            text,
                            "#8041 backing-file re-cohere",
                        );
                    } else if index.is_index_generation_stale(&normalized_uri, doc_gen_val.get())
                        && let Ok(url) = url::Url::parse(&normalized_uri)
                    {
                        tracing::debug!(
                            "Reconciling stale index for {} (doc gen {} > indexed gen)",
                            normalized_uri,
                            doc_gen_val.get()
                        );
                        // #11305: the captured accepted generation is the
                        // commit identity -- it is no longer discarded in
                        // favor of the initial-import path.
                        self.commit_live_save_reconciliation(
                            index,
                            &normalized_uri,
                            url,
                            doc_gen_val,
                            &instance,
                            text,
                            "stale-index reconciliation",
                        );
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

    /// Commit one didSave workspace-source reconciliation through the live
    /// source API (#11305).
    ///
    /// The save's captured `(document instance, accepted generation)` is
    /// revalidated by the sanctioned oracle
    /// ([`super::document_generation_still_current`]) immediately before the
    /// commit, then crosses [`WorkspaceIndex::index_live_file`] as a typed
    /// non-zero [`SourceCommit`]. Consequences:
    ///
    /// - a newer accepted edit makes the held save stale (`None`, no
    ///   workspace mutation);
    /// - close/reopen under the same URI makes the old instance's save work
    ///   stale even at an equal numeric generation (instance `Arc::ptr_eq`);
    /// - same-content saves remain proved no-ops bound to the SAME current
    ///   instance/generation;
    /// - failures stay typed and never roll back or discard the open buffer.
    #[cfg(all(feature = "workspace", not(target_arch = "wasm32")))]
    pub(crate) fn commit_live_save_reconciliation(
        &self,
        index: &perl_workspace::workspace_index::WorkspaceIndex,
        normalized_uri: &str,
        url: url::Url,
        generation: NonZeroU32,
        instance: &Arc<AtomicU32>,
        text: String,
        reason: &str,
    ) -> Option<SourceCommitOutcome> {
        if !super::document_generation_still_current(
            &self.documents,
            normalized_uri,
            generation.get(),
            instance,
        ) {
            tracing::debug!(
                uri = %normalized_uri,
                reason,
                "Skipping save reconciliation: document superseded before live commit"
            );
            return None;
        }
        let outcome = index.index_live_file(url, text, SourceCommit::new(generation));
        match &outcome {
            SourceCommitOutcome::Accepted | SourceCommitOutcome::NoOp => {}
            SourceCommitOutcome::RejectedStale => {
                tracing::debug!(
                    uri = %normalized_uri,
                    reason,
                    "Save reconciliation rejected: workspace accepted newer source"
                );
            }
            SourceCommitOutcome::Failed(e) => {
                tracing::warn!("Failed {} for {}: {}", reason, normalized_uri, e);
            }
        }
        Some(outcome)
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
}
