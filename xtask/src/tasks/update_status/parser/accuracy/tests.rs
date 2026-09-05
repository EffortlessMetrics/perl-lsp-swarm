//! Reader-entry and rendering proof for the parser-accuracy artifact.
//!
//! Split from `accuracy.rs` so each module stays inside the 400-line
//! anti-regression gate in `update_status::mod_tests`. The fail-closed trust
//! controls live in the sibling `trust_tests` module; the shared row and
//! artifact builders below are `pub(super)` so both use one definition.

use super::{
    ParserAccuracyArtifactSummary, ParserAccuracyLegacyPopulation, ParserAccuracyMetricSummary,
    parser_accuracy_metric_summary, read_parser_accuracy_artifact,
};

/// The advertised example artifact must be consumable by the production reader.
///
/// The schema suite proves the fixture is *shape*-valid, which is a weaker
/// claim: the fixture previously carried both an `investigation_only` and a
/// stale `insufficient_data` row for `whitespace_invariance_rate`, passing
/// the schema while the runtime validators rejected the duplicate. So the
/// documented valid example could not be read by the code that reads real
/// artifacts, and the negative-control suite rested on a contradictory
/// input. This runs the fixture through the real entry point.
#[test]
fn example_artifact_is_consumable_by_the_production_reader() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/parser-accuracy/example-artifact.json");
    let raw = std::fs::read_to_string(&fixture)
        .unwrap_or_else(|error| panic!("example artifact must be readable: {error}"));

    let Ok(root) = tempfile::tempdir() else {
        panic!("temp root must be creatable");
    };
    let metrics_dir = root.path().join("target/metrics");
    if let Err(error) = std::fs::create_dir_all(&metrics_dir) {
        panic!("metrics directory must be creatable: {error}");
    }
    if let Err(error) = std::fs::write(metrics_dir.join("parser_accuracy.json"), &raw) {
        panic!("example artifact must be writable into the temp root: {error}");
    }

    let artifact = read_parser_accuracy_artifact(root.path());
    assert!(
        artifact.is_some(),
        "the advertised example artifact must load through the production reader"
    );
}
pub(super) fn investigation_row(
    metric: &str,
    value: f64,
    sample_count: u64,
) -> ParserAccuracyMetricSummary {
    investigation_row_with_disposition(
        metric,
        value,
        sample_count,
        "investigation_only",
        "not_proven",
        "none",
        false,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn investigation_row_with_disposition(
    metric: &str,
    value: f64,
    sample_count: u64,
    evidence_class: &str,
    terminal_disposition: &str,
    packet_policy: &str,
    floor_eligible: bool,
) -> ParserAccuracyMetricSummary {
    ParserAccuracyMetricSummary::InvestigationOnly {
        metric: metric.to_string(),
        value,
        sample_count,
        transformation_profile: "trailing_horizontal_whitespace.legacy.v1".to_string(),
        evidence_class: evidence_class.to_string(),
        terminal_disposition: terminal_disposition.to_string(),
        reason: "legacy_hash_oracle_untrusted".to_string(),
        packet_policy: packet_policy.to_string(),
        floor_eligible,
        unknown_fields: serde_json::Map::new(),
    }
}

pub(super) fn valid_population() -> ParserAccuracyLegacyPopulation {
    ParserAccuracyLegacyPopulation {
        transformation_profile: "trailing_horizontal_whitespace.legacy.v1".to_string(),
        population_identity: format!("sha256:{}", "a".repeat(64)),
        aggregate_metric: "whitespace_invariance_rate".to_string(),
        population_total_count: 4,
        population_applied_count: 2,
        population_unclassified_count: 2,
        manifest_schema_version: 1,
    }
}

pub(super) fn artifact_with_rows(
    rows: Vec<ParserAccuracyMetricSummary>,
    population: ParserAccuracyLegacyPopulation,
) -> ParserAccuracyArtifactSummary {
    ParserAccuracyArtifactSummary {
        schema_version: 1,
        subsystem: "parser_accuracy".to_string(),
        cadence: "pr".to_string(),
        denominator: super::ParserAccuracyDenominator {
            fixture_count: 4,
            fixture_family_count: 1,
            scored_line_count: 4,
            scored_symbol_count: 2,
            fully_labeled_region_count: 1,
            partial_labeled_region_count: 1,
            unknown_region_count: 1,
            negative_region_count: 1,
            dynamic_boundary_case_count: 0,
            unsupported_construct_case_count: 0,
            real_project_file_count: 0,
            generated_fixture_count: 0,
            hand_labeled_fixture_count: 4,
        },
        families: vec![],
        metrics: rows,
        legacy_population: population,
        failure_packets: vec![],
    }
}

#[test]
fn typed_investigation_rows_render_from_artifact_fields() {
    let summary = parser_accuracy_metric_summary(&[
        ParserAccuracyMetricSummary::Measured {
            metric: "line_construct_f1".to_string(),
            value: 1.0,
            sample_count: 125,
            unmodeled_fields: serde_json::Map::new(),
        },
        investigation_row("whitespace_invariance_rate", 0.4, 47),
        investigation_row("comment_invariance_rate", 1.0, 47),
    ]);

    assert!(summary.contains("line_construct_f1=1.0 (n=125)"));
    assert!(summary.contains(
        "whitespace_invariance_rate: investigation_only (not_proven; legacy_hash_oracle_untrusted; trailing_horizontal_whitespace.legacy.v1; observed=0.4; n=47)"
    ));
    assert!(summary.contains("1 additional investigation_only rows"));
    assert!(!summary.contains("whitespace_invariance_rate=0.4"));
}

#[test]
fn measured_rows_render_as_trusted_even_with_legacy_shaped_names() {
    // The typed contract replaced the three-name quarantine classifier: a
    // measured row is trusted evidence regardless of its name, and legacy
    // observations are typed investigation_only at construction.
    let summary = parser_accuracy_metric_summary(&[ParserAccuracyMetricSummary::Measured {
        metric: "whitespace_invariance_rate".to_string(),
        value: 0.4,
        sample_count: 47,
        unmodeled_fields: serde_json::Map::new(),
    }]);

    assert!(
        summary.contains("whitespace_invariance_rate=0.4 (n=47)"),
        "measured rows must not be downgraded by name resemblance: {summary}"
    );
    assert!(!summary.contains("investigation_only"));
}
