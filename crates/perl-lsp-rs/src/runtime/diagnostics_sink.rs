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
//! Lock order: workspace identity → documents → config → sink (push only).
//! Folder ownership is sampled before these guards and generation-fenced.
//! No path may acquire folders while these guards are held. Expensive
//! analysis and projection happen before this boundary; the final closure keeps
//! these read-only authority guards through response selection or outbound
//! enqueue, then releases them immediately.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicU32;

use parking_lot::Mutex;
use perl_lsp_rs_core::config::AcceptedCriticSnapshot;
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
    /// Accepted native critic policy the candidate's critic rows were produced
    /// under (#13304), when the payload carries any. `None` means the payload
    /// is policy-independent: a clear, a syntax-only or fast parse-error
    /// publication, or a run that produced no publishable native rows.
    pub(crate) accepted_critic_snapshot: Option<AcceptedCriticSnapshot>,
    /// Workspace topology generation at which the accepted Critic snapshot
    /// was captured. A topology change can leave the same owning root selected
    /// while still invalidating in-flight workspace-scoped rows.
    pub(crate) accepted_topology_generation: Option<u32>,
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
            accepted_critic_snapshot: None,
            accepted_topology_generation: None,
        }
    }

    pub(crate) fn with_folder_config_generation(mut self, generation: Option<u64>) -> Self {
        self.folder_config_generation = generation;
        self
    }

    /// Bind the accepted critic policy the candidate's critic rows were
    /// produced under, so the sink can reject a publication whose policy moved
    /// after analysis (#13304).
    #[must_use]
    pub(crate) fn with_accepted_critic_snapshot(
        mut self,
        snapshot: Option<AcceptedCriticSnapshot>,
    ) -> Self {
        self.accepted_critic_snapshot = snapshot;
        self
    }

    /// Bind the workspace topology generation used by the accepted Critic
    /// snapshot so the sink can reject rows after any folder membership move.
    #[must_use]
    pub(crate) fn with_accepted_topology_generation(mut self, generation: u32) -> Self {
        self.accepted_topology_generation = Some(generation);
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
    /// Document identity and generation are current, but the accepted critic
    /// policy the candidate's rows were produced under is no longer live
    /// configuration (#13304). Publishing would present dead-policy rows as
    /// the current answer.
    RejectedSupersededCriticPolicy,
    /// Validation passed but the outbound transport rejected the frame; the
    /// ledger entry is rolled back so receipt truth reflects the client.
    OutboundFailure,
}

/// Why a staged diagnostic effect could not linearize against its accepted
/// document and Critic subject.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DiagnosticSubjectRejection {
    DocumentClosed,
    WrongDocumentInstance,
    SupersededGeneration,
    SupersededCriticPolicy,
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

    /// Validate one staged diagnostic subject and commit its effect while all
    /// authorities that can move that subject remain locked.
    ///
    /// The closure is the linearization point: document replacement/change,
    /// workspace-folder rebind and accepted Critic configuration movement all
    /// block until it returns. Callers must finish every fallible or expensive
    /// staging step before entering this boundary.
    pub(crate) fn commit_if_diagnostic_subject_current<T>(
        &self,
        uri: &str,
        document_instance: &Arc<AtomicU32>,
        generation: u32,
        workspace_generation: Option<u64>,
        accepted_folder_config_generation: Option<u64>,
        accepted_critic_snapshot: Option<&AcceptedCriticSnapshot>,
        accepted_topology_generation: Option<u32>,
        commit: impl FnOnce() -> T,
    ) -> Result<T, DiagnosticSubjectRejection> {
        let normalized_uri = self.normalize_uri_key(uri);
        // Resolve folder ownership before the identity critical section. The
        // topology generation and unavailable phase fence this sample against
        // a concurrent folder/configuration transaction.
        let folder_config_generation = self.project_config_generation_for_uri(&normalized_uri);
        let sampled_topology =
            self.workspace_topology_generation.load(std::sync::atomic::Ordering::SeqCst);
        let live_root = super::diagnostics::critic_root_for_document(
            &normalized_uri,
            &self.workspace_folders,
            &self.root_path,
            &self.single_file_project_config,
        )
        .map(|path| path.to_string_lossy().into_owned());
        let _identity_guard = self.workspace_identity_lock.lock();
        if !self.workspace_topology_stable.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(DiagnosticSubjectRejection::SupersededCriticPolicy);
        }
        if accepted_critic_snapshot
            .is_some_and(|snapshot| live_root.as_deref() != snapshot.owning_root())
        {
            return Err(DiagnosticSubjectRejection::SupersededCriticPolicy);
        }
        if workspace_generation.is_some_and(|generation| {
            self.workspace_identity_generation.load(std::sync::atomic::Ordering::SeqCst)
                != generation
                || folder_config_generation != accepted_folder_config_generation
        }) {
            return Err(DiagnosticSubjectRejection::SupersededGeneration);
        }
        let live_topology =
            self.workspace_topology_generation.load(std::sync::atomic::Ordering::SeqCst);
        if sampled_topology != live_topology
            || accepted_topology_generation.is_some_and(|accepted| accepted != live_topology)
        {
            return Err(DiagnosticSubjectRejection::SupersededCriticPolicy);
        }
        let documents = self.documents.lock();
        let Some(document) = documents.get(&normalized_uri) else {
            return Err(DiagnosticSubjectRejection::DocumentClosed);
        };
        if !Arc::ptr_eq(&document.generation, document_instance) {
            return Err(DiagnosticSubjectRejection::WrongDocumentInstance);
        }
        if document.current_generation() != generation {
            return Err(DiagnosticSubjectRejection::SupersededGeneration);
        }

        // Hold policy authority through the irreversible effect, rather than
        // carrying a pre-lock boolean across a concurrent configuration write.
        let config = self.config.lock();
        if let Some(snapshot) = accepted_critic_snapshot
            && (live_root.as_deref() != snapshot.owning_root() || !snapshot.is_current(&config))
        {
            return Err(DiagnosticSubjectRejection::SupersededCriticPolicy);
        }
        let committed = commit();
        drop(config);
        drop(documents);
        Ok(committed)
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
        self.commit_push_diagnostics_after_staging(identity, payload, disposition, || {})
    }

    /// Testable staging seam immediately before the shared linearization
    /// boundary. Production supplies a no-op; falsifiers move accepted state
    /// here to prove the final boundary, rather than an earlier precheck, owns
    /// publication authority.
    fn commit_push_diagnostics_after_staging(
        &self,
        identity: &PushDiagnosticIdentity,
        payload: Value,
        disposition: PushDiagnosticsDisposition,
        after_staging: impl FnOnce(),
    ) -> PushDiagnosticsCommitOutcome {
        after_staging();
        let result = self.commit_if_diagnostic_subject_current(
            &identity.normalized_uri,
            &identity.document_instance,
            identity.generation,
            Some(identity.workspace_generation),
            identity.folder_config_generation,
            identity.accepted_critic_snapshot.as_ref(),
            identity.accepted_topology_generation,
            || {
                let mut committed = self.push_diagnostics_sink.committed.lock();
                self.enqueue_committed_push_diagnostic(
                    &mut committed,
                    identity,
                    payload,
                    disposition,
                )
            },
        );
        let rejection = match result {
            Ok(outcome) => return outcome,
            Err(DiagnosticSubjectRejection::DocumentClosed) => {
                PushDiagnosticsCommitOutcome::RejectedDocumentClosed
            }
            Err(DiagnosticSubjectRejection::WrongDocumentInstance) => {
                PushDiagnosticsCommitOutcome::RejectedWrongDocumentInstance
            }
            Err(DiagnosticSubjectRejection::SupersededGeneration) => {
                PushDiagnosticsCommitOutcome::RejectedSupersededGeneration
            }
            Err(DiagnosticSubjectRejection::SupersededCriticPolicy) => {
                PushDiagnosticsCommitOutcome::RejectedSupersededCriticPolicy
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

    /// Ledger compare/record + outbound enqueue. Caller has proven ticket
    /// currency and holds both the sink lock and the shared subject-authority
    /// guards through this irreversible operation.
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
    use perl_lsp_rs_core::config::AcceptedCriticSnapshot;
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
        let folder_generation = server.project_config_generation_for_uri(&key);
        let docs = server.documents.lock();
        let doc = docs.get(&key).expect("document must be open");
        PushDiagnosticIdentity::for_document(
            &key,
            &doc.generation,
            doc.current_generation(),
            server.workspace_identity_generation.load(std::sync::atomic::Ordering::SeqCst),
        )
        .with_folder_config_generation(folder_generation)
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

    /// #13304: document identity and generation cannot observe configuration
    /// movement, so the sink must re-check the accepted critic policy the rows
    /// were produced under. An implementation that validates only the document
    /// ticket publishes dead-policy rows here and turns this red.
    #[test]
    fn critic_policy_movement_after_analysis_is_rejected_without_frame() {
        let (server, buf) = make_server();
        let uri = "file:///sink_critic_policy_test.pl";
        let identity = open_document(
            &server,
            uri,
            "my $x = 1;
",
        );
        assert!(wait_for_frames(&buf, 1), "didOpen publication must flush first");

        // The policy the candidate's rows were produced under.
        let accepted_snapshot = AcceptedCriticSnapshot::capture(&server.config.lock(), None);
        let accepted_fingerprint = accepted_snapshot.fingerprint();
        let bound = identity.clone().with_accepted_critic_snapshot(Some(accepted_snapshot.clone()));

        // Control: while the policy is still live, binding it changes nothing.
        let frames_before = frame_count(&buf);
        assert_eq!(
            server.commit_push_diagnostics(
                &bound,
                json!({ "uri": uri, "version": 1, "diagnostics": [] }),
                PushDiagnosticsDisposition::Replacement,
            ),
            PushDiagnosticsCommitOutcome::CommittedCurrent
        );
        assert!(
            wait_for_frames(&buf, frames_before + 1),
            "a candidate under the live policy must still enqueue exactly one frame"
        );

        // Configuration moves underneath the in-flight candidate. The document
        // is untouched: same instance, same generation, same text.
        server.config.lock().perlcritic_enabled = false;
        let live_fingerprint = { server.config.lock().effective_critic_state(None).fingerprint() };
        assert_ne!(
            live_fingerprint, accepted_fingerprint,
            "the mutation must actually move the accepted policy, or this test proves nothing"
        );

        let settled_frames = frame_count(&buf);
        assert_eq!(
            server.commit_push_diagnostics(
                &bound,
                json!({ "uri": uri, "version": 1, "diagnostics": [] }),
                PushDiagnosticsDisposition::Replacement,
            ),
            PushDiagnosticsCommitOutcome::RejectedSupersededCriticPolicy
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert_eq!(
            frame_count(&buf),
            settled_frames,
            "rows produced under a dead policy must not reach the client"
        );

        // The same document ticket still publishes when no policy is bound, so
        // the rejection above is the policy gate and not a document-ticket
        // regression.
        assert_eq!(
            server.commit_push_diagnostics(
                &identity,
                json!({ "uri": uri, "version": 1, "diagnostics": [] }),
                PushDiagnosticsDisposition::Replacement,
            ),
            PushDiagnosticsCommitOutcome::CommittedCurrent
        );
    }

    #[test]
    fn unstable_workspace_topology_rejects_accepted_subject() {
        let (server, buf) = make_server();
        let uri = "file:///sink_unstable_topology_test.pl";
        let identity = open_document(&server, uri, "my $x = 1;\n");
        assert!(wait_for_frames(&buf, 1), "didOpen publication must flush first");
        let accepted_generation =
            server.workspace_topology_generation.load(std::sync::atomic::Ordering::SeqCst);
        let bound = identity.with_accepted_topology_generation(accepted_generation);

        server.workspace_topology_stable.store(false, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(
            server.commit_push_diagnostics(
                &bound,
                json!({ "uri": uri, "version": 1, "diagnostics": [] }),
                PushDiagnosticsDisposition::Replacement,
            ),
            PushDiagnosticsCommitOutcome::RejectedSupersededCriticPolicy
        );
        server.workspace_topology_stable.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// #13304: two distinct accepted filter shapes can alias under the legacy
    /// 64-bit observation token. The sink must compare the sealed snapshot,
    /// not authorize publication from that token.
    #[test]
    fn critic_policy_legacy_fingerprint_collision_is_rejected_without_frame()
    -> Result<(), Box<dyn std::error::Error>> {
        const SEPARATOR: char = '\u{1f}';
        let (server, buf) = make_server();
        let uri = "file:///sink_critic_collision_test.pl";
        {
            let mut config = server.config.lock();
            config.perlcritic_enabled = true;
            config.native_critic_include = vec![format!("a{SEPARATOR}b")];
        }
        let identity = open_document(&server, uri, "my $x = 1;\n");
        if !wait_for_frames(&buf, 1) {
            return Err("didOpen publication must flush first".into());
        }

        let accepted = AcceptedCriticSnapshot::capture(&server.config.lock(), None);
        let bound = identity.clone().with_accepted_critic_snapshot(Some(accepted.clone()));
        if server.commit_push_diagnostics(
            &bound,
            json!({ "uri": uri, "version": 1, "diagnostics": [] }),
            PushDiagnosticsDisposition::Replacement,
        ) != PushDiagnosticsCommitOutcome::CommittedCurrent
        {
            return Err("the sealed snapshot must commit while its policy is live".into());
        }
        if !wait_for_frames(&buf, 2) {
            return Err("live-policy control publication must flush".into());
        }

        server.config.lock().native_critic_include = vec!["a".to_string(), "b".to_string()];
        let live = AcceptedCriticSnapshot::capture(&server.config.lock(), None);
        if accepted == live {
            return Err("collision fixture must move the accepted policy".into());
        }
        if accepted.fingerprint() != live.fingerprint() {
            return Err("fixture must alias under the legacy 64-bit token".into());
        }
        if accepted.result_identity_fingerprint() == live.result_identity_fingerprint() {
            return Err("canonical accepted-state identities must distinguish the fixture".into());
        }

        let settled_frames = frame_count(&buf);
        if server.commit_push_diagnostics(
            &bound,
            json!({ "uri": uri, "version": 1, "diagnostics": [] }),
            PushDiagnosticsDisposition::Replacement,
        ) != PushDiagnosticsCommitOutcome::RejectedSupersededCriticPolicy
        {
            return Err("a legacy-token collision must not authorize stale rows".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
        if frame_count(&buf) != settled_frames {
            return Err("a colliding stale policy must not enqueue a frame".into());
        }
        Ok(())
    }

    /// #13304: the sink must re-resolve the document's live owning root. Exact
    /// configuration equality at the stored root cannot make a folder rebind
    /// current.
    #[test]
    fn critic_policy_workspace_folder_rebind_is_rejected_without_frame()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let fallback = temp.path().join("fallback");
        let original_owner = temp.path().join("owner");
        let script = original_owner.join("sink_rebind_test.pl");
        std::fs::create_dir_all(&fallback)?;
        std::fs::create_dir_all(&original_owner)?;
        std::fs::write(&script, "my $x = 1;\n")?;

        let uri = url::Url::from_file_path(&script).map_err(|_| "bad document URI")?.to_string();
        let owner_uri = url::Url::from_directory_path(&original_owner)
            .map_err(|_| "bad owner URI")?
            .to_string();
        let (server, buf) = make_server();
        *server.root_path.lock() = Some(fallback);
        server.workspace_folders.lock().push(
            crate::runtime::workspace_folder::WorkspaceFolderState::new(owner_uri)
                .with_path(original_owner.clone()),
        );
        let identity = open_document(&server, &uri, "my $x = 1;\n");
        if !wait_for_frames(&buf, 1) {
            return Err("didOpen publication must flush first".into());
        }

        let owner = original_owner.to_string_lossy().into_owned();
        let accepted = AcceptedCriticSnapshot::capture(&server.config.lock(), Some(&owner));
        let bound = identity.clone().with_accepted_critic_snapshot(Some(accepted.clone()));
        if server.commit_push_diagnostics(
            &bound,
            json!({ "uri": uri, "version": 1, "diagnostics": [] }),
            PushDiagnosticsDisposition::Replacement,
        ) != PushDiagnosticsCommitOutcome::CommittedCurrent
        {
            return Err("the original folder owner must commit while live".into());
        }
        if !wait_for_frames(&buf, 2) {
            return Err("live-owner control publication must flush".into());
        }

        server.workspace_folders.lock().clear();
        if !accepted.is_current(&server.config.lock()) {
            return Err("configuration must remain current at the stored root".into());
        }
        let settled_frames = frame_count(&buf);
        if server.commit_push_diagnostics(
            &bound,
            json!({ "uri": uri, "version": 1, "diagnostics": [] }),
            PushDiagnosticsDisposition::Replacement,
        ) != PushDiagnosticsCommitOutcome::RejectedSupersededCriticPolicy
        {
            return Err("a workspace-folder rebind must stale the push candidate".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
        if frame_count(&buf) != settled_frames {
            return Err("a root-rebound candidate must not enqueue a frame".into());
        }
        Ok(())
    }

    #[test]
    fn generation_movement_after_staging_is_rejected_without_frame()
    -> Result<(), Box<dyn std::error::Error>> {
        let (server, buf) = make_server();
        let uri = "file:///sink_generation_after_staging.pl";
        let identity = open_document(&server, uri, "my $x = 1;\n");
        if !wait_for_frames(&buf, 1) {
            return Err("didOpen publication must flush first".into());
        }
        let settled_frames = frame_count(&buf);
        let live_generation = StdArc::clone(&identity.document_instance);
        let generation = identity.generation;
        let outcome = server.commit_push_diagnostics_after_staging(
            &identity,
            json!({ "uri": uri, "version": 1, "diagnostics": [] }),
            PushDiagnosticsDisposition::Replacement,
            || live_generation.store(generation + 1, Ordering::SeqCst),
        );
        if outcome != PushDiagnosticsCommitOutcome::RejectedSupersededGeneration {
            return Err(
                format!("generation movement must reject at final commit: {outcome:?}").into()
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
        if frame_count(&buf) != settled_frames {
            return Err("a generation moved after staging must not enqueue a frame".into());
        }
        Ok(())
    }

    #[test]
    fn policy_movement_after_staging_is_rejected_without_frame()
    -> Result<(), Box<dyn std::error::Error>> {
        let (server, buf) = make_server();
        let uri = "file:///sink_policy_after_staging.pl";
        let identity = open_document(&server, uri, "my $x = 1;\n");
        if !wait_for_frames(&buf, 1) {
            return Err("didOpen publication must flush first".into());
        }
        let accepted = AcceptedCriticSnapshot::capture(&server.config.lock(), None);
        let bound = identity.with_accepted_critic_snapshot(Some(accepted));
        let settled_frames = frame_count(&buf);
        let outcome = server.commit_push_diagnostics_after_staging(
            &bound,
            json!({ "uri": uri, "version": 1, "diagnostics": [] }),
            PushDiagnosticsDisposition::Replacement,
            || {
                server.config.lock().native_critic_exclude =
                    vec!["native.testing.require_use_strict".to_string()];
            },
        );
        if outcome != PushDiagnosticsCommitOutcome::RejectedSupersededCriticPolicy {
            return Err(format!("policy movement must reject at final commit: {outcome:?}").into());
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
        if frame_count(&buf) != settled_frames {
            return Err("a policy moved after staging must not enqueue a frame".into());
        }
        Ok(())
    }

    #[test]
    fn root_rebind_after_staging_is_rejected_without_frame()
    -> Result<(), Box<dyn std::error::Error>> {
        let (server, buf) = make_server();
        let uri = "file:///workspace-a/sink_root_after_staging.pl";
        server.test_set_workspace_folder_uris(&["file:///workspace-a/"]);
        let identity = open_document(&server, uri, "my $x = 1;\n");
        if !wait_for_frames(&buf, 1) {
            return Err("didOpen publication must flush first".into());
        }
        let accepted = server.capture_accepted_critic(uri);
        let bound = identity.with_accepted_critic_snapshot(Some(accepted));
        let settled_frames = frame_count(&buf);
        let outcome = server.commit_push_diagnostics_after_staging(
            &bound,
            json!({ "uri": uri, "version": 1, "diagnostics": [] }),
            PushDiagnosticsDisposition::Replacement,
            || server.test_set_workspace_folder_uris(&["file:///workspace-b/"]),
        );
        if outcome != PushDiagnosticsCommitOutcome::RejectedSupersededCriticPolicy {
            return Err(format!("root rebind must reject at final commit: {outcome:?}").into());
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
        if frame_count(&buf) != settled_frames {
            return Err("a root rebound after staging must not enqueue a frame".into());
        }
        Ok(())
    }

    #[test]
    fn topology_movement_after_staging_is_rejected_without_frame()
    -> Result<(), Box<dyn std::error::Error>> {
        let (server, buf) = make_server();
        let uri = "file:///workspace-a/sink_topology_after_staging.pl";
        server.test_set_workspace_folder_uris(&["file:///workspace-a/"]);
        let identity = open_document(&server, uri, "my $x = 1;\n");
        if !wait_for_frames(&buf, 1) {
            return Err("didOpen publication must flush first".into());
        }
        let accepted = server.capture_accepted_critic(uri);
        let accepted_topology_generation =
            server.workspace_topology_generation.load(Ordering::SeqCst);
        let bound = identity
            .with_accepted_critic_snapshot(Some(accepted))
            .with_accepted_topology_generation(accepted_topology_generation);
        let settled_frames = frame_count(&buf);
        let outcome = server.commit_push_diagnostics_after_staging(
            &bound,
            json!({ "uri": uri, "version": 1, "diagnostics": [] }),
            PushDiagnosticsDisposition::Replacement,
            || {
                server.workspace_topology_generation.fetch_add(1, Ordering::SeqCst);
            },
        );
        if outcome != PushDiagnosticsCommitOutcome::RejectedSupersededCriticPolicy {
            return Err(
                format!("topology movement must reject at final commit: {outcome:?}").into()
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
        if frame_count(&buf) != settled_frames {
            return Err("a topology move after staging must not enqueue a frame".into());
        }
        Ok(())
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
    fn workspace_folder_change_rejects_pre_change_candidate() {
        let (server, _buf) = make_server();
        let identity = open_document(&server, "file:///sink_folder_change_test.pl", "my $x = 1;\n");

        server
            .handle_did_change_workspace_folders(Some(json!({
                "event": {
                    "added": [{ "uri": "file:///sink-folder-root/", "name": "root" }],
                    "removed": []
                }
            })))
            .expect("workspace folder change should succeed");

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
}
