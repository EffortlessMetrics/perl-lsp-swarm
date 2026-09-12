//! Parser accuracy artifact types and status-row formatting.
//!
//! Holds the serde-deserializable structs for the JSON artifact produced by
//! `cargo xtask metrics parser-accuracy --json` plus the helpers that render
//! them into status-doc rows. Proof lives in the `tests`/`trust_tests` children.

use std::fs;
use std::path::Path;

use serde::Deserialize;

mod trust;
use trust::trust_disposition_is_fail_closed;

/// Fields the schema permits only on an `investigation_only` row.
///
/// `measuredMetric` and `insufficientDataMetric` both set
/// `additionalProperties: false` and list none of these, so a measured row
/// carrying `terminal_disposition` is schema-invalid. Those two variants are
/// projections here (3 of 13 and 3 of 5 schema fields), so they cannot use
/// `deny_unknown_fields` without rejecting legitimate keys like `confidence`
/// or `delta` — but they must still refuse a *contradictory trust claim*, which
/// would otherwise render as trusted accuracy.
pub(super) const FORBIDDEN_TRUST_FIELDS: [&str; 5] = [
    "evidence_class",
    "terminal_disposition",
    "packet_policy",
    "floor_eligible",
    "transformation_profile",
];

// Strictness here is deliberately per-object, not blanket.
//
// `.ci/schemas/parser-accuracy.schema.json` sets `additionalProperties: false`
// everywhere, but this reader is a *projection*: the artifact carries twelve
// top-level keys and status rendering needs eight, so the artifact and the
// family/measured/insufficient rows must stay permissive — `deny_unknown_fields`
// on them would reject every real artifact.
//
// The objects modeled *completely* are strict, because that is where the trust
// contract lives and where a stray field would smuggle a contradictory claim
// past the typed checks: `legacy_population` (7/7 schema fields) and
// `denominator` (13/13). The investigation row is complete too but cannot use
// the attribute — on an internally tagged enum it applies to every variant,
// including the two projections — so it is enforced via `unknown_fields` below.
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
    pub(super) failure_packets: Vec<ParserAccuracyFailurePacket>,
}

/// The one field of a failure packet the trust contract depends on.
///
/// The generator refuses a packet naming an investigation metric, because
/// investigation rows declare `packet_policy: none`. The reader held these as
/// opaque `serde_json::Value`, so it could not apply the same rule and would
/// report an active defect packet against a row that emits none.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct ParserAccuracyFailurePacket {
    #[serde(default)]
    pub(super) metric: Option<String>,
}

/// Retained identity of the quarantined legacy metamorphic population.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ParserAccuracyLegacyPopulation {
    pub(super) transformation_profile: String,
    pub(super) population_identity: String,
    pub(super) aggregate_metric: String,
    /// Every metric under legacy quarantine, not just the aggregate.
    ///
    /// The generator quarantines three legacy observations but `aggregate_metric`
    /// names only the whitespace one, so a rule keyed on that name let a measured
    /// `comment_invariance_rate` render as trusted accuracy. Enforcing a
    /// *declaration* keeps the name classifier this contract retired retired: the
    /// artifact says what is quarantined, and the reader holds it to that.
    pub(super) quarantined_metrics: Vec<String>,
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
        /// Schema fields this projection does not model, checked against
        /// [`FORBIDDEN_TRUST_FIELDS`] rather than rejected wholesale.
        #[serde(flatten)]
        unmodeled_fields: serde_json::Map<String, serde_json::Value>,
    },
    InsufficientData {
        metric: String,
        reason: String,
        sample_count: u64,
        /// As [`ParserAccuracyMetricSummary::Measured`].
        #[serde(flatten)]
        unmodeled_fields: serde_json::Map<String, serde_json::Value>,
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
        /// Anything the schema's `investigationOnlyMetric` does not permit, so
        /// a non-empty map is an artifact the schema rejects. `confidence` is
        /// legal on measured and insufficient rows but not here: without this a
        /// hand-edited row could carry a forbidden trust signal and still render.
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
        ParserAccuracyMetricSummary::Measured { metric, value, sample_count, .. } => {
            format!("{metric}={value:.1} (n={sample_count})")
        }
        ParserAccuracyMetricSummary::InsufficientData { metric, reason, sample_count, .. } => {
            format!("{metric}: insufficient_data ({reason}; n={sample_count})")
        }
    }
}

#[cfg(test)]
mod relational_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod trust_tests;
