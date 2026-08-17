#[path = "support/zed_settings_behavior.rs"]
mod zed_settings_behavior;

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::{Digest, Sha256};

const CONTRACT: &str = ".ci/fixtures/zed-perl-upstream/settings-behavior.v1.json";
const SCHEMA: &str = "schemas/perllsp-settings.schema.json";
const TEMPLATE: &str = ".ci/fixtures/zed-perl-upstream/receipts/settings-behavior-template.json";
const HOST_TEMPLATE: &str = ".ci/fixtures/zed-perl-upstream/receipts/exact-source-template.json";

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

fn unavailable_loader(_relative: &str) -> Result<Vec<u8>, String> {
    Err("host receipts are unavailable in fixture tests".to_string())
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
    zed_settings_behavior::validate_receipt(&receipt, &contract, &unavailable_loader)
        .map_err(io::Error::other)?;
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
    assert!(
        zed_settings_behavior::validate_contract(&process_leak, &schema).is_err(),
        "process-owned `perl.binary.*` keys must not count as server settings"
    );

    let mut missing_key = read_json(&root, CONTRACT)?;
    missing_key["probes"][0]["schema_pointer"] =
        Value::String("/properties/perl/properties/notARealSetting".to_string());
    assert!(
        zed_settings_behavior::validate_contract(&missing_key, &schema).is_err(),
        "probes pointing outside the canonical schema must be rejected"
    );

    let mut wrong_type = read_json(&root, CONTRACT)?;
    wrong_type["probes"][0]["expected_type"] = Value::String("integer".to_string());
    assert!(
        zed_settings_behavior::validate_contract(&wrong_type, &schema).is_err(),
        "probes whose expected type drifts from the schema node must be rejected"
    );
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
    let error = zed_settings_behavior::validate_receipt(&receipt, &contract, &unavailable_loader)
        .expect_err("a pass-shaped receipt without host rows must not validate");
    assert_eq!(error, "wrong host receipt role population", "unexpected rejection cause: {error}");
    Ok(())
}

#[test]
fn validator_cli_rejects_drifted_contract_digest() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let mut receipt = read_json(&root, TEMPLATE)?;
    receipt["result"] = Value::String("pass".to_string());
    receipt["observed_at"] = Value::String("2026-08-13T23:30:00Z".to_string());
    receipt["contract"]["sha256"] = Value::String(format!("sha256:{}", "0".repeat(64)));
    receipt["claim_boundary"]["settings_behavior"] =
        Value::String("proven_for_exact_subject".to_string());
    let receipt_path = std::env::temp_dir()
        .join(format!("zed-settings-behavior-drifted-receipt-{}.json", std::process::id()));
    fs::write(&receipt_path, serde_json::to_vec(&receipt)?)?;

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_validate-zed-settings-behavior"))
        .arg(root.join(CONTRACT))
        .arg(root.join(SCHEMA))
        .arg(&receipt_path)
        .output()
        .map_err(|error| io::Error::other(format!("failed to run validator CLI: {error}")))?;
    let _status = output.status;
    let _ = fs::remove_file(&receipt_path);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "validator CLI must reject a receipt whose contract digest does not bind the contract bytes"
    );
    assert!(
        stderr.contains("receipt contract digest does not match contract bytes"),
        "validator CLI reported an unexpected failure: {stderr}"
    );
    let _ = fs::remove_file(&receipt_path);
    Ok(())
}

fn sha256_of(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut encoded = String::from("sha256:");
    for byte in digest {
        encoded.push_str(&format!("{byte:02x}"));
    }
    encoded
}

fn sha256_fill(nibble: char) -> String {
    let mut value = String::from("sha256:");
    for _ in 0..64 {
        value.push(nibble);
    }
    value
}

fn pass_shaped_settings_receipt(root: &Path) -> Result<Value, Box<dyn Error>> {
    let mut receipt = read_json(root, TEMPLATE)?;
    receipt["result"] = Value::String("pass".to_string());
    receipt["observed_at"] = Value::String("2026-08-13T23:30:00Z".to_string());
    receipt["contract"]["sha256"] = Value::String(sha256_fill('0'));
    receipt["claim_boundary"]["settings_behavior"] =
        Value::String("proven_for_exact_subject".to_string());
    Ok(receipt)
}

fn host_row(role: &str, relative_path: &str, receipt_sha256: &str) -> Value {
    host_row_with_identity(role, relative_path, receipt_sha256, &sha256_fill('9'))
}

fn host_row_with_identity(
    role: &str,
    relative_path: &str,
    receipt_sha256: &str,
    host_identity_sha256: &str,
) -> Value {
    serde_json::json!({
        "role": role,
        "relative_path": relative_path,
        "schema_version": "zed_host_compat.v1",
        "evidence_stage": "exact_source_dev_extension",
        "result": "pass",
        "receipt_sha256": receipt_sha256,
        "settings_sha256": sha256_fill('d'),
        "host_identity_sha256": host_identity_sha256,
    })
}

fn exact_source_pass_child(root: &Path) -> Result<Value, Box<dyn Error>> {
    let mut child = read_json(root, HOST_TEMPLATE)?;
    child["result"] = Value::String("pass".to_string());
    child["observed_at"] = Value::String("2026-08-15T00:00:00Z".to_string());
    child["zed"]["version"] = Value::String("0.0.0-test".to_string());
    child["zed"]["channel"] = Value::String("stable".to_string());
    child["zed"]["build"] = Value::String("stable.1.abcdef0".to_string());
    child["extension"]["base_commit"] =
        Value::String("eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee".to_string());
    child["extension"]["candidate_commit"] =
        Value::String("ffffffffffffffffffffffffffffffffffffffff".to_string());
    child["extension"]["manifest_version"] = Value::String("0.5.0".to_string());
    child["extension"]["wasm_sha256"] = Value::String(sha256_fill('a'));
    child["extension"]["install_route"] = Value::String("dev_extension".to_string());
    child["perllsp"]["command"] = Value::String("<perllsp>".to_string());
    child["perllsp"]["arguments"] = serde_json::json!(["--stdio"]);
    child["perllsp"]["version"] = Value::String("0.0.0-test".to_string());
    child["perllsp"]["build_commit"] =
        Value::String("dddddddddddddddddddddddddddddddddddddddd".to_string());
    child["perllsp"]["binary_sha256"] = Value::String(sha256_fill('b'));
    child["perllsp"]["resolution_route"] = Value::String("binary_override".to_string());
    child["platform"] = serde_json::json!({
        "os": "linux",
        "version": "test",
        "architecture": "x86_64"
    });
    child["profile"] = serde_json::json!({
        "clean_profile": true,
        "prior_extension_absent": true,
        "prior_managed_cache_absent": true,
        "other_perl_servers_disabled": true
    });
    child["workspace"] = serde_json::json!({
        "fixture_id": "zed-test-v1",
        "fixture_sha256": sha256_fill('c'),
        "root_identity": "workspace"
    });
    child["configuration"]["settings_sha256"] = Value::String(sha256_fill('d'));
    child["configuration"]["workspace_configuration_observed"] = Value::Bool(true);
    child["artifacts"] = serde_json::json!({
        "zed_log": "artifacts/zed.log#sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "language_server_log": "artifacts/lsp.log#sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "process_inventory": "artifacts/process.json#sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        "redacted": true
    });
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
        child["journey"][cell] = serde_json::json!({
            "result": "pass",
            "evidence": format!("observed {cell}")
        });
    }
    child["activation"]["pod"] = serde_json::json!({
        "result": "pass",
        "evidence": "POD stayed separate"
    });
    Ok(child)
}

#[test]
fn fabricated_host_summaries_cannot_promote_behavior() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let contract = read_json(&root, CONTRACT)?;
    let mut receipt = pass_shaped_settings_receipt(&root)?;
    receipt["host_receipts"] = serde_json::json!([
        host_row("project_only", "host/project-only.json", &sha256_fill('1')),
        host_row("zed_override", "host/zed-override.json", &sha256_fill('2')),
        host_row("zed_override_removed", "host/zed-override-removed.json", &sha256_fill('3')),
        host_row("live_edit", "host/live-edit.json", &sha256_fill('4')),
    ]);

    let error = zed_settings_behavior::validate_receipt(&receipt, &contract, &unavailable_loader)
        .expect_err("fabricated summaries must not promote behavior");
    assert!(
        error.contains("could not be loaded"),
        "aggregate must load every referenced host receipt: {error}"
    );

    let child = exact_source_pass_child(&root)?;
    let child_bytes = serde_json::to_vec(&child)?;
    let mut digest_mismatch = receipt.clone();
    digest_mismatch["host_receipts"] =
        serde_json::json!([host_row("project_only", "host/project-only.json", &sha256_fill('1'))]);
    let error =
        zed_settings_behavior::validate_receipt(&digest_mismatch, &contract, &|_relative: &str| {
            Ok(child_bytes.clone())
        })
        .expect_err("declared digests must bind the loaded bytes");
    assert!(
        error.contains("does not match the referenced receipt bytes"),
        "receipt digest must be derived from bytes: {error}"
    );

    let mut not_a_pass = receipt.clone();
    let template_bytes = fs::read(root.join(HOST_TEMPLATE))?;
    let template_digest = sha256_of(&template_bytes);
    not_a_pass["host_receipts"] =
        serde_json::json!([host_row("project_only", "host/project-only.json", &template_digest)]);
    let error =
        zed_settings_behavior::validate_receipt(&not_a_pass, &contract, &|_relative: &str| {
            Ok(template_bytes.clone())
        })
        .expect_err("referenced receipts must pass exact-source validation");
    assert!(
        error.contains("failed exact-source validation"),
        "aggregate must delegate to the host receipt authority: {error}"
    );

    let child_digest = sha256_of(&child_bytes);
    let mut other_settings = child.clone();
    other_settings["configuration"]["settings_sha256"] = Value::String(sha256_fill('e'));
    let other_settings_bytes = serde_json::to_vec(&other_settings)?;
    let other_settings_digest = sha256_of(&other_settings_bytes);
    let mut settings_drift = receipt.clone();
    settings_drift["host_receipts"] = serde_json::json!([host_row(
        "project_only",
        "host/project-only.json",
        &other_settings_digest
    )]);
    let error =
        zed_settings_behavior::validate_receipt(&settings_drift, &contract, &|_relative: &str| {
            Ok(other_settings_bytes.clone())
        })
        .expect_err("settings digests must come from the loaded receipt");
    assert!(
        error.contains("settings_sha256 is not the loaded receipt's settings digest"),
        "settings digest must be read from loaded bytes: {error}"
    );

    let mut identity_drift = receipt.clone();
    identity_drift["host_receipts"] =
        serde_json::json!([host_row("project_only", "host/project-only.json", &child_digest)]);
    let error =
        zed_settings_behavior::validate_receipt(&identity_drift, &contract, &|_relative: &str| {
            Ok(child_bytes.clone())
        })
        .expect_err("host identity must be derived from loaded bytes");
    assert!(
        error.contains("host_identity_sha256 is not derived"),
        "host identity must be derived from loaded bytes: {error}"
    );

    let derived_identity = zed_settings_behavior::derived_host_identity(&child)
        .ok_or_else(|| io::Error::other("test child receipt lacks identity fields"))?;

    let mut relabeled = receipt.clone();
    relabeled["host_receipts"] = serde_json::json!([
        host_row_with_identity("project_only", "host/same.json", &child_digest, &derived_identity),
        host_row_with_identity("zed_override", "host/same.json", &child_digest, &derived_identity),
    ]);
    let error =
        zed_settings_behavior::validate_receipt(&relabeled, &contract, &|_relative: &str| {
            Ok(child_bytes.clone())
        })
        .expect_err("one receipt cannot serve two roles");
    assert!(
        error.contains("reuses another role's host receipt path"),
        "roles must reference distinct receipts: {error}"
    );

    let mut relabeled_bytes = receipt.clone();
    relabeled_bytes["host_receipts"] = serde_json::json!([
        host_row_with_identity("project_only", "host/a.json", &child_digest, &derived_identity),
        host_row_with_identity("zed_override", "host/b.json", &child_digest, &derived_identity),
    ]);
    let error =
        zed_settings_behavior::validate_receipt(&relabeled_bytes, &contract, &|_relative: &str| {
            Ok(child_bytes.clone())
        })
        .expect_err("identical bytes cannot serve two roles");
    assert!(
        error.contains("relabels another role's host receipt bytes"),
        "roles must bind distinct receipt bytes: {error}"
    );
    Ok(())
}

#[test]
fn non_enum_probe_values_are_validated_against_the_schema() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let schema = read_json(&root, SCHEMA)?;

    let mut boolean_as_string = read_json(&root, CONTRACT)?;
    boolean_as_string["probes"][0]["project_value"] = Value::String("yes".to_string());
    let error = zed_settings_behavior::validate_contract(&boolean_as_string, &schema)
        .expect_err("boolean probes must reject string values");
    assert_eq!(error, "probe `boolean_inlay_hints` uses a value the canonical schema rejects");

    let mut integer_as_bool = read_json(&root, CONTRACT)?;
    integer_as_bool["probes"][2]["zed_value"] = Value::Bool(true);
    assert!(
        zed_settings_behavior::validate_contract(&integer_as_bool, &schema).is_err(),
        "integer probes must reject boolean values"
    );

    let mut integer_below_minimum = read_json(&root, CONTRACT)?;
    integer_below_minimum["probes"][2]["zed_value"] = serde_json::json!(-1);
    assert!(
        zed_settings_behavior::validate_contract(&integer_below_minimum, &schema).is_err(),
        "integer probes must enforce the schema minimum (0)"
    );

    let mut integer_fractional = read_json(&root, CONTRACT)?;
    integer_fractional["probes"][2]["project_value"] = serde_json::json!(40.5);
    assert!(
        zed_settings_behavior::validate_contract(&integer_fractional, &schema).is_err(),
        "integer probes must reject fractional values"
    );

    let mut array_with_wrong_items = read_json(&root, CONTRACT)?;
    array_with_wrong_items["probes"][3]["zed_value"] = serde_json::json!([42]);
    let error = zed_settings_behavior::validate_contract(&array_with_wrong_items, &schema)
        .expect_err("array probes must validate item types");
    assert_eq!(error, "probe `path_list_include_paths` uses a value the canonical schema rejects");
    Ok(())
}

fn synthetic_pass_receipt(
    root: &Path,
) -> Result<(Value, Value, BTreeMap<String, Vec<u8>>), Box<dyn Error>> {
    let contract = read_json(root, CONTRACT)?;
    let mut receipt = pass_shaped_settings_receipt(root)?;

    let role_settings = [
        ("project_only", 'd'),
        ("zed_override", 'e'),
        ("zed_override_removed", 'd'),
        ("live_edit", 'e'),
    ];
    let mut host_bytes: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut rows = Vec::new();
    for (role, settings_nibble) in role_settings {
        let mut child = exact_source_pass_child(root)?;
        child["observed_at"] = Value::String(format!("2026-08-15T00:0{}:00Z", host_bytes.len()));
        child["configuration"]["settings_sha256"] = Value::String(sha256_fill(settings_nibble));
        let bytes = serde_json::to_vec(&child)?;
        let identity = zed_settings_behavior::derived_host_identity(&child)
            .ok_or_else(|| io::Error::other("synthetic child receipt lacks identity fields"))?;
        let relative_path = format!("host/{role}.json");
        let mut row = host_row_with_identity(role, &relative_path, &sha256_of(&bytes), &identity);
        row["settings_sha256"] = Value::String(sha256_fill(settings_nibble));
        rows.push(row);
        host_bytes.insert(relative_path, bytes);
    }
    receipt["host_receipts"] = Value::Array(rows);

    let observed = contract
        .get("probes")
        .and_then(Value::as_array)
        .ok_or_else(|| io::Error::other("contract lacks probes"))?;
    let mut probe_rows = Vec::new();
    for probe in observed {
        let row = serde_json::json!({
            "id": probe.get("id"),
            "key": probe.get("key"),
            "result": "pass",
            "project_observed": probe.get("project_value"),
            "zed_override_observed": probe.get("zed_value"),
            "restored_observed": probe.get("project_value"),
            "effect_before": "before",
            "effect_override": "override",
            "effect_restored": "before",
            "evidence": "observed reversible effect"
        });
        probe_rows.push(row);
    }
    receipt["probes"] = Value::Array(probe_rows);
    receipt["precedence"] = serde_json::json!({
        "result": "pass",
        "sequence": contract.get("precedence_sequence"),
        "project_only_effect": "project effect",
        "zed_override_effect": "override effect",
        "restored_project_effect": "project effect",
        "evidence": "observed precedence sequence"
    });
    receipt["restart"] = serde_json::json!({
        "result": "pass",
        "setting": "perl.inlayHints.enabled",
        "from": true,
        "to": false,
        "disposition": "live_configuration",
        "configuration_notification_observed": true,
        "server_pid_before": 4242,
        "server_pid_after": 4242,
        "effect_before": "enabled",
        "effect_after": "disabled",
        "evidence": "observed live configuration change"
    });
    Ok((contract, receipt, host_bytes))
}

#[test]
fn synthetic_pass_receipt_validates_and_single_field_drift_is_named() -> Result<(), Box<dyn Error>>
{
    let root = repo_root()?;
    let (contract, receipt, host_bytes) = synthetic_pass_receipt(&root)?;
    let loader = |relative: &str| -> Result<Vec<u8>, String> {
        host_bytes
            .get(relative)
            .cloned()
            .ok_or_else(|| format!("unknown host receipt `{relative}`"))
    };
    zed_settings_behavior::validate_receipt(&receipt, &contract, &loader).map_err(|error| {
        io::Error::other(format!("synthetic pass receipt was rejected: {error}"))
    })?;

    let mut probe_drift = receipt.clone();
    probe_drift["probes"][0]["zed_override_observed"] = serde_json::json!("drifted");
    let error = zed_settings_behavior::validate_receipt(&probe_drift, &contract, &loader)
        .expect_err("probe observation drift must be named");
    assert_eq!(
        error, "probe `boolean_inlay_hints` lacks reversible behavior proof",
        "unexpected rejection cause: {error}"
    );

    let mut precedence_drift = receipt.clone();
    precedence_drift["precedence"]["zed_override_effect"] =
        Value::String("project effect".to_string());
    let error = zed_settings_behavior::validate_receipt(&precedence_drift, &contract, &loader)
        .expect_err("precedence drift must be named");
    assert_eq!(error, "precedence is not reversibly proven", "unexpected rejection cause: {error}");

    let mut restart_drift = receipt.clone();
    restart_drift["restart"]["disposition"] = Value::String("manual_restart".to_string());
    let error = zed_settings_behavior::validate_receipt(&restart_drift, &contract, &loader)
        .expect_err("restart disposition drift must be named");
    assert_eq!(
        error, "passing receipt has no valid live/restart disposition",
        "unexpected rejection cause: {error}"
    );

    let mut duplicate_role = receipt.clone();
    duplicate_role["host_receipts"][3]["role"] = Value::String("zed_override_removed".to_string());
    let error = zed_settings_behavior::validate_receipt(&duplicate_role, &contract, &loader)
        .expect_err("duplicate roles must be named");
    assert_eq!(
        error, "duplicate host role `zed_override_removed`",
        "unexpected rejection cause: {error}"
    );
    Ok(())
}
