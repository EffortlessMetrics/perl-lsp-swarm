//! Deterministic work receipts and work-honesty paths.

use super::{
    ReachabilityClaimLimitation, ReachabilityStageId, ReachabilitySubjectIdentity,
    ReachabilityTerminalState, ReachabilityWorkDimension,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Work-honesty violation detected by the tracker.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReachabilityWorkHonestyError {
    /// Reuse was recorded without a declared current subject identity.
    ReuseWithoutDeclaredIdentity {
        /// The stage that attempted the reuse record.
        stage: ReachabilityStageId,
    },
    /// A full construction was attempted after a validated reuse was
    /// recorded for the same target — a full rebuild is not reuse.
    FullConstructionAfterValidatedReuse {
        /// The stage that attempted the record.
        stage: ReachabilityStageId,
    },
}

impl std::fmt::Display for ReachabilityWorkHonestyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReuseWithoutDeclaredIdentity { stage } => write!(
                f,
                "stage `{}` recorded reuse without a declared current subject identity",
                stage.as_str()
            ),
            Self::FullConstructionAfterValidatedReuse { stage } => write!(
                f,
                "stage `{}` recorded a full construction after a validated reuse",
                stage.as_str()
            ),
        }
    }
}

impl std::error::Error for ReachabilityWorkHonestyError {}

/// The target one work-path entry describes.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ReachabilityWorkPathTarget {
    /// Graph input construction (#10915).
    GraphInput,
    /// SCC/component-graph construction (#10921).
    ComponentGraph,
    /// Production/test closure (#10928).
    Closure,
    /// Query or explanation projection (#10935).
    QueryProjection,
    /// Diagnostic composition/projection (#10941/#10947).
    DiagnosticProjection,
    /// Result reuse revalidation (#10957).
    ResultReuse,
}

/// One work-honesty path recorded in a receipt.
///
/// A cache hit, cloned structure, unchanged count, or matching digest is not
/// work avoided without the declared current identity and work path, so
/// validated-reuse entries retain the exact subject identity they reused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReachabilityWorkPath {
    stage: ReachabilityStageId,
    target: ReachabilityWorkPathTarget,
    reused_identity: Option<ReachabilitySubjectIdentity>,
    fully_constructed: bool,
}

impl ReachabilityWorkPath {
    /// The stage that recorded this path.
    #[must_use]
    pub const fn stage(&self) -> &ReachabilityStageId {
        &self.stage
    }

    /// The target this path describes.
    #[must_use]
    pub const fn target(&self) -> &ReachabilityWorkPathTarget {
        &self.target
    }

    /// Whether this path is a validated reuse rather than a full
    /// construction.
    #[must_use]
    pub const fn is_validated_reuse(&self) -> bool {
        !self.fully_constructed
    }

    /// The exact subject identity a validated reuse reused.
    #[must_use]
    pub fn reused_identity(&self) -> Option<&ReachabilitySubjectIdentity> {
        self.reused_identity.as_ref()
    }
}

/// One budget-exhaustion attempt recorded in a receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReachabilityExhaustionAttempt {
    /// The dimension whose limit was hit.
    pub dimension: ReachabilityWorkDimension,
    /// The limit in force.
    pub limit: u64,
    /// The units already charged when the attempt was refused.
    pub charged: u64,
}

/// One stage limitation attached during the operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReachabilityStageLimitation {
    /// The stage that recorded the limitation.
    pub stage: ReachabilityStageId,
    /// The limitation recorded.
    pub limitation: ReachabilityClaimLimitation,
}

/// The deterministic receipt of one reachability operation's work.
///
/// Receipts are canonical: counters are keyed by dimension in sorted order,
/// stage records preserve execution order, and receipts built from permuted
/// charge orders serialize identically. Receipts are produced by
/// [`super::ReachabilityWorkTracker::finish`] and are read-only thereafter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReachabilityWorkReceipt {
    charged: BTreeMap<ReachabilityWorkDimension, u64>,
    exhausted_attempts: Vec<ReachabilityExhaustionAttempt>,
    work_paths: Vec<ReachabilityWorkPath>,
    checkpoints_observed: Vec<ReachabilityStageId>,
    completed_stages: Vec<ReachabilityStageId>,
    stage_limitations: Vec<ReachabilityStageLimitation>,
    terminal: Option<ReachabilityTerminalState>,
    work_after_eligibility_lost: u64,
    work_after_eligibility_lost_overflow: bool,
    instrument_identity: Option<ReachabilitySubjectIdentity>,
    instrument_evidence_complete: bool,
}

impl ReachabilityWorkReceipt {
    /// Build a receipt from tracker state. Tracker-only; receipts are
    /// read-only everywhere else.
    #[allow(clippy::too_many_arguments)] // mirrors the receipt fields exactly
    pub(super) fn from_parts(
        charged: BTreeMap<ReachabilityWorkDimension, u64>,
        exhausted_attempts: Vec<ReachabilityExhaustionAttempt>,
        work_paths: Vec<ReachabilityWorkPath>,
        checkpoints_observed: Vec<ReachabilityStageId>,
        completed_stages: Vec<ReachabilityStageId>,
        stage_limitations: Vec<ReachabilityStageLimitation>,
        terminal: Option<ReachabilityTerminalState>,
        work_after_eligibility_lost: u64,
        work_after_eligibility_lost_overflow: bool,
        instrument_identity: Option<ReachabilitySubjectIdentity>,
        instrument_evidence_complete: bool,
    ) -> Self {
        Self {
            charged,
            exhausted_attempts,
            work_paths,
            checkpoints_observed,
            completed_stages,
            stage_limitations,
            terminal,
            work_after_eligibility_lost,
            work_after_eligibility_lost_overflow,
            instrument_identity,
            instrument_evidence_complete,
        }
    }

    /// Construct one work-path record. Tracker-only. A record is a full
    /// construction exactly when it reused no current subject identity, so
    /// the two facts cannot disagree.
    pub(super) fn work_path_record(
        stage: ReachabilityStageId,
        target: ReachabilityWorkPathTarget,
        reused_identity: Option<ReachabilitySubjectIdentity>,
    ) -> ReachabilityWorkPath {
        let fully_constructed = reused_identity.is_none();
        ReachabilityWorkPath { stage, target, reused_identity, fully_constructed }
    }

    /// The charged units per dimension, in canonical order.
    #[must_use]
    pub fn charged(&self) -> &BTreeMap<ReachabilityWorkDimension, u64> {
        &self.charged
    }

    /// Recorded budget-exhaustion attempts.
    #[must_use]
    pub fn exhausted_attempts(&self) -> &[ReachabilityExhaustionAttempt] {
        &self.exhausted_attempts
    }

    /// Recorded work-honesty paths.
    #[must_use]
    pub fn work_paths(&self) -> &[ReachabilityWorkPath] {
        &self.work_paths
    }

    /// Checkpoints at which the control was observed, in execution order.
    #[must_use]
    pub fn checkpoints_observed(&self) -> &[ReachabilityStageId] {
        &self.checkpoints_observed
    }

    /// Stages completed before the operation ended.
    #[must_use]
    pub fn completed_stages(&self) -> &[ReachabilityStageId] {
        &self.completed_stages
    }

    /// Every limitation attached by any stage; never removable.
    #[must_use]
    pub fn stage_limitations(&self) -> &[ReachabilityStageLimitation] {
        &self.stage_limitations
    }

    /// The terminal observation, if the operation ended terminally.
    #[must_use]
    pub fn terminal(&self) -> Option<&ReachabilityTerminalState> {
        self.terminal.as_ref()
    }

    /// Units charged after publication eligibility was lost.
    #[must_use]
    pub const fn work_after_eligibility_lost(&self) -> u64 {
        self.work_after_eligibility_lost
    }

    /// Whether the post-eligibility work accounting itself overflowed, so
    /// [`Self::work_after_eligibility_lost`] is a lower bound rather than an
    /// exact total. The receipt is already non-publishable when this is set.
    #[must_use]
    pub const fn work_after_eligibility_lost_overflow(&self) -> bool {
        self.work_after_eligibility_lost_overflow
    }

    /// The instrument identity that supplied evidence, when retained.
    #[must_use]
    pub fn instrument_identity(&self) -> Option<&ReachabilitySubjectIdentity> {
        self.instrument_identity.as_ref()
    }

    /// Whether complete instrument evidence backs this receipt.
    ///
    /// A missing instrument is never zero work; without complete evidence
    /// the operation cannot claim exactness.
    #[must_use]
    pub const fn instrument_evidence_complete(&self) -> bool {
        self.instrument_evidence_complete
    }

    /// Whether one target was satisfied by validated reuse.
    #[must_use]
    pub fn is_validated_reuse_of(&self, target: &ReachabilityWorkPathTarget) -> bool {
        self.work_paths.iter().any(|path| path.target() == target && path.is_validated_reuse())
    }
}
