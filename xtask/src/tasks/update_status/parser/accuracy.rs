//! Parser accuracy artifact types and status-row formatting.
//!
//! Holds the serde-deserializable structs for the JSON artifact produced by
//! `cargo xtask metrics parser-accuracy --json` plus the helper functions that
//! render those structs into status-doc rows.

use std::fs;
use std::path::Path;

use serde::Deserialize;

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
/// are not sha256-tagged digests, populations whose counts do not close, and
/// investigation rows that claim floor eligibility or packet emission all
/// reject the artifact instead of silently rendering trusted accuracy.
fn trust_disposition_is_fail_closed(artifact: &ParserAccuracyArtifactSummary) -> bool {
    let population = &artifact.legacy_population;
    let Some(digest) = population.population_identity.strip_prefix("sha256:") else {
        return false;
    };
    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return false;
    }
    if population.transformation_profile.is_empty()
        || population.aggregate_metric.is_empty()
        || population.manifest_schema_version == 0
        || population.population_total_count == 0
    {
        return false;
    }
    if population.population_applied_count + population.population_unclassified_count
        != population.population_total_count
    {
        return false;
    }

    let mut saw_aggregate_investigation = false;
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
            } => {
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
                    saw_aggregate_investigation = true;
                }
            }
            ParserAccuracyMetricSummary::Measured { metric, .. }
            | ParserAccuracyMetricSummary::InsufficientData { metric, .. }
                if metric == &population.aggregate_metric =>
            {
                return false;
            }
            ParserAccuracyMetricSummary::Measured { .. }
            | ParserAccuracyMetricSummary::InsufficientData { .. } => {}
        }
    }
    saw_aggregate_investigation
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
        parser_accuracy_metric_summary, trust_disposition_is_fail_closed,
    };

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
        let artifact_json = |legacy_population: serde_json::Value| {
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
                "metrics": [{
                    "state": "insufficient_data",
                    "metric": "line_construct_f1",
                    "reason": "line-level gold scorer is not wired yet",
                    "sample_count": 0,
                    "confidence": "low",
                }],
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

        // A missing population block must not deserialize to a readable artifact.
        let missing = artifact_json(serde_json::json!({}));
        assert!(
            serde_json::from_str::<ParserAccuracyArtifactSummary>(&missing).is_err(),
            "artifact without retained population evidence must fail closed"
        );

        // Counts that do not close over retained rows reject the artifact.
        let unclosed = artifact_json(population_json(serde_json::json!({
            "population_unclassified_count": 1,
        })));
        let unclosed: ParserAccuracyArtifactSummary = serde_json::from_str(&unclosed)
            .expect("artifact with a well-shaped population must deserialize");
        assert!(
            !trust_disposition_is_fail_closed(&unclosed),
            "population counts that do not close must fail closed"
        );

        // An identity that is not a sha256-tagged digest rejects the artifact.
        let untagged = artifact_json(population_json(serde_json::json!({
            "population_identity": "legacy-digest",
        })));
        let untagged: ParserAccuracyArtifactSummary = serde_json::from_str(&untagged)
            .expect("artifact with a well-shaped population must deserialize");
        assert!(
            !trust_disposition_is_fail_closed(&untagged),
            "an untagged population identity must fail closed"
        );

        // A zero population is not a valid retained population.
        let empty = artifact_json(population_json(serde_json::json!({
            "population_total_count": 0,
            "population_applied_count": 0,
            "population_unclassified_count": 0,
        })));
        let empty: ParserAccuracyArtifactSummary = serde_json::from_str(&empty)
            .expect("artifact with a well-shaped population must deserialize");
        assert!(
            !trust_disposition_is_fail_closed(&empty),
            "an empty retained population must fail closed"
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
