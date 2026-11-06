use super::{
    Arc, AtomicU32, JsonRpcError, LspServer, NonZeroU32, Value, invalid_params, json,
    source_path_from_uri,
};
#[cfg(feature = "workspace")]
use crate::runtime::workspace::read_perl_source_file_with_byte_limit;
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

            // Notify coordinator of pending change to track cleanup work
            #[cfg(feature = "workspace")]
            if let Some(coordinator) = self.coordinator() {
                coordinator.notify_change(uri);
            }

            // Close returns source authority to the URI target, including when
            // the editor discards an ordinary unsaved edit without a watcher
            // event. Retire its indexed buffer facts with the session so failed
            // disk reads cannot leave discarded source searchable.
            #[cfg(feature = "workspace")]
            let mut retired_index_version = 0;
            let _closed_open_session = {
                #[cfg(feature = "workspace")]
                let _transition = self.indexing_transition_lock.lock();
                let was_open = self.document_is_open(uri);
                // Consume divergence under the same transition lock as eviction;
                // the marker must not leak into a successor open session.
                let _backing_transition = self.take_backing_file_transition(uri);
                self.evict_open_document_session_state(uri);
                self.clear_formatting_receipt_for_close(uri);
                #[cfg(feature = "workspace")]
                if was_open && let Some(coordinator) = self.coordinator() {
                    for key in Self::uri_key_variants(uri) {
                        coordinator.index().remove_file(&key);
                    }
                    retired_index_version = coordinator.index().write_version();
                }
                was_open
            };

            #[cfg(feature = "workspace")]
            if _closed_open_session && let Some(coordinator) = self.coordinator() {
                // Disk I/O stays outside the transition lock. Use the same
                // regular-file and Perl admission authority as discovery and
                // watchers: a directory/FIFO or declassified script is not
                // current Perl source. Repeated close of an unopened URI does
                // not create a new filesystem read surface.
                // The index revision is global: an unrelated write also
                // invalidates a read. One fresh retry preserves availability
                // without letting sustained mutation make close unbounded.
                for _ in 0..2 {
                    let content = source_path_from_uri(uri).and_then(|path| {
                        let url = url::Url::from_file_path(&path).ok()?;
                        let admission = self.discovery_admission_config(&path);
                        let byte_limit = crate::state::max_file_size_bytes();
                        match read_perl_source_file_with_byte_limit(
                            &path,
                            &admission,
                            Some(byte_limit),
                        ) {
                            // Decode before canonical binary classification: valid
                            // UTF-16 bytes contain NULs, and __DATA__ may be binary.
                            // Decoding can expand bytes beyond the ingress ceiling.
                            Ok(content) => content
                                .filter(|text| {
                                    text.len() <= byte_limit
                                        && !perl_parser_core::source_file::is_binary_content(text)
                                })
                                .map(|content| (url, content)),
                            Err(error) => {
                                tracing::debug!(
                                    "Failed to reload closed-file source {}: {}",
                                    path.display(),
                                    error
                                );
                                None
                            }
                        }
                    });
                    #[cfg(test)]
                    {
                        let hook = self.close_disk_loaded_hook.lock().take();
                        if let Some(hook) = hook {
                            hook();
                        }
                    }
                    let _transition = self.indexing_transition_lock.lock();
                    // A successor buffer remains authoritative. A watcher or
                    // scan sharing this transition lock can also publish newer
                    // closed-source facts while our disk read is off-lock.
                    if self.document_is_open(uri) {
                        break;
                    }
                    let current_revision = coordinator.index().write_version();
                    if current_revision != retired_index_version {
                        retired_index_version = current_revision;
                        continue;
                    }
                    let restored = if let Some((url, content)) = content {
                        // Closed-source initial import has no live generation.
                        // Use the resolved file identity, including equivalent
                        // URI spellings accepted by the close eviction surface.
                        match coordinator.index().index_initial_file(url, content) {
                            Ok(()) => true,
                            Err(error) => {
                                tracing::warn!("Failed to re-index {} on close: {}", uri, error);
                                false
                            }
                        }
                    } else {
                        false
                    };
                    if !restored {
                        // A fresh retry may reject backing or fail to import
                        // after a watcher published intervening facts. Its
                        // unchanged revision lets this failed handoff retire
                        // those obsolete facts too.
                        for key in Self::uri_key_variants(uri) {
                            coordinator.index().remove_file(&key);
                        }
                    }
                    break;
                }
            }

            // Closing hands project metadata authority back to disk (or to
            // definite absence for an unsaved metadata file). The open buffer
            // has been evicted and no document/folder lock is held here.
            self.refresh_metadata_for_document_uri(uri);

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
                        // Identical-text is a no-op only while the buffer is
                        // already synchronized. After a Full-sync violation the
                        // retained predecessor still equals the saved snapshot,
                        // but current answers stay closed until an admitted
                        // full replacement recovers the document.
                        (doc.full_sync_required() || doc.text.as_str() != saved_text)
                            .then(|| (saved_text.to_owned(), doc.version))
                    })
                };
                if let Some((saved_text, version)) = replacement {
                    tracing::debug!(uri, "didSave text differs from in-memory buffer; reconciling");
                    let result = self.handle_did_save_text_replacement(uri, &saved_text, version);
                    // didSave is a notification: an unstorable replacement can
                    // return Ok while marking Full-sync required. Refresh only
                    // when the saved text became the current admitted buffer.
                    if result.is_ok() {
                        let accepted = {
                            let documents = self.documents_guard();
                            self.get_document(&documents, &normalized_uri).is_some_and(|doc| {
                                !doc.full_sync_required() && doc.text_str() == saved_text.as_str()
                            })
                        };
                        if accepted {
                            self.refresh_metadata_for_document_uri(uri);
                            // The accepted parse ticket owns the consumer
                            // refresh after its index commit (#18116). Enqueue
                            // success alone does not establish fresh facts.
                        }
                    }
                    return result;
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
                // Read generation, document-instance handle, and -- only when
                // a commit is actually warranted -- the text in a SINGLE lock
                // to avoid TOCTOU. The staleness gate runs inside the lock
                // scope so the common up-to-date save does not pay the
                // full-text clone. The non-zero requirement is applied here
                // so generation zero (minimal/no-parse documents) can never
                // cross a live source commit.
                let doc_info = {
                    let documents = self.documents_guard();
                    self.get_document(&documents, &normalized_uri).and_then(|d| {
                        // A didSave without includeText cannot recover Full-sync.
                        // Predecessor `text_str` stays evidence only; generation-only
                        // index admission must not republish it as current.
                        if d.full_sync_required() {
                            return None;
                        }
                        let doc_gen_val = NonZeroU32::new(d.current_generation())?;
                        let needs_commit = backing_transition.is_some()
                            || self.coordinator().is_some_and(|coordinator| {
                                coordinator
                                    .index()
                                    .is_index_generation_stale(&normalized_uri, doc_gen_val.get())
                            });
                        needs_commit
                            .then(|| (doc_gen_val, d.generation.clone(), d.text_str().to_string()))
                    })
                };
                let mut index_reconciled = false;
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
                        let outcome = self.commit_live_save_reconciliation(
                            index,
                            &normalized_uri,
                            url,
                            doc_gen_val,
                            &instance,
                            text,
                            "#8041 backing-file re-cohere",
                        );
                        index_reconciled = matches!(
                            outcome,
                            Some(SourceCommitOutcome::Accepted | SourceCommitOutcome::NoOp)
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
                        let outcome = self.commit_live_save_reconciliation(
                            index,
                            &normalized_uri,
                            url,
                            doc_gen_val,
                            &instance,
                            text,
                            "stale-index reconciliation",
                        );
                        index_reconciled = matches!(
                            outcome,
                            Some(SourceCommitOutcome::Accepted | SourceCommitOutcome::NoOp)
                        );
                    }
                }

                // Reconciliation restores source-generation freshness, even
                // for same-content NoOp outcomes. Republish consumers the
                // same way the watched-file tails do (#18116); relying on the
                // client's watcher to echo the save is client-dependent.
                if index_reconciled {
                    self.refresh_diagnostics_after_watched_change();
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

    /// Handle willSaveWaitUntil request.
    ///
    /// Formatter-owned edits are withdrawn (#11955): this surface is no longer
    /// advertised and direct requests receive the truthful method-not-advertised
    /// refusal rather than an executable edit or a successful empty result.
    /// #8092 owns selecting and proving one save owner.
    pub(crate) fn handle_will_save_wait_until(
        &self,
        _params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        Err(crate::protocol::method_not_advertised())
    }
}
