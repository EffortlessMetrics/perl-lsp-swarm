#![warn(missing_docs)]
#![cfg_attr(clippy, allow(missing_docs))]

//! Shared receipt contracts for the upstream Perl core harness lane.

use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

pub const DISCOVERY_SCHEMA_VERSION: &str = "perl_core_harness.discovery.v1";
pub const RUN_REPORT_SCHEMA_VERSION: &str = "perl_core_harness.report.v1";
pub const COMPILE_BASELINE_SCHEMA_VERSION: &str = "perl_core_harness.compile_baseline.v1";
pub const COMPILE_BASELINE_V2_SCHEMA_VERSION: &str = "perl_core_harness.compile_baseline.v2";
pub const SMOKE_SCHEMA_VERSION: &str = "perl_core_harness.smoke.v1";
pub const PREPARE_SCHEMA_VERSION: &str = "perl_core_harness.prepare.v1";
pub const GAP_MAP_SCHEMA_VERSION: &str = "perl_core_harness.gap_map.v1";
pub const RUNNER_RECORD_SCHEMA_VERSION: &str = "perl_core_harness.runner_record.v1";
pub const COMPARISON_SERIES_SCHEMA_VERSION: &str = "perl_core_harness.comparison_series.v1";
pub const SERIES_MANIFEST_SCHEMA_VERSION: &str = COMPARISON_SERIES_SCHEMA_VERSION;
pub const SERIES_MANIFEST_NORMALIZATION_VERSION: &str = "path-normalization.v1";
pub const EVIDENCE_BUNDLE_SCHEMA_VERSION: &str = "perl_core_harness.evidence_bundle.v1";
pub const EVIDENCE_REPRODUCTION_SCHEMA_VERSION: &str = "perl_core_harness.evidence_reproduction.v1";

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

/// Immutable identity and denominator for a staged Perl harness comparison series.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SeriesManifest {
    pub schema_version: String,
    pub series_id: String,
    pub profile: HarnessProfile,
    pub profile_roots: Vec<String>,
    pub repository_commit: String,
    pub perl_requested_ref: String,
    pub perl_resolved_ref: String,
    pub runner: HarnessRunner,
    pub normalized_manifest: Vec<String>,
    pub manifest_hash: String,
    pub preparation_receipt_id: String,
    pub preparation_receipt_digest: String,
    pub harness_schema_version: String,
    pub compiler_subject_identity: String,
    pub invocation_identity: String,
    pub capability_identity: String,
    pub environment_identity: String,
    pub normalization_version: String,
    pub created_at: String,
    pub replaces_series_id: Option<String>,
    pub change_reason: Option<String>,
}

/// A reviewed transition proving that an accepted semantic boundary retired.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BoundaryRetirement {
    pub path: String,
    pub id: String,
    pub source_start: usize,
    pub source_end: usize,
    pub transition_id: String,
    pub replacement_issue: String,
    pub evidence_bundle: String,
}

/// Versioned compile baseline bound to one immutable comparison series.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CompileBaselineV2 {
    pub schema_version: String,
    pub report_schema_version: String,
    pub series_id: String,
    pub manifest_hash: String,
    pub repository_commit: String,
    pub perl_resolved_ref: String,
    pub preparation_receipt_id: String,
    pub compiler_subject_identity: String,
    pub invocation_identity: String,
    pub capability_identity: String,
    pub environment_identity: String,
    pub source_report_digest: String,
    pub accepted_transition_id: Option<String>,
    pub evidence_bundle: Option<String>,
    pub mode: HarnessMode,
    pub profile: HarnessProfile,
    pub runner: HarnessRunner,
    pub file_membership: Vec<String>,
    pub files_total: usize,
    pub files_passed: usize,
    pub files_failed: usize,
    pub tap_assertions_total: usize,
    pub tap_assertions_passed: usize,
    pub buckets: BTreeMap<String, usize>,
    pub expected_failures: Vec<RunFailure>,
    pub file_results: Vec<RunFileResult>,
    pub semantic_boundaries: Vec<ObservedSemanticBoundary>,
    pub boundary_retirements: Vec<BoundaryRetirement>,
}

/// A durable artifact class referenced by an upstream evidence bundle.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceArtifactKind {
    /// Immutable comparison-series manifest.
    ComparisonSeries,
    /// Preparation receipt for the pinned upstream tree.
    PreparationReceipt,
    /// Raw discovery output retained as diagnostic evidence.
    DiscoveryRaw,
    /// Normalized discovery report used for authority.
    DiscoveryNormalized,
    /// Parse-mode report.
    ParseReport,
    /// Compile-mode report.
    CompileReport,
    /// Optional execute-mode report.
    ExecuteReport,
    /// Normalized upstream runner records.
    RunnerRecords,
    /// Explicit semantic-boundary inventory.
    SemanticBoundaries,
    /// Bucketed gap map.
    GapMap,
    /// Structural smoke report.
    SmokeReport,
    /// Candidate baseline before acceptance.
    BaselineCandidate,
    /// Accepted baseline.
    BaselineAccepted,
    /// Deterministic direct-reproduction descriptor.
    Reproduction,
}

impl EvidenceArtifactKind {
    /// Stable machine-readable name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ComparisonSeries => "comparison_series",
            Self::PreparationReceipt => "preparation_receipt",
            Self::DiscoveryRaw => "discovery_raw",
            Self::DiscoveryNormalized => "discovery_normalized",
            Self::ParseReport => "parse_report",
            Self::CompileReport => "compile_report",
            Self::ExecuteReport => "execute_report",
            Self::RunnerRecords => "runner_records",
            Self::SemanticBoundaries => "semantic_boundaries",
            Self::GapMap => "gap_map",
            Self::SmokeReport => "smoke_report",
            Self::BaselineCandidate => "baseline_candidate",
            Self::BaselineAccepted => "baseline_accepted",
            Self::Reproduction => "reproduction",
        }
    }

    /// Stable filename used under a bundle's normalized evidence directory.
    pub const fn filename(self) -> &'static str {
        match self {
            Self::ComparisonSeries => "comparison-series.json",
            Self::PreparationReceipt => "preparation-receipt.json",
            Self::DiscoveryRaw => "discovery-raw.bin",
            Self::DiscoveryNormalized => "discovery.json",
            Self::ParseReport => "parse.json",
            Self::CompileReport => "compile.json",
            Self::ExecuteReport => "execute.json",
            Self::RunnerRecords => "runner-records.jsonl",
            Self::SemanticBoundaries => "semantic-boundaries.json",
            Self::GapMap => "gap-map.json",
            Self::SmokeReport => "smoke.json",
            Self::BaselineCandidate => "baseline-candidate.json",
            Self::BaselineAccepted => "baseline-accepted.json",
            Self::Reproduction => "reproduction.json",
        }
    }

    /// Artifacts required for a complete parse/compile acceptance bundle.
    pub const fn required() -> &'static [Self] {
        &[
            Self::ComparisonSeries,
            Self::PreparationReceipt,
            Self::DiscoveryRaw,
            Self::DiscoveryNormalized,
            Self::ParseReport,
            Self::CompileReport,
            Self::RunnerRecords,
            Self::SemanticBoundaries,
            Self::GapMap,
            Self::SmokeReport,
            Self::BaselineCandidate,
            Self::BaselineAccepted,
            Self::Reproduction,
        ]
    }
}

impl fmt::Display for EvidenceArtifactKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Retention class for a bundle artifact.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceRetentionClass {
    /// Normalized evidence committed with the repository.
    Committed,
    /// Large diagnostic output retained outside the repository.
    Diagnostic,
}

/// Visibility classification for an evidence artifact.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceVisibility {
    /// Safe for the normalized public packet.
    Public,
    /// Diagnostic evidence that may contain private execution details.
    Private,
    /// Redacted before publication.
    Redacted,
}

/// Lifecycle state of a bundle's authority.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceBundleLifecycle {
    /// Measurement packet not yet attached to a publication commit.
    Measurement,
    /// Publication identity has been mechanically verified.
    Published,
    /// Landed identity has been recorded after merge.
    Landed,
    /// Historical packet that is readable but not active authority.
    Historical,
}

/// Completeness state for the normalized evidence packet.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceCompleteness {
    /// All required artifacts and normalized authority are present.
    Complete,
    /// One or more required artifacts are missing.
    Incomplete,
}

/// One content-addressed artifact entry in an evidence bundle.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceArtifact {
    /// Artifact class.
    pub kind: EvidenceArtifactKind,
    /// Stable bundle-relative path; never a host filesystem path.
    pub logical_path: String,
    /// SHA-256 digest of the source bytes.
    pub content_digest: String,
    /// Source byte length.
    pub size_bytes: u64,
    /// Repository or external retention class.
    pub retention: EvidenceRetentionClass,
    /// Public/private/redacted classification.
    pub visibility: EvidenceVisibility,
    /// Whether the source was available when the measurement ran.
    pub available_at_measurement: bool,
}

/// Completeness and claim boundary attached to a bundle.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceBundleCompleteness {
    /// Complete or incomplete status.
    pub status: EvidenceCompleteness,
    /// Required artifact classes for this bundle.
    pub required_artifacts: Vec<EvidenceArtifactKind>,
    /// Required classes absent from the packet.
    pub missing_artifacts: Vec<EvidenceArtifactKind>,
    /// Whether normalized evidence is sufficient after raw artifact expiry.
    pub normalized_authority: bool,
    /// Explicit claim boundary, such as `parse_compile_acceptance`.
    pub claim_boundary: String,
}

/// Separate Git identities for measurement, publication, and landing.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceBundleLineage {
    /// Repository state whose compiler and harness were executed.
    pub measurement_sha: String,
    /// Evidence-only publication state, once verified.
    pub publication_sha: Option<String>,
    /// Final merged state, which may be a squash descendant.
    pub landed_sha: Option<String>,
}

/// Content-addressed index for one accepted upstream evidence bundle.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceBundleIndex {
    /// Versioned bundle schema.
    pub schema_version: String,
    /// Stable caller-declared bundle identity.
    pub bundle_id: String,
    /// Digest of the canonical index contents excluding this field.
    pub bundle_digest: String,
    /// Comparison-series identity.
    pub series_id: String,
    /// Exact normalized denominator hash.
    pub manifest_hash: String,
    /// Repository commit retained by the series.
    pub repository_commit: String,
    /// Selected profile and upstream runner.
    pub profile: HarnessProfile,
    pub runner: HarnessRunner,
    /// Requested and resolved Perl identities.
    pub perl_requested_ref: String,
    pub perl_resolved_ref: String,
    /// Preparation identity copied from the series.
    pub preparation_receipt_id: String,
    pub preparation_receipt_digest: String,
    /// Measured subject identities copied from the series.
    pub compiler_subject_identity: String,
    pub invocation_identity: String,
    pub capability_identity: String,
    pub environment_identity: String,
    /// Measurement/publication/landing lineage.
    pub lineage: EvidenceBundleLineage,
    /// Artifact inventory.
    pub artifacts: Vec<EvidenceArtifact>,
    /// Completeness and claim boundary.
    pub completeness: EvidenceBundleCompleteness,
    /// Current authority lifecycle.
    pub lifecycle: EvidenceBundleLifecycle,
    /// Creation timestamp.
    pub created_at: String,
}

/// Deterministic direct-reproduction descriptor stored in a bundle.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceReproductionDescriptor {
    /// Versioned reproduction schema.
    pub schema_version: String,
    /// Series reproduced by the commands.
    pub series_id: String,
    /// Profile and modes covered.
    pub profile: HarnessProfile,
    pub modes: Vec<HarnessMode>,
    /// Explicit commands with no reliance on PR prose.
    pub commands: Vec<String>,
    /// Subject identity expected by the reproduction.
    pub compiler_subject_identity: String,
    /// Whether the commands are deterministic for the declared inputs.
    pub deterministic: bool,
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
    SeriesMismatch,
    ManifestMismatch,
    UnexpectedFile,
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
    MissingBoundaryInventory,
    BoundaryRemovedWithoutRetirement,
    MeasuredSubjectMismatch,
    PreparationIdentityMismatch,
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
        assert_eq!(COMPILE_BASELINE_V2_SCHEMA_VERSION, "perl_core_harness.compile_baseline.v2");
        assert_eq!(COMPARISON_SERIES_SCHEMA_VERSION, "perl_core_harness.comparison_series.v1");
        assert_eq!(SMOKE_SCHEMA_VERSION, "perl_core_harness.smoke.v1");
        assert_eq!(PREPARE_SCHEMA_VERSION, "perl_core_harness.prepare.v1");
        assert_eq!(GAP_MAP_SCHEMA_VERSION, "perl_core_harness.gap_map.v1");
        assert_eq!(RUNNER_RECORD_SCHEMA_VERSION, "perl_core_harness.runner_record.v1");
        assert_eq!(SERIES_MANIFEST_SCHEMA_VERSION, "perl_core_harness.comparison_series.v1");
        assert_eq!(SERIES_MANIFEST_NORMALIZATION_VERSION, "path-normalization.v1");
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
    fn legacy_runner_record_defaults_semantic_boundaries() -> Result<(), Box<dyn std::error::Error>>
    {
        let decoded: RunnerRecord = serde_json::from_str(
            r#"{"schema_version":"perl_core_harness.runner_record.v1","mode":"compile","path":"base/ok.t","status":"pass","assertions_passed":1,"assertions_total":1,"bucket":null,"first_diagnostic":null}"#,
        )?;

        assert!(decoded.semantic_boundaries.is_empty());
        Ok(())
    }
}
