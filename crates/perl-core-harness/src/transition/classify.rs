use crate::transition::model::AcceptedBaseline;
use color_eyre::eyre::Result;
use perl_core_harness_types::{
    CompatibilityTransition, ObservedSemanticBoundary, RunFailure, RunReport, RunnerStatus,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classification {
    pub transition: CompatibilityTransition,
    pub reason: String,
    pub requires_candidate: bool,
    pub semantic_boundary_change: bool,
}

pub fn classify_transition(
    accepted: &AcceptedBaseline,
    current: &RunReport,
) -> Result<Classification> {
    let accepted_by_path = accepted
        .file_results()
        .iter()
        .map(|result| (result.path.as_str(), result))
        .collect::<BTreeMap<_, _>>();
    let current_by_path = current
        .file_results
        .iter()
        .map(|result| (result.path.as_str(), result))
        .collect::<BTreeMap<_, _>>();

    let mut identity_mismatches = Vec::new();
    match accepted {
        AcceptedBaseline::V1(value) => {
            if current.schema_version != value.report_schema_version {
                identity_mismatches.push(format!(
                    "report schema differs (accepted {}, current {})",
                    value.report_schema_version, current.schema_version
                ));
            }
            if current.mode != value.mode {
                identity_mismatches.push(format!(
                    "mode differs (accepted {}, current {})",
                    value.mode, current.mode
                ));
            }
            if current.profile != value.profile {
                identity_mismatches.push(format!(
                    "profile differs (accepted {}, current {})",
                    value.profile, current.profile
                ));
            }
        }
        AcceptedBaseline::V2(value) => {
            if current.schema_version != value.report_schema_version {
                identity_mismatches.push(format!(
                    "report schema differs (accepted {}, current {})",
                    value.report_schema_version, current.schema_version
                ));
            }
            if current.mode != value.mode {
                identity_mismatches.push(format!(
                    "mode differs (accepted {}, current {})", value.mode, current.mode
                ));
            }
            if current.profile != value.profile {
                identity_mismatches.push(format!(
                    "profile differs (accepted {}, current {})",
                    value.profile, current.profile
                ));
            }
            if current.runner != value.runner {
                identity_mismatches.push(format!(
                    "runner differs (accepted {}, current {})",
                    value.runner, current.runner
                ));
            }
            if current.commit != value.repository_commit {
                identity_mismatches.push(format!(
                    "repository commit differs (accepted {}, current {})",
                    value.repository_commit, current.commit
                ));
            }
            if current.perl_ref != value.perl_resolved_ref {
                identity_mismatches.push(format!(
                    "Perl reference differs (accepted {}, current {})",
                    value.perl_resolved_ref, current.perl_ref
                ));
            }
            let expected_membership =
                value.file_membership.iter().map(String::as_str).collect::<BTreeSet<_>>();
            let observed_membership = current
                .file_results
                .iter()
                .map(|result| result.path.as_str())
                .collect::<BTreeSet<_>>();
            if expected_membership != observed_membership {
                identity_mismatches
                    .push("file membership differs from the immutable v2 denominator".into());
            }
        }
    }
    if !identity_mismatches.is_empty() {
        return Ok(Classification {
            transition: CompatibilityTransition::NotProven,
            reason: format!(
                "accepted and current observations are not comparable: {}",
                identity_mismatches.join("; ")
            ),
            requires_candidate: false,
            semantic_boundary_change: false,
        });
    }

    let accepted_paths =
        accepted_by_path.keys().copied().collect::<std::collections::BTreeSet<_>>();
    let mut regressions = Vec::new();
    let mut improvements = Vec::new();
    let mut other_result_change = false;
    for (bucket, current_count) in &current.buckets {
        let baseline_count = accepted.buckets().get(bucket).copied().unwrap_or(0);
        if *current_count > baseline_count {
            regressions.push(format!(
                "bucket {bucket} increased from {baseline_count} to {current_count}"
            ));
        }
    }
    for (path, accepted_result) in accepted_by_path {
        let current_result = *current_by_path.get(path).ok_or_else(|| {
            color_eyre::eyre::eyre!("current observation is missing accepted file {path}")
        })?;
        if accepted_result.status == RunnerStatus::Pass
            && current_result.status == RunnerStatus::Fail
        {
            regressions.push(format!("{path} changed from pass to fail"));
        }
        if current_result.assertions_passed < accepted_result.assertions_passed {
            regressions.push(format!(
                "{path} passed fewer assertions ({}/{})",
                current_result.assertions_passed, accepted_result.assertions_passed
            ));
        }
        if current_result.assertions_total < accepted_result.assertions_total {
            regressions.push(format!(
                "{path} declared fewer assertions ({}/{})",
                current_result.assertions_total, accepted_result.assertions_total
            ));
        }
        if accepted_result.status == RunnerStatus::Fail
            && current_result.status == RunnerStatus::Pass
        {
            improvements.push(format!("{path} changed from fail to pass"));
        }
        if current_result.assertions_passed > accepted_result.assertions_passed {
            improvements.push(format!(
                "{path} passed more assertions ({}/{})",
                current_result.assertions_passed, accepted_result.assertions_passed
            ));
        }
        if current_result != accepted_result {
            other_result_change = true;
        }
    }

    for path in current_by_path.keys() {
        if !accepted_paths.contains(path) {
            other_result_change = true;
        }
    }

    let accepted_boundaries = accepted.semantic_boundaries().map(sorted_boundaries);
    let current_boundaries = sorted_boundaries(&current.semantic_boundaries);
    let semantic_boundary_change =
        accepted_boundaries.as_ref().is_some_and(|boundaries| boundaries != &current_boundaries);
    let failure_inventory_change =
        sorted_failures(accepted.failures()) != sorted_failures(&current.failures);
    // Increases are regressions (handled above). Any remaining bucket-map delta is a
    // ratchet-view change that must not fall through to exact v2 no-change acceptance.
    let bucket_inventory_change = accepted.buckets() != &current.buckets;

    if !regressions.is_empty() {
        return Ok(Classification {
            transition: CompatibilityTransition::Regression,
            reason: format!(
                "complete observation regresses the accepted ratchet: {}",
                regressions.join("; ")
            ),
            requires_candidate: false,
            semantic_boundary_change,
        });
    }

    let accepted_state = accepted.state();
    let current_state = run_state(current);
    if current_state.files_passed < accepted_state.files_passed
        || current_state.tap_assertions_passed < accepted_state.tap_assertions_passed
    {
        return Ok(Classification {
            transition: CompatibilityTransition::Regression,
            reason: format!(
                "complete observation regressed from {}/{} files and {}/{} assertions to {}/{} files and {}/{} assertions",
                accepted_state.files_passed,
                accepted_state.files_total,
                accepted_state.tap_assertions_passed,
                accepted_state.tap_assertions_total,
                current_state.files_passed,
                current_state.files_total,
                current_state.tap_assertions_passed,
                current_state.tap_assertions_total,
            ),
            requires_candidate: false,
            semantic_boundary_change,
        });
    }

    if !improvements.is_empty()
        || current_state.files_passed > accepted_state.files_passed
        || current_state.tap_assertions_passed > accepted_state.tap_assertions_passed
    {
        return Ok(Classification {
            transition: CompatibilityTransition::ImprovementCandidate,
            reason: format!(
                "complete observation may improve the accepted ratchet: {}",
                if improvements.is_empty() {
                    format!(
                        "aggregate result changed from {}/{} to {}/{} files",
                        accepted_state.files_passed,
                        accepted_state.files_total,
                        current_state.files_passed,
                        current_state.files_total,
                    )
                } else {
                    improvements.join("; ")
                }
            ),
            requires_candidate: true,
            semantic_boundary_change,
        });
    }

    if matches!(accepted, AcceptedBaseline::V1(_)) {
        return Ok(Classification {
            transition: CompatibilityTransition::ContractCorrectionCandidate,
            reason: "observation matches the legacy score, but the accepted v1 ratchet lacks immutable series and typed authority identity; migration requires explicit review".into(),
            requires_candidate: true,
            semantic_boundary_change,
        });
    }

    if semantic_boundary_change
        || other_result_change
        || failure_inventory_change
        || bucket_inventory_change
    {
        return Ok(Classification {
            transition: CompatibilityTransition::ContractCorrectionCandidate,
            reason: if failure_inventory_change {
                "compile score did not regress, but typed failure inventory changed relative to the accepted v2 ratchet"
                    .into()
            } else if bucket_inventory_change {
                "compile score did not regress, but bucket inventory changed relative to the accepted v2 ratchet"
                    .into()
            } else {
                "compile score did not regress, but file-result or semantic-boundary evidence changed relative to the accepted v2 ratchet"
                    .into()
            },
            requires_candidate: true,
            semantic_boundary_change,
        });
    }

    Ok(Classification {
        transition: CompatibilityTransition::NoChange,
        reason: "complete observation exactly matches the accepted v2 ratchet".into(),
        requires_candidate: false,
        semantic_boundary_change: false,
    })
}

fn run_state(report: &RunReport) -> crate::transition::model::TransitionRunState {
    crate::transition::model::TransitionRunState {
        files_total: report.summary.files_total,
        files_passed: report.summary.files_passed,
        files_failed: report.summary.files_failed,
        tap_assertions_total: report.summary.tap_assertions_total,
        tap_assertions_passed: report.summary.tap_assertions_passed,
    }
}

fn sorted_failures(failures: &[RunFailure]) -> Vec<RunFailure> {
    let mut values = failures
        .iter()
        .map(|failure| {
            let mut canonical = failure.clone();
            canonical.lsp_impact.sort();
            canonical
        })
        .collect::<Vec<_>>();
    values.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.bucket.cmp(&right.bucket))
            .then_with(|| left.phase.cmp(&right.phase))
            .then_with(|| left.first_diagnostic.cmp(&right.first_diagnostic))
            .then_with(|| left.workstream.cmp(&right.workstream))
            .then_with(|| left.lsp_impact.cmp(&right.lsp_impact))
    });
    values
}

fn sorted_boundaries(boundaries: &[ObservedSemanticBoundary]) -> Vec<ObservedSemanticBoundary> {
    let mut values = boundaries.to_vec();
    values.sort_by_key(|boundary| {
        (
            boundary.path.clone(),
            boundary.id.clone(),
            boundary.source_span.start,
            boundary.source_span.end,
        )
    });
    values
}
