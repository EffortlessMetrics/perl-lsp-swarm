//! Static debug-adapter authority contract for the staged Zed extension
//! candidate (#9485, train phase `debug_adapter_authority`).
//!
//! The behavioral proof (identity separation, schema acceptance/rejection,
//! request-kind selection, PATH/managed precedence, target/member projection,
//! cleanup boundaries) lives in the candidate's own `cargo test` suite, which
//! `scripts/check-zed-upstream-candidate.sh` executes. This test binds the
//! durable fixture surface those behaviors depend on: the manifest/schema
//! authority, the LSP/DAP identity separation, and the canonical release
//! topology projection the managed route consumes.

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;

const PACKET_ROOT: &str = ".ci/fixtures/zed-perl-upstream";
const CANDIDATE_ROOT: &str = ".ci/fixtures/zed-perl-upstream/zed-perl";
const RELEASE_CONTRACT_PATH: &str = "docs/reference/downstream-dap-integrations.json";

fn repo_root() -> Result<PathBuf, Box<dyn Error>> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| io::Error::other("xtask manifest has no repository parent").into())
}

fn read_text(root: &Path, relative: &str) -> Result<String, Box<dyn Error>> {
    Ok(fs::read_to_string(root.join(relative))?)
}

fn parse_toml(text: &str) -> Result<toml::Value, Box<dyn Error>> {
    Ok(toml::from_str::<toml::Value>(text)?)
}

fn parse_json(text: &str) -> Result<Value, Box<dyn Error>> {
    Ok(serde_json::from_str(text)?)
}

fn extension_manifest(root: &Path) -> Result<toml::Value, Box<dyn Error>> {
    parse_toml(&read_text(root, &format!("{CANDIDATE_ROOT}/extension.toml"))?)
}

fn packet_manifest(root: &Path) -> Result<toml::Value, Box<dyn Error>> {
    parse_toml(&read_text(root, &format!("{PACKET_ROOT}/manifest.toml"))?)
}

#[test]
fn extension_declares_exactly_one_perl_dap_debug_adapter() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let extension = extension_manifest(&root)?;

    let debug_adapters = extension
        .get("debug_adapters")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| io::Error::other("extension.toml lacks a [debug_adapters] table"))?;
    let adapter_ids: BTreeSet<&str> = debug_adapters.keys().map(String::as_str).collect();
    assert_eq!(
        adapter_ids,
        BTreeSet::from(["perl-dap"]),
        "the extension must declare exactly the `perl-dap` debug adapter"
    );

    let entry = debug_adapters
        .get("perl-dap")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| io::Error::other("[debug_adapters.perl-dap] is not a table"))?;
    let schema_path = entry
        .get("schema_path")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| io::Error::other("[debug_adapters.perl-dap] lacks schema_path"))?;
    assert_eq!(schema_path, "debug_adapter_schemas/perl-dap.json");
    assert!(
        root.join(CANDIDATE_ROOT).join(schema_path).is_file(),
        "declared debugger configuration schema `{schema_path}` must exist"
    );

    Ok(())
}

#[test]
fn debug_adapter_identity_is_separate_from_every_language_server_id() -> Result<(), Box<dyn Error>>
{
    let root = repo_root()?;
    let extension = extension_manifest(&root)?;
    let packet = packet_manifest(&root)?;

    let server_ids: BTreeSet<String> = extension
        .get("language_servers")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| io::Error::other("extension.toml lacks [language_servers]"))?
        .keys()
        .cloned()
        .collect();
    assert_eq!(
        server_ids,
        BTreeSet::from([
            "perlnavigator-server".to_string(),
            "perl-lsp".to_string(),
            "perllsp".to_string(),
        ]),
        "the DAP increment must not change the three LSP provider identities"
    );
    assert!(
        !server_ids.contains("perl-dap"),
        "no language-server ID may alias the debug-adapter ID"
    );

    let server_id = packet
        .get("server_id")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| io::Error::other("packet manifest lacks server_id"))?;
    let debug_adapter_id = packet
        .get("debug_adapter_id")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| io::Error::other("packet manifest lacks debug_adapter_id"))?;
    let debug_binary = packet
        .get("debug_binary")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| io::Error::other("packet manifest lacks debug_binary"))?;
    assert_eq!(debug_adapter_id, "perl-dap");
    assert_eq!(debug_binary, "perl-dap");
    assert_ne!(server_id, debug_adapter_id, "adapter ID must not alias the LSP server ID");
    assert_ne!(packet.get("binary").and_then(toml::Value::as_str), Some(debug_binary));

    let copied: BTreeSet<String> = packet
        .get("copied_files")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| io::Error::other("packet manifest lacks copied_files"))?
        .iter()
        .filter_map(toml::Value::as_str)
        .map(str::to_string)
        .collect();
    assert!(
        copied.contains("debug_adapter_schemas/perl-dap.json"),
        "the submission packet must carry the debugger configuration schema"
    );
    for file in &copied {
        assert!(
            root.join(PACKET_ROOT).join("zed-perl").join(file).is_file()
                || root.join(file).is_file(),
            "copied_files entry `{file}` must resolve"
        );
    }

    Ok(())
}

#[test]
fn debugger_schema_rejects_unsupported_shapes_and_preserves_forward_compat()
-> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let schema = parse_json(&read_text(
        &root,
        &format!("{CANDIDATE_ROOT}/debug_adapter_schemas/perl-dap.json"),
    )?)?;

    let required: Vec<&str> = schema
        .get("required")
        .and_then(Value::as_array)
        .ok_or_else(|| io::Error::other("schema lacks required"))?
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(required, vec!["request", "program"], "launch needs request+program");

    let request_enum: Vec<&str> = schema
        .pointer("/properties/request/enum")
        .and_then(Value::as_array)
        .ok_or_else(|| io::Error::other("schema request lacks enum"))?
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(
        request_enum,
        vec!["launch"],
        "only `launch` is supported; attach must not be silently selectable"
    );

    assert_eq!(
        schema.pointer("/properties/program/minLength").and_then(Value::as_i64),
        Some(1),
        "`program` must reject the empty string"
    );
    assert_eq!(schema.pointer("/properties/args/type").and_then(Value::as_str), Some("array"));
    assert_eq!(schema.pointer("/properties/env/type").and_then(Value::as_str), Some("object"));
    assert_eq!(
        schema.get("additionalProperties").and_then(Value::as_bool),
        Some(true),
        "forward-compatible pass-through keys must be preserved"
    );

    Ok(())
}

#[test]
fn managed_projection_consumes_the_canonical_release_topology() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let packet = packet_manifest(&root)?;
    let release_contract = parse_json(&read_text(&root, RELEASE_CONTRACT_PATH)?)?;

    // The adapter consumes the same managed target set the accepted perllsp
    // route projects from the canonical release contract — not a private
    // copied table and not another issue's topology.
    let managed: BTreeSet<String> = packet
        .get("managed_targets")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| io::Error::other("packet manifest lacks managed_targets"))?
        .iter()
        .filter_map(toml::Value::as_str)
        .map(str::to_string)
        .collect();
    let unsupported: BTreeSet<String> = packet
        .get("unsupported_managed_targets")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| io::Error::other("packet manifest lacks unsupported_managed_targets"))?
        .iter()
        .filter_map(toml::Value::as_str)
        .map(str::to_string)
        .collect();

    let released_targets: BTreeSet<String> = release_contract
        .get("targets")
        .and_then(Value::as_array)
        .ok_or_else(|| io::Error::other("release contract lacks targets"))?
        .iter()
        .filter_map(|entry| entry.get("triple"))
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect();
    assert!(
        managed.is_subset(&released_targets),
        "managed targets absent from the canonical release contract: {:?}",
        managed.difference(&released_targets).collect::<Vec<_>>()
    );
    assert!(
        released_targets.is_superset(&unsupported),
        "unsupported targets must still be canonical release-contract triples"
    );
    assert!(
        managed.is_disjoint(&unsupported),
        "a target cannot be managed and unsupported at once"
    );
    assert!(
        unsupported.contains("aarch64-pc-windows-msvc"),
        "Windows ARM64 must remain explicitly unclaimed"
    );

    // The canonical archives ship the perl-dap member for every platform
    // family the managed route can select.
    let archive_pattern = release_contract
        .get("archive_name_pattern")
        .and_then(Value::as_str)
        .ok_or_else(|| io::Error::other("release contract lacks archive_name_pattern"))?;
    assert_eq!(archive_pattern, "perllsp-{version}-{triple}{ext}");
    for platform_family in ["unix", "windows"] {
        let required_binaries = release_contract
            .pointer(&format!("/platforms/{platform_family}/required_binaries"))
            .and_then(Value::as_array)
            .ok_or_else(|| io::Error::other("release contract lacks required_binaries"))?;
        assert!(
            required_binaries.iter().any(|binary| {
                binary.as_str().is_some_and(|name| name == "perl-dap" || name == "perl-dap.exe")
            }),
            "canonical {platform_family} archives must ship the `perl-dap` member"
        );
    }

    Ok(())
}
