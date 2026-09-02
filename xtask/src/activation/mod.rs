//! Versioned activation inventory: schema, deterministic generation from
//! existing repository authorities, and fail-closed validation (#9204).
//!
//! This module only builds and validates the inventory. It does not
//! implement activation *checking* semantics (`check`/`report`/`explain`
//! belong to #9205) and does not change any runtime behavior.

mod derive;
mod model;
mod overrides;
mod render;
mod validate;

pub use derive::{UNOWNED, derived_class_index};
pub use model::{
    ActivationClass, ActivationError, ActivationInventory, ActivationRow, INVENTORY_PATH,
    OVERRIDES_PATH, SCHEMA_PATH,
};
pub use overrides::{
    OverrideRecord, OverridesFile, load as load_overrides, validate as validate_overrides,
};
pub use render::render_list;
pub use validate::{validate, validate_inventory_value};

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use model::DerivationEntry;

/// Fold every rule's rows into one surface-keyed map.
///
/// Two rules claiming the same surface is the "one surface appears in two
/// incompatible activation classes" defect. It is caught here rather than
/// letting the later rule silently win: a `BTreeMap::insert` that returns an
/// old row means the inventory would otherwise have hidden a real
/// classification conflict behind whichever rule happened to run last.
fn merge_rule_rows(
    rule_outputs: Vec<derive::RuleOutput>,
) -> Result<BTreeMap<String, ActivationRow>, ActivationError> {
    let mut rows: BTreeMap<String, ActivationRow> = BTreeMap::new();
    let mut collisions = Vec::new();
    for output in rule_outputs {
        for row in output.rows {
            if let Some(previous) = rows.insert(row.surface_id.clone(), row.clone()) {
                collisions.push(format!(
                    "duplicate surface id `{}`: rule `{}` emits class `{}` and rule `{}` emits class `{}`",
                    row.surface_id,
                    previous.class_authority.rule,
                    previous.class.as_str(),
                    row.class_authority.rule,
                    row.class.as_str()
                ));
            }
        }
    }
    ActivationError::from_violations(collisions)?;
    Ok(rows)
}

/// Recompute the inventory in memory from every current authority. Does not
/// read or write `policy/activation-inventory.v1.json`.
pub fn generate(root: &Path) -> Result<ActivationInventory, ActivationError> {
    let rule_outputs = derive::derive_all(root)?;
    let mut derivation: Vec<DerivationEntry> =
        rule_outputs.iter().map(|output| output.entry.clone()).collect();

    let mut rows = merge_rule_rows(rule_outputs)?;
    let derived_index: BTreeMap<String, ActivationClass> =
        rows.iter().map(|(surface_id, row)| (surface_id.clone(), row.class)).collect();

    let overrides_file = overrides::load(root)?;
    let violations = overrides::validate(root, &overrides_file, &derived_index);
    ActivationError::from_violations(violations)?;

    let override_rows = overrides::build_rows(&overrides_file);
    derivation.push(overrides::derivation_entry(&overrides_file, override_rows.len()));
    for row in override_rows {
        rows.insert(row.surface_id.clone(), row);
    }

    derivation.sort_by(|left, right| left.rule.cmp(&right.rule));
    let rows: Vec<ActivationRow> = rows.into_values().collect();

    let inventory = ActivationInventory {
        schema: model::SCHEMA_PATH.to_string(),
        schema_version: model::SCHEMA_VERSION.to_string(),
        policy: model::POLICY_NAME.to_string(),
        owner: model::OWNER.to_string(),
        controlling_issue: model::CONTROLLING_ISSUE.to_string(),
        derivation,
        rows,
    };

    // The generator must never be able to emit an artifact that
    // `cargo xtask activation validate` would then reject. Row-level rules
    // (authority paths that exist, product rows with a consumer, a shim with
    // a retirement boundary) apply to override-built rows just as much as to
    // derived ones, and only the artifact-level check sees both together.
    let value = serde_json::to_value(&inventory).map_err(|error| {
        ActivationError::new(format!("cannot serialize generated inventory: {error}"))
    })?;
    validate::validate_inventory_value(root, &value)?;

    Ok(inventory)
}

/// Regenerate in memory and fail with `activation inventory is stale` if the
/// committed artifact would change.
pub fn check_drift(root: &Path) -> Result<ActivationInventory, ActivationError> {
    let inventory = generate(root)?;
    let expected = inventory.to_bytes()?;
    let actual = fs::read(root.join(model::INVENTORY_PATH)).map_err(|error| {
        ActivationError::new(format!("{}: cannot read: {error}", model::INVENTORY_PATH))
    })?;
    if actual != expected {
        return Err(ActivationError::new(format!(
            "{}: activation inventory is stale; run `cargo xtask activation generate --write`",
            model::INVENTORY_PATH
        )));
    }
    Ok(inventory)
}

/// Regenerate and rewrite the committed artifact.
pub fn write(root: &Path) -> Result<ActivationInventory, ActivationError> {
    let inventory = generate(root)?;
    let bytes = inventory.to_bytes()?;
    fs::write(root.join(model::INVENTORY_PATH), &bytes).map_err(|error| {
        ActivationError::new(format!("{}: cannot write: {error}", model::INVENTORY_PATH))
    })?;
    Ok(inventory)
}

/// Row counts per class, in [`ActivationClass::all`] order.
#[must_use]
pub fn class_counts(inventory: &ActivationInventory) -> Vec<(ActivationClass, usize)> {
    ActivationClass::all()
        .iter()
        .map(|class| {
            let count = inventory.rows.iter().filter(|row| row.class == *class).count();
            (*class, count)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::{
        ClassAuthority, ClassAuthorityKind, DerivationEntry, Promotion, PromotionState,
        Publication, PublicationState, Registration, RegistrationState,
    };

    /// The real authorities cannot currently produce two rules that claim one
    /// surface, so the collision guard is proved against synthetic rule output
    /// rather than by corrupting a committed authority file.
    fn row(surface_id: &str, class: ActivationClass, rule: &str) -> ActivationRow {
        ActivationRow {
            surface_id: surface_id.to_string(),
            class,
            class_authority: ClassAuthority {
                kind: ClassAuthorityKind::Derived,
                authority: "features.toml".to_string(),
                rule: rule.to_string(),
            },
            semantic_authority: "features.toml".to_string(),
            consumers: Vec::new(),
            compile_profiles: Vec::new(),
            registration: Registration {
                state: RegistrationState::NotEstablished,
                authority: None,
                detail: None,
            },
            data_authority: None,
            observable_contract: None,
            proof_references: Vec::new(),
            publication: Publication {
                state: PublicationState::NotApplicable,
                authority: "features.toml".to_string(),
            },
            maturity_authority: None,
            owner: "test".to_string(),
            promotion: Promotion { state: PromotionState::NotEvaluated, blocker: None },
            retirement: None,
            notes: None,
        }
    }

    fn output(rows: Vec<ActivationRow>, rule: &str) -> derive::RuleOutput {
        derive::RuleOutput {
            rows,
            entry: DerivationEntry {
                rule: rule.to_string(),
                authority: "features.toml".to_string(),
                emits: "product".to_string(),
                considered: 0,
                emitted: 0,
                not_seeded_reason: String::new(),
            },
        }
    }

    #[test]
    fn distinct_surfaces_merge_without_collision() {
        let merged = merge_rule_rows(vec![
            output(vec![row("feature:a", ActivationClass::Product, "rule-one")], "rule-one"),
            output(vec![row("feature:b", ActivationClass::Lab, "rule-two")], "rule-two"),
        ]);
        assert!(merged.is_ok(), "distinct surface ids must merge cleanly");
    }

    #[test]
    fn two_rules_claiming_one_surface_in_different_classes_fail() {
        let merged = merge_rule_rows(vec![
            output(vec![row("feature:a", ActivationClass::Product, "rule-one")], "rule-one"),
            output(vec![row("feature:a", ActivationClass::Lab, "rule-two")], "rule-two"),
        ]);
        let message = match merged {
            Ok(_) => "collision unexpectedly merged".to_string(),
            Err(error) => error.to_string(),
        };
        assert!(message.contains("duplicate surface id `feature:a`"), "{message}");
        assert!(message.contains("rule `rule-one` emits class `product`"), "{message}");
        assert!(message.contains("rule `rule-two` emits class `lab`"), "{message}");
    }
}
