//! CA02B projection/catalog join. Structural source checking is the scoped xtask
//! binary; these tests reuse the real canonical and CA02A semantic validators.
#[path = "high_risk_model.rs"]
mod model;
use super::{
    CONFIGURATION_AUTHORITY, authority_by_id,
    capability::{CapabilityDeclaration, validate_capabilities},
};
use model::Projection;
type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn projection() -> TestResult<Projection> {
    Ok(serde_json::from_str(include_str!(
        "../../../../fixtures/configuration_authority/high_risk_bindings.v1.json"
    ))?)
}

fn validate(projection: &Projection) -> TestResult {
    model::validate(projection)?;
    let mut capabilities = Vec::new();
    let mut covered = std::collections::BTreeSet::new();
    for row in &projection.rows {
        if row.kind != "active" {
            if authority_by_id(&row.id).is_some() {
                return Err(
                    format!("{}: non-active row shadows canonical authority", row.id).into()
                );
            }
            continue;
        }
        let field = authority_by_id(&row.id)
            .ok_or_else(|| format!("{}: unknown canonical field", row.id))?;
        if row.rust_field != format!("{:?}.{}", field.owner, field.rust_field)
            || row.sources
                != field.sources.iter().map(|source| format!("{source:?}")).collect::<Vec<_>>()
            || row.canonical_consumers
                != field
                    .consumers
                    .iter()
                    .map(|consumer| format!("{consumer:?}"))
                    .collect::<Vec<_>>()
            || row.invalidation != format!("{:?}", field.invalidation)
        {
            return Err(format!(
                "{}: canonical storage/source/consumer/invalidation drift",
                row.id
            )
            .into());
        }
        let declaration: CapabilityDeclaration =
            serde_json::from_value(row.capability.clone().ok_or("missing capability")?)?;
        if declaration.field_id != row.id {
            return Err("capability attached to another field".into());
        }
        capabilities.push(declaration);
        covered.insert(row.id.as_str());
    }
    validate_capabilities(&capabilities)
        .map_err(|error| format!("capability violation: {error:?}"))?;
    for id in &projection.remaining_low_risk {
        if authority_by_id(id).is_none() || !covered.insert(id) {
            return Err(format!("unknown/duplicate remaining ID {id}").into());
        }
    }
    let canonical = CONFIGURATION_AUTHORITY
        .iter()
        .map(|field| field.id)
        .collect::<std::collections::BTreeSet<_>>();
    if covered != canonical {
        return Err(
            "high-risk bindings plus explicit low-risk remainder do not partition catalog".into()
        );
    }
    Ok(())
}

#[test]
fn high_risk_projection_joins_canonical_authority_and_capability_contract() -> TestResult {
    validate(&projection()?)
}

#[test]
fn retired_limits_remain_absent_from_the_active_catalog() -> TestResult {
    let projection = projection()?;
    validate(&projection)?;
    for (id, member, key) in [
        ("limits.ast_cache_max_entries", "ast_cache_max_entries", "astCacheMaxEntries"),
        ("limits.ast_cache_ttl_secs", "ast_cache_ttl_secs", "astCacheTtlSecs"),
        ("limits.symbol_cache_max_entries", "symbol_cache_max_entries", "symbolCacheMaxEntries"),
        ("limits.indexed_files", "max_indexed_files", "maxIndexedFiles"),
        ("limits.total_symbols", "max_total_symbols", "maxTotalSymbols"),
        ("limits.workspace_scan_deadline_ms", "workspace_scan_deadline", "workspaceScanDeadlineMs"),
    ] {
        let row =
            projection.rows.iter().find(|row| row.id == id).ok_or("missing retired limit row")?;
        let witness = projection
            .witnesses
            .get(&format!("limit.removed.{member}"))
            .ok_or("missing retired parser witness")?;
        if authority_by_id(id).is_some()
            || row.kind != "removed"
            || row.disposition != "remove_false_contract"
            || row.owner != "#16176"
            || row.capability.is_some()
            || row.projections.len() != 1
            || row.projections.iter().any(|field| {
                field.present
                    || field.pointer
                        != format!("/properties/perl/properties/limits/properties/{key}")
            })
            || witness.path != "crates/perl-lsp-rs-core/src/runtime/limits/mod.rs"
            || witness.function != "@no-key:LspLimits::update_from_value"
            || witness.expression != key
        {
            return Err(
                format!("{id}: retirement identity or source absence evidence drifted").into()
            );
        }
    }
    Ok(())
}

#[test]
fn catalog_join_rejects_exact_identity_and_partition_errors() -> TestResult {
    validate(&projection()?)?;
    for case in
        ["unknown", "sources", "consumers", "invalidation", "capability", "remainder", "partition"]
    {
        let mut changed = projection()?;
        let id = "limits.completion_cap";
        let expected = match case {
            "remainder" => {
                changed.remaining_low_risk.push("unknown.low_risk".into());
                "unknown/duplicate remaining ID unknown.low_risk".into()
            }
            "partition" => {
                changed.remaining_low_risk.pop().ok_or("missing remainder control")?;
                "high-risk bindings plus explicit low-risk remainder do not partition catalog"
                    .into()
            }
            _ => {
                let row = changed
                    .rows
                    .iter_mut()
                    .find(|row| row.id == id)
                    .ok_or("missing active control")?;
                match case {
                    "unknown" => {
                        row.id = "limits.unregistered".into();
                        *row.capability
                            .as_mut()
                            .and_then(|value| value.get_mut("proof"))
                            .and_then(|value| value.get_mut("negative_control_id"))
                            .ok_or("missing first-effect requirement identity")? =
                            serde_json::json!("pending:#7479:limits.unregistered");
                        "limits.unregistered: unknown canonical field".into()
                    }
                    "capability" => {
                        *row.capability
                            .as_mut()
                            .and_then(|value| value.get_mut("field_id"))
                            .ok_or("missing capability identity")? =
                            serde_json::json!("limits.other");
                        "capability attached to another field".into()
                    }
                    _ => {
                        match case {
                            "sources" => row.sources.reverse(),
                            "consumers" => {
                                row.canonical_consumers = vec!["BoundedExecution".into()]
                            }
                            "invalidation" => row.invalidation = "None".into(),
                            _ => return Err("unknown catalog join control".into()),
                        }
                        format!("{id}: canonical storage/source/consumer/invalidation drift")
                    }
                }
            }
        };
        let actual = validate(&changed);
        if !matches!(&actual, Err(error) if error.to_string() == expected) {
            return Err(format!("{case}: expected {expected}, got {actual:?}").into());
        }
    }
    Ok(())
}

#[test]
fn retired_row_cannot_shadow_an_active_catalog_identity() -> TestResult {
    let mut changed = projection()?;
    changed.rows.retain(|row| row.id != "limits.completion_cap");
    let row = changed
        .rows
        .iter_mut()
        .find(|row| row.kind == "removed")
        .ok_or("missing retirement control")?;
    row.id = "limits.completion_cap".into();
    if !matches!(validate(&changed), Err(error) if error.to_string() == "limits.completion_cap: non-active row shadows canonical authority")
    {
        return Err("retired row did not produce the exact canonical-shadow error".into());
    }
    Ok(())
}

#[test]
fn canonical_storage_identity_rejects_the_exact_wrong_field() -> TestResult {
    let mut changed = projection()?;
    let row = changed
        .rows
        .iter_mut()
        .find(|row| row.id == "limits.completion_cap")
        .ok_or("missing active control")?;
    row.rust_field = "Limits.other".into();
    if !matches!(validate(&changed), Err(error) if error.to_string() == "limits.completion_cap: canonical storage/source/consumer/invalidation drift")
    {
        return Err("wrong storage did not produce the exact canonical-binding error".into());
    }
    Ok(())
}

#[test]
fn high_risk_authority_mutations_are_rejected() -> TestResult {
    for (id, source) in [
        ("ai.endpoint", "ProjectFile"),
        ("ai.api_key_env", "ProjectFile"),
        ("workspace.external_include_paths", "GlobalClientSettings"),
    ] {
        let mut changed = projection()?;
        let row = changed.rows.iter_mut().find(|row| row.id == id).ok_or("missing control row")?;
        row.sources.push(source.into());
        if validate(&changed).is_ok() {
            return Err(format!("accepted authority expansion for {id}").into());
        }
    }
    Ok(())
}

#[test]
fn high_risk_missing_envelope_is_rejected() -> TestResult {
    let mut changed = projection()?;
    let row = changed
        .rows
        .iter_mut()
        .find(|row| row.id == "limits.completion_cap")
        .ok_or("missing limit")?;
    *row.capability
        .as_mut()
        .and_then(|value| value.get_mut("composition"))
        .ok_or("missing composition")? = serde_json::json!([]);
    if validate(&changed).is_ok() {
        return Err("accepted missing hard-envelope owner".into());
    }
    Ok(())
}

#[test]
fn high_risk_disposition_and_proof_mutations_are_rejected() -> TestResult {
    for case in 0..5 {
        let mut changed = projection()?;
        let id = match case {
            0 => "testRunner.command",
            1 => "formatting.engine",
            2 => "workspace.perl_path",
            3 => "project.perl.version",
            _ => "ai.endpoint",
        };
        let row = changed
            .rows
            .iter_mut()
            .find(|row| row.id == id)
            .ok_or("missing rejection-control row")?;
        match case {
            0 => row.kind = "active".into(),
            1 => row.first_effect = None,
            2 => row.disposition = "behavior_backed".into(),
            3 => row.owner.clear(),
            _ => {
                row.currentness.status = "structurally_bound".into();
                row.currentness.witness = row.consumers.first().cloned();
            }
        }
        if validate(&changed).is_ok() {
            return Err(format!("accepted missing disposition/proof in case {case}").into());
        }
    }
    Ok(())
}

#[test]
fn high_risk_field_cannot_hide_in_the_low_risk_remainder() -> TestResult {
    let mut changed = projection()?;
    changed.rows.retain(|row| row.id != "ai.endpoint");
    changed.remaining_low_risk.push("ai.endpoint".into());
    if validate(&changed).is_ok() {
        return Err("lost high-risk row passed through low-risk enumeration".into());
    }
    Ok(())
}

#[test]
fn first_effect_requirement_cannot_disagree_with_its_unresolved_owner() -> TestResult {
    for change_status in [false, true] {
        let mut changed = projection()?;
        let row = changed
            .rows
            .iter_mut()
            .find(|row| row.id == "formatting.engine")
            .ok_or("missing formatter")?;
        let obligation = row.first_effect.as_mut().ok_or("missing first-effect obligation")?;
        if change_status {
            obligation.status = "structurally_bound".into();
            obligation.witness = row.consumers.first().cloned();
        } else {
            obligation.owner = "#10801".into();
        }
        if validate(&changed).is_ok() {
            return Err("contradictory first-effect ownership accepted".into());
        }
    }
    Ok(())
}
