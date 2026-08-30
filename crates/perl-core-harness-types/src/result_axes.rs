//! Strict v2 result axes for the upstream Perl core harness lane.
//!
//! A v1 harness record answered one question — did the runner pass? That single
//! answer silently merged six independent facts: whether the evidence is usable
//! at all, what the product actually did, what the compiler admitted, what
//! semantics are genuinely supported, which mechanism produced the correctness
//! signal, and how the observation relates to accepted state.
//!
//! This module keeps those facts separate and makes dishonest combinations
//! unrepresentable. [`ResultAxes`] holds private fields and is reachable only
//! through [`ResultAxes::new`], which enforces every cross-axis invariant.
//! Deserialization routes through the same check, so a receipt read from disk
//! cannot assert a combination that could not have been constructed.
//!
//! This module is representation-only. It adds no production consumer, migrates
//! no baseline, and accepts no transition.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use crate::{
    CompatibilityRailAvailability, CompatibilityTransition, HarnessMode,
    RESULT_AXES_SCHEMA_VERSION, RunnerStatus, SmokeStatus,
};

/// Whether the evidence behind a record may be used to draw a domain conclusion.
///
/// This axis is decided before any product outcome. Evidence that is not
/// [`EvidenceValidity::Valid`] cannot carry a clean or failing observation,
/// because no domain conclusion was actually obtained.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceValidity {
    /// The evidence is complete, fresh, and usable for a current conclusion.
    Valid,
    /// The instrument did not establish enough to decide the domain question.
    NotProven,
    /// The evidence is structurally unusable, such as a malformed receipt.
    Invalid,
    /// The evidence was valid once but its freshness contract has expired.
    Stale,
    /// The measurement was cancelled before it could settle.
    Cancelled,
}

impl EvidenceValidity {
    /// Stable snake_case wire name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Valid => "valid",
            Self::NotProven => "not_proven",
            Self::Invalid => "invalid",
            Self::Stale => "stale",
            Self::Cancelled => "cancelled",
        }
    }

    /// Whether this evidence may support a current domain conclusion.
    #[must_use]
    pub fn supports_domain_conclusion(self) -> bool {
        matches!(self, Self::Valid)
    }
}

impl fmt::Display for EvidenceValidity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What the product was observed to do, separate from whether that is acceptable.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservedOutcome {
    /// The subject ran and produced no failures.
    Clean,
    /// The subject ran and produced real product failures.
    ///
    /// This is an honest red observation, not an instrument problem: it pairs
    /// with [`EvidenceValidity::Valid`].
    FailuresObserved,
    /// The process or protocol itself failed, so no product outcome was observed.
    ProcessOrProtocolFailed,
    /// This subject was deliberately not measured.
    NotAssessed,
}

impl ObservedOutcome {
    /// Stable snake_case wire name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::FailuresObserved => "failures_observed",
            Self::ProcessOrProtocolFailed => "process_or_protocol_failed",
            Self::NotAssessed => "not_assessed",
        }
    }

    /// Whether this outcome asserts something about the product itself.
    #[must_use]
    pub fn is_domain_outcome(self) -> bool {
        matches!(self, Self::Clean | Self::FailuresObserved)
    }
}

impl fmt::Display for ObservedOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What the compiler admitted about a subject, separate from what it supports.
///
/// Admission is a compile-time statement. It never implies that the behavior
/// executes correctly.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityAdmission {
    /// The behavior is modeled as an ordinary implemented fact.
    Implemented,
    /// A static fact was retained without executing the behavior.
    StaticallyClassified,
    /// The subject is admitted only through reviewed, source-locked debt.
    AcceptedDebt,
    /// The subject is not currently safe to admit.
    Unsupported,
    /// Admission was not assessed for this subject.
    NotAssessed,
}

impl CompatibilityAdmission {
    /// Stable snake_case wire name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Implemented => "implemented",
            Self::StaticallyClassified => "statically_classified",
            Self::AcceptedDebt => "accepted_debt",
            Self::Unsupported => "unsupported",
            Self::NotAssessed => "not_assessed",
        }
    }

    /// Whether this admission could ever underwrite general semantic support.
    ///
    /// Only [`CompatibilityAdmission::Implemented`] can. Debt, static
    /// classification, unsupported, and unassessed admissions cannot, however
    /// much execution evidence accompanies them.
    #[must_use]
    pub fn may_underwrite_general_support(self) -> bool {
        matches!(self, Self::Implemented)
    }
}

impl fmt::Display for CompatibilityAdmission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How much of the semantics is genuinely supported.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticSupport {
    /// Supported in general, not only for pinned shapes or replayed fixtures.
    General,
    /// Supported for a declared subset only.
    Partial,
    /// Known to be blocked, typically by source-locked or downstream debt.
    Blocked,
    /// No support rail exists for this subject.
    Unavailable,
    /// Support was not assessed for this subject.
    NotAssessed,
}

impl SemanticSupport {
    /// Stable snake_case wire name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Partial => "partial",
            Self::Blocked => "blocked",
            Self::Unavailable => "unavailable",
            Self::NotAssessed => "not_assessed",
        }
    }

    /// Whether this is a positive support claim requiring backing evidence.
    #[must_use]
    pub fn is_positive_claim(self) -> bool {
        matches!(self, Self::General | Self::Partial)
    }
}

impl fmt::Display for SemanticSupport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Every category name a `by_admission` distribution may use.
///
/// Held to [`CompatibilityAdmission`] by
/// `distribution_categories_match_the_axis_vocabularies`, so the two cannot drift.
pub const ADMISSION_CATEGORIES: [&str; 5] =
    ["accepted_debt", "implemented", "not_assessed", "statically_classified", "unsupported"];

/// Every category name a `by_support` distribution may use.
///
/// Held to [`SemanticSupport`] by the same test.
pub const SUPPORT_CATEGORIES: [&str; 5] =
    ["blocked", "general", "not_assessed", "partial", "unavailable"];

/// Which mechanism produced the execution or correctness signal.
///
/// A weaker mechanism can never be presented as a stronger one. Fixture replay
/// in particular proves that recorded output still matches; it does not prove
/// runtime semantics.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CorrectnessMechanism {
    /// No execution or correctness rail ran.
    None,
    /// Recorded fixtures were replayed and compared.
    FixtureReplay,
    /// The subject executed through EIR.
    EirExecution,
    /// The subject was differentially compared against real Perl.
    RealPerlOracle,
    /// The subject was compared against curated gold output.
    CuratedGold,
}

impl CorrectnessMechanism {
    /// Stable snake_case wire name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::FixtureReplay => "fixture_replay",
            Self::EirExecution => "eir_execution",
            Self::RealPerlOracle => "real_perl_oracle",
            Self::CuratedGold => "curated_gold",
        }
    }

    /// Whether the mechanism actually exercised runtime behavior.
    #[must_use]
    pub fn executes_behavior(self) -> bool {
        matches!(self, Self::EirExecution | Self::RealPerlOracle | Self::CuratedGold)
    }
}

impl fmt::Display for CorrectnessMechanism {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One named axis, used when a record must report which axes it cannot fill.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultAxis {
    /// The [`EvidenceValidity`] axis.
    Evidence,
    /// The [`ObservedOutcome`] axis.
    Observation,
    /// The [`CompatibilityAdmission`] axis.
    Admission,
    /// The [`SemanticSupport`] axis.
    Support,
    /// The [`CorrectnessMechanism`] axis.
    Mechanism,
}

impl ResultAxis {
    /// Stable snake_case wire name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Evidence => "evidence",
            Self::Observation => "observation",
            Self::Admission => "admission",
            Self::Support => "support",
            Self::Mechanism => "mechanism",
        }
    }
}

impl fmt::Display for ResultAxis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A cross-axis combination that would overstate what the evidence establishes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResultAxisViolation {
    /// A clean or failing product outcome was claimed without valid evidence.
    DomainOutcomeWithoutValidEvidence {
        /// The evidence axis that cannot support a domain conclusion.
        evidence: EvidenceValidity,
        /// The domain outcome that was claimed anyway.
        observation: ObservedOutcome,
    },
    /// A process or protocol failure was recorded as valid evidence.
    ProcessFailureClaimedAsValidEvidence,
    /// A positive support claim was made without valid evidence.
    PositiveSupportWithoutValidEvidence {
        /// The support claim that lacks valid evidence.
        support: SemanticSupport,
        /// The evidence axis that cannot back it.
        evidence: EvidenceValidity,
    },
    /// An admission that cannot underwrite general support claimed it anyway.
    AdmissionCannotUnderwriteGeneralSupport {
        /// The admission that was recorded.
        admission: CompatibilityAdmission,
    },
    /// An unsupported or unassessed admission carried a positive support claim.
    AdmissionCannotUnderwritePositiveSupport {
        /// The admission that was recorded.
        admission: CompatibilityAdmission,
        /// The positive support claim it cannot back.
        support: SemanticSupport,
    },
    /// A positive support claim was made with no correctness mechanism at all.
    PositiveSupportWithoutMechanism {
        /// The positive support claim.
        support: SemanticSupport,
    },
    /// A positive support claim was made for a subject that was not observed.
    PositiveSupportWithoutObservation {
        /// The positive support claim.
        support: SemanticSupport,
        /// The observation that records no product outcome.
        observation: ObservedOutcome,
    },
    /// Fixture replay was presented as general semantic support.
    FixtureReplayClaimedGeneralSupport,
}

impl fmt::Display for ResultAxisViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DomainOutcomeWithoutValidEvidence { evidence, observation } => write!(
                f,
                "observation '{observation}' asserts a product outcome, but evidence is '{evidence}'; \
                 evidence validity is decided before domain outcome"
            ),
            Self::ProcessFailureClaimedAsValidEvidence => f.write_str(
                "observation 'process_or_protocol_failed' cannot carry evidence 'valid'; \
                 an instrument failure is not_proven, not a product result",
            ),
            Self::PositiveSupportWithoutValidEvidence { support, evidence } => {
                write!(f, "support '{support}' is a positive claim but evidence is '{evidence}'")
            }
            Self::AdmissionCannotUnderwriteGeneralSupport { admission } => write!(
                f,
                "admission '{admission}' cannot become semantic support 'general'; \
                 compile admission and accepted debt never imply general support"
            ),
            Self::AdmissionCannotUnderwritePositiveSupport { admission, support } => write!(
                f,
                "admission '{admission}' cannot carry positive semantic support '{support}'"
            ),
            Self::PositiveSupportWithoutMechanism { support } => write!(
                f,
                "support '{support}' requires a correctness mechanism; \
                 a missing rail stays unavailable rather than becoming support"
            ),
            Self::PositiveSupportWithoutObservation { support, observation } => write!(
                f,
                "support '{support}' is a positive claim but the observation is '{observation}'; \
                 a subject that was not observed cannot be reported as supported"
            ),
            Self::FixtureReplayClaimedGeneralSupport => f.write_str(
                "mechanism 'fixture_replay' cannot imply semantic support 'general'; \
                 replay proves recorded output, not runtime semantics",
            ),
        }
    }
}

impl Error for ResultAxisViolation {}

/// Deserialization shape for [`ResultAxes`], checked before it becomes one.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResultAxesRepr {
    evidence: EvidenceValidity,
    observation: ObservedOutcome,
    admission: CompatibilityAdmission,
    support: SemanticSupport,
    mechanism: CorrectnessMechanism,
}

impl TryFrom<ResultAxesRepr> for ResultAxes {
    type Error = ResultAxisViolation;

    fn try_from(repr: ResultAxesRepr) -> Result<Self, Self::Error> {
        Self::new(repr.evidence, repr.observation, repr.admission, repr.support, repr.mechanism)
    }
}

/// The six separated result axes for one subject.
///
/// Fields are private on purpose: every value of this type has passed
/// [`ResultAxes::new`], including values produced by deserialization, so an
/// overstated combination cannot enter the system through any route.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
// Unknown-field rejection lives on `ResultAxesRepr`: with `try_from`,
// deserialization is generated from the repr, so a `deny_unknown_fields` here
// would be inert.
#[serde(try_from = "ResultAxesRepr")]
pub struct ResultAxes {
    evidence: EvidenceValidity,
    observation: ObservedOutcome,
    admission: CompatibilityAdmission,
    support: SemanticSupport,
    mechanism: CorrectnessMechanism,
}

impl ResultAxes {
    /// Build one axis set, rejecting every combination that overstates evidence.
    ///
    /// # Errors
    ///
    /// Returns the first [`ResultAxisViolation`] found. Checks run in
    /// dependency order: evidence validity first, then domain outcome, then
    /// admission, then support and mechanism.
    pub fn new(
        evidence: EvidenceValidity,
        observation: ObservedOutcome,
        admission: CompatibilityAdmission,
        support: SemanticSupport,
        mechanism: CorrectnessMechanism,
    ) -> Result<Self, ResultAxisViolation> {
        if observation.is_domain_outcome() && !evidence.supports_domain_conclusion() {
            return Err(ResultAxisViolation::DomainOutcomeWithoutValidEvidence {
                evidence,
                observation,
            });
        }
        if observation == ObservedOutcome::ProcessOrProtocolFailed
            && evidence.supports_domain_conclusion()
        {
            return Err(ResultAxisViolation::ProcessFailureClaimedAsValidEvidence);
        }
        if support.is_positive_claim() {
            if !evidence.supports_domain_conclusion() {
                return Err(ResultAxisViolation::PositiveSupportWithoutValidEvidence {
                    support,
                    evidence,
                });
            }
            if !observation.is_domain_outcome() {
                return Err(ResultAxisViolation::PositiveSupportWithoutObservation {
                    support,
                    observation,
                });
            }
            if matches!(
                admission,
                CompatibilityAdmission::Unsupported | CompatibilityAdmission::NotAssessed
            ) {
                return Err(ResultAxisViolation::AdmissionCannotUnderwritePositiveSupport {
                    admission,
                    support,
                });
            }
            if mechanism == CorrectnessMechanism::None {
                return Err(ResultAxisViolation::PositiveSupportWithoutMechanism { support });
            }
        }
        if support == SemanticSupport::General {
            if !admission.may_underwrite_general_support() {
                return Err(ResultAxisViolation::AdmissionCannotUnderwriteGeneralSupport {
                    admission,
                });
            }
            if mechanism == CorrectnessMechanism::FixtureReplay {
                return Err(ResultAxisViolation::FixtureReplayClaimedGeneralSupport);
            }
        }
        Ok(Self { evidence, observation, admission, support, mechanism })
    }

    /// Build the axis set for a subject that was deliberately not measured.
    #[must_use]
    pub fn not_assessed(evidence: EvidenceValidity) -> Self {
        Self {
            evidence,
            observation: ObservedOutcome::NotAssessed,
            admission: CompatibilityAdmission::NotAssessed,
            support: SemanticSupport::NotAssessed,
            mechanism: CorrectnessMechanism::None,
        }
    }

    /// The evidence-validity axis.
    #[must_use]
    pub fn evidence(&self) -> EvidenceValidity {
        self.evidence
    }

    /// The observed-outcome axis.
    #[must_use]
    pub fn observation(&self) -> ObservedOutcome {
        self.observation
    }

    /// The compatibility-admission axis.
    #[must_use]
    pub fn admission(&self) -> CompatibilityAdmission {
        self.admission
    }

    /// The semantic-support axis.
    #[must_use]
    pub fn support(&self) -> SemanticSupport {
        self.support
    }

    /// The correctness-mechanism axis.
    #[must_use]
    pub fn mechanism(&self) -> CorrectnessMechanism {
        self.mechanism
    }

    /// Axes that carry no assessment and must not be read as zero or pass.
    #[must_use]
    pub fn not_assessed_axes(&self) -> Vec<ResultAxis> {
        let mut axes = Vec::new();
        if self.observation == ObservedOutcome::NotAssessed {
            axes.push(ResultAxis::Observation);
        }
        if self.admission == CompatibilityAdmission::NotAssessed {
            axes.push(ResultAxis::Admission);
        }
        if self.support == SemanticSupport::NotAssessed {
            axes.push(ResultAxis::Support);
        }
        if self.mechanism == CorrectnessMechanism::None {
            axes.push(ResultAxis::Mechanism);
        }
        axes
    }
}

/// Whether a record was measured now or adapted from historical v1 evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceOrigin {
    /// The record was produced by a current measurement.
    CurrentMeasurement,
    /// The record was adapted from historical v1 evidence and is read-only.
    LegacyAdapted,
}

/// How completely the measurement itself ran.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeasurementCompletion {
    /// Every declared subject settled.
    Complete,
    /// The measurement started but did not cover every declared subject.
    Incomplete,
    /// The measurement never ran.
    NotAttempted,
}

/// Exact identity every current aggregate must retain.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ResultSubjectIdentity {
    /// Immutable comparison-series identity.
    pub series_id: String,
    /// Exact compiler/runner subject the numbers describe.
    pub subject_identity: String,
    /// Evidence bundle the aggregate was computed from.
    pub evidence_bundle_id: String,
    /// Commit the measurement ran at.
    pub measurement_sha: String,
    /// Exact denominator; an aggregate without one is not comparable.
    pub denominator: usize,
}

impl ResultSubjectIdentity {
    /// Reject an aggregate identity that cannot anchor a comparison.
    ///
    /// # Errors
    ///
    /// Returns [`ResultReportViolation::MissingSubjectIdentity`] when a required
    /// identity string is empty, or [`ResultReportViolation::MissingDenominator`]
    /// when the denominator is zero.
    pub fn validate(&self) -> Result<(), ResultReportViolation> {
        for (field, value) in [
            ("series_id", &self.series_id),
            ("subject_identity", &self.subject_identity),
            ("evidence_bundle_id", &self.evidence_bundle_id),
            ("measurement_sha", &self.measurement_sha),
        ] {
            if value.trim().is_empty() {
                return Err(ResultReportViolation::MissingSubjectIdentity { field });
            }
        }
        if self.denominator == 0 {
            return Err(ResultReportViolation::MissingDenominator);
        }
        Ok(())
    }
}

/// Availability and counts for one optional correctness or execution rail.
///
/// A rail that did not run has no counts at all. Representing it as zero of zero
/// would let an absent rail read as a clean one.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CorrectnessRailSummary {
    /// Mechanism this rail would use.
    pub mechanism: CorrectnessMechanism,
    /// Whether the rail produced usable evidence.
    pub availability: CompatibilityRailAvailability,
    /// Why the rail is in this state.
    pub reason: String,
    /// Evidence references backing the rail.
    pub evidence_refs: Vec<String>,
    /// Files the rail covered, absent unless the rail actually ran.
    pub files_total: Option<usize>,
    /// Files the rail passed, absent unless the rail actually ran.
    pub files_passed: Option<usize>,
}

/// Reject a required string that is empty or whitespace-only.
///
/// The published schema carries `minLength: 1` on these fields; without the
/// same check here, Rust would accept records its own contract rejects.
fn require_non_blank(field: &'static str, value: &str) -> Result<(), ResultReportViolation> {
    if value.trim().is_empty() {
        return Err(ResultReportViolation::BlankField { field });
    }
    Ok(())
}

impl CorrectnessRailSummary {
    /// An explicitly absent rail: no mechanism, no counts, a stated reason.
    #[must_use]
    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            mechanism: CorrectnessMechanism::None,
            availability: CompatibilityRailAvailability::NotAvailable,
            reason: reason.into(),
            evidence_refs: Vec::new(),
            files_total: None,
            files_passed: None,
        }
    }

    /// Whether this rail actually ran.
    ///
    /// Availability is the authority on this, not the mechanism: a record can
    /// name a mechanism it never executed, so keying absence off the mechanism
    /// would let `not_available` coexist with a full set of counts.
    #[must_use]
    pub fn ran(&self) -> bool {
        self.availability != CompatibilityRailAvailability::NotAvailable
    }

    /// Reject a rail whose state contradicts whether it ran.
    ///
    /// The law is symmetric. A rail that did not run claims nothing: no
    /// mechanism, no evidence, no counts. A rail that did run must name the
    /// mechanism it used and cite evidence, and an `available` rail must report
    /// what it covered.
    ///
    /// # Errors
    ///
    /// Returns the first [`ResultReportViolation`] found.
    pub fn validate(&self, rail: &str) -> Result<(), ResultReportViolation> {
        if self.ran() {
            if self.mechanism == CorrectnessMechanism::None {
                return Err(ResultReportViolation::RailRanWithoutMechanism {
                    rail: rail.to_string(),
                });
            }
            if self.evidence_refs.is_empty() {
                return Err(ResultReportViolation::RailRanWithoutEvidence {
                    rail: rail.to_string(),
                });
            }
            if self.availability == CompatibilityRailAvailability::Available
                && self.files_total.is_none()
            {
                return Err(ResultReportViolation::RailAvailableWithoutCounts {
                    rail: rail.to_string(),
                });
            }
        } else {
            // An absent rail stays absent in every field, so it can never be
            // read as a rail that ran and found nothing wrong.
            let claimed = if self.mechanism != CorrectnessMechanism::None {
                Some("a correctness mechanism")
            } else if !self.evidence_refs.is_empty() {
                Some("evidence references")
            } else if self.files_total.is_some() || self.files_passed.is_some() {
                Some("file counts")
            } else {
                None
            };
            if let Some(aspect) = claimed {
                return Err(ResultReportViolation::AbsentRailReportsWork {
                    rail: rail.to_string(),
                    aspect,
                });
            }
        }
        require_non_blank("reason", &self.reason)?;
        for reference in &self.evidence_refs {
            require_non_blank("evidence_refs", reference)?;
        }
        match (self.files_total, self.files_passed) {
            (Some(total), Some(passed)) if passed > total => {
                Err(ResultReportViolation::RailPassedExceedsTotal { rail: rail.to_string() })
            }
            (None, Some(_)) | (Some(_), None) => {
                Err(ResultReportViolation::RailCountsIncomplete { rail: rail.to_string() })
            }
            _ => Ok(()),
        }
    }
}

/// Parse-rail acceptance counts for one run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ParseRailSummary {
    /// Files the parse rail covered.
    pub files_total: usize,
    /// Files the parse rail accepted.
    pub files_passed: usize,
    /// Axes describing the parse rail as a whole.
    pub axes: ResultAxes,
}

/// Admission, debt, and support distribution for one run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AdmissionSummary {
    /// Count per [`CompatibilityAdmission`] wire name.
    pub by_admission: BTreeMap<String, usize>,
    /// Count per [`SemanticSupport`] wire name.
    pub by_support: BTreeMap<String, usize>,
    /// Subjects admitted only through reviewed debt.
    pub accepted_debt_count: usize,
    /// Debt entries pinned to an exact source shape.
    pub source_locked_count: usize,
    /// Debt entries that block a downstream consumer.
    pub downstream_blocking_count: usize,
}

impl AdmissionSummary {
    /// Total subjects counted by admission.
    #[must_use]
    pub fn admission_total(&self) -> usize {
        self.by_admission.values().sum()
    }

    /// Reject a distribution that does not describe the stated denominator.
    ///
    /// # Errors
    ///
    /// Returns a [`ResultReportViolation`] when either distribution disagrees
    /// with the denominator, or when debt counts exceed the admitted debt.
    pub fn validate(&self, denominator: usize) -> Result<(), ResultReportViolation> {
        // A misspelled category still sums to the denominator, so the totals
        // below cannot be trusted until the keys are known to be real.
        for category in self.by_admission.keys() {
            if !ADMISSION_CATEGORIES.contains(&category.as_str()) {
                return Err(ResultReportViolation::UnknownDistributionCategory {
                    axis: ResultAxis::Admission,
                    category: category.clone(),
                });
            }
        }
        for category in self.by_support.keys() {
            if !SUPPORT_CATEGORIES.contains(&category.as_str()) {
                return Err(ResultReportViolation::UnknownDistributionCategory {
                    axis: ResultAxis::Support,
                    category: category.clone(),
                });
            }
        }
        let admission_total = self.admission_total();
        if admission_total != denominator {
            return Err(ResultReportViolation::AdmissionTotalMismatch {
                counted: admission_total,
                denominator,
            });
        }
        let support_total: usize = self.by_support.values().sum();
        if support_total != denominator {
            return Err(ResultReportViolation::SupportTotalMismatch {
                counted: support_total,
                denominator,
            });
        }
        // An absent key and an explicit zero mean the same thing, but a declared
        // zero must not excuse a distribution that says otherwise.
        let admitted_debt =
            self.by_admission.get(CompatibilityAdmission::AcceptedDebt.as_str()).copied();
        if admitted_debt.unwrap_or(0) != self.accepted_debt_count {
            return Err(ResultReportViolation::DebtCountMismatch {
                declared: self.accepted_debt_count,
                distributed: admitted_debt.unwrap_or(0),
            });
        }
        if self.source_locked_count > self.accepted_debt_count
            || self.downstream_blocking_count > self.accepted_debt_count
        {
            return Err(ResultReportViolation::DebtDetailExceedsDebt);
        }
        Ok(())
    }
}

/// How one observation relates to the accepted state, without moving it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CurrentVersusAccepted {
    /// Canonical transition vocabulary, reused rather than redefined.
    pub transition: CompatibilityTransition,
    /// Whether a reviewer must accept this before it becomes state.
    pub requires_acceptance: bool,
    /// Digest of the accepted baseline this observation was compared against.
    pub accepted_baseline_digest: Option<String>,
    /// Why this transition was assigned.
    pub reason: String,
}

impl CurrentVersusAccepted {
    /// Reject a transition that would present a candidate as accepted state.
    ///
    /// # Errors
    ///
    /// Returns a [`ResultReportViolation`] when a candidate transition does not
    /// require acceptance, when a settled transition demands it, or when an
    /// improvement or regression is claimed with no baseline to compare against.
    pub fn validate(&self) -> Result<(), ResultReportViolation> {
        require_non_blank("reason", &self.reason)?;
        // A blank digest names no baseline; treating it as present would let an
        // improvement claim cite nothing at all.
        if let Some(digest) = &self.accepted_baseline_digest {
            require_non_blank("accepted_baseline_digest", digest)?;
        }
        // `requires_acceptance` means "a reviewer must accept this before it moves
        // the ratchet", which is the sense `classify_compatibility_transition` in
        // perl-core-harness emits: an improvement or a contract correction can be
        // accepted into state, a regression cannot — it is a blocking signal, not
        // something to adopt.
        let awaits_acceptance = matches!(
            self.transition,
            CompatibilityTransition::ImprovementCandidate
                | CompatibilityTransition::ContractCorrectionCandidate
        );
        if awaits_acceptance && !self.requires_acceptance {
            return Err(ResultReportViolation::CandidateTreatedAsAccepted {
                transition: self.transition,
            });
        }
        if !awaits_acceptance && self.requires_acceptance {
            return Err(ResultReportViolation::SettledTransitionRequiresAcceptance {
                transition: self.transition,
            });
        }
        if matches!(
            self.transition,
            CompatibilityTransition::ImprovementCandidate | CompatibilityTransition::Regression
        ) && self.accepted_baseline_digest.is_none()
        {
            return Err(ResultReportViolation::ComparisonWithoutBaseline {
                transition: self.transition,
            });
        }
        Ok(())
    }
}

/// A structural problem in an aggregate v2 result report.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResultReportViolation {
    /// The report declares an unexpected schema version.
    UnexpectedSchemaVersion {
        /// The version found on the record.
        found: String,
    },
    /// A required subject-identity field was empty.
    MissingSubjectIdentity {
        /// Name of the empty field.
        field: &'static str,
    },
    /// The aggregate carries no denominator.
    MissingDenominator,
    /// Admission counts do not describe the declared denominator.
    AdmissionTotalMismatch {
        /// Sum of the admission distribution.
        counted: usize,
        /// Declared denominator.
        denominator: usize,
    },
    /// Support counts do not describe the declared denominator.
    SupportTotalMismatch {
        /// Sum of the support distribution.
        counted: usize,
        /// Declared denominator.
        denominator: usize,
    },
    /// Declared debt disagrees with the admission distribution.
    DebtCountMismatch {
        /// Debt count declared on the summary.
        declared: usize,
        /// Debt count present in the distribution.
        distributed: usize,
    },
    /// Source-locked or downstream-blocking counts exceed total accepted debt.
    DebtDetailExceedsDebt,
    /// A rail that did not run nonetheless reported work.
    AbsentRailReportsWork {
        /// Rail name.
        rail: String,
        /// What the absent rail claimed.
        aspect: &'static str,
    },
    /// A rail that ran did not name the mechanism it used.
    RailRanWithoutMechanism {
        /// Rail name.
        rail: String,
    },
    /// A rail that ran cited no evidence.
    RailRanWithoutEvidence {
        /// Rail name.
        rail: String,
    },
    /// An available rail did not report what it covered.
    RailAvailableWithoutCounts {
        /// Rail name.
        rail: String,
    },
    /// A rail reported more passes than subjects.
    RailPassedExceedsTotal {
        /// Rail name.
        rail: String,
    },
    /// A rail reported one count without the other.
    RailCountsIncomplete {
        /// Rail name.
        rail: String,
    },
    /// A required string was empty or whitespace-only.
    BlankField {
        /// Name of the blank field.
        field: &'static str,
    },
    /// A distribution used a category outside the axis vocabulary.
    UnknownDistributionCategory {
        /// Which distribution carried the unknown key.
        axis: ResultAxis,
        /// The unrecognized category.
        category: String,
    },
    /// A complete measurement claimed valid evidence while observing nothing.
    CompleteMeasurementObservedNothing,
    /// A clean aggregate contradicted a detail record that observed failures.
    CleanAggregateOverFailingDetail,
    /// An aggregate claimed support that no rail in the report backs.
    AggregateSupportWithoutRail {
        /// The positive support claim.
        support: SemanticSupport,
        /// The mechanism it named.
        mechanism: CorrectnessMechanism,
    },
    /// Evidence that is not valid cannot serve as current authority.
    EvidenceNotValidForCurrentAuthority {
        /// The evidence axis that disqualifies the report.
        evidence: EvidenceValidity,
    },
    /// The parse rail reported more passes than subjects.
    ParsePassedExceedsTotal,
    /// The parse rail denominator disagrees with the aggregate denominator.
    ParseTotalMismatch {
        /// Parse rail total.
        counted: usize,
        /// Declared denominator.
        denominator: usize,
    },
    /// A candidate transition was recorded as if it were accepted state.
    CandidateTreatedAsAccepted {
        /// The candidate transition.
        transition: CompatibilityTransition,
    },
    /// A settled transition demanded acceptance it does not need.
    SettledTransitionRequiresAcceptance {
        /// The settled transition.
        transition: CompatibilityTransition,
    },
    /// An improvement or regression was claimed with no accepted baseline.
    ComparisonWithoutBaseline {
        /// The transition claimed without a baseline.
        transition: CompatibilityTransition,
    },
    /// Valid aggregate evidence was claimed from an incomplete measurement.
    ValidEvidenceFromIncompleteMeasurement {
        /// The completion state that cannot support valid evidence.
        completion: MeasurementCompletion,
    },
    /// A legacy-adapted record tried to act as current authority.
    LegacyAdaptedRecordClaimsCurrentAuthority {
        /// Axes the legacy evidence could not fill.
        unavailable_axes: Vec<ResultAxis>,
    },
    /// A legacy-adapted record claimed a non-historical transition.
    LegacyAdaptedRecordClaimsTransition {
        /// The transition a historical record may not claim.
        transition: CompatibilityTransition,
    },
}

impl fmt::Display for ResultReportViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedSchemaVersion { found } => {
                write!(f, "unsupported result-axes schema version '{found}'")
            }
            Self::MissingSubjectIdentity { field } => {
                write!(
                    f,
                    "subject identity field '{field}' is empty; aggregates must name their subject"
                )
            }
            Self::MissingDenominator => f.write_str(
                "aggregate has no denominator; a ratio without a denominator is not comparable",
            ),
            Self::AdmissionTotalMismatch { counted, denominator } => write!(
                f,
                "admission distribution counts {counted} subjects but the denominator is {denominator}"
            ),
            Self::SupportTotalMismatch { counted, denominator } => write!(
                f,
                "support distribution counts {counted} subjects but the denominator is {denominator}"
            ),
            Self::DebtCountMismatch { declared, distributed } => write!(
                f,
                "declared accepted debt {declared} disagrees with distributed accepted debt {distributed}"
            ),
            Self::DebtDetailExceedsDebt => {
                f.write_str("source-locked or downstream-blocking debt exceeds total accepted debt")
            }
            Self::AbsentRailReportsWork { rail, aspect } => write!(
                f,
                "rail '{rail}' is not available but reports {aspect}; \
                 an absent rail stays absent rather than becoming zero-or-pass"
            ),
            Self::RailRanWithoutMechanism { rail } => {
                write!(f, "rail '{rail}' ran but declares mechanism 'none'")
            }
            Self::RailRanWithoutEvidence { rail } => {
                write!(f, "rail '{rail}' ran but cites no evidence")
            }
            Self::RailAvailableWithoutCounts { rail } => {
                write!(f, "rail '{rail}' is available but reports no file counts")
            }
            Self::BlankField { field } => {
                write!(
                    f,
                    "field '{field}' is empty; a required identifier or reason must be stated"
                )
            }
            Self::UnknownDistributionCategory { axis, category } => write!(
                f,
                "the {axis} distribution uses category '{category}', which is not part of that \
                 axis vocabulary"
            ),
            Self::CleanAggregateOverFailingDetail => f.write_str(
                "the aggregate observation is 'clean' while a file or invocation record \
                 observed failures",
            ),
            Self::AggregateSupportWithoutRail { support, mechanism } => write!(
                f,
                "the aggregate claims support '{support}' from mechanism '{mechanism}', but no \
                 correctness rail in this report ran with that mechanism"
            ),
            Self::CompleteMeasurementObservedNothing => f.write_str(
                "a complete measurement with valid evidence recorded no observation; \
                 a run that assessed nothing is not a complete run",
            ),
            Self::EvidenceNotValidForCurrentAuthority { evidence } => write!(
                f,
                "evidence is '{evidence}', so this report cannot serve as current authority"
            ),
            Self::RailPassedExceedsTotal { rail } => {
                write!(f, "rail '{rail}' passed more subjects than it covered")
            }
            Self::RailCountsIncomplete { rail } => {
                write!(
                    f,
                    "rail '{rail}' reported one of files_total/files_passed without the other"
                )
            }
            Self::ParsePassedExceedsTotal => {
                f.write_str("parse rail passed more subjects than it covered")
            }
            Self::ParseTotalMismatch { counted, denominator } => write!(
                f,
                "parse rail covers {counted} subjects but the denominator is {denominator}"
            ),
            Self::CandidateTreatedAsAccepted { transition } => write!(
                f,
                "transition '{transition:?}' is a candidate and must require acceptance; \
                 a candidate transition is not accepted state"
            ),
            Self::SettledTransitionRequiresAcceptance { transition } => {
                write!(f, "transition '{transition:?}' is settled and cannot require acceptance")
            }
            Self::ComparisonWithoutBaseline { transition } => write!(
                f,
                "transition '{transition:?}' compares against an accepted baseline that is absent"
            ),
            Self::ValidEvidenceFromIncompleteMeasurement { completion } => {
                write!(f, "aggregate claims valid evidence from a '{completion:?}' measurement")
            }
            Self::LegacyAdaptedRecordClaimsCurrentAuthority { unavailable_axes } => {
                let names: Vec<&str> = unavailable_axes.iter().map(|axis| axis.as_str()).collect();
                write!(
                    f,
                    "legacy-adapted evidence cannot satisfy current authority; unavailable axes: {}",
                    names.join(", ")
                )
            }
            Self::LegacyAdaptedRecordClaimsTransition { transition } => {
                write!(f, "legacy-adapted evidence may only be historical, not '{transition:?}'")
            }
        }
    }
}

impl Error for ResultReportViolation {}

/// Per-invocation result axes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct InvocationAxesResult {
    /// Exact invocation this record describes.
    pub invocation_identity: String,
    /// Separated axes for the invocation.
    pub axes: ResultAxes,
    /// Stated limits on what this invocation can prove.
    pub limitations: Vec<String>,
}

/// Per-file, per-mode result axes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FileModeAxesResult {
    /// Path relative to the prepared tree.
    pub path: String,
    /// Harness mode the record describes.
    pub mode: HarnessMode,
    /// Separated axes for this file and mode.
    pub axes: ResultAxes,
    /// Failure bucket, when one was assigned.
    pub bucket: Option<String>,
}

/// A complete v2 result report for one measured series.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RunAxesReport {
    /// Schema version; always [`RESULT_AXES_SCHEMA_VERSION`] for current records.
    pub schema_version: String,
    /// Whether this was measured now or adapted from history.
    pub origin: EvidenceOrigin,
    /// How completely the measurement ran.
    pub completion: MeasurementCompletion,
    /// Exact denominator and subject identity.
    pub subject: ResultSubjectIdentity,
    /// Run-level axis rollup.
    pub axes: ResultAxes,
    /// Parse rail summary.
    pub parse: ParseRailSummary,
    /// Admission, debt, and support distribution.
    pub admission: AdmissionSummary,
    /// Optional correctness and execution rails, by rail name.
    pub correctness_rails: BTreeMap<String, CorrectnessRailSummary>,
    /// Relationship to accepted state.
    pub current_versus_accepted: CurrentVersusAccepted,
    /// Per-file, per-mode records.
    pub files: Vec<FileModeAxesResult>,
    /// Per-invocation records.
    pub invocations: Vec<InvocationAxesResult>,
    /// What this report deliberately does not claim.
    pub claim_boundary: String,
    /// Stated limitations on the report as a whole.
    pub limitations: Vec<String>,
}

impl RunAxesReport {
    /// Reject a report whose parts do not support the whole.
    ///
    /// Runs every component validator plus the cross-part rules: valid evidence
    /// needs a complete measurement, and legacy-adapted evidence stays
    /// historical.
    ///
    /// # Errors
    ///
    /// Returns the first [`ResultReportViolation`] found.
    pub fn validate(&self) -> Result<(), ResultReportViolation> {
        if self.schema_version != RESULT_AXES_SCHEMA_VERSION {
            return Err(ResultReportViolation::UnexpectedSchemaVersion {
                found: self.schema_version.clone(),
            });
        }
        self.subject.validate()?;
        self.admission.validate(self.subject.denominator)?;
        self.current_versus_accepted.validate()?;

        if self.parse.files_passed > self.parse.files_total {
            return Err(ResultReportViolation::ParsePassedExceedsTotal);
        }
        if self.parse.files_total != self.subject.denominator {
            return Err(ResultReportViolation::ParseTotalMismatch {
                counted: self.parse.files_total,
                denominator: self.subject.denominator,
            });
        }
        require_non_blank("claim_boundary", &self.claim_boundary)?;
        for limitation in &self.limitations {
            require_non_blank("limitations", limitation)?;
        }
        for file in &self.files {
            require_non_blank("path", &file.path)?;
            if let Some(bucket) = &file.bucket {
                require_non_blank("bucket", bucket)?;
            }
        }
        for invocation in &self.invocations {
            require_non_blank("invocation_identity", &invocation.invocation_identity)?;
            for limitation in &invocation.limitations {
                require_non_blank("limitations", limitation)?;
            }
        }
        for (rail, summary) in &self.correctness_rails {
            require_non_blank("correctness_rails", rail)?;
            summary.validate(rail)?;
        }
        if self.axes.evidence() == EvidenceValidity::Valid {
            if self.completion != MeasurementCompletion::Complete {
                return Err(ResultReportViolation::ValidEvidenceFromIncompleteMeasurement {
                    completion: self.completion,
                });
            }
            // A finished run that observed nothing is a contradiction: it would
            // otherwise present itself as complete, valid, and authoritative
            // while every axis says nothing was assessed.
            if !self.axes.observation().is_domain_outcome() {
                return Err(ResultReportViolation::CompleteMeasurementObservedNothing);
            }
        }
        if self.axes.support().is_positive_claim() {
            // A run-level support claim names the mechanism that produced it, so
            // some rail must actually have run with that mechanism. Otherwise the
            // aggregate asserts evidence no rail in the report carries.
            let mechanism = self.axes.mechanism();
            // General support needs a rail whose evidence is usable now. A partial
            // rail covers only a declared subset and a stale one has outlived its
            // freshness contract, so neither can carry a general claim across the
            // whole denominator.
            let backed = self.correctness_rails.values().any(|rail| {
                rail.mechanism == mechanism
                    && match self.axes.support() {
                        SemanticSupport::General => {
                            rail.availability == CompatibilityRailAvailability::Available
                        }
                        _ => rail.ran(),
                    }
            });
            if !backed {
                return Err(ResultReportViolation::AggregateSupportWithoutRail {
                    support: self.axes.support(),
                    mechanism,
                });
            }
        }
        if self.axes.observation() == ObservedOutcome::Clean {
            let failing_detail = self
                .files
                .iter()
                .any(|file| file.axes.observation() == ObservedOutcome::FailuresObserved)
                || self
                    .invocations
                    .iter()
                    .any(|run| run.axes.observation() == ObservedOutcome::FailuresObserved);
            if failing_detail {
                return Err(ResultReportViolation::CleanAggregateOverFailingDetail);
            }
        }
        if self.origin == EvidenceOrigin::LegacyAdapted {
            if self.current_versus_accepted.transition != CompatibilityTransition::Historical {
                return Err(ResultReportViolation::LegacyAdaptedRecordClaimsTransition {
                    transition: self.current_versus_accepted.transition,
                });
            }
            let unavailable = self.axes.not_assessed_axes();
            if !unavailable.is_empty() && self.axes.support().is_positive_claim() {
                return Err(ResultReportViolation::LegacyAdaptedRecordClaimsCurrentAuthority {
                    unavailable_axes: unavailable,
                });
            }
        }
        Ok(())
    }

    /// Whether this report may serve as current authority for its series.
    ///
    /// # Errors
    ///
    /// Returns a [`ResultReportViolation`] when the report is structurally
    /// invalid, when its evidence is not valid, or when it is legacy-adapted.
    pub fn admissible_as_current_authority(&self) -> Result<(), ResultReportViolation> {
        self.validate()?;
        if self.origin == EvidenceOrigin::LegacyAdapted {
            return Err(ResultReportViolation::LegacyAdaptedRecordClaimsCurrentAuthority {
                unavailable_axes: self.axes.not_assessed_axes(),
            });
        }
        if self.axes.evidence() != EvidenceValidity::Valid {
            // Stale, cancelled, invalid, and not-proven evidence all fail here
            // for their own reason; reporting them as an incomplete measurement
            // would point the reader at the wrong axis.
            return Err(ResultReportViolation::EvidenceNotValidForCurrentAuthority {
                evidence: self.axes.evidence(),
            });
        }
        Ok(())
    }
}

/// One v1 record translated forward, naming everything it cannot establish.
///
/// Authority is **derived, never stored**. An earlier revision carried a
/// `sufficient_for_current_authority` field, but a public — and deserializable —
/// flag is exactly the forgeable surface this module exists to remove: a
/// hand-written receipt could set it and walk past the boundary. It is now
/// computed from the axes, which have themselves passed [`ResultAxes::new`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LegacyAdaptation {
    /// Axes derivable from the historical record.
    pub axes: ResultAxes,
    /// Schema version the record came from.
    pub source_schema_version: String,
}

impl LegacyAdaptation {
    /// Axes the historical record simply does not contain.
    ///
    /// Derived from the axes rather than declared, so it cannot disagree
    /// with them.
    #[must_use]
    pub fn unavailable_axes(&self) -> Vec<ResultAxis> {
        self.axes.not_assessed_axes()
    }

    /// Whether the adapted record can satisfy current authority.
    ///
    /// Always false. A `LegacyAdaptation` is historical evidence by
    /// construction, so no arrangement of its axes promotes it — deriving this
    /// from the axes would still let a forged record with every axis filled in
    /// claim authority it never earned.
    #[must_use]
    pub fn sufficient_for_current_authority(&self) -> bool {
        false
    }

    /// Reject an adapted record being used as current authority.
    ///
    /// # Errors
    ///
    /// Always returns
    /// [`ResultReportViolation::LegacyAdaptedRecordClaimsCurrentAuthority`],
    /// naming the axes the historical evidence could not fill.
    pub fn require_current_authority(&self) -> Result<(), ResultReportViolation> {
        Err(ResultReportViolation::LegacyAdaptedRecordClaimsCurrentAuthority {
            unavailable_axes: self.unavailable_axes(),
        })
    }
}

/// Translate a v1 [`RunnerStatus`] forward without inventing facts.
///
/// A v1 `pass` records that the runner did not fail. It says nothing about
/// admission, semantic support, or which mechanism ran, so those axes stay
/// unassessed and the adaptation is never sufficient for current authority.
///
/// A v1 `fail` is **not** promoted to a product failure. `RunnerStatus::Fail`
/// is also written for a test that was discovered but produced no runner record
/// at all — a harness failure, recorded under the `harness_prepare` bucket —
/// so the bare status cannot distinguish a product regression from an
/// instrument problem. Reading it as `failures_observed` would commit exactly
/// the conflation this module exists to prevent, so it adapts to unproven
/// evidence with no product outcome established.
#[must_use]
pub fn adapt_legacy_runner_status(
    status: RunnerStatus,
    source_schema_version: impl Into<String>,
) -> LegacyAdaptation {
    match status {
        RunnerStatus::Pass => legacy_clean(source_schema_version),
        RunnerStatus::Fail => legacy_indeterminate_failure(source_schema_version),
    }
}

/// Translate a v1 [`SmokeStatus`] forward without inventing facts.
///
/// `SmokeStatus::Fail` is derived purely from structural failures — a missing
/// report, a profile mismatch, an unbucketed or unknown-bucket failure, or a
/// semantic boundary — so it never asserts on its own that the product failed.
/// It adapts to unproven evidence rather than a product outcome.
#[must_use]
pub fn adapt_legacy_smoke_status(
    status: SmokeStatus,
    source_schema_version: impl Into<String>,
) -> LegacyAdaptation {
    match status {
        SmokeStatus::Pass => legacy_clean(source_schema_version),
        SmokeStatus::Fail => legacy_indeterminate_failure(source_schema_version),
    }
}

/// A v1 record that reported no failure of any kind.
fn legacy_clean(source_schema_version: impl Into<String>) -> LegacyAdaptation {
    legacy_adaptation(EvidenceValidity::Valid, ObservedOutcome::Clean, source_schema_version)
}

/// A v1 failure whose kind the historical record cannot establish.
fn legacy_indeterminate_failure(source_schema_version: impl Into<String>) -> LegacyAdaptation {
    legacy_adaptation(
        EvidenceValidity::NotProven,
        ObservedOutcome::ProcessOrProtocolFailed,
        source_schema_version,
    )
}

fn legacy_adaptation(
    evidence: EvidenceValidity,
    observation: ObservedOutcome,
    source_schema_version: impl Into<String>,
) -> LegacyAdaptation {
    let axes = ResultAxes {
        evidence,
        observation,
        admission: CompatibilityAdmission::NotAssessed,
        support: SemanticSupport::NotAssessed,
        mechanism: CorrectnessMechanism::None,
    };
    LegacyAdaptation { axes, source_schema_version: source_schema_version.into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn axes(
        evidence: EvidenceValidity,
        observation: ObservedOutcome,
        admission: CompatibilityAdmission,
        support: SemanticSupport,
        mechanism: CorrectnessMechanism,
    ) -> Result<ResultAxes, ResultAxisViolation> {
        ResultAxes::new(evidence, observation, admission, support, mechanism)
    }

    fn subject() -> ResultSubjectIdentity {
        ResultSubjectIdentity {
            series_id: "upstream-base".to_string(),
            subject_identity: "perl-compiler@abcdef".to_string(),
            evidence_bundle_id: "bundle-1".to_string(),
            measurement_sha: "a".repeat(40),
            denominator: 4,
        }
    }

    fn distribution(pairs: &[(&str, usize)]) -> BTreeMap<String, usize> {
        pairs.iter().map(|(key, value)| ((*key).to_string(), *value)).collect()
    }

    fn report(axes: ResultAxes, transition: CurrentVersusAccepted) -> RunAxesReport {
        RunAxesReport {
            schema_version: RESULT_AXES_SCHEMA_VERSION.to_string(),
            origin: EvidenceOrigin::CurrentMeasurement,
            completion: MeasurementCompletion::Complete,
            subject: subject(),
            axes,
            parse: ParseRailSummary { files_total: 4, files_passed: 4, axes },
            admission: AdmissionSummary {
                by_admission: distribution(&[("implemented", 4)]),
                by_support: distribution(&[("general", 4)]),
                accepted_debt_count: 0,
                source_locked_count: 0,
                downstream_blocking_count: 0,
            },
            correctness_rails: BTreeMap::from([(
                "eir".to_string(),
                CorrectnessRailSummary {
                    mechanism: CorrectnessMechanism::EirExecution,
                    availability: CompatibilityRailAvailability::Available,
                    reason: "eir rail ran".to_string(),
                    evidence_refs: vec!["bundle-1".to_string()],
                    files_total: Some(4),
                    files_passed: Some(4),
                },
            )]),
            current_versus_accepted: transition,
            files: Vec::new(),
            invocations: Vec::new(),
            claim_boundary: "compile admission only".to_string(),
            limitations: Vec::new(),
        }
    }

    fn no_change() -> CurrentVersusAccepted {
        CurrentVersusAccepted {
            transition: CompatibilityTransition::NoChange,
            requires_acceptance: false,
            accepted_baseline_digest: Some("sha256:0".to_string()),
            reason: "identical to accepted state".to_string(),
        }
    }

    // ---- Fixture class 1: valid clean implemented observation ----------------

    #[test]
    fn valid_clean_implemented_observation_is_representable() -> Result<(), ResultAxisViolation> {
        let observed = axes(
            EvidenceValidity::Valid,
            ObservedOutcome::Clean,
            CompatibilityAdmission::Implemented,
            SemanticSupport::General,
            CorrectnessMechanism::EirExecution,
        )?;
        assert_eq!(observed.evidence(), EvidenceValidity::Valid);
        assert_eq!(observed.support(), SemanticSupport::General);
        assert!(observed.not_assessed_axes().is_empty());
        Ok(())
    }

    // ---- Fixture class 2: valid complete product regression ------------------

    #[test]
    fn complete_red_observation_keeps_valid_evidence() -> Result<(), ResultAxisViolation> {
        let observed = axes(
            EvidenceValidity::Valid,
            ObservedOutcome::FailuresObserved,
            CompatibilityAdmission::Implemented,
            SemanticSupport::Partial,
            CorrectnessMechanism::RealPerlOracle,
        )?;
        assert_eq!(observed.evidence(), EvidenceValidity::Valid);
        assert_eq!(observed.observation(), ObservedOutcome::FailuresObserved);
        Ok(())
    }

    // ---- Fixture class 3: source-locked debt, downstream support blocked -----

    #[test]
    fn source_locked_debt_admits_without_support() -> Result<(), ResultAxisViolation> {
        let observed = axes(
            EvidenceValidity::Valid,
            ObservedOutcome::Clean,
            CompatibilityAdmission::AcceptedDebt,
            SemanticSupport::Blocked,
            CorrectnessMechanism::None,
        )?;
        assert_eq!(observed.admission(), CompatibilityAdmission::AcceptedDebt);
        assert_eq!(observed.support(), SemanticSupport::Blocked);
        Ok(())
    }

    // ---- Fixture class 4: valid statically classified file -------------------

    #[test]
    fn statically_classified_file_may_reach_partial_only() -> Result<(), ResultAxisViolation> {
        let observed = axes(
            EvidenceValidity::Valid,
            ObservedOutcome::Clean,
            CompatibilityAdmission::StaticallyClassified,
            SemanticSupport::Partial,
            CorrectnessMechanism::FixtureReplay,
        )?;
        assert_eq!(observed.support(), SemanticSupport::Partial);

        let escalated = axes(
            EvidenceValidity::Valid,
            ObservedOutcome::Clean,
            CompatibilityAdmission::StaticallyClassified,
            SemanticSupport::General,
            CorrectnessMechanism::EirExecution,
        );
        assert_eq!(
            escalated,
            Err(ResultAxisViolation::AdmissionCannotUnderwriteGeneralSupport {
                admission: CompatibilityAdmission::StaticallyClassified,
            }),
            "compile admission must not imply general semantic support"
        );
        Ok(())
    }

    // ---- Fixture class 5: unsupported compiler result ------------------------

    #[test]
    fn unsupported_admission_cannot_carry_positive_support() {
        let rejected = axes(
            EvidenceValidity::Valid,
            ObservedOutcome::Clean,
            CompatibilityAdmission::Unsupported,
            SemanticSupport::Partial,
            CorrectnessMechanism::EirExecution,
        );
        assert_eq!(
            rejected,
            Err(ResultAxisViolation::AdmissionCannotUnderwritePositiveSupport {
                admission: CompatibilityAdmission::Unsupported,
                support: SemanticSupport::Partial,
            })
        );
    }

    // ---- Fixture class 6: process/instrument NOT_PROVEN ----------------------

    #[test]
    fn instrument_failure_is_not_proven_not_a_product_result() -> Result<(), ResultAxisViolation> {
        let observed = axes(
            EvidenceValidity::NotProven,
            ObservedOutcome::ProcessOrProtocolFailed,
            CompatibilityAdmission::NotAssessed,
            SemanticSupport::NotAssessed,
            CorrectnessMechanism::None,
        )?;
        assert_eq!(observed.evidence(), EvidenceValidity::NotProven);

        let forged = axes(
            EvidenceValidity::Valid,
            ObservedOutcome::ProcessOrProtocolFailed,
            CompatibilityAdmission::NotAssessed,
            SemanticSupport::NotAssessed,
            CorrectnessMechanism::None,
        );
        assert_eq!(forged, Err(ResultAxisViolation::ProcessFailureClaimedAsValidEvidence));
        Ok(())
    }

    #[test]
    fn not_proven_evidence_cannot_carry_a_domain_outcome() {
        let rejected = axes(
            EvidenceValidity::NotProven,
            ObservedOutcome::Clean,
            CompatibilityAdmission::NotAssessed,
            SemanticSupport::NotAssessed,
            CorrectnessMechanism::None,
        );
        assert_eq!(
            rejected,
            Err(ResultAxisViolation::DomainOutcomeWithoutValidEvidence {
                evidence: EvidenceValidity::NotProven,
                observation: ObservedOutcome::Clean,
            })
        );
    }

    // ---- Fixture class 7: stale and cancelled evidence -----------------------

    #[test]
    fn stale_and_cancelled_evidence_assess_nothing() {
        for evidence in [EvidenceValidity::Stale, EvidenceValidity::Cancelled] {
            let observed = ResultAxes::not_assessed(evidence);
            assert_eq!(observed.evidence(), evidence);
            assert_eq!(observed.observation(), ObservedOutcome::NotAssessed);
            assert_eq!(
                observed.not_assessed_axes(),
                vec![
                    ResultAxis::Observation,
                    ResultAxis::Admission,
                    ResultAxis::Support,
                    ResultAxis::Mechanism,
                ]
            );

            let forged = axes(
                evidence,
                ObservedOutcome::Clean,
                CompatibilityAdmission::Implemented,
                SemanticSupport::NotAssessed,
                CorrectnessMechanism::None,
            );
            assert!(forged.is_err(), "{evidence} evidence must not carry a clean observation");
        }
    }

    // ---- Fixture class 8: fixture replay without runtime support -------------

    #[test]
    fn fixture_replay_cannot_become_general_support() {
        let rejected = axes(
            EvidenceValidity::Valid,
            ObservedOutcome::Clean,
            CompatibilityAdmission::Implemented,
            SemanticSupport::General,
            CorrectnessMechanism::FixtureReplay,
        );
        assert_eq!(rejected, Err(ResultAxisViolation::FixtureReplayClaimedGeneralSupport));
    }

    // ---- Fixture class 9: EIR execution with declared profile ----------------

    #[test]
    fn eir_execution_may_underwrite_general_support() -> Result<(), ResultAxisViolation> {
        let observed = axes(
            EvidenceValidity::Valid,
            ObservedOutcome::Clean,
            CompatibilityAdmission::Implemented,
            SemanticSupport::General,
            CorrectnessMechanism::EirExecution,
        )?;
        assert!(observed.mechanism().executes_behavior());
        Ok(())
    }

    // ---- Fixture class 10: unavailable oracle/gold/EIR rails ------------------

    #[test]
    fn absent_rails_stay_unavailable_rather_than_zero() -> Result<(), ResultReportViolation> {
        for rail in ["curated_gold", "differential_oracle", "eir"] {
            let summary = CorrectnessRailSummary::unavailable("no rail provisioned");
            summary.validate(rail)?;
            assert_eq!(summary.files_total, None, "absent rail must not report a denominator");
            assert_eq!(summary.files_passed, None, "absent rail must not report passes");
            assert_ne!(summary.availability, CompatibilityRailAvailability::Available);
        }
        Ok(())
    }

    #[test]
    fn positive_support_requires_a_mechanism() {
        let rejected = axes(
            EvidenceValidity::Valid,
            ObservedOutcome::Clean,
            CompatibilityAdmission::Implemented,
            SemanticSupport::Partial,
            CorrectnessMechanism::None,
        );
        assert_eq!(
            rejected,
            Err(ResultAxisViolation::PositiveSupportWithoutMechanism {
                support: SemanticSupport::Partial,
            })
        );
    }

    // ---- Fixture class 11: candidate stronger than accepted, unaccepted ------

    #[test]
    fn improvement_candidate_is_not_accepted_state() -> Result<(), ResultReportViolation> {
        let candidate = CurrentVersusAccepted {
            transition: CompatibilityTransition::ImprovementCandidate,
            requires_acceptance: true,
            accepted_baseline_digest: Some("sha256:1".to_string()),
            reason: "more files admitted than the accepted ratchet".to_string(),
        };
        candidate.validate()?;

        let forged = CurrentVersusAccepted { requires_acceptance: false, ..candidate };
        assert_eq!(
            forged.validate(),
            Err(ResultReportViolation::CandidateTreatedAsAccepted {
                transition: CompatibilityTransition::ImprovementCandidate,
            })
        );
        Ok(())
    }

    // ---- Fixture class 12: observation weaker than the accepted ratchet ------

    /// Shaped exactly as `classify_compatibility_transition` in perl-core-harness
    /// emits a regression: `requires_acceptance` is false, because a regression is
    /// a blocking signal rather than something a reviewer adopts into the ratchet.
    #[test]
    fn regression_requires_a_baseline_to_regress_from() -> Result<(), ResultReportViolation> {
        let regression = CurrentVersusAccepted {
            transition: CompatibilityTransition::Regression,
            requires_acceptance: false,
            accepted_baseline_digest: Some("sha256:2".to_string()),
            reason: "fewer files admitted than the accepted ratchet".to_string(),
        };
        regression.validate()?;

        let adopted = CurrentVersusAccepted { requires_acceptance: true, ..regression.clone() };
        assert_eq!(
            adopted.validate(),
            Err(ResultReportViolation::SettledTransitionRequiresAcceptance {
                transition: CompatibilityTransition::Regression,
            }),
            "a regression is not accepted into state"
        );

        let baseless = CurrentVersusAccepted { accepted_baseline_digest: None, ..regression };
        assert_eq!(
            baseless.validate(),
            Err(ResultReportViolation::ComparisonWithoutBaseline {
                transition: CompatibilityTransition::Regression,
            })
        );
        Ok(())
    }

    // ---- Fixture class 13: legacy v1 pass with insufficient information ------

    #[test]
    fn legacy_v1_pass_does_not_become_implemented_or_general() {
        let adapted = adapt_legacy_runner_status(RunnerStatus::Pass, "perl_core_harness.report.v1");
        assert_eq!(adapted.axes.observation(), ObservedOutcome::Clean);
        assert_eq!(adapted.axes.admission(), CompatibilityAdmission::NotAssessed);
        assert_eq!(adapted.axes.support(), SemanticSupport::NotAssessed);
        assert_eq!(adapted.axes.mechanism(), CorrectnessMechanism::None);
        assert_eq!(
            adapted.unavailable_axes(),
            vec![ResultAxis::Admission, ResultAxis::Support, ResultAxis::Mechanism],
            "the adapter must name every axis the legacy record cannot fill"
        );
        assert!(!adapted.sufficient_for_current_authority());
        assert_eq!(
            adapted.require_current_authority(),
            Err(ResultReportViolation::LegacyAdaptedRecordClaimsCurrentAuthority {
                unavailable_axes: vec![
                    ResultAxis::Admission,
                    ResultAxis::Support,
                    ResultAxis::Mechanism,
                ],
            })
        );
    }

    /// A `LegacyAdaptation` is historical by construction. Deriving authority
    /// from its axes still let a forged record with every axis filled in claim
    /// it, so the refusal must be unconditional.
    #[test]
    fn a_fully_assessed_legacy_record_still_cannot_claim_authority()
    -> Result<(), Box<dyn std::error::Error>> {
        let forged = LegacyAdaptation {
            axes: ResultAxes::new(
                EvidenceValidity::Valid,
                ObservedOutcome::Clean,
                CompatibilityAdmission::Implemented,
                SemanticSupport::General,
                CorrectnessMechanism::EirExecution,
            )?,
            source_schema_version: "perl_core_harness.report.v1".to_string(),
        };
        assert!(
            forged.unavailable_axes().is_empty(),
            "this fixture is only meaningful while every axis is assessed"
        );
        assert!(!forged.sufficient_for_current_authority());
        assert!(
            forged.require_current_authority().is_err(),
            "legacy evidence is historical however complete its axes look"
        );

        // The same forgery through deserialization.
        let decoded: LegacyAdaptation = serde_json::from_str(
            r#"{"axes":{"evidence":"valid","observation":"clean","admission":"implemented","support":"general","mechanism":"eir_execution"},"source_schema_version":"perl_core_harness.report.v1"}"#,
        )?;
        assert!(decoded.require_current_authority().is_err());
        Ok(())
    }

    /// A bare v1 `fail` cannot tell a product regression from a harness
    /// failure: `SmokeStatus::Fail` is derived only from structural failures,
    /// and `RunnerStatus::Fail` is also written for a discovered test that
    /// produced no runner record. Neither may be promoted to a product outcome.
    #[test]
    fn an_ambiguous_v1_failure_does_not_become_a_product_failure() {
        for adapted in [
            adapt_legacy_smoke_status(SmokeStatus::Fail, "perl_core_harness.smoke.v1"),
            adapt_legacy_runner_status(RunnerStatus::Fail, "perl_core_harness.report.v1"),
        ] {
            assert_eq!(
                adapted.axes.observation(),
                ObservedOutcome::ProcessOrProtocolFailed,
                "a v1 failure of unknown kind must not read as an observed product failure"
            );
            assert_eq!(adapted.axes.evidence(), EvidenceValidity::NotProven);
            assert!(!adapted.sufficient_for_current_authority());
        }
    }

    #[test]
    fn legacy_adapted_report_cannot_be_current_authority() -> Result<(), ResultAxisViolation> {
        let adapted = adapt_legacy_runner_status(RunnerStatus::Pass, "perl_core_harness.report.v1");
        let mut historical = report(
            adapted.axes,
            CurrentVersusAccepted {
                transition: CompatibilityTransition::Historical,
                requires_acceptance: false,
                accepted_baseline_digest: None,
                reason: "retained for history".to_string(),
            },
        );
        historical.origin = EvidenceOrigin::LegacyAdapted;
        historical.admission.by_admission = distribution(&[("not_assessed", 4)]);
        historical.admission.by_support = distribution(&[("not_assessed", 4)]);

        assert_eq!(historical.validate(), Ok(()), "a historical record stays readable");
        assert!(
            matches!(
                historical.admissible_as_current_authority(),
                Err(ResultReportViolation::LegacyAdaptedRecordClaimsCurrentAuthority { .. })
            ),
            "legacy evidence must not silently satisfy current authority"
        );

        let mut promoted = historical;
        promoted.current_versus_accepted.transition = CompatibilityTransition::ImprovementCandidate;
        promoted.current_versus_accepted.requires_acceptance = true;
        promoted.current_versus_accepted.accepted_baseline_digest = Some("sha256:3".to_string());
        assert_eq!(
            promoted.validate(),
            Err(ResultReportViolation::LegacyAdaptedRecordClaimsTransition {
                transition: CompatibilityTransition::ImprovementCandidate,
            })
        );
        Ok(())
    }

    // ---- Fixture class 14: unknown fields and unknown enum values ------------

    #[test]
    fn axes_reject_unknown_fields() {
        let json = r#"{
            "evidence": "valid",
            "observation": "clean",
            "admission": "implemented",
            "support": "general",
            "mechanism": "eir_execution",
            "unexpected_authority_field": true
        }"#;
        let err = serde_json::from_str::<ResultAxes>(json).err();
        assert!(
            err.is_some_and(|err| err.to_string().contains("unexpected_authority_field")),
            "unknown field must be rejected"
        );
    }

    #[test]
    fn axes_reject_unknown_enum_values() {
        let json = r#"{
            "evidence": "probably_fine",
            "observation": "clean",
            "admission": "implemented",
            "support": "general",
            "mechanism": "eir_execution"
        }"#;
        assert!(
            serde_json::from_str::<ResultAxes>(json).is_err(),
            "an unknown evidence value must not deserialize"
        );
    }

    // ---- Fixture class 15: source-locked debt forged as general support ------

    #[test]
    fn accepted_debt_cannot_be_forged_into_general_support() {
        let rejected = axes(
            EvidenceValidity::Valid,
            ObservedOutcome::Clean,
            CompatibilityAdmission::AcceptedDebt,
            SemanticSupport::General,
            CorrectnessMechanism::RealPerlOracle,
        );
        assert_eq!(
            rejected,
            Err(ResultAxisViolation::AdmissionCannotUnderwriteGeneralSupport {
                admission: CompatibilityAdmission::AcceptedDebt,
            })
        );
    }

    #[test]
    fn deserialization_cannot_bypass_the_constructor() {
        let json = r#"{
            "evidence": "valid",
            "observation": "clean",
            "admission": "accepted_debt",
            "support": "general",
            "mechanism": "real_perl_oracle"
        }"#;
        let err = serde_json::from_str::<ResultAxes>(json)
            .err()
            .map(|err| err.to_string())
            .unwrap_or_default();
        assert!(
            err.contains("cannot become semantic support 'general'"),
            "a forged receipt must be rejected on read, got: {err}"
        );
    }

    // ---- Fixture class 16: compile counts inserted into runtime totals -------

    #[test]
    fn runtime_rail_cannot_borrow_compile_counts() {
        let forged = CorrectnessRailSummary {
            mechanism: CorrectnessMechanism::None,
            availability: CompatibilityRailAvailability::NotAvailable,
            reason: "no execution rail provisioned".to_string(),
            evidence_refs: Vec::new(),
            files_total: Some(4),
            files_passed: Some(4),
        };
        assert_eq!(
            forged.validate("execution"),
            Err(ResultReportViolation::AbsentRailReportsWork {
                rail: "execution".to_string(),
                aspect: "file counts",
            })
        );
    }

    /// An absent rail must stay absent in *every* field. Keying absence off the
    /// mechanism alone let `not_available` coexist with a real mechanism and a
    /// full set of passing counts.
    #[test]
    fn an_unavailable_rail_cannot_claim_a_mechanism_or_counts() {
        let forged = CorrectnessRailSummary {
            mechanism: CorrectnessMechanism::RealPerlOracle,
            availability: CompatibilityRailAvailability::NotAvailable,
            reason: "claimed absent".to_string(),
            evidence_refs: Vec::new(),
            files_total: Some(100),
            files_passed: Some(100),
        };
        assert_eq!(
            forged.validate("execution"),
            Err(ResultReportViolation::AbsentRailReportsWork {
                rail: "execution".to_string(),
                aspect: "a correctness mechanism",
            }),
            "availability, not mechanism, decides whether a rail ran"
        );

        let evidence_only = CorrectnessRailSummary {
            mechanism: CorrectnessMechanism::None,
            availability: CompatibilityRailAvailability::NotAvailable,
            reason: "claimed absent".to_string(),
            evidence_refs: vec!["bundle-1".to_string()],
            files_total: None,
            files_passed: None,
        };
        assert_eq!(
            evidence_only.validate("eir"),
            Err(ResultReportViolation::AbsentRailReportsWork {
                rail: "eir".to_string(),
                aspect: "evidence references",
            })
        );
    }

    #[test]
    fn a_failing_rail_is_named_even_when_it_is_not_a_known_rail() -> Result<(), ResultAxisViolation>
    {
        let mut candidate = report(
            axes(
                EvidenceValidity::Valid,
                ObservedOutcome::Clean,
                CompatibilityAdmission::Implemented,
                SemanticSupport::General,
                CorrectnessMechanism::EirExecution,
            )?,
            no_change(),
        );
        candidate.correctness_rails.insert(
            "some_future_rail".to_string(),
            CorrectnessRailSummary {
                mechanism: CorrectnessMechanism::None,
                availability: CompatibilityRailAvailability::NotAvailable,
                reason: "not provisioned".to_string(),
                evidence_refs: Vec::new(),
                files_total: Some(1),
                files_passed: Some(1),
            },
        );

        assert_eq!(
            candidate.validate(),
            Err(ResultReportViolation::AbsentRailReportsWork {
                rail: "some_future_rail".to_string(),
                aspect: "file counts",
            }),
            "the violation must name the offending rail, not a generic placeholder"
        );
        Ok(())
    }

    #[test]
    fn a_rail_that_ran_needs_a_mechanism_evidence_and_counts() {
        let mechanismless = CorrectnessRailSummary {
            mechanism: CorrectnessMechanism::None,
            availability: CompatibilityRailAvailability::Available,
            reason: "claimed available".to_string(),
            evidence_refs: vec!["bundle-1".to_string()],
            files_total: None,
            files_passed: None,
        };
        assert_eq!(
            mechanismless.validate("eir"),
            Err(ResultReportViolation::RailRanWithoutMechanism { rail: "eir".to_string() })
        );

        let evidenceless = CorrectnessRailSummary {
            mechanism: CorrectnessMechanism::EirExecution,
            availability: CompatibilityRailAvailability::Available,
            reason: "claimed available".to_string(),
            evidence_refs: Vec::new(),
            files_total: Some(4),
            files_passed: Some(4),
        };
        assert_eq!(
            evidenceless.validate("eir"),
            Err(ResultReportViolation::RailRanWithoutEvidence { rail: "eir".to_string() })
        );

        let countless = CorrectnessRailSummary {
            mechanism: CorrectnessMechanism::EirExecution,
            availability: CompatibilityRailAvailability::Available,
            reason: "claimed available".to_string(),
            evidence_refs: vec!["bundle-1".to_string()],
            files_total: None,
            files_passed: None,
        };
        assert_eq!(
            countless.validate("eir"),
            Err(ResultReportViolation::RailAvailableWithoutCounts { rail: "eir".to_string() })
        );
    }

    // ---- Fixture class 17: missing denominator/series/subject ----------------

    #[test]
    fn aggregates_require_denominator_and_subject() {
        let mut identity = subject();
        identity.denominator = 0;
        assert_eq!(identity.validate(), Err(ResultReportViolation::MissingDenominator));

        let mut identity = subject();
        identity.series_id = String::new();
        assert_eq!(
            identity.validate(),
            Err(ResultReportViolation::MissingSubjectIdentity { field: "series_id" })
        );

        let mut identity = subject();
        identity.subject_identity = "   ".to_string();
        assert_eq!(
            identity.validate(),
            Err(ResultReportViolation::MissingSubjectIdentity { field: "subject_identity" })
        );
    }

    #[test]
    fn admission_distribution_must_describe_the_denominator() -> Result<(), ResultAxisViolation> {
        let mut candidate = report(
            axes(
                EvidenceValidity::Valid,
                ObservedOutcome::Clean,
                CompatibilityAdmission::Implemented,
                SemanticSupport::General,
                CorrectnessMechanism::EirExecution,
            )?,
            no_change(),
        );
        assert_eq!(candidate.validate(), Ok(()));

        candidate.admission.by_admission = distribution(&[("implemented", 3)]);
        assert_eq!(
            candidate.validate(),
            Err(ResultReportViolation::AdmissionTotalMismatch { counted: 3, denominator: 4 })
        );
        Ok(())
    }

    #[test]
    fn valid_evidence_requires_a_complete_measurement() -> Result<(), ResultAxisViolation> {
        let mut candidate = report(
            axes(
                EvidenceValidity::Valid,
                ObservedOutcome::Clean,
                CompatibilityAdmission::Implemented,
                SemanticSupport::General,
                CorrectnessMechanism::EirExecution,
            )?,
            no_change(),
        );
        candidate.completion = MeasurementCompletion::Incomplete;
        assert_eq!(
            candidate.validate(),
            Err(ResultReportViolation::ValidEvidenceFromIncompleteMeasurement {
                completion: MeasurementCompletion::Incomplete,
            })
        );
        Ok(())
    }

    // ---- Fixture class 18: deterministic serialization -----------------------

    #[test]
    fn serialization_is_deterministic_across_input_order() -> Result<(), Box<dyn std::error::Error>>
    {
        let observed = ResultAxes::new(
            EvidenceValidity::Valid,
            ObservedOutcome::Clean,
            CompatibilityAdmission::Implemented,
            SemanticSupport::General,
            CorrectnessMechanism::EirExecution,
        )?;

        let mut forward = report(observed, no_change());
        forward
            .correctness_rails
            .insert("eir".to_string(), CorrectnessRailSummary::unavailable("not provisioned"));
        forward
            .correctness_rails
            .insert("curated_gold".to_string(), CorrectnessRailSummary::unavailable("no gold"));

        let mut reverse = report(observed, no_change());
        reverse
            .correctness_rails
            .insert("curated_gold".to_string(), CorrectnessRailSummary::unavailable("no gold"));
        reverse
            .correctness_rails
            .insert("eir".to_string(), CorrectnessRailSummary::unavailable("not provisioned"));

        assert_eq!(serde_json::to_string(&forward)?, serde_json::to_string(&reverse)?);
        Ok(())
    }

    #[test]
    fn report_roundtrips_through_json() -> Result<(), Box<dyn std::error::Error>> {
        let observed = ResultAxes::new(
            EvidenceValidity::Valid,
            ObservedOutcome::FailuresObserved,
            CompatibilityAdmission::Implemented,
            SemanticSupport::Partial,
            CorrectnessMechanism::FixtureReplay,
        )?;
        let mut candidate = report(observed, no_change());
        candidate.files.push(FileModeAxesResult {
            path: "base/ok.t".to_string(),
            mode: HarnessMode::Compile,
            axes: observed,
            bucket: Some("compile_effect".to_string()),
        });
        candidate.invocations.push(InvocationAxesResult {
            invocation_identity: "invocation-1".to_string(),
            axes: observed,
            limitations: vec!["fixture replay only".to_string()],
        });

        let encoded = serde_json::to_string(&candidate)?;
        let decoded: RunAxesReport = serde_json::from_str(&encoded)?;
        assert_eq!(decoded, candidate);
        Ok(())
    }

    #[test]
    fn report_rejects_a_foreign_schema_version() -> Result<(), ResultAxisViolation> {
        let mut candidate = report(
            axes(
                EvidenceValidity::Valid,
                ObservedOutcome::Clean,
                CompatibilityAdmission::Implemented,
                SemanticSupport::General,
                CorrectnessMechanism::EirExecution,
            )?,
            no_change(),
        );
        candidate.schema_version = "perl_core_harness.result_axes.v1".to_string();
        assert_eq!(
            candidate.validate(),
            Err(ResultReportViolation::UnexpectedSchemaVersion {
                found: "perl_core_harness.result_axes.v1".to_string(),
            })
        );
        Ok(())
    }

    // ---- Published schema parity --------------------------------------------
    //
    // The schema is only worth publishing if it decides the same cases as the
    // Rust constructor. These tests compare the two on identical fixtures, so
    // either drifting from the other turns the suite red.

    fn schema_document() -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../schemas/perl_core_harness.result_axes.v2.schema.json");
        Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
    }

    fn schema() -> Result<jsonschema::Validator, Box<dyn std::error::Error>> {
        Ok(jsonschema::validator_for(&schema_document()?)?)
    }

    /// The published `result_axes` subschema, rooted so its `$ref`s still resolve.
    fn axes_schema() -> Result<jsonschema::Validator, Box<dyn std::error::Error>> {
        let document = schema_document()?;
        let defs = document.get("$defs").ok_or("schema is missing $defs")?.clone();
        if defs.get("result_axes").is_none() {
            return Err("schema is missing $defs.result_axes".into());
        }
        let rooted = serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$ref": "#/$defs/result_axes",
            "$defs": defs,
        });
        Ok(jsonschema::validator_for(&rooted)?)
    }

    /// Every axis combination the constructor decides, as a wire object.
    fn axis_fixtures() -> Vec<(&'static str, serde_json::Value)> {
        let case = |evidence, observation, admission, support, mechanism| {
            serde_json::json!({
                "evidence": evidence,
                "observation": observation,
                "admission": admission,
                "support": support,
                "mechanism": mechanism,
            })
        };
        vec![
            (
                "clean implemented general",
                case("valid", "clean", "implemented", "general", "eir_execution"),
            ),
            (
                "complete red observation",
                case("valid", "failures_observed", "implemented", "partial", "real_perl_oracle"),
            ),
            (
                "source-locked debt blocked",
                case("valid", "clean", "accepted_debt", "blocked", "none"),
            ),
            (
                "statically classified partial",
                case("valid", "clean", "statically_classified", "partial", "fixture_replay"),
            ),
            (
                "unsupported unavailable",
                case("valid", "clean", "unsupported", "unavailable", "none"),
            ),
            (
                "instrument not proven",
                case(
                    "not_proven",
                    "process_or_protocol_failed",
                    "not_assessed",
                    "not_assessed",
                    "none",
                ),
            ),
            (
                "stale not assessed",
                case("stale", "not_assessed", "not_assessed", "not_assessed", "none"),
            ),
            (
                "cancelled not assessed",
                case("cancelled", "not_assessed", "not_assessed", "not_assessed", "none"),
            ),
            (
                "forged: debt claims general",
                case("valid", "clean", "accepted_debt", "general", "real_perl_oracle"),
            ),
            (
                "forged: static classification claims general",
                case("valid", "clean", "statically_classified", "general", "eir_execution"),
            ),
            (
                "forged: fixture replay claims general",
                case("valid", "clean", "implemented", "general", "fixture_replay"),
            ),
            (
                "forged: support without mechanism",
                case("valid", "clean", "implemented", "partial", "none"),
            ),
            (
                "forged: unsupported claims partial",
                case("valid", "clean", "unsupported", "partial", "eir_execution"),
            ),
            (
                "forged: clean outcome without valid evidence",
                case("not_proven", "clean", "not_assessed", "not_assessed", "none"),
            ),
            (
                "forged: process failure claims valid evidence",
                case("valid", "process_or_protocol_failed", "not_assessed", "not_assessed", "none"),
            ),
            (
                "forged: support claimed for an unobserved subject",
                case("valid", "not_assessed", "implemented", "general", "eir_execution"),
            ),
            (
                "forged: stale evidence claims support",
                case("stale", "not_assessed", "implemented", "general", "eir_execution"),
            ),
        ]
    }

    #[test]
    fn published_schema_decides_axes_exactly_as_the_constructor()
    -> Result<(), Box<dyn std::error::Error>> {
        let axes_schema = axes_schema()?;

        for (name, fixture) in axis_fixtures() {
            let rust_accepts = serde_json::from_value::<ResultAxes>(fixture.clone()).is_ok();
            let schema_accepts = axes_schema.is_valid(&fixture);
            assert_eq!(
                rust_accepts, schema_accepts,
                "'{name}': Rust constructor accepts={rust_accepts} but published schema \
                 accepts={schema_accepts}; the schema and the invariants have drifted"
            );
            let expected_valid = !name.starts_with("forged:");
            assert_eq!(
                rust_accepts, expected_valid,
                "'{name}': expected acceptance {expected_valid}, got {rust_accepts}"
            );
        }
        Ok(())
    }

    /// How the published schema is expected to relate to the Rust validator for
    /// one report fixture.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum SchemaAgreement {
        /// Both accept, or both reject.
        Same,
        /// Rust rejects and the schema accepts, because the rule compares two
        /// values and JSON Schema cannot express cross-field arithmetic.
        RustStricter,
    }

    fn valid_report() -> Result<RunAxesReport, ResultAxisViolation> {
        let observed = ResultAxes::new(
            EvidenceValidity::Valid,
            ObservedOutcome::Clean,
            CompatibilityAdmission::Implemented,
            SemanticSupport::General,
            CorrectnessMechanism::EirExecution,
        )?;
        Ok(report(observed, no_change()))
    }

    /// Report-level fixtures, each naming what the two validators should do.
    ///
    /// The axis-level parity sweep proved `ResultAxes` exhaustively; this is the
    /// equivalent for the aggregate, where three real drifts were found in
    /// review (a schema missing the completion rule, blank digests, and unknown
    /// distribution categories).
    fn report_fixtures() -> Result<
        Vec<(&'static str, serde_json::Value, bool, SchemaAgreement)>,
        Box<dyn std::error::Error>,
    > {
        let mut fixtures: Vec<(&'static str, serde_json::Value, bool, SchemaAgreement)> =
            Vec::new();

        fixtures.push((
            "valid report",
            serde_json::to_value(valid_report()?)?,
            true,
            SchemaAgreement::Same,
        ));

        let mut incomplete = valid_report()?;
        incomplete.completion = MeasurementCompletion::Incomplete;
        fixtures.push((
            "valid evidence from an incomplete measurement",
            serde_json::to_value(incomplete)?,
            false,
            SchemaAgreement::Same,
        ));

        let mut blank_digest = valid_report()?;
        blank_digest.current_versus_accepted.accepted_baseline_digest = Some(String::new());
        fixtures.push((
            "blank accepted baseline digest",
            serde_json::to_value(blank_digest)?,
            false,
            SchemaAgreement::Same,
        ));

        let mut misspelled = valid_report()?;
        misspelled.admission.by_admission = distribution(&[("implmented", 4)]);
        fixtures.push((
            "misspelled admission category",
            serde_json::to_value(misspelled)?,
            false,
            SchemaAgreement::Same,
        ));

        let mut misspelled_support = valid_report()?;
        misspelled_support.admission.by_support = distribution(&[("generl", 4)]);
        fixtures.push((
            "misspelled support category",
            serde_json::to_value(misspelled_support)?,
            false,
            SchemaAgreement::Same,
        ));

        let mut forged_rail = valid_report()?;
        forged_rail.correctness_rails.insert(
            "execution".to_string(),
            CorrectnessRailSummary {
                mechanism: CorrectnessMechanism::RealPerlOracle,
                availability: CompatibilityRailAvailability::NotAvailable,
                reason: "claimed absent".to_string(),
                evidence_refs: Vec::new(),
                files_total: Some(100),
                files_passed: Some(100),
            },
        );
        fixtures.push((
            "unavailable rail claiming a mechanism and counts",
            serde_json::to_value(forged_rail)?,
            false,
            SchemaAgreement::Same,
        ));

        let mut vacuous = valid_report()?;
        vacuous.axes = ResultAxes::not_assessed(EvidenceValidity::Valid);
        vacuous.parse.axes = vacuous.axes;
        vacuous.admission.by_admission = distribution(&[("not_assessed", 4)]);
        vacuous.admission.by_support = distribution(&[("not_assessed", 4)]);
        fixtures.push((
            "complete run with valid evidence that assessed nothing",
            serde_json::to_value(vacuous)?,
            false,
            SchemaAgreement::Same,
        ));

        let mut legacy_promoted = valid_report()?;
        legacy_promoted.origin = EvidenceOrigin::LegacyAdapted;
        fixtures.push((
            "legacy-adapted record carrying a non-historical transition",
            serde_json::to_value(legacy_promoted)?,
            false,
            SchemaAgreement::Same,
        ));

        let mut half_counts = valid_report()?;
        half_counts.correctness_rails.insert(
            "eir".to_string(),
            CorrectnessRailSummary {
                mechanism: CorrectnessMechanism::EirExecution,
                availability: CompatibilityRailAvailability::Partial,
                reason: "partial rail".to_string(),
                evidence_refs: vec!["bundle-1".to_string()],
                files_total: Some(4),
                files_passed: None,
            },
        );
        fixtures.push((
            "rail reporting one count without the other",
            serde_json::to_value(half_counts)?,
            false,
            SchemaAgreement::Same,
        ));

        let mut blank_bucket = valid_report()?;
        blank_bucket.files.push(FileModeAxesResult {
            path: "base/ok.t".to_string(),
            mode: HarnessMode::Compile,
            axes: blank_bucket.axes,
            bucket: Some("   ".to_string()),
        });
        fixtures.push((
            "file carrying a blank failure bucket",
            serde_json::to_value(blank_bucket)?,
            false,
            SchemaAgreement::Same,
        ));

        let mut unbacked_support = valid_report()?;
        unbacked_support.correctness_rails = BTreeMap::new();
        fixtures.push((
            "aggregate support with no rail backing its mechanism",
            serde_json::to_value(unbacked_support)?,
            false,
            SchemaAgreement::RustStricter,
        ));

        let mut weak_rail = valid_report()?;
        weak_rail.correctness_rails.insert(
            "eir".to_string(),
            CorrectnessRailSummary {
                mechanism: CorrectnessMechanism::EirExecution,
                availability: CompatibilityRailAvailability::Stale,
                reason: "freshness expired".to_string(),
                evidence_refs: vec!["bundle-1".to_string()],
                files_total: Some(4),
                files_passed: Some(4),
            },
        );
        fixtures.push((
            "general support resting on a stale rail",
            serde_json::to_value(weak_rail)?,
            false,
            SchemaAgreement::RustStricter,
        ));

        let mut adopted_regression = valid_report()?;
        adopted_regression.current_versus_accepted = CurrentVersusAccepted {
            transition: CompatibilityTransition::Regression,
            requires_acceptance: true,
            accepted_baseline_digest: Some("sha256:9".to_string()),
            reason: "regression presented as adoptable".to_string(),
        };
        fixtures.push((
            "regression presented as awaiting acceptance",
            serde_json::to_value(adopted_regression)?,
            false,
            SchemaAgreement::Same,
        ));

        let mut blank_boundary = valid_report()?;
        blank_boundary.claim_boundary = "   ".to_string();
        fixtures.push((
            "blank claim boundary",
            serde_json::to_value(blank_boundary)?,
            false,
            SchemaAgreement::Same,
        ));

        let mut no_denominator = valid_report()?;
        no_denominator.subject.denominator = 0;
        no_denominator.parse.files_total = 0;
        no_denominator.parse.files_passed = 0;
        no_denominator.admission.by_admission = BTreeMap::new();
        no_denominator.admission.by_support = BTreeMap::new();
        fixtures.push((
            "missing denominator",
            serde_json::to_value(no_denominator)?,
            false,
            SchemaAgreement::Same,
        ));

        // Rules that compare two numbers. JSON Schema 2020-12 has no way to
        // relate one property to another, so the schema cannot carry these and
        // the Rust validator is deliberately the stricter authority.
        let mut short_distribution = valid_report()?;
        short_distribution.admission.by_admission = distribution(&[("implemented", 3)]);
        fixtures.push((
            "admission distribution under the denominator",
            serde_json::to_value(short_distribution)?,
            false,
            SchemaAgreement::RustStricter,
        ));

        let mut debt_mismatch = valid_report()?;
        debt_mismatch.admission.by_admission =
            distribution(&[("implemented", 3), ("accepted_debt", 1)]);
        debt_mismatch.admission.by_support = distribution(&[("general", 3), ("blocked", 1)]);
        debt_mismatch.admission.accepted_debt_count = 0;
        fixtures.push((
            "declared zero debt against a distribution that has some",
            serde_json::to_value(debt_mismatch)?,
            false,
            SchemaAgreement::RustStricter,
        ));

        let mut parse_overrun = valid_report()?;
        parse_overrun.parse.files_passed = 5;
        fixtures.push((
            "parse rail passing more than it covered",
            serde_json::to_value(parse_overrun)?,
            false,
            SchemaAgreement::RustStricter,
        ));

        Ok(fixtures)
    }

    #[test]
    fn published_schema_and_rust_agree_on_report_fixtures() -> Result<(), Box<dyn std::error::Error>>
    {
        let validator = schema()?;
        for (name, fixture, rust_should_accept, agreement) in report_fixtures()? {
            let decoded: RunAxesReport = serde_json::from_value(fixture.clone())?;
            let rust_accepts = decoded.validate().is_ok();
            assert_eq!(
                rust_accepts, rust_should_accept,
                "'{name}': Rust validator acceptance was {rust_accepts}, expected \
                 {rust_should_accept}"
            );

            let schema_accepts = validator.is_valid(&fixture);
            match agreement {
                SchemaAgreement::Same => assert_eq!(
                    schema_accepts, rust_accepts,
                    "'{name}': published schema accepts={schema_accepts} but Rust \
                     accepts={rust_accepts}; the schema and the validator have drifted"
                ),
                SchemaAgreement::RustStricter => {
                    assert!(!rust_accepts, "'{name}' is only rust-stricter if Rust rejects it");
                    assert!(
                        schema_accepts,
                        "'{name}': marked RustStricter, but the schema rejected it too — \
                         reclassify the fixture as Same"
                    );
                }
            }
        }
        Ok(())
    }

    #[test]
    fn distribution_categories_match_the_axis_vocabularies() {
        let admissions = [
            CompatibilityAdmission::Implemented,
            CompatibilityAdmission::StaticallyClassified,
            CompatibilityAdmission::AcceptedDebt,
            CompatibilityAdmission::Unsupported,
            CompatibilityAdmission::NotAssessed,
        ];
        for admission in admissions {
            assert!(
                ADMISSION_CATEGORIES.contains(&admission.as_str()),
                "admission '{admission}' is missing from ADMISSION_CATEGORIES"
            );
        }
        assert_eq!(ADMISSION_CATEGORIES.len(), admissions.len());

        let supports = [
            SemanticSupport::General,
            SemanticSupport::Partial,
            SemanticSupport::Blocked,
            SemanticSupport::Unavailable,
            SemanticSupport::NotAssessed,
        ];
        for support in supports {
            assert!(
                SUPPORT_CATEGORIES.contains(&support.as_str()),
                "support '{support}' is missing from SUPPORT_CATEGORIES"
            );
        }
        assert_eq!(SUPPORT_CATEGORIES.len(), supports.len());
    }

    /// `not_assessed` builds a value without going through `new`, so the two
    /// must be proven to agree or the constructor guarantee is only a comment.
    #[test]
    fn not_assessed_agrees_with_the_constructor() {
        for evidence in [
            EvidenceValidity::Valid,
            EvidenceValidity::NotProven,
            EvidenceValidity::Invalid,
            EvidenceValidity::Stale,
            EvidenceValidity::Cancelled,
        ] {
            let built = ResultAxes::not_assessed(evidence);
            let constructed = ResultAxes::new(
                built.evidence(),
                built.observation(),
                built.admission(),
                built.support(),
                built.mechanism(),
            );
            assert_eq!(
                constructed,
                Ok(built),
                "not_assessed({evidence}) built a value the constructor would reject"
            );
        }
    }

    #[test]
    fn a_complete_run_that_assessed_nothing_is_not_authoritative()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut candidate = report(ResultAxes::not_assessed(EvidenceValidity::Valid), no_change());
        candidate.completion = MeasurementCompletion::Complete;
        candidate.admission.by_admission = distribution(&[("not_assessed", 4)]);
        candidate.admission.by_support = distribution(&[("not_assessed", 4)]);
        candidate.parse.axes = ResultAxes::not_assessed(EvidenceValidity::Valid);

        assert_eq!(
            candidate.validate(),
            Err(ResultReportViolation::CompleteMeasurementObservedNothing),
            "a complete, valid, authoritative report cannot have assessed nothing"
        );
        assert!(candidate.admissible_as_current_authority().is_err());
        Ok(())
    }

    #[test]
    fn general_support_cannot_rest_on_a_partial_or_stale_rail()
    -> Result<(), Box<dyn std::error::Error>> {
        for weak in [CompatibilityRailAvailability::Partial, CompatibilityRailAvailability::Stale] {
            let mut candidate = valid_report()?;
            candidate.correctness_rails.insert(
                "eir".to_string(),
                CorrectnessRailSummary {
                    mechanism: CorrectnessMechanism::EirExecution,
                    availability: weak,
                    reason: "subset only".to_string(),
                    evidence_refs: vec!["bundle-1".to_string()],
                    files_total: Some(1),
                    files_passed: Some(1),
                },
            );
            assert_eq!(
                candidate.validate(),
                Err(ResultReportViolation::AggregateSupportWithoutRail {
                    support: SemanticSupport::General,
                    mechanism: CorrectnessMechanism::EirExecution,
                }),
                "a {weak:?} rail cannot carry a general claim across the whole denominator"
            );
        }
        Ok(())
    }

    #[test]
    fn a_clean_aggregate_cannot_sit_over_a_failing_detail() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut candidate = valid_report()?;
        let failing = ResultAxes::new(
            EvidenceValidity::Valid,
            ObservedOutcome::FailuresObserved,
            CompatibilityAdmission::Implemented,
            SemanticSupport::Partial,
            CorrectnessMechanism::EirExecution,
        )?;
        candidate.files.push(FileModeAxesResult {
            path: "base/broken.t".to_string(),
            mode: HarnessMode::Compile,
            axes: failing,
            bucket: Some("compile_effect".to_string()),
        });
        assert_eq!(
            candidate.validate(),
            Err(ResultReportViolation::CleanAggregateOverFailingDetail)
        );
        Ok(())
    }

    #[test]
    fn stale_evidence_is_rejected_for_its_own_reason() -> Result<(), Box<dyn std::error::Error>> {
        let mut candidate = report(
            ResultAxes::not_assessed(EvidenceValidity::Stale),
            CurrentVersusAccepted {
                transition: CompatibilityTransition::NotProven,
                requires_acceptance: false,
                accepted_baseline_digest: None,
                reason: "evidence went stale".to_string(),
            },
        );
        candidate.completion = MeasurementCompletion::Complete;
        candidate.admission.by_admission = distribution(&[("not_assessed", 4)]);
        candidate.admission.by_support = distribution(&[("not_assessed", 4)]);
        candidate.parse.axes = ResultAxes::not_assessed(EvidenceValidity::Stale);

        candidate.validate()?;
        assert_eq!(
            candidate.admissible_as_current_authority(),
            Err(ResultReportViolation::EvidenceNotValidForCurrentAuthority {
                evidence: EvidenceValidity::Stale,
            }),
            "stale evidence must be reported as stale, not as an incomplete measurement"
        );
        Ok(())
    }

    #[test]
    fn published_schema_accepts_a_complete_report() -> Result<(), Box<dyn std::error::Error>> {
        let validator = schema()?;
        let observed = ResultAxes::new(
            EvidenceValidity::Valid,
            ObservedOutcome::Clean,
            CompatibilityAdmission::Implemented,
            SemanticSupport::General,
            CorrectnessMechanism::EirExecution,
        )?;
        let mut candidate = report(observed, no_change());
        candidate.correctness_rails.insert(
            "curated_gold".to_string(),
            CorrectnessRailSummary::unavailable("no curated gold rail"),
        );
        candidate.files.push(FileModeAxesResult {
            path: "base/ok.t".to_string(),
            mode: HarnessMode::Compile,
            axes: observed,
            bucket: None,
        });
        candidate.invocations.push(InvocationAxesResult {
            invocation_identity: "invocation-1".to_string(),
            axes: observed,
            limitations: Vec::new(),
        });
        candidate.validate()?;

        let encoded = serde_json::to_value(&candidate)?;
        if let Err(error) = validator.validate(&encoded) {
            return Err(format!("valid report rejected by published schema: {error}").into());
        }
        Ok(())
    }

    #[test]
    fn published_schema_rejects_a_rail_that_borrowed_counts()
    -> Result<(), Box<dyn std::error::Error>> {
        let validator = schema()?;
        let observed = ResultAxes::new(
            EvidenceValidity::Valid,
            ObservedOutcome::Clean,
            CompatibilityAdmission::Implemented,
            SemanticSupport::General,
            CorrectnessMechanism::EirExecution,
        )?;
        let mut candidate = report(observed, no_change());
        candidate.correctness_rails.insert(
            "execution".to_string(),
            CorrectnessRailSummary {
                mechanism: CorrectnessMechanism::None,
                availability: CompatibilityRailAvailability::NotAvailable,
                reason: "no execution rail provisioned".to_string(),
                evidence_refs: Vec::new(),
                files_total: Some(4),
                files_passed: Some(4),
            },
        );

        assert!(candidate.validate().is_err(), "the Rust validator must reject borrowed counts");
        let encoded = serde_json::to_value(&candidate)?;
        assert!(
            !validator.is_valid(&encoded),
            "the published schema must also reject a mechanism-less rail reporting counts"
        );
        Ok(())
    }

    #[test]
    fn published_schema_rejects_an_aggregate_without_a_denominator()
    -> Result<(), Box<dyn std::error::Error>> {
        let validator = schema()?;
        let observed = ResultAxes::new(
            EvidenceValidity::Valid,
            ObservedOutcome::Clean,
            CompatibilityAdmission::Implemented,
            SemanticSupport::General,
            CorrectnessMechanism::EirExecution,
        )?;
        let mut candidate = report(observed, no_change());
        candidate.subject.denominator = 0;
        candidate.parse.files_total = 0;
        candidate.parse.files_passed = 0;
        candidate.admission.by_admission = BTreeMap::new();
        candidate.admission.by_support = BTreeMap::new();

        assert!(candidate.validate().is_err(), "the Rust validator must reject this");
        let encoded = serde_json::to_value(&candidate)?;
        assert!(
            !validator.is_valid(&encoded),
            "the published schema must also reject an aggregate with no denominator"
        );
        Ok(())
    }

    #[test]
    fn published_schema_rejects_an_aggregate_missing_subject_identity()
    -> Result<(), Box<dyn std::error::Error>> {
        let validator = schema()?;
        let observed = ResultAxes::new(
            EvidenceValidity::Valid,
            ObservedOutcome::Clean,
            CompatibilityAdmission::Implemented,
            SemanticSupport::General,
            CorrectnessMechanism::EirExecution,
        )?;
        let mut candidate = report(observed, no_change());
        candidate.subject.series_id = String::new();

        assert!(candidate.validate().is_err(), "the Rust validator must reject this");
        let encoded = serde_json::to_value(&candidate)?;
        assert!(
            !validator.is_valid(&encoded),
            "the published schema must also reject an aggregate with no series identity"
        );
        Ok(())
    }

    #[test]
    fn published_schema_rejects_a_candidate_presented_as_accepted()
    -> Result<(), Box<dyn std::error::Error>> {
        let validator = schema()?;
        let observed = ResultAxes::new(
            EvidenceValidity::Valid,
            ObservedOutcome::Clean,
            CompatibilityAdmission::Implemented,
            SemanticSupport::General,
            CorrectnessMechanism::EirExecution,
        )?;
        let mut candidate = report(observed, no_change());
        candidate.current_versus_accepted = CurrentVersusAccepted {
            transition: CompatibilityTransition::ImprovementCandidate,
            requires_acceptance: false,
            accepted_baseline_digest: Some("sha256:4".to_string()),
            reason: "forged as settled".to_string(),
        };

        assert!(candidate.validate().is_err(), "the Rust validator must reject this");
        let encoded = serde_json::to_value(&candidate)?;
        assert!(
            !validator.is_valid(&encoded),
            "the published schema must also reject a candidate that does not require acceptance"
        );
        Ok(())
    }

    #[test]
    fn axis_wire_names_match_the_declared_vocabulary() {
        assert_eq!(EvidenceValidity::NotProven.as_str(), "not_proven");
        assert_eq!(ObservedOutcome::ProcessOrProtocolFailed.as_str(), "process_or_protocol_failed");
        assert_eq!(CompatibilityAdmission::StaticallyClassified.as_str(), "statically_classified");
        assert_eq!(SemanticSupport::Unavailable.as_str(), "unavailable");
        assert_eq!(CorrectnessMechanism::FixtureReplay.as_str(), "fixture_replay");
        assert_eq!(ResultAxis::Mechanism.as_str(), "mechanism");
    }
}
