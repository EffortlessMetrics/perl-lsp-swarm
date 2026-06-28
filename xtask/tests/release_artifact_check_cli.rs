//! CLI integration tests for `cargo xtask release artifact-check`.
//!
//! Unit tests for the pure helpers live inline in
//! `tasks/release_artifact_check.rs`. These tests exercise the full command
//! against real fixture archives under `tests/fixtures/release-artifacts/`.

use assert_cmd::Command;
use color_eyre::eyre::Result;
use std::path::PathBuf;

fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/release-artifacts").join(rel)
}

#[test]
fn help_mentions_dist_flag() -> Result<()> {
    let output =
        Command::cargo_bin("xtask")?.args(["release", "artifact-check", "--help"]).output()?;
    assert!(output.status.success(), "help should exit 0");
    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("--dist"), "help should mention --dist; got: {stdout}");
    Ok(())
}

#[test]
fn good_dist_passes() -> Result<()> {
    let output = Command::cargo_bin("xtask")?
        .args(["release", "artifact-check", "--allow-partial", "--dist"])
        .arg(fixture("good"))
        .output()?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "good fixture should pass; stderr: {stderr}");
    Ok(())
}

#[test]
fn missing_dap_binary_fails() -> Result<()> {
    let output = Command::cargo_bin("xtask")?
        .args(["release", "artifact-check", "--allow-partial", "--dist"])
        .arg(fixture("bad-missing-dap"))
        .output()?;
    assert!(!output.status.success(), "missing perl-dap should fail the check");
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("perl-dap"), "failure should name the missing binary; got: {stderr}");
    Ok(())
}

#[test]
fn missing_dist_directory_errors() -> Result<()> {
    let output = Command::cargo_bin("xtask")?
        .args(["release", "artifact-check", "--dist"])
        .arg(fixture("does-not-exist"))
        .output()?;
    assert!(!output.status.success(), "nonexistent dist should error");
    Ok(())
}
