use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

const REQUIRED_HOST_ROLES: &[&str] = &[
    "project_only",
    "zed_override",
    "zed_override_removed",
    "live_edit",
];

fn nonempty(value: &Value, pointer: &str) -> bool {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .is_some_and(|text| !text.trim().is_empty())
}

fn is_sha256(value: &Value, pointer: &str) -> bool {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .is_some_and(|text| {
            text.len() == "sha256:".len() + 64
                && text.starts_with("sha256:")
                && text["sha256:".len()..]
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
        })
}

fn probe_map<'a>(value: &'a Value, pointer: &str) -> Result<BTreeMap<&'a str, &'a Value>, String> {
    let probes = value
        .pointer(pointer)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("missing probe array at `{pointer}`"))?;
    let mut indexed = BTreeMap::new();
    for probe in probes {
        let id = probe
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("probe at `{pointer}` lacks id"))?;
        if indexed.insert(id, probe).is_some() {
            return Err(format!("duplicate probe id `{id}`"));
        }
    }
    Ok(indexed)
}

pub fn validate_contract(contract: &Value, canonical_schema: &Value) -> Result<(), String> {
    if contract.get("schema_version").and_then(Value::as_str)
        != Some("zed_settings_behavior_contract.v1")
    {
        return Err("wrong settings behavior contract schema".to_string());
    }
    if contract.get("host_receipt_schema").and_then(Value::as_str) != Some("zed_host_compat.v1")
        || contract.get("host_evidence_stage").and_then(Value::as_str)
            != Some("exact_source_dev_extension")
    {
        return Err("settings contract has the wrong host evidence authority".to_string());
    }
    if contract.get("zed_process_prefix").and_then(Value::as_str) != Some("lsp.perllsp.binary")
        || contract.get("server_settings_prefix").and_then(Value::as_str)
            != Some("lsp.perllsp.settings.perl")
    {
        return Err("process and server settings authorities are not separated".to_string());
    }
    if contract
        .pointer("/claim_boundary/settings_behavior")
        .and_then(Value::as_str)
        != Some("not_run")
        || contract
            .pointer("/claim_boundary/full_zed_support")
            .and_then(Value::as_str)
            != Some("not_proven")
        || contract
            .pointer("/claim_boundary/public_registry")
            .and_then(Value::as_str)
            != Some("not_proven")
    {
        return Err("static settings contract overclaims behavior or public support".to_string());
    }

    let probes = probe_map(contract, "/probes")?;
    if probes.len() != 5 {
        return Err("settings contract must retain five typed probes".to_string());
    }
    let required_types = BTreeSet::from(["boolean", "string", "integer", "array"]);
    let observed_types = probes
        .values()
        .filter_map(|probe| probe.get("expected_type").and_then(Value::as_str))
        .collect::<BTreeSet<_>>();
    if !required_types.is_subset(&observed_types) {
        return Err("settings probes do not cover boolean, enum/string, integer, and array".to_string());
    }

    for (id, probe) in probes {
        let key = probe
            .get("key")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("probe `{id}` lacks key"))?;
        if !key.starts_with("perl.") || key.starts_with("perl.binary") || key.contains("binary.") {
            return Err(format!("probe `{id}` is outside canonical perl.* server settings"));
        }
        let pointer = probe
            .get("schema_pointer")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("probe `{id}` lacks schema pointer"))?;
        let schema_node = canonical_schema
            .pointer(pointer)
            .ok_or_else(|| format!("probe `{id}` is absent from the canonical schema"))?;
        let expected_type = probe
            .get("expected_type")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("probe `{id}` lacks expected type"))?;
        if schema_node.get("type").and_then(Value::as_str) != Some(expected_type) {
            return Err(format!("probe `{id}` type disagrees with canonical schema"));
        }
        if !nonempty(probe, "/observable") {
            return Err(format!("probe `{id}` lacks an observable effect"));
        }
        if expected_type == "string" {
            let allowed = schema_node
                .get("enum")
                .and_then(Value::as_array)
                .ok_or_else(|| format!("enum probe `{id}` lacks canonical enum"))?;
            for field in ["project_value", "zed_value"] {
                let value = probe
                    .get(field)
                    .ok_or_else(|| format!("probe `{id}` lacks {field}"))?;
                if !allowed.contains(value) {
                    return Err(format!("probe `{id}` {field} is outside the canonical enum"));
                }
            }
        }
    }

    let sequence = contract
        .get("precedence_sequence")
        .and_then(Value::as_array)
        .ok_or_else(|| "missing precedence sequence".to_string())?;
    if sequence != &serde_json::json!([
        "project_only",
        "zed_override",
        "zed_override_removed"
    ])
    .as_array()
    .expect("literal array")
    {
        return Err("unexpected precedence experiment sequence".to_string());
    }
    Ok(())
}

pub fn validate_receipt(receipt: &Value, contract: &Value) -> Result<(), String> {
    if receipt.get("schema_version").and_then(Value::as_str)
        != Some("zed_settings_behavior_receipt.v1")
    {
        return Err("wrong settings behavior receipt schema".to_string());
    }
    if receipt
        .pointer("/claim_boundary/full_zed_support")
        .and_then(Value::as_str)
        != Some("not_proven")
        || receipt
            .pointer("/claim_boundary/public_registry")
            .and_then(Value::as_str)
            != Some("not_proven")
    {
        return Err("settings receipt overclaims full or public Zed support".to_string());
    }
    for field in [
        "binary_path_forwarded_to_server",
        "binary_arguments_forwarded_to_server",
        "binary_environment_forwarded_to_server",
    ] {
        if receipt
            .pointer(&format!("/process_settings/{field}"))
            .and_then(Value::as_bool)
            != Some(false)
        {
            return Err(format!("process setting `{field}` leaked into server configuration"));
        }
    }

    let result = receipt
        .get("result")
        .and_then(Value::as_str)
        .ok_or_else(|| "settings receipt lacks result".to_string())?;
    if result == "not_run" {
        if receipt
            .pointer("/claim_boundary/settings_behavior")
            .and_then(Value::as_str)
            != Some("not_run")
        {
            return Err("not-run settings receipt has a promoted behavior cell".to_string());
        }
        return Ok(());
    }
    if result != "pass" {
        return Err("only not_run or pass candidates are accepted by this validator".to_string());
    }
    if !nonempty(receipt, "/observed_at") || !is_sha256(receipt, "/contract/sha256") {
        return Err("passing settings receipt lacks time or contract identity".to_string());
    }
    if receipt
        .pointer("/claim_boundary/settings_behavior")
        .and_then(Value::as_str)
        != Some("proven_for_exact_subject")
    {
        return Err("passing settings receipt lacks exact-subject claim boundary".to_string());
    }

    let host_receipts = receipt
        .get("host_receipts")
        .and_then(Value::as_array)
        .ok_or_else(|| "passing settings receipt lacks host receipts".to_string())?;
    let mut roles = BTreeSet::new();
    let mut host_identity: Option<&str> = None;
    for row in host_receipts {
        let role = row
            .get("role")
            .and_then(Value::as_str)
            .ok_or_else(|| "host receipt row lacks role".to_string())?;
        if !roles.insert(role) {
            return Err(format!("duplicate host receipt role `{role}`"));
        }
        if row.get("schema_version").and_then(Value::as_str) != Some("zed_host_compat.v1")
            || row.get("evidence_stage").and_then(Value::as_str)
                != Some("exact_source_dev_extension")
            || row.get("result").and_then(Value::as_str) != Some("pass")
            || !is_sha256(row, "/receipt_sha256")
            || !is_sha256(row, "/settings_sha256")
            || !is_sha256(row, "/host_identity_sha256")
        {
            return Err(format!("host receipt role `{role}` lacks exact passing identity"));
        }
        let identity = row
            .get("host_identity_sha256")
            .and_then(Value::as_str)
            .expect("checked above");
        if let Some(expected) = host_identity {
            if identity != expected {
                return Err("settings experiment mixed different host subjects".to_string());
            }
        } else {
            host_identity = Some(identity);
        }
    }
    if roles != REQUIRED_HOST_ROLES.iter().copied().collect() {
        return Err("settings experiment lacks the exact required host roles".to_string());
    }

    let contract_probes = probe_map(contract, "/probes")?;
    let receipt_probes = probe_map(receipt, "/probes")?;
    if contract_probes.keys().copied().collect::<BTreeSet<_>>()
        != receipt_probes.keys().copied().collect::<BTreeSet<_>>()
    {
        return Err("settings receipt probe population differs from the contract".to_string());
    }
    for (id, expected) in contract_probes {
        let observed = receipt_probes
            .get(id)
            .copied()
            .ok_or_else(|| format!("missing receipt probe `{id}`"))?;
        if observed.get("key") != expected.get("key")
            || observed.get("result").and_then(Value::as_str) != Some("pass")
            || observed.get("project_observed") != expected.get("project_value")
            || observed.get("zed_override_observed") != expected.get("zed_value")
            || observed.get("restored_observed") != expected.get("project_value")
            || !nonempty(observed, "/effect_before")
            || !nonempty(observed, "/effect_override")
            || !nonempty(observed, "/effect_restored")
            || !nonempty(observed, "/evidence")
        {
            return Err(format!("settings probe `{id}` lacks direct before/override/restored proof"));
        }
        if observed.get("effect_before") == observed.get("effect_override")
            || observed.get("effect_before") != observed.get("effect_restored")
        {
            return Err(format!("settings probe `{id}` does not show reversible behavior"));
        }
    }

    let precedence = receipt
        .get("precedence")
        .ok_or_else(|| "settings receipt lacks precedence result".to_string())?;
    if precedence.get("result").and_then(Value::as_str) != Some("pass")
        || precedence.get("sequence") != contract.get("precedence_sequence")
        || !nonempty(precedence, "/project_only_effect")
        || !nonempty(precedence, "/zed_override_effect")
        || !nonempty(precedence, "/restored_project_effect")
        || !nonempty(precedence, "/evidence")
        || precedence.get("project_only_effect") == precedence.get("zed_override_effect")
        || precedence.get("project_only_effect") != precedence.get("restored_project_effect")
    {
        return Err("settings precedence was not reversibly behavior-proven".to_string());
    }

    let restart = receipt
        .get("restart")
        .ok_or_else(|| "settings receipt lacks restart result".to_string())?;
    let disposition = restart
        .get("disposition")
        .and_then(Value::as_str)
        .ok_or_else(|| "settings restart disposition is missing".to_string())?;
    let allowed = contract
        .get("restart_dispositions")
        .and_then(Value::as_array)
        .ok_or_else(|| "contract lacks restart dispositions".to_string())?;
    if restart.get("result").and_then(Value::as_str) != Some("pass")
        || !allowed.contains(&Value::String(disposition.to_string()))
        || matches!(disposition, "no_effect" | "instrument_failed")
        || !nonempty(restart, "/effect_before")
        || !nonempty(restart, "/effect_after")
        || restart.get("effect_before") == restart.get("effect_after")
        || !nonempty(restart, "/evidence")
    {
        return Err("settings live/restart behavior is not proven".to_string());
    }
    let before = restart.get("server_pid_before").and_then(Value::as_u64);
    let after = restart.get("server_pid_after").and_then(Value::as_u64);
    match disposition {
        "live_configuration" => {
            if restart
                .get("configuration_notification_observed")
                .and_then(Value::as_bool)
                != Some(true)
                || before.is_none()
                || before != after
            {
                return Err("live configuration requires a notification and stable process".to_string());
            }
        }
        "zed_managed_restart" | "manual_restart" => {
            if before.is_none() || after.is_none() || before == after {
                return Err("restart disposition requires a changed exact server process".to_string());
            }
        }
        _ => return Err("passing settings receipt has an unsupported disposition".to_string()),
    }

    Ok(())
}
