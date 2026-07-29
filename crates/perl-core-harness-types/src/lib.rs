#![warn(missing_docs)]
#![cfg_attr(clippy, allow(missing_docs))]

//! Shared receipt contracts for the upstream Perl core harness lane.

use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;

pub const DISCOVERY_SCHEMA_VERSION: &str = "perl_core_harness.discovery.v1";
pub const RUN_REPORT_SCHEMA_VERSION: &str = "perl_core_harness.report.v1";
pub const COMPILE_BASELINE_SCHEMA_VERSION: &str = "perl_core_harness.compile_baseline.v1";
pub const SMOKE_SCHEMA_VERSION: &str = "perl_core_harness.smoke.v1";
pub const PREPARE_SCHEMA_VERSION: &str = "perl_core_harness.prepare.v1";
pub const GAP_MAP_SCHEMA_VERSION: &str = "perl_core_harness.gap_map.v1";
pub const RUNNER_RECORD_SCHEMA_VERSION: &str = "perl_core_harness.runner_record.v1";
pub const CURATED_GOLD_SCHEMA_VERSION: &str = "curated_gold.v1";
pub const CURATED_GOLD_COMPARISON_SCHEMA_VERSION: &str = "curated_gold_comparison.v1";

/// A semantic fact class that can be independently labeled by curated gold.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum CuratedGoldFactClass {
    #[serde(rename = "PackageSubTable")]
    PackageSubTable,
    #[serde(rename = "ScopeBinding")]
    ScopeBinding,
    #[serde(rename = "PlaceAccess")]
    PlaceAccess,
    #[serde(rename = "ContextDemand")]
    ContextDemand,
    #[serde(rename = "ControlFlow")]
    ControlFlow,
    #[serde(rename = "CompileEffect")]
    CompileEffect,
    #[serde(rename = "DynamicBoundary")]
    DynamicBoundary,
    #[serde(rename = "ImportExport")]
    ImportExport,
    #[serde(rename = "IsaComposition")]
    IsaComposition,
    #[serde(rename = "ConstantPrototype")]
    ConstantPrototype,
    #[serde(rename = "FrameworkGeneratedMember")]
    FrameworkGeneratedMember,
}

impl CuratedGoldFactClass {
    pub const ALL: [Self; 11] = [
        Self::PackageSubTable,
        Self::ScopeBinding,
        Self::PlaceAccess,
        Self::ContextDemand,
        Self::ControlFlow,
        Self::CompileEffect,
        Self::DynamicBoundary,
        Self::ImportExport,
        Self::IsaComposition,
        Self::ConstantPrototype,
        Self::FrameworkGeneratedMember,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::PackageSubTable => "PackageSubTable",
            Self::ScopeBinding => "ScopeBinding",
            Self::PlaceAccess => "PlaceAccess",
            Self::ContextDemand => "ContextDemand",
            Self::ControlFlow => "ControlFlow",
            Self::CompileEffect => "CompileEffect",
            Self::DynamicBoundary => "DynamicBoundary",
            Self::ImportExport => "ImportExport",
            Self::IsaComposition => "IsaComposition",
            Self::ConstantPrototype => "ConstantPrototype",
            Self::FrameworkGeneratedMember => "FrameworkGeneratedMember",
        }
    }
}

impl fmt::Display for CuratedGoldFactClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Source backing for a curated-gold fixture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum CuratedGoldSource {
    /// Source embedded in the fixture for small, self-contained examples.
    Inline { text: String },
    /// A path in a declared repository revision.
    Repository { path: String, revision: String },
}

/// Confidence assigned by the independent gold author.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CuratedGoldConfidence {
    High,
    Medium,
    Low,
}

/// Freshness of an observed fact relative to the declared source snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CuratedGoldFreshness {
    Fresh,
    Stale,
    Unknown,
}

/// A source range attached to a curated fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CuratedGoldRange {
    pub start_byte: u32,
    pub end_byte: u32,
}

/// One normalized expected or observed semantic fact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CuratedGoldFact {
    pub fact_id: String,
    pub value: Value,
    pub range: Option<CuratedGoldRange>,
    pub provenance: String,
    pub confidence: CuratedGoldConfidence,
    pub freshness: CuratedGoldFreshness,
    pub dynamic_boundary: Option<String>,
}

/// A read-only, independently authored semantic expectation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CuratedGoldFixture {
    pub schema_version: String,
    pub fixture_id: String,
    pub fact_class: CuratedGoldFactClass,
    pub source: CuratedGoldSource,
    pub source_content_hash: String,
    pub expected_facts: Vec<CuratedGoldFact>,
    pub expectation_hash: String,
    pub author_identity: String,
    pub reviewer_identity: String,
    pub review_receipt: String,
    pub rationale: String,
    pub coverage_intent: String,
    pub confidence: CuratedGoldConfidence,
    pub perl_references: Vec<String>,
    pub allowed_dynamic_boundaries: Vec<String>,
    pub minimum_compiler_capability: String,
}

/// Classification emitted for one gold comparison item.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CuratedGoldComparisonKind {
    ExactAgreement,
    MissingFact,
    ExtraFact,
    ValueMismatch,
    RangeMismatch,
    ProvenanceMismatch,
    ConfidenceOrFreshnessMismatch,
    ExpectedDynamicBoundary,
    UnexpectedDynamicBoundary,
    UnimplementedFactClass,
    StaleFixture,
}

/// Overall status of a deterministic gold comparison receipt.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CuratedGoldComparisonStatus {
    ExactAgreement,
    HasMismatches,
    StaleFixture,
    UnimplementedFactClass,
}

/// One deterministic comparison result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CuratedGoldComparisonItem {
    pub kind: CuratedGoldComparisonKind,
    pub fact_id: Option<String>,
    pub detail: String,
}

/// Deterministic receipt for a read-only curated-gold comparison.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CuratedGoldComparisonReceipt {
    pub schema_version: String,
    pub fixture_id: String,
    pub fact_class: CuratedGoldFactClass,
    pub source_content_hash: String,
    pub expectation_hash: String,
    pub compiler_capability: String,
    pub status: CuratedGoldComparisonStatus,
    pub comparisons: Vec<CuratedGoldComparisonItem>,
}

/// Upstream Perl test scheduler to query.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum HarnessRunner {
    /// Bootstrap runner in upstream `t/TEST`.
    Test,
    /// TAP::Harness-backed runner in upstream `t/harness`.
    Harness,
}

impl HarnessRunner {
    pub fn script_name(self) -> &'static str {
        match self {
            Self::Test => "TEST",
            Self::Harness => "harness",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Test => "test",
            Self::Harness => "harness",
        }
    }
}

impl fmt::Display for HarnessRunner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Compiler/test mode for later run slices.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum HarnessMode {
    Parse,
    Compile,
    Execute,
}

impl HarnessMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Parse => "parse",
            Self::Compile => "compile",
            Self::Execute => "execute",
        }
    }
}

impl fmt::Display for HarnessMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Staged upstream Perl core profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum HarnessProfile {
    Base,
    Comp,
    Run,
    Core,
    Lib,
    Full,
}

impl HarnessProfile {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Base => "base",
            Self::Comp => "comp",
            Self::Run => "run",
            Self::Core => "core",
            Self::Lib => "lib",
            Self::Full => "full",
        }
    }

    pub fn roots(self) -> &'static [&'static str] {
        match self {
            Self::Base => &["base"],
            Self::Comp => &["comp"],
            Self::Run => &["run"],
            Self::Core => &["base", "comp", "run", "cmd", "io", "re", "opbasic", "op"],
            Self::Lib => &["lib"],
            Self::Full => &["base", "comp", "run", "cmd", "io", "re", "opbasic", "op", "uni"],
        }
    }
}

impl fmt::Display for HarnessProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Machine-readable discovery manifest.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveryReport {
    pub schema_version: String,
    pub commit: String,
    pub timestamp: String,
    pub perl_ref: String,
    pub prepared_tree: String,
    pub host_perl: String,
    pub runner: HarnessRunner,
    pub profile: HarnessProfile,
    pub tests: Vec<DiscoveredTest>,
}

/// One upstream test discovered by `--dumptests`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveredTest {
    pub path: String,
    pub root: String,
}

/// Machine-readable parse/compile/execute report.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunReport {
    pub schema_version: String,
    pub commit: String,
    pub timestamp: String,
    pub perl_ref: String,
    pub prepared_tree: String,
    pub run_tree: String,
    pub host_perl: String,
    pub runner: HarnessRunner,
    pub mode: HarnessMode,
    pub profile: HarnessProfile,
    pub harness_status: Option<i32>,
    pub summary: RunSummary,
    pub buckets: BTreeMap<String, usize>,
    pub file_results: Vec<RunFileResult>,
    pub failures: Vec<RunFailure>,
    /// Boundary facts observed while compiling the profile.
    #[serde(default)]
    pub semantic_boundaries: Vec<ObservedSemanticBoundary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunSummary {
    pub files_total: usize,
    pub files_passed: usize,
    pub files_failed: usize,
    pub tap_assertions_total: usize,
    pub tap_assertions_passed: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunFileResult {
    pub path: String,
    pub status: RunnerStatus,
    pub assertions_passed: usize,
    pub assertions_total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunFailure {
    pub path: String,
    pub phase: String,
    pub bucket: String,
    pub first_diagnostic: String,
    pub workstream: String,
    pub lsp_impact: Vec<String>,
}

/// Checked-in baseline for a Perl core harness run report.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompileBaseline {
    pub schema_version: String,
    pub report_schema_version: String,
    pub mode: HarnessMode,
    pub profile: HarnessProfile,
    pub files_total: usize,
    pub files_passed: usize,
    pub files_failed: usize,
    pub tap_assertions_total: usize,
    pub tap_assertions_passed: usize,
    pub buckets: BTreeMap<String, usize>,
    pub expected_failures: Vec<RunFailure>,
    pub file_results: Vec<RunFileResult>,
    /// Semantic boundaries accepted by this compile receipt.
    ///
    /// `None` preserves compatibility with baselines written before boundary
    /// inventory ratcheting existed; `Some([])` is an explicit empty inventory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_boundaries: Option<Vec<ObservedSemanticBoundary>>,
}

/// Preparation receipt for an upstream Perl source tree.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PrepareReceipt {
    pub schema_version: String,
    pub requested_ref: String,
    pub resolved_ref: Option<String>,
    pub source_url: String,
    pub source_dir: String,
    pub prepared_tree: String,
    pub host_os: String,
    pub host_arch: String,
    pub configure_command: String,
    pub test_prep_command: String,
    pub status: PrepareStatus,
    pub first_error: Option<String>,
    pub started_at: String,
    pub finished_at: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrepareStatus {
    Pass,
    Fail,
}

/// Manual/advisory smoke summary for a real prepared Perl tree.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SmokeReport {
    pub schema_version: String,
    pub timestamp: String,
    pub repo_commit: String,
    pub perl_requested_ref: String,
    pub perl_resolved_ref: String,
    pub prepared_tree: String,
    pub host_perl: String,
    pub runner: HarnessRunner,
    pub profile: HarnessProfile,
    pub modes_requested: Vec<HarnessMode>,
    pub discovery_report: String,
    pub parse_report: Option<String>,
    pub compile_report: Option<String>,
    pub gap_map: String,
    pub discovery_total: usize,
    pub parse_files_total: Option<usize>,
    pub parse_files_passed: Option<usize>,
    pub parse_files_failed: Option<usize>,
    pub compile_files_total: Option<usize>,
    pub compile_files_passed: Option<usize>,
    pub compile_files_failed: Option<usize>,
    pub parse_buckets: Option<BTreeMap<String, usize>>,
    pub compile_buckets: Option<BTreeMap<String, usize>>,
    pub status: SmokeStatus,
    pub structural_failures: Vec<SmokeStructuralFailure>,
}

/// Bucketed gap map generated by real-tree smoke receipts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GapMap {
    pub schema_version: String,
    pub profile: HarnessProfile,
    pub mode: String,
    pub total_files: usize,
    pub passed_files: usize,
    pub failed_files: usize,
    pub buckets: BTreeMap<String, usize>,
    pub files_by_bucket: BTreeMap<String, Vec<String>>,
    pub first_failure_by_bucket: BTreeMap<String, RunFailure>,
    pub workstreams: BTreeMap<String, usize>,
    pub lsp_impact: BTreeMap<String, usize>,
    pub top_parse_failures: Vec<RunFailure>,
    pub top_compile_failures: Vec<RunFailure>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SmokeStatus {
    Pass,
    Fail,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SmokeStructuralFailure {
    pub mode: Option<HarnessMode>,
    pub path: Option<String>,
    pub kind: SmokeFailureKind,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SmokeFailureKind {
    MissingReport,
    ProfileMismatch,
    UnbucketedFailure,
    UnknownBucket,
    SemanticBoundary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BaselineComparison {
    pub violations: Vec<BaselineViolation>,
}

impl BaselineComparison {
    pub fn is_clean(&self) -> bool {
        self.violations.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BaselineViolation {
    pub kind: BaselineViolationKind,
    pub path: Option<String>,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BaselineViolationKind {
    SchemaMismatch,
    ModeMismatch,
    ProfileMismatch,
    PreviouslyPassingFileFailed,
    UnexpectedNewFailure,
    UnknownBucket,
    UnbucketedFailure,
    BucketCountIncreased,
    MissingExpectedFile,
    AssertionRegression,
    SemanticBoundary,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunnerStatus {
    Pass,
    Fail,
}

/// How a compiler receipt classified a non-static semantic boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticBoundaryDisposition {
    /// The compiler modeled the behavior as an ordinary static fact.
    ImplementedStatic,
    /// The compiler retained a static fact without executing its behavior.
    StaticallyClassified,
    /// The behavior is an ordinary runtime expression and is not a boundary.
    OrdinaryRuntime,
    /// The behavior is an ordinary runtime expression and does not block compilation.
    DeferredRuntime,
    /// The behavior is registered for a later lifecycle phase such as `END`.
    DeferredLifecycle,
    /// The behavior may affect compilation but is governed by an explicit classifier.
    GovernedCompileTimeDynamic,
    /// The pinned source shape is accepted by an exact compatibility guard.
    SourceLockedCompatibility,
    /// The behavior is not currently safe to accept in a compile receipt.
    Unsupported,
    /// The classifier could not determine a disposition.
    Unknown,
}

/// Confidence retained with a semantic-boundary classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticBoundaryConfidence {
    /// The classification follows an exact structural or source contract.
    Exact,
    /// The classification is safe but deliberately preserves dynamic uncertainty.
    Conservative,
    /// The compiler could not establish a safe classification.
    Unresolved,
}

/// Whether a semantic boundary depends on a pinned path or source contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticBoundaryLockScope {
    /// The classification is general and has no pinned compatibility guard.
    None,
    /// The classification is restricted to a pinned test path.
    Path,
    /// The classification is restricted to an exact source shape.
    Source,
    /// The classification requires both the pinned path and exact source shape.
    PathAndSource,
}

/// Byte range in the source that emitted a semantic boundary.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticBoundarySourceSpan {
    /// Inclusive byte offset where the boundary starts.
    pub start: usize,
    /// Exclusive byte offset where the boundary ends.
    pub end: usize,
}

/// A semantic boundary observed while producing a harness runner record.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticBoundaryRecord {
    /// Stable identifier for the classifier or exact guarded boundary.
    pub id: String,
    /// Classification used by the receipt lane.
    pub disposition: SemanticBoundaryDisposition,
    /// Compiler-provided explanation for the boundary.
    pub reason: String,
    /// Byte range of the source expression or phase that emitted the boundary.
    pub source_span: SemanticBoundarySourceSpan,
    /// Compiler source kind that produced the boundary.
    pub source_kind: String,
    /// Confidence in the retained classification.
    pub confidence: SemanticBoundaryConfidence,
    /// Whether the boundary makes the compile receipt fail.
    pub blocks_compilation: bool,
    /// Whether downstream static facts must be withheld or qualified.
    pub blocks_downstream_static_facts: bool,
    /// Whether the classification is guarded by a pinned path or source contract.
    pub lock_scope: SemanticBoundaryLockScope,
    /// Workstream responsible for deepening or replacing this classification.
    pub owner_workstream: String,
    /// Focused test or receipt path that supports this classification.
    pub supporting_test: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunnerRecord {
    pub schema_version: String,
    pub mode: String,
    pub path: String,
    pub status: RunnerStatus,
    pub assertions_passed: usize,
    pub assertions_total: usize,
    pub bucket: Option<String>,
    pub first_diagnostic: Option<String>,
    /// Non-static semantic boundaries observed for this invocation.
    #[serde(default)]
    pub semantic_boundaries: Vec<SemanticBoundaryRecord>,
}

/// A semantic boundary attributed to one file in a harness run report.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ObservedSemanticBoundary {
    /// Normalized test path that emitted the boundary.
    pub path: String,
    /// Stable boundary identifier from the runner.
    pub id: String,
    /// Classification used by the receipt lane.
    pub disposition: SemanticBoundaryDisposition,
    /// Compiler-provided explanation for the boundary.
    pub reason: String,
    /// Byte range of the source expression or phase that emitted the boundary.
    pub source_span: SemanticBoundarySourceSpan,
    /// Compiler source kind that produced the boundary.
    pub source_kind: String,
    /// Confidence in the retained classification.
    pub confidence: SemanticBoundaryConfidence,
    /// Whether the boundary makes the compile receipt fail.
    pub blocks_compilation: bool,
    /// Whether downstream static facts must be withheld or qualified.
    pub blocks_downstream_static_facts: bool,
    /// Whether the classification is guarded by a pinned path or source contract.
    pub lock_scope: SemanticBoundaryLockScope,
    /// Workstream responsible for deepening or replacing this classification.
    pub owner_workstream: String,
    /// Focused test or receipt path that supports this classification.
    pub supporting_test: String,
}

pub fn workstream_for_bucket(bucket: &str) -> &'static str {
    match bucket {
        "parse_recovery" => "parser_recovery",
        "source_decode" => "source_loading",
        "hir_lowering" => "hir",
        "compile_effect" => "compile_time_effects",
        "scope_pad" => "scope_and_pad",
        "package_stash" => "package_stash",
        "pragma_feature" => "pragma_model",
        "module_resolution" => "module_resolution",
        "runtime_value_model" => "runtime_value_model",
        "runtime_control_flow" => "runtime_control_flow",
        "runtime_io" => "runtime_io",
        "runtime_regex" => "runtime_regex",
        "runtime_require_use" => "runtime_require_use",
        "runtime_test_harness" => "runtime_test_harness",
        "cli_switch" => "harness_cli_compat",
        "harness_prepare" => "harness_integration",
        "unknown" => "compiler_conformance",
        _ => "compiler_conformance",
    }
}

pub fn lsp_impact_for_bucket(bucket: &str) -> Vec<&'static str> {
    match bucket {
        "parse_recovery" => vec!["diagnostics", "syntax_tree", "semantic_tokens"],
        "source_decode" => vec!["workspace_index", "diagnostics"],
        "hir_lowering" => vec!["definition", "rename", "diagnostics"],
        "compile_effect" => vec!["definition", "references", "diagnostics"],
        "scope_pad" => vec!["rename", "definition", "diagnostics"],
        "package_stash" => vec!["workspace_symbols", "completion", "definition"],
        "pragma_feature" => vec!["diagnostics", "semantic_tokens"],
        "module_resolution" => vec!["definition", "hover", "completion"],
        "runtime_value_model"
        | "runtime_control_flow"
        | "runtime_io"
        | "runtime_regex"
        | "runtime_require_use"
        | "runtime_test_harness" => vec!["compiler_conformance"],
        "cli_switch" | "harness_prepare" => vec!["compiler_conformance"],
        _ => vec!["compiler_conformance"],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_constants_are_stable() {
        assert_eq!(DISCOVERY_SCHEMA_VERSION, "perl_core_harness.discovery.v1");
        assert_eq!(RUN_REPORT_SCHEMA_VERSION, "perl_core_harness.report.v1");
        assert_eq!(COMPILE_BASELINE_SCHEMA_VERSION, "perl_core_harness.compile_baseline.v1");
        assert_eq!(SMOKE_SCHEMA_VERSION, "perl_core_harness.smoke.v1");
        assert_eq!(PREPARE_SCHEMA_VERSION, "perl_core_harness.prepare.v1");
        assert_eq!(GAP_MAP_SCHEMA_VERSION, "perl_core_harness.gap_map.v1");
        assert_eq!(RUNNER_RECORD_SCHEMA_VERSION, "perl_core_harness.runner_record.v1");
        assert_eq!(CURATED_GOLD_SCHEMA_VERSION, "curated_gold.v1");
        assert_eq!(CURATED_GOLD_COMPARISON_SCHEMA_VERSION, "curated_gold_comparison.v1");
    }

    #[test]
    fn enum_display_and_roots_match_harness_contract() {
        assert_eq!(HarnessRunner::Test.script_name(), "TEST");
        assert_eq!(HarnessRunner::Test.as_str(), "test");
        assert_eq!(HarnessRunner::Harness.script_name(), "harness");
        assert_eq!(HarnessRunner::Harness.as_str(), "harness");

        assert_eq!(HarnessMode::Parse.as_str(), "parse");
        assert_eq!(HarnessMode::Compile.as_str(), "compile");
        assert_eq!(HarnessMode::Execute.as_str(), "execute");

        assert_eq!(HarnessProfile::Base.as_str(), "base");
        assert_eq!(HarnessProfile::Base.roots(), &["base"]);
        assert!(HarnessProfile::Core.roots().contains(&"comp"));
    }

    #[test]
    fn bucket_mappings_remain_behavior_oriented() {
        assert_eq!(workstream_for_bucket("source_decode"), "source_loading");
        assert_eq!(workstream_for_bucket("hir_lowering"), "hir");
        assert_eq!(workstream_for_bucket("compile_effect"), "compile_time_effects");
        assert_eq!(workstream_for_bucket("scope_pad"), "scope_and_pad");
        assert_eq!(workstream_for_bucket("package_stash"), "package_stash");
        assert_eq!(workstream_for_bucket("pragma_feature"), "pragma_model");
        assert_eq!(workstream_for_bucket("module_resolution"), "module_resolution");
        assert_eq!(workstream_for_bucket("runtime_value_model"), "runtime_value_model");
        assert_eq!(workstream_for_bucket("runtime_control_flow"), "runtime_control_flow");
        assert_eq!(workstream_for_bucket("runtime_io"), "runtime_io");
        assert_eq!(workstream_for_bucket("runtime_regex"), "runtime_regex");
        assert_eq!(workstream_for_bucket("runtime_require_use"), "runtime_require_use");
        assert_eq!(workstream_for_bucket("runtime_test_harness"), "runtime_test_harness");
        assert_eq!(workstream_for_bucket("cli_switch"), "harness_cli_compat");
        assert_eq!(workstream_for_bucket("harness_prepare"), "harness_integration");
        assert_eq!(workstream_for_bucket("unknown_bucket"), "compiler_conformance");
        assert_eq!(lsp_impact_for_bucket("source_decode"), vec!["workspace_index", "diagnostics"]);
        assert_eq!(
            lsp_impact_for_bucket("compile_effect"),
            vec!["definition", "references", "diagnostics"]
        );
        assert_eq!(
            lsp_impact_for_bucket("module_resolution"),
            vec!["definition", "hover", "completion"]
        );
        assert_eq!(lsp_impact_for_bucket("runtime_value_model"), vec!["compiler_conformance"]);
        assert_eq!(lsp_impact_for_bucket("runtime_control_flow"), vec!["compiler_conformance"]);
        assert_eq!(lsp_impact_for_bucket("runtime_io"), vec!["compiler_conformance"]);
        assert_eq!(lsp_impact_for_bucket("runtime_regex"), vec!["compiler_conformance"]);
        assert_eq!(lsp_impact_for_bucket("runtime_require_use"), vec!["compiler_conformance"]);
        assert_eq!(lsp_impact_for_bucket("runtime_test_harness"), vec!["compiler_conformance"]);
        assert_eq!(lsp_impact_for_bucket("cli_switch"), vec!["compiler_conformance"]);
        assert_eq!(lsp_impact_for_bucket("harness_prepare"), vec!["compiler_conformance"]);
        assert_eq!(lsp_impact_for_bucket("unknown_bucket"), vec!["compiler_conformance"]);
    }

    #[test]
    fn runner_record_roundtrips() -> Result<(), Box<dyn std::error::Error>> {
        let record = RunnerRecord {
            schema_version: RUNNER_RECORD_SCHEMA_VERSION.to_string(),
            mode: "compile".to_string(),
            path: "base/ok.t".to_string(),
            status: RunnerStatus::Pass,
            assertions_passed: 1,
            assertions_total: 1,
            bucket: None,
            first_diagnostic: None,
            semantic_boundaries: Vec::new(),
        };

        let encoded = serde_json::to_string(&record)?;
        let decoded: RunnerRecord = serde_json::from_str(&encoded)?;
        assert_eq!(decoded, record);
        Ok(())
    }

    #[test]
    fn curated_gold_contract_roundtrips_with_strict_types() -> Result<(), Box<dyn std::error::Error>>
    {
        let fixture = CuratedGoldFixture {
            schema_version: CURATED_GOLD_SCHEMA_VERSION.to_string(),
            fixture_id: "gold.package.main".to_string(),
            fact_class: CuratedGoldFactClass::PackageSubTable,
            source: CuratedGoldSource::Inline { text: "package main;\n".to_string() },
            source_content_hash:
                "sha256:0000000000000000000000000000000000000000000000000000000000000000"
                    .to_string(),
            expected_facts: vec![CuratedGoldFact {
                fact_id: "package:main".to_string(),
                value: serde_json::json!({"name": "main"}),
                range: Some(CuratedGoldRange { start_byte: 0, end_byte: 12 }),
                provenance: "ExplicitSource".to_string(),
                confidence: CuratedGoldConfidence::High,
                freshness: CuratedGoldFreshness::Fresh,
                dynamic_boundary: None,
            }],
            expectation_hash:
                "sha256:1111111111111111111111111111111111111111111111111111111111111111"
                    .to_string(),
            author_identity: "author@example.test".to_string(),
            reviewer_identity: "reviewer@example.test".to_string(),
            review_receipt: "review-123".to_string(),
            rationale: "source-backed package".to_string(),
            coverage_intent: "package basics".to_string(),
            confidence: CuratedGoldConfidence::High,
            perl_references: vec!["perlfunc/package".to_string()],
            allowed_dynamic_boundaries: Vec::new(),
            minimum_compiler_capability: "compiler.facts.v1".to_string(),
        };
        let decoded: CuratedGoldFixture = serde_json::from_str(&serde_json::to_string(&fixture)?)?;
        if decoded != fixture {
            return Err("curated-gold fixture did not roundtrip".into());
        }
        Ok(())
    }

    #[test]
    fn legacy_runner_record_defaults_semantic_boundaries() -> Result<(), Box<dyn std::error::Error>>
    {
        let decoded: RunnerRecord = serde_json::from_str(
            r#"{"schema_version":"perl_core_harness.runner_record.v1","mode":"compile","path":"base/ok.t","status":"pass","assertions_passed":1,"assertions_total":1,"bucket":null,"first_diagnostic":null}"#,
        )?;

        assert!(decoded.semantic_boundaries.is_empty());
        Ok(())
    }
}
