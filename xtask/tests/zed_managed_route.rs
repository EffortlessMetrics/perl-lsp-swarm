#[path = "support/zed_managed_route.rs"]
mod zed_managed_route;

use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;

const CONTRACT: &str = ".ci/fixtures/zed-perl-upstream/managed-route.v1.json";
const TEMPLATE: &str =
    ".ci/fixtures/zed-perl-upstream/receipts/managed-route-template.json";

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
fn checked_contract_and_not_run_template_validate() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let contract = read_json(&root, CONTRACT)?;
    let receipt = read_json(&root, TEMPLATE)?;
    zed_managed_route::validate_contract(&contract).map_err(io::Error::other)?;
    zed_managed_route::validate_receipt(&receipt, &contract).map_err(io::Error::other)?;
    assert_eq!(receipt.get("result").and_then(Value::as_str), Some("not_run"));
    assert_eq!(
        receipt
            .pointer("/claim_boundary/official_registry")
            .and_then(Value::as_str),
        Some("not_proven")
    );
    Ok(())
}

#[test]
fn contract_mutations_reject_path_fallback_and_recovery_gaps() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let mut route = read_json(&root, CONTRACT)?;
    route["resolution_route"] = Value::String("worktree_path".to_string());
    assert!(zed_managed_route::validate_contract(&route).is_err());

    let mut fallback = read_json(&root, CONTRACT)?;
    fallback["failure_invariants"]["provider_fallback_forbidden"] = Value::Bool(false);
    assert!(zed_managed_route::validate_contract(&fallback).is_err());

    let mut missing = read_json(&root, CONTRACT)?;
    missing["recovery_scenarios"]
        .as_array_mut()
        .ok_or_else(|| io::Error::other("recovery_scenarios is not an array"))?
        .pop();
    assert!(zed_managed_route::validate_contract(&missing).is_err());
    Ok(())
}

#[test]
fn pass_candidate_cannot_substitute_path_or_omit_known_good_recovery() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let contract = read_json(&root, CONTRACT)?;
    let mut receipt = read_json(&root, TEMPLATE)?;
    receipt["result"] = Value::String("pass".to_string());
    receipt["observed_at"] = Value::String("2026-08-14T00:00:00Z".to_string());
    receipt["contract"]["sha256"] = Value::String(format!("sha256:{}", "0".repeat(64)));
    receipt["claim_boundary"]["real_zed_managed_route"] =
        Value::String("proven_for_exact_subject".to_string());
    receipt["selection"]["resolution_route"] = Value::String("worktree_path".to_string());
    assert!(zed_managed_route::validate_receipt(&receipt, &contract).is_err());
    Ok(())
}

#[test]
fn source_guards_preserve_exact_asset_cache_and_no_fallback_boundaries(
) -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let source = fs::read_to_string(root.join("xtask/tests/support/zed_managed_route.rs"))?;
    assert!(source.contains("managed_public_artifact"));
    assert!(source.contains("perllsp --stdio"));
    assert!(source.contains("prior_managed_cache_absent"));
    assert!(source.contains("selected_subject_sha256"));
    assert!(source.contains("restart_subject_sha256"));
    assert!(source.contains("fallback_server_id"));
    assert!(source.contains("older_versions_preserved_until_launch"));
    assert!(source.contains("new Zed managed route"));
    Ok(())
}

#[test]
fn validator_cli_reuses_the_support_authority() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let source = fs::read_to_string(root.join("xtask/src/bin/validate-zed-managed-route.rs"))?;
    assert!(source.contains("support/zed_managed_route.rs"));
    assert!(source.contains("validate_contract"));
    assert!(source.contains("validate_receipt"));
    assert!(source.contains("contract digest mismatch"));
    Ok(())
}
