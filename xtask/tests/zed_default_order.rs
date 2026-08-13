#[path = "support/zed_default_order.rs"]
mod zed_default_order;

use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;

const CONTRACT: &str = ".ci/fixtures/zed-perl-upstream/default-order.v1.json";
const TEMPLATE: &str =
    ".ci/fixtures/zed-perl-upstream/receipts/default-order-template.json";

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
    zed_default_order::validate_contract(&contract).map_err(io::Error::other)?;
    zed_default_order::validate_receipt(&receipt, &contract).map_err(io::Error::other)?;
    assert_eq!(receipt.get("result").and_then(Value::as_str), Some("not_run"));
    assert_eq!(
        receipt
            .pointer("/claim_boundary/publication_order")
            .and_then(Value::as_str),
        Some("unresolved")
    );
    Ok(())
}

#[test]
fn contract_rejects_aliasing_order_drift_and_static_ruling() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let mut alias = read_json(&root, CONTRACT)?;
    alias["provider_identities"]["effortlessmetrics"] = Value::String("perl-lsp".to_string());
    assert!(zed_default_order::validate_contract(&alias).is_err());

    let mut order = read_json(&root, CONTRACT)?;
    order["candidate_order"] = serde_json::json!([
        "perllsp",
        "!perl-lsp",
        "!perlnavigator-server",
        "..."
    ]);
    assert!(zed_default_order::validate_contract(&order).is_err());

    let mut ruling = read_json(&root, CONTRACT)?;
    ruling["claim_boundary"]["publication_order"] =
        Value::String("zed_defaults_first_safe".to_string());
    assert!(zed_default_order::validate_contract(&ruling).is_err());
    Ok(())
}

#[test]
fn pass_candidate_cannot_omit_matrix_or_selection_evidence() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let contract = read_json(&root, CONTRACT)?;
    let mut receipt = read_json(&root, TEMPLATE)?;
    receipt["result"] = Value::String("pass".to_string());
    receipt["observed_at"] = Value::String("2026-08-13T23:45:00Z".to_string());
    receipt["contract"]["sha256"] = Value::String(format!("sha256:{}", "0".repeat(64)));
    receipt["claim_boundary"]["host_compatibility"] =
        Value::String("proven_for_exact_matrix".to_string());
    assert!(zed_default_order::validate_receipt(&receipt, &contract).is_err());
    Ok(())
}

#[test]
fn source_guards_preserve_final_quiet_state_and_derived_ruling() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let source = fs::read_to_string(root.join("xtask/tests/support/zed_default_order.rs"))?;
    assert!(source.contains("candidate_defaults_candidate_extension"));
    assert!(source.contains("perlnavigator-server"));
    assert!(source.contains("perllsp --stdio"));
    assert!(source.contains("zed_defaults_first_safe"));
    assert!(source.contains("extension_first_required"));
    assert!(source.contains("coordinated_release_required"));
    assert!(source.contains("ellipsis did not preserve"));
    assert!(source.contains("missing_selected_server"));
    Ok(())
}

#[test]
fn validator_cli_reuses_the_support_authority() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let source = fs::read_to_string(root.join("xtask/src/bin/validate-zed-default-order.rs"))?;
    assert!(source.contains("support/zed_default_order.rs"));
    assert!(source.contains("validate_contract"));
    assert!(source.contains("validate_receipt"));
    assert!(source.contains("contract digest mismatch"));
    Ok(())
}
