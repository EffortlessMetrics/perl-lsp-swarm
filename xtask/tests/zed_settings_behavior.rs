#[path = "support/zed_settings_behavior.rs"]
mod zed_settings_behavior;

use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;

const CONTRACT: &str = ".ci/fixtures/zed-perl-upstream/settings-behavior.v1.json";
const SCHEMA: &str = "schemas/perllsp-settings.schema.json";
const TEMPLATE: &str = ".ci/fixtures/zed-perl-upstream/receipts/settings-behavior-template.json";

fn repo_root() -> Result<PathBuf, Box<dyn Error>> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| io::Error::other("xtask manifest has no repository parent").into())
}

fn read_json(root: &Path, relative: &str) -> Result<Value, Box<dyn Error>> {
    Ok(serde_json::from_slice(&fs::read(root.join(relative))?)?)
}

#[test]
fn checked_contract_matches_canonical_schema() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let contract = read_json(&root, CONTRACT)?;
    let schema = read_json(&root, SCHEMA)?;
    zed_settings_behavior::validate_contract(&contract, &schema).map_err(io::Error::other)?;
    Ok(())
}

#[test]
fn not_run_template_is_valid_and_cannot_promote_support() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let contract = read_json(&root, CONTRACT)?;
    let receipt = read_json(&root, TEMPLATE)?;
    zed_settings_behavior::validate_receipt(&receipt, &contract).map_err(io::Error::other)?;
    assert_eq!(receipt.get("result").and_then(Value::as_str), Some("not_run"));
    assert_eq!(
        receipt.pointer("/claim_boundary/full_zed_support").and_then(Value::as_str),
        Some("not_proven")
    );
    assert_eq!(
        receipt.pointer("/claim_boundary/public_registry").and_then(Value::as_str),
        Some("not_proven")
    );
    Ok(())
}

#[test]
fn contract_mutations_reject_process_fields_and_schema_drift() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let schema = read_json(&root, SCHEMA)?;
    let mut process_leak = read_json(&root, CONTRACT)?;
    process_leak["probes"][0]["key"] = Value::String("perl.binary.path".to_string());
    assert!(zed_settings_behavior::validate_contract(&process_leak, &schema).is_err());

    let mut missing_key = read_json(&root, CONTRACT)?;
    missing_key["probes"][0]["schema_pointer"] =
        Value::String("/properties/perl/properties/notARealSetting".to_string());
    assert!(zed_settings_behavior::validate_contract(&missing_key, &schema).is_err());

    let mut wrong_type = read_json(&root, CONTRACT)?;
    wrong_type["probes"][0]["expected_type"] = Value::String("integer".to_string());
    assert!(zed_settings_behavior::validate_contract(&wrong_type, &schema).is_err());
    Ok(())
}

#[test]
fn pass_candidate_requires_reversible_effects_and_exact_host_roles() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let contract = read_json(&root, CONTRACT)?;
    let mut receipt = read_json(&root, TEMPLATE)?;
    receipt["result"] = Value::String("pass".to_string());
    receipt["observed_at"] = Value::String("2026-08-13T23:30:00Z".to_string());
    receipt["contract"]["sha256"] = Value::String(format!("sha256:{}", "0".repeat(64)));
    receipt["claim_boundary"]["settings_behavior"] =
        Value::String("proven_for_exact_subject".to_string());
    assert!(zed_settings_behavior::validate_receipt(&receipt, &contract).is_err());
    Ok(())
}

#[test]
fn validator_cli_reuses_the_support_authority() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let source = fs::read_to_string(root.join("xtask/src/bin/validate-zed-settings-behavior.rs"))?;
    assert!(source.contains("support/zed_settings_behavior.rs"));
    assert!(source.contains("validate_contract"));
    assert!(source.contains("validate_receipt"));
    assert!(source.contains("contract digest does not match"));
    Ok(())
}
