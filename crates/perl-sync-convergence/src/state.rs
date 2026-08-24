//! Closed transition vocabulary for `convergence_transaction.v1`.
//!
//! The set of states is closed: unknown spellings fail at the serde boundary
//! and no default state exists, so missing or instrument-failed evidence can
//! never silently become a passing state (negative controls 4 and 6).

use serde::{Deserialize, Serialize};
use std::fmt;

/// Lifecycle state of one convergence generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransitionState {
    /// Exact inputs observed; not yet planned.
    Observed,
    /// Planned candidate exists but is not materialized.
    Planned,
    /// Candidate tree materialized locally.
    Materialized,
    /// Candidate branch/PR published to the source repository.
    Published,
    /// Awaiting source admission decision.
    AdmissionPending,
    /// Source admission accepted.
    Admitted,
    /// Source admission rejected with immutable evidence.
    Rejected,
    /// Merge in progress on the source repository.
    MergePending,
    /// Landed merge completed.
    Merged,
    /// Post-merge verification passed.
    PostMergeVerified,
    /// Superseded by a successor generation.
    Superseded,
    /// No-op: nothing to converge; receipt retained without a PR.
    Noop,
    /// Proof did not establish the claim; never read as pass.
    NotProven,
    /// Instrument failure recorded; never read as pass.
    InstrumentFailure,
}

impl TransitionState {
    /// Canonical wire spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Observed => "observed",
            Self::Planned => "planned",
            Self::Materialized => "materialized",
            Self::Published => "published",
            Self::AdmissionPending => "admission_pending",
            Self::Admitted => "admitted",
            Self::Rejected => "rejected",
            Self::MergePending => "merge_pending",
            Self::Merged => "merged",
            Self::PostMergeVerified => "post_merge_verified",
            Self::Superseded => "superseded",
            Self::Noop => "noop",
            Self::NotProven => "not_proven",
            Self::InstrumentFailure => "instrument_failure",
        }
    }

    /// Whether this state is terminal for the generation's own lifecycle.
    ///
    /// Terminal states are retained as immutable history; only supersession
    /// or explicit invalidation can make descendants stale afterwards.
    #[must_use]
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::PostMergeVerified
                | Self::Superseded
                | Self::Noop
                | Self::NotProven
                | Self::InstrumentFailure
        )
    }

    /// Whether this state represents unresolved or failed evidence that must
    /// never be reported as success.
    #[must_use]
    pub fn is_unresolved_or_failed(self) -> bool {
        matches!(self, Self::Rejected | Self::NotProven | Self::InstrumentFailure)
    }
}

impl fmt::Display for TransitionState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One legal next action for a resumable transaction.
///
/// The vocabulary mirrors the controller's decision points; a fresh process
/// reconstructs exactly which actions are permitted from the durable journal
/// (issue #11282 acceptance: reconstruct current state and next legal actions).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermittedAction {
    /// Observe exact GitHub/source/swarm state into a new generation.
    ObserveInputs,
    /// Plan the projection candidate.
    PlanCandidate,
    /// Materialize the projected complete tree.
    MaterializeCandidate,
    /// Publish the candidate branch and PR (non-forced transport).
    PublishCandidate,
    /// Await source admission outcome.
    AwaitAdmission,
    /// Record admission rejection evidence.
    RecordRejection,
    /// Start the landing merge.
    StartLandingMerge,
    /// Verify the post-merge landing.
    VerifyLanding,
    /// Record a no-op receipt instead of opening a PR.
    RecordNoOp,
    /// Reconcile exact state and reclaim an expired lease via takeover.
    TakeoverAfterReconciliation,
    /// Create a successor generation because exact inputs moved.
    StartSuccessorGeneration,
}

impl PermittedAction {
    /// Canonical wire spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ObserveInputs => "observe_inputs",
            Self::PlanCandidate => "plan_candidate",
            Self::MaterializeCandidate => "materialize_candidate",
            Self::PublishCandidate => "publish_candidate",
            Self::AwaitAdmission => "await_admission",
            Self::RecordRejection => "record_rejection",
            Self::StartLandingMerge => "start_landing_merge",
            Self::VerifyLanding => "verify_landing",
            Self::RecordNoOp => "record_no_op",
            Self::TakeoverAfterReconciliation => "takeover_after_reconciliation",
            Self::StartSuccessorGeneration => "start_successor_generation",
        }
    }
}

impl fmt::Display for PermittedAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn full_closed_vocabulary_round_trips() {
        let all = [
            TransitionState::Observed,
            TransitionState::Planned,
            TransitionState::Materialized,
            TransitionState::Published,
            TransitionState::AdmissionPending,
            TransitionState::Admitted,
            TransitionState::Rejected,
            TransitionState::MergePending,
            TransitionState::Merged,
            TransitionState::PostMergeVerified,
            TransitionState::Superseded,
            TransitionState::Noop,
            TransitionState::NotProven,
            TransitionState::InstrumentFailure,
        ];
        assert_eq!(all.len(), 14);
        for state in all {
            let json = serde_json::to_string(&state).unwrap();
            assert_eq!(json.trim_matches('"'), state.as_str());
            assert_eq!(serde_json::from_str::<TransitionState>(&json).unwrap(), state);
        }
    }

    #[test]
    fn unknown_state_fails_closed() {
        assert!(serde_json::from_str::<TransitionState>("\"passed\"").is_err());
        assert!(serde_json::from_str::<TransitionState>("\"ok\"").is_err());
    }

    #[test]
    fn unresolved_states_never_report_success() {
        assert!(TransitionState::NotProven.is_unresolved_or_failed());
        assert!(TransitionState::InstrumentFailure.is_unresolved_or_failed());
        assert!(TransitionState::Rejected.is_unresolved_or_failed());
        assert!(!TransitionState::Merged.is_unresolved_or_failed());
    }
}
