//! Shared fail-closed validators for Zed host receipts.
//!
//! These helpers live under `xtask/tests/support` so the host and public-registry
//! programme contracts can reject the same false-green mutations without inventing
//! a live Zed run.

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

const CELL_RESULTS: &[&str] = &[
    "pass",
    "fail",
    "not_proven",
    "unsupported",
    "legitimate_empty",
    "instrument_failed",
];

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
    required: &[&str],
    allowed: &[&str],
) -> Result<&'a Map<String, Value>, String> {
    let value = parent.get(key).ok_or_else(|| format!("missing `{key}`"))?;
    let object = object(value, key)?;
    require_keys(object, required, key)?;
    reject_extra_keys(object, allowed, key)?;
    Ok(object)
}

fn string_or_null(value: &Value, context: &str) -> Result<(), String> {
    if value.is_null() || value.is_string() {
        Ok(())
    } else {
        Err(format!("{context} must be a string or null"))
    }
}

fn bool_or_null(value: &Value, context: &str) -> Result<(), String> {
    if value.is_null() || value.is_boolean() {
        Ok(())
    } else {
        Err(format!("{context} must be a boolean or null"))
    }
}

fn enum_or_null(value: &Value, allowed: &[&str], context: &str) -> Result<(), String> {
    if value.is_null() {
        return Ok(());
    }
    let text = value.as_str().ok_or_else(|| format!("{context} must be a string or null"))?;
    if allowed.contains(&text) {
        Ok(())
    } else {
        Err(format!("{context} has invalid value `{text}`"))
    }
}

fn commit_or_null(value: &Value, context: &str) -> Result<(), String> {
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

fn sha256_or_null(value: &Value, context: &str) -> Result<(), String> {
    if value.is_null() {
        return Ok(());
    }
    let text = value.as_str().ok_or_else(|| format!("{context} must be a string or null"))?;
    if is_sha256_digest(text) {
        Ok(())
    } else {
        Err(format!("{context} must be a sha256 digest"))
    }
}

fn string_array(value: &Value, context: &str) -> Result<(), String> {
    let values = value.as_array().ok_or_else(|| format!("{context} must be an array"))?;
    if values.iter().all(Value::is_string) {
        Ok(())
    } else {
        Err(format!("{context} must contain only strings"))
    }
}

fn validate_cell(value: &Value, context: &str) -> Result<(), String> {
    let cell = object(value, context)?;
    require_keys(cell, &["result", "evidence"], context)?;
    reject_extra_keys(cell, &["result", "evidence"], context)?;
    enum_or_null(
        cell.get("result").ok_or_else(|| format!("{context}.result is missing"))?,
        CELL_RESULTS,
        &format!("{context}.result"),
    )?;
    if cell.get("result").and_then(Value::as_str).is_none() {
        return Err(format!("{context}.result must be a string"));
    }
    string_or_null(
        cell.get("evidence").ok_or_else(|| format!("{context}.evidence is missing"))?,
        &format!("{context}.evidence"),
    )
}

fn validate_cells(
    parent: &Map<String, Value>,
    key: &str,
    cells: &[&str],
) -> Result<(), String> {
    let group = required_object(parent, key, cells, cells)?;
    for cell in cells {
        validate_cell(
            group.get(*cell).ok_or_else(|| format!("{key}.{cell} is missing"))?,
            &format!("{key}.{cell}"),
        )?;
    }
    Ok(())
}

/// Validate the complete structural contract declared by
/// `.ci/schemas/zed-host-compat.v1.schema.json`.
pub fn validate_schema(receipt: &Value) -> Result<(), String> {
    let top = object(receipt, "receipt")?;
    require_keys(top, TOP_LEVEL_REQUIRED, "receipt")?;
    reject_extra_keys(top, TOP_LEVEL_ALLOWED, "receipt")?;

    if top.get("schema_version").and_then(Value::as_str) != Some("zed_host_compat.v1") {
        return Err("wrong receipt schema".to_string());
    }
    enum_or_null(
        top.get("evidence_stage").ok_or_else(|| "missing evidence_stage".to_string())?,
        &["exact_source_dev_extension", "public_registry_install"],
        "evidence_stage",
    )?;
    if top.get("evidence_stage").and_then(Value::as_str).is_none() {
        return Err("evidence_stage must be a string".to_string());
    }
    enum_or_null(
        top.get("result").ok_or_else(|| "missing result".to_string())?,
        &["not_run", "pass", "fail", "instrument_failed"],
        "result",
    )?;
    if top.get("result").and_then(Value::as_str).is_none() {
        return Err("result must be a string".to_string());
    }
    string_or_null(
        top.get("observed_at").ok_or_else(|| "missing observed_at".to_string())?,
        "observed_at",
    )?;

    if let Some(public_subject) = top.get("public_subject") {
        let subject = object(public_subject, "public_subject")?;
        require_keys(subject, &["relative_path", "sha256"], "public_subject")?;
        reject_extra_keys(subject, &["relative_path", "sha256"], "public_subject")?;
        let path = subject
            .get("relative_path")
            .ok_or_else(|| "public_subject.relative_path is missing".to_string())?;
        if !path.is_null() && path.as_str() != Some(PUBLIC_SUBJECT_RELATIVE_PATH) {
            return Err("public_subject.relative_path is not canonical".to_string());
        }
        sha256_or_null(
            subject.get("sha256").ok_or_else(|| "public_subject.sha256 is missing".to_string())?,
            "public_subject.sha256",
        )?;
    }

    let zed = required_object(top, "zed", &["product", "version", "channel", "build"], &["product", "version", "channel", "build"])?;
    if zed.get("product").and_then(Value::as_str) != Some("Zed") {
        return Err("zed.product must be `Zed`".to_string());
    }
    for key in ["version", "channel", "build"] {
        string_or_null(zed.get(key).ok_or_else(|| format!("zed.{key} is missing"))?, &format!("zed.{key}"))?;
    }

    let extension = required_object(
        top,
        "extension",
        &["repository", "base_commit", "candidate_commit", "manifest_version", "wasm_sha256", "install_route"],
        &["repository", "base_commit", "candidate_commit", "manifest_version", "wasm_sha256", "install_route"],
    )?;
    if extension.get("repository").and_then(Value::as_str) != Some("tree-sitter-perl/zed-perl") {
        return Err("extension.repository is not canonical".to_string());
    }
    commit_or_null(extension.get("base_commit").ok_or_else(|| "extension.base_commit is missing".to_string())?, "extension.base_commit")?;
    commit_or_null(extension.get("candidate_commit").ok_or_else(|| "extension.candidate_commit is missing".to_string())?, "extension.candidate_commit")?;
    string_or_null(extension.get("manifest_version").ok_or_else(|| "extension.manifest_version is missing".to_string())?, "extension.manifest_version")?;
    sha256_or_null(extension.get("wasm_sha256").ok_or_else(|| "extension.wasm_sha256 is missing".to_string())?, "extension.wasm_sha256")?;
    enum_or_null(extension.get("install_route").ok_or_else(|| "extension.install_route is missing".to_string())?, &["dev_extension", "official_registry"], "extension.install_route")?;

    let perllsp = required_object(
        top,
        "perllsp",
        &["server_id", "command", "arguments", "version", "build_commit", "binary_sha256", "resolution_route"],
        &["server_id", "command", "arguments", "version", "build_commit", "binary_sha256", "resolution_route"],
    )?;
    if perllsp.get("server_id").and_then(Value::as_str) != Some("perllsp") {
        return Err("perllsp.server_id is not canonical".to_string());
    }
    string_or_null(perllsp.get("command").ok_or_else(|| "perllsp.command is missing".to_string())?, "perllsp.command")?;
    string_array(perllsp.get("arguments").ok_or_else(|| "perllsp.arguments is missing".to_string())?, "perllsp.arguments")?;
    string_or_null(perllsp.get("version").ok_or_else(|| "perllsp.version is missing".to_string())?, "perllsp.version")?;
    commit_or_null(perllsp.get("build_commit").ok_or_else(|| "perllsp.build_commit is missing".to_string())?, "perllsp.build_commit")?;
    sha256_or_null(perllsp.get("binary_sha256").ok_or_else(|| "perllsp.binary_sha256 is missing".to_string())?, "perllsp.binary_sha256")?;
    enum_or_null(perllsp.get("resolution_route").ok_or_else(|| "perllsp.resolution_route is missing".to_string())?, &["binary_override", "worktree_path", "managed_download"], "perllsp.resolution_route")?;

    let platform = required_object(top, "platform", &["os", "version", "architecture"], &["os", "version", "architecture"])?;
    enum_or_null(platform.get("os").ok_or_else(|| "platform.os is missing".to_string())?, &["macos", "linux", "windows"], "platform.os")?;
    string_or_null(platform.get("version").ok_or_else(|| "platform.version is missing".to_string())?, "platform.version")?;
    enum_or_null(platform.get("architecture").ok_or_else(|| "platform.architecture is missing".to_string())?, &["x86_64", "aarch64"], "platform.architecture")?;

    let profile = required_object(
        top,
        "profile",
        &["clean_profile", "prior_extension_absent", "prior_managed_cache_absent", "other_perl_servers_disabled"],
        &["clean_profile", "prior_extension_absent", "prior_managed_cache_absent", "other_perl_servers_disabled"],
    )?;
    for key in ["clean_profile", "prior_extension_absent", "prior_managed_cache_absent", "other_perl_servers_disabled"] {
        bool_or_null(profile.get(key).ok_or_else(|| format!("profile.{key} is missing"))?, &format!("profile.{key}"))?;
    }

    let workspace = required_object(top, "workspace", &["fixture_id", "fixture_sha256", "root_identity"], &["fixture_id", "fixture_sha256", "root_identity"])?;
    string_or_null(workspace.get("fixture_id").ok_or_else(|| "workspace.fixture_id is missing".to_string())?, "workspace.fixture_id")?;
    sha256_or_null(workspace.get("fixture_sha256").ok_or_else(|| "workspace.fixture_sha256 is missing".to_string())?, "workspace.fixture_sha256")?;
    string_or_null(workspace.get("root_identity").ok_or_else(|| "workspace.root_identity is missing".to_string())?, "workspace.root_identity")?;

    let configuration = required_object(
        top,
        "configuration",
        &["settings_sha256", "server_order", "workspace_configuration_observed", "precedence_observed", "live_update_observed"],
        &["settings_sha256", "server_order", "workspace_configuration_observed", "precedence_observed", "live_update_observed"],
    )?;
    sha256_or_null(configuration.get("settings_sha256").ok_or_else(|| "configuration.settings_sha256 is missing".to_string())?, "configuration.settings_sha256")?;
    string_array(configuration.get("server_order").ok_or_else(|| "configuration.server_order is missing".to_string())?, "configuration.server_order")?;
    bool_or_null(configuration.get("workspace_configuration_observed").ok_or_else(|| "configuration.workspace_configuration_observed is missing".to_string())?, "configuration.workspace_configuration_observed")?;
    string_or_null(configuration.get("precedence_observed").ok_or_else(|| "configuration.precedence_observed is missing".to_string())?, "configuration.precedence_observed")?;
    string_or_null(configuration.get("live_update_observed").ok_or_else(|| "configuration.live_update_observed is missing".to_string())?, "configuration.live_update_observed")?;

    validate_cells(top, "activation", PUBLIC_REQUIRED_ACTIVATION)?;
    validate_cells(top, "journey", PUBLIC_REQUIRED_JOURNEY)?;

    let artifacts = required_object(top, "artifacts", &["zed_log", "language_server_log", "process_inventory", "redacted"], &["zed_log", "language_server_log", "process_inventory", "redacted"])?;
    for key in ["zed_log", "language_server_log", "process_inventory"] {
        string_or_null(artifacts.get(key).ok_or_else(|| format!("artifacts.{key} is missing"))?, &format!("artifacts.{key}"))?;
    }
    bool_or_null(artifacts.get("redacted").ok_or_else(|| "artifacts.redacted is missing".to_string())?, "artifacts.redacted")?;
    string_array(top.get("limitations").ok_or_else(|| "limitations is missing".to_string())?, "limitations")?;
    if !top.get("claim_boundary").and_then(Value::as_str).is_some_and(|text| !text.is_empty()) {
        return Err("claim_boundary must be a non-empty string".to_string());
    }

    if top.get("result").and_then(Value::as_str) == Some("pass") {
        if top.get("observed_at").and_then(Value::as_str).is_none()
            || !["version", "channel", "build"].iter().all(|key| zed.get(*key).and_then(Value::as_str).is_some())
            || !["base_commit", "candidate_commit", "manifest_version", "wasm_sha256", "install_route"].iter().all(|key| extension.get(*key).and_then(Value::as_str).is_some())
            || !["command", "version", "build_commit", "binary_sha256", "resolution_route"].iter().all(|key| perllsp.get(*key).and_then(Value::as_str).is_some())
            || perllsp.get("arguments") != Some(&serde_json::json!(["--stdio"]))
            || profile.get("clean_profile").and_then(Value::as_bool) != Some(true)
            || profile.get("other_perl_servers_disabled").and_then(Value::as_bool) != Some(true)
            || !["zed_log", "language_server_log", "process_inventory"].iter().all(|key| artifacts.get(*key).and_then(Value::as_str).is_some())
            || artifacts.get("redacted").and_then(Value::as_bool) != Some(true)
        {
            return Err("pass receipt violates required non-null schema fields".to_string());
        }
    }

    if top.get("evidence_stage").and_then(Value::as_str) == Some("public_registry_install")
        && top.get("result").and_then(Value::as_str) == Some("pass")
    {
        let subject = top.get("public_subject").ok_or_else(|| "public pass lacks public_subject".to_string())?;
        let subject = object(subject, "public_subject")?;
        if subject.get("relative_path").and_then(Value::as_str) != Some(PUBLIC_SUBJECT_RELATIVE_PATH)
            || !subject.get("sha256").and_then(Value::as_str).is_some_and(is_sha256_digest)
            || perllsp.get("resolution_route").and_then(Value::as_str) != Some("managed_download")
            || profile.get("prior_extension_absent").and_then(Value::as_bool) != Some(true)
            || profile.get("prior_managed_cache_absent").and_then(Value::as_bool) != Some(true)
        {
            return Err("public pass violates public-registry schema conditions".to_string());
        }
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
