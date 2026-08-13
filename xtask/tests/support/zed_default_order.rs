use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

fn text(value: &Value, pointer: &str) -> Option<&str> {
    value.pointer(pointer).and_then(Value::as_str).filter(|text| !text.trim().is_empty())
}

fn digest(value: &Value, pointer: &str) -> bool {
    text(value, pointer).is_some_and(|value| {
        value.starts_with("sha256:")
            && value.len() == 71
            && value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

fn index<'a>(value: &'a Value, pointer: &str) -> Result<BTreeMap<String, &'a Value>, String> {
    let rows = value
        .pointer(pointer)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("missing array at `{pointer}`"))?;
    let mut indexed = BTreeMap::new();
    for row in rows {
        let id = row
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("row at `{pointer}` lacks id"))?;
        if indexed.insert(id.to_string(), row).is_some() {
            return Err(format!("duplicate id `{id}` at `{pointer}`"));
        }
    }
    Ok(indexed)
}

fn strings(value: &Value, field: &str) -> Result<Vec<String>, String> {
    value
        .get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("missing array `{field}`"))?
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_string)
                .ok_or_else(|| format!("`{field}` contains a non-string"))
        })
        .collect()
}

fn exact_set(value: &Value, field: &str, expected: &[&str]) -> Result<(), String> {
    let actual = strings(value, field)?.into_iter().collect::<BTreeSet<_>>();
    let expected = expected
        .iter()
        .map(|item| (*item).to_string())
        .collect::<BTreeSet<_>>();
    (actual == expected)
        .then_some(())
        .ok_or_else(|| format!("`{field}` has the wrong provider set"))
}

pub fn validate_contract(contract: &Value) -> Result<(), String> {
    if contract.get("schema_version").and_then(Value::as_str)
        != Some("zed_default_order_contract.v1")
        || contract.get("host_receipt_schema").and_then(Value::as_str)
            != Some("zed_host_compat.v1")
        || contract.get("host_evidence_stage").and_then(Value::as_str)
            != Some("exact_source_dev_extension")
    {
        return Err("wrong default-order contract identity".to_string());
    }
    if text(contract, "/provider_identities/default") != Some("perlnavigator-server")
        || text(contract, "/provider_identities/tree_sitter_perl") != Some("perl-lsp")
        || text(contract, "/provider_identities/effortlessmetrics") != Some("perllsp")
    {
        return Err("provider identities were collapsed or changed".to_string());
    }
    if contract.get("candidate_order")
        != Some(&serde_json::json!([
            "perlnavigator-server",
            "!perl-lsp",
            "!perllsp",
            "..."
        ]))
    {
        return Err("candidate default order drift".to_string());
    }
    let matrix = index(contract, "/matrix")?;
    let expected_matrix = BTreeSet::from([
        "current_defaults_public_extension".to_string(),
        "candidate_defaults_public_extension".to_string(),
        "current_defaults_candidate_extension".to_string(),
        "candidate_defaults_candidate_extension".to_string(),
    ]);
    if matrix.keys().cloned().collect::<BTreeSet<_>>() != expected_matrix {
        return Err("four-cell compatibility matrix drift".to_string());
    }
    let expected_cases = BTreeSet::from([
        "default_only".to_string(),
        "select_perllsp".to_string(),
        "select_perl_lsp".to_string(),
        "deliberate_multi_server".to_string(),
        "missing_selected_server".to_string(),
        "ellipsis_preserves_user_registration".to_string(),
    ]);
    let actual_cases = contract
        .get("selection_cases")
        .and_then(Value::as_array)
        .ok_or_else(|| "missing selection cases".to_string())?
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    if actual_cases != expected_cases {
        return Err("selection case denominator drift".to_string());
    }
    for (pointer, expected) in [
        ("/claim_boundary/host_compatibility", "not_run"),
        ("/claim_boundary/publication_order", "unresolved"),
        ("/claim_boundary/full_zed_support", "not_proven"),
        ("/claim_boundary/public_registry", "not_proven"),
    ] {
        if text(contract, pointer) != Some(expected) {
            return Err(format!("static contract overclaims `{pointer}`"));
        }
    }
    Ok(())
}

pub fn validate_receipt(receipt: &Value, contract: &Value) -> Result<(), String> {
    if receipt.get("schema_version").and_then(Value::as_str)
        != Some("zed_default_order_receipt.v1")
    {
        return Err("wrong default-order receipt schema".to_string());
    }
    if text(receipt, "/claim_boundary/full_zed_support") != Some("not_proven")
        || text(receipt, "/claim_boundary/public_registry") != Some("not_proven")
    {
        return Err("default-order receipt overclaims support".to_string());
    }
    let result = receipt.get("result").and_then(Value::as_str).unwrap_or_default();
    if result == "not_run" {
        if text(receipt, "/claim_boundary/host_compatibility") != Some("not_run")
            || text(receipt, "/claim_boundary/publication_order") != Some("unresolved")
        {
            return Err("not-run receipt contains a ruling".to_string());
        }
        return Ok(());
    }
    if result != "pass" || text(receipt, "/observed_at").is_none() || !digest(receipt, "/contract/sha256") {
        return Err("passing default-order receipt lacks exact identity".to_string());
    }

    let expected_matrix = index(contract, "/matrix")?;
    let observed_matrix = index(receipt, "/matrix")?;
    if expected_matrix.keys().collect::<Vec<_>>() != observed_matrix.keys().collect::<Vec<_>>() {
        return Err("matrix row population drift".to_string());
    }
    let mut zed_identity: Option<&str> = None;
    for (id, row) in &observed_matrix {
        if !matches!(row.get("result").and_then(Value::as_str), Some("pass" | "limited"))
            || !digest(row, "/zed_identity_sha256")
            || !digest(row, "/defaults_sha256")
            || !digest(row, "/extension_sha256")
            || !digest(row, "/profile_sha256")
            || !digest(row, "/process_inventory_sha256")
            || row.get("clean_profile_quiet").and_then(Value::as_bool).is_none()
            || text(row, "/evidence").is_none()
        {
            return Err(format!("matrix row `{id}` lacks direct exact-host evidence"));
        }
        let current = text(row, "/zed_identity_sha256").expect("checked above");
        if zed_identity.is_some_and(|expected| expected != current) {
            return Err("matrix rows mix different Zed host subjects".to_string());
        }
        zed_identity = Some(current);
    }

    let final_row = observed_matrix
        .get("candidate_defaults_candidate_extension")
        .copied()
        .ok_or_else(|| "missing final candidate combination".to_string())?;
    if final_row.get("result").and_then(Value::as_str) != Some("pass")
        || final_row.get("clean_profile_quiet").and_then(Value::as_bool) != Some(true)
    {
        return Err("final defaults/extension combination is not quiet".to_string());
    }
    exact_set(final_row, "started_server_ids", &["perlnavigator-server"])?;
    exact_set(final_row, "failed_server_ids", &[])?;

    let cases = index(receipt, "/selection_cases")?;
    let expected_cases = contract
        .get("selection_cases")
        .and_then(Value::as_array)
        .ok_or_else(|| "contract lacks selection cases".to_string())?
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    if cases.keys().cloned().collect::<BTreeSet<_>>() != expected_cases {
        return Err("selection case population drift".to_string());
    }
    for (id, row) in &cases {
        if row.get("result").and_then(Value::as_str) != Some("pass")
            || text(row, "/evidence").is_none()
        {
            return Err(format!("selection case `{id}` lacks evidence"));
        }
    }
    exact_set(cases["default_only"], "started_server_ids", &["perlnavigator-server"])?;
    exact_set(cases["default_only"], "failed_server_ids", &[])?;
    exact_set(cases["select_perllsp"], "started_server_ids", &["perllsp"])?;
    exact_set(cases["select_perllsp"], "failed_server_ids", &[])?;
    if text(cases["select_perllsp"], "/command") != Some("perllsp --stdio") {
        return Err("perllsp selection did not launch exact perllsp --stdio".to_string());
    }
    exact_set(cases["select_perl_lsp"], "started_server_ids", &["perl-lsp"])?;
    if text(cases["select_perl_lsp"], "/command").is_none()
        || text(cases["select_perl_lsp"], "/command").is_some_and(|command| command.contains("perllsp"))
    {
        return Err("perl-lsp selection collapsed into perllsp".to_string());
    }
    exact_set(
        cases["deliberate_multi_server"],
        "started_server_ids",
        &["perl-lsp", "perllsp"],
    )?;
    exact_set(cases["missing_selected_server"], "started_server_ids", &[])?;
    exact_set(cases["missing_selected_server"], "failed_server_ids", &["perllsp"])?;
    let preserved = strings(cases["ellipsis_preserves_user_registration"], "started_server_ids")?;
    if !preserved.iter().any(|id| {
        !matches!(id.as_str(), "perlnavigator-server" | "perl-lsp" | "perllsp")
    }) {
        return Err("ellipsis did not preserve an independent user registration".to_string());
    }

    let b_quiet = observed_matrix["candidate_defaults_public_extension"]
        .get("clean_profile_quiet")
        .and_then(Value::as_bool)
        .expect("checked above");
    let c_quiet = observed_matrix["current_defaults_candidate_extension"]
        .get("clean_profile_quiet")
        .and_then(Value::as_bool)
        .expect("checked above");
    let expected_ruling = if b_quiet {
        "zed_defaults_first_safe"
    } else if c_quiet {
        "extension_first_required"
    } else {
        "coordinated_release_required"
    };
    let ruling = receipt.get("ruling").unwrap_or(&Value::Null);
    if ruling.get("result").and_then(Value::as_str) != Some("pass")
        || text(ruling, "/value") != Some(expected_ruling)
        || text(ruling, "/unsafe_interval_avoided").is_none()
        || strings(ruling, "maintainer_sequence")?.is_empty()
        || strings(ruling, "invalidation")?.is_empty()
        || text(ruling, "/evidence").is_none()
    {
        return Err("publication order is not derived from the matrix".to_string());
    }
    if text(receipt, "/claim_boundary/host_compatibility") != Some("proven_for_exact_matrix")
        || text(receipt, "/claim_boundary/publication_order") != Some(expected_ruling)
    {
        return Err("claim boundary disagrees with the derived ruling".to_string());
    }
    Ok(())
}
