//! Versioned compiler capability profiles.
//!
//! This module owns the wire contract for `compiler_profile.v1`.  It does not
//! decide whether the compiler satisfies a profile; later gates consume these
//! declarations together with generated capability state.  Keeping loading,
//! validation, and canonical identity here prevents each consumer from
//! inventing a subtly different profile model.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs, path::Path};

pub const SCHEMA_VERSION: &str = "compiler_profile.v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CompilerProfile {
    pub schema_version: String,
    pub profile_id: String,
    pub profile_version: String,
    pub purpose: String,
    #[serde(default)]
    pub comparison_series: Vec<String>,
    #[serde(default)]
    pub required_ast_kinds: Vec<String>,
    #[serde(default)]
    pub required_hir_nodes: Vec<String>,
    #[serde(default)]
    pub required_pir_operations: Vec<String>,
    #[serde(default)]
    pub required_place_kinds: Vec<String>,
    #[serde(default)]
    pub required_context_dimensions: Vec<String>,
    #[serde(default)]
    pub required_cfg_edges: Vec<String>,
    #[serde(default)]
    pub required_fact_classes: Vec<String>,
    #[serde(default)]
    pub allowed_dynamic_boundaries: Vec<DynamicBoundary>,
    #[serde(default)]
    pub explicit_exclusions: Vec<ProfileExclusion>,
    #[serde(default)]
    pub unsupported_bridges: Vec<UnsupportedBridge>,
    #[serde(default)]
    pub consumers: Vec<String>,
    #[serde(default)]
    pub gold_oracle_eir_requirements: Vec<String>,
    pub owner_issue: String,
    #[serde(default)]
    pub predecessor_profile: Option<String>,
    pub change_reason: String,
    pub claim_boundary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DynamicBoundary {
    pub id: String,
    pub construct: String,
    pub reason: String,
    pub owner_issue: String,
    pub stop_condition: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProfileExclusion {
    pub id: String,
    pub construct: String,
    pub reason: String,
    pub reviewed_by: String,
    pub owner_issue: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct UnsupportedBridge {
    pub id: String,
    pub construct: String,
    pub representation: String,
    pub owner_issue: String,
    pub stop_condition: String,
}

impl CompilerProfile {
    /// Parse and validate a profile document without consulting compiler state.
    pub fn from_str(source: &str) -> Result<Self> {
        let profile: Self = serde_yaml_ng::from_str(source).context("parse compiler profile")?;
        profile.validate()?;
        Ok(profile)
    }

    /// Load and validate a profile from its durable repository path.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let source = fs::read_to_string(path)
            .with_context(|| format!("read compiler profile {}", path.display()))?;
        Self::from_str(&source)
            .with_context(|| format!("validate compiler profile {}", path.display()))
    }

    /// Reject malformed identity and unowned boundary/debt declarations early.
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SCHEMA_VERSION {
            bail!(
                "unsupported compiler profile schema {:?}; expected {:?}",
                self.schema_version,
                SCHEMA_VERSION
            );
        }
        for (name, value) in [
            ("profile_id", &self.profile_id),
            ("profile_version", &self.profile_version),
            ("purpose", &self.purpose),
            ("owner_issue", &self.owner_issue),
            ("change_reason", &self.change_reason),
            ("claim_boundary", &self.claim_boundary),
        ] {
            if value.trim().is_empty() {
                bail!("compiler profile field {name} must not be empty");
            }
        }
        if self.predecessor_profile.as_deref().is_some_and(|value| value.trim().is_empty()) {
            bail!("compiler profile field predecessor_profile must not be empty when present");
        }
        if !self.profile_id.ends_with("_v1") {
            bail!("profile_id must be versioned with the _v1 suffix");
        }
        if !self.profile_version.starts_with('v') {
            bail!("profile_version must start with 'v'");
        }
        validate_unique_strings("comparison_series", &self.comparison_series)?;
        validate_required_strings("required_ast_kinds", &self.required_ast_kinds)?;
        validate_required_strings("required_hir_nodes", &self.required_hir_nodes)?;
        validate_required_strings("required_pir_operations", &self.required_pir_operations)?;
        validate_required_strings("required_place_kinds", &self.required_place_kinds)?;
        validate_required_strings(
            "required_context_dimensions",
            &self.required_context_dimensions,
        )?;
        validate_required_strings("required_cfg_edges", &self.required_cfg_edges)?;
        validate_required_strings("required_fact_classes", &self.required_fact_classes)?;
        validate_required_strings("consumers", &self.consumers)?;
        validate_required_strings(
            "gold_oracle_eir_requirements",
            &self.gold_oracle_eir_requirements,
        )?;
        for boundary in &self.allowed_dynamic_boundaries {
            boundary.validate()?;
        }
        for exclusion in &self.explicit_exclusions {
            exclusion.validate()?;
        }
        for bridge in &self.unsupported_bridges {
            bridge.validate()?;
        }
        ensure_distinct_ids(
            self.allowed_dynamic_boundaries.iter().map(|entry| entry.id.as_str()),
            "allowed_dynamic_boundaries",
        )?;
        ensure_distinct_ids(
            self.explicit_exclusions.iter().map(|entry| entry.id.as_str()),
            "explicit_exclusions",
        )?;
        ensure_distinct_ids(
            self.unsupported_bridges.iter().map(|entry| entry.id.as_str()),
            "unsupported_bridges",
        )?;
        Ok(())
    }

    /// Return stable JSON identity bytes with order-insensitive lists sorted.
    pub fn canonical_json(&self) -> Result<String> {
        self.validate()?;
        let mut normalized = self.clone();
        for values in [
            &mut normalized.comparison_series,
            &mut normalized.required_ast_kinds,
            &mut normalized.required_hir_nodes,
            &mut normalized.required_pir_operations,
            &mut normalized.required_place_kinds,
            &mut normalized.required_context_dimensions,
            &mut normalized.required_cfg_edges,
            &mut normalized.required_fact_classes,
            &mut normalized.consumers,
            &mut normalized.gold_oracle_eir_requirements,
        ] {
            values.sort();
        }
        normalized.allowed_dynamic_boundaries.sort_by(|a, b| a.id.cmp(&b.id));
        normalized.explicit_exclusions.sort_by(|a, b| a.id.cmp(&b.id));
        normalized.unsupported_bridges.sort_by(|a, b| a.id.cmp(&b.id));
        serde_json::to_string_pretty(&normalized).context("serialize canonical compiler profile")
    }

    pub fn identity_sha256(&self) -> Result<String> {
        let canonical = self.canonical_json()?;
        let digest = Sha256::digest(canonical.as_bytes());
        Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
    }
}

const PROFILE_PATHS: [&str; 2] = [
    "docs/compiler/profiles/selected_upstream_v1.yaml",
    "docs/compiler/profiles/lsp_exactness_v1.yaml",
];

/// Validate one profile and print its stable identity.
pub fn check(path: &Path) -> Result<()> {
    let profile = CompilerProfile::load(path)?;
    println!("{} {} {}", profile.profile_id, profile.profile_version, profile.identity_sha256()?);
    Ok(())
}

/// Validate the committed v1 profile set and print their stable identities.
pub fn list(root: &Path) -> Result<()> {
    for relative_path in PROFILE_PATHS {
        check(&root.join(relative_path))?;
    }
    Ok(())
}

impl DynamicBoundary {
    fn validate(&self) -> Result<()> {
        validate_owned_entry(
            "dynamic boundary",
            &self.id,
            &self.construct,
            "reason",
            &self.reason,
            &self.owner_issue,
            Some(&self.stop_condition),
        )
    }
}

impl ProfileExclusion {
    fn validate(&self) -> Result<()> {
        validate_owned_entry(
            "profile exclusion",
            &self.id,
            &self.construct,
            "reason",
            &self.reason,
            &self.owner_issue,
            None,
        )?;
        if self.reviewed_by.trim().is_empty() {
            bail!("profile exclusion {} must name reviewed_by", self.id);
        }
        Ok(())
    }
}

impl UnsupportedBridge {
    fn validate(&self) -> Result<()> {
        validate_owned_entry(
            "unsupported bridge",
            &self.id,
            &self.construct,
            "representation",
            &self.representation,
            &self.owner_issue,
            Some(&self.stop_condition),
        )
    }
}

fn validate_owned_entry(
    kind: &str,
    id: &str,
    construct: &str,
    detail_name: &str,
    detail: &str,
    owner_issue: &str,
    stop_condition: Option<&String>,
) -> Result<()> {
    for (field, value) in
        [("id", id), ("construct", construct), (detail_name, detail), ("owner_issue", owner_issue)]
    {
        if value.trim().is_empty() {
            bail!("{kind} {id:?} has empty {field}");
        }
    }
    if let Some(stop_condition) = stop_condition
        && stop_condition.trim().is_empty() {
            bail!("{kind} {id:?} must have a stop_condition");
        }
    Ok(())
}

fn validate_unique_strings(name: &str, values: &[String]) -> Result<()> {
    if values.iter().any(|value| value.trim().is_empty()) {
        bail!("{name} must not contain empty entries");
    }
    let unique = values.iter().collect::<BTreeSet<_>>();
    if unique.len() != values.len() {
        bail!("{name} must not contain duplicate entries");
    }
    Ok(())
}

fn validate_required_strings(name: &str, values: &[String]) -> Result<()> {
    validate_unique_strings(name, values)?;
    if values.is_empty() {
        bail!("{name} must contain at least one entry");
    }
    Ok(())
}

fn ensure_distinct_ids<'a>(ids: impl Iterator<Item = &'a str>, name: &str) -> Result<()> {
    let ids = ids.collect::<Vec<_>>();
    if ids.iter().any(|id| id.trim().is_empty()) {
        bail!("{name} must not contain an empty id");
    }
    let unique = ids.iter().collect::<BTreeSet<_>>();
    if unique.len() != ids.len() {
        bail!("{name} must not contain duplicate ids");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SELECTED: &str =
        include_str!("../../../docs/compiler/profiles/selected_upstream_v1.yaml");
    const LSP: &str = include_str!("../../../docs/compiler/profiles/lsp_exactness_v1.yaml");

    #[test]
    fn committed_profiles_load_and_have_stable_identity() -> Result<()> {
        for (source, expected_identity) in [
            (SELECTED, "57970e560564a1ded1a1a244a0bba877a248e97a06fd686bced3e9cc456f2e22"),
            (LSP, "1e1aae7db3c2d6f001819d6e9d6093496b050fd796177311cf2478e7d9e033f6"),
        ] {
            let profile = CompilerProfile::from_str(source)?;
            assert_eq!(profile.schema_version, SCHEMA_VERSION);
            assert_eq!(profile.owner_issue, "#5215");
            assert!(!profile.canonical_json()?.contains("\\\\"));
            assert_eq!(profile.identity_sha256()?, expected_identity);
        }
        Ok(())
    }

    #[test]
    fn canonical_identity_ignores_order_but_not_membership() -> Result<()> {
        let first =
            CompilerProfile::from_str(&SELECTED.replace("  - comp\n  - run", "  - run\n  - comp"))?;
        let second = CompilerProfile::from_str(SELECTED)?;
        assert_eq!(first.identity_sha256()?, second.identity_sha256()?);

        let changed = SELECTED.replace("- comp", "- compare");
        let changed = CompilerProfile::from_str(&changed)?;
        assert_ne!(changed.identity_sha256()?, second.identity_sha256()?);
        Ok(())
    }

    #[test]
    fn malformed_schema_and_unowned_boundary_fail_closed() -> Result<()> {
        let bad_schema = SELECTED.replace(SCHEMA_VERSION, "compiler_profile.v2");
        assert!(
            CompilerProfile::from_str(&bad_schema).is_err(),
            "a profile with an unsupported schema must fail closed"
        );

        let bad_boundary = LSP.replace("owner_issue: \"#5215\"", "owner_issue: \"\"");
        assert!(
            CompilerProfile::from_str(&bad_boundary).is_err(),
            "an unowned boundary must fail closed"
        );

        let empty_predecessor =
            LSP.replace("predecessor_profile: null", "predecessor_profile: \"   \"");
        assert!(
            CompilerProfile::from_str(&empty_predecessor).is_err(),
            "a present but empty predecessor profile must fail closed"
        );

        let empty_required = LSP.replace(
            "required_hir_nodes:\n  - binding\n  - place\n  - source_anchor",
            "required_hir_nodes: []",
        );
        let empty_required_error = CompilerProfile::from_str(&empty_required)
            .expect_err("an empty required obligation must fail closed")
            .to_string();
        assert!(
            empty_required_error.contains("required_hir_nodes"),
            "empty required obligation error should name the field: {empty_required_error}"
        );

        let missing_required =
            LSP.replace("required_hir_nodes:\n  - binding\n  - place\n  - source_anchor\n", "");
        assert!(
            CompilerProfile::from_str(&missing_required).is_err(),
            "an omitted required obligation must fail closed"
        );

        let empty_representation =
            LSP.replace("representation: legacy text scan", "representation: \"\"");
        let representation_error = CompilerProfile::from_str(&empty_representation)
            .expect_err("an unsupported bridge without representation must fail closed")
            .to_string();
        assert!(
            representation_error.contains("representation")
                && !representation_error.contains("empty reason"),
            "unsupported bridge error should identify representation: {representation_error}"
        );
        Ok(())
    }

    #[test]
    fn exclusion_boundary_and_bridge_are_distinct_typed_states() -> Result<()> {
        let profile = CompilerProfile::from_str(LSP)?;
        assert_eq!(profile.allowed_dynamic_boundaries.len(), 1);
        assert_eq!(profile.explicit_exclusions.len(), 1);
        assert_eq!(profile.unsupported_bridges.len(), 1);
        assert_ne!(profile.allowed_dynamic_boundaries[0].id, profile.unsupported_bridges[0].id);
        Ok(())
    }
}
