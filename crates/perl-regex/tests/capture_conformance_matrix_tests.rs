//! Capture-numbering conformance matrix tests (issue #7036).
//!
//! Loads versioned JSON fixtures under `tests/fixtures/conformance/` with
//! programme `perl_regex_capture_conformance` and compares expected capture
//! declarations against [`perl_regex::RegexAnalyzer::analyze_captures`].
#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]
#![deny(clippy::map_err_ignore)]

use perl_regex::{
    RegexAnalyzer,
    analyzer::{
        CaptureLanguageProfile, FeatureState, ModifierSequence, PerlVersion, RegexLanguageProfile,
        RegexOperator,
    },
    conformance::{ConformanceCompleteness, OracleDisposition, SCHEMA_VERSION},
};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureProfile {
    perl_minor: Option<u16>,
    enhanced_xx: String,
    source_utf8: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpectedCaptureFacts {
    declaration_count: usize,
    numbers: Vec<u32>,
    named_families: Vec<String>,
    diagnostic_codes: Vec<String>,
    complete: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct CaptureFixtureRow {
    id: String,
    family: String,
    authority: String,
    pattern: String,
    operator: String,
    profile: FixtureProfile,
    modifier_sequence: String,
    positive_source: Option<String>,
    negative_source: Option<String>,
    expected: ExpectedCaptureFacts,
    completeness: String,
    owner_issue: u32,
    oracle_disposition: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct CaptureFixtureFile {
    schema_version: u32,
    programme: String,
    family: String,
    owner_issue: u32,
    rows: Vec<CaptureFixtureRow>,
}

const FIXTURE_NUMBERING: &str = include_str!("fixtures/conformance/captures_numbering.json");

fn parse_fixture(json: &str, label: &str) -> CaptureFixtureFile {
    serde_json::from_str(json)
        .unwrap_or_else(|err| panic!("fixture {label} failed to parse: {err}"))
}

fn all_fixtures() -> Vec<(String, CaptureFixtureFile)> {
    vec![("captures_numbering".to_owned(), parse_fixture(FIXTURE_NUMBERING, "captures_numbering"))]
}

fn resolve_operator(raw: &str) -> RegexOperator {
    match raw {
        "match" => RegexOperator::Match,
        "quote_regex" => RegexOperator::QuoteRegex,
        "substitution" => RegexOperator::Substitution,
        "transliteration" => RegexOperator::Transliteration,
        other => panic!("unknown operator {other:?}"),
    }
}

fn build_profile(profile: &FixtureProfile) -> CaptureLanguageProfile {
    let perl_minor = profile.perl_minor.unwrap_or(26);
    let enhanced_xx = match profile.enhanced_xx.as_str() {
        "enabled" => FeatureState::Enabled,
        "disabled" => FeatureState::Disabled,
        "unknown" => FeatureState::Unknown,
        other => panic!("unknown enhanced_xx {other:?}"),
    };
    let source_utf8 = match profile.source_utf8.as_str() {
        "enabled" => FeatureState::Enabled,
        "disabled" => FeatureState::Disabled,
        "unknown" => FeatureState::Unknown,
        other => panic!("unknown source_utf8 {other:?}"),
    };
    CaptureLanguageProfile::new(
        RegexLanguageProfile::new(Some(PerlVersion::new(5, perl_minor)), enhanced_xx),
        source_utf8,
    )
}

fn effective_modifiers(
    row: &CaptureFixtureRow,
    file: &str,
) -> perl_regex::analyzer::EffectiveModifiers {
    let operator = resolve_operator(&row.operator);
    let profile = RegexLanguageProfile::new(
        Some(PerlVersion::new(5, row.profile.perl_minor.unwrap_or(26))),
        match row.profile.enhanced_xx.as_str() {
            "enabled" => FeatureState::Enabled,
            "disabled" => FeatureState::Disabled,
            "unknown" => FeatureState::Unknown,
            other => panic!("fixture {file}/{}: unknown enhanced_xx {other:?}", row.id),
        },
    );
    let sequence = ModifierSequence::new(&row.modifier_sequence, 0)
        .unwrap_or_else(|| panic!("fixture {file}/{}: modifier_sequence offset overflow", row.id));
    RegexAnalyzer::analyze_modifiers(operator, sequence, profile).effective
}

fn assert_row_facts(row: &CaptureFixtureRow, file: &str) {
    let ctx = format!("fixture {file}/{}", row.id);
    let analysis = RegexAnalyzer::analyze_captures(
        &row.pattern,
        effective_modifiers(row, file),
        build_profile(&row.profile),
    );
    let exp = &row.expected;

    assert_eq!(analysis.declarations.len(), exp.declaration_count, "{ctx}: declaration_count");
    let actual_numbers: Vec<u32> = analysis
        .declarations
        .iter()
        .map(|declaration| declaration.number.expect("missing capture number in proven row"))
        .collect();
    assert_eq!(actual_numbers, exp.numbers, "{ctx}: capture numbers");

    let actual_families: Vec<String> =
        analysis.named_families.iter().map(|family| family.name.clone()).collect();
    assert_eq!(actual_families, exp.named_families, "{ctx}: named families");

    let mut actual_codes: Vec<&str> =
        analysis.diagnostics.iter().map(|d| d.code.as_str()).collect();
    actual_codes.sort_unstable();
    let mut expected_codes = exp.diagnostic_codes.iter().map(String::as_str).collect::<Vec<_>>();
    expected_codes.sort_unstable();
    assert_eq!(actual_codes, expected_codes, "{ctx}: diagnostic_codes");

    assert_eq!(analysis.status.is_complete(), exp.complete, "{ctx}: completeness flag");
}

#[test]
fn capture_fixture_schema_version_matches_crate() {
    for (file, fixture) in all_fixtures() {
        assert_eq!(
            fixture.schema_version, SCHEMA_VERSION,
            "fixture {file} schema_version must match crate SCHEMA_VERSION"
        );
    }
}

#[test]
fn capture_fixture_ids_are_unique_globally() {
    let mut seen = std::collections::BTreeMap::new();
    for (file, fixture) in all_fixtures() {
        for row in &fixture.rows {
            if let Some(prior) = seen.insert(row.id.clone(), file.clone()) {
                panic!(
                    "duplicate capture conformance id {:?} in {file} (first seen in {prior})",
                    row.id
                );
            }
        }
    }
}

#[test]
fn capture_fixtures_on_disk_match_harness() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/conformance");
    let mut on_disk = std::fs::read_dir(&dir)
        .unwrap_or_else(|err| panic!("read conformance dir: {err}"))
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("captures_") && name.ends_with(".json"))
        .collect::<Vec<_>>();
    on_disk.sort_unstable();

    let loaded: Vec<String> =
        all_fixtures().into_iter().map(|(name, _)| format!("{name}.json")).collect();
    assert_eq!(on_disk, loaded, "capture fixture files on disk and harness diverge");
}

#[test]
fn capture_fixture_schema_is_load_bearing() {
    for (file, fixture) in all_fixtures() {
        assert_eq!(fixture.programme, "perl_regex_capture_conformance", "fixture {file}");
        assert_eq!(fixture.owner_issue, 7036, "fixture {file}");
        assert_eq!(
            fixture.family, "captures.numbering",
            "fixture {file} family must match programme slice"
        );
        for row in &fixture.rows {
            assert!(!row.authority.trim().is_empty(), "fixture {file}/{} has no authority", row.id);
            assert!(
                row.positive_source.is_none() && row.negative_source.is_none(),
                "fixture {file}/{} must not claim oracle sources in this slice",
                row.id
            );
            assert_eq!(
                row.completeness,
                ConformanceCompleteness::Proven.as_str(),
                "{file}/{}",
                row.id
            );
            assert_eq!(
                row.oracle_disposition,
                OracleDisposition::NotApplicable.as_str(),
                "{file}/{}",
                row.id
            );
            assert_eq!(row.owner_issue, 7036, "{file}/{}", row.id);
            assert!(row.id.starts_with(&row.family), "{file}/{} id must start with family", row.id);
        }
    }
}

#[test]
fn all_capture_conformance_rows_match_analyzer_output() {
    for (file, fixture) in all_fixtures() {
        for row in &fixture.rows {
            assert_row_facts(row, &file);
        }
    }
}

#[test]
#[should_panic(expected = "capture numbers")]
fn capture_mutation_wrong_number_detected() {
    let fixture = parse_fixture(FIXTURE_NUMBERING, "captures_numbering");
    let mut row = fixture
        .rows
        .iter()
        .find(|row| row.id == "captures.numbering.mixed-group-forms")
        .expect("row captures.numbering.mixed-group-forms")
        .clone();
    row.expected.numbers = vec![9, 9, 9, 9];
    assert_row_facts(&row, "captures_numbering");
}
