use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

fn text<'a>(value: &'a Value, pointer: &str) -> Option<&'a str> {
    value.pointer(pointer).and_then(Value::as_str).filter(|text| !text.trim().is_empty())
}

fn digest(value: &Value, pointer: &str) -> bool {
    text(value, pointer).is_some_and(|value| {
        value.starts_with("sha256:")
            && value.len() == 71
            && value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

fn probes(value: &Value) -> Result<BTreeMap<String, &Value>, String> {
    let rows = value
        .get("probes")
        .and_then(Value::as_array)
        .ok_or_else(|| "missing probes".to_string())?;
    let mut result = BTreeMap::new();
    for row in rows {
        let id =
            row.get("id").and_then(Value::as_str).ok_or_else(|| "probe lacks id".to_string())?;
        if result.insert(id.to_string(), row).is_some() {
            return Err(format!("duplicate probe `{id}`"));
        }
    }
    Ok(result)
}

pub fn validate_contract(contract: &Value, schema: &Value) -> Result<(), String> {
    if contract.get("schema_version").and_then(Value::as_str)
        != Some("zed_settings_behavior_contract.v1")
        || contract.get("host_receipt_schema").and_then(Value::as_str) != Some("zed_host_compat.v1")
        || contract.get("host_evidence_stage").and_then(Value::as_str)
            != Some("exact_source_dev_extension")
    {
        return Err("wrong settings contract identity".to_string());
    }
    if contract.get("zed_process_prefix").and_then(Value::as_str) != Some("lsp.perllsp.binary")
        || contract.get("server_settings_prefix").and_then(Value::as_str)
            != Some("lsp.perllsp.settings.perl")
    {
        return Err("process/server settings authorities are mixed".to_string());
    }
    for (pointer, expected) in [
        ("/claim_boundary/settings_behavior", "not_run"),
        ("/claim_boundary/full_zed_support", "not_proven"),
        ("/claim_boundary/public_registry", "not_proven"),
    ] {
        if text(contract, pointer) != Some(expected) {
            return Err(format!("contract overclaims `{pointer}`"));
        }
    }

    let rows = probes(contract)?;
    if rows.len() != 5 {
        return Err("expected five typed settings probes".to_string());
    }
    let mut types = BTreeSet::new();
    for (id, row) in rows {
        let key = row.get("key").and_then(Value::as_str).unwrap_or_default();
        let pointer = row
            .get("schema_pointer")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("probe `{id}` lacks schema pointer"))?;
        let expected_type = row
            .get("expected_type")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("probe `{id}` lacks expected type"))?;
        if !key.starts_with("perl.")
            || key.contains("binary.")
            || text(row, "/observable").is_none()
        {
            return Err(format!("probe `{id}` is not a canonical observable server setting"));
        }
        let node = schema
            .pointer(pointer)
            .ok_or_else(|| format!("probe `{id}` is absent from the canonical schema"))?;
        if node.get("type").and_then(Value::as_str) != Some(expected_type) {
            return Err(format!("probe `{id}` type drift"));
        }
        if expected_type == "string" {
            let allowed = node
                .get("enum")
                .and_then(Value::as_array)
                .ok_or_else(|| format!("probe `{id}` lacks a canonical enum"))?;
            if !allowed.contains(row.get("project_value").unwrap_or(&Value::Null))
                || !allowed.contains(row.get("zed_value").unwrap_or(&Value::Null))
            {
                return Err(format!("probe `{id}` uses a non-canonical enum value"));
            }
        }
        types.insert(expected_type);
    }
    if !["boolean", "string", "integer", "array"].iter().all(|kind| types.contains(kind)) {
        return Err("typed settings denominator is incomplete".to_string());
    }
    if contract.get("precedence_sequence")
        != Some(&serde_json::json!(["project_only", "zed_override", "zed_override_removed"]))
    {
        return Err("wrong precedence sequence".to_string());
    }
    Ok(())
}

pub fn validate_receipt(receipt: &Value, contract: &Value) -> Result<(), String> {
    if receipt.get("schema_version").and_then(Value::as_str)
        != Some("zed_settings_behavior_receipt.v1")
    {
        return Err("wrong settings receipt schema".to_string());
    }
    for field in [
        "binary_path_forwarded_to_server",
        "binary_arguments_forwarded_to_server",
        "binary_environment_forwarded_to_server",
    ] {
        if receipt.pointer(&format!("/process_settings/{field}")).and_then(Value::as_bool)
            != Some(false)
        {
            return Err(format!("process field `{field}` leaked to the server"));
        }
    }
    if text(receipt, "/claim_boundary/full_zed_support") != Some("not_proven")
        || text(receipt, "/claim_boundary/public_registry") != Some("not_proven")
    {
        return Err("settings receipt overclaims support".to_string());
    }
    let result = receipt.get("result").and_then(Value::as_str).unwrap_or_default();
    if result == "not_run" {
        return (text(receipt, "/claim_boundary/settings_behavior") == Some("not_run"))
            .then_some(())
            .ok_or_else(|| "not-run receipt promoted behavior".to_string());
    }
    if result != "pass"
        || text(receipt, "/observed_at").is_none()
        || !digest(receipt, "/contract/sha256")
        || text(receipt, "/claim_boundary/settings_behavior") != Some("proven_for_exact_subject")
    {
        return Err("passing settings receipt lacks exact identity".to_string());
    }

    let host_rows = receipt
        .get("host_receipts")
        .and_then(Value::as_array)
        .ok_or_else(|| "missing host receipts".to_string())?;
    let mut roles = BTreeSet::new();
    let mut identity: Option<&str> = None;
    for row in host_rows {
        let role = row.get("role").and_then(Value::as_str).unwrap_or_default();
        roles.insert(role);
        let current = text(row, "/host_identity_sha256");
        if row.get("schema_version").and_then(Value::as_str) != Some("zed_host_compat.v1")
            || row.get("evidence_stage").and_then(Value::as_str)
                != Some("exact_source_dev_extension")
            || row.get("result").and_then(Value::as_str) != Some("pass")
            || !digest(row, "/receipt_sha256")
            || !digest(row, "/settings_sha256")
            || current.is_none()
            || identity.is_some_and(|expected| Some(expected) != current)
        {
            return Err(format!("host role `{role}` lacks a matching exact receipt"));
        }
        identity = current;
    }
    let expected_roles =
        BTreeSet::from(["project_only", "zed_override", "zed_override_removed", "live_edit"]);
    if roles != expected_roles {
        return Err("wrong host receipt role population".to_string());
    }

    let expected = probes(contract)?;
    let observed = probes(receipt)?;
    if expected.keys().collect::<Vec<_>>() != observed.keys().collect::<Vec<_>>() {
        return Err("probe population drift".to_string());
    }
    for (id, contract_row) in expected {
        let row = observed.get(&id).copied().ok_or_else(|| format!("missing `{id}`"))?;
        if row.get("key") != contract_row.get("key")
            || row.get("result").and_then(Value::as_str) != Some("pass")
            || row.get("project_observed") != contract_row.get("project_value")
            || row.get("zed_override_observed") != contract_row.get("zed_value")
            || row.get("restored_observed") != contract_row.get("project_value")
            || text(row, "/effect_before").is_none()
            || text(row, "/effect_override").is_none()
            || text(row, "/effect_restored").is_none()
            || text(row, "/evidence").is_none()
            || row.get("effect_before") == row.get("effect_override")
            || row.get("effect_before") != row.get("effect_restored")
        {
            return Err(format!("probe `{id}` lacks reversible behavior proof"));
        }
    }

    let precedence = receipt.get("precedence").unwrap_or(&Value::Null);
    if precedence.get("result").and_then(Value::as_str) != Some("pass")
        || precedence.get("sequence") != contract.get("precedence_sequence")
        || text(precedence, "/evidence").is_none()
        || precedence.get("project_only_effect") == precedence.get("zed_override_effect")
        || precedence.get("project_only_effect") != precedence.get("restored_project_effect")
    {
        return Err("precedence is not reversibly proven".to_string());
    }

    let restart = receipt.get("restart").unwrap_or(&Value::Null);
    let disposition = restart.get("disposition").and_then(Value::as_str).unwrap_or_default();
    let before = restart.get("server_pid_before").and_then(Value::as_u64);
    let after = restart.get("server_pid_after").and_then(Value::as_u64);
    if restart.get("result").and_then(Value::as_str) != Some("pass")
        || text(restart, "/effect_before").is_none()
        || text(restart, "/effect_after").is_none()
        || text(restart, "/evidence").is_none()
        || restart.get("effect_before") == restart.get("effect_after")
    {
        return Err("live/restart effect is not proven".to_string());
    }
    match disposition {
        "live_configuration"
            if before.is_some()
                && before == after
                && restart.get("configuration_notification_observed").and_then(Value::as_bool)
                    == Some(true) =>
        {
            Ok(())
        }
        "zed_managed_restart" | "manual_restart"
            if before.is_some() && after.is_some() && before != after =>
        {
            Ok(())
        }
        _ => Err("passing receipt has no valid live/restart disposition".to_string()),
    }
}
