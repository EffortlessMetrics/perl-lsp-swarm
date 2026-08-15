//! Shared fail-closed validators for Zed host receipts.
//!
//! These helpers live under `xtask/tests/support` so the host and public-registry
//! programme contracts can reject the same false-green mutations without inventing
//! a live Zed run.

use chrono::DateTime;
use serde_json::{Map, Value};
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

const TOP_LEVEL_REQUIRED: &[&str] = &[
    "schema_version",
    "evidence_stage",
    "result",
    "observed_at",
    "zed",
    "extension",
    "perllsp",
    "platform",
    "profile",
    "workspace",
    "configuration",
    "activation",
    "journey",
    "artifacts",
    "limitations",
    "claim_boundary",
];

const TOP_LEVEL_ALLOWED: &[&str] = &[
    "schema_version",
    "evidence_stage",
    "result",
    "observed_at",
    "public_subject",
    "zed",
    "extension",
    "perllsp",
    "platform",
    "profile",
    "workspace",
    "configuration",
    "activation",
    "journey",
    "artifacts",
    "limitations",
    "claim_boundary",
];

const ZED_FIELDS: &[&str] = &["product", "version", "channel", "build"];
const EXTENSION_FIELDS: &[&str] = &[
    "repository",
    "base_commit",
    "candidate_commit",
    "manifest_version",
    "wasm_sha256",
    "install_route",
];
const PERLLSP_FIELDS: &[&str] = &[
    "server_id",
    "command",
    "arguments",
    "version",
    "build_commit",
    "binary_sha256",
    "resolution_route",
];
const PLATFORM_FIELDS: &[&str] = &["os", "version", "architecture"];
const PROFILE_FIELDS: &[&str] = &[
    "clean_profile",
    "prior_extension_absent",
    "prior_managed_cache_absent",
    "other_perl_servers_disabled",
];
const WORKSPACE_FIELDS: &[&str] = &["fixture_id", "fixture_sha256", "root_identity"];
const CONFIGURATION_FIELDS: &[&str] = &[
    "settings_sha256",
    "server_order",
    "workspace_configuration_observed",
    "precedence_observed",
    "live_update_observed",
];
const ARTIFACT_FIELDS: &[&str] = &[
    "zed_log",
    "language_server_log",
    "process_inventory",
    "redacted",
];
const CELL_FIELDS: &[&str] = &["result", "evidence"];
const CELL_RESULTS: &[&str] =
    &["pass", "fail", "not_proven", "unsupported", "legitimate_empty", "instrument_failed"];

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

fn is_full_commit(text: &str) -> bool {
    text.len() == 40 && text.bytes().all(|byte| byte.is_ascii_hexdigit())
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

fn object<'a>(value: &'a Value, context: &str) -> Result<&'a Map<String, Value>, String> {
    value.as_object().ok_or_else(|| format!("{context} must be an object"))
}

fn field<'a>(object: &'a Map<String, Value>, key: &str, context: &str) -> Result<&'a Value, String> {
    object.get(key).ok_or_else(|| format!("{context}.{key} is missing"))
}

fn require_keys(
    object: &Map<String, Value>,
    required: &[&str],
    context: &str,
) -> Result<(), String> {
    for key in required {
        if !object.contains_key(*key) {
            return Err(format!("{context} is missing required key `{key}`"));
        }
    }
    Ok(())
}

fn reject_extra_keys(
    object: &Map<String, Value>,
    allowed: &[&str],
    context: &str,
) -> Result<(), String> {
    for key in object.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(format!("{context} contains unexpected key `{key}`"));
        }
    }
    Ok(())
}

fn required_object<'a>(
    parent: &'a Map<String, Value>,
    key: &str,
    fields: &[&str],
) -> Result<&'a Map<String, Value>, String> {
    let value = parent.get(key).ok_or_else(|| format!("missing `{key}`"))?;
    let object = object(value, key)?;
    require_keys(object, fields, key)?;
    reject_extra_keys(object, fields, key)?;
    Ok(object)
}

fn optional_string(value: &Value, context: &str) -> Result<(), String> {
    if value.is_null() || value.is_string() {
        Ok(())
    } else {
        Err(format!("{context} must be a string or null"))
    }
}

fn optional_bool(value: &Value, context: &str) -> Result<(), String> {
    if value.is_null() || value.is_boolean() {
        Ok(())
    } else {
        Err(format!("{context} must be a boolean or null"))
    }
}

fn optional_enum(value: &Value, allowed: &[&str], context: &str) -> Result<(), String> {
    if value.is_null() {
        return Ok(());
    }
    required_enum(value, allowed, context)
}

fn required_enum(value: &Value, allowed: &[&str], context: &str) -> Result<(), String> {
    let text = value.as_str().ok_or_else(|| format!("{context} must be a string"))?;
    if allowed.contains(&text) {
        Ok(())
    } else {
        Err(format!("{context} has invalid value `{text}`"))
    }
}

fn optional_commit(value: &Value, context: &str) -> Result<(), String> {
    if value.is_null() {
        return Ok(());
    }
    let text = value.as_str().ok_or_else(|| format!("{context} must be a string or null"))?;
    if is_full_commit(text) {
        Ok(())
    } else {
        Err(format!("{context} must be a full 40-hex commit"))
    }
}

fn optional_sha256(value: &Value, context: &str) -> Result<(), String> {
    if value.is_null() {
        return Ok(());
    }
    let text = value.as_str().ok_or_else(|| format!("{context} must be a string or null"))?;
    if is_sha256_digest(text) { Ok(()) } else { Err(format!("{context} must be a sha256 digest")) }
}

fn string_array(value: &Value, context: &str) -> Result<(), String> {
    let values = value.as_array().ok_or_else(|| format!("{context} must be an array"))?;
    if values.iter().all(Value::is_string) {
        Ok(())
    } else {
        Err(format!("{context} must contain only strings"))
    }
}

fn validate_optional_strings(
    object: &Map<String, Value>,
    keys: &[&str],
    context: &str,
) -> Result<(), String> {
    for key in keys {
        optional_string(field(object, key, context)?, &format!("{context}.{key}"))?;
    }
    Ok(())
}

fn validate_optional_bools(
    object: &Map<String, Value>,
    keys: &[&str],
    context: &str,
) -> Result<(), String> {
    for key in keys {
        optional_bool(field(object, key, context)?, &format!("{context}.{key}"))?;
    }
    Ok(())
}

fn validate_cell(value: &Value, context: &str) -> Result<(), String> {
    let cell = object(value, context)?;
    require_keys(cell, CELL_FIELDS, context)?;
    reject_extra_keys(cell, CELL_FIELDS, context)?;
    required_enum(field(cell, "result", context)?, CELL_RESULTS, &format!("{context}.result"))?;
    optional_string(field(cell, "evidence", context)?, &format!("{context}.evidence"))
}

fn validate_cells(parent: &Map<String, Value>, key: &str, cells: &[&str]) -> Result<(), String> {
    let group = required_object(parent, key, cells)?;
    for cell in cells {
        validate_cell(field(group, cell, key)?, &format!("{key}.{cell}"))?;
    }
    Ok(())
}

fn validate_public_subject(value: &Value) -> Result<(), String> {
    let subject = object(value, "public_subject")?;
    const FIELDS: &[&str] = &["relative_path", "sha256"];
    require_keys(subject, FIELDS, "public_subject")?;
    reject_extra_keys(subject, FIELDS, "public_subject")?;
    if field(subject, "relative_path", "public_subject")?.as_str()
        != Some(PUBLIC_SUBJECT_RELATIVE_PATH)
    {
        return Err("public_subject.relative_path is not canonical".to_string());
    }
    optional_sha256(field(subject, "sha256", "public_subject")?, "public_subject.sha256")
}

fn validate_observed_at(value: &Value) -> Result<(), String> {
    let Some(text) = value.as_str() else {
        return if value.is_null() {
            Ok(())
        } else {
            Err("observed_at must be a string or null".to_string())
        };
    };
    DateTime::parse_from_rfc3339(text)
        .map(|_| ())
        .map_err(|error| format!("observed_at is not RFC 3339: {error}"))
}

fn all_string_fields(object: &Map<String, Value>, fields: &[&str]) -> bool {
    fields.iter().all(|key| object.get(*key).and_then(Value::as_str).is_some())
}

fn validate_pass_shape(
    top: &Map<String, Value>,
    zed: &Map<String, Value>,
    extension: &Map<String, Value>,
    perllsp: &Map<String, Value>,
    profile: &Map<String, Value>,
    artifacts: &Map<String, Value>,
) -> Result<(), String> {
    if top.get("observed_at").and_then(Value::as_str).is_none()
        || !all_string_fields(zed, &["version", "channel", "build"])
        || !all_string_fields(
            extension,
            &[
                "base_commit",
                "candidate_commit",
                "manifest_version",
                "wasm_sha256",
                "install_route",
            ],
        )
        || !all_string_fields(
            perllsp,
            &["command", "version", "build_commit", "binary_sha256", "resolution_route"],
        )
        || perllsp.get("arguments") != Some(&serde_json::json!(["--stdio"]))
        || profile.get("clean_profile").and_then(Value::as_bool) != Some(true)
        || profile.get("other_perl_servers_disabled").and_then(Value::as_bool) != Some(true)
        || !all_string_fields(artifacts, &["zed_log", "language_server_log", "process_inventory"])
        || artifacts.get("redacted").and_then(Value::as_bool) != Some(true)
    {
        return Err("pass receipt violates required non-null schema fields".to_string());
    }
    Ok(())
}

fn validate_public_pass_shape(
    top: &Map<String, Value>,
    perllsp: &Map<String, Value>,
    profile: &Map<String, Value>,
) -> Result<(), String> {
    let subject = top
        .get("public_subject")
        .ok_or_else(|| "public pass lacks public_subject".to_string())?;
    validate_public_subject(subject)?;
    let subject = object(subject, "public_subject")?;
    if !subject.get("sha256").and_then(Value::as_str).is_some_and(is_sha256_digest)
        || perllsp.get("resolution_route").and_then(Value::as_str) != Some("managed_download")
        || profile.get("prior_extension_absent").and_then(Value::as_bool) != Some(true)
        || profile.get("prior_managed_cache_absent").and_then(Value::as_bool) != Some(true)
    {
        return Err("public pass violates public-registry schema conditions".to_string());
    }
    Ok(())
}

/// Validate the complete structural contract declared by
/// `.ci/schemas/zed-host-compat.v1.schema.json`.
pub fn validate_schema(receipt: &Value) -> Result<(), String> {
    let top = object(receipt, "receipt")?;
    require_keys(top, TOP_LEVEL_REQUIRED, "receipt")?;
    reject_extra_keys(top, TOP_LEVEL_ALLOWED, "receipt")?;

    if field(top, "schema_version", "receipt")?.as_str() != Some("zed_host_compat.v1") {
        return Err("wrong receipt schema".to_string());
    }
    required_enum(
        field(top, "evidence_stage", "receipt")?,
        &["exact_source_dev_extension", "public_registry_install"],
        "evidence_stage",
    )?;
    required_enum(
        field(top, "result", "receipt")?,
        &["not_run", "pass", "fail", "instrument_failed"],
        "result",
    )?;
    validate_observed_at(field(top, "observed_at", "receipt")?)?;
    if let Some(public_subject) = top.get("public_subject") {
        validate_public_subject(public_subject)?;
    }

    let zed = required_object(top, "zed", ZED_FIELDS)?;
    if field(zed, "product", "zed")?.as_str() != Some("Zed") {
        return Err("zed.product must be `Zed`".to_string());
    }
    validate_optional_strings(zed, &["version", "channel", "build"], "zed")?;

    let extension = required_object(top, "extension", EXTENSION_FIELDS)?;
    if field(extension, "repository", "extension")?.as_str()
        != Some("tree-sitter-perl/zed-perl")
    {
        return Err("extension.repository is not canonical".to_string());
    }
    optional_commit(field(extension, "base_commit", "extension")?, "extension.base_commit")?;
    optional_commit(
        field(extension, "candidate_commit", "extension")?,
        "extension.candidate_commit",
    )?;
    optional_string(
        field(extension, "manifest_version", "extension")?,
        "extension.manifest_version",
    )?;
    optional_sha256(
        field(extension, "wasm_sha256", "extension")?,
        "extension.wasm_sha256",
    )?;
    optional_enum(
        field(extension, "install_route", "extension")?,
        &["dev_extension", "official_registry"],
        "extension.install_route",
    )?;

    let perllsp = required_object(top, "perllsp", PERLLSP_FIELDS)?;
    if field(perllsp, "server_id", "perllsp")?.as_str() != Some("perllsp") {
        return Err("perllsp.server_id is not canonical".to_string());
    }
    validate_optional_strings(perllsp, &["command", "version"], "perllsp")?;
    string_array(field(perllsp, "arguments", "perllsp")?, "perllsp.arguments")?;
    optional_commit(
        field(perllsp, "build_commit", "perllsp")?,
        "perllsp.build_commit",
    )?;
    optional_sha256(
        field(perllsp, "binary_sha256", "perllsp")?,
        "perllsp.binary_sha256",
    )?;
    optional_enum(
        field(perllsp, "resolution_route", "perllsp")?,
        &["binary_override", "worktree_path", "managed_download"],
        "perllsp.resolution_route",
    )?;

    let platform = required_object(top, "platform", PLATFORM_FIELDS)?;
    optional_enum(
        field(platform, "os", "platform")?,
        &["macos", "linux", "windows"],
        "platform.os",
    )?;
    optional_string(field(platform, "version", "platform")?, "platform.version")?;
    optional_enum(
        field(platform, "architecture", "platform")?,
        &["x86_64", "aarch64"],
        "platform.architecture",
    )?;

    let profile = required_object(top, "profile", PROFILE_FIELDS)?;
    validate_optional_bools(profile, PROFILE_FIELDS, "profile")?;

    let workspace = required_object(top, "workspace", WORKSPACE_FIELDS)?;
    validate_optional_strings(workspace, &["fixture_id", "root_identity"], "workspace")?;
    optional_sha256(
        field(workspace, "fixture_sha256", "workspace")?,
        "workspace.fixture_sha256",
    )?;

    let configuration = required_object(top, "configuration", CONFIGURATION_FIELDS)?;
    optional_sha256(
        field(configuration, "settings_sha256", "configuration")?,
        "configuration.settings_sha256",
    )?;
    string_array(
        field(configuration, "server_order", "configuration")?,
        "configuration.server_order",
    )?;
    optional_bool(
        field(configuration, "workspace_configuration_observed", "configuration")?,
        "configuration.workspace_configuration_observed",
    )?;
    validate_optional_strings(
        configuration,
        &["precedence_observed", "live_update_observed"],
        "configuration",
    )?;

    validate_cells(top, "activation", PUBLIC_REQUIRED_ACTIVATION)?;
    validate_cells(top, "journey", PUBLIC_REQUIRED_JOURNEY)?;

    let artifacts = required_object(top, "artifacts", ARTIFACT_FIELDS)?;
    validate_optional_strings(
        artifacts,
        &["zed_log", "language_server_log", "process_inventory"],
        "artifacts",
    )?;
    optional_bool(field(artifacts, "redacted", "artifacts")?, "artifacts.redacted")?;
    string_array(field(top, "limitations", "receipt")?, "limitations")?;
    if !field(top, "claim_boundary", "receipt")?
        .as_str()
        .is_some_and(|text| !text.is_empty())
    {
        return Err("claim_boundary must be a non-empty string".to_string());
    }

    if top.get("result").and_then(Value::as_str) == Some("pass") {
        validate_pass_shape(top, zed, extension, perllsp, profile, artifacts)?;
    }
    if top.get("evidence_stage").and_then(Value::as_str) == Some("public_registry_install")
        && top.get("result").and_then(Value::as_str) == Some("pass")
    {
        validate_public_pass_shape(top, perllsp, profile)?;
    }
    Ok(())
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
    validate_schema(receipt)?;
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
