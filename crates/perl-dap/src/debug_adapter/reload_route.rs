//! Debug-adapter wiring for the `perl-lsp/loadedModuleReload` custom DAP
//! family (reload train R03, #10102).
//!
//! This module binds the negotiated wire family (R01B, #10138) and the
//! session reconciliation engine (`crate::reload::reconciliation`) into
//! the debug session lifecycle:
//!
//! - the family request name is **not a standard command**: it stays
//!   absent from `SUPPORTED_COMMANDS`, and its only route exists under the
//!   exact preview/test profile that R04's advertiser will own. Outside
//!   that profile — every production construction today — the request
//!   receives the adapter's ordinary unknown-command response, so the
//!   family is unavailable without being generally advertised;
//! - an admitted request routes through the session wiring: the terminal
//!   outcome advances or preserves the runtime-module generation on the
//!   debug session (ADR-0046 §4), invalidates exactly the composed table
//!   (frames, arguments, variables, evaluate results, exception facts),
//!   marks the affected source's desired breakpoints pending/unverified
//!   while preserving them, and only then publishes the response — the
//!   client can never observe `reloaded` while old affected handles are
//!   still accepted as current;
//! - standard events carry the invalidation (`invalidated` with the exact
//!   areas actually invalidated, `breakpoint` changed events for the
//!   affected pending breakpoints). No `loadedSource` event is emitted
//!   without an observation: refresh without the #10098 mechanism
//!   read-back is reported `unavailable`, never asserted as a change.
//!
//! The runtime transaction itself is #10098's remainder and is not
//! implemented here: version 1 sessions are unbacked (admitted-shape
//! requests receive the typed `family_not_backed_for_session` rejection),
//! and only the preview/test profile can supply mechanism backing and
//! terminal outcomes for the routing and reconciliation proof.

use super::{DapMessage, DebugAdapter, Value, lock_or_recover};
use crate::reload::LoadedModuleReloadEligibility;
use crate::reload::LoadedModuleReloadOutcome;
use crate::reload::ReloadSessionWiring;
use crate::reload::RuntimeModuleGenerationClock;
use crate::reload::{PreMutationFailureCause, ReloadTransactionPhase};
#[cfg(any(test, feature = "test-helpers"))]
use crate::reload_family::{
    ClientFamilyDeclaration, LOADED_MODULE_RELOAD_FAMILY, LoadedModuleReloadOutcomeBody,
    LoadedModuleReloadResponseBody, LoadedModuleReloadWireResponse,
};
use crate::reload_family::{LOADED_MODULE_RELOAD_REQUEST, ReloadRequestEvaluation};
use std::collections::BTreeMap;

/// Adapter-level route state for the reload family.
pub(super) struct ReloadRouteState {
    /// The exact preview/test profile gate. Never enabled by a production
    /// construction; outside the profile the family request is unavailable.
    preview_profile: bool,
    /// Whether the profile claims reload mechanism backing (test profile
    /// only; the #10098 mechanism does not exist yet).
    backed: bool,
    /// Session epoch, replaced on debuggee replacement together with the
    /// family wiring: prior family and operation identities never survive.
    epoch: u64,
    /// Session-scoped family wiring (negotiation, admission, terminals).
    wiring: Option<ReloadSessionWiring>,
    /// Adapter-issued opaque subject identity to affected source path.
    /// Populated by the preview/test profile; the live loaded-source
    /// observation owner (#9585/#10098) replaces this when it lands.
    subject_sources: BTreeMap<String, String>,
    /// The terminal outcome the runtime transaction would deliver for the
    /// next admitted operation; preview/test profile only.
    seeded_outcome: Option<LoadedModuleReloadOutcome>,
}

impl Default for ReloadRouteState {
    fn default() -> Self {
        ReloadRouteState {
            preview_profile: false,
            backed: false,
            epoch: 1,
            wiring: None,
            subject_sources: BTreeMap::new(),
            seeded_outcome: None,
        }
    }
}

impl ReloadRouteState {
    fn ensure_wiring(&mut self) -> &mut ReloadSessionWiring {
        self.wiring.get_or_insert_with(|| ReloadSessionWiring::new(self.epoch, self.backed))
    }
}

impl DebugAdapter {
    /// Enable the exact preview/test profile for the loaded-module reload
    /// family route (R03, #10102).
    ///
    /// This is the routing gate R04's advertiser will own: production
    /// constructions never call it, the family stays unadvertised (no
    /// capability key mentions it), and without the profile the family
    /// request receives the adapter's ordinary unknown-command response.
    /// `backed` states whether the profile claims reload mechanism
    /// backing; version 1 production sessions are always unbacked.
    pub fn enable_loaded_module_reload_preview_profile(&mut self, backed: bool) {
        let mut route = lock_or_recover(&self.reload_route, "debug_adapter.reload_route");
        route.preview_profile = true;
        route.backed = backed;
        route.subject_sources.clear();
        route.seeded_outcome = None;
        // A fresh profile constructs fresh session wiring at the current
        // epoch; prior negotiation and operation identities do not carry.
        route.wiring = Some(ReloadSessionWiring::new(route.epoch, backed));
    }

    /// Negotiate a client family declaration on the reload route exactly
    /// as a conforming client's declaration would (fail-closed for absent
    /// declarations, wrong family identities, and no overlapping version).
    /// Test-profile helper: the client-facing declaration channel is the
    /// R04 advertiser's surface.
    #[cfg(any(test, feature = "test-helpers"))]
    pub fn declare_loaded_module_reload_client_for_test(
        &self,
        versions: &[u32],
    ) -> Result<u32, String> {
        let mut route = lock_or_recover(&self.reload_route, "debug_adapter.reload_route");
        let declaration = ClientFamilyDeclaration {
            family: LOADED_MODULE_RELOAD_FAMILY.to_string(),
            versions: versions.to_vec(),
        };
        route
            .ensure_wiring()
            .negotiate(Some(&declaration))
            .map_err(|refusal| refusal.wire_code().as_str().to_string())
    }

    /// Bind an adapter-issued opaque subject identity to its affected
    /// source path for the preview/test profile. The live subject
    /// issuance surface (#9585/#10098) replaces this when it lands.
    #[cfg(any(test, feature = "test-helpers"))]
    pub fn seed_loaded_module_reload_subject_for_test(
        &self,
        module_identity: &str,
        source_path: &str,
    ) {
        let mut route = lock_or_recover(&self.reload_route, "debug_adapter.reload_route");
        route.subject_sources.insert(module_identity.to_string(), source_path.to_string());
    }

    /// Supply the terminal outcome the runtime transaction (#10098, not
    /// yet landed) would deliver for the next admitted operation.
    /// Preview/test profile only.
    #[cfg(any(test, feature = "test-helpers"))]
    pub fn seed_loaded_module_reload_outcome_for_test(&self, outcome: LoadedModuleReloadOutcome) {
        let mut route = lock_or_recover(&self.reload_route, "debug_adapter.reload_route");
        route.seeded_outcome = Some(outcome);
    }

    /// The current reload-route session epoch (test observability).
    #[cfg(any(test, feature = "test-helpers"))]
    pub fn loaded_module_reload_epoch_for_test(&self) -> u64 {
        let route = lock_or_recover(&self.reload_route, "debug_adapter.reload_route");
        route.epoch
    }

    /// Replace the reload wiring on debuggee replacement: a new epoch
    /// invalidates prior family and operation identities, and the
    /// runtime-module generation resets with the new debuggee process
    /// (it lives on `DebugSession`).
    pub(super) fn reset_reload_route_for_replacement_session(&self) {
        let mut route = lock_or_recover(&self.reload_route, "debug_adapter.reload_route");
        if route.wiring.is_none() {
            return;
        }
        route.epoch = route.epoch.saturating_add(1);
        route.subject_sources.clear();
        route.seeded_outcome = None;
        route.wiring = Some(ReloadSessionWiring::new(route.epoch, route.backed));
    }

    /// Whether the family request has a route in this adapter: only under
    /// the exact preview/test profile.
    pub(super) fn loaded_module_reload_route_enabled(&self) -> bool {
        let route = lock_or_recover(&self.reload_route, "debug_adapter.reload_route");
        route.preview_profile
    }

    /// Handle one `perl-lsp/loadedModuleReload` request under the preview
    /// profile.
    ///
    /// Sequencing (issue #10102): fail-closed admission with no backend
    /// action → terminal routing on the session's generation clock →
    /// composed invalidation of session-owned inspection state → affected
    /// desired breakpoints marked pending → standard `invalidated`/
    /// `breakpoint` events → response. A post-mutation reconciliation
    /// limit never rewrites the causal outcome: it is carried by the
    /// reconciliation dispositions on the response.
    pub(super) fn handle_loaded_module_reload(
        &self,
        seq: i64,
        request_seq: i64,
        arguments: Option<Value>,
    ) -> DapMessage {
        let raw = arguments.unwrap_or(Value::Null);
        let mut route = lock_or_recover(&self.reload_route, "debug_adapter.reload_route");
        if !route.preview_profile {
            // Unreachable through dispatch (the route is gated there too);
            // fail closed to the ordinary unknown-command shape rather
            // than trusting two gates to one invariant.
            return Self::unknown_command_loaded_module_reload(seq, request_seq);
        }

        // Fail-closed wire admission (R01B): every gate runs before any
        // backend action; a rejection publishes the typed code and never
        // touches the debug session.
        let admitted = match route.ensure_wiring().evaluate(&raw) {
            ReloadRequestEvaluation::Admitted { operation_id } => operation_id,
            ReloadRequestEvaluation::Response(rejection) => {
                return DapMessage::Response {
                    seq,
                    request_seq,
                    success: false,
                    command: LOADED_MODULE_RELOAD_REQUEST.to_string(),
                    body: serde_json::to_value(&rejection).ok(),
                    message: None,
                };
            }
        };

        // The terminal outcome source: the #10098 mechanism when it lands;
        // without one the honest terminal for an admitted operation on an
        // unbacked runtime is the frozen `unsupported_runtime` refusal —
        // availability is never product authority.
        let seeded = route.seeded_outcome.take();
        let subject_source = raw
            .get("subject")
            .and_then(|subject| subject.get("moduleIdentity"))
            .and_then(Value::as_str)
            .and_then(|identity| route.subject_sources.get(identity).cloned());

        // Terminal routing on the debug session's generation clock. Lock
        // order is reload_route → session everywhere this route runs.
        let mut session_guard = lock_or_recover(&self.session, "debug_adapter.reload_route");
        let mut scratch_clock = RuntimeModuleGenerationClock::new();
        let has_session = session_guard.is_some();
        // Command-readiness gate (review finding): a held session is only
        // an admission fact when the debuggee is actually suspended. A
        // running or terminated session owns no consistent inspection
        // snapshot to invalidate and cannot accept a mutation handshake,
        // so mutating terminals refuse `not_stopped_or_not_command_ready`
        // exactly like the absent-session case.
        let session_ready = has_session
            && session_guard.as_ref().is_some_and(|session| {
                matches!(session.state, crate::debug_adapter::session::DebugState::Stopped)
            });
        let clock: &mut RuntimeModuleGenerationClock = match session_guard.as_mut() {
            Some(session) => &mut session.module_generation,
            None => &mut scratch_clock,
        };

        let mut reasons: Vec<String> = Vec::new();
        let outcome = match seeded {
            // A mutating outcome for a subject this adapter never bound to
            // a source cannot be reconciled (which desired breakpoints
            // would become pending is unknowable), so it never publishes:
            // the honest terminal is the frozen inexact/stale-identity
            // refusal — the composition fails closed rather than routing a
            // reload whose affected source it cannot name (review finding).
            Some(outcome)
                if session_ready
                    && crate::reload::outcome_is_mutating(&outcome)
                    && subject_source.is_none() =>
            {
                LoadedModuleReloadOutcome::Refused {
                    disposition: LoadedModuleReloadEligibility::SourceNotExactOrStale,
                }
            }
            Some(outcome) if session_ready => {
                if crate::reload::outcome_is_mutating(&outcome) && clock.current().is_exhausted() {
                    // Bounded exhaustion (#10097): fail closed as a
                    // deterministic pre-mutation prepare failure rather
                    // than risking a reused generation.
                    reasons.push("generation_exhausted".to_string());
                    LoadedModuleReloadOutcome::FailedBeforeMutation {
                        phase: ReloadTransactionPhase::Prepare,
                        cause: PreMutationFailureCause::PrepareFailed,
                    }
                } else {
                    outcome
                }
            }
            // A mutating outcome cannot be routed without a debuggee
            // process: the honest terminal is the frozen
            // not-stopped/command-ready refusal, never a published reload.
            Some(
                LoadedModuleReloadOutcome::Reloaded
                | LoadedModuleReloadOutcome::IndeterminatePossiblyApplied { .. },
            ) => LoadedModuleReloadOutcome::Refused {
                disposition: LoadedModuleReloadEligibility::NotStoppedOrNotCommandReady,
            },
            Some(other) => other,
            None => LoadedModuleReloadOutcome::Refused {
                disposition: LoadedModuleReloadEligibility::UnsupportedRuntime,
            },
        };

        let routed = route.ensure_wiring().route_terminal(admitted, &outcome, clock, &reasons);
        let routed = match routed {
            Ok(routed) => routed,
            Err(_refusal) => {
                // Post-admission wiring refusals cannot occur through the
                // dispatch path (evaluate already rejected replays and
                // unknown identities); fail closed to the ordinary
                // unavailable response rather than inventing a wire shape.
                drop(session_guard);
                return Self::unknown_command_loaded_module_reload(seq, request_seq);
            }
        };

        // Composed invalidation of session-owned state — exactly the
        // always-stale and stale-when-advanced kinds. Thread references
        // are adapter projections (re-projected, nothing to clear) and
        // durable desired breakpoint configuration is preserved (the
        // affected source is marked pending below, never dropped).
        if routed.mutated
            && let Some(session) = session_guard.as_mut()
        {
            session.stack_frames.clear();
            session.stack_frame_arguments.clear();
            session.variable_cache.clear();
            // Suspension authority (review finding): frame ids mint their
            // wire identity from `stopped_generation`
            // (`debug_adapter/process.rs`), so clearing the table alone
            // cannot retire a pre-reload frame handle — the rebuilt
            // suspension would re-mint the numerically identical id and
            // old handles would pass membership checks against the new
            // table. Advancing the clock with the composed invalidation
            // makes every generation-bound pre-reload identity
            // authority-dead (`reload/invalidation.rs` staleness), keeping
            // this route's frozen promise that no client observes
            // `reloaded` while old affected handles are still current.
            // Refusals and pre-mutation failures move nothing, matching
            // the module-clock `GenerationEffect` doctrine.
            session.stopped_generation = session.stopped_generation.saturating_add(1);
            *lock_or_recover(&self.last_exception_message, "debug_adapter.last_exception") = None;
        }
        drop(session_guard);
        drop(route);

        // Affected desired breakpoints become pending/unverified; every
        // affected record emits a generation-bound `breakpoint` changed
        // event. Unrelated sources are untouched.
        let mut pending_breakpoints = Vec::new();
        if routed.mutated
            && let Some(source_path) = subject_source
        {
            pending_breakpoints =
                self.breakpoints.mark_breakpoints_pending_reconciliation(&source_path);
        }

        // Standard events before the response: the client observes the
        // invalidation and the pending breakpoints before the terminal
        // result can imply current reload success. No `loadedSource`
        // event without an observation — refresh is reported
        // `unavailable` on the response, never asserted as a change.
        if routed.mutated {
            self.send_event(
                "invalidated",
                Some(serde_json::json!({
                    "areas": routed.invalidated_areas(),
                })),
            );
            for record in &pending_breakpoints {
                self.send_event(
                    "breakpoint",
                    Some(serde_json::json!({
                        "reason": "changed",
                        "breakpoint": {
                            "id": record.id,
                            "verified": record.verified,
                            "line": record.line,
                            "column": record.column,
                            "message": record.message,
                        },
                    })),
                );
            }
        }

        DapMessage::Response {
            seq,
            request_seq,
            success: routed.response.success,
            command: LOADED_MODULE_RELOAD_REQUEST.to_string(),
            body: serde_json::to_value(&routed.response).ok(),
            message: None,
        }
    }

    /// The ordinary unavailable shape for the family request: identical to
    /// the adapter's unknown-command failure, used when the profile gate
    /// is closed.
    fn unknown_command_loaded_module_reload(seq: i64, request_seq: i64) -> DapMessage {
        DapMessage::Response {
            seq,
            request_seq,
            success: false,
            command: LOADED_MODULE_RELOAD_REQUEST.to_string(),
            body: None,
            message: Some(format!("Unknown command: {LOADED_MODULE_RELOAD_REQUEST}")),
        }
    }
}

/// The outcome body of a wire response, if it is one (test observability).
#[cfg(any(test, feature = "test-helpers"))]
pub(super) fn loaded_module_reload_outcome_body(
    wire: &LoadedModuleReloadWireResponse,
) -> Option<&LoadedModuleReloadOutcomeBody> {
    match &wire.body {
        LoadedModuleReloadResponseBody::Outcome(outcome) => Some(outcome),
        LoadedModuleReloadResponseBody::Rejected(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debug_adapter::session::{DebugSession, DebugState, ResumeMode};
    use crate::debug_adapter::variable_cache::{VariableCache, VariableCacheKind};
    use crate::protocol::{SetBreakpointsArguments, Source, SourceBreakpoint};
    use crate::reload::IndeterminateCause;
    use crate::reload::LoadedModuleReloadEligibility;
    use crate::types::StackFrame;
    use perl_tdd_support::must;
    use std::collections::HashMap;
    use std::process::{Child, Command, Stdio};
    use std::sync::mpsc::sync_channel;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    const MODULE_IDENTITY: &str = "opaque-module-token-r03";

    fn noop_child() -> Child {
        if let Ok(child) = Command::new("perl")
            .arg("-e")
            .arg("1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            return child;
        }
        #[cfg(windows)]
        let (program, args): (&str, &[&str]) = ("cmd", &["/c", "exit", "0"]);
        #[cfg(not(windows))]
        let (program, args): (&str, &[&str]) = ("true", &[]);
        must(
            Command::new(program)
                .args(args)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn(),
        )
    }

    /// Desired-breakpoint sources for the affected/unrelated cells, in a
    /// unique temp directory so concurrent runs never collide.
    struct SeededSources {
        affected: std::path::PathBuf,
        unrelated: std::path::PathBuf,
    }

    impl SeededSources {
        fn new() -> Self {
            // Unique even under parallel test processes and coarse clocks:
            // process id plus a per-process monotonic counter alongside the
            // timestamp (review finding: timestamp-only names can collide).
            static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let unique = format!(
                "{}-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|elapsed| elapsed.as_nanos())
                    .unwrap_or_default()
            );
            let dir = std::env::temp_dir().join(format!("plsw-r03-route-{unique}"));
            must(std::fs::create_dir_all(&dir));
            let body: String = (0..8).map(|index| format!("my $v{index} = {index};\n")).collect();
            let affected = dir.join("affected_module.pl");
            let unrelated = dir.join("unrelated_module.pl");
            must(std::fs::write(&affected, &body));
            must(std::fs::write(&unrelated, &body));
            SeededSources { affected, unrelated }
        }

        fn path_string(&self, affected: bool) -> String {
            let path = if affected { &self.affected } else { &self.unrelated };
            path.to_string_lossy().into_owned()
        }
    }

    impl Drop for SeededSources {
        fn drop(&mut self) {
            if let Some(parent) = self.affected.parent() {
                let _ = std::fs::remove_dir_all(parent);
            }
        }
    }

    fn frame(id: i32, path: &str) -> StackFrame {
        StackFrame::new(
            id,
            "main::frame",
            crate::types::Source { name: None, path: path.to_string(), source_reference: None },
            4,
        )
    }

    /// Seed a stopped session owning frames, frame arguments, a cached
    /// evaluate result, and a retained exception message — exactly the
    /// session state the composed invalidation table covers.
    fn seed_stopped_session(adapter: &DebugAdapter, sources: &SeededSources) {
        let mut guard = lock_or_recover(&adapter.session, "reload_route.test.session");
        let mut session = DebugSession {
            process: noop_child(),
            state: DebugState::Stopped,
            stack_frames: vec![
                frame(1, &sources.path_string(true)),
                frame(2, &sources.path_string(false)),
            ],
            stack_frame_arguments: HashMap::from([(1, vec!["$x".to_string()])]),
            variable_cache: VariableCache::default(),
            thread_id: 1,
            last_resume_mode: ResumeMode::Unknown,
            stopped_generation: 3,
            module_generation: RuntimeModuleGenerationClock::new(),
        };
        session.variable_cache.upsert(9001, VariableCacheKind::EvaluateResult, Vec::new());
        *guard = Some(session);
        *lock_or_recover(&adapter.last_exception_message, "reload_route.test.exception") =
            Some("old-code exception fact".to_string());
    }

    fn session_state_snapshot(adapter: &DebugAdapter) -> (usize, bool, Option<String>) {
        let guard = lock_or_recover(&adapter.session, "reload_route.test.snapshot");
        let exception = lock_or_recover(
            &adapter.last_exception_message,
            "reload_route.test.snapshot.exception",
        )
        .clone();
        match guard.as_ref() {
            Some(session) => (
                session.stack_frames.len(),
                session.variable_cache.root_count(9001).is_some(),
                exception,
            ),
            None => (0, false, exception),
        }
    }

    /// The session's stopped-suspension frame-authority generation, if a
    /// session exists (test observability for the invalidation witness).
    fn stopped_generation_snapshot(adapter: &DebugAdapter) -> Option<u64> {
        lock_or_recover(&adapter.session, "reload_route.test.generation")
            .as_ref()
            .map(|session| session.stopped_generation)
    }

    /// Re-seed the held session's execution state (running/stopped).
    fn force_session_state(adapter: &DebugAdapter, state: DebugState) {
        if let Some(session) = lock_or_recover(&adapter.session, "reload_route.test.force").as_mut()
        {
            session.state = state;
        }
    }

    fn request(operation_id: u64, epoch: u64) -> Value {
        serde_json::json!({
            "family": LOADED_MODULE_RELOAD_FAMILY,
            "familyVersion": 1,
            "sessionEpoch": epoch,
            "operationId": operation_id,
            "subject": {
                "moduleIdentity": MODULE_IDENTITY,
                "savedSourceDigest": "sha256:0f12e4d6a9b8c7d5e3f1a0b9c8d7e6f5a4b3c2d1e0f9a8b7c6d5e4f3a2b1c0d",
                "logicalSourceUri": "perl-lsp-subject:epoch=1;observation=3",
                "observationGeneration": 3
            },
            "deadlineMs": 5000
        })
    }

    fn rejection_code(response: &DapMessage) -> String {
        let DapMessage::Response { body: Some(body), .. } = response else {
            return "no-body".to_string();
        };
        body.get("body")
            .and_then(|body| body.get("code"))
            .and_then(Value::as_str)
            .unwrap_or("no-code")
            .to_string()
    }

    fn set_one_breakpoint(adapter: &DebugAdapter, path: &str, line: i64) {
        let breakpoints = adapter.breakpoints.set_breakpoints(&SetBreakpointsArguments {
            source: Source { name: None, path: Some(path.to_string()) },
            breakpoints: Some(vec![SourceBreakpoint {
                line,
                column: None,
                condition: None,
                hit_condition: None,
                log_message: None,
            }]),
            source_modified: None,
        });
        assert!(
            breakpoints.first().is_some_and(|breakpoint| breakpoint.verified),
            "the seeded breakpoint on {path} must verify against real Perl source"
        );
    }

    #[test]
    fn family_request_is_unavailable_outside_the_preview_profile() -> TestResult {
        let mut adapter = DebugAdapter::new();
        let response = adapter.handle_request(2, LOADED_MODULE_RELOAD_REQUEST, Some(request(1, 1)));
        let DapMessage::Response { success, message, .. } = response else {
            return Err("expected a response".into());
        };
        assert!(!success);
        assert!(
            message.as_deref().unwrap_or_default().contains("Unknown command"),
            "outside the profile the family request is unavailable: {message:?}"
        );
        // Ordinary standard DAP remains usable without the family.
        let initialize = adapter.handle_request(3, "initialize", None);
        let DapMessage::Response { success: initialize_success, .. } = initialize else {
            return Err("initialize must answer".into());
        };
        assert!(initialize_success, "initialize stays usable");
        Ok(())
    }

    #[test]
    fn unnegotiated_profile_request_reaches_no_backend_action() -> TestResult {
        let sources = SeededSources::new();
        let mut adapter = DebugAdapter::new();
        adapter.enable_loaded_module_reload_preview_profile(true);
        seed_stopped_session(&adapter, &sources);
        let queries_before = adapter.debugger_query_count_for_test();

        let response = adapter.handle_request(2, LOADED_MODULE_RELOAD_REQUEST, Some(request(1, 1)));
        assert!(
            !matches!(response, DapMessage::Response { success: true, .. }),
            "an unnegotiated request never succeeds"
        );
        assert_eq!(rejection_code(&response), "family_not_negotiated");

        // No backend action and no invalidation: the debugger wrote
        // nothing and every stale-able observation is intact.
        assert_eq!(adapter.debugger_query_count_for_test(), queries_before);
        let (frames, has_cache, exception) = session_state_snapshot(&adapter);
        assert_eq!(frames, 2, "an unnegotiated request must not invalidate frames");
        assert!(has_cache, "an unnegotiated request must not invalidate variables");
        assert_eq!(exception.as_deref(), Some("old-code exception fact"));
        Ok(())
    }

    #[test]
    fn unbacked_admitted_shape_request_is_a_typed_rejection_not_a_terminal() -> TestResult {
        let sources = SeededSources::new();
        let mut adapter = DebugAdapter::new();
        adapter.enable_loaded_module_reload_preview_profile(false);
        adapter.declare_loaded_module_reload_client_for_test(&[1])?;
        seed_stopped_session(&adapter, &sources);
        let response = adapter.handle_request(2, LOADED_MODULE_RELOAD_REQUEST, Some(request(1, 1)));
        assert_eq!(rejection_code(&response), "family_not_backed_for_session");
        let (frames, _, _) = session_state_snapshot(&adapter);
        assert_eq!(frames, 2, "a typed rejection never invalidates session state");
        Ok(())
    }

    #[test]
    fn reloaded_routes_exact_invalidation_events_and_reconciliation() -> TestResult {
        let sources = SeededSources::new();
        let mut adapter = DebugAdapter::new();
        let (event_sender, receiver) = sync_channel(64);
        adapter.set_event_sender(event_sender);
        adapter.enable_loaded_module_reload_preview_profile(true);
        adapter.declare_loaded_module_reload_client_for_test(&[1])?;
        adapter.seed_loaded_module_reload_subject_for_test(
            MODULE_IDENTITY,
            &sources.path_string(true),
        );
        adapter.seed_loaded_module_reload_outcome_for_test(LoadedModuleReloadOutcome::Reloaded);
        seed_stopped_session(&adapter, &sources);
        // Desired breakpoints: affected source (verified) and an unrelated
        // source (verified) — durable configuration for both.
        set_one_breakpoint(&adapter, &sources.path_string(true), 2);
        set_one_breakpoint(&adapter, &sources.path_string(false), 3);
        let affected_before = adapter.breakpoints.get_breakpoints(&sources.path_string(true));
        let unrelated_before = adapter.breakpoints.get_breakpoints(&sources.path_string(false));

        let response = adapter.handle_request(2, LOADED_MODULE_RELOAD_REQUEST, Some(request(1, 1)));
        let DapMessage::Response { success, body: Some(body), .. } = &response else {
            return Err("expected a response body".into());
        };
        assert!(success, "reloaded is the only clean terminal success");
        let wire: LoadedModuleReloadWireResponse = serde_json::from_value(body.clone())?;
        let outcome = loaded_module_reload_outcome_body(&wire).ok_or("expected an outcome body")?;
        assert_eq!(outcome.kind.as_str(), "reloaded");
        assert!(!outcome.possibly_applied);
        let witness = outcome.generation.ok_or("witness required")?;
        assert!(witness.advanced && witness.previous + 1 == witness.current);
        assert_eq!(
            outcome.reconciliation,
            crate::reload::reconciliation_dispositions_for(&LoadedModuleReloadOutcome::Reloaded),
            "the R03 fill of the reconciliation surface"
        );

        // Exactly the composed table applied: frames, arguments,
        // variables, and exception facts stale; unrelated breakpoints
        // current; the affected source's desired breakpoints preserved
        // but explicitly pending.
        let (frames, has_cache, exception) = session_state_snapshot(&adapter);
        assert_eq!(frames, 0, "old frames cannot survive the new generation");
        assert!(!has_cache, "evaluate results cannot survive the new generation");
        assert_eq!(exception, None, "old exception facts are stale");
        let affected_after = adapter.breakpoints.get_breakpoints(&sources.path_string(true));
        assert_eq!(
            affected_after.len(),
            affected_before.len(),
            "desired configuration is preserved, never dropped"
        );
        assert!(
            affected_after.iter().all(|record| !record.verified
                && record.message.as_deref() == Some("Pending reconciliation after module reload")),
            "affected desired breakpoints are explicitly pending: {affected_after:?}"
        );
        let unrelated_after = adapter.breakpoints.get_breakpoints(&sources.path_string(false));
        let project = |records: &[crate::breakpoints::BreakpointRecord]| {
            records
                .iter()
                .map(|record| (record.id, record.line, record.verified, record.message.clone()))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            project(&unrelated_after),
            project(&unrelated_before),
            "unrelated source breakpoints remain installed and current"
        );

        // Standard events, in order: invalidated with the exact areas,
        // then one breakpoint changed event per affected pending record.
        let mut seen = Vec::new();
        while let Ok(message) = receiver.try_recv() {
            if let DapMessage::Event { event, body, .. } = message {
                seen.push((event, body));
            }
        }
        let areas = seen
            .iter()
            .find(|(event, _)| event == "invalidated")
            .and_then(|(_, body)| body.as_ref())
            .and_then(|body| body.get("areas"))
            .and_then(Value::as_array)
            .ok_or("an invalidated event with areas must be emitted")?;
        assert_eq!(
            areas.iter().filter_map(Value::as_str).collect::<Vec<_>>(),
            crate::reload::MUTATION_INVALIDATED_AREAS[..]
        );
        let changed = seen.iter().filter(|(event, _)| event == "breakpoint").count();
        assert_eq!(changed, affected_after.len(), "one changed event per affected breakpoint");
        let invalidated_at = seen.iter().position(|(event, _)| event == "invalidated");
        let first_breakpoint_at = seen.iter().position(|(event, _)| event == "breakpoint");
        assert!(
            invalidated_at.is_some_and(|at| first_breakpoint_at.is_some_and(|other| at < other)),
            "invalidated precedes the breakpoint events"
        );

        // A replayed operation is a typed stale rejection, never a second
        // terminal.
        let replay = adapter.handle_request(3, LOADED_MODULE_RELOAD_REQUEST, Some(request(1, 1)));
        assert_eq!(rejection_code(&replay), "operation_stale");
        Ok(())
    }

    #[test]
    fn indeterminate_routes_with_the_same_conservative_invalidation() -> TestResult {
        let sources = SeededSources::new();
        let mut adapter = DebugAdapter::new();
        let (event_sender, receiver) = sync_channel(64);
        adapter.set_event_sender(event_sender);
        adapter.enable_loaded_module_reload_preview_profile(true);
        adapter.declare_loaded_module_reload_client_for_test(&[1])?;
        adapter.seed_loaded_module_reload_subject_for_test(
            MODULE_IDENTITY,
            &sources.path_string(true),
        );
        adapter.seed_loaded_module_reload_outcome_for_test(
            LoadedModuleReloadOutcome::IndeterminatePossiblyApplied {
                phase: ReloadTransactionPhase::RuntimeAcknowledgementReadBack,
                cause: IndeterminateCause::TimeoutAfterMutationBegan,
            },
        );
        seed_stopped_session(&adapter, &sources);

        let response = adapter.handle_request(2, LOADED_MODULE_RELOAD_REQUEST, Some(request(1, 1)));
        let DapMessage::Response { success, body: Some(body), .. } = &response else {
            return Err("expected a response body".into());
        };
        assert!(!success, "an indeterminate outcome is never DAP success");
        let wire: LoadedModuleReloadWireResponse = serde_json::from_value(body.clone())?;
        let outcome = loaded_module_reload_outcome_body(&wire).ok_or("expected an outcome body")?;
        assert_eq!(outcome.kind.as_str(), "indeterminate_possibly_applied");
        assert!(outcome.possibly_applied);
        assert!(outcome.generation.ok_or("witness required")?.advanced);
        assert_eq!(
            outcome.reconciliation.inspection_invalidation,
            crate::reload_family::WireReconciliationDisposition::Invalidated,
            "the conservative invalidation applies to indeterminate outcomes identically"
        );
        let (frames, _, _) = session_state_snapshot(&adapter);
        assert_eq!(frames, 0, "old frames cannot survive a possibly applied reload");
        let invalidated = receiver.try_recv().ok().and_then(|message| match message {
            DapMessage::Event { event, .. } => Some(event),
            _ => None,
        });
        assert_eq!(invalidated.as_deref(), Some("invalidated"));
        Ok(())
    }

    #[test]
    fn refusal_routes_without_invalidation_or_events() -> TestResult {
        let sources = SeededSources::new();
        let mut adapter = DebugAdapter::new();
        let (event_sender, receiver) = sync_channel(64);
        adapter.set_event_sender(event_sender);
        adapter.enable_loaded_module_reload_preview_profile(true);
        adapter.declare_loaded_module_reload_client_for_test(&[1])?;
        adapter.seed_loaded_module_reload_outcome_for_test(LoadedModuleReloadOutcome::Refused {
            disposition: LoadedModuleReloadEligibility::OutsideLaunchAuthority,
        });
        seed_stopped_session(&adapter, &sources);

        let response = adapter.handle_request(2, LOADED_MODULE_RELOAD_REQUEST, Some(request(1, 1)));
        let DapMessage::Response { success, body: Some(body), .. } = &response else {
            return Err("expected a response body".into());
        };
        assert!(!success);
        let wire: LoadedModuleReloadWireResponse = serde_json::from_value(body.clone())?;
        let outcome = loaded_module_reload_outcome_body(&wire).ok_or("expected an outcome body")?;
        assert_eq!(outcome.kind.as_str(), "refused");
        let witness = outcome.generation.ok_or("witness required")?;
        assert!(!witness.advanced && witness.previous == witness.current);
        assert_eq!(
            outcome.reconciliation,
            crate::reload::reconciliation_dispositions_for(&LoadedModuleReloadOutcome::Refused {
                disposition: LoadedModuleReloadEligibility::OutsideLaunchAuthority,
            }),
            "a refusal reconciles nothing"
        );
        // A pre-mutation refusal preserves old frame/value authority.
        let (frames, has_cache, exception) = session_state_snapshot(&adapter);
        assert_eq!(frames, 2);
        assert!(has_cache);
        assert_eq!(exception.as_deref(), Some("old-code exception fact"));
        assert!(receiver.try_recv().is_err(), "a refusal emits no events");
        Ok(())
    }

    #[test]
    fn session_replacement_invalidates_prior_family_identities() -> TestResult {
        let mut adapter = DebugAdapter::new();
        adapter.enable_loaded_module_reload_preview_profile(true);
        adapter.declare_loaded_module_reload_client_for_test(&[1])?;
        let old_epoch = adapter.loaded_module_reload_epoch_for_test();

        // A replacement session (the launch/restart path) bumps the epoch
        // and reconstructs the wiring; prior negotiation dies with it
        // (gate precedence: negotiated presence before session epoch).
        adapter.reset_reload_route_for_replacement_session();
        let new_epoch = adapter.loaded_module_reload_epoch_for_test();
        assert_eq!(new_epoch, old_epoch + 1);
        let unnegotiated =
            adapter.handle_request(2, LOADED_MODULE_RELOAD_REQUEST, Some(request(1, old_epoch)));
        assert_eq!(
            rejection_code(&unnegotiated),
            "family_not_negotiated",
            "prior negotiation never survives the replacement"
        );

        // A session negotiated under the new epoch refuses old-epoch
        // requests with the typed stale-session code.
        adapter.declare_loaded_module_reload_client_for_test(&[1])?;
        let stale =
            adapter.handle_request(3, LOADED_MODULE_RELOAD_REQUEST, Some(request(1, old_epoch)));
        assert_eq!(rejection_code(&stale), "session_stale");
        Ok(())
    }

    #[test]
    fn mutating_outcome_without_a_session_refuses_not_stopped() -> TestResult {
        let mut adapter = DebugAdapter::new();
        adapter.enable_loaded_module_reload_preview_profile(true);
        adapter.declare_loaded_module_reload_client_for_test(&[1])?;
        adapter.seed_loaded_module_reload_outcome_for_test(LoadedModuleReloadOutcome::Reloaded);
        // No debuggee session: a published reload is impossible; the
        // honest terminal is the frozen not-stopped refusal.
        let response = adapter.handle_request(2, LOADED_MODULE_RELOAD_REQUEST, Some(request(1, 1)));
        let DapMessage::Response { success, body: Some(body), .. } = &response else {
            return Err("expected a response body".into());
        };
        assert!(!success);
        let wire: LoadedModuleReloadWireResponse = serde_json::from_value(body.clone())?;
        let outcome = loaded_module_reload_outcome_body(&wire).ok_or("expected an outcome body")?;
        assert_eq!(outcome.kind.as_str(), "refused");
        assert_eq!(
            serde_json::to_value(outcome.disposition)?,
            serde_json::json!("not_stopped_or_not_command_ready")
        );
        assert!(!outcome.generation.ok_or("witness required")?.advanced);
        Ok(())
    }

    #[test]
    fn mutating_outcome_for_an_unbound_subject_refuses_inexact_identity() -> TestResult {
        let sources = SeededSources::new();
        let mut adapter = DebugAdapter::new();
        let (event_sender, receiver) = sync_channel(64);
        adapter.set_event_sender(event_sender);
        adapter.enable_loaded_module_reload_preview_profile(true);
        adapter.declare_loaded_module_reload_client_for_test(&[1])?;
        // No subject binding is registered: the adapter never issued this
        // subject's source identity, so a mutating terminal can never
        // publish — the affected source to reconcile is unknowable.
        adapter.seed_loaded_module_reload_outcome_for_test(
            LoadedModuleReloadOutcome::IndeterminatePossiblyApplied {
                phase: ReloadTransactionPhase::RuntimeMutationBegins,
                cause: IndeterminateCause::TransportLossAfterMutationBegan,
            },
        );
        seed_stopped_session(&adapter, &sources);

        let response = adapter.handle_request(2, LOADED_MODULE_RELOAD_REQUEST, Some(request(1, 1)));
        let DapMessage::Response { success, body: Some(body), .. } = &response else {
            return Err("expected a response body".into());
        };
        assert!(!success);
        let wire: LoadedModuleReloadWireResponse = serde_json::from_value(body.clone())?;
        let outcome = loaded_module_reload_outcome_body(&wire).ok_or("expected an outcome body")?;
        assert_eq!(outcome.kind.as_str(), "refused");
        assert_eq!(
            serde_json::to_value(outcome.disposition)?,
            serde_json::json!("source_not_exact_or_stale")
        );
        assert!(!outcome.generation.ok_or("witness required")?.advanced);
        // Nothing was invalidated and nothing was emitted.
        let (frames, _, _) = session_state_snapshot(&adapter);
        assert_eq!(frames, 2, "an unbound subject never triggers invalidation");
        assert!(receiver.try_recv().is_err());
        Ok(())
    }

    /// Review finding (admission gating): a session that exists but is not
    /// stopped is not command-ready; a seeded mutating outcome must refuse
    /// with the frozen disposition instead of routing a reload into a
    /// running debuggee.
    #[test]
    fn mutating_outcome_against_a_running_session_refuses_not_stopped() -> TestResult {
        let sources = SeededSources::new();
        let mut adapter = DebugAdapter::new();
        let (event_sender, receiver) = sync_channel(64);
        adapter.set_event_sender(event_sender);
        adapter.enable_loaded_module_reload_preview_profile(true);
        adapter.declare_loaded_module_reload_client_for_test(&[1])?;
        adapter.seed_loaded_module_reload_subject_for_test(
            MODULE_IDENTITY,
            &sources.path_string(true),
        );
        adapter.seed_loaded_module_reload_outcome_for_test(LoadedModuleReloadOutcome::Reloaded);
        seed_stopped_session(&adapter, &sources);
        force_session_state(&adapter, DebugState::Running);

        let response = adapter.handle_request(2, LOADED_MODULE_RELOAD_REQUEST, Some(request(1, 1)));
        let DapMessage::Response { success, body: Some(body), .. } = &response else {
            return Err("expected a response body".into());
        };
        assert!(!success);
        let wire: LoadedModuleReloadWireResponse = serde_json::from_value(body.clone())?;
        let outcome = loaded_module_reload_outcome_body(&wire).ok_or("expected an outcome body")?;
        assert_eq!(outcome.kind.as_str(), "refused");
        assert_eq!(
            serde_json::to_value(outcome.disposition)?,
            serde_json::json!("not_stopped_or_not_command_ready"),
            "a non-stopped session is not command-ready"
        );
        assert!(!outcome.generation.ok_or("witness required")?.advanced);

        // No invalidation, no events: the refusal is purely pre-mutation.
        let (frames, has_cache, exception) = session_state_snapshot(&adapter);
        assert_eq!(frames, 2, "a not-ready refusal never invalidates frames");
        assert!(has_cache);
        assert_eq!(exception.as_deref(), Some("old-code exception fact"));
        assert!(receiver.try_recv().is_err(), "a not-ready refusal emits no events");
        Ok(())
    }

    /// Review finding (frame authority): composed invalidation must
    /// advance `stopped_generation` so pre-reload generation-derived
    /// frame identities can never be valid against the rebuilt table.
    #[test]
    fn mutating_route_advances_stopped_frame_authority() -> TestResult {
        let sources = SeededSources::new();
        let mut adapter = DebugAdapter::new();
        adapter.enable_loaded_module_reload_preview_profile(true);
        adapter.declare_loaded_module_reload_client_for_test(&[1])?;
        adapter.seed_loaded_module_reload_subject_for_test(
            MODULE_IDENTITY,
            &sources.path_string(true),
        );
        adapter.seed_loaded_module_reload_outcome_for_test(LoadedModuleReloadOutcome::Reloaded);
        seed_stopped_session(&adapter, &sources);
        let generation_before =
            stopped_generation_snapshot(&adapter).ok_or("session must exist")?;

        let response = adapter.handle_request(2, LOADED_MODULE_RELOAD_REQUEST, Some(request(1, 1)));
        let DapMessage::Response { success: true, .. } = &response else {
            return Err("the reloaded route must succeed for this witness".into());
        };

        // The suspension authority moved: any client-held frame identity
        // minted from the previous generation is authority-dead, and the
        // cleared table cannot revive it by numeric coincidence.
        let generation_after = stopped_generation_snapshot(&adapter).ok_or("session must die?")?;
        assert!(
            generation_after > generation_before,
            "composed invalidation must advance stopped_generation \
             (before {generation_before}, after {generation_after})"
        );
        Ok(())
    }

    /// A refusal routes without touching frame authority either: only a
    /// terminal that mutated the debuggee advances the suspension clock.
    #[test]
    fn refusing_route_preserves_stopped_frame_authority() -> TestResult {
        let sources = SeededSources::new();
        let mut adapter = DebugAdapter::new();
        adapter.enable_loaded_module_reload_preview_profile(true);
        adapter.declare_loaded_module_reload_client_for_test(&[1])?;
        adapter.seed_loaded_module_reload_subject_for_test(
            MODULE_IDENTITY,
            &sources.path_string(true),
        );
        adapter.seed_loaded_module_reload_outcome_for_test(LoadedModuleReloadOutcome::Refused {
            disposition: LoadedModuleReloadEligibility::OutsideLaunchAuthority,
        });
        seed_stopped_session(&adapter, &sources);
        let generation_before =
            stopped_generation_snapshot(&adapter).ok_or("session must exist")?;

        let response = adapter.handle_request(2, LOADED_MODULE_RELOAD_REQUEST, Some(request(1, 1)));
        let DapMessage::Response { success: false, .. } = &response else {
            return Err("a refused outcome is not success".into());
        };
        assert_eq!(
            stopped_generation_snapshot(&adapter),
            Some(generation_before),
            "a pre-mutation refusal preserves the suspension authority"
        );
        Ok(())
    }
}
