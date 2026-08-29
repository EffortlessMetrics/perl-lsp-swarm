//! Black-box CLI regressions for the editor UX scorecard.
//!
//! This integration target runs the already-built `xtask` binary through
//! `assert_cmd`; it must not recursively invoke Cargo from a unit-test
//! process.

use anyhow::{Result, anyhow, ensure};
use assert_cmd::cargo::cargo_bin_cmd;
use std::fs;

#[test]
fn malformed_measurement_cli_exits_nonzero_before_writing_artifacts() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let input_path = temp.path().join("malformed.json");
    let output_path = temp.path().join("scorecard.json");
    let status_path = temp.path().join("status.md");
    fs::write(&input_path, "[{ malformed measurement")?;

    let output = cargo_bin_cmd!("xtask")
        .args(["ux-scorecard", "--ratchet-check", "--input"])
        .arg(&input_path)
        .arg("--output")
        .arg(&output_path)
        .arg("--status-md")
        .arg(&status_path)
        .output()?;

    ensure!(!output.status.success(), "malformed measurement CLI unexpectedly succeeded");
    let stderr = String::from_utf8(output.stderr).map_err(|error| anyhow!(error))?;
    ensure!(
        stderr.contains("parsing"),
        "malformed measurement error missing parsing context: {stderr}"
    );
    ensure!(!output_path.exists(), "malformed measurement created scorecard artifact");
    ensure!(!status_path.exists(), "malformed measurement created status artifact");
    Ok(())
}
