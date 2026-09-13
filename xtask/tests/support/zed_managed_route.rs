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

use chrono::DateTime;
use serde_json::Value;
use std::collections::BTreeSet;

pub const CONTRACT_ID: &str = "zed_managed_route.v1";
pub const CONTRACT_REVISION: u64 = 1;
pub const RECEIPT_ID: &str = "zed_managed_route_receipt.v1";
pub const CONTRACT_RELATIVE_PATH: &str = ".ci/fixtures/zed-perl-upstream/managed-route.v1.json";

/// The only resolution route the contract admits: the managed public artifact.
pub const MANAGED_PUBLIC_ARTIFACT: &str = "managed_public_artifact";

/// The exact server command the managed route must select.
pub const SERVER_COMMAND: &str = "perllsp --stdio";

/// The first-mile row requires the prior managed cache to be absent.
pub const PRIOR_MANAGED_CACHE_ABSENT: &str = "prior_managed_cache_absent";

/// Older managed versions stay in the cache until the next launch observes them.
pub const OLDER_VERSIONS_PRESERVED_UNTIL_LAUNCH: &str = "older_versions_preserved_until_launch";

/// The complete set of known-good cache-recovery scenarios the contract owns.
pub const REQUIRED_RECOVERY_SCENARIOS: [&str; 9] = [
    "missing_asset",
    "duplicate_matching_asset",
    "wrong_target",
    "checksum_mismatch",
    "unsafe_archive_member",
    "missing_expected_executable",
    "partial_download",
    "extraction_failure",
    "launch_failure",
];

/// Journeys a `pass` receipt must have observed.
pub const REQUIRED_JOURNEYS: [&str; 4] =
    ["first_mile_install", "restart_cache_reuse", "normal_disable", "shutdown_no_orphan"];

const RECOVERY_FACTS: [&str; 11] = [
    "failure_scenario",
    "known_good_before_sha256",
    "known_good_after_sha256",
    "restored_subject_sha256",
    "failed_candidate_identity",
    "failed_candidate_selected",
    "fallback_server_id",
    "rejection_reason",
    "restored_result",
    "evidence",
    "evidence_record",
];

fn journey_facts(journey: &str) -> &'static [&'static str] {
    match journey {
        "first_mile_install" => &[
            "cache_identity",
            "binary_sha256",
            "command",
            "arguments",
            "running_perllsp_processes",
            "cache_evidence",
            "process_evidence",
        ],
        "restart_cache_reuse" => &[
            "cache_identity",
            "binary_sha256",
            "command",
            "arguments",
            "running_perllsp_processes",
            "downloads",
            "cache_evidence",
            "download_evidence",
            "process_evidence",
        ],
        "normal_disable" => {
            &["cache_identity", "binary_sha256", "cache_retained", "cache_evidence"]
        }
        "shutdown_no_orphan" => &["remaining_perllsp_processes", "process_evidence"],
        _ => &[],
    }
}

fn closed_object(value: &Value, fields: &[&str], label: &str) -> Result<(), String> {
    let object = value.as_object().ok_or_else(|| format!("{label} must be an object"))?;
    if object.len() != fields.len() || fields.iter().any(|field| !object.contains_key(*field)) {
        return Err(format!("{label} must contain exactly its required fields"));
    }
    Ok(())
}

fn observation_shape(row: &Value, facts: &[&str], result: &str) -> Result<(), String> {
    let fields: Vec<&str> = std::iter::once("result").chain(facts.iter().copied()).collect();
    closed_object(row, &fields, "observation")?;
    if text(row, "/result") != Some(result)
        || (result == "not_run" && facts.iter().any(|field| row.get(*field) != Some(&Value::Null)))
    {
        return Err(format!("observation must be {result} with matching evidence fields"));
    }
    Ok(())
}

const REQUIRED_FAILURE_INVARIANTS: [&str; 7] = [
    "provider_fallback_forbidden",
    "path_route_forbidden",
    "worktree_route_forbidden",
    "binary_override_forbidden",
    "partial_download_install_forbidden",
    "unsafe_archive_member_forbidden",
    "checksum_mismatch_install_forbidden",
];

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

fn null_or_missing(value: &Value, pointer: &str) -> bool {
    value.pointer(pointer).is_none_or(Value::is_null)
}

fn required_text(value: &Value, pointer: &str) -> Result<(), String> {
    text(value, pointer)
        .map(|_| ())
        .ok_or_else(|| format!("`{pointer}` must be a non-empty string"))
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
    closed_object(
        contract,
        &[
            "contract",
            "revision",
            "claim",
            "first_mile",
            "resolution_route",
            "selection",
            "failure_invariants",
            "recovery_scenarios",
            "required_journeys",
            "digests",
        ],
        "contract",
    )?;
    for (section, fields) in [
        (
            "claim",
            &[
                "route",
                "explicit_binary_override",
                "worktree_path_candidate",
                "path_candidate",
                "selected_provider",
                "other_providers",
                "server_command",
                "asset_subject_authority",
                "host_subject_authority",
            ][..],
        ),
        ("first_mile", &["prior_managed_cache", "first_mile_row"][..]),
        (
            "selection",
            &[
                "selected",
                "fallback_server_id",
                "fallback_allowed",
                "older_versions_preserved_until_launch",
                "cache_reuse_after_restart",
                "normal_disable",
                "shutdown",
            ][..],
        ),
        (
            "digests",
            &["selected_subject_sha256", "restart_subject_sha256", "asset_sha256", "binary_sha256"]
                [..],
        ),
    ] {
        closed_object(
            contract.get(section).ok_or_else(|| format!("missing {section}"))?,
            fields,
            section,
        )?;
    }
    for (pointer, expected) in [
        ("/claim/route", "extension route = managed public artifact"),
        ("/first_mile/first_mile_row", "cache-absent install through the managed route only"),
        ("/selection/selected", "perllsp"),
        ("/selection/cache_reuse_after_restart", "managed cache reused without re-download"),
        ("/selection/normal_disable", "extension disabled without deleting managed cache"),
        ("/selection/shutdown", "no orphan perllsp process after shutdown"),
        (
            "/digests/selected_subject_sha256",
            "sha256: exact installed binary selected by the managed route",
        ),
        (
            "/digests/restart_subject_sha256",
            "sha256: recorded after restart for cache-reuse parity",
        ),
        (
            "/digests/asset_sha256",
            "sha256: downloaded archive bytes from the selected public asset",
        ),
        (
            "/digests/binary_sha256",
            "sha256: extracted executable bytes, independently bound to the host receipt",
        ),
    ] {
        if text(contract, pointer) != Some(expected) {
            return Err(format!("{pointer} must be `{expected}`"));
        }
    }
    if text(contract, "/contract") != Some(CONTRACT_ID) {
        return Err(format!("contract identity must be `{CONTRACT_ID}`"));
    }
    if contract.pointer("/revision").and_then(Value::as_u64) != Some(CONTRACT_REVISION) {
        return Err(format!("contract revision must be {CONTRACT_REVISION}"));
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
    for (pointer, authority) in
        [("/claim/asset_subject_authority", "#7980"), ("/claim/host_subject_authority", "#7984")]
    {
        if text(contract, pointer) != Some(authority) {
            return Err(format!("{pointer} must identify {authority}"));
        }
    }
    if contract.get("required_journeys") != Some(&serde_json::json!(REQUIRED_JOURNEYS)) {
        return Err("contract required_journeys must match the managed journey set".to_string());
    }
    if text(contract, "/first_mile/prior_managed_cache") != Some(PRIOR_MANAGED_CACHE_ABSENT) {
        return Err("first mile requires `prior_managed_cache_absent`".to_string());
    }
    if contract.pointer("/selection/older_versions_preserved_until_launch").and_then(Value::as_bool)
        != Some(true)
    {
        return Err(format!("`{OLDER_VERSIONS_PRESERVED_UNTIL_LAUNCH}` must be true"));
    }
    if contract.pointer("/selection/fallback_allowed").and_then(Value::as_bool) != Some(false) {
        return Err("selection.fallback_allowed must be false".to_string());
    }
    if !null_or_missing(contract, "/selection/fallback_server_id") {
        return Err("selection.fallback_server_id must be absent or null".to_string());
    }

    let invariants = contract
        .get("failure_invariants")
        .ok_or_else(|| "contract lacks `failure_invariants`".to_string())?;
    let invariant_object =
        invariants.as_object().ok_or_else(|| "failure_invariants must be an object".to_string())?;
    for key in invariant_object.keys() {
        if !REQUIRED_FAILURE_INVARIANTS.contains(&key.as_str()) {
            return Err(format!("unknown failure invariant `{key}`"));
        }
    }
    for key in REQUIRED_FAILURE_INVARIANTS {
        required_flag(invariants, &format!("/{key}"))?;
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
/// and keep the claim boundary honest about what was actually proven. This is
/// structural validation; the CLI additionally validates upstream authority and
/// byte/subject bindings before accepting a passing candidate.
pub fn validate_receipt(receipt: &Value, contract: &Value) -> Result<(), String> {
    validate_contract(contract)?;
    if text(receipt, "/receipt") != Some(RECEIPT_ID) {
        return Err(format!("receipt identity must be `{RECEIPT_ID}`"));
    }
    if text(receipt, "/contract/relative_path") != Some(CONTRACT_RELATIVE_PATH) {
        return Err(
            "receipt contract.relative_path does not identify the checked contract".to_string()
        );
    }
    if text(receipt, "/contract/schema_version") != Some(CONTRACT_ID) {
        return Err(
            "receipt contract.schema_version does not identify the checked contract".to_string()
        );
    }
    if text(receipt, "/claim_boundary/official_registry") != Some("not_proven") {
        return Err("official registry must remain not_proven".to_string());
    }
    let result = text(receipt, "/result").ok_or_else(|| "receipt lacks `result`".to_string())?;
    if !matches!(result, "not_run" | "pass" | "mismatch" | "unsupported" | "not_proven") {
        return Err(format!("unknown receipt result `{result}`"));
    }

    if result == "not_run" {
        if !null_or_missing(receipt, "/observed_at") {
            return Err("a not_run receipt must not carry `observed_at`".to_string());
        }
        for pointer in [
            "/contract/sha256",
            "/subject/zed_version",
            "/subject/extension_version",
            "/subject/fixture_id",
            "/subject/asset_sha256",
            "/subject/binary_sha256",
            "/subject/zed_build",
            "/subject/extension_candidate_commit",
            "/subject/extension_wasm_sha256",
            "/subject/fixture_sha256",
            "/subject/version",
            "/subject/target",
            "/subject/installed_path",
            "/upstream/asset_receipt_sha256",
            "/upstream/host_receipt_sha256",
            "/selection/resolution_route",
            "/selection/selected_provider",
            "/selection/fallback_server_id",
            "/selection/fallback_allowed",
            "/selection/prior_managed_cache_absent",
            "/selection/selected_subject_sha256",
            "/selection/restart_subject_sha256",
            "/selection/older_versions_preserved_until_launch",
        ] {
            if !null_or_missing(receipt, pointer) {
                return Err(format!("a not_run receipt must not carry `{pointer}`"));
            }
        }
        validate_journeys(receipt, "not_run")?;
        let observations = receipt
            .get("recovery_observations")
            .and_then(Value::as_object)
            .ok_or_else(|| "a not_run receipt must preserve all recovery slots".to_string())?;
        if observations.len() != REQUIRED_RECOVERY_SCENARIOS.len() {
            return Err("a not_run receipt must preserve all recovery slots".to_string());
        }
        for scenario in REQUIRED_RECOVERY_SCENARIOS {
            let row = observations
                .get(scenario)
                .ok_or_else(|| format!("missing structured not_run slot {scenario}"))?;
            observation_shape(row, &RECOVERY_FACTS, "not_run")
                .map_err(|error| format!("not_run recovery slot {scenario}: {error}"))?;
        }
        if text(receipt, "/claim_boundary/real_zed_managed_route") != Some("not_proven") {
            return Err("real Zed route must stay not_proven on a not_run receipt".to_string());
        }
        return Ok(());
    }

    let observed_at = text(receipt, "/observed_at")
        .ok_or_else(|| format!("a `{result}` receipt must carry a non-empty observed_at"))?;
    if DateTime::parse_from_rfc3339(observed_at).is_err() {
        return Err("receipt observed_at must be RFC3339".to_string());
    }
    if !digest(receipt, "/contract/sha256") {
        return Err("receipt must record the contract sha256 digest".to_string());
    }

    // Non-success outcomes are evidence that the route was not proven.  They
    // still identify the observation and contract, but must not be forced to
    // manufacture successful selection/journey data.
    if result != "pass" {
        if text(receipt, "/claim_boundary/real_zed_managed_route") != Some("not_proven") {
            return Err(format!("a `{result}` receipt must keep the real Zed route not_proven"));
        }
        return Ok(());
    }

    for pointer in [
        "/subject/zed_version",
        "/subject/extension_version",
        "/subject/fixture_id",
        "/subject/zed_build",
        "/subject/extension_candidate_commit",
        "/subject/version",
        "/subject/target",
        "/subject/installed_path",
    ] {
        required_text(receipt, pointer)?;
    }
    for pointer in [
        "/subject/asset_sha256",
        "/subject/binary_sha256",
        "/subject/extension_wasm_sha256",
        "/subject/fixture_sha256",
        "/upstream/asset_receipt_sha256",
        "/upstream/host_receipt_sha256",
    ] {
        if !digest(receipt, pointer) {
            return Err(format!("receipt must record exact digest {pointer}"));
        }
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
    if receipt.pointer("/selection/fallback_allowed").and_then(Value::as_bool) != Some(false) {
        return Err("receipt must record fallback_allowed=false".to_string());
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
    if !digest(receipt, "/selection/restart_subject_sha256") {
        return Err("receipt must record `restart_subject_sha256`".to_string());
    }
    if receipt.pointer("/selection/selected_subject_sha256")
        != receipt.pointer("/subject/binary_sha256")
    {
        return Err("selected subject digest must equal the installed binary digest".to_string());
    }
    if receipt.pointer("/selection/restart_subject_sha256")
        != receipt.pointer("/selection/selected_subject_sha256")
    {
        return Err("restart subject digest must equal the selected subject digest".to_string());
    }
    if receipt.pointer("/selection/older_versions_preserved_until_launch").and_then(Value::as_bool)
        != Some(true)
    {
        return Err("receipt must record older versions preserved until launch".to_string());
    }

    validate_journeys(receipt, "pass")?;

    let observations = receipt
        .get("recovery_observations")
        .and_then(Value::as_object)
        .ok_or_else(|| "receipt must carry recovery observations".to_string())?;
    for scenario in REQUIRED_RECOVERY_SCENARIOS {
        let observation = observations
            .get(scenario)
            .ok_or_else(|| format!("missing recovery scenario `{scenario}`"))?;
        validate_recovery(observation, scenario, receipt.pointer("/subject/binary_sha256"))
            .map_err(|error| format!("recovery scenario `{scenario}`: {error}"))?;
    }
    if observations.len() != REQUIRED_RECOVERY_SCENARIOS.len() {
        return Err("recovery observations must be exactly the contract scenario set".to_string());
    }

    if text(receipt, "/claim_boundary/real_zed_managed_route") != Some("proven_for_exact_subject") {
        return Err("a pass receipt must bound its claim to `proven_for_exact_subject`".to_string());
    }
    Ok(())
}

fn validate_journeys(receipt: &Value, result: &str) -> Result<(), String> {
    let journeys = receipt.get("journeys").ok_or_else(|| "missing journeys".to_string())?;
    closed_object(journeys, &REQUIRED_JOURNEYS, "journeys")?;
    for journey in REQUIRED_JOURNEYS {
        let row = journeys.get(journey).ok_or_else(|| format!("missing journey {journey}"))?;
        let facts = journey_facts(journey);
        observation_shape(row, facts, result)
            .map_err(|error| format!("journey {journey}: {error}"))?;
        if result == "not_run" {
            continue;
        }
        for field in facts.iter().filter(|field| field.ends_with("_evidence")) {
            required_text(row, &format!("/{field}"))?;
        }
        if journey == "shutdown_no_orphan" {
            if row.get("remaining_perllsp_processes").and_then(Value::as_u64) != Some(0) {
                return Err("shutdown must leave zero perllsp processes".to_string());
            }
            continue;
        }
        required_text(row, "/cache_identity")?;
        if row.get("cache_identity") != journeys.pointer("/first_mile_install/cache_identity")
            || row.get("binary_sha256") != receipt.pointer("/subject/binary_sha256")
        {
            return Err(format!("journey {journey} must preserve the selected cache and binary"));
        }
        if journey == "normal_disable" {
            if row.get("cache_retained").and_then(Value::as_bool) != Some(true) {
                return Err("normal disable must retain the managed cache".to_string());
            }
            continue;
        }
        required_text(row, "/command")?;
        if row.get("arguments") != Some(&serde_json::json!(["--stdio"]))
            || row.get("running_perllsp_processes").and_then(Value::as_u64) != Some(1)
        {
            return Err(format!(
                "journey {journey} must observe exactly one perllsp --stdio process"
            ));
        }
        if journey == "restart_cache_reuse"
            && row.get("downloads").and_then(Value::as_u64) != Some(0)
        {
            return Err("restart must reuse the managed cache with zero downloads".to_string());
        }
    }
    Ok(())
}

fn validate_recovery(
    observation: &Value,
    scenario: &str,
    selected: Option<&Value>,
) -> Result<(), String> {
    observation_shape(observation, &RECOVERY_FACTS, "pass")?;
    if text(observation, "/failure_scenario") != Some(scenario) {
        return Err("failure_scenario must identify the containing recovery scenario".to_string());
    }
    for pointer in
        ["/known_good_before_sha256", "/known_good_after_sha256", "/restored_subject_sha256"]
    {
        if !digest(observation, pointer) || observation.pointer(pointer) != selected {
            return Err(format!("{pointer} must match the known-good installed binary"));
        }
    }
    if observation.get("failed_candidate_selected").and_then(Value::as_bool) != Some(false)
        || observation.get("fallback_server_id") != Some(&Value::Null)
        || text(observation, "/restored_result") != Some("pass")
    {
        return Err("failed candidate must remain unselected, without fallback, and managed recovery must pass".to_string());
    }
    for pointer in
        ["/failed_candidate_identity", "/rejection_reason", "/evidence", "/evidence_record"]
    {
        required_text(observation, pointer)?;
    }
    Ok(())
}
