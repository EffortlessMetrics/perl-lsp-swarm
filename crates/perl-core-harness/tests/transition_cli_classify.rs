//! Discriminating proof for lean transition classify CLI load+classify I/O.

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
fn classify_cli_rejects_unknown_command() {
    let output = Command::new(env!("CARGO_BIN_EXE_perl-core-harness-transition"))
        .args(["check"])
        .output()
        .expect("spawn classify CLI");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unknown perl-core-harness-transition command"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn classify_cli_rejects_unrecognized_option() {
    let output = Command::new(env!("CARGO_BIN_EXE_perl-core-harness-transition"))
        .args([
            "classify",
            "--accepted-baseline",
            "accepted.json",
            "--compile",
            "compile.json",
            "--output",
            "out.json",
            "--series",
            "series.json",
        ])
        .output()
        .expect("spawn classify CLI");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let marker = "unrecognized option(s): --series";
    let observed = stderr.contains(marker);
    assert_eq!(observed, true);
    assert_eq!(marker, "unrecognized option(s): --series");
}

#[test]
fn classify_cli_rejects_v1_accepted_baseline() {
    let dir = tempdir().expect("tempdir");
    let accepted = dir.path().join("accepted.json");
    let compile = dir.path().join("compile.json");
    let output = dir.path().join("out.json");
    fs::write(
        &accepted,
        r#"{"schema_version":"perl_core_harness.compile_baseline.v1","report_schema_version":"x"}"#,
    )
    .expect("write accepted");
    write_report(&compile, 1, 1);
    let result = Command::new(env!("CARGO_BIN_EXE_perl-core-harness-transition"))
        .args([
            "classify",
            "--accepted-baseline",
            accepted.to_str().expect("utf8"),
            "--compile",
            compile.to_str().expect("utf8"),
            "--output",
            output.to_str().expect("utf8"),
        ])
        .output()
        .expect("spawn classify CLI");
    assert!(!result.status.success());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("unsupported accepted baseline schema"), "unexpected stderr: {stderr}");
}

#[test]
fn classify_cli_rejects_output_aliasing_accepted_baseline() {
    let dir = tempdir().expect("tempdir");
    let accepted = dir.path().join("accepted.json");
    let compile = dir.path().join("compile.json");
    write_baseline(&accepted, 1, 1);
    write_report(&compile, 1, 1);
    let accepted_path = accepted.to_str().expect("utf8");
    let result = Command::new(env!("CARGO_BIN_EXE_perl-core-harness-transition"))
        .args([
            "classify",
            "--accepted-baseline",
            accepted_path,
            "--compile",
            compile.to_str().expect("utf8"),
            "--output",
            accepted_path,
        ])
        .output()
        .expect("spawn classify CLI");
    assert!(!result.status.success());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("output path must not alias"), "unexpected stderr: {stderr}");
    let retained = fs::read_to_string(&accepted).expect("accepted retained");
    assert!(retained.contains(COMPILE_BASELINE_V2_SCHEMA_VERSION));
}

#[test]
fn classify_cli_writes_no_change_receipt_for_exact_v2_match() {
    let dir = tempdir().expect("tempdir");
    let accepted = dir.path().join("accepted.json");
    let compile = dir.path().join("compile.json");
    let output = dir.path().join("out.json");
    write_baseline(&accepted, 2, 2);
    write_report(&compile, 2, 2);
    let result = Command::new(env!("CARGO_BIN_EXE_perl-core-harness-transition"))
        .args([
            "classify",
            "--accepted-baseline",
            accepted.to_str().expect("utf8"),
            "--compile",
            compile.to_str().expect("utf8"),
            "--output",
            output.to_str().expect("utf8"),
        ])
        .output()
        .expect("spawn classify CLI");
    assert!(
        result.status.success(),
        "classify failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let value: Value =
        serde_json::from_str(&fs::read_to_string(&output).expect("read receipt")).expect("decode");
    assert_eq!(value["schema_version"], "perl_core_harness.transition_classify_result.v1");
    assert_eq!(value["command"], "classify");
    assert_eq!(value["transition"], "no_change");
    assert_eq!(value["requires_candidate"], false);
    assert_eq!(value["semantic_boundary_change"], false);
    assert!(
        value["claim_boundary"].as_str().expect("claim boundary").contains("classify_transition")
    );
}

#[test]
fn classify_cli_writes_regression_receipt_for_pass_to_fail() {
    let dir = tempdir().expect("tempdir");
    let accepted = dir.path().join("accepted.json");
    let compile = dir.path().join("compile.json");
    let output = dir.path().join("out.json");
    write_baseline(&accepted, 2, 1);
    let mut current = sample_report(2, 1);
    current.file_results[0].status = RunnerStatus::Fail;
    current.file_results[0].assertions_passed = 0;
    current.file_results[1].status = RunnerStatus::Pass;
    current.file_results[1].assertions_passed = 1;
    fs::write(&compile, serde_json::to_string_pretty(&current).expect("encode")).expect("write");
    let result = Command::new(env!("CARGO_BIN_EXE_perl-core-harness-transition"))
        .args([
            "classify",
            "--accepted-baseline",
            accepted.to_str().expect("utf8"),
            "--compile",
            compile.to_str().expect("utf8"),
            "--output",
            output.to_str().expect("utf8"),
        ])
        .output()
        .expect("spawn classify CLI");
    assert!(
        result.status.success(),
        "classify failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let value: Value =
        serde_json::from_str(&fs::read_to_string(&output).expect("read receipt")).expect("decode");
    assert_eq!(value["transition"], "regression");
    assert!(value["reason"].as_str().expect("reason").contains("changed from pass to fail"));
}

fn write_baseline(path: &Path, total: usize, passed: usize) {
    let baseline = sample_v2_baseline(total, passed);
    fs::write(path, serde_json::to_string_pretty(&baseline).expect("encode")).expect("write");
}

fn write_report(path: &Path, total: usize, passed: usize) {
    let report = sample_report(total, passed);
    fs::write(path, serde_json::to_string_pretty(&report).expect("encode")).expect("write");
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
