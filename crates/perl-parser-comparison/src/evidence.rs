//! Generic evidence types for parser comparison.
//!
//! Subject execution and scored correctness are deliberately separate. A
//! parser accepting source cleanly is an execution observation; it becomes a
//! correctness result only when an applicable observer compares it with a
//! reviewed expectation.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

/// Stable role of a parser subject in the comparison programme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum SubjectRole {
    /// Current exact-pinned upstream Tree-sitter Perl subject.
    CurrentUpstreamTreeSitter,
    /// Historical vendored C Tree-sitter snapshot.
    HistoricalTreeSitterC,
    /// Experimental Pest/PEG parser.
    ExperimentalPest,
    /// Native recursive-descent parser.
    NativeRecursiveDescent,
    /// Tree-sitter-style facade over the native parser.
    NativeTreeSitterFacade,
}

/// Terminal disposition of executing one parser subject.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum ExecutionDisposition {
    /// Subject returned a result without its declared recovery/error signal.
    AcceptedClean,
    /// Subject returned a result while reporting recovery or diagnostics.
    AcceptedRecovered,
    /// Subject rejected the source as parser input.
    Rejected,
    /// Subject does not support the requested operation.
    Unsupported,
    /// Subject panicked, aborted, or otherwise crashed.
    Crashed,
    /// Subject exceeded its execution deadline.
    TimedOut,
    /// Subject could not be configured or initialized.
    SetupFailed,
    /// Required instrumentation is unavailable for this subject.
    InstrumentUnavailable,
    /// Required instrumentation ran but failed.
    InstrumentFailed,
    /// Subject was planned but did not run.
    NotRun,
}

/// Observation plane that an independent observer may score.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum ObservationPlane {
    /// Tree or AST structure and semantic roles.
    Structure,
    /// Source ranges, positions, and reconstruction geometry.
    SourceGeometry,
    /// Recovery state, diagnostics, and following-code salvage.
    Recovery,
    /// Ownership and ordering of deferred bodies such as heredocs.
    BodyOwnership,
    /// Equivalence of fresh and edited final-state observations.
    IncrementalFinalState,
    /// Query, highlight, or capture projection behavior.
    QueryOrHighlight,
    /// Registered bounded plane not yet promoted to a dedicated variant.
    Registered(String),
}

/// Whether a subject can expose one observation plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum ObservationAvailability {
    /// Plane is observable through the current adapter.
    Observable,
    /// Plane is observable with declared limitations.
    ObservableWithLimitations,
    /// Subject does not support the plane.
    Unsupported,
    /// Adapter or evidence has not yet proved the plane observable.
    NotProven,
    /// Required instrument for the plane is unavailable.
    InstrumentUnavailable,
}

/// State of execution instrumentation for one subject result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum InstrumentState {
    /// Required execution instrumentation was available and completed.
    Available,
    /// Required execution instrumentation was unavailable.
    Unavailable,
    /// Required execution instrumentation failed.
    Failed,
}

/// Bounded summary of diagnostics and recovery observed during execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct DiagnosticSummary {
    diagnostic_count: usize,
    recovery_observed: bool,
    error_node_observed: bool,
}

impl DiagnosticSummary {
    /// Construct a diagnostic/recovery summary.
    pub const fn new(
        diagnostic_count: usize,
        recovery_observed: bool,
        error_node_observed: bool,
    ) -> Self {
        Self {
            diagnostic_count,
            recovery_observed,
            error_node_observed,
        }
    }

    /// Number of diagnostics or equivalent parser findings observed.
    pub const fn diagnostic_count(&self) -> usize {
        self.diagnostic_count
    }

    /// Whether the subject reported recovery behavior.
    pub const fn recovery_observed(&self) -> bool {
        self.recovery_observed
    }

    /// Whether an error node or equivalent structural marker was observed.
    pub const fn error_node_observed(&self) -> bool {
        self.error_node_observed
    }
}

/// Execution evidence for one parser subject before correctness scoring.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SubjectExecution {
    subject: SubjectRole,
    disposition: ExecutionDisposition,
    diagnostics: DiagnosticSummary,
    observations: BTreeMap<ObservationPlane, ObservationAvailability>,
    debug_projection: Option<String>,
    instrument_state: InstrumentState,
    error: Option<String>,
}

impl SubjectExecution {
    pub(crate) fn new(
        subject: SubjectRole,
        disposition: ExecutionDisposition,
        diagnostics: DiagnosticSummary,
        observations: BTreeMap<ObservationPlane, ObservationAvailability>,
        debug_projection: Option<String>,
        instrument_state: InstrumentState,
        error: Option<String>,
    ) -> Self {
        Self {
            subject,
            disposition,
            diagnostics,
            observations,
            debug_projection,
            instrument_state,
            error,
        }
    }

    /// Stable role of the executed parser subject.
    pub const fn subject(&self) -> SubjectRole {
        self.subject
    }

    /// Terminal execution disposition.
    pub const fn disposition(&self) -> ExecutionDisposition {
        self.disposition
    }

    /// Diagnostic and recovery summary.
    pub const fn diagnostics(&self) -> &DiagnosticSummary {
        &self.diagnostics
    }

    /// Deterministically ordered observation capability map.
    pub const fn observations(
        &self,
    ) -> &BTreeMap<ObservationPlane, ObservationAvailability> {
        &self.observations
    }

    /// Capability of one observation plane, when registered.
    pub fn observation(
        &self,
        plane: &ObservationPlane,
    ) -> Option<ObservationAvailability> {
        self.observations.get(plane).copied()
    }

    /// Bounded raw projection retained only for debugging.
    pub fn debug_projection(&self) -> Option<&str> {
        self.debug_projection.as_deref()
    }

    /// Instrument state for this execution.
    pub const fn instrument_state(&self) -> InstrumentState {
        self.instrument_state
    }

    /// Bounded execution/setup error text, when present.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
}

/// Outcome of comparing one observation with one expectation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum ScoredOutcome {
    /// Observed projection matches the reviewed expectation.
    MatchesExpected,
    /// Observed projection differs from the reviewed expectation.
    Mismatch,
    /// Observed projection is plausible but semantically wrong.
    WrongButPlausible,
    /// Expected material is silently absent.
    SilentlyEmpty,
    /// Requested plane cannot be observed for this subject.
    NotObservable,
    /// Case or plane has no reviewed expectation.
    Unscored,
    /// Evidence completed but cannot establish a result.
    Unknown,
    /// Required evidence did not establish the proposition.
    NotProven,
}

impl ScoredOutcome {
    const fn is_scored(self) -> bool {
        matches!(
            self,
            Self::MatchesExpected | Self::Mismatch | Self::WrongButPlausible | Self::SilentlyEmpty
        )
    }
}

/// Validated comparison of one observation against one expectation.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ScoredComparison {
    observer_id: Option<String>,
    plane: ObservationPlane,
    expected_fingerprint: Option<String>,
    actual_fingerprint: Option<String>,
    outcome: ScoredOutcome,
    first_mismatch: Option<String>,
}

impl ScoredComparison {
    /// Construct a scored comparison with an explicit observer and expectation.
    pub fn scored(
        observer_id: impl Into<String>,
        plane: ObservationPlane,
        expected_fingerprint: impl Into<String>,
        actual_fingerprint: impl Into<String>,
        outcome: ScoredOutcome,
        first_mismatch: Option<String>,
    ) -> Result<Self, ComparisonModelError> {
        let observer_id = observer_id.into();
        if observer_id.trim().is_empty() {
            return Err(ComparisonModelError::MissingObserverId);
        }
        if !outcome.is_scored() {
            return Err(ComparisonModelError::NonScoredOutcomeInScoredComparison(outcome));
        }

        let expected_fingerprint = expected_fingerprint.into();
        if expected_fingerprint.trim().is_empty() {
            return Err(ComparisonModelError::MissingExpectedFingerprint);
        }
        let actual_fingerprint = actual_fingerprint.into();
        if outcome == ScoredOutcome::MatchesExpected
            && expected_fingerprint != actual_fingerprint
        {
            return Err(ComparisonModelError::MatchFingerprintMismatch);
        }

        Ok(Self {
            observer_id: Some(observer_id),
            plane,
            expected_fingerprint: Some(expected_fingerprint),
            actual_fingerprint: Some(actual_fingerprint),
            outcome,
            first_mismatch,
        })
    }

    /// Construct an explicit unscored, unknown, or not-proven result.
    pub fn unscored(
        plane: ObservationPlane,
        outcome: ScoredOutcome,
    ) -> Result<Self, ComparisonModelError> {
        if outcome.is_scored() {
            return Err(ComparisonModelError::ScoredOutcomeWithoutObserver(outcome));
        }
        Ok(Self {
            observer_id: None,
            plane,
            expected_fingerprint: None,
            actual_fingerprint: None,
            outcome,
            first_mismatch: None,
        })
    }

    /// Observer identity used to score this comparison.
    pub fn observer_id(&self) -> Option<&str> {
        self.observer_id.as_deref()
    }

    /// Observation plane that was scored or explicitly left unscored.
    pub const fn plane(&self) -> &ObservationPlane {
        &self.plane
    }

    /// Expected reviewed fingerprint, when scored.
    pub fn expected_fingerprint(&self) -> Option<&str> {
        self.expected_fingerprint.as_deref()
    }

    /// Actual observed fingerprint, when scored.
    pub fn actual_fingerprint(&self) -> Option<&str> {
        self.actual_fingerprint.as_deref()
    }

    /// Typed scored or unscored outcome.
    pub const fn outcome(&self) -> ScoredOutcome {
        self.outcome
    }

    /// First mismatching field or bounded comparison plane, when known.
    pub fn first_mismatch(&self) -> Option<&str> {
        self.first_mismatch.as_deref()
    }
}

/// Construction error for the generic comparison model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ComparisonModelError {
    /// A scored comparison omitted its observer identity.
    MissingObserverId,
    /// A scored comparison omitted its reviewed expected fingerprint.
    MissingExpectedFingerprint,
    /// A non-scored outcome was passed to the scored constructor.
    NonScoredOutcomeInScoredComparison(ScoredOutcome),
    /// A scored outcome was passed to the unscored constructor.
    ScoredOutcomeWithoutObserver(ScoredOutcome),
    /// A claimed match carried different expected and actual fingerprints.
    MatchFingerprintMismatch,
}

impl fmt::Display for ComparisonModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingObserverId => write!(f, "scored comparison requires an observer id"),
            Self::MissingExpectedFingerprint => {
                write!(f, "scored comparison requires an expected fingerprint")
            }
            Self::NonScoredOutcomeInScoredComparison(outcome) => write!(
                f,
                "outcome {outcome:?} is not valid for a scored comparison"
            ),
            Self::ScoredOutcomeWithoutObserver(outcome) => write!(
                f,
                "outcome {outcome:?} cannot be constructed without an observer"
            ),
            Self::MatchFingerprintMismatch => write!(
                f,
                "matches-expected outcome requires identical expected and actual fingerprints"
            ),
        }
    }
}

impl Error for ComparisonModelError {}
