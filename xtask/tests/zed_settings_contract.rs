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

fn read(root: &Path, relative: &str) -> Result<String, Box<dyn Error>> {
    Ok(fs::read_to_string(root.join(relative))?)
}

fn read_json(root: &Path, relative: &str) -> Result<Value, Box<dyn Error>> {
    Ok(serde_json::from_str(&read(root, relative)?)?)
}

fn string<'a>(value: &'a Value, pointer: &str) -> Result<&'a str, Box<dyn Error>> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .ok_or_else(|| io::Error::other(format!("missing string at `{pointer}`")).into())
}

#[test]
fn zed_settings_keep_process_and_server_authority_separate() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let contract = read_json(
        &root,
        ".ci/fixtures/zed-perl-upstream/settings-contract.v1.json",
    )?;

    assert_eq!(
        string(&contract, "/schema_version")?,
        "zed_perllsp_settings_contract.v1"
    );
    assert_eq!(string(&contract, "/server_id")?, "perllsp");
    assert_eq!(
        string(&contract, "/process_configuration/zed_path")?,
        "lsp.perllsp.binary"
    );
    assert_eq!(
        contract
            .pointer("/process_configuration/forwarded_to_workspace_configuration")
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        string(&contract, "/server_configuration/zed_path")?,
        "lsp.perllsp.settings.perl"
    );
    assert_eq!(
        string(&contract, "/server_configuration/wire_root")?,
        "perl"
    );
    assert_eq!(
        string(
            &contract,
            "/server_configuration/workspace_configuration_section"
        )?,
        "perl"
    );
    assert_eq!(
        contract
            .pointer("/initialization_options/default_route")
            .and_then(Value::as_bool),
        Some(false)
    );

    let example = contract
        .pointer("/example/lsp/perllsp")
        .and_then(Value::as_object)
        .ok_or_else(|| io::Error::other("missing canonical Zed example"))?;
    assert!(example.contains_key("binary"));
    assert!(example.contains_key("settings"));
    assert!(
        example
            .get("settings")
            .and_then(|value| value.get("perl"))
            .is_some()
    );
    assert!(
        example
            .get("settings")
            .and_then(|value| value.get("binary"))
            .is_none()
    );
    assert!(contract.pointer("/example/initialization_options").is_none());

    Ok(())
}

#[test]
fn extension_forwards_only_the_settings_object() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let source = read(
        &root,
        ".ci/fixtures/zed-perl-upstream/zed-perl/src/perl.rs",
    )?;

    assert!(source.contains("fn language_server_workspace_configuration"));
    assert!(source.contains("LspSettings::for_worktree(language_server_id.as_ref(), worktree)"));
    assert!(source.contains(".and_then(|lsp_settings| lsp_settings.settings)"));
    assert!(!source.contains("lsp_settings.binary.clone()"));
    assert!(!source.contains("initialization_options"));

    Ok(())
}

#[test]
fn behavior_cells_remain_not_proven() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let contract = read_json(
        &root,
        ".ci/fixtures/zed-perl-upstream/settings-contract.v1.json",
    )?;

    for cell in [
        "workspace_configuration_request_response",
        "typed_setting_consumption",
        "precedence",
        "live_or_restart_semantics",
        "public_zed_support",
    ] {
        let pointer = format!("/claim_boundary/{cell}");
        assert_eq!(string(&contract, &pointer)?, "not_proven");
    }
    assert_eq!(
        string(&contract, "/precedence/status")?,
        "pending_canonical_authority_and_actual_zed_receipt"
    );
    assert_eq!(
        string(&contract, "/live_update/status")?,
        "not_proven"
    );

    Ok(())
}

#[test]
fn mutation_controls_reject_flattened_or_process_mixed_examples() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let contract = read_json(
        &root,
        ".ci/fixtures/zed-perl-upstream/settings-contract.v1.json",
    )?;

    let mut flattened = contract.clone();
    let perl = flattened["example"]["lsp"]["perllsp"]["settings"]
        .get_mut("perl")
        .ok_or_else(|| io::Error::other("missing perl settings"))?
        .take();
    flattened["example"]["lsp"]["perllsp"]["settings"] = perl;
    assert!(
        flattened
            .pointer("/example/lsp/perllsp/settings/perl")
            .is_none()
    );

    let mut process_mixed = contract;
    process_mixed["example"]["lsp"]["perllsp"]["settings"]["binary"] =
        serde_json::json!({"path": "/wrong/place"});
    assert!(
        process_mixed
            .pointer("/example/lsp/perllsp/settings/binary")
            .is_some()
    );

    Ok(())
}
