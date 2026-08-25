//! One transport-neutral reachability operation, work-budget, and
//! terminal-outcome contract (#11553).
//!
//! This module defines the shared substrate that every reachability stage —
//! graph admission, SCC condensation, production/test closure,
//! classification, query, explanation, diagnostic projection, and proof —
//! consumes for operation identity, deterministic work accounting, and
//! terminal semantics. It is deliberately stage-composable: one operation
//! subject and one work tracker flow through every stage, each stage appends
//! its exact output identity and its limitations, and no stage can erase an
//! upstream limitation or promote an incomplete stage to an exact claim.
//!
//! # Composition, not duplication
//!
//! - The canonical semantic fact vocabulary ([`crate::SemanticFactStatus`],
//!   [`crate::SemanticFreshness`], [`crate::SemanticReasonCode`]) is not
//!   redefined here. [`ReachabilitySemanticOutcome`] is the stage-local
//!   projection of the #8169 semantic truth outcome carried by
//!   [`ReachabilityOperationOutcome::Completed`]; it composes that semantic
//!   truth axis with the independent execution terminality expressed by the
//!   remaining outcome variants.
//! - Cancellation, deadline, and supersession are **not** owned here. Stages
//!   poll one [`ReachabilityOperationControl`] port that the owning runtime
//!   binds to its canonical external control. This module creates no request
//!   registry, scheduler, clock source, or timer.
//! - Work budgets are deterministic work-unit dimensions with per-dimension
//!   limits and checked arithmetic. Elapsed wall time is not a work
//!   dimension; a host timeout without a complete operation receipt is an
//!   instrument failure, never a semantic budget result.
//!
//! # Ownership fence
//!
//! This module owns no LSP protocol, parser, graph algorithm, SCC traversal,
//! provider policy, diagnostic code, or scheduler behavior. The
//! `architecture_fence` test enforces the import boundary mechanically.
//!
//! # Governing law
//!
//! Only a complete, current, denominator-sufficient operation may admit an
//! exact structural or semantic result. Cancellation, deadline expiry,
//! budget exhaustion, supersession, product failure, and instrument failure
//! are typed terminal outcomes that can never surface as exact unreachable,
//! legitimate empty, an ordinary diagnostic, compatibility success, or an
//! unchanged result reuse.

mod budget;
mod outcome;
mod receipt;
mod subject;
mod tracker;
mod view;

#[cfg(test)]
mod tests;

pub use budget::{
    ReachabilityCancellationPolling, ReachabilityDimensionLimit, ReachabilityExecutionProfile,
    ReachabilityExecutionPurpose, ReachabilityProfileId, ReachabilityRetentionLimits,
    ReachabilityUnlimitedJustification, ReachabilityWorkBudget, ReachabilityWorkDimension,
};
pub use outcome::{
    ReachabilityClaimLimitation, ReachabilityFactFamilyLedger, ReachabilityFactFamilyStatus,
    ReachabilityIneligibilityReason, ReachabilityOperationOutcome,
    ReachabilityPublicationEligibility, ReachabilitySemanticOutcome,
};
pub use receipt::{
    ReachabilityExhaustionAttempt, ReachabilityStageLimitation, ReachabilityWorkHonestyError,
    ReachabilityWorkPath, ReachabilityWorkPathTarget, ReachabilityWorkReceipt,
};
pub use subject::{
    ReachabilityOperationSubject, ReachabilityStageOutput, ReachabilitySubjectIdentity,
    ReachabilitySubjectIdentityKind,
};
pub use tracker::{
    ReachabilityChargeError, ReachabilityOperationControl, ReachabilityTerminalObservation,
    ReachabilityTerminalState, ReachabilityWorkTracker,
};
pub use view::{ReachabilityBoundedView, ReachabilityCompleteResultRef, ReachableViewProfileId};

use serde::{Deserialize, Serialize};

/// Closed, extensible-under-review set of reachability operation kinds.
///
/// Unknown operation kinds fail closed: free-form strings cannot acquire
/// semantic authority, so [`ReachabilityOperationKind::parse`] rejects
/// unknown names. New kinds enter only through review of this contract.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ReachabilityOperationKind {
    /// Freeze one admitted liveness graph input (#10915).
    GraphAdmission,
    /// Deterministic SCC components and condensed DAG (#10921).
    SccCondensation,
    /// Production closure traversal (#10928).
    ProductionClosure,
    /// Test closure traversal (#10928).
    TestClosure,
    /// Component/entity classification.
    Classification,
    /// Snapshot-bound entity query (#10935).
    EntityQuery,
    /// Source partition projection.
    SourcePartition,
    /// Bounded explanation view (#10935).
    BoundedExplanation,
    /// Policy projection (#8101 seam).
    PolicyProjection,
    /// Diagnostic candidate composition (#10941 seam).
    DiagnosticCandidateComposition,
    /// Diagnostic transport projection (#10947 seam).
    DiagnosticTransportProjection,
    /// Result reuse revalidation (#10957 seam).
    ResultReuseRevalidation,
    /// Semantic proof (#11006 seam).
    SemanticProof,
    /// Exact process proof (#11012 seam).
    ExactProcessProof,
}

impl ReachabilityOperationKind {
    /// The stable kebab-case name of this kind.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::GraphAdmission => "graph-admission",
            Self::SccCondensation => "scc-condensation",
            Self::ProductionClosure => "production-closure",
            Self::TestClosure => "test-closure",
            Self::Classification => "classification",
            Self::EntityQuery => "entity-query",
            Self::SourcePartition => "source-partition",
            Self::BoundedExplanation => "bounded-explanation",
            Self::PolicyProjection => "policy-projection",
            Self::DiagnosticCandidateComposition => "diagnostic-candidate-composition",
            Self::DiagnosticTransportProjection => "diagnostic-transport-projection",
            Self::ResultReuseRevalidation => "result-reuse-revalidation",
            Self::SemanticProof => "semantic-proof",
            Self::ExactProcessProof => "exact-process-proof",
        }
    }

    /// Parse one operation kind, failing closed on unknown names.
    ///
    /// # Errors
    ///
    /// Returns [`ReachabilityContractError::UnknownOperationKind`] when the
    /// name is not one of the closed kinds.
    pub fn parse(name: &str) -> Result<Self, ReachabilityContractError> {
        const KINDS: [ReachabilityOperationKind;
            ReachabilityOperationKind::ExactProcessProof as usize + 1] = [
            ReachabilityOperationKind::GraphAdmission,
            ReachabilityOperationKind::SccCondensation,
            ReachabilityOperationKind::ProductionClosure,
            ReachabilityOperationKind::TestClosure,
            ReachabilityOperationKind::Classification,
            ReachabilityOperationKind::EntityQuery,
            ReachabilityOperationKind::SourcePartition,
            ReachabilityOperationKind::BoundedExplanation,
            ReachabilityOperationKind::PolicyProjection,
            ReachabilityOperationKind::DiagnosticCandidateComposition,
            ReachabilityOperationKind::DiagnosticTransportProjection,
            ReachabilityOperationKind::ResultReuseRevalidation,
            ReachabilityOperationKind::SemanticProof,
            ReachabilityOperationKind::ExactProcessProof,
        ];
        KINDS
            .into_iter()
            .find(|kind| kind.as_str() == name)
            .ok_or_else(|| ReachabilityContractError::UnknownOperationKind(name.to_string()))
    }
}

/// Stable identity of one reachability operation.
///
/// Identifiers are opaque, non-empty, and assigned by the owning runtime;
/// this contract never mints them.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct ReachabilityOperationId(String);

impl<'de> Deserialize<'de> for ReachabilityOperationId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        ReachabilityOperationId::new(value).map_err(serde::de::Error::custom)
    }
}

impl ReachabilityOperationId {
    /// Construct an operation identifier, rejecting empty values.
    ///
    /// # Errors
    ///
    /// Returns [`ReachabilityContractError::EmptyIdentity`] when `value` is
    /// empty.
    pub fn new(value: impl Into<String>) -> Result<Self, ReachabilityContractError> {
        let value = value.into();
        if value.is_empty() {
            return Err(ReachabilityContractError::EmptyIdentity);
        }
        Ok(Self(value))
    }

    /// The opaque identifier value.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Deterministic identity of one stage within a reachability operation.
///
/// Stage identifiers are declared by the consuming stage (for example graph
/// admission, SCC condensation, closure traversal); they are opaque,
/// non-empty strings, never display names or URIs.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct ReachabilityStageId(String);

impl<'de> Deserialize<'de> for ReachabilityStageId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        ReachabilityStageId::new(value).map_err(serde::de::Error::custom)
    }
}

impl ReachabilityStageId {
    /// Construct a stage identifier, rejecting empty values.
    ///
    /// # Errors
    ///
    /// Returns [`ReachabilityContractError::EmptyIdentity`] when `value` is
    /// empty.
    pub fn new(value: impl Into<String>) -> Result<Self, ReachabilityContractError> {
        let value = value.into();
        if value.is_empty() {
            return Err(ReachabilityContractError::EmptyIdentity);
        }
        Ok(Self(value))
    }

    /// The opaque stage identifier value.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Identity of one fact family in a denominator ledger.
///
/// Fact families are named by their owning producer; the identifier is
/// opaque and non-empty.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct ReachabilityFactFamilyId(String);

impl<'de> Deserialize<'de> for ReachabilityFactFamilyId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        ReachabilityFactFamilyId::new(value).map_err(serde::de::Error::custom)
    }
}

impl ReachabilityFactFamilyId {
    /// Construct a fact-family identifier, rejecting empty values.
    ///
    /// # Errors
    ///
    /// Returns [`ReachabilityContractError::EmptyIdentity`] when `value` is
    /// empty.
    pub fn new(value: impl Into<String>) -> Result<Self, ReachabilityContractError> {
        let value = value.into();
        if value.is_empty() {
            return Err(ReachabilityContractError::EmptyIdentity);
        }
        Ok(Self(value))
    }

    /// The opaque fact-family identifier value.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Fail-closed validation error for the reachability operation contract.
///
/// Every constructor that can produce an incoherent subject, budget,
/// profile, or outcome returns this error instead of silently normalizing
/// the incoherence.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReachabilityContractError {
    /// An opaque identity, stage, family, or profile value was empty.
    EmptyIdentity,
    /// The tracker's budget profile does not match the subject's declared
    /// work-budget profile identity.
    BudgetProfileMismatch,
    /// A free-form string was supplied where a closed operation kind is
    /// required; unknown kinds fail closed.
    UnknownOperationKind(String),
    /// A free-form string was supplied where a closed work dimension is
    /// required.
    UnknownWorkDimension(String),
    /// The budget selected no operation kind.
    EmptyOperationKindSelection,
    /// A dimension required by the operation has neither a limit nor a
    /// reviewed unlimited justification.
    MissingRequiredDimension {
        /// The dimension without a limit.
        dimension: ReachabilityWorkDimension,
    },
    /// An unlimited dimension lacked the required reviewed reason or
    /// higher-level safety bound.
    UnlimitedWithoutSafetyBound {
        /// The dimension declared unlimited without a bound.
        dimension: ReachabilityWorkDimension,
    },
    /// A `Complete` semantic outcome was claimed without a value; exact
    /// empties must use `LegitimateEmpty`.
    CompleteWithoutValue,
    /// A `LegitimateEmpty` semantic outcome was claimed while retaining a
    /// value.
    EmptyWithRetainedValue,
    /// A `Partial` semantic outcome retained a value without an explicit
    /// claim-ceiling limitation.
    PartialWithoutLimitation,
    /// A value was retained alongside a truth state that cannot carry one.
    ValueWithNonValuedTruth,
    /// An exact or legitimate-empty claim conflicts with stage limitations
    /// or a terminal state recorded in the work receipt.
    ClaimConflictsWithLimitations,
    /// An exact or legitimate-empty claim was made over a receipt without
    /// complete instrument evidence.
    MissingInstrumentEvidence,
    /// The retained value or receipt shape does not match the requested
    /// operation.
    IncoherentOutcome,
    /// A work-honesty rule was violated (for example reuse recorded without
    /// the declared current subject identity).
    WorkHonesty(ReachabilityWorkHonestyError),
    /// A view was truncated without a truncation reason, or retained
    /// proof/currentness fields were dropped.
    IncoherentBoundedView,
}

impl std::fmt::Display for ReachabilityContractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyIdentity => write!(f, "an opaque reachability identity was empty"),
            Self::BudgetProfileMismatch => {
                write!(f, "the budget profile does not match the subject's declared profile")
            }
            Self::UnknownOperationKind(name) => {
                write!(f, "unknown reachability operation kind `{name}`")
            }
            Self::UnknownWorkDimension(name) => {
                write!(f, "unknown reachability work dimension `{name}`")
            }
            Self::EmptyOperationKindSelection => {
                write!(f, "a reachability budget selected no operation kind")
            }
            Self::MissingRequiredDimension { dimension } => write!(
                f,
                "required work dimension `{}` has no limit or reviewed unlimited justification",
                dimension.as_str()
            ),
            Self::UnlimitedWithoutSafetyBound { dimension } => write!(
                f,
                "unlimited work dimension `{}` lacks a reviewed reason and safety bound",
                dimension.as_str()
            ),
            Self::CompleteWithoutValue => {
                write!(f, "a Complete truth outcome requires a value")
            }
            Self::EmptyWithRetainedValue => {
                write!(f, "a LegitimateEmpty truth outcome cannot retain a value")
            }
            Self::PartialWithoutLimitation => {
                write!(f, "a retained partial value requires an explicit claim-ceiling limitation")
            }
            Self::ValueWithNonValuedTruth => {
                write!(f, "a value cannot be retained alongside this truth outcome")
            }
            Self::ClaimConflictsWithLimitations => {
                write!(f, "an exact claim conflicts with receipt limitations or terminal state")
            }
            Self::MissingInstrumentEvidence => {
                write!(f, "an exact claim requires complete instrument evidence")
            }
            Self::IncoherentOutcome => write!(f, "the outcome shape is incoherent"),
            Self::WorkHonesty(error) => write!(f, "work honesty violation: {error}"),
            Self::IncoherentBoundedView => {
                write!(f, "the bounded view dropped required proof fields")
            }
        }
    }
}

impl std::error::Error for ReachabilityContractError {}
