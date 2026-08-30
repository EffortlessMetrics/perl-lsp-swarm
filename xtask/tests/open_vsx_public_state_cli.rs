//! End-to-end CLI contract for the read-only Open VSX public-state probe (#9923).

use anyhow::{Result, anyhow};
use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;

#[test]
fn an_intact_identity_exits_zero_with_an_exact_receipt() -> Result<()> {
    let root = repo_root()?;
    let temp = tempfile::TempDir::new()?;
    let receipt_path = temp.path().join("available.json");

    let output = run_probe(
        &root.join("fixtures/open_vsx_public_state/synthetic_available_exact.json"),
        &receipt_path,
    )?;

    assert!(
        output.status.success(),
        "intact identity should exit zero; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value = serde_json::from_slice(&fs::read(&receipt_path)?)?;
    assert_eq!(receipt["state"].as_str(), Some("available_exact"));
    assert_eq!(receipt["schema_version"].as_str(), Some("open_vsx_public_state.v1"));
    assert_eq!(receipt["identity"]["extension_id"].as_str(), Some("EffortlessMetrics.perl-lsp-rs"));
    Ok(())
}

#[test]
fn a_blocking_state_still_persists_its_receipt() -> Result<()> {
    let root = repo_root()?;
    let temp = tempfile::TempDir::new()?;
    let receipt_path = temp.path().join("incident.json");

    let output = run_probe(
        &root.join("fixtures/open_vsx_public_state/incident_shape_listing_absent.json"),
        &receipt_path,
    )?;

    assert!(!output.status.success(), "a missing extension must not exit zero");
    assert!(receipt_path.is_file(), "the receipt must survive a blocking state");
    let receipt: Value = serde_json::from_slice(&fs::read(&receipt_path)?)?;
    assert_eq!(receipt["state"].as_str(), Some("extension_missing"));
    Ok(())
}

#[test]
fn a_provider_failure_is_never_reported_as_a_missing_extension() -> Result<()> {
    let root = repo_root()?;
    let temp = tempfile::TempDir::new()?;
    let receipt_path = temp.path().join("rate-limited.json");

    let output = run_probe(
        &root.join("fixtures/open_vsx_public_state/synthetic_provider_rate_limited.json"),
        &receipt_path,
    )?;

    assert!(!output.status.success(), "an unproven state must not exit zero");
    let receipt: Value = serde_json::from_slice(&fs::read(&receipt_path)?)?;
    assert_eq!(receipt["state"].as_str(), Some("provider_not_proven"));
    Ok(())
}

#[test]
fn the_probe_creates_a_missing_nested_receipt_directory() -> Result<()> {
    let root = repo_root()?;
    let temp = tempfile::TempDir::new()?;
    let receipt_path = temp.path().join("new/nested/receipts/open-vsx-public-state.json");
    let parent =
        receipt_path.parent().ok_or_else(|| anyhow!("nested receipt path must have a parent"))?;
    assert!(!parent.exists(), "nested receipt parent must start absent");

    run_probe(
        &root.join("fixtures/open_vsx_public_state/synthetic_available_exact.json"),
        &receipt_path,
    )?;

    assert!(receipt_path.is_file(), "the probe did not persist the nested receipt");
    Ok(())
}

#[test]
fn the_probe_refuses_to_overwrite_its_own_observation() -> Result<()> {
    let root = repo_root()?;
    let temp = tempfile::TempDir::new()?;
    let observation = temp.path().join("observation.json");
    fs::copy(
        root.join("fixtures/open_vsx_public_state/synthetic_available_exact.json"),
        &observation,
    )?;
    let original = fs::read(&observation)?;

    let output = run_probe(&observation, &observation)?;

    assert!(!output.status.success(), "writing the receipt over its input must fail closed");
    assert_eq!(fs::read(&observation)?, original, "the observation was modified");
    Ok(())
}

#[test]
fn the_cargo_xtask_surface_classifies_identically() -> Result<()> {
    let root = repo_root()?;
    let temp = tempfile::TempDir::new()?;
    let receipt_path = temp.path().join("xtask-available.json");

    let mut command = cargo_bin_cmd!("xtask");
    let output = command
        .arg("open-vsx-public-state")
        .arg("--input")
        .arg(root.join("fixtures/open_vsx_public_state/synthetic_available_exact.json"))
        .arg("--out")
        .arg(&receipt_path)
        .output()?;

    assert!(
        output.status.success(),
        "cargo xtask surface failed; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value = serde_json::from_slice(&fs::read(&receipt_path)?)?;
    assert_eq!(receipt["state"].as_str(), Some("available_exact"));
    Ok(())
}

#[test]
fn an_input_that_violates_the_published_contract_is_refused() -> Result<()> {
    // Raised in review: deserialization alone enforced only what the Rust types
    // encode, so published constraints the classifier did not restate went
    // unchecked and could produce a receipt violating the receipt schema.
    let root = repo_root()?;
    let temp = tempfile::TempDir::new()?;
    let observation = temp.path().join("observation.json");
    let receipt_path = temp.path().join("receipt.json");

    let mut document: Value = serde_json::from_slice(&fs::read(
        root.join("fixtures/open_vsx_public_state/synthetic_available_exact.json"),
    )?)?;
    // Empty instrument name: admitted by the Rust type, refused by the contract.
    document["instrument"]["name"] = Value::String(String::new());
    fs::write(&observation, serde_json::to_vec_pretty(&document)?)?;

    let output = run_probe(&observation, &receipt_path)?;

    assert!(!output.status.success(), "a non-conforming observation must be refused");
    assert!(
        !receipt_path.exists(),
        "no receipt may be emitted for an observation that is not a valid observation"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("does not conform"),
        "the refusal should name the contract; stderr={stderr}"
    );
    Ok(())
}

fn run_probe(input: &Path, out: &Path) -> Result<Output> {
    let mut command = cargo_bin_cmd!("open-vsx-public-state");
    Ok(command.arg("--input").arg(input).arg("--out").arg(out).output()?)
}

fn repo_root() -> Result<PathBuf> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| anyhow!("xtask manifest directory must have a parent"))
}
