//! End-to-end CLI contract for the authority-bound publication-drift classifier.

use anyhow::{Result, anyhow};
use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn clean_fixture_succeeds_with_verified_authority_receipt() -> Result<()> {
    let root = repo_root()?;
    let temp = TempDir::new()?;
    let receipt_path = temp.path().join("clean-receipt.json");

    let mut command = cargo_bin_cmd!("publication-drift");
    let output = command
        .arg("--input")
        .arg(root.join("fixtures/publication_drift/clean.json"))
        .arg("--repo-root")
        .arg(&root)
        .arg("--out")
        .arg(&receipt_path)
        .output()?;

    assert!(
        output.status.success(),
        "clean classifier invocation failed; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let receipt: Value = serde_json::from_slice(&fs::read(&receipt_path)?)?;
    assert_eq!(receipt["verdict"].as_str(), Some("clean"));
    assert_eq!(receipt["manifest_verification"]["status"].as_str(), Some("verified"));
    assert_eq!(receipt["authority_valid"].as_bool(), Some(true));
    Ok(())
}

#[test]
fn cargo_xtask_surface_uses_the_same_clean_authority() -> Result<()> {
    let root = repo_root()?;
    let temp = TempDir::new()?;
    let receipt_path = temp.path().join("xtask-clean-receipt.json");

    let mut command = cargo_bin_cmd!("xtask");
    let output = command
        .arg("publication-drift")
        .arg("--input")
        .arg(root.join("fixtures/publication_drift/clean.json"))
        .arg("--repo-root")
        .arg(&root)
        .arg("--out")
        .arg(&receipt_path)
        .output()?;

    assert!(
        output.status.success(),
        "cargo xtask publication-drift failed; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value = serde_json::from_slice(&fs::read(&receipt_path)?)?;
    assert_eq!(receipt["verdict"].as_str(), Some("clean"));
    assert_eq!(receipt["authority_valid"].as_bool(), Some(true));
    Ok(())
}

#[test]
fn known_windows_drift_blocks_after_writing_drift_receipt() -> Result<()> {
    let root = repo_root()?;
    let temp = TempDir::new()?;
    let receipt_path = temp.path().join("drift-receipt.json");

    let mut command = cargo_bin_cmd!("publication-drift");
    let output = command
        .arg("--input")
        .arg(root.join("fixtures/publication_drift/windows_arm64_target_drift.json"))
        .arg("--repo-root")
        .arg(&root)
        .arg("--out")
        .arg(&receipt_path)
        .output()?;

    assert!(!output.status.success(), "known product drift must return a blocking exit");
    assert!(receipt_path.is_file(), "blocking classifier invocation must retain its receipt");

    let receipt: Value = serde_json::from_slice(&fs::read(&receipt_path)?)?;
    assert_eq!(receipt["verdict"].as_str(), Some("drift"));
    assert_eq!(receipt["manifest_verification"]["status"].as_str(), Some("verified"));
    assert!(
        has_blocker(&receipt, "same_version_divergent_product"),
        "missing same_version_divergent_product blocker; blockers={}",
        receipt["blockers"]
    );
    assert!(
        has_blocker(&receipt, "product_drift"),
        "missing product_drift blocker; blockers={}",
        receipt["blockers"]
    );
    Ok(())
}

fn repo_root() -> Result<PathBuf> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest
        .parent()
        .ok_or_else(|| anyhow!("xtask manifest must live below the repository root"))?;
    Ok(root.to_path_buf())
}

fn has_blocker(receipt: &Value, code: &str) -> bool {
    receipt["blockers"].as_array().is_some_and(|blockers| {
        blockers.iter().any(|blocker| blocker["code"].as_str() == Some(code))
    })
}
