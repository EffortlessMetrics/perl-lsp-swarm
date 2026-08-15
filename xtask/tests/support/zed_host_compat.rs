//! Shared fail-closed validators for Zed host receipts.
//!
//! These helpers live under `xtask/tests/support` so the host and public-registry
//! programme contracts can reject the same false-green mutations without inventing
//! a live Zed run.

use serde_json::Value;
use sha2::{Digest, Sha256};

pub const PUBLIC_SUBJECT_RELATIVE_PATH: &str =
    ".ci/fixtures/zed-perl-upstream/receipts/public-registry-subject.v1.json";

const EXACT_SOURCE_REQUIRED_JOURNEY: &[&str] = &[
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
];

const PUBLIC_REQUIRED_JOURNEY: &[&str] = &[
    "manifest_discovery",
    "perl_attachment",
    "initialize",
    "workspace_root",
    "diagnostics",
    "completion",
    "hover",
    "definition",
    "references",
    "document_symbols",
    "workspace_symbols",
    "safe_edit_or_refusal",
    "unicode_positions",
    "mixed_newlines",
    "semantic_tokens",
    "post_edit_freshness",
    "restart",
    "shutdown",
];

const PUBLIC_REQUIRED_ACTIVATION: &[&str] =
    &["pl", "pm", "t", "PL", "psgi", "cgi", "fcgi", "shebang", "pod"];

pub fn nonempty_string(value: &Value, pointer: &str) -> bool {
    value.pointer(pointer).and_then(Value::as_str).is_some_and(|text| !text.trim().is_empty())
}

pub fn exact_sha256(value: &Value, pointer: &str) -> bool {
    value.pointer(pointer).and_then(Value::as_str).is_some_and(is_sha256_digest)
}

fn is_sha256_digest(text: &str) -> bool {
    text.len() == "sha256:".len() + 64
        && text.starts_with("sha256:")
        && text["sha256:".len()..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub fn content_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity("sha256:".len() + 64);
    encoded.push_str("sha256:");
    for byte in digest {
        encoded.push_str(&format!("{byte:02x}"));
    }
    encoded
}

fn cell_result<'a>(receipt: &'a Value, group: &str, cell: &str) -> Option<&'a str> {
    receipt.pointer(&format!("/{group}/{cell}/result")).and_then(Value::as_str)
}

fn same_string(left: &Value, left_pointer: &str, right: &Value, right_pointer: &str) -> bool {
    match (
        left.pointer(left_pointer).and_then(Value::as_str),
        right.pointer(right_pointer).and_then(Value::as_str),
    ) {
        (Some(left_text), Some(right_text)) => {
            !left_text.trim().is_empty() && left_text == right_text
        }
        _ => false,
    }
}

fn require_cells(receipt: &Value, group: &str, cells: &[&str]) -> Result<(), String> {
    for cell in cells {
        if cell_result(receipt, group, cell) != Some("pass")
            || !nonempty_string(receipt, &format!("/{group}/{cell}/evidence"))
        {
            return Err(format!("required {group} cell `{cell}` is not proven"));
        }
    }
    Ok(())
}

/// Validate a pass candidate.
///
/// Public-registry passes must supply the exact published subject bytes. The
/// validator hashes those bytes itself and compares against the receipt binding.
pub fn validate_pass(receipt: &Value, public_subject_bytes: Option<&[u8]>) -> Result<(), String> {
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
        || receipt.pointer("/profile/other_perl_servers_disabled").and_then(Value::as_bool)
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
    if receipt.pointer("/configuration/workspace_configuration_observed").and_then(Value::as_bool)
        != Some(true)
    {
        return Err("workspace/configuration was not observed".to_string());
    }

    if stage == "public_registry_install" {
        require_cells(receipt, "journey", PUBLIC_REQUIRED_JOURNEY)?;
        require_cells(receipt, "activation", PUBLIC_REQUIRED_ACTIVATION)?;
        validate_public_registry_pass(receipt, public_subject_bytes)?;
    } else {
        require_cells(receipt, "journey", EXACT_SOURCE_REQUIRED_JOURNEY)?;
        if cell_result(receipt, "activation", "pod") != Some("pass")
            || !nonempty_string(receipt, "/activation/pod/evidence")
        {
            return Err("POD separation is not proven".to_string());
        }
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

fn validate_public_registry_pass(
    receipt: &Value,
    public_subject_bytes: Option<&[u8]>,
) -> Result<(), String> {
    if receipt.pointer("/perllsp/resolution_route").and_then(Value::as_str)
        != Some("managed_download")
    {
        return Err(
            "public registry pass requires perllsp resolution_route=managed_download".to_string()
        );
    }
    if receipt.pointer("/profile/prior_extension_absent").and_then(Value::as_bool) != Some(true)
        || receipt.pointer("/profile/prior_managed_cache_absent").and_then(Value::as_bool)
            != Some(true)
    {
        return Err(
            "public registry pass requires a clean profile without prior extension or managed cache"
                .to_string(),
        );
    }

    let subject_bytes = public_subject_bytes
        .ok_or_else(|| "public registry pass requires the bound public subject".to_string())?;
    let subject_sha256 = content_sha256(subject_bytes);
    let subject: Value = serde_json::from_slice(subject_bytes)
        .map_err(|error| format!("public subject parse failed: {error}"))?;
    if subject.get("schema_version").and_then(Value::as_str)
        != Some("zed_public_registry_subject.v1")
    {
        return Err("public subject schema mismatch".to_string());
    }
    if subject.get("status").and_then(Value::as_str) != Some("published") {
        return Err("public subject is not published".to_string());
    }

    let bound_path = receipt
        .pointer("/public_subject/relative_path")
        .and_then(Value::as_str)
        .ok_or_else(|| "public receipt missing public_subject.relative_path".to_string())?;
    if bound_path != PUBLIC_SUBJECT_RELATIVE_PATH {
        return Err("public receipt binds the wrong subject path".to_string());
    }
    let bound_sha =
        receipt.pointer("/public_subject/sha256").and_then(Value::as_str).ok_or_else(|| {
            "public receipt missing content-addressed public_subject.sha256".to_string()
        })?;
    if !is_sha256_digest(bound_sha) {
        return Err("public receipt missing content-addressed public_subject.sha256".to_string());
    }
    if bound_sha != subject_sha256 {
        return Err("public_subject.sha256 does not match the bound subject bytes".to_string());
    }

    if !same_string(receipt, "/extension/candidate_commit", &subject, "/extension/commit")
        || !same_string(
            receipt,
            "/extension/manifest_version",
            &subject,
            "/extension/manifest_version",
        )
        || !same_string(receipt, "/perllsp/binary_sha256", &subject, "/perllsp_asset/asset_sha256")
        || !same_string(receipt, "/zed/build", &subject, "/zed_defaults/released_build")
        || !nonempty_string(&subject, "/registry/repository_commit")
        || !nonempty_string(&subject, "/registry/extension_version")
        || !nonempty_string(&subject, "/perllsp_asset/release")
        || !nonempty_string(&subject, "/perllsp_asset/target")
        || !nonempty_string(&subject, "/perllsp_asset/asset_url")
    {
        return Err("public receipt identities disagree with the bound public subject".to_string());
    }

    let managed = [
        "x86_64-unknown-linux-musl",
        "aarch64-unknown-linux-musl",
        "x86_64-apple-darwin",
        "aarch64-apple-darwin",
        "x86_64-pc-windows-msvc",
    ];
    let target = subject
        .pointer("/perllsp_asset/target")
        .and_then(Value::as_str)
        .ok_or_else(|| "public subject missing perllsp_asset.target".to_string())?;
    if !managed.contains(&target) {
        return Err(format!(
            "public subject target `{target}` is outside the managed Zed download contract"
        ));
    }

    Ok(())
}
