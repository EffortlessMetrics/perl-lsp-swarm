//! Accessor coverage for transition accepted-baseline model types (#5171).

use perl_core_harness::transition::{AcceptedBaseline, TransitionRunState};
use perl_core_harness_types::{
    COMPILE_BASELINE_SCHEMA_VERSION, COMPILE_BASELINE_V2_SCHEMA_VERSION, CompileBaseline,
    CompileBaselineV2, HarnessMode, HarnessProfile, HarnessRunner, RUN_REPORT_SCHEMA_VERSION,
    RunFileResult, RunnerStatus,
};
use std::collections::BTreeMap;

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

fn sample_v1(total: usize, passed: usize) -> AcceptedBaseline {
    AcceptedBaseline::V1(CompileBaseline {
        schema_version: COMPILE_BASELINE_SCHEMA_VERSION.into(),
        report_schema_version: RUN_REPORT_SCHEMA_VERSION.into(),
        mode: HarnessMode::Compile,
        profile: HarnessProfile::Base,
        files_total: total,
        files_passed: passed,
        files_failed: total - passed,
        tap_assertions_total: total,
        tap_assertions_passed: passed,
        buckets: BTreeMap::from([("parse_recovery".into(), 1)]),
        expected_failures: Vec::new(),
        file_results: sample_results(total, passed),
        semantic_boundaries: Some(Vec::new()),
    })
}

fn sample_v2(total: usize, passed: usize) -> AcceptedBaseline {
    AcceptedBaseline::V2(Box::new(CompileBaselineV2 {
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
        buckets: BTreeMap::from([("parse_recovery".into(), 1)]),
        expected_failures: Vec::new(),
        file_results: sample_results(total, passed),
        semantic_boundaries: Vec::new(),
        boundary_retirements: Vec::new(),
    }))
}

#[test]
fn v1_accessors_surface_file_results_failures_buckets_and_state() {
    let accepted = sample_v1(2, 1);
    assert_eq!(accepted.file_results().len(), 2);
    assert!(accepted.failures().is_empty());
    assert_eq!(accepted.buckets().get("parse_recovery"), Some(&1));
    assert_eq!(
        accepted.state(),
        TransitionRunState {
            files_total: 2,
            files_passed: 1,
            files_failed: 1,
            tap_assertions_total: 2,
            tap_assertions_passed: 1,
        }
    );
    assert_eq!(
        accepted.semantic_boundaries(),
        Some(&[] as &[perl_core_harness_types::ObservedSemanticBoundary])
    );
}

#[test]
fn v2_accessors_surface_file_results_failures_buckets_and_state() {
    let accepted = sample_v2(2, 2);
    assert_eq!(accepted.file_results().len(), 2);
    assert!(accepted.failures().is_empty());
    assert_eq!(accepted.buckets().get("parse_recovery"), Some(&1));
    assert_eq!(
        accepted.state(),
        TransitionRunState {
            files_total: 2,
            files_passed: 2,
            files_failed: 0,
            tap_assertions_total: 2,
            tap_assertions_passed: 2,
        }
    );
    assert_eq!(
        accepted.semantic_boundaries(),
        Some(&[] as &[perl_core_harness_types::ObservedSemanticBoundary])
    );
}
