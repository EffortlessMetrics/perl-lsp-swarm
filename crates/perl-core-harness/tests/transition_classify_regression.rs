//! Discriminating regression matrix for validated compiler transition evidence.

use perl_core_harness::transition::{
    AcceptedBaseline, Classification, COMPILER_COMPARISON_CONTEXT_SCHEMA_VERSION,
    CompilerComparisonContext, classify_transition, classify_transition_with_context,
    run_report_digest,
};
use perl_core_harness_types::{
    COMPILE_BASELINE_SCHEMA_VERSION, COMPILE_BASELINE_V2_SCHEMA_VERSION, CompatibilityTransition,
    CompileBaseline, CompileBaselineV2, HarnessMode, HarnessProfile, HarnessRunner,
    RUN_REPORT_SCHEMA_VERSION, RunFailure, RunFileResult, RunReport, RunSummary, RunnerStatus,
};
use std::collections::BTreeMap;

/// RIPR-named observer covering the three settled context-bound outcomes.
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
fn context_free_v2_classification_is_not_proven() {
    let accepted = sample_v2_baseline(2, 2);
    let current = sample_report(2, 2);
    let classification = classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current);
    assert_eq!(classification.transition, CompatibilityTransition::NotProven);
    assert!(classification.reason.contains("comparison context is required"));
}

#[test]
fn compensated_file_swap_is_still_regression() {
    let classification = compensated_swap_classification();
    assert_eq!(classification.transition, CompatibilityTransition::Regression);
    assert!(!classification.requires_candidate);
    assert!(
        classification
            .reason
            .contains("base/0.t changed from pass to fail")
    );
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
    assert!(classification.reason.contains("membership"));
}

#[test]
fn duplicate_current_file_path_is_not_proven() {
    let accepted = sample_v2_baseline(1, 1);
    let mut current = sample_report(1, 1);
    let duplicate = current.file_results[0].clone();
    current.file_results.push(duplicate);
    reconcile_report(&mut current);
    let context = comparison_context(&accepted, &current);
    let classification = classify_transition_with_context(
        &AcceptedBaseline::V2(Box::new(accepted)),
        &current,
        &context,
    );
    assert_eq!(classification.transition, CompatibilityTransition::NotProven);
    assert!(
        classification
            .reason
            .contains("current observation repeats file-result path")
    );
}

#[test]
fn duplicate_accepted_file_path_is_not_proven() {
    let mut accepted = sample_v2_baseline(1, 1);
    let duplicate = accepted.file_results[0].clone();
    accepted.file_results.push(duplicate);
    let current = sample_report(1, 1);
    let context = comparison_context(&accepted, &current);
    let classification = classify_transition_with_context(
        &AcceptedBaseline::V2(Box::new(accepted)),
        &current,
        &context,
    );
    assert_eq!(classification.transition, CompatibilityTransition::NotProven);
    assert!(classification.reason.contains("accepted observation repeats"));
}

#[test]
fn bucket_inventory_drift_is_not_no_change() {
    let mut accepted = sample_v2_baseline(2, 2);
    accepted.buckets.insert("parse_recovery".into(), 1);
    let current = sample_report(2, 2);
    let context = comparison_context(&accepted, &current);
    let classification = classify_transition_with_context(
        &AcceptedBaseline::V2(Box::new(accepted)),
        &current,
        &context,
    );
    assert_eq!(classification.transition, CompatibilityTransition::NotProven);
    assert!(classification.reason.contains("failure buckets"));
}

#[test]
fn duplicate_file_membership_is_not_proven() {
    let mut accepted = sample_v2_baseline(1, 1);
    accepted
        .file_membership
        .push(accepted.file_membership[0].clone());
    let current = sample_report(1, 1);
    let context = comparison_context(&accepted, &current);
    let classification = classify_transition_with_context(
        &AcceptedBaseline::V2(Box::new(accepted)),
        &current,
        &context,
    );
    assert_eq!(classification.transition, CompatibilityTransition::NotProven);
    assert!(classification.reason.contains("file_membership repeats"));
}

#[test]
fn failed_harness_status_is_not_proven() {
    let accepted = sample_v2_baseline(2, 1);
    let mut current = sample_report(2, 1);
    current.harness_status = Some(1);
    current.file_results[0].status = RunnerStatus::Fail;
    current.file_results[0].assertions_passed = 0;
    let context = comparison_context(&accepted, &current);
    let classification = classify_transition_with_context(
        &AcceptedBaseline::V2(Box::new(accepted)),
        &current,
        &context,
    );
    assert_eq!(classification.transition, CompatibilityTransition::NotProven);
    assert!(classification.reason.contains("harness_status"));
}

#[test]
fn forged_summary_blocks_regression_and_no_change() {
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
fn accepted_membership_mismatch_is_not_proven() {
    let mut accepted = sample_v2_baseline(2, 2);
    accepted.file_membership.pop();
    let current = sample_report(2, 2);
    let context = comparison_context(&accepted, &current);
    let classification = classify_transition_with_context(
        &AcceptedBaseline::V2(Box::new(accepted)),
        &current,
        &context,
    );
    assert_eq!(classification.transition, CompatibilityTransition::NotProven);
    assert!(
        classification
            .reason
            .contains("file_results do not match immutable file_membership")
    );
}

#[test]
fn failed_row_without_owned_failure_is_not_proven() {
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
fn fabricated_bucket_cardinality_is_not_proven() {
    let accepted = sample_v2_baseline(2, 2);
    let mut current = sample_report(2, 1);
    current.buckets.insert("failure_1".into(), 4);
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
fn changed_invocation_context_is_not_proven() {
    let accepted = sample_v2_baseline(2, 2);
    let current = sample_report(2, 2);
    let mut context = comparison_context(&accepted, &current);
    context.invocation_identity = "other-invocation".into();
    let classification = classify_transition_with_context(
        &AcceptedBaseline::V2(Box::new(accepted)),
        &current,
        &context,
    );
    assert_eq!(classification.transition, CompatibilityTransition::NotProven);
    assert!(classification.reason.contains("accepted V2 ratchet does not match"));
}

#[test]
fn current_report_digest_mismatch_is_not_proven() {
    let accepted = sample_v2_baseline(2, 2);
    let current = sample_report(2, 2);
    let mut context = comparison_context(&accepted, &current);
    context.current_source_report_digest = format!("sha256:{}", "0".repeat(64));
    let classification = classify_transition_with_context(
        &AcceptedBaseline::V2(Box::new(accepted)),
        &current,
        &context,
    );
    assert_eq!(classification.transition, CompatibilityTransition::NotProven);
    assert!(classification.reason.contains("report digest"));
}

#[test]
fn v1_same_path_different_mode_cannot_emit_regression() {
    let accepted = sample_v1_baseline(1, 1);
    let mut current = sample_report(1, 0);
    current.mode = HarnessMode::Parse;
    reconcile_report(&mut current);
    let context = context_for_current_only(&current);
    let classification = classify_transition_with_context(
        &AcceptedBaseline::V1(Box::new(accepted)),
        &current,
        &context,
    );
    assert_eq!(classification.transition, CompatibilityTransition::NotProven);
    assert!(classification.reason.contains("V1 baselines lack"));
}

fn compensated_swap_classification() -> Classification {
    let accepted = sample_v2_baseline(2, 1);
    let mut current = sample_report(2, 1);
    current.file_results[0].status = RunnerStatus::Fail;
    current.file_results[0].assertions_passed = 0;
    current.file_results[1].status = RunnerStatus::Pass;
    current.file_results[1].assertions_passed = 1;
    reconcile_report(&mut current);
    let context = comparison_context(&accepted, &current);
    classify_transition_with_context(
        &AcceptedBaseline::V2(Box::new(accepted)),
        &current,
        &context,
    )
}

fn exact_match_classification() -> Classification {
    let accepted = sample_v2_baseline(2, 2);
    let current = sample_report(2, 2);
    let context = comparison_context(&accepted, &current);
    classify_transition_with_context(
        &AcceptedBaseline::V2(Box::new(accepted)),
        &current,
        &context,
    )
}

fn unexpected_file_classification() -> Classification {
    let accepted = sample_v2_baseline(1, 1);
    let mut current = sample_report(1, 1);
    let mut extra = current.file_results[0].clone();
    extra.path = "unexpected/extra.t".into();
    current.file_results.push(extra);
    reconcile_report(&mut current);
    let context = comparison_context(&accepted, &current);
    classify_transition_with_context(
        &AcceptedBaseline::V2(Box::new(accepted)),
        &current,
        &context,
    )
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
    let token = path.replace(['/', '.'], "_");
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

fn sample_v1_baseline(total: usize, passed: usize) -> CompileBaseline {
    let file_results = sample_results(total, passed);
    let (expected_failures, buckets) = failure_inventory(&file_results);
    CompileBaseline {
        schema_version: COMPILE_BASELINE_SCHEMA_VERSION.into(),
        report_schema_version: RUN_REPORT_SCHEMA_VERSION.into(),
        mode: HarnessMode::Compile,
        profile: HarnessProfile::Base,
        files_total: total,
        files_passed: passed,
        files_failed: total - passed,
        tap_assertions_total: total,
        tap_assertions_passed: passed,
        buckets,
        expected_failures,
        file_results,
        semantic_boundaries: Some(Vec::new()),
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

fn context_for_current_only(current: &RunReport) -> CompilerComparisonContext {
    CompilerComparisonContext {
        schema_version: COMPILER_COMPARISON_CONTEXT_SCHEMA_VERSION.into(),
        series_id: "series".into(),
        manifest_hash: "manifest".into(),
        accepted_repository_commit: "a".repeat(40),
        accepted_compiler_subject_identity: "accepted-compiler".into(),
        accepted_source_report_digest: "sha256:accepted".into(),
        current_repository_commit: current.commit.clone(),
        current_compiler_subject_identity: format!("compiler@{}", current.commit),
        current_source_report_digest: run_report_digest(current)
            .unwrap_or_else(|error| format!("invalid:{error}")),
        perl_resolved_ref: current.perl_ref.clone(),
        preparation_receipt_id: "prepare".into(),
        invocation_identity: "invocation".into(),
        capability_identity: "capability".into(),
        environment_identity: "environment".into(),
        mode: current.mode,
        profile: current.profile,
        runner: current.runner,
        file_membership: current
            .file_results
            .iter()
            .map(|result| result.path.clone())
            .collect(),
    }
}
