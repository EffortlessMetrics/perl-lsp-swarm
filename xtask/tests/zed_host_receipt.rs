#[path = "support/zed_host_compat.rs"]
mod zed_host_compat;

use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;
use zed_host_compat::validate_pass;

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
    assert!(
        validate_pass(&template, None).is_err(),
        "not_run template must fail closed under validate_pass"
    );
    Ok(())
}

#[test]
fn false_green_mutations_are_rejected() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let template =
        read_json(&root, ".ci/fixtures/zed-perl-upstream/receipts/exact-source-template.json")?;

    let mut empty_pass = template.clone();
    empty_pass["result"] = Value::String("pass".to_string());
    assert_eq!(
        validate_pass(&empty_pass, None).expect_err("empty pass"),
        "exact Zed host identity is missing"
    );

    let mut wrong_provider = empty_pass.clone();
    wrong_provider["perllsp"]["server_id"] = Value::String("perl-lsp".to_string());
    assert_eq!(
        validate_pass(&wrong_provider, None).expect_err("wrong provider"),
        "exact perllsp process identity is missing"
    );

    let mut wrong_transport = empty_pass.clone();
    wrong_transport["perllsp"]["arguments"] = serde_json::json!(["mcp", "--stdio"]);
    assert_eq!(
        validate_pass(&wrong_transport, None).expect_err("wrong transport"),
        "exact perllsp process identity is missing"
    );

    let mut cross_stage = empty_pass;
    cross_stage["evidence_stage"] = Value::String("public_registry_install".to_string());
    cross_stage["extension"]["install_route"] = Value::String("dev_extension".to_string());
    assert_eq!(
        validate_pass(&cross_stage, None).expect_err("cross stage"),
        "evidence stage `public_registry_install` cannot use install route `dev_extension`"
    );

    Ok(())
}
