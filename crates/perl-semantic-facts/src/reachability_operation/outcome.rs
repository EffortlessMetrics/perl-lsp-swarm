//! Semantic truth projection, operation outcome, denominator ledger, and
//! publication eligibility.

use super::receipt::ReachabilityWorkReceipt;
use super::tracker::ReachabilityTerminalObservation;
use super::{
    ReachabilityContractError, ReachabilityFactFamilyId, ReachabilityStageId,
    ReachabilitySubjectIdentity, ReachabilitySubjectIdentityKind, ReachabilityTerminalState,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The stage-local projection of the #8169 semantic truth outcome.
///
/// This is not a new semantic vocabulary: it is the projection of the
/// canonical semantic truth distinctions (#8169/#8911) onto one reachability
/// stage, carried by [`ReachabilityOperationOutcome::Completed`] next to the
/// independently typed execution terminality of the remaining variants. The
/// canonical fact-level vocabulary remains [`crate::SemanticFactStatus`];
/// provider-level outcome selection remains with the provider layers.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReachabilitySemanticOutcome {
    /// Complete semantics over a sufficient current denominator.
    Complete,
    /// Useful partial semantics whose claim ceiling must stay explicit.
    Partial {
        /// Non-empty limitations bounding the claim.
        limitations: Vec<ReachabilityClaimLimitation>,
    },
    /// Complete semantics proving a legitimate empty result.
    LegitimateEmpty,
    /// Required semantic state is not ready.
    NotReady,
    /// Facts belong to an older generation than the accepted subject.
    Stale,
    /// Multiple candidates prevent one authoritative answer.
    Ambiguous,
    /// A dynamic boundary prevents a static answer.
    Dynamic,
    /// The requested subject is unsupported.
    Unsupported,
    /// A semantic producer instrument failed.
    InstrumentFailure,
}

impl ReachabilitySemanticOutcome {
    /// Whether this truth state may carry a retained value.
    #[must_use]
    pub const fn may_carry_value(&self) -> bool {
        matches!(self, Self::Complete | Self::Partial { .. } | Self::LegitimateEmpty)
    }

    /// Whether this truth state admits an exact answer (complete value or
    /// proven legitimate empty).
    #[must_use]
    pub const fn is_exact(&self) -> bool {
        matches!(self, Self::Complete | Self::LegitimateEmpty)
    }
}

/// One explicit claim-ceiling limitation retained by a stage.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ReachabilityClaimLimitation {
    /// A required fact family was missing from the denominator.
    MissingFactFamily(ReachabilityFactFamilyId),
    /// The denominator was only partially sufficient.
    PartialDenominator,
    /// A dynamic boundary caps exactness for affected subjects.
    DynamicBoundary,
    /// A family is unsupported for this subject.
    UnsupportedFamily(ReachabilityFactFamilyId),
    /// A stage ended terminally; downstream claims inherit the ceiling.
    TerminalStage(ReachabilityStageId),
    /// Computation was bounded before completeness.
    BoundedComputation,
}

/// The terminal outcome of one reachability operation.
///
/// Execution terminality is independent of semantic truth: a terminal
/// operation never produces an exact value, a legitimate empty, an ordinary
/// diagnostic, a compatibility success, or an unchanged result reuse.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReachabilityOperationOutcome<T> {
    /// The operation ran to its end with a semantic truth classification.
    Completed {
        /// The #8169 semantic truth projection for this stage.
        semantic_outcome: ReachabilitySemanticOutcome,
        /// The retained value, consistent with the truth classification.
        value: Option<T>,
        /// The canonical work receipt.
        work_receipt: ReachabilityWorkReceipt,
    },
    /// Live cancellation was observed at one stage.
    Cancelled {
        /// The stage observing cancellation.
        stage: ReachabilityStageId,
        /// The exact external control identity.
        control_identity: ReachabilitySubjectIdentity,
        /// The canonical work receipt.
        work_receipt: ReachabilityWorkReceipt,
    },
    /// The operation deadline expired at one stage.
    DeadlineExceeded {
        /// The stage observing expiry.
        stage: ReachabilityStageId,
        /// The exact deadline/profile identity.
        deadline_profile: ReachabilitySubjectIdentity,
        /// The canonical work receipt.
        work_receipt: ReachabilityWorkReceipt,
    },
    /// A deterministic work limit was reached.
    ResourceExhausted {
        /// The stage refused by the budget.
        stage: ReachabilityStageId,
        /// The exhausted dimension.
        dimension: super::ReachabilityWorkDimension,
        /// The limit in force.
        limit: u64,
        /// Units already charged.
        charged: u64,
        /// The canonical work receipt.
        work_receipt: ReachabilityWorkReceipt,
    },
    /// A newer accepted subject superseded this operation.
    SupersededOrStale {
        /// The stage observing supersession.
        stage: ReachabilityStageId,
        /// The subject this operation expected.
        expected: ReachabilitySubjectIdentity,
        /// The subject the authority accepted.
        observed: ReachabilitySubjectIdentity,
        /// The canonical work receipt.
        work_receipt: ReachabilityWorkReceipt,
    },
    /// A bounded product failure ended the operation.
    ProductFailure {
        /// The failing stage.
        stage: ReachabilityStageId,
        /// A bounded cause description.
        cause: String,
        /// The canonical work receipt.
        work_receipt: ReachabilityWorkReceipt,
    },
    /// An instrument failure ended the operation, including checked-counter
    /// overflow and missing instruments.
    InstrumentFailure {
        /// The failing stage.
        stage: ReachabilityStageId,
        /// A bounded cause description.
        cause: String,
        /// The canonical work receipt.
        work_receipt: ReachabilityWorkReceipt,
    },
}

impl<T> ReachabilityOperationOutcome<T> {
    /// Construct a `Completed` outcome under the consistency laws.
    ///
    /// Laws (all fail closed):
    /// - `Complete` requires a value; exact empties are `LegitimateEmpty`.
    /// - `LegitimateEmpty` forbids a retained value.
    /// - `Partial` requires at least one explicit claim-ceiling limitation
    ///   when a value is retained.
    /// - Truth states that cannot carry a value forbid one.
    /// - `Complete`/`LegitimateEmpty` conflict with any stage limitation or
    ///   terminal state recorded in the receipt
    ///   ([`ReachabilityContractError::ClaimConflictsWithLimitations`]);
    /// - `Complete`/`LegitimateEmpty` over a receipt without complete
    ///   instrument evidence fail with
    ///   [`ReachabilityContractError::MissingInstrumentEvidence`].
    ///
    /// # Errors
    ///
    /// Returns [`ReachabilityContractError`] when a law would be violated.
    pub fn complete(
        semantic_outcome: ReachabilitySemanticOutcome,
        value: Option<T>,
        work_receipt: ReachabilityWorkReceipt,
    ) -> Result<Self, ReachabilityContractError> {
        let receipt_has_limitations_or_terminal =
            work_receipt.terminal().is_some() || !work_receipt.stage_limitations().is_empty();
        let receipt_has_instrument_evidence = work_receipt.instrument_evidence_complete();
        match (&semantic_outcome, value.as_ref()) {
            (ReachabilitySemanticOutcome::Complete, None) => {
                return Err(ReachabilityContractError::CompleteWithoutValue);
            }
            (ReachabilitySemanticOutcome::LegitimateEmpty, Some(_)) => {
                return Err(ReachabilityContractError::EmptyWithRetainedValue);
            }
            (ReachabilitySemanticOutcome::Partial { limitations }, Some(_))
                if limitations.is_empty() =>
            {
                return Err(ReachabilityContractError::PartialWithoutLimitation);
            }
            (truth, Some(_)) if !truth.may_carry_value() => {
                return Err(ReachabilityContractError::ValueWithNonValuedTruth);
            }
            _ => {}
        }
        if semantic_outcome.is_exact() && receipt_has_limitations_or_terminal {
            return Err(ReachabilityContractError::ClaimConflictsWithLimitations);
        }
        if semantic_outcome.is_exact() && !receipt_has_instrument_evidence {
            return Err(ReachabilityContractError::MissingInstrumentEvidence);
        }
        Ok(Self::Completed { semantic_outcome, value, work_receipt })
    }

    /// Construct the terminal outcome implied by a latched terminal state.
    ///
    /// Budget exhaustion, overflow, and external control observations map to
    /// their distinct variants.
    ///
    /// # Errors
    ///
    /// Returns [`ReachabilityContractError::IncoherentOutcome`] when
    /// `terminal` is a variant this mapping does not cover (the non-exhaustive
    /// wildcard arm; current variants are all covered, future variants fail
    /// closed rather than mapping to a wrong terminal).
    pub fn terminal_from(
        terminal: &ReachabilityTerminalState,
        stage: ReachabilityStageId,
        work_receipt: ReachabilityWorkReceipt,
    ) -> Result<Self, ReachabilityContractError> {
        match terminal {
            ReachabilityTerminalState::External(ReachabilityTerminalObservation::Cancelled {
                control_identity,
            }) => Ok(Self::Cancelled {
                stage,
                control_identity: control_identity.clone(),
                work_receipt,
            }),
            ReachabilityTerminalState::External(
                ReachabilityTerminalObservation::DeadlineExceeded { deadline_profile },
            ) => Ok(Self::DeadlineExceeded {
                stage,
                deadline_profile: deadline_profile.clone(),
                work_receipt,
            }),
            ReachabilityTerminalState::External(ReachabilityTerminalObservation::Superseded {
                expected,
                observed,
            }) => Ok(Self::SupersededOrStale {
                stage,
                expected: expected.clone(),
                observed: observed.clone(),
                work_receipt,
            }),
            ReachabilityTerminalState::ResourceExhausted { dimension, limit, charged } => {
                Ok(Self::ResourceExhausted {
                    stage,
                    dimension: *dimension,
                    limit: *limit,
                    charged: *charged,
                    work_receipt,
                })
            }
            ReachabilityTerminalState::CounterOverflow { dimension } => {
                Ok(Self::InstrumentFailure {
                    stage,
                    cause: format!("work counter overflow in {}", dimension.as_str()),
                    work_receipt,
                })
            }
            #[allow(unreachable_patterns)]
            _ => Err(ReachabilityContractError::IncoherentOutcome),
        }
    }

    /// Whether this outcome is an execution terminal (anything other than
    /// `Completed`).
    #[must_use]
    pub const fn is_execution_terminal(&self) -> bool {
        !matches!(self, Self::Completed { .. })
    }

    /// Whether this outcome may carry an exact claim: a completed value or
    /// a proven legitimate empty over a sufficient current denominator with
    /// complete instrument evidence.
    #[must_use]
    pub fn may_claim_exact(&self) -> bool {
        match self {
            Self::Completed { semantic_outcome, value, work_receipt } => {
                semantic_outcome.is_exact()
                    && work_receipt.terminal().is_none()
                    && work_receipt.stage_limitations().is_empty()
                    && work_receipt.instrument_evidence_complete()
                    && match semantic_outcome {
                        ReachabilitySemanticOutcome::Complete => value.is_some(),
                        ReachabilitySemanticOutcome::LegitimateEmpty => value.is_none(),
                        _ => false,
                    }
            }
            _ => false,
        }
    }

    /// The retained partial value, for diagnostics, explanation, or progress
    /// only. Its incompleteness stays explicit through the semantic
    /// outcome; it can never enter exact policy or result reuse as a
    /// complete value.
    #[must_use]
    pub fn retained_partial_value(&self) -> Option<&T> {
        match self {
            Self::Completed {
                semantic_outcome: ReachabilitySemanticOutcome::Partial { .. },
                value: Some(value),
                ..
            } => Some(value),
            _ => None,
        }
    }

    /// The canonical work receipt of this outcome.
    #[must_use]
    pub fn work_receipt(&self) -> &ReachabilityWorkReceipt {
        match self {
            Self::Completed { work_receipt, .. }
            | Self::Cancelled { work_receipt, .. }
            | Self::DeadlineExceeded { work_receipt, .. }
            | Self::ResourceExhausted { work_receipt, .. }
            | Self::SupersededOrStale { work_receipt, .. }
            | Self::ProductFailure { work_receipt, .. }
            | Self::InstrumentFailure { work_receipt, .. } => work_receipt,
        }
    }
}

/// Status of one fact family in a denominator ledger.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReachabilityFactFamilyStatus {
    /// The family is present and complete for the accepted profile.
    Complete,
    /// The family is present but its denominator is partial.
    Partial,
    /// The family is missing.
    Missing,
    /// The family is unsupported for this subject.
    Unsupported,
    /// The family belongs to an older generation.
    Stale,
}

impl ReachabilityFactFamilyStatus {
    /// Whether this status can support an exact claim.
    #[must_use]
    pub const fn is_complete(&self) -> bool {
        matches!(self, Self::Complete)
    }
}

/// The fact-family denominator ledger of one operation.
///
/// A missing required family is never complete empty: it caps the claim at
/// partial/not-ready, exactly as the graph-admission consumer (#10915)
/// requires.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReachabilityFactFamilyLedger {
    families: BTreeMap<ReachabilityFactFamilyId, ReachabilityFactFamilyStatus>,
}

impl ReachabilityFactFamilyLedger {
    /// Construct a ledger from family statuses, canonicalized by family id.
    #[must_use]
    pub fn new(families: BTreeMap<ReachabilityFactFamilyId, ReachabilityFactFamilyStatus>) -> Self {
        Self { families }
    }

    /// The status of one family, missing families fail closed to
    /// [`ReachabilityFactFamilyStatus::Missing`].
    #[must_use]
    pub fn status(&self, family: &ReachabilityFactFamilyId) -> ReachabilityFactFamilyStatus {
        self.families.get(family).copied().unwrap_or(ReachabilityFactFamilyStatus::Missing)
    }

    /// Whether every required family is complete. The denominator of an
    /// exact claim must pass this check.
    #[must_use]
    pub fn requires_complete(&self, required: &[ReachabilityFactFamilyId]) -> bool {
        required.iter().all(|family| self.status(family).is_complete())
    }
}

/// Why one publication/cache eligibility check refused.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReachabilityIneligibilityReason {
    /// The observed accepted authority no longer matches the subject.
    SubjectSuperseded,
    /// A required stage did not complete.
    RequiredStageIncomplete(ReachabilityStageId),
    /// The semantic outcome does not support the requested claim.
    ClaimNotSupported,
    /// A required fact-family denominator is incomplete.
    DenominatorIncomplete(ReachabilityFactFamilyId),
    /// The operation ended in a terminal state.
    TerminalState,
    /// Instrument evidence is missing or incomplete.
    InstrumentEvidenceIncomplete,
}

/// The publication/cache eligibility verdict of one completed operation.
///
/// The contract exposes this predicate; owning publication and cache layers
/// consume it. The contract itself does not publish, cache, or choose
/// policy. Cancelled, exhausted, failed, or stale attempts are ineligible:
/// they create no current graph, closure, query, policy, diagnostic, or
/// result-id entry, cannot relabel a prior result current for changed
/// inputs, and cannot trigger name/reference-heuristic fallback.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReachabilityPublicationEligibility {
    eligible: bool,
    reasons: Vec<ReachabilityIneligibilityReason>,
}

impl ReachabilityPublicationEligibility {
    /// Whether the result may become current/reusable.
    #[must_use]
    pub const fn is_eligible(&self) -> bool {
        self.eligible
    }

    /// Every refusal reason, when ineligible.
    #[must_use]
    pub fn reasons(&self) -> &[ReachabilityIneligibilityReason] {
        &self.reasons
    }

    /// Evaluate eligibility for one outcome, fail-closed.
    ///
    /// A result is eligible only when the subject still matches the
    /// observed accepted authority, every required stage completed, the
    /// semantic outcome supports the claim, the required denominator is
    /// complete, no terminal state occurred, and instrument evidence is
    /// complete.
    #[must_use]
    pub fn evaluate<T>(
        subject: &super::ReachabilityOperationSubject,
        observed_authority: Option<&ReachabilitySubjectIdentity>,
        authority_kind: ReachabilitySubjectIdentityKind,
        required_stages: &[ReachabilityStageId],
        outcome: &ReachabilityOperationOutcome<T>,
        ledger: &ReachabilityFactFamilyLedger,
        required_families: &[ReachabilityFactFamilyId],
    ) -> Self {
        let mut reasons = Vec::new();
        if !subject.authority_matches(authority_kind, observed_authority) {
            reasons.push(ReachabilityIneligibilityReason::SubjectSuperseded);
        }
        let receipt = outcome.work_receipt();
        for stage in required_stages {
            if !receipt.completed_stages().contains(stage) {
                reasons
                    .push(ReachabilityIneligibilityReason::RequiredStageIncomplete(stage.clone()));
            }
        }
        if !outcome.may_claim_exact() {
            reasons.push(ReachabilityIneligibilityReason::ClaimNotSupported);
        }
        for family in required_families {
            if !ledger.status(family).is_complete() {
                reasons
                    .push(ReachabilityIneligibilityReason::DenominatorIncomplete(family.clone()));
            }
        }
        if receipt.terminal().is_some() || outcome.is_execution_terminal() {
            reasons.push(ReachabilityIneligibilityReason::TerminalState);
        }
        if !receipt.instrument_evidence_complete() {
            reasons.push(ReachabilityIneligibilityReason::InstrumentEvidenceIncomplete);
        }
        Self { eligible: reasons.is_empty(), reasons }
    }
}
