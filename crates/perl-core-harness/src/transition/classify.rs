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
//! `file_results` before `NoChange`. Compiler/invocation/capability/environment
//! subject ids are retained on the V2 ratchet but are not present on
//! [`RunReport`]; they are bound in receipts by a later slice.

use crate::transition::model::AcceptedBaseline;
use perl_core_harness_types::{
    COMPILE_BASELINE_V2_SCHEMA_VERSION, CompatibilityTransition, CompileBaselineV2,
    RUN_REPORT_SCHEMA_VERSION, RunFileResult, RunReport, RunnerStatus,
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
    // Only a terminal successful harness status is a complete observation.
    // `None` (e.g. signal-killed runs) must not manufacture Regression/NoChange.
    if current.harness_status != Some(0) {
        return not_proven(format!(
            "accepted and current observations are not comparable: current harness_status {:?} is not a complete successful run",
            current.harness_status
        ));
    }
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

    if let AcceptedBaseline::V2(value) = accepted
        && accepted.file_results() == current.file_results.as_slice()
        && accepted.buckets() == &current.buckets
        && accepted.failures() == current.failures.as_slice()
        && value.semantic_boundaries == current.semantic_boundaries
        && current.harness_status == Some(0)
        && summary_matches_file_results(current)
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
    if value.schema_version != COMPILE_BASELINE_V2_SCHEMA_VERSION {
        return Some(format!(
            "accepted and current observations are not comparable: unsupported accepted V2 schema {}",
            value.schema_version
        ));
    }
    if value.report_schema_version != RUN_REPORT_SCHEMA_VERSION
        || current.schema_version != RUN_REPORT_SCHEMA_VERSION
    {
        return Some(
            "accepted and current observations are not comparable: report schema is not the supported run-report version"
                .into(),
        );
    }
    if let Some(path) = first_duplicate_str(value.file_membership.iter().map(String::as_str)) {
        return Some(format!(
            "accepted and current observations are not comparable: accepted V2 file_membership repeats path {path}"
        ));
    }
    let membership = value.file_membership.iter().map(String::as_str).collect::<BTreeSet<_>>();
    let accepted_paths =
        value.file_results.iter().map(|result| result.path.as_str()).collect::<BTreeSet<_>>();
    if membership != accepted_paths {
        return Some(
            "accepted and current observations are not comparable: accepted V2 file_results do not match immutable file_membership"
                .into(),
        );
    }
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

fn summary_matches_file_results(current: &RunReport) -> bool {
    let files_total = current.file_results.len();
    let files_passed =
        current.file_results.iter().filter(|result| result.status == RunnerStatus::Pass).count();
    let files_failed = files_total.saturating_sub(files_passed);
    let tap_assertions_total =
        current.file_results.iter().map(|result| result.assertions_total).sum::<usize>();
    let tap_assertions_passed =
        current.file_results.iter().map(|result| result.assertions_passed).sum::<usize>();
    current.summary.files_total == files_total
        && current.summary.files_passed == files_passed
        && current.summary.files_failed == files_failed
        && current.summary.tap_assertions_total == tap_assertions_total
        && current.summary.tap_assertions_passed == tap_assertions_passed
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
    first_duplicate_str(results.iter().map(|result| result.path.as_str()))
}

fn first_duplicate_str<'a>(paths: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let mut seen = BTreeSet::new();
    paths.into_iter().find(|path| !seen.insert(*path))
}

fn index_by_path(results: &[RunFileResult]) -> BTreeMap<&str, &RunFileResult> {
    results.iter().map(|result| (result.path.as_str(), result)).collect()
}
