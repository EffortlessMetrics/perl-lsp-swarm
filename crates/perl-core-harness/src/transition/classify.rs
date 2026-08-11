//! Minimal transition classifier core.
//!
//! This slice only settles three discriminating outcomes against an accepted
//! ratchet:
//! - incomparable observations → [`CompatibilityTransition::NotProven`]
//! - any accepted pass that becomes a fail → [`CompatibilityTransition::Regression`]
//! - exact V2 file-result identity → [`CompatibilityTransition::NoChange`]
//!
//! Bucket inventory, typed failures, assertion deltas, improvements, and V1
//! migration remain follow-up classifier slices. Aggregate `summary` fields are
//! ignored so forged totals cannot manufacture a transition.

use crate::transition::model::AcceptedBaseline;
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

/// Classify `current` against `accepted` for the minimal core outcomes above.
pub fn classify_transition(accepted: &AcceptedBaseline, current: &RunReport) -> Classification {
    if let Some(path) = first_duplicate_path(accepted.file_results()) {
        return not_proven(format!(
            "accepted and current observations are not comparable: accepted observation repeats file-result path {path}"
        ));
    }
    if let Some(path) = first_duplicate_path(&current.file_results) {
        return not_proven(format!(
            "accepted and current observations are not comparable: current observation repeats file-result path {path}"
        ));
    }

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
                "complete observation regresses the accepted ratchet: {}",
                regressions.join("; ")
            ),
            requires_candidate: false,
            semantic_boundary_change: false,
        };
    }

    if matches!(accepted, AcceptedBaseline::V2(_))
        && accepted.file_results() == current.file_results.as_slice()
    {
        return Classification {
            transition: CompatibilityTransition::NoChange,
            reason: "complete observation exactly matches the accepted v2 ratchet".into(),
            requires_candidate: false,
            semantic_boundary_change: false,
        };
    }

    not_proven(
        "observation is comparable but outside the minimal classifier slice; defer to a later transition arm"
            .into(),
    )
}

fn v2_incomparable(value: &CompileBaselineV2, current: &RunReport) -> Option<String> {
    let accepted_subject = (
        value.report_schema_version.as_str(),
        value.mode,
        value.profile,
        value.runner,
        value.perl_resolved_ref.as_str(),
    );
    let current_subject = (
        current.schema_version.as_str(),
        current.mode,
        current.profile,
        current.runner,
        current.perl_ref.as_str(),
    );
    let expected_membership =
        value.file_membership.iter().map(String::as_str).collect::<BTreeSet<_>>();
    let observed_membership =
        current.file_results.iter().map(|result| result.path.as_str()).collect::<BTreeSet<_>>();
    if accepted_subject == current_subject && expected_membership == observed_membership {
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

fn first_duplicate_path(results: &[RunFileResult]) -> Option<&str> {
    let mut seen = BTreeSet::new();
    for result in results {
        if !seen.insert(result.path.as_str()) {
            return Some(result.path.as_str());
        }
    }
    None
}

fn index_by_path(results: &[RunFileResult]) -> BTreeMap<&str, &RunFileResult> {
    results.iter().map(|result| (result.path.as_str(), result)).collect()
}
