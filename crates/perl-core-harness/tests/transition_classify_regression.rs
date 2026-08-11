//! Regression matrix for transition classification (#5171).

use color_eyre::eyre::{Result, bail};
use perl_core_harness::transition::{AcceptedBaseline, classify_transition};
use perl_core_harness_types::{
    COMPILE_BASELINE_SCHEMA_VERSION, COMPILE_BASELINE_V2_SCHEMA_VERSION, CompatibilityTransition,
    CompileBaseline, CompileBaselineV2, HarnessMode, HarnessProfile, HarnessRunner,
    RUN_REPORT_SCHEMA_VERSION, RunFileResult, RunReport, RunSummary, RunnerStatus,
};
use std::collections::BTreeMap;

type TestResult = Result<()>;

#[test]
fn observed_24_of_25_against_accepted_25_is_regression() -> TestResult {
    let mut accepted = sample_v1_baseline(25, 25);
    accepted.file_results = sample_results(25, 25);
    let mut current = sample_report(25, 24);
    current.file_results = sample_results(25, 24);
    let classification = classify_transition(&AcceptedBaseline::V1(accepted), &current)?;
    if classification.transition != CompatibilityTransition::Regression
        || classification.requires_candidate
    {
        bail!("24/25 observation was not preserved as a non-candidate regression");
    }
    Ok(())
}

#[test]
fn compensated_file_swap_is_still_regression() -> TestResult {
    let accepted = sample_v2_baseline(2, 1);
    let mut current = sample_report(2, 1);
    current.file_results[0].status = RunnerStatus::Fail;
    current.file_results[0].assertions_passed = 0;
    current.file_results[1].status = RunnerStatus::Pass;
    current.file_results[1].assertions_passed = 1;
    let classification = classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current)?;
    if classification.transition != CompatibilityTransition::Regression {
        bail!("a pass/fail swap hid a file-level regression");
    }
    Ok(())
}

#[test]
fn higher_score_is_candidate_not_acceptance() -> TestResult {
    let accepted = sample_v2_baseline(2, 1);
    let current = sample_report(2, 2);
    let classification = classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current)?;
    if classification.transition != CompatibilityTransition::ImprovementCandidate
        || !classification.requires_candidate
    {
        bail!("improvement was not classified as a candidate");
    }
    Ok(())
}

#[test]
fn equal_legacy_score_requires_reviewed_migration() -> TestResult {
    let accepted = sample_v1_baseline(2, 2);
    let current = sample_report(2, 2);
    let classification = classify_transition(&AcceptedBaseline::V1(accepted), &current)?;
    if classification.transition != CompatibilityTransition::ContractCorrectionCandidate
        || !classification.requires_candidate
        || classification.semantic_boundary_change
    {
        bail!(
            "legacy authority migration must remain a candidate without asserting unavailable boundary change"
        );
    }
    Ok(())
}

#[test]
fn exact_v2_match_is_no_change() -> TestResult {
    let accepted = sample_v2_baseline(2, 2);
    let current = sample_report(2, 2);
    let classification = classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current)?;
    if classification.transition != CompatibilityTransition::NoChange
        || classification.requires_candidate
    {
        bail!("exact v2 match was not no_change");
    }
    Ok(())
}

#[test]
fn bucket_count_increase_is_regression() -> TestResult {
    let mut accepted_v2 = sample_v2_baseline(2, 1);
    accepted_v2.buckets.insert("parse_recovery".into(), 1);
    accepted_v2.buckets.insert("compile_effect".into(), 0);
    let mut current = sample_report(2, 1);
    current.buckets.insert("parse_recovery".into(), 0);
    current.buckets.insert("compile_effect".into(), 1);
    let classification =
        classify_transition(&AcceptedBaseline::V2(Box::new(accepted_v2)), &current)?;
    if classification.transition != CompatibilityTransition::Regression {
        bail!("bucket count increase was not classified as regression");
    }
    Ok(())
}

#[test]
fn accepted_bucket_disappearing_is_not_no_change() -> TestResult {
    let mut accepted_v2 = sample_v2_baseline(2, 2);
    accepted_v2.buckets.insert("parse_recovery".into(), 1);
    let current = sample_report(2, 2);
    let classification =
        classify_transition(&AcceptedBaseline::V2(Box::new(accepted_v2)), &current)?;
    if classification.transition != CompatibilityTransition::ContractCorrectionCandidate {
        bail!("accepted bucket disappearing was silently treated as no-change acceptance");
    }
    Ok(())
}

#[test]
fn accepted_bucket_decrease_is_not_no_change() -> TestResult {
    let mut accepted_v2 = sample_v2_baseline(2, 2);
    accepted_v2.buckets.insert("parse_recovery".into(), 2);
    let mut current = sample_report(2, 2);
    current.buckets.insert("parse_recovery".into(), 1);
    let classification =
        classify_transition(&AcceptedBaseline::V2(Box::new(accepted_v2)), &current)?;
    if classification.transition != CompatibilityTransition::ContractCorrectionCandidate {
        bail!("accepted bucket decrease was silently treated as no-change acceptance");
    }
    Ok(())
}

#[test]
fn typed_failure_inventory_change_is_not_no_change() -> TestResult {
    let mut accepted_v2 = sample_v2_baseline(1, 0);
    accepted_v2.expected_failures.push(perl_core_harness_types::RunFailure {
        path: "base/0.t".into(),
        phase: "compile".into(),
        bucket: "parse_recovery".into(),
        first_diagnostic: "diag-a".into(),
        workstream: "parser".into(),
        lsp_impact: Vec::new(),
    });
    accepted_v2.buckets.insert("parse_recovery".into(), 1);
    let mut current = sample_report(1, 0);
    current.failures.push(perl_core_harness_types::RunFailure {
        path: "base/0.t".into(),
        phase: "parse".into(),
        bucket: "parse_recovery".into(),
        first_diagnostic: "diag-b".into(),
        workstream: "parser".into(),
        lsp_impact: Vec::new(),
    });
    current.buckets.insert("parse_recovery".into(), 1);
    let classification =
        classify_transition(&AcceptedBaseline::V2(Box::new(accepted_v2)), &current)?;
    if classification.transition != CompatibilityTransition::ContractCorrectionCandidate {
        bail!("typed failure inventory change was not a correction candidate");
    }
    Ok(())
}

#[test]
fn different_v2_measurement_subject_is_not_proven() -> TestResult {
    let accepted = sample_v2_baseline(1, 1);
    let mut current = sample_report(1, 1);
    current.perl_ref = "other-perl".into();
    let classification = classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current)?;
    if classification.transition != CompatibilityTransition::NotProven
        || classification.requires_candidate
    {
        bail!("a different measurement subject was treated as an exact ratchet match");
    }
    Ok(())
}

#[test]
fn later_implementation_sha_remains_classifiable() -> TestResult {
    let accepted = sample_v2_baseline(2, 1);
    let mut current = sample_report(2, 1);
    current.commit = "b".repeat(40);
    current.file_results[0].status = RunnerStatus::Fail;
    current.file_results[0].assertions_passed = 0;
    current.file_results[1].status = RunnerStatus::Pass;
    current.file_results[1].assertions_passed = 1;
    let classification = classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current)?;
    if classification.transition != CompatibilityTransition::Regression {
        bail!("a later implementation SHA must still be able to classify as regression");
    }
    Ok(())
}

#[test]
fn unexpected_current_file_is_not_no_change() -> TestResult {
    let accepted = sample_v2_baseline(1, 1);
    let mut current = sample_report(1, 1);
    current.file_results.push(RunFileResult {
        path: "unexpected/extra.t".into(),
        status: RunnerStatus::Pass,
        assertions_passed: 1,
        assertions_total: 1,
    });
    let classification = classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current)?;
    if classification.transition != CompatibilityTransition::NotProven
        || classification.requires_candidate
    {
        bail!("an unexpected file result was treated as comparable evidence");
    }
    Ok(())
}

#[test]
fn v1_missing_accepted_file_is_not_proven() -> TestResult {
    let mut accepted = sample_v1_baseline(2, 2);
    accepted.file_results = sample_results(2, 2);
    let mut current = sample_report(1, 1);
    current.file_results = sample_results(1, 1);
    let classification = classify_transition(&AcceptedBaseline::V1(accepted), &current)?;
    if classification.transition != CompatibilityTransition::NotProven
        || classification.requires_candidate
    {
        bail!(
            "a V1 observation missing an accepted file must classify as NotProven, not Err or NoChange"
        );
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
    let classification = classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current)?;
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
    let classification = classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current)?;
    if classification.transition != CompatibilityTransition::NotProven
        || classification.requires_candidate
        || !classification.reason.contains("accepted observation repeats")
    {
        bail!("duplicate accepted paths must be rejected before comparison");
    }
    Ok(())
}

#[test]
fn additional_failed_assertions_are_regression() -> TestResult {
    let mut accepted = sample_v2_baseline(1, 0);
    accepted.file_results[0].assertions_passed = 1;
    accepted.file_results[0].assertions_total = 2;
    accepted.tap_assertions_passed = 1;
    accepted.tap_assertions_total = 2;
    let mut current = sample_report(1, 0);
    current.file_results[0].assertions_passed = 1;
    current.file_results[0].assertions_total = 3;
    current.summary.tap_assertions_passed = 1;
    current.summary.tap_assertions_total = 3;
    let classification = classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current)?;
    if classification.transition != CompatibilityTransition::Regression
        || classification.requires_candidate
    {
        bail!("more failed assertions must classify as regression, not a correction candidate");
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

fn sample_v1_baseline(total: usize, passed: usize) -> CompileBaseline {
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
        expected_failures: Vec::new(),
        file_results: sample_results(total, passed),
        semantic_boundaries: Some(Vec::new()),
    }
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
