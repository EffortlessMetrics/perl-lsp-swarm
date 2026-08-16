//! Minimal transition classifier core.
//!
//! This slice only settles three discriminating outcomes against an accepted
//! ratchet:
//! - incomparable observations → [`CompatibilityTransition::NotProven`]
//! - any accepted pass that becomes a fail → [`CompatibilityTransition::Regression`]
//! - exact V2 observation identity → [`CompatibilityTransition::NoChange`]
//!
//! Assertion deltas, improvements, and V1 migration remain follow-up classifier
//! slices. Aggregate `summary` fields cannot manufacture a transition: they are
//! ignored for Regression/Improvement scoring and must reconcile with detailed
//! `file_results` before any definitive outcome. Compiler/invocation/capability/
//! environment subject ids are retained on the V2 ratchet but are not present on
//! [`RunReport`]; they are bound in receipts by a later slice.

use crate::transition::model::AcceptedBaseline;
use crate::transition::validate::{validate_accepted_baseline, validate_run_report};
use perl_core_harness_types::{
    CompatibilityTransition, CompileBaseline, CompileBaselineV2, RunFileResult, RunReport,
    RunnerStatus,
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
///
/// Canonical validation runs before any definitive transition arm. Invalid or
/// incomplete evidence yields [`CompatibilityTransition::NotProven`].
pub fn classify_transition(accepted: &AcceptedBaseline, current: &RunReport) -> Classification {
    if let Err(error) = validate_accepted_baseline(accepted) {
        return not_proven(format!(
            "accepted and current observations are not comparable: {}",
            error.reason
        ));
    }
    if let Err(error) = validate_run_report(current) {
        return not_proven(format!(
            "accepted and current observations are not comparable: {}",
            error.reason
        ));
    }

    match accepted {
        AcceptedBaseline::V2(value) => {
            if let Some(reason) = v2_subject_incomparable(value, current) {
                return not_proven(reason);
            }
        }
        AcceptedBaseline::V1(value) => {
            if let Some(reason) = v1_subject_incomparable(value, current) {
                return not_proven(reason);
            }
        }
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

    if let AcceptedBaseline::V2(value) = accepted
        && accepted.file_results() == current.file_results.as_slice()
        && accepted.buckets() == &current.buckets
        && accepted.failures() == current.failures.as_slice()
        && value.semantic_boundaries == current.semantic_boundaries
        && current.harness_status == Some(0)
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

fn v2_subject_incomparable(value: &CompileBaselineV2, current: &RunReport) -> Option<String> {
    let membership = value.file_membership.iter().map(String::as_str).collect::<BTreeSet<_>>();
    let accepted_subject =
        (value.mode, value.profile, value.runner, value.perl_resolved_ref.as_str());
    let current_subject =
        (current.mode, current.profile, current.runner, current.perl_ref.as_str());
    let observed_membership =
        current.file_results.iter().map(|result| result.path.as_str()).collect::<BTreeSet<_>>();
    if accepted_subject == current_subject && membership == observed_membership {
        return None;
    }
    Some(
        "accepted and current observations are not comparable: V2 subject identity or immutable file membership differs"
            .into(),
    )
}

/// V1 baselines carry mode and profile but no canonical membership list, so
/// comparability is limited to the run subject dimensions that are present.
///
/// V1 subject checks are deliberately more conservative than V2: runner is
/// required on V1 baselines via the struct field, but there is no immutable
/// `file_membership` denominator to cross-check, so file sets are compared
/// directly from `file_results`.
fn v1_subject_incomparable(value: &CompileBaseline, current: &RunReport) -> Option<String> {
    // Mode and profile must match so the observations are measuring the same thing.
    if value.mode != current.mode {
        return Some(format!(
            "accepted and current observations are not comparable: V1 accepted mode {:?} differs from current mode {:?}",
            value.mode, current.mode
        ));
    }
    if value.profile != current.profile {
        return Some(format!(
            "accepted and current observations are not comparable: V1 accepted profile {:?} differs from current profile {:?}",
            value.profile, current.profile
        ));
    }
    None
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
