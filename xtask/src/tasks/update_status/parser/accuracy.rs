//! Parser accuracy artifact types and status-row formatting.
//!
//! Holds the serde-deserializable structs for the JSON artifact produced by
//! `cargo xtask metrics parser-accuracy --json` plus the helper functions that
//! render those structs into status-doc rows.

use std::fs;
use std::path::Path;

use serde::Deserialize;
use xtask::parser_accuracy_legacy_population::is_canonical_population_identity;

// Strictness here is deliberately per-object, not blanket.
//
// `.ci/schemas/parser-accuracy.schema.json` sets `additionalProperties: false`
// everywhere, but this reader is a *projection*: the artifact carries twelve
// top-level keys (`commit`, `generated_at`, `gold_drift`, `metric_runtime`
// besides these) and status rendering needs eight. So the artifact and the
// family/measured/insufficient rows stay permissive — making them strict would
// reject every real artifact.
//
// The objects this reader models *completely* are strict, because that is where
// the trust contract lives and where a stray field would smuggle a contradictory
// claim past the typed checks: `legacy_population` (all seven schema fields) and
// `denominator` (all thirteen). The investigation row is likewise complete and is
// enforced through `unknown_fields` below — it cannot use `deny_unknown_fields`
// because that attribute applies to every variant of an internally tagged enum,
// including the two that are projections.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct ParserAccuracyArtifactSummary {
    pub(super) schema_version: u32,
    pub(super) subsystem: String,
    pub(super) cadence: String,
    pub(super) denominator: ParserAccuracyDenominator,
    pub(super) families: Vec<ParserAccuracyFamilySummary>,
    pub(super) metrics: Vec<ParserAccuracyMetricSummary>,
    pub(super) legacy_population: ParserAccuracyLegacyPopulation,
    #[serde(default)]
    pub(super) failure_packets: Vec<serde_json::Value>,
}

/// Retained identity of the quarantined legacy metamorphic population.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ParserAccuracyLegacyPopulation {
    pub(super) transformation_profile: String,
    pub(super) population_identity: String,
    pub(super) aggregate_metric: String,
    pub(super) population_total_count: u64,
    pub(super) population_applied_count: u64,
    pub(super) population_unclassified_count: u64,
    pub(super) manifest_schema_version: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ParserAccuracyDenominator {
    pub(super) fixture_count: u64,
    pub(super) fixture_family_count: u64,
    pub(super) scored_line_count: u64,
    pub(super) scored_symbol_count: u64,
    pub(super) fully_labeled_region_count: u64,
    pub(super) partial_labeled_region_count: u64,
    pub(super) unknown_region_count: u64,
    pub(super) negative_region_count: u64,
    pub(super) dynamic_boundary_case_count: u64,
    pub(super) unsupported_construct_case_count: u64,
    pub(super) real_project_file_count: u64,
    pub(super) generated_fixture_count: u64,
    pub(super) hand_labeled_fixture_count: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ParserAccuracyFamilySummary {
    pub(super) family: String,
    pub(super) fixture_count: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub(super) enum ParserAccuracyMetricSummary {
    Measured {
        metric: String,
        value: f64,
        sample_count: u64,
    },
    InsufficientData {
        metric: String,
        reason: String,
        sample_count: u64,
    },
    InvestigationOnly {
        metric: String,
        value: f64,
        sample_count: u64,
        transformation_profile: String,
        evidence_class: String,
        terminal_disposition: String,
        reason: String,
        packet_policy: String,
        floor_eligible: bool,
        /// Anything the schema's `investigationOnlyMetric` does not permit.
        ///
        /// The schema sets `additionalProperties: false` on this variant, so a
        /// non-empty map is an artifact the schema rejects. Notably `confidence`
        /// is legal on measured and insufficient rows but not here, so without
        /// this a hand-edited row could carry a trust signal the contract
        /// forbids and still render as current status.
        #[serde(flatten)]
        unknown_fields: serde_json::Map<String, serde_json::Value>,
    },
}

impl ParserAccuracyMetricSummary {
    fn name(&self) -> &str {
        match self {
            ParserAccuracyMetricSummary::Measured { metric, .. }
            | ParserAccuracyMetricSummary::InsufficientData { metric, .. }
            | ParserAccuracyMetricSummary::InvestigationOnly { metric, .. } => metric,
        }
    }
}

pub(super) fn read_parser_accuracy_artifact(root: &Path) -> Option<ParserAccuracyArtifactSummary> {
    let path = root.join("target/metrics/parser_accuracy.json");
    let raw = fs::read_to_string(path).ok()?;
    let artifact: ParserAccuracyArtifactSummary = serde_json::from_str(&raw).ok()?;
    if artifact.schema_version != 1 || artifact.subsystem != "parser_accuracy" {
        return None;
    }
    if !trust_disposition_is_fail_closed(&artifact) {
        return None;
    }
    Some(artifact)
}

/// Fail-closed consumption of the typed trust and disposition contract.
///
/// Unknown trust or disposition values, contradictory shapes, identities that
/// are not canonical sha256 digests, populations whose counts do not close, and
/// investigation rows that claim floor eligibility or packet emission all
/// reject the artifact instead of silently rendering trusted accuracy.
///
/// The one case that is *not* a rejection is a retained population with zero
/// applied rows: a manifest whose fixtures are all excluded by the legacy
/// whitespace heuristic has nothing to observe, so the aggregate is honestly
/// `insufficient_data` rather than an investigation row. Refusing that shape
/// would fail a valid no-observation run instead of reporting it.
fn trust_disposition_is_fail_closed(artifact: &ParserAccuracyArtifactSummary) -> bool {
    let population = &artifact.legacy_population;
    if !is_canonical_population_identity(&population.population_identity) {
        return false;
    }
    if population.transformation_profile.is_empty()
        || population.aggregate_metric.is_empty()
        || population.manifest_schema_version == 0
        || population.population_total_count == 0
    {
        return false;
    }
    // Checked, because these are attacker- or migration-supplied `u64`s: a
    // wrapping sum can forge a closing population, and in a debug build the
    // plain `+` aborts status generation outright.
    let Some(closed) =
        population.population_applied_count.checked_add(population.population_unclassified_count)
    else {
        return false;
    };
    if closed != population.population_total_count {
        return false;
    }

    let expects_observation = population.population_applied_count > 0;
    let mut aggregate_investigation_rows = 0_usize;
    let mut aggregate_insufficient_rows = 0_usize;
    for metric in &artifact.metrics {
        match metric {
            ParserAccuracyMetricSummary::InvestigationOnly {
                metric,
                value: _,
                sample_count,
                transformation_profile,
                evidence_class,
                terminal_disposition,
                reason,
                packet_policy,
                floor_eligible,
                unknown_fields,
            } => {
                // The schema forbids extra properties on this variant, so an
                // artifact carrying any is one the schema rejects.
                if !unknown_fields.is_empty() {
                    return false;
                }
                if evidence_class != "investigation_only"
                    || terminal_disposition != "not_proven"
                    || packet_policy != "none"
                    || *floor_eligible
                    || reason.is_empty()
                    || transformation_profile.is_empty()
                    || *sample_count == 0
                {
                    return false;
                }
                if metric == &population.aggregate_metric {
                    if transformation_profile != &population.transformation_profile {
                        return false;
                    }
                    if *sample_count != population.population_applied_count {
                        return false;
                    }
                    aggregate_investigation_rows += 1;
                }
            }
            // The aggregate may be `insufficient_data` only when the retained
            // population applied to nothing. With applied rows present, an
            // untyped aggregate is the exact conflation this contract exists to
            // reject.
            ParserAccuracyMetricSummary::InsufficientData { metric, .. }
                if metric == &population.aggregate_metric =>
            {
                if expects_observation {
                    return false;
                }
                aggregate_insufficient_rows += 1;
            }
            // A measured aggregate is trusted accuracy by another name, at any
            // applied count.
            ParserAccuracyMetricSummary::Measured { metric, .. }
                if metric == &population.aggregate_metric =>
            {
                return false;
            }
            ParserAccuracyMetricSummary::Measured { .. }
            | ParserAccuracyMetricSummary::InsufficientData { .. } => {}
        }
    }

    // Exactly one row carries the aggregate. Without the uniqueness check two
    // otherwise-valid rows with different values both pass and the rendered
    // status is decided by array order.
    if expects_observation {
        aggregate_investigation_rows == 1
    } else {
        aggregate_investigation_rows == 0 && aggregate_insufficient_rows == 1
    }
}

pub(super) fn parser_accuracy_rows(artifact: Option<&ParserAccuracyArtifactSummary>) -> String {
    const ARTIFACT_PATH: &str = "`target/metrics/parser_accuracy.json`";
    const SPEC_PATH: &str = "`.kiro/specs/parser-accuracy-observability`";
    const SCHEMA_PATH: &str = "`.ci/schemas/parser-accuracy.schema.json`";

    let Some(artifact) = artifact else {
        return format!(
            "| **Accuracy denominator** | insufficient_data | Generate with `cargo xtask metrics parser-accuracy --json`; missing artifact is not treated as zero | {ARTIFACT_PATH}; {SPEC_PATH} |\n\
             | **Accuracy scorers** | insufficient_data | line/AST/symbol scoring rows wait for real denominators and validated artifact input | {SCHEMA_PATH} |"
        );
    };

    let d = &artifact.denominator;
    let family_summary = parser_accuracy_family_summary(&artifact.families);
    let metric_summary = parser_accuracy_metric_summary(&artifact.metrics);
    format!(
        "| **Accuracy denominator** | {} fixtures / {} families | {} scored lines, {} scored symbols, {} fully labeled, {} partial, {} unknown, {} negative, {} dynamic boundaries, {} unsupported, {} real-project, {} generated, {} hand-labeled; cadence `{}` | {ARTIFACT_PATH}; {SPEC_PATH} |\n\
         | **Accuracy families** | {} | fixture family inventory from parser accuracy manifest | {ARTIFACT_PATH} |\n\
         | **Accuracy scorers** | {} | missing accuracy rows stay `insufficient_data`; they are not rendered as zero or pass | {SCHEMA_PATH} |\n\
         | **Failure packets** | {} active packets | See `parser_accuracy_failure_packets.json` for committed packet details | generated |\n\
         | **Fixture inventory** | {} fixtures / {} families | See `parser_accuracy_fixture_inventory.json` for compact fixture metadata | generated |",
        d.fixture_count,
        d.fixture_family_count,
        d.scored_line_count,
        d.scored_symbol_count,
        d.fully_labeled_region_count,
        d.partial_labeled_region_count,
        d.unknown_region_count,
        d.negative_region_count,
        d.dynamic_boundary_case_count,
        d.unsupported_construct_case_count,
        d.real_project_file_count,
        d.generated_fixture_count,
        d.hand_labeled_fixture_count,
        artifact.cadence,
        family_summary,
        metric_summary,
        artifact.failure_packets.len(),
        d.fixture_count,
        d.fixture_family_count,
    )
}

fn parser_accuracy_family_summary(families: &[ParserAccuracyFamilySummary]) -> String {
    if families.is_empty() {
        return "insufficient_data".to_string();
    }

    let rendered = families
        .iter()
        .take(6)
        .map(|family| format!("{} ({})", family.family, family.fixture_count))
        .collect::<Vec<_>>()
        .join(", ");
    let hidden = families.len().saturating_sub(6);
    if hidden == 0 { rendered } else { format!("{rendered}, +{hidden} more") }
}

fn parser_accuracy_metric_summary(metrics: &[ParserAccuracyMetricSummary]) -> String {
    const SUMMARY_METRICS: &[&str] = &[
        "line_construct_f1",
        "ast_node_kind_f1",
        "symbol_decl_f1",
        "symbol_ref_f1",
        "dynamic_false_precision_count",
        "fast_path_wrong_result_count",
        "method_completion_receiver_hit_rate",
        "method_completion_false_receiver_count",
        "method_completion_dynamic_receiver_fallback_count",
        "method_completion_visible_symbol_relevance",
        "diagnostic_dynamic_boundary_false_positive_count",
        "diagnostic_undefined_symbol_false_positive_count",
        "diagnostic_undefined_symbol_false_negative_count",
        "document_symbol_span_exact_rate",
        "goto_definition_hit_rate",
        "goto_definition_span_exact_rate",
        "goto_definition_false_target_count",
        "references_precision",
        "references_recall",
        "references_false_positive_count",
        "hover_origin_accuracy",
        "whitespace_invariance_rate",
    ];

    if metrics.is_empty() {
        return "insufficient_data".to_string();
    }

    let mut parts = Vec::new();
    let selected_metrics = SUMMARY_METRICS
        .iter()
        .filter_map(|name| metrics.iter().find(|metric| metric.name() == *name))
        .collect::<Vec<_>>();
    let selected = selected_metrics
        .iter()
        .map(|metric| render_parser_accuracy_metric(metric))
        .collect::<Vec<_>>();
    if !selected.is_empty() {
        parts.push(format!("selected {}", selected.join(", ")));
    }

    let trusted_measured_count = metrics
        .iter()
        .filter(|metric| matches!(metric, ParserAccuracyMetricSummary::Measured { .. }))
        .count();
    let selected_trusted_measured_count = selected_metrics
        .iter()
        .filter(|metric| matches!(metric, ParserAccuracyMetricSummary::Measured { .. }))
        .count();
    let investigation_count = metrics
        .iter()
        .filter(|metric| matches!(metric, ParserAccuracyMetricSummary::InvestigationOnly { .. }))
        .count();
    let selected_investigation_count = selected_metrics
        .iter()
        .filter(|metric| matches!(metric, ParserAccuracyMetricSummary::InvestigationOnly { .. }))
        .count();
    let insufficient_count = metrics
        .iter()
        .filter(|metric| matches!(metric, ParserAccuracyMetricSummary::InsufficientData { .. }))
        .count();

    let additional_measured =
        trusted_measured_count.saturating_sub(selected_trusted_measured_count);
    if additional_measured > 0 {
        parts.push(format!("{additional_measured} additional measured rows"));
    }
    let additional_investigation = investigation_count.saturating_sub(selected_investigation_count);
    if additional_investigation > 0 {
        parts.push(format!("{additional_investigation} additional investigation_only rows"));
    }
    if insufficient_count > 0 {
        parts.push(format!("{insufficient_count} insufficient_data rows preserved"));
    }
    parts.join("; ")
}

fn render_parser_accuracy_metric(metric: &ParserAccuracyMetricSummary) -> String {
    match metric {
        ParserAccuracyMetricSummary::InvestigationOnly {
            metric,
            value,
            sample_count,
            transformation_profile,
            evidence_class,
            terminal_disposition,
            reason,
            ..
        } => format!(
            "{metric}: {evidence_class} ({terminal_disposition}; {reason}; {transformation_profile}; observed={value:.1}; n={sample_count})"
        ),
        ParserAccuracyMetricSummary::Measured { metric, value, sample_count } => {
            format!("{metric}={value:.1} (n={sample_count})")
        }
        ParserAccuracyMetricSummary::InsufficientData { metric, reason, sample_count } => {
            format!("{metric}: insufficient_data ({reason}; n={sample_count})")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ParserAccuracyArtifactSummary, ParserAccuracyLegacyPopulation, ParserAccuracyMetricSummary,
        parser_accuracy_metric_summary, read_parser_accuracy_artifact,
        trust_disposition_is_fail_closed,
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

    fn investigation_row(
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
    fn investigation_row_with_disposition(
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

    fn valid_population() -> ParserAccuracyLegacyPopulation {
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

    fn artifact_with_rows(
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
        }]);

        assert!(
            summary.contains("whitespace_invariance_rate=0.4 (n=47)"),
            "measured rows must not be downgraded by name resemblance: {summary}"
        );
        assert!(!summary.contains("investigation_only"));
    }

    #[test]
    fn fail_closed_reader_rejects_missing_or_stale_population_evidence() {
        // The base artifact is deliberately VALID and asserted so below. Before
        // this, the base carried no row for the aggregate metric at all, so
        // `trust_disposition_is_fail_closed` returned false for every case in
        // this test whether or not the mutation under test was present — the
        // controls could not discriminate.
        let artifact_json = |legacy_population: serde_json::Value,
                             extra_metrics: Vec<serde_json::Value>| {
            let mut metrics = vec![
                serde_json::json!({
                    "state": "insufficient_data",
                    "metric": "line_construct_f1",
                    "reason": "line-level gold scorer is not wired yet",
                    "sample_count": 0,
                    "confidence": "low",
                }),
                serde_json::json!({
                    "state": "investigation_only",
                    "metric": "whitespace_invariance_rate",
                    "value": 0.5,
                    // Tracks the population block, so a control that mutates
                    // only the counts is not also failing the applied-count
                    // check and thereby proving nothing about its own subject.
                    "sample_count": legacy_population
                        .get("population_applied_count")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(2),
                    "transformation_profile": "trailing_horizontal_whitespace.legacy.v1",
                    "evidence_class": "investigation_only",
                    "terminal_disposition": "not_proven",
                    "reason": "legacy_hash_oracle_untrusted",
                    "packet_policy": "none",
                    "floor_eligible": false,
                }),
            ];
            metrics.extend(extra_metrics);
            serde_json::json!({
                "schema_version": 1,
                "subsystem": "parser_accuracy",
                "cadence": "pr",
                "denominator": {
                    "fixture_count": 4,
                    "fixture_family_count": 1,
                    "scored_line_count": 4,
                    "scored_symbol_count": 2,
                    "fully_labeled_region_count": 1,
                    "partial_labeled_region_count": 1,
                    "unknown_region_count": 1,
                    "negative_region_count": 1,
                    "dynamic_boundary_case_count": 0,
                    "unsupported_construct_case_count": 0,
                    "real_project_file_count": 0,
                    "generated_fixture_count": 0,
                    "hand_labeled_fixture_count": 4,
                },
                "families": [{
                    "family": "packages",
                    "fixture_count": 4,
                }],
                "metrics": metrics,
                "legacy_population": legacy_population,
                "failure_packets": [],
                "gold_drift": {},
                "metric_runtime": {},
            })
            .to_string()
        };
        let population_json = |overrides: serde_json::Value| {
            let mut base = serde_json::json!({
                "transformation_profile": "trailing_horizontal_whitespace.legacy.v1",
                "population_identity": format!("sha256:{}", "a".repeat(64)),
                "aggregate_metric": "whitespace_invariance_rate",
                "population_total_count": 4,
                "population_applied_count": 2,
                "population_unclassified_count": 2,
                "manifest_schema_version": 1,
            });
            if let (Some(fields), serde_json::Value::Object(overrides)) =
                (base.as_object_mut(), overrides)
            {
                for (key, value) in overrides {
                    fields.insert(key, value);
                }
            }
            base
        };
        let parse = |raw: &str| -> ParserAccuracyArtifactSummary {
            serde_json::from_str(raw)
                .expect("artifact with a well-shaped population must deserialize")
        };

        // Positive control. Without this every assertion below could pass
        // against a base that was already failing for an unrelated reason.
        let valid = artifact_json(population_json(serde_json::json!({})), Vec::new());
        assert!(
            trust_disposition_is_fail_closed(&parse(&valid)),
            "the base artifact must be valid, or the negative controls prove nothing"
        );

        // A missing population block must not deserialize to a readable artifact.
        let missing = artifact_json(serde_json::json!({}), Vec::new());
        assert!(
            serde_json::from_str::<ParserAccuracyArtifactSummary>(&missing).is_err(),
            "artifact without retained population evidence must fail closed"
        );

        // Counts that do not close over retained rows reject the artifact.
        let unclosed = artifact_json(
            population_json(serde_json::json!({ "population_unclassified_count": 1 })),
            Vec::new(),
        );
        assert!(
            !trust_disposition_is_fail_closed(&parse(&unclosed)),
            "population counts that do not close must fail closed"
        );

        // An identity that is not a sha256-tagged digest rejects the artifact.
        let untagged = artifact_json(
            population_json(serde_json::json!({ "population_identity": "legacy-digest" })),
            Vec::new(),
        );
        assert!(
            !trust_disposition_is_fail_closed(&parse(&untagged)),
            "an untagged population identity must fail closed"
        );

        // Uppercase hex is not the canonical format. The schema pins
        // `^sha256:[0-9a-f]{64}$`, so an `is_ascii_hexdigit` check would admit
        // an artifact the schema rejects.
        let uppercase = artifact_json(
            population_json(serde_json::json!({
                "population_identity": format!("sha256:{}", "A".repeat(64)),
            })),
            Vec::new(),
        );
        assert!(
            !trust_disposition_is_fail_closed(&parse(&uppercase)),
            "an uppercase population digest must fail closed"
        );
        let mixed_case = artifact_json(
            population_json(serde_json::json!({
                "population_identity": format!("sha256:{}{}", "A", "a".repeat(63)),
            })),
            Vec::new(),
        );
        assert!(
            !trust_disposition_is_fail_closed(&parse(&mixed_case)),
            "a single uppercase digit in the digest must fail closed"
        );

        // Counts that overflow u64 must be refused, not wrapped into a total
        // that closes and not left to abort a debug build.
        //
        // This is a *forged* population: `u64::MAX + 5` wraps to 4, which
        // matches the declared total, and the aggregate row's sample count is
        // set to the same `u64::MAX` so the applied-count check agrees too.
        // Every other invariant holds, so only the checked addition can reject
        // it — with `wrapping_add` this artifact renders as current status.
        let overflow = artifact_json(
            population_json(serde_json::json!({
                "population_total_count": 4,
                "population_applied_count": u64::MAX,
                "population_unclassified_count": 5,
            })),
            Vec::new(),
        );
        assert!(
            !trust_disposition_is_fail_closed(&parse(&overflow)),
            "population counts that overflow must fail closed"
        );

        // Two rows carrying the aggregate leave the reported value to array
        // order. Both rows here are individually well formed.
        let duplicate = artifact_json(
            population_json(serde_json::json!({})),
            vec![serde_json::json!({
                "state": "investigation_only",
                "metric": "whitespace_invariance_rate",
                "value": 0.9,
                "sample_count": 2,
                "transformation_profile": "trailing_horizontal_whitespace.legacy.v1",
                "evidence_class": "investigation_only",
                "terminal_disposition": "not_proven",
                "reason": "legacy_hash_oracle_untrusted",
                "packet_policy": "none",
                "floor_eligible": false,
            })],
        );
        assert!(
            !trust_disposition_is_fail_closed(&parse(&duplicate)),
            "a duplicated aggregate row must fail closed"
        );

        // The schema forbids extra properties on an investigation row.
        // `confidence` is legal on measured and insufficient rows but not here,
        // so it is the exact trust signal this contract must not admit.
        let stray_field = artifact_json(
            population_json(serde_json::json!({
                "aggregate_metric": "comment_invariance_rate",
            })),
            vec![serde_json::json!({
                "state": "investigation_only",
                "metric": "comment_invariance_rate",
                "value": 0.5,
                "sample_count": 2,
                "transformation_profile": "trailing_horizontal_whitespace.legacy.v1",
                "evidence_class": "investigation_only",
                "terminal_disposition": "not_proven",
                "reason": "legacy_hash_oracle_untrusted",
                "packet_policy": "none",
                "floor_eligible": false,
                "confidence": "high",
            })],
        );
        assert!(
            !trust_disposition_is_fail_closed(&parse(&stray_field)),
            "an investigation row carrying a schema-forbidden field must fail closed"
        );

        // A zero population is not a valid retained population.
        let empty = artifact_json(
            population_json(serde_json::json!({
                "population_total_count": 0,
                "population_applied_count": 0,
                "population_unclassified_count": 0,
            })),
            Vec::new(),
        );
        assert!(
            !trust_disposition_is_fail_closed(&parse(&empty)),
            "an empty retained population must fail closed"
        );
    }

    #[test]
    fn zero_applied_population_reports_instead_of_failing() {
        // A retained population whose fixtures are all excluded by the legacy
        // whitespace heuristic has nothing to observe, so the generator emits
        // the aggregate as `insufficient_data`. That is a valid no-observation
        // run: rejecting it would fail a valid custom `--manifest` and make
        // `--json` write an artifact this reader then refuses.
        let artifact = |aggregate_row: serde_json::Value| {
            serde_json::json!({
                "schema_version": 1,
                "subsystem": "parser_accuracy",
                "cadence": "pr",
                "denominator": {
                    "fixture_count": 4,
                    "fixture_family_count": 1,
                    "scored_line_count": 4,
                    "scored_symbol_count": 2,
                    "fully_labeled_region_count": 1,
                    "partial_labeled_region_count": 1,
                    "unknown_region_count": 1,
                    "negative_region_count": 1,
                    "dynamic_boundary_case_count": 0,
                    "unsupported_construct_case_count": 0,
                    "real_project_file_count": 0,
                    "generated_fixture_count": 0,
                    "hand_labeled_fixture_count": 4,
                },
                "families": [{ "family": "packages", "fixture_count": 4 }],
                "metrics": [aggregate_row],
                "legacy_population": {
                    "transformation_profile": "trailing_horizontal_whitespace.legacy.v1",
                    "population_identity": format!("sha256:{}", "a".repeat(64)),
                    "aggregate_metric": "whitespace_invariance_rate",
                    "population_total_count": 4,
                    "population_applied_count": 0,
                    "population_unclassified_count": 4,
                    "manifest_schema_version": 1,
                },
                "failure_packets": [],
                "gold_drift": {},
                "metric_runtime": {},
            })
            .to_string()
        };
        let parse = |raw: &str| -> ParserAccuracyArtifactSummary {
            serde_json::from_str(raw).expect("well-shaped artifact must deserialize")
        };

        let insufficient = artifact(serde_json::json!({
            "state": "insufficient_data",
            "metric": "whitespace_invariance_rate",
            "reason": "no retained fixture matched the legacy whitespace profile",
            "sample_count": 0,
            "confidence": "low",
        }));
        assert!(
            trust_disposition_is_fail_closed(&parse(&insufficient)),
            "a zero-applied population must report, not fail the artifact"
        );

        // Opposite-direction control: with nothing applied there is nothing to
        // have investigated, so an investigation row is a claim about evidence
        // that does not exist.
        let investigated = artifact(serde_json::json!({
            "state": "investigation_only",
            "metric": "whitespace_invariance_rate",
            "value": 0.5,
            "sample_count": 2,
            "transformation_profile": "trailing_horizontal_whitespace.legacy.v1",
            "evidence_class": "investigation_only",
            "terminal_disposition": "not_proven",
            "reason": "legacy_hash_oracle_untrusted",
            "packet_policy": "none",
            "floor_eligible": false,
        }));
        assert!(
            !trust_disposition_is_fail_closed(&parse(&investigated)),
            "a zero-applied population must not carry investigation evidence"
        );

        // And a measured aggregate stays trusted accuracy by another name at
        // any applied count.
        let measured = artifact(serde_json::json!({
            "state": "measured",
            "metric": "whitespace_invariance_rate",
            "value": 1.0,
            "sample_count": 4,
        }));
        assert!(
            !trust_disposition_is_fail_closed(&parse(&measured)),
            "a measured aggregate must fail closed at any applied count"
        );
    }

    #[test]
    fn fail_closed_reader_rejects_untyped_aggregates_and_stale_counts() {
        let population = valid_population();

        // The aggregate serialized as trusted measured evidence is rejected.
        let trusted_aggregate = artifact_with_rows(
            vec![ParserAccuracyMetricSummary::Measured {
                metric: "whitespace_invariance_rate".to_string(),
                value: 0.4,
                sample_count: 2,
            }],
            population.clone(),
        );
        assert!(
            !trust_disposition_is_fail_closed(&trusted_aggregate),
            "a legacy aggregate serialized as trusted measured evidence must fail closed"
        );

        // A retained population without any typed aggregate row is rejected.
        let missing_aggregate = artifact_with_rows(vec![], population.clone());
        assert!(
            !trust_disposition_is_fail_closed(&missing_aggregate),
            "a population without a typed aggregate row must fail closed"
        );

        // A stale aggregate count (from an older population) is rejected even
        // though the row itself is typed investigation evidence.
        let stale_aggregate = artifact_with_rows(
            vec![investigation_row("whitespace_invariance_rate", 0.4, 3)],
            population.clone(),
        );
        assert!(
            !trust_disposition_is_fail_closed(&stale_aggregate),
            "an aggregate from an older population must fail closed"
        );

        // An aggregate bound to a different profile than the retained
        // population is rejected.
        let mismatched_profile = artifact_with_rows(
            vec![investigation_row("whitespace_invariance_rate", 0.4, 2)],
            ParserAccuracyLegacyPopulation {
                transformation_profile: "newline_style.legacy.v1".to_string(),
                ..population.clone()
            },
        );
        assert!(
            !trust_disposition_is_fail_closed(&mismatched_profile),
            "an aggregate observed under another profile must fail closed"
        );
    }

    #[test]
    fn fail_closed_reader_rejects_unknown_dispositions_and_floor_admission() {
        let population = valid_population();

        for (label, row) in [
            (
                "unknown evidence class",
                investigation_row_with_disposition(
                    "whitespace_invariance_rate",
                    0.4,
                    2,
                    "trusted",
                    "not_proven",
                    "none",
                    false,
                ),
            ),
            (
                "unknown terminal disposition",
                investigation_row_with_disposition(
                    "whitespace_invariance_rate",
                    0.4,
                    2,
                    "investigation_only",
                    "pass",
                    "none",
                    false,
                ),
            ),
            (
                "non-none packet policy",
                investigation_row_with_disposition(
                    "whitespace_invariance_rate",
                    0.4,
                    2,
                    "investigation_only",
                    "not_proven",
                    "defect",
                    false,
                ),
            ),
            (
                "floor-admitted investigation row",
                investigation_row_with_disposition(
                    "whitespace_invariance_rate",
                    0.4,
                    2,
                    "investigation_only",
                    "not_proven",
                    "none",
                    true,
                ),
            ),
        ] {
            let artifact = artifact_with_rows(vec![row], population.clone());
            assert!(!trust_disposition_is_fail_closed(&artifact), "{label} must fail closed");
        }
    }
}
