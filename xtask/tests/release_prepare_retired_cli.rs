//! Command-surface proof that the retired `release prepare` front door stays
//! fail-closed (#15392).
//!
//! If an unsafe legacy implementation reappears behind this surface — one that
//! claims "Tests passed", prints release-complete banners, directs the
//! operator to tag/publish, or writes a `release/` directory — these tests
//! fail.

use anyhow::Result;
use assert_cmd::cargo::cargo_bin_cmd;

#[test]
fn release_prepare_refuses_fail_closed_without_mutation() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let output = cargo_bin_cmd!("xtask")
        .args(["release", "prepare", "9.9.9", "--yes"])
        .current_dir(tmp.path())
        .output()?;

    assert!(
        !output.status.success(),
        "retired front door must exit non-zero; stdout: {} stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("release-turnkey"),
        "refusal must route to the canonical path; got: {combined}"
    );
    assert!(combined.contains("#15392"), "refusal must cite the ruling; got: {combined}");
    assert!(
        !combined.contains("Tests passed"),
        "no proof may be claimed by the retired surface; got: {combined}"
    );
    assert!(
        !combined.contains("Release preparation complete"),
        "no completion may be claimed by the retired surface; got: {combined}"
    );
    assert!(
        !combined.contains("git tag") && !combined.contains("publish-crates"),
        "retired surface must not issue tag/publish directions; got: {combined}"
    );
    assert!(!tmp.path().join("release").exists(), "refusal must perform zero filesystem mutation");
    assert_eq!(
        std::fs::read_dir(tmp.path())?.count(),
        0,
        "refusal must leave the working directory untouched"
    );

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

    assert!(!output.status.success(), "refusal must not depend on the confirmation flag");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(combined.contains("fail-closed"), "got: {combined}");
    assert!(!tmp.path().join("release").exists());

    Ok(())
}
