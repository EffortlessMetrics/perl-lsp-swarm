//! Transition classifier over canonically validated compiler evidence.
//!
//! Raw accepted baselines and current reports are validated before any
//! definitive transition arm. Invalid, incomplete, or incomparable evidence
//! remains [`CompatibilityTransition::NotProven`].

use crate::transition::model::AcceptedBaseline;
use crate::transition::validation::{
    ValidatedAcceptedBaseline, ValidatedRunReport, validate_accepted_baseline,
    validate_run_report,
};
use perl_core_harness_types::{
    CompatibilityTransition, CompileBaselineV2, RunFileResult, RunReport, RunnerStatus,
};
use std::collections::{BTreeMap, BTreeSet};

/// Classification result for one accepted/current observation pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classification {
    /// Measured transition relative to the accepted ratchet.
    pub transition: CompatibilityTransition,
    /// Human-readable classification rationale.
    pub reason: String,
    /// Whether landing this observation requires an explicit candidate review.
    pub requires_candidate: bool,
    /// Whether semantic-boundary evidence changed (always false in this slice).
    pub semantic_boundary_change: bool,
}

/// Validate and classify `current` against `accepted`.
///
/// This compatibility entry point retains the existing API while ensuring raw
/// deserialized evidence cannot reach a definitive transition arm.
pub fn classify_transition(accepted: &AcceptedBaseline, current: &RunReport) -> Classification {
    let accepted = match validate_accepted_baseline(accepted) {
        Ok(validated) => validated,
        Err(validation) => {
            return not_proven(format!(
                "accepted and current observations are not comparable: {validation}"
            ));
        }
    };
    let current = match validate_run_report(current) {
        Ok(validated) => validated,
        Err(validation) => {
            return not_proven(format!(
                "accepted and current observations are not comparable: {validation}"
            ));
        }
    };
    classify_validated_transition(accepted, current)
}

/// Classify evidence that has already passed the canonical validators.
#[must_use]
pub fn classify_validated_transition(
    accepted: ValidatedAcceptedBaseline<'_>,
    current: ValidatedRunReport<'_>,
) -> Classification {
    let accepted = accepted.as_inner();
    let current = current.as_inner();

    if let AcceptedBaseline::V2(value) = accepted
        && let Some(reason) = v2_incomparable(value, current)
    {
        return not_proven(reason);
    }

    let accepted_by_path = index_by_path(accepted.file_results());
    let current_by_path = index_by_path(&current.file_results);
    let mut regressions = Vec::new();
    for (path, accepted_result) in &accepted_by_path {
        let Some(current_result) = current_by_path.get(path).copied() else {
            return not_proven(format!(
                "accepted and current observations are not comparable: current observation is missing accepted file {path}"
            ));
        };
        if accepted_result.status == RunnerStatus::Pass
            && current_result.status == RunnerStatus::Fail
        {
            regressions.push(format!("{path} changed from pass to fail"));
        }
    }
    if !regressions.is_empty() {
        return Classification {
            transition: CompatibilityTransition::Regression,
            reason: format!(
                "complete validated observation regresses the accepted ratchet: {}",
                regressions.join("; ")
            ),
            requires_candidate: false,
            semantic_boundary_change: false,
        };
    }

    if let AcceptedBaseline::V2(value) = accepted
        && accepted.file_results() == current.file_results.as_slice()
        && accepted.buckets() == &current.buckets
        && accepted.failures() == current.failures.as_slice()
        && value.semantic_boundaries == current.semantic_boundaries
    {
        return Classification {
            transition: CompatibilityTransition::NoChange,
            reason: "complete validated observation exactly matches the accepted v2 ratchet".into(),
            requires_candidate: false,
            semantic_boundary_change: false,
        };
    }

    not_proven(
        "validated observation is comparable but outside the minimal classifier slice; defer to a later transition arm"
            .into(),
    )
}

fn v2_incomparable(value: &CompileBaselineV2, current: &RunReport) -> Option<String> {
    let accepted_subject =
        (value.mode, value.profile, value.runner, value.perl_resolved_ref.as_str());
    let current_subject =
        (current.mode, current.profile, current.runner, current.perl_ref.as_str());
    let accepted_membership =
        value.file_membership.iter().map(String::as_str).collect::<BTreeSet<_>>();
    let observed_membership =
        current.file_results.iter().map(|result| result.path.as_str()).collect::<BTreeSet<_>>();
    if accepted_subject == current_subject && accepted_membership == observed_membership {
        return None;
    }
    Some(
        "accepted and current observations are not comparable: V2 subject identity or immutable file membership differs"
            .into(),
    )
}

fn not_proven(reason: String) -> Classification {
    Classification {
        transition: CompatibilityTransition::NotProven,
        reason,
        requires_candidate: false,
        semantic_boundary_change: false,
    }
}

fn index_by_path(results: &[RunFileResult]) -> BTreeMap<&str, &RunFileResult> {
    results.iter().map(|result| (result.path.as_str(), result)).collect()
}
