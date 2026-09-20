use anyhow::Result;
use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::{Value, json};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn fixture_path(name: &str) -> String {
    Path::new("tests").join("fixtures").join("release-evidence").join(name).display().to_string()
}

fn copy_complete_bundle() -> Result<TempDir> {
    let temp = TempDir::new()?;
    for entry in fs::read_dir(fixture_path("complete"))? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            fs::copy(entry.path(), temp.path().join(entry.file_name()))?;
        }
    }
    Ok(temp)
}

fn mutate_parser_ratchet<F>(bundle: &Path, mutate: F) -> Result<()>
where
    F: FnOnce(&mut Value),
{
    let path = bundle.join("parser-ratchet-release.json");
    let mut value: Value = serde_json::from_str(&fs::read_to_string(&path)?)?;
    mutate(&mut value);
    fs::write(path, serde_json::to_string_pretty(&value)?)?;
    Ok(())
}

fn verify_bundle(bundle: &Path, receipt: &Path) -> assert_cmd::assert::Assert {
    let bundle_arg = bundle.display().to_string();
    let receipt_arg = receipt.display().to_string();
    cargo_bin_cmd!("xtask")
        .args([
            "release",
            "verify-evidence",
            "--version",
            "0.13.0",
            "--bundle-dir",
            &bundle_arg,
            "--receipt",
            &receipt_arg,
        ])
        .assert()
}

fn summary_has_blocking_failure(summary: &Value, needle: &str) -> bool {
    summary.get("blocking_failures").and_then(Value::as_array).is_some_and(|items| {
        items.iter().any(|item| item.as_str().is_some_and(|message| message.contains(needle)))
    })
}

#[test]
fn fixture_complete_bundle_passes() -> Result<()> {
    let temp = TempDir::new()?;
    let receipt = temp.path().join("release-evidence.json");
    let fixture = fixture_path("complete");

    verify_bundle(Path::new(&fixture), &receipt).success();

    let summary: Value = serde_json::from_str(&fs::read_to_string(&receipt)?)?;
    assert_eq!(summary.get("status").and_then(Value::as_str), Some("pass"));
    Ok(())
}

#[test]
fn fixture_missing_parser_ratchet_release_fails() -> Result<()> {
    let temp = TempDir::new()?;
    let receipt = temp.path().join("release-evidence.json");
    let fixture = fixture_path("missing-parser");

    verify_bundle(Path::new(&fixture), &receipt).failure();

    let summary: Value = serde_json::from_str(&fs::read_to_string(&receipt)?)?;
    assert_eq!(summary.get("status").and_then(Value::as_str), Some("fail"));
    Ok(())
}

#[test]
fn pass_shaped_parser_ratchet_scaffold_is_release_blocking_failure() -> Result<()> {
    let bundle = copy_complete_bundle()?;
    let receipt = bundle.path().join("release-evidence.json");
    fs::write(
        bundle.path().join("parser-ratchet-release.json"),
        serde_json::to_string_pretty(&json!({
            "check": "parser-ratchet",
            "schema_version": "1",
            "event": "local",
            "profile": "release",
            "base_sha": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "head_sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "selected": true,
            "selection_reason": ["force-selected (scaffold only; measurements disabled)"],
            "verdict": "pass",
            "repro": { "command": "cargo xtask parser-ratchet run --profile release --force-selected" }
        }))?,
    )?;

    verify_bundle(bundle.path(), &receipt).failure();

    let summary: Value = serde_json::from_str(&fs::read_to_string(&receipt)?)?;
    assert!(summary_has_blocking_failure(&summary, "registry/schema validation failed"));
    Ok(())
}

#[test]
fn pr_profile_parser_ratchet_cannot_satisfy_release_bundle() -> Result<()> {
    let bundle = copy_complete_bundle()?;
    let receipt = bundle.path().join("release-evidence.json");
    mutate_parser_ratchet(bundle.path(), |value| value["profile"] = json!("pr"))?;

    verify_bundle(bundle.path(), &receipt).failure();

    let summary: Value = serde_json::from_str(&fs::read_to_string(&receipt)?)?;
    assert!(summary_has_blocking_failure(&summary, "field 'profile' must be 'release'"));
    Ok(())
}

#[test]
fn parser_ratchet_candidate_must_match_release_head() -> Result<()> {
    let bundle = copy_complete_bundle()?;
    let receipt = bundle.path().join("release-evidence.json");
    mutate_parser_ratchet(bundle.path(), |value| {
        value["candidate_sha"] = json!("cccccccccccccccccccccccccccccccccccccccc");
    })?;

    verify_bundle(bundle.path(), &receipt).failure();

    let summary: Value = serde_json::from_str(&fs::read_to_string(&receipt)?)?;
    assert!(summary_has_blocking_failure(&summary, "candidate_sha must match head_sha"));
    Ok(())
}

#[test]
fn incomplete_parser_ratchet_measurement_cannot_pass_release() -> Result<()> {
    let bundle = copy_complete_bundle()?;
    let receipt = bundle.path().join("release-evidence.json");
    mutate_parser_ratchet(bundle.path(), |value| {
        value["measurement_disposition"] = json!("incomplete");
        value["evidence_bundle"]["completed_evidence_count"] = json!(0);
    })?;

    verify_bundle(bundle.path(), &receipt).failure();

    let summary: Value = serde_json::from_str(&fs::read_to_string(&receipt)?)?;
    assert!(summary_has_blocking_failure(&summary, "measurement_disposition"));
    assert!(summary_has_blocking_failure(&summary, "completed_evidence_count"));
    Ok(())
}

#[test]
fn instrument_failure_cannot_be_serialized_as_release_pass() -> Result<()> {
    let bundle = copy_complete_bundle()?;
    let receipt = bundle.path().join("release-evidence.json");
    mutate_parser_ratchet(bundle.path(), |value| {
        value["instrument_state"] = json!("instrument_failed");
        value["evidence_bundle"]["producer_results"][0]["state"] = json!("instrument_failed");
    })?;

    verify_bundle(bundle.path(), &receipt).failure();

    let summary: Value = serde_json::from_str(&fs::read_to_string(&receipt)?)?;
    assert!(summary_has_blocking_failure(&summary, "instrument_state"));
    assert!(summary_has_blocking_failure(&summary, "must be complete"));
    Ok(())
}

#[test]
fn fixture_advisory_failure_produces_classified_warning() -> Result<()> {
    let temp = TempDir::new()?;
    let receipt = temp.path().join("release-evidence.json");
    let fixture = fixture_path("advisory-warning");

    verify_bundle(Path::new(&fixture), &receipt).success();

    let summary: Value = serde_json::from_str(&fs::read_to_string(&receipt)?)?;
    let warnings = summary
        .get("warnings")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("warnings missing"))?;

    assert!(warnings.iter().any(|item| {
        item.as_str().map(|v| v.contains("classified advisory warning")).unwrap_or(false)
    }));

    let entries = summary
        .get("receipts")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("receipts missing"))?;
    assert!(entries.iter().any(|entry| {
        entry.get("name").and_then(Value::as_str) == Some("advisory-status")
            && entry.get("classification").and_then(Value::as_str)
                == Some("failure-advisory-warning")
    }));

    Ok(())
}
