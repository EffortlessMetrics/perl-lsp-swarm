//! Discriminating regression matrix for the minimal transition classifier slice.
//!
//! Oracles use `assert_eq!` / `assert!` so RIPR can observe transition outcomes
//! (bail!-only checks are treated as unknown oracles).

use perl_core_harness::transition::{AcceptedBaseline, Classification, classify_transition};
use perl_core_harness_types::{
    COMPILE_BASELINE_V2_SCHEMA_VERSION, CompatibilityTransition, CompileBaselineV2, HarnessMode,
    HarnessProfile, HarnessRunner, RUN_REPORT_SCHEMA_VERSION, RunFileResult, RunReport, RunSummary,
    RunnerStatus,
};
use std::collections::BTreeMap;

/// RIPR-named observer covering the three settled classify_transition outcomes.
#[test]
fn classify_transition_call_presence_observer() {
    let regression = compensated_swap_classification();
    assert_eq!(regression.transition, CompatibilityTransition::Regression);
    assert!(!regression.requires_candidate);
    assert!(regression.reason.contains("changed from pass to fail"));

    let no_change = exact_match_classification();
    assert_eq!(no_change.transition, CompatibilityTransition::NoChange);
    assert!(!no_change.requires_candidate);
    assert!(!no_change.semantic_boundary_change);

    let not_proven = unexpected_file_classification();
    assert_eq!(not_proven.transition, CompatibilityTransition::NotProven);
    assert!(!not_proven.requires_candidate);
    assert!(not_proven.reason.contains("not comparable"));
}

#[test]
fn compensated_file_swap_is_still_regression() {
    let classification = compensated_swap_classification();
    assert_eq!(classification.transition, CompatibilityTransition::Regression);
    assert!(!classification.requires_candidate);
    assert!(classification.reason.contains("base/0.t changed from pass to fail"));
}

#[test]
fn exact_v2_match_is_no_change() {
    let classification = exact_match_classification();
    assert_eq!(classification.transition, CompatibilityTransition::NoChange);
    assert!(!classification.requires_candidate);
    assert!(!classification.semantic_boundary_change);
}

#[test]
fn unexpected_current_file_is_not_proven() {
    let classification = unexpected_file_classification();
    assert_eq!(classification.transition, CompatibilityTransition::NotProven);
    assert!(!classification.requires_candidate);
    assert!(classification.reason.contains("file membership"));
}

#[test]
fn duplicate_current_file_path_is_not_proven() {
    let accepted = sample_v2_baseline(1, 0);
    let mut current = sample_report(1, 0);
    current.file_results.push(RunFileResult {
        path: "base/0.t".into(),
        status: RunnerStatus::Pass,
        assertions_passed: 1,
        assertions_total: 1,
    });
    let classification = classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current);
    assert_eq!(classification.transition, CompatibilityTransition::NotProven);
    assert!(!classification.requires_candidate);
    assert!(classification.reason.contains("current observation repeats file-result path"));
}

#[test]
fn duplicate_accepted_file_path_is_not_proven() {
    let mut accepted = sample_v2_baseline(1, 1);
    accepted.file_results.push(RunFileResult {
        path: "base/0.t".into(),
        status: RunnerStatus::Pass,
        assertions_passed: 1,
        assertions_total: 1,
    });
    let current = sample_report(1, 1);
    let classification = classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current);
    assert_eq!(classification.transition, CompatibilityTransition::NotProven);
    assert!(!classification.requires_candidate);
    assert!(classification.reason.contains("accepted observation repeats"));
}

fn compensated_swap_classification() -> Classification {
    let accepted = sample_v2_baseline(2, 1);
    let mut current = sample_report(2, 1);
    current.file_results[0].status = RunnerStatus::Fail;
    current.file_results[0].assertions_passed = 0;
    current.file_results[1].status = RunnerStatus::Pass;
    current.file_results[1].assertions_passed = 1;
    classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current)
}

fn exact_match_classification() -> Classification {
    let accepted = sample_v2_baseline(2, 2);
    let current = sample_report(2, 2);
    classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current)
}

fn unexpected_file_classification() -> Classification {
    let accepted = sample_v2_baseline(1, 1);
    let mut current = sample_report(1, 1);
    current.file_results.push(RunFileResult {
        path: "unexpected/extra.t".into(),
        status: RunnerStatus::Pass,
        assertions_passed: 1,
        assertions_total: 1,
    });
    classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current)
}

fn sample_report(total: usize, passed: usize) -> RunReport {
    RunReport {
        schema_version: RUN_REPORT_SCHEMA_VERSION.into(),
        commit: "a".repeat(40),
        timestamp: "2026-08-11T00:00:00Z".into(),
        perl_ref: "perl".into(),
        prepared_tree: "<prepared>".into(),
        run_tree: "<run>".into(),
        host_perl: "perl".into(),
        runner: HarnessRunner::Test,
        mode: HarnessMode::Compile,
        profile: HarnessProfile::Base,
        harness_status: Some(0),
        summary: RunSummary {
            files_total: total,
            files_passed: passed,
            files_failed: total - passed,
            tap_assertions_total: total,
            tap_assertions_passed: passed,
        },
        buckets: BTreeMap::new(),
        file_results: sample_results(total, passed),
        failures: Vec::new(),
        semantic_boundaries: Vec::new(),
    }
}

fn sample_results(total: usize, passed: usize) -> Vec<RunFileResult> {
    (0..total)
        .map(|index| {
            let status = if index < passed { RunnerStatus::Pass } else { RunnerStatus::Fail };
            RunFileResult {
                path: format!("base/{index}.t"),
                status,
                assertions_passed: if status == RunnerStatus::Pass { 1 } else { 0 },
                assertions_total: 1,
            }
        })
        .collect()
}

fn sample_v2_baseline(total: usize, passed: usize) -> CompileBaselineV2 {
    CompileBaselineV2 {
        schema_version: COMPILE_BASELINE_V2_SCHEMA_VERSION.into(),
        report_schema_version: RUN_REPORT_SCHEMA_VERSION.into(),
        series_id: "series".into(),
        manifest_hash: "manifest".into(),
        repository_commit: "a".repeat(40),
        perl_resolved_ref: "perl".into(),
        preparation_receipt_id: "prepare".into(),
        compiler_subject_identity: "compiler".into(),
        invocation_identity: "invocation".into(),
        capability_identity: "capability".into(),
        environment_identity: "environment".into(),
        source_report_digest: "digest".into(),
        accepted_transition_id: Some("transition".into()),
        evidence_bundle: Some("bundle".into()),
        mode: HarnessMode::Compile,
        profile: HarnessProfile::Base,
        runner: HarnessRunner::Test,
        file_membership: sample_results(total, passed)
            .iter()
            .map(|result| result.path.clone())
            .collect(),
        files_total: total,
        files_passed: passed,
        files_failed: total - passed,
        tap_assertions_total: total,
        tap_assertions_passed: passed,
        buckets: BTreeMap::new(),
        expected_failures: Vec::new(),
        file_results: sample_results(total, passed),
        semantic_boundaries: Vec::new(),
        boundary_retirements: Vec::new(),
    }
}
