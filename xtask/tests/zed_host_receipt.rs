use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;

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

fn nonempty_string(value: &Value, pointer: &str) -> bool {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .is_some_and(|text| !text.trim().is_empty())
}

fn exact_sha256(value: &Value, pointer: &str) -> bool {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .is_some_and(|text| {
            text.len() == "sha256:".len() + 64
                && text.starts_with("sha256:")
                && text["sha256:".len()..]
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
        })
}

fn cell_result<'a>(receipt: &'a Value, group: &str, cell: &str) -> Option<&'a str> {
    receipt
        .pointer(&format!("/{group}/{cell}/result"))
        .and_then(Value::as_str)
}

fn validate_pass(receipt: &Value) -> Result<(), String> {
    if receipt.get("schema_version").and_then(Value::as_str) != Some("zed_host_compat.v1") {
        return Err("wrong receipt schema".to_string());
    }
    if receipt.get("result").and_then(Value::as_str) != Some("pass") {
        return Err("receipt is not a pass candidate".to_string());
    }

    let stage = receipt
        .get("evidence_stage")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing evidence stage".to_string())?;
    let install_route = receipt
        .pointer("/extension/install_route")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing extension install route".to_string())?;
    match (stage, install_route) {
        ("exact_source_dev_extension", "dev_extension")
        | ("public_registry_install", "official_registry") => {}
        _ => {
            return Err(format!(
                "evidence stage `{stage}` cannot use install route `{install_route}`"
            ));
        }
    }

    if receipt.pointer("/zed/product").and_then(Value::as_str) != Some("Zed")
        || !nonempty_string(receipt, "/zed/version")
        || !nonempty_string(receipt, "/zed/channel")
        || !nonempty_string(receipt, "/zed/build")
    {
        return Err("exact Zed host identity is missing".to_string());
    }
    if receipt.pointer("/extension/repository").and_then(Value::as_str)
        != Some("tree-sitter-perl/zed-perl")
        || !nonempty_string(receipt, "/extension/base_commit")
        || !nonempty_string(receipt, "/extension/candidate_commit")
        || !exact_sha256(receipt, "/extension/wasm_sha256")
    {
        return Err("exact extension identity is missing".to_string());
    }
    if receipt.pointer("/perllsp/server_id").and_then(Value::as_str) != Some("perllsp")
        || !nonempty_string(receipt, "/perllsp/command")
        || receipt.pointer("/perllsp/arguments") != Some(&serde_json::json!(["--stdio"]))
        || !nonempty_string(receipt, "/perllsp/version")
        || !nonempty_string(receipt, "/perllsp/build_commit")
        || !exact_sha256(receipt, "/perllsp/binary_sha256")
        || !nonempty_string(receipt, "/perllsp/resolution_route")
    {
        return Err("exact perllsp process identity is missing".to_string());
    }
    if receipt.pointer("/profile/clean_profile").and_then(Value::as_bool) != Some(true)
        || receipt
            .pointer("/profile/other_perl_servers_disabled")
            .and_then(Value::as_bool)
            != Some(true)
    {
        return Err("clean-profile provider isolation is missing".to_string());
    }
    if !nonempty_string(receipt, "/workspace/fixture_id")
        || !exact_sha256(receipt, "/workspace/fixture_sha256")
        || !nonempty_string(receipt, "/workspace/root_identity")
    {
        return Err("workspace fixture identity is missing".to_string());
    }
    if receipt
        .pointer("/configuration/workspace_configuration_observed")
        .and_then(Value::as_bool)
        != Some(true)
    {
        return Err("workspace/configuration was not observed".to_string());
    }

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
        if cell_result(receipt, "journey", cell) != Some("pass")
            || !nonempty_string(receipt, &format!("/journey/{cell}/evidence"))
        {
            return Err(format!("required journey cell `{cell}` is not proven"));
        }
    }

    if cell_result(receipt, "activation", "pod") != Some("pass")
        || !nonempty_string(receipt, "/activation/pod/evidence")
    {
        return Err("POD separation is not proven".to_string());
    }
    if !nonempty_string(receipt, "/artifacts/zed_log")
        || !nonempty_string(receipt, "/artifacts/language_server_log")
        || !nonempty_string(receipt, "/artifacts/process_inventory")
        || receipt.pointer("/artifacts/redacted").and_then(Value::as_bool) != Some(true)
    {
        return Err("bounded redacted evidence artifacts are missing".to_string());
    }

    Ok(())
}

#[test]
fn schema_and_template_are_valid_json_and_fail_closed() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let schema = read_json(&root, ".ci/schemas/zed-host-compat.v1.schema.json")?;
    let template = read_json(
        &root,
        ".ci/fixtures/zed-perl-upstream/receipts/exact-source-template.json",
    )?;

    assert_eq!(
        schema.get("title").and_then(Value::as_str),
        Some("Zed perllsp host compatibility receipt")
    );
    assert_eq!(template.get("result").and_then(Value::as_str), Some("not_run"));
    assert_eq!(
        template.get("evidence_stage").and_then(Value::as_str),
        Some("exact_source_dev_extension")
    );
    assert!(validate_pass(&template).is_err());
    Ok(())
}

#[test]
fn false_green_mutations_are_rejected() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let template = read_json(
        &root,
        ".ci/fixtures/zed-perl-upstream/receipts/exact-source-template.json",
    )?;

    let mut empty_pass = template.clone();
    empty_pass["result"] = Value::String("pass".to_string());
    assert!(validate_pass(&empty_pass).is_err());

    let mut wrong_provider = empty_pass.clone();
    wrong_provider["perllsp"]["server_id"] = Value::String("perl-lsp".to_string());
    assert!(validate_pass(&wrong_provider).is_err());

    let mut wrong_transport = empty_pass.clone();
    wrong_transport["perllsp"]["arguments"] = serde_json::json!(["mcp", "--stdio"]);
    assert!(validate_pass(&wrong_transport).is_err());

    let mut cross_stage = empty_pass;
    cross_stage["evidence_stage"] = Value::String("public_registry_install".to_string());
    cross_stage["extension"]["install_route"] = Value::String("dev_extension".to_string());
    assert!(
        validate_pass(&cross_stage)
            .is_err_and(|message| message.contains("cannot use install route"))
    );

    Ok(())
}
