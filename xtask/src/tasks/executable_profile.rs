//! Versioned executable-profile manifests (#4773, slice S2).
//!
//! This module owns the wire contract for `executable_profile.v1`: the
//! versioned manifest declaring which Perl subset, capability policy, and
//! budgets are admitted for evaluation under PLSP-SPEC-0035 C1. It validates
//! declarations only; it performs no evaluation and makes no EIR claim.
//! Unsupported-boundary membership validation against the #4770 catalog lands
//! in S4; S2 manifests carry an empty boundary list and fail closed otherwise.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs, path::Path};

pub const SCHEMA_VERSION: &str = "executable_profile.v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutableProfile {
    pub schema_version: String,
    pub profile_id: String,
    pub profile_version: String,
    pub purpose: String,
    pub supported_perl_versions: Vec<String>,
    pub supported_constructs: Vec<String>,
    pub supported_builtins: Vec<BuiltinEntry>,
    pub supported_regex_subset: String,
    pub module_policy: String,
    pub filesystem_policy: String,
    pub io_policy: String,
    pub environment_policy: String,
    pub network_policy: String,
    pub xs_policy: String,
    pub source_filter_policy: String,
    pub dynamic_boundary_behavior: String,
    pub warnings_behavior: String,
    pub exceptions_behavior: String,
    pub tap_behavior: String,
    pub budgets: Budgets,
    pub determinism_guarantees: String,
    pub oracle_agreement_thresholds: String,
    pub unsupported_boundary_ids: Vec<String>,
    pub owner_issue: String,
    #[serde(default)]
    pub predecessor_profile: Option<String>,
    pub change_reason: String,
    pub claim_boundary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BuiltinEntry {
    pub name: String,
    pub behavior: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Budgets {
    pub max_steps: u64,
    pub max_time_ms: u64,
    pub max_memory_bytes: u64,
    pub max_recursion_depth: u64,
    pub max_output_bytes: u64,
}

impl ExecutableProfile {
    /// Parse and validate a manifest document without evaluating anything.
    pub fn from_str(source: &str) -> Result<Self> {
        let profile: Self = serde_yaml_ng::from_str(source).context("parse executable profile")?;
        profile.validate()?;
        Ok(profile)
    }

    /// Load and validate a manifest from its repository path.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let source = fs::read_to_string(path)
            .with_context(|| format!("read executable profile {}", path.display()))?;
        Self::from_str(&source)
            .with_context(|| format!("validate executable profile {}", path.display()))
    }

    /// Reject malformed identity, missing declarations, and premature
    /// boundary wiring early. Everything unlisted fails closed.
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SCHEMA_VERSION {
            bail!(
                "unsupported executable profile schema {:?}; expected {:?}",
                self.schema_version,
                SCHEMA_VERSION
            );
        }
        for (name, value) in [
            ("profile_id", &self.profile_id),
            ("profile_version", &self.profile_version),
            ("purpose", &self.purpose),
            ("supported_regex_subset", &self.supported_regex_subset),
            ("module_policy", &self.module_policy),
            ("filesystem_policy", &self.filesystem_policy),
            ("io_policy", &self.io_policy),
            ("environment_policy", &self.environment_policy),
            ("network_policy", &self.network_policy),
            ("xs_policy", &self.xs_policy),
            ("source_filter_policy", &self.source_filter_policy),
            ("dynamic_boundary_behavior", &self.dynamic_boundary_behavior),
            ("warnings_behavior", &self.warnings_behavior),
            ("exceptions_behavior", &self.exceptions_behavior),
            ("tap_behavior", &self.tap_behavior),
            ("determinism_guarantees", &self.determinism_guarantees),
            ("oracle_agreement_thresholds", &self.oracle_agreement_thresholds),
            ("owner_issue", &self.owner_issue),
            ("change_reason", &self.change_reason),
            ("claim_boundary", &self.claim_boundary),
        ] {
            if value.trim().is_empty() {
                bail!("executable profile field {name} must not be empty");
            }
        }
        if self.predecessor_profile.as_deref().is_some_and(|value| value.trim().is_empty()) {
            bail!("executable profile field predecessor_profile must not be empty when present");
        }
        let has_name_and_suffix =
            self.profile_id.strip_suffix("_v1").is_some_and(|prefix| !prefix.is_empty());
        if !is_snake_case_id(&self.profile_id) || !has_name_and_suffix {
            bail!(
                "profile_id must be lowercase snake_case ASCII with a non-empty name and the _v1 suffix"
            );
        }
        if !is_profile_version(&self.profile_version) {
            bail!("profile_version must be 'v' followed by version characters (no whitespace)");
        }
        validate_required_strings("supported_perl_versions", &self.supported_perl_versions)?;
        validate_required_strings("supported_constructs", &self.supported_constructs)?;
        for builtin in &self.supported_builtins {
            builtin.validate()?;
        }
        ensure_distinct_values(
            self.supported_builtins.iter().map(|entry| entry.name.as_str()),
            "supported_builtins",
        )?;
        validate_boundary_ids(&self.unsupported_boundary_ids)?;
        Ok(())
    }

    /// Return stable JSON identity bytes with order-insensitive lists sorted.
    pub fn canonical_json(&self) -> Result<String> {
        self.validate()?;
        let mut normalized = self.clone();
        normalized.supported_perl_versions.sort();
        normalized.supported_constructs.sort();
        normalized.supported_builtins.sort_by(|a, b| a.name.cmp(&b.name));
        normalized.unsupported_boundary_ids.sort();
        serde_json::to_string_pretty(&normalized).context("serialize canonical executable profile")
    }

    pub fn identity_sha256(&self) -> Result<String> {
        let canonical = self.canonical_json()?;
        let digest = Sha256::digest(canonical.as_bytes());
        Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
    }

    /// Deterministic human-readable summary of the admitted subset.
    pub fn report_string(&self) -> Result<String> {
        self.validate()?;
        let mut lines = vec![
            format!("profile: {} {}", self.profile_id, self.profile_version),
            format!("manifest_sha256: {}", self.identity_sha256()?),
            format!("perl_versions: {}", self.supported_perl_versions.len()),
            format!("constructs: {}", self.supported_constructs.len()),
            format!("builtins: {}", self.supported_builtins.len()),
            format!("regex_subset: {}", self.supported_regex_subset),
            format!("boundaries: {}", self.unsupported_boundary_ids.len()),
            format!("claim_boundary: {}", self.claim_boundary),
        ];
        lines.push(String::new());
        Ok(lines.join("\n"))
    }
}

/// Validate one manifest and print its stable identity.
pub fn check(path: &Path) -> Result<()> {
    let profile = ExecutableProfile::load(path)?;
    println!("{} {} {}", profile.profile_id, profile.profile_version, profile.identity_sha256()?);
    Ok(())
}

/// Validate one manifest and print its deterministic subset summary.
pub fn report(path: &Path) -> Result<()> {
    let profile = ExecutableProfile::load(path)?;
    print!("{}", profile.report_string()?);
    Ok(())
}

impl BuiltinEntry {
    fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            bail!("supported builtin name must not be empty");
        }
        if self.behavior.trim().is_empty() {
            bail!("supported builtin {:?} must declare its behavior", self.name);
        }
        Ok(())
    }
}

/// S2 boundary rule: IDs keep the SPEC-0038 shape and uniqueness contract so
/// S4 inherits the mechanism, but the list itself stays empty until the #4770
/// catalog loader lands. Any entry fails closed.
fn is_snake_case_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn is_profile_version(value: &str) -> bool {
    value.len() > 1
        && value.starts_with('v')
        && value.bytes().skip(1).all(|byte| {
            byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'_' || byte == b'-'
        })
}

fn validate_boundary_ids(ids: &[String]) -> Result<()> {
    for id in ids {
        if !is_snake_case_id(id) {
            bail!("unsupported boundary id {id:?} must be lowercase snake_case ASCII");
        }
    }
    ensure_distinct_values(ids.iter().map(String::as_str), "unsupported_boundary_ids")?;
    if !ids.is_empty() {
        bail!(
            "unsupported_boundary_ids must stay empty until S4 wires the #4770 catalog; refusing {}",
            ids.join(", ")
        );
    }
    Ok(())
}

fn ensure_distinct_values<'a>(values: impl Iterator<Item = &'a str>, name: &str) -> Result<()> {
    let values = values.collect::<Vec<_>>();
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
    ensure_distinct_values(values.iter().map(String::as_str), name)?;
    if values.is_empty() {
        bail!("{name} must contain at least one entry");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_MINIMAL: &str = r#"schema_version: executable_profile.v1
profile_id: narrow_eval_v1
profile_version: v1
purpose: Minimal executable-profile fixture for S2 validator proof.
supported_perl_versions:
  - "5.38"
  - "5.40"
supported_constructs:
  - scalar_assignment
  - if_statement
  - while_loop
supported_builtins:
  - name: length
    behavior: Returns the character length of a defined scalar; boundary on undef.
supported_regex_subset: bounded-literals-and-char-classes-only
module_policy: Static modules only; no ambient module loading.
filesystem_policy: No filesystem access during evaluation.
io_policy: No I/O during evaluation.
environment_policy: No environment reads during evaluation.
network_policy: No network access during evaluation.
xs_policy: No XS during evaluation.
source_filter_policy: No source filters during evaluation.
dynamic_boundary_behavior: Stop and report an explicit boundary; never guess.
warnings_behavior: Warnings are collected and reported; none are fatal.
exceptions_behavior: Exceptions unwind to the evaluation boundary and are reported.
tap_behavior: TAP totals are reported from real runs only; never synthesized.
budgets:
  max_steps: 10000
  max_time_ms: 5000
  max_memory_bytes: 16777216
  max_recursion_depth: 64
  max_output_bytes: 1048576
determinism_guarantees: Same inputs and profile version evaluate deterministically.
oracle_agreement_thresholds: Exact agreement with real Perl on the profiled subset.
unsupported_boundary_ids: []
owner_issue: '#4773'
predecessor_profile: null
change_reason: Initial S2 validator fixture.
claim_boundary: Fixture only; admits no evaluation and claims no EIR conformance.
"#;

    #[test]
    fn valid_minimal_profile_loads_and_has_stable_identity() -> Result<()> {
        let profile = ExecutableProfile::from_str(VALID_MINIMAL)?;
        assert_eq!(profile.schema_version, SCHEMA_VERSION);
        assert_eq!(profile.owner_issue, "#4773");
        assert!(!profile.canonical_json()?.contains("\\\\"));
        assert_eq!(
            profile.identity_sha256()?,
            "5afd8007850d3efd4960634994e44c581f32b9f91c6f9b79dd27c541f6017297"
        );
        Ok(())
    }

    #[test]
    fn canonical_identity_ignores_order_but_not_membership() -> Result<()> {
        let reordered = VALID_MINIMAL.replace(
            "  - scalar_assignment\n  - if_statement\n  - while_loop",
            "  - while_loop\n  - scalar_assignment\n  - if_statement",
        );
        let first = ExecutableProfile::from_str(&reordered)?;
        let second = ExecutableProfile::from_str(VALID_MINIMAL)?;
        assert_eq!(first.identity_sha256()?, second.identity_sha256()?);

        let changed = VALID_MINIMAL.replace("- while_loop", "- for_loop");
        let changed = ExecutableProfile::from_str(&changed)?;
        assert_ne!(changed.identity_sha256()?, second.identity_sha256()?);
        Ok(())
    }

    #[test]
    fn empty_subset_declarations_fail_closed() -> Result<()> {
        let empty_constructs = VALID_MINIMAL.replace(
            "supported_constructs:\n  - scalar_assignment\n  - if_statement\n  - while_loop",
            "supported_constructs: []",
        );
        let error = ExecutableProfile::from_str(&empty_constructs)
            .expect_err("a manifest with no declared constructs must fail closed")
            .to_string();
        assert!(
            error.contains("supported_constructs"),
            "empty constructs error should name the field: {error}"
        );

        let empty_perl = VALID_MINIMAL.replace(
            "supported_perl_versions:\n  - \"5.38\"\n  - \"5.40\"",
            "supported_perl_versions: []",
        );
        assert!(
            ExecutableProfile::from_str(&empty_perl).is_err(),
            "a manifest with no declared Perl versions must fail closed"
        );

        let missing_budgets = VALID_MINIMAL.replace(
            "budgets:\n  max_steps: 10000\n  max_time_ms: 5000\n  max_memory_bytes: 16777216\n  max_recursion_depth: 64\n  max_output_bytes: 1048576\n",
            "",
        );
        assert!(
            ExecutableProfile::from_str(&missing_budgets).is_err(),
            "a manifest with missing budgets must fail closed"
        );
        Ok(())
    }

    #[test]
    fn boundary_ids_fail_closed_until_s4() -> Result<()> {
        let with_id = VALID_MINIMAL.replace(
            "unsupported_boundary_ids: []",
            "unsupported_boundary_ids:\n  - outside_profile_eval",
        );
        let error = ExecutableProfile::from_str(&with_id)
            .expect_err("a shape-valid boundary id must still fail closed until S4")
            .to_string();
        assert!(
            error.contains("S4") && error.contains("#4770"),
            "premature boundary wiring error should name the S4 gate: {error}"
        );

        let duplicate = VALID_MINIMAL.replace(
            "unsupported_boundary_ids: []",
            "unsupported_boundary_ids:\n  - same_id\n  - same_id",
        );
        let error = ExecutableProfile::from_str(&duplicate)
            .expect_err("duplicate boundary ids must fail closed")
            .to_string();
        assert!(
            error.contains("unsupported_boundary_ids") && error.contains("duplicate"),
            "duplicate boundary error should name the field: {error}"
        );

        let bad_shape = VALID_MINIMAL
            .replace("unsupported_boundary_ids: []", "unsupported_boundary_ids:\n  - Has-Capitals");
        let error = ExecutableProfile::from_str(&bad_shape)
            .expect_err("a malformed boundary id must fail closed")
            .to_string();
        assert!(
            error.contains("snake_case"),
            "malformed boundary id error should name the shape rule: {error}"
        );
        Ok(())
    }

    #[test]
    fn malformed_identity_and_empty_policy_fail_closed() -> Result<()> {
        let bad_schema = VALID_MINIMAL.replace(SCHEMA_VERSION, "executable_profile.v2");
        assert!(
            ExecutableProfile::from_str(&bad_schema).is_err(),
            "a manifest with an unsupported schema must fail closed"
        );

        let bad_id = VALID_MINIMAL.replace("profile_id: narrow_eval_v1", "profile_id: narrow_eval");
        assert!(
            ExecutableProfile::from_str(&bad_id).is_err(),
            "a manifest with an unversioned profile id must fail closed"
        );

        let bad_version = VALID_MINIMAL.replace("profile_version: v1", "profile_version: 1");
        assert!(
            ExecutableProfile::from_str(&bad_version).is_err(),
            "a manifest with an unversioned profile version must fail closed"
        );

        let spaced_id =
            VALID_MINIMAL.replace("profile_id: narrow_eval_v1", "profile_id: 'narrow eval_v1'");
        let error = ExecutableProfile::from_str(&spaced_id)
            .expect_err("a profile id with whitespace must fail closed")
            .to_string();
        assert!(
            error.contains("profile_id") && error.contains("snake_case"),
            "whitespace identity error should name the shape rule: {error}"
        );

        let spaced_version = VALID_MINIMAL.replace("profile_version: v1", "profile_version: 'v 1'");
        assert!(
            ExecutableProfile::from_str(&spaced_version).is_err(),
            "a profile version with whitespace must fail closed"
        );

        let bare_version = VALID_MINIMAL.replace("profile_version: v1", "profile_version: v");
        assert!(
            ExecutableProfile::from_str(&bare_version).is_err(),
            "a bare 'v' profile version must fail closed"
        );

        let empty_policy = VALID_MINIMAL.replace(
            "environment_policy: No environment reads during evaluation.",
            "environment_policy: \"\"",
        );
        let error = ExecutableProfile::from_str(&empty_policy)
            .expect_err("a manifest with an empty environment policy must fail closed")
            .to_string();
        assert!(
            error.contains("environment_policy"),
            "empty policy error should name the field: {error}"
        );

        let builtins_dup = VALID_MINIMAL.replace(
            "supported_builtins:\n  - name: length\n    behavior: Returns the character length of a defined scalar; boundary on undef.\n",
            "supported_builtins:\n  - name: length\n    behavior: A.\n  - name: length\n    behavior: B.\n",
        );
        assert_ne!(
            builtins_dup, VALID_MINIMAL,
            "the duplicate-builtin replacement must modify the manifest"
        );
        let error = ExecutableProfile::from_str(&builtins_dup)
            .expect_err("a manifest with duplicate builtin names must fail closed")
            .to_string();
        assert!(
            error.contains("supported_builtins") && error.contains("duplicate"),
            "duplicate builtin error should name the field: {error}"
        );

        let empty_name = VALID_MINIMAL.replace("profile_id: narrow_eval_v1", "profile_id: _v1");
        assert!(
            ExecutableProfile::from_str(&empty_name).is_err(),
            "a profile id with an empty name part must fail closed"
        );
        Ok(())
    }

    #[test]
    fn committed_schema_matches_validator_contract() -> Result<()> {
        let schema_source = include_str!("../../../schemas/executable_profile.v1.schema.json");
        let schema: serde_json::Value = serde_json::from_str(schema_source)?;
        assert_eq!(
            schema.get("$schema").and_then(serde_json::Value::as_str),
            Some("https://json-schema.org/draft/2020-12/schema")
        );
        assert_eq!(
            schema
                .get("properties")
                .and_then(|properties| properties.get("schema_version"))
                .and_then(|version| version.get("const"))
                .and_then(serde_json::Value::as_str),
            Some(SCHEMA_VERSION)
        );
        let required = schema
            .get("required")
            .and_then(serde_json::Value::as_array)
            .map(|entries| entries.iter().filter_map(serde_json::Value::as_str).collect::<Vec<_>>())
            .unwrap_or_default();
        for field in [
            "supported_constructs",
            "supported_builtins",
            "budgets",
            "environment_policy",
            "claim_boundary",
        ] {
            assert!(required.contains(&field), "schema must require {field}");
        }
        let pattern = |name: &str| {
            schema
                .get("properties")
                .and_then(|properties| properties.get(name))
                .and_then(|property| property.get("pattern"))
                .and_then(serde_json::Value::as_str)
        };
        assert_eq!(
            pattern("profile_id"),
            Some("^[a-z0-9_]+_v1$"),
            "schema must pin the whitespace-free profile_id shape"
        );
        assert_eq!(
            pattern("profile_version"),
            Some("^v[0-9A-Za-z._-]+$"),
            "schema must pin the whitespace-free profile_version shape"
        );
        assert_eq!(
            schema
                .get("properties")
                .and_then(|properties| properties.get("unsupported_boundary_ids"))
                .and_then(|property| property.get("maxItems"))
                .and_then(serde_json::Value::as_u64),
            Some(0),
            "schema must keep the S2 boundary list empty until S4"
        );
        assert!(
            schema
                .get("properties")
                .and_then(|properties| properties.get("supported_builtins"))
                .and_then(|property| property.get("description"))
                .and_then(serde_json::Value::as_str)
                .is_some_and(|text| text.contains("unique")),
            "schema must document the builtin name-uniqueness custom rule"
        );
        Ok(())
    }

    #[test]
    fn report_output_is_deterministic() -> Result<()> {
        let profile = ExecutableProfile::from_str(VALID_MINIMAL)?;
        let expected = format!(
            "profile: narrow_eval_v1 v1\nmanifest_sha256: {}\nperl_versions: 2\nconstructs: 3\nbuiltins: 1\nregex_subset: bounded-literals-and-char-classes-only\nboundaries: 0\nclaim_boundary: Fixture only; admits no evaluation and claims no EIR conformance.\n",
            profile.identity_sha256()?
        );
        assert_eq!(profile.report_string()?, expected);
        assert_eq!(ExecutableProfile::from_str(VALID_MINIMAL)?.report_string()?, expected);
        Ok(())
    }

    #[test]
    fn omitted_schema_required_lists_fail_closed() -> Result<()> {
        let missing_builtins = VALID_MINIMAL.replace(
            "supported_builtins:\n  - name: length\n    behavior: Returns the character length of a defined scalar; boundary on undef.\n",
            "",
        );
        assert!(
            ExecutableProfile::from_str(&missing_builtins).is_err(),
            "a manifest omitting schema-required supported_builtins must fail closed"
        );

        let missing_boundaries = VALID_MINIMAL.replace("unsupported_boundary_ids: []\n", "");
        assert!(
            ExecutableProfile::from_str(&missing_boundaries).is_err(),
            "a manifest omitting schema-required unsupported_boundary_ids must fail closed"
        );
        Ok(())
    }
}
