//! Dependency-neutral product-health rail contracts.
//!
//! This module deliberately contains contracts only.  It does not read status
//! files or infer truth from issue, PR, workflow, or Markdown state.

use color_eyre::eyre::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const SCHEMA: &str = "product_health_rail.v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RailResult {
    Pass,
    PassWithDeclaredLimitations,
    Failed,
    NotProven,
    Stale,
    Invalid,
    NoCurrentSource,
    ConflictingCurrentSources,
    Unsupported,
    NotApplicable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Applicability {
    Required,
    Conditional,
    Optional,
    NotApplicable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Rail {
    pub schema: String,
    pub rail_id: String,
    pub area: String,
    pub proposition: String,
    pub source_schema: String,
    pub source_digest: String,
    pub subject: String,
    pub currentness: String,
    pub result: RailResult,
    pub applicability: Applicability,
    pub limitations: Vec<String>,
    pub nonclaims: Vec<String>,
    pub claim_ceiling: String,
    pub source_detail: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Adapter {
    pub schema: String,
    pub adapter_id: String,
    pub source_family: String,
    pub accepted_source_schemas: Vec<String>,
    pub validator_id: String,
    pub subject_selector: String,
    pub currentness_authority: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Registry {
    pub schema: String,
    pub adapters: Vec<Adapter>,
    pub rails: Vec<Rail>,
}

pub fn validate_registry(registry: &Registry) -> Result<()> {
    ensure!(registry.schema == "product_health_rail_registry.v1", "unsupported registry schema");
    ensure!(!registry.rails.is_empty(), "registry must declare at least one rail");
    ensure!(!registry.adapters.is_empty(), "registry must declare at least one adapter");

    let mut rail_ids = std::collections::BTreeSet::new();
    for rail in &registry.rails {
        ensure!(rail.schema == SCHEMA, "rail {} has unsupported schema", rail.rail_id);
        ensure!(!rail.rail_id.is_empty(), "rail id must not be empty");
        ensure!(rail_ids.insert(&rail.rail_id), "duplicate rail id {}", rail.rail_id);
        ensure!(!rail.source_schema.is_empty(), "rail {} has no source schema", rail.rail_id);
        ensure!(!rail.source_digest.is_empty(), "rail {} has no source digest", rail.rail_id);
        ensure!(!rail.subject.is_empty(), "rail {} has no exact subject", rail.rail_id);
        ensure!(!rail.currentness.is_empty(), "rail {} has no currentness relation", rail.rail_id);
        ensure!(!rail.claim_ceiling.is_empty(), "rail {} has no claim ceiling", rail.rail_id);
        ensure!(
            rail.result != RailResult::Pass || rail.limitations.is_empty(),
            "pass rail {} must use pass_with_declared_limitations for limitations",
            rail.rail_id
        );
        ensure!(
            rail.result != RailResult::PassWithDeclaredLimitations || !rail.limitations.is_empty(),
            "limited pass rail {} must declare at least one limitation",
            rail.rail_id
        );
    }
    let mut adapter_ids = std::collections::BTreeSet::new();
    for adapter in &registry.adapters {
        ensure!(adapter.schema == "product_health_rail_adapter.v1", "unsupported adapter schema");
        ensure!(!adapter.adapter_id.is_empty(), "adapter id must not be empty");
        ensure!(
            adapter_ids.insert(&adapter.adapter_id),
            "duplicate adapter id {}",
            adapter.adapter_id
        );
        ensure!(
            !adapter.accepted_source_schemas.is_empty(),
            "adapter {} accepts no schemas",
            adapter.adapter_id
        );
        ensure!(
            !adapter.validator_id.is_empty(),
            "adapter {} has no validator",
            adapter.adapter_id
        );
    }
    for rail in &registry.rails {
        ensure!(
            registry.adapters.iter().any(|adapter| {
                adapter.accepted_source_schemas.iter().any(|schema| schema == &rail.source_schema)
            }),
            "rail {} has no adapter for source schema {}",
            rail.rail_id,
            rail.source_schema
        );
    }
    Ok(())
}

pub fn canonical_json(registry: &Registry) -> Result<String> {
    validate_registry(registry)?;
    let mut normalized = registry.clone();
    normalized.rails.sort_by(|a, b| a.rail_id.cmp(&b.rail_id));
    normalized.adapters.sort_by(|a, b| a.adapter_id.cmp(&b.adapter_id));
    for adapter in &mut normalized.adapters {
        adapter.accepted_source_schemas.sort();
        adapter.accepted_source_schemas.dedup();
    }
    Ok(serde_json::to_string(&normalized)?)
}

pub fn run() -> Result<()> {
    let registry = fixture_registry();
    validate_registry(&registry)?;
    let first = canonical_json(&registry)?;
    let second = canonical_json(&Registry {
        rails: registry.rails.iter().rev().cloned().collect(),
        adapters: registry.adapters.iter().rev().cloned().collect(),
        ..registry.clone()
    })?;
    ensure!(first == second, "canonical rail serialization is order-dependent");
    println!("validated {SCHEMA} and product_health_rail_registry.v1");
    Ok(())
}

fn fixture_registry() -> Registry {
    Registry {
        schema: "product_health_rail_registry.v1".into(),
        adapters: vec![Adapter {
            schema: "product_health_rail_adapter.v1".into(),
            adapter_id: "fixture.adapter".into(),
            source_family: "fixture".into(),
            accepted_source_schemas: vec!["fixture.v1".into()],
            validator_id: "fixture.validator.v1".into(),
            subject_selector: "fixture.subject".into(),
            currentness_authority: "fixture.currentness".into(),
        }],
        rails: vec![Rail {
            schema: SCHEMA.into(),
            rail_id: "fixture.parser".into(),
            area: "parser".into(),
            proposition: "fixture parser contract holds".into(),
            source_schema: "fixture.v1".into(),
            source_digest: "sha256:fixture".into(),
            subject: "fixture-subject".into(),
            currentness: "exact:fixture".into(),
            result: RailResult::Pass,
            applicability: Applicability::Required,
            limitations: vec![],
            nonclaims: vec!["does not establish release authority".into()],
            claim_ceiling: "fixture parser proposition only".into(),
            source_detail: BTreeMap::new(),
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_bytes_ignore_registration_order() {
        let registry = fixture_registry();
        let reversed = Registry {
            rails: registry.rails.iter().rev().cloned().collect(),
            adapters: registry.adapters.iter().rev().cloned().collect(),
            ..registry.clone()
        };
        assert_eq!(canonical_json(&registry).unwrap(), canonical_json(&reversed).unwrap());
    }

    #[test]
    fn pass_with_limitations_is_not_collapsed_to_pass() {
        let mut registry = fixture_registry();
        registry.rails[0].limitations.push("bounded fixture".into());
        assert!(validate_registry(&registry).is_err());
        registry.rails[0].result = RailResult::PassWithDeclaredLimitations;
        validate_registry(&registry).unwrap();
    }

    #[test]
    fn accepted_source_schema_order_does_not_change_canonical_bytes() {
        let mut registry = fixture_registry();
        registry.rails[0].source_schema = "a.v1".into();
        registry.adapters[0].accepted_source_schemas =
            vec!["b.v1".into(), "a.v1".into(), "a.v1".into()];
        let mut equivalent = registry.clone();
        equivalent.adapters[0].accepted_source_schemas = vec!["a.v1".into(), "b.v1".into()];
        assert_eq!(canonical_json(&registry).unwrap(), canonical_json(&equivalent).unwrap());
    }

    #[test]
    fn limited_pass_requires_a_limitation() {
        let mut registry = fixture_registry();
        registry.rails[0].result = RailResult::PassWithDeclaredLimitations;
        assert!(validate_registry(&registry).is_err());
    }

    #[test]
    fn duplicate_identity_fails_closed() {
        let mut registry = fixture_registry();
        registry.rails.push(registry.rails[0].clone());
        assert!(validate_registry(&registry).is_err());
    }
}
