//! Discriminating proof for the lean transition classify CLI slice.
//!
//! Covers: required-arg rejection, V2-only baseline gate, and one end-to-end
//! Regression classification written through the binary I/O path.

use perl_core_harness_types::{
    COMPILE_BASELINE_V2_SCHEMA_VERSION, CompileBaselineV2, HarnessMode, HarnessProfile,
    HarnessRunner, RUN_REPORT_SCHEMA_VERSION, RunFileResult, RunReport, RunSummary, RunnerStatus,
};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;

#[test]
fn classify_cli_rejects_missing_required_option() {
    let output = Command::new(env!("CARGO_BIN_EXE_perl-core-harness-transition"))
        .args(["classify", "--accepted-baseline", "accepted.json", "--compile", "compile.json"])
        .output()
        .expect("spawn classify CLI");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("required option --output was not supplied"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn classify_cli_rejects_non_v2_baseline() {
    let dir = tempdir().expect("tempdir");
    let accepted = dir.path().join("accepted.json");
    let compile = dir.path().join("compile.json");
    let out = dir.path().join("out.json");
    fs::write(&accepted, r#"{"schema_version":"perl_core_harness.compile_baseline.v1"}"#)
        .expect("write accepted");
    write_json(&compile, &sample_report(1, 1));

    let output = Command::new(env!("CARGO_BIN_EXE_perl-core-harness-transition"))
        .args([
            "classify",
            "--accepted-baseline",
            accepted.to_str().expect("utf8 path"),
            "--compile",
            compile.to_str().expect("utf8 path"),
            "--output",
            out.to_str().expect("utf8 path"),
        ])
        .output()
        .expect("spawn classify CLI");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("this CLI slice accepts only"), "unexpected stderr: {stderr}");
}

#[test]
fn classify_cli_rejects_inconsistent_accepted_counts() {
    let dir = tempdir().expect("tempdir");
    let accepted = dir.path().join("accepted.json");
    let compile = dir.path().join("compile.json");
    let out = dir.path().join("out.json");
    let mut baseline = sample_v2_baseline(2, 2);
    baseline.files_passed = 0;
    write_json(&accepted, &baseline);
    write_json(&compile, &sample_report(2, 2));

    let output = Command::new(env!("CARGO_BIN_EXE_perl-core-harness-transition"))
        .args([
            "classify",
            "--accepted-baseline",
            accepted.to_str().expect("utf8 path"),
            "--compile",
            compile.to_str().expect("utf8 path"),
            "--output",
            out.to_str().expect("utf8 path"),
        ])
        .output()
        .expect("spawn classify CLI");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("internally inconsistent"), "unexpected stderr: {stderr}");
}

#[test]
fn classify_cli_creates_missing_output_directories() {
    let dir = tempdir().expect("tempdir");
    let accepted = dir.path().join("accepted.json");
    let compile = dir.path().join("compile.json");
    let out = dir.path().join("artifacts").join("transitions").join("out.json");
    write_json(&accepted, &sample_v2_baseline(1, 1));
    write_json(&compile, &sample_report(1, 1));

    let output = Command::new(env!("CARGO_BIN_EXE_perl-core-harness-transition"))
        .args([
            "classify",
            "--accepted-baseline",
            accepted.to_str().expect("utf8 path"),
            "--compile",
            compile.to_str().expect("utf8 path"),
            "--output",
            out.to_str().expect("utf8 path"),
        ])
        .output()
        .expect("spawn classify CLI");
    assert!(
        output.status.success(),
        "classify failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(out.is_file());
}

#[test]
fn classify_cli_writes_regression_for_pass_to_fail() {
    let dir = tempdir().expect("tempdir");
    let accepted = dir.path().join("accepted.json");
    let compile = dir.path().join("compile.json");
    let out = dir.path().join("classification.json");

    let mut current = sample_report(2, 1);
    current.file_results[0].status = RunnerStatus::Fail;
    current.file_results[0].assertions_passed = 0;
    current.file_results[1].status = RunnerStatus::Pass;
    current.file_results[1].assertions_passed = 1;

    write_json(&accepted, &sample_v2_baseline(2, 1));
    write_json(&compile, &current);

    let output = Command::new(env!("CARGO_BIN_EXE_perl-core-harness-transition"))
        .args([
            "classify",
            "--accepted-baseline",
            accepted.to_str().expect("utf8 path"),
            "--compile",
            compile.to_str().expect("utf8 path"),
            "--output",
            out.to_str().expect("utf8 path"),
        ])
        .output()
        .expect("spawn classify CLI");
    assert!(
        output.status.success(),
        "classify failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let value: Value = serde_json::from_str(&fs::read_to_string(&out).expect("read output"))
        .expect("decode output");
    assert_eq!(value["schema_version"], "perl_core_harness.transition_classification.v1");
    assert_eq!(value["transition"], "regression");
    assert_eq!(value["requires_candidate"], false);
    assert_eq!(value["accepted_state_change_permitted"], false);
    assert!(
        value["reason"]
            .as_str()
            .expect("reason string")
            .contains("base/0.t changed from pass to fail")
    );
    assert!(
        value["claim_boundary"]
            .as_str()
            .expect("claim boundary")
            .contains("cannot accept or lower")
    );
}

fn write_json(path: &Path, value: &impl serde::Serialize) {
    fs::write(path, format!("{}\n", serde_json::to_string_pretty(value).expect("serialize")))
        .expect("write fixture");
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
