//! Journey tests for the new Zed managed route contract (#8753).
//!
//! The contract proves infrastructure only: the checked-in fixture and the
//! `not_run` receipt template must validate, mutations that open a path
//! fallback or drop known-good recovery rows must fail closed, and a `pass`
//! candidate can never substitute a worktree/PATH route or omit recovery.

#[path = "support/zed_managed_route.rs"]
mod zed_managed_route;

#[path = "support/zed_managed_cli_tests.rs"]
mod cli_tests;

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
    missing["selection"]["resolution_route"] =
        Value::String(zed_managed_route::MANAGED_PUBLIC_ARTIFACT.to_string());
    missing["recovery_observations"] = Value::Object(serde_json::Map::new());
    assert!(zed_managed_route::validate_receipt(&missing, &contract).is_err());
    Ok(())
}

fn valid_pass(receipt: &mut Value) -> Result<(), Box<dyn Error>> {
    receipt["result"] = Value::String("pass".to_string());
    receipt["observed_at"] = Value::String("2026-08-14T00:00:00Z".to_string());
    receipt["contract"]["sha256"] = Value::String(format!("sha256:{}", "0".repeat(64)));
    for key in [
        "zed_version",
        "zed_build",
        "extension_version",
        "extension_candidate_commit",
        "fixture_id",
        "version",
        "target",
        "installed_path",
    ] {
        receipt["subject"][key] = Value::String(format!("{key}-fixture"));
    }
    receipt["subject"]["asset_sha256"] = Value::String(format!("sha256:{}", "1".repeat(64)));
    receipt["subject"]["binary_sha256"] = Value::String(format!("sha256:{}", "b".repeat(64)));
    for key in ["extension_wasm_sha256", "fixture_sha256"] {
        receipt["subject"][key] = Value::String(format!("sha256:{}", "a".repeat(64)));
    }
    receipt["upstream"] = serde_json::json!({
        "asset_receipt_sha256": format!("sha256:{}", "2".repeat(64)),
        "host_receipt_sha256": format!("sha256:{}", "3".repeat(64))
    });
    receipt["claim_boundary"]["real_zed_managed_route"] =
        Value::String("proven_for_exact_subject".to_string());
    receipt["selection"]["resolution_route"] =
        Value::String(zed_managed_route::MANAGED_PUBLIC_ARTIFACT.to_string());
    receipt["selection"]["selected_provider"] = Value::String("perllsp".to_string());
    receipt["selection"]["fallback_allowed"] = Value::Bool(false);
    receipt["selection"]["prior_managed_cache_absent"] = Value::Bool(true);
    receipt["selection"]["selected_subject_sha256"] = receipt["subject"]["binary_sha256"].clone();
    receipt["selection"]["restart_subject_sha256"] = receipt["subject"]["binary_sha256"].clone();
    receipt["selection"]["older_versions_preserved_until_launch"] = Value::Bool(true);
    for journey in zed_managed_route::REQUIRED_JOURNEYS {
        receipt["journeys"][journey] = Value::String("pass".to_string());
    }
    let observations = receipt["recovery_observations"]
        .as_object_mut()
        .ok_or_else(|| io::Error::other("template recovery observations must be an object"))?;
    for scenario in zed_managed_route::REQUIRED_RECOVERY_SCENARIOS {
        observations.insert(
            scenario.to_string(),
            serde_json::json!({
                "result": "pass",
                "known_good_before_sha256": format!("sha256:{}", "b".repeat(64)),
                "known_good_after_sha256": format!("sha256:{}", "b".repeat(64)),
                "restored_subject_sha256": format!("sha256:{}", "b".repeat(64)),
                "failed_candidate_identity": format!("synthetic-attempt-{scenario}"),
                "failed_candidate_selected": false,
                "fallback_server_id": null,
                "rejection_reason": format!("synthetic-{scenario}-rejection"),
                "restored_result": "pass",
                "evidence": "synthetic validator fixture only"
            }),
        );
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
fn authority_requires_all_nine_recovery_scenarios() -> Result<(), Box<dyn Error>> {
    let contract = read_json(&repo_root()?, CONTRACT)?;
    let rows = contract
        .get("recovery_scenarios")
        .and_then(Value::as_array)
        .ok_or_else(|| io::Error::other("missing recovery rows"))?;
    for required in ["extraction_failure", "launch_failure"] {
        if !rows.iter().any(|row| row.as_str() == Some(required)) {
            return Err(io::Error::other(format!("missing required scenario {required}")).into());
        }
    }
    Ok(())
}

#[test]
fn pass_requires_structured_recovery_evidence() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let contract = read_json(&root, CONTRACT)?;
    let mut receipt = read_json(&root, TEMPLATE)?;
    valid_pass(&mut receipt)?;
    let observations = receipt
        .get_mut("recovery_observations")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| io::Error::other("missing recovery observations"))?;
    for value in observations.values_mut() {
        *value = Value::String("pass".to_string());
    }
    // A self-attested pass string proves none of the per-failure subject facts.
    if zed_managed_route::validate_receipt(&receipt, &contract).is_ok() {
        return Err(io::Error::other("unstructured recovery pass accepted").into());
    }
    Ok(())
}

#[test]
fn every_recovery_fact_is_required_independently() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let contract = read_json(&root, CONTRACT)?;
    let mut baseline = read_json(&root, TEMPLATE)?;
    valid_pass(&mut baseline)?;
    zed_managed_route::validate_receipt(&baseline, &contract).map_err(io::Error::other)?;
    for scenario in zed_managed_route::REQUIRED_RECOVERY_SCENARIOS {
        for (field, replacement) in [
            ("result", serde_json::json!("not_run")),
            ("known_good_before_sha256", serde_json::json!(format!("sha256:{}", "f".repeat(64)))),
            ("known_good_after_sha256", serde_json::json!(format!("sha256:{}", "f".repeat(64)))),
            ("restored_subject_sha256", serde_json::json!(format!("sha256:{}", "f".repeat(64)))),
            ("failed_candidate_identity", Value::Null),
            ("failed_candidate_selected", Value::Bool(true)),
            ("fallback_server_id", serde_json::json!("other-provider")),
            ("rejection_reason", serde_json::json!("")),
            ("restored_result", serde_json::json!("not_proven")),
            ("evidence", Value::Null),
        ] {
            let mut mutated = baseline.clone();
            mutated["recovery_observations"][scenario][field] = replacement;
            if zed_managed_route::validate_receipt(&mutated, &contract).is_ok() {
                return Err(
                    io::Error::other(format!("accepted {scenario}.{field} mutation")).into()
                );
            }
        }
    }
    Ok(())
}

#[test]
fn template_cannot_claim_new_subject_or_upstream_observations() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let contract = read_json(&root, CONTRACT)?;
    let template = read_json(&root, TEMPLATE)?;
    for pointer in [
        "/subject/binary_sha256",
        "/subject/target",
        "/subject/fixture_sha256",
        "/subject/extension_candidate_commit",
        "/upstream/asset_receipt_sha256",
        "/upstream/host_receipt_sha256",
    ] {
        let mut mutated = template.clone();
        *mutated.pointer_mut(pointer).ok_or_else(|| io::Error::other(pointer.to_string()))? =
            serde_json::json!("observed");
        if zed_managed_route::validate_receipt(&mutated, &contract).is_ok() {
            return Err(io::Error::other(format!("not_run accepted {pointer}")).into());
        }
    }
    Ok(())
}

#[test]
fn template_preserves_every_unobserved_recovery_slot() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let contract = read_json(&root, CONTRACT)?;
    let template = read_json(&root, TEMPLATE)?;
    for scenario in zed_managed_route::REQUIRED_RECOVERY_SCENARIOS {
        let mut missing = template.clone();
        missing
            .get_mut("recovery_observations")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| io::Error::other("missing recovery slots"))?
            .remove(scenario);
        if zed_managed_route::validate_receipt(&missing, &contract).is_ok() {
            return Err(io::Error::other(format!("template dropped {scenario}")).into());
        }
        let mut observed = template.clone();
        observed["recovery_observations"][scenario]["evidence"] =
            serde_json::json!("claimed observation");
        if zed_managed_route::validate_receipt(&observed, &contract).is_ok() {
            return Err(io::Error::other(format!("template claims {scenario} evidence")).into());
        }
    }
    Ok(())
}
