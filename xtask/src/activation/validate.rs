//! Schema and structural validation for the committed activation inventory
//! artifact and its hand-maintained override ledger (#9204).

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use serde_json::Value;

use super::derive;
use super::model::{
    ActivationClass, ActivationError, ActivationInventory, ActivationRow, INVENTORY_PATH,
    RegistrationState, SCHEMA_PATH,
};
use super::overrides;

/// Load, schema-validate, and structurally check the committed inventory,
/// then structurally check the override ledger against a fresh derivation.
pub fn validate(root: &Path) -> Result<ActivationInventory, ActivationError> {
    let bytes = fs::read(root.join(INVENTORY_PATH))
        .map_err(|error| ActivationError::new(format!("{INVENTORY_PATH}: cannot read: {error}")))?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|error| {
        ActivationError::new(format!("{INVENTORY_PATH}: invalid JSON: {error}"))
    })?;
    let inventory = validate_inventory_value(root, &value)?;

    let derived = derive::derived_class_index(root)?;
    let overrides_file = overrides::load(root)?;
    let violations = overrides::validate(root, &overrides_file, &derived);
    ActivationError::from_violations(violations)?;

    Ok(inventory)
}

/// Validate an in-memory JSON document against schema plus row-level
/// consistency rules. Does not validate the override ledger; use
/// [`validate`] for the full committed-state check.
pub fn validate_inventory_value(
    root: &Path,
    value: &Value,
) -> Result<ActivationInventory, ActivationError> {
    let schema_text = fs::read_to_string(root.join(SCHEMA_PATH))
        .map_err(|error| ActivationError::new(format!("{SCHEMA_PATH}: cannot read: {error}")))?;
    let schema: Value = serde_json::from_str(&schema_text)
        .map_err(|error| ActivationError::new(format!("{SCHEMA_PATH}: invalid JSON: {error}")))?;

    let mut violations = Vec::new();
    violations.extend(schema_violations(&schema, value)?);
    check_raw_rows(value, &mut violations);

    // Always attempt typed decode so structural checks below can run even
    // when the row above has schema or raw-check defects (e.g. an empty
    // string, or unsorted rows) that decode itself tolerates. Only a
    // genuine decode failure (e.g. an enum string with no matching variant)
    // short-circuits, and even then every violation collected so far is
    // still reported together with the decode failure.
    let inventory: ActivationInventory = match serde_json::from_value(value.clone()) {
        Ok(inventory) => inventory,
        Err(error) => {
            violations.push(format!("{INVENTORY_PATH}: typed decode failed: {error}"));
            return Err(ActivationError::many(&violations));
        }
    };

    validate_rows(root, &inventory, &mut violations);
    ActivationError::from_violations(violations)?;

    Ok(inventory)
}

fn schema_violations(schema: &Value, value: &Value) -> Result<Vec<String>, ActivationError> {
    let validator = jsonschema::validator_for(schema)
        .map_err(|error| ActivationError::new(format!("{SCHEMA_PATH}: invalid schema: {error}")))?;
    Ok(validator.iter_errors(value).map(|error| format!("schema: {error}")).collect())
}

/// Checks that must run on the raw JSON before typed decode, because an
/// unknown `class` string would otherwise make `serde` decode fail hard
/// before we can report the specific rule violated.
fn check_raw_rows(value: &Value, violations: &mut Vec<String>) {
    let Some(rows) = value.get("rows").and_then(Value::as_array) else {
        violations.push("rows: missing or not an array".to_string());
        return;
    };
    let mut seen = BTreeSet::new();
    let mut previous: Option<&str> = None;
    for row in rows {
        let surface_id = row.get("surface_id").and_then(Value::as_str).unwrap_or("<missing>");
        if !seen.insert(surface_id) {
            violations.push(format!("duplicate surface id `{surface_id}`"));
        }
        let class = row.get("class").and_then(Value::as_str).unwrap_or("<missing>");
        if ActivationClass::from_str(class).is_none() {
            violations.push(format!("row `{surface_id}`: unknown activation class `{class}`"));
        }
        if let Some(previous_id) = previous
            && previous_id > surface_id
        {
            violations.push(format!(
                "rows are not sorted by surface id (`{previous_id}` before `{surface_id}`)"
            ));
        }
        previous = Some(surface_id);
    }
}

fn validate_rows(root: &Path, inventory: &ActivationInventory, violations: &mut Vec<String>) {
    for row in &inventory.rows {
        check_authority_path(
            root,
            &row.surface_id,
            "class_authority.authority",
            &row.class_authority.authority,
            violations,
        );
        check_authority_path(
            root,
            &row.surface_id,
            "semantic_authority",
            &row.semantic_authority,
            violations,
        );
        check_authority_path(
            root,
            &row.surface_id,
            "publication.authority",
            &row.publication.authority,
            violations,
        );
        if let Some(authority) = &row.registration.authority {
            check_authority_path(
                root,
                &row.surface_id,
                "registration.authority",
                authority,
                violations,
            );
        }
        if let Some(authority) = &row.maturity_authority {
            check_authority_path(
                root,
                &row.surface_id,
                "maturity_authority",
                authority,
                violations,
            );
        }
        // A consumer or proof reference that names a repository path must
        // resolve. These are the fields a downstream reader would dereference,
        // so a stale one is a false pointer, not a cosmetic defect. Values
        // without a `/` are package or harness names, not paths, and are left
        // to their own authority.
        for consumer in &row.consumers {
            if consumer.contains('/') {
                check_authority_path(root, &row.surface_id, "consumers", consumer, violations);
            }
        }
        for proof in &row.proof_references {
            if proof.id.contains('/') {
                check_authority_path(
                    root,
                    &row.surface_id,
                    "proof_references",
                    &proof.id,
                    violations,
                );
            }
        }
        validate_class_rules(row, violations);
    }
}

fn validate_class_rules(row: &ActivationRow, violations: &mut Vec<String>) {
    if row.owner.trim().is_empty() {
        violations.push(format!("row `{}`: requires a non-blank owner", row.surface_id));
    }
    // `established` asserts the surface is actually wired into its consuming
    // mechanism. Without an authority naming where, the claim is unfalsifiable
    // by the reader it exists for. The schema encodes the same rule for
    // external consumers; this restates it so the failure reads as a rule
    // rather than as a raw schema error.
    if row.registration.state == RegistrationState::Established
        && row.registration.authority.as_deref().map(str::trim).unwrap_or("").is_empty()
    {
        violations.push(format!(
            "row `{}`: established registration requires an authority naming where it is registered",
            row.surface_id
        ));
    }
    if row.class == ActivationClass::Product {
        // `unowned` is the closed token a derivation records when its
        // authority declares no owner. A product surface with no owner is a
        // contradiction, so the token is admissible on other classes and not
        // on this one.
        if row.owner == derive::UNOWNED {
            violations.push(format!(
                "row `{}`: product row requires a real owner, not `{}`",
                row.surface_id,
                derive::UNOWNED
            ));
        }
        if row.semantic_authority.trim().is_empty() {
            violations.push(format!(
                "row `{}`: product row requires a semantic authority",
                row.surface_id
            ));
        }
        if row.consumers.is_empty() {
            violations.push(format!(
                "row `{}`: product row requires at least one consumer",
                row.surface_id
            ));
        }
    }
    if row.class == ActivationClass::CompatibilityShim && row.retirement.is_none() {
        violations.push(format!(
            "row `{}`: compatibility shim requires a retirement owner and boundary",
            row.surface_id
        ));
    }
}

fn check_authority_path(
    root: &Path,
    surface_id: &str,
    label: &str,
    value: &str,
    violations: &mut Vec<String>,
) {
    let mut parts = value.splitn(2, '#');
    let path_part = parts.next().unwrap_or(value);
    let fragment = parts.next();
    if !is_repository_relative(path_part) {
        violations.push(format!(
            "row `{surface_id}`: authority path `{path_part}` referenced by {label} is not \
             repository-relative (`{value}`)"
        ));
        return;
    }
    if path_part.is_empty() || !root.join(path_part).exists() {
        violations.push(format!(
            "row `{surface_id}`: missing authority path `{path_part}` referenced by {label} (`{value}`)"
        ));
        return;
    }
    if let Some(fragment) = fragment
        && !authority_fragment_exists(root, path_part, fragment)
    {
        violations.push(format!(
            "row `{surface_id}`: missing authority fragment `{fragment}` in `{path_part}` referenced by {label} (`{value}`)"
        ));
    }
}

/// An authority reference must name a path inside the repository.
///
/// `Path::join` silently discards `root` for an absolute path, and a `..`
/// component climbs out of it, so an unconstrained check would consult the
/// host filesystem: `/etc/hostname` would "exist" and a dangling in-repository
/// reference could be masked by a file outside the tree. Only `Normal`
/// components are admissible.
pub(super) fn is_repository_relative(path: &str) -> bool {
    !path.is_empty()
        && std::path::Path::new(path)
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
}

fn authority_fragment_exists(root: &Path, path: &str, fragment: &str) -> bool {
    let Ok(text) = fs::read_to_string(root.join(path)) else {
        return false;
    };
    if path == "features.toml" {
        return toml::from_str::<toml::Value>(&text)
            .ok()
            .and_then(|value| value.get("feature")?.as_array().cloned())
            .is_some_and(|rows| {
                rows.iter().any(|row| row.get("id").and_then(toml::Value::as_str) == Some(fragment))
            });
    }
    if path == ".ci/gate-policy.yaml" {
        return serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&text)
            .ok()
            .and_then(|value| value.get("gates")?.as_sequence().cloned())
            .is_some_and(|rows| {
                rows.iter().any(|row| {
                    row.get("name").and_then(serde_yaml_ng::Value::as_str) == Some(fragment)
                })
            });
    }
    let Ok(document) = toml::from_str::<toml::Value>(&text) else {
        return false;
    };
    if path.ends_with("Cargo.toml")
        && let Some((section, name)) = fragment.split_once('.')
    {
        match section {
            "bench" => {
                return document.get("bench").and_then(toml::Value::as_array).is_some_and(|rows| {
                    rows.iter()
                        .any(|row| row.get("name").and_then(toml::Value::as_str) == Some(name))
                });
            }
            "features" => {
                return document
                    .get("features")
                    .and_then(toml::Value::as_table)
                    .is_some_and(|features| features.contains_key(name));
            }
            _ => {}
        }
    }
    let mut current = &document;
    for component in fragment.split('.') {
        let Some(next) = current.get(component) else {
            return false;
        };
        current = next;
    }
    true
}
