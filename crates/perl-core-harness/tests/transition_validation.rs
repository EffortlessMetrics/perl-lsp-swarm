//! Canonical evidence-validation controls for transition classification.

use perl_core_harness::transition::{
    AcceptedBaseline, EvidenceValidationKind, classify_transition, validate_accepted_baseline,
    validate_run_report,
};
use perl_core_harness_types::{
    COMPILE_BASELINE_V2_SCHEMA_VERSION, CompatibilityTransition, CompileBaselineV2, HarnessMode,
    HarnessProfile, HarnessRunner, RUN_REPORT_SCHEMA_VERSION, RunFileResult, RunReport, RunSummary,
    RunnerStatus,
};
use std::collections::BTreeMap;

#[test]
fn accepted_per_file_assertion_overflow_is_rejected() {
    let mut baseline = sample_v2_baseline(1, 1);
    baseline.file_results[0].assertions_passed = 2;
    baseline.tap_assertions_passed = 2;
    let accepted = AcceptedBaseline::V2(Box::new(baseline));

    let error = validate_accepted_baseline(&accepted).err();
    assert_eq!(
        error.as_ref().map(|value| value.kind),
        Some(EvidenceValidationKind::AssertionBounds)
    );
}

#[test]
fn accepted_aggregate_tap_mismatch_is_rejected() {
    let mut baseline = sample_v2_baseline(2, 2);
    baseline.tap_assertions_total = 99;
    let accepted = AcceptedBaseline::V2(Box::new(baseline));

    let error = validate_accepted_baseline(&accepted).err();
    assert_eq!(
        error.as_ref().map(|value| value.kind),
        Some(EvidenceValidationKind::AssertionTotalMismatch)
    );
}

#[test]
fn fabricated_current_tap_totals_block_pass_to_fail_regression() {
    let accepted = sample_v2_baseline(2, 2);
    let mut current = sample_report(2, 2);
    current.file_results[0].status = RunnerStatus::Fail;
    current.file_results[0].assertions_passed = 0;
    current.summary.files_passed = 1;
    current.summary.files_failed = 1;
    current.summary.tap_assertions_passed = 1;
    current.summary.tap_assertions_total = 99;

    let classification =
        classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current);
    assert_eq!(classification.transition, CompatibilityTransition::NotProven);
    assert!(classification.reason.contains("aggregate TAP assertions"));
}

#[test]
fn nonterminal_current_run_is_not_proven() {
    let accepted = sample_v2_baseline(1, 1);
    let mut current = sample_report(1, 1);
    current.harness_status = None;

    let validation = validate_run_report(&current).err();
    assert_eq!(
        validation.as_ref().map(|value| value.kind),
        Some(EvidenceValidationKind::IncompleteRun)
    );
    let classification =
        classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current);
    assert_eq!(classification.transition, CompatibilityTransition::NotProven);
}

#[test]
fn valid_complete_pass_to_fail_is_regression() {
    let accepted = sample_v2_baseline(2, 2);
    let mut current = sample_report(2, 2);
    current.file_results[0].status = RunnerStatus::Fail;
    current.file_results[0].assertions_passed = 0;
    current.summary.files_passed = 1;
    current.summary.files_failed = 1;
    current.summary.tap_assertions_passed = 1;

    let classification =
        classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current);
    assert_eq!(classification.transition, CompatibilityTransition::Regression);
    assert!(classification.reason.contains("changed from pass to fail"));
}

#[test]
fn valid_exact_match_remains_no_change() {
    let accepted = sample_v2_baseline(2, 2);
    let current = sample_report(2, 2);

    let classification =
        classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current);
    assert_eq!(classification.transition, CompatibilityTransition::NoChange);
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
                assertions_passed: usize::from(status == RunnerStatus::Pass),
                assertions_total: 1,
            }
        })
        .collect()
}

fn sample_v2_baseline(total: usize, passed: usize) -> CompileBaselineV2 {
    let file_results = sample_results(total, passed);
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
        file_membership: file_results.iter().map(|result| result.path.clone()).collect(),
        files_total: total,
        files_passed: passed,
        files_failed: total - passed,
        tap_assertions_total: total,
        tap_assertions_passed: passed,
        buckets: BTreeMap::new(),
        expected_failures: Vec::new(),
        file_results,
        semantic_boundaries: Vec::new(),
        boundary_retirements: Vec::new(),
    }
}
