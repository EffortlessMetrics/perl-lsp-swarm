#![warn(missing_docs)]
#![cfg_attr(clippy, allow(missing_docs))]

//! Shared receipt contracts for the upstream Perl core harness lane.
#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.

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
pub const BOUNDARY_RETIREMENT_SCHEMA_VERSION: &str = "perl_core_harness.boundary_retirement.v1";
pub const SEMANTIC_BOUNDARY_REGISTRY_SCHEMA_VERSION: &str =
    "perl_core_harness.semantic_boundary_registry.v1";
pub const FAILURE_CLUSTER_SCHEMA_VERSION: &str = "perl_core_harness.failure_cluster.v1";
pub const FAILURE_CLUSTER_HISTORY_SCHEMA_VERSION: &str =
    "perl_core_harness.failure_cluster_history.v1";
pub const LANDED_LINEAGE_SCHEMA_VERSION: &str = "perl_core_harness.landed_lineage.v1";
pub const CURRENT_AUTHORITY_INDEX_SCHEMA_VERSION: &str =
    "perl_core_harness.current_authority_index.v1";
pub const COMPILER_COMPATIBILITY_SCHEMA_VERSION: &str =
    "perl_core_harness.compiler_compatibility.v1";

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
#[serde(deny_unknown_fields)]
pub struct BoundaryRetirement {
    /// Schema for this retirement receipt.
    pub schema_version: String,
    pub path: String,
    pub id: String,
    pub source_start: usize,
    pub source_end: usize,
    /// Comparison series that emitted the retired boundary.
    pub series_id: String,
    /// Comparison-series manifest hash identifying the retired boundary's denominator.
    pub manifest_hash: String,
    /// Compiler measurement commit used for the replacement run.
    pub measurement_sha: String,
    /// Stable digest of the replacement run report.
    pub source_report_digest: String,
    pub transition_id: String,
    pub replacement_issue: String,
    /// Content-addressed #5171 evidence bundle reference.
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

/// One upstream test discovered by `--dumptests`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveredTest {
    pub path: String,
    pub root: String,
}

/// Machine-readable parse/compile/execute report.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct RunSummary {
    pub files_total: usize,
    pub files_passed: usize,
    pub files_failed: usize,
    pub tap_assertions_total: usize,
    pub tap_assertions_passed: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RunFileResult {
    pub path: String,
    pub status: RunnerStatus,
    pub assertions_passed: usize,
    pub assertions_total: usize,
    /// Rail that produced this file's execution result.
    ///
    /// Carried so that deriving runner records back out of a report preserves
    /// the mechanism instead of silently defaulting it. `None` for parse and
    /// compile results, and skipped when absent so existing reports and
    /// checked-in baselines keep their wire form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mechanism: Option<ExecutionMechanism>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
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
    BoundaryRetirementReceiptMismatch,
    BoundaryRetirementReferencesUnknownBoundary,
    MeasuredSubjectMismatch,
    PreparationIdentityMismatch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunnerStatus {
    Pass,
    Fail,
}

/// How an execution-like harness receipt produced its result.
///
/// The three mechanisms are independent evidence classes and never satisfy one
/// another. Recognizing a fixture's exact source shape and emitting
/// scaffold-generated TAP is not evaluation through an executable compiler
/// representation, and neither is agreement with a real Perl process. The
/// protocol bytes can be real TAP while the represented program behavior is
/// scaffold-generated, so the mechanism — not the `execute` mode string — is
/// what a consumer must read before attributing runtime meaning to a receipt.
///
/// The mechanism names the rail that produced the receipt, not a claim that the
/// rail succeeded: a failed fixture-replay attempt is still fixture-replay
/// evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMechanism {
    /// Exact fixture or source-shape recognition with scaffold-generated
    /// behavior. Bounded harness-wiring evidence only.
    FixtureReplay,
    /// Source admitted by a versioned executable profile and evaluated through
    /// EIR, bound to an exact evaluator identity.
    EirExecution,
    /// Separately bounded differential evidence from a real Perl process.
    RealPerlOracle,
}

impl ExecutionMechanism {
    /// Closed list of every execution mechanism.
    pub const ALL: [Self; 3] = [Self::FixtureReplay, Self::EirExecution, Self::RealPerlOracle];

    /// Stable canonical tag, identical to the serialized form.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FixtureReplay => "fixture_replay",
            Self::EirExecution => "eir_execution",
            Self::RealPerlOracle => "real_perl_oracle",
        }
    }
}

impl fmt::Display for ExecutionMechanism {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Mechanisms this repository can currently produce evidence for.
///
/// [`ExecutionMechanism::EirExecution`] and
/// [`ExecutionMechanism::RealPerlOracle`] are deliberately absent. No
/// executable profile, evaluator identity, or differential oracle rail exists
/// yet, so a receipt claiming either would be a relabeling of scaffold
/// behavior rather than new evidence. Landing such a rail means adding its
/// mechanism here together with the evidence that backs it.
pub const SUPPORTED_EXECUTION_MECHANISMS: &[ExecutionMechanism] =
    &[ExecutionMechanism::FixtureReplay];

/// Why an execution-mechanism claim on a receipt is not admissible.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExecutionMechanismViolation {
    /// An execution-mode receipt did not name the rail that produced it.
    Missing {
        /// Harness mode the receipt declared.
        mode: String,
    },
    /// A non-execution receipt named an execution mechanism.
    OutsideExecution {
        /// Harness mode the receipt declared.
        mode: String,
        /// Mechanism the receipt claimed.
        mechanism: ExecutionMechanism,
    },
    /// The receipt claimed a mechanism no current rail can supply.
    Unsupported {
        /// Mechanism the receipt claimed.
        mechanism: ExecutionMechanism,
    },
}

impl fmt::Display for ExecutionMechanismViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing { mode } => {
                write!(f, "{mode}-mode receipt does not declare an execution mechanism")
            }
            Self::OutsideExecution { mode, mechanism } => write!(
                f,
                "{mode}-mode receipt declares execution mechanism {mechanism}, \
                 which only execution receipts may carry"
            ),
            Self::Unsupported { mechanism } => write!(
                f,
                "receipt claims execution mechanism {mechanism}, which no current rail \
                 can supply; relabeling a receipt does not create its evidence"
            ),
        }
    }
}

/// Reject an execution-mechanism claim a harness receipt cannot support.
///
/// Execution receipts must name their rail; parse and compile receipts must
/// not, because a mechanism there would attribute execution evidence to a
/// receipt that never ran anything. A mechanism outside
/// [`SUPPORTED_EXECUTION_MECHANISMS`] is refused wherever it appears, so a
/// scaffold receipt cannot be promoted to an EIR or oracle claim by editing one
/// field.
pub fn validate_execution_mechanism(
    mode: &str,
    mechanism: Option<ExecutionMechanism>,
) -> Result<(), ExecutionMechanismViolation> {
    let is_execution = mode == HarnessMode::Execute.as_str();
    match mechanism {
        None if is_execution => {
            Err(ExecutionMechanismViolation::Missing { mode: mode.to_string() })
        }
        None => Ok(()),
        Some(mechanism) if !is_execution => {
            Err(ExecutionMechanismViolation::OutsideExecution { mode: mode.to_string(), mechanism })
        }
        Some(mechanism) if !SUPPORTED_EXECUTION_MECHANISMS.contains(&mechanism) => {
            Err(ExecutionMechanismViolation::Unsupported { mechanism })
        }
        Some(_) => Ok(()),
    }
}

/// Reject a receipt collection whose per-file execution-mechanism claims are
/// not admissible for the mode that produced them.
///
/// Reports and baselines carry the same claim as runner records do, and a
/// report is what becomes a checked-in baseline and what the ratchet compares
/// against. Validating only the runner-record format would leave the artifact
/// of record forgeable by editing one field of the report instead (#14363).
///
/// The offending path is returned with the violation so the caller can name the
/// file rather than the collection.
pub fn validate_file_result_mechanisms(
    mode: HarnessMode,
    file_results: &[RunFileResult],
) -> Result<(), (&str, ExecutionMechanismViolation)> {
    for result in file_results {
        if let Err(violation) = validate_execution_mechanism(mode.as_str(), result.mechanism) {
            return Err((result.path.as_str(), violation));
        }
    }
    Ok(())
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
    /// Rail that produced an execution receipt.
    ///
    /// `None` for parse and compile receipts, which execute nothing. Required
    /// on execution receipts and validated by
    /// [`validate_execution_mechanism`]; the field is skipped when absent so
    /// parse and compile artifacts keep their existing wire form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mechanism: Option<ExecutionMechanism>,
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

/// Lifecycle state of a governed semantic-boundary registry entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticBoundaryRegistryState {
    /// The boundary is currently emitted and must be observed in fresh evidence.
    Active,
    /// The boundary is expected to disappear after a reviewed replacement lands.
    Retiring,
    /// The boundary has disappeared in an exact-series receipt and is retained as history.
    Retired,
}

/// Reviewed replacement strategy for a governed semantic boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticBoundaryReplacementStrategy {
    GeneralParser,
    HirSemantics,
    CompileWorld,
    AbstractCompileTimeEngine,
    PlatformCapabilityModel,
    ExecutableProfileEir,
    LongLivedTestHarnessCompatibility,
}

/// One durable owner and proof record for an observed semantic boundary.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SemanticBoundaryRegistryEntry {
    pub id: String,
    pub disposition: SemanticBoundaryDisposition,
    pub source_kind: String,
    pub semantic_meaning: String,
    pub series_id: String,
    pub profile: HarnessProfile,
    pub path: String,
    pub manifest_hash: String,
    pub source_span: SemanticBoundarySourceSpan,
    pub source_shape: String,
    pub lock_scope: SemanticBoundaryLockScope,
    pub reason: String,
    pub ambient_dependency: String,
    pub blocks_downstream_static_facts: bool,
    pub owner_issue: String,
    pub supporting_test: String,
    pub wrong_file_test: String,
    pub changed_shape_test: String,
    pub introduction_pr: String,
    pub introduction_commit: String,
    pub first_accepted_bundle: String,
    pub replacement_strategy: SemanticBoundaryReplacementStrategy,
    pub state: SemanticBoundaryRegistryState,
    pub retirement_pr: Option<String>,
    pub retirement_bundle: Option<String>,
    pub review_after: Option<String>,
    pub permanent_boundary_rationale: Option<String>,
}

/// Versioned machine-readable semantic-boundary registry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SemanticBoundaryRegistry {
    pub schema_version: String,
    pub entries: Vec<SemanticBoundaryRegistryEntry>,
}

/// Normalized, membership-independent root-cause signature for one failure cluster.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FailureClusterSignature {
    pub schema_version: String,
    pub series_id: String,
    pub profile: HarnessProfile,
    pub mode: HarnessMode,
    pub stage: String,
    pub bucket: String,
    pub workstream: String,
    pub source_shape_fingerprint: String,
    pub fact_classes: Vec<String>,
    pub lsp_surfaces: Vec<String>,
}

/// Deterministically reproducible compiler work cluster derived from one bundle.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FailureCluster {
    pub cluster_id: String,
    pub signature: FailureClusterSignature,
    pub affected_files: Vec<String>,
    pub representative_failure: RunFailure,
    pub direct_reproduction: String,
    pub impacted_layer: String,
    pub fact_classes: Vec<String>,
    pub lsp_surfaces: Vec<String>,
    pub occurrence_count: usize,
    pub exact_series_proof_required: bool,
}

/// A semantic-boundary debt candidate kept separate from product failures.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FailureDebtCandidate {
    pub path: String,
    pub id: String,
    pub disposition: SemanticBoundaryDisposition,
    pub reason: String,
    pub owner_workstream: String,
    pub exact_series_proof_required: bool,
}

/// Deterministic cluster and debt-candidate report for one evidence bundle.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FailureClusterReport {
    pub schema_version: String,
    pub bundle_id: String,
    pub series_id: String,
    pub manifest_hash: String,
    pub repository_commit: String,
    pub profile: HarnessProfile,
    pub mode: HarnessMode,
    pub clusters: Vec<FailureCluster>,
    pub debt_candidates: Vec<FailureDebtCandidate>,
}

/// Lifecycle state for a persisted compiler failure cluster.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureClusterHistoryStatus {
    Unassigned,
    Investigating,
    BuilderReady,
    InBuild,
    Resolved,
    AcceptedDebt,
}

/// Whether a history entry is present in the current authoritative bundle.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureClusterHistoryPresence {
    Observed,
    AbsentUnresolved,
    Resolved,
    AcceptedDebt,
}

/// Whether the persisted cluster identity is backed by typed evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureClusterIdentityQuality {
    Provisional,
    Typed,
}

/// Explicit before/after evidence for a cluster or stage transition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FailureClusterHistoryTransition {
    pub transition_id: String,
    pub from_cluster_id: String,
    pub to_cluster_id: Option<String>,
    pub to_presence: FailureClusterHistoryPresence,
    pub from_stage: String,
    pub to_stage: String,
    pub before_series_id: String,
    pub before_manifest_hash: String,
    pub before_bundle_id: String,
    pub after_series_id: String,
    pub after_manifest_hash: String,
    pub after_bundle_id: String,
    pub proof_plan: String,
    pub stop_condition: String,
    pub implementation_pr: Option<String>,
}

/// Durable state for one cluster across authoritative evidence bundles.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FailureClusterHistoryEntry {
    pub cluster_id: String,
    pub signature_schema_version: String,
    pub identity_quality: FailureClusterIdentityQuality,
    pub series_id: String,
    pub manifest_hash: String,
    pub first_seen_series_id: String,
    pub first_seen_manifest_hash: String,
    pub last_seen_series_id: String,
    pub last_seen_manifest_hash: String,
    pub first_seen_bundle: String,
    pub last_seen_bundle: String,
    pub current_affected_files: Vec<String>,
    pub historical_affected_files: Vec<String>,
    pub current_fact_classes: Vec<String>,
    pub fact_classes: Vec<String>,
    pub current_lsp_surfaces: Vec<String>,
    pub lsp_surfaces: Vec<String>,
    pub occurrence_count: usize,
    pub current_stage: Option<String>,
    pub current_authority_bundle: Option<String>,
    pub observed_in_current_bundle: bool,
    pub absence_since_bundle: Option<String>,
    pub presence: FailureClusterHistoryPresence,
    pub impacted_layer: String,
    pub owner_issue: Option<String>,
    pub status: FailureClusterHistoryStatus,
    pub direct_reproduction: String,
    pub proposed_transition: String,
    pub stop_condition: String,
    pub accepted_debt_refs: Vec<String>,
    pub resolution_pr: Option<String>,
    pub resolution_bundle: Option<String>,
    pub transitions: Vec<FailureClusterHistoryTransition>,
}

/// Versioned persistent cluster history and ownership ledger.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FailureClusterHistory {
    pub schema_version: String,
    pub entries: Vec<FailureClusterHistoryEntry>,
}

/// Transition observed by a published bundle relative to the accepted state.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityTransition {
    /// The observation does not change the accepted state.
    NoChange,
    /// The observation is a candidate improvement that still needs acceptance.
    ImprovementCandidate,
    /// The observation regresses from the accepted state.
    Regression,
    /// The declared contract may need a reviewed correction.
    ContractCorrectionCandidate,
    /// The evidence is insufficient to establish a transition.
    NotProven,
    /// The record is retained only for history.
    Historical,
}

/// Lifecycle of a comparison-series authority record.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CurrentAuthorityStatus {
    /// The record is the current authority for its series.
    Current,
    /// The record is retained for historical inspection.
    Historical,
    /// The record was replaced by a later authority record.
    Superseded,
}

/// Post-merge identity binding for one published evidence bundle.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LandedLineage {
    pub schema_version: String,
    pub series_id: String,
    pub profile: HarnessProfile,
    pub manifest_hash: String,
    pub evidence_bundle_id: String,
    pub evidence_bundle_digest: String,
    pub measurement_sha: String,
    pub publication_sha: String,
    pub landed_sha: String,
    pub publication_base_sha: String,
    pub authoritative_artifacts: BTreeMap<String, String>,
    pub publication_paths: Vec<String>,
    pub accepted_transition_id: Option<String>,
    pub accepted_baseline_digest: Option<String>,
    pub accepted_baseline_evidence_bundle: Option<String>,
    pub observation_transition: CompatibilityTransition,
    pub recorder_schema_version: String,
    pub created_reason: String,
    pub supersedes: Option<String>,
}

/// One series entry in the deterministic current-authority index.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CurrentAuthorityEntry {
    pub series_id: String,
    pub profile: HarnessProfile,
    pub manifest_hash: String,
    pub observation_bundle_path: String,
    pub observation_bundle_id: String,
    pub observation_bundle_digest: String,
    pub observation_transition: CompatibilityTransition,
    pub accepted_baseline_path: Option<String>,
    pub accepted_baseline_digest: Option<String>,
    pub accepted_baseline_evidence_bundle: Option<String>,
    pub accepted_transition_id: Option<String>,
    pub landed_lineage_path: String,
    pub status: CurrentAuthorityStatus,
    pub claim_boundary: String,
    pub unavailable_rails: Vec<String>,
}

/// Deterministic index of current and historical series authorities.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CurrentAuthorityIndex {
    pub schema_version: String,
    pub entries: Vec<CurrentAuthorityEntry>,
}

/// Availability of an optional correctness or execution rail.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityRailAvailability {
    Available,
    /// The rail has evidence, but only for a declared subset of its contract.
    Partial,
    NotAvailable,
    /// Evidence exists, but its freshness contract prevents current use.
    Stale,
}

/// Explicit state for a rail that may be absent without becoming zero/pass.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CompatibilityRailState {
    pub availability: CompatibilityRailAvailability,
    pub reason: String,
    pub schema_version: Option<String>,
    pub evidence_refs: Vec<String>,
}

/// Parse or compile acceptance counts for one immutable series.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CompatibilityRunState {
    pub schema_version: String,
    pub mode: HarnessMode,
    pub files_total: usize,
    pub files_passed: usize,
    pub files_failed: usize,
    pub tap_assertions_total: usize,
    pub tap_assertions_passed: usize,
    pub baseline_schema_version: String,
    pub report_schema_version: String,
    pub evidence_bundle_id: String,
    pub cluster_count: usize,
}

/// The measured state of one current evidence bundle.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CompatibilityObservation {
    pub observation_bundle_id: String,
    pub measurement_sha: String,
    pub parse: CompatibilityRunState,
    pub compile: CompatibilityRunState,
    pub debt: CompatibilityDebtState,
    pub clusters: CompatibilityClusterState,
    pub execution: CompatibilityRailState,
    pub curated_gold: CompatibilityRailState,
    pub differential_oracle: CompatibilityRailState,
    pub eir: CompatibilityRailState,
    pub claim_boundary: String,
}

/// A proposed transition from the accepted ratchet to the current observation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CompatibilityTransitionCandidate {
    pub transition: CompatibilityTransition,
    pub reason: String,
    pub requires_acceptance: bool,
}

/// The accepted ratchet retained separately from current measured evidence.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CompatibilityAcceptedRatchet {
    pub baseline_schema_version: String,
    pub baseline_digest: String,
    pub baseline_evidence_bundle_id: Option<String>,
    pub accepted_transition_id: Option<String>,
    pub files_total: usize,
    pub files_passed: usize,
}

/// Boundary and accepted-debt counts kept separate from acceptance.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CompatibilityDebtState {
    pub boundary_count: usize,
    pub source_locked_count: usize,
    pub downstream_blocking_count: usize,
    pub by_disposition: BTreeMap<String, usize>,
    pub by_lock_scope: BTreeMap<String, usize>,
    pub registry: CompatibilityRailState,
    pub history: CompatibilityRailState,
}

/// Active cluster and owner counts for one series.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CompatibilityClusterState {
    pub active_count: usize,
    pub unassigned_count: usize,
    pub by_status: BTreeMap<String, usize>,
    pub history_bundle_id: Option<String>,
}

/// Identity and evidence lineage for one independently compared series.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CompatibilitySeriesIdentity {
    pub series_id: String,
    pub profile: HarnessProfile,
    pub profile_roots: Vec<String>,
    pub manifest_hash: String,
    pub denominator: usize,
    pub repository_commit: String,
    pub perl_requested_ref: String,
    pub perl_resolved_ref: String,
    pub runner: HarnessRunner,
    pub compiler_subject_identity: String,
    pub invocation_identity: String,
    pub capability_identity: String,
    pub environment_identity: String,
    pub preparation_receipt_id: String,
    pub preparation_receipt_digest: String,
    pub measurement_sha: String,
    pub publication_sha: Option<String>,
    pub landed_sha: Option<String>,
    pub evidence_bundle_id: String,
}

/// Complete typed compatibility state consumed by generated ledgers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CompilerCompatibilitySeries {
    pub identity: CompatibilitySeriesIdentity,
    pub current_observation: CompatibilityObservation,
    pub transition_candidate: CompatibilityTransitionCandidate,
    pub accepted_ratchet: CompatibilityAcceptedRatchet,
    pub parse: CompatibilityRunState,
    pub compile: CompatibilityRunState,
    pub debt: CompatibilityDebtState,
    pub clusters: CompatibilityClusterState,
    pub execution: CompatibilityRailState,
    pub curated_gold: CompatibilityRailState,
    pub differential_oracle: CompatibilityRailState,
    pub eir: CompatibilityRailState,
    pub claim_boundary: String,
}

/// Versioned, series-separated compiler compatibility input state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CompilerCompatibilityState {
    pub schema_version: String,
    pub repository_commit: String,
    pub series: Vec<CompilerCompatibilitySeries>,
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
    fn schema_constants_are_stable() -> Result<(), String> {
        let expected = [
            (DISCOVERY_SCHEMA_VERSION, "perl_core_harness.discovery.v1"),
            (RUN_REPORT_SCHEMA_VERSION, "perl_core_harness.report.v1"),
            (COMPILE_BASELINE_SCHEMA_VERSION, "perl_core_harness.compile_baseline.v1"),
            (COMPILE_BASELINE_V2_SCHEMA_VERSION, "perl_core_harness.compile_baseline.v2"),
            (COMPARISON_SERIES_SCHEMA_VERSION, "perl_core_harness.comparison_series.v1"),
            (SMOKE_SCHEMA_VERSION, "perl_core_harness.smoke.v1"),
            (PREPARE_SCHEMA_VERSION, "perl_core_harness.prepare.v1"),
            (GAP_MAP_SCHEMA_VERSION, "perl_core_harness.gap_map.v1"),
            (RUNNER_RECORD_SCHEMA_VERSION, "perl_core_harness.runner_record.v1"),
            (SERIES_MANIFEST_SCHEMA_VERSION, "perl_core_harness.comparison_series.v1"),
            (SERIES_MANIFEST_NORMALIZATION_VERSION, "path-normalization.v1"),
            (BOUNDARY_RETIREMENT_SCHEMA_VERSION, "perl_core_harness.boundary_retirement.v1"),
            (
                SEMANTIC_BOUNDARY_REGISTRY_SCHEMA_VERSION,
                "perl_core_harness.semantic_boundary_registry.v1",
            ),
            (FAILURE_CLUSTER_SCHEMA_VERSION, "perl_core_harness.failure_cluster.v1"),
            (
                FAILURE_CLUSTER_HISTORY_SCHEMA_VERSION,
                "perl_core_harness.failure_cluster_history.v1",
            ),
            (LANDED_LINEAGE_SCHEMA_VERSION, "perl_core_harness.landed_lineage.v1"),
            (
                CURRENT_AUTHORITY_INDEX_SCHEMA_VERSION,
                "perl_core_harness.current_authority_index.v1",
            ),
        ];
        for (actual, expected) in expected {
            if actual != expected {
                return Err(format!("schema constant {actual:?} did not equal {expected:?}"));
            }
        }
        if COMPILER_COMPATIBILITY_SCHEMA_VERSION != "perl_core_harness.compiler_compatibility.v1" {
            return Err("compiler compatibility schema constant changed".to_string());
        }
        Ok(())
    }

    #[test]
    fn run_report_rejects_unknown_fields() {
        let json = r#"{
            "schema_version": "perl_core_harness.report.v1",
            "commit": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "timestamp": "2026-08-12T00:00:00Z",
            "perl_ref": "perl",
            "prepared_tree": "<prepared>",
            "run_tree": "<run>",
            "host_perl": "perl",
            "runner": "test",
            "mode": "compile",
            "profile": "base",
            "harness_status": 0,
            "summary": {
                "files_total": 0,
                "files_passed": 0,
                "files_failed": 0,
                "tap_assertions_total": 0,
                "tap_assertions_passed": 0
            },
            "buckets": {},
            "file_results": [],
            "failures": [],
            "unexpected_authority_field": true
        }"#;
        let err = serde_json::from_str::<RunReport>(json).err();
        assert!(
            err.is_some_and(|err| err.to_string().contains("unexpected_authority_field")),
            "unknown field must be rejected with 'unexpected_authority_field'"
        );
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
            mechanism: None,
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

    fn execute_record(mechanism: Option<ExecutionMechanism>) -> RunnerRecord {
        RunnerRecord {
            schema_version: RUNNER_RECORD_SCHEMA_VERSION.to_string(),
            mode: "execute".to_string(),
            path: "base/if.t".to_string(),
            status: RunnerStatus::Pass,
            assertions_passed: 2,
            assertions_total: 2,
            bucket: None,
            first_diagnostic: None,
            mechanism,
            semantic_boundaries: Vec::new(),
        }
    }

    #[test]
    fn execution_receipt_without_a_mechanism_is_rejected() {
        assert_eq!(
            validate_execution_mechanism("execute", None),
            Err(ExecutionMechanismViolation::Missing { mode: "execute".to_string() }),
            "an execution receipt that does not name its rail is not evidence"
        );
    }

    #[test]
    fn non_execution_receipt_carrying_a_mechanism_is_rejected() {
        for mode in ["parse", "compile"] {
            let violation =
                validate_execution_mechanism(mode, Some(ExecutionMechanism::FixtureReplay));
            assert_eq!(
                violation,
                Err(ExecutionMechanismViolation::OutsideExecution {
                    mode: mode.to_string(),
                    mechanism: ExecutionMechanism::FixtureReplay,
                }),
                "{mode} executes nothing, so it must not attribute execution evidence"
            );
        }
    }

    #[test]
    fn relabeling_a_replay_receipt_does_not_create_its_evidence() {
        for mechanism in [ExecutionMechanism::EirExecution, ExecutionMechanism::RealPerlOracle] {
            assert_eq!(
                validate_execution_mechanism("execute", Some(mechanism)),
                Err(ExecutionMechanismViolation::Unsupported { mechanism }),
                "{mechanism} has no rail, so editing one field must not promote the claim"
            );
        }
    }

    #[test]
    fn fixture_replay_is_the_only_admissible_execution_mechanism_today() {
        assert_eq!(
            SUPPORTED_EXECUTION_MECHANISMS,
            &[ExecutionMechanism::FixtureReplay],
            "adding a mechanism here without its evidence rail would re-open #8254"
        );
        assert!(
            validate_execution_mechanism("execute", Some(ExecutionMechanism::FixtureReplay))
                .is_ok()
        );
    }

    #[test]
    fn unsupported_mechanisms_are_still_distinct_named_classes() {
        // The closed taxonomy has to survive even while two of its members are
        // inadmissible: an EIR rail lands by supplying evidence, not by
        // discovering the vocabulary.
        assert_eq!(ExecutionMechanism::ALL.len(), 3);
        assert_ne!(ExecutionMechanism::FixtureReplay, ExecutionMechanism::EirExecution);
        assert_ne!(ExecutionMechanism::FixtureReplay, ExecutionMechanism::RealPerlOracle);
        assert_eq!(ExecutionMechanism::FixtureReplay.as_str(), "fixture_replay");
        assert_eq!(ExecutionMechanism::EirExecution.as_str(), "eir_execution");
        assert_eq!(ExecutionMechanism::RealPerlOracle.as_str(), "real_perl_oracle");
    }

    #[test]
    fn unknown_mechanism_tags_fail_closed() {
        // A free-form string would let a new label enter the receipt without a
        // reviewed evidence class; the closed enum makes that a decode failure.
        let decoded = serde_json::from_str::<RunnerRecord>(
            r#"{"schema_version":"perl_core_harness.runner_record.v1","mode":"execute","path":"base/if.t","status":"pass","assertions_passed":2,"assertions_total":2,"bucket":null,"first_diagnostic":null,"mechanism":"compiler_execution"}"#,
        );

        assert!(decoded.is_err(), "an unknown mechanism tag must not decode");
    }

    #[test]
    fn execution_mechanism_roundtrips_on_the_wire() -> Result<(), Box<dyn std::error::Error>> {
        let record = execute_record(Some(ExecutionMechanism::FixtureReplay));

        let encoded = serde_json::to_string(&record)?;
        assert!(
            encoded.contains(r#""mechanism":"fixture_replay""#),
            "execute receipts must state their mechanism on the wire: {encoded}"
        );
        let decoded: RunnerRecord = serde_json::from_str(&encoded)?;
        assert_eq!(decoded, record);
        Ok(())
    }

    #[test]
    fn parse_and_compile_receipts_keep_their_existing_wire_form()
    -> Result<(), Box<dyn std::error::Error>> {
        let record = RunnerRecord {
            schema_version: RUNNER_RECORD_SCHEMA_VERSION.to_string(),
            mode: "compile".to_string(),
            path: "base/ok.t".to_string(),
            status: RunnerStatus::Pass,
            assertions_passed: 1,
            assertions_total: 1,
            bucket: None,
            first_diagnostic: None,
            mechanism: None,
            semantic_boundaries: Vec::new(),
        };

        let encoded = serde_json::to_string(&record)?;
        assert!(
            !encoded.contains("mechanism"),
            "a receipt that executed nothing must not gain a mechanism key: {encoded}"
        );
        Ok(())
    }

    #[test]
    fn legacy_execute_records_do_not_silently_gain_a_mechanism()
    -> Result<(), Box<dyn std::error::Error>> {
        // Historical execute receipts predate the field. They decode, so they
        // stay readable, but they do not become admissible evidence.
        let decoded: RunnerRecord = serde_json::from_str(
            r#"{"schema_version":"perl_core_harness.runner_record.v1","mode":"execute","path":"base/if.t","status":"pass","assertions_passed":2,"assertions_total":2,"bucket":null,"first_diagnostic":null}"#,
        )?;

        assert_eq!(decoded.mechanism, None);
        assert!(
            validate_execution_mechanism(&decoded.mode, decoded.mechanism).is_err(),
            "a legacy execute receipt is not current evidence"
        );
        Ok(())
    }

    #[test]
    fn run_file_results_carry_the_mechanism_for_report_derivation()
    -> Result<(), Box<dyn std::error::Error>> {
        let result = RunFileResult {
            path: "base/if.t".to_string(),
            status: RunnerStatus::Pass,
            assertions_passed: 2,
            assertions_total: 2,
            mechanism: Some(ExecutionMechanism::FixtureReplay),
        };

        let decoded: RunFileResult = serde_json::from_str(&serde_json::to_string(&result)?)?;
        assert_eq!(decoded, result);

        // Baselines written before the field decode unchanged.
        let legacy: RunFileResult = serde_json::from_str(
            r#"{"path":"base/if.t","status":"pass","assertions_passed":2,"assertions_total":2}"#,
        )?;
        assert_eq!(legacy.mechanism, None);
        Ok(())
    }
}
