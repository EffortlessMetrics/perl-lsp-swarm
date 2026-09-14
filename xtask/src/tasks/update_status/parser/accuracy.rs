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
    #[serde(default)]
    pub(super) failure_packets: Vec<serde_json::Value>,
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
    Measured { metric: String, value: f64, sample_count: u64 },
    InsufficientData { metric: String, reason: String, sample_count: u64 },
}

impl ParserAccuracyMetricSummary {
    fn name(&self) -> &str {
        match self {
            ParserAccuracyMetricSummary::Measured { metric, .. }
            | ParserAccuracyMetricSummary::InsufficientData { metric, .. } => metric,
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
    Some(artifact)
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
        .filter(|metric| {
            matches!(
                metric,
                ParserAccuracyMetricSummary::Measured { metric, .. }
                    if !is_legacy_untrusted_metric(metric)
            )
        })
        .count();
    let selected_trusted_measured_count = selected_metrics
        .iter()
        .filter(|metric| {
            matches!(
                metric,
                ParserAccuracyMetricSummary::Measured { metric, .. }
                    if !is_legacy_untrusted_metric(metric)
            )
        })
        .count();
    let investigation_count = metrics
        .iter()
        .filter(|metric| {
            matches!(
                metric,
                ParserAccuracyMetricSummary::Measured { metric, .. }
                    if is_legacy_untrusted_metric(metric)
            )
        })
        .count();
    let selected_investigation_count = selected_metrics
        .iter()
        .filter(|metric| {
            matches!(
                metric,
                ParserAccuracyMetricSummary::Measured { metric, .. }
                    if is_legacy_untrusted_metric(metric)
            )
        })
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
        ParserAccuracyMetricSummary::Measured { metric, value, sample_count }
            if is_legacy_untrusted_metric(metric) =>
        {
            let transformation = match metric.as_str() {
                "whitespace_invariance_rate" => "trailing whitespace",
                "comment_invariance_rate" => "EOF comment",
                "newline_style_invariance_rate" => "LF-to-CRLF",
                _ => "legacy metamorphic transform",
            };
            format!(
                "{metric}: investigation_only (legacy_oracle_untrusted; {transformation}; observed={value:.1}; n={sample_count})"
            )
        }
        ParserAccuracyMetricSummary::Measured { metric, value, sample_count } => {
            format!("{metric}={value:.1} (n={sample_count})")
        }
        ParserAccuracyMetricSummary::InsufficientData { metric, reason, sample_count } => {
            format!("{metric}: insufficient_data ({reason}; n={sample_count})")
        }
    }
}

fn is_legacy_untrusted_metric(metric: &str) -> bool {
    matches!(
        metric,
        "whitespace_invariance_rate" | "comment_invariance_rate" | "newline_style_invariance_rate"
    )
}

#[cfg(test)]
mod tests {
    use super::{
        ParserAccuracyMetricSummary, is_legacy_untrusted_metric, parser_accuracy_metric_summary,
    };

    #[test]
    fn legacy_metamorphic_hash_rows_render_as_investigation_only() {
        let summary = parser_accuracy_metric_summary(&[
            ParserAccuracyMetricSummary::Measured {
                metric: "line_construct_f1".to_string(),
                value: 1.0,
                sample_count: 125,
            },
            ParserAccuracyMetricSummary::Measured {
                metric: "whitespace_invariance_rate".to_string(),
                value: 0.4,
                sample_count: 46,
            },
            ParserAccuracyMetricSummary::Measured {
                metric: "comment_invariance_rate".to_string(),
                value: 1.0,
                sample_count: 46,
            },
        ]);

        assert!(summary.contains("line_construct_f1=1.0 (n=125)"));
        assert!(summary.contains(
            "whitespace_invariance_rate: investigation_only (legacy_oracle_untrusted; trailing whitespace; observed=0.4; n=46)"
        ));
        assert!(summary.contains("1 additional investigation_only rows"));
        assert!(!summary.contains("whitespace_invariance_rate=0.4"));
    }

    #[test]
    fn legacy_metamorphic_metric_classifier_is_closed() {
        for metric in [
            "whitespace_invariance_rate",
            "comment_invariance_rate",
            "newline_style_invariance_rate",
        ] {
            assert!(is_legacy_untrusted_metric(metric));
        }
        assert!(!is_legacy_untrusted_metric("repeated_parse_stability_rate"));
        assert!(!is_legacy_untrusted_metric("line_construct_f1"));
    }
}
