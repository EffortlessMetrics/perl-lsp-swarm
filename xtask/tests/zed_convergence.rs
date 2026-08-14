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

fn string<'a>(value: &'a Value, pointer: &str) -> Result<&'a str, Box<dyn Error>> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .ok_or_else(|| io::Error::other(format!("missing string at `{pointer}`")).into())
}

fn validate_required_files(root: &Path, manifest: &Value) -> Result<(), String> {
    let required = manifest
        .pointer("/required_files")
        .and_then(Value::as_array)
        .ok_or_else(|| "missing required_files".to_string())?;

    for entry in required {
        let relative =
            entry.as_str().ok_or_else(|| "required_files entry is not a string".to_string())?;
        if !root.join(relative).is_file() {
            return Err(format!("missing converged file `{relative}`"));
        }
    }

    Ok(())
}

#[test]
fn convergence_contains_every_static_authority_once() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let manifest = read_json(&root, ".ci/fixtures/zed-perl-upstream/convergence.v1.json")?;

    assert_eq!(string(&manifest, "/schema_version")?, "zed_perllsp_convergence.v1");
    assert_eq!(string(&manifest, "/status")?, "static_substrate_complete_execution_not_proven");
    validate_required_files(&root, &manifest).map_err(io::Error::other)?;

    assert_eq!(string(&manifest, "/mainline/candidate/result")?, "present_on_main");
    assert_eq!(string(&manifest, "/mainline/settings/result")?, "imported_static_contract");
    assert_eq!(string(&manifest, "/mainline/managed_assets/result")?, "imported_static_contract");
    assert_eq!(
        string(&manifest, "/mainline/dormant_defaults/result")?,
        "present_on_main_independent_packet"
    );
    assert_eq!(string(&manifest, "/mainline/submission/result")?, "imported_blocked_fan_in");

    Ok(())
}

#[test]
fn product_identity_and_launch_contract_survive_convergence() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let source =
        fs::read_to_string(root.join(".ci/fixtures/zed-perl-upstream/zed-perl/src/perl.rs"))?;

    for identity in ["perlnavigator-server", "perl-lsp", "perllsp"] {
        assert!(source.contains(identity), "missing provider identity `{identity}`");
    }
    assert!(source.contains("--stdio"));
    assert!(!source.contains("remove_old_downloads(\"perllsp-\""));

    Ok(())
}

#[test]
fn static_contracts_cannot_manufacture_execution_evidence() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let convergence = read_json(&root, ".ci/fixtures/zed-perl-upstream/convergence.v1.json")?;
    for cell in ["public_asset_execution", "actual_zed", "public_registry", "support_promotion"] {
        let pointer = format!("/claim_boundary/{cell}");
        assert_eq!(string(&convergence, &pointer)?, "not_proven");
    }

    let settings = read_json(&root, ".ci/fixtures/zed-perl-upstream/settings-contract.v1.json")?;
    for cell in [
        "workspace_configuration_request_response",
        "typed_setting_consumption",
        "precedence",
        "live_or_restart_semantics",
        "public_zed_support",
    ] {
        let pointer = format!("/claim_boundary/{cell}");
        assert_eq!(string(&settings, &pointer)?, "not_proven");
    }

    let assets = read_json(&root, ".ci/fixtures/zed-perl-upstream/managed-downloads.v1.json")?;
    for cell in [
        "archive_extraction",
        "perllsp_version_execution",
        "stdio_initialize_shutdown",
        "actual_zed_host",
    ] {
        let pointer = format!("/claim_boundary/{cell}");
        assert_eq!(string(&assets, &pointer)?, "not_proven");
    }

    let exact_source =
        read_json(&root, ".ci/fixtures/zed-perl-upstream/receipts/exact-source-template.json")?;
    let public =
        read_json(&root, ".ci/fixtures/zed-perl-upstream/receipts/public-registry-template.json")?;
    assert_eq!(string(&exact_source, "/result")?, "not_run");
    assert_eq!(string(&public, "/result")?, "not_run");

    let submission_text =
        fs::read_to_string(root.join(".ci/fixtures/zed-perl-upstream/submission/manifest.toml"))?;
    let submission: toml::Value = toml::from_str(&submission_text)?;
    assert_eq!(
        submission.get("status").and_then(toml::Value::as_str),
        Some("blocked_pending_fan_in")
    );
    assert_eq!(submission.get("ready").and_then(toml::Value::as_bool), Some(false));

    Ok(())
}

#[test]
fn missing_authority_is_a_hard_failure() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let mut manifest = read_json(&root, ".ci/fixtures/zed-perl-upstream/convergence.v1.json")?;
    let required = manifest
        .pointer_mut("/required_files")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| io::Error::other("missing mutable required_files"))?;
    required.push(Value::String(".ci/fixtures/zed-perl-upstream/does-not-exist".to_string()));

    assert!(validate_required_files(&root, &manifest).is_err());
    Ok(())
}
