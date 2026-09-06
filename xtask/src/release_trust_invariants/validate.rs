//! Structural validation for `release_trust_invariants.v1`.

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use super::error::RegistryError;
use super::model::{
    AuthorityStatus, ISSUE, PARENT_ISSUE, REGISTRY_NAME, REGISTRY_PATH, SCHEMA_PATH,
    SCHEMA_VERSION, TrustInvariantRegistry,
};

/// Load, schema-validate, and structurally check the committed registry.
pub fn load_and_validate(root: &Path) -> Result<TrustInvariantRegistry, RegistryError> {
    let bytes = fs::read(root.join(REGISTRY_PATH))
        .map_err(|error| RegistryError::new(format!("{REGISTRY_PATH}: cannot read: {error}")))?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|error| RegistryError::new(format!("{REGISTRY_PATH}: invalid JSON: {error}")))?;
    decode_and_validate(root, &value)
}

/// Validate an in-memory JSON document against schema plus structural rules.
pub fn validate_registry_value(
    root: &Path,
    value: &Value,
) -> Result<TrustInvariantRegistry, RegistryError> {
    decode_and_validate(root, value)
}

fn decode_and_validate(
    root: &Path,
    value: &Value,
) -> Result<TrustInvariantRegistry, RegistryError> {
    let schema_text = fs::read_to_string(root.join(SCHEMA_PATH))
        .map_err(|error| RegistryError::new(format!("{SCHEMA_PATH}: cannot read: {error}")))?;
    let schema: Value = serde_json::from_str(&schema_text)
        .map_err(|error| RegistryError::new(format!("{SCHEMA_PATH}: invalid JSON: {error}")))?;
    validate_schema(&schema, value)?;
    let registry: TrustInvariantRegistry =
        serde_json::from_value(value.clone()).map_err(|error| {
            RegistryError::new(format!("{REGISTRY_PATH}: typed decode failed: {error}"))
        })?;
    validate_registry(&registry)?;
    Ok(registry)
}

fn validate_schema(schema: &Value, value: &Value) -> Result<(), RegistryError> {
    let validator = jsonschema::validator_for(schema)
        .map_err(|error| RegistryError::new(format!("{SCHEMA_PATH}: invalid schema: {error}")))?;
    let violations: Vec<String> =
        validator.iter_errors(value).map(|error| format!("schema: {error}")).collect();
    RegistryError::from_violations(violations)
}

fn validate_registry(registry: &TrustInvariantRegistry) -> Result<(), RegistryError> {
    let mut violations = Vec::new();
    if registry.schema_version != SCHEMA_VERSION {
        violations.push(format!(
            "schema_version: expected `{SCHEMA_VERSION}`, found `{}`",
            registry.schema_version
        ));
    }
    if registry.registry != REGISTRY_NAME {
        violations
            .push(format!("registry: expected `{REGISTRY_NAME}`, found `{}`", registry.registry));
    }
    if registry.issue != ISSUE {
        violations.push(format!("issue: expected {ISSUE}, found {}", registry.issue));
    }
    if registry.parent_issue != PARENT_ISSUE {
        violations.push(format!(
            "parent_issue: expected {PARENT_ISSUE}, found {}",
            registry.parent_issue
        ));
    }
    if registry.claim_boundary.trim().is_empty() {
        violations.push("claim_boundary: must be non-empty".to_string());
    }

    let owners = validate_owner_authorities(&registry.owner_authorities, &mut violations);
    let producers =
        validate_producer_authorities(&registry.producer_authorities, &owners, &mut violations);
    let controls =
        validate_negative_controls(&registry.negative_control_catalog, &owners, &mut violations);
    validate_invariants(registry, &owners, &producers, &controls, &mut violations);
    validate_controller_requirements(registry, &owners, &mut violations);
    RegistryError::from_violations(violations)
}

fn validate_owner_authorities<'a>(
    authorities: &'a [super::model::OwnerAuthority],
    violations: &mut Vec<String>,
) -> BTreeMap<u32, &'a super::model::OwnerAuthority> {
    let mut by_issue = BTreeMap::new();
    for authority in authorities {
        if authority.issue == 0 {
            violations.push("owner_authorities: owner issue is missing".to_string());
            continue;
        }
        if authority.title.trim().is_empty() {
            violations.push(format!("owner_authorities.#{}: title is empty", authority.issue));
        }
        if by_issue.insert(authority.issue, authority).is_some() {
            violations
                .push(format!("owner_authorities: duplicate owner issue {}", authority.issue));
        }
        match authority.status {
            AuthorityStatus::Current => {
                if authority.successor.is_some() {
                    violations.push(format!(
                        "owner_authorities.#{}: current owner cannot declare a successor",
                        authority.issue
                    ));
                }
            }
            AuthorityStatus::Superseded => {
                if authority.successor.is_none() {
                    violations.push(format!(
                        "owner_authorities.#{}: superseded owner has no successor",
                        authority.issue
                    ));
                }
            }
        }
    }
    for authority in authorities {
        if let Some(successor) = authority.successor {
            match by_issue.get(&successor) {
                Some(target) if matches!(target.status, AuthorityStatus::Current) => {}
                Some(_) => violations.push(format!(
                    "owner_authorities.#{}: successor {successor} is not current",
                    authority.issue
                )),
                None => violations.push(format!(
                    "owner_authorities.#{}: successor {successor} does not exist",
                    authority.issue
                )),
            }
        }
    }
    by_issue
}

fn validate_producer_authorities<'a>(
    authorities: &'a [super::model::ProducerAuthority],
    owners: &BTreeMap<u32, &super::model::OwnerAuthority>,
    violations: &mut Vec<String>,
) -> BTreeMap<super::model::ProducerKind, &'a super::model::ProducerAuthority> {
    let mut by_kind = BTreeMap::new();
    for authority in authorities {
        if by_kind.insert(authority.producer_kind, authority).is_some() {
            violations.push(format!(
                "producer_authorities: duplicate producer_kind `{}`",
                authority.producer_kind.as_str()
            ));
        }
        if authority.command_or_workflow.trim().is_empty() {
            violations.push(format!(
                "producer_authorities.`{}`: command_or_workflow is empty",
                authority.producer_kind.as_str()
            ));
        }
        match owners.get(&authority.owner_issue) {
            Some(owner) if matches!(owner.status, AuthorityStatus::Current) => {}
            Some(_) => violations.push(format!(
                "producer_authorities.`{}`: owner issue {} is superseded",
                authority.producer_kind.as_str(),
                authority.owner_issue
            )),
            None => violations.push(format!(
                "producer_authorities.`{}`: owner issue {} is missing",
                authority.producer_kind.as_str(),
                authority.owner_issue
            )),
        }
        match authority.status {
            AuthorityStatus::Current => {
                if authority.successor.is_some() {
                    violations.push(format!(
                        "producer_authorities.`{}`: current producer cannot declare a successor",
                        authority.producer_kind.as_str()
                    ));
                }
            }
            AuthorityStatus::Superseded => {
                if authority.successor.is_none() {
                    violations.push(format!(
                        "producer_authorities.`{}`: superseded producer has no successor",
                        authority.producer_kind.as_str()
                    ));
                }
            }
        }
    }
    for authority in authorities {
        if let Some(successor) = authority.successor {
            match by_kind.get(&successor) {
                Some(target) if matches!(target.status, AuthorityStatus::Current) => {}
                Some(_) => violations.push(format!(
                    "producer_authorities.`{}`: successor `{}` is not current",
                    authority.producer_kind.as_str(),
                    successor.as_str()
                )),
                None => violations.push(format!(
                    "producer_authorities.`{}`: successor `{}` does not exist",
                    authority.producer_kind.as_str(),
                    successor.as_str()
                )),
            }
        }
    }
    by_kind
}

fn validate_negative_controls<'a>(
    catalog: &'a [super::model::NegativeControl],
    owners: &BTreeMap<u32, &super::model::OwnerAuthority>,
    violations: &mut Vec<String>,
) -> BTreeSet<&'a str> {
    let mut ids = BTreeSet::new();
    for control in catalog {
        if control.id.trim().is_empty() {
            violations.push("negative_control_catalog: empty id".to_string());
            continue;
        }
        if !ids.insert(control.id.as_str()) {
            violations.push(format!("negative_control_catalog: duplicate id `{}`", control.id));
        }
        if control.description.trim().is_empty() {
            violations
                .push(format!("negative_control_catalog.`{}`: description is empty", control.id));
        }
        if !owners.contains_key(&control.owner_issue) {
            violations.push(format!(
                "negative_control_catalog.`{}`: owner issue {} is missing",
                control.id, control.owner_issue
            ));
        }
    }
    ids
}

fn validate_invariants(
    registry: &TrustInvariantRegistry,
    owners: &BTreeMap<u32, &super::model::OwnerAuthority>,
    producers: &BTreeMap<super::model::ProducerKind, &super::model::ProducerAuthority>,
    controls: &BTreeSet<&str>,
    violations: &mut Vec<String>,
) {
    let mut seen = BTreeSet::new();
    let mut previous: Option<&str> = None;
    for row in &registry.invariants {
        if row.invariant_id.trim().is_empty() {
            violations.push("invariants: invariant_id is empty".to_string());
            continue;
        }
        if !seen.insert(row.invariant_id.as_str()) {
            violations.push(format!("invariants: duplicate invariant_id `{}`", row.invariant_id));
        }
        if let Some(prior) = previous
            && row.invariant_id.as_str() < prior
        {
            violations.push(format!(
                "invariants: `{}` is not in sorted invariant_id order",
                row.invariant_id
            ));
        }
        previous = Some(row.invariant_id.as_str());

        match owners.get(&row.owner_issue) {
            Some(owner) if matches!(owner.status, AuthorityStatus::Current) => {}
            Some(_) => violations.push(format!(
                "invariant `{}`: owner issue {} is superseded; bind a current owner",
                row.invariant_id, row.owner_issue
            )),
            None => violations.push(format!(
                "invariant `{}`: owner issue {} is missing",
                row.invariant_id, row.owner_issue
            )),
        }

        match producers.get(&row.producer_kind) {
            Some(producer) if matches!(producer.status, AuthorityStatus::Current) => {}
            Some(_) => violations.push(format!(
                "invariant `{}`: producer `{}` is superseded",
                row.invariant_id,
                row.producer_kind.as_str()
            )),
            None => violations.push(format!(
                "invariant `{}`: producer `{}` is unknown or ownerless",
                row.invariant_id,
                row.producer_kind.as_str()
            )),
        }

        if row.denominator.authority.trim().is_empty()
            || row.denominator.completeness_rule.trim().is_empty()
        {
            violations.push(format!(
                "invariant `{}`: denominator authority/completeness is omitted",
                row.invariant_id
            ));
        }
        if row.applicability.platforms.is_empty() || row.applicability.profiles.is_empty() {
            violations.push(format!(
                "invariant `{}`: applicability platforms/profiles are omitted",
                row.invariant_id
            ));
        }
        if row.negative_control_ids.is_empty() {
            violations
                .push(format!("invariant `{}`: negative_control_ids is empty", row.invariant_id));
        }
        let mut control_seen = BTreeSet::new();
        for control_id in &row.negative_control_ids {
            if !control_seen.insert(control_id.as_str()) {
                violations.push(format!(
                    "invariant `{}`: duplicate negative_control_id `{control_id}`",
                    row.invariant_id
                ));
            }
            if !controls.contains(control_id.as_str()) {
                violations.push(format!(
                    "invariant `{}`: unknown negative_control_id `{control_id}`",
                    row.invariant_id
                ));
            }
        }
        if row.release_consumers.is_empty() {
            violations
                .push(format!("invariant `{}`: release_consumers is empty", row.invariant_id));
        }
        if row.claim_boundary.trim().is_empty() {
            violations.push(format!("invariant `{}`: claim_boundary is empty", row.invariant_id));
        }
        if !matches!(row.terminal_input_states.not_proven, super::model::TerminalSemantics::Blocks)
        {
            violations
                .push(format!("invariant `{}`: terminal not_proven must block", row.invariant_id));
        }
    }
}

fn validate_controller_requirements(
    registry: &TrustInvariantRegistry,
    owners: &BTreeMap<u32, &super::model::OwnerAuthority>,
    violations: &mut Vec<String>,
) {
    let invariant_ids: BTreeSet<&str> =
        registry.invariants.iter().map(|row| row.invariant_id.as_str()).collect();
    let mut seen_controllers = BTreeSet::new();
    for requirement in &registry.controller_requirements {
        if !seen_controllers.insert(requirement.controller_issue) {
            violations.push(format!(
                "controller_requirements: duplicate controller issue {}",
                requirement.controller_issue
            ));
        }
        match owners.get(&requirement.controller_issue) {
            Some(owner) if matches!(owner.status, AuthorityStatus::Current) => {}
            Some(_) => violations.push(format!(
                "controller_requirements.#{}: controller is superseded",
                requirement.controller_issue
            )),
            None => violations.push(format!(
                "controller_requirements.#{}: controller owner is missing",
                requirement.controller_issue
            )),
        }
        if requirement.mandatory_invariant_ids.is_empty() {
            violations.push(format!(
                "controller_requirements.#{}: mandatory_invariant_ids is empty",
                requirement.controller_issue
            ));
        }
        let mut seen_ids = BTreeSet::new();
        for invariant_id in &requirement.mandatory_invariant_ids {
            if !seen_ids.insert(invariant_id.as_str()) {
                violations.push(format!(
                    "controller_requirements.#{}: duplicate invariant_id `{invariant_id}`",
                    requirement.controller_issue
                ));
            }
            if !invariant_ids.contains(invariant_id.as_str()) {
                violations.push(format!(
                    "controller_requirements.#{}: mandatory invariant `{invariant_id}` has no row",
                    requirement.controller_issue
                ));
            }
        }
    }
    for required in [4343_u32, 5900, 4346, 4350] {
        if !seen_controllers.contains(&required) {
            violations.push(format!(
                "controller_requirements: mandatory controller #{required} is missing"
            ));
        }
    }
}
