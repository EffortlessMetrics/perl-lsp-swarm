//! The bounded loaded-module reload transaction executor (reload train R02,
//! #10098).
//!
//! R01 (#10097) froze *what* a reload transaction means; this module
//! executes one. It owns exactly three things and nothing else:
//!
//! 1. the **channel seam** ([`ReloadRuntimeChannel`]) through which a
//!    transaction reaches a live debuggee, split so that the call which
//!    crosses the possibly-applied boundary is a *different method* from
//!    the read-only ones;
//! 2. the **command builder** ([`plan_commands`]), which derives every
//!    debugger command from the bound subject under a strict allowlist so
//!    no raw path, debugger command, or Perl expression can reach the
//!    runtime;
//! 3. the **state machine** ([`execute_reload`]), which walks the frozen
//!    phases and classifies exactly one terminal
//!    [`LoadedModuleReloadOutcome`].
//!
//! # The one law this module exists to enforce
//!
//! `query_inc_entries` (`debug_adapter/output.rs`) maps a framed-query
//! timeout to an empty list. That is correct for a read-only query and
//! catastrophic for a mutation: the transport cannot distinguish "the
//! command never ran" from "the command ran and the answer was lost".
//!
//! So the seam refuses to let a caller make that mistake:
//! [`ChannelSettlement::NotIssued`] means the bytes never reached the
//! debuggee, and [`ChannelSettlement::Unsettled`] means they may have. The
//! executor turns the second into
//! [`LoadedModuleReloadOutcome::IndeterminatePossiblyApplied`] with a
//! generation advance, *always*. The invariant
//! [`ReloadExecution::mutation_issued`] ⟺ generation advanced is asserted
//! exhaustively in this module's tests.
//!
//! # Reachability
//!
//! Nothing here is routed from a DAP request. The capability projection
//! stays [`super::ReloadCapabilityProjection::Unadvertised`] until R04
//! (#10104) proves the transaction through the public binary, and the
//! adapter-side wiring belongs to R03 (#10102). This module is the
//! executor those leaves consume; it advertises nothing on its own.
//!
//! # What this module does not claim
//!
//! Executing a reload does **not** migrate existing blessed instances,
//! closures, captured lexicals, or already-resolved methods, and does not
//! remove symbols the old source defined. Those limits are the frozen
//! [`super::PERL_RUNTIME_LIMITATIONS`]; a `Reloaded` outcome here means
//! exactly "the module source was re-executed under bounded semantics and
//! the runtime read back the refreshed registration" — never more.

use super::eligibility::LoadedModuleReloadEligibility;
use super::generation::{GenerationAdvance, RuntimeModuleGenerationClock};
use super::mechanism::ReloadMechanism;
use super::subject::{LoadedModuleSubject, SubjectCurrentnessView};
use super::transaction::{
    IndeterminateCause, LoadedModuleReloadOutcome, LoadedModuleReloadPlan, PreMutationFailureCause,
    ReloadTransactionPhase,
};

/// Marker prefix for the read-only preflight observation.
const PREFLIGHT_MARKER: &str = "PERLLSP_RELOAD_PREFLIGHT";
/// Marker prefix for the mutation acknowledgement.
const MUTATION_MARKER: &str = "PERLLSP_RELOAD_MUTATION";
/// Marker prefix for the post-mutation read-back observation.
const READBACK_MARKER: &str = "PERLLSP_RELOAD_READBACK";

/// Why a framed exchange did not produce an answer.
///
/// Every variant means the same thing to a *mutation*: the runtime may
/// have been changed. The distinction exists for diagnosis and for the
/// terminal cause code, never to let one of them be treated as clean.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnsettledKind {
    /// The framed answer did not arrive before the deadline.
    Timeout,
    /// The transport closed before the framed answer arrived.
    TransportLoss,
    /// The operation was cancelled while in flight.
    Cancelled,
}

impl UnsettledKind {
    /// All unsettled kinds in closed order.
    pub const ALL: [UnsettledKind; 3] =
        [UnsettledKind::Timeout, UnsettledKind::TransportLoss, UnsettledKind::Cancelled];

    /// Stable diagnostic code.
    pub const fn as_str(self) -> &'static str {
        match self {
            UnsettledKind::Timeout => "timeout",
            UnsettledKind::TransportLoss => "transport_loss",
            UnsettledKind::Cancelled => "cancelled",
        }
    }

    /// The post-boundary indeterminate cause this kind maps to.
    ///
    /// A cancellation after the boundary cannot prove non-application, so
    /// it is an ambiguous acknowledgement rather than a clean cancel.
    pub const fn post_boundary_cause(self) -> IndeterminateCause {
        match self {
            UnsettledKind::Timeout => IndeterminateCause::TimeoutAfterMutationBegan,
            UnsettledKind::TransportLoss => IndeterminateCause::TransportLossAfterMutationBegan,
            UnsettledKind::Cancelled => IndeterminateCause::AmbiguousAcknowledgement,
        }
    }
}

/// The result of one framed exchange with the debuggee.
///
/// The split between [`ChannelSettlement::NotIssued`] and
/// [`ChannelSettlement::Unsettled`] is the whole safety property of this
/// seam: an implementation that cannot tell the two apart must report
/// `Unsettled`, which is the conservative answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelSettlement {
    /// The commands ran and these framed output lines came back.
    Acknowledged(Vec<String>),
    /// The commands were never written to the debuggee: no session, no
    /// stdin, or a write error before any byte reached it. Nothing ran.
    NotIssued(String),
    /// The commands may have run, but no framed answer settled.
    Unsettled(UnsettledKind),
}

/// The transport a reload transaction drives.
///
/// Implementations own framing, deadlines, and cancellation. They must not
/// collapse a lost answer into an empty success: returning
/// `Acknowledged(vec![])` for a timed-out mutation is exactly the bug this
/// seam exists to make unrepresentable.
pub trait ReloadRuntimeChannel {
    /// The live currentness view for the revalidation immediately before
    /// mutation. `None` when the debuggee is not stopped and command-ready.
    fn currentness_view(&mut self) -> Option<SubjectCurrentnessView>;

    /// Run read-only observation commands.
    fn run_readonly(&mut self, commands: &[String]) -> ChannelSettlement;

    /// Run the one mutation command set.
    ///
    /// Returning anything other than [`ChannelSettlement::NotIssued`]
    /// asserts that the bytes reached the debuggee, which crosses the
    /// possibly-applied boundary irrevocably.
    fn run_mutation(&mut self, commands: &[String]) -> ChannelSettlement;
}

/// Why a subject cannot be turned into executable debugger commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandPlanError {
    /// The runtime `%INC` key is not a plain relative Perl module key.
    ///
    /// The key is the only subject field interpolated into debugger
    /// command text, so it carries a strict allowlist. Anything else —
    /// quotes, parentheses, semicolons, whitespace, newlines, `..`
    /// traversal, an absolute path, or a non-`.pm` suffix — is refused
    /// before any command is built.
    UnsafeModuleKey,
    /// The mechanism has no executable implementation in this cohort.
    MechanismNotExecutable,
}

impl CommandPlanError {
    /// All command-plan errors in closed order.
    pub const ALL: [CommandPlanError; 2] =
        [CommandPlanError::UnsafeModuleKey, CommandPlanError::MechanismNotExecutable];

    /// Stable diagnostic code.
    pub const fn as_str(self) -> &'static str {
        match self {
            CommandPlanError::UnsafeModuleKey => "unsafe_module_key",
            CommandPlanError::MechanismNotExecutable => "mechanism_not_executable",
        }
    }

    /// The refusal disposition this error projects to.
    ///
    /// An unusable key is an inexact identity; an unimplemented mechanism
    /// is an unsupported runtime family. Neither ever admits the subject.
    pub const fn refusal(self) -> LoadedModuleReloadEligibility {
        match self {
            CommandPlanError::UnsafeModuleKey => {
                LoadedModuleReloadEligibility::SourceNotExactOrStale
            }
            CommandPlanError::MechanismNotExecutable => {
                LoadedModuleReloadEligibility::UnsupportedRuntime
            }
        }
    }
}

/// Whether a mechanism has an executable implementation in this cohort.
///
/// Only [`ReloadMechanism::IncDeletionAndRequire`] does. The `do`/require
/// helper needs package handling this cohort has not earned, the
/// workspace helper needs its own injection authority and lifecycle, and
/// Class::Refresh is a measured compatibility subject that never becomes
/// product authority by being installed. Each of those refuses rather
/// than silently degrading to the `%INC` path.
pub const fn mechanism_is_executable(mechanism: ReloadMechanism) -> bool {
    matches!(mechanism, ReloadMechanism::IncDeletionAndRequire)
}

/// Whether a runtime `%INC` key is safe to interpolate into command text.
///
/// Accepts only `Segment(/Segment)*.pm` where a segment is one or more of
/// `[A-Za-z0-9_-]` plus internal `.`. Rejects absolute paths, empty
/// segments, `..` traversal, and every quoting or statement-separating
/// character.
fn module_key_is_safe(inc_key: &str) -> bool {
    if inc_key.is_empty() || inc_key.len() > 255 || !inc_key.ends_with(".pm") {
        return false;
    }
    let mut segments = 0_usize;
    for segment in inc_key.split('/') {
        segments += 1;
        if segment.is_empty() || segment == ".." || segment == "." {
            return false;
        }
        if !segment.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.') {
            return false;
        }
    }
    segments > 0
}

/// The three command sets one transaction issues, derived from the subject.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReloadCommandPlan {
    /// Read-only preflight: is the subject still registered, and where?
    pub preflight: Vec<String>,
    /// The single mutation command set. Issuing it crosses the boundary.
    pub mutation: Vec<String>,
    /// Read-only post-mutation read-back of the registration.
    pub read_back: Vec<String>,
}

/// Build the command plan for one bound subject and mechanism.
///
/// Every command is derived from the bound subject; nothing is taken from
/// a caller-supplied string. The `%INC` key is the only interpolated
/// field and passes [`module_key_is_safe`] first.
///
/// The preflight deliberately does **not** compile the replacement source.
/// Compiling Perl runs `BEGIN` blocks, so an in-debuggee "syntax check"
/// would itself be a runtime mutation and would blur the possibly-applied
/// boundary this contract exists to keep sharp. Preflight therefore
/// establishes only what is provably side-effect-free: that the subject is
/// still registered in `%INC` at the resolved path the subject was bound
/// to. Compile-preflight, if it is ever wanted, needs its own out-of-band
/// authority and is not part of this cohort.
pub fn plan_commands(
    subject: &LoadedModuleSubject,
    mechanism: ReloadMechanism,
) -> Result<ReloadCommandPlan, CommandPlanError> {
    if !mechanism_is_executable(mechanism) {
        return Err(CommandPlanError::MechanismNotExecutable);
    }
    let key = subject.inc_key();
    if !module_key_is_safe(key) {
        return Err(CommandPlanError::UnsafeModuleKey);
    }
    // `q(...)` quoting is safe because the allowlist above excludes every
    // parenthesis, quote, backslash, and statement separator.
    let observe = |marker: &str| {
        format!(
            "p \"{marker} \" . (exists $INC{{q({key})}} ? q(present) : q(absent)) \
             . \" \" . (defined $INC{{q({key})}} ? $INC{{q({key})}} : q(-))"
        )
    };
    Ok(ReloadCommandPlan {
        preflight: vec![observe(PREFLIGHT_MARKER)],
        mutation: vec![format!(
            "p do {{ delete $INC{{q({key})}}; \
             my $ok = eval {{ require q({key}); 1 }} ? 1 : 0; \
             \"{MUTATION_MARKER} $ok\" }}"
        )],
        read_back: vec![observe(READBACK_MARKER)],
    })
}

/// A parsed `%INC` registration observation.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RegistrationObservation {
    present: bool,
    path: String,
}

/// Parse the registration marker out of framed output lines.
///
/// Returns `None` when the marker is absent: an answer that does not carry
/// the marker is not an observation, and is never read as "absent".
fn parse_registration(lines: &[String], marker: &str) -> Option<RegistrationObservation> {
    for line in lines {
        let Some(index) = line.find(marker) else {
            continue;
        };
        let rest = line.get(index + marker.len()..).unwrap_or("");
        let mut fields = rest.split_whitespace();
        let state = fields.next()?;
        let path = fields.next().unwrap_or("-");
        let present = match state {
            "present" => true,
            "absent" => false,
            _ => continue,
        };
        return Some(RegistrationObservation { present, path: path.to_string() });
    }
    None
}

/// Parse the mutation acknowledgement flag out of framed output lines.
///
/// `Some(true)` means the debuggee reported the `require` succeeded;
/// `Some(false)` means it reported failure; `None` means no marker came
/// back at all, which after the boundary is an ambiguous acknowledgement,
/// never a success and never a clean failure.
fn parse_mutation_ack(lines: &[String]) -> Option<bool> {
    for line in lines {
        let Some(index) = line.find(MUTATION_MARKER) else {
            continue;
        };
        let rest = line.get(index + MUTATION_MARKER.len()..).unwrap_or("");
        match rest.split_whitespace().next() {
            Some("1") => return Some(true),
            Some("0") => return Some(false),
            _ => continue,
        }
    }
    None
}

/// The outcome of one executed reload transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReloadExecution {
    /// The terminal outcome in the frozen R01 vocabulary.
    pub outcome: LoadedModuleReloadOutcome,
    /// The phase the transaction reached.
    pub phase_reached: ReloadTransactionPhase,
    /// Whether the mutation bytes reached the debuggee.
    ///
    /// This is the possibly-applied boundary as an observable fact: it is
    /// true exactly when the runtime-module generation advanced.
    pub mutation_issued: bool,
    /// The mechanism the transaction executed under.
    pub mechanism: ReloadMechanism,
    /// What the generation clock did.
    pub generation: GenerationAdvance,
}

impl ReloadExecution {
    /// Whether the outcome may be projected to a client as clean.
    pub fn projects_as_clean(&self) -> bool {
        self.outcome.projects_as_clean()
    }
}

/// Assemble an execution result, applying the outcome to the clock.
fn settle(
    outcome: LoadedModuleReloadOutcome,
    phase_reached: ReloadTransactionPhase,
    mutation_issued: bool,
    mechanism: ReloadMechanism,
    clock: &mut RuntimeModuleGenerationClock,
) -> ReloadExecution {
    let generation = clock.apply(&outcome);
    ReloadExecution { outcome, phase_reached, mutation_issued, mechanism, generation }
}

/// Execute one bounded loaded-module reload transaction.
///
/// The plan must already be admitted by [`super::plan_reload`]; this
/// function revalidates currentness immediately before mutation and then
/// walks the frozen phases:
///
/// ```text
/// preflight  revalidate identity, then observe registration (read-only)
/// prepare    build the command plan
/// mutate     issue exactly one mutation  ← possibly-applied boundary
/// read back  observe the registration again
/// commit     advance the runtime-module generation
/// ```
///
/// Once the mutation is issued, every path returns either `Reloaded` or
/// `IndeterminatePossiblyApplied`; there is no route back to a clean
/// pre-mutation failure, because there is no evidence that could earn one.
pub fn execute_reload<C: ReloadRuntimeChannel + ?Sized>(
    plan: &LoadedModuleReloadPlan,
    mechanism: ReloadMechanism,
    channel: &mut C,
    clock: &mut RuntimeModuleGenerationClock,
) -> ReloadExecution {
    let subject = plan.subject();

    // Admission: the command plan is derivable at all. An unsafe key or an
    // unimplemented mechanism refuses here, before the debuggee is touched.
    let commands = match plan_commands(subject, mechanism) {
        Ok(commands) => commands,
        Err(error) => {
            return settle(
                LoadedModuleReloadOutcome::Refused { disposition: error.refusal() },
                ReloadTransactionPhase::Admission,
                false,
                mechanism,
                clock,
            );
        }
    };

    // Preflight: revalidate the exact identity against the live view. A
    // stale plan is refused here rather than mutating whatever now
    // occupies the subject's key.
    let refuse_preflight = |disposition, clock: &mut RuntimeModuleGenerationClock| {
        settle(
            LoadedModuleReloadOutcome::Refused { disposition },
            ReloadTransactionPhase::Preflight,
            false,
            mechanism,
            clock,
        )
    };
    let Some(view) = channel.currentness_view() else {
        return refuse_preflight(LoadedModuleReloadEligibility::NotStoppedOrNotCommandReady, clock);
    };
    if !subject.is_current_against(&view)
        || subject.session_generation() != plan.admitted_session_generation()
        || subject.suspension_generation() != plan.admitted_suspension_generation()
    {
        return refuse_preflight(LoadedModuleReloadEligibility::SourceNotExactOrStale, clock);
    }

    // Preflight observation: still registered, at the bound path?
    match channel.run_readonly(&commands.preflight) {
        ChannelSettlement::Acknowledged(lines) => {
            match parse_registration(&lines, PREFLIGHT_MARKER) {
                Some(observation) if !observation.present => {
                    return refuse_preflight(LoadedModuleReloadEligibility::NotLoaded, clock);
                }
                Some(observation) if observation.path != subject.resolved_runtime_path() => {
                    // The key now resolves somewhere else: the runtime
                    // mapping no longer binds exactly one subject.
                    return refuse_preflight(
                        LoadedModuleReloadEligibility::AmbiguousRuntimeMapping,
                        clock,
                    );
                }
                Some(_) => {}
                None => {
                    // No marker came back. Nothing was mutated, so this is
                    // an ordinary pre-mutation failure.
                    return settle(
                        LoadedModuleReloadOutcome::FailedBeforeMutation {
                            phase: ReloadTransactionPhase::Preflight,
                            cause: PreMutationFailureCause::PrepareFailed,
                        },
                        ReloadTransactionPhase::Preflight,
                        false,
                        mechanism,
                        clock,
                    );
                }
            }
        }
        ChannelSettlement::NotIssued(_)
        | ChannelSettlement::Unsettled(UnsettledKind::Timeout)
        | ChannelSettlement::Unsettled(UnsettledKind::TransportLoss) => {
            return settle(
                LoadedModuleReloadOutcome::FailedBeforeMutation {
                    phase: ReloadTransactionPhase::Preflight,
                    cause: PreMutationFailureCause::PrepareFailed,
                },
                ReloadTransactionPhase::Preflight,
                false,
                mechanism,
                clock,
            );
        }
        ChannelSettlement::Unsettled(UnsettledKind::Cancelled) => {
            return settle(
                LoadedModuleReloadOutcome::FailedBeforeMutation {
                    phase: ReloadTransactionPhase::Preflight,
                    cause: PreMutationFailureCause::CancelledBeforeMutationBegan,
                },
                ReloadTransactionPhase::Preflight,
                false,
                mechanism,
                clock,
            );
        }
    }

    // The boundary. Everything after this point is possibly applied.
    let mutation = channel.run_mutation(&commands.mutation);
    let ack = match mutation {
        ChannelSettlement::NotIssued(_) => {
            // The bytes never reached the debuggee: still pre-mutation.
            return settle(
                LoadedModuleReloadOutcome::FailedBeforeMutation {
                    phase: ReloadTransactionPhase::Prepare,
                    cause: PreMutationFailureCause::PrepareFailed,
                },
                ReloadTransactionPhase::Prepare,
                false,
                mechanism,
                clock,
            );
        }
        ChannelSettlement::Unsettled(kind) => {
            return settle(
                LoadedModuleReloadOutcome::IndeterminatePossiblyApplied {
                    phase: ReloadTransactionPhase::RuntimeMutationBegins,
                    cause: kind.post_boundary_cause(),
                },
                ReloadTransactionPhase::RuntimeMutationBegins,
                true,
                mechanism,
                clock,
            );
        }
        ChannelSettlement::Acknowledged(lines) => parse_mutation_ack(&lines),
    };

    // Read-back runs whatever the acknowledgement said: a failed `require`
    // still deleted the `%INC` entry, so the registration is the only
    // evidence that can distinguish a completed reload from a partial one.
    let indeterminate = |cause, clock: &mut RuntimeModuleGenerationClock| {
        settle(
            LoadedModuleReloadOutcome::IndeterminatePossiblyApplied {
                phase: ReloadTransactionPhase::RuntimeAcknowledgementReadBack,
                cause,
            },
            ReloadTransactionPhase::RuntimeAcknowledgementReadBack,
            true,
            mechanism,
            clock,
        )
    };
    let read_back = match channel.run_readonly(&commands.read_back) {
        ChannelSettlement::Acknowledged(lines) => lines,
        ChannelSettlement::NotIssued(_) => {
            return indeterminate(IndeterminateCause::ReadBackInconclusive, clock);
        }
        ChannelSettlement::Unsettled(kind) => {
            return indeterminate(kind.post_boundary_cause(), clock);
        }
    };
    let Some(observation) = parse_registration(&read_back, READBACK_MARKER) else {
        return indeterminate(IndeterminateCause::ReadBackInconclusive, clock);
    };

    // The only route to `Reloaded`: the debuggee acknowledged the require
    // succeeded *and* read back a refreshed registration at the bound
    // path. A prompt, an empty frame, or a missing flag never gets here.
    let reloaded = ack == Some(true)
        && observation.present
        && observation.path == subject.resolved_runtime_path();
    if reloaded {
        settle(
            LoadedModuleReloadOutcome::Reloaded,
            ReloadTransactionPhase::CommitGeneration,
            true,
            mechanism,
            clock,
        )
    } else {
        indeterminate(IndeterminateCause::ReadBackInconclusive, clock)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reload::subject::{ModuleClassification, SubjectCandidate};
    use crate::reload::transaction::{phase_permits_outcome, plan_reload};
    use crate::reload::{GenerationEffect, ReloadAdmissionObservation};

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    const KEY: &str = "App/Core.pm";
    const PATH: &str = "/ws/lib/App/Core.pm";

    fn candidate() -> SubjectCandidate {
        SubjectCandidate {
            session_generation: Some(4),
            suspension_generation: Some(12),
            observation_generation: Some(3),
            inc_key: KEY.to_string(),
            resolved_runtime_path: PATH.to_string(),
            saved_content_digest: "sha256:9f2c".to_string(),
            logical_source_uri: "file:///ws/lib/App/Core.pm".to_string(),
            perl_identity: "perl 5.42.0".to_string(),
            launch_root: "/ws".to_string(),
            module_classification: Some(ModuleClassification::SourceBackedPerlModule),
            operation_identity: 9,
        }
    }

    fn admitted_observation() -> ReloadAdmissionObservation {
        ReloadAdmissionObservation {
            stopped_and_command_ready: true,
            runtime_supported: true,
            loaded_in_runtime: true,
            within_launch_authority: true,
            runtime_mapping_unambiguous: true,
            identity_binding_complete: true,
            identity_current: true,
            client_source_matches_saved: true,
            module_classification: ModuleClassification::SourceBackedPerlModule,
            active_frame_in_target: false,
        }
    }

    fn current_view() -> SubjectCurrentnessView {
        SubjectCurrentnessView {
            session_generation: 4,
            suspension_generation: 12,
            observation_generation: 3,
            saved_content_digest: "sha256:9f2c".to_string(),
            perl_identity: "perl 5.42.0".to_string(),
        }
    }

    fn admitted_plan() -> Result<LoadedModuleReloadPlan, Box<dyn std::error::Error>> {
        let subject = candidate().bind().map_err(|_| "candidate must bind")?;
        plan_reload(&subject, &admitted_observation()).map_err(|_| "plan must admit".into())
    }

    /// A scripted channel: each exchange returns the next queued settlement.
    struct ScriptedChannel {
        view: Option<SubjectCurrentnessView>,
        readonly: Vec<ChannelSettlement>,
        mutation: ChannelSettlement,
        issued_mutations: Vec<Vec<String>>,
        readonly_calls: usize,
    }

    impl ScriptedChannel {
        fn new(readonly: Vec<ChannelSettlement>, mutation: ChannelSettlement) -> ScriptedChannel {
            ScriptedChannel {
                view: Some(current_view()),
                readonly,
                mutation,
                issued_mutations: Vec::new(),
                readonly_calls: 0,
            }
        }

        fn ok(marker: &str, present: bool, path: &str) -> ChannelSettlement {
            let state = if present { "present" } else { "absent" };
            ChannelSettlement::Acknowledged(vec![format!("{marker} {state} {path}")])
        }

        /// The happy path: preflight present, mutation ok, read-back present.
        fn happy() -> ScriptedChannel {
            ScriptedChannel::new(
                vec![
                    ScriptedChannel::ok(PREFLIGHT_MARKER, true, PATH),
                    ScriptedChannel::ok(READBACK_MARKER, true, PATH),
                ],
                ChannelSettlement::Acknowledged(vec![format!("{MUTATION_MARKER} 1")]),
            )
        }
    }

    impl ReloadRuntimeChannel for ScriptedChannel {
        fn currentness_view(&mut self) -> Option<SubjectCurrentnessView> {
            self.view.clone()
        }

        fn run_readonly(&mut self, _commands: &[String]) -> ChannelSettlement {
            let settlement = self
                .readonly
                .get(self.readonly_calls)
                .cloned()
                .unwrap_or(ChannelSettlement::Unsettled(UnsettledKind::Timeout));
            self.readonly_calls += 1;
            settlement
        }

        fn run_mutation(&mut self, commands: &[String]) -> ChannelSettlement {
            self.issued_mutations.push(commands.to_vec());
            self.mutation.clone()
        }
    }

    fn run(channel: &mut ScriptedChannel) -> Result<ReloadExecution, Box<dyn std::error::Error>> {
        let plan = admitted_plan()?;
        let mut clock = RuntimeModuleGenerationClock::new();
        Ok(execute_reload(&plan, ReloadMechanism::IncDeletionAndRequire, channel, &mut clock))
    }

    // ---------------------------------------------------------------
    // The load-bearing invariant
    // ---------------------------------------------------------------

    /// Every reachable execution: the phase/outcome pair is contract-valid,
    /// and the mutation-issued flag agrees exactly with the generation.
    ///
    /// This is the possibly-applied boundary stated as one property. It is
    /// the test that fails if any future branch invents a route from an
    /// issued mutation back to a clean terminal state.
    #[test]
    fn every_execution_holds_the_possibly_applied_boundary() -> TestResult {
        let readonly_settlements = || {
            vec![
                ScriptedChannel::ok(PREFLIGHT_MARKER, true, PATH),
                ScriptedChannel::ok(PREFLIGHT_MARKER, false, PATH),
                ScriptedChannel::ok(PREFLIGHT_MARKER, true, "/other/App/Core.pm"),
                ChannelSettlement::Acknowledged(vec!["  DB<2> ".to_string()]),
                ChannelSettlement::NotIssued("no stdin".to_string()),
                ChannelSettlement::Unsettled(UnsettledKind::Timeout),
                ChannelSettlement::Unsettled(UnsettledKind::TransportLoss),
                ChannelSettlement::Unsettled(UnsettledKind::Cancelled),
            ]
        };
        let mutation_settlements = vec![
            ChannelSettlement::Acknowledged(vec![format!("{MUTATION_MARKER} 1")]),
            ChannelSettlement::Acknowledged(vec![format!("{MUTATION_MARKER} 0")]),
            ChannelSettlement::Acknowledged(vec!["  DB<3> ".to_string()]),
            ChannelSettlement::NotIssued("write failed".to_string()),
            ChannelSettlement::Unsettled(UnsettledKind::Timeout),
            ChannelSettlement::Unsettled(UnsettledKind::TransportLoss),
            ChannelSettlement::Unsettled(UnsettledKind::Cancelled),
        ];
        let read_back_settlements = || {
            vec![
                ScriptedChannel::ok(READBACK_MARKER, true, PATH),
                ScriptedChannel::ok(READBACK_MARKER, false, PATH),
                ScriptedChannel::ok(READBACK_MARKER, true, "/other/App/Core.pm"),
                ChannelSettlement::Acknowledged(vec!["  DB<4> ".to_string()]),
                ChannelSettlement::NotIssued("gone".to_string()),
                ChannelSettlement::Unsettled(UnsettledKind::Timeout),
                ChannelSettlement::Unsettled(UnsettledKind::Cancelled),
            ]
        };

        let mut executions = 0_usize;
        let mut saw_reloaded = false;
        let mut saw_indeterminate = false;
        let mut saw_refused = false;
        let mut saw_failed = false;

        for view in [Some(current_view()), None] {
            for preflight in readonly_settlements() {
                for mutation in &mutation_settlements {
                    for read_back in read_back_settlements() {
                        let mut channel = ScriptedChannel::new(
                            vec![preflight.clone(), read_back],
                            mutation.clone(),
                        );
                        channel.view = view.clone();
                        let execution = run(&mut channel)?;
                        executions += 1;

                        // 1. The phase/outcome pair is contract-valid.
                        assert!(
                            phase_permits_outcome(execution.phase_reached, &execution.outcome),
                            "invalid phase/outcome pair: {execution:?}"
                        );

                        // 2. Issuing the mutation and advancing the
                        //    generation are the same event.
                        let advanced = execution.generation.advanced();
                        assert_eq!(
                            execution.mutation_issued, advanced,
                            "mutation_issued must equal generation advance: {execution:?}"
                        );
                        assert_eq!(
                            execution.outcome.generation_effect() == GenerationEffect::Advance,
                            advanced,
                            "generation effect must match the clock: {execution:?}"
                        );

                        // 3. An issued mutation never projects as clean
                        //    unless it is a fully evidenced reload.
                        if execution.mutation_issued {
                            assert!(
                                matches!(
                                    execution.outcome,
                                    LoadedModuleReloadOutcome::Reloaded
                                        | LoadedModuleReloadOutcome::IndeterminatePossiblyApplied { .. }
                                ),
                                "issued mutation must be reloaded or indeterminate: {execution:?}"
                            );
                        } else {
                            assert!(
                                !matches!(
                                    execution.outcome,
                                    LoadedModuleReloadOutcome::Reloaded
                                        | LoadedModuleReloadOutcome::IndeterminatePossiblyApplied { .. }
                                ),
                                "unissued mutation must not claim runtime effect: {execution:?}"
                            );
                        }

                        match execution.outcome {
                            LoadedModuleReloadOutcome::Reloaded => saw_reloaded = true,
                            LoadedModuleReloadOutcome::IndeterminatePossiblyApplied { .. } => {
                                saw_indeterminate = true;
                            }
                            LoadedModuleReloadOutcome::Refused { .. } => saw_refused = true,
                            LoadedModuleReloadOutcome::FailedBeforeMutation { .. } => {
                                saw_failed = true;
                            }
                        }
                    }
                }
            }
        }

        // The sweep is non-vacuous: all four terminal classes occur.
        assert!(executions >= 700, "sweep must be broad, ran {executions}");
        assert!(saw_reloaded && saw_indeterminate && saw_refused && saw_failed);
        Ok(())
    }

    // ---------------------------------------------------------------
    // The twelve fixture races from #10098
    // ---------------------------------------------------------------

    /// 1. Ordinary module reload succeeds end to end.
    #[test]
    fn race_01_ordinary_module_reloads() -> TestResult {
        let execution = run(&mut ScriptedChannel::happy())?;
        assert_eq!(execution.outcome, LoadedModuleReloadOutcome::Reloaded);
        assert_eq!(execution.phase_reached, ReloadTransactionPhase::CommitGeneration);
        assert!(execution.mutation_issued);
        assert!(execution.generation.advanced());
        Ok(())
    }

    /// 2. The mechanism is unavailable: refuse, never fall back to `%INC`.
    #[test]
    fn race_02_unavailable_mechanism_refuses_without_fallback() -> TestResult {
        for mechanism in [
            ReloadMechanism::DoOrRequireHelper,
            ReloadMechanism::WorkspaceRuntimeHelperObserver,
            ReloadMechanism::ClassRefreshCompatibilitySubject,
        ] {
            let plan = admitted_plan()?;
            let mut clock = RuntimeModuleGenerationClock::new();
            let mut channel = ScriptedChannel::happy();
            let execution = execute_reload(&plan, mechanism, &mut channel, &mut clock);
            assert_eq!(
                execution.outcome,
                LoadedModuleReloadOutcome::Refused {
                    disposition: LoadedModuleReloadEligibility::UnsupportedRuntime
                },
                "{mechanism:?} must refuse"
            );
            assert!(!execution.mutation_issued);
            assert!(
                channel.issued_mutations.is_empty(),
                "{mechanism:?} must not silently execute the %INC path"
            );
            assert!(!clock.current().is_exhausted());
            assert_eq!(clock.current(), Default::default());
        }
        Ok(())
    }

    /// 3. Preflight cannot observe the registration: pre-mutation failure,
    ///    and nothing was written to the debuggee.
    #[test]
    fn race_03_preflight_failure_never_mutates() -> TestResult {
        let mut channel = ScriptedChannel::new(
            vec![ChannelSettlement::NotIssued("no session".to_string())],
            ChannelSettlement::Acknowledged(vec![format!("{MUTATION_MARKER} 1")]),
        );
        let execution = run(&mut channel)?;
        assert_eq!(
            execution.outcome,
            LoadedModuleReloadOutcome::FailedBeforeMutation {
                phase: ReloadTransactionPhase::Preflight,
                cause: PreMutationFailureCause::PrepareFailed,
            }
        );
        assert!(!execution.mutation_issued);
        assert!(channel.issued_mutations.is_empty());
        Ok(())
    }

    /// 4. An active frame enters the target between plan and execution.
    ///    Admission owns this class; the executor must never admit it.
    #[test]
    fn race_04_active_frame_refuses_at_admission() -> TestResult {
        let subject = candidate().bind().map_err(|_| "bind")?;
        let observation =
            ReloadAdmissionObservation { active_frame_in_target: true, ..admitted_observation() };
        assert_eq!(
            plan_reload(&subject, &observation),
            Err(LoadedModuleReloadEligibility::ActiveFrameInTarget)
        );
        Ok(())
    }

    /// 5. The saved source changes between plan and execution: the digest
    ///    no longer matches the live view, so the plan is stale.
    #[test]
    fn race_05_source_changed_after_plan_refuses() -> TestResult {
        let mut channel = ScriptedChannel::happy();
        channel.view = Some(SubjectCurrentnessView {
            saved_content_digest: "sha256:different".to_string(),
            ..current_view()
        });
        let execution = run(&mut channel)?;
        assert_eq!(
            execution.outcome,
            LoadedModuleReloadOutcome::Refused {
                disposition: LoadedModuleReloadEligibility::SourceNotExactOrStale
            }
        );
        assert!(channel.issued_mutations.is_empty());
        Ok(())
    }

    /// 6. The same module key now resolves under a different include root:
    ///    the runtime mapping is ambiguous, so nothing is mutated.
    #[test]
    fn race_06_same_name_other_include_root_refuses() -> TestResult {
        let mut channel = ScriptedChannel::new(
            vec![ScriptedChannel::ok(PREFLIGHT_MARKER, true, "/other/lib/App/Core.pm")],
            ChannelSettlement::Acknowledged(vec![format!("{MUTATION_MARKER} 1")]),
        );
        let execution = run(&mut channel)?;
        assert_eq!(
            execution.outcome,
            LoadedModuleReloadOutcome::Refused {
                disposition: LoadedModuleReloadEligibility::AmbiguousRuntimeMapping
            }
        );
        assert!(channel.issued_mutations.is_empty());
        Ok(())
    }

    /// 7. The runtime rejects the reload (`require` died). The `%INC` entry
    ///    was already deleted, so this is possibly applied — never a clean
    ///    failure.
    #[test]
    fn race_07_runtime_rejection_is_possibly_applied() -> TestResult {
        let mut channel = ScriptedChannel::new(
            vec![
                ScriptedChannel::ok(PREFLIGHT_MARKER, true, PATH),
                ScriptedChannel::ok(READBACK_MARKER, false, "-"),
            ],
            ChannelSettlement::Acknowledged(vec![format!("{MUTATION_MARKER} 0")]),
        );
        let execution = run(&mut channel)?;
        assert_eq!(
            execution.outcome,
            LoadedModuleReloadOutcome::IndeterminatePossiblyApplied {
                phase: ReloadTransactionPhase::RuntimeAcknowledgementReadBack,
                cause: IndeterminateCause::ReadBackInconclusive,
            }
        );
        assert!(execution.mutation_issued);
        assert!(execution.generation.advanced());
        assert!(!execution.projects_as_clean());
        Ok(())
    }

    /// 8. Timeout *before* the boundary: nothing ran, so no generation moves.
    #[test]
    fn race_08_timeout_before_boundary_advances_nothing() -> TestResult {
        let mut channel = ScriptedChannel::new(
            vec![ChannelSettlement::Unsettled(UnsettledKind::Timeout)],
            ChannelSettlement::Acknowledged(vec![format!("{MUTATION_MARKER} 1")]),
        );
        let execution = run(&mut channel)?;
        assert_eq!(
            execution.outcome,
            LoadedModuleReloadOutcome::FailedBeforeMutation {
                phase: ReloadTransactionPhase::Preflight,
                cause: PreMutationFailureCause::PrepareFailed,
            }
        );
        assert!(!execution.generation.advanced());
        assert!(channel.issued_mutations.is_empty());
        Ok(())
    }

    /// 9. Timeout and transport loss *after* mutation begins: possibly
    ///    applied, generation advanced, never projected clean or empty.
    ///
    ///    This is the exact case `query_inc_entries` answers with an empty
    ///    list for a read-only query. A mutation must not.
    #[test]
    fn race_09_loss_after_boundary_is_never_clean_or_empty() -> TestResult {
        for (kind, expected) in [
            (UnsettledKind::Timeout, IndeterminateCause::TimeoutAfterMutationBegan),
            (UnsettledKind::TransportLoss, IndeterminateCause::TransportLossAfterMutationBegan),
        ] {
            let mut channel = ScriptedChannel::new(
                vec![ScriptedChannel::ok(PREFLIGHT_MARKER, true, PATH)],
                ChannelSettlement::Unsettled(kind),
            );
            let execution = run(&mut channel)?;
            assert_eq!(
                execution.outcome,
                LoadedModuleReloadOutcome::IndeterminatePossiblyApplied {
                    phase: ReloadTransactionPhase::RuntimeMutationBegins,
                    cause: expected,
                },
                "{kind:?}"
            );
            assert!(execution.mutation_issued);
            assert!(execution.generation.advanced());
            assert!(!execution.projects_as_clean());
        }
        Ok(())
    }

    /// 10. Cancellation on both sides of the boundary. Before: a clean
    ///     pre-mutation cancel. After: possibly applied, because a cancel
    ///     cannot prove non-application.
    #[test]
    fn race_10_cancellation_splits_at_the_boundary() -> TestResult {
        let mut before = ScriptedChannel::new(
            vec![ChannelSettlement::Unsettled(UnsettledKind::Cancelled)],
            ChannelSettlement::Acknowledged(vec![format!("{MUTATION_MARKER} 1")]),
        );
        let execution = run(&mut before)?;
        assert_eq!(
            execution.outcome,
            LoadedModuleReloadOutcome::FailedBeforeMutation {
                phase: ReloadTransactionPhase::Preflight,
                cause: PreMutationFailureCause::CancelledBeforeMutationBegan,
            }
        );
        assert!(!execution.generation.advanced());
        assert!(before.issued_mutations.is_empty());

        let mut after = ScriptedChannel::new(
            vec![ScriptedChannel::ok(PREFLIGHT_MARKER, true, PATH)],
            ChannelSettlement::Unsettled(UnsettledKind::Cancelled),
        );
        let execution = run(&mut after)?;
        assert_eq!(
            execution.outcome,
            LoadedModuleReloadOutcome::IndeterminatePossiblyApplied {
                phase: ReloadTransactionPhase::RuntimeMutationBegins,
                cause: IndeterminateCause::AmbiguousAcknowledgement,
            }
        );
        assert!(execution.generation.advanced());
        Ok(())
    }

    /// 11. The debuggee exits during the transaction: the read-back is
    ///     never issued, and the outcome stays possibly applied.
    #[test]
    fn race_11_debuggee_exit_during_transaction() -> TestResult {
        let mut channel = ScriptedChannel::new(
            vec![
                ScriptedChannel::ok(PREFLIGHT_MARKER, true, PATH),
                ChannelSettlement::NotIssued("process exited".to_string()),
            ],
            ChannelSettlement::Acknowledged(vec![format!("{MUTATION_MARKER} 1")]),
        );
        let execution = run(&mut channel)?;
        assert_eq!(
            execution.outcome,
            LoadedModuleReloadOutcome::IndeterminatePossiblyApplied {
                phase: ReloadTransactionPhase::RuntimeAcknowledgementReadBack,
                cause: IndeterminateCause::ReadBackInconclusive,
            }
        );
        assert!(execution.mutation_issued);
        assert!(execution.generation.advanced());
        Ok(())
    }

    /// 12. A repeated request against a stale plan: the session generation
    ///     moved, so the plan is refused rather than re-executed.
    #[test]
    fn race_12_repeated_request_against_stale_plan_refuses() -> TestResult {
        let mut channel = ScriptedChannel::happy();
        channel.view = Some(SubjectCurrentnessView { suspension_generation: 13, ..current_view() });
        let execution = run(&mut channel)?;
        assert_eq!(
            execution.outcome,
            LoadedModuleReloadOutcome::Refused {
                disposition: LoadedModuleReloadEligibility::SourceNotExactOrStale
            }
        );
        assert!(channel.issued_mutations.is_empty());

        // A replaced session is refused the same way.
        let mut replaced = ScriptedChannel::happy();
        replaced.view = Some(SubjectCurrentnessView { session_generation: 5, ..current_view() });
        let execution = run(&mut replaced)?;
        assert_eq!(
            execution.outcome,
            LoadedModuleReloadOutcome::Refused {
                disposition: LoadedModuleReloadEligibility::SourceNotExactOrStale
            }
        );
        assert!(replaced.issued_mutations.is_empty());
        Ok(())
    }

    /// The debuggee not being stopped and command-ready refuses before any
    /// observation is attempted.
    #[test]
    fn not_command_ready_refuses_before_observing() -> TestResult {
        let mut channel = ScriptedChannel::happy();
        channel.view = None;
        let execution = run(&mut channel)?;
        assert_eq!(
            execution.outcome,
            LoadedModuleReloadOutcome::Refused {
                disposition: LoadedModuleReloadEligibility::NotStoppedOrNotCommandReady
            }
        );
        assert_eq!(channel.readonly_calls, 0);
        assert!(channel.issued_mutations.is_empty());
        Ok(())
    }

    // ---------------------------------------------------------------
    // No arbitrary command, path, or expression surface
    // ---------------------------------------------------------------

    /// A prompt is not an acknowledgement. Framed output that carries no
    /// mutation marker is ambiguous, not success.
    #[test]
    fn prompt_alone_is_not_an_acknowledgement() -> TestResult {
        let mut channel = ScriptedChannel::new(
            vec![
                ScriptedChannel::ok(PREFLIGHT_MARKER, true, PATH),
                ScriptedChannel::ok(READBACK_MARKER, true, PATH),
            ],
            ChannelSettlement::Acknowledged(vec![
                "  DB<2> ".to_string(),
                "ok".to_string(),
                "1".to_string(),
            ]),
        );
        let execution = run(&mut channel)?;
        assert_eq!(
            execution.outcome,
            LoadedModuleReloadOutcome::IndeterminatePossiblyApplied {
                phase: ReloadTransactionPhase::RuntimeAcknowledgementReadBack,
                cause: IndeterminateCause::ReadBackInconclusive,
            }
        );
        assert!(!execution.projects_as_clean());
        Ok(())
    }

    /// The module key allowlist refuses every injection shape, and the
    /// refusal happens before any command text is built.
    #[test]
    fn unsafe_module_keys_are_refused_before_any_command() -> TestResult {
        let hostile = [
            "App/Core.pm'); system('id'); ('",
            "App/Core.pm\nq foo",
            "/abs/App/Core.pm",
            "../../etc/passwd.pm",
            "App/Core.pm; print 1",
            "App Core.pm",
            "App/(Core).pm",
            "App/Core.pl",
            "App//Core.pm",
            "App/Core.pm\"",
            "",
        ];
        for key in hostile {
            assert!(!module_key_is_safe(key), "{key:?} must be refused");
            let hostile_candidate = SubjectCandidate { inc_key: key.to_string(), ..candidate() };
            // A key that cannot bind at all is already refused upstream;
            // only bindable-but-unsafe keys reach the command planner.
            if let Ok(subject) = hostile_candidate.bind() {
                assert_eq!(
                    plan_commands(&subject, ReloadMechanism::IncDeletionAndRequire),
                    Err(CommandPlanError::UnsafeModuleKey),
                    "{key:?} must not produce commands"
                );
            }
        }
        // The ordinary key is accepted, so the guard is not vacuous.
        assert!(module_key_is_safe(KEY));
        assert!(module_key_is_safe("Deep/Nested/Mod-2.0/Thing.pm"));
        Ok(())
    }

    /// An unsafe key refuses the whole transaction without touching the
    /// debuggee, and reports an inexact-identity disposition.
    #[test]
    fn unsafe_key_refuses_the_transaction() -> TestResult {
        let hostile = SubjectCandidate { inc_key: "App/Core.pm; die".to_string(), ..candidate() }
            .bind()
            .map_err(|_| "hostile candidate must still bind")?;
        let plan = plan_reload(&hostile, &admitted_observation())
            .map_err(|_| "admission is observation-driven")?;
        let mut clock = RuntimeModuleGenerationClock::new();
        let mut channel = ScriptedChannel::happy();
        let execution =
            execute_reload(&plan, ReloadMechanism::IncDeletionAndRequire, &mut channel, &mut clock);
        assert_eq!(
            execution.outcome,
            LoadedModuleReloadOutcome::Refused {
                disposition: LoadedModuleReloadEligibility::SourceNotExactOrStale
            }
        );
        assert_eq!(execution.phase_reached, ReloadTransactionPhase::Admission);
        assert_eq!(channel.readonly_calls, 0);
        assert!(channel.issued_mutations.is_empty());
        Ok(())
    }

    /// The generated commands interpolate only the bound `%INC` key, and
    /// the mutation is exactly one command.
    #[test]
    fn commands_are_derived_only_from_the_bound_subject() -> TestResult {
        let subject = candidate().bind().map_err(|_| "bind")?;
        let commands = plan_commands(&subject, ReloadMechanism::IncDeletionAndRequire)
            .map_err(|_| "safe key must plan")?;
        assert_eq!(commands.mutation.len(), 1, "exactly one mutation command");
        assert_eq!(commands.preflight.len(), 1);
        assert_eq!(commands.read_back.len(), 1);
        for command in commands
            .preflight
            .iter()
            .chain(commands.mutation.iter())
            .chain(commands.read_back.iter())
        {
            assert!(!command.contains('\n'), "no command may embed a newline: {command}");
            // The resolved path is compared, never executed.
            assert!(!command.contains(PATH), "the runtime path is never interpolated: {command}");
            assert!(command.contains(KEY));
        }
        assert!(commands.mutation[0].contains("delete $INC"));
        assert!(commands.mutation[0].contains("require"));
        // Preflight is read-only: it never deletes, requires, or evals.
        assert!(!commands.preflight[0].contains("delete"));
        assert!(!commands.preflight[0].contains("require"));
        assert!(!commands.preflight[0].contains("eval"));
        assert!(!commands.read_back[0].contains("delete"));
        assert!(!commands.read_back[0].contains("require"));
        Ok(())
    }

    /// Exactly one mechanism is executable; the record still describes all
    /// four, so refusal is a decision rather than an omission.
    #[test]
    fn exactly_one_mechanism_is_executable() {
        let executable: Vec<ReloadMechanism> =
            ReloadMechanism::ALL.into_iter().filter(|m| mechanism_is_executable(*m)).collect();
        assert_eq!(executable, vec![ReloadMechanism::IncDeletionAndRequire]);
        assert_eq!(super::super::mechanism_records().len(), 4);
    }

    // ---------------------------------------------------------------
    // Live proof: the generated commands against a real perl5db debuggee
    // ---------------------------------------------------------------
    //
    // The scripted tests above prove the state machine. They cannot prove
    // that the *command text* is valid Perl, that `p do { ... }` survives
    // perl5db's evaluator, or that `delete $INC` + `require` actually
    // replaces the running code. This fixture proves exactly that, against
    // a real `perl -d` process, and then feeds the debuggee's own output
    // back through the real parser and executor.
    //
    // This is not public-binary proof: it drives perl5db directly rather
    // than through the shipped `perl-dap` adapter. Exact-binary and
    // installed proof are R04 (#10104).

    /// Live debuggee output: everything perl5db wrote to stdout.
    struct LiveRun {
        stdout: String,
    }

    impl LiveRun {
        /// Framed lines carrying a marker, as the framed capture would
        /// hand them to the executor.
        fn lines_with(&self, marker: &str) -> Vec<String> {
            self.stdout
                .lines()
                .filter(|line| line.contains(marker))
                .map(|line| line.to_string())
                .collect()
        }
    }

    /// A channel replaying one real debuggee's recorded output.
    ///
    /// The settlements are real perl5db lines, parsed by the production
    /// parser; only the transport is replayed.
    struct ReplayChannel {
        preflight: Vec<String>,
        mutation: Vec<String>,
        read_back: Vec<String>,
        readonly_calls: usize,
    }

    impl ReloadRuntimeChannel for ReplayChannel {
        fn currentness_view(&mut self) -> Option<SubjectCurrentnessView> {
            Some(current_view())
        }

        fn run_readonly(&mut self, _commands: &[String]) -> ChannelSettlement {
            let lines = if self.readonly_calls == 0 {
                self.preflight.clone()
            } else {
                self.read_back.clone()
            };
            self.readonly_calls += 1;
            ChannelSettlement::Acknowledged(lines)
        }

        fn run_mutation(&mut self, _commands: &[String]) -> ChannelSettlement {
            ChannelSettlement::Acknowledged(self.mutation.clone())
        }
    }

    /// Whether a real `perl -d` is usable here. A missing interpreter or a
    /// missing debugger is an instrument skip, never a pass.
    fn perl_debugger_available(oracle: &perl_lsp_rs_core::config::PerlOracleEnv) -> bool {
        oracle
            .clone()
            .into_command()
            .arg("-d")
            .arg("-e")
            .arg("1")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    /// Drive a real `perl -d` debuggee through the given command stream.
    fn drive_live_debuggee(
        oracle: perl_lsp_rs_core::config::PerlOracleEnv,
        scratch: &std::path::Path,
        program: &std::path::Path,
        commands: &[String],
    ) -> Result<LiveRun, String> {
        use std::io::Write as _;

        let mut command = oracle.into_command();
        command
            .arg("-d")
            .arg("-I")
            .arg(scratch)
            .arg("--")
            .arg(program)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let mut child = command.spawn().map_err(|error| format!("spawn perl -d: {error}"))?;

        // Write the whole command stream, then close stdin so perl5db
        // reaches EOF and exits even if `q` is swallowed.
        {
            let Some(mut stdin) = child.stdin.take() else {
                let _ = child.kill();
                return Err("perl -d has no stdin".to_string());
            };
            for line in commands {
                stdin
                    .write_all(format!("{line}\n").as_bytes())
                    .map_err(|error| format!("write debugger command: {error}"))?;
            }
            stdin.write_all(b"q\n").map_err(|error| format!("write quit: {error}"))?;
            stdin.flush().map_err(|error| format!("flush: {error}"))?;
        }

        // perl5db writes its prompt and `p` output to STDERR whenever
        // STDIN is not a terminal, so both streams are captured and
        // merged. Bounded reads: a hung debugger is killed rather than
        // hanging the suite, mirroring the measurement harness.
        let read_capped = |pipe: Option<Box<dyn std::io::Read + Send>>| {
            std::thread::spawn(move || {
                let mut buffer = Vec::new();
                let Some(mut pipe) = pipe else { return buffer };
                let mut chunk = [0u8; 8192];
                loop {
                    if buffer.len() >= 256 * 1024 {
                        break;
                    }
                    match std::io::Read::read(&mut pipe, &mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(read) => buffer.extend_from_slice(&chunk[..read]),
                    }
                }
                buffer
            })
        };
        let out_reader = read_capped(
            child.stdout.take().map(|pipe| Box::new(pipe) as Box<dyn std::io::Read + Send>),
        );
        let err_reader = read_capped(
            child.stderr.take().map(|pipe| Box::new(pipe) as Box<dyn std::io::Read + Send>),
        );

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) => {
                    if std::time::Instant::now() >= deadline {
                        let _ = child.kill();
                        return Err("perl -d exceeded the 20s deadline".to_string());
                    }
                    std::thread::sleep(std::time::Duration::from_millis(25));
                }
                Err(error) => return Err(format!("wait perl -d: {error}")),
            }
        }
        let mut merged = out_reader.join().unwrap_or_default();
        merged.extend_from_slice(&err_reader.join().unwrap_or_default());
        Ok(LiveRun { stdout: strip_terminal_controls(&String::from_utf8_lossy(&merged)) })
    }

    /// Strip ANSI escape sequences and stray C0 controls from perl5db's
    /// decorated prompt so marker fields parse cleanly.
    fn strip_terminal_controls(raw: &str) -> String {
        let mut out = String::with_capacity(raw.len());
        let mut chars = raw.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\u{1b}' {
                // CSI: ESC '[' ... final byte in @..~
                if chars.peek() == Some(&'[') {
                    chars.next();
                    for inner in chars.by_ref() {
                        if ('\u{40}'..='\u{7e}').contains(&inner) {
                            break;
                        }
                    }
                }
                continue;
            }
            if c == '\n' || c == '\t' || !c.is_control() {
                out.push(c);
            }
        }
        out
    }

    /// Read one marker's trailing field out of live debuggee output.
    fn live_field(run: &LiveRun, marker: &str) -> Option<String> {
        run.stdout.lines().find_map(|line| {
            let index = line.find(marker)?;
            let rest = line.get(index + marker.len()..)?;
            rest.split_whitespace().next().map(|field| field.to_string())
        })
    }

    /// The generated command plan, executed against a real `perl -d`,
    /// actually replaces the running module — and the debuggee's own
    /// output drives the executor to `Reloaded`.
    ///
    /// Non-vacuity is explicit: the same subroutine returns 41 before the
    /// transaction and 42 after it. If `delete $INC` + `require` did
    /// nothing, the "after" value would still be 41 and this test fails.
    #[test]
    fn live_perl_debuggee_reload_replaces_running_code() -> TestResult {
        let Some(oracle) = perl_lsp_rs_core::config::PerlOracleEnv::for_dap_test_fixture() else {
            // No perl on PATH: instrument skip, not a pass.
            return Ok(());
        };
        if !perl_debugger_available(&oracle) {
            return Ok(());
        }

        let scratch = std::env::temp_dir().join(format!(
            "perl-reload-live-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|since| since.as_nanos())
                .unwrap_or(0),
        ));
        let module_dir = scratch.join("App");
        std::fs::create_dir_all(&module_dir)?;
        let module_path = module_dir.join("Core.pm");
        std::fs::write(&module_path, "package App::Core;\nsub answer { 41 }\n1;\n")?;
        let program_path = scratch.join("main.pl");
        std::fs::write(
            &program_path,
            "use App::Core;\nmy $x = App::Core::answer();\nprint \"RAN $x\\n\";\n",
        )?;

        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            // The subject names the real runtime path perl will resolve.
            let resolved = module_path.to_string_lossy().into_owned();
            let subject = SubjectCandidate {
                inc_key: KEY.to_string(),
                resolved_runtime_path: resolved.clone(),
                ..candidate()
            }
            .bind()
            .map_err(|_| "live subject must bind")?;
            let commands = plan_commands(&subject, ReloadMechanism::IncDeletionAndRequire)
                .map_err(|_| "live subject must plan")?;

            // The harness rewrites the module on disk from inside the
            // debuggee, so the rewrite is serialized with the transaction
            // exactly the way an editor save would be.
            let escaped_path = resolved.replace('\\', "\\\\").replace('"', "\\\"");
            let rewrite = format!(
                "p do {{ open(my $fh, '>', \"{escaped_path}\") or die; \
                 print $fh \"package App::Core; sub answer {{ 42 }} 1;\"; \
                 close $fh; \"PERLLSP_TEST_REWROTE ok\" }}"
            );
            let mut stream =
                vec!["p \"PERLLSP_TEST_BEFORE \" . App::Core::answer()".to_string(), rewrite];
            stream.extend(commands.preflight.iter().cloned());
            stream.extend(commands.mutation.iter().cloned());
            stream.extend(commands.read_back.iter().cloned());
            stream.push("p \"PERLLSP_TEST_AFTER \" . App::Core::answer()".to_string());

            let run = drive_live_debuggee(oracle, &scratch, &program_path, &stream)?;
            let context = || format!("debuggee stdout was:\n{}", run.stdout);

            // The harness itself worked.
            assert_eq!(
                live_field(&run, "PERLLSP_TEST_BEFORE").as_deref(),
                Some("41"),
                "module must start at 41; {}",
                context()
            );
            assert_eq!(
                live_field(&run, "PERLLSP_TEST_REWROTE").as_deref(),
                Some("ok"),
                "harness must rewrite the module; {}",
                context()
            );

            // The generated commands are valid Perl and produced markers.
            assert_eq!(
                live_field(&run, PREFLIGHT_MARKER).as_deref(),
                Some("present"),
                "preflight must observe the loaded module; {}",
                context()
            );
            assert_eq!(
                live_field(&run, MUTATION_MARKER).as_deref(),
                Some("1"),
                "mutation must report a successful require; {}",
                context()
            );
            assert_eq!(
                live_field(&run, READBACK_MARKER).as_deref(),
                Some("present"),
                "read-back must observe the refreshed registration; {}",
                context()
            );

            // The discriminating assertion: the running code changed.
            assert_eq!(
                live_field(&run, "PERLLSP_TEST_AFTER").as_deref(),
                Some("42"),
                "the reload must replace the running sub; {}",
                context()
            );

            // Close the loop: the debuggee's own lines, parsed by the
            // production parser, drive the executor to `Reloaded`.
            let mut channel = ReplayChannel {
                preflight: run.lines_with(PREFLIGHT_MARKER),
                mutation: run.lines_with(MUTATION_MARKER),
                read_back: run.lines_with(READBACK_MARKER),
                readonly_calls: 0,
            };
            let plan = plan_reload(&subject, &admitted_observation())
                .map_err(|_| "live subject must admit")?;
            let mut clock = RuntimeModuleGenerationClock::new();
            let execution = execute_reload(
                &plan,
                ReloadMechanism::IncDeletionAndRequire,
                &mut channel,
                &mut clock,
            );
            assert_eq!(
                execution.outcome,
                LoadedModuleReloadOutcome::Reloaded,
                "real debuggee output must drive the executor to Reloaded; {}",
                context()
            );
            assert!(execution.mutation_issued);
            assert!(execution.generation.advanced());
            Ok(())
        })();

        let _ = std::fs::remove_dir_all(&scratch);
        result
    }

    /// Marker parsing refuses partial and malformed frames instead of
    /// reading them as "absent".
    #[test]
    fn registration_parsing_refuses_malformed_frames() {
        assert_eq!(parse_registration(&[], PREFLIGHT_MARKER), None);
        assert_eq!(
            parse_registration(&[format!("{PREFLIGHT_MARKER} maybe /p")], PREFLIGHT_MARKER),
            None
        );
        assert_eq!(parse_registration(&[format!("{PREFLIGHT_MARKER}")], PREFLIGHT_MARKER), None);
        let present = parse_registration(
            &[format!("  DB<2> {PREFLIGHT_MARKER} present /ws/lib/App/Core.pm")],
            PREFLIGHT_MARKER,
        );
        assert_eq!(
            present,
            Some(RegistrationObservation {
                present: true,
                path: "/ws/lib/App/Core.pm".to_string()
            })
        );
        assert_eq!(parse_mutation_ack(&[]), None);
        assert_eq!(parse_mutation_ack(&[format!("{MUTATION_MARKER} 2")]), None);
        assert_eq!(parse_mutation_ack(&[format!("{MUTATION_MARKER} 1")]), Some(true));
        assert_eq!(parse_mutation_ack(&[format!("{MUTATION_MARKER} 0")]), Some(false));
    }
}
