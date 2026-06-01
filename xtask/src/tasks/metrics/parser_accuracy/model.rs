use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use color_eyre::eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ParserAccuracyManifest {
    pub(super) schema_version: u32,
    pub(super) fixtures: Vec<FixtureMetadata>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct GoldBaseline {
    pub(super) schema_version: u32,
    pub(super) expectation_signatures: BTreeSet<String>,
    #[serde(default)]
    pub(super) dynamic_expectation_signatures: Option<BTreeSet<String>>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct FixtureMetadata {
    pub(super) id: String,
    pub(super) family: String,
    pub(super) label_mode: LabelMode,
    pub(super) source_path: String,
    pub(super) scored_lines: u64,
    pub(super) scored_symbols: u64,
    pub(super) fully_labeled_regions: u64,
    pub(super) partial_labeled_regions: u64,
    pub(super) unknown_regions: u64,
    pub(super) negative_regions: u64,
    pub(super) dynamic_boundaries: u64,
    pub(super) unsupported_constructs: u64,
    pub(super) real_project_file: bool,
    pub(super) generated: bool,
    #[serde(default)]
    pub(super) line_expectations: Vec<LineExpectation>,
    #[serde(default)]
    pub(super) ast_expectations: Vec<AstExpectation>,
    #[serde(default)]
    pub(super) symbol_expectations: SymbolExpectations,
    #[serde(default)]
    pub(super) symbol_safety_regions: Vec<SymbolSafetyRegion>,
    #[serde(default)]
    pub(super) recovery_expectations: Vec<RecoveryExpectation>,
    #[serde(default)]
    pub(super) incremental_expectations: Vec<IncrementalExpectation>,
    #[serde(default)]
    pub(super) span_expectations: Vec<SpanExpectation>,
    #[serde(default)]
    pub(super) provider_expectations: ProviderExpectations,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum LabelMode {
    Full,
    Partial,
    Unknown,
    Negative,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(super) struct LineExpectation {
    pub(super) line: u64,
    pub(super) expected_tags: BTreeSet<LineTag>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(super) struct AstExpectation {
    pub(super) id: String,
    pub(super) kind: String,
    pub(super) line: u64,
    pub(super) span_text: String,
    pub(super) parent_kind: Option<String>,
    pub(super) depth: Option<u64>,
    pub(super) operator: Option<String>,
    pub(super) parent_operator: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AstPrediction {
    pub(super) kind: String,
    pub(super) line: u64,
    pub(super) span_text: String,
    pub(super) parent_kind: Option<String>,
    pub(super) depth: u64,
    pub(super) operator: Option<String>,
    pub(super) parent_operator: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct AstDelimiterPair {
    pub(super) open: char,
    pub(super) close: char,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub(super) struct ProviderExpectations {
    #[serde(default)]
    pub(super) method_completion: Vec<MethodCompletionProviderExpectation>,
    #[serde(default)]
    pub(super) diagnostics: Vec<DiagnosticProviderExpectation>,
    #[serde(default)]
    pub(super) navigation: Vec<NavigationProviderExpectation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(super) struct MethodCompletionProviderExpectation {
    pub(super) id: String,
    pub(super) cursor_marker: String,
    pub(super) expected_receiver_package: Option<String>,
    #[serde(default)]
    pub(super) expected_present: Vec<String>,
    #[serde(default)]
    pub(super) expected_absent: Vec<String>,
    pub(super) expected_fallback: bool,
    #[serde(default)]
    pub(super) import_visibility: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(super) struct DiagnosticProviderExpectation {
    pub(super) id: String,
    pub(super) expected_code: String,
    pub(super) message_contains: String,
    pub(super) expected_present: bool,
    #[serde(default)]
    pub(super) dynamic_boundary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(super) struct NavigationProviderExpectation {
    pub(super) id: String,
    pub(super) symbol: String,
    #[serde(default)]
    pub(super) cursor_marker: Option<String>,
    #[serde(default)]
    pub(super) cursor_symbol: Option<String>,
    #[serde(default)]
    pub(super) expected_document_symbols: Vec<String>,
    #[serde(default)]
    pub(super) expected_definition_span: Option<String>,
    #[serde(default)]
    pub(super) expected_references: Vec<String>,
    #[serde(default)]
    pub(super) unexpected_references: Vec<String>,
    #[serde(default)]
    pub(super) hover_contains: Vec<String>,
    #[serde(default)]
    pub(super) rename_new_name: Option<String>,
    #[serde(default)]
    pub(super) expected_rename_safe_edit: Option<bool>,
    #[serde(default)]
    pub(super) expected_rename_edit_count: Option<u64>,
    #[serde(default)]
    pub(super) expected_safe_delete_blocked: Option<bool>,
    #[serde(default)]
    pub(super) expected_safe_delete_blocker_count: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub(super) struct SymbolExpectations {
    #[serde(default)]
    pub(super) entities: Vec<SymbolEntityExpectation>,
    #[serde(default)]
    pub(super) occurrences: Vec<SymbolOccurrenceExpectation>,
    #[serde(default)]
    pub(super) edges: Vec<SymbolEdgeExpectation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(super) struct SymbolEntityExpectation {
    pub(super) id: String,
    pub(super) kind: String,
    pub(super) canonical_name: String,
    pub(super) span_text: String,
    pub(super) package: Option<String>,
    pub(super) scope: Option<String>,
    pub(super) provenance: String,
    pub(super) confidence: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(super) struct SymbolOccurrenceExpectation {
    pub(super) id: String,
    pub(super) kind: String,
    pub(super) canonical_name: Option<String>,
    pub(super) span_text: String,
    pub(super) package: Option<String>,
    pub(super) scope: Option<String>,
    pub(super) provenance: String,
    pub(super) confidence: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(super) struct SymbolEdgeExpectation {
    pub(super) id: String,
    pub(super) kind: String,
    pub(super) from: String,
    pub(super) to: String,
    pub(super) provenance: String,
    pub(super) confidence: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(super) struct SymbolSafetyRegion {
    pub(super) kind: SymbolSafetyRegionKind,
    pub(super) line: u64,
    pub(super) span_text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum SymbolSafetyRegionKind {
    Comment,
    Pod,
    String,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(super) struct RecoveryExpectation {
    pub(super) id: String,
    pub(super) first_error_line: u64,
    pub(super) error_region: LineRange,
    pub(super) recovery_line: u64,
    #[serde(default)]
    pub(super) post_error_line_expectations: Vec<LineExpectation>,
    #[serde(default)]
    pub(super) post_error_symbol_spans: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub(super) struct LineRange {
    pub(super) start: u64,
    pub(super) end: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(super) struct IncrementalExpectation {
    pub(super) id: String,
    #[serde(default)]
    pub(super) edits: Vec<IncrementalEditExpectation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(super) struct IncrementalEditExpectation {
    pub(super) old_text: String,
    pub(super) new_text: String,
    #[serde(default)]
    pub(super) occurrence: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(super) struct SpanExpectation {
    pub(super) id: String,
    pub(super) span_text: String,
    #[serde(default)]
    pub(super) occurrence: Option<u64>,
    pub(super) byte_start: usize,
    pub(super) byte_end: usize,
    pub(super) line_start: u64,
    pub(super) line_end: u64,
    pub(super) utf16_start: SpanPositionExpectation,
    pub(super) utf16_end: SpanPositionExpectation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub(super) struct SpanPositionExpectation {
    pub(super) line: u32,
    pub(super) character: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct SymbolEntityKey {
    pub(super) kind: String,
    pub(super) canonical_name: String,
    pub(super) span_text: String,
    pub(super) package: Option<String>,
    pub(super) scope: Option<String>,
    pub(super) provenance: String,
    pub(super) confidence: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct SymbolOccurrenceKey {
    pub(super) kind: String,
    pub(super) canonical_name: Option<String>,
    pub(super) span_text: String,
    pub(super) package: Option<String>,
    pub(super) scope: Option<String>,
    pub(super) provenance: String,
    pub(super) confidence: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct SymbolEdgeKey {
    pub(super) kind: String,
    pub(super) from: String,
    pub(super) to: String,
    pub(super) provenance: String,
    pub(super) confidence: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct SymbolPredictions {
    pub(super) entities: BTreeSet<SymbolEntityKey>,
    pub(super) occurrences: BTreeSet<SymbolOccurrenceKey>,
    pub(super) safety_spans: BTreeSet<SymbolSpanLocation>,
    pub(super) edges: BTreeSet<SymbolEdgeKey>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct SymbolSpanLocation {
    pub(super) line: u64,
    pub(super) span_text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum LineTag {
    PackageDecl,
    SubDecl,
    MethodDecl,
    VariableDecl,
    Import,
    Export,
    FunctionCall,
    MethodCall,
    Regex,
    QuoteLike,
    HeredocOpener,
    HeredocBody,
    HeredocTerminator,
    Pod,
    FormatDecl,
    GivenWhen,
    DoWhile,
    UntilLoop,
    DynamicBoundary,
    ParseError,
    RecoveryRegion,
    UnsupportedConstruct,
}

pub(super) const LINE_TAG_VOCABULARY: &[LineTag] = &[
    LineTag::PackageDecl,
    LineTag::SubDecl,
    LineTag::MethodDecl,
    LineTag::VariableDecl,
    LineTag::Import,
    LineTag::Export,
    LineTag::FunctionCall,
    LineTag::MethodCall,
    LineTag::Regex,
    LineTag::QuoteLike,
    LineTag::HeredocOpener,
    LineTag::HeredocBody,
    LineTag::HeredocTerminator,
    LineTag::Pod,
    LineTag::FormatDecl,
    LineTag::GivenWhen,
    LineTag::DoWhile,
    LineTag::UntilLoop,
    LineTag::DynamicBoundary,
    LineTag::ParseError,
    LineTag::RecoveryRegion,
    LineTag::UnsupportedConstruct,
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct ParserAccuracyArtifact {
    pub(super) schema_version: u32,
    pub(super) subsystem: String,
    pub(super) generated_at: String,
    pub(super) commit: String,
    pub(super) cadence: Cadence,
    pub(super) denominator: Denominator,
    pub(super) families: Vec<FamilySummary>,
    pub(super) metrics: Vec<MetricRow>,
    pub(super) failure_packets: Vec<FailurePacket>,
    pub(super) gold_drift: GoldDrift,
    pub(super) metric_runtime: MetricRuntime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Cadence {
    Pr,
    MergeGate,
    Nightly,
    Release,
}

impl Cadence {
    pub(super) fn parse(value: &str) -> Result<Self> {
        match value {
            "pr" => Ok(Self::Pr),
            "merge_gate" => Ok(Self::MergeGate),
            "nightly" => Ok(Self::Nightly),
            "release" => Ok(Self::Release),
            other => bail!("unsupported parser accuracy cadence '{other}'"),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Denominator {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct FamilySummary {
    pub(super) family: String,
    pub(super) fixture_count: u64,
    pub(super) label_modes: Vec<LabelMode>,
    pub(super) scored_line_count: u64,
    pub(super) scored_symbol_count: u64,
    pub(super) dynamic_boundary_case_count: u64,
    pub(super) unsupported_construct_case_count: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub(super) enum MetricRow {
    Measured {
        metric: String,
        value: f64,
        #[serde(skip_serializing_if = "Option::is_none")]
        previous: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        delta: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        floor: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        threshold: Option<f64>,
        sample_count: u64,
        direction: Direction,
        confidence: Confidence,
        cadence: Cadence,
        #[serde(skip_serializing_if = "Option::is_none")]
        macro_value: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        micro_value: Option<f64>,
    },
    InsufficientData {
        metric: String,
        reason: String,
        sample_count: u64,
        confidence: Confidence,
    },
}

impl MetricRow {
    pub(super) fn name(&self) -> &str {
        match self {
            MetricRow::Measured { metric, .. } | MetricRow::InsufficientData { metric, .. } => {
                metric
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Direction {
    Up,
    Down,
    Flat,
    Neutral,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Confidence {
    High,
    Low,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct FailurePacket {
    pub(super) failure_kind: String,
    pub(super) likely_layer: String,
    pub(super) fixture_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) family: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) metric: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) line: Option<u64>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub(super) expected: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub(super) actual: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub(super) nearest_predictions: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) source_excerpt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) details: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) suggested_next_fix: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct GoldDrift {
    pub(super) schema_error_count: u64,
    pub(super) span_error_count: u64,
    pub(super) duplicate_symbol_id_count: u64,
    pub(super) missing_resolves_to_target_count: u64,
    pub(super) changed_line_count: u64,
    pub(super) changed_line_sample_count: u64,
    pub(super) changed_symbol_count: u64,
    pub(super) changed_symbol_sample_count: u64,
    pub(super) removed_expectation_count: u64,
    pub(super) removed_expectation_sample_count: u64,
    pub(super) added_expectation_count: u64,
    pub(super) added_expectation_sample_count: u64,
    pub(super) dynamic_expectation_change_count: u64,
    pub(super) dynamic_expectation_sample_count: u64,
    pub(super) weakening_explanation_required_count: u64,
    pub(super) weakening_explanation_sample_count: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(super) struct MetricRuntime {
    pub(super) runtime_ms: f64,
    pub(super) timeout_count: u64,
    pub(super) flake_count: u64,
    pub(super) artifact_size_bytes: u64,
    pub(super) ci_runner_failure_count: u64,
    pub(super) orphan_process_count: u64,
    pub(super) cache_hit_rate: Option<f64>,
    #[serde(skip_serializing_if = "is_zero_u64")]
    pub(super) cache_sample_count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) peak_rss_mb: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) allocated_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) allocation_count: Option<u64>,
}

pub(super) fn is_zero_u64(value: &u64) -> bool {
    *value == 0
}

#[derive(Debug, Default)]
pub(super) struct MetricSourceCache {
    pub(super) sources: BTreeMap<PathBuf, String>,
    pub(super) hit_count: u64,
    pub(super) miss_count: u64,
}

impl MetricSourceCache {
    pub(super) fn read<'a>(&'a mut self, path: &Path, label: &str) -> Result<&'a str> {
        let key = path.to_path_buf();
        match self.sources.entry(key) {
            std::collections::btree_map::Entry::Occupied(entry) => {
                self.hit_count += 1;
                Ok(entry.into_mut().as_str())
            }
            std::collections::btree_map::Entry::Vacant(entry) => {
                self.miss_count += 1;
                let source = fs::read_to_string(path).with_context(|| {
                    format!("reading parser accuracy {label} {}", path.display())
                })?;
                Ok(entry.insert(source).as_str())
            }
        }
    }

    pub(super) fn hit_rate(&self) -> Option<f64> {
        let total = self.hit_count + self.miss_count;
        if total == 0 { None } else { Some(self.hit_count as f64 / total as f64) }
    }

    pub(super) fn sample_count(&self) -> u64 {
        self.hit_count + self.miss_count
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct LineScore {
    pub(super) line_count: u64,
    pub(super) true_positive_count: u64,
    pub(super) false_positive_count: u64,
    pub(super) false_negative_count: u64,
    pub(super) exact_match_count: u64,
    pub(super) expected_parse_error_count: u64,
    pub(super) false_parse_error_count: u64,
    pub(super) missed_parse_error_count: u64,
    pub(super) expected_dynamic_boundary_count: u64,
    pub(super) correct_dynamic_boundary_count: u64,
    pub(super) expected_unsupported_construct_count: u64,
    pub(super) correct_unsupported_construct_count: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct AstScore {
    pub(super) expected_node_count: u64,
    pub(super) predicted_node_count: u64,
    pub(super) node_kind_true_positive_count: u64,
    pub(super) node_kind_false_positive_count: u64,
    pub(super) node_kind_false_negative_count: u64,
    pub(super) span_exact_count: u64,
    pub(super) span_near_count: u64,
    pub(super) parent_child_expected_count: u64,
    pub(super) parent_child_correct_count: u64,
    pub(super) tree_depth_expected_count: u64,
    pub(super) tree_depth_correct_count: u64,
    pub(super) operator_precedence_expected_count: u64,
    pub(super) operator_precedence_correct_count: u64,
    pub(super) delimiter_pairing_expected_count: u64,
    pub(super) delimiter_pairing_correct_count: u64,
    pub(super) unexpected_error_node_count: u64,
    pub(super) missing_expected_node_count: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct SymbolScore {
    pub(super) entity_expected_count: u64,
    pub(super) entity_predicted_count: u64,
    pub(super) entity_true_positive_count: u64,
    pub(super) entity_false_positive_count: u64,
    pub(super) entity_false_negative_count: u64,
    pub(super) occurrence_expected_count: u64,
    pub(super) occurrence_predicted_count: u64,
    pub(super) occurrence_true_positive_count: u64,
    pub(super) occurrence_false_positive_count: u64,
    pub(super) occurrence_false_negative_count: u64,
    pub(super) edge_expected_count: u64,
    pub(super) edge_predicted_count: u64,
    pub(super) edge_true_positive_count: u64,
    pub(super) edge_false_positive_count: u64,
    pub(super) edge_false_negative_count: u64,
    pub(super) entity_by_kind: BTreeMap<String, KindScore>,
    pub(super) occurrence_by_kind: BTreeMap<String, KindScore>,
    pub(super) false_positive_sample_count: u64,
    pub(super) false_import_count: u64,
    pub(super) false_export_count: u64,
    pub(super) false_exact_resolution_count: u64,
    pub(super) false_dynamic_resolution_count: u64,
    pub(super) dynamic_false_precision_count: u64,
    pub(super) dynamic_false_precision_sample_count: u64,
    pub(super) comment_safety_region_count: u64,
    pub(super) pod_safety_region_count: u64,
    pub(super) string_safety_region_count: u64,
    pub(super) unknown_safety_region_count: u64,
    pub(super) symbols_emitted_in_comments: u64,
    pub(super) symbols_emitted_in_pod: u64,
    pub(super) symbols_emitted_in_strings: u64,
    pub(super) symbols_emitted_in_unknown_regions: u64,
    pub(super) proof_score: ProofScore,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct ProofScore {
    pub(super) true_positive_by_bucket: BTreeMap<ProofBucket, u64>,
    pub(super) predicted_by_bucket: BTreeMap<ProofBucket, u64>,
    pub(super) high_confidence_false_positive_count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum ProofBucket {
    Exact,
    High,
    Medium,
    Low,
    Heuristic,
    Dynamic,
}

impl ProofBucket {
    pub(super) fn precision_metric(self) -> &'static str {
        match self {
            ProofBucket::Exact => "exact_fact_precision",
            ProofBucket::High => "high_confidence_precision",
            ProofBucket::Medium => "medium_confidence_precision",
            ProofBucket::Low => "low_confidence_precision",
            ProofBucket::Heuristic => "heuristic_fact_precision",
            ProofBucket::Dynamic => "dynamic_boundary_precision",
        }
    }

    pub(super) fn insufficient_reason(self) -> &'static str {
        match self {
            ProofBucket::Exact => {
                "no exact fact predictions are available in fully labeled fixtures"
            }
            ProofBucket::High => {
                "no high-confidence fact predictions are available in fully labeled fixtures"
            }
            ProofBucket::Medium => {
                "no medium-confidence fact predictions are available in fully labeled fixtures"
            }
            ProofBucket::Low => {
                "no low-confidence fact predictions are available in fully labeled fixtures"
            }
            ProofBucket::Heuristic => {
                "no heuristic fact predictions are available in fully labeled fixtures"
            }
            ProofBucket::Dynamic => {
                "no dynamic-boundary fact predictions are available in fully labeled fixtures"
            }
        }
    }
}

pub(super) trait ProofShape {
    fn provenance(&self) -> &str;
    fn confidence(&self) -> &str;
}

impl ProofShape for SymbolEntityKey {
    fn provenance(&self) -> &str {
        &self.provenance
    }

    fn confidence(&self) -> &str {
        &self.confidence
    }
}

impl ProofShape for SymbolOccurrenceKey {
    fn provenance(&self) -> &str {
        &self.provenance
    }

    fn confidence(&self) -> &str {
        &self.confidence
    }
}

impl ProofShape for SymbolEdgeKey {
    fn provenance(&self) -> &str {
        &self.provenance
    }

    fn confidence(&self) -> &str {
        &self.confidence
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct KindScore {
    pub(super) expected_count: u64,
    pub(super) predicted_count: u64,
    pub(super) true_positive_count: u64,
    pub(super) false_positive_count: u64,
    pub(super) false_negative_count: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct RecoveryScore {
    pub(super) expectation_count: u64,
    pub(super) first_error_line_correct_count: u64,
    pub(super) error_region_true_positive_count: u64,
    pub(super) error_region_false_positive_count: u64,
    pub(super) error_region_false_negative_count: u64,
    pub(super) spillover_lines: Vec<u64>,
    pub(super) recovery_parse_micros: Vec<u64>,
    pub(super) post_error_line_score: LineScore,
    pub(super) post_error_symbol_expected_count: u64,
    pub(super) post_error_symbol_found_count: u64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct IncrementalScore {
    pub(super) expectation_count: u64,
    pub(super) full_parse_equivalent_count: u64,
    pub(super) edit_apply_equivalent_count: u64,
    pub(super) no_panic_count: u64,
    pub(super) no_progress_count: u64,
    pub(super) timeout_count: u64,
    pub(super) fallback_count: u64,
    pub(super) checkpoint_hit_count: u64,
    pub(super) checkpoint_miss_count: u64,
    pub(super) content_hash_hit_count: u64,
    pub(super) content_hash_miss_count: u64,
    pub(super) semantic_fact_cache_hit_count: u64,
    pub(super) semantic_fact_cache_miss_count: u64,
    pub(super) workspace_shard_reuse_count: u64,
    pub(super) workspace_shard_replacement_attempt_count: u64,
    pub(super) unchanged_file_skip_count: u64,
    pub(super) unchanged_file_index_attempt_count: u64,
    pub(super) reparse_byte_ratios: Vec<f64>,
    pub(super) reused_token_ratios: Vec<f64>,
    pub(super) reused_node_ratios: Vec<f64>,
    pub(super) changed_range_sample_count: u64,
    pub(super) changed_range_correct_count: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct SpanScore {
    pub(super) expectation_count: u64,
    pub(super) byte_exact_count: u64,
    pub(super) line_exact_count: u64,
    pub(super) utf16_exact_count: u64,
    pub(super) near_count: u64,
    pub(super) invalid_count: u64,
    pub(super) out_of_bounds_count: u64,
    pub(super) inverted_count: u64,
    pub(super) non_char_boundary_count: u64,
    pub(super) crlf_sample_count: u64,
    pub(super) crlf_position_error_count: u64,
    pub(super) unicode_sample_count: u64,
    pub(super) unicode_position_error_count: u64,
    pub(super) tab_sample_count: u64,
    pub(super) tab_column_mismatch_count: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct UnsupportedScore {
    pub(super) manifest_construct_count: u64,
    pub(super) family_count: u64,
    pub(super) line_labeled_construct_count: u64,
    pub(super) detected_count: u64,
    pub(super) salvaged_count: u64,
    pub(super) false_exact_count: u64,
    pub(super) false_exact_sample_count: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct MethodCompletionProviderScore {
    pub(super) receiver_expected_count: u64,
    pub(super) receiver_hit_count: u64,
    pub(super) fallback_expected_count: u64,
    pub(super) fallback_correct_count: u64,
    pub(super) false_receiver_count: u64,
    pub(super) relevance_assertion_count: u64,
    pub(super) relevance_assertion_correct_count: u64,
    pub(super) import_visibility_expected_count: u64,
    pub(super) import_visibility_correct_count: u64,
    pub(super) completion_query_micros: Vec<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct DiagnosticProviderScore {
    pub(super) dynamic_boundary_expected_absent_count: u64,
    pub(super) dynamic_boundary_false_positive_count: u64,
    pub(super) undefined_expected_absent_count: u64,
    pub(super) undefined_false_positive_count: u64,
    pub(super) undefined_expected_present_count: u64,
    pub(super) undefined_false_negative_count: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct NavigationProviderScore {
    pub(super) document_symbol_expected_count: u64,
    pub(super) document_symbol_returned_count: u64,
    pub(super) document_symbol_span_exact_count: u64,
    pub(super) goto_definition_expected_count: u64,
    pub(super) goto_definition_hit_count: u64,
    pub(super) goto_definition_span_exact_count: u64,
    pub(super) goto_definition_false_target_count: u64,
    pub(super) definition_query_micros: Vec<u64>,
    pub(super) references_expected_count: u64,
    pub(super) references_hit_count: u64,
    pub(super) references_returned_count: u64,
    pub(super) references_false_positive_count: u64,
    pub(super) references_absent_assertion_count: u64,
    pub(super) reference_query_micros: Vec<u64>,
    pub(super) hover_expected_count: u64,
    pub(super) hover_origin_correct_count: u64,
    pub(super) rename_safe_edit_expected_count: u64,
    pub(super) rename_safe_edit_correct_count: u64,
    pub(super) safe_delete_blocker_expected_count: u64,
    pub(super) safe_delete_blocker_correct_count: u64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct ScaleCostScore {
    pub(super) fixture_count: u64,
    pub(super) file_bytes: u64,
    pub(super) source_lines: u64,
    pub(super) token_count: u64,
    pub(super) ast_node_count: u64,
    pub(super) symbol_count: u64,
    pub(super) import_count: u64,
    pub(super) export_count: u64,
    pub(super) sub_count: u64,
    pub(super) package_count: u64,
    pub(super) max_nesting_depth: u64,
    pub(super) max_brace_depth: u64,
    pub(super) max_regex_length: u64,
    pub(super) max_heredoc_body_bytes: u64,
    pub(super) quote_like_count: u64,
    pub(super) dynamic_boundary_count: u64,
    pub(super) lex_ms: Vec<f64>,
    pub(super) parse_ms: Vec<f64>,
    pub(super) ast_projection_ms: Vec<f64>,
    pub(super) semantic_extraction_ms: Vec<f64>,
    pub(super) workspace_insert_ms: Vec<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct DeterminismScore {
    pub(super) fixture_count: u64,
    pub(super) token_stream_stable_count: u64,
    pub(super) parse_hash_stable_count: u64,
    pub(super) ast_hash_stable_count: u64,
    pub(super) semantic_fact_hash_stable_count: u64,
    pub(super) diagnostic_hash_stable_count: u64,
    pub(super) repeated_parse_stable_count: u64,
    pub(super) whitespace_invariance_stable_count: u64,
    pub(super) whitespace_invariance_sample_count: u64,
    pub(super) comment_invariance_stable_count: u64,
    pub(super) comment_invariance_sample_count: u64,
    pub(super) newline_style_invariance_stable_count: u64,
    pub(super) newline_style_invariance_sample_count: u64,
}
