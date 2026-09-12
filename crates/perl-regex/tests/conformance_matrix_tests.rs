//! Versioned regex-analysis conformance matrix tests.
//!
//! These tests load fixture files from `tests/fixtures/conformance/`,
//! validate their schema, and verify that every row's `expected` facts
//! match actual [`perl_regex::RegexAnalyzer::analyze_modifiers`] output.
//!
//! # Load-bearing guarantees
//!
//! - Schema version in every fixture equals [`perl_regex::conformance::SCHEMA_VERSION`].
//! - No fixture file contains duplicate `id` values.
//! - Every `id` starts with its declared `family`.
//! - Specific deterministic assertion tests named after each concept ID give
//!   crisp failure messages if the analyzer diverges from the fixture.
//! - Mutation guards verify that altering an expected fact actually breaks the test.
//!
//! # Covered fixture families (the modifier slice of issue #7036)
//!
//! - `modifiers.extended` — `/x`, `/xx`, profile-qualified `enhanced_xx`
//! - `modifiers.charset`  — `/a`, `/aa`, `/d`, `/l`, `/u`, conflict diagnostics
//! - `modifiers.capture`  — `/n` and non-capturing defaults
//! - `modifiers.substitution` — `/e`, `/ee`, `/r`, operator legality
//! - `modifiers.misc`     — `/g`, `/c`, `/i`, `/m`, `/s`, illegality cases
// Test assertions and fixture helpers use unwrap/panic/expect; the workspace-wide
// denies are production-code rules and do not apply to test drivers.
#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]
#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598)

use perl_regex::{
    RegexAnalyzer,
    analyzer::{
        CaptureMode, CharacterSetMode, ExtendedMode, FeatureState, ModifierSequence, PerlVersion,
        RegexLanguageProfile, RegexOperator,
    },
    conformance::SCHEMA_VERSION,
    validator::RegexDiagnosticCode,
};
use serde::Deserialize;

// ---------------------------------------------------------------------------
// Local fixture schema types (serde-deserializable wrappers for the JSON
// fixture format; these are test-only and NOT part of the public API)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureProfile {
    perl_minor: Option<u16>,
    enhanced_xx: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpectedFacts {
    #[serde(default)]
    extended_mode: Option<String>,
    #[serde(default)]
    character_set_mode: Option<String>,
    #[serde(default)]
    capture_mode: Option<String>,
    #[serde(default)]
    case_insensitive: Option<bool>,
    #[serde(default)]
    multiline: Option<bool>,
    #[serde(default)]
    single_line: Option<bool>,
    #[serde(default)]
    global: Option<bool>,
    #[serde(default)]
    non_destructive: Option<bool>,
    #[serde(default)]
    substitution_evaluation_depth: Option<usize>,
    diagnostic_codes: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureRow {
    id: String,
    family: String,
    authority: String,
    operator: String,
    profile: FixtureProfile,
    modifier_sequence: String,
    positive_source: Option<String>,
    negative_source: Option<String>,
    expected: ExpectedFacts,
    completeness: String,
    owner_issue: u32,
    oracle_disposition: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureFile {
    schema_version: u32,
    programme: String,
    family: String,
    owner_issue: u32,
    rows: Vec<FixtureRow>,
}

// ---------------------------------------------------------------------------
// Fixture loading helpers
// ---------------------------------------------------------------------------

const FIXTURE_EXTENDED: &str = include_str!("fixtures/conformance/modifiers_extended.json");
const FIXTURE_CHARSET: &str = include_str!("fixtures/conformance/modifiers_charset.json");
const FIXTURE_CAPTURE: &str = include_str!("fixtures/conformance/modifiers_capture.json");
const FIXTURE_SUBSTITUTION: &str = include_str!("fixtures/conformance/modifiers_substitution.json");
const FIXTURE_MISC: &str = include_str!("fixtures/conformance/modifiers_misc.json");

fn parse_fixture(json: &str, label: &str) -> FixtureFile {
    serde_json::from_str(json)
        .unwrap_or_else(|err| panic!("fixture {label} failed to parse: {err}"))
}

fn all_fixtures() -> Vec<(String, FixtureFile)> {
    vec![
        ("modifiers_extended".to_owned(), parse_fixture(FIXTURE_EXTENDED, "modifiers_extended")),
        ("modifiers_charset".to_owned(), parse_fixture(FIXTURE_CHARSET, "modifiers_charset")),
        ("modifiers_capture".to_owned(), parse_fixture(FIXTURE_CAPTURE, "modifiers_capture")),
        (
            "modifiers_substitution".to_owned(),
            parse_fixture(FIXTURE_SUBSTITUTION, "modifiers_substitution"),
        ),
        ("modifiers_misc".to_owned(), parse_fixture(FIXTURE_MISC, "modifiers_misc")),
    ]
}

// ---------------------------------------------------------------------------
// Mapping helpers: JSON string → analyzer types
// ---------------------------------------------------------------------------

fn resolve_operator(s: &str) -> RegexOperator {
    match s {
        "bare_match" => RegexOperator::BareMatch,
        "match" => RegexOperator::Match,
        "quote_regex" => RegexOperator::QuoteRegex,
        "substitution" => RegexOperator::Substitution,
        "transliteration" => RegexOperator::Transliteration,
        "transliteration_alias" => RegexOperator::TransliterationAlias,
        other => panic!("unknown operator in fixture: {other:?}"),
    }
}

fn resolve_feature_state(s: &str) -> FeatureState {
    match s {
        "enabled" => FeatureState::Enabled,
        "disabled" => FeatureState::Disabled,
        "unknown" => FeatureState::Unknown,
        other => panic!("unknown enhanced_xx value in fixture: {other:?}"),
    }
}

fn build_profile(fp: &FixtureProfile) -> RegexLanguageProfile {
    let version = fp.perl_minor.map(|minor| PerlVersion::new(5, minor));
    RegexLanguageProfile::new(version, resolve_feature_state(&fp.enhanced_xx))
}

// ---------------------------------------------------------------------------
// Schema validation tests
// ---------------------------------------------------------------------------

/// Every fixture file must declare `schema_version` equal to
/// [`SCHEMA_VERSION`] so stale fixtures are caught at compile time.
#[test]
fn all_fixtures_declare_current_schema_version() {
    for (label, fixture) in all_fixtures() {
        assert_eq!(
            fixture.schema_version, SCHEMA_VERSION,
            "fixture {label}: schema_version {} != expected {SCHEMA_VERSION}",
            fixture.schema_version
        );
    }
}

/// All `id` values across all fixtures must be unique (globally, not just
/// within one file) so concept identity survives cross-fixture composition.
#[test]
fn all_fixture_ids_are_globally_unique() {
    use std::collections::HashMap;
    let mut seen: HashMap<String, String> = HashMap::new();
    for (file, fixture) in all_fixtures() {
        for row in &fixture.rows {
            if let Some(prior_file) = seen.insert(row.id.clone(), file.clone()) {
                panic!(
                    "duplicate conformance id {:?} in {file} (first seen in {prior_file})",
                    row.id
                );
            }
        }
    }
}

/// Every `id` must start with its declared `family` so the namespace is
/// legible without loading the full fixture graph.
#[test]
fn all_row_ids_start_with_their_family() {
    for (file, fixture) in all_fixtures() {
        for row in &fixture.rows {
            assert!(
                row.id.starts_with(&row.family),
                "fixture {file}: row id {:?} does not start with family {:?}",
                row.id,
                row.family
            );
        }
    }
}

/// Each fixture file declares a top-level `family` and every row inside
/// must belong to the same family.
#[test]
fn rows_belong_to_declared_file_family() {
    for (file, fixture) in all_fixtures() {
        for row in &fixture.rows {
            assert_eq!(
                row.family, fixture.family,
                "fixture {file}: row {:?} family {:?} != file family {:?}",
                row.id, row.family, fixture.family
            );
        }
    }
}

#[test]
fn fixture_schema_is_load_bearing() {
    for (file, fixture) in all_fixtures() {
        assert_eq!(fixture.programme, "perl_regex_modifier_conformance", "fixture {file}");
        assert_eq!(fixture.owner_issue, 7036, "fixture {file}");
        assert!(!fixture.rows.is_empty(), "fixture {file} must contain rows");
        for row in &fixture.rows {
            assert!(!row.authority.trim().is_empty(), "fixture {file}/{} has no authority", row.id);
            assert_eq!(row.owner_issue, 7036, "fixture {file}/{}", row.id);
            assert_eq!(row.completeness, "proven", "fixture {file}/{}", row.id);
            assert_eq!(row.oracle_disposition, "not_applicable", "fixture {file}/{}", row.id);
            assert!(
                row.positive_source.is_none(),
                "fixture {file}/{} unexpectedly claims positive oracle source",
                row.id
            );
            assert!(
                row.negative_source.is_none(),
                "fixture {file}/{} unexpectedly claims negative oracle source",
                row.id
            );
            assert!(
                row.expected.extended_mode.is_some()
                    || row.expected.character_set_mode.is_some()
                    || row.expected.capture_mode.is_some()
                    || row.expected.case_insensitive.is_some()
                    || row.expected.multiline.is_some()
                    || row.expected.single_line.is_some()
                    || row.expected.global.is_some()
                    || row.expected.non_destructive.is_some()
                    || row.expected.substitution_evaluation_depth.is_some()
                    || row.expected.diagnostic_codes.is_some(),
                "fixture {file}/{} has no expected fact",
                row.id
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Per-row assertion helper
// ---------------------------------------------------------------------------

/// Run `analyze_modifiers` for `row` and assert every stated expected fact.
fn assert_row_facts(row: &FixtureRow, file: &str) {
    let operator = resolve_operator(&row.operator);
    let profile = build_profile(&row.profile);
    let sequence = ModifierSequence::new(&row.modifier_sequence, 0)
        .unwrap_or_else(|| panic!("fixture {file}/{}: modifier_sequence offset overflow", row.id));

    let analysis = RegexAnalyzer::analyze_modifiers(operator, sequence, profile);
    let exp = &row.expected;
    let ctx = format!("fixture {file}/{}", row.id);

    // --- Extended mode ---
    if let Some(ref expected_ext) = exp.extended_mode {
        let actual = analysis.effective.extended.as_str();
        assert_eq!(
            actual,
            expected_ext.as_str(),
            "{ctx}: extended_mode expected {expected_ext:?}, got {actual:?}"
        );
    }

    // --- Character-set mode ---
    if let Some(ref expected_cs) = exp.character_set_mode {
        let actual = analysis.effective.character_set.as_str();
        assert_eq!(
            actual,
            expected_cs.as_str(),
            "{ctx}: character_set_mode expected {expected_cs:?}, got {actual:?}"
        );
    }

    // --- Capture mode ---
    if let Some(ref expected_cap) = exp.capture_mode {
        let actual = analysis.effective.captures.as_str();
        assert_eq!(
            actual,
            expected_cap.as_str(),
            "{ctx}: capture_mode expected {expected_cap:?}, got {actual:?}"
        );
    }

    // --- Boolean effective flags ---
    if let Some(expected_ci) = exp.case_insensitive {
        assert_eq!(
            analysis.effective.case_insensitive, expected_ci,
            "{ctx}: case_insensitive expected {expected_ci}"
        );
    }
    if let Some(expected_ml) = exp.multiline {
        assert_eq!(
            analysis.effective.multiline, expected_ml,
            "{ctx}: multiline expected {expected_ml}"
        );
    }
    if let Some(expected_sl) = exp.single_line {
        assert_eq!(
            analysis.effective.single_line, expected_sl,
            "{ctx}: single_line expected {expected_sl}"
        );
    }
    if let Some(expected_g) = exp.global {
        assert_eq!(analysis.effective.global, expected_g, "{ctx}: global expected {expected_g}");
    }
    if let Some(expected_r) = exp.non_destructive {
        assert_eq!(
            analysis.effective.non_destructive, expected_r,
            "{ctx}: non_destructive expected {expected_r}"
        );
    }
    if let Some(expected_depth) = exp.substitution_evaluation_depth {
        assert_eq!(
            analysis.effective.substitution_evaluation_depth, expected_depth,
            "{ctx}: substitution_evaluation_depth expected {expected_depth}"
        );
    }

    // --- Diagnostic codes (exact multiset match) ---
    // Collect actual codes as a sorted list of strings.
    let mut actual_codes: Vec<&str> =
        analysis.diagnostics.iter().map(|d| d.code.as_str()).collect();
    actual_codes.sort_unstable();
    let mut expected_sorted: Vec<&str> = exp
        .diagnostic_codes
        .as_ref()
        .unwrap_or_else(|| panic!("{ctx}: diagnostic_codes field is required"))
        .iter()
        .map(String::as_str)
        .collect();
    expected_sorted.sort_unstable();
    assert_eq!(
        actual_codes, expected_sorted,
        "{ctx}: diagnostic_codes mismatch\n  expected: {expected_sorted:?}\n  actual:   {actual_codes:?}"
    );
}

// ---------------------------------------------------------------------------
// Bulk assertion: every row in every fixture
// ---------------------------------------------------------------------------

/// Run `assert_row_facts` for every row in every fixture file.
///
/// This is the primary load-bearing conformance gate: if the analyzer changes
/// the semantics of any covered modifier concept, this test fails.
#[test]
fn all_conformance_rows_match_analyzer_output() {
    for (file, fixture) in all_fixtures() {
        for row in &fixture.rows {
            assert_row_facts(row, &file);
        }
    }
}

// ---------------------------------------------------------------------------
// Focused per-concept tests (deterministic, individually named)
// ---------------------------------------------------------------------------
//
// These give sharper failure messages than the bulk loop above: each one
// names the exact concept ID, expected value, and output field.

#[test]
fn concept_x_off_produces_extended_mode_off() {
    let f = parse_fixture(FIXTURE_EXTENDED, "modifiers_extended");
    let row = f.rows.iter().find(|r| r.id == "modifiers.extended.x-off").unwrap();
    assert_row_facts(row, "modifiers_extended");
    assert_eq!(
        run_modifier_analysis("", 26, false, RegexOperator::Match).effective.extended,
        ExtendedMode::Off
    );
}

#[test]
fn concept_x_basic_produces_extended() {
    let result = run_modifier_analysis("x", 26, false, RegexOperator::Match);
    assert_eq!(result.effective.extended, ExtendedMode::Extended);
    assert!(result.diagnostics.is_empty());
}

#[test]
fn concept_xx_produces_extra_extended_on_526_and_above() {
    let result = run_modifier_analysis("xx", 26, false, RegexOperator::Match);
    assert!(matches!(result.effective.extended, ExtendedMode::ExtraExtended { .. }));
    assert!(result.diagnostics.is_empty());
}

#[test]
fn concept_xx_collapses_to_extended_below_526() {
    let result = run_modifier_analysis("xx", 24, false, RegexOperator::Match);
    assert_eq!(result.effective.extended, ExtendedMode::Extended);
    assert_eq!(
        result
            .diagnostics
            .iter()
            .filter(|d| d.code == RegexDiagnosticCode::ModifierRequiresPerlVersion)
            .count(),
        1
    );
}

#[test]
fn concept_xx_with_unknown_version_stays_unresolved() {
    let sequence = ModifierSequence::new("xx", 0).unwrap();
    let result = RegexAnalyzer::analyze_modifiers(
        RegexOperator::Match,
        sequence,
        RegexLanguageProfile::new(None, FeatureState::Unknown),
    );
    assert_eq!(result.effective.extended.as_str(), "extra_extended_unknown");
    assert!(result.diagnostics.is_empty());
    assert!(result.requirements.iter().any(|requirement| {
        matches!(requirement.disposition, perl_regex::analyzer::RequirementDisposition::Unknown)
    }));
}

#[test]
fn concept_n_sets_non_capturing_by_default() {
    let result = run_modifier_analysis("n", 26, false, RegexOperator::Match);
    assert_eq!(result.effective.captures, CaptureMode::NonCapturingByDefault);
    assert!(result.diagnostics.is_empty());
}

#[test]
fn concept_a_sets_ascii_mode() {
    let result = run_modifier_analysis("a", 26, false, RegexOperator::Match);
    assert_eq!(result.effective.character_set, CharacterSetMode::Ascii);
    assert!(result.diagnostics.is_empty());
}

#[test]
fn concept_aa_sets_ascii_restricted_mode() {
    let result = run_modifier_analysis("aa", 26, false, RegexOperator::Match);
    assert_eq!(result.effective.character_set, CharacterSetMode::AsciiRestricted);
    assert!(result.diagnostics.is_empty());
}

#[test]
fn concept_conflicting_charset_modifiers_emit_diagnostic() {
    let result = run_modifier_analysis("al", 26, false, RegexOperator::Match);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == RegexDiagnosticCode::ConflictingCharacterSetModifiers)
    );
}

#[test]
fn concept_e_sets_eval_depth_1() {
    let result = run_modifier_analysis("e", 26, false, RegexOperator::Substitution);
    assert_eq!(result.effective.substitution_evaluation_depth, 1);
    assert!(result.diagnostics.is_empty());
}

#[test]
fn concept_ee_sets_eval_depth_2() {
    let result = run_modifier_analysis("ee", 26, false, RegexOperator::Substitution);
    assert_eq!(result.effective.substitution_evaluation_depth, 2);
    assert!(result.diagnostics.is_empty());
}

#[test]
fn concept_e_rejected_for_match_operator() {
    let result = run_modifier_analysis("e", 26, false, RegexOperator::Match);
    assert_eq!(result.effective.substitution_evaluation_depth, 0);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == RegexDiagnosticCode::ModifierNotAllowedForOperator)
    );
}

#[test]
fn concept_r_sets_non_destructive_for_substitution() {
    let result = run_modifier_analysis("r", 26, false, RegexOperator::Substitution);
    assert!(result.effective.non_destructive);
    assert!(result.diagnostics.is_empty());
}

#[test]
fn concept_r_rejected_for_match_operator() {
    let result = run_modifier_analysis("r", 26, false, RegexOperator::Match);
    assert!(!result.effective.non_destructive);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == RegexDiagnosticCode::ModifierNotAllowedForOperator)
    );
}

#[test]
fn concept_c_without_g_emits_no_effect_diagnostic() {
    let result = run_modifier_analysis("c", 26, false, RegexOperator::Match);
    assert!(!result.effective.keep_match_position);
    assert!(result.diagnostics.iter().any(|d| d.code == RegexDiagnosticCode::ModifierHasNoEffect));
}

#[test]
fn concept_gc_sets_keep_match_position() {
    let result = run_modifier_analysis("gc", 26, false, RegexOperator::Match);
    assert!(result.effective.global);
    assert!(result.effective.keep_match_position);
    assert!(!result.diagnostics.iter().any(|d| d.code == RegexDiagnosticCode::ModifierHasNoEffect));
}

#[test]
fn concept_unknown_modifier_emits_diagnostic() {
    let result = run_modifier_analysis("z", 26, false, RegexOperator::Match);
    assert!(result.diagnostics.iter().any(|d| d.code == RegexDiagnosticCode::UnknownModifier));
}

// ---------------------------------------------------------------------------
// Mutation guards
//
// These verify that the conformance assertions are genuinely discriminating:
// changing an expected value must break the test (caught here rather than
// leaking into production).
// ---------------------------------------------------------------------------

/// Mutation guard: asserting the WRONG extended mode fails.
#[test]
#[should_panic(expected = "extended_mode")]
fn mutation_wrong_extended_mode_detected() {
    // Force an incorrect expected value: "x" produces `Extended`, not `Off`.
    let analysis = run_modifier_analysis("x", 26, false, RegexOperator::Match);
    // Deliberately assert the wrong value to prove the guard works.
    assert_eq!(
        analysis.effective.extended.as_str(),
        "off",
        "extended_mode expected \"off\", got {:?}",
        analysis.effective.extended.as_str()
    );
}

/// Mutation guard: asserting the WRONG diagnostic set fails.
#[test]
#[should_panic(expected = "diagnostic_codes mismatch")]
fn mutation_missing_diagnostic_detected() {
    // "xx" on Perl 5.24 emits `ModifierRequiresPerlVersion`; asserting
    // empty codes must fail.
    let row = FixtureRow {
        id: "mutation-test".to_owned(),
        family: "mutation".to_owned(),
        operator: "match".to_owned(),
        profile: FixtureProfile { perl_minor: Some(24), enhanced_xx: "disabled".to_owned() },
        modifier_sequence: "xx".to_owned(),
        expected: ExpectedFacts {
            extended_mode: None,
            character_set_mode: None,
            capture_mode: None,
            case_insensitive: None,
            multiline: None,
            single_line: None,
            global: None,
            non_destructive: None,
            substitution_evaluation_depth: None,
            // Wrong: the row SHOULD have a diagnostic code
            diagnostic_codes: Some(vec![]),
        },
        authority: "mutation".to_owned(),
        positive_source: None,
        negative_source: None,
        completeness: "proven".to_owned(),
        owner_issue: 7036,
        oracle_disposition: "not_applicable".to_owned(),
    };
    assert_row_facts(&row, "mutation-test");
}

// ---------------------------------------------------------------------------
// Convenience helper
// ---------------------------------------------------------------------------

fn run_modifier_analysis(
    raw: &str,
    perl_minor: u16,
    enhanced_xx: bool,
    operator: RegexOperator,
) -> perl_regex::analyzer::ModifierAnalysis {
    let sequence = ModifierSequence::new(raw, 0)
        .unwrap_or_else(|| panic!("modifier sequence overflow for {raw:?}"));
    let feature = if enhanced_xx { FeatureState::Enabled } else { FeatureState::Disabled };
    let profile = RegexLanguageProfile::new(Some(PerlVersion::new(5, perl_minor)), feature);
    RegexAnalyzer::analyze_modifiers(operator, sequence, profile)
}
