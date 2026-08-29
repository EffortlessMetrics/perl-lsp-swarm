use assert_cmd::cargo::cargo_bin_cmd;
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use tempfile::TempDir;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

static METRICS_OUTPUT_LOCK: Mutex<()> = Mutex::new(());

struct OutputRestore {
    path: PathBuf,
    original: Option<Vec<u8>>,
}

impl Drop for OutputRestore {
    fn drop(&mut self) {
        match &self.original {
            Some(original) => {
                let _ = fs::write(&self.path, original);
            }
            None => {
                let _ = fs::remove_file(&self.path);
            }
        }
    }
}

fn workspace_root() -> TestResult<PathBuf> {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(PathBuf::from)
        .ok_or_else(|| "xtask manifest must have a workspace parent".into())
}

#[test]
fn invalid_timing_receipt_fails_publicly_without_overwriting_output() -> TestResult {
    let _lock = METRICS_OUTPUT_LOCK.lock().map_err(|_| "metrics output lock poisoned")?;
    let root = workspace_root()?;
    let output_path = root.join(".ci/metrics/editor_ux.json");
    let restore =
        OutputRestore { path: output_path.clone(), original: fs::read(&output_path).ok() };
    let sentinel = br#"{"sentinel":"preserve-me"}
"#;
    fs::write(&output_path, sentinel)?;

    let receipts = TempDir::new()?;
    fs::write(
        receipts.path().join("malformed-timing.json"),
        br#"{
            "result": "pass",
            "duration_ms": 10.0,
            "time_to_first_useful_result_ms": "not-a-number"
        }"#,
    )?;

    let output = cargo_bin_cmd!("xtask")
        .args([
            "metrics",
            "lsp-stats",
            "--json",
            "--receipt-dir",
            receipts.path().to_str().ok_or("receipt directory path is not valid UTF-8")?,
        ])
        .output()?;
    let preserved_output = fs::read(&output_path)?;

    assert!(
        !output.status.success(),
        "malformed timing receipt unexpectedly succeeded\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_ne!(output.status.code(), Some(0), "invalid input must exit nonzero");
    assert_eq!(preserved_output, sentinel, "invalid input overwrote the output artifact");
    drop(restore);
    Ok(())
}

#[test]
fn valid_receipt_reaches_public_aggregation_and_writes_json() -> TestResult {
    let _lock = METRICS_OUTPUT_LOCK.lock().map_err(|_| "metrics output lock poisoned")?;
    let root = workspace_root()?;
    let output_path = root.join(".ci/metrics/editor_ux.json");
    let restore =
        OutputRestore { path: output_path.clone(), original: fs::read(&output_path).ok() };

    let receipts = TempDir::new()?;
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

    let output = cargo_bin_cmd!("xtask")
        .args([
            "metrics",
            "lsp-stats",
            "--json",
            "--receipt-dir",
            receipts.path().to_str().ok_or("receipt directory path is not valid UTF-8")?,
        ])
        .output()?;

    assert!(
        output.status.success(),
        "valid receipt failed public aggregation\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
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
    drop(restore);
    Ok(())
}

#[test]
fn invalid_nested_timing_receipt_fails_publicly_without_overwriting_output() -> TestResult {
    let _lock = METRICS_OUTPUT_LOCK.lock().map_err(|_| "metrics output lock poisoned")?;
    let root = workspace_root()?;
    let output_path = root.join(".ci/metrics/editor_ux.json");
    let restore =
        OutputRestore { path: output_path.clone(), original: fs::read(&output_path).ok() };
    let sentinel = br#"{"sentinel":"preserve-nested"}
"#;
    fs::write(&output_path, sentinel)?;

    let receipts = TempDir::new()?;
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

    let output = cargo_bin_cmd!("xtask")
        .args([
            "metrics",
            "lsp-stats",
            "--json",
            "--receipt-dir",
            receipts.path().to_str().ok_or("receipt directory path is not valid UTF-8")?,
        ])
        .output()?;
    let preserved_output = fs::read(&output_path)?;

    assert!(!output.status.success(), "nested malformed timing unexpectedly succeeded");
    assert_ne!(output.status.code(), Some(0), "nested invalid input must exit nonzero");
    assert_eq!(preserved_output, sentinel, "nested invalid input overwrote the output artifact");
    drop(restore);
    Ok(())
}
