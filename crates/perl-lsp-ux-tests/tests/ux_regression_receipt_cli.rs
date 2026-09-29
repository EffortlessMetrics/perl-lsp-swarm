//! Executable controls for the small UX receipt command used after the gate run.

use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Result, ensure};
use serde_json::Value;
use tempfile::TempDir;

const SHA: &str = "409ddc8a14f01758b1686ac39363a93eeb446d8b";

fn emit(root: &Path) -> Result<std::process::Output> {
    Ok(Command::new(env!("CARGO_BIN_EXE_ux-regression-receipt"))
        .args([
            "--input",
            root.join("ux.log").to_str().ok_or_else(|| anyhow::anyhow!("log path"))?,
            "--receipt",
            root.join("ux.json").to_str().ok_or_else(|| anyhow::anyhow!("receipt path"))?,
            "--exit-status-file",
            root.join("ux.exit").to_str().ok_or_else(|| anyhow::anyhow!("exit path"))?,
            "--sha",
            SHA,
        ])
        .output()?)
}

#[test]
fn passing_run_emits_exact_subject_nonblocking_receipt() -> Result<()> {
    let temp = TempDir::new()?;
    fs::write(temp.path().join("ux.log"), "running 1 test\ntest result: ok. 1 passed; 0 failed\n")?;
    fs::write(temp.path().join("ux.exit"), "0\n")?;
    let output = emit(temp.path())?;
    ensure!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let value: Value = serde_json::from_slice(&fs::read(temp.path().join("ux.json"))?)?;
    ensure!(value["schema_version"] == 2);
    ensure!(value["sha"] == SHA);
    ensure!(value["result"] == "pass");
    ensure!(value["blocking"] == false);
    ensure!(value["merge_action"] == "merge_allowed");
    let stdout = String::from_utf8(output.stdout)?;
    ensure!(
        stdout
            == format!("Wrote UX regression receipt: {}\n", temp.path().join("ux.json").display()),
        "stdout was {stdout:?}"
    );
    Ok(())
}

#[test]
fn omitting_receipt_prints_the_json_payload() -> Result<()> {
    let temp = TempDir::new()?;
    fs::write(temp.path().join("ux.log"), "running 1 test\ntest result: ok. 1 passed; 0 failed\n")?;
    fs::write(temp.path().join("ux.exit"), "0\n")?;
    let output = Command::new(env!("CARGO_BIN_EXE_ux-regression-receipt"))
        .args([
            "--input",
            temp.path().join("ux.log").to_str().ok_or_else(|| anyhow::anyhow!("log path"))?,
            "--exit-status-file",
            temp.path().join("ux.exit").to_str().ok_or_else(|| anyhow::anyhow!("exit path"))?,
            "--sha",
            SHA,
        ])
        .output()?;
    ensure!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout)?;
    ensure!(stdout.ends_with('\n'), "payload stdout must end with a newline");
    let value: Value = serde_json::from_str(stdout.trim_end())?;
    ensure!(value["kind"] == "ux_regression_receipt");
    ensure!(value["sha"] == SHA);
    ensure!(value["result"] == "pass");
    ensure!(!temp.path().join("ux.json").exists());
    Ok(())
}

#[test]
fn nonzero_exit_cannot_turn_an_earlier_green_summary_into_pass() -> Result<()> {
    let temp = TempDir::new()?;
    fs::write(temp.path().join("ux.log"), "test result: ok. 1 passed; 0 failed\n")?;
    fs::write(temp.path().join("ux.exit"), "101\n")?;
    let output = emit(temp.path())?;
    ensure!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let value: Value = serde_json::from_slice(&fs::read(temp.path().join("ux.json"))?)?;
    ensure!(value["result"] == "fail");
    ensure!(value["blocking"] == true);
    ensure!(value["merge_action"] != "merge_allowed");
    Ok(())
}

#[test]
fn missing_log_or_status_and_malformed_status_emit_no_receipt() -> Result<()> {
    for missing in ["log", "status", "malformed_status"] {
        let temp = TempDir::new()?;
        if missing != "log" {
            fs::write(temp.path().join("ux.log"), "test result: ok. 1 passed; 0 failed\n")?;
        }
        if missing == "malformed_status" {
            fs::write(temp.path().join("ux.exit"), "not-an-exit-code\n")?;
        } else if missing != "status" {
            fs::write(temp.path().join("ux.exit"), "0\n")?;
        }
        let output = emit(temp.path())?;
        ensure!(!output.status.success(), "{missing} unexpectedly succeeded");
        ensure!(!temp.path().join("ux.json").exists(), "{missing} emitted a receipt");
    }
    Ok(())
}
