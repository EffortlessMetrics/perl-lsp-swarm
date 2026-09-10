//! Bind managed-route observations to independently validated asset and host receipts.

#[path = "zed_host_compat.rs"]
mod zed_host_compat;

use serde_json::Value;

pub use zed_host_compat::content_sha256;

fn same(left: &Value, left_path: &str, right: &Value, right_path: &str) -> Result<(), String> {
    let value = left.pointer(left_path).filter(|value| !value.is_null());
    if value.is_none() || value != right.pointer(right_path) {
        return Err(format!("subject mismatch: {left_path} must match {right_path}"));
    }
    Ok(())
}

fn selected_row<'a>(value: &'a Value, target: &str) -> Result<&'a Value, String> {
    let mut rows = value
        .get("targets")
        .and_then(Value::as_array)
        .ok_or_else(|| "asset authority lacks target rows".to_string())?
        .iter()
        .filter(|row| row.get("target").and_then(Value::as_str) == Some(target));
    let row =
        rows.next().ok_or_else(|| "selected target is absent from asset authority".to_string())?;
    if rows.next().is_some() || row.get("disposition").and_then(Value::as_str) != Some("managed") {
        return Err("selected target must be a unique managed row".to_string());
    }
    Ok(row)
}

/// Validate host authority and cross-receipt subjects using captured input bytes.
/// The caller must also run the existing Python asset receipt validator against
/// these same asset and asset-contract bytes; this function does not replace it.
pub fn validate_bound_pass(
    receipt: &Value,
    asset_bytes: &[u8],
    host_bytes: &[u8],
    asset_contract_bytes: &[u8],
) -> Result<(), String> {
    for (pointer, bytes) in [
        ("/upstream/asset_receipt_sha256", asset_bytes),
        ("/upstream/host_receipt_sha256", host_bytes),
    ] {
        if receipt.pointer(pointer).and_then(Value::as_str) != Some(content_sha256(bytes).as_str())
        {
            return Err(format!("upstream receipt digest mismatch: {pointer}"));
        }
    }
    let asset: Value = serde_json::from_slice(asset_bytes).map_err(|error| error.to_string())?;
    let host: Value = serde_json::from_slice(host_bytes).map_err(|error| error.to_string())?;
    let contract: Value =
        serde_json::from_slice(asset_contract_bytes).map_err(|error| error.to_string())?;
    if asset.get("result").and_then(Value::as_str) != Some("pass") {
        return Err("managed route requires a passing asset receipt".to_string());
    }
    zed_host_compat::validate_pass(&host, None)?;
    if host.get("evidence_stage").and_then(Value::as_str) != Some("exact_source_dev_extension")
        || host.pointer("/perllsp/resolution_route").and_then(Value::as_str)
            != Some("managed_download")
        || host.pointer("/profile/prior_managed_cache_absent").and_then(Value::as_bool)
            != Some(true)
    {
        return Err(
            "host receipt must prove the exact-source managed route with absent prior cache"
                .to_string(),
        );
    }
    for (managed, upstream) in [
        ("zed_version", "/zed/version"),
        ("zed_build", "/zed/build"),
        ("extension_version", "/extension/manifest_version"),
        ("extension_candidate_commit", "/extension/candidate_commit"),
        ("extension_wasm_sha256", "/extension/wasm_sha256"),
        ("fixture_id", "/workspace/fixture_id"),
        ("fixture_sha256", "/workspace/fixture_sha256"),
        ("binary_sha256", "/perllsp/binary_sha256"),
        ("version", "/perllsp/version"),
    ] {
        same(receipt, &format!("/subject/{managed}"), &host, upstream)?;
    }
    for (asset_path, contract_path) in [
        ("/release/repository", "/source/repository"),
        ("/release/id", "/source/release_id"),
        ("/release/tag", "/source/tag"),
        ("/release/version", "/source/version"),
    ] {
        same(&asset, asset_path, &contract, contract_path)?;
    }
    same(receipt, "/subject/version", &asset, "/release/version")?;
    let target = receipt
        .pointer("/subject/target")
        .and_then(Value::as_str)
        .ok_or_else(|| "managed receipt lacks subject.target".to_string())?;
    let row = selected_row(&asset, target)?;
    let expected = selected_row(&contract, target)?;
    for (asset_path, contract_path) in [
        ("/os", "/os"),
        ("/architecture", "/architecture"),
        ("/asset/id", "/asset_id"),
        ("/asset/name", "/asset_name"),
        ("/asset/size", "/asset_size"),
        ("/asset/sha256", "/asset_digest"),
        ("/asset/archive_type", "/archive_type"),
        ("/archive/required_member", "/archive_member"),
    ] {
        same(row, asset_path, expected, contract_path)?;
    }
    same(&host, "/platform/os", expected, "/os")?;
    same(&host, "/platform/architecture", expected, "/architecture")?;
    same(receipt, "/subject/asset_sha256", row, "/asset/sha256")?;
    same(receipt, "/subject/binary_sha256", row, "/binary/sha256")?;
    same(receipt, "/subject/installed_path", expected, "/installed_path")?;
    let member = expected
        .get("archive_member")
        .and_then(Value::as_str)
        .and_then(|member| member.rsplit('/').next())
        .ok_or_else(|| "asset contract lacks member basename".to_string())?;
    if row.pointer("/binary/name").and_then(Value::as_str) != Some(member)
        || row.pointer("/archive/installed_name").and_then(Value::as_str) != Some(member)
        || row.pointer("/binary/executable").and_then(Value::as_bool) != Some(true)
    {
        return Err("selected asset must expose the expected executable".to_string());
    }
    let installed = expected
        .get("installed_path")
        .and_then(Value::as_str)
        .ok_or_else(|| "asset contract lacks installed path".to_string())?;
    let command = host
        .pointer("/perllsp/command")
        .and_then(Value::as_str)
        .ok_or_else(|| "host lacks managed command".to_string())?
        .replace('\\', "/");
    if !command.ends_with(&format!("/{installed}")) {
        return Err("host command does not end at the selected managed install path".to_string());
    }
    Ok(())
}
