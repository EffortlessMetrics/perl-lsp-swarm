//! Exact legacy parser-accuracy population identity for #13654.
//!
//! The production projector owns the historical applicability rule. These
//! tests bind its complete current output and prove that count-preserving swaps
//! and source mutations change the retained population identity.

use std::error::Error;
use std::path::PathBuf;

use xtask::parser_accuracy_legacy_population::{
    LegacyApplicability, LegacyFixtureInput, LegacyPopulationError,
    build_legacy_whitespace_population, load_legacy_whitespace_population,
};

const EXPECTED_APPLIED_CASE_COUNT: usize = 47;
const EXPECTED_POPULATION_IDENTITY: &str =
    "sha256:47a8013e2b01ae7d48ed107076aea29d3fc1ac23c5d05a486f1100faf5ffb63c";

type TestResult = Result<(), Box<dyn Error>>;

fn project_root() -> PathBuf {
    let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let _ = root.pop();
    root
}

fn fixture(id: &str, source: &str) -> LegacyFixtureInput {
    LegacyFixtureInput::new(id.to_owned(), format!("fixtures/{id}.pl"), source.as_bytes().to_vec())
}

#[test]
fn legacy_whitespace_population_is_bound_to_exact_current_rows() -> TestResult {
    let population = load_legacy_whitespace_population(&project_root())?;
    let summary = population.summary()?;
    let canonical_rows = population.canonical_ndjson()?;

    assert_eq!(
        summary.applied_case_count, EXPECTED_APPLIED_CASE_COUNT,
        "legacy applied population changed; review and retain the exact new rows"
    );
    assert!(
        summary.unclassified_case_count > 0,
        "legacy whole-file omissions must remain explicit until typed applicability replaces them"
    );
    assert_eq!(
        summary.total_case_count,
        population.rows().len(),
        "summary denominator must derive from retained rows"
    );
    assert_eq!(
        summary.applied_case_count + summary.unclassified_case_count,
        summary.total_case_count,
        "every live fixture must retain exactly one legacy disposition"
    );
    assert_eq!(
        summary.population_identity, EXPECTED_POPULATION_IDENTITY,
        "legacy population identity changed; update only after reviewing these canonical rows:\n{canonical_rows}"
    );

    Ok(())
}

#[test]
fn shuffled_input_preserves_canonical_rows_and_identity() -> TestResult {
    let ordered_inputs = vec![
        fixture("alpha", "my $alpha = 1;\n"),
        fixture("beta", "my $beta = 2;\n"),
        fixture("heredoc", "print <<'END';\nEND\n"),
    ];
    let mut shuffled_inputs = ordered_inputs.clone();
    shuffled_inputs.reverse();

    let ordered = build_legacy_whitespace_population(1, ordered_inputs)?;
    let shuffled = build_legacy_whitespace_population(1, shuffled_inputs)?;

    assert_eq!(ordered.canonical_ndjson()?, shuffled.canonical_ndjson()?);
    assert_eq!(ordered.population_identity()?, shuffled.population_identity()?);

    Ok(())
}

#[test]
fn equal_count_case_swap_changes_population_identity() -> TestResult {
    let original = build_legacy_whitespace_population(
        1,
        vec![fixture("alpha", "my $alpha = 1;\n"), fixture("beta", "print <<'END';\nEND\n")],
    )?;
    let swapped = build_legacy_whitespace_population(
        1,
        vec![fixture("alpha", "print <<'END';\nEND\n"), fixture("beta", "my $beta = 1;\n")],
    )?;

    assert_eq!(original.applied_count(), swapped.applied_count());
    assert_ne!(original.population_identity()?, swapped.population_identity()?);

    let original_applied = original
        .rows()
        .iter()
        .find(|row| row.legacy_applicability == LegacyApplicability::Applied)
        .map(|row| row.case_id.as_str());
    let swapped_applied = swapped
        .rows()
        .iter()
        .find(|row| row.legacy_applicability == LegacyApplicability::Applied)
        .map(|row| row.case_id.as_str());
    assert_ne!(original_applied, swapped_applied);

    Ok(())
}

#[test]
fn exact_source_mutation_changes_source_and_population_identity() -> TestResult {
    let original =
        build_legacy_whitespace_population(1, vec![fixture("alpha", "my $alpha = 1;\n")])?;
    let mutated =
        build_legacy_whitespace_population(1, vec![fixture("alpha", "my $alpha = 2;\n")])?;

    let original_digests =
        original.rows().iter().map(|row| row.source_content_digest.as_str()).collect::<Vec<_>>();
    let mutated_digests =
        mutated.rows().iter().map(|row| row.source_content_digest.as_str()).collect::<Vec<_>>();
    assert_ne!(original_digests, mutated_digests);
    assert_ne!(original.population_identity()?, mutated.population_identity()?);

    Ok(())
}

#[test]
fn duplicate_fixture_identity_fails_closed() {
    let result = build_legacy_whitespace_population(
        1,
        vec![fixture("duplicate", "my $x = 1;\n"), fixture("duplicate", "my $x = 2;\n")],
    );

    assert!(matches!(result, Err(LegacyPopulationError::DuplicateFixtureId { .. })));
}

#[test]
fn empty_or_unsupported_population_fails_closed() {
    assert!(matches!(
        build_legacy_whitespace_population(1, Vec::new()),
        Err(LegacyPopulationError::EmptyPopulation)
    ));
    assert!(matches!(
        build_legacy_whitespace_population(2, vec![fixture("alpha", "1;\n")]),
        Err(LegacyPopulationError::UnsupportedManifestSchema { observed: 2 })
    ));
}

#[test]
fn case_identity_is_nonordinal_and_tracks_fixture_identity() -> TestResult {
    let population = build_legacy_whitespace_population(
        1,
        vec![fixture("zeta", "my $zeta = 1;\n"), fixture("alpha", "my $alpha = 1;\n")],
    )?;

    let case_ids = population.rows().iter().map(|row| row.case_id.as_str()).collect::<Vec<_>>();
    assert_eq!(
        case_ids,
        vec![
            "trailing_horizontal_whitespace.legacy.v1::alpha",
            "trailing_horizontal_whitespace.legacy.v1::zeta",
        ]
    );

    Ok(())
}
