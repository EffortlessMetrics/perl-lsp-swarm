Warning: truncated output (original token count: 90406)
Total output lines: 9682

//! Parser accuracy scorecard contract and denominator inventory.
//!
//! The implementation starts with denominator rows and then adds accuracy
//! scoring layers in small, schema-valid slices.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Instant;

use chrono::Utc;
use color_eyre::eyre::{Context, Result, bail, eyre};
use perl_lsp_rs_core::providers::completion::CompletionProvider;
use perl_lsp_rs_core::providers::diagnostics::{Diagnostic, DiagnosticsProvider};
use perl_lsp_rs_core::providers::navigation::definition_shadow::{
    DefinitionCutoverResult, goto_definition_cutover,
};
use perl_lsp_rs_core::providers::navigation::hover_shadow::{HoverCutoverResult, hover_cutover};
use perl_lsp_rs_core::providers::navigation::references_shadow::{
    ReferencesCutoverResult, find_references_cutover,
};
use perl_lsp_rs_core::providers::navigation::rename_shadow::{RenameCutoverResult, rename_cutover};
use perl_lsp_rs_core::providers::navigation::safe_delete_shadow::{
    SafeDeleteCutoverResult, safe_delete_cutover,
};
use perl_parser::apply_edits;
use perl_parser::edit::Edit as CoreEdit;
use perl_parser::incremental_v2::IncrementalParserV2;
use perl_parser::position::Position;
use perl_parser::{
    Edit as TextEdit, IncrementalState, Node, NodeKind, ParseError, Parser, PositionMapper,
    TokenKind, TokenStream,
};
use perl_semantic_facts::{AnchorFact, AnchorId, EntityFact, EntityId, EntityKind, Provenance};
use perl_workspace::position::Range;
use perl_workspace::semantic::queries::QueryContext;
use perl_workspace::workspace::document_store::DocumentStore;
use perl_workspace::workspace::workspace_index::{FileFactShard, WorkspaceIndex};
use serde::{Deserialize, Serialize};

use crate::allocation_tracker::{get_current_memory_usage, measure_allocations};
use crate::tasks::metrics::ratchet::MetricReceipt;
use crate::utils::project_root;

mod failure_packet;

const DEFAULT_MANIFEST: &str = "crates/perl-corpus/fixtures/parser_accuracy/manifest.json";
const DEFAULT_OUTPUT: &str = "target/metrics/parser_accuracy.json";
const DEFAULT_RATCHET_RECEIPT: &str = "target/receipts/metrics/parser_accuracy.json";
const GOLD_BASELINE: &str = ".ci/metrics/baselines/parser_accuracy_gold.json";
const FAILURE_PACKET_STATUS_RECEIPT: &str =
    "docs/project/status/parser_accuracy_failure_packets.json";
const FIXTURE_INVENTORY_STATUS_RECEIPT: &str =
    "docs/project/status/parser_accuracy_fixture_inventory.json";
const FAILURE_WORKLIST_STATUS_RECEIPT: &str =
    "docs/project/status/parser_accuracy_failure_worklist.md";
const NEXT_POINTER_STATUS_RECEIPT: &str = "docs/project/status/parser_accuracy_next.md";
const SAFETY_FLOOR_METRICS: &[(&str, f64)] =
    &[("dynamic_false_precision_count", 0.0), ("fast_path_wrong_result_count", 0.0)];
const DEFERRED_PRECISION_RECALL_CANDIDATES: &[&str] = &[
    "line_construct_precision",
    "line_construct_recall",
    "line_construct_f1",
    "ast_node_kind_precision",
    "ast_node_kind_recall",
    "ast_node_kind_f1",
    "symbol_decl_precision",
    "symbol_decl_recall",
    "symbol_decl_f1",
    "symbol_ref_precision",
    "symbol_ref_recall",
    "symbol_ref_f1",
    "symbol_edge_precision",
    "symbol_edge_recall",
    "symbol_edge_f1",
];

#[derive(Debug, Clone, Deserialize)]
struct ParserAccuracyManifest {
    schema_version: u32,
    fixtures: Vec<FixtureMetadata>,
}

#[derive(Debug, Clone, Deserialize)]
struct GoldBaseline {
    schema_version: u32,
    expectation_signatures: BTreeSet<String>,
    #[serde(default)]
    dynamic_expectation_signatures: Option<BTreeSet<String>>,
}

#[derive(Debug, Clone, Deserialize)]
struct FixtureMetadata {
    id: String,
    family: String,
    label_mode: LabelMode,
    source_path: String,
    scored_lines: u64,
    scored_symbols: u64,
    fully_labeled_regions: u64,
    partial_labeled_regions: u64,
    unknown_regions: u64,
    negative_regions: u64,
    dynamic_boundaries: u64,
    unsupported_constructs: u64,
    real_project_file: bool,
    generated: bool,
    #[serde(default)]
    line_expectations: Vec<LineExpectation>,
    #[serde(default)]
    ast_expectations: Vec<AstExpectation>,
    #[serde(default)]
    forbidden_nodes: Vec<ForbiddenNode>,
    #[serde(default)]
    symbol_expectations: SymbolExpectations,
    #[serde(default)]
    symbol_safety_regions: Vec<SymbolSafetyRegion>,
    #[serde(default)]
    recovery_expectations: Vec<RecoveryExpectation>,
    #[serde(default)]
    incremental_expectations: Vec<IncrementalExpectation>,
    #[serde(default)]
    span_expectations: Vec<SpanExpectation>,
    #[serde(default)]
    provider_expectations: ProviderExpectations,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum LabelMode {
    Full,
    Partial,
    Unknown,
    Negative,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct LineExpectation {
    line: u64,
    expected_tags: BTreeSet<LineTag>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct AstExpectation {
    id: String,
    kind: String,
    line: u64,
    span_text: String,
    parent_kind: Option<String>,
    depth: Option<u64>,
    operator: Option<String>,
    parent_operator: Option<String>,
}

/// A node shape asserted absent by `parser_accuracy_e2e`.
///
/// The metric does not score these — a forbidden node contributes no prediction
/// and no expectation. It is modelled here so the gold-drift audit can see its
/// id: without that, deleting a negative assertion is invisible, because the
/// fixture keeps its positive expectations and stays non-hollow.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct ForbiddenNode {
    id: String,
    #[allow(dead_code)]
    kind: String,
    #[allow(dead_code)]
    line: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AstPrediction {
    kind: String,
    line: u64,
    span_text: String,
    parent_kind: Option<String>,
    depth: u64,
    operator: Option<String>,
    parent_operator: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AstDelimiterPair {
    open: char,
    close: char,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
struct ProviderExpectations {
    #[serde(default)]
    method_completion: Vec<MethodCompletionProviderExpectation>,
    #[serde(default)]
    diagnostics: Vec<DiagnosticProviderExpectation>,
    #[serde(default)]
    navigation: Vec<NavigationProviderExpectation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct MethodCompletionProviderExpectation {
    id: String,
    cursor_marker: String,
    expected_receiver_package: Option<String>,
    #[serde(default)]
    expected_present: Vec<String>,
    #[serde(default)]
    expected_absent: Vec<String>,
    expected_fallback: bool,
    #[serde(default)]
    import_visibility: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct DiagnosticProviderExpectation {
    id: String,
    expected_code: String,
    message_contains: String,
    expected_present: bool,
    #[serde(default)]
    dynamic_boundary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct NavigationProviderExpectation {
    id: String,
    symbol: String,
    #[serde(default)]
    cursor_marker: Option<String>,
    #[serde(default)]
    cursor_symbol: Option<String>,
    #[serde(default)]
    expected_document_symbols: Vec<String>,
    #[serde(default)]
    expected_definition_span: Option<String>,
    #[serde(default)]
    expected_references: Vec<String>,
    #[serde(default)]
    unexpected_references: Vec<String>,
    #[serde(default)]
    hover_contains: Vec<String>,
    #[serde(default)]
    rename_new_name: Option<String>,
    #[serde(default)]
    expected_rename_safe_edit: Option<bool>,
    #[serde(default)]
    expected_rename_edit_count: Option<u64>,
    #[serde(default)]
    expected_safe_delete_blocked: Option<bool>,
    #[serde(default)]
    expected_safe_delete_blocker_count: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
struct SymbolExpectations {
    #[serde(default)]
    entities: Vec<SymbolEntityExpectation>,
    #[serde(default)]
    occurrences: Vec<SymbolOccurrenceExpectation>,
    #[serde(default)]
    edges: Vec<SymbolEdgeExpectation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct SymbolEntityExpectation {
    id: String,
    kind: String,
    canonical_name: String,
    span_text: String,
    package: Option<String>,
    scope: Option<String>,
    provenance: String,
    confidence: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct SymbolOccurrenceExpectation {
    id: String,
    kind: String,
    canonical_name: Option<String>,
    span_text: String,
    package: Option<String>,
    scope: Option<String>,
    provenance: String,
    confidence: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct SymbolEdgeExpectation {
    id: String,
    kind: String,
    from: String,
    to: String,
    provenance: String,
    confidence: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct SymbolSafetyRegion {
    kind: SymbolSafetyRegionKind,
    line: u64,
    span_text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SymbolSafetyRegionKind {
    Comment,
    Pod,
    String,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct RecoveryExpectation {
    id: String,
    first_error_line: u64,
    error_region: LineRange,
    recovery_line: u64,
    #[serde(default)]
    post_error_line_expectations: Vec<LineExpectation>,
    #[serde(default)]
    post_error_symbol_spans: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
struct LineRange {
    start: u64,
    end: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct IncrementalExpectation {
    id: String,
    #[serde(default)]
    edits: Vec<IncrementalEditExpectation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct IncrementalEditExpectation {
    old_text: String,
    new_text: String,
    #[serde(default)]
    occurrence: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct SpanExpectation {
    id: String,
    span_text: String,
    #[serde(default)]
    occurrence: Option<u64>,
    byte_start: usize,
    byte_end: usize,
    line_start: u64,
    line_end: u64,
    utf16_start: SpanPositionExpectation,
    utf16_end: SpanPositionExpectation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
struct SpanPositionExpectation {
    line: u32,
    character: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SymbolEntityKey {
    kind: String,
    canonical_name: String,
    span_text: String,
    package: Option<String>,
    scope: Option<String>,
    provenance: String,
    confidence: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SymbolOccurrenceKey {
    kind: String,
    canonical_name: Option<String>,
    span_text: String,
    package: Option<String>,
    scope: Option<String>,
    provenance: String,
    confidence: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SymbolEdgeKey {
    kind: String,
    from: String,
    to: String,
    provenance: String,
    confidence: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct SymbolPredictions {
    entities: BTreeSet<SymbolEntityKey>,
    occurrences: BTreeSet<SymbolOccurrenceKey>,
    safety_spans: BTreeSet<SymbolSpanLocation>,
    edges: BTreeSet<SymbolEdgeKey>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SymbolSpanLocation {
    line: u64,
    span_text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum LineTag {
    PackageDecl,
    SubDecl,
    MethodDecl,
    VariableDecl,
    Import,
    Export,
    FunctionCall,
    MethodCall,
    Regex,
    RegexMatch,
    Division,
    DefinedOr,
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

const LINE_TAG_VOCABULARY: &[LineTag] = &[
    LineTag::PackageDecl,
    LineTag::SubDecl,
    LineTag::MethodDecl,
    LineTag::VariableDecl,
    LineTag::Import,
    LineTag::Export,
    LineTag::FunctionCall,
    LineTag::MethodCall,
    LineTag::Regex,
    LineTag::RegexMatch,
    LineTag::Division,
    LineTag::DefinedOr,
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
pub struct ParserAccuracyArtifact {
    schema_version: u32,
    subsystem: String,
    generated_at: String,
    commit: String,
    cadence: Cadence,
    denominator: Denominator,
    families: Vec<FamilySummary>,
    metrics: Vec<MetricRow>,
    failure_packets: Vec<FailurePacket>,
    gold_drift: GoldDrift,
    metric_runtime: MetricRuntime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cadence {
    Pr,
    MergeGate,
    Nightly,
    Release,
}

impl Cadence {
    fn parse(value: &str) -> Result<Self> {
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
struct Denominator {
    fixture_count: u64,
    fixture_family_count: u64,
    scored_line_count: u64,
    scored_symbol_count: u64,
    fully_labeled_region_count: u64,
    partial_labeled_region_count: u64,
    unknown_region_count: u64,
    negative_region_count: u64,
    dynamic_boundary_case_count: u64,
    unsupported_construct_case_count: u64,
    real_project_file_count: u64,
    generated_fixture_count: u64,
    hand_labeled_fixture_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct FamilySummary {
    family: String,
    fixture_count: u64,
    label_modes: Vec<LabelMode>,
    scored_line_count: u64,
    scored_symbol_count: u64,
    dynamic_boundary_case_count: u64,
    unsupported_construct_case_count: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum MetricRow {
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
    fn name(&self) -> &str {
        match self {
            MetricRow::Measured { metric, .. } | MetricRow::InsufficientData { metric, .. } => {
                metric
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Direction {
    Up,
    Down,
    Flat,
    Neutral,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Confidence {
    High,
    Low,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct FailurePacket {
    failure_kind: String,
    likely_layer: String,
    fixture_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    family: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    metric: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<u64>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    expected: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    actual: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    nearest_predictions: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_excerpt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    suggested_next_fix: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct GoldDrift {
    schema_error_count: u64,
    span_error_count: u64,
    duplicate_symbol_id_count: u64,
    missing_resolves_to_target_count: u64,
    changed_line_count: u64,
    changed_line_sample_count: u64,
    changed_symbol_count: u64,
    changed_symbol_sample_count: u64,
    removed_expectation_count: u64,
    removed_expectation_sample_count: u64,
    added_expectation_count: u64,
    added_expectation_sample_count: u64,
    dynamic_expectation_change_count: u64,
    dynamic_expectation_sample_count: u64,
    weakening_explanation_required_count: u64,
    weakening_explanation_sample_count: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct MetricRuntime {
    runtime_ms: f64,
    timeout_count: u64,
    flake_count: u64,
    artifact_size_bytes: u64,
    ci_runner_failure_count: u64,
    orphan_process_count: u64,
    cache_hit_rate: Option<f64>,
    #[serde(skip_serializing_if = "is_zero_u64")]
    cache_sample_count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    peak_rss_mb: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    allocated_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    allocation_count: Option<u64>,
}

#[derive(Debug, Default)]
struct MetricSourceCache {
    sources: BTreeMap<PathBuf, String>,
    hit_count: u64,
    miss_count: u64,
}

impl MetricSourceCache {
    fn read<'a>(&'a mut self, path: &Path, label: &str) -> Result<&'a str> {
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

    fn hit_rate(&self) -> Option<f64> {
        ratio(self.hit_count, self.hit_count + self.miss_count)
    }

    fn sample_count(&self) -> u64 {
        self.hit_count + self.miss_count
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct LineScore {
    line_count: u64,
    true_positive_count: u64,
    false_positive_count: u64,
    false_negative_count: u64,
    exact_match_count: u64,
    expected_parse_error_count: u64,
    false_parse_error_count: u64,
    missed_parse_error_count: u64,
    expected_dynamic_boundary_count: u64,
    correct_dynamic_boundary_count: u64,
    expected_unsupported_construct_count: u64,
    correct_unsupported_construct_count: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct AstScore {
    expected_node_count: u64,
    predicted_node_count: u64,
    node_kind_true_positive_count: u64,
    node_kind_false_positive_count: u64,
    node_kind_false_negative_count: u64,
    span_exact_count: u64,
    span_near_count: u64,
    parent_child_expected_count: u64,
    parent_child_correct_count: u64,
    tree_depth_expected_count: u64,
    tree_depth_correct_count: u64,
    operator_precedence_expected_count: u64,
    operator_precedence_correct_count: u64,
    delimiter_pairing_expected_count: u64,
    delimiter_pairing_correct_count: u64,
    unexpected_error_node_count: u64,
    missing_expected_node_count: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct SymbolScore {
    entity_expected_count: u64,
    entity_predicted_count: u64,
    entity_true_positive_count: u64,
    entity_false_positive_count: u64,
    entity_false_negative_count: u64,
    occurrence_expected_count: u64,
    occurrence_predicted_count: u64,
    occurrence_true_positive_count: u64,
    occurrence_false_positive_count: u64,
    occurrence_false_negative_count: u64,
    edge_expected_count: u64,
    edge_predicted_count: u64,
    edge_true_positive_count: u64,
    edge_false_positive_count: u64,
    edge_false_negative_count: u64,
    entity_by_kind: BTreeMap<String, KindScore>,
    occurrence_by_kind: BTreeMap<String, KindScore>,
    false_positive_sample_count: u64,
    false_import_count: u64,
    false_export_count: u64,
    false_exact_resolution_count: u64,
    false_dynamic_resolution_count: u64,
    dynamic_false_precision_count: u64,
    dynamic_false_precision_sample_count: u64,
    comment_safety_region_count: u64,
    pod_safety_region_count: u64,
    string_safety_region_count: u64,
    unknown_safety_region_count: u64,
    symbols_emitted_in_comments: u64,
    symbols_emitted_in_pod: u64,
    symbols_emitted_in_strings: u64,
    symbols_emitted_in_unknown_regions: u64,
    proof_score: ProofScore,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ProofScore {
    true_positive_by_bucket: BTreeMap<ProofBucket, u64>,
    predicted_by_bucket: BTreeMap<ProofBucket, u64>,
    high_confidence_false_positive_count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ProofBucket {
    Exact,
    High,
    Medium,
    Low,
    Heuristic,
    Dynamic,
}

impl ProofBucket {
    fn precision_metric(self) -> &'static str {
        match self {
            ProofBucket::Exact => "exact_fact_precision",
            ProofBucket::High => "high_confidence_precision",
            ProofBucket::Medium => "medium_confidence_precision",
            ProofBucket::Low => "low_confidence_precision",
            ProofBucket::Heuristic => "heuristic_fact_precision",
            ProofBucket::Dynamic => "dynamic_boundary_precision",
        }
    }

    fn insufficient_reason(self) -> &'static str {
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

trait ProofShape {
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
struct KindScore {
    expected_count: u64,
    predicted_count: u64,
    true_positive_count: u64,
    false_positive_count: u64,
    false_negative_count: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct RecoveryScore {
    expectation_count: u64,
    first_error_line_correct_count: u64,
    error_region_true_positive_count: u64,
    error_region_false_positive_count: u64,
    error_region_false_negative_count: u64,
    spillover_lines: Vec<u64>,
    recovery_parse_micros: Vec<u64>,
    post_error_line_score: LineScore,
    post_error_symbol_expected_count: u64,
    post_error_symbol_found_count: u64,
}

#[derive(Debug, Clone, Default, PartialEq)]
struct IncrementalScore {
    expectation_count: u64,
    full_parse_equivalent_count: u64,
    edit_apply_equivalent_count: u64,
    no_panic_count: u64,
    no_progress_count: u64,
    timeout_count: u64,
    fallback_count: u64,
    checkpoint_hit_count: u64,
    checkpoint_miss_count: u64,
    content_hash_hit_count: u64,
    content_hash_miss_count: u64,
    semantic_fact_cache_hit_count: u64,
    semantic_fact_cache_miss_count: u64,
    workspace_shard_reuse_count: u64,
    workspace_shard_replacement_attempt_count: u64,
    unchanged_file_skip_count: u64,
    unchanged_file_index_attempt_count: u64,
    reparse_byte_ratios: Vec<f64>,
    reused_token_ratios: Vec<f64>,
    reused_node_ratios: Vec<f64>,
    changed_range_sample_count: u64,
    changed_range_correct_count: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct SpanScore {
    expectation_count: u64,
    byte_exact_count: u64,
    line_exact_count: u64,
    utf16_exact_count: u64,
    near_count: u64,
    invalid_count: u64,
    out_of_bounds_count: u64,
    inverted_count: u64,
    non_char_boundary_count: u64,
    crlf_sample_count: u64,
    crlf_position_error_count: u64,
    unicode_sample_count: u64,
    unicode_position_error_count: u64,
    tab_sample_count: u64,
    tab_column_mismatch_count: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct UnsupportedScore {
    manifest_construct_count: u64,
    family_count: u64,
    line_labeled_construct_count: u64,
    detected_count: u64,
    salvaged_count: u64,
    false_exact_count: u64,
    false_exact_sample_count: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct MethodCompletionProviderScore {
    receiver_expected_count: u64,
    receiver_hit_count: u64,
    fallback_expected_count: u64,
    fallback_correct_count: u64,
    false_receiver_count: u64,
    relevance_assertion_count: u64,
    relevance_assertion_correct_count: u64,
    import_visibility_expected_count: u64,
    import_visibility_correct_count: u64,
    completion_query_micros: Vec<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct DiagnosticProviderScore {
    dynamic_boundary_expected_absent_count: u64,
    dynamic_boundary_false_positive_count: u64,
    undefined_expected_absent_count: u64,
    undefined_false_positive_count: u64,
    undefined_expected_present_count: u64,
    undefined_false_negative_count: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct NavigationProviderScore {
    document_symbol_expected_count: u64,
    document_symbol_returned_count: u64,
    document_symbol_span_exact_count: u64,
    goto_definition_expected_count: u64,
    goto_definition_hit_count: u64,
    goto_definition_span_exact_count: u64,
    goto_definition_false_target_count: u64,
    definition_query_micros: Vec<u64>,
    references_expected_count: u64,
    references_hit_count: u64,
    references_returned_count: u64,
    references_false_positive_count: u64,
    references_absent_assertion_count: u64,
    reference_query_micros: Vec<u64>,
    hover_expected_count: u64,
    hover_origin_correct_count: u64,
    rename_safe_edit_expected_count: u64,
    rename_safe_edit_correct_count: u64,
    safe_delete_blocker_expected_count: u64,
    safe_delete_blocker_correct_count: u64,
}

#[derive(Debug, Clone, Default, PartialEq)]
struct ScaleCostScore {
    fixture_count: u64,
    file_bytes: u64,
    source_lines: u64,
    token_count: u64,
    ast_node_count: u64,
    symbol_count: u64,
    import_count: u64,
    export_count: u64,
    sub_count: u64,
    package_count: u64,
    max_nesting_depth: u64,
    max_brace_depth: u64,
    max_regex_length: u64,
    max_heredoc_body_bytes: u64,
    quote_like_count: u64,
    dynamic_boundary_count: u64,
    lex_ms: Vec<f64>,
    parse_ms: Vec<f64>,
    ast_projection_ms: Vec<f64>,
    semantic_extraction_ms: Vec<f64>,
    workspace_insert_ms: Vec<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct DeterminismScore {
    fixture_count: u64,
    token_stream_stable_count: u64,
    parse_hash_stable_count: u64,
    ast_hash_stable_count: u64,
    semantic_fact_hash_stable_count: u64,
    diagnostic_hash_stable_count: u64,
    repeated_parse_stable_count: u64,
    whitespace_invariance_stable_count: u64,
    whitespace_invariance_sample_count: u64,
    comment_invariance_stable_count: u64,
    comment_invariance_sample_count: u64,
    newline_style_invariance_stable_count: u64,
    newline_style_invariance_sample_count: u64,
}

/// Run `cargo xtask metrics parser-accuracy`.
pub fn run(
    json: bool,
    check: bool,
    export_status_receipts: bool,
    manifest: Option<PathBuf>,
    output: Option<PathBuf>,
    cadence: &str,
) -> Result<()> {
    let root = project_root()?;
    let cadence = Cadence::parse(cadence)?;
    let manifest_path = manifest.unwrap_or_else(|| root.join(DEFAULT_MANIFEST));
    let output_path = output.unwrap_or_else(|| root.join(DEFAULT_OUTPUT));

    let (manifest, artifact) = build_status_artifact(&root, &manifest_path, cadence)?;

    if check {
        validate_artifact_contract(&artifact)?;
        println!(
            "parser accuracy artifact check passed: {} fixtures across {} families",
            artifact.denominator.fixture_count, artifact.denominator.fixture_family_count
        );
        return Ok(());
    }

    if json {
        write_artifact(&output_path, &artifact)?;
        write_ratchet_receipt(&root, &artifact)?;
        println!("parser accuracy artifact written: {}", output_path.display());
    } else if export_status_receipts {
        write_ratchet_receipt(&root, &artifact)?;
    } else {
        print_summary(&artifact);
    }

    if export_status_receipts {
        write_status_receipts(&root, &manifest, &artifact)?;
    }

    Ok(())
}

pub fn refresh_default_artifact_for_status(root: &Path) -> Result<()> {
    let manifest_path = root.join(DEFAULT_MANIFEST);
    let output_path = root.join(DEFAULT_OUTPUT);
    let (_manifest, artifact) = build_status_artifact(root, &manifest_path, Cadence::Pr)?;
    validate_artifact_contract(&artifact)?;
    write_artifact(&output_path, &artifact)?;
    write_ratchet_receipt(root, &artifact)?;
    println!("parser accuracy artifact written: {}", output_path.display());
    Ok(())
}

fn build_status_artifact(
    root: &Path,
    manifest_path: &Path,
    cadence: Cadence,
) -> Result<(ParserAccuracyManifest, ParserAccuracyArtifact)> {
    let start = Instant::now();
    let manifest = read_manifest(root, manifest_path)?;
    let rss_before = get_current_memory_usage().ok();
    let (artifact, allocation_measurement) =
        measure_allocations(|| build_artifact(root, &manifest, cadence));
    let rss_after = get_current_memory_usage().ok();
    let mut artifact = artifact?;
    artifact.metric_runtime.runtime_ms = start.elapsed().as_secs_f64() * 1000.0;
    artifact.metric_runtime.peak_rss_mb =
        measured_memory_mb(rss_before, rss_after, allocation_measurement.peak_delta_mb());
    artifact.metric_runtime.allocated_bytes = Some(allocation_measurement.allocated_bytes);
    artifact.metric_runtime.allocation_count = Some(allocation_measurement.allocation_count);
    settle_artifact_size(&mut artifact)?;
    sync_allocation_metric_rows(&mut artifact, cadence);
    sync_runtime_metric_rows(&mut artifact, cadence);
    Ok((manifest, artifact))
}

fn measured_memory_mb(
    rss_before: Option<f64>,
    rss_after: Option<f64>,
    fallback_mb: f64,
) -> Option<f64> {
    if let (Some(before), Some(after)) = (rss_before, rss_after) {
        if after > before {
            return Some(after - before);
        }
    }
    (fallback_mb > 0.0).then_some(fallback_mb)
}

fn read_manifest(root: &Path, path: &Path) -> Result<ParserAccuracyManifest> {
    let manifest_path = if path.is_absolute() { path.to_path_buf() } else { root.join(path) };
    let raw = fs::read_to_string(&manifest_path)
        .with_context(|| format!("reading parser accuracy manifest {}", manifest_path.display()))?;
    let manifest: ParserAccuracyManifest = serde_json::from_str(&raw)
        .with_context(|| format!("parsing parser accuracy manifest {}", manifest_path.display()))?;
    if manifest.schema_version != 1 {
        bail!("unsupported parser accuracy manifest schema_version {}", manifest.schema_version);
    }
    for fixture in &manifest.fixtures {
        let source_path = root.join(&fixture.source_path);
        if !source_path.exists() {
            bail!(
                "parser accuracy fixture '{}' source does not exist: {}",
                fixture.id,
                source_path.display()
            );
        }
    }
    Ok(manifest)
}

fn read_gold_baseline(root: &Path) -> Result<Option<GoldBaseline>> {
    let baseline_path = root.join(GOLD_BASELINE);
    if !baseline_path.exists() {
        return Ok(None);
    }

    let raw = fs::read_to_string(&baseline_path).with_context(|| {
        format!("reading parser accuracy gold baseline {}", baseline_path.display())
    })?;
    let baseline: GoldBaseline = serde_json::from_str(&raw).with_context(|| {
        format!("parsing parser accuracy gold baseline {}", baseline_path.display())
    })?;
    if baseline.schema_version != 1 {
        bail!(
            "unsupported parser accuracy gold baseline schema_version {}",
            baseline.schema_version
        );
    }
    Ok(Some(baseline))
}

fn build_artifact(
    root: &Path,
    manifest: &ParserAccuracyManifest,
    cadence: Cadence,
) -> Result<ParserAccuracyArtifact> {
    let denominator = compute_denominator(manifest);
    let families = summarize_families(manifest);
    let fixture_count = denominator.fixture_count as f64;
    let mut source_cache = MetricSourceCache::default();
    let line_score = score_manifest_line_tags(root, manifest, &mut source_cache)?;
    let ast_score = score_manifest_ast(root, manifest, &mut source_cache)?;
    let symbol_score = score_manifest_symbols(root, manifest, &mut source_cache)?;
    let recovery_score = score_manifest_recovery(root, manifest, &mut source_cache)?;
    let incremental_score = score_manifest_incremental(root, manifest, &mut source_cache)?;
    let span_score = score_manifest_spans(root, manifest, &mut source_cache)?;
    let unsupported_score =
        score_manifest_unsupported(root, manifest, &line_score, &mut source_cache)?;
    let method_completion_provider_score =
        score_method_completion_provider_expectations(root, manifest, &mut source_cache)?;
    let diagnostic_provider_score =
        score_diagnostic_provider_expectations(root, manifest, &mut source_cache)?;
    let navigation_provider_score =
        score_navigation_provider_expectations(root, manifest, &mut source_cache)?;
    let scale_cost_score = score_manifest_scale_cost(root, manifest, &mut source_cache)?;
    let determinism_score = score_manifest_determinism(root, manifest, &mut source_cache)?;
    let gold_drift = audit_gold_drift(root, manifest, &mut source_cache)?;
    let mut metrics = vec![measured_value(
        "denominator_fixture_count",
        fixture_count,
        denominator.fixture_count,
        cadence,
    )];
    metrics.extend(line_metrics(&line_score, cadence));
    metrics.extend(ast_metrics(&ast_score, cadence));
    metrics.extend(symbol_metrics(&symbol_score, cadence));
    metrics.extend(safety_metrics(&line_score, &symbol_score, cadence));
    metrics.extend(recovery_metrics(&recovery_score, cadence));
    metrics.extend(incremental_metrics(&incremental_score, cadence));
    metrics.extend(span_metrics(&span_score, cadence));
    metrics.extend(confidence_metrics(&symbol_score, cadence));
    metrics.extend(unsupported_metrics(&unsupported_score, cadence));
    metrics.extend(provider_impact_metrics(
        &method_completion_provider_score,
        &diagnostic_provider_score,
        &navigation_provider_score,
        cadence,
    ));
    metrics.extend(scale_metrics(&scale_cost_score, cadence));
    metrics.extend(cost_metrics(
        &scale_cost_score,
        &recovery_score,
        &method_completion_provider_score,
        &navigation_provider_score,
        cadence,
    ));
    metrics.extend(cache_reuse_metrics(&incremental_score, cadence));
    metrics.extend(determinism_metrics(&determinism_score, cadence));
    metrics.extend(gold_drift_metrics(&gold_drift, denominator.fixture_count, cadence));
    apply_safety_floor_metadata(&mut metrics);

    Ok(ParserAccuracyArtifact {
        schema_version: 1,
        subsystem: "parser_accuracy".to_string(),
        generated_at: Utc::now().to_rfc3339(),
        commit: git_commit(root),
        cadence,
        denominator,
        families,
        metrics,
        failure_packets: failure_packet::collect_failure_packets(root, manifest)?,
        gold_drift,
        metric_runtime: MetricRuntime {
            cache_hit_rate: source_cache.hit_rate(),
            cache_sample_count: source_cache.sample_count(),
            ..MetricRuntime::default()
        },
    })
}

fn compute_denominator(manifest: &ParserAccuracyManifest) -> Denominator {
    let mut families = BTreeSet::new();
    let mut denominator =
        Denominator { fixture_count: manifest.fixtures.len() as u64, ..Denominator::default() };

    for fixture in &manifest.fixtures {
        families.insert(fixture.family.clone());
        denominator.scored_line_count += fixture.scored_lines;
        denominator.scored_symbol_count += fixture.scored_symbols;
        denominator.fully_labeled_region_count += fixture.fully_labeled_regions;
        denominator.partial_labeled_region_count += fixture.partial_labeled_regions;
        denominator.unknown_region_count += fixture.unknown_regions;
        denominator.negative_region_count += fixture.negative_regions;
        denominator.dynamic_boundary_case_count += fixture.dynamic_boundaries;
        denominator.unsupported_construct_case_count += fixture.unsupported_constructs;
        if fixture.real_project_file {
            denominator.real_project_file_count += 1;
        }
        if fixture.generated {
            denominator.generated_fixture_count += 1;
        } else {
            denominator.hand_labeled_fixture_count += 1;
        }
    }

    denominator.fixture_family_count = families.len() as u64;
    denominator
}

fn summarize_families(manifest: &ParserAccuracyManifest) -> Vec<FamilySummary> {
    #[derive(Default)]
    struct Accumulator {
        fixture_count: u64,
        label_modes: BTreeSet<LabelMode>,
        scored_line_count: u64,
        scored_symbol_count: u64,
        dynamic_boundary_case_count: u64,
        unsupported_construct_case_count: u64,
    }

    let mut by_family = BTreeMap::<String, Accumulator>::new();
    for fixture in &manifest.fixtures {
        let entry = by_family.entry(fixture.family.clone()).or_default();
        entry.fixture_count += 1;
        entry.label_modes.insert(fixture.label_mode);
        entry.scored_line_count += fixture.scored_lines;
        entry.scored_symbol_count += fixture.scored_symbols;
        entry.dynamic_boundary_case_count += fixture.dynamic_boundaries;
        entry.unsupported_construct_case_count += fixture.unsupported_constructs;
    }

    by_family
        .into_iter()
        .map(|(family, entry)| FamilySummary {
            family,
            fixture_count: entry.fixture_count,
            label_modes: entry.label_modes.into_iter().collect(),
            scored_line_count: entry.scored_line_count,
            scored_symbol_count: entry.scored_symbol_count,
            dynamic_boundary_case_count: entry.dynamic_boundary_case_count,
            unsupported_construct_case_count: entry.unsupported_construct_case_count,
        })
        .collect()
}

fn score_manifest_line_tags(
    root: &Path,
    manifest: &ParserAccuracyManifest,
    source_cache: &mut MetricSourceCache,
) -> Result<LineScore> {
    let mut score = LineScore::default();
    for fixture in &manifest.fixtures {
        if fixture.line_expectations.is_empty() {
            continue;
        }
        let source_path = root.join(&fixture.source_path);
        let source = source_cache.read(&source_path, "fixture source")?;
        let actual_by_line = extract_line_tags(source);
        for expectation in &fixture.line_expectations {
            let actual = actual_by_line.get(&expectation.line).cloned().unwrap_or_default();
            let actual = comparable_actual_line_tags(&expectation.expected_tags, &actual);
            score_line_tags(&expectation.expected_tags, &actual, &mut score);
        }
    }
    Ok(score)
}

fn extract_line_tags(source: &str) -> BTreeMap<u64, BTreeSet<LineTag>> {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    let line_starts = line_starts(source);
    let mut by_line = BTreeMap::new();
    collect_node_line_tags(&output.ast, &line_starts, &mut by_line);
    normalize_diagnostic_line_tags(&output, &line_starts, &mut by_line);
    by_line
}

fn line_starts(source: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (index, byte) in source.bytes().enumerate() {
        if byte == b'\n' {
            starts.push(index + 1);
        }
    }
    starts
}

fn line_for_offset(line_starts: &[usize], offset: usize) -> u64 {
    match line_starts.binary_search(&offset) {
        Ok(index) => (index + 1) as u64,
        Err(0) => 1,
        Err(index) => index as u64,
    }
}

fn collect_node_line_tags(
    node: &Node,
    line_starts: &[usize],
    by_line: &mut BTreeMap<u64, BTreeSet<LineTag>>,
) {
    if let Some(tag) = line_tag_for_node(node) {
        let line = line_for_offset(line_starts, node.location.start);
        by_line.entry(line).or_default().insert(tag);
    }
    if let NodeKind::FunctionCall { name, args } = &node.kind
        && name == "require"
        && args.first().is_some_and(|arg| matches!(arg.kind, NodeKind::Variable { .. }))
    {
        let line = line_for_offset(line_starts, node.location.start);
        by_line.entry(line).or_default().insert(LineTag::DynamicBoundary);
    }
    node.for_each_child(|child| collect_node_line_tags(child, line_starts, by_line));
}

fn normalize_diagnostic_line_tags(
    output: &perl_parser::ParseOutput,
    line_starts: &[usize],
    by_line: &mut BTreeMap<u64, BTreeSet<LineTag>>,
) {
    let mut error_lines = BTreeSet::new();
    for diagnostic in &output.diagnostics {
        if let Some(location) = quote_like_parse_error_location(diagnostic) {
            error_lines.insert(line_for_offset(line_starts, location));
        }
    }

    for line in error_lines {
        by_line.entry(line).or_default().insert(LineTag::ParseError);
    }
}

fn comparable_actual_line_tags(
    expected: &BTreeSet<LineTag>,
    actual: &BTreeSet<LineTag>,
) -> BTreeSet<LineTag> {
    if expected.len() == 1
        && expected.contains(&LineTag::ParseError)
        && actual.contains(&LineTag::ParseError)
    {
        return BTreeSet::from([LineTag::ParseError]);
    }

    let mut comparable = actual.clone();
    if expected.contains(&LineTag::Regex) && comparable.remove(&LineTag::RegexMatch) {
        comparable.insert(LineTag::Regex);
    }
    comparable
}

fn quote_like_parse_error_location(error: &ParseError) -> Option<usize> {
    if let ParseError::SyntaxError { message, location } = error
        && is_unclosed_quote_like_diagnostic(message)
    {
        return Some(*location);
    }

    None
}

fn is_unclosed_quote_like_diagnostic(message: &str) -> bool {
    (message.starts_with("Unclosed ")
        && (message.contains(" delimiter in string operator ")
            || message.starts_with("Unclosed qw() delimiter")))
        || message.starts_with("Missing replacement in substitution")
        || message.starts_with("Missing closing delimiter in substitution")
        || message.starts_with("Missing replacement list in transliteration")
        || message.starts_with("Missing closing delimiter in transliteration")
}

fn line_tag_for_node(node: &Node) -> Option<LineTag> {
    match &node.kind {
        NodeKind::Package { .. } => Some(LineTag::PackageDecl),
        NodeKind::Subroutine { .. } => Some(LineTag::SubDecl),
        NodeKind::Method { .. } => Some(LineTag::MethodDecl),
        NodeKind::VariableDeclaration { .. } | NodeKind::VariableListDeclaration { .. } => {
            Some(LineTag::VariableDecl)
        }
        NodeKind::Use { .. } | NodeKind::No { .. } => Some(LineTag::Import),
        NodeKind::FunctionCall { name, .. } if name == "require" => Some(LineTag::Import),
        NodeKind::FunctionCall { .. } => Some(LineTag::FunctionCall),
        NodeKind::MethodCall { .. } => Some(LineTag::MethodCall),
        NodeKind::Eval { .. } => Some(LineTag::FunctionCall),
        NodeKind::Regex { .. }
        | NodeKind::Substitution { .. }
        | NodeKind::Transliteration { .. } => Some(LineTag::Regex),
        NodeKind::Match { .. } => Some(LineTag::RegexMatch),
        NodeKind::Binary { op, .. } if op == "/" => Some(LineTag::Division),
        NodeKind::Binary { op, .. } if op == "//" => Some(LineTag::DefinedOr),
        NodeKind::Heredoc { .. } => Some(LineTag::HeredocOpener),
        NodeKind::Format { .. } => Some(LineTag::FormatDecl),
        NodeKind::Given { .. } | NodeKind::When { .. } | NodeKind::Default { .. } => {
            Some(LineTag::GivenWhen)
        }
        NodeKind::Do { .. } => Some(LineTag::DoWhile),
        NodeKind::Error { .. } => Some(LineTag::ParseError),
        NodeKind::UnknownRest => Some(LineTag::UnsupportedConstruct),
        _ => None,
    }
}

fn score_line_tags(
    expected: &BTreeSet<LineTag>,
    actual: &BTreeSet<LineTag>,
    score: &mut LineScore,
) {
    score.line_count += 1;
    let true_positives = expected.intersection(actual).count() as u64;
    let false_positives = actual.difference(expected).count() as u64;
    let false_negatives = expected.difference(actual).count() as u64;
    score.true_positive_count += true_positives;
    score.false_positive_count += false_positives;
    score.false_negative_count += false_negatives;
    if expected == actual {
        score.exact_match_count += 1;
    }

    let expected_parse_error = expected.contains(&LineTag::ParseError);
    let actual_parse_error = actual.contains(&LineTag::ParseError);
    if expected_parse_error {
        score.expected_parse_error_count += 1;
    }
    if actual_parse_error && !expected_parse_error {
        score.false_parse_error_count += 1;
    }
    if expected_parse_error && !actual_parse_error {
        score.missed_parse_error_count += 1;
    }

    if expected.contains(&LineTag::DynamicBoundary) {
        score.expected_dynamic_boundary_count += 1;
        if actual.contains(&LineTag::DynamicBoundary) {
            score.correct_dynamic_boundary_count += 1;
        }
    }

    if expected.contains(&LineTag::UnsupportedConstruct) {
        score.expected_unsupported_construct_count += 1;
        if actual.contains(&LineTag::UnsupportedConstruct) {
            score.correct_unsupported_construct_count += 1;
        }
    }
}

fn line_metrics(score: &LineScore, cadence: Cadence) -> Vec<MetricRow> {
    if score.line_count == 0 {
        return vec![insufficient("line_construct_f1", "line-level gold labels are not available")];
    }

    let precision_denominator = score.true_positive_count + score.false_positive_count;
    let recall_denominator = score.true_positive_count + score.false_negative_count;
    let precision = ratio(score.true_positive_count, precision_denominator);
    let recall = ratio(score.true_positive_count, recall_denominator);
    let f1 = match (precision, recall) {
        (Some(precision), Some(recall)) if precision + recall > 0.0 => {
            Some(2.0 * precision * recall / (precision + recall))
        }
        _ => None,
    };

    let mut rows = vec![
        measured_count(
            "line_construct_true_positive_count",
            score.true_positive_count,
            score.line_count,
            cadence,
        ),
        measured_count(
            "line_construct_false_positive_count",
            score.false_positive_count,
            score.line_count,
            cadence,
        ),
        measured_count(
            "line_construct_false_negative_count",
            score.false_negative_count,
            score.line_count,
            cadence,
        ),
        measured_rate(
            "line_construct_exact_match_rate",
            score.exact_match_count,
            score.line_count,
            cadence,
        ),
    ];

    rows.push(optional_measured_rate(
        "line_construct_precision",
        precision,
        precision_denominator,
        "no predicted line tags were available",
        cadence,
    ));
    rows.push(optional_measured_rate(
        "line_construct_recall",
        recall,
        recall_denominator,
        "no expected line tags were available",
        cadence,
    ));
    rows.push(optional_measured_rate(
        "line_construct_f1",
        f1,
        recall_denominator,
        "line precision or recall denominator is unavailable",
        cadence,
    ));
    rows.push(measured_rate(
        "line_error_false_positive_rate",
        score.false_parse_error_count,
        score.line_count,
        cadence,
    ));
    rows.push(optional_measured_rate(
        "line_error_false_negative_rate",
        ratio(score.missed_parse_error_count, score.expected_parse_error_count),
        score.expected_parse_error_count,
        "no expected parse-error line labels are available",
        cadence,
    ));
    rows.push(optional_measured_rate(
        "line_dynamic_boundary_correct_rate",
        ratio(score.correct_dynamic_boundary_count, score.expected_dynamic_boundary_count),
        score.expected_dynamic_boundary_count,
        "no expected dynamic-boundary line labels are available",
        cadence,
    ));
    rows.push(optional_measured_rate(
        "line_unsupported_detection_rate",
        ratio(
            score.correct_unsupported_construct_count,
            score.expected_unsupported_construct_count,
        ),
        score.expected_unsupported_construct_count,
        "no expected unsupported-construct line labels are available",
        cadence,
    ));

    rows
}

fn score_manifest_recovery(
    root: &Path,
    manifest: &ParserAccuracyManifest,
    source_cache: &mut MetricSourceCache,
) -> Result<RecoveryScore> {
    let mut score = RecoveryScore::default();
    for fixture in &manifest.fixtures {
        if fixture.recovery_expectations.is_empty() {
            continue;
        }
        let source_path = root.join(&fixture.source_path);
        let source = source_cache.read(&source_path, "recovery fixture source")?;
        let (prediction, recovery_micros) = extract_recovery_prediction(&source_path, source);
        score.recovery_parse_micros.push(recovery_micros);
        for expectation in &fixture.recovery_expectations {
            score_recovery_expectation(expectation, &prediction, &mut score);
        }
    }
    Ok(score)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct RecoveryPrediction {
    first_error_line: Option<u64>,
    error_region_lines: BTreeSet<u64>,
    actual_by_line: BTreeMap<u64, BTreeSet<LineTag>>,
    symbol_spans: BTreeSet<SymbolSpanLocation>,
}

fn extract_recovery_prediction(source_path: &Path, source: &str) -> (RecoveryPrediction, u64) {
    let mut parser = Parser::new(source);
    let recovery_start = Instant::now();
    let output = parser.parse_with_recovery();
    let recovery_micros = recovery_start.elapsed().as_micros() as u64;
    let line_starts = line_starts(source);
    let mut error_lines = BTreeSet::new();
    for diagnostic in &output.diagnostics {
        if let Some(location) = parse_error_location(diagnostic) {
            error_lines.insert(line_for_offset(&line_starts, location));
        }
    }
    collect_error_node_lines(&output.ast, &line_starts, &mut error_lines);

    let mut actual_by_line = BTreeMap::new();
    collect_node_line_tags(&output.ast, &line_starts, &mut actual_by_line);
    let symbol_spans = extract_symbol_predictions(source_path, source)
        .map(|predictions| predictions.safety_spans)
        .unwrap_or_default();

    (
        RecoveryPrediction {
            first_error_line: error_lines.first().copied(),
            error_region_lines: error_lines,
            actual_by_line,
            symbol_spans,
        },
        recovery_micros,
    )
}

fn parse_error_location(error: &ParseError) -> Option<usize> {
    match error {
        ParseError::UnexpectedToken { location, .. }
        | ParseError::SyntaxError { location, .. }
        | ParseError::Recovered { location, .. } => Some(*location),
        _ => None,
    }
}

fn collect_error_node_lines(node: &Node, line_starts: &[usize], lines: &mut BTreeSet<u64>) {
    if matches!(node.kind, NodeKind::Error { .. }) {
        lines.insert(line_for_offset(line_starts, node.location.start));
    }
    node.for_each_child(|child| collect_error_node_lines(child, line_starts, lines));
}

fn score_recovery_expectation(
    expectation: &RecoveryExpectation,
    prediction: &RecoveryPrediction,
    score: &mut RecoveryScore,
) {
    score.expectation_count += 1;
    if prediction.first_error_line == Some(expectation.first_error_line) {
        score.first_error_line_correct_count += 1;
    }

    let expected_region = line_range_set(expectation.error_region);
    score.error_region_true_positive_count +=
        expected_region.intersection(&prediction.error_region_lines).count() as u64;
    score.error_region_false_positive_count +=
        prediction.error_region_lines.difference(&expected_region).count() as u64;
    score.error_region_false_negative_count +=
        expected_region.difference(&prediction.error_region_lines).count() as u64;

    let actual_end = prediction
        .error_region_lines
        .iter()
        .next_back()
        .copied()
        .unwrap_or(expectation.error_region.end);
    score.spillover_lines.push(actual_end.saturating_sub(expectation.error_region.end));

    for line_expectation in &expectation.post_error_line_expectations {
        let actual =
            prediction.actual_by_line.get(&line_expectation.line).cloned().unwrap_or_default();
        let actual = comparable_actual_line_tags(&line_expectation.expected_tags, &actual);
        score_line_tags(&line_expectation.expected_tags, &actual, &mut score.post_error_line_score);
    }

    for span in &expectation.post_error_symbol_spans {
        score.post_error_symbol_expected_count += 1;
        if prediction
            .symbol_spans
            .iter()
            .any(|actual| actual.line >= expectation.recovery_line && actual.span_text == *span)
        {
            score.post_error_symbol_found_count += 1;
        }
    }
}

fn line_range_set(range: LineRange) -> BTreeSet<u64> {
    if range.end < range.start {
        return BTreeSet::new();
    }
    (range.start..=range.end).collect()
}

fn score_manifest_incremental(
    root: &Path,
    manifest: &ParserAccuracyManifest,
    source_cache: &mut MetricSourceCache,
) -> Result<IncrementalScore> {
    let mut score = IncrementalScore::default();
    let content_hash_index = WorkspaceIndex::new();
    for fixture in &manifest.fixtures {
        let source_path = root.join(&fixture.source_path);
        let source = source_cache.read(&source_path, "incremental fixture source")?;
        score_content_hash_reuse_probe(&content_hash_index, &source_path, source, &mut score)?;
        if fixture.incremental_expectations.is_empty() {
            continue;
        }
        for expectation in &fixture.incremental_expectations {
            score_incremental_expectation(source, expectation, &mut score);
        }
    }
    Ok(score)
}

fn score_content_hash_reuse_probe(
    index: &WorkspaceIndex,
    source_path: &Path,
    source: &str,
    score: &mut IncrementalScore,
) -> Result<()> {
    let source_path_text = source_path.to_string_lossy();
    let first_hit = content_hash_probe_hits(index, &source_path_text, source);
    let first_semantic_hit = semantic_fact_cache_probe_hits(index, &source_path_text);
    if index.index_file_str(&source_path_text, source).is_err() {
        return Ok(());
    }
    record_content_hash_probe(score, first_hit);
    record_semantic_fact_cache_probe(score, first_semantic_hit);
    record_unchanged_file_skip_probe(score, first_hit);
    record_workspace_shard_reuse_probe(index, &source_path_text, score);

    let second_hit = content_hash_probe_hits(index, &source_path_text, source);
    let second_semantic_hit = semantic_fact_cache_probe_hits(index, &source_path_text);
    index.index_file_str(&source_path_text, source).map_err(|err| {
        eyre!(
            "re-indexing parser accuracy fixture {} for content-hash metric: {err}",
            source_path.display()
        )
    })?;
    record_content_hash_probe(score, second_hit);
    record_semantic_fact_cache_probe(score, second_semantic_hit);
    record_unchanged_file_skip_probe(score, second_hit);

    Ok(())
}

fn content_hash_probe_hits(index: &WorkspaceIndex, source_path_text: &str, source: &str) -> bool {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    let content_hash = hasher.finish();
    index.file_fact_shard(source_path_text).is_some_and(|shard| shard.content_hash == content_hash)
}

fn semantic_fact_cache_probe_hits(index: &WorkspaceIndex, source_path_text: &str) -> bool {
    index.file_fact_shard(source_path_text).is_some()
}

fn record_workspace_shard_reuse_probe(
    index: &WorkspaceIndex,
    source_path_text: &str,
    score: &mut IncrementalScore,
) {
    let Some(shard) = index.file_fact_shard(source_path_text) else {
        return;
    };
    let key = DocumentStore::uri_key(&shard.source_uri);
    let replacement = index.replace_fact_shard_incremental(&key, shard);
    score.workspace_shard_replacement_attempt_count += 1;
    if replacement.content_unchanged {
        score.workspace_shard_reuse_count += 1;
    }
}

fn record_content_hash_probe(score: &mut IncrementalScore, hit: bool) {
    if hit {
        score.content_hash_hit_count += 1;
    } else {
        score.content_hash_miss_count += 1;
    }
}

fn record_semantic_fact_cache_probe(score: &mut IncrementalScore, hit: bool) {
    if hit {
        score.semantic_fact_cache_hit_count += 1;
    } else {
        score.semantic_fact_cache_miss_count += 1;
    }
}

fn record_unchanged_file_skip_probe(score: &mut IncrementalScore, skipped: bool) {
    score.unchanged_file_index_attempt_count += 1;
    if skipped {
        score.unchanged_file_skip_count += 1;
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
struct IncrementalExpectationResult {
    full_parse_equivalent: bool,
    edit_apply_equivalent: bool,
    fallback_used: bool,
    checkpoint_hit_count: u64,
    checkpoint_miss_count: u64,
    reparse_byte_ratio: Option<f64>,
    reused_token_ratio: Option<f64>,
    reused_node_ratio: Option<f64>,
    changed_range_correct: Option<bool>,
}

fn score_incremental_expectation(
    source: &str,
    expectation: &IncrementalExpectation,
    score: &mut IncrementalScore,
) {
    score.expectation_count += 1;
    let outcome =
        catch_unwind(AssertUnwindSafe(|| run_incremental_expectation(source, expectation)));

    let result = match outcome {
        Ok(Ok(result)) => {
            score.no_panic_count += 1;
            result
        }
        Ok(Err(error)) => {
            score.no_panic_count += 1;
            if error.to_string().contains("did not advance") {
                score.no_progress_count += 1;
            }
            return;
        }
        Err(_) => return,
    };

    if result.full_parse_equivalent {
        score.full_parse_equivalent_count += 1;
    }
    if result.edit_apply_equivalent {
        score.edit_apply_equivalent_count += 1;
    }
    if result.fallback_used {
        score.fallback_count += 1;
    }
    score.checkpoint_hit_count += result.checkpoint_hit_count;
    score.checkpoint_miss_count += result.checkpoint_miss_count;
    if let Some(value) = result.reparse_byte_ratio {
        score.reparse_byte_ratios.push(value);
    }
    if let Some(value) = result.reused_token_ratio {
        score.reused_token_ratios.push(value);
    }
    if let Some(value) = result.reused_node_ratio {
        score.reused_node_ratios.push(value);
    }
    if let Some(correct) = result.changed_range_correct {
        score.changed_range_sample_count += 1;
        if correct {
            score.changed_range_correct_count += 1;
        }
    }
}

fn run_incremental_expectation(
    source: &str,
    expectation: &IncrementalExpectation,
) -> Result<IncrementalExpectationResult> {
    let resolved_edits = resolve_incremental_edits(source, &expectation.edits)
        .with_context(|| format!("resolving incremental expectation '{}'", expectation.id))?;
    let expected_source = apply_resolved_edits(source, &resolved_edits)?;
    let mut parser = IncrementalParserV2::new();
    let _initial_ast = parser.parse(source)?;
    for edit in &resolved_edits {
        parser.edit(edit.core_edit.clone());
    }
    let incremental_ast = parser.parse(&expected_source)?;
    let mut full_parser = Parser::new(&expected_source);
    let full_ast = full_parser.parse()?;

    let apply_result = run_incremental_apply_path(source, &resolved_edits)?;
    let total_nodes = parser.reused_nodes + parser.reparsed_nodes;
    let reused_node_ratio =
        if total_nodes == 0 { None } else { Some(parser.reused_nodes as f64 / total_nodes as f64) };

    Ok(IncrementalExpectationResult {
        full_parse_equivalent: incremental_ast == full_ast,
        edit_apply_equivalent: apply_result.edit_apply_equivalent,
        fallback_used: apply_result.fallback_used,
        checkpoint_hit_count: apply_result.checkpoint_hit_count,
        checkpoint_miss_count: apply_result.checkpoint_miss_count,
        reparse_byte_ratio: apply_result.reparse_byte_ratio,
        reused_token_ratio: apply_result.reused_token_ratio,
        reused_node_ratio,
        changed_range_correct: apply_result.changed_range_correct,
    })
}

#[derive(Debug, Clone, PartialEq)]
struct ResolvedIncrementalEdit {
    start_byte: usize,
    old_end_byte: usize,
    new_end_byte: usize,
    new_text: String,
    core_edit: CoreEdit,
}

#[derive(Debug, Clone, Default, PartialEq)]
struct IncrementalApplyResult {
    edit_apply_equivalent: bool,
    fallback_used: bool,
    checkpoint_hit_count: u64,
    checkpoint_miss_count: u64,
    reparse_byte_ratio: Option<f64>,
    reused_token_ratio: Option<f64>,
    changed_range_correct: Option<bool>,
}

fn run_incremental_apply_path(
    source: &str,
    resolved_edits: &[ResolvedIncrementalEdit],
) -> Result<IncrementalApplyResult> {
    let mut state = IncrementalState::new(source.to_string());
    let expected_source = apply_resolved_edits(source, resolved_edits)?;
    let mut result = IncrementalApplyResult::default();
    let mut total_reparsed_bytes = 0usize;
    let mut total_reused_tokens = 0usize;
    let mut total_tokens = 0usize;
    let mut changed_ranges_cover_edits = true;

    for edit in resolved_edits {
        if state.find_lex_checkpoint(edit.start_byte).is_some() {
            result.checkpoint_hit_count += 1;
        } else {
            result.checkpoint_miss_count += 1;
        }
        let text_edit = TextEdit {
            start_byte: edit.start_byte,
            old_end_byte: edit.old_end_byte,
            new_end_byte: edit.new_end_byte,
            new_text: edit.new_text.clone(),
        };
        let reparse =
            apply_edits(&mut state, &[text_edit]).map_err(|error| eyre!(error.to_string()))?;
        if reparse
            .changed_ranges
            .iter()
            .any(|range| range.start == 0 && range.end == state.source.len())
        {
            result.fallback_used = true;
        }
        total_reparsed_bytes += reparse.reparsed_bytes;
        total_reused_tokens += reparse.reused_tokens;
        total_tokens += reparse.token_count;
        let expected_range = edit.start_byte..edit.new_end_byte;
        changed_ranges_cover_edits &= reparse
            .changed_ranges
            .iter()
            .any(|range| range.start <= expected_range.start && range.end >= expected_range.end);
    }

    result.edit_apply_equivalent = state.source == expected_source;
    if !resolved_edits.is_empty() {
        result.changed_range_correct = Some(changed_ranges_cover_edits);
    }
    if !state.source.is_empty() {
        result.reparse_byte_ratio = Some(total_reparsed_bytes as f64 / state.source.len() as f64);
    }
    if total_tokens > 0 {
        result.reused_token_ratio = Some(total_reused_tokens as f64 / total_tokens as f64);
    }
    Ok(result)
}

fn resolve_incremental_edits(
    source: &str,
    edits: &[IncrementalEditExpectation],
) -> Result<Vec<ResolvedIncrementalEdit>> {
    let mut current = source.to_string();
    let mut resolved = Vec::new();
    for edit in edits {
        let occurrence = edit.occurrence.unwrap_or(1);
        let start =
            find_text_occurrence(&current, &edit.old_text, occurrence).ok_or_else(|| {
                eyre!("could not find edit text '{}' occurrence {}", edit.old_text, occurrence)
            })?;
        let old_end = start + edit.old_text.len();
        let new_end = start + edit.new_text.len();
        let mut next = current.clone();
        next.replace_range(start..old_end, &edit.new_text);
        let core_edit = CoreEdit::new(
            start,
            old_end,
            new_end,
            position_at(&current, start)?,
            position_at(&current, old_end)?,
            position_at(&next, new_end)?,
        );
        resolved.push(ResolvedIncrementalEdit {
            start_byte: start,
            old_end_byte: old_end,
            new_end_byte: new_end,
            new_text: edit.new_text.clone(),
            core_edit,
        });
        current = next;
    }
    Ok(resolved)
}

fn apply_resolved_edits(source: &str, edits: &[ResolvedIncrementalEdit]) -> Result<String> {
    let mut current = source.to_string();
    for edit in edits {
        current.replace_range(edit.start_byte..edit.old_end_byte, &edit.new_text);
    }
    Ok(current)
}

fn find_text_occurrence(source: &str, needle: &str, occurrence: u64) -> Option<usize> {
    if needle.is_empty() || occurrence == 0 {
        return None;
    }
    let mut seen = 0u64;
    for (offset, _) in source.match_indices(needle) {
        seen += 1;
        if seen == occurrence {
            return Some(offset);
        }
    }
    None
}

fn position_at(source: &str, byte: usize) -> Result<Position> {
    if byte > source.len() || !source.is_char_boundary(byte) {
        bail!("byte offset {byte} is not a valid source boundary");
    }
    let mut position = Position::start();
    position.advance(&source[..byte]);
    Ok(position)
}

fn score_manifest_spans(
    root: &Path,
    manifest: &ParserAccuracyManifest,
    source_cache: &mut MetricSourceCache,
) -> Result<SpanScore> {
    let mut score = SpanScore::default();
    for fixture in &manifest.fixtures {
        if fixture.span_expectations.is_empty() {
            continue;
        }
        let source_path = root.join(&fixture.source_path);
        let source = source_cache.read(&source_path, "span fixture source")?;
        for expectation in &fixture.span_expectations {
            score_span_expectation(source, expectation, &mut score);
        }
    }
    Ok(score)
}

fn score_manifest_unsupported(
    root: &Path,
    manifest: &ParserAccuracyManifest,
    line_score: &LineScore,
    source_cache: &mut MetricSourceCache,
) -> Result<UnsupportedScore> {
    let mut score = UnsupportedScore {
        line_labeled_construct_count: line_score.expected_unsupported_construct_count,
        detected_count: line_score.correct_unsupported_construct_count,
        ..UnsupportedScore::default()
    };
    let mut families = BTreeSet::new();

    for fixture in &manifest.fixtures {
        if fixture.unsupported_constructs == 0 {
            continue;
        }
        score.manifest_construct_count += fixture.unsupported_constructs;
        families.insert(fixture.family.clone());

        if fixture.symbol_expectations.entities.is_empty()
            && fixture.symbol_expectations.occurrences.is_empty()
            && fixture.symbol_expectations.edges.is_empty()
        {
            continue;
        }

        let source_path = root.join(&fixture.source_path);
        let source = source_cache.read(&source_path, "unsupported fixture source")?;
        let predictions = extract_symbol_predictions(&source_path, source)?;
        score_unsupported_symbol_expectations(
            &fixture.symbol_expectations,
            &predictions,
            &mut score,
        );
    }

    score.family_count = families.len() as u64;
    Ok(score)
}

fn score_unsupported_symbol_expectations(
    expectations: &SymbolExpectations,
    predictions: &SymbolPredictions,
    score: &mut UnsupportedScore,
) {
    let expected_entities = expectations
        .entities
        .iter()
        .map(entity_key_from_expectation)
        .filter(is_conservative_symbol_entity)
        .collect::<BTreeSet<_>>();
    let expected_occurrences = expectations
        .occurrences
        .iter()
        .map(occurrence_key_from_expectation)
        .filter(is_conservative_symbol_occurrence)
        .collect::<BTreeSet<_>>();
    let expected_edges = expectations
        .edges
        .iter()
        .map(edge_key_from_expectation)
        .filter(is_conservative_symbol_edge)
        .collect::<BTreeSet<_>>();

    score.false_exact_sample_count +=
        (expected_entities.len() + expected_occurrences.len() + expected_edges.len()) as u64;
    score.salvaged_count += expected_entities.intersection(&predictions.entities).count() as u64;
    score.salvaged_count +=
        expected_occurrences.intersection(&predictions.occurrences).count() as u64;
    score.salvaged_count += expected_edges.intersection(&predictions.edges).count() as u64;

    for expected in &expected_entities {
        if predictions.entities.iter().any(|prediction| {
            prediction.span_text == expected.span_text
                && prediction.provenance == "ExactAst"
                && prediction.confidence == "High"
        }) {
            score.false_exact_count += 1;
        }
    }
    for expected in &expected_occurrences {
        if predictions.occurrences.iter().any(|prediction| {
            prediction.span_text == expected.span_text
                && prediction.canonical_name.is_some()
                && prediction.provenance == "ExactAst"
                && prediction.confidence == "High"
        }) {
            score.false_exact_count += 1;
        }
    }
    for expected in &expected_edges {
        if predictions.edges.iter().any(|prediction| {
            prediction.from == expected.from
                && prediction.to == expected.to
                && prediction.provenance == "ExactAst"
                && prediction.confidence == "High"
        }) {
            score.false_exact_count += 1;
        }
    }
}

fn is_conservative_symbol_entity(entity: &SymbolEntityKey) -> bool {
    entity.provenance != "ExactAst" || entity.confidence != "High"
}

fn is_conservative_symbol_occurrence(occurrence: &SymbolOccurrenceKey) -> bool {
    occurrence.provenance != "ExactAst" || occurrence.confidence != "High"
}

fn is_conservative_symbol_edge(edge: &SymbolEdgeKey) -> bool {
    edge.provenance != "ExactAst" || edge.confidence != "High"
}

fn score_method_completion_provider_expectations(
    root: &Path,
    manifest: &ParserAccuracyManifest,
    source_cache: &mut MetricSourceCache,
) -> Result<MethodCompletionProviderScore> {
    let mut score = MethodCompletionProviderScore::default();

    for fixture in &manifest.fixtures {
        if fixture.provider_expectations.method_completion.is_empty() {
            continue;
        }

        let source_path = root.join(&fixture.source_path);
        let source = source_cache.read(&source_path, "provider fixture source")?;
        let provider_source = provider_completion_source(source)?;
        let index_source = provider_completion_index_source(source)?;

        let index = Arc::new(WorkspaceIndex::new());
        let source_path_text = source_path.to_string_lossy();
        index.index_file_str(&source_path_text, &index_source).map_err(|err| {
            eyre!("indexing parser accuracy provider fixture {}: {err}", source_path.display())
        })?;

        let mut parser = Parser::new(&provider_source);
        let output = parser.parse_with_recovery();
        let provider = CompletionProvider::new_with_index_and_source(
            &output.ast,
            &provider_source,
            Some(index),
        );

        for expectation in &fixture.provider_expectations.method_completion {
            let cursor = locate_cursor_marker(source, &expectation.cursor_marker)
                .with_context(|| format!("locating cursor marker for {}", expectation.id))?;
            let query_start = Instant::now();
            let completions = provider.get_completions(&provider_source, cursor);
            score.completion_query_micros.push(query_start.elapsed().as_micros() as u64);
            let labels = completions
                .iter()
                .map(|item| item.label.as_ref().to_owned())
                .collect::<BTreeSet<_>>();
            score_method_completion_expectation(expectation, &labels, &mut score);
        }
    }

    Ok(score)
}

fn score_method_completion_expectation(
    expectation: &MethodCompletionProviderExpectation,
    labels: &BTreeSet<String>,
    score: &mut MethodCompletionProviderScore,
) {
    let expects_fallback =
        expectation.expected_fallback || expectation.expected_receiver_package.is_none();

    if expects_fallback {
        score.fallback_expected_count += 1;
        if expectation.expected_absent.iter().any(|label| labels.contains(label)) {
            score.false_receiver_count += 1;
        } else {
            score.fallback_correct_count += 1;
        }
    } else {
        score.receiver_expected_count += 1;
        if expectation.expected_present.iter().any(|label| labels.contains(label)) {
            score.receiver_hit_count += 1;
        }
    }

    for label in &expectation.expected_present {
        score.relevance_assertion_count += 1;
        if labels.contains(label) {
            score.relevance_assertion_correct_count += 1;
        }
    }
    for label in &expectation.expected_absent {
        score.relevance_assertion_count += 1;
        if !labels.contains(label) {
            score.relevance_assertion_correct_count += 1;
        }
    }

    if expectation.import_visibility {
        score.import_visibility_expected_count += 1;
        let present_labels_match =
            expectation.expected_present.iter().all(|label| labels.contains(label));
        let absent_labels_match =
            expectation.expected_absent.iter().all(|label| !labels.contains(label));
        if present_labels_match && absent_labels_match {
            score.import_visibility_correct_count += 1;
        }
    }
}

fn score_diagnostic_provider_expectations(
    root: &Path,
    manifest: &ParserAccuracyManifest,
    source_cache: &mut MetricSourceCache,
) -> Result<DiagnosticProviderScore> {
    let mut score = DiagnosticProviderScore::default();

    for fixture in &manifest.fixtures {
        if fixture.provider_expectations.diagnostics.is_empty() {
            continue;
        }

        let source_path = root.join(&fixture.source_path);
        let source = source_cache.read(&source_path, "diagnostic provider fixture source")?;
        let provider_source = provider_diagnostic_source(source)?;
        let index_source = provider_diagnostic_index_source(source)?;

        let index = WorkspaceIndex::new();
        let source_path_text = source_path.to_string_lossy();
        index.index_file_str(&source_path_text, &index_source).map_err(|err| {
            eyre!("indexing parser accuracy diagnostic fixture {}: {err}", source_path.display())
        })?;

        let mut parser = Parser::new(&provider_source);
        let output = parser.parse_with_recovery();
        let ast = Arc::new(output.ast);
        let provider = DiagnosticsProvider::new();
        let diagnostics = index
            .with_semantic_queries_for_uri(&source_path_text, |file_id, semantic_queries| {
                provider.get_diagnostics_with_path_and_semantics(
                    &ast,
                    &output.diagnostics,
                    &provider_source,
                    None,
                    &[],
                    Some(&source_path),
                    file_id,
                    &semantic_queries,
                )
            })
            .ok_or_else(|| {
                eyre!(
                    "missing semantic queries for parser accuracy diagnostic fixture {}",
                    source_path.display()
                )
            })?;

        for expectation in &fixture.provider_expectations.diagnostics {
            score_diagnostic_expectation(expectation, &diagnostics, &mut score);
        }
    }

    Ok(score)
}

fn score_diagnostic_expectation(
    expectation: &DiagnosticProviderExpectation,
    diagnostics: &[Diagnostic],
    score: &mut DiagnosticProviderScore,
) {
    let matched = diagnostics.iter().any(|diagnostic| {
        diagnostic.code.as_deref() == Some(expectation.expected_code.as_str())
            && diagnostic.message.contains(&expectation.message_contains)
    });

    if expectation.expected_present {
        score.undefined_expected_present_count += 1;
        if !matched {
            score.undefined_false_negative_count += 1;
        }
    } else if expectation.dynamic_boundary {
        score.dynamic_boundary_expected_absent_count += 1;
        if matched {
            score.dynamic_boundary_false_positive_count += 1;
        }
    } else {
        score.undefined_expected_absent_count += 1;
        if matched {
            score.undefined_false_positive_count += 1;
        }
    }
}

fn score_navigation_provider_expectations(
    root: &Path,
    manifest: &ParserAccuracyManifest,
    source_cache: &mut MetricSourceCache,
) -> Result<NavigationProviderScore> {
    let mut score = NavigationProviderScore::default();

    for fixture in &manifest.fixtures {
        if fixture.provider_expectations.navigation.is_empty() {
            continue;
        }

        let source_path = root.join(&fixture.source_path);
        let source = source_cache.read(&source_path, "navigation provider fixture source")?;
        let provider_source = provider_navigation_source(source)?;
        let index_source = provider_navigation_index_source(source)?;

        let index = WorkspaceIndex::new();
        let source_path_text = source_path.to_string_lossy();
        index.index_file_str(&source_path_text, &index_source).map_err(|err| {
            eyre!("indexing parser accuracy navigation fixture {}: {err}", source_path.display())
        })?;
        let shard = index.file_fact_shard(&source_path_text).ok_or_else(|| {
            eyre!(
                "missing semantic fact shard for parser accuracy navigation fixture {}",
                source_path.display()
            )
        })?;
        let document_symbol_spans = navigation_document_symbol_spans(
            &provider_source,
            &index.file_symbols(&source_path_text),
        );
        let anchors_by_id =
            shard.anchors.iter().map(|anchor| (anchor.id, anchor)).collect::<BTreeMap<_, _>>();

        for expectation in &fixture.provider_expectations.navigation {
            score_navigation_document_symbols(expectation, &document_symbol_spans, &mut score);
            score_navigation_goto_definition(
                expectation,
                source,
                &index_source,
                &index,
                &source_path_text,
                &anchors_by_id,
                &mut score,
            )?;
            score_navigation_references(
                expectation,
                &index_source,
                &index,
                &source_path_text,
                &shard,
                &anchors_by_id,
                &mut score,
            )?;
            score_navigation_hover(expectation, source, &index, &source_path_text, &mut score)?;
            score_navigation_rename_safe_edit(
                expectation,
                &index,
                &source_path_text,
                &shard,
                &mut score,
            )?;
            score_navigation_safe_delete_blocker(
                expectation,
                &index,
                &source_path_text,
                &shard,
                &mut score,
            )?;
        }
    }

    Ok(score)
}

fn navigation_document_symbol_spans(
    source: &str,
    symbols: &[perl_workspace::workspace::workspace_index::WorkspaceSymbol],
) -> BTreeSet<String> {
    symbols
        .iter()
        .filter_map(|symbol| range_span_text(source, &symbol.range))
        .filter(|span| !span.trim().is_empty())
        .collect()
}

fn score_navigation_document_symbols(
    expectation: &NavigationProviderExpectation,
    document_symbol_spans: &BTreeSet<String>,
    score: &mut NavigationProviderScore,
) {
    if expectation.expected_document_symbols.is_empty() {
        return;
    }

    let expected_spans =
        expectation.expected_document_symbols.iter().cloned().collect::<BTreeSet<_>>();

    score.document_symbol_expected_count += expected_spans.len() as u64;
    score.document_symbol_returned_count += document_symbol_spans.len() as u64;

    for expected_span in &expected_spans {
        if document_symbol_spans.contains(expected_span) {
            score.document_symbol_span_exact_count += 1;
        }
    }
}

fn score_navigation_goto_definition(
    expectation: &NavigationProviderExpectation,
    source: &str,
    index_source: &str,
    index: &WorkspaceIndex,
    source_path_text: &str,
    anchors_by_id: &BTreeMap<AnchorId, &perl_semantic_facts::AnchorFact>,
    score: &mut NavigationProviderScore,
) -> Result<()> {
    let Some(expected_span) = expectation.expected_definition_span.as_ref() else {
        return Ok(());
    };
    score.goto_definition_expected_count += 1;

    let cursor_offset = navigation_cursor_offset(source, expectation)?;
    let actual_spans = index
        .with_semantic_queries_for_uri(source_path_text, |file_id, semantic_queries| {
            let context = QueryContext::new(file_id, None, cursor_offset);
            let query_start = Instant::now();
            let outcome =
                goto_definition_cutover(index, &semantic_queries, &expectation.symbol, &context);
            score.definition_query_micros.push(query_start.elapsed().as_micros() as u64);
            definition_result_spans(index_source, &outcome.result, anchors_by_id)
        })
        .ok_or_else(|| eyre!("missing semantic queries for navigation fixture"))?;

    if actual_spans.is_empty() {
        return Ok(());
    }

    score.goto_definition_hit_count += 1;
    if actual_spans.contains(expected_span) {
        score.goto_definition_span_exact_count += 1;
    } else {
        score.goto_definition_false_target_count += 1;
    }
    Ok(())
}

fn score_navigation_references(
    expectation: &NavigationProviderExpectation,
    index_source: &str,
    index: &WorkspaceIndex,
    source_path_text: &str,
    shard: &FileFactShard,
    anchors_by_id: &BTreeMap<AnchorId, &perl_semantic_facts::AnchorFact>,
    score: &mut NavigationProviderScore,
) -> Result<()> {
    if expectation.expected_references.is_empty() && expectation.unexpected_references.is_empty() {
        return Ok(());
    }

    let entity_id = resolve_navigation_entity_id(shard, &expectation.symbol)
        .with_context(|| format!("resolving navigation entity for {}", expectation.id))?;
    let actual_spans = index
        .with_semantic_queries_for_uri(source_path_text, |_file_id, semantic_queries| {
            let query_start = Instant::now();
            let outcome =
                find_references_cutover(index, &semantic_queries, &expectation.symbol, entity_id);
            score.reference_query_micros.push(query_start.elapsed().as_micros() as u64);
            reference_result_spans(index_source, &outcome.result, anchors_by_id)
        })
        .ok_or_else(|| eyre!("missing semantic queries for navigation fixture"))?;

    let expected_spans = expectation.expected_references.iter().cloned().collect::<BTreeSet<_>>();
    let unexpected_spans =
        expectation.unexpected_references.iter().cloned().collect::<BTreeSet<_>>();
    let true_positive_count = actual_spans.intersection(&expected_spans).count() as u64;
    let unexpected_hit_count = actual_spans.intersection(&unexpected_spans).count() as u64;
    let extra_count = actual_spans
        .difference(&expected_spans)
        .filter(|span| !unexpected_spans.contains(*span))
        .count() as u64;

    score.references_expected_count += expected_spans.len() as u64;
    score.references_hit_count += true_positive_count;
    score.references_returned_count += actual_spans.len() as u64;
    score.references_absent_assertion_count += unexpected_spans.len() as u64;
    score.references_false_positive_count += unexpected_hit_count + extra_count;
    Ok(())
}

fn score_navigation_hover(
    expectation: &NavigationProviderExpectation,
    source: &str,
    index: &WorkspaceIndex,
    source_path_text: &str,
    score: &mut NavigationProviderScore,
) -> Result<()> {
    if expectation.hover_contains.is_empty() {
        return Ok(());
    }
    score.hover_expected_count += 1;

    let cursor_offset = navigation_cursor_offset(source, expectation)?;
    let hover_symbol = expectation
        .cursor_symbol
        .as_deref()
        .unwrap_or_else(|| bare_symbol_name(&expectation.symbol));
    let markdown = index
        .with_semantic_queries_for_uri(source_path_text, |file_id, semantic_queries| {
            let byte_offset = cursor_offset?;
            let outcome =
                hover_cutover(None, &semantic_queries, hover_symbol, file_id, byte_offset, None);
            hover_result_markdown(&outcome.result)
        })
        .ok_or_else(|| eyre!("missing semantic queries for navigation fixture"))?;

    if let Some(markdown) = markdown
        && expectation.hover_contains.iter().all(|expected| markdown.contains(expected))
    {
        score.hover_origin_correct_count += 1;
    }
    Ok(())
}

fn score_navigation_rename_safe_edit(
    expectation: &NavigationProviderExpectation,
    index: &WorkspaceIndex,
    source_path_text: &str,
    shard: &FileFactShard,
    score: &mut NavigationProviderScore,
) -> Result<()> {
    let Some(expected_safe_edit) = expectation.expected_rename_safe_edit else {
        return Ok(());
    };
    let new_name = expectation
        .rename_new_name
        .as_deref()
        .ok_or_else(|| eyre!("rename expectation {} is missing rename_new_name", expectation.id))?;

    score.rename_safe_edit_expected_count += 1;

    let entity_id = resolve_navigation_entity_id(shard, &expectation.symbol)
        .with_context(|| format!("resolving rename entity for {}", expectation.id))?;
    let bare_old_name = bare_symbol_name(&expectation.symbol);
    let outcome = index
        .with_semantic_queries_for_uri(source_path_text, |_file_id, semantic_queries| {
            rename_cutover(true, &semantic_queries, entity_id, new_name)
        })
        .ok_or_else(|| eyre!("missing semantic queries for navigation rename fixture"))?;

    let allowed = matches!(&outcome.result, RenameCutoverResult::Allowed { .. });
    let edits: &[perl_semantic_facts::PlannedEdit] = match &outcome.result {
        RenameCutoverResult::Allowed { edits } => edits.as_slice(),
        RenameCutoverResult::Blocked { .. } => &[],
    };
    let edit_count_matches = match expectation.expected_rename_edit_count {
        Some(expected_count) => edits.len() as u64 == expected_count,
        None => true,
    };
    let actual_safe_edit = allowed
        && !edits.is_empty()
        && edit_count_matches
        && edits.iter().all(|edit| edit.old_text == bare_old_name && edit.new_text == new_name);

    if actual_safe_edit == expected_safe_edit {
        score.rename_safe_edit_correct_count += 1;
    }
    Ok(())
}

fn score_navigation_safe_delete_blocker(
    expectation: &NavigationProviderExpectation,
    index: &WorkspaceIndex,
    source_path_text: &str,
    shard: &FileFactShard,
    score: &mut NavigationProviderScore,
) -> Result<()> {
    let Some(expected_blocked) = expectation.expected_safe_delete_blocked else {
        return Ok(());
    };

    score.safe_delete_blocker_expected_count += 1;

    let entity_id = resolve_navigation_entity_id(shard, &expectation.symbol)
        .with_context(|| format!("resolving safe-delete entity for {}", expectation.id))?;
    let outcome = index
        .with_semantic_queries_for_uri(source_path_text, |_file_id, semantic_queries| {
            safe_delete_cutover(true, &semantic_queries, entity_id, &expectation.symbol)
        })
        .ok_or_else(|| eyre!("missing semantic queries for navigation safe-delete fixture"))?;

    let actual_blocker_count = match &outcome.result {
        SafeDeleteCutoverResult::Allowed => 0,
        SafeDeleteCutoverResult::Blocked { blockers } => blockers.len() as u64,
    };
    let actual_blocked = actual_blocker_count > 0;
    let blocker_count_matches = match expectation.expected_safe_delete_blocker_count {
        Some(expected_count) => actual_blocker_count == expected_count,
        None => true,
    };

    if actual_blocked == expected_blocked && blocker_count_matches {
        score.safe_delete_blocker_correct_count += 1;
    }
    Ok(())
}

fn definition_result_spans(
    source: &str,
    result: &DefinitionCutoverResult,
    anchors_by_id: &BTreeMap<AnchorId, &perl_semantic_facts::AnchorFact>,
) -> BTreeSet<String> {
    match result {
        DefinitionCutoverResult::Exact(candidate) => anchors_by_id
            .get(&candidate.anchor_id)
            .map(|anchor| BTreeSet::from([anchor_text(source, anchor)]))
            .unwrap_or_default(),
        DefinitionCutoverResult::Ambiguous(candidates) => candidates
            .iter()
            .filter_map(|candidate| anchors_by_id.get(&candidate.anchor_id))
            .map(|anchor| anchor_text(source, anchor))
            .collect(),
        DefinitionCutoverResult::LegacyFallback(location) => location
            .as_ref()
            .and_then(|location| range_span_text(source, &location.range))
            .map(|span| BTreeSet::from([span]))
            .unwrap_or_default(),
    }
}

fn reference_result_spans(
    source: &str,
    result: &ReferencesCutoverResult,
    anchors_by_id: &BTreeMap<AnchorId, &perl_semantic_facts::AnchorFact>,
) -> BTreeSet<String> {
    match result {
        ReferencesCutoverResult::Exact(occurrences)
        | ReferencesCutoverResult::Ambiguous(occurrences) => occurrences
            .iter()
            .filter_map(|occurrence| anchors_by_id.get(&occurrence.anchor_id))
            .map(|anchor| anchor_text(source, anchor))
            .collect(),
        ReferencesCutoverResult::LegacyFallback(locations) => locations
            .iter()
            .filter_map(|location| range_span_text(source, &location.range))
            .collect(),
    }
}

fn hover_result_markdown(result: &HoverCutoverResult) -> Option<String> {
    match result {
        HoverCutoverResult::Exact(explanation)
        | HoverCutoverResult::Ambiguous(explanation)
        | HoverCutoverResult::DynamicBoundary(explanation) => Some(explanation.markdown.clone()),
        HoverCutoverResult::LegacyFallback(markdown) => markdown.clone(),
    }
}

fn navigation_cursor_offset(
    source: &str,
    expectation: &NavigationProviderExpectation,
) -> Result<Option<u32>> {
    let Some(marker) = expectation.cursor_marker.as_deref() else {
        return Ok(None);
    };
    let cursor_symbol = expectation
        .cursor_symbol
        .as_deref()
        .unwrap_or_else(|| bare_symbol_name(&expectation.symbol));
    let offset = locate_navigation_cursor_marker(source, marker, cursor_symbol)
        .with_context(|| format!("locating navigation cursor marker for {}", expectation.id))?;
    Ok(Some(
        u32::try_from(offset).with_context(|| {
            format!("navigation cursor offset for {} exceeds u32", expectation.id)
        })?,
    ))
}

fn locate_navigation_cursor_marker(source: &str, marker: &str, symbol: &str) -> Result<usize> {
    let marker_offset =
        source.find(marker).ok_or_else(|| eyre!("cursor marker '{marker}' was not found"))?;
    let line_start = source[..marker_offset].rfind('\n').map(|idx| idx + 1).unwrap_or(0);
    let comment_offset =
        source[line_start..marker_offset]
            .rfind('#')
            .map(|idx| line_start + idx)
            .ok_or_else(|| eyre!("cursor marker '{marker}' is not inside a line comment"))?;
    let line_prefix = &source[line_start..comment_offset];
    let symbol_offset = line_prefix
        .rfind(symbol)
        .ok_or_else(|| eyre!("cursor symbol '{symbol}' was not found before marker '{marker}'"))?;
    Ok(line_start + symbol_offset)
}

fn resolve_navigation_entity_id(shard: &FileFactShard, symbol: &str) -> Result<EntityId> {
    if let Some(entity) = shard.entities.iter().find(|entity| entity.canonical_name == symbol) {
        return Ok(entity.id);
    }

    let suffix = format!("::{symbol}");
    let mut candidates = shard
        .entities
        .iter()
        .filter(|entity| entity.canonical_name.ends_with(&suffix))
        .map(|entity| entity.id);
    let Some(first) = candidates.next() else {
        return Err(eyre!("symbol '{symbol}' was not found in navigation fact shard"));
    };
    if candidates.next().is_some() {
        return Err(eyre!("symbol '{symbol}' is ambiguous in navigation fact shard"));
    }
    Ok(first)
}

fn bare_symbol_name(symbol: &str) -> &str {
    match symbol.rsplit_once("::") {
        Some((_, bare)) => bare,
        None => symbol,
    }
}

fn range_span_text(source: &str, range: &Range) -> Option<String> {
    source.get(range.start.byte..range.end.byte).map(ToString::to_string)
}

fn provider_navigation_source(source: &str) -> Result<String> {
    let masked_support = mask_provider_index_support_blocks(source)?;
    mask_cursor_marker_comments(&masked_support)
}

fn provider_navigation_index_source(source: &str) -> Result<String> {
    mask_cursor_marker_comments(source)
}

fn provider_diagnostic_source(source: &str) -> Result<String> {
    let masked_support = mask_provider_index_support_blocks(source)?;
    mask_cursor_marker_comments(&masked_support)
}

fn provider_diagnostic_index_source(source: &str) -> Result<String> {
    mask_cursor_marker_comments(source)
}

fn provider_completion_source(source: &str) -> Result<String> {
    let masked_support = mask_provider_index_support_blocks(source)?;
    mask_cursor_marker_comments(&masked_support)
}

fn provider_completion_index_source(source: &str) -> Result<String> {
    let support_source = source_from_provider_index_support_blocks(source)?;
    mask_cursor_marker_comments(&support_source)
}

fn locate_cursor_marker(source: &str, marker: &str) -> Result<usize> {
    let marker_offset =
        source.find(marker).ok_or_else(|| eyre!("cursor marker '{marker}' was not found"))?;
    let line_start = source[..marker_offset].rfind('\n').map(|idx| idx + 1).unwrap_or(0);
    let comment_offset =
        source[line_start..marker_offset]
            .rfind('#')
            .map(|idx| line_start + idx)
            .ok_or_else(|| eyre!("cursor marker '{marker}' is not inside a line comment"))?;

    let mut cursor = comment_offset;
    let bytes = source.as_bytes();
    while cursor > line_start && matches!(bytes[cursor - 1], b' ' | b'\t') {
        cursor -= 1;
    }
    Ok(cursor)
}

fn mask_cursor_marker_comments(source: &str) -> Result<String> {
    let mut ranges = Vec::new();
    let mut search_start = 0usize;
    while let Some(relative_marker) = source[search_start..].find("cursor:") {
        let marker_offset = search_start + relative_marker;
        let line_start = source[..marker_offset].rfind('\n').map(|idx| idx + 1).unwrap_or(0);
        let Some(comment_offset) = source[line_start..marker_offset].rfind('#') else {
            search_start = marker_offset + "cursor:".len();
            continue;
        };
        let comment_offset = line_start + comment_offset;
        let line_end = source[marker_offset..]
            .find('\n')
            .map(|idx| marker_offset + idx)
            .unwrap_or(source.len());
        ranges.push(comment_offset..line_end);
        search_start = line_end;
    }
    mask_ranges_preserving_newlines(source, &ranges)
}

fn mask_provider_index_support_blocks(source: &str) -> Result<String> {
    mask_ranges_preserving_newlines(source, &provider_index_support_ranges(source)?)
}

fn source_from_provider_index_support_blocks(source: &str) -> Result<String> {
    let ranges = provider_index_support_ranges(source)?;
    if ranges.is_empty() {
        return Ok(source.to_string());
    }

    let mut bytes = source.as_bytes().to_vec();
    for byte in &mut bytes {
        if !matches!(*byte, b'\n' | b'\r') {
            *byte = b' ';
        }
    }
    for range in ranges {
        bytes[range.clone()].copy_from_slice(&source.as_bytes()[range]);
    }
    String::from_utf8(bytes).context("provider index support source must remain utf-8")
}

fn provider_index_support_ranges(source: &str) -> Result<Vec<std::ops::Range<usize>>> {
    const START: &str = "# provider-index-support:start";
    const END: &str = "# provider-index-support:end";

    let mut ranges = Vec::new();
    let mut search_start = 0usize;
    while let Some(relative_start) = source[search_start..].find(START) {
        let start = search_start + relative_start;
        let after_start = start + START.len();
        let relative_end = source[after_start..]
            .find(END)
            .ok_or_else(|| eyre!("provider index support block is missing end marker"))?;
        let end_marker = after_start + relative_end;
        let end =…40406 tokens truncated…               cursor_symbol: Some("own_sub".to_string()),
                            expected_document_symbols: vec![],
                            expected_definition_span: Some("own_sub".to_string()),
                            expected_references: vec![],
                            unexpected_references: vec![],
                            hover_contains: vec![],
                            rename_new_name: None,
                            expected_rename_safe_edit: None,
                            expected_rename_edit_count: None,
                            expected_safe_delete_blocked: None,
                            expected_safe_delete_blocker_count: None,
                        },
                        NavigationProviderExpectation {
                            id: "qualified_call_goto".to_string(),
                            symbol: "Accuracy::Navigation::UseCases::own_sub".to_string(),
                            cursor_marker: Some("cursor:qualified_call".to_string()),
                            cursor_symbol: Some("own_sub".to_string()),
                            expected_document_symbols: vec![],
                            expected_definition_span: Some("own_sub".to_string()),
                            expected_references: vec![],
                            unexpected_references: vec![],
                            hover_contains: vec![
                                "Accuracy::Navigation::UseCases::own_sub".to_string(),
                                "Subroutine".to_string(),
                            ],
                            rename_new_name: None,
                            expected_rename_safe_edit: None,
                            expected_rename_edit_count: None,
                            expected_safe_delete_blocked: None,
                            expected_safe_delete_blocker_count: None,
                        },
                        NavigationProviderExpectation {
                            id: "imported_symbol_goto_and_hover".to_string(),
                            symbol: "imported_nav".to_string(),
                            cursor_marker: Some("cursor:imported_nav".to_string()),
                            cursor_symbol: Some("imported_nav".to_string()),
                            expected_document_symbols: vec![],
                            expected_definition_span: Some("imported_nav".to_string()),
                            expected_references: vec![],
                            unexpected_references: vec![],
                            hover_contains: vec![],
                            rename_new_name: None,
                            expected_rename_safe_edit: None,
                            expected_rename_edit_count: None,
                            expected_safe_delete_blocked: None,
                            expected_safe_delete_blocker_count: None,
                        },
                        NavigationProviderExpectation {
                            id: "own_sub_references".to_string(),
                            symbol: "Accuracy::Navigation::UseCases::own_sub".to_string(),
                            cursor_marker: None,
                            cursor_symbol: None,
                            expected_document_symbols: vec![],
                            expected_definition_span: None,
                            expected_references: vec![
                                "Accuracy::Navigation::UseCases::own_sub()".to_string(),
                            ],
                            unexpected_references: vec!["&$name()".to_string()],
                            hover_contains: vec![],
                            rename_new_name: None,
                            expected_rename_safe_edit: None,
                            expected_rename_edit_count: None,
                            expected_safe_delete_blocked: None,
                            expected_safe_delete_blocker_count: None,
                        },
                        NavigationProviderExpectation {
                            id: "own_sub_rename_safe_edits".to_string(),
                            symbol: "Accuracy::Navigation::UseCases::own_sub".to_string(),
                            cursor_marker: None,
                            cursor_symbol: None,
                            expected_document_symbols: vec![],
                            expected_definition_span: None,
                            expected_references: vec![],
                            unexpected_references: vec![],
                            hover_contains: vec![],
                            rename_new_name: Some("renamed_sub".to_string()),
                            expected_rename_safe_edit: Some(true),
                            expected_rename_edit_count: Some(3),
                            expected_safe_delete_blocked: None,
                            expected_safe_delete_blocker_count: None,
                        },
                        NavigationProviderExpectation {
                            id: "own_sub_safe_delete_blocked".to_string(),
                            symbol: "Accuracy::Navigation::UseCases::own_sub".to_string(),
                            cursor_marker: None,
                            cursor_symbol: None,
                            expected_document_symbols: vec![],
                            expected_definition_span: None,
                            expected_references: vec![],
                            unexpected_references: vec![],
                            hover_contains: vec![],
                            rename_new_name: None,
                            expected_rename_safe_edit: None,
                            expected_rename_edit_count: None,
                            expected_safe_delete_blocked: Some(true),
                            expected_safe_delete_blocker_count: Some(1),
                        },
                    ],
                },
            }],
        }
    }

    #[test]
    fn denominator_counts_manifest_inventory() {
        let denominator = compute_denominator(&fixture_manifest());
        assert_eq!(denominator.fixture_count, 2);
        assert_eq!(denominator.fixture_family_count, 2);
        assert_eq!(denominator.scored_line_count, 5);
        assert_eq!(denominator.scored_symbol_count, 2);
        assert_eq!(denominator.unknown_region_count, 1);
        assert_eq!(denominator.negative_region_count, 1);
        assert_eq!(denominator.dynamic_boundary_case_count, 1);
        assert_eq!(denominator.hand_labeled_fixture_count, 2);
    }

    #[test]
    fn metric_source_cache_reports_reused_fixture_source_hits() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let source_path = tmp.path().join("fixture.pl");
        fs::write(&source_path, "package Cache;\n1;\n")?;
        let mut source_cache = MetricSourceCache::default();

        assert_eq!(source_cache.hit_rate(), None);
        assert_eq!(source_cache.read(&source_path, "test fixture source")?, "package Cache;\n1;\n");
        assert_eq!(source_cache.hit_rate(), Some(0.0));
        assert_eq!(source_cache.read(&source_path, "test fixture source")?, "package Cache;\n1;\n");
        assert_eq!(source_cache.hit_rate(), Some(0.5));

        Ok(())
    }

    #[test]
    fn measured_memory_prefers_positive_rss_delta_and_falls_back_to_allocator_peak() {
        assert_eq!(measured_memory_mb(Some(10.0), Some(13.5), 1.0), Some(3.5));
        assert_eq!(measured_memory_mb(Some(13.5), Some(10.0), 2.25), Some(2.25));
        assert_eq!(measured_memory_mb(None, Some(10.0), 0.0), None);
    }

    #[test]
    fn family_summary_groups_label_modes() -> Result<()> {
        let families = summarize_families(&fixture_manifest());
        assert_eq!(families.len(), 2);
        let dynamic = families
            .iter()
            .find(|family| family.family == "dynamic_require")
            .ok_or_else(|| color_eyre::eyre::eyre!("dynamic family should exist"))?;
        assert_eq!(dynamic.fixture_count, 1);
        assert_eq!(dynamic.label_modes, vec![LabelMode::Partial]);
        assert_eq!(dynamic.dynamic_boundary_case_count, 1);
        Ok(())
    }

    #[test]
    fn method_completion_provider_scorer_measures_receiver_and_fallback() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        write_method_completion_provider_fixture(tmp.path())?;
        let mut source_cache = MetricSourceCache::default();

        let score = score_method_completion_provider_expectations(
            tmp.path(),
            &method_completion_provider_manifest(),
            &mut source_cache,
        )?;

        assert_eq!(score.receiver_expected_count, 1);
        assert_eq!(score.receiver_hit_count, 1);
        assert_eq!(score.fallback_expected_count, 2);
        assert_eq!(score.fallback_correct_count, 2);
        assert_eq!(score.false_receiver_count, 0);
        assert_eq!(score.relevance_assertion_count, 9);
        assert_eq!(score.relevance_assertion_correct_count, 9);
        assert_eq!(score.import_visibility_expected_count, 1);
        assert_eq!(score.import_visibility_correct_count, 1);
        assert_eq!(score.completion_query_micros.len(), 3);

        let metrics = provider_impact_metrics(
            &score,
            &DiagnosticProviderScore::default(),
            &NavigationProviderScore::default(),
            Cadence::Pr,
        );
        let false_receiver = metrics
            .iter()
            .find(|metric| {
                matches!(
                    metric,
                    MetricRow::Measured { metric, .. }
                        if metric == "method_completion_false_receiver_count"
                )
            })
            .ok_or_else(|| eyre!("method completion false receiver row should be measured"))?;
        assert!(matches!(
            false_receiver,
            MetricRow::Measured { value, sample_count: 2, .. }
                if (*value - 0.0).abs() < f64::EPSILON
        ));
        let visible_symbol_relevance = metrics
            .iter()
            .find(|metric| {
                matches!(
                    metric,
                    MetricRow::Measured { metric, .. }
                        if metric == "provider_completion_visible_symbol_relevance"
                )
            })
            .ok_or_else(|| eyre!("provider completion visible-symbol row should be measured"))?;
        assert!(matches!(
            visible_symbol_relevance,
            MetricRow::Measured { value, sample_count: 9, .. }
                if (*value - 1.0).abs() < f64::EPSILON
        ));
        let import_visibility = metrics
            .iter()
            .find(|metric| {
                matches!(
                    metric,
                    MetricRow::Measured { metric, .. }
                        if metric == "provider_completion_import_visibility_accuracy"
                )
            })
            .ok_or_else(|| eyre!("provider completion import visibility row should be measured"))?;
        assert!(matches!(
            import_visibility,
            MetricRow::Measured { value, sample_count: 1, .. }
                if (*value - 1.0).abs() < f64::EPSILON
        ));
        let cost_metrics = cost_metrics(
            &ScaleCostScore::default(),
            &RecoveryScore::default(),
            &score,
            &NavigationProviderScore::default(),
            Cadence::Pr,
        );
        let completion_query_ms = cost_metrics
            .iter()
            .find(|metric| {
                matches!(
                    metric,
                    MetricRow::Measured { metric, .. } if metric == "completion_query_ms_p95"
                )
            })
            .ok_or_else(|| eyre!("completion query timing row should be measured"))?;
        assert!(matches!(
            completion_query_ms,
            MetricRow::Measured { value, sample_count: 3, .. } if *value >= 0.0
        ));
        Ok(())
    }

    #[test]
    fn diagnostic_provider_scorer_measures_false_positive_and_negative_rows() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        write_diagnostic_provider_fixture(tmp.path())?;
        let mut source_cache = MetricSourceCache::default();

        let score = score_diagnostic_provider_expectations(
            tmp.path(),
            &diagnostic_provider_manifest(),
            &mut source_cache,
        )?;

        assert_eq!(score.dynamic_boundary_expected_absent_count, 4);
        assert_eq!(score.dynamic_boundary_false_positive_count, 0);
        assert_eq!(score.undefined_expected_absent_count, 2);
        assert_eq!(score.undefined_false_positive_count, 0);
        assert_eq!(score.undefined_expected_present_count, 2);
        assert_eq!(score.undefined_false_negative_count, 0);

        let metrics = provider_impact_metrics(
            &MethodCompletionProviderScore::default(),
            &score,
            &NavigationProviderScore::default(),
            Cadence::Pr,
        );
        let dynamic_false_positive = metrics
            .iter()
            .find(|metric| {
                matches!(
                    metric,
                    MetricRow::Measured { metric, .. }
                        if metric == "diagnostic_dynamic_boundary_false_positive_count"
                )
            })
            .ok_or_else(|| eyre!("diagnostic dynamic-boundary false-positive row should exist"))?;
        assert!(matches!(
            dynamic_false_positive,
            MetricRow::Measured { value, sample_count: 4, .. }
                if (*value - 0.0).abs() < f64::EPSILON
        ));
        let false_negative_rate = metrics
            .iter()
            .find(|metric| {
                matches!(
                    metric,
                    MetricRow::Measured { metric, .. }
                        if metric == "provider_diagnostic_false_negative_rate"
                )
            })
            .ok_or_else(|| eyre!("provider diagnostic false-negative row should be measured"))?;
        assert!(matches!(
            false_negative_rate,
            MetricRow::Measured { value, sample_count: 2, .. }
                if (*value - 0.0).abs() < f64::EPSILON
        ));
        let false_positive_rate = metrics
            .iter()
            .find(|metric| {
                matches!(
                    metric,
                    MetricRow::Measured { metric, .. }
                        if metric == "provider_diagnostic_false_positive_rate"
                )
            })
            .ok_or_else(|| eyre!("provider diagnostic false-positive row should be measured"))?;
        assert!(matches!(
            false_positive_rate,
            MetricRow::Measured { value, sample_count: 6, .. }
                if (*value - 0.0).abs() < f64::EPSILON
        ));
        Ok(())
    }

    #[test]
    fn navigation_provider_scorer_measures_provider_rows() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        write_navigation_provider_fixture(tmp.path())?;
        let mut source_cache = MetricSourceCache::default();

        let score = score_navigation_provider_expectations(
            tmp.path(),
            &navigation_provider_manifest(),
            &mut source_cache,
        )?;

        assert_eq!(score.document_symbol_expected_count, 11);
        assert_eq!(score.document_symbol_returned_count, 11);
        assert_eq!(score.document_symbol_span_exact_count, 11);
        assert_eq!(score.goto_definition_expected_count, 3);
        assert_eq!(score.goto_definition_hit_count, 3);
        assert_eq!(score.goto_definition_span_exact_count, 3);
        assert_eq!(score.goto_definition_false_target_count, 0);
        assert_eq!(score.definition_query_micros.len(), 3);
        assert_eq!(score.references_expected_count, 1);
        assert_eq!(score.references_hit_count, 1);
        assert_eq!(score.references_returned_count, 1);
        assert_eq!(score.references_false_positive_count, 0);
        assert_eq!(score.reference_query_micros.len(), 1);
        assert_eq!(score.hover_expected_count, 1);
        assert_eq!(score.hover_origin_correct_count, 1);
        assert_eq!(score.rename_safe_edit_expected_count, 1);
        assert_eq!(score.rename_safe_edit_correct_count, 0);
        assert_eq!(score.safe_delete_blocker_expected_count, 1);
        assert_eq!(score.safe_delete_blocker_correct_count, 1);

        let metrics = provider_impact_metrics(
            &MethodCompletionProviderScore::default(),
            &DiagnosticProviderScore::default(),
            &score,
            Cadence::Pr,
        );
        let goto_hit_rate = metrics
            .iter()
            .find(|metric| {
                matches!(
                    metric,
                    MetricRow::Measured { metric, .. } if metric == "goto_definition_hit_rate"
                )
            })
            .ok_or_else(|| eyre!("navigation goto-definition row should exist"))?;
        assert!(matches!(
            goto_hit_rate,
            MetricRow::Measured { value, sample_count: 3, .. }
                if (*value - 1.0).abs() < f64::EPSILON
        ));
        let document_symbol_precision = metrics
            .iter()
            .find(|metric| {
                matches!(
                    metric,
                    MetricRow::Measured { metric, .. }
                        if metric == "provider_document_symbol_precision"
                )
            })
            .ok_or_else(|| eyre!("provider document-symbol precision row should exist"))?;
        assert!(matches!(
            document_symbol_precision,
            MetricRow::Measured { value, sample_count: 11, .. }
                if (*value - 1.0).abs() < f64::EPSILON
        ));
        let document_symbol_recall = metrics
            .iter()
            .find(|metric| {
                matches!(
                    metric,
                    MetricRow::Measured { metric, .. }
                        if metric == "provider_document_symbol_recall"
                )
            })
            .ok_or_else(|| eyre!("provider document-symbol recall row should exist"))?;
        assert!(matches!(
            document_symbol_recall,
            MetricRow::Measured { value, sample_count: 11, .. }
                if (*value - 1.0).abs() < f64::EPSILON
        ));
        let goto_hit_rate = metrics
            .iter()
            .find(|metric| {
                matches!(
                    metric,
                    MetricRow::Measured { metric, .. }
                        if metric == "provider_goto_definition_hit_rate"
                )
            })
            .ok_or_else(|| eyre!("provider goto-definition hit-rate row should exist"))?;
        assert!(matches!(
            goto_hit_rate,
            MetricRow::Measured { value, sample_count: 3, .. }
                if (*value - 1.0).abs() < f64::EPSILON
        ));
        let hover_origin_accuracy = metrics
            .iter()
            .find(|metric| {
                matches!(
                    metric,
                    MetricRow::Measured { metric, .. }
                        if metric == "provider_hover_symbol_origin_accuracy"
                )
            })
            .ok_or_else(|| eyre!("provider hover symbol-origin row should exist"))?;
        assert!(matches!(
            hover_origin_accuracy,
            MetricRow::Measured { value, sample_count: 1, .. }
                if (*value - 1.0).abs() < f64::EPSILON
        ));
        let references_precision = metrics
            .iter()
            .find(|metric| {
                matches!(
                    metric,
                    MetricRow::Measured { metric, .. }
                        if metric == "provider_references_precision"
                )
            })
            .ok_or_else(|| eyre!("provider references precision row should exist"))?;
        assert!(matches!(
            references_precision,
            MetricRow::Measured { value, sample_count: 1, .. }
                if (*value - 1.0).abs() < f64::EPSILON
        ));
        let references_recall = metrics
            .iter()
            .find(|metric| {
                matches!(
                    metric,
                    MetricRow::Measured { metric, .. }
                        if metric == "provider_references_recall"
                )
            })
            .ok_or_else(|| eyre!("provider references recall row should exist"))?;
        assert!(matches!(
            references_recall,
            MetricRow::Measured { value, sample_count: 1, .. }
                if (*value - 1.0).abs() < f64::EPSILON
        ));
        let rename_safe_edit_accuracy = metrics
            .iter()
            .find(|metric| {
                matches!(
                    metric,
                    MetricRow::Measured { metric, .. }
                        if metric == "provider_rename_safe_edit_accuracy"
                )
            })
            .ok_or_else(|| eyre!("provider rename safe-edit row should exist"))?;
        assert!(matches!(
            rename_safe_edit_accuracy,
            MetricRow::Measured { value, sample_count: 1, .. }
                if (*value - 0.0).abs() < f64::EPSILON
        ));
        let safe_delete_blocker_accuracy = metrics
            .iter()
            .find(|metric| {
                matches!(
                    metric,
                    MetricRow::Measured { metric, .. }
                        if metric == "provider_safe_delete_blocker_accuracy"
                )
            })
            .ok_or_else(|| eyre!("provider safe-delete blocker row should exist"))?;
        assert!(matches!(
            safe_delete_blocker_accuracy,
            MetricRow::Measured { value, sample_count: 1, .. }
                if (*value - 1.0).abs() < f64::EPSILON
        ));
        let cost_metrics = cost_metrics(
            &ScaleCostScore::default(),
            &RecoveryScore::default(),
            &MethodCompletionProviderScore::default(),
            &score,
            Cadence::Pr,
        );
        let definition_query_ms = cost_metrics
            .iter()
            .find(|metric| {
                matches!(
                    metric,
                    MetricRow::Measured { metric, .. } if metric == "definition_query_ms_p95"
                )
            })
            .ok_or_else(|| eyre!("definition query timing row should be measured"))?;
        assert!(matches!(
            definition_query_ms,
            MetricRow::Measured { value, sample_count: 3, .. } if *value >= 0.0
        ));
        let reference_query_ms = cost_metrics
            .iter()
            .find(|metric| {
                matches!(
                    metric,
                    MetricRow::Measured { metric, .. } if metric == "reference_query_ms_p95"
                )
            })
            .ok_or_else(|| eyre!("reference query timing row should be measured"))?;
        assert!(matches!(
            reference_query_ms,
            MetricRow::Measured { value, sample_count: 1, .. } if *value >= 0.0
        ));
        Ok(())
    }

    #[test]
    fn line_tag_vocabulary_includes_required_contract() {
        assert_eq!(LINE_TAG_VOCABULARY.len(), 25);
        assert!(LINE_TAG_VOCABULARY.contains(&LineTag::PackageDecl));
        assert!(LINE_TAG_VOCABULARY.contains(&LineTag::RegexMatch));
        assert!(LINE_TAG_VOCABULARY.contains(&LineTag::Division));
        assert!(LINE_TAG_VOCABULARY.contains(&LineTag::DefinedOr));
        assert!(LINE_TAG_VOCABULARY.contains(&LineTag::DynamicBoundary));
        assert!(LINE_TAG_VOCABULARY.contains(&LineTag::UnsupportedConstruct));
    }

    #[test]
    fn parser_accuracy_manifest_accepts_slash_ambiguity_line_tags() -> Result<()> {
        let raw =
            include_str!("../../../../crates/perl-corpus/fixtures/parser_accuracy/manifest.json");
        let manifest: ParserAccuracyManifest = serde_json::from_str(raw)?;
        let fixture = manifest
            .fixtures
            .iter()
            .find(|fixture| fixture.id == "slash_ambiguity")
            .ok_or_else(|| eyre!("slash ambiguity fixture should exist"))?;

        let expected_by_line: BTreeMap<u64, BTreeSet<LineTag>> = fixture
            .line_expectations
            .iter()
            .map(|expectation| (expectation.line, expectation.expected_tags.clone()))
            .collect();

        assert!(expected_by_line.get(&5).is_some_and(|tags| tags.contains(&LineTag::Division)));
        assert!(expected_by_line.get(&7).is_some_and(|tags| tags.contains(&LineTag::RegexMatch)));
        assert!(expected_by_line.get(&8).is_some_and(|tags| tags.contains(&LineTag::DefinedOr)));
        Ok(())
    }

    #[test]
    fn line_scorer_counts_false_positive_and_false_negative() {
        let expected = tags(&[LineTag::PackageDecl, LineTag::SubDecl]);
        let actual = tags(&[LineTag::PackageDecl, LineTag::MethodCall]);
        let mut score = LineScore::default();

        score_line_tags(&expected, &actual, &mut score);

        assert_eq!(score.true_positive_count, 1);
        assert_eq!(score.false_positive_count, 1);
        assert_eq!(score.false_negative_count, 1);
        assert_eq!(score.exact_match_count, 0);
    }

    #[test]
    fn comparable_line_tags_accept_legacy_regex_expectation_for_regex_match() {
        let comparable =
            comparable_actual_line_tags(&tags(&[LineTag::Regex]), &tags(&[LineTag::RegexMatch]));

        assert_eq!(comparable, tags(&[LineTag::Regex]));
    }

    #[test]
    fn line_tags_classify_slash_ambiguity_operators() -> Result<()> {
        let actual_by_line = extract_line_tags(
            "package Accuracy::SlashAmbiguity;\n\n\
             sub classify_slashes {\n\
             my ($total, $count, $line, $maybe) = @_;\n\
             my $ratio = $total / $count;\n\
             my @parts = split /,/, $line;\n\
             my $matched = $line =~ /^ok:/;\n\
             my $fallback = $maybe // $ratio;\n\
             }\n",
        );

        let division_line =
            actual_by_line.get(&5).ok_or_else(|| eyre!("division line should have parser tags"))?;
        let match_line = actual_by_line
            .get(&7)
            .ok_or_else(|| eyre!("regex match line should have parser tags"))?;
        let defined_or_line = actual_by_line
            .get(&8)
            .ok_or_else(|| eyre!("defined-or line should have parser tags"))?;

        assert!(division_line.contains(&LineTag::Division));
        assert!(match_line.contains(&LineTag::RegexMatch));
        assert!(defined_or_line.contains(&LineTag::DefinedOr));
        Ok(())
    }

    #[test]
    fn line_tags_count_eval_as_function_like_call() -> Result<()> {
        let actual_by_line =
            extract_line_tags("package Accuracy::Eval;\n\nmy $code = '1';\neval $code;\n");
        let line_tags = actual_by_line
            .get(&4)
            .ok_or_else(|| eyre!("expected line 4 tags for eval expression"))?;

        assert!(line_tags.contains(&LineTag::FunctionCall));
        Ok(())
    }

    #[test]
    fn line_tags_normalize_diagnostic_lines_to_parse_error() -> Result<()> {
        for (operator, source) in [
            ("q", "package Accuracy::Unclosed;\n\nmy $message = q{still open\n"),
            ("qq", "package Accuracy::Unclosed;\n\nmy $message = qq{still open\n"),
            ("qw", "package Accuracy::Unclosed;\n\nmy @words = qw{still open\n"),
            ("s", "package Accuracy::Unclosed;\n\nmy $pattern = s/unterminated/foo;\n"),
            ("tr", "package Accuracy::Unclosed;\n\nmy $table = tr/a/b;\n"),
            ("y", "package Accuracy::Unclosed;\n\nmy $table = y/a/b;\n"),
        ] {
            let actual_by_line = extract_line_tags(source);
            let line_tags = actual_by_line
                .get(&3)
                .ok_or_else(|| eyre!("expected line 3 tags for unclosed {operator} expression"))?;
            let expected = tags(&[LineTag::ParseError]);

            assert!(line_tags.contains(&LineTag::ParseError));
            if matches!(operator, "q" | "qq" | "qw") {
                assert!(line_tags.contains(&LineTag::VariableDecl));
            }
            assert_eq!(comparable_actual_line_tags(&expected, line_tags), expected);
        }

        Ok(())
    }

    #[test]
    fn line_metrics_emit_measured_scores_and_insufficient_missing_denominators() {
        let mut score = LineScore::default();
        score_line_tags(
            &tags(&[LineTag::Import, LineTag::DynamicBoundary]),
            &tags(&[LineTag::Import, LineTag::DynamicBoundary]),
            &mut score,
        );

        let metrics = line_metrics(&score, Cadence::Pr);

        assert!(metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, value, sample_count: 2, .. }
                    if metric == "line_construct_f1" && (*value - 1.0).abs() < f64::EPSILON
            )
        }));
        assert!(metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::InsufficientData { metric, sample_count: 0, .. }
                    if metric == "line_unsupported_detection_rate"
            )
        }));
    }

    #[test]
    fn ast_scorer_counts_wrong_parent_child_edge() {
        let expectations = vec![AstExpectation {
            id: "sub_wrong_parent".to_string(),
            kind: "Subroutine".to_string(),
            line: 1,
            span_text: "sub answer { 42 }".to_string(),
            parent_kind: Some("Block".to_string()),
            depth: Some(1),
            operator: None,
            parent_operator: None,
        }];
        let predictions = vec![AstPrediction {
            kind: "Subroutine".to_string(),
            line: 1,
            span_text: "sub answer { 42 }".to_string(),
            parent_kind: Some("Program".to_string()),
            depth: 1,
            operator: None,
            parent_operator: None,
        }];
        let mut score = AstScore::default();

        score_ast_expectations(&expectations, &predictions, &mut score);

        assert_eq!(score.node_kind_true_positive_count, 1);
        assert_eq!(score.parent_child_expected_count, 1);
        assert_eq!(score.parent_child_correct_count, 0);
    }

    #[test]
    fn ast_scorer_counts_delimiter_pairing_from_gold_span() {
        let expectations = vec![AstExpectation {
            id: "subroutine".to_string(),
            kind: "Subroutine".to_string(),
            line: 3,
            span_text: "sub answer { 42 }".to_string(),
            parent_kind: Some("Program".to_string()),
            depth: Some(1),
            operator: None,
            parent_operator: None,
        }];
        let predictions = vec![AstPrediction {
            kind: "Subroutine".to_string(),
            line: 3,
            span_text: "sub answer { 42 }".to_string(),
            parent_kind: Some("Program".to_string()),
            depth: 1,
            operator: None,
            parent_operator: None,
        }];
        let mut score = AstScore::default();

        score_ast_expectations(&expectations, &predictions, &mut score);

        assert_eq!(score.delimiter_pairing_expected_count, 1);
        assert_eq!(score.delimiter_pairing_correct_count, 1);
        let metrics = ast_metrics(&score, Cadence::Pr);
        assert!(metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, value, sample_count: 1, .. }
                    if metric == "ast_delimiter_pairing_accuracy"
                        && (*value - 1.0).abs() < f64::EPSILON
            )
        }));
    }

    #[test]
    fn ast_scorer_prefers_best_shape_when_kind_and_line_are_ambiguous() -> Result<()> {
        let expectations = vec![AstExpectation {
            id: "operator_multiplication".to_string(),
            kind: "Binary".to_string(),
            line: 3,
            span_text: "2 * 3".to_string(),
            parent_kind: Some("Binary".to_string()),
            depth: Some(3),
            operator: Some("*".to_string()),
            parent_operator: Some("+".to_string()),
        }];
        let predictions = vec![
            AstPrediction {
                kind: "Binary".to_string(),
                line: 3,
                span_text: "1 + 2 * 3".to_string(),
                parent_kind: Some("VariableDeclaration".to_string()),
                depth: 2,
                operator: Some("+".to_string()),
                parent_operator: None,
            },
            AstPrediction {
                kind: "Binary".to_string(),
                line: 3,
                span_text: "2 * 3".to_string(),
                parent_kind: Some("Binary".to_string()),
                depth: 3,
                operator: Some("*".to_string()),
                parent_operator: Some("+".to_string()),
            },
        ];
        let mut score = AstScore::default();

        score_ast_expectations(&expectations, &predictions, &mut score);

        assert_eq!(score.node_kind_true_positive_count, 1);
        assert_eq!(score.span_exact_count, 1);
        assert_eq!(score.parent_child_correct_count, 1);
        assert_eq!(score.tree_depth_correct_count, 1);
        assert_eq!(score.operator_precedence_correct_count, 1);
        Ok(())
    }

    #[test]
    fn ast_metrics_emit_measured_scores_and_insufficient_missing_denominators() {
        let mut score = AstScore::default();
        score_ast_expectations(
            &[AstExpectation {
                id: "binary_precedence".to_string(),
                kind: "Binary".to_string(),
                line: 1,
                span_text: "2 * 3".to_string(),
                parent_kind: Some("Binary".to_string()),
                depth: Some(3),
                operator: Some("*".to_string()),
                parent_operator: Some("+".to_string()),
            }],
            &[AstPrediction {
                kind: "Binary".to_string(),
                line: 1,
                span_text: "2 * 3".to_string(),
                parent_kind: Some("Binary".to_string()),
                depth: 3,
                operator: Some("*".to_string()),
                parent_operator: Some("+".to_string()),
            }],
            &mut score,
        );

        let metrics = ast_metrics(&score, Cadence::Pr);

        assert!(metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, value, sample_count: 1, .. }
                    if metric == "ast_operator_precedence_accuracy"
                        && (*value - 1.0).abs() < f64::EPSILON
            )
        }));
        assert!(metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::InsufficientData { metric, sample_count: 0, .. }
                    if metric == "ast_delimiter_pairing_accuracy"
            )
        }));
    }

    #[test]
    fn symbol_scorer_counts_typeglob_hit_and_generated_accessor_gap() -> Result<()> {
        let expectations = SymbolExpectations {
            entities: vec![SymbolEntityExpectation {
                id: "generated_name".to_string(),
                kind: "GeneratedMember".to_string(),
                canonical_name: "Accuracy::GeneratedAccessor::name".to_string(),
                span_text: "name".to_string(),
                package: Some("Accuracy::GeneratedAccessor".to_string()),
                scope: None,
                provenance: "FrameworkSynthesis".to_string(),
                confidence: "Medium".to_string(),
            }],
            occurrences: vec![SymbolOccurrenceExpectation {
                id: "typeglob_alias".to_string(),
                kind: "TypeglobReference".to_string(),
                canonical_name: None,
                span_text: "*alias".to_string(),
                package: None,
                scope: None,
                provenance: "DynamicBoundary".to_string(),
                confidence: "Low".to_string(),
            }],
            edges: vec![],
        };
        let predictions = SymbolPredictions {
            entities: BTreeSet::new(),
            occurrences: [SymbolOccurrenceKey {
                kind: "TypeglobReference".to_string(),
                canonical_name: None,
                span_text: "*alias".to_string(),
                package: None,
                scope: None,
                provenance: "DynamicBoundary".to_string(),
                confidence: "Low".to_string(),
            }]
            .into_iter()
            .collect(),
            safety_spans: BTreeSet::new(),
            edges: BTreeSet::new(),
        };
        let mut score = SymbolScore::default();

        score_symbol_expectations(&expectations, &predictions, false, &mut score);

        assert_eq!(score.occurrence_true_positive_count, 1);
        assert_eq!(score.entity_false_negative_count, 1);
        let generated = score
            .entity_by_kind
            .get("GeneratedMember")
            .ok_or_else(|| eyre!("generated member score should be present"))?;
        assert_eq!(generated.false_negative_count, 1);
        Ok(())
    }

    #[test]
    fn parser_accuracy_entity_kind_splits_variable_declaration_scope() -> Result<()> {
        let source = concat!(
            "package Accuracy::Vars;\n",
            "our ($VERSION, @EXPORT_OK) = (1, qw(run));\n",
            "sub run { my ($alpha, $local) = (1, 2); }\n",
        );
        let entity = EntityFact {
            id: EntityId(1),
            kind: EntityKind::Variable,
            canonical_name: "Accuracy::Vars::EXPORT_OK".to_string(),
            anchor_id: Some(AnchorId(1)),
            scope_id: None,
            provenance: perl_semantic_facts::Provenance::ExactAst,
            confidence: perl_semantic_facts::Confidence::High,
        };
        let version_start =
            source.find("$VERSION").ok_or_else(|| eyre!("version fixture anchor missing"))?;
        let version_anchor = AnchorFact {
            id: AnchorId(1),
            file_id: perl_semantic_facts::FileId(1),
            span_start_byte: version_start as u32,
            span_end_byte: (version_start + "$VERSION".len()) as u32,
            scope_id: None,
            provenance: perl_semantic_facts::Provenance::ExactAst,
            confidence: perl_semantic_facts::Confidence::High,
        };
        let global_start =
            source.find("@EXPORT_OK").ok_or_else(|| eyre!("global fixture anchor missing"))?;
        let global_anchor = AnchorFact {
            span_start_byte: global_start as u32,
            span_end_byte: (global_start + "@EXPORT_OK".len()) as u32,
            ..version_anchor.clone()
        };
        let alpha_start =
            source.find("$alpha").ok_or_else(|| eyre!("alpha fixture anchor missing"))?;
        let alpha_anchor = AnchorFact {
            span_start_byte: alpha_start as u32,
            span_end_byte: (alpha_start + "$alpha".len()) as u32,
            ..version_anchor.clone()
        };
        let lexical_start =
            source.find("$local").ok_or_else(|| eyre!("lexical fixture anchor missing"))?;
        let lexical_anchor = AnchorFact {
            span_start_byte: lexical_start as u32,
            span_end_byte: (lexical_start + "$local".len()) as u32,
            ..version_anchor.clone()
        };

        assert_eq!(
            parser_accuracy_entity_kind(source, &entity, Some(&version_anchor)),
            "GlobalVariable"
        );
        assert_eq!(
            parser_accuracy_entity_kind(source, &entity, Some(&global_anchor)),
            "GlobalVariable"
        );
        assert_eq!(
            parser_accuracy_entity_kind(source, &entity, Some(&alpha_anchor)),
            "LexicalVariable"
        );
        assert_eq!(
            parser_accuracy_entity_kind(source, &entity, Some(&lexical_anchor)),
            "LexicalVariable"
        );
        assert_eq!(parser_accuracy_entity_kind(source, &entity, None), "Variable");
        Ok(())
    }

    #[test]
    fn parser_accuracy_entity_kind_projects_inherited_subroutine_declarations() {
        let source = concat!(
            "package Accuracy::Parent;\n",
            "sub inherited { 1 }\n",
            "package Accuracy::Child;\n",
            "our @ISA = qw(Accuracy::Parent);\n",
            "sub own { 1 }\n",
        );
        let inherited = EntityFact {
            id: EntityId(1),
            kind: EntityKind::Subroutine,
            canonical_name: "Accuracy::Parent::inherited".to_string(),
            anchor_id: None,
            scope_id: None,
            provenance: perl_semantic_facts::Provenance::ExactAst,
            confidence: perl_semantic_facts::Confidence::High,
        };
        let own = EntityFact {
            id: EntityId(2),
            canonical_name: "Accuracy::Child::own".to_string(),
            ..inherited.clone()
        };

        assert_eq!(parser_accuracy_entity_kind(source, &inherited, None), "InheritedMethod");
        assert_eq!(parser_accuracy_entity_kind(source, &own, None), "Subroutine");
    }

    #[test]
    fn parser_accuracy_entity_kind_projects_role_subroutine_declarations() {
        let role_method = EntityFact {
            id: EntityId(1),
            kind: EntityKind::Subroutine,
            canonical_name: "Accuracy::Role::provided".to_string(),
            anchor_id: None,
            scope_id: None,
            provenance: perl_semantic_facts::Provenance::ExactAst,
            confidence: perl_semantic_facts::Confidence::High,
        };
        let consumer_method = EntityFact {
            id: EntityId(2),
            canonical_name: "Accuracy::RoleConsumer::local_method".to_string(),
            ..role_method.clone()
        };

        assert_eq!(parser_accuracy_entity_kind("", &role_method, None), "RoleMethod");
        assert_eq!(parser_accuracy_entity_kind("", &consumer_method, None), "Subroutine");
    }

    #[test]
    fn scorecard_export_projection_collects_qw_export_names() {
        let source = concat!(
            "package Accuracy::Exports;\n",
            "our @EXPORT_OK = qw(answer helper);\n",
            "package Accuracy::Consumer;\n",
        );
        let mut predictions = SymbolPredictions::default();

        add_export_occurrence_predictions(source, &mut predictions);

        assert!(predictions.occurrences.contains(&SymbolOccurrenceKey {
            kind: "Export".to_string(),
            canonical_name: Some("Accuracy::Exports::answer".to_string()),
            span_text: "answer".to_string(),
            package: Some("Accuracy::Exports".to_string()),
            scope: None,
            provenance: "ImportExportInference".to_string(),
            confidence: "Medium".to_string(),
        }));
        assert!(predictions.occurrences.contains(&SymbolOccurrenceKey {
            kind: "Export".to_string(),
            canonical_name: Some("Accuracy::Exports::helper".to_string()),
            span_text: "helper".to_string(),
            package: Some("Accuracy::Exports".to_string()),
            scope: None,
            provenance: "ImportExportInference".to_string(),
            confidence: "Medium".to_string(),
        }));
    }

    #[test]
    fn scorecard_import_projection_collects_qw_import_names() {
        let source =
            concat!("package Accuracy::Consumer;\n", "use Accuracy::Exports qw(answer helper);\n",);
        let mut predictions = SymbolPredictions::default();

        add_import_occurrence_predictions(source, &mut predictions);

        assert!(predictions.occurrences.contains(&SymbolOccurrenceKey {
            kind: "Import".to_string(),
            canonical_name: Some("Accuracy::Exports::answer".to_string()),
            span_text: "answer".to_string(),
            package: Some("Accuracy::Exports".to_string()),
            scope: None,
            provenance: "ImportExportInference".to_string(),
            confidence: "Medium".to_string(),
        }));
        assert!(predictions.occurrences.contains(&SymbolOccurrenceKey {
            kind: "Import".to_string(),
            canonical_name: Some("Accuracy::Exports::helper".to_string()),
            span_text: "helper".to_string(),
            package: Some("Accuracy::Exports".to_string()),
            scope: None,
            provenance: "ImportExportInference".to_string(),
            confidence: "Medium".to_string(),
        }));
    }

    #[test]
    fn scorecard_import_projection_keeps_semicolon_separated_modules_distinct() {
        let source = concat!("use Accuracy::One qw(first); use Accuracy::Two qw[second];\n",);
        let mut predictions = SymbolPredictions::default();

        add_import_occurrence_predictions(source, &mut predictions);

        assert!(predictions.occurrences.contains(&SymbolOccurrenceKey {
            kind: "Import".to_string(),
            canonical_name: Some("Accuracy::One::first".to_string()),
            span_text: "first".to_string(),
            package: Some("Accuracy::One".to_string()),
            scope: None,
            provenance: "ImportExportInference".to_string(),
            confidence: "Medium".to_string(),
        }));
        assert!(predictions.occurrences.contains(&SymbolOccurrenceKey {
            kind: "Import".to_string(),
            canonical_name: Some("Accuracy::Two::second".to_string()),
            span_text: "second".to_string(),
            package: Some("Accuracy::Two".to_string()),
            scope: None,
            provenance: "ImportExportInference".to_string(),
            confidence: "Medium".to_string(),
        }));
        assert!(!predictions.occurrences.iter().any(|occurrence| {
            occurrence.canonical_name.as_deref() == Some("Accuracy::One::second")
        }));
    }

    #[test]
    fn qw_names_supports_multiple_delimiters_and_occurrences() {
        assert_eq!(
            qw_names("our @EXPORT_OK = qw(one two); our @EXPORT = qw[three four];"),
            vec!["one".to_string(), "two".to_string(), "three".to_string(), "four".to_string(),]
        );
        assert_eq!(
            qw_names("our @EXPORT_OK = qw{one two}; our @EXPORT = qw<three four>;"),
            vec!["one".to_string(), "two".to_string(), "three".to_string(), "four".to_string(),]
        );
    }

    #[test]
    fn qw_names_balances_nested_paired_delimiters() {
        assert_eq!(
            qw_names("our @EXPORT_OK = qw(foo(bar) baz);"),
            vec!["foo(bar)".to_string(), "baz".to_string()]
        );
    }

    #[test]
    fn qw_names_handles_empty_and_whitespace_bodies() {
        assert_eq!(
            qw_names("our @EXPORT_OK = qw(  answer   helper  );"),
            vec!["answer".to_string(), "helper".to_string()]
        );
        assert!(qw_names("our @EXPORT_OK = qw();").is_empty());
    }

    #[test]
    fn symbol_metrics_emit_measured_kind_rows() {
        let mut score = SymbolScore::default();
        score.entity_expected_count = 1;
        score.entity_true_positive_count = 1;
        score.entity_by_kind.insert(
            "Package".to_string(),
            KindScore {
                expected_count: 1,
                predicted_count: 1,
                true_positive_count: 1,
                false_positive_count: 0,
                false_negative_count: 0,
            },
        );

        let metrics = symbol_metrics(&score, Cadence::Pr);

        assert!(metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, value, sample_count: 1, .. }
                    if metric == "symbol_decl_package_f1"
                        && (*value - 1.0).abs() < f64::EPSILON
            )
        }));
        assert!(metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::InsufficientData { metric, sample_count: 0, .. }
                    if metric == "symbol_decl_generated_accessor_f1"
            )
        }));
    }

    #[test]
    fn dynamic_false_precision_counts_exact_resolution_for_dynamic_boundary() {
        let expectations = SymbolExpectations {
            entities: vec![],
            occurrences: vec![SymbolOccurrenceExpectation {
                id: "dynamic_require".to_string(),
                kind: "FunctionCall".to_string(),
                canonical_name: None,
                span_text: "require $module".to_string(),
                package: None,
                scope: None,
                provenance: "DynamicBoundary".to_string(),
                confidence: "Low".to_string(),
            }],
            edges: vec![],
        };
        let predictions = SymbolPredictions {
            entities: BTreeSet::new(),
            occurrences: [SymbolOccurrenceKey {
                kind: "FunctionCall".to_string(),
                canonical_name: Some("Accuracy::Plugin".to_string()),
                span_text: "require $module".to_string(),
                package: Some("Accuracy".to_string()),
                scope: None,
                provenance: "ExactAst".to_string(),
                confidence: "High".to_string(),
            }]
            .into_iter()
            .collect(),
            safety_spans: BTreeSet::new(),
            edges: BTreeSet::new(),
        };
        let mut score = SymbolScore::default();

        score_symbol_expectations(&expectations, &predictions, false, &mut score);

        assert_eq!(score.dynamic_false_precision_sample_count, 1);
        assert_eq!(score.dynamic_false_precision_count, 1);
    }

    #[test]
    fn symbol_safety_regions_count_comment_pod_string_and_unknown_hits() {
        let regions = vec![
            SymbolSafetyRegion {
                kind: SymbolSafetyRegionKind::Comment,
                line: 2,
                span_text: "commented_out".to_string(),
            },
            SymbolSafetyRegion {
                kind: SymbolSafetyRegionKind::Pod,
                line: 6,
                span_text: "podded".to_string(),
            },
            SymbolSafetyRegion {
                kind: SymbolSafetyRegionKind::String,
                line: 3,
                span_text: "stringy".to_string(),
            },
            SymbolSafetyRegion {
                kind: SymbolSafetyRegionKind::Unknown,
                line: 9,
                span_text: "dynamic_name".to_string(),
            },
        ];
        let predictions = SymbolPredictions {
            entities: BTreeSet::new(),
            occurrences: BTreeSet::new(),
            safety_spans: [
                SymbolSpanLocation { line: 2, span_text: "commented_out".to_string() },
                SymbolSpanLocation { line: 3, span_text: "stringy".to_string() },
            ]
            .into_iter()
            .collect(),
            edges: BTreeSet::new(),
        };
        let mut score = SymbolScore::default();

        score_symbol_safety_regions(&regions, &predictions, &mut score);

        assert_eq!(score.symbols_emitted_in_comments, 1);
        assert_eq!(score.symbols_emitted_in_strings, 1);
        assert_eq!(score.symbols_emitted_in_pod, 0);
        assert_eq!(score.symbols_emitted_in_unknown_regions, 0);
    }

    #[test]
    fn safety_metrics_emit_dynamic_false_precision_floor_candidate() {
        let line_score =
            LineScore { line_count: 2, false_parse_error_count: 0, ..LineScore::default() };
        let symbol_score = SymbolScore {
            false_positive_sample_count: 3,
            entity_false_positive_count: 1,
            occurrence_false_positive_count: 1,
            dynamic_false_precision_sample_count: 1,
            dynamic_false_precision_count: 0,
            comment_safety_region_count: 1,
            symbols_emitted_in_comments: 0,
            ..SymbolScore::default()
        };

        let metrics = safety_metrics(&line_score, &symbol_score, Cadence::Pr);

        assert!(metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, value, sample_count: 1, .. }
                    if metric == "dynamic_false_precision_count"
                        && (*value - 0.0).abs() < f64::EPSILON
            )
        }));
        assert!(metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, value, sample_count: 3, .. }
                    if metric == "false_symbol_count"
                        && (*value - 2.0).abs() < f64::EPSILON
            )
        }));
    }

    #[test]
    fn safety_floor_metadata_marks_only_zero_false_precision_candidates() {
        let mut metrics = vec![
            measured_count("dynamic_false_precision_count", 0, 1, Cadence::Pr),
            measured_count("fast_path_wrong_result_count", 0, 1, Cadence::Pr),
            measured_count("line_construct_f1", 1, 1, Cadence::Pr),
        ];

        apply_safety_floor_metadata(&mut metrics);

        for name in ["dynamic_false_precision_count", "fast_path_wrong_result_count"] {
            assert!(metrics.iter().any(|metric| {
                matches!(
                    metric,
                    MetricRow::Measured {
                        metric,
                        value,
                        previous: Some(0.0),
                        delta: Some(0.0),
                        floor: Some(0.0),
                        threshold: Some(0.0),
                        direction: Direction::Down,
                        sample_count: 1,
                        ..
                    } if metric == name && (*value - 0.0).abs() < f64::EPSILON
                )
            }));
        }
        assert!(metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured {
                    metric,
                    floor: None,
                    threshold: None,
                    direction: Direction::Neutral,
                    ..
                } if metric == "line_construct_f1"
            )
        }));
    }

    #[test]
    fn ratchet_receipt_keeps_precision_recall_metrics_out_of_floor_map() {
        let artifact = ParserAccuracyArtifact {
            schema_version: 1,
            subsystem: "parser_accuracy".to_string(),
            generated_at: "2026-05-03T00:00:00Z".to_string(),
            commit: "test".to_string(),
            cadence: Cadence::Pr,
            denominator: Denominator::default(),
            families: Vec::new(),
            metrics: vec![
                measured_count("dynamic_false_precision_count", 0, 1, Cadence::Pr),
                measured_count("fast_path_wrong_result_count", 0, 1, Cadence::Pr),
                measured_value("line_construct_f1", 0.875, 8, Cadence::Pr),
                measured_value("symbol_decl_precision", 0.9, 10, Cadence::Pr),
            ],
            failure_packets: Vec::new(),
            gold_drift: GoldDrift::default(),
            metric_runtime: MetricRuntime::default(),
        };

        let receipt = ratchet_receipt_for_artifact(&artifact);

        assert_eq!(receipt.floor_metrics.len(), 2);
        assert_eq!(receipt.floor_metrics.get("dynamic_false_precision_count"), Some(&Some(0.0)));
        assert_eq!(receipt.floor_metrics.get("fast_path_wrong_result_count"), Some(&Some(0.0)));
        assert!(
            !receipt.floor_metrics.contains_key("line_construct_f1"),
            "precision/recall rows must stay out of hard floors until sample counts stabilize"
        );
        assert_eq!(receipt.improvement_metrics.get("line_construct_f1"), Some(&Some(0.875)));
        assert_eq!(receipt.improvement_metrics.get("symbol_decl_precision"), Some(&Some(0.9)));
    }

    #[test]
    fn status_receipts_export_failure_packets_inventory_worklist_and_pointer() -> Result<()> {
        let manifest = fixture_manifest();
        let artifact = ParserAccuracyArtifact {
            schema_version: 1,
            subsystem: "parser_accuracy".to_string(),
            generated_at: "2026-05-05T00:00:00Z".to_string(),
            commit: "test-commit".to_string(),
            cadence: Cadence::Pr,
            denominator: compute_denominator(&manifest),
            families: summarize_families(&manifest),
            metrics: vec![measured_count("dynamic_false_precision_count", 0, 1, Cadence::Pr)],
            failure_packets: vec![FailurePacket {
                failure_kind: "missing_symbol_reference".to_string(),
                likely_layer: "semantic_fact_extraction".to_string(),
                fixture_id: "qualified_refs".to_string(),
                family: Some("qualified_references".to_string()),
                metric: Some("symbol_ref_f1".to_string()),
                line: Some(7),
                expected: vec!["Accuracy::Refs::target".to_string()],
                actual: vec![],
                nearest_predictions: vec!["Accuracy::Refs::nearby".to_string()],
                source_excerpt: Some("Accuracy::Refs::target();".to_string()),
                details: None,
                suggested_next_fix: Some(
                    "Inspect reference extraction before changing gold.".to_string(),
                ),
            }],
            gold_drift: GoldDrift::default(),
            metric_runtime: MetricRuntime::default(),
        };

        let failure_receipt = render_failure_packet_status_receipt(&artifact)?;
        assert!(failure_receipt.contains("\"failure_packet_count\": 1"));
        assert!(failure_receipt.contains("\"actual_nearest\""));
        assert!(failure_receipt.contains("\"suggested_next_pr\""));

        let inventory_receipt = render_fixture_inventory_status_receipt(&manifest, &artifact)?;
        assert!(inventory_receipt.contains("\"fixture_count\": 2"));
        assert!(inventory_receipt.contains("\"provider_expectation_counts\""));

        let worklist = render_failure_worklist_status_receipt(&artifact);
        assert!(worklist.contains("| missing_symbol_reference | 1 | semantic_fact_extraction |"));
        assert!(worklist.contains("fix(semantic): resolve parser-accuracy semantic fact packet"));

        let pointer = render_next_pointer_status_receipt(&artifact);
        assert!(pointer.contains("| Pointer | `missing_symbol_reference` |"));
        assert!(pointer.contains("| First fixture | `qualified_refs` |"));

        let files = status_receipt_files(Path::new("."), &manifest, &artifact)?;
        assert!(
            files.iter().any(|file| file.name == FAILURE_WORKLIST_STATUS_RECEIPT),
            "status export must include parser_accuracy_failure_worklist.md"
        );
        assert!(
            files.iter().any(|file| file.name == NEXT_POINTER_STATUS_RECEIPT),
            "status export must include parser_accuracy_next.md"
        );
        Ok(())
    }

    #[test]
    fn next_pointer_lists_measurement_gaps_when_failure_packets_are_empty() {
        let manifest = fixture_manifest();
        let artifact = ParserAccuracyArtifact {
            schema_version: 1,
            subsystem: "parser_accuracy".to_string(),
            generated_at: "2026-05-05T00:00:00Z".to_string(),
            commit: "test-commit".to_string(),
            cadence: Cadence::Pr,
            denominator: compute_denominator(&manifest),
            families: summarize_families(&manifest),
            metrics: vec![
                insufficient("gold_changed_line_count", "gold drift baseline is not wired yet"),
                insufficient("completion_query_ms_p95", "provider query timing is not wired yet"),
                insufficient(
                    "provider_goto_definition_hit_rate",
                    "provider gold fixtures are not wired yet",
                ),
            ],
            failure_packets: Vec::new(),
            gold_drift: GoldDrift::default(),
            metric_runtime: MetricRuntime::default(),
        };

        let pointer = render_next_pointer_status_receipt(&artifact);

        assert!(pointer.contains("Pointer: no active failure packets."));
        assert!(pointer.contains("## Next Measurement Gaps"));
        assert!(pointer.contains("provider_goto_definition_hit_rate"));
        assert!(pointer.contains(
            "test(parser-accuracy): wire provider gold fixture for provider_goto_definition_hit_rate"
        ));
        assert!(pointer.contains("completion_query_ms_p95"));
        assert!(!pointer.contains("## Capability Handoff"));
        assert!(matches!(
            (
                pointer.find("provider_goto_definition_hit_rate"),
                pointer.find("completion_query_ms_p95")
            ),
            (Some(provider_index), Some(timing_index)) if provider_index < timing_index
        ));
    }

    #[test]
    fn next_pointer_hands_off_when_measurement_queue_is_empty() {
        let manifest = fixture_manifest();
        let artifact = ParserAccuracyArtifact {
            schema_version: 1,
            subsystem: "parser_accuracy".to_string(),
            generated_at: "2026-05-05T00:00:00Z".to_string(),
            commit: "test-commit".to_string(),
            cadence: Cadence::Pr,
            denominator: compute_denominator(&manifest),
            families: summarize_families(&manifest),
            metrics: vec![measured_count("line_construct_f1", 1, 1, Cadence::Pr)],
            failure_packets: Vec::new(),
            gold_drift: GoldDrift::default(),
            metric_runtime: MetricRuntime::default(),
        };

        let pointer = render_next_pointer_status_receipt(&artifact);

        assert!(pointer.contains("Pointer: no active failure packets."));
        assert!(pointer.contains("| none | n/a | n/a |"));
        assert!(pointer.contains("## Capability Handoff"));
        assert!(pointer.contains("parser.md#raw-failure-buckets"));
        assert!(
            pointer.contains(
                "only when the generated parser status lists a nonzero raw failure bucket"
            )
        );
        assert!(pointer.contains("do not start parser bucket work from stale context"));
        assert!(matches!(
            (
                pointer.find("Use the measurement gap table only"),
                pointer.find("## Capability Handoff")
            ),
            (Some(guidance_index), Some(handoff_index)) if guidance_index < handoff_index
        ));
    }

    #[test]
    fn status_receipt_stale_check_ignores_commit_only() {
        let existing = r#"{"schema_version":1,"commit":"old","fixture_count":2}"#;
        let generated = r#"{"schema_version":1,"commit":"new","fixture_count":2}"#;
        let stale = r#"{"schema_version":1,"commit":"new","fixture_count":3}"#;

        assert!(status_receipt_equivalent_ignoring_commit(existing, generated));
        assert!(!status_receipt_equivalent_ignoring_commit(existing, stale));
    }

    #[test]
    fn recovery_scorer_counts_local_spillover_and_salvaged_symbols() {
        let expectation = RecoveryExpectation {
            id: "recover_before_sub".to_string(),
            first_error_line: 3,
            error_region: LineRange { start: 3, end: 3 },
            recovery_line: 5,
            post_error_line_expectations: vec![LineExpectation {
                line: 5,
                expected_tags: tags(&[LineTag::SubDecl]),
            }],
            post_error_symbol_spans: vec!["after_recovery".to_string()],
        };
        let prediction = RecoveryPrediction {
            first_error_line: Some(3),
            error_region_lines: [3].into_iter().collect(),
            actual_by_line: [(5, tags(&[LineTag::SubDecl]))].into_iter().collect(),
            symbol_spans: [SymbolSpanLocation { line: 5, span_text: "after_recovery".to_string() }]
                .into_iter()
                .collect(),
        };
        let mut score = RecoveryScore::default();

        score_recovery_expectation(&expectation, &prediction, &mut score);

        assert_eq!(score.first_error_line_correct_count, 1);
        assert_eq!(score.error_region_true_positive_count, 1);
        assert_eq!(score.spillover_lines, vec![0]);
        assert_eq!(score.post_error_line_score.exact_match_count, 1);
        assert_eq!(score.post_error_symbol_found_count, 1);
    }

    #[test]
    fn recovery_metrics_emit_measured_containment_rows() {
        let score = RecoveryScore {
            expectation_count: 1,
            first_error_line_correct_count: 1,
            error_region_true_positive_count: 1,
            spillover_lines: vec![0, 2],
            recovery_parse_micros: vec![1_000, 2_000],
            post_error_line_score: LineScore {
                line_count: 1,
                true_positive_count: 1,
                exact_match_count: 1,
                ..LineScore::default()
            },
            post_error_symbol_expected_count: 1,
            post_error_symbol_found_count: 1,
            ..RecoveryScore::default()
        };

        let metrics = recovery_metrics(&score, Cadence::Pr);

        assert!(metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, value, sample_count: 2, .. }
                    if metric == "recovery_spillover_p95_lines"
                        && (*value - 2.0).abs() < f64::EPSILON
            )
        }));
        assert!(metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, value, sample_count: 1, .. }
                    if metric == "recovery_post_error_symbol_recall"
                        && (*value - 1.0).abs() < f64::EPSILON
            )
        }));
        assert!(metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, value, sample_count: 1, .. }
                    if metric == "recovery_post_error_line_f1"
                        && (*value - 1.0).abs() < f64::EPSILON
            )
        }));
        let cost_metrics = cost_metrics(
            &ScaleCostScore::default(),
            &score,
            &MethodCompletionProviderScore::default(),
            &NavigationProviderScore::default(),
            Cadence::Pr,
        );
        assert!(cost_metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, value, sample_count: 2, .. }
                    if metric == "recovery_ms_p95"
                        && (*value - 2.0).abs() < f64::EPSILON
            )
        }));
    }

    #[test]
    fn incremental_scorer_compares_full_parse_and_apply_path() {
        let source =
            "package Accuracy::IncrementalSmallEdit;\n\nmy $value = 1;\n\nsub value { $value }\n";
        let expectation = IncrementalExpectation {
            id: "small_value_edit_matches_full_parse".to_string(),
            edits: vec![IncrementalEditExpectation {
                old_text: "my $value = 1;".to_string(),
                new_text: "my $value = 2;".to_string(),
                occurrence: None,
            }],
        };
        let mut score = IncrementalScore::default();

        score_incremental_expectation(source, &expectation, &mut score);

        assert_eq!(score.expectation_count, 1);
        assert_eq!(score.no_panic_count, 1);
        assert_eq!(score.edit_apply_equivalent_count, 1);
        assert_eq!(score.full_parse_equivalent_count, 1);
        assert_eq!(score.changed_range_correct_count, 1);
        assert_eq!(score.checkpoint_hit_count, 1);
        assert_eq!(score.checkpoint_miss_count, 0);
        assert_eq!(score.reparse_byte_ratios.len(), 1);
        assert_eq!(score.reused_token_ratios.len(), 1);
        assert!(score.reused_token_ratios[0] > 0.0);
        assert_eq!(score.reused_node_ratios.len(), 1);
    }

    #[test]
    fn incremental_metrics_emit_equivalence_and_reuse_rows() {
        let score = IncrementalScore {
            expectation_count: 1,
            full_parse_equivalent_count: 1,
            edit_apply_equivalent_count: 1,
            no_panic_count: 1,
            checkpoint_hit_count: 1,
            reparse_byte_ratios: vec![0.25],
            reused_token_ratios: vec![0.5],
            reused_node_ratios: vec![0.75],
            changed_range_sample_count: 1,
            changed_range_correct_count: 1,
            ..IncrementalScore::default()
        };

        let metrics = incremental_metrics(&score, Cadence::Pr);

        assert!(metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, value, sample_count: 1, .. }
                    if metric == "incremental_full_parse_equivalence_rate"
                        && (*value - 1.0).abs() < f64::EPSILON
            )
        }));
        assert!(metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, value, sample_count: 1, .. }
                    if metric == "incremental_reused_token_ratio"
                        && (*value - 0.5).abs() < f64::EPSILON
            )
        }));
    }

    #[test]
    fn span_scorer_counts_utf16_unicode_crlf_and_tab_coordinates() {
        let source = "package Span;\r\nmy $emoji = \"😀\";\n\treturn \"café\";\r\n";
        let expectation = SpanExpectation {
            id: "emoji_string".to_string(),
            span_text: "\"😀\"".to_string(),
            occurrence: None,
            byte_start: 27,
            byte_end: 33,
            line_start: 2,
            line_end: 2,
            utf16_start: SpanPositionExpectation { line: 1, character: 12 },
            utf16_end: SpanPositionExpectation { line: 1, character: 16 },
        };
        let tab_expectation = SpanExpectation {
            id: "tabbed_return".to_string(),
            span_text: "\treturn \"café\";".to_string(),
            occurrence: None,
            byte_start: 35,
            byte_end: 51,
            line_start: 3,
            line_end: 3,
            utf16_start: SpanPositionExpectation { line: 2, character: 0 },
            utf16_end: SpanPositionExpectation { line: 2, character: 15 },
        };
        let mut score = SpanScore::default();

        score_span_expectation(source, &expectation, &mut score);
        score_span_expectation(source, &tab_expectation, &mut score);

        assert_eq!(score.expectation_count, 2);
        assert_eq!(score.byte_exact_count, 2);
        assert_eq!(score.line_exact_count, 2);
        assert_eq!(score.utf16_exact_count, 2);
        assert_eq!(score.crlf_sample_count, 2);
        assert_eq!(score.unicode_sample_count, 2);
        assert_eq!(score.tab_sample_count, 1);
        assert_eq!(score.unicode_position_error_count, 0);
        assert_eq!(score.tab_column_mismatch_count, 0);
    }

    #[test]
    fn span_metrics_emit_coordinate_rows() {
        let score = SpanScore {
            expectation_count: 2,
            byte_exact_count: 2,
            line_exact_count: 2,
            utf16_exact_count: 2,
            near_count: 2,
            crlf_sample_count: 1,
            unicode_sample_count: 1,
            tab_sample_count: 1,
            ..SpanScore::default()
        };

        let metrics = span_metrics(&score, Cadence::Pr);

        assert!(metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, value, sample_count: 2, .. }
                    if metric == "utf16_range_exact_rate"
                        && (*value - 1.0).abs() < f64::EPSILON
            )
        }));
        assert!(metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, value, sample_count: 1, .. }
                    if metric == "unicode_position_error_count"
                        && (*value - 0.0).abs() < f64::EPSILON
            )
        }));
    }

    #[test]
    fn confidence_metrics_emit_precision_and_calibration_rows() {
        let mut proof_score = ProofScore::default();
        proof_score.true_positive_by_bucket.insert(ProofBucket::Exact, 1);
        proof_score.predicted_by_bucket.insert(ProofBucket::Exact, 2);
        proof_score.true_positive_by_bucket.insert(ProofBucket::High, 1);
        proof_score.predicted_by_bucket.insert(ProofBucket::High, 2);
        proof_score.predicted_by_bucket.insert(ProofBucket::Medium, 1);
        proof_score.high_confidence_false_positive_count = 1;
        let score = SymbolScore { proof_score, ..SymbolScore::default() };

        let metrics = confidence_metrics(&score, Cadence::Pr);

        assert!(metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, value, sample_count: 2, .. }
                    if metric == "exact_fact_precision"
                        && (*value - 0.5).abs() < f64::EPSILON
            )
        }));
        assert!(metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, value, sample_count: 2, .. }
                    if metric == "confidence_calibration_error"
                        && (*value - 0.5).abs() < f64::EPSILON
            )
        }));
        assert!(metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::InsufficientData { metric, sample_count: 0, .. }
                    if metric == "low_confidence_precision"
            )
        }));
    }

    #[test]
    fn unsupported_metrics_emit_construct_rows() {
        let score = UnsupportedScore {
            manifest_construct_count: 2,
            family_count: 2,
            line_labeled_construct_count: 1,
            detected_count: 1,
            salvaged_count: 1,
            false_exact_count: 0,
            false_exact_sample_count: 1,
        };

        let metrics = unsupported_metrics(&score, Cadence::Pr);

        assert!(metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, value, sample_count: 1, .. }
                    if metric == "unsupported_construct_detected_count"
                        && (*value - 1.0).abs() < f64::EPSILON
            )
        }));
        assert!(metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, value, sample_count: 2, .. }
                    if metric == "unsupported_construct_family_count"
                        && (*value - 2.0).abs() < f64::EPSILON
            )
        }));
        assert!(metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, value, sample_count: 1, .. }
                    if metric == "unsupported_construct_false_exact_count"
                        && (*value - 0.0).abs() < f64::EPSILON
            )
        }));
    }

    #[test]
    fn provider_impact_metrics_remain_insufficient_until_gold_exists() {
        let metrics = provider_impact_metrics(
            &MethodCompletionProviderScore::default(),
            &DiagnosticProviderScore::default(),
            &NavigationProviderScore::default(),
            Cadence::Pr,
        );

        assert!(metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::InsufficientData { metric, sample_count: 0, .. }
                    if metric == "provider_goto_definition_hit_rate"
            )
        }));
        assert!(metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::InsufficientData { metric, sample_count: 0, .. }
                    if metric == "provider_diagnostic_false_negative_rate"
            )
        }));
    }

    #[test]
    fn scale_and_cost_metrics_emit_shape_rows_and_timing_rows() {
        let score = ScaleCostScore {
            fixture_count: 2,
            file_bytes: 120,
            source_lines: 10,
            token_count: 30,
            ast_node_count: 20,
            symbol_count: 6,
            import_count: 2,
            export_count: 1,
            sub_count: 3,
            package_count: 2,
            max_nesting_depth: 4,
            max_brace_depth: 3,
            max_regex_length: 12,
            max_heredoc_body_bytes: 40,
            quote_like_count: 2,
            dynamic_boundary_count: 1,
            lex_ms: vec![0.1, 0.2],
            parse_ms: vec![0.3, 0.4],
            ast_projection_ms: vec![0.01, 0.02],
            semantic_extraction_ms: vec![0.5, 0.7],
            workspace_insert_ms: vec![0.8, 1.2],
        };

        let mut metrics = scale_metrics(&score, Cadence::Pr);
        metrics.extend(cost_metrics(
            &score,
            &RecoveryScore::default(),
            &MethodCompletionProviderScore::default(),
            &NavigationProviderScore::default(),
            Cadence::Pr,
        ));

        assert!(metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, value, sample_count: 2, .. }
                    if metric == "scale_token_count"
                        && (*value - 30.0).abs() < f64::EPSILON
            )
        }));
        assert!(metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, value, sample_count: 2, .. }
                    if metric == "parse_ms_p95"
                        && (*value - 0.4).abs() < f64::EPSILON
            )
        }));
        assert!(metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, value, sample_count: 2, .. }
                    if metric == "workspace_insert_ms_p95"
                        && (*value - 1.2).abs() < f64::EPSILON
            )
        }));
        assert!(metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::InsufficientData { metric, sample_count: 0, .. }
                    if metric == "peak_rss_mb"
            )
        }));
    }

    #[test]
    fn cache_reuse_metrics_emit_fast_path_rows() {
        let score = IncrementalScore {
            expectation_count: 2,
            full_parse_equivalent_count: 1,
            fallback_count: 1,
            checkpoint_hit_count: 3,
            checkpoint_miss_count: 1,
            content_hash_hit_count: 3,
            content_hash_miss_count: 1,
            semantic_fact_cache_hit_count: 2,
            semantic_fact_cache_miss_count: 2,
            workspace_shard_reuse_count: 3,
            workspace_shard_replacement_attempt_count: 4,
            unchanged_file_skip_count: 1,
            unchanged_file_index_attempt_count: 4,
            ..IncrementalScore::default()
        };

        let metrics = cache_reuse_metrics(&score, Cadence::Pr);

        assert!(metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, value, sample_count: 4, .. }
                    if metric == "lexer_checkpoint_reuse_rate"
                        && (*value - 0.75).abs() < f64::EPSILON
            )
        }));
        assert!(metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, value, sample_count: 4, .. }
                    if metric == "parser_checkpoint_reuse_rate"
                        && (*value - 0.75).abs() < f64::EPSILON
            )
        }));
        assert!(metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, value, sample_count: 4, .. }
                    if metric == "content_hash_hit_rate"
                        && (*value - 0.75).abs() < f64::EPSILON
            )
        }));
        assert!(metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, value, sample_count: 4, .. }
                    if metric == "semantic_fact_cache_hit_rate"
                        && (*value - 0.5).abs() < f64::EPSILON
            )
        }));
        assert!(metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, value, sample_count: 4, .. }
                    if metric == "workspace_shard_reuse_rate"
                        && (*value - 0.75).abs() < f64::EPSILON
            )
        }));
        assert!(metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, value, sample_count: 4, .. }
                    if metric == "unchanged_file_skip_rate"
                        && (*value - 0.25).abs() < f64::EPSILON
            )
        }));
        assert!(metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, value, sample_count: 2, .. }
                    if metric == "fast_path_wrong_result_count"
                        && (*value - 1.0).abs() < f64::EPSILON
            )
        }));
    }

    #[test]
    fn determinism_metrics_emit_hash_stability_rows() {
        let score = DeterminismScore {
            fixture_count: 2,
            token_stream_stable_count: 2,
            parse_hash_stable_count: 2,
            ast_hash_stable_count: 1,
            semantic_fact_hash_stable_count: 2,
            diagnostic_hash_stable_count: 2,
            repeated_parse_stable_count: 2,
            whitespace_invariance_stable_count: 1,
            whitespace_invariance_sample_count: 2,
            comment_invariance_stable_count: 1,
            comment_invariance_sample_count: 2,
            newline_style_invariance_stable_count: 1,
            newline_style_invariance_sample_count: 2,
        };

        let metrics = determinism_metrics(&score, Cadence::Pr);

        assert!(metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, value, sample_count: 2, .. }
                    if metric == "ast_hash_stability_rate"
                        && (*value - 0.5).abs() < f64::EPSILON
            )
        }));
        assert!(metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, value, sample_count: 2, .. }
                    if metric == "whitespace_invariance_rate"
                        && (*value - 0.5).abs() < f64::EPSILON
            )
        }));
        assert!(metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, value, sample_count: 2, .. }
                    if metric == "comment_invariance_rate"
                        && (*value - 0.5).abs() < f64::EPSILON
            )
        }));
        assert!(metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, value, sample_count: 2, .. }
                    if metric == "newline_style_invariance_rate"
                        && (*value - 0.5).abs() < f64::EPSILON
            )
        }));
    }

    #[test]
    fn whitespace_invariance_variant_adds_trailing_spaces_without_crossing_literal_boundaries() {
        assert_eq!(
            whitespace_invariance_variant("package Demo;\n\n1;\n"),
            Some("package Demo;  \n\n1;  \n".to_string())
        );
        assert_eq!(
            whitespace_invariance_variant("package Demo;\r\n1;\r\n"),
            Some("package Demo;  \r\n1;  \r\n".to_string())
        );
        assert!(whitespace_invariance_variant("print <<'EOF';\nEOF\n").is_none());
    }

    #[test]
    fn newline_style_invariance_variant_converts_lf_to_crlf() {
        assert_eq!(
            newline_style_invariance_variant("package Demo;\n1;\n"),
            Some("package Demo;\r\n1;\r\n".to_string())
        );
        assert!(newline_style_invariance_variant("package Demo;\r\n1;\r\n").is_none());
        assert!(newline_style_invariance_variant("print <<'EOF';\nEOF\n").is_none());
    }

    #[test]
    fn runtime_metric_rows_are_synced_after_artifact_size_settles() {
        let mut artifact = ParserAccuracyArtifact {
            schema_version: 1,
            subsystem: "parser_accuracy".to_string(),
            generated_at: "2026-05-02T00:00:00Z".to_string(),
            commit: "test".to_string(),
            cadence: Cadence::Pr,
            denominator: Denominator::default(),
            families: Vec::new(),
            metrics: Vec::new(),
            failure_packets: Vec::new(),
            gold_drift: GoldDrift::default(),
            metric_runtime: MetricRuntime {
                runtime_ms: 12.5,
                artifact_size_bytes: 900,
                cache_hit_rate: Some(0.25),
                cache_sample_count: 4,
                ..MetricRuntime::default()
            },
        };

        sync_runtime_metric_rows(&mut artifact, Cadence::Pr);

        assert!(artifact.metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, value, sample_count: 1, .. }
                    if metric == "metric_runtime_ms"
                        && (*value - 12.5).abs() < f64::EPSILON
            )
        }));
        assert!(artifact.metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, value, sample_count: 1, .. }
                    if metric == "metric_artifact_size_bytes"
                        && (*value - 900.0).abs() < f64::EPSILON
            )
        }));
        assert!(artifact.metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, value, sample_count: 4, .. }
                    if metric == "metric_cache_hit_rate"
                        && (*value - 0.25).abs() < f64::EPSILON
            )
        }));
    }

    #[test]
    fn allocation_rows_are_replaced_after_runtime_measurement() {
        let mut artifact = ParserAccuracyArtifact {
            schema_version: 1,
            subsystem: "parser_accuracy".to_string(),
            generated_at: "2026-05-02T00:00:00Z".to_string(),
            commit: "test".to_string(),
            cadence: Cadence::Pr,
            denominator: Denominator::default(),
            families: Vec::new(),
            metrics: vec![
                insufficient("peak_rss_mb", "memory telemetry is not wired yet"),
                insufficient("allocated_bytes", "allocation telemetry is not wired yet"),
                insufficient("allocation_count", "allocation telemetry is not wired yet"),
            ],
            failure_packets: Vec::new(),
            gold_drift: GoldDrift::default(),
            metric_runtime: MetricRuntime {
                peak_rss_mb: Some(1.5),
                allocated_bytes: Some(1234),
                allocation_count: Some(56),
                ..MetricRuntime::default()
            },
        };

        sync_allocation_metric_rows(&mut artifact, Cadence::Pr);

        assert!(artifact.metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, value, sample_count: 1, .. }
                    if metric == "peak_rss_mb"
                        && (*value - 1.5).abs() < f64::EPSILON
            )
        }));
        assert!(artifact.metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, value, sample_count: 1, .. }
                    if metric == "allocated_bytes"
                        && (*value - 1234.0).abs() < f64::EPSILON
            )
        }));
        assert!(artifact.metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, value, sample_count: 1, .. }
                    if metric == "allocation_count"
                        && (*value - 56.0).abs() < f64::EPSILON
            )
        }));
    }

    #[test]
    fn gold_drift_audit_counts_span_duplicate_and_missing_edge_errors() {
        let span_expectations = vec![
            SpanExpectation {
                id: "good".to_string(),
                span_text: "Alpha".to_string(),
                occurrence: None,
                byte_start: 0,
                byte_end: 5,
                line_start: 1,
                line_end: 1,
                utf16_start: SpanPositionExpectation { line: 0, character: 0 },
                utf16_end: SpanPositionExpectation { line: 0, character: 5 },
            },
            SpanExpectation {
                id: "bad_text".to_string(),
                span_text: "Beta".to_string(),
                occurrence: None,
                byte_start: 0,
                byte_end: 5,
                line_start: 1,
                line_end: 1,
                utf16_start: SpanPositionExpectation { line: 0, character: 0 },
                utf16_end: SpanPositionExpectation { line: 0, character: 4 },
            },
        ];
        let expectations = SymbolExpectations {
            entities: vec![SymbolEntityExpectation {
                id: "dup".to_string(),
                kind: "Package".to_string(),
                canonical_name: "Alpha".to_string(),
                span_text: "Alpha".to_string(),
                package: None,
                scope: None,
                provenance: "ExactAst".to_string(),
                confidence: "High".to_string(),
            }],
            occurrences: vec![SymbolOccurrenceExpectation {
                id: "dup".to_string(),
                kind: "Reference".to_string(),
                canonical_name: Some("Alpha::missing".to_string()),
                span_text: "missing".to_string(),
                package: None,
                scope: None,
                provenance: "ExactAst".to_string(),
                confidence: "High".to_string(),
            }],
            edges: vec![SymbolEdgeExpectation {
                id: "edge".to_string(),
                kind: "Defines".to_string(),
                from: "Alpha".to_string(),
                to: "Alpha::missing".to_string(),
                provenance: "ExactAst".to_string(),
                confidence: "High".to_string(),
            }],
        };
        let mut seen = BTreeSet::new();

        assert_eq!(count_span_expectation_errors("Alpha", &span_expectations), 1);
        assert_eq!(count_duplicate_symbol_ids(&expectations, &mut seen), 1);
        assert_eq!(count_missing_edge_targets(&expectations), 1);
    }

    #[test]
    fn gold_drift_audit_counts_baseline_expectation_deltas() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        write_fixture_sources(tmp.path())?;
        let manifest = fixture_manifest();
        let mut baseline_signatures = gold_expectation_signatures(&manifest);
        baseline_signatures.remove("package_basic::line::3::SubDecl");
        baseline_signatures.remove("package_basic::symbol_entity::package_basic_answer_entity");
        baseline_signatures.insert("package_basic::line::99::SubDecl".to_string());
        let baseline_dir = tmp.path().join(".ci/metrics/baselines");
        fs::create_dir_all(&baseline_dir)?;
        fs::write(
            baseline_dir.join("parser_accuracy_gold.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": 1,
                "expectation_signatures": baseline_signatures,
            }))?,
        )?;
        let mut source_cache = MetricSourceCache::default();

        let drift = audit_gold_drift(tmp.path(), &manifest, &mut source_cache)?;

        assert_eq!(drift.added_expectation_count, 2);
        assert_eq!(
            drift.added_expectation_sample_count,
            gold_expectation_signatures(&manifest).len() as u64
        );
        assert_eq!(drift.changed_line_count, 2);
        assert_eq!(
            drift.changed_line_sample_count,
            line_gold_signatures(&gold_expectation_signatures(&manifest)).len() as u64 + 1
        );
        assert_eq!(drift.changed_symbol_count, 1);
        assert_eq!(
            drift.changed_symbol_sample_count,
            symbol_gold_signatures(&gold_expectation_signatures(&manifest)).len() as u64
        );
        assert_eq!(drift.removed_expectation_count, 1);
        assert_eq!(
            drift.removed_expectation_sample_count,
            gold_expectation_signatures(&manifest).len() as u64 - 1
        );
        assert_eq!(drift.weakening_explanation_required_count, 1);
        assert_eq!(
            drift.weakening_explanation_sample_count,
            drift.removed_expectation_sample_count
        );
        assert_eq!(drift.dynamic_expectation_change_count, 0);
        assert_eq!(drift.dynamic_expectation_sample_count, 0);
        Ok(())
    }

    #[test]
    fn gold_drift_audit_counts_dynamic_baseline_expectation_deltas() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        write_fixture_sources(tmp.path())?;
        let manifest = fixture_manifest();
        let current_dynamic_signatures = dynamic_gold_signatures(&manifest);
        let removed_signature = current_dynamic_signatures
            .iter()
            .next()
            .cloned()
            .ok_or_else(|| eyre!("fixture manifest should include dynamic gold signatures"))?;
        let mut baseline_dynamic_signatures = current_dynamic_signatures.clone();
        baseline_dynamic_signatures.remove(&removed_signature);
        baseline_dynamic_signatures
            .insert("dynamic_require_boundary::line::99::DynamicBoundary".to_string());
        let baseline_dir = tmp.path().join(".ci/metrics/baselines");
        fs::create_dir_all(&baseline_dir)?;
        fs::write(
            baseline_dir.join("parser_accuracy_gold.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": 1,
                "expectation_signatures": gold_expectation_signatures(&manifest),
                "dynamic_expectation_signatures": baseline_dynamic_signatures,
            }))?,
        )?;
        let mut source_cache = MetricSourceCache::default();

        let drift = audit_gold_drift(tmp.path(), &manifest, &mut source_cache)?;

        assert_eq!(
            drift.dynamic_expectation_change_count,
            current_dynamic_signatures.symmetric_difference(&baseline_dynamic_signatures).count()
                as u64
        );
        assert_eq!(
            drift.dynamic_expectation_sample_count,
            current_dynamic_signatures.union(&baseline_dynamic_signatures).count() as u64
        );
        Ok(())
    }

    #[test]
    fn gold_drift_metrics_emit_validation_and_baseline_rows() {
        let drift = GoldDrift {
            span_error_count: 1,
            duplicate_symbol_id_count: 2,
            missing_resolves_to_target_count: 3,
            changed_line_count: 1,
            changed_line_sample_count: 4,
            changed_symbol_count: 1,
            changed_symbol_sample_count: 4,
            removed_expectation_count: 1,
            removed_expectation_sample_count: 4,
            added_expectation_count: 1,
            added_expectation_sample_count: 4,
            dynamic_expectation_change_count: 1,
            dynamic_expectation_sample_count: 4,
            weakening_explanation_required_count: 1,
            weakening_explanation_sample_count: 4,
            ..GoldDrift::default()
        };

        let metrics = gold_drift_metrics(&drift, 4, Cadence::Pr);

        assert!(metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, value, sample_count: 4, .. }
                    if metric == "gold_span_errors"
                        && (*value - 1.0).abs() < f64::EPSILON
            )
        }));
        assert!(metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, value, sample_count: 4, .. }
                    if metric == "gold_changed_line_count"
                        && (*value - 1.0).abs() < f64::EPSILON
            )
        }));
        assert!(metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, value, sample_count: 4, .. }
                    if metric == "gold_changed_symbol_count"
                        && (*value - 1.0).abs() < f64::EPSILON
            )
        }));
        assert!(metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, value, sample_count: 4, .. }
                    if metric == "gold_removed_expectation_count"
                        && (*value - 1.0).abs() < f64::EPSILON
            )
        }));
        assert!(metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, value, sample_count: 4, .. }
                    if metric == "gold_added_expectation_count"
                        && (*value - 1.0).abs() < f64::EPSILON
            )
        }));
        assert!(metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, value, sample_count: 4, .. }
                    if metric == "gold_dynamic_expectation_change_count"
                        && (*value - 1.0).abs() < f64::EPSILON
            )
        }));
        assert!(metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, value, sample_count: 4, .. }
                    if metric == "gold_weakening_explanation_required_count"
                        && (*value - 1.0).abs() < f64::EPSILON
            )
        }));
    }

    #[test]
    fn artifact_uses_measured_line_ast_and_symbol_scores() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        write_fixture_sources(tmp.path())?;
        let artifact = build_artifact(tmp.path(), &fixture_manifest(), Cadence::Pr)?;
        validate_artifact_contract(&artifact)?;
        assert!(artifact.metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, sample_count, .. }
                    if metric == "line_construct_f1"
                        && *sample_count > 0
            )
        }));
        assert!(artifact.metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, sample_count, .. }
                    if metric == "ast_node_kind_f1"
                        && *sample_count > 0
            )
        }));
        assert!(artifact.metrics.iter().any(|metric| {
            matches!(
                metric,
                MetricRow::Measured { metric, sample_count, .. }
                    if metric == "symbol_decl_f1"
                        && *sample_count > 0
            )
        }));
        Ok(())
    }

    #[test]
    fn artifact_failure_packets_include_actionable_context() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        write_fixture_sources(tmp.path())?;
        let artifact = build_artifact(tmp.path(), &fixture_manifest(), Cadence::Pr)?;

        let packet = artifact
            .failure_packets
            .iter()
            .find(|packet| packet.metric.as_deref() == Some("ast_node_kind_f1"))
            .ok_or_else(|| eyre!("expected at least one AST failure packet"))?;

        assert_eq!(packet.likely_layer, "ast_projection");
        assert_eq!(packet.family.as_deref(), Some("packages"));
        assert_eq!(packet.line, Some(1));
        assert!(!packet.expected.is_empty());
        assert!(!packet.actual.is_empty());
        assert!(!packet.nearest_predictions.is_empty());
        assert!(packet.source_excerpt.as_deref().is_some_and(|line| line.contains("package")));
        assert!(packet.suggested_next_fix.is_some());
        Ok(())
    }
}
