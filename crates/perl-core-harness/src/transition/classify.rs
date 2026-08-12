//! Transition classifier over canonically validated compiler evidence.
//!
//! Context-free classification is deliberately non-authoritative because V1
//! lacks complete comparison identity and `RunReport` does not carry the V2
//! series/invocation/capability/environment subject set. Definitive transition
//! arms require an exact [`CompilerComparisonContext`].

use crate::transition::model::AcceptedBaseline;
use crate::transition::validation::{
    CompilerComparisonContext, ValidatedComparison, validate_accepted_baseline,
    validate_comparison, validate_run_report_structure,
};
use perl_core_harness_types::{
    CompatibilityTransition, RunFileResult, RunReport, RunnerStatus,
};
use std::collections::BTreeMap;

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

/// Structurally validate raw subjects, then refuse a definitive transition
/// because no exact series/invocation context was supplied.
pub fn classify_transition(accepted: &AcceptedBaseline, current: &RunReport) -> Classification {
    if let Err(validation) = validate_accepted_baseline(accepted) {
        return not_proven(format!(
            "accepted and current observations are not comparable: {validation}"
        ));
    }
    if let Err(validation) = validate_run_report_structure(current) {
        return not_proven(format!(
            "accepted and current observations are not comparable: {validation}"
        ));
    }
    not_proven(
        "accepted and current observations are not comparable: an exact V2 compiler comparison context is required"
            .into(),
    )
}

/// Validate an exact V2 comparison context and classify the resulting evidence.
pub fn classify_transition_with_context(
    accepted: &AcceptedBaseline,
    current: &RunReport,
    context: &CompilerComparisonContext,
) -> Classification {
    match validate_comparison(accepted, current, context) {
        Ok(comparison) => classify_validated_transition(comparison),
        Err(validation) => not_proven(format!(
            "accepted and current observations are not comparable: {validation}"
        )),
    }
}

/// Classify evidence that has already passed the canonical pair validator.
#[must_use]
pub fn classify_validated_transition(comparison: ValidatedComparison<'_>) -> Classification {
    let accepted = comparison.accepted();
    let current = comparison.current();
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

    let AcceptedBaseline::V2(value) = accepted else {
        return not_proven(
            "validated comparison unexpectedly retained a V1 baseline without complete subject identity"
                .into(),
        );
    };
    if accepted.file_results() == current.file_results.as_slice()
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

fn not_proven(reason: String) -> Classification {
    Classification {
        transition: CompatibilityTransition::NotProven,
        reason,
        requires_candidate: false,
        semantic_boundary_change: false,
    }
}

fn index_by_path(results: &[RunFileResult]) -> BTreeMap<&str, &RunFileResult> {
    results
        .iter()
        .map(|result| (result.path.as_str(), result))
        .collect()
}
