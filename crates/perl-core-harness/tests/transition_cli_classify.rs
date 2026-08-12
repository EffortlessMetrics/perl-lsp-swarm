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
        .args(["accept"])
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
            "--discovery",
            "discovery.json",
        ])
        .output()
        .expect("spawn classify CLI");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let marker = "unrecognized option(s): --discovery";
    let observed = stderr.contains(marker);
    assert_eq!(observed, true);
    assert_eq!(marker, "unrecognized option(s): --discovery");
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
fn classify_cli_rejects_output_aliasing_series() {
    let dir = tempdir().expect("tempdir");
    let accepted = dir.path().join("accepted.json");
    let compile = dir.path().join("compile.json");
    let series = dir.path().join("series.json");
    write_baseline(&accepted, 1, 1);
    write_report(&compile, 1, 1);
    write_series(&series, "series", "manifest", &["base/0.t"]);
    let series_path = series.to_str().expect("utf8");
    let result = Command::new(env!("CARGO_BIN_EXE_perl-core-harness-transition"))
        .args([
            "classify",
            "--accepted-baseline",
            accepted.to_str().expect("utf8"),
            "--compile",
            compile.to_str().expect("utf8"),
            "--series",
            series_path,
            "--output",
            series_path,
        ])
        .output()
        .expect("spawn classify CLI");
    assert!(!result.status.success());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("output path must not alias") && stderr.contains("--series"),
        "unexpected stderr: {stderr}"
    );
    let retained = fs::read_to_string(&series).expect("series retained");
    assert!(retained.contains("perl_core_harness.comparison_series.v1"));
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
    let accepted_digest = value["accepted_baseline_digest"].as_str().expect("accepted digest");
    let compile_digest = value["compile_digest"].as_str().expect("compile digest");
    assert!(accepted_digest.starts_with("sha256:"));
    assert!(compile_digest.starts_with("sha256:"));
    assert_ne!(accepted_digest, compile_digest);
    assert!(value["claim_boundary"].as_str().expect("claim boundary").contains("input digests"));
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

#[test]
fn check_cli_accepts_fresh_classify_receipt() {
    let dir = tempdir().expect("tempdir");
    let accepted = dir.path().join("accepted.json");
    let compile = dir.path().join("compile.json");
    let receipt = dir.path().join("out.json");
    write_baseline(&accepted, 2, 2);
    write_report(&compile, 2, 2);
    let classify = Command::new(env!("CARGO_BIN_EXE_perl-core-harness-transition"))
        .args([
            "classify",
            "--accepted-baseline",
            accepted.to_str().expect("utf8"),
            "--compile",
            compile.to_str().expect("utf8"),
            "--output",
            receipt.to_str().expect("utf8"),
        ])
        .output()
        .expect("spawn classify CLI");
    assert!(
        classify.status.success(),
        "classify failed: {}",
        String::from_utf8_lossy(&classify.stderr)
    );
    let check = Command::new(env!("CARGO_BIN_EXE_perl-core-harness-transition"))
        .args([
            "check",
            "--accepted-baseline",
            accepted.to_str().expect("utf8"),
            "--compile",
            compile.to_str().expect("utf8"),
            "--receipt",
            receipt.to_str().expect("utf8"),
        ])
        .output()
        .expect("spawn check CLI");
    assert!(check.status.success(), "check failed: {}", String::from_utf8_lossy(&check.stderr));
}

#[test]
fn check_cli_rejects_forged_transition() {
    let dir = tempdir().expect("tempdir");
    let accepted = dir.path().join("accepted.json");
    let compile = dir.path().join("compile.json");
    let receipt_path = dir.path().join("out.json");
    write_baseline(&accepted, 2, 2);
    write_report(&compile, 2, 2);
    let classify = Command::new(env!("CARGO_BIN_EXE_perl-core-harness-transition"))
        .args([
            "classify",
            "--accepted-baseline",
            accepted.to_str().expect("utf8"),
            "--compile",
            compile.to_str().expect("utf8"),
            "--output",
            receipt_path.to_str().expect("utf8"),
        ])
        .output()
        .expect("spawn classify CLI");
    assert!(classify.status.success());
    let mut value: Value =
        serde_json::from_str(&fs::read_to_string(&receipt_path).expect("read")).expect("decode");
    value["transition"] = Value::String("regression".into());
    fs::write(&receipt_path, serde_json::to_string_pretty(&value).expect("encode"))
        .expect("write forged receipt");
    let check = Command::new(env!("CARGO_BIN_EXE_perl-core-harness-transition"))
        .args([
            "check",
            "--accepted-baseline",
            accepted.to_str().expect("utf8"),
            "--compile",
            compile.to_str().expect("utf8"),
            "--receipt",
            receipt_path.to_str().expect("utf8"),
        ])
        .output()
        .expect("spawn check CLI");
    assert!(!check.status.success());
    let stderr = String::from_utf8_lossy(&check.stderr);
    assert!(stderr.contains("classify receipt transition mismatch"), "unexpected stderr: {stderr}");
}

#[test]
fn check_cli_rejects_mutated_compile_bytes() {
    let dir = tempdir().expect("tempdir");
    let accepted = dir.path().join("accepted.json");
    let compile = dir.path().join("compile.json");
    let receipt = dir.path().join("out.json");
    write_baseline(&accepted, 2, 2);
    write_report(&compile, 2, 2);
    let classify = Command::new(env!("CARGO_BIN_EXE_perl-core-harness-transition"))
        .args([
            "classify",
            "--accepted-baseline",
            accepted.to_str().expect("utf8"),
            "--compile",
            compile.to_str().expect("utf8"),
            "--output",
            receipt.to_str().expect("utf8"),
        ])
        .output()
        .expect("spawn classify CLI");
    assert!(classify.status.success());
    // Keep decoded classification identical (still exact NoChange) while changing
    // byte identity so only the digest gate can fail.
    let mut report = sample_report(2, 2);
    report.timestamp = "2026-08-11T00:00:01Z".into();
    fs::write(&compile, serde_json::to_string_pretty(&report).expect("encode")).expect("write");
    let check = Command::new(env!("CARGO_BIN_EXE_perl-core-harness-transition"))
        .args([
            "check",
            "--accepted-baseline",
            accepted.to_str().expect("utf8"),
            "--compile",
            compile.to_str().expect("utf8"),
            "--receipt",
            receipt.to_str().expect("utf8"),
        ])
        .output()
        .expect("spawn check CLI");
    assert!(!check.status.success());
    let stderr = String::from_utf8_lossy(&check.stderr);
    assert!(
        stderr.contains("compile_digest does not match current evidence bytes"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn check_cli_rejects_missing_receipt_option() {
    let output = Command::new(env!("CARGO_BIN_EXE_perl-core-harness-transition"))
        .args(["check", "--accepted-baseline", "accepted.json", "--compile", "compile.json"])
        .output()
        .expect("spawn check CLI");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("required option --receipt was not supplied"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn classify_cli_binds_matching_series_identity() {
    let dir = tempdir().expect("tempdir");
    let accepted = dir.path().join("accepted.json");
    let compile = dir.path().join("compile.json");
    let series = dir.path().join("series.json");
    let output = dir.path().join("out.json");
    write_baseline(&accepted, 2, 2);
    write_report(&compile, 2, 2);
    write_series(&series, "series", "manifest", &["base/0.t", "base/1.t"]);
    let result = Command::new(env!("CARGO_BIN_EXE_perl-core-harness-transition"))
        .args([
            "classify",
            "--accepted-baseline",
            accepted.to_str().expect("utf8"),
            "--compile",
            compile.to_str().expect("utf8"),
            "--series",
            series.to_str().expect("utf8"),
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
    assert_eq!(value["transition"], "no_change");
}

#[test]
fn classify_cli_rejects_series_id_mismatch() {
    let dir = tempdir().expect("tempdir");
    let accepted = dir.path().join("accepted.json");
    let compile = dir.path().join("compile.json");
    let series = dir.path().join("series.json");
    let output = dir.path().join("out.json");
    write_baseline(&accepted, 2, 2);
    write_report(&compile, 2, 2);
    write_series(&series, "other-series", "manifest", &["base/0.t", "base/1.t"]);
    let result = Command::new(env!("CARGO_BIN_EXE_perl-core-harness-transition"))
        .args([
            "classify",
            "--accepted-baseline",
            accepted.to_str().expect("utf8"),
            "--compile",
            compile.to_str().expect("utf8"),
            "--series",
            series.to_str().expect("utf8"),
            "--output",
            output.to_str().expect("utf8"),
        ])
        .output()
        .expect("spawn classify CLI");
    assert!(!result.status.success());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("accepted baseline is not bound to series"),
        "unexpected stderr: {stderr}"
    );
    assert!(!output.exists(), "mismatched series must not write a classify receipt");
}

#[test]
fn classify_cli_defers_series_membership_mismatch() {
    let dir = tempdir().expect("tempdir");
    let accepted = dir.path().join("accepted.json");
    let compile = dir.path().join("compile.json");
    let series = dir.path().join("series.json");
    let output = dir.path().join("out.json");
    write_baseline(&accepted, 2, 2);
    write_report(&compile, 2, 2);
    // Same series_id/hash labels; membership differs — deferred this slice.
    write_series(&series, "series", "manifest", &["base/0.t", "base/9.t"]);
    let result = Command::new(env!("CARGO_BIN_EXE_perl-core-harness-transition"))
        .args([
            "classify",
            "--accepted-baseline",
            accepted.to_str().expect("utf8"),
            "--compile",
            compile.to_str().expect("utf8"),
            "--series",
            series.to_str().expect("utf8"),
            "--output",
            output.to_str().expect("utf8"),
        ])
        .output()
        .expect("spawn classify CLI");
    assert!(result.status.success(), "stderr={}", String::from_utf8_lossy(&result.stderr));
    assert!(output.exists(), "matching series_id/hash must still write a classify receipt");
}

fn write_baseline(path: &Path, total: usize, passed: usize) {
    let baseline = sample_v2_baseline(total, passed);
    fs::write(path, serde_json::to_string_pretty(&baseline).expect("encode")).expect("write");
}

fn write_report(path: &Path, total: usize, passed: usize) {
    let report = sample_report(total, passed);
    fs::write(path, serde_json::to_string_pretty(&report).expect("encode")).expect("write");
}

fn write_series(path: &Path, series_id: &str, manifest_hash: &str, files: &[&str]) {
    let body = format!(
        r#"{{
  "schema_version": "perl_core_harness.comparison_series.v1",
  "series_id": "{series_id}",
  "profile": "base",
  "profile_roots": ["base"],
  "repository_commit": "{commit}",
  "perl_requested_ref": "perl",
  "perl_resolved_ref": "perl",
  "runner": "test",
  "normalized_manifest": {files},
  "manifest_hash": "{manifest_hash}",
  "preparation_receipt_id": "prepare",
  "preparation_receipt_digest": "sha256:prep",
  "harness_schema_version": "perl_core_harness.discovery.v1",
  "compiler_subject_identity": "compiler",
  "invocation_identity": "invocation",
  "capability_identity": "capability",
  "environment_identity": "environment",
  "normalization_version": "path-normalization.v1",
  "created_at": "2026-08-11T00:00:00Z",
  "replaces_series_id": null,
  "change_reason": null
}}"#,
        commit = "a".repeat(40),
        files = serde_json::to_string(files).expect("encode files"),
    );
    fs::write(path, body).expect("write series");
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
