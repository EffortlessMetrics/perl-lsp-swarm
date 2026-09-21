//! Shared private evidence shape. This is a projection of the canonical catalog,
//! not an effective settings store or a new authority registry.
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Projection {
    pub schema_version: String,
    pub witnesses: BTreeMap<String, Witness>,
    pub rows: Vec<Row>,
    pub remaining_low_risk: Vec<String>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Witness {
    pub path: String,
    /// Inherent `Type::method`, trait `Type::Trait::method`, or free function.
    pub function: String,
    pub expression: String,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Row {
    pub id: String,
    pub kind: String,
    pub disposition: String,
    pub owner: String,
    pub rust_field: String,
    pub sources: Vec<String>,
    pub canonical_consumers: Vec<String>,
    pub invalidation: String,
    pub writers: Vec<String>,
    pub consumers: Vec<String>,
    pub projections: Vec<ProjectionField>,
    pub docs: String,
    pub currentness: Obligation,
    pub first_effect: Option<Obligation>,
    pub capability: Option<serde_json::Value>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Obligation {
    pub owner: String,
    pub status: String,
    pub witness: Option<String>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProjectionField {
    pub path: String,
    pub pointer: String,
    pub present: bool,
}
type CheckResult = Result<(), Box<dyn std::error::Error>>;
fn issue(value: &str) -> bool {
    value
        .strip_prefix('#')
        .is_some_and(|number| !number.is_empty() && number.chars().all(|c| c.is_ascii_digit()))
}
fn obligation(value: &Obligation, projection: &Projection) -> CheckResult {
    if !issue(&value.owner) {
        return Err("obligation requires an exact issue owner".into());
    }
    match value.status.as_str() {
        "unresolved" if value.witness.is_none() => Ok(()),
        "structurally_bound"
            if value.witness.as_ref().is_some_and(|id| projection.witnesses.contains_key(id)) =>
        {
            Ok(())
        }
        _ => Err("unproved or missing result/configuration-generation binding".into()),
    }
}
pub(crate) fn high_risk(id: &str) -> bool {
    id.starts_with("ai.")
        || id.starts_with("critic.")
        || id.starts_with("limits.")
        || matches!(
            id,
            "formatting.engine"
                | "formatting.profile"
                | "formatting.extra_args"
                | "formatting.enabled"
                | "formatting.format_on_save"
                | "formatting.timeout_secs"
                | "workspace.external_include_paths"
                | "workspace.include_paths"
                | "workspace.perl5lib_precedence"
                | "workspace.use_perl5lib"
                | "workspace.use_system_inc"
                | "workspace.perl_path"
                | "workspace.perl_args"
                | "workspace.resolution_timeout_ms"
        )
}
pub(crate) fn validate(projection: &Projection) -> CheckResult {
    if projection.schema_version != "high_risk_configuration_bindings.v1" {
        return Err("unknown projection schema".into());
    }
    let mut ids = BTreeSet::new();
    for row in &projection.rows {
        if !ids.insert(&row.id) {
            return Err(format!("duplicate row {}", row.id).into());
        }
        if !issue(&row.owner) || row.docs.trim().is_empty() {
            return Err(format!("{}: missing disposition/docs owner", row.id).into());
        }
        if !matches!(
            row.disposition.as_str(),
            "behavior_backed"
                | "trusted_only_adapter"
                | "resource_may_reduce_only"
                | "remove_false_contract"
                | "parsed_pending_with_owner"
                | "bounded_compatibility_with_exit"
                | "unsupported_rejected"
                | "presentation_only"
        ) {
            return Err(format!("{}: unknown disposition", row.id).into());
        }
        match row.kind.as_str() {
            "active"
                if row.capability.is_some()
                    && !row.rust_field.is_empty()
                    && !row.sources.is_empty()
                    && !row.canonical_consumers.is_empty()
                    && !row.invalidation.is_empty()
                    && !row.writers.is_empty() => {}
            "removed"
                if row.disposition == "remove_false_contract"
                    && row.capability.is_none()
                    && row.writers.is_empty()
                    && row.consumers.is_empty()
                    && !row.projections.is_empty()
                    && row.projections.iter().all(|field| !field.present) => {}
            "derived"
                if row.id == "project.perl.version"
                    && row.owner == "#10132"
                    && row.capability.is_none()
                    && !row.consumers.is_empty() => {}
            _ => return Err(format!("{}: invalid active/removed/derived binding", row.id).into()),
        }
        if row.id.starts_with("testRunner.") && row.kind != "removed" {
            return Err("removed runner field cannot be active".into());
        }
        if matches!(row.id.as_str(), "workspace.perl_path" | "workspace.perl_args")
            && row.disposition != "unsupported_rejected"
        {
            return Err("rejected workspace Perl contract cannot be documented active".into());
        }
        if row.kind == "active"
            && row.consumers.is_empty()
            && !matches!(
                row.disposition.as_str(),
                "parsed_pending_with_owner" | "trusted_only_adapter" | "unsupported_rejected"
            )
        {
            return Err(format!("{}: disconnected production consumer", row.id).into());
        }
        for id in row.writers.iter().chain(&row.consumers) {
            if !projection.witnesses.contains_key(id) {
                return Err(format!("{}: unknown structural witness {id}", row.id).into());
            }
        }
        obligation(&row.currentness, projection)?;
        // CA02B owns exact source bindings and unresolved owners, not accepted
        // generation assembly. A consumer read alone cannot prove its result
        // includes the configuration generation. Promotion needs a later
        // contract and its own structural result-identity proof.
        if row.currentness.status != "unresolved" || row.currentness.witness.is_some() {
            return Err(format!(
                "{}: configuration-generation result identity is not proven by CA02B",
                row.id
            )
            .into());
        }
        if let Some(proof) = &row.first_effect {
            obligation(proof, projection)?;
        }
        if let Some(requirement) = row.capability.as_ref().and_then(|value| value.get("proof"))
            && requirement.get("kind").and_then(serde_json::Value::as_str) == Some("first_effect")
        {
            let obligation = row.first_effect.as_ref().ok_or("missing first-effect obligation")?;
            let owner = requirement
                .get("owner")
                .and_then(serde_json::Value::as_str)
                .ok_or("missing first-effect owner")?;
            let expected = format!("pending:{owner}:{}", row.id);
            if obligation.owner != owner
                || obligation.status != "unresolved"
                || obligation.witness.is_some()
                || requirement.get("negative_control_id").and_then(serde_json::Value::as_str)
                    != Some(expected.as_str())
            {
                return Err(
                    format!("{}: first-effect requirement/obligation mismatch", row.id).into()
                );
            }
        }
        if row.id == "formatting.engine" && row.first_effect.is_none() {
            return Err("formatter engine lacks first-spawn proof owner".into());
        }
    }
    let remaining: BTreeSet<_> = projection.remaining_low_risk.iter().collect();
    if remaining.len() != projection.remaining_low_risk.len()
        || remaining.iter().any(|id| ids.contains(id))
        || remaining.iter().any(|id| high_risk(id))
    {
        return Err("remaining low-risk enumeration overlaps or duplicates bindings".into());
    }
    Ok(())
}
