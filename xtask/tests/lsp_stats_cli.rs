use assert_cmd::cargo::cargo_bin_cmd;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

/// Run `xtask metrics lsp-stats` against a receipt directory, writing the
/// `--json` receipt to a temporary output path so the tests never touch the
/// tracked `.ci/metrics/editor_ux.json` artifact.
fn run_with_temp_output(receipts: &TempDir, output: &TempDir) -> TestResult<std::process::Output> {
    let output_path = output.path().join("editor_ux.json");
    Ok(cargo_bin_cmd!("xtask")
        .args([
            "metrics",
            "lsp-stats",
            "--json",
            "--receipt-dir",
            receipts.path().to_str().ok_or("receipt directory path is not valid UTF-8")?,
            "--output",
            output_path.to_str().ok_or("output path is not valid UTF-8")?,
        ])
        .output()?)
}

#[test]
fn invalid_timing_receipt_fails_publicly_without_overwriting_output() -> TestResult {
    let receipts = TempDir::new()?;
    let output = TempDir::new()?;
    let output_path = output.path().join("editor_ux.json");
    let sentinel = br#"{"sentinel":"preserve-me"}
"#;
    fs::write(&output_path, sentinel)?;

    fs::write(
        receipts.path().join("malformed-timing.json"),
        br#"{
            "result": "pass",
            "duration_ms": 10.0,
            "time_to_first_useful_result_ms": "not-a-number"
        }"#,
    )?;

    let process_output = run_with_temp_output(&receipts, &output)?;
    let preserved_output = fs::read(&output_path)?;

    assert!(
        !process_output.status.success(),
        "malformed timing receipt unexpectedly succeeded\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&process_output.stdout),
        String::from_utf8_lossy(&process_output.stderr)
    );
    assert_ne!(process_output.status.code(), Some(0), "invalid input must exit nonzero");
    assert_eq!(preserved_output, sentinel, "invalid input overwrote the output artifact");
    Ok(())
}

#[test]
fn valid_receipt_reaches_public_aggregation_and_writes_json() -> TestResult {
    let receipts = TempDir::new()?;
    let output = TempDir::new()?;
    let output_path = output.path().join("editor_ux.json");

    fs::write(
        receipts.path().join("public-delegation.json"),
        br#"{
            "kind": "ux_scenario_run",
            "schema_version": 1,
            "measured_at": "2026-08-28T00:00:00Z",
            "run_identity": {"sha": "abcdef12", "branch": "main"},
            "workflow_id": "simple_file_smoke",
            "scenario_file": "ux_scenario_01_simple_file.rs",
            "test_name": "public_cli_delegation",
            "ci_tier": "pr",
            "result": "pass",
            "duration_ms": 10.0,
            "time_to_first_useful_result_ms": 5.0,
            "operation_timings": [{
                "operation": "completion",
                "time_to_first_useful_result_ms": 5.0
            }],
            "assertions": {"passed": 1, "failed": 0, "basis": "instrumented"},
            "canonical_repro": "cargo test -p perl-lsp-ux-tests public_cli_delegation",
            "friendly_repro": "just ux-tests"
        }"#,
    )?;

    let process_output = run_with_temp_output(&receipts, &output)?;

    assert!(
        process_output.status.success(),
        "valid receipt failed public aggregation\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&process_output.stdout),
        String::from_utf8_lossy(&process_output.stderr)
    );
    let generated: serde_json::Value = serde_json::from_slice(&fs::read(&output_path)?)?;
    assert_eq!(generated["schema_version"], 1);
    assert_eq!(generated["subsystem"], "editor_ux");
    let workflow = generated["workflows"]
        .as_array()
        .and_then(|workflows| {
            workflows.iter().find(|workflow| workflow["id"] == "simple_file_smoke")
        })
        .ok_or("supplied workflow was not emitted")?;
    assert_eq!(workflow["pass_rate"]["state"], "measured");
    assert_eq!(workflow["pass_rate"]["value"], 1.0);
    assert_eq!(workflow["pass_rate"]["basis"][0], "1 receipts");
    assert_eq!(generated["top_line"]["workflow_pass_rate"]["value"], 1.0);
    Ok(())
}

#[test]
fn contradictory_completed_timing_fails_publicly_without_overwriting_output() -> TestResult {
    let receipts = TempDir::new()?;
    let output = TempDir::new()?;
    let output_path = output.path().join("editor_ux.json");
    let sentinel = br#"{"sentinel":"preserve-semantic-timing"}
"#;
    fs::write(&output_path, sentinel)?;

    // This receipt is structurally schema-valid: the operation has a completed
    // measurement and no status. It is semantically impossible because the
    // producer always copies the first completed operation to the top-level
    // TTFR summary.
    fs::write(
        receipts.path().join("missing-top-level-timing.json"),
        br#"{
            "kind": "ux_scenario_run",
            "schema_version": 1,
            "measured_at": "2026-08-28T00:00:00Z",
            "run_identity": {"sha": "abcdef12", "branch": "main"},
            "workflow_id": "simple_file_smoke",
            "scenario_file": "ux_scenario_01_simple_file.rs",
            "test_name": "missing_top_level_timing",
            "ci_tier": "pr",
            "result": "pass",
            "duration_ms": 10.0,
            "operation_timings": [{
                "operation": "completion",
                "time_to_first_useful_result_ms": 5.0
            }],
            "assertions": {"passed": 1, "failed": 0, "basis": "instrumented"},
            "canonical_repro": "cargo test -p perl-lsp-ux-tests missing_top_level_timing",
            "friendly_repro": "just ux-tests"
        }"#,
    )?;

    let process_output = run_with_temp_output(&receipts, &output)?;
    let preserved_output = fs::read(&output_path)?;
    let stderr = String::from_utf8_lossy(&process_output.stderr);

    assert!(
        !process_output.status.success(),
        "semantically contradictory timing unexpectedly succeeded\nstdout: {}\nstderr: {stderr}",
        String::from_utf8_lossy(&process_output.stdout)
    );
    assert!(
        stderr.contains("top-level TTFR is absent"),
        "public command did not report the semantic timing contradiction: {stderr}"
    );
    assert_eq!(
        preserved_output, sentinel,
        "semantic timing rejection overwrote the output artifact"
    );
    Ok(())
}

#[test]
fn invalid_nested_timing_receipt_fails_publicly_without_overwriting_output() -> TestResult {
    let receipts = TempDir::new()?;
    let output = TempDir::new()?;
    let output_path = output.path().join("editor_ux.json");
    let sentinel = br#"{"sentinel":"preserve-nested"}
"#;
    fs::write(&output_path, sentinel)?;

    fs::write(
        receipts.path().join("malformed-nested-timing.json"),
        br#"{
            "result": "pass",
            "operation_timings": [{
                "operation": "hover",
                "time_to_first_useful_result_ms": "not-a-number"
            }]
        }"#,
    )?;

    let process_output = run_with_temp_output(&receipts, &output)?;
    let preserved_output = fs::read(&output_path)?;

    assert!(!process_output.status.success(), "nested malformed timing unexpectedly succeeded");
    assert_ne!(process_output.status.code(), Some(0), "nested invalid input must exit nonzero");
    assert_eq!(preserved_output, sentinel, "nested invalid input overwrote the output artifact");
    Ok(())
}

#[test]
fn tracked_metrics_artifact_is_untouched_by_cli_tests() -> TestResult {
    // The `--output` override must keep the run off the tracked artifact:
    // this test passes only when the CLI wrote its receipt to the temporary
    // path and left the default file alone.
    let receipts = TempDir::new()?;
    let output = TempDir::new()?;

    fs::write(
        receipts.path().join("tracked-artifact.json"),
        br#"{
            "kind": "ux_scenario_run",
            "schema_version": 1,
            "measured_at": "2026-08-28T00:00:00Z",
            "run_identity": {"sha": "abcdef12", "branch": "main"},
            "workflow_id": "simple_file_smoke",
            "scenario_file": "ux_scenario_01_simple_file.rs",
            "test_name": "tracked_artifact_untouched",
            "ci_tier": "pr",
            "result": "pass",
            "duration_ms": 10.0,
            "time_to_first_useful_result_ms": 5.0,
            "operation_timings": [{
                "operation": "completion",
                "time_to_first_useful_result_ms": 5.0
            }],
            "assertions": {"passed": 1, "failed": 0, "basis": "instrumented"},
            "canonical_repro": "cargo test -p perl-lsp-ux-tests tracked_artifact_untouched",
            "friendly_repro": "just ux-tests"
        }"#,
    )?;

    let workspace_root = workspace_root()?;
    let tracked = workspace_root.join(".ci/metrics/editor_ux.json");
    let tracked_before = fs::read(&tracked).ok();

    let process_output = run_with_temp_output(&receipts, &output)?;
    assert!(
        process_output.status.success(),
        "valid receipt failed aggregation\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&process_output.stdout),
        String::from_utf8_lossy(&process_output.stderr)
    );

    let written = fs::read_to_string(output.path().join("editor_ux.json"))?;
    assert!(
        written.contains("\"subsystem\""),
        "the CLI did not write its receipt to the temporary output path"
    );
    let tracked_after = fs::read(&tracked).ok();
    assert_eq!(tracked_before, tracked_after, "the CLI overwrote the tracked metrics artifact");
    Ok(())
}

fn workspace_root() -> TestResult<PathBuf> {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "xtask manifest must have a workspace parent".into())
}
