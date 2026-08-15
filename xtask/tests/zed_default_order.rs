#[path = "support/zed_default_order.rs"]
mod zed_default_order;

use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;
use serde_json::json;

const CONTRACT: &str = ".ci/fixtures/zed-perl-upstream/default-order.v1.json";
const TEMPLATE: &str = ".ci/fixtures/zed-perl-upstream/receipts/default-order-template.json";

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

fn sha256_fill(nibble: char) -> String {
    let mut value = String::with_capacity("sha256:".len() + 64);
    value.push_str("sha256:");
    for _ in 0..64 {
        value.push(nibble);
    }
    value
}

fn canonical_host_pass(root: &Path) -> Result<Value, Box<dyn Error>> {
    let mut host =
        read_json(root, ".ci/fixtures/zed-perl-upstream/receipts/exact-source-template.json")?;
    host["result"] = Value::String("pass".to_string());
    host["observed_at"] = Value::String("2026-08-15T00:00:00Z".to_string());
    host["zed"]["version"] = Value::String("0.0.0-test".to_string());
    host["zed"]["channel"] = Value::String("stable".to_string());
    host["zed"]["build"] = Value::String("zed-build-test".to_string());
    host["extension"]["candidate_commit"] =
        Value::String("ffffffffffffffffffffffffffffffffffffffff".to_string());
    host["extension"]["wasm_sha256"] = Value::String(sha256_fill('a'));
    host["perllsp"]["command"] = Value::String("perllsp".to_string());
    host["perllsp"]["version"] = Value::String("0.0.0-test".to_string());
    host["perllsp"]["build_commit"] =
        Value::String("eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee".to_string());
    host["perllsp"]["binary_sha256"] = Value::String(sha256_fill('b'));
    host["perllsp"]["resolution_route"] = Value::String("explicit_path".to_string());
    host["profile"]["clean_profile"] = Value::Bool(true);
    host["profile"]["other_perl_servers_disabled"] = Value::Bool(true);
    host["workspace"]["fixture_id"] = Value::String("zed-default-order-test".to_string());
    host["workspace"]["fixture_sha256"] = Value::String(sha256_fill('c'));
    host["workspace"]["root_identity"] = Value::String("test-root".to_string());
    host["configuration"]["workspace_configuration_observed"] = Value::Bool(true);
    host["activation"]["pod"]["result"] = Value::String("pass".to_string());
    host["activation"]["pod"]["evidence"] = Value::String("synthetic test input".to_string());
    for cell in [
        "manifest_discovery",
        "perl_attachment",
        "initialize",
        "workspace_root",
        "diagnostics",
        "hover",
        "definition",
        "references",
        "post_edit_freshness",
        "restart",
        "shutdown",
    ] {
        host["journey"][cell]["result"] = Value::String("pass".to_string());
        host["journey"][cell]["evidence"] = Value::String("synthetic test input".to_string());
    }
    host["artifacts"]["zed_log"] = Value::String("synthetic-zed-log".to_string());
    host["artifacts"]["language_server_log"] = Value::String("synthetic-server-log".to_string());
    host["artifacts"]["process_inventory"] =
        Value::String("synthetic-process-inventory".to_string());
    host["artifacts"]["redacted"] = Value::Bool(true);
    Ok(host)
}

fn matrix_row_mut<'a>(receipt: &'a mut Value, id: &str) -> Result<&'a mut Value, Box<dyn Error>> {
    receipt
        .pointer_mut("/matrix")
        .and_then(Value::as_array_mut)
        .and_then(|rows| {
            rows.iter_mut().find(|row| row.get("id").and_then(Value::as_str) == Some(id))
        })
        .ok_or_else(|| format!("missing test matrix row {id}").into())
}

fn passing_default_order_receipt(root: &Path) -> Result<(Value, Value), Box<dyn Error>> {
    let contract = read_json(root, CONTRACT)?;
    let mut receipt = read_json(root, TEMPLATE)?;
    let host = canonical_host_pass(root)?;
    receipt["result"] = Value::String("pass".to_string());
    receipt["observed_at"] = Value::String("2026-08-15T00:00:00Z".to_string());
    receipt["contract"]["sha256"] = Value::String(sha256_fill('d'));
    receipt["claim_boundary"]["host_compatibility"] =
        Value::String("proven_for_exact_matrix".to_string());
    receipt["claim_boundary"]["publication_order"] =
        Value::String("zed_defaults_first_safe".to_string());
    for id in [
        "current_defaults_public_extension",
        "candidate_defaults_public_extension",
        "current_defaults_candidate_extension",
        "candidate_defaults_candidate_extension",
    ] {
        let row = matrix_row_mut(&mut receipt, id)?;
        row["result"] = Value::String("pass".to_string());
        row["zed_identity_sha256"] = Value::String(sha256_fill('e'));
        row["defaults_sha256"] = Value::String(sha256_fill('f'));
        row["extension_sha256"] = Value::String(sha256_fill('0'));
        row["profile_sha256"] = Value::String(sha256_fill('1'));
        row["process_inventory_sha256"] = Value::String(sha256_fill('2'));
        row["clean_profile_quiet"] = Value::Bool(true);
        row["started_server_ids"] = json!(["perlnavigator-server"]);
        row["failed_server_ids"] = json!([]);
        row["evidence"] = Value::String("synthetic test input".to_string());
        row["host_receipt"] = host.clone();
    }
    for row in receipt
        .pointer_mut("/selection_cases")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| "missing selection cases in test receipt")?
    {
        row["result"] = Value::String("pass".to_string());
        row["evidence"] = Value::String("synthetic test input".to_string());
    }
    let cases = receipt
        .pointer_mut("/selection_cases")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| "missing selection cases in test receipt")?;
    for row in cases {
        match row.get("id").and_then(Value::as_str) {
            Some("default_only") => row["started_server_ids"] = json!(["perlnavigator-server"]),
            Some("select_perllsp") => {
                row["started_server_ids"] = json!(["perllsp"]);
                row["command"] = Value::String("perllsp --stdio".to_string());
            }
            Some("select_perl_lsp") => {
                row["started_server_ids"] = json!(["perl-lsp"]);
                row["command"] = Value::String("perl-lsp --stdio".to_string());
            }
            Some("deliberate_multi_server") => {
                row["started_server_ids"] = json!(["perl-lsp", "perllsp"]);
            }
            Some("missing_selected_server") => {
                row["started_server_ids"] = json!([]);
                row["failed_server_ids"] = json!(["perllsp"]);
            }
            Some("ellipsis_preserves_user_registration") => {
                row["started_server_ids"] = json!(["user-provider"]);
            }
            _ => {}
        }
    }
    receipt["ruling"]["result"] = Value::String("pass".to_string());
    receipt["ruling"]["value"] = Value::String("zed_defaults_first_safe".to_string());
    receipt["ruling"]["unsafe_interval_avoided"] =
        Value::String("synthetic test interval".to_string());
    receipt["ruling"]["maintainer_sequence"] = json!(["synthetic test sequence"]);
    receipt["ruling"]["invalidation"] = json!(["synthetic test invalidation"]);
    receipt["ruling"]["evidence"] = Value::String("synthetic test input".to_string());
    Ok((contract, receipt))
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
        receipt.pointer("/claim_boundary/publication_order").and_then(Value::as_str),
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
    order["candidate_order"] =
        serde_json::json!(["perllsp", "!perl-lsp", "!perlnavigator-server", "..."]);
    assert!(zed_default_order::validate_contract(&order).is_err());

    let mut ruling = read_json(&root, CONTRACT)?;
    ruling["claim_boundary"]["publication_order"] =
        Value::String("zed_defaults_first_safe".to_string());
    assert!(zed_default_order::validate_contract(&ruling).is_err());

    let mut tuple = read_json(&root, CONTRACT)?;
    tuple["matrix"][1]["defaults"] = Value::String("current".to_string());
    assert!(zed_default_order::validate_contract(&tuple).is_err());
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
fn pass_candidate_rejects_contradictory_quietness() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let (contract, mut receipt) = passing_default_order_receipt(&root)?;
    let row = matrix_row_mut(&mut receipt, "candidate_defaults_public_extension")?;
    row["failed_server_ids"] = json!(["perllsp"]);
    assert!(zed_default_order::validate_receipt(&receipt, &contract).is_err());

    let (contract, mut receipt) = passing_default_order_receipt(&root)?;
    let row = matrix_row_mut(&mut receipt, "candidate_defaults_public_extension")?;
    row["started_server_ids"] = json!(["perlnavigator-server", "perllsp"]);
    assert!(zed_default_order::validate_receipt(&receipt, &contract).is_err());
    Ok(())
}

#[test]
fn pass_candidate_rejects_fabricated_digest_only_evidence() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let (contract, mut receipt) = passing_default_order_receipt(&root)?;
    let row = matrix_row_mut(&mut receipt, "candidate_defaults_public_extension")?;
    row["host_receipt"] = json!({
        "schema_version": "zed_host_compat.v1",
        "result": "pass"
    });
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
    assert!(source.contains("zed_host_compat"));
    assert!(source.contains("host_receipt"));
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
