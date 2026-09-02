//! Terminal process results and the evidence they carry.

use std::time::Duration;

use super::encoding::{ContentFingerprint, PlanFingerprint};
use super::identity::{PlanId, RunId, SchemaVersion};
use super::validation::PlanRejection;

/// Which output channel a piece of stream evidence belongs to.
///
/// stdout and stderr never share an identity; a result carries one of each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StreamChannel {
    /// The child's standard output.
    Stdout,
    /// The child's standard error.
    Stderr,
}

/// Whether a channel's retained bytes are the whole of what was observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TruncationState {
    /// Everything observed was retained.
    Complete,
    /// Observation stopped at the budget, so more output may have existed.
    ObservationTruncated {
        /// The observation limit that was reached.
        limit_bytes: u64,
    },
    /// Everything was observed but retention stopped at the budget.
    RetentionTruncated {
        /// The retention limit that was reached.
        limit_bytes: u64,
    },
}

impl TruncationState {
    /// Whether the retained bytes are the complete channel content.
    pub fn is_complete(self) -> bool {
        matches!(self, Self::Complete)
    }
}

/// What can be said about a lossy text view of raw bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodedViewLimitation {
    /// The bytes decoded as UTF-8 without loss.
    ValidUtf8,
    /// The bytes contained invalid UTF-8; a text view is lossy.
    ///
    /// The raw bytes remain the evidence; the decoded view does not.
    LossyUtf8,
    /// No decoded view was produced.
    NotDecoded,
}

/// Everything a result records about one output channel.
///
/// Observed and retained quantities are separate fields on purpose: a
/// supervisor may see far more bytes than it is allowed to keep, and a
/// consumer must be able to tell the difference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamEvidence {
    channel: StreamChannel,
    observed_bytes: u64,
    observed_fingerprint: ContentFingerprint,
    retained: Vec<u8>,
    truncation: TruncationState,
    decoded_view: DecodedViewLimitation,
}

impl StreamEvidence {
    /// Record evidence for a channel.
    pub fn new(
        channel: StreamChannel,
        observed_bytes: u64,
        observed_fingerprint: ContentFingerprint,
        retained: Vec<u8>,
        truncation: TruncationState,
    ) -> Self {
        let decoded_view = if retained.is_empty() {
            DecodedViewLimitation::NotDecoded
        } else if std::str::from_utf8(&retained).is_ok() {
            DecodedViewLimitation::ValidUtf8
        } else {
            DecodedViewLimitation::LossyUtf8
        };
        Self { channel, observed_bytes, observed_fingerprint, retained, truncation, decoded_view }
    }

    /// Record evidence for a channel whose content was fully observed and kept.
    pub fn complete(channel: StreamChannel, bytes: Vec<u8>) -> Self {
        let fingerprint = ContentFingerprint::of(&bytes);
        let observed = bytes.len() as u64;
        Self::new(channel, observed, fingerprint, bytes, TruncationState::Complete)
    }

    /// An empty channel.
    pub fn empty(channel: StreamChannel) -> Self {
        Self::complete(channel, Vec::new())
    }

    /// The channel this evidence belongs to.
    pub fn channel(&self) -> StreamChannel {
        self.channel
    }

    /// How many bytes the supervisor observed.
    pub fn observed_bytes(&self) -> u64 {
        self.observed_bytes
    }

    /// The identity of the observed content.
    pub fn observed_fingerprint(&self) -> ContentFingerprint {
        self.observed_fingerprint
    }

    /// The bytes actually retained.
    pub fn retained(&self) -> &[u8] {
        &self.retained
    }

    /// Whether retention was complete.
    pub fn truncation(&self) -> TruncationState {
        self.truncation
    }

    /// What can be said about a text view of the retained bytes.
    pub fn decoded_view(&self) -> DecodedViewLimitation {
        self.decoded_view
    }

    /// A lossy text view of the retained bytes.
    ///
    /// Pair with [`Self::decoded_view`]: a lossy view is not evidence.
    pub fn retained_lossy(&self) -> String {
        String::from_utf8_lossy(&self.retained).into_owned()
    }
}

/// Why a run was cancelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CancellationReason {
    /// The requesting operation was superseded or withdrawn.
    OperationSuperseded,
    /// The owning session or workspace is shutting down.
    Shutdown,
    /// A person cancelled the operation.
    UserRequested,
}

/// What the supervisor observed about the child's own settlement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservedSettlement {
    /// The child never started.
    NotStarted,
    /// The child exited with a status code.
    Exited {
        /// The exit code.
        code: i32,
    },
    /// The child was terminated by a signal.
    Signaled {
        /// The signal number.
        signal: i32,
    },
    /// The supervisor could not observe how the child settled.
    NotObserved,
}

/// The control-plane state at the moment a run settles.
///
/// Recorded independently of what the child did, so that a child exiting zero
/// during cleanup cannot erase the fact that the run timed out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ControlState {
    /// A cancellation was requested, and why.
    pub cancellation_requested: Option<CancellationReason>,
    /// The run had started when cancellation was requested.
    pub started_before_cancellation: bool,
    /// The wall-clock deadline elapsed.
    pub deadline_reached: bool,
    /// An output budget was exceeded.
    pub output_limit_exceeded: bool,
    /// Required cleanup did not complete.
    pub cleanup_failed: bool,
    /// The supervisor itself failed.
    pub supervisor_failed: bool,
}

/// How a run ended.
///
/// The states are closed and mutually exclusive. Nothing here collapses a
/// control-plane cause into a child-exit cause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalDisposition {
    /// The child ran and exited with a status code.
    ///
    /// A nonzero code is an executed process result, not an instrument
    /// failure.
    CompletedExit {
        /// The child's exit code.
        code: i32,
    },
    /// Validation refused the plan before any start attempt.
    SpawnRejected(PlanRejection),
    /// The plan was valid but the process could not be created.
    SpawnFailed {
        /// A bounded description of the spawn failure.
        detail: SpawnFailureDetail,
    },
    /// Cancellation arrived before the child started.
    CancelledBeforeStart(CancellationReason),
    /// Cancellation arrived while the child was running.
    CancelledRunning(CancellationReason),
    /// The wall-clock deadline elapsed.
    TimedOut,
    /// An output budget was exceeded and the run was terminated for it.
    OutputLimitExceeded,
    /// The child was terminated by an external signal.
    Signaled {
        /// The signal number.
        signal: i32,
    },
    /// Required cleanup failed, and nothing more specific applies.
    CleanupFailed,
    /// The supervisor itself failed.
    SupervisorFailed,
    /// The backend cannot execute this plan's profile on this platform.
    UnsupportedBackend,
    /// A required identity or authorization was stale or absent at start.
    StaleOrUnauthorized(PlanRejection),
    /// The run's outcome was never established.
    ///
    /// Never a synonym for success, zero, or green.
    NotProven,
}

/// A bounded, non-prose classification of a spawn failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SpawnFailureDetail {
    /// The executable did not exist at the resolved path.
    ExecutableNotFound,
    /// The executable existed but could not be executed.
    PermissionDenied,
    /// The working directory did not exist or was not usable.
    WorkingDirectoryUnusable,
    /// The operating system refused to create the process.
    ResourceExhausted,
    /// Something else prevented the spawn.
    Other,
}

impl TerminalDisposition {
    /// Elect the terminal disposition from control state and child settlement.
    ///
    /// The precedence is fixed and total:
    ///
    /// 1. supervisor failure
    /// 2. output-limit exceeded
    /// 3. deadline reached
    /// 4. cancellation (running, else before start)
    /// 5. cleanup failure
    /// 6. child signalled
    /// 7. child exited
    /// 8. not proven
    ///
    /// Control-plane causes outrank the child's own settlement, which is what
    /// stops a timeout or cancellation from becoming an ordinary success when
    /// the child happens to exit zero during cleanup. Cleanup failure sits
    /// below the causes that describe *why* the run ended and is additionally
    /// always recorded in its own field, so electing another cause never
    /// discards it.
    ///
    /// The operating-system mechanics that populate [`ControlState`] belong to
    /// the Linux lifecycle lane; this function is the pure rule it applies.
    pub fn elect(control: ControlState, settlement: ObservedSettlement) -> Self {
        if control.supervisor_failed {
            return Self::SupervisorFailed;
        }
        if control.output_limit_exceeded {
            return Self::OutputLimitExceeded;
        }
        if control.deadline_reached {
            return Self::TimedOut;
        }
        if let Some(reason) = control.cancellation_requested {
            return if control.started_before_cancellation {
                Self::CancelledRunning(reason)
            } else {
                Self::CancelledBeforeStart(reason)
            };
        }
        if control.cleanup_failed {
            return Self::CleanupFailed;
        }
        match settlement {
            ObservedSettlement::Signaled { signal } => Self::Signaled { signal },
            ObservedSettlement::Exited { code } => Self::CompletedExit { code },
            ObservedSettlement::NotStarted | ObservedSettlement::NotObserved => Self::NotProven,
        }
    }

    /// Whether the child ran to completion under its own control.
    pub fn is_completed_exit(&self) -> bool {
        matches!(self, Self::CompletedExit { .. })
    }

    /// Whether the run ended in an ordinary zero exit.
    ///
    /// This is the *only* success predicate. It is deliberately narrow: no
    /// control-plane termination and no unobserved outcome can satisfy it.
    pub fn is_ordinary_success(&self) -> bool {
        matches!(self, Self::CompletedExit { code: 0 })
    }
}

/// Whether the supervisor proved it cleaned up what it started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupDisposition {
    /// Cleanup completed and was observed.
    Completed,
    /// Cleanup was attempted and failed.
    Failed,
    /// Cleanup was not required because nothing was started.
    NotRequired,
    /// Cleanup was never observed.
    ///
    /// The default for an abandoned handle: a request to clean up is not
    /// cleanup, and a dropped handle proves nothing.
    NotObserved,
}

impl CleanupDisposition {
    /// Whether cleanup was actually proven.
    pub fn is_proven(self) -> bool {
        matches!(self, Self::Completed | Self::NotRequired)
    }
}

/// What was done about the child's descendants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeDisposition {
    /// An owned process group was terminated and reaped.
    GroupTerminated,
    /// Only the direct child was signalled; descendants may survive.
    ImmediateChildOnly,
    /// No termination was needed.
    NotRequired,
    /// Nothing is known about the descendants.
    Unknown,
}

impl TreeDisposition {
    /// Whether this disposition supports a process-tree cleanup claim.
    ///
    /// Signalling the immediate child does not.
    pub fn proves_tree_cleanup(self) -> bool {
        matches!(self, Self::GroupTerminated | Self::NotRequired)
    }
}

/// What class of evidence produced a result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EvidenceClass {
    /// Produced by a deterministic in-memory fake. Never OS evidence.
    Fake,
    /// Produced by executing a real process on Linux.
    ExactLinux,
    /// Produced by executing a real process on another platform.
    ExactOtherPlatform,
}

impl EvidenceClass {
    /// Whether this class can support a claim about real process behavior.
    pub fn is_executed(self) -> bool {
        matches!(self, Self::ExactLinux | Self::ExactOtherPlatform)
    }
}

/// The backend that produced a result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendIdentity {
    name: String,
    evidence_class: EvidenceClass,
}

impl BackendIdentity {
    /// Identify a backend.
    pub fn new(name: impl Into<String>, evidence_class: EvidenceClass) -> Self {
        Self { name: name.into(), evidence_class }
    }

    /// The backend's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// What class of evidence the backend produces.
    pub fn evidence_class(&self) -> EvidenceClass {
        self.evidence_class
    }
}

/// An explicit non-claim attached to a result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Limitation {
    /// The result came from a fake and says nothing about real processes.
    FakeEvidenceOnly,
    /// Only the direct child was terminated; descendants are unaccounted for.
    DescendantsUnaccounted,
    /// Cleanup was never observed.
    ///
    /// Distinct from [`Self::CleanupFailed`]: this is "we do not know",
    /// not "we checked and it did not work".
    CleanupNotObserved,
    /// Cleanup was attempted, observed, and failed.
    CleanupFailed,
    /// Retained output is not the whole of what the child produced.
    OutputIncomplete,
    /// A text view of the retained bytes is lossy.
    DecodedViewLossy,
    /// Nothing here claims sandboxing, isolation, or hermeticity.
    ///
    /// Present on every result: types, timeouts, and process ownership do not
    /// constrain what admitted code can reach.
    NoIsolationClaimed,
}

/// Bounded work metadata about a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WorkMetadata {
    /// Wall-clock time from start attempt to settlement.
    pub wall_time: Duration,
    /// How many events the run emitted.
    pub events_emitted: u64,
}

/// Why a set of result components could not describe one coherent run.
///
/// Assembling a result is where evidence becomes a claim, so the combinations
/// that would make the claim false are refused here rather than documented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultInconsistency {
    /// Stream evidence was supplied for the wrong channel.
    StreamChannelMismatch {
        /// The channel the slot requires.
        expected: StreamChannel,
        /// The channel the evidence carries.
        found: StreamChannel,
    },
    /// Evidence claims completeness but retains a different number of bytes
    /// than it observed.
    CompleteEvidenceCountMismatch {
        /// The channel whose evidence is inconsistent.
        channel: StreamChannel,
    },
    /// Evidence claims completeness but its fingerprint is not the fingerprint
    /// of the bytes it retained.
    CompleteEvidenceFingerprintMismatch {
        /// The channel whose evidence is inconsistent.
        channel: StreamChannel,
    },
    /// More bytes were retained than were ever observed.
    RetainedExceedsObserved {
        /// The channel whose evidence is inconsistent.
        channel: StreamChannel,
    },
    /// Evidence contradicts the limit it says stopped it.
    ///
    /// Observation truncated at a limit cannot have observed fewer bytes than
    /// that limit, and retention truncated at a limit cannot have retained
    /// more than it.
    TruncationLimitContradicted {
        /// The channel whose evidence is inconsistent.
        channel: StreamChannel,
    },
    /// A cleanup disposition contradicts the elected terminal cause.
    ///
    /// `TerminalDisposition::elect` ranks cleanup failure above a completed
    /// exit and above a signal, so a failed cleanup cannot accompany either;
    /// and a `CleanupFailed` terminal cause must carry a failed cleanup.
    CleanupContradictsDisposition,
    /// A completed child exit was paired with a failed cleanup.
    ///
    /// The terminal precedence in [`TerminalDisposition::elect`] puts cleanup
    /// failure above a completed exit, so this pairing would let a run whose
    /// cleanup failed report an ordinary success.
    CompletedExitWithFailedCleanup,
}

impl std::fmt::Display for ResultInconsistency {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StreamChannelMismatch { expected, found } => {
                write!(f, "expected {expected:?} evidence but found {found:?}")
            }
            Self::CompleteEvidenceCountMismatch { channel } => {
                write!(f, "{channel:?} evidence claims completeness with a mismatched byte count")
            }
            Self::CompleteEvidenceFingerprintMismatch { channel } => {
                write!(f, "{channel:?} evidence claims completeness with a mismatched fingerprint")
            }
            Self::RetainedExceedsObserved { channel } => {
                write!(f, "{channel:?} evidence retains more bytes than it observed")
            }
            Self::TruncationLimitContradicted { channel } => {
                write!(f, "{channel:?} evidence contradicts the limit it says stopped it")
            }
            Self::CleanupContradictsDisposition => {
                f.write_str("the cleanup disposition contradicts the elected terminal cause")
            }
            Self::CompletedExitWithFailedCleanup => {
                f.write_str("a completed exit cannot be paired with a failed cleanup")
            }
        }
    }
}

impl std::error::Error for ResultInconsistency {}

/// Attach the limitations a result's own components imply.
///
/// Shared by every constructor so that no assembly path can publish a result
/// whose limitations disagree with its predicates.
fn derive_limitations(
    limitations: &mut Vec<Limitation>,
    disposition: &TerminalDisposition,
    cleanup: CleanupDisposition,
    tree: TreeDisposition,
    streams: Option<(&StreamEvidence, &StreamEvidence)>,
    backend: &BackendIdentity,
) {
    limitations.push(Limitation::NoIsolationClaimed);
    if backend.evidence_class() == EvidenceClass::Fake {
        limitations.push(Limitation::FakeEvidenceOnly);
    }
    match cleanup {
        CleanupDisposition::Failed => limitations.push(Limitation::CleanupFailed),
        CleanupDisposition::NotObserved => limitations.push(Limitation::CleanupNotObserved),
        CleanupDisposition::Completed | CleanupDisposition::NotRequired => {}
    }
    if tree == TreeDisposition::ImmediateChildOnly || tree == TreeDisposition::Unknown {
        limitations.push(Limitation::DescendantsUnaccounted);
    }
    let child_settled = matches!(
        disposition,
        TerminalDisposition::CompletedExit { .. } | TerminalDisposition::Signaled { .. }
    );
    let streams_complete = streams.is_some_and(|(stdout, stderr)| {
        stdout.truncation().is_complete() && stderr.truncation().is_complete()
    });
    // Kept in step with `claims_complete_output` deliberately: a result whose
    // predicate says the output is partial must say so in its limitations too.
    if !streams_complete || !child_settled {
        limitations.push(Limitation::OutputIncomplete);
    }
    if let Some((stdout, stderr)) = streams
        && (stdout.decoded_view() == DecodedViewLimitation::LossyUtf8
            || stderr.decoded_view() == DecodedViewLimitation::LossyUtf8)
    {
        limitations.push(Limitation::DecodedViewLossy);
    }
    limitations.sort();
    limitations.dedup();
}

fn check_stream(
    evidence: &StreamEvidence,
    expected: StreamChannel,
) -> Result<(), ResultInconsistency> {
    if evidence.channel() != expected {
        return Err(ResultInconsistency::StreamChannelMismatch {
            expected,
            found: evidence.channel(),
        });
    }
    if evidence.retained().len() as u64 > evidence.observed_bytes() {
        return Err(ResultInconsistency::RetainedExceedsObserved { channel: expected });
    }
    match evidence.truncation() {
        TruncationState::Complete => {
            if evidence.observed_bytes() != evidence.retained().len() as u64 {
                return Err(ResultInconsistency::CompleteEvidenceCountMismatch {
                    channel: expected,
                });
            }
            if evidence.observed_fingerprint() != ContentFingerprint::of(evidence.retained()) {
                return Err(ResultInconsistency::CompleteEvidenceFingerprintMismatch {
                    channel: expected,
                });
            }
        }
        TruncationState::ObservationTruncated { limit_bytes } => {
            // Observation stopped at the limit, so at least that much was seen.
            if evidence.observed_bytes() < limit_bytes {
                return Err(ResultInconsistency::TruncationLimitContradicted { channel: expected });
            }
        }
        TruncationState::RetentionTruncated { limit_bytes } => {
            // Retention stopped at the limit, so no more than that was kept.
            if evidence.retained().len() as u64 > limit_bytes {
                return Err(ResultInconsistency::TruncationLimitContradicted { channel: expected });
            }
        }
    }
    Ok(())
}

/// The terminal truth about one start attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessResult {
    schema_version: SchemaVersion,
    plan_id: PlanId,
    plan_fingerprint: PlanFingerprint,
    run_id: RunId,
    disposition: TerminalDisposition,
    stdout: StreamEvidence,
    stderr: StreamEvidence,
    cleanup: CleanupDisposition,
    tree: TreeDisposition,
    backend: BackendIdentity,
    work: WorkMetadata,
    limitations: Vec<Limitation>,
}

impl ProcessResult {
    /// Assemble a result.
    ///
    /// Limitations implied by the evidence are added automatically so that a
    /// backend cannot quietly omit them; callers may add more.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        plan_id: PlanId,
        plan_fingerprint: PlanFingerprint,
        run_id: RunId,
        disposition: TerminalDisposition,
        stdout: StreamEvidence,
        stderr: StreamEvidence,
        cleanup: CleanupDisposition,
        tree: TreeDisposition,
        backend: BackendIdentity,
        work: WorkMetadata,
        mut limitations: Vec<Limitation>,
    ) -> Result<Self, ResultInconsistency> {
        check_stream(&stdout, StreamChannel::Stdout)?;
        check_stream(&stderr, StreamChannel::Stderr)?;
        if disposition.is_completed_exit() && cleanup == CleanupDisposition::Failed {
            return Err(ResultInconsistency::CompletedExitWithFailedCleanup);
        }
        // A signal is elected only when no cleanup failure was recorded, and a
        // `CleanupFailed` cause exists only because cleanup failed. Both
        // directions follow from the precedence rule, so both are enforced.
        if matches!(disposition, TerminalDisposition::Signaled { .. })
            && cleanup == CleanupDisposition::Failed
        {
            return Err(ResultInconsistency::CleanupContradictsDisposition);
        }
        if disposition == TerminalDisposition::CleanupFailed
            && cleanup != CleanupDisposition::Failed
        {
            return Err(ResultInconsistency::CleanupContradictsDisposition);
        }
        limitations.push(Limitation::NoIsolationClaimed);
        if backend.evidence_class() == EvidenceClass::Fake {
            limitations.push(Limitation::FakeEvidenceOnly);
        }
        match cleanup {
            CleanupDisposition::Failed => limitations.push(Limitation::CleanupFailed),
            CleanupDisposition::NotObserved => limitations.push(Limitation::CleanupNotObserved),
            CleanupDisposition::Completed | CleanupDisposition::NotRequired => {}
        }
        if tree == TreeDisposition::ImmediateChildOnly || tree == TreeDisposition::Unknown {
            limitations.push(Limitation::DescendantsUnaccounted);
        }
        let child_settled = matches!(
            disposition,
            TerminalDisposition::CompletedExit { .. } | TerminalDisposition::Signaled { .. }
        );
        if !stdout.truncation().is_complete()
            || !stderr.truncation().is_complete()
            || !child_settled
        {
            // Kept in step with `claims_complete_output` deliberately: a
            // result whose predicate says the output is partial must say so
            // in its limitations too.
            limitations.push(Limitation::OutputIncomplete);
        }
        if stdout.decoded_view() == DecodedViewLimitation::LossyUtf8
            || stderr.decoded_view() == DecodedViewLimitation::LossyUtf8
        {
            limitations.push(Limitation::DecodedViewLossy);
        }
        limitations.sort();
        limitations.dedup();
        Ok(Self {
            schema_version: super::PROCESS_DOMAIN_SCHEMA_VERSION,
            plan_id,
            plan_fingerprint,
            run_id,
            disposition,
            stdout,
            stderr,
            cleanup,
            tree,
            backend,
            work,
            limitations,
        })
    }

    /// Assemble a supervisor-failure result that cannot itself be inconsistent.
    ///
    /// A backend needs a result it can always produce when assembling the real
    /// one fails; empty evidence with no cleanup requirement is coherent by
    /// construction.
    pub fn supervisor_failure(
        plan_id: PlanId,
        plan_fingerprint: PlanFingerprint,
        run_id: RunId,
        backend: BackendIdentity,
    ) -> Self {
        // A supervisor that failed proves nothing about cleanup, and a failure
        // can happen after the child started. Claiming cleanup was
        // unnecessary would be a stronger statement than the situation
        // supports, so the conservative pair is recorded instead.
        let cleanup = CleanupDisposition::NotObserved;
        let tree = TreeDisposition::Unknown;
        let disposition = TerminalDisposition::SupervisorFailed;
        let mut limitations = Vec::new();
        derive_limitations(&mut limitations, &disposition, cleanup, tree, None, &backend);
        Self {
            schema_version: super::PROCESS_DOMAIN_SCHEMA_VERSION,
            plan_id,
            plan_fingerprint,
            run_id,
            disposition,
            stdout: StreamEvidence::empty(StreamChannel::Stdout),
            stderr: StreamEvidence::empty(StreamChannel::Stderr),
            cleanup,
            tree,
            backend,
            work: WorkMetadata::default(),
            limitations,
        }
    }

    /// The domain schema version.
    pub fn schema_version(&self) -> SchemaVersion {
        self.schema_version
    }

    /// The plan this result settles.
    pub fn plan_id(&self) -> &PlanId {
        &self.plan_id
    }

    /// The fingerprint of the plan this result settles.
    pub fn plan_fingerprint(&self) -> PlanFingerprint {
        self.plan_fingerprint
    }

    /// The start attempt this result settles.
    pub fn run_id(&self) -> &RunId {
        &self.run_id
    }

    /// How the run ended.
    pub fn disposition(&self) -> &TerminalDisposition {
        &self.disposition
    }

    /// Evidence for the child's standard output.
    pub fn stdout(&self) -> &StreamEvidence {
        &self.stdout
    }

    /// Evidence for the child's standard error.
    pub fn stderr(&self) -> &StreamEvidence {
        &self.stderr
    }

    /// Whether cleanup was proven.
    pub fn cleanup(&self) -> CleanupDisposition {
        self.cleanup
    }

    /// What was done about descendants.
    pub fn tree(&self) -> TreeDisposition {
        self.tree
    }

    /// The backend that produced the result.
    pub fn backend(&self) -> &BackendIdentity {
        &self.backend
    }

    /// Bounded work metadata.
    pub fn work(&self) -> WorkMetadata {
        self.work
    }

    /// The result's explicit non-claims.
    pub fn limitations(&self) -> &[Limitation] {
        &self.limitations
    }

    /// Whether the child's own settlement was established.
    ///
    /// Only a run whose child settled on its own terms can say anything about
    /// the whole of what that child produced. A supervisor failure, an
    /// unproven outcome, a refusal before start, or a run the supervisor cut
    /// short cannot.
    fn child_settlement_established(&self) -> bool {
        matches!(
            self.disposition,
            TerminalDisposition::CompletedExit { .. } | TerminalDisposition::Signaled { .. }
        )
    }

    /// Whether the run was an ordinary zero exit.
    pub fn is_ordinary_success(&self) -> bool {
        self.disposition.is_ordinary_success()
    }

    /// Whether the retained output is the complete output of the child.
    ///
    /// False whenever either channel was truncated or the run ended for
    /// exceeding an output budget: a bounded capture is never a complete one.
    pub fn claims_complete_output(&self) -> bool {
        self.stdout.truncation().is_complete()
            && self.stderr.truncation().is_complete()
            && self.child_settlement_established()
    }

    /// Whether this result can support a process-tree cleanup claim.
    pub fn claims_tree_cleanup(&self) -> bool {
        self.tree.proves_tree_cleanup() && self.cleanup.is_proven()
    }

    /// Whether this result can support a claim about real process behavior.
    pub fn is_executed_evidence(&self) -> bool {
        self.backend.evidence_class().is_executed()
    }
}
