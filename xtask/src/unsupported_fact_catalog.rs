//! Typed loader for the unsupported-fact catalog (PLSP-SPEC-0038, #4770 slice 1).
//!
//! Slice 1 is schema + seed TOML + this loader with shape tests. The loader
//! parses `policy/compiler/unsupported-facts.v1.toml` into typed structs,
//! checks the seed-shape contract (exact seed set, ID shape, the
//! `outside_profile` suffix placement, and the oracle `unknown` projection),
//! and canonicalizes the catalog for the determinism proof. Slice 2 adds the
//! `compiler-profile unsupported --check/--report` CLI and rejection
//! fixtures; this module wires no CLI, gate, or consumer behavior.

use color_eyre::eyre::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

/// Repository-relative path of the versioned seed catalog (SPEC-0038 E0).
pub const CATALOG_PATH: &str = "policy/compiler/unsupported-facts.v1.toml";
/// Schema-version const shared by the TOML catalog and the JSON Schema (E0).
pub const SCHEMA_VERSION: &str = "unsupported_fact_catalog.v1";
/// Suffix marking profile-scoped classes (SPEC-0038 I4).
pub const OUTSIDE_PROFILE_SUFFIX: &str = "_outside_profile";

/// Exact SPEC-0038 V1 seed set: no more, no fewer (V1-V2).
pub const SEED_IDS: &[&str] = &[
    "ambient_module_resolution",
    "ambiguous_bareword",
    "autoload",
    "compile_time_execution_outside_profile",
    "dynamic_stash_mutation",
    "generated_no_source",
    "host_capability_outside_profile",
    "regex_construct_outside_profile",
    "runtime_builtin_outside_profile",
    "source_filter",
    "symbolic_ref",
    "unknown_boundary_evidence",
    "unsupported_parser_construct",
    "xs_native_dependency",
];

/// Seed IDs whose limitation is profile membership rather than fundamental
/// modeling (I4). Exactly these IDs carry [`OUTSIDE_PROFILE_SUFFIX`].
pub const OUTSIDE_PROFILE_IDS: &[&str] = &[
    "compile_time_execution_outside_profile",
    "host_capability_outside_profile",
    "regex_construct_outside_profile",
    "runtime_builtin_outside_profile",
];

/// Parsed form of the unsupported-fact catalog (SPEC-0038 E0-E1).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UnsupportedFactCatalog {
    pub schema_version: String,
    pub entries: Vec<UnsupportedFactEntry>,
}

/// One catalog entry (SPEC-0038 E1). Blocking flags are plain booleans, never
/// optional (E2). Table-valued fields sort last: TOML emission requires
/// values before sub-tables.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UnsupportedFactEntry {
    pub class_id: String,
    pub owning_layer: OwningLayer,
    pub trigger_rule: String,
    pub compile_acceptance_blocks: bool,
    pub downstream_blocks: bool,
    pub allowed_provider_behaviors: Vec<ProviderBehavior>,
    pub curated_gold_treatment: String,
    pub oracle_result_class: OracleResultClass,
    pub oracle_promotion_effect: OraclePromotionEffect,
    pub profile_boundary_behavior: String,
    pub owner_issue: String,
    pub distinctions: Distinctions,
    pub history: EntryHistory,
}

/// Compiler layer that recognizes the class (SPEC-0038 E1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OwningLayer {
    Parser,
    Hir,
    Pir,
    Semantic,
    Provider,
    Oracle,
    Eir,
}

/// Closed 4-value provider behavior vocabulary (SPEC-0038 E1). There is
/// deliberately no `exact` variant: a blocking class paired with exact
/// provider behavior fails closed at parse time, which is the slice-1
/// enforcement of E3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderBehavior {
    Block,
    Fallback,
    Advisory,
    RequireConfirmation,
}

/// Oracle result-class vocabulary, shared with `oracle_receipt.v1` (E1/P4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OracleResultClass {
    OracleAgrees,
    CompilerMissing,
    CompilerExtra,
    RangeMismatch,
    ProvenanceMismatch,
    ConfidenceOrFreshnessMismatch,
    DynamicOrUnsupported,
    OracleAmbientUnbounded,
    StaleOrPartial,
    Unknown,
}

/// Oracle promotion-effect vocabulary, shared with `oracle_receipt.v1` (E1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OraclePromotionEffect {
    SupportsPromotion,
    BlocksPromotion,
    KnownLimitation,
    Unknown,
}

/// Explicit separation from neighboring concepts (SPEC-0038 E1).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Distinctions {
    pub not_dynamic_boundary: String,
    pub not_low_confidence: String,
    pub not_stale_evidence: String,
    pub not_fallback: String,
    pub nearest_neighbor: String,
}

/// Introduction version and migration record (SPEC-0038 E1/I1).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntryHistory {
    pub introduced_in: String,
    #[serde(default)]
    pub migrations: Vec<String>,
}

/// Parse catalog TOML text into the typed struct. Unknown fields and
/// out-of-vocabulary enum values fail closed here.
pub fn parse_catalog(text: &str) -> Result<UnsupportedFactCatalog> {
    toml::from_str(text).with_context(|| format!("failed to parse {CATALOG_PATH}"))
}

/// Load the seed catalog from a repository root.
pub fn load_catalog_file(root: &Path) -> Result<UnsupportedFactCatalog> {
    let path = root.join(CATALOG_PATH);
    let text =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    parse_catalog(&text)
}

/// Check the slice-1 shape contract, collecting one violation per defect.
/// An empty return means the catalog satisfies the contract.
pub fn validate_catalog(catalog: &UnsupportedFactCatalog) -> Vec<String> {
    let mut violations = Vec::new();
    validate_schema_version(catalog, &mut violations);
    validate_seed_ids(catalog, &mut violations);
    validate_id_shapes(catalog, &mut violations);
    validate_outside_profile_suffix(catalog, &mut violations);
    validate_oracle_unknown_projection(catalog, &mut violations);
    violations
}

fn validate_schema_version(catalog: &UnsupportedFactCatalog, violations: &mut Vec<String>) {
    if catalog.schema_version != SCHEMA_VERSION {
        violations.push(format!(
            "schema_version mismatch: expected '{SCHEMA_VERSION}', found '{}'",
            catalog.schema_version
        ));
    }
}

fn validate_seed_ids(catalog: &UnsupportedFactCatalog, violations: &mut Vec<String>) {
    let expected: BTreeSet<&str> = SEED_IDS.iter().copied().collect();
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for entry in &catalog.entries {
        if !seen.insert(entry.class_id.as_str()) {
            violations.push(format!("duplicate class_id '{}'", entry.class_id));
        }
    }
    for missing in expected.difference(&seen) {
        violations.push(format!("missing seed class_id '{missing}'"));
    }
    for unexpected in seen.difference(&expected) {
        violations.push(format!("unknown class_id '{unexpected}'"));
    }
}

fn validate_id_shapes(catalog: &UnsupportedFactCatalog, violations: &mut Vec<String>) {
    for entry in &catalog.entries {
        let well_formed = !entry.class_id.is_empty()
            && entry
                .class_id
                .chars()
                .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_');
        if !well_formed {
            violations.push(format!(
                "class_id shape violation: '{}' must match [a-z0-9_]+ (I2)",
                entry.class_id
            ));
        }
    }
}

fn validate_outside_profile_suffix(catalog: &UnsupportedFactCatalog, violations: &mut Vec<String>) {
    let expected: BTreeSet<&str> = OUTSIDE_PROFILE_IDS.iter().copied().collect();
    let suffixed: BTreeSet<&str> = catalog
        .entries
        .iter()
        .map(|entry| entry.class_id.as_str())
        .filter(|id| id.ends_with(OUTSIDE_PROFILE_SUFFIX))
        .collect();
    for missing in expected.difference(&suffixed) {
        violations.push(format!(
            "outside_profile suffix missing: '{missing}' must carry the '{OUTSIDE_PROFILE_SUFFIX}' suffix (I4)"
        ));
    }
    for misplaced in suffixed.difference(&expected) {
        violations.push(format!(
            "outside_profile suffix misplaced: '{misplaced}' carries the suffix but is not a profile-scoped class (I4)"
        ));
    }
}

fn validate_oracle_unknown_projection(
    catalog: &UnsupportedFactCatalog,
    violations: &mut Vec<String>,
) {
    for entry in &catalog.entries {
        if entry.oracle_result_class == OracleResultClass::Unknown
            && entry.oracle_promotion_effect != OraclePromotionEffect::BlocksPromotion
        {
            violations.push(format!(
                "class '{}': oracle result_class 'unknown' must project to 'blocks_promotion', found '{:?}' (E1/P4)",
                entry.class_id, entry.oracle_promotion_effect
            ));
        }
    }
}

/// Serialize the catalog in canonical form: entries sorted by `class_id`,
/// struct field order, no host paths or timestamps. The seed file is stored
/// in exactly this form, so canonicalization round-trips byte-identical.
pub fn canonical_toml(catalog: &UnsupportedFactCatalog) -> Result<String> {
    let mut sorted = catalog.clone();
    sorted.entries.sort_by(|left, right| left.class_id.cmp(&right.class_id));
    toml::to_string(&sorted).with_context(|| "failed to serialize catalog to canonical TOML")
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult<T = ()> = Result<T>;

    fn seed_text() -> Result<String> {
        let root = crate::utils::project_root()?;
        let path = root.join(CATALOG_PATH);
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))
    }

    fn parse_seed() -> Result<UnsupportedFactCatalog> {
        parse_catalog(&seed_text()?)
    }

    /// Unwrap a parse failure into its message, failing the test on
    /// unexpected success.
    fn parse_failure_text(result: Result<UnsupportedFactCatalog>) -> TestResult<String> {
        match result {
            Ok(_) => {
                Err(color_eyre::eyre::eyre!("expected catalog parse to fail, but it succeeded"))
            }
            Err(err) => Ok(format!("{err:?}")),
        }
    }

    fn find_entry<'a>(
        catalog: &'a UnsupportedFactCatalog,
        class_id: &str,
    ) -> Result<&'a UnsupportedFactEntry> {
        catalog
            .entries
            .iter()
            .find(|entry| entry.class_id == class_id)
            .ok_or_else(|| color_eyre::eyre::eyre!("seed catalog is missing '{class_id}'"))
    }

    fn find_entry_mut<'a>(
        catalog: &'a mut UnsupportedFactCatalog,
        class_id: &str,
    ) -> Result<&'a mut UnsupportedFactEntry> {
        catalog
            .entries
            .iter_mut()
            .find(|entry| entry.class_id == class_id)
            .ok_or_else(|| color_eyre::eyre::eyre!("seed catalog is missing '{class_id}'"))
    }

    /// Round-trip a mutated catalog through canonical TOML so falsifiers
    /// exercise the real text pipeline, then validate.
    fn validate_round_tripped(catalog: &UnsupportedFactCatalog) -> Result<Vec<String>> {
        let text = canonical_toml(catalog)?;
        let reparsed = parse_catalog(&text)?;
        Ok(validate_catalog(&reparsed))
    }

    #[test]
    fn loads_seed_catalog_from_repo_file() -> TestResult {
        // The file entry point is slice 2's seam; pin it before any CLI
        // wires it so a path/read regression cannot hide.
        let root = crate::utils::project_root()?;
        let catalog = load_catalog_file(&root)?;
        assert_eq!(catalog.entries.len(), SEED_IDS.len());
        assert_eq!(catalog.schema_version, SCHEMA_VERSION);
        Ok(())
    }

    #[test]
    fn loads_seed_catalog_with_exact_shape() -> TestResult {
        let text = seed_text()?;
        let catalog = parse_catalog(&text)?;
        let violations = validate_catalog(&catalog);
        assert!(violations.is_empty(), "seed catalog violations: {violations:?}");
        assert_eq!(catalog.entries.len(), SEED_IDS.len());
        let ids: Vec<&str> = catalog.entries.iter().map(|entry| entry.class_id.as_str()).collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        assert_eq!(ids, sorted, "seed entries must be stored in canonical order");
        let unknown = find_entry(&catalog, "unknown_boundary_evidence")?;
        assert_eq!(unknown.oracle_result_class, OracleResultClass::Unknown);
        assert_eq!(unknown.oracle_promotion_effect, OraclePromotionEffect::BlocksPromotion);
        // SPEC-0038 V1 assigns each seed ID its recognizing layer, and the
        // seed fixes each class's compile-acceptance effect; a swap is
        // silent wrong data, so the seed pins every triple.
        let expected_layers: [(&str, OwningLayer, bool); 14] = [
            ("ambient_module_resolution", OwningLayer::Hir, false),
            ("ambiguous_bareword", OwningLayer::Parser, false),
            ("autoload", OwningLayer::Hir, false),
            ("compile_time_execution_outside_profile", OwningLayer::Semantic, false),
            ("dynamic_stash_mutation", OwningLayer::Hir, false),
            ("generated_no_source", OwningLayer::Semantic, false),
            ("host_capability_outside_profile", OwningLayer::Semantic, false),
            ("regex_construct_outside_profile", OwningLayer::Semantic, false),
            ("runtime_builtin_outside_profile", OwningLayer::Semantic, false),
            ("source_filter", OwningLayer::Parser, true),
            ("symbolic_ref", OwningLayer::Hir, false),
            ("unknown_boundary_evidence", OwningLayer::Semantic, true),
            ("unsupported_parser_construct", OwningLayer::Parser, true),
            ("xs_native_dependency", OwningLayer::Semantic, false),
        ];
        for (class_id, layer, compile_blocks) in expected_layers {
            let entry = find_entry(&catalog, class_id)?;
            assert_eq!(
                entry.owning_layer, layer,
                "seed layer for '{class_id}' must match SPEC-0038 V1"
            );
            assert_eq!(
                entry.compile_acceptance_blocks, compile_blocks,
                "seed compile flag for '{class_id}' must not drift silently"
            );
        }
        // Every neighbor reference must resolve to a different seed class;
        // a typo would otherwise leave a broken distinction.
        for entry in &catalog.entries {
            assert!(
                entry.distinctions.nearest_neighbor != entry.class_id,
                "seed class '{}' must not neighbor itself",
                entry.class_id
            );
            find_entry(&catalog, &entry.distinctions.nearest_neighbor)?;
        }
        let source_filter = find_entry(&catalog, "source_filter")?;
        assert!(source_filter.compile_acceptance_blocks);
        for entry in &catalog.entries {
            assert!(
                entry.downstream_blocks,
                "seed class '{}' must block or qualify downstream facts",
                entry.class_id
            );
            assert!(
                !entry.allowed_provider_behaviors.is_empty(),
                "seed class '{}' must allow at least one provider behavior",
                entry.class_id
            );
        }
        Ok(())
    }

    #[test]
    fn rejects_dropped_class() -> TestResult {
        let mut catalog = parse_seed()?;
        catalog
            .entries
            .pop()
            .ok_or_else(|| color_eyre::eyre::eyre!("seed catalog has no entries"))?;
        let violations = validate_round_tripped(&catalog)?;
        assert!(
            violations.iter().any(|violation| violation.contains("missing seed class_id")),
            "dropped class must fail: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_renamed_id() -> TestResult {
        let mut catalog = parse_seed()?;
        find_entry_mut(&mut catalog, "ambiguous_bareword")?.class_id = "renamed_class".to_string();
        let violations = validate_round_tripped(&catalog)?;
        assert!(
            violations
                .iter()
                .any(|violation| violation.contains("unknown class_id 'renamed_class'")),
            "renamed ID must fail: {violations:?}"
        );
        assert!(
            violations
                .iter()
                .any(|violation| violation.contains("missing seed class_id 'ambiguous_bareword'")),
            "renamed ID must report the missing seed: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_missing_downstream_blocks() -> TestResult {
        let text = seed_text()?;
        let (head, tail) = text
            .split_once("downstream_blocks = true\n")
            .ok_or_else(|| color_eyre::eyre::eyre!("seed text has no downstream_blocks line"))?;
        let broken = format!("{head}{tail}");
        let message = parse_failure_text(parse_catalog(&broken))?;
        assert!(
            message.contains("downstream_blocks"),
            "missing boolean must be named in the error: {message}"
        );
        Ok(())
    }

    #[test]
    fn rejects_exact_provider_behavior_on_blocking_class() -> TestResult {
        let text = seed_text()?;
        let broken = text.replacen(
            "allowed_provider_behaviors = [\"fallback\"",
            "allowed_provider_behaviors = [\"exact\"",
            1,
        );
        let message = parse_failure_text(parse_catalog(&broken))?;
        assert!(
            message.contains("exact"),
            "exact behavior must be rejected as out of vocabulary: {message}"
        );
        Ok(())
    }

    #[test]
    fn rejects_profile_class_missing_suffix() -> TestResult {
        let mut catalog = parse_seed()?;
        find_entry_mut(&mut catalog, "regex_construct_outside_profile")?.class_id =
            "regex_construct".to_string();
        let violations = validate_round_tripped(&catalog)?;
        assert!(
            violations.iter().any(|violation| violation.contains("outside_profile suffix missing")
                && violation.contains("regex_construct_outside_profile")),
            "suffix-less profile class must fail the suffix rule: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_suffix_on_non_profile_class() -> TestResult {
        let mut catalog = parse_seed()?;
        find_entry_mut(&mut catalog, "source_filter")?.class_id =
            "source_filter_outside_profile".to_string();
        let violations = validate_round_tripped(&catalog)?;
        assert!(
            violations
                .iter()
                .any(|violation| violation.contains("outside_profile suffix misplaced")
                    && violation.contains("source_filter_outside_profile")),
            "misplaced suffix must fail the suffix rule: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_duplicate_class_id() -> TestResult {
        let mut catalog = parse_seed()?;
        let first = catalog
            .entries
            .first()
            .ok_or_else(|| color_eyre::eyre::eyre!("seed catalog has no entries"))?
            .clone();
        let class_id = first.class_id.clone();
        catalog.entries.push(first);
        let violations = validate_round_tripped(&catalog)?;
        assert!(
            violations
                .iter()
                .any(|violation| violation.contains(&format!("duplicate class_id '{class_id}'"))),
            "duplicate ID must fail: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_bad_id_shape() -> TestResult {
        let mut catalog = parse_seed()?;
        find_entry_mut(&mut catalog, "ambiguous_bareword")?.class_id =
            "Ambiguous Bareword!".to_string();
        let violations = validate_round_tripped(&catalog)?;
        assert!(
            violations.iter().any(|violation| violation.contains("must match [a-z0-9_]+")),
            "non-snake ID must fail the shape rule: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_unknown_oracle_result_agreeing() -> TestResult {
        let mut catalog = parse_seed()?;
        find_entry_mut(&mut catalog, "unknown_boundary_evidence")?.oracle_promotion_effect =
            OraclePromotionEffect::SupportsPromotion;
        let violations = validate_round_tripped(&catalog)?;
        assert!(
            violations.iter().any(|violation| violation.contains("blocks_promotion")),
            "unknown result must project to blocks_promotion: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_wrong_schema_version() -> TestResult {
        let text = seed_text()?;
        let broken = text.replacen(
            "schema_version = \"unsupported_fact_catalog.v1\"",
            "schema_version = \"unsupported_fact_catalog.v0\"",
            1,
        );
        let catalog = parse_catalog(&broken)?;
        let violations = validate_catalog(&catalog);
        assert!(
            violations.iter().any(|violation| violation.contains("schema_version mismatch")),
            "wrong schema version must fail: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn unknown_fields_denied() -> TestResult {
        let text = seed_text()?;
        let top_level = text.replacen('\n', "\nextra_top_level = true\n", 1);
        let message = parse_failure_text(parse_catalog(&top_level))?;
        assert!(
            message.contains("unknown field"),
            "top-level unknown field must be denied: {message}"
        );
        let in_entry = text.replacen("[[entries]]\n", "[[entries]]\nextra_entry_field = true\n", 1);
        let message = parse_failure_text(parse_catalog(&in_entry))?;
        assert!(
            message.contains("unknown field"),
            "entry-level unknown field must be denied: {message}"
        );
        let in_distinctions = text.replacen(
            "[entries.distinctions]\n",
            "[entries.distinctions]\nextra_distinction = true\n",
            1,
        );
        let message = parse_failure_text(parse_catalog(&in_distinctions))?;
        assert!(
            message.contains("unknown field"),
            "distinctions-level unknown field must be denied: {message}"
        );
        let in_history =
            text.replacen("[entries.history]\n", "[entries.history]\nextra_history = true\n", 1);
        let message = parse_failure_text(parse_catalog(&in_history))?;
        assert!(
            message.contains("unknown field"),
            "history-level unknown field must be denied: {message}"
        );
        Ok(())
    }

    #[test]
    fn determinism_round_trip_byte_identical() -> TestResult {
        let file_bytes = seed_text()?;
        let catalog = parse_catalog(&file_bytes)?;
        let once = canonical_toml(&catalog)?;
        assert_eq!(once.len(), file_bytes.len(), "seed file must be stored in canonical form");
        assert_eq!(once, file_bytes, "seed file must be stored in canonical form");
        let twice = canonical_toml(&parse_catalog(&once)?)?;
        assert_eq!(twice, once, "canonical serialization must round-trip byte-identical");
        Ok(())
    }

    #[test]
    fn canonical_toml_sorts_entries() -> TestResult {
        let file_bytes = seed_text()?;
        let mut catalog = parse_catalog(&file_bytes)?;
        catalog.entries.reverse();
        let canonical = canonical_toml(&catalog)?;
        assert_eq!(canonical, file_bytes, "canonical form must not depend on input entry order");
        Ok(())
    }
}
