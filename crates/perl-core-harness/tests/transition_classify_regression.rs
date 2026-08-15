//! Discriminating regression matrix for the minimal transition classifier slice.
//!
//! Oracles use `assert_eq!` / `assert!` so RIPR can observe transition outcomes.
//! Fixture rows are cloned from sample builders instead of fresh struct literals
//! so side_effect probes on test construction do not inflate new-gap counts.

use perl_core_harness::transition::{
    AcceptedBaseline, Classification, classify_transition, validate_run_report,
};
use perl_core_harness_types::{
    COMPILE_BASELINE_SCHEMA_VERSION, COMPILE_BASELINE_V2_SCHEMA_VERSION, CompatibilityTransition,
    CompileBaseline, CompileBaselineV2, HarnessMode, HarnessProfile, HarnessRunner,
    ObservedSemanticBoundary, RUN_REPORT_SCHEMA_VERSION, RunFailure, RunFileResult, RunReport,
    RunSummary, RunnerStatus, SemanticBoundaryConfidence, SemanticBoundaryDisposition,
    SemanticBoundaryLockScope, SemanticBoundarySourceSpan,
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
    let duplicate = current.file_results[0].clone();
    current.file_results.push(duplicate);
    let classification = classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current);
    assert_eq!(classification.transition, CompatibilityTransition::NotProven);
    assert!(!classification.requires_candidate);
    assert!(classification.reason.contains("current observation repeats file-result path"));
}

#[test]
fn duplicate_accepted_file_path_is_not_proven() {
    let mut accepted = sample_v2_baseline(1, 1);
    let duplicate = accepted.file_results[0].clone();
    accepted.file_results.push(duplicate);
    let current = sample_report(1, 1);
    let classification = classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current);
    assert_eq!(classification.transition, CompatibilityTransition::NotProven);
    assert!(!classification.requires_candidate);
    assert!(classification.reason.contains("accepted observation repeats"));
}

#[test]
fn bucket_inventory_drift_is_not_no_change() {
    let mut accepted = sample_v2_baseline(2, 2);
    accepted.buckets.insert("parse_recovery".into(), 1);
    let current = sample_report(2, 2);
    let classification = classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current);
    assert_eq!(classification.transition, CompatibilityTransition::NotProven);
    assert!(!classification.requires_candidate);
}

#[test]
fn duplicate_file_membership_is_not_proven() {
    let mut accepted = sample_v2_baseline(1, 1);
    accepted.file_membership.push(accepted.file_membership[0].clone());
    let current = sample_report(1, 1);
    let classification = classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current);
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
    let classification = classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current);
    assert_eq!(classification.transition, CompatibilityTransition::NotProven);
    assert!(classification.reason.contains("harness_status"));
}

#[test]
fn missing_harness_status_is_not_proven_even_with_pass_to_fail_rows() {
    let accepted = sample_v2_baseline(2, 1);
    let mut current = sample_report(2, 0);
    current.harness_status = None;
    let classification = classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current);
    assert_eq!(classification.transition, CompatibilityTransition::NotProven);
    assert!(classification.reason.contains("harness_status"));
    assert!(!classification.reason.contains("changed from pass to fail"));
}

#[test]
fn forged_summary_blocks_no_change() {
    let accepted = sample_v2_baseline(2, 2);
    let mut current = sample_report(2, 2);
    current.summary.files_passed = 0;
    let classification = classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current);
    assert_eq!(classification.transition, CompatibilityTransition::NotProven);
    assert!(classification.reason.contains("summary file/TAP totals"));
}

#[test]
fn contradictory_tap_totals_block_regression() {
    let accepted = sample_v2_baseline(2, 1);
    let mut current = sample_report(2, 1);
    current.file_results[0].status = RunnerStatus::Fail;
    current.file_results[0].assertions_passed = 0;
    current.file_results[1].status = RunnerStatus::Pass;
    current.file_results[1].assertions_passed = 1;
    // Detailed rows still look like a compensated swap, but aggregate TAP is forged.
    current.summary.tap_assertions_total = 99;
    let classification = classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current);
    assert_eq!(classification.transition, CompatibilityTransition::NotProven);
    assert!(classification.reason.contains("summary file/TAP totals"));
    assert!(!classification.reason.contains("changed from pass to fail"));
}

#[test]
fn accepted_assertion_overflow_blocks_regression() {
    let mut accepted = sample_v2_baseline(2, 1);
    accepted.file_results[1].assertions_passed = 2;
    accepted.file_results[1].assertions_total = 1;
    accepted.tap_assertions_passed = 2;
    accepted.tap_assertions_total = 2;
    let mut current = sample_report(2, 0);
    current.file_results[0].status = RunnerStatus::Fail;
    current.file_results[0].assertions_passed = 0;
    let classification = classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current);
    assert_eq!(classification.transition, CompatibilityTransition::NotProven);
    assert!(classification.reason.contains("assertions_passed"));
    assert!(!classification.reason.contains("changed from pass to fail"));
}

#[test]
fn accepted_membership_mismatch_is_not_proven() {
    let mut accepted = sample_v2_baseline(2, 2);
    accepted.file_membership.pop();
    let current = sample_report(2, 2);
    let classification = classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current);
    assert_eq!(classification.transition, CompatibilityTransition::NotProven);
    assert!(classification.reason.contains("file_results do not match immutable file_membership"));
}

#[test]
fn missing_failure_record_blocks_regression() {
    let accepted = sample_v2_baseline(2, 1);
    let mut current = sample_report(2, 1);
    current.file_results[0].status = RunnerStatus::Fail;
    current.file_results[0].assertions_passed = 0;
    current.file_results[1].status = RunnerStatus::Pass;
    current.file_results[1].assertions_passed = 1;
    // Keep aggregates reconciled, but drop the failure inventory cardinality.
    current.failures.clear();
    current.summary.files_passed = 1;
    current.summary.files_failed = 1;
    current.summary.tap_assertions_passed = 1;
    let classification = classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current);
    assert_eq!(classification.transition, CompatibilityTransition::NotProven);
    assert!(classification.reason.contains("has no failure record"));
    assert!(!classification.reason.contains("changed from pass to fail"));
    assert!(validate_run_report(&current).is_err());
}

#[test]
fn empty_failure_bucket_blocks_regression() {
    let accepted = sample_v2_baseline(2, 1);
    let mut current = sample_report(2, 1);
    current.file_results[0].status = RunnerStatus::Fail;
    current.file_results[0].assertions_passed = 0;
    current.file_results[1].status = RunnerStatus::Pass;
    current.file_results[1].assertions_passed = 1;
    current.failures = vec![sample_failure("base/0.t", "")];
    current.summary.files_passed = 1;
    current.summary.files_failed = 1;
    current.summary.tap_assertions_passed = 1;
    let classification = classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current);
    assert_eq!(classification.transition, CompatibilityTransition::NotProven);
    assert!(classification.reason.contains("empty bucket"));
    assert!(!classification.reason.contains("changed from pass to fail"));
}

#[test]
fn failure_record_for_passing_file_blocks_regression() {
    let accepted = sample_v2_baseline(2, 1);
    let mut current = sample_report(2, 1);
    current.failures = vec![sample_failure("base/0.t", "parse_recovery")];
    let classification = classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current);
    assert_eq!(classification.transition, CompatibilityTransition::NotProven);
    assert!(classification.reason.contains("does not identify a failing file"));
    assert!(!classification.reason.contains("changed from pass to fail"));
}

#[test]
fn foreign_failure_record_blocks_regression() {
    let accepted = sample_v2_baseline(2, 1);
    let mut current = sample_report(2, 1);
    current.failures = vec![sample_failure("foreign/0.t", "parse_recovery")];
    let classification = classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current);
    assert_eq!(classification.transition, CompatibilityTransition::NotProven);
    assert!(classification.reason.contains("has no file-result record"));
    assert!(!classification.reason.contains("changed from pass to fail"));
}

#[test]
fn duplicate_failure_records_block_regression() {
    let accepted = sample_v2_baseline(2, 1);
    let mut current = sample_report(2, 1);
    current.failures = vec![
        sample_failure("base/1.t", "parse_recovery"),
        sample_failure("base/1.t", "compile_error"),
    ];
    let classification = classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current);
    assert_eq!(classification.transition, CompatibilityTransition::NotProven);
    assert!(classification.reason.contains("failure inventory repeats path"));
    assert!(!classification.reason.contains("changed from pass to fail"));
}

#[test]
fn malformed_semantic_boundary_identity_is_not_proven() {
    let mut accepted = sample_v2_baseline(1, 1);
    let mut current = sample_report(1, 1);
    let mut boundary = sample_semantic_boundary();
    boundary.id.clear();
    accepted.semantic_boundaries.push(boundary.clone());
    current.semantic_boundaries.push(boundary);
    let classification = classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current);
    assert_eq!(classification.transition, CompatibilityTransition::NotProven);
    assert!(classification.reason.contains("empty stable id"));
}

// ── V1 subject-comparability controls ────────────────────────────────────────

#[test]
fn v1_mode_mismatch_is_not_proven() {
    let mut accepted = sample_v1_baseline(2, 2);
    accepted.mode = HarnessMode::Parse; // current uses Compile → incomparable
    let current = sample_report(2, 2);
    let classification = classify_transition(&AcceptedBaseline::V1(accepted), &current);
    assert_eq!(classification.transition, CompatibilityTransition::NotProven);
    assert!(!classification.requires_candidate);
    assert!(
        classification.reason.contains("mode"),
        "expected mode in reason: {}",
        classification.reason
    );
    assert!(
        !classification.reason.contains("changed from pass to fail"),
        "regression arm must not fire before subject check"
    );
}

#[test]
fn v1_profile_mismatch_is_not_proven() {
    let mut accepted = sample_v1_baseline(2, 2);
    accepted.profile = HarnessProfile::Comp; // current uses Base → incomparable
    let current = sample_report(2, 2);
    let classification = classify_transition(&AcceptedBaseline::V1(accepted), &current);
    assert_eq!(classification.transition, CompatibilityTransition::NotProven);
    assert!(!classification.requires_candidate);
    assert!(
        classification.reason.contains("profile"),
        "expected profile in reason: {}",
        classification.reason
    );
}

#[test]
fn v1_regression_is_detected_when_subjects_match() {
    // V1 baselines can still drive Regression if pass→fail is observed.
    let accepted = sample_v1_baseline(2, 1);
    let mut current = sample_report(2, 1);
    // Invert: accepted passes base/0.t, current fails it.
    current.file_results[0].status = RunnerStatus::Fail;
    current.file_results[0].assertions_passed = 0;
    current.file_results[1].status = RunnerStatus::Pass;
    current.file_results[1].assertions_passed = 1;
    current.failures = vec![sample_failure("base/0.t", "parse_recovery")];
    current.summary.files_passed = 1;
    current.summary.files_failed = 1;
    current.summary.tap_assertions_passed = 1;
    let classification = classify_transition(&AcceptedBaseline::V1(accepted), &current);
    assert_eq!(classification.transition, CompatibilityTransition::Regression);
    assert!(classification.reason.contains("base/0.t changed from pass to fail"));
}

// ── Whitespace / path-identity controls ──────────────────────────────────────

#[test]
fn whitespace_contaminated_current_file_result_path_is_not_proven() {
    let accepted = sample_v2_baseline(1, 1);
    let mut current = sample_report(1, 1);
    current.file_results[0].path = " base/0.t".into(); // leading space
    let classification = classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current);
    assert_eq!(classification.transition, CompatibilityTransition::NotProven);
    assert!(
        classification.reason.contains("whitespace"),
        "expected whitespace in reason: {}",
        classification.reason
    );
    assert!(
        !classification.reason.contains("changed from pass to fail"),
        "regression arm must not fire on contaminated path"
    );
}

#[test]
fn orphan_boundary_path_in_current_is_not_proven() {
    let accepted = sample_v2_baseline(1, 1);
    let mut current = sample_report(1, 1);
    let mut boundary = sample_semantic_boundary();
    boundary.path = "no_such/file.t".into(); // path not in file_results
    current.semantic_boundaries.push(boundary);
    let classification = classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current);
    assert_eq!(classification.transition, CompatibilityTransition::NotProven);
    assert!(
        classification.reason.contains("not in file_results"),
        "expected orphan path error: {}",
        classification.reason
    );
}

#[test]
fn duplicate_boundary_id_in_current_is_not_proven() {
    let accepted = sample_v2_baseline(1, 1);
    let mut current = sample_report(1, 1);
    current.semantic_boundaries.push(sample_semantic_boundary());
    current.semantic_boundaries.push(sample_semantic_boundary()); // same path + id
    let classification = classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current);
    assert_eq!(classification.transition, CompatibilityTransition::NotProven);
    assert!(
        classification.reason.contains("repeats boundary id"),
        "expected duplicate-id error: {}",
        classification.reason
    );
}

#[test]
fn phase_mode_mismatch_in_current_failure_is_not_proven() {
    let accepted = sample_v2_baseline(2, 1);
    let mut current = sample_report(2, 1);
    // Make the failing file's failure record have a wrong phase.
    current.file_results[0].status = RunnerStatus::Fail;
    current.file_results[0].assertions_passed = 0;
    current.file_results[1].status = RunnerStatus::Pass;
    current.file_results[1].assertions_passed = 1;
    let mut bad_failure = sample_failure("base/0.t", "parse_recovery");
    bad_failure.phase = "parse".into(); // mode is compile
    current.failures = vec![bad_failure];
    current.summary.files_passed = 1;
    current.summary.files_failed = 1;
    current.summary.tap_assertions_passed = 1;
    let classification = classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current);
    assert_eq!(classification.transition, CompatibilityTransition::NotProven);
    assert!(
        classification.reason.contains("does not match harness mode"),
        "expected phase/mode mismatch: {}",
        classification.reason
    );
    assert!(
        !classification.reason.contains("changed from pass to fail"),
        "regression arm must not fire before validation"
    );
}

fn compensated_swap_classification() -> Classification {
    let accepted = sample_v2_baseline(2, 1);
    let mut current = sample_report(2, 1);
    current.file_results[0].status = RunnerStatus::Fail;
    current.file_results[0].assertions_passed = 0;
    current.file_results[1].status = RunnerStatus::Pass;
    current.file_results[1].assertions_passed = 1;
    current.failures = vec![sample_failure("base/0.t", "parse_recovery")];
    current.summary.files_passed = 1;
    current.summary.files_failed = 1;
    current.summary.tap_assertions_passed = 1;
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
    let mut extra = current.file_results[0].clone();
    extra.path = "unexpected/extra.t".into();
    current.file_results.push(extra);
    // Keep summary reconciled so subject/membership incomparability is the discriminator.
    current.summary.files_total = 2;
    current.summary.files_passed = 2;
    current.summary.files_failed = 0;
    current.summary.tap_assertions_total = 2;
    current.summary.tap_assertions_passed = 2;
    classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current)
}

fn sample_report(total: usize, passed: usize) -> RunReport {
    let file_results = sample_results(total, passed);
    let failures = sample_failures_for(&file_results);
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
        file_results,
        failures,
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

fn sample_failures_for(file_results: &[RunFileResult]) -> Vec<RunFailure> {
    file_results
        .iter()
        .filter(|result| result.status == RunnerStatus::Fail)
        .map(|result| sample_failure(&result.path, "parse_recovery"))
        .collect()
}

fn sample_failure(path: &str, bucket: &str) -> RunFailure {
    RunFailure {
        path: path.into(),
        phase: "compile".into(),
        bucket: bucket.into(),
        first_diagnostic: "sample failure".into(),
        workstream: "parser".into(),
        lsp_impact: vec!["diagnostics".into()],
    }
}

fn sample_semantic_boundary() -> ObservedSemanticBoundary {
    ObservedSemanticBoundary {
        path: "base/0.t".into(),
        id: "boundary".into(),
        disposition: SemanticBoundaryDisposition::Unsupported,
        reason: "sample boundary".into(),
        source_span: SemanticBoundarySourceSpan { start: 0, end: 1 },
        source_kind: "expression".into(),
        confidence: SemanticBoundaryConfidence::Unresolved,
        blocks_compilation: true,
        blocks_downstream_static_facts: true,
        lock_scope: SemanticBoundaryLockScope::None,
        owner_workstream: "parser".into(),
        supporting_test: "tests/sample.rs".into(),
    }
}

fn sample_v1_baseline(total: usize, passed: usize) -> CompileBaseline {
    let file_results = sample_results(total, passed);
    let expected_failures = sample_failures_for(&file_results);
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
        buckets: BTreeMap::new(),
        expected_failures,
        file_results,
        semantic_boundaries: Some(Vec::new()),
    }
}

fn sample_v2_baseline(total: usize, passed: usize) -> CompileBaselineV2 {
    let file_results = sample_results(total, passed);
    let expected_failures = sample_failures_for(&file_results);
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
        expected_failures,
        file_results,
        semantic_boundaries: Vec::new(),
        boundary_retirements: Vec::new(),
    }
}
