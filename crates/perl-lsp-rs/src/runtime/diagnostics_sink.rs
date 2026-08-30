//! Accepted-ticket push-diagnostics publication sink (#11673).
//!
//! Every parser-triggered `textDocument/publishDiagnostics` replacement or
//! clear commits through [`LspServer::commit_push_diagnostics`]: the
//! irreversible outbound enqueue happens inside one sink-local critical
//! section that (1) re-validates the candidate's accepted parse ticket
//! against the live document instance + generation, (2) compares and records
//! the committed diagnostic ticket and a monotonic per-URI sequence, and
//! (3) sends exactly one replacement or clear while still inside the section.
//!
//! This closes the gap left by check-before-callback guards
//! (`commit_parse_effect_if_current` wrapping a callback): the outbound
//! enqueue itself now carries the currentness decision. It also closes the
//! close/reopen ABA hole of value-only generation comparisons -- a stale
//! candidate derived from a removed document instance is rejected by instance
//! identity (`Arc::ptr_eq`), even when its numeric counter still matches.
//!
//! Lock order: workspace identity lock → sink `committed` lock → documents lock
//! (brief, read-only inside validation). No path may acquire them in reverse
//! order; `workspace_folders` must never be acquired while any of those locks
//! are held. Publish paths take their snapshots under the documents lock and
//! release it before committing.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicU32;

use parking_lot::Mutex;
use serde_json::Value;

use super::LspServer;

/// Accepted parse state a push-diagnostics candidate was derived from.
///
/// Mirrors the identity carried by `parse_worker::PublishedParseTicket`
/// (document instance + accepted generation); guarded no-parse paths mint the
/// same identity from the minimal state they just installed.
#[derive(Clone)]
pub(crate) struct PushDiagnosticIdentity {
    pub(crate) normalized_uri: String,
    pub(crate) document_instance: Arc<AtomicU32>,
    pub(crate) generation: u32,
    pub(crate) workspace_generation: u64,
    pub(crate) folder_config_generation: Option<u64>,
}

impl PushDiagnosticIdentity {
    pub(crate) fn for_document(
        normalized_uri: &str,
        document_instance: &Arc<AtomicU32>,
        generation: u32,
        workspace_generation: u64,
    ) -> Self {
        Self {
            normalized_uri: normalized_uri.to_string(),
            document_instance: Arc::clone(document_instance),
            generation,
            workspace_generation,
            folder_config_generation: None,
        }
    }

    pub(crate) fn with_folder_config_generation(mut self, generation: Option<u64>) -> Self {
        self.folder_config_generation = generation;
        self
    }
}

/// Whether the committed payload replaces the client-visible diagnostic set
/// or clears it (empty payload). Distinct outcomes so receipts can tell an
/// honest clean-clear from a content replacement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PushDiagnosticsDisposition {
    Replacement,
    Clear,
}

/// Exact outcome of one sink-boundary publication attempt. Claim-local
/// vocabulary (#11673), shaped for retargeting onto #11672's
/// `ParseEffectCommitOutcome` family when that contract lands.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PushDiagnosticsCommitOutcome {
    /// Current replacement enqueued at the boundary.
    CommittedCurrent,
    /// Current clear enqueued at the boundary.
    SafeClearCommitted,
    /// Document absent from the map: closed entirely, never opened, or
    /// already replaced by shutdown cleanup.
    RejectedDocumentClosed,
    /// Live document exists but is a different instance than the candidate's
    /// (close/reopen ABA or URI reuse).
    RejectedWrongDocumentInstance,
    /// Same document instance, but a newer generation was accepted before
    /// this candidate reached the boundary (or after an earlier commit in the
    /// ledger).
    RejectedSupersededGeneration,
    /// Validation passed but the outbound transport rejected the frame; the
    /// ledger entry is rolled back so receipt truth reflects the client.
    OutboundFailure,
}

/// Last diagnostic publication this server committed for a normalized URI.
struct CommittedPushDiagnostic {
    document_instance: Arc<AtomicU32>,
    generation: u32,
    sequence: u64,
}

/// Sink state: one committed-diagnostic record per open document URI.
#[derive(Default)]
pub(crate) struct PushDiagnosticsSink {
    committed: Mutex<HashMap<String, CommittedPushDiagnostic>>,
}

impl PushDiagnosticsSink {
    /// Test/receipt observation of the last committed record for `uri`:
    /// `(accepted generation, monotonic sequence)`.
    #[cfg(any(test, feature = "expose_lsp_test_api"))]
    pub(crate) fn last_committed(&self, normalized_uri: &str) -> Option<(u32, u64)> {
        self.committed.lock().get(normalized_uri).map(|entry| (entry.generation, entry.sequence))
    }
}

impl LspServer {
    pub(crate) fn project_config_for_uri(
        &self,
        uri: &str,
    ) -> Option<perl_lsp_rs_core::config::ProjectConfig> {
        self.folder_for_doc_uri(uri)
            .and_then(|folder| folder.project_config)
            .or_else(|| self.single_file_project_config.lock().clone())
    }

    pub(crate) fn project_config_generation_for_uri(&self, uri: &str) -> Option<u64> {
        self.folder_for_doc_uri(uri).map(|folder| folder.project_config_generation).or_else(|| {
            self.single_file_project_config.lock().as_ref()?;
            Some(
                self.single_file_project_config_generation
                    .load(std::sync::atomic::Ordering::SeqCst),
            )
        })
    }

    pub(crate) fn set_single_file_project_config(
        &self,
        config: Option<perl_lsp_rs_core::config::ProjectConfig>,
    ) {
        let mut single_file_project_config = self.single_file_project_config.lock();
        let changed = *single_file_project_config != config;
        *single_file_project_config = config;
        if changed {
            self.single_file_project_config_generation
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }

    pub(crate) fn invalidate_workspace_identity(&self) {
        let _identity_guard = self.workspace_identity_lock.lock();
        self.workspace_identity_generation.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    /// Commit one push-diagnostics replacement/clear at the sink boundary.
    ///
    /// See the module docs for the boundary contract. `payload` must be the
    /// complete `textDocument/publishDiagnostics` params value; it is sent via
    /// the ordinary outbound sink only while the boundary holds.
    pub(crate) fn commit_push_diagnostics(
        &self,
        identity: &PushDiagnosticIdentity,
        payload: Value,
        disposition: PushDiagnosticsDisposition,
    ) -> PushDiagnosticsCommitOutcome {
        // Serialize the generation check and irreversible enqueue with
        // configuration invalidation.
        let folder_config_generation =
            self.project_config_generation_for_uri(&identity.normalized_uri);
        let _identity_guard = self.workspace_identity_lock.lock();
        let mut committed = self.push_diagnostics_sink.committed.lock();

        if self.workspace_identity_generation.load(std::sync::atomic::Ordering::SeqCst)
            != identity.workspace_generation
        {
            return PushDiagnosticsCommitOutcome::RejectedSupersededGeneration;
        }

        if folder_config_generation != identity.folder_config_generation {
            return PushDiagnosticsCommitOutcome::RejectedSupersededGeneration;
        }

        // 1+2. Exact currency at the boundary: live document instance AND
        // accepted generation, checked under one brief documents acquisition.
        let currency = {
            let docs = self.documents.lock();
            docs.get(&identity.normalized_uri).map(|doc| {
                (
                    Arc::ptr_eq(&doc.generation, &identity.document_instance),
                    doc.current_generation(),
                )
            })
        };
        let rejection = match currency {
            None => PushDiagnosticsCommitOutcome::RejectedDocumentClosed,
            Some((false, _)) => PushDiagnosticsCommitOutcome::RejectedWrongDocumentInstance,
            Some((true, live_generation)) if live_generation != identity.generation => {
                PushDiagnosticsCommitOutcome::RejectedSupersededGeneration
            }
            Some((true, _)) => {
                return self.enqueue_committed_push_diagnostic(
                    &mut committed,
                    identity,
                    payload,
                    disposition,
                );
            }
        };

        tracing::debug!(
            uri = %identity.normalized_uri,
            generation = identity.generation,
            ?rejection,
            "Rejected push-diagnostics candidate at sink boundary"
        );
        rejection
    }

    /// Ledger compare/record + outbound enqueue. Caller has already proven
    /// ticket currency and holds the sink lock.
    fn enqueue_committed_push_diagnostic(
        &self,
        committed: &mut HashMap<String, CommittedPushDiagnostic>,
        identity: &PushDiagnosticIdentity,
        payload: Value,
        disposition: PushDiagnosticsDisposition,
    ) -> PushDiagnosticsCommitOutcome {
        // 3. Sequence law: a candidate older than what this sink already
        // committed for the same document instance can no longer enqueue --
        // its validation raced a newer acceptance between the check above and
        // this ledger step is impossible (same critical section), but a
        // caller holding an older pre-validated identity must not regress the
        // record either.
        if let Some(entry) = committed.get(&identity.normalized_uri)
            && Arc::ptr_eq(&entry.document_instance, &identity.document_instance)
            && identity.generation < entry.generation
        {
            return PushDiagnosticsCommitOutcome::RejectedSupersededGeneration;
        }

        let sequence =
            committed.get(&identity.normalized_uri).map(|entry| entry.sequence + 1).unwrap_or(1);
        let previous = committed.insert(
            identity.normalized_uri.clone(),
            CommittedPushDiagnostic {
                document_instance: Arc::clone(&identity.document_instance),
                generation: identity.generation,
                sequence,
            },
        );

        // 4. Irreversible enqueue inside the boundary. A concurrent callback
        // blocks here until this send has been recorded, then fails its own
        // validation if the world moved on.
        match self.notify("textDocument/publishDiagnostics", payload) {
            Ok(()) => {
                tracing::debug!(
                    uri = %identity.normalized_uri,
                    generation = identity.generation,
                    sequence,
                    ?disposition,
                    "Committed push diagnostics at sink boundary"
                );
                // Attach the required-effect outcome to active-document
                // parser readiness (#11675): a committed replacement or
                // clear for this exact ticket is the profile-v1
                // diagnostics row's accepted terminal outcome.
                self.attach_active_document_effect(
                    &identity.normalized_uri,
                    &identity.document_instance,
                    identity.generation,
                    crate::runtime::readiness::CoreEffectKind::ParserDiagnosticsPublication,
                );
                match disposition {
                    PushDiagnosticsDisposition::Replacement => {
                        PushDiagnosticsCommitOutcome::CommittedCurrent
                    }
                    PushDiagnosticsDisposition::Clear => {
                        PushDiagnosticsCommitOutcome::SafeClearCommitted
                    }
                }
            }
            Err(error) => {
                // Roll the ledger back so receipts do not claim a publication
                // the client never received.
                match previous {
                    Some(restored) => {
                        committed.insert(identity.normalized_uri.clone(), restored);
                    }
                    None => {
                        committed.remove(&identity.normalized_uri);
                    }
                }
                tracing::error!(
                    uri = %identity.normalized_uri,
                    error = %error,
                    "Outbound failure committing push diagnostics"
                );
                PushDiagnosticsCommitOutcome::OutboundFailure
            }
        }
    }

    /// Receipt observation for focused tests.
    #[cfg(any(test, feature = "expose_lsp_test_api"))]
    pub(crate) fn test_last_committed_push_diagnostic(
        &self,
        normalized_uri: &str,
    ) -> Option<(u32, u64)> {
        self.push_diagnostics_sink.last_committed(normalized_uri)
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "tracked conversion debt: https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3021"
    )]
    #![allow(clippy::expect_used)]

    use super::{
        LspServer, PushDiagnosticIdentity, PushDiagnosticsCommitOutcome, PushDiagnosticsDisposition,
    };
    use serde_json::json;
    use std::io::Write;
    use std::sync::Arc as StdArc;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct SharedVecWriter {
        inner: StdArc<parking_lot::Mutex<Vec<u8>>>,
    }
    impl Write for SharedVecWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.inner.lock().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn make_server() -> (LspServer, StdArc<parking_lot::Mutex<Vec<u8>>>) {
        let buf = StdArc::new(parking_lot::Mutex::new(Vec::<u8>::new()));
        let writer = SharedVecWriter { inner: StdArc::clone(&buf) };
        let server =
            LspServer::with_io(Box::new(std::io::Cursor::new(Vec::<u8>::new())), Box::new(writer));
        (server, buf)
    }

    fn open_document(server: &LspServer, uri: &str, text: &str) -> PushDiagnosticIdentity {
        server
            .test_handle_did_open(Some(json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "perl",
                    "version": 1,
                    "text": text
                }
            })))
            .expect("didOpen should succeed");
        let key = server.normalize_uri_key(uri);
        let docs = server.documents.lock();
        let doc = docs.get(&key).expect("document must be open");
        PushDiagnosticIdentity::for_document(
            &key,
            &doc.generation,
            doc.current_generation(),
            server.workspace_identity_generation.load(std::sync::atomic::Ordering::SeqCst),
        )
        .with_folder_config_generation(server.project_config_generation_for_uri(&key))
    }

    fn frame_count(buf: &StdArc<parking_lot::Mutex<Vec<u8>>>) -> usize {
        String::from_utf8_lossy(&buf.lock()).matches("publishDiagnostics").count()
    }

    /// Wait for the outbound writer thread to flush at least `minimum` frames.
    fn wait_for_frames(buf: &StdArc<parking_lot::Mutex<Vec<u8>>>, minimum: usize) -> bool {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while std::time::Instant::now() < deadline {
            if frame_count(buf) >= minimum {
                return true;
            }
            std::thread::yield_now();
        }
        false
    }

    #[test]
    fn current_commit_records_ticket_and_sequence() {
        let (server, _buf) = make_server();
        let identity = open_document(&server, "file:///sink_seq_test.pl", "my $x = 1;\n");

        // didOpen already committed through the sink (debounced-immediate
        // full publish); the ledger sequence must advance by exactly one per
        // subsequent commit, whatever the baseline is.
        let baseline = server
            .test_last_committed_push_diagnostic(&identity.normalized_uri)
            .unwrap_or((identity.generation, 0));

        let outcome = server.commit_push_diagnostics(
            &identity,
            json!({ "uri": "file:///sink_seq_test.pl", "diagnostics": [] }),
            PushDiagnosticsDisposition::Clear,
        );
        assert_eq!(outcome, PushDiagnosticsCommitOutcome::SafeClearCommitted);
        assert_eq!(
            server.test_last_committed_push_diagnostic(&identity.normalized_uri),
            Some((identity.generation, baseline.1 + 1))
        );

        let outcome = server.commit_push_diagnostics(
            &identity,
            json!({ "uri": "file:///sink_seq_test.pl", "version": 1, "diagnostics": [] }),
            PushDiagnosticsDisposition::Replacement,
        );
        assert_eq!(outcome, PushDiagnosticsCommitOutcome::CommittedCurrent);
        assert_eq!(
            server.test_last_committed_push_diagnostic(&identity.normalized_uri),
            Some((identity.generation, baseline.1 + 2)),
            "sequence must advance monotonically per committed publication"
        );
    }

    #[test]
    fn superseded_generation_callback_is_rejected_without_frame() {
        let (server, buf) = make_server();
        let uri = "file:///sink_superseded_test.pl";
        let identity = open_document(&server, uri, "my $stale = 1;\n");
        assert!(wait_for_frames(&buf, 1), "didOpen publication must flush first");

        // First explicit commit for generation N succeeds and adds a frame.
        let frames_before = frame_count(&buf);
        assert_eq!(
            server.commit_push_diagnostics(
                &identity,
                json!({ "uri": uri, "version": 1, "diagnostics": [] }),
                PushDiagnosticsDisposition::Replacement,
            ),
            PushDiagnosticsCommitOutcome::CommittedCurrent
        );
        assert!(
            wait_for_frames(&buf, frames_before + 1),
            "current candidate must enqueue exactly one frame"
        );

        // N+1 acceptance advances the live generation.
        let live_generation = {
            let docs = server.documents.lock();
            StdArc::clone(&docs.get(&identity.normalized_uri).unwrap().generation)
        };
        live_generation.store(identity.generation + 1, Ordering::SeqCst);

        // The late N candidate must be rejected at the boundary: no further
        // frame may reach the client after newer state was accepted.
        assert_eq!(
            server.commit_push_diagnostics(
                &identity,
                json!({ "uri": uri, "version": 1, "diagnostics": [] }),
                PushDiagnosticsDisposition::Replacement,
            ),
            PushDiagnosticsCommitOutcome::RejectedSupersededGeneration
        );
        let final_frames = frame_count(&buf);
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert_eq!(
            frame_count(&buf),
            final_frames,
            "rejected stale candidate must not enqueue a frame"
        );
    }

    #[test]
    fn workspace_identity_invalidation_rejects_pre_reload_candidate() {
        let (server, _buf) = make_server();
        let identity = open_document(&server, "file:///sink_reload_test.pl", "my $x = 1;\n");

        server.invalidate_workspace_identity();

        assert_eq!(
            server.commit_push_diagnostics(
                &identity,
                json!({ "uri": identity.normalized_uri, "diagnostics": [] }),
                PushDiagnosticsDisposition::Clear,
            ),
            PushDiagnosticsCommitOutcome::RejectedSupersededGeneration
        );
    }

    #[test]
    fn folder_config_invalidation_rejects_only_owned_push_candidate() {
        let (server, _buf) = make_server();
        server.workspace_folders.lock().extend([
            super::super::workspace_folder::WorkspaceFolderState::new(
                "file:///sink-root-one/".to_string(),
            ),
            super::super::workspace_folder::WorkspaceFolderState::new(
                "file:///sink-root-two/".to_string(),
            ),
        ]);

        let first = open_document(&server, "file:///sink-root-one/first.pl", "my $x = 1;\n");
        let second = open_document(&server, "file:///sink-root-two/second.pl", "my $y = 1;\n");
        let first_generation = server
            .workspace_folders
            .lock()
            .first()
            .expect("first folder must exist")
            .project_config_generation;

        server.workspace_folders.lock()[0].project_config_generation += 1;

        assert_eq!(
            server.commit_push_diagnostics(
                &first,
                json!({ "uri": first.normalized_uri, "diagnostics": [] }),
                PushDiagnosticsDisposition::Clear,
            ),
            PushDiagnosticsCommitOutcome::RejectedSupersededGeneration
        );
        assert_eq!(
            server.commit_push_diagnostics(
                &second,
                json!({ "uri": second.normalized_uri, "diagnostics": [] }),
                PushDiagnosticsDisposition::Clear,
            ),
            PushDiagnosticsCommitOutcome::SafeClearCommitted
        );
        assert_eq!(
            server.workspace_folders.lock()[0].project_config_generation,
            first_generation + 1
        );
    }

    #[test]
    fn wrong_instance_callback_after_close_reopen_is_rejected() {
        let (server, buf) = make_server();
        let uri = "file:///sink_aba_test.pl";
        let identity = open_document(&server, uri, "my $old = 1;\n");
        assert!(wait_for_frames(&buf, 1), "didOpen publication must flush first");
        let frames_before = frame_count(&buf);

        // didClose + didOpen same URI: fresh instance, coincidentally equal
        // numeric generation.
        let rope = ropey::Rope::from_str("my $new = 2;\n");
        let fresh = super::super::DocumentState::from_parts(
            rope,
            "my $new = 2;\n".to_string(),
            1,
            StdArc::new(AtomicU32::new(crate::state::FIRST_ACCEPTED_DOCUMENT_GENERATION.get())),
        );
        server.documents.lock().insert(identity.normalized_uri.clone(), fresh);

        assert_eq!(
            server.commit_push_diagnostics(
                &identity,
                json!({ "uri": uri, "version": 1, "diagnostics": [] }),
                PushDiagnosticsDisposition::Replacement,
            ),
            PushDiagnosticsCommitOutcome::RejectedWrongDocumentInstance
        );
        assert_eq!(
            frame_count(&buf),
            frames_before,
            "wrong-instance candidate must not enqueue a frame"
        );
    }

    #[test]
    fn closed_document_candidate_is_rejected() {
        let (server, _buf) = make_server();
        let identity = open_document(&server, "file:///sink_closed_test.pl", "my $y = 1;\n");

        server.documents.lock().remove(&identity.normalized_uri);

        assert_eq!(
            server.commit_push_diagnostics(
                &identity,
                json!({ "uri": "file:///sink_closed_test.pl", "diagnostics": [] }),
                PushDiagnosticsDisposition::Clear,
            ),
            PushDiagnosticsCommitOutcome::RejectedDocumentClosed
        );
        // The earlier didOpen commit remains the truthful receipt for this
        // URI; a closed-document rejection never adds to it.
        let after = server.test_last_committed_push_diagnostic(&identity.normalized_uri);
        assert!(after.is_none_or(|(_, sequence)| sequence >= 1));
    }
}
