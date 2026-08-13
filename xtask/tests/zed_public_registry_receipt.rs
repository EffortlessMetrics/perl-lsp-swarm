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
    Ok(serde_json::from_str(&fs::read_to_string(root.join(relative))?)?)
}

#[test]
fn public_template_uses_the_official_registry_stage() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let receipt =
        read_json(&root, ".ci/fixtures/zed-perl-upstream/receipts/public-registry-template.json")?;
    assert_eq!(
        receipt.get("evidence_stage").and_then(Value::as_str),
        Some("public_registry_install")
    );
    assert_eq!(receipt.get("result").and_then(Value::as_str), Some("not_run"));
    assert_eq!(
        receipt.pointer("/extension/install_route").and_then(Value::as_str),
        Some("official_registry")
    );
    assert_eq!(receipt.pointer("/perllsp/server_id").and_then(Value::as_str), Some("perllsp"));
    assert_eq!(receipt.pointer("/perllsp/arguments"), Some(&serde_json::json!(["--stdio"])));
    Ok(())
}

#[test]
fn public_subject_cannot_invent_publication_or_promotion() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let subject = read_json(
        &root,
        ".ci/fixtures/zed-perl-upstream/receipts/public-registry-subject.v1.json",
    )?;
    assert_eq!(subject.get("status").and_then(Value::as_str), Some("blocked_pending_publication"));
    assert_eq!(subject.pointer("/registry/extension_id").and_then(Value::as_str), Some("perl"));
    assert_eq!(
        subject.pointer("/registry/submodule_path").and_then(Value::as_str),
        Some("extensions/perl")
    );
    for cell in ["registry_row", "managed_download_row", "path_row", "documentation_projection"] {
        assert_eq!(
            subject.pointer(&format!("/promotion/{cell}")).and_then(Value::as_str),
            Some("not_proven")
        );
    }
    Ok(())
}

#[test]
fn managed_and_path_routes_remain_distinct() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let subject = read_json(
        &root,
        ".ci/fixtures/zed-perl-upstream/receipts/public-registry-subject.v1.json",
    )?;
    assert_eq!(
        subject.pointer("/promotion/managed_download_row").and_then(Value::as_str),
        Some("not_proven")
    );
    assert_eq!(subject.pointer("/promotion/path_row").and_then(Value::as_str), Some("not_proven"));
    assert!(subject.pointer("/perllsp_asset/asset_sha256").is_some());
    assert!(subject.pointer("/clean_profile/path_override_absent").is_some());
    Ok(())
}
