//! Generic evidence types for parser comparison.
//!
//! Harness completion, subject disposition, instrument completeness,
//! observability, and reviewed conformance are deliberately separate. A parser
//! accepting source cleanly is execution evidence; it becomes a correctness
//! result only when a compatible observer evaluates a reviewed expectation.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

const MAX_STABLE_ID_BYTES: usize = 128;
const MAX_FINGERPRINT_BYTES: usize = 1_024;
const MAX_DIVERGENCE_PATH_BYTES: usize = 256;

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

/// Checked stable identifier used by extensible comparison vocabularies.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StableId(String);

impl StableId {
    /// Construct a lowercase stable identifier.
    ///
    /// IDs begin with an ASCII lowercase letter or digit and may contain ASCII
    /// lowercase letters, digits, `.`, `-`, and `_`.
    pub fn new(value: impl Into<String>) -> Result<Self, EvidenceValueError> {
        let value = value.into();
        validate_nonempty_bounded("stable_id", &value, MAX_STABLE_ID_BYTES)?;

        let mut characters = value.char_indices();
        let Some((_, first)) = characters.next() else {
            return Err(EvidenceValueError::Empty { kind: "stable_id" });
        };
        if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
            return Err(EvidenceValueError::InvalidCharacter {
                kind: "stable_id",
                index: 0,
                character: first,
            });
        }

        for (index, character) in characters {
            if !character.is_ascii_lowercase()
                && !character.is_ascii_digit()
                && !matches!(character, '.' | '-' | '_')
            {
                return Err(EvidenceValueError::InvalidCharacter {
                    kind: "stable_id",
                    index,
                    character,
                });
            }
        }

        Ok(Self(value))
    }

    /// Borrow the checked identifier.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for StableId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Exact observer identity used for one comparison-domain evaluation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObserverId(StableId);

impl ObserverId {
    /// Construct a checked observer identifier.
    pub fn new(value: impl Into<String>) -> Result<Self, EvidenceValueError> {
        StableId::new(value).map(Self)
    }

    /// Borrow the observer identifier.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Borrow the underlying stable identifier.
    pub const fn stable_id(&self) -> &StableId {
        &self.0
    }
}

impl fmt::Display for ObserverId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Exact independently reviewed expectation or obligation identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ReviewedExpectationId(StableId);

impl ReviewedExpectationId {
    /// Construct a checked reviewed-expectation identifier.
    pub fn new(value: impl Into<String>) -> Result<Self, EvidenceValueError> {
        StableId::new(value).map(Self)
    }

    /// Borrow the reviewed-expectation identifier.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Borrow the underlying stable identifier.
    pub const fn stable_id(&self) -> &StableId {
        &self.0
    }
}

impl fmt::Display for ReviewedExpectationId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Canonical bounded fingerprint of an expected or observed semantic value.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SemanticFingerprint(String);

impl SemanticFingerprint {
    /// Construct a non-empty bounded fingerprint.
    pub fn new(value: impl Into<String>) -> Result<Self, EvidenceValueError> {
        let value = value.into();
        validate_nonempty_bounded("semantic_fingerprint", &value, MAX_FINGERPRINT_BYTES)?;
        validate_no_control_characters("semantic_fingerprint", &value)?;
        Ok(Self(value))
    }

    /// Borrow the canonical fingerprint.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SemanticFingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Typed bounded path to the first mismatching field or semantic position.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DivergencePath(String);

impl DivergencePath {
    /// Construct a non-empty bounded divergence path.
    pub fn new(value: impl Into<String>) -> Result<Self, EvidenceValueError> {
        let value = value.into();
        validate_nonempty_bounded("divergence_path", &value, MAX_DIVERGENCE_PATH_BYTES)?;
        validate_no_control_characters("divergence_path", &value)?;
        Ok(Self(value))
    }

    /// Borrow the divergence path.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DivergencePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Bounded diagnostic text with explicit truncation accounting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundedText {
    text: String,
    original_bytes: usize,
    omitted_bytes: usize,
}

impl BoundedText {
    /// Bound text to at most `max_bytes` without splitting a UTF-8 code point.
    pub fn new(value: impl Into<String>, max_bytes: usize) -> Result<Self, EvidenceValueError> {
        if max_bytes == 0 {
            return Err(EvidenceValueError::ZeroLimit { kind: "bounded_text" });
        }

        let value = value.into();
        let original_bytes = value.len();
        let mut end = original_bytes.min(max_bytes);
        while !value.is_char_boundary(end) {
            end -= 1;
        }

        Ok(Self {
            text: value[..end].to_owned(),
            original_bytes,
            omitted_bytes: original_bytes - end,
        })
    }

    /// Borrow the retained text.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Original UTF-8 byte length before bounding.
    pub const fn original_bytes(&self) -> usize {
        self.original_bytes
    }

    /// Number of UTF-8 bytes omitted from the retained text.
    pub const fn omitted_bytes(&self) -> usize {
        self.omitted_bytes
    }

    /// Whether the original text was truncated.
    pub const fn is_truncated(&self) -> bool {
        self.omitted_bytes > 0
    }
}

/// Terminal harness or process failure before trustworthy subject output exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum HarnessFailure {
    /// Execution was planned but did not run.
    NotRun,
    /// The subject or harness could not be configured or initialized.
    SetupFailed,
    /// The harness or supervisor cancelled execution externally.
    Cancelled,
    /// The subject exceeded its externally enforced deadline.
    TimedOut,
    /// The worker panicked, aborted, exited, or was signalled.
    CrashedOrSignalled,
    /// Bounded process output exceeded its declared limit.
    OutputLimited,
    /// A worker response was missing, malformed, or protocol-incompatible.
    WorkerProtocolFailed,
    /// The shared process supervisor failed independently of the subject.
    SupervisorFailed,
}

/// Harness/process terminal state for one subject execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum HarnessOutcome {
    /// The harness returned a trustworthy decoded subject result.
    Completed,
    /// Execution failed before trustworthy subject output was available.
    Failed(HarnessFailure),
}

/// Terminal disposition reported by a completed parser subject.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum SubjectDisposition {
    /// Subject returned a result without its declared recovery/error signal.
    AcceptedClean,
    /// Subject returned a result while reporting recovery or diagnostics.
    AcceptedRecovered,
    /// Subject rejected the source as parser input.
    Rejected,
    /// Subject does not support the requested operation.
    Unsupported,
    /// Subject deterministically cancelled its own operation.
    Cancelled,
    /// Subject exhausted its own declared budget.
    BudgetExhausted,
    /// Subject returned a catastrophic terminal state without a harness failure.
    Catastrophic,
    /// Registered bounded subject disposition not yet promoted to a dedicated variant.
    Registered(StableId),
}

/// State of the required comparison instrumentation for one subject result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum InstrumentState {
    /// Required instrumentation completed and is decisive for supported planes.
    Complete,
    /// Instrumentation returned partial evidence with declared limitations.
    Partial,
    /// Required instrumentation is unavailable for this subject or operation.
    Unavailable,
    /// Required instrumentation ran but failed.
    Failed,
    /// Instrument output exceeded a bound and was truncated.
    Truncated,
    /// Instrument output used an incompatible schema or protocol.
    SchemaMismatch,
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
    Registered(StableId),
}

/// Terminal disposition of one requested observation plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum ObservationDisposition {
    /// A complete normalized observation is available.
    Observed,
    /// A normalized observation is available with declared limitations.
    ObservedWithLimitations,
    /// The subject does not support the requested plane.
    Unsupported,
    /// The plane does not apply to this case or operation.
    NotApplicable,
    /// The subject completed but the requested plane cannot be observed.
    NotObservable,
    /// Required evidence did not establish the observation.
    NotProven,
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
        Self { diagnostic_count, recovery_observed, error_node_observed }
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
    harness: HarnessOutcome,
    subject_disposition: Option<SubjectDisposition>,
    diagnostics: DiagnosticSummary,
    observations: BTreeMap<ObservationPlane, ObservationDisposition>,
    debug_projection: Option<BoundedText>,
    instrument_state: InstrumentState,
    error: Option<BoundedText>,
}

impl SubjectExecution {
    /// Construct a completed subject execution.
    pub fn completed(
        subject: SubjectRole,
        subject_disposition: SubjectDisposition,
        diagnostics: DiagnosticSummary,
        observations: BTreeMap<ObservationPlane, ObservationDisposition>,
        debug_projection: Option<BoundedText>,
        instrument_state: InstrumentState,
    ) -> Result<Self, ComparisonModelError> {
        validate_observation_states(HarnessOutcome::Completed, instrument_state, &observations)?;
        Ok(Self {
            subject,
            harness: HarnessOutcome::Completed,
            subject_disposition: Some(subject_disposition),
            diagnostics,
            observations,
            debug_projection,
            instrument_state,
            error: None,
        })
    }

    /// Construct a failed harness/process execution with no parser disposition.
    pub fn failed(
        subject: SubjectRole,
        failure: HarnessFailure,
        diagnostics: DiagnosticSummary,
        observations: BTreeMap<ObservationPlane, ObservationDisposition>,
        debug_projection: Option<BoundedText>,
        instrument_state: InstrumentState,
        error: Option<BoundedText>,
    ) -> Result<Self, ComparisonModelError> {
        let harness = HarnessOutcome::Failed(failure);
        validate_observation_states(harness, instrument_state, &observations)?;
        if instrument_state == InstrumentState::Complete {
            return Err(ComparisonModelError::CompleteInstrumentFromFailedHarness);
        }
        Ok(Self {
            subject,
            harness,
            subject_disposition: None,
            diagnostics,
            observations,
            debug_projection,
            instrument_state,
            error,
        })
    }

    /// Stable role of the executed parser subject.
    pub const fn subject(&self) -> SubjectRole {
        self.subject
    }

    /// Harness/process terminal state.
    pub const fn harness(&self) -> HarnessOutcome {
        self.harness
    }

    /// Parser-owned disposition, present only after completed harness execution.
    pub fn subject_disposition(&self) -> Option<&SubjectDisposition> {
        self.subject_disposition.as_ref()
    }

    /// Diagnostic and recovery summary.
    pub const fn diagnostics(&self) -> &DiagnosticSummary {
        &self.diagnostics
    }

    /// Deterministically ordered observation disposition map.
    pub const fn observations(&self) -> &BTreeMap<ObservationPlane, ObservationDisposition> {
        &self.observations
    }

    /// Terminal disposition of one observation plane, when planned.
    pub fn observation(&self, plane: &ObservationPlane) -> Option<ObservationDisposition> {
        self.observations.get(plane).copied()
    }

    /// Bounded raw projection retained only for diagnostics.
    pub fn debug_projection(&self) -> Option<&BoundedText> {
        self.debug_projection.as_ref()
    }

    /// Required-instrument terminal state.
    pub const fn instrument_state(&self) -> InstrumentState {
        self.instrument_state
    }

    /// Bounded harness/setup/process error text, when present.
    pub fn error(&self) -> Option<&BoundedText> {
        self.error.as_ref()
    }

    fn can_score(&self, plane: &ObservationPlane) -> Result<(), ComparisonModelError> {
        if self.harness != HarnessOutcome::Completed {
            return Err(ComparisonModelError::ScoringRequiresCompletedHarness);
        }
        if self.instrument_state != InstrumentState::Complete {
            return Err(ComparisonModelError::ScoringRequiresCompleteInstrument);
        }
        if self.observation(plane) != Some(ObservationDisposition::Observed) {
            return Err(ComparisonModelError::ScoringRequiresObservedPlane);
        }
        Ok(())
    }
}

/// Top-level reviewed conformance outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum ConformanceOutcome {
    /// Actual observation equals the reviewed expected observation.
    MatchesExpected,
    /// Actual observation differs from the reviewed expected observation.
    Mismatch,
    /// No reviewed expectation applies to this observation.
    Unscored,
    /// Evidence completed but cannot establish the proposition.
    Unknown,
    /// Required evidence did not establish the proposition.
    NotProven,
}

/// Non-decisive conformance outcome used when no match or mismatch is valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum NonDecisiveOutcome {
    /// No reviewed expectation applies to this observation.
    Unscored,
    /// Evidence completed but cannot establish the proposition.
    Unknown,
    /// Required evidence did not establish the proposition.
    NotProven,
}

impl From<NonDecisiveOutcome> for ConformanceOutcome {
    fn from(outcome: NonDecisiveOutcome) -> Self {
        match outcome {
            NonDecisiveOutcome::Unscored => Self::Unscored,
            NonDecisiveOutcome::Unknown => Self::Unknown,
            NonDecisiveOutcome::NotProven => Self::NotProven,
        }
    }
}

/// Typed class of a reviewed conformance mismatch.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum MismatchClass {
    /// Observed construct or node kind differs from the reviewed expectation.
    WrongKind,
    /// Observed parent, field, or semantic role differs.
    WrongParentOrField,
    /// Observed ordering or ownership differs.
    WrongOrderOrOwnership,
    /// Observed value or payload differs.
    WrongValueOrPayload,
    /// Observed source range or geometry differs.
    WrongRangeOrGeometry,
    /// Observed recovery or terminal state differs.
    WrongRecoveryOrTerminalState,
    /// Required material is silently absent from the observation.
    SilentlyEmpty,
    /// Observation is structurally plausible but semantically wrong.
    WrongButPlausible,
    /// Registered bounded mismatch class not yet promoted to a dedicated variant.
    Registered(StableId),
}

/// Required details for one reviewed mismatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MismatchDetail {
    class: MismatchClass,
    first_divergence: DivergencePath,
}

impl MismatchDetail {
    /// Construct typed mismatch details.
    pub fn new(class: MismatchClass, first_divergence: DivergencePath) -> Self {
        Self { class, first_divergence }
    }

    /// Typed mismatch class.
    pub fn class(&self) -> &MismatchClass {
        &self.class
    }

    /// First deterministic divergent path.
    pub fn first_divergence(&self) -> &DivergencePath {
        &self.first_divergence
    }
}

/// Validated comparison of one observation against one reviewed expectation.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ScoredComparison {
    observer_id: Option<ObserverId>,
    expectation_id: Option<ReviewedExpectationId>,
    plane: ObservationPlane,
    expected_fingerprint: Option<SemanticFingerprint>,
    actual_fingerprint: Option<SemanticFingerprint>,
    outcome: ConformanceOutcome,
    mismatch: Option<MismatchDetail>,
}

impl ScoredComparison {
    /// Construct a decisive reviewed match.
    pub fn matches_expected(
        execution: &SubjectExecution,
        observer_id: ObserverId,
        expectation_id: ReviewedExpectationId,
        plane: ObservationPlane,
        expected_fingerprint: SemanticFingerprint,
        actual_fingerprint: SemanticFingerprint,
    ) -> Result<Self, ComparisonModelError> {
        execution.can_score(&plane)?;
        if expected_fingerprint != actual_fingerprint {
            return Err(ComparisonModelError::MatchFingerprintMismatch);
        }
        Ok(Self {
            observer_id: Some(observer_id),
            expectation_id: Some(expectation_id),
            plane,
            expected_fingerprint: Some(expected_fingerprint),
            actual_fingerprint: Some(actual_fingerprint),
            outcome: ConformanceOutcome::MatchesExpected,
            mismatch: None,
        })
    }

    /// Construct a decisive reviewed mismatch with typed first divergence.
    pub fn mismatch(
        execution: &SubjectExecution,
        observer_id: ObserverId,
        expectation_id: ReviewedExpectationId,
        plane: ObservationPlane,
        expected_fingerprint: SemanticFingerprint,
        actual_fingerprint: SemanticFingerprint,
        mismatch: MismatchDetail,
    ) -> Result<Self, ComparisonModelError> {
        execution.can_score(&plane)?;
        if expected_fingerprint == actual_fingerprint {
            return Err(ComparisonModelError::MismatchFingerprintMatch);
        }
        Ok(Self {
            observer_id: Some(observer_id),
            expectation_id: Some(expectation_id),
            plane,
            expected_fingerprint: Some(expected_fingerprint),
            actual_fingerprint: Some(actual_fingerprint),
            outcome: ConformanceOutcome::Mismatch,
            mismatch: Some(mismatch),
        })
    }

    /// Construct an explicit non-decisive result without fabricated authority.
    pub fn non_decisive(plane: ObservationPlane, outcome: NonDecisiveOutcome) -> Self {
        Self {
            observer_id: None,
            expectation_id: None,
            plane,
            expected_fingerprint: None,
            actual_fingerprint: None,
            outcome: match outcome {
                NonDecisiveOutcome::Unscored => ConformanceOutcome::Unscored,
                NonDecisiveOutcome::Unknown => ConformanceOutcome::Unknown,
                NonDecisiveOutcome::NotProven => ConformanceOutcome::NotProven,
            },
            mismatch: None,
        }
    }

    /// Observer identity used to score this comparison.
    pub fn observer_id(&self) -> Option<&ObserverId> {
        self.observer_id.as_ref()
    }

    /// Independently reviewed expectation identity, when scored.
    pub fn expectation_id(&self) -> Option<&ReviewedExpectationId> {
        self.expectation_id.as_ref()
    }

    /// Observation plane that was scored or explicitly left non-decisive.
    pub fn plane(&self) -> &ObservationPlane {
        &self.plane
    }

    /// Expected reviewed fingerprint, when scored.
    pub fn expected_fingerprint(&self) -> Option<&SemanticFingerprint> {
        self.expected_fingerprint.as_ref()
    }

    /// Actual observed fingerprint, when scored.
    pub fn actual_fingerprint(&self) -> Option<&SemanticFingerprint> {
        self.actual_fingerprint.as_ref()
    }

    /// Reviewed conformance outcome.
    pub const fn outcome(&self) -> ConformanceOutcome {
        self.outcome
    }

    /// Typed mismatch details, present only for a mismatch.
    pub fn mismatch_detail(&self) -> Option<&MismatchDetail> {
        self.mismatch.as_ref()
    }
}

/// Construction error for the generic comparison model.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ComparisonModelError {
    /// A failed harness execution claimed complete subject instrumentation.
    CompleteInstrumentFromFailedHarness,
    /// Failed harness execution carried an observed plane.
    ObservationFromFailedHarness,
    /// A complete observation was attached to incomplete instrumentation.
    ObservedFromIncompleteInstrument,
    /// A limited observation was attached to unusable instrumentation.
    LimitedObservationFromUnusableInstrument,
    /// Decisive scoring requires a completed harness execution.
    ScoringRequiresCompletedHarness,
    /// Decisive scoring requires complete instrumentation.
    ScoringRequiresCompleteInstrument,
    /// Decisive scoring requires an exactly observed plane.
    ScoringRequiresObservedPlane,
    /// A claimed match carried different expected and actual fingerprints.
    MatchFingerprintMismatch,
    /// A claimed mismatch carried identical expected and actual fingerprints.
    MismatchFingerprintMatch,
    /// A bounded or checked evidence value was invalid.
    EvidenceValue(EvidenceValueError),
}

impl fmt::Display for ComparisonModelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CompleteInstrumentFromFailedHarness => formatter.write_str(
                "failed harness execution cannot carry complete subject instrumentation",
            ),
            Self::ObservationFromFailedHarness => formatter
                .write_str("failed harness execution cannot carry an observed comparison plane"),
            Self::ObservedFromIncompleteInstrument => {
                formatter.write_str("complete observation requires complete instrumentation")
            }
            Self::LimitedObservationFromUnusableInstrument => formatter.write_str(
                "limited observation requires complete, partial, or truncated instrumentation",
            ),
            Self::ScoringRequiresCompletedHarness => {
                formatter.write_str("decisive scoring requires completed harness execution")
            }
            Self::ScoringRequiresCompleteInstrument => {
                formatter.write_str("decisive scoring requires complete instrumentation")
            }
            Self::ScoringRequiresObservedPlane => {
                formatter.write_str("decisive scoring requires an exactly observed plane")
            }
            Self::MatchFingerprintMismatch => formatter.write_str(
                "matches-expected outcome requires identical expected and actual fingerprints",
            ),
            Self::MismatchFingerprintMatch => formatter
                .write_str("mismatch outcome requires different expected and actual fingerprints"),
            Self::EvidenceValue(error) => error.fmt(formatter),
        }
    }
}

impl Error for ComparisonModelError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::EvidenceValue(error) => Some(error),
            _ => None,
        }
    }
}

impl From<EvidenceValueError> for ComparisonModelError {
    fn from(error: EvidenceValueError) -> Self {
        Self::EvidenceValue(error)
    }
}

/// Validation error for checked comparison evidence values.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum EvidenceValueError {
    /// Required value was empty.
    Empty {
        /// Stable value kind.
        kind: &'static str,
    },
    /// Value exceeded its declared byte limit.
    TooLong {
        /// Stable value kind.
        kind: &'static str,
        /// Actual UTF-8 byte length.
        actual: usize,
        /// Maximum accepted UTF-8 byte length.
        maximum: usize,
    },
    /// Value contained a character outside its allowed vocabulary.
    InvalidCharacter {
        /// Stable value kind.
        kind: &'static str,
        /// UTF-8 byte index of the invalid character.
        index: usize,
        /// Invalid character.
        character: char,
    },
    /// A bounded text value was constructed with a zero-byte limit.
    ZeroLimit {
        /// Stable value kind.
        kind: &'static str,
    },
}

impl fmt::Display for EvidenceValueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty { kind } => write!(formatter, "{kind} must not be empty"),
            Self::TooLong { kind, actual, maximum } => {
                write!(formatter, "{kind} is {actual} bytes; maximum is {maximum} bytes")
            }
            Self::InvalidCharacter { kind, index, character } => {
                write!(formatter, "{kind} contains invalid character {character:?} at byte {index}")
            }
            Self::ZeroLimit { kind } => {
                write!(formatter, "{kind} requires a non-zero byte limit")
            }
        }
    }
}

impl Error for EvidenceValueError {}

fn validate_observation_states(
    harness: HarnessOutcome,
    instrument_state: InstrumentState,
    observations: &BTreeMap<ObservationPlane, ObservationDisposition>,
) -> Result<(), ComparisonModelError> {
    for disposition in observations.values() {
        if matches!(harness, HarnessOutcome::Failed(_))
            && matches!(
                disposition,
                ObservationDisposition::Observed | ObservationDisposition::ObservedWithLimitations
            )
        {
            return Err(ComparisonModelError::ObservationFromFailedHarness);
        }

        if *disposition == ObservationDisposition::Observed
            && instrument_state != InstrumentState::Complete
        {
            return Err(ComparisonModelError::ObservedFromIncompleteInstrument);
        }

        if *disposition == ObservationDisposition::ObservedWithLimitations
            && !matches!(
                instrument_state,
                InstrumentState::Complete | InstrumentState::Partial | InstrumentState::Truncated
            )
        {
            return Err(ComparisonModelError::LimitedObservationFromUnusableInstrument);
        }
    }

    Ok(())
}

fn validate_nonempty_bounded(
    kind: &'static str,
    value: &str,
    maximum: usize,
) -> Result<(), EvidenceValueError> {
    if value.is_empty() {
        return Err(EvidenceValueError::Empty { kind });
    }
    if value.len() > maximum {
        return Err(EvidenceValueError::TooLong { kind, actual: value.len(), maximum });
    }
    Ok(())
}

fn validate_no_control_characters(
    kind: &'static str,
    value: &str,
) -> Result<(), EvidenceValueError> {
    if let Some((index, character)) =
        value.char_indices().find(|(_, character)| character.is_control())
    {
        return Err(EvidenceValueError::InvalidCharacter { kind, index, character });
    }
    Ok(())
}
