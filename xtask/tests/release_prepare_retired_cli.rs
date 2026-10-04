//! Command-surface proof that the retired `release prepare` front door stays
//! fail-closed (#15392).
//!
//! If an unsafe legacy implementation reappears behind this surface — one that
//! claims "Tests passed", prints release-complete banners, directs the
//! operator to tag/publish, or writes a `release/` directory — these tests
//! fail.

use anyhow::{Result, anyhow};
use assert_cmd::cargo::cargo_bin_cmd;

#[test]
fn release_prepare_refuses_fail_closed_without_mutation() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let output = cargo_bin_cmd!("xtask")
        .args(["release", "prepare", "9.9.9", "--yes"])
        .current_dir(tmp.path())
        .output()?;

    if output.status.success() {
        return Err(anyhow!(
            "retired front door unexpectedly exited zero; stdout: {} stderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if !combined.contains("release-turnkey") {
        return Err(anyhow!("refusal omitted canonical route: {combined}"));
    }
    if !combined.contains("#15392") {
        return Err(anyhow!("refusal omitted ruling reference: {combined}"));
    }
    if combined.contains("Tests passed") || combined.contains("Release preparation complete") {
        return Err(anyhow!("retired surface claimed proof or completion: {combined}"));
    }
    if combined.contains("git tag") || combined.contains("publish-crates") {
        return Err(anyhow!("retired surface issued tag/publish directions: {combined}"));
    }
    if tmp.path().join("release").exists() || std::fs::read_dir(tmp.path())?.count() != 0 {
        return Err(anyhow!("refusal mutated the working directory"));
    }

    Ok(())
}

#[test]
fn release_prepare_confirmation_flag_cannot_bypass_refusal() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    // Without `--yes` the legacy surface prompted interactively; both shapes
    // must refuse identically so no input path re-enables eligibility.
    let output = cargo_bin_cmd!("xtask")
        .args(["release", "prepare", "0.0.0"])
        .current_dir(tmp.path())
        .output()?;

    if output.status.success() {
        return Err(anyhow!("refusal unexpectedly depended on the confirmation flag"));
    }
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if !combined.contains("fail-closed") || tmp.path().join("release").exists() {
        return Err(anyhow!("confirmation-free refusal was incomplete: {combined}"));
    }

    Ok(())
}
