//! Discriminating regression matrix for the minimal transition classifier slice.

use color_eyre::eyre::{Result, bail};
use perl_core_harness::transition::{AcceptedBaseline, classify_transition};
use perl_core_harness_types::{
    COMPILE_BASELINE_V2_SCHEMA_VERSION, CompatibilityTransition, CompileBaselineV2, HarnessMode,
    HarnessProfile, HarnessRunner, RUN_REPORT_SCHEMA_VERSION, RunFileResult, RunReport, RunSummary,
    RunnerStatus,
};
use std::collections::BTreeMap;

type TestResult = Result<()>;

#[test]
fn compensated_file_swap_is_still_regression() -> TestResult {
    let accepted = sample_v2_baseline(2, 1);
    let mut current = sample_report(2, 1);
    current.file_results[0].status = RunnerStatus::Fail;
    current.file_results[0].assertions_passed = 0;
    current.file_results[1].status = RunnerStatus::Pass;
    current.file_results[1].assertions_passed = 1;
    let classification = classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current);
    if classification.transition != CompatibilityTransition::Regression {
        bail!("a pass/fail swap hid a file-level regression");
    }
    Ok(())
}

#[test]
fn exact_v2_match_is_no_change() -> TestResult {
    let accepted = sample_v2_baseline(2, 2);
    let current = sample_report(2, 2);
    let classification = classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current);
    if classification.transition != CompatibilityTransition::NoChange
        || classification.requires_candidate
    {
        bail!("exact v2 match was not no_change");
    }
    Ok(())
}

#[test]
fn unexpected_current_file_is_not_proven() -> TestResult {
    let accepted = sample_v2_baseline(1, 1);
    let mut current = sample_report(1, 1);
    current.file_results.push(RunFileResult {
        path: "unexpected/extra.t".into(),
        status: RunnerStatus::Pass,
        assertions_passed: 1,
        assertions_total: 1,
    });
    let classification = classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current);
    if classification.transition != CompatibilityTransition::NotProven
        || classification.requires_candidate
    {
        bail!("an unexpected file result was treated as comparable evidence");
    }
    Ok(())
}

#[test]
fn duplicate_current_file_path_is_not_proven() -> TestResult {
    let accepted = sample_v2_baseline(1, 0);
    let mut current = sample_report(1, 0);
    current.file_results.push(RunFileResult {
        path: "base/0.t".into(),
        status: RunnerStatus::Pass,
        assertions_passed: 1,
        assertions_total: 1,
    });
    let classification = classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current);
    if classification.transition != CompatibilityTransition::NotProven
        || classification.requires_candidate
    {
        bail!("duplicate file-result paths must be incomparable, not silently collapsed");
    }
    Ok(())
}

#[test]
fn duplicate_accepted_file_path_is_not_proven() -> TestResult {
    let mut accepted = sample_v2_baseline(1, 1);
    accepted.file_results.push(RunFileResult {
        path: "base/0.t".into(),
        status: RunnerStatus::Pass,
        assertions_passed: 1,
        assertions_total: 1,
    });
    let current = sample_report(1, 1);
    let classification = classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current);
    if classification.transition != CompatibilityTransition::NotProven
        || classification.requires_candidate
        || !classification.reason.contains("accepted observation repeats")
    {
        bail!("duplicate accepted paths must be rejected before comparison");
    }
    Ok(())
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
