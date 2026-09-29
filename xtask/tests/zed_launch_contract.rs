//! Cross-artifact contract for the Zed perllsp launch projection (#11304).
//!
//! The staged extension classifies user argv against a checked versioned
//! projection instead of a hand-copied denylist. This test keeps the
//! projection, the canonical parser authority it names, the managed-download
//! identity, and the host-receipt argv pin coherent as one claim.

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;

const PROJECTION_RELATIVE_PATH: &str = ".ci/fixtures/zed-perl-upstream/launch-contract.v1.json";
const EXTENSION_SOURCE_RELATIVE_PATH: &str = ".ci/fixtures/zed-perl-upstream/zed-perl/src/perl.rs";
const MANAGED_DOWNLOADS_RELATIVE_PATH: &str =
    ".ci/fixtures/zed-perl-upstream/managed-downloads.v1.json";
const RECEIPT_SCHEMA_RELATIVE_PATH: &str = ".ci/schemas/zed-host-compat.v1.schema.json";

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

fn string_array<'a>(value: &'a Value, pointer: &str) -> Result<Vec<&'a str>, Box<dyn Error>> {
    let entries = value
        .pointer(pointer)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("missing array at `{pointer}`"))?;
    let mut strings = Vec::with_capacity(entries.len());
    for entry in entries {
        let text = entry.as_str().ok_or_else(|| format!("non-string entry at `{pointer}`"))?;
        strings.push(text);
    }
    Ok(strings)
}

fn validate_projection(projection: &Value, root: &Path) -> Result<(), String> {
    if projection.get("schema_version").and_then(Value::as_str)
        != Some("zed_perllsp_launch_contract.v1")
    {
        return Err("unexpected launch projection schema version".to_string());
    }
    if projection.get("product_protocol").and_then(Value::as_str) != Some("lsp") {
        return Err("launch projection must bind product protocol lsp".to_string());
    }
    if projection.get("fail_closed_default").and_then(Value::as_bool) != Some(true) {
        return Err("launch projection must fail closed by default".to_string());
    }
    let transport = projection
        .get("required_transport_flag")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing required_transport_flag".to_string())?;
    if transport != "--stdio" {
        return Err("managed Zed launches require exact --stdio transport".to_string());
    }

    let source_path = projection
        .pointer("/authority/source_path")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing authority.source_path".to_string())?;
    let authority_source = fs::read_to_string(root.join(source_path))
        .map_err(|error| format!("canonical CLI authority `{source_path}` unreadable: {error}"))?;
    if !authority_source.contains("pub struct LspArgs") {
        return Err(format!("authority source `{source_path}` no longer declares LspArgs"));
    }

    let admitted = projection
        .get("admitted_arguments")
        .and_then(Value::as_array)
        .ok_or_else(|| "missing admitted_arguments".to_string())?;
    if admitted.is_empty() {
        return Err("admitted_arguments must not be empty".to_string());
    }

    let mut classified: BTreeSet<String> = BTreeSet::from([transport.to_string()]);
    for row in admitted {
        let flag = row
            .get("flag")
            .and_then(Value::as_str)
            .ok_or_else(|| "admitted row lacks flag".to_string())?;
        if !flag.starts_with("--") {
            return Err(format!("admitted flag `{flag}` must be spelled with CLI dashes"));
        }
        if !classified.insert(flag.to_string()) {
            return Err(format!("admitted flag `{flag}` duplicates another classified token"));
        }
        if row.get("value").and_then(Value::as_bool).is_none() {
            return Err(format!("admitted flag `{flag}` lacks its value kind"));
        }
    }

    for pointer in ["/rejected_flags", "/rejected_short_flags", "/rejected_exact_tokens"] {
        let rejected = string_array(projection, pointer).map_err(|error| error.to_string())?;
        if rejected.is_empty() {
            return Err(format!("`{pointer}` must not be empty"));
        }
        for flag in rejected {
            if !classified.insert(flag.to_string()) {
                return Err(format!(
                    "`{pointer}` entry `{flag}` duplicates another classified token"
                ));
            }
        }
    }

    for mcp_token in ["mcp", "--mcp"] {
        if !classified.contains(mcp_token) {
            return Err(format!(
                "launch projection must explicitly reject the MCP route token `{mcp_token}`"
            ));
        }
    }

    Ok(())
}

#[test]
fn launch_projection_is_present_and_authoritative() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let projection = read_json(&root, PROJECTION_RELATIVE_PATH)?;
    validate_projection(&projection, &root).map_err(io::Error::other)?;
    Ok(())
}

#[test]
fn effective_command_is_pinned_across_artifacts() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let projection = read_json(&root, PROJECTION_RELATIVE_PATH)?;
    let managed = read_json(&root, MANAGED_DOWNLOADS_RELATIVE_PATH)?;
    let receipt_schema = read_json(&root, RECEIPT_SCHEMA_RELATIVE_PATH)?;

    let transport = projection
        .get("required_transport_flag")
        .and_then(Value::as_str)
        .ok_or_else(|| io::Error::other("launch projection lacks required_transport_flag"))?;

    let managed_arguments = string_array(&managed, "/identity/arguments")?;
    if managed_arguments != [transport] {
        return Err(io::Error::other(
            "managed-download identity arguments drifted from the launch projection transport",
        )
        .into());
    }

    if !contains_stdio_const_pin(&receipt_schema) {
        return Err(io::Error::other(
            "host receipt schema lost its exact [\"--stdio\"] pass-shape argv pin",
        )
        .into());
    }

    Ok(())
}

/// Search the receipt schema for a `{"const": ["--stdio"]}` node so a pass
/// receipt can never bind non-LSP argv while relabeling it as LSP.
fn contains_stdio_const_pin(value: &Value) -> bool {
    match value {
        Value::Object(map) => {
            map.get("const").is_some_and(|pin| pin == &serde_json::json!(["--stdio"]))
                || map.values().any(contains_stdio_const_pin)
        }
        Value::Array(items) => items.iter().any(contains_stdio_const_pin),
        _ => false,
    }
}

#[test]
fn extension_embeds_the_checked_projection() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let source = fs::read_to_string(root.join(EXTENSION_SOURCE_RELATIVE_PATH))?;

    assert!(
        source.contains("include_str!(\"../../launch-contract.v1.json\")"),
        "the staged extension must consume the checked launch projection, not an inline list"
    );
    assert!(
        source.contains("normalize_perllsp_args"),
        "structured perllsp argument normalization entry point went missing"
    );

    Ok(())
}

#[test]
fn mutation_controls_reject_false_projection_claims() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let projection = read_json(&root, PROJECTION_RELATIVE_PATH)?;

    let mut wrong_version = projection.clone();
    wrong_version["schema_version"] = Value::String("zed_perllsp_launch_contract.v0".into());
    assert!(validate_projection(&wrong_version, &root).is_err());

    let mut permissive = projection.clone();
    permissive["fail_closed_default"] = Value::Bool(false);
    assert!(validate_projection(&permissive, &root).is_err());

    let mut no_transport = projection.clone();
    no_transport["required_transport_flag"] = Value::String("--socket".into());
    assert!(validate_projection(&no_transport, &root).is_err());

    let mut unclassified_rejection = projection.clone();
    unclassified_rejection["rejected_flags"] = serde_json::json!([]);
    assert!(validate_projection(&unclassified_rejection, &root).is_err());

    let mut duplicated_admission = projection.clone();
    duplicated_admission["admitted_arguments"][1]["flag"] = Value::String("--log".to_string());
    assert!(validate_projection(&duplicated_admission, &root).is_err());

    Ok(())
}
