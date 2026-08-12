//! Canonical evidence-validation controls for transition classification.

use perl_core_harness::transition::{
    AcceptedBaseline, COMPILER_COMPARISON_CONTEXT_SCHEMA_VERSION, CompilerComparisonContext,
    EvidenceValidationKind, classify_transition_with_context, run_report_digest,
    validate_accepted_baseline, validate_run_report,
};
use perl_core_harness_types::{
    COMPILE_BASELINE_V2_SCHEMA_VERSION, CompatibilityTransition, CompileBaselineV2, HarnessMode,
    HarnessProfile, HarnessRunner, RUN_REPORT_SCHEMA_VERSION, RunFailure, RunFileResult, RunReport,
    RunSummary, RunnerStatus,
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
    reconcile_report(&mut current);
    current.summary.tap_assertions_total = 99;
    let context = comparison_context(&accepted, &current);

    let classification = classify_transition_with_context(
        &AcceptedBaseline::V2(Box::new(accepted)),
        &current,
        &context,
    );
    assert_eq!(classification.transition, CompatibilityTransition::NotProven);
    assert!(classification.reason.contains("aggregate TAP assertions"));
}

#[test]
fn nonterminal_current_run_is_not_proven() {
    let accepted = sample_v2_baseline(1, 1);
    let mut current = sample_report(1, 1);
    current.harness_status = None;
    let context = comparison_context(&accepted, &current);

    let validation = validate_run_report(&current, &context).err();
    assert_eq!(
        validation.as_ref().map(|value| value.kind),
        Some(EvidenceValidationKind::IncompleteRun)
    );
    let classification = classify_transition_with_context(
        &AcceptedBaseline::V2(Box::new(accepted)),
        &current,
        &context,
    );
    assert_eq!(classification.transition, CompatibilityTransition::NotProven);
}

#[test]
fn empty_failure_inventory_blocks_valid_looking_regression() {
    let accepted = sample_v2_baseline(2, 2);
    let mut current = sample_report(2, 1);
    current.failures.clear();
    current.buckets.clear();
    let context = comparison_context(&accepted, &current);

    let classification = classify_transition_with_context(
        &AcceptedBaseline::V2(Box::new(accepted)),
        &current,
        &context,
    );
    assert_eq!(classification.transition, CompatibilityTransition::NotProven);
    assert!(classification.reason.contains("owned failure row"));
}

#[test]
fn wrong_bucket_count_blocks_valid_looking_regression() {
    let accepted = sample_v2_baseline(2, 2);
    let mut current = sample_report(2, 1);
    current.buckets.insert("failure_base_1_t".into(), 3);
    let context = comparison_context(&accepted, &current);

    let classification = classify_transition_with_context(
        &AcceptedBaseline::V2(Box::new(accepted)),
        &current,
        &context,
    );
    assert_eq!(classification.transition, CompatibilityTransition::NotProven);
    assert!(classification.reason.contains("cardinalities"));
}

#[test]
fn valid_complete_pass_to_fail_is_regression() {
    let accepted = sample_v2_baseline(2, 2);
    let current = sample_report(2, 1);
    let context = comparison_context(&accepted, &current);

    let classification = classify_transition_with_context(
        &AcceptedBaseline::V2(Box::new(accepted)),
        &current,
        &context,
    );
    assert_eq!(classification.transition, CompatibilityTransition::Regression);
    assert!(classification.reason.contains("changed from pass to fail"));
}

#[test]
fn valid_exact_match_remains_no_change() {
    let accepted = sample_v2_baseline(2, 2);
    let current = sample_report(2, 2);
    let context = comparison_context(&accepted, &current);

    let classification = classify_transition_with_context(
        &AcceptedBaseline::V2(Box::new(accepted)),
        &current,
        &context,
    );
    assert_eq!(classification.transition, CompatibilityTransition::NoChange);
}

fn sample_report(total: usize, passed: usize) -> RunReport {
    let mut report = RunReport {
        schema_version: RUN_REPORT_SCHEMA_VERSION.into(),
        commit: "b".repeat(40),
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
            files_total: 0,
            files_passed: 0,
            files_failed: 0,
            tap_assertions_total: 0,
            tap_assertions_passed: 0,
        },
        buckets: BTreeMap::new(),
        file_results: sample_results(total, passed),
        failures: Vec::new(),
        semantic_boundaries: Vec::new(),
    };
    reconcile_report(&mut report);
    report
}

fn sample_results(total: usize, passed: usize) -> Vec<RunFileResult> {
    (0..total)
        .map(|index| {
            let status = if index < passed {
                RunnerStatus::Pass
            } else {
                RunnerStatus::Fail
            };
            RunFileResult {
                path: format!("base/{index}.t"),
                status,
                assertions_passed: usize::from(status == RunnerStatus::Pass),
                assertions_total: 1,
            }
        })
        .collect()
}

fn failure_for(path: &str) -> RunFailure {
    let token = path
        .chars()
        .map(|character| if matches!(character, '/' | '.') { '_' } else { character })
        .collect::<String>();
    RunFailure {
        path: path.into(),
        phase: "compile".into(),
        bucket: format!("failure_{token}"),
        first_diagnostic: format!("failure in {path}"),
        workstream: "compiler".into(),
        lsp_impact: Vec::new(),
    }
}

fn failure_inventory(
    results: &[RunFileResult],
) -> (Vec<RunFailure>, BTreeMap<String, usize>) {
    let failures = results
        .iter()
        .filter(|result| result.status == RunnerStatus::Fail)
        .map(|result| failure_for(&result.path))
        .collect::<Vec<_>>();
    let mut buckets = BTreeMap::new();
    for failure in &failures {
        *buckets.entry(failure.bucket.clone()).or_default() += 1;
    }
    (failures, buckets)
}

fn reconcile_report(report: &mut RunReport) {
    let files_total = report.file_results.len();
    let files_passed = report
        .file_results
        .iter()
        .filter(|result| result.status == RunnerStatus::Pass)
        .count();
    report.summary = RunSummary {
        files_total,
        files_passed,
        files_failed: files_total.saturating_sub(files_passed),
        tap_assertions_total: report
            .file_results
            .iter()
            .map(|result| result.assertions_total)
            .sum(),
        tap_assertions_passed: report
            .file_results
            .iter()
            .map(|result| result.assertions_passed)
            .sum(),
    };
    let (failures, buckets) = failure_inventory(&report.file_results);
    report.failures = failures;
    report.buckets = buckets;
}

fn sample_v2_baseline(total: usize, passed: usize) -> CompileBaselineV2 {
    let file_results = sample_results(total, passed);
    let (expected_failures, buckets) = failure_inventory(&file_results);
    CompileBaselineV2 {
        schema_version: COMPILE_BASELINE_V2_SCHEMA_VERSION.into(),
        report_schema_version: RUN_REPORT_SCHEMA_VERSION.into(),
        series_id: "series".into(),
        manifest_hash: "manifest".into(),
        repository_commit: "a".repeat(40),
        perl_resolved_ref: "perl".into(),
        preparation_receipt_id: "prepare".into(),
        compiler_subject_identity: "accepted-compiler".into(),
        invocation_identity: "invocation".into(),
        capability_identity: "capability".into(),
        environment_identity: "environment".into(),
        source_report_digest: "sha256:accepted".into(),
        accepted_transition_id: Some("transition".into()),
        evidence_bundle: Some("bundle".into()),
        mode: HarnessMode::Compile,
        profile: HarnessProfile::Base,
        runner: HarnessRunner::Test,
        file_membership: file_results
            .iter()
            .map(|result| result.path.clone())
            .collect(),
        files_total: total,
        files_passed: passed,
        files_failed: total - passed,
        tap_assertions_total: total,
        tap_assertions_passed: passed,
        buckets,
        expected_failures,
        file_results,
        semantic_boundaries: Vec::new(),
        boundary_retirements: Vec::new(),
    }
}

fn comparison_context(
    accepted: &CompileBaselineV2,
    current: &RunReport,
) -> CompilerComparisonContext {
    CompilerComparisonContext {
        schema_version: COMPILER_COMPARISON_CONTEXT_SCHEMA_VERSION.into(),
        series_id: accepted.series_id.clone(),
        manifest_hash: accepted.manifest_hash.clone(),
        accepted_repository_commit: accepted.repository_commit.clone(),
        accepted_compiler_subject_identity: accepted.compiler_subject_identity.clone(),
        accepted_source_report_digest: accepted.source_report_digest.clone(),
        current_repository_commit: current.commit.clone(),
        current_compiler_subject_identity: format!("compiler@{}", current.commit),
        current_source_report_digest: run_report_digest(current)
            .unwrap_or_else(|error| format!("invalid:{error}")),
        perl_resolved_ref: accepted.perl_resolved_ref.clone(),
        preparation_receipt_id: accepted.preparation_receipt_id.clone(),
        invocation_identity: accepted.invocation_identity.clone(),
        capability_identity: accepted.capability_identity.clone(),
        environment_identity: accepted.environment_identity.clone(),
        mode: accepted.mode,
        profile: accepted.profile,
        runner: accepted.runner,
        file_membership: accepted.file_membership.clone(),
    }
}
