//! Contract and receipt validation for the new Zed managed route (#8753).
//!
//! The managed route requires that the extension resolves `perllsp` through
//! the managed public artifact only: no explicit binary override, no
//! worktree/PATH candidate, and no provider fallback. The contract also owns
//! the closed set of known-good cache-recovery scenarios; a receipt may only
//! claim `pass` when every required scenario is accounted for and the exact
//! subject digests are recorded.
//!
//! This module is infrastructure authority only: a template receipt ships as
//! `not_run`, and the successor evidence issue performs the real Zed runs.

use serde_json::Value;
use std::collections::BTreeSet;

/// The only resolution route the contract admits: the managed public artifact.
pub const MANAGED_PUBLIC_ARTIFACT: &str = "managed_public_artifact";

/// The exact server command the managed route must select.
pub const SERVER_COMMAND: &str = "perllsp --stdio";

/// The first-mile row requires the prior managed cache to be absent.
pub const PRIOR_MANAGED_CACHE_ABSENT: &str = "prior_managed_cache_absent";

/// Older managed versions stay in the cache until the next launch observes them.
pub const OLDER_VERSIONS_PRESERVED_UNTIL_LAUNCH: &str = "older_versions_preserved_until_launch";

/// The complete set of known-good cache-recovery scenarios the contract owns.
pub const REQUIRED_RECOVERY_SCENARIOS: [&str; 7] = [
    "missing_asset",
    "duplicate_matching_asset",
    "wrong_target",
    "checksum_mismatch",
    "unsafe_archive_member",
    "missing_expected_executable",
    "partial_download",
];

/// Journeys a `pass` receipt must have observed.
pub const REQUIRED_JOURNEYS: [&str; 4] =
    ["first_mile_install", "restart_cache_reuse", "normal_disable", "shutdown_no_orphan"];

fn text<'a>(value: &'a Value, pointer: &str) -> Option<&'a str> {
    value.pointer(pointer).and_then(Value::as_str).filter(|text| !text.trim().is_empty())
}

fn digest(value: &Value, pointer: &str) -> bool {
    value.pointer(pointer).and_then(Value::as_str).is_some_and(|value| {
        value.starts_with("sha256:")
            && value.len() == 71
            && value[7..].bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

fn opt_digest(value: &Value, pointer: &str) -> bool {
    match value.pointer(pointer) {
        None | Some(Value::Null) => true,
        Some(_) => digest(value, pointer),
    }
}

fn required_flag(invariants: &Value, pointer: &str) -> Result<(), String> {
    invariants
        .pointer(pointer)
        .and_then(Value::as_bool)
        .filter(|flag| *flag)
        .map(|_| ())
        .ok_or_else(|| format!("failure invariant `{pointer}` must be true"))
}

/// Validate the managed-route contract document.
pub fn validate_contract(contract: &Value) -> Result<(), String> {
    if text(contract, "/contract").is_none() {
        return Err("contract document lacks a `contract` identity".to_string());
    }
    let route = text(contract, "/resolution_route")
        .ok_or_else(|| "contract lacks `resolution_route`".to_string())?;
    if route != MANAGED_PUBLIC_ARTIFACT {
        return Err(format!(
            "resolution_route must be `{MANAGED_PUBLIC_ARTIFACT}`, found `{route}`"
        ));
    }

    for pointer in [
        "/claim/explicit_binary_override",
        "/claim/worktree_path_candidate",
        "/claim/path_candidate",
    ] {
        if text(contract, pointer) != Some("absent") {
            return Err(format!("`{pointer}` must be `absent`"));
        }
    }
    if text(contract, "/claim/server_command") != Some(SERVER_COMMAND) {
        return Err(format!("server_command must be `{SERVER_COMMAND}`"));
    }
    if text(contract, "/claim/selected_provider") != Some("perllsp") {
        return Err("selected provider must be `perllsp`".to_string());
    }
    if text(contract, "/claim/other_providers") != Some("disabled") {
        return Err("other providers must be `disabled`".to_string());
    }
    if text(contract, "/first_mile/prior_managed_cache") != Some(PRIOR_MANAGED_CACHE_ABSENT) {
        return Err("first mile requires `prior_managed_cache_absent`".to_string());
    }
    if contract.pointer("/selection/older_versions_preserved_until_launch").and_then(Value::as_bool)
        != Some(true)
    {
        return Err(format!("`{OLDER_VERSIONS_PRESERVED_UNTIL_LAUNCH}` must be true"));
    }

    let invariants = contract
        .get("failure_invariants")
        .ok_or_else(|| "contract lacks `failure_invariants`".to_string())?;
    for pointer in
        ["/provider_fallback_forbidden", "/path_route_forbidden", "/worktree_route_forbidden"]
    {
        required_flag(invariants, pointer)?;
    }

    let scenarios = contract
        .get("recovery_scenarios")
        .and_then(Value::as_array)
        .ok_or_else(|| "contract lacks a `recovery_scenarios` array".to_string())?;
    let present: BTreeSet<String> =
        scenarios.iter().filter_map(Value::as_str).map(str::to_string).collect();
    if present.len() != scenarios.len() {
        return Err("recovery_scenarios contains duplicates".to_string());
    }
    for required in REQUIRED_RECOVERY_SCENARIOS {
        if !present.contains(required) {
            return Err(format!("known-good recovery scenario `{required}` is missing"));
        }
    }
    if scenarios.len() != REQUIRED_RECOVERY_SCENARIOS.len() {
        return Err("recovery_scenarios must be exactly the known-good set".to_string());
    }
    Ok(())
}

/// Validate a managed-route receipt against its contract.
///
/// A `not_run` template must not claim any observation. A `pass` receipt must
/// name the managed route (never a worktree/PATH fallback), record the exact
/// subject digests for first mile and restart, carry every required journey,
/// and keep the claim boundary honest about what was actually proven.
pub fn validate_receipt(receipt: &Value, contract: &Value) -> Result<(), String> {
    validate_contract(contract)?;
    if text(receipt, "/receipt").is_none() {
        return Err("receipt lacks a `receipt` identity".to_string());
    }
    let result = text(receipt, "/result").ok_or_else(|| "receipt lacks `result`".to_string())?;
    if !matches!(result, "not_run" | "pass" | "mismatch" | "unsupported" | "not_proven") {
        return Err(format!("unknown receipt result `{result}`"));
    }

    if result == "not_run" {
        if receipt.get("observed_at").is_none_or(Value::is_null) {
            // A not_run template must not claim an observation time or boundary.
        } else {
            return Err("a not_run receipt must not carry `observed_at`".to_string());
        }
        if text(receipt, "/claim_boundary/official_registry")
            .is_some_and(|value| value != "not_proven")
        {
            return Err("official registry must stay not_proven on a not_run receipt".to_string());
        }
        return Ok(());
    }

    if receipt.get("observed_at").is_none_or(Value::is_null) {
        return Err(format!("a `{result}` receipt must carry `observed_at`"));
    }
    if !digest(receipt, "/contract/sha256") {
        return Err("receipt must record the contract sha256 digest".to_string());
    }

    let contract_route = text(contract, "/resolution_route").unwrap_or_default();
    let receipt_route = text(receipt, "/selection/resolution_route")
        .ok_or_else(|| "receipt lacks `selection.resolution_route`".to_string())?;
    if receipt_route != contract_route {
        return Err(format!(
            "receipt resolution_route `{receipt_route}` does not satisfy the contract route \
             `{contract_route}`"
        ));
    }
    if text(receipt, "/selection/selected_provider") != Some("perllsp") {
        return Err("receipt must record `perllsp` as the selected provider".to_string());
    }
    if !receipt.pointer("/selection/fallback_server_id").is_none_or(Value::is_null) {
        return Err("receipt must record no fallback server id".to_string());
    }
    if receipt.pointer("/selection/prior_managed_cache_absent").and_then(Value::as_bool)
        != Some(true)
    {
        return Err(
            "receipt must record `prior_managed_cache_absent` for the first mile".to_string()
        );
    }
    if !digest(receipt, "/selection/selected_subject_sha256") {
        return Err("receipt must record `selected_subject_sha256`".to_string());
    }
    if !opt_digest(receipt, "/selection/restart_subject_sha256") {
        return Err("`restart_subject_sha256` must be a sha256 digest when present".to_string());
    }

    for journey in REQUIRED_JOURNEYS {
        if text(receipt, &format!("/journeys/{journey}")).is_none() {
            return Err(format!("`{result}` receipt must record journey `{journey}`"));
        }
    }

    if text(receipt, "/claim_boundary/real_zed_managed_route") != Some("proven_for_exact_subject") {
        return Err("a pass receipt must bound its claim to `proven_for_exact_subject`".to_string());
    }
    Ok(())
}
