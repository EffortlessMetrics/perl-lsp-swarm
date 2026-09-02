//! Journey tests for the new Zed managed route contract (#8753).
//!
//! The contract proves infrastructure only: the checked-in fixture and the
//! `not_run` receipt template must validate, mutations that open a path
//! fallback or drop known-good recovery rows must fail closed, and a `pass`
//! candidate can never substitute a worktree/PATH route or omit recovery.

#[path = "support/zed_managed_route.rs"]
mod zed_managed_route;

use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;

const CONTRACT: &str = ".ci/fixtures/zed-perl-upstream/managed-route.v1.json";
const TEMPLATE: &str = ".ci/fixtures/zed-perl-upstream/receipts/managed-route-template.json";

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
        receipt.pointer("/claim_boundary/official_registry").and_then(Value::as_str),
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
fn pass_candidate_cannot_substitute_path_or_omit_known_good_recovery() -> Result<(), Box<dyn Error>>
{
    let root = repo_root()?;
    let contract = read_json(&root, CONTRACT)?;
    let mut receipt = read_json(&root, TEMPLATE)?;
    valid_pass(&mut receipt)?;
    assert!(zed_managed_route::validate_receipt(&receipt, &contract).is_ok());
    receipt["selection"]["resolution_route"] = Value::String("worktree_path".to_string());
    assert!(zed_managed_route::validate_receipt(&receipt, &contract).is_err());

    let mut missing = receipt.clone();
    missing["recovery_observations"] = Value::Object(serde_json::Map::new());
    assert!(zed_managed_route::validate_receipt(&missing, &contract).is_err());
    Ok(())
}

fn valid_pass(receipt: &mut Value) -> Result<(), Box<dyn Error>> {
    receipt["result"] = Value::String("pass".to_string());
    receipt["observed_at"] = Value::String("2026-08-14T00:00:00Z".to_string());
    receipt["contract"]["sha256"] = Value::String(format!("sha256:{}", "0".repeat(64)));
    for key in ["zed_version", "extension_version", "fixture_id"] {
        receipt["subject"][key] = Value::String(format!("{key}-fixture"));
    }
    receipt["subject"]["asset_sha256"] = Value::String(format!("sha256:{}", "1".repeat(64)));
    receipt["claim_boundary"]["real_zed_managed_route"] =
        Value::String("proven_for_exact_subject".to_string());
    receipt["selection"]["resolution_route"] =
        Value::String(zed_managed_route::MANAGED_PUBLIC_ARTIFACT.to_string());
    receipt["selection"]["selected_provider"] = Value::String("perllsp".to_string());
    receipt["selection"]["fallback_allowed"] = Value::Bool(false);
    receipt["selection"]["prior_managed_cache_absent"] = Value::Bool(true);
    receipt["selection"]["selected_subject_sha256"] = receipt["subject"]["asset_sha256"].clone();
    receipt["selection"]["restart_subject_sha256"] = receipt["subject"]["asset_sha256"].clone();
    receipt["selection"]["older_versions_preserved_until_launch"] = Value::Bool(true);
    for journey in zed_managed_route::REQUIRED_JOURNEYS {
        receipt["journeys"][journey] = Value::String("pass".to_string());
    }
    let observations = receipt["recovery_observations"]
        .as_object_mut()
        .ok_or_else(|| io::Error::other("template recovery observations must be an object"))?;
    for scenario in zed_managed_route::REQUIRED_RECOVERY_SCENARIOS {
        observations.insert(scenario.to_string(), Value::String("pass".to_string()));
    }
    Ok(())
}

#[test]
fn receipt_contract_rejects_malformed_identity_and_claims() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let contract = read_json(&root, CONTRACT)?;
    let template = read_json(&root, TEMPLATE)?;
    for malformed in [
        Value::Bool(true),
        Value::Number(1.into()),
        Value::Null,
        Value::Array(vec![]),
        Value::Object(serde_json::Map::new()),
    ] {
        let mut receipt = template.clone();
        receipt["contract"]["schema_version"] = malformed;
        assert!(zed_managed_route::validate_receipt(&receipt, &contract).is_err());
    }
    let mut receipt = template.clone();
    receipt["receipt"] = Value::String("wrong".to_string());
    assert!(zed_managed_route::validate_receipt(&receipt, &contract).is_err());
    let mut receipt = template;
    receipt["claim_boundary"]["official_registry"] = Value::String("proven".to_string());
    assert!(zed_managed_route::validate_receipt(&receipt, &contract).is_err());
    Ok(())
}

#[test]
fn contract_requires_all_failure_invariants_and_revision() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let contract = read_json(&root, CONTRACT)?;
    for key in [
        "provider_fallback_forbidden",
        "path_route_forbidden",
        "worktree_route_forbidden",
        "binary_override_forbidden",
        "partial_download_install_forbidden",
        "unsafe_archive_member_forbidden",
        "checksum_mismatch_install_forbidden",
    ] {
        let mut mutated = contract.clone();
        mutated["failure_invariants"][key] = Value::Bool(false);
        assert!(zed_managed_route::validate_contract(&mutated).is_err(), "{key}");
    }
    let mut revision = contract.clone();
    revision["revision"] = Value::Number(2.into());
    assert!(zed_managed_route::validate_contract(&revision).is_err());
    let mut unknown = contract;
    unknown["failure_invariants"]["future_invariant"] = Value::Bool(true);
    assert!(zed_managed_route::validate_contract(&unknown).is_err());
    Ok(())
}

#[test]
fn receipt_requires_typed_observed_at_and_fallback_authority() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let contract = read_json(&root, CONTRACT)?;
    let mut baseline = read_json(&root, TEMPLATE)?;
    valid_pass(&mut baseline)?;
    for observed_at in [
        Value::Bool(true),
        Value::Number(1.into()),
        Value::String(String::new()),
        Value::String("not-a-date".to_string()),
    ] {
        let mut receipt = baseline.clone();
        receipt["observed_at"] = observed_at;
        assert!(zed_managed_route::validate_receipt(&receipt, &contract).is_err());
    }
    let mut fallback = baseline;
    fallback["selection"]["fallback_allowed"] = Value::Bool(true);
    assert!(zed_managed_route::validate_receipt(&fallback, &contract).is_err());
    Ok(())
}

#[test]
fn contract_and_receipt_authority_is_behaviorally_bound() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let contract = read_json(&root, CONTRACT)?;
    assert!(zed_managed_route::validate_contract(&contract).is_ok());
    let mut non_null = contract.clone();
    non_null["selection"]["fallback_server_id"] = Value::String("fallback".to_string());
    assert!(zed_managed_route::validate_contract(&non_null).is_err());
    let mut non_null_number = contract;
    non_null_number["selection"]["fallback_server_id"] = Value::Number(1.into());
    assert!(zed_managed_route::validate_contract(&non_null_number).is_err());

    let contract = read_json(&root, CONTRACT)?;
    let mut receipt = read_json(&root, TEMPLATE)?;
    valid_pass(&mut receipt)?;
    assert!(zed_managed_route::validate_receipt(&receipt, &contract).is_ok());
    receipt["selection"]["fallback_server_id"] = Value::String("fallback".to_string());
    assert!(zed_managed_route::validate_receipt(&receipt, &contract).is_err());
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
