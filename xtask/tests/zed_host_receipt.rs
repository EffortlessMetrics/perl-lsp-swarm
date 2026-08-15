#[path = "support/zed_host_compat.rs"]
mod zed_host_compat;

use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;
use zed_host_compat::{validate_pass, validate_schema};

fn repo_root() -> Result<PathBuf, Box<dyn Error>> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| io::Error::other("xtask manifest has no repository parent").into())
}

fn read_json(root: &Path, relative: &str) -> Result<Value, Box<dyn Error>> {
    let text = fs::read_to_string(root.join(relative))?;
    Ok(serde_json::from_str(&text)?)
}

#[test]
fn schema_and_template_are_valid_json_and_fail_closed() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let schema = read_json(&root, ".ci/schemas/zed-host-compat.v1.schema.json")?;
    let template =
        read_json(&root, ".ci/fixtures/zed-perl-upstream/receipts/exact-source-template.json")?;

    assert_eq!(
        schema.get("title").and_then(Value::as_str),
        Some("Zed perllsp host compatibility receipt"),
        "schema title must identify the Zed host receipt"
    );
    assert_eq!(
        template.get("result").and_then(Value::as_str),
        Some("not_run"),
        "exact-source template must remain not_run"
    );
    assert_eq!(
        template.get("evidence_stage").and_then(Value::as_str),
        Some("exact_source_dev_extension"),
        "exact-source template must use the development-extension stage"
    );
    validate_schema(&template).map_err(io::Error::other)?;
    assert!(
        validate_pass(&template, None).is_err(),
        "not_run template must fail closed under validate_pass"
    );
    Ok(())
}

fn sha256_fill(nibble: char) -> String {
    let mut value = String::with_capacity("sha256:".len() + 64);
    value.push_str("sha256:");
    for _ in 0..64 {
        value.push(nibble);
    }
    value
}

fn valid_exact_source_pass(mut receipt: Value) -> Value {
    receipt["result"] = Value::String("pass".to_string());
    receipt["observed_at"] = Value::String("2026-08-15T00:00:00Z".to_string());
    receipt["zed"]["version"] = Value::String("0.0.0-test".to_string());
    receipt["zed"]["channel"] = Value::String("stable".to_string());
    receipt["zed"]["build"] = Value::String("stable.1.abcdef0".to_string());
    receipt["extension"]["base_commit"] =
        Value::String("eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee".to_string());
    receipt["extension"]["candidate_commit"] =
        Value::String("ffffffffffffffffffffffffffffffffffffffff".to_string());
    receipt["extension"]["manifest_version"] = Value::String("0.5.0".to_string());
    receipt["extension"]["wasm_sha256"] = Value::String(sha256_fill('a'));
    receipt["extension"]["install_route"] = Value::String("dev_extension".to_string());
    receipt["perllsp"]["command"] = Value::String("<perllsp>".to_string());
    receipt["perllsp"]["arguments"] = serde_json::json!(["--stdio"]);
    receipt["perllsp"]["version"] = Value::String("0.0.0-test".to_string());
    receipt["perllsp"]["build_commit"] =
        Value::String("dddddddddddddddddddddddddddddddddddddddd".to_string());
    receipt["perllsp"]["binary_sha256"] = Value::String(sha256_fill('b'));
    receipt["perllsp"]["resolution_route"] = Value::String("binary_override".to_string());
    receipt["platform"] = serde_json::json!({
        "os": "linux",
        "version": "test",
        "architecture": "x86_64"
    });
    receipt["profile"] = serde_json::json!({
        "clean_profile": true,
        "prior_extension_absent": true,
        "prior_managed_cache_absent": true,
        "other_perl_servers_disabled": true
    });
    receipt["workspace"] = serde_json::json!({
        "fixture_id": "zed-test-v1",
        "fixture_sha256": sha256_fill('c'),
        "root_identity": "workspace"
    });
    receipt["configuration"]["settings_sha256"] = Value::String(sha256_fill('d'));
    receipt["configuration"]["workspace_configuration_observed"] = Value::Bool(true);
    receipt["artifacts"] = serde_json::json!({
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
        receipt["journey"][cell] = serde_json::json!({
            "result": "pass",
            "evidence": format!("observed {cell}")
        });
    }
    receipt["activation"]["pod"] = serde_json::json!({
        "result": "pass",
        "evidence": "POD stayed separate"
    });
    receipt
}

#[test]
fn schema_rejects_invalid_emitter_values() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let template =
        read_json(&root, ".ci/fixtures/zed-perl-upstream/receipts/exact-source-template.json")?;

    let mut wrong_route = template.clone();
    wrong_route["perllsp"]["resolution_route"] = Value::String("explicit_binary_path".to_string());
    assert_eq!(
        validate_schema(&wrong_route).expect_err("invalid resolution route"),
        "perllsp.resolution_route has invalid value `explicit_binary_path`"
    );

    let mut darwin = template.clone();
    darwin["platform"]["os"] = Value::String("darwin".to_string());
    assert_eq!(
        validate_schema(&darwin).expect_err("non-schema platform os"),
        "platform.os has invalid value `darwin`"
    );

    let mut limited = template.clone();
    limited["journey"]["hover"]["result"] = Value::String("limited".to_string());
    assert_eq!(
        validate_schema(&limited).expect_err("invalid journey cell result"),
        "journey.hover.result has invalid value `limited`"
    );

    let mut extra = template.clone();
    extra["unbound_evidence"] = Value::Bool(true);
    assert_eq!(
        validate_schema(&extra).expect_err("unbound top-level key"),
        "receipt contains unexpected key `unbound_evidence`"
    );

    let mut uppercase = template;
    uppercase["extension"]["base_commit"] = Value::String("A".repeat(40));
    assert_eq!(
        validate_schema(&uppercase).expect_err("uppercase commit identity"),
        "extension.base_commit must be a full 40-hex commit"
    );
    Ok(())
}

#[test]
fn false_green_mutations_are_rejected() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let template =
        read_json(&root, ".ci/fixtures/zed-perl-upstream/receipts/exact-source-template.json")?;
    let valid = valid_exact_source_pass(template);
    validate_schema(&valid).map_err(io::Error::other)?;
    validate_pass(&valid, None).map_err(io::Error::other)?;

    let mut wrong_provider = valid.clone();
    wrong_provider["perllsp"]["server_id"] = Value::String("perl-lsp".to_string());
    assert_eq!(
        validate_pass(&wrong_provider, None).expect_err("wrong provider"),
        "perllsp.server_id is not canonical"
    );

    let mut wrong_transport = valid.clone();
    wrong_transport["perllsp"]["arguments"] = serde_json::json!(["mcp", "--stdio"]);
    assert_eq!(
        validate_pass(&wrong_transport, None).expect_err("wrong transport"),
        "pass receipt violates required non-null schema fields"
    );

    let mut cross_stage = valid;
    cross_stage["evidence_stage"] = Value::String("public_registry_install".to_string());
    cross_stage["perllsp"]["resolution_route"] = Value::String("managed_download".to_string());
    cross_stage["public_subject"] = serde_json::json!({
        "relative_path": zed_host_compat::PUBLIC_SUBJECT_RELATIVE_PATH,
        "sha256": sha256_fill('e')
    });
    assert_eq!(
        validate_pass(&cross_stage, None).expect_err("cross stage"),
        "evidence stage `public_registry_install` cannot use install route `dev_extension`"
    );

    Ok(())
}
