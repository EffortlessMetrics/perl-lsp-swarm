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

/// Which of a channel's two capture bounds stopped it, and where.
///
/// [`CaptureBudget`](super::CaptureBudget) carries two independent limits —
/// how much the supervisor reads, and how much of what it read it keeps — so a
/// channel can reach either, both, or neither. They are therefore recorded as
/// two separate facts rather than as one choice between them.
///
/// Collapsing them into a single choice would make an ordinary run
/// unrepresentable. `CaptureBudget::observe_only(1024)` observes up to 1024
/// bytes and retains none of them; a child that writes more than that reaches
/// *both* bounds, and naming only one would assert that the other was complete
/// when it was not. A contract that cannot say what happened is worse than a
/// verbose one.
///
/// # Obligation this places on a backend
///
/// Each bound recorded here names the exact byte count at which that bound
/// stopped, and [`ProcessResult::new`] enforces it. Reporting an observation
/// bound asserts that reading stopped *at* it, so a backend must clamp each
/// read to what is left of the budget. One that reads fixed-size buffers and
/// stops after the read that crosses the limit will have observed *more* than
/// it claims, and its result is refused as
/// [`ResultInconsistency::TruncationLimitContradicted`] — correctly, because it
/// did not stop where it says it did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TruncationState {
    observation_limit_bytes: Option<u64>,
    retention_limit_bytes: Option<u64>,
}

impl TruncationState {
    /// Neither bound was reached: everything observed was retained.
    pub const fn complete() -> Self {
        Self { observation_limit_bytes: None, retention_limit_bytes: None }
    }

    /// Observation stopped at its limit; everything observed was retained.
    pub const fn observation_truncated(limit_bytes: u64) -> Self {
        Self { observation_limit_bytes: Some(limit_bytes), retention_limit_bytes: None }
    }

    /// Retention stopped at its limit; everything the child wrote was observed.
    pub const fn retention_truncated(limit_bytes: u64) -> Self {
        Self { observation_limit_bytes: None, retention_limit_bytes: Some(limit_bytes) }
    }

    /// Both bounds were reached, each at its own limit.
    pub const fn observation_and_retention_truncated(
        observation_limit_bytes: u64,
        retention_limit_bytes: u64,
    ) -> Self {
        Self {
            observation_limit_bytes: Some(observation_limit_bytes),
            retention_limit_bytes: Some(retention_limit_bytes),
        }
    }

    /// The byte count at which observation stopped, if it was bounded.
    pub fn observation_limit(self) -> Option<u64> {
        self.observation_limit_bytes
    }

    /// The byte count at which retention stopped, if it was bounded.
    pub fn retention_limit(self) -> Option<u64> {
        self.retention_limit_bytes
    }

    /// Whether observation reached its bound, so more output may have existed.
    pub fn observation_was_truncated(self) -> bool {
        self.observation_limit_bytes.is_some()
    }

    /// Whether retention reached its bound, so some observed bytes were dropped.
    pub fn retention_was_truncated(self) -> bool {
        self.retention_limit_bytes.is_some()
    }

    /// Whether the retained bytes are the complete channel content.
    ///
    /// True only when *neither* bound was reached. A channel that hit either
    /// one is not complete, so no single-axis check can stand in for this.
    pub fn is_complete(self) -> bool {
        self.observation_limit_bytes.is_none() && self.retention_limit_bytes.is_none()
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
    observed_fingerprint: Option<ContentFingerprint>,
    retained: Vec<u8>,
    truncation: TruncationState,
    decoded_view: DecodedViewLimitation,
}

impl StreamEvidence {
    /// Record evidence for a channel.
    pub fn new(
        channel: StreamChannel,
        observed_bytes: u64,
        observed_fingerprint: Option<ContentFingerprint>,
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
        Self::new(channel, observed, Some(fingerprint), bytes, TruncationState::complete())
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

    /// The identity of the observed content, when the supervisor established it.
    ///
    /// `None` means the count is known and the identity is not — the ordinary
    /// state after a supervisor fails mid-stream, having read bytes it can no
    /// longer identify. It is deliberately distinct from a fingerprint of
    /// nothing, which would assert an identity the supervisor never computed.
    pub fn observed_fingerprint(&self) -> Option<ContentFingerprint> {
        self.observed_fingerprint
    }

    /// Evidence that a channel was read without its content being identified.
    ///
    /// The honest shape for a supervisor that failed after reading: the byte
    /// count the ledger admitted is a fact, the content is not retained, and
    /// no identity is claimed for it.
    pub fn observed_but_unidentified(channel: StreamChannel, observed_bytes: u64) -> Self {
        if observed_bytes == 0 {
            // Nothing was read, so the content is known: it is empty. Claiming
            // an unknown identity for nothing would be the false-negative this
            // constructor exists to avoid, in the other direction.
            return Self::empty(channel);
        }
        Self::new(
            channel,
            observed_bytes,
            None,
            Vec::new(),
            TruncationState::retention_truncated(0),
        )
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
        let candidate = Self::elect_by_precedence(control, settlement);
        if !candidate.contradicted_by(settlement) {
            return candidate;
        }
        // The elected cause is disproved by the child's own account. A cleanup
        // failure is a separate observation that the contradiction does not
        // touch, and the precedence order ranks it above an unproven outcome —
        // so falling straight to `NotProven` would discard a known failure in
        // favour of uncertainty. It is taken only when it is not itself
        // contradicted; the election never falls further than that, because
        // the settlement is half of what is being contradicted.
        if control.cleanup_failed && !Self::CleanupFailed.contradicted_by(settlement) {
            return Self::CleanupFailed;
        }
        Self::NotProven
    }

    /// The cause the fixed precedence order selects, before any coherence
    /// check against the child's own account.
    fn elect_by_precedence(control: ControlState, settlement: ObservedSettlement) -> Self {
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

    /// Whether the child's own account disproves this cause.
    ///
    /// The control plane and the settlement are two independent witnesses.
    /// When they disagree, electing the control plane's cause would publish a
    /// claim the other half of the evidence contradicts, so the election fails
    /// closed instead.
    ///
    /// One rule for both directions, keyed on the two predicates that already
    /// classify every cause, rather than a clause per pair. Stating it for the
    /// cancellation pair alone left `OutputLimitExceeded` and `CleanupFailed`
    /// electable beside a settlement saying nothing ever ran — an output
    /// budget reached by a child that never existed, and a cleanup failure the
    /// result assembler already refuses to pair with a pre-start outcome.
    ///
    /// [`ObservedSettlement::NotObserved`] contradicts nothing: not having seen
    /// how the child settled is consistent with every cause.
    fn contradicted_by(&self, settlement: ObservedSettlement) -> bool {
        match settlement {
            ObservedSettlement::NotStarted => self.requires_a_started_child(),
            ObservedSettlement::Exited { .. } | ObservedSettlement::Signaled { .. } => {
                self.asserts_no_child_started()
            }
            ObservedSettlement::NotObserved => false,
        }
    }

    /// Whether the child ran to completion under its own control.
    pub fn is_completed_exit(&self) -> bool {
        matches!(self, Self::CompletedExit { .. })
    }

    /// Whether [`Self::elect`] ranks this cause *below* a cleanup failure.
    ///
    /// A cause below that rank cannot be the elected outcome of a run whose
    /// cleanup demonstrably failed: `CleanupFailed` would have won. Pairing
    /// one with [`CleanupDisposition::Failed`] therefore publishes a weaker
    /// claim than the evidence supports — an ordinary success, a signal, or an
    /// unproven outcome standing in for a known failure.
    ///
    /// Written as an exhaustive `match` rather than a `matches!`, so a new
    /// terminal cause has to be placed in the precedence order rather than
    /// silently defaulting to "above cleanup failure".
    fn ranks_below_cleanup_failure(&self) -> bool {
        match self {
            // Ranks 6, 7, and 8: the child's own settlement, and the absence
            // of any established outcome.
            Self::Signaled { .. } | Self::CompletedExit { .. } | Self::NotProven => true,
            // Ranks 1 to 4: control-plane causes that describe *why* the run
            // ended. Each legitimately accompanies a failed cleanup, which the
            // result still records in its own field.
            Self::SupervisorFailed
            | Self::OutputLimitExceeded
            | Self::TimedOut
            | Self::CancelledRunning(_)
            | Self::CancelledBeforeStart(_) => false,
            // Rank 5 itself, and the pre-start causes, which are governed by
            // `asserts_no_child_started` instead.
            Self::CleanupFailed
            | Self::SpawnRejected(_)
            | Self::SpawnFailed { .. }
            | Self::UnsupportedBackend
            | Self::StaleOrUnauthorized(_) => false,
        }
    }

    /// Whether this cause establishes that the child settled on its own terms.
    ///
    /// Only an exit or a signal is the child's own account of how it ended.
    /// Every other cause describes the control plane or the supervisor.
    pub fn establishes_child_settlement(&self) -> bool {
        matches!(self, Self::CompletedExit { .. } | Self::Signaled { .. })
    }

    /// Whether this cause could only have arisen after a child started.
    ///
    /// Broader than [`Self::establishes_child_settlement`], and deliberately
    /// so: an exit or a signal is the child's *own account* of how it ended,
    /// while a running cancellation and an output-limit termination prove only
    /// that something was running to cancel or to produce the output. Both
    /// facts are needed, and conflating them is what let
    /// `CancelledRunning` sit beside a cleanup disposition meaning "nothing
    /// started, so nothing needed cleaning up".
    ///
    /// `CleanupFailed` is present: `ProcessResult::new` already refuses a
    /// pre-start outcome carrying [`CleanupDisposition::Failed`], so by the
    /// contract's own rule a cleanup that failed had a child to clean up
    /// after.
    ///
    /// `TimedOut`, `SupervisorFailed`, and `NotProven` are absent: a deadline
    /// can elapse while a spawn is still hanging, and the other two say
    /// nothing about whether a child exists.
    ///
    /// Exhaustive, so a new terminal cause is classified rather than admitted
    /// by default.
    fn requires_a_started_child(&self) -> bool {
        match self {
            Self::CompletedExit { .. }
            | Self::Signaled { .. }
            | Self::CancelledRunning(_)
            | Self::OutputLimitExceeded
            | Self::CleanupFailed => true,
            Self::TimedOut
            | Self::SupervisorFailed
            | Self::NotProven
            | Self::SpawnRejected(_)
            | Self::SpawnFailed { .. }
            | Self::CancelledBeforeStart(_)
            | Self::UnsupportedBackend
            | Self::StaleOrUnauthorized(_) => false,
        }
    }

    /// Whether this cause asserts that no child process ever ran.
    ///
    /// These are positive claims that the start did not happen, not statements
    /// that the outcome is unknown. `SupervisorFailed` and `NotProven` are
    /// deliberately absent: they can occur after a child has started, so they
    /// may legitimately carry partial output and an unobserved cleanup.
    pub fn asserts_no_child_started(&self) -> bool {
        matches!(
            self,
            Self::SpawnRejected(_)
                | Self::SpawnFailed { .. }
                | Self::CancelledBeforeStart(_)
                | Self::UnsupportedBackend
                | Self::StaleOrUnauthorized(_)
        )
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
    /// Stream evidence the backend supplied could not describe one coherent
    /// run and was dropped.
    ///
    /// Only [`ProcessResult::supervisor_failure`] can produce this: it is
    /// infallible by construction, so incoherent evidence is replaced rather
    /// than refused. The replacement is empty, and empty evidence is a
    /// positive claim that nothing was seen — which for a backend that
    /// reported *something*, however malformed, is not true. This limitation
    /// is what keeps the two apart, so a supervisor defect does not read as a
    /// child that produced no output.
    StreamEvidenceDiscarded,
}

/// Bounded work metadata about a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WorkMetadata {
    /// Wall-clock time from start attempt to settlement.
    pub wall_time: Duration,
    /// How many events the run admitted to its ledger.
    ///
    /// This counts what the run produced, including a terminal event the
    /// supervisor synthesized — not what any one consumer happened to retrieve
    /// before calling `wait`. A caller that never polls still sees the full
    /// count.
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
    /// Retained content was available and no identity was published for it.
    ///
    /// A missing `observed_fingerprint` means *the supervisor could not
    /// establish the identity*. When retention was unbounded the retained
    /// bytes are the whole of what was observed, so the identity is available
    /// and withholding it would let a result claim complete output with no
    /// content identity at all.
    AvailableContentLacksIdentity {
        /// The channel whose content identity is missing.
        channel: StreamChannel,
    },
    /// An output-limit outcome carried no stream that reached a bound.
    ///
    /// `OutputLimitExceeded` says a capture budget stopped the run, so at
    /// least one channel has to name the bound it hit. Two complete streams
    /// contradict the cause outright.
    OutputLimitWithoutATruncatedStream,
    /// A child that demonstrably ran carried no-child cleanup evidence.
    ///
    /// [`CleanupDisposition::NotRequired`] means cleanup was unnecessary
    /// *because nothing was started*. Pairing it with an exit or a signal
    /// asserts both that the child ran and that it never did. The inverse of
    /// [`Self::PreStartOutcomeCarriesChildEvidence`].
    SettledChildCarriesNoChildCleanup,
    /// A no-child-started outcome carried evidence only a child could produce.
    ///
    /// `SpawnRejected`, `SpawnFailed`, `CancelledBeforeStart`,
    /// `UnsupportedBackend`, and `StaleOrUnauthorized` all assert that no
    /// process ever ran. Output bytes, an observed cleanup, or a terminated
    /// process group would each have to have come from a child that, by the
    /// disposition's own claim, never existed.
    PreStartOutcomeCarriesChildEvidence,
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
            Self::AvailableContentLacksIdentity { channel } => {
                write!(f, "{channel:?} retained its content but published no identity for it")
            }
            Self::OutputLimitWithoutATruncatedStream => {
                f.write_str("an output-limit outcome carried no stream that reached its bound")
            }
            Self::SettledChildCarriesNoChildCleanup => {
                f.write_str("a child that ran cannot carry cleanup evidence saying none started")
            }
            Self::PreStartOutcomeCarriesChildEvidence => {
                f.write_str("an outcome in which no child started cannot carry child evidence")
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
    discarded_stream_evidence: bool,
) {
    limitations.push(Limitation::NoIsolationClaimed);
    // A parameter rather than something inferred from the streams: once the
    // substitution has happened the empty evidence is indistinguishable from
    // evidence that was empty to begin with, which is the whole point of
    // recording it. Only the caller that performed the repair knows.
    if discarded_stream_evidence {
        limitations.push(Limitation::StreamEvidenceDiscarded);
    }
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
    let truncation = evidence.truncation();
    let retained_len = evidence.retained().len() as u64;

    // The two bounds are checked independently, because they are independent
    // facts. Checking them as one choice is what previously made a channel
    // that reached both unable to describe itself honestly.
    match truncation.observation_limit() {
        // Observation stopped *at* the limit, so that is exactly how much was
        // seen. Reading past the point you say you stopped at contradicts the
        // stop point as surely as stopping short of it.
        Some(limit_bytes) if evidence.observed_bytes() != limit_bytes => {
            return Err(ResultInconsistency::TruncationLimitContradicted { channel: expected });
        }
        _ => {}
    }
    match truncation.retention_limit() {
        // Retention stopped *at* the limit, so exactly that much was kept:
        // keeping less contradicts the stop point as surely as keeping more
        // does. And retention can only have been truncated if there was more
        // to keep than the limit allowed.
        Some(limit_bytes)
            if retained_len != limit_bytes || evidence.observed_bytes() <= limit_bytes =>
        {
            return Err(ResultInconsistency::TruncationLimitContradicted { channel: expected });
        }
        // Retention was not bounded, so everything observed had to be kept —
        // including when observation itself stopped early.
        None if retained_len != evidence.observed_bytes() => {
            return Err(ResultInconsistency::CompleteEvidenceCountMismatch { channel: expected });
        }
        _ => {}
    }
    // The fingerprint identifies the content actually observed, so it can be
    // checked whenever the retained bytes are the whole of it — which is
    // exactly when retention was unbounded, established by the branch above.
    // Observation stopping early does not weaken that: the bytes it did see
    // were all kept, so their identity must match.
    if truncation.retention_limit().is_none() {
        // Retention was unbounded, so the retained bytes are the whole of what
        // was observed and the identity is available. `None` here would be a
        // supervisor declining to publish an identity it demonstrably has.
        match evidence.observed_fingerprint() {
            None => {
                return Err(ResultInconsistency::AvailableContentLacksIdentity {
                    channel: expected,
                });
            }
            Some(fingerprint) if fingerprint != ContentFingerprint::of(evidence.retained()) => {
                return Err(ResultInconsistency::CompleteEvidenceFingerprintMismatch {
                    channel: expected,
                });
            }
            Some(_) => {}
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
        // One rule for every cause the precedence order ranks below a cleanup
        // failure, rather than one clause per cause: stating it for the exit
        // and the signal but not for `NotProven` let a *known* failure be
        // published as an outcome nothing established, which is strictly less
        // than the evidence supports.
        if cleanup == CleanupDisposition::Failed && disposition.ranks_below_cleanup_failure() {
            // The exit keeps its own named variant: it is the case where the
            // weaker claim would read as an ordinary success.
            return Err(if disposition.is_completed_exit() {
                ResultInconsistency::CompletedExitWithFailedCleanup
            } else {
                ResultInconsistency::CleanupContradictsDisposition
            });
        }
        // The other direction: a `CleanupFailed` cause exists only because
        // cleanup failed.
        if disposition == TerminalDisposition::CleanupFailed
            && cleanup != CleanupDisposition::Failed
        {
            return Err(ResultInconsistency::CleanupContradictsDisposition);
        }
        // An outcome that states no child ever started cannot also carry the
        // things only a running child produces: bytes on a stream, a cleanup
        // someone observed completing or failing, or a terminated process
        // group. `NotRequired`/`Unknown` are the coherent pair here, and
        // `NotObserved` cleanup is allowed because a refused start genuinely
        // observed nothing.
        // The cause names a capture budget as the reason the run ended, so
        // some channel must record the bound it reached. Both streams claiming
        // completeness contradicts the cause directly.
        // `is_complete()` rather than the observation bound alone: a run
        // terminated for reaching a *retention* budget has both channels
        // retention-truncated and neither observation-truncated, and that is a
        // coherent outcome. Keying on one axis made it unrepresentable — the
        // over-rejection this rule's own documentation already ruled out by
        // saying two *complete* streams contradict the cause.
        if disposition == TerminalDisposition::OutputLimitExceeded
            && stdout.truncation().is_complete()
            && stderr.truncation().is_complete()
        {
            return Err(ResultInconsistency::OutputLimitWithoutATruncatedStream);
        }
        // The inverse of the pre-start rule below: `NotRequired` cleanup means
        // nothing was started, so a disposition that proves the child ran
        // cannot carry it. `TreeDisposition::NotRequired` is different and
        // stays legal — a child that exited on its own needs no termination.
        //
        // Keyed on "a child must have started", not on "the child gave its own
        // account". The narrower predicate covered the exit and the signal and
        // left `CancelledRunning` and `OutputLimitExceeded` admissible beside
        // cleanup evidence asserting nothing ever ran — you cannot cancel a
        // running child, or terminate one for its output, without one.
        if disposition.requires_a_started_child() && cleanup == CleanupDisposition::NotRequired {
            return Err(ResultInconsistency::SettledChildCarriesNoChildCleanup);
        }
        if disposition.asserts_no_child_started() {
            // Two facts, not one. A byte count above zero is the obvious
            // evidence of a child, but a truncation state is evidence too: it
            // says a capture bound was *reached*, which only output can do.
            // `observation_truncated(0)` carries a zero count and still makes
            // that claim, so keying on the count alone let a refused start
            // report a bound its non-existent child had hit.
            let produced_output = stdout.observed_bytes() > 0
                || stderr.observed_bytes() > 0
                || !stdout.truncation().is_complete()
                || !stderr.truncation().is_complete();
            let cleanup_claims_a_child =
                matches!(cleanup, CleanupDisposition::Completed | CleanupDisposition::Failed);
            let tree_claims_a_child = matches!(
                tree,
                TreeDisposition::GroupTerminated | TreeDisposition::ImmediateChildOnly
            );
            if produced_output || cleanup_claims_a_child || tree_claims_a_child {
                return Err(ResultInconsistency::PreStartOutcomeCarriesChildEvidence);
            }
        }
        derive_limitations(
            &mut limitations,
            &disposition,
            cleanup,
            tree,
            Some((&stdout, &stderr)),
            &backend,
            // This constructor refuses incoherent evidence rather than
            // repairing it, so it never has a repair to record.
            false,
        );
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
    /// The stream evidence is a parameter rather than empty, because a
    /// supervisor can fail *after* reading. Hard-coding empty streams would
    /// report zero observed bytes to a consumer that had already been handed
    /// chunk events saying otherwise — a positive claim that nothing was seen,
    /// which is not the same as declining to claim. Callers that observed
    /// nothing pass [`StreamEvidence::empty`]; callers that read bytes they
    /// can no longer identify pass
    /// [`StreamEvidence::observed_but_unidentified`].
    ///
    /// A backend needs a result it can always produce when assembling the real
    /// one fails; empty evidence with no cleanup requirement is coherent by
    /// construction.
    pub fn supervisor_failure(
        plan_id: PlanId,
        plan_fingerprint: PlanFingerprint,
        run_id: RunId,
        backend: BackendIdentity,
        work: WorkMetadata,
        stdout: StreamEvidence,
        stderr: StreamEvidence,
    ) -> Self {
        // A supervisor that failed proves nothing about cleanup, and a failure
        // can happen after the child started. Claiming cleanup was
        // unnecessary would be a stronger statement than the situation
        // supports, so the conservative pair is recorded instead.
        let cleanup = CleanupDisposition::NotObserved;
        let tree = TreeDisposition::Unknown;
        let disposition = TerminalDisposition::SupervisorFailed;
        // This constructor is infallible by design — a backend needs a result
        // it can always produce — but that must not become a way to publish
        // evidence nothing checked. Incoherent evidence is dropped rather than
        // propagated: a fallback that republished a swapped channel or an
        // impossible count would be worse than one that says less.
        let mut discarded_evidence = false;
        let stdout = match check_stream(&stdout, StreamChannel::Stdout) {
            Ok(()) => stdout,
            Err(_) => {
                discarded_evidence = true;
                StreamEvidence::empty(StreamChannel::Stdout)
            }
        };
        let stderr = match check_stream(&stderr, StreamChannel::Stderr) {
            Ok(()) => stderr,
            Err(_) => {
                discarded_evidence = true;
                StreamEvidence::empty(StreamChannel::Stderr)
            }
        };
        // Dropping the evidence silently would leave a supervisor defect
        // wearing the shape of a child that produced nothing: the result would
        // report zero observed bytes as an observation rather than as the
        // repair it is. The substitution is therefore recorded, not just made.
        let mut limitations = Vec::new();
        // Derived from the evidence actually stored, so the limitations cannot
        // describe streams this result does not carry.
        derive_limitations(
            &mut limitations,
            &disposition,
            cleanup,
            tree,
            Some((&stdout, &stderr)),
            &backend,
            discarded_evidence,
        );
        Self {
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
        self.disposition.establishes_child_settlement()
    }

    /// Whether this outcome asserts that no child process ever ran.
    ///
    /// Distinct from "the child's fate is unknown": these dispositions each
    /// state positively that the start did not happen, so nothing a child
    /// could have produced may accompany them.
    pub fn asserts_no_child_started(&self) -> bool {
        self.disposition.asserts_no_child_started()
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
