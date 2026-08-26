//! Exact supervised instrumented-runner capture route (#12285).
//!
//! This module takes one prepared `t/TEST` subject admitted by the
//! observed-discovery route (#12283), constructs a disposable copy, applies a
//! reviewed exact-anchor patch at the upstream child-invocation decision seam,
//! executes the instrumented runner under the bounded capture supervisor with
//! an isolated private trace channel, and assembles the instrumented parent
//! discovery receipt plus the strict #12284 trace receipt through the landed
//! constructors. It never patches the pinned source in place, never expands
//! target selectors itself, and never synthesizes an observed field.
//!
//! Channel law: the instrumented runner owns the row and terminal frames of
//! the trace stream; the capture route owns the header frame, whose every
//! field is route-owned channel identity (schema, session, parent process
//! nonce, parent receipt digest, expected row count, encoding, newline
//! policy). The terminal integrity digest binds the instrument's row bytes
//! only, so route-minted framing never rewrites an observation.
//!
//! The pinned real-tree observation remains explicitly unproven until a real
//! prepared tree is captured: where exact upstream preparation is unavailable,
//! the process fixture stays hermetic and no trace row is fabricated.

use crate::artifacts::{
    CaptureLimits, Options, parse_deadline_with_default, reject_output_aliases,
    reject_subject_destinations, run_bounded_command_with_limit, sanitize_perl_env, write_json,
};
use crate::build::{find_target, sha256_bytes};
use crate::invocation_trace::build::{
    build_invocation_trace_receipt, enforce_uncontaminated_result_streams,
};
use crate::invocation_trace::model::{
    EffectiveInvocationTraceReceiptV1, MAX_TRACE_STREAM_BYTES, ObservedInvocationTraceInput,
    TraceStreamOutcome, TraceSubjectIdentity, UPSTREAM_INVOCATION_TRACE_SCHEMA_VERSION,
};
use crate::io::read_matrix;
use crate::observed_discovery::build::build_observed_discovery_receipt;
use crate::observed_discovery::capture::{
    OBSERVE_DISCOVERY_DEFAULT_DEADLINE_SECONDS, OBSERVE_DISCOVERY_MAX_DEADLINE_SECONDS,
    completion_from_outcome, resolve_host_interpreter, selector_arguments,
};
use crate::observed_discovery::model::{
    DiscoveryObservationState, DiscoverySubjectIdentity, MAX_RAW_STREAM_BYTES,
    ObservedDiscoveryInput, ProcessCompletion, RunnerArtifactIdentity, UpstreamDiscoveryReceiptV1,
};
use crate::runner_model::RunnerKind;
use color_eyre::eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Versioned identity of the instrumentation work receipt.
pub const INSTRUMENTATION_WORK_SCHEMA_VERSION: &str = "perl_core_harness.instrumentation_work.v1";
/// Versioned identity of the reviewed exact-anchor patch specification.
pub const EXACT_PATCH_SCHEMA_VERSION: &str = "perl_core_harness.exact_runner_patch.v1";
/// Identity of the patch application tool retained by every work receipt.
pub const PATCH_TOOL_IDENTITY: &str = "perl-core-harness/exact-anchor-patch/1";
/// Trace-channel file basename inside the disposable copy's `t` directory.
pub const TRACE_CHANNEL_BASENAME: &str = ".perl-core-harness-trace.jsonl";
/// Environment variable naming the trace channel file (relative to cwd).
pub const TRACE_ENV_FILE: &str = "PERL_CORE_HARNESS_TRACE_FILE";
/// Environment variable carrying the trace session identity.
pub const TRACE_ENV_SESSION: &str = "PERL_CORE_HARNESS_TRACE_SESSION";
/// Environment variable carrying the expected instrumented artifact digest.
pub const TRACE_ENV_ARTIFACT: &str = "PERL_CORE_HARNESS_TRACE_ARTIFACT_SHA256";
/// Environment variable carrying the traced target identity.
pub const TRACE_ENV_TARGET: &str = "PERL_CORE_HARNESS_TRACE_TARGET";
/// Environment variable carrying the instrumentation subject identity.
pub const TRACE_ENV_INSTRUMENTATION: &str = "PERL_CORE_HARNESS_TRACE_INSTRUMENTATION";
/// Capture point identity retained by the work receipt.
pub const INSTRUMENTED_CAPTURE_POINT: &str = "t/TEST runtests invocation decision";
/// Default capture deadline for one instrumented observation run.
pub const OBSERVE_INVOCATIONS_DEFAULT_DEADLINE_SECONDS: u64 =
    OBSERVE_DISCOVERY_DEFAULT_DEADLINE_SECONDS;
/// Maximum capture deadline for one instrumented observation run.
pub const OBSERVE_INVOCATIONS_MAX_DEADLINE_SECONDS: u64 = OBSERVE_DISCOVERY_MAX_DEADLINE_SECONDS;
/// Upper bound on files copied into one disposable prepared copy.
pub const MAX_PREPARED_COPY_FILES: u64 = 100_000;
/// Upper bound on bytes copied into one disposable prepared copy.
pub const MAX_PREPARED_COPY_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Fixed claim boundary retained by every instrumentation work receipt.
pub const INSTRUMENTATION_CLAIM_BOUNDARY: &str = "instrumented upstream trace capture under \
                                                   exact disposable patch subjects; ordinary \
                                                   runner equivalence, executed compiler \
                                                   results, and production execution remain \
                                                   unproven";
/// Mandatory limitation retained by every instrumentation work receipt.
pub const LIMITATION_INSTRUMENTED_NOT_ORDINARY: &str =
    "instrumented_runner_is_not_the_ordinary_runner_subject";
/// Mandatory limitation retained by every instrumentation work receipt.
pub const LIMITATION_TRACE_NOT_EXECUTION: &str =
    "trace_rows_are_observations_of_invocation_decisions_not_executed_results";
/// Mandatory limitation retained by every instrumentation work receipt.
pub const LIMITATION_HEADER_IS_PLAN_FRAMING: &str = "header_frame_is_process_plan_owned_channel_framing_row_and_terminal_frames_are_instrument_bytes";
/// Mandatory limitation retained by every instrumentation work receipt.
pub const LIMITATION_DISPOSABLE_MANIFEST: &str =
    "prepared_tree_manifests_bind_the_disposable_copy_only";

static INSTRUMENT_NONCE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// One reviewed exact-anchor patch operation. The anchor must occur exactly
/// once in the ordinary artifact bytes; anything else is drift or ambiguity,
/// never a fuzzy insertion.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExactPatchOp {
    /// Reviewed label identifying the operation in receipts.
    pub label: String,
    /// Exact anchor bytes that must occur exactly once.
    pub anchor: String,
    /// Replacement bytes substituted for the anchor.
    pub replacement: String,
}

/// Reviewed exact-anchor patch specification for one runner artifact.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExactPatchSpec {
    /// Schema identity; always [`EXACT_PATCH_SCHEMA_VERSION`].
    pub schema_version: String,
    /// Runner route the patch targets (`test`).
    pub runner: String,
    /// Prepared-tree-relative artifact the patch targets (`t/TEST`).
    pub target_artifact: String,
    /// SHA-256 the ordinary artifact must measure before patching.
    pub expected_ordinary_sha256: String,
    /// Operations applied in declaration order.
    pub operations: Vec<ExactPatchOp>,
}

/// Typed refusal of one patch application. Fuzzy application, foreign
/// subjects, and drifted sources never fall back to guessing an insertion
/// point.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "detail")]
pub enum PatchApplicationError {
    /// The measured ordinary artifact does not match the pinned subject.
    SubjectDrift {
        /// Digest the patch pinned.
        expected: String,
        /// Digest measured from the prepared tree.
        measured: String,
    },
    /// The anchor does not occur in the artifact.
    AnchorMissing {
        /// Reviewed label of the operation.
        label: String,
    },
    /// The anchor occurs more than once; insertion would be ambiguous.
    AnchorAmbiguous {
        /// Reviewed label of the operation.
        label: String,
        /// Occurrences measured in the artifact.
        occurrences: usize,
    },
}

impl PatchApplicationError {
    /// Stable refusal message retained by callers.
    pub fn message(&self) -> String {
        match self {
            Self::SubjectDrift { expected, measured } => format!(
                "prepared t/TEST measures {measured} but the patch pins ordinary subject {expected}; \
                 source drift is rejected, never guessed into an insertion point"
            ),
            Self::AnchorMissing { label } => format!(
                "patch operation {label} anchors on bytes absent from the pinned ordinary artifact"
            ),
            Self::AnchorAmbiguous { label, occurrences } => format!(
                "patch operation {label} anchors {occurrences} times; exact-match patching refuses \
                 the ambiguous insertion"
            ),
        }
    }
}

/// Apply one reviewed exact-anchor patch to the ordinary artifact bytes.
///
/// Every anchor must match exactly once; the patch is refused otherwise. The
/// result is a deterministic function of the ordinary bytes and the spec.
pub fn apply_exact_patch(
    ordinary: &[u8],
    spec: &ExactPatchSpec,
) -> Result<Vec<u8>, PatchApplicationError> {
    let measured = sha256_bytes(ordinary);
    if measured != spec.expected_ordinary_sha256 {
        return Err(PatchApplicationError::SubjectDrift {
            expected: spec.expected_ordinary_sha256.clone(),
            measured,
        });
    }
    let mut current = ordinary.to_vec();
    for operation in &spec.operations {
        let anchor = operation.anchor.as_bytes();
        let occurrences =
            current.windows(anchor.len().max(1)).filter(|window| *window == anchor).count();
        if occurrences == 0 {
            return Err(PatchApplicationError::AnchorMissing { label: operation.label.clone() });
        }
        if occurrences > 1 {
            return Err(PatchApplicationError::AnchorAmbiguous {
                label: operation.label.clone(),
                occurrences,
            });
        }
        let index =
            current.windows(anchor.len().max(1)).position(|window| window == anchor).unwrap_or(0);
        let mut patched = Vec::with_capacity(current.len() + operation.replacement.len());
        patched.extend_from_slice(&current[..index]);
        patched.extend_from_slice(operation.replacement.as_bytes());
        patched.extend_from_slice(&current[index + anchor.len()..]);
        current = patched;
    }
    Ok(current)
}

/// Proven work accounting of one instrumented capture. The structural zeroes
/// are re-proven during validation; unknown work is never numeric zero.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstrumentationWork {
    /// Instrumented processes executed (always one per capture).
    pub instrumented_processes: u64,
    /// Patch operations applied with exact matches.
    pub patch_operations_applied: u64,
    /// Exact anchor matches measured across operations.
    pub exact_anchor_matches: u64,
    /// Files in the disposable-copy manifest before patching.
    pub manifest_files_before: u64,
    /// Files in the disposable-copy manifest after patching.
    pub manifest_files_after: u64,
    /// Manifest files whose digest changed (only the runner artifact may).
    pub manifest_files_changed: u64,
    /// Retained trace bytes (after the retention bound).
    pub trace_bytes: u64,
    /// Frames consumed by the strict trace decoder.
    pub trace_frames: u64,
    /// Row frames consumed by the strict trace decoder.
    pub trace_rows: u64,
    /// Rows deriving `observed_complete`.
    pub complete_rows: u64,
    /// Rows deriving `observed_partial`.
    pub partial_rows: u64,
    /// Rows with malformed frames.
    pub malformed_rows: u64,
    /// Rows with conflicting framing.
    pub conflicting_rows: u64,
    /// Rows deriving `runner_failed`.
    pub runner_failed_rows: u64,
    /// Rows deriving `instrument_failed`.
    pub instrument_failed_rows: u64,
    /// Rows deriving `subject_mismatch`.
    pub subject_mismatch_rows: u64,
    /// Rows deriving `not_proven`.
    pub not_proven_rows: u64,
    /// Canonical plan projections attempted by construction.
    pub canonical_plan_projections: u64,
    /// Canonical plan projections accepted by construction.
    pub canonical_plan_projections_accepted: u64,
    /// Trace-frame bytes found in ordinary result streams.
    pub ordinary_output_contamination_count: u64,
    /// Fields synthesized from source, profile, or expected plans.
    pub fields_synthesized: u64,
    /// Direct-probe rows consumed as observation evidence.
    pub direct_rows_consumed: u64,
    /// Disagreements between supervisor and trace terminal completions.
    pub terminal_disagreements: u64,
    /// Cleanup failures detected after receipt assembly.
    pub cleanup_failures: u64,
}

/// Typed cleanup evidence for the disposable copy and trace channel.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CleanupRecord {
    /// The disposable prepared copy was removed.
    pub instrumented_tree_removed: bool,
    /// The private trace channel file was removed.
    pub trace_file_removed: bool,
    /// The run directory holding both was removed.
    pub run_directory_removed: bool,
    /// Typed cleanup failures; any entry fails the observation.
    pub failures: Vec<String>,
}

/// Reviewed patch application facts retained by the work receipt.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatchRecord {
    /// SHA-256 of the exact patch specification bytes.
    pub spec_sha256: String,
    /// Identity of the patch application tool.
    pub tool_identity: String,
    /// Capture point identity the patch instruments.
    pub capture_point: String,
    /// Trace schema identity the channel emits.
    pub trace_schema: String,
    /// Labels of the operations applied, in order.
    pub operations_applied: Vec<String>,
    /// Exact anchor matches measured across operations.
    pub exact_anchor_matches: u64,
}

/// Terminal observation vocabulary of one instrumented capture. Only
/// `observed_complete` is a complete observation; every other state is a
/// typed, retained failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstrumentationState {
    /// Parent discovery complete, trace stream strictly decoded, every row
    /// complete, no contamination, no terminal disagreement, cleanup proven.
    ObservedComplete,
    /// The stream decoded but at least one row is not `observed_complete`.
    TracePartial,
    /// The instrumented runner process failed (nonzero exit or signal).
    RunnerFailed,
    /// The trace stream did not decode strictly.
    TraceMalformed,
    /// The instrument or capture failed around the runner.
    InstrumentFailed,
    /// Ordinary result streams carried trace-frame bytes.
    ContaminatedParent,
    /// The instrumented parent discovery receipt could not be constructed.
    ParentConstructionFailed,
    /// The instrument's terminal frame disagrees with the supervisor.
    TerminalDisagreement,
    /// Cleanup left the disposable copy, trace file, or run directory behind.
    CleanupFailed,
    /// The instrumented parent discovery receipt is not `observed_complete`
    /// (truncated output, partial membership, malformed stream, subject
    /// mismatch): the trace alone can never complete the observation.
    ParentIncomplete,
    /// Missing terminal evidence; nothing else can be claimed.
    NotProven,
}

impl InstrumentationState {
    /// Only `observed_complete` proves a complete instrumented observation.
    pub fn is_complete(self) -> bool {
        matches!(self, Self::ObservedComplete)
    }
}

/// Full evidence payload bound by [`InstrumentationWorkReceiptV1::payload_digest`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstrumentationWorkPayload {
    /// Instrumentation subject identity of this capture.
    pub instrumentation_id: String,
    /// Admitted upstream runner route of the instrumented process.
    pub runner: RunnerKind,
    /// Target identity the capture ran for.
    pub target_id: String,
    /// Exact ordinary runner artifact identity measured before patching.
    pub ordinary_artifact: RunnerArtifactIdentity,
    /// Exact instrumented runner artifact identity after patching.
    pub instrumented_artifact: RunnerArtifactIdentity,
    /// Reviewed patch application facts.
    pub patch: PatchRecord,
    /// Disposable-copy manifest before patching (path to SHA-256).
    pub manifest_before: BTreeMap<String, String>,
    /// Disposable-copy manifest after patching (path to SHA-256).
    pub manifest_after: BTreeMap<String, String>,
    /// Trace session identity shared by the stream frames.
    pub trace_session_id: String,
    /// Capture identity of the instrumented process.
    pub process_nonce: String,
    /// Supervisor-side terminal completion of the instrumented process.
    pub supervisor_completion: ProcessCompletion,
    /// Instrument-side terminal completion from the trace terminal frame.
    pub trace_terminal_completion: Option<ProcessCompletion>,
    /// Derived state of the instrumented parent discovery receipt.
    pub parent_state: Option<DiscoveryObservationState>,
    /// Typed reason the parent discovery receipt could not be constructed.
    pub parent_construction_error: Option<String>,
    /// Typed reason the trace receipt could not be constructed.
    pub trace_construction_error: Option<String>,
    /// Digest of the instrumented parent discovery receipt.
    pub parent_receipt_digest: Option<String>,
    /// Digest of the effective-invocation trace receipt.
    pub trace_receipt_digest: Option<String>,
    /// Strict decode outcome of the trace stream.
    pub trace_decode: Option<TraceStreamOutcome>,
    /// Proven work accounting.
    pub work: InstrumentationWork,
    /// Typed cleanup evidence.
    pub cleanup: CleanupRecord,
    /// Derived terminal state of the whole instrumented capture.
    pub state: InstrumentationState,
    /// Mandatory limitations retained verbatim.
    pub limitations: Vec<String>,
    /// Fixed claim boundary retained verbatim.
    pub claim_boundary: String,
}

/// Immutable versioned instrumentation work receipt.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstrumentationWorkReceiptV1 {
    /// Schema identity; always [`INSTRUMENTATION_WORK_SCHEMA_VERSION`].
    pub schema_version: String,
    /// SHA-256 over the canonical serialization of `payload`.
    pub payload_digest: String,
    /// Full evidence payload.
    pub payload: InstrumentationWorkPayload,
}

/// Exact configuration for one instrumented observation capture.
#[derive(Debug, Clone)]
pub struct ObserveInvocationsConfig {
    /// Pinned target matrix (file or bundle directory).
    pub matrix: PathBuf,
    /// Exact target id from the matrix.
    pub target_id: String,
    /// Admitted upstream runner; only `test` is current.
    pub runner: RunnerKind,
    /// Prepared Perl tree containing the exact ordinary `t/TEST` artifact.
    pub perl_tree: PathBuf,
    /// Host Perl interpreter used to execute the instrumented runner.
    pub host_perl: PathBuf,
    /// Measuring repository commit (lower-case hex, 40-64 characters).
    pub repository_commit: String,
    /// Resolved upstream Perl source reference.
    pub perl_ref: String,
    /// Caller-supplied prepared-tree identity reference.
    pub prepared_tree_identity: String,
    /// Caller-supplied host Perl identity reference.
    pub host_perl_identity: String,
    /// Instrumentation subject identity for this capture.
    pub instrumentation_id: String,
    /// Reviewed exact-anchor patch specification file.
    pub patch: PathBuf,
    /// Instrumented parent discovery receipt output path.
    pub output: PathBuf,
    /// Effective-invocation trace receipt output path.
    pub trace_output: PathBuf,
    /// Instrumentation work receipt output path.
    pub work_output: PathBuf,
    /// Finite capture bounds (deadline, cancellation).
    pub limits: CaptureLimits,
}

/// The three receipts of one instrumented capture. The parent and trace
/// receipts are absent exactly when their strict construction failed; the
/// work receipt always retains the typed failure.
pub struct InstrumentedObservation {
    /// Instrumented parent discovery receipt, when constructible.
    pub parent: Option<UpstreamDiscoveryReceiptV1>,
    /// Effective-invocation trace receipt, when constructible.
    pub trace: Option<EffectiveInvocationTraceReceiptV1>,
    /// Instrumentation work receipt, always present.
    pub work: InstrumentationWorkReceiptV1,
}

/// Route-owned header frame for the composed trace stream. Field spellings
/// mirror the strict decoder's header vocabulary exactly; the composed stream
/// must decode through the landed #12284 decoder.
#[derive(serde::Serialize)]
struct PlanHeaderFrame<'a> {
    frame: &'static str,
    schema_version: &'a str,
    trace_session_id: &'a str,
    parent_process_nonce: &'a str,
    parent_receipt_digest: &'a str,
    expected_row_count: u32,
    encoding: &'static str,
    newline: &'static str,
}

/// Prescan of the instrument-owned stream bytes: row-line count and the
/// terminal frame's declared row count. Reading the instrument's own
/// declaration never rewrites an observation.
struct InstrumentPrescan {
    row_lines: u32,
    declared_row_count: Option<u32>,
}

fn prescan_instrument_stream(bytes: &[u8]) -> InstrumentPrescan {
    let mut row_lines = 0u32;
    let mut declared = None;
    let text = String::from_utf8_lossy(bytes);
    for line in text.split('\n') {
        if line.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        match value.get("frame").and_then(|tag| tag.as_str()) {
            Some("row") => row_lines += 1,
            Some("terminal") => {
                declared = value
                    .get("row_count")
                    .and_then(|count| count.as_u64())
                    .map(|count| u32::try_from(count).unwrap_or(u32::MAX));
            }
            _ => {}
        }
    }
    InstrumentPrescan { row_lines, declared_row_count: declared }
}

/// Mint one capture identity unique to this instrumented observation. The
/// counter is fixed-width so equivalent captures mint identities of one
/// stable shape (deterministic receipts up to the masked identity values).
fn mint_instrument_nonce() -> Result<String> {
    let since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("reading the system clock for the capture identity")?;
    let counter = INSTRUMENT_NONCE_COUNTER.fetch_add(1, Ordering::Relaxed);
    Ok(format!(
        "observe-invocations-{}-{}-{counter:06}",
        since_epoch.as_millis(),
        std::process::id()
    ))
}

/// Recursive bounded copy producing a path-to-digest manifest. The manifest
/// covers files only; empty directories are not receipt identity.
fn copy_prepared_tree(source: &Path, destination: &Path) -> Result<BTreeMap<String, String>> {
    let mut manifest = BTreeMap::new();
    let mut file_count = 0u64;
    let mut byte_count = 0u64;
    let mut stack = vec![(source.to_path_buf(), String::new())];
    while let Some((directory, prefix)) = stack.pop() {
        let entries =
            fs::read_dir(&directory).with_context(|| format!("reading {}", directory.display()))?;
        for entry in entries {
            let entry = entry.with_context(|| format!("reading {}", directory.display()))?;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().replace('\\', "/");
            let relative =
                if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                stack.push((path, relative));
            } else if file_type.is_file() {
                file_count += 1;
                if file_count > MAX_PREPARED_COPY_FILES {
                    bail!(
                        "prepared tree exceeds the {MAX_PREPARED_COPY_FILES}-file disposable-copy \
                         bound"
                    );
                }
                let bytes =
                    fs::read(&path).with_context(|| format!("copying {}", path.display()))?;
                byte_count += bytes.len() as u64;
                if byte_count > MAX_PREPARED_COPY_BYTES {
                    bail!(
                        "prepared tree exceeds the {MAX_PREPARED_COPY_BYTES}-byte disposable-copy \
                         bound"
                    );
                }
                let target = destination.join(&relative);
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)
                        .with_context(|| format!("creating {}", parent.display()))?;
                }
                fs::write(&target, &bytes)
                    .with_context(|| format!("writing {}", target.display()))?;
                manifest.insert(relative, sha256_bytes(&bytes));
            } else {
                bail!("prepared tree member {relative} is neither file nor directory");
            }
        }
    }
    Ok(manifest)
}

/// Derive the manifest delta (changed digests) between two manifests.
fn manifest_changes(
    before: &BTreeMap<String, String>,
    after: &BTreeMap<String, String>,
) -> Vec<String> {
    let mut changed = Vec::new();
    for (path, digest) in after {
        match before.get(path) {
            Some(previous) if previous == digest => {}
            Some(_) => changed.push(path.clone()),
            None => changed.push(path.clone()),
        }
    }
    for path in before.keys() {
        if !after.contains_key(path) {
            changed.push(path.clone());
        }
    }
    changed
}

/// Run one exact instrumented observation and assemble the strict receipts.
///
/// The ordinary prepared tree is never modified: the patch applies to a
/// disposable copy, the trace channel is a private file inside that copy, and
/// cleanup removes both after the evidence is retained.
pub fn observe_invocations(config: &ObserveInvocationsConfig) -> Result<InstrumentedObservation> {
    if config.runner != RunnerKind::Test {
        bail!(
            "runner {:?} is not an admitted instrumentation route; the exact t/harness lane is \
             separate",
            config.runner
        );
    }
    validate_commit_shape(&config.repository_commit)?;
    validate_instrumentation_id(&config.instrumentation_id)?;
    let matrix = read_matrix(&config.matrix)?;
    let entry =
        find_target(&matrix, &config.target_id).map_err(|error| color_eyre::eyre::eyre!(error))?;
    let perl_tree = fs::canonicalize(&config.perl_tree).with_context(|| {
        format!("canonicalizing prepared Perl tree {}", config.perl_tree.display())
    })?;
    if !perl_tree.is_dir() {
        bail!("prepared Perl tree is not a directory: {}", perl_tree.display());
    }
    let host_perl = resolve_host_interpreter(&config.host_perl)?;
    let receipt_destinations =
        [config.output.clone(), config.trace_output.clone(), config.work_output.clone()];
    reject_output_aliases(
        &[
            perl_tree.join("t").join("TEST"),
            host_perl.clone(),
            config.matrix.clone(),
            config.patch.clone(),
        ],
        &receipt_destinations,
    )?;
    reject_subject_destinations(&host_perl, &perl_tree, &receipt_destinations)?;
    for destination in &receipt_destinations {
        crate::observed_discovery::capture::reject_matrix_output_alias(
            &config.matrix,
            destination,
        )?;
    }

    // Reviewed patch subject: the spec must pin the exact ordinary artifact.
    let patch_bytes = fs::read(&config.patch)
        .with_context(|| format!("reading patch specification {}", config.patch.display()))?;
    let spec: ExactPatchSpec = serde_json::from_slice(&patch_bytes)
        .with_context(|| format!("decoding patch specification {}", config.patch.display()))?;
    if spec.schema_version != EXACT_PATCH_SCHEMA_VERSION {
        bail!(
            "patch specification carries unknown schema {}; expected {EXACT_PATCH_SCHEMA_VERSION}",
            spec.schema_version
        );
    }
    if spec.runner != "test" || spec.target_artifact != "t/TEST" {
        bail!(
            "patch specification targets {} on {}, outside the exact t/TEST instrumentation route",
            spec.runner,
            spec.target_artifact
        );
    }
    let ordinary_path = perl_tree.join("t").join("TEST");
    let ordinary_bytes = fs::read(&ordinary_path)
        .with_context(|| format!("reading ordinary runner artifact {}", ordinary_path.display()))?;
    let ordinary_digest = sha256_bytes(&ordinary_bytes);
    let instrumented_bytes = apply_exact_patch(&ordinary_bytes, &spec)
        .map_err(|error| color_eyre::eyre::eyre!(error.message()))?;
    let instrumented_digest = sha256_bytes(&instrumented_bytes);

    // Disposable prepared copy: bounded copy, patch, and manifest delta.
    let run_directory = tempfile::tempdir().context("creating the instrumented-run directory")?;
    let instrumented_tree = run_directory.path().join("instrumented-tree");
    let manifest_before = copy_prepared_tree(&perl_tree, &instrumented_tree)?;
    let instrumented_artifact_path = instrumented_tree.join("t").join("TEST");
    fs::write(&instrumented_artifact_path, &instrumented_bytes).with_context(|| {
        format!("writing instrumented artifact {}", instrumented_artifact_path.display())
    })?;
    let mut manifest_after = manifest_before.clone();
    manifest_after.insert("t/TEST".to_string(), instrumented_digest.clone());
    let changed = manifest_changes(&manifest_before, &manifest_after);
    if changed != ["t/TEST"] {
        bail!("instrumentation may only change the runner artifact; manifest delta: {changed:?}");
    }

    // Selector argv stays a pure function of target authority: the traced
    // upstream scan keeps the verbatim selector spelling.
    let selectors =
        selector_arguments(&matrix, entry).map_err(|error| color_eyre::eyre::eyre!(error))?;
    let mut argv = vec!["TEST".to_string(), "--dumptests".to_string()];
    argv.extend(selectors);
    let process_nonce = mint_instrument_nonce()?;
    let trace_session_id = format!("trace-{process_nonce}");
    let mut environment = BTreeMap::from([
        ("LC_ALL".to_string(), "C".to_string()),
        (TRACE_ENV_FILE.to_string(), TRACE_CHANNEL_BASENAME.to_string()),
        (TRACE_ENV_SESSION.to_string(), trace_session_id.clone()),
        (TRACE_ENV_ARTIFACT.to_string(), instrumented_digest.clone()),
        (TRACE_ENV_TARGET.to_string(), config.target_id.clone()),
        (TRACE_ENV_INSTRUMENTATION.to_string(), config.instrumentation_id.clone()),
    ]);
    for (key, value) in &entry.contract.environment {
        environment.insert(key.clone(), value.clone());
    }
    let instrumented_t_dir = instrumented_tree.join("t");
    let mut command = Command::new(&host_perl);
    command.current_dir(&instrumented_t_dir);
    command.args(&argv);
    for (key, value) in &environment {
        command.env(key, value);
    }
    sanitize_perl_env(&mut command);
    let (outcome, stdout, stderr) =
        run_bounded_command_with_limit(command, &config.limits, MAX_RAW_STREAM_BYTES);
    let stdout_bytes = stdout.retained_bytes().unwrap_or_default();
    let stderr_bytes = stderr.retained_bytes().unwrap_or_default();
    let stdout_truncated = stdout.was_truncated();
    let stderr_truncated = stderr.was_truncated();
    let instrument_channel_failed =
        stdout.capture_failure().is_some() || stderr.capture_failure().is_some();
    let supervisor_completion = if instrument_channel_failed {
        ProcessCompletion::InstrumentFailed
    } else {
        completion_from_outcome(&outcome)
    };

    let matrix_fingerprint =
        matrix.fingerprint().map_err(|error| color_eyre::eyre::eyre!(error))?;
    let target_contract_digest = crate::observed_discovery::build::sha256_json(&entry.contract)
        .map_err(|error| color_eyre::eyre::eyre!(error))?;

    // Instrumented parent discovery receipt: same strict #12281 construction,
    // instrumented subject and artifact.
    let parent_input = ObservedDiscoveryInput {
        subject: DiscoverySubjectIdentity {
            repository_commit: config.repository_commit.clone(),
            perl_ref: config.perl_ref.clone(),
            prepared_tree_identity: config.prepared_tree_identity.clone(),
            host_perl_identity: config.host_perl_identity.clone(),
            matrix_fingerprint: matrix_fingerprint.clone(),
            target_id: config.target_id.clone(),
            target_contract_digest: target_contract_digest.clone(),
            variant_target_id: None,
            instrumentation_id: Some(config.instrumentation_id.clone()),
        },
        runner: RunnerKind::Test,
        runner_artifact: RunnerArtifactIdentity {
            canonical_path: "t/TEST".to_string(),
            content_sha256: instrumented_digest.clone(),
        },
        argv: argv.clone(),
        working_directory: "t".to_string(),
        environment: environment.clone(),
        discovery_frame: crate::runner_model::DiscoveryFrame::CanonicalRepositoryPath,
        completion: supervisor_completion,
        process_nonce: process_nonce.clone(),
        stdout_bytes: stdout_bytes.clone(),
        stdout_truncated,
        stderr_bytes: stderr_bytes.clone(),
        stderr_truncated,
    };

    // Trace channel: private file inside the disposable copy.
    let trace_path = instrumented_t_dir.join(TRACE_CHANNEL_BASENAME);
    let mut trace_bytes = fs::read(&trace_path).unwrap_or_default();
    let mut trace_truncated = false;
    if trace_bytes.len() > MAX_TRACE_STREAM_BYTES {
        trace_bytes.truncate(MAX_TRACE_STREAM_BYTES);
        trace_truncated = true;
    }

    let mut parent_opt = None;
    let mut trace_opt = None;
    let mut parent_construction_error: Option<String> = None;
    let mut contamination = false;
    let mut trace_construction_error: Option<String> = None;

    match build_observed_discovery_receipt(&matrix, &parent_input) {
        Ok(receipt) => {
            // Contamination of ordinary result streams voids the transport
            // contract before any trace receipt exists; the contaminated
            // parent stays retained as evidence.
            contamination = enforce_uncontaminated_result_streams(&receipt).is_err();
            parent_opt = Some(receipt);
        }
        Err(error) => {
            parent_construction_error = Some(error);
        }
    }

    if let Some(parent_receipt) = parent_opt.as_ref().filter(|_| !contamination) {
        let prescan = prescan_instrument_stream(&trace_bytes);
        let expected_row_count = prescan.declared_row_count.unwrap_or(prescan.row_lines);
        let header = PlanHeaderFrame {
            frame: "header",
            schema_version: UPSTREAM_INVOCATION_TRACE_SCHEMA_VERSION,
            trace_session_id: &trace_session_id,
            parent_process_nonce: &process_nonce,
            parent_receipt_digest: &parent_receipt.payload_digest,
            expected_row_count,
            encoding: "utf-8",
            newline: "lf",
        };
        let mut stream =
            serde_json::to_vec(&header).context("serializing the trace channel header frame")?;
        stream.push(b'\n');
        stream.extend_from_slice(&trace_bytes);
        let subject = TraceSubjectIdentity {
            repository_commit: config.repository_commit.clone(),
            perl_ref: config.perl_ref.clone(),
            prepared_tree_identity: config.prepared_tree_identity.clone(),
            host_perl_identity: config.host_perl_identity.clone(),
            matrix_fingerprint,
            target_id: config.target_id.clone(),
            target_contract_digest,
            variant_target_id: None,
            instrumentation_id: Some(config.instrumentation_id.clone()),
            trace_session_id: trace_session_id.clone(),
            parent_process_nonce: process_nonce.clone(),
            parent_receipt_digest: parent_receipt.payload_digest.clone(),
        };
        let input = ObservedInvocationTraceInput {
            subject,
            runner: RunnerKind::Test,
            runner_artifact: RunnerArtifactIdentity {
                canonical_path: "t/TEST".to_string(),
                content_sha256: instrumented_digest.clone(),
            },
            parent_receipt: parent_receipt.clone(),
            trace_bytes: stream,
            trace_truncated,
        };
        match build_invocation_trace_receipt(&input) {
            Ok(receipt) => trace_opt = Some(receipt),
            Err(error) => trace_construction_error = Some(error),
        }
    }

    // Terminal cross-check: the instrument's self-observation must agree with
    // the supervisor's terminal evidence for the capture to be complete.
    let trace_terminal_completion = trace_opt
        .as_ref()
        .and_then(|receipt| receipt.payload.terminal.as_ref().map(|terminal| terminal.completion));
    let terminal_disagreements = match (trace_terminal_completion, parent_opt.as_ref()) {
        (Some(trace_completion), Some(parent_receipt)) => {
            u64::from(trace_completion != parent_receipt.payload.terminal.completion)
        }
        _ => 0,
    };

    // Cleanup: remove the private trace file and the disposable copy, then
    // prove both are gone before the run directory drops.
    let run_root = run_directory.path().to_path_buf();
    let mut cleanup = CleanupRecord::default();
    if trace_path.exists()
        && let Err(error) = fs::remove_file(&trace_path)
    {
        cleanup.failures.push(format!("removing trace channel: {error}"));
    }
    // A trace file that never existed is proven absence, not leftover state.
    cleanup.trace_file_removed = !trace_path.exists();
    if let Err(error) = fs::remove_dir_all(&instrumented_tree) {
        cleanup.failures.push(format!("removing instrumented tree: {error}"));
    }
    cleanup.instrumented_tree_removed = !instrumented_tree.exists();
    drop(run_directory);
    cleanup.run_directory_removed = cleanup.failures.is_empty() && !run_root.exists();

    let mut work = InstrumentationWork {
        instrumented_processes: 1,
        patch_operations_applied: spec.operations.len() as u64,
        exact_anchor_matches: spec.operations.len() as u64,
        manifest_files_before: manifest_before.len() as u64,
        manifest_files_after: manifest_after.len() as u64,
        manifest_files_changed: changed.len() as u64,
        trace_bytes: trace_bytes.len() as u64,
        terminal_disagreements,
        cleanup_failures: cleanup.failures.len() as u64,
        ..InstrumentationWork::default()
    };
    let mut parent_state = None;
    let mut trace_decode = None;
    if let Some(parent_receipt) = &parent_opt {
        parent_state = Some(parent_receipt.payload.state);
    }
    if let Some(trace_receipt) = &trace_opt {
        trace_decode = Some(trace_receipt.payload.trace_decode.clone());
        let trace_work = &trace_receipt.payload.work;
        work.trace_frames = trace_work.trace_frames_consumed;
        work.trace_rows = trace_work.trace_rows_consumed;
        work.complete_rows = trace_work.complete_rows;
        work.partial_rows = trace_work.partial_rows;
        work.malformed_rows = trace_work.malformed_rows;
        work.conflicting_rows = trace_work.conflicting_rows;
        work.runner_failed_rows = trace_work.runner_failed_rows;
        work.instrument_failed_rows = trace_work.instrument_failed_rows;
        work.subject_mismatch_rows = trace_work.subject_mismatch_rows;
        work.not_proven_rows = trace_work.not_proven_rows;
        work.canonical_plan_projections = trace_work.canonical_plan_projections_attempted;
        work.canonical_plan_projections_accepted = trace_work.canonical_plan_projections_accepted;
    }
    if contamination {
        work.ordinary_output_contamination_count = 1;
    }
    let state = derive_instrumentation_state(
        &cleanup,
        parent_opt.is_none(),
        contamination,
        trace_opt.is_none(),
        trace_construction_error.is_some(),
        terminal_disagreements,
        &trace_decode,
        supervisor_completion,
        parent_state,
        &work,
    );

    let payload = InstrumentationWorkPayload {
        instrumentation_id: config.instrumentation_id.clone(),
        runner: RunnerKind::Test,
        target_id: config.target_id.clone(),
        ordinary_artifact: RunnerArtifactIdentity {
            canonical_path: "t/TEST".to_string(),
            content_sha256: ordinary_digest,
        },
        instrumented_artifact: RunnerArtifactIdentity {
            canonical_path: "t/TEST".to_string(),
            content_sha256: instrumented_digest,
        },
        patch: PatchRecord {
            spec_sha256: sha256_bytes(&patch_bytes),
            tool_identity: PATCH_TOOL_IDENTITY.to_string(),
            capture_point: INSTRUMENTED_CAPTURE_POINT.to_string(),
            trace_schema: UPSTREAM_INVOCATION_TRACE_SCHEMA_VERSION.to_string(),
            operations_applied: spec.operations.iter().map(|op| op.label.clone()).collect(),
            exact_anchor_matches: spec.operations.len() as u64,
        },
        manifest_before,
        manifest_after,
        trace_session_id,
        process_nonce,
        supervisor_completion,
        trace_terminal_completion,
        parent_state,
        parent_construction_error,
        trace_construction_error,
        parent_receipt_digest: parent_opt.as_ref().map(|receipt| receipt.payload_digest.clone()),
        trace_receipt_digest: trace_opt.as_ref().map(|receipt| receipt.payload_digest.clone()),
        trace_decode,
        work,
        cleanup,
        state,
        limitations: required_limitations(),
        claim_boundary: INSTRUMENTATION_CLAIM_BOUNDARY.to_string(),
    };
    let payload_digest =
        instrumentation_payload_digest(&payload).map_err(|error| color_eyre::eyre::eyre!(error))?;
    Ok(InstrumentedObservation {
        parent: parent_opt,
        trace: trace_opt,
        work: InstrumentationWorkReceiptV1 {
            schema_version: INSTRUMENTATION_WORK_SCHEMA_VERSION.to_string(),
            payload_digest,
            payload,
        },
    })
}

impl CleanupRecord {
    /// Cleanup proof: every disposable artifact gone and no typed failure.
    pub fn is_proven(&self) -> bool {
        self.instrumented_tree_removed
            && self.trace_file_removed
            && self.run_directory_removed
            && self.failures.is_empty()
    }
}

/// Derive the single terminal state of one instrumented capture.
#[expect(
    clippy::too_many_arguments,
    reason = "the derivation consumes every typed failure surface of the capture"
)]
fn derive_instrumentation_state(
    cleanup: &CleanupRecord,
    parent_absent: bool,
    contamination: bool,
    trace_absent: bool,
    trace_error: bool,
    terminal_disagreements: u64,
    trace_decode: &Option<TraceStreamOutcome>,
    supervisor_completion: ProcessCompletion,
    parent_state: Option<DiscoveryObservationState>,
    work: &InstrumentationWork,
) -> InstrumentationState {
    if !cleanup.failures.is_empty() {
        return InstrumentationState::CleanupFailed;
    }
    if contamination {
        return InstrumentationState::ContaminatedParent;
    }
    if parent_absent {
        return InstrumentationState::ParentConstructionFailed;
    }
    if trace_absent {
        return if trace_error {
            InstrumentationState::InstrumentFailed
        } else {
            InstrumentationState::NotProven
        };
    }
    if terminal_disagreements > 0 {
        return InstrumentationState::TerminalDisagreement;
    }
    match supervisor_completion {
        ProcessCompletion::Unknown
        | ProcessCompletion::Cancelled
        | ProcessCompletion::TimedOut { .. } => return InstrumentationState::NotProven,
        _ => {}
    }
    if matches!(supervisor_completion, ProcessCompletion::InstrumentFailed)
        || matches!(parent_state, Some(DiscoveryObservationState::InstrumentFailed))
    {
        return InstrumentationState::InstrumentFailed;
    }
    if matches!(supervisor_completion, ProcessCompletion::ExitStatus { code: 0 })
        && matches!(parent_state, Some(DiscoveryObservationState::RunnerFailed))
    {
        // The parent derived a runner failure the supervisor did not observe
        // directly (for example a contaminated decode); retain the parent's
        // stronger evidence.
        return InstrumentationState::RunnerFailed;
    }
    if matches!(supervisor_completion, ProcessCompletion::ExitStatus { code } if code != 0)
        || matches!(supervisor_completion, ProcessCompletion::Signalled { .. })
    {
        return InstrumentationState::RunnerFailed;
    }
    // The parent discovery receipt is the denominator of the observation: no
    // trace-only completion is possible while the parent is truncated,
    // partial, malformed, or subject-mismatched.
    if !matches!(parent_state, Some(DiscoveryObservationState::ObservedComplete)) {
        return InstrumentationState::ParentIncomplete;
    }
    if trace_decode.as_ref().is_none_or(|outcome| !outcome.is_complete()) {
        return InstrumentationState::TraceMalformed;
    }
    if work.instrument_failed_rows > 0 {
        return InstrumentationState::InstrumentFailed;
    }
    if work.complete_rows != work.trace_rows || work.trace_rows == 0 {
        return InstrumentationState::TracePartial;
    }
    if work.canonical_plan_projections_accepted != work.trace_rows {
        return InstrumentationState::TracePartial;
    }
    InstrumentationState::ObservedComplete
}

/// Deterministic SHA-256 over the canonical serialization of the payload.
pub fn instrumentation_payload_digest(
    payload: &InstrumentationWorkPayload,
) -> Result<String, String> {
    let bytes = serde_json::to_vec(payload)
        .map_err(|error| format!("serializing instrumentation work payload: {error}"))?;
    Ok(sha256_bytes(&bytes))
}

/// Re-check an instrumentation work receipt against the ordinary artifact
/// bytes and the exact patch specification: the patch must re-apply to the
/// same instrumented digest, and the manifest delta must be the runner
/// artifact alone.
pub fn validate_instrumentation_work(
    receipt: &InstrumentationWorkReceiptV1,
    ordinary_bytes: &[u8],
    spec: &ExactPatchSpec,
) -> Result<(), String> {
    if receipt.schema_version != INSTRUMENTATION_WORK_SCHEMA_VERSION {
        return Err(format!("unsupported instrumentation work schema {}", receipt.schema_version));
    }
    let payload = &receipt.payload;
    let recomputed = instrumentation_payload_digest(payload)?;
    if recomputed != receipt.payload_digest {
        return Err("payload digest does not bind the recorded instrumentation work".to_string());
    }
    if payload.limitations != required_limitations() {
        return Err(
            "instrumentation work receipts retain exactly their mandatory limitations".to_string()
        );
    }
    if payload.claim_boundary != INSTRUMENTATION_CLAIM_BOUNDARY {
        return Err("instrumentation work receipts retain their fixed claim boundary".to_string());
    }
    if payload.ordinary_artifact.content_sha256 != sha256_bytes(ordinary_bytes) {
        return Err("ordinary artifact digest does not bind the supplied bytes".to_string());
    }
    let reapplied = apply_exact_patch(ordinary_bytes, spec)
        .map_err(|error| format!("re-applying the reviewed patch failed: {}", error.message()))?;
    if payload.instrumented_artifact.content_sha256 != sha256_bytes(&reapplied) {
        return Err("instrumented artifact digest does not match the re-applied patch".to_string());
    }
    if payload.patch.spec_sha256.len() != 64
        || !payload.patch.spec_sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("patch specification digest must be 64 hexadecimal characters".to_string());
    }
    if payload.patch.tool_identity != PATCH_TOOL_IDENTITY {
        return Err("patch tool identity disagrees with the current patch tool".to_string());
    }
    if payload.patch.capture_point != INSTRUMENTED_CAPTURE_POINT {
        return Err("patch capture point disagrees with the instrumented seam".to_string());
    }
    if payload.patch.trace_schema != UPSTREAM_INVOCATION_TRACE_SCHEMA_VERSION {
        return Err("patch trace schema disagrees with the #12284 contract".to_string());
    }
    let changes = manifest_changes(&payload.manifest_before, &payload.manifest_after);
    if changes != ["t/TEST"] {
        return Err(format!(
            "instrumentation may only change the runner artifact; manifest delta: {changes:?}"
        ));
    }
    // The recorded specification must be the exact supplied specification:
    // another spec with equal patched bytes never validates this receipt.
    let serialized = serde_json::to_vec(spec)
        .map_err(|error| format!("serializing the supplied patch specification: {error}"))?;
    let supplied_digest = sha256_bytes(&serialized);
    if payload.patch.spec_sha256 != supplied_digest {
        return Err(
            "patch specification digest does not bind the supplied specification".to_string()
        );
    }
    let supplied_labels =
        spec.operations.iter().map(|operation| operation.label.clone()).collect::<Vec<_>>();
    if payload.patch.operations_applied != supplied_labels {
        return Err("patch operation labels disagree with the supplied specification".to_string());
    }
    if payload.patch.exact_anchor_matches != spec.operations.len() as u64 {
        return Err(
            "patch anchor-match count disagrees with the supplied specification".to_string()
        );
    }
    if payload.work.fields_synthesized != 0 {
        return Err("instrumented capture synthesizes no fields".to_string());
    }
    if payload.work.direct_rows_consumed != 0 {
        return Err("instrumented capture consumes no direct-probe rows".to_string());
    }
    if !cleanup_is_proven(&payload.cleanup) {
        return Err("instrumented capture cleanup is not proven".to_string());
    }
    Ok(())
}

/// Cleanup proof: every disposable artifact gone and no typed failure.
pub fn cleanup_is_proven(cleanup: &CleanupRecord) -> bool {
    cleanup.is_proven()
}

/// The mandatory limitation set, sorted.
pub(crate) fn required_limitations() -> Vec<String> {
    let mut limitations = vec![
        LIMITATION_INSTRUMENTED_NOT_ORDINARY.to_string(),
        LIMITATION_TRACE_NOT_EXECUTION.to_string(),
        LIMITATION_HEADER_IS_PLAN_FRAMING.to_string(),
        LIMITATION_DISPOSABLE_MANIFEST.to_string(),
    ];
    limitations.sort();
    limitations
}

/// Run one instrumented observation, write every constructible receipt, and
/// validate the written evidence by reconstruction before reporting the
/// terminal disposition. Every non-complete state is a typed failure exit.
pub fn observe_invocations_command(config: &ObserveInvocationsConfig) -> Result<()> {
    let observation = observe_invocations(config)?;
    // Absent receipts must not leave a previous run's successful evidence in
    // place: file-based consumers would ingest stale proof for this run.
    clear_stale_output(&config.output, observation.parent.is_none())?;
    clear_stale_output(&config.trace_output, observation.trace.is_none())?;
    if let Some(parent) = &observation.parent {
        write_json(&config.output, parent)?;
    }
    if let Some(trace) = &observation.trace {
        write_json(&config.trace_output, trace)?;
    }
    write_json(&config.work_output, &observation.work)?;
    let matrix = read_matrix(&config.matrix)?;
    if let Some(parent) = &observation.parent {
        crate::observed_discovery::build::check_observed_discovery_against(&matrix, parent)
            .map_err(|error| {
                color_eyre::eyre::eyre!(
                    "written parent receipt {} does not reconstruct against the pinned matrix: \
                     {error}",
                    config.output.display()
                )
            })?;
    }
    if let (Some(parent), Some(trace)) = (&observation.parent, &observation.trace) {
        crate::invocation_trace::build::check_invocation_trace_against(parent, trace).map_err(
            |error| {
                color_eyre::eyre::eyre!(
                    "written trace receipt {} does not reconstruct against its parent: {error}",
                    config.trace_output.display()
                )
            },
        )?;
    }
    let work = &observation.work.payload;
    tracing::info!(
        target = %config.target_id,
        instrumentation = %config.instrumentation_id,
        state = ?work.state,
        instrumented_processes = work.work.instrumented_processes,
        trace_rows = work.work.trace_rows,
        complete_rows = work.work.complete_rows,
        partial_rows = work.work.partial_rows,
        malformed_rows = work.work.malformed_rows,
        conflicting_rows = work.work.conflicting_rows,
        projections = work.work.canonical_plan_projections,
        projections_accepted = work.work.canonical_plan_projections_accepted,
        contamination = work.work.ordinary_output_contamination_count,
        fields_synthesized = work.work.fields_synthesized,
        direct_rows_consumed = work.work.direct_rows_consumed,
        terminal_disagreements = work.work.terminal_disagreements,
        "instrumented invocation capture"
    );
    if !work.state.is_complete() {
        bail!(
            "instrumented observation state is {:?}, not observed_complete; the typed work \
             receipt is retained at {}",
            work.state,
            config.work_output.display()
        );
    }
    Ok(())
}

/// Parse `perl-core-harness-artifacts observe-invocations` options.
pub(crate) fn observe_invocations_from_options(mut options: Options) -> Result<()> {
    let config = ObserveInvocationsConfig {
        matrix: PathBuf::from(options.required("--matrix")?),
        target_id: options.required("--target")?,
        runner: parse_runner(&options.required("--runner")?)?,
        perl_tree: PathBuf::from(options.required("--perl-tree")?),
        host_perl: PathBuf::from(options.required("--host-perl")?),
        repository_commit: options.required("--commit")?,
        perl_ref: options.required("--perl-ref")?,
        prepared_tree_identity: options.required("--prepared-tree-identity")?,
        host_perl_identity: options.required("--host-perl-identity")?,
        instrumentation_id: options.required("--instrumentation-id")?,
        patch: PathBuf::from(options.required("--patch")?),
        output: PathBuf::from(options.required("--output")?),
        trace_output: PathBuf::from(options.required("--trace-output")?),
        work_output: PathBuf::from(options.required("--work-output")?),
        limits: CaptureLimits {
            deadline: parse_deadline_with_default(
                options.optional("--deadline-seconds")?.as_deref(),
                OBSERVE_INVOCATIONS_DEFAULT_DEADLINE_SECONDS,
                OBSERVE_INVOCATIONS_MAX_DEADLINE_SECONDS,
            )?,
            cancel_file: options.optional("--cancel-file")?.map(PathBuf::from),
        },
    };
    options.finish()?;
    observe_invocations_command(&config)
}

/// Remove one output file when its receipt is absent from the current run so
/// a stale success can never survive a typed failure.
fn clear_stale_output(path: &Path, absent: bool) -> Result<()> {
    if absent && path.exists() {
        fs::remove_file(path)
            .with_context(|| format!("removing stale receipt {}", path.display()))?;
    }
    Ok(())
}

fn parse_runner(value: &str) -> Result<RunnerKind> {
    match RunnerKind::parse(value) {
        Ok(RunnerKind::Test) => Ok(RunnerKind::Test),
        Ok(other) => bail!(
            "runner {other:?} is not an admitted instrumentation route; only --runner test is \
             current"
        ),
        Err(error) => bail!("{error}"),
    }
}

/// Fail fast on a malformed repository commit before spending a supervised
/// run; the receipt constructor re-validates the same law afterwards.
fn validate_commit_shape(commit: &str) -> Result<()> {
    let lowercase_hex =
        commit.bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'));
    if commit.len() < 40 || commit.len() > 64 || !lowercase_hex {
        bail!(
            "--commit must be a 40-64 character lower-case hexadecimal repository commit, found \
             {commit}"
        );
    }
    Ok(())
}

/// Fail fast on a malformed instrumentation subject identity.
fn validate_instrumentation_id(instrumentation_id: &str) -> Result<()> {
    if instrumentation_id.is_empty()
        || instrumentation_id.len() > 128
        || !instrumentation_id.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
    {
        bail!(
            "--instrumentation-id must be 1-128 printable ASCII characters, found \
             {instrumentation_id}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod contract_tests {
    //! Focused unit proof for the pure seams of the instrumentation route:
    //! exact-anchor patching, manifest deltas, stream prescan, and the state
    //! derivation. Process-level behavior is proven by the hermetic
    //! exact-process suite (`tests/observed_invocations_capture.rs`).

    use super::{
        CleanupRecord, ExactPatchOp, ExactPatchSpec, InstrumentationState, InstrumentationWork,
        PatchApplicationError, apply_exact_patch, derive_instrumentation_state, manifest_changes,
        prescan_instrument_stream, required_limitations,
    };
    use crate::invocation_trace::model::{
        TraceStreamOutcome, UPSTREAM_INVOCATION_TRACE_SCHEMA_VERSION,
    };
    use crate::observed_discovery::model::{DiscoveryObservationState, ProcessCompletion};
    use color_eyre::eyre::Result;

    fn spec(anchor: &str, replacement: &str) -> ExactPatchSpec {
        ExactPatchSpec {
            schema_version: super::EXACT_PATCH_SCHEMA_VERSION.to_string(),
            runner: "test".to_string(),
            target_artifact: "t/TEST".to_string(),
            expected_ordinary_sha256: super::sha256_bytes(b"#!./perl\n# ordinary\n"),
            operations: vec![ExactPatchOp {
                label: "seam".to_string(),
                anchor: anchor.to_string(),
                replacement: replacement.to_string(),
            }],
        }
    }

    #[test]
    fn exact_anchors_apply_once_and_reproduce_deterministically() {
        let ordinary = b"#!./perl\n# ordinary\n";
        let spec = spec("# ordinary", "# instrumented\n# capture-point: decision");
        let first = apply_exact_patch(ordinary, &spec).expect("exact anchor applies");
        let second = apply_exact_patch(ordinary, &spec).expect("re-application is deterministic");
        assert_eq!(first, second);
        assert!(first.starts_with(b"#!./perl\n# instrumented"));
    }

    #[test]
    fn subject_drift_is_rejected_never_guessed() {
        let ordinary = b"#!./perl\n# drifted bytes\n";
        let spec = spec("# ordinary", "# instrumented");
        let Err(PatchApplicationError::SubjectDrift { expected, measured }) =
            apply_exact_patch(ordinary, &spec)
        else {
            panic!("drifted ordinary artifact must refuse the patch");
        };
        assert_ne!(expected, measured);
        assert!(
            spec.expected_ordinary_sha256 == expected,
            "the refusal carries the pinned subject"
        );
    }

    #[test]
    fn missing_and_ambiguous_anchors_refuse_exact_application() {
        let ordinary = b"#!./perl\n# ordinary\n";
        let Err(PatchApplicationError::AnchorMissing { label }) =
            apply_exact_patch(ordinary, &spec("# absent", "# x"))
        else {
            panic!("absent anchor must refuse");
        };
        assert_eq!(label, "seam");

        let ambiguous_source = b"#!./perl\n# ordinary\n# ordinary\n";
        let mut ambiguous_spec = spec("# ordinary", "# x");
        ambiguous_spec.expected_ordinary_sha256 = super::sha256_bytes(ambiguous_source);
        let Err(PatchApplicationError::AnchorAmbiguous { occurrences, .. }) =
            apply_exact_patch(ambiguous_source, &ambiguous_spec)
        else {
            panic!("ambiguous anchor must refuse");
        };
        assert_eq!(occurrences, 2);
    }

    #[test]
    fn sequential_operations_apply_in_declaration_order() {
        let ordinary = b"#!./perl\n# upstream\n";
        let spec = ExactPatchSpec {
            schema_version: super::EXACT_PATCH_SCHEMA_VERSION.to_string(),
            runner: "test".to_string(),
            target_artifact: "t/TEST".to_string(),
            expected_ordinary_sha256: super::sha256_bytes(ordinary),
            operations: vec![
                ExactPatchOp {
                    label: "first".to_string(),
                    anchor: "# upstream".to_string(),
                    replacement: "# upstream-instrumented".to_string(),
                },
                ExactPatchOp {
                    label: "second".to_string(),
                    anchor: "# upstream-instrumented".to_string(),
                    replacement: "# upstream-instrumented-twice".to_string(),
                },
            ],
        };
        let patched = apply_exact_patch(ordinary, &spec).expect("sequential anchors apply");
        assert!(patched.ends_with(b"# upstream-instrumented-twice\n"));
    }

    #[test]
    fn manifest_delta_detects_exactly_the_changed_runner_artifact() {
        let before =
            [("t/TEST".to_string(), "a".to_string()), ("t/base/if.t".to_string(), "b".to_string())]
                .into_iter()
                .collect();
        let after =
            [("t/TEST".to_string(), "c".to_string()), ("t/base/if.t".to_string(), "b".to_string())]
                .into_iter()
                .collect();
        assert_eq!(manifest_changes(&before, &after), vec!["t/TEST".to_string()]);
        let touched_member =
            [("t/TEST".to_string(), "a".to_string()), ("t/base/if.t".to_string(), "z".to_string())]
                .into_iter()
                .collect();
        assert_eq!(
            manifest_changes(&before, &touched_member).len(),
            1,
            "any non-artifact change is visible in the delta"
        );
    }

    #[test]
    fn prescan_reads_the_instrument_declaration_without_rewriting_it() {
        let rows = r#"{"frame":"row","sequence":0}
{"frame":"row","sequence":1}
{"frame":"terminal","row_count":2}
"#;
        let prescan = prescan_instrument_stream(rows.as_bytes());
        assert_eq!(prescan.row_lines, 2);
        assert_eq!(prescan.declared_row_count, Some(2));
        // A truncated stream keeps its counted rows and no declaration.
        let truncated = r#"{"frame":"row","sequence":0}
{"frame":"row""#;
        let prescan = prescan_instrument_stream(truncated.as_bytes());
        assert_eq!(prescan.row_lines, 1);
        assert_eq!(prescan.declared_row_count, None);
    }

    #[test]
    fn state_derivation_never_completes_a_failed_capture() {
        let proven_cleanup = CleanupRecord {
            instrumented_tree_removed: true,
            trace_file_removed: true,
            run_directory_removed: true,
            failures: Vec::new(),
        };
        let mut work = InstrumentationWork {
            trace_rows: 2,
            complete_rows: 2,
            canonical_plan_projections: 2,
            canonical_plan_projections_accepted: 2,
            ..InstrumentationWork::default()
        };
        let complete = TraceStreamOutcome::Complete;
        let parent_complete = Some(DiscoveryObservationState::ObservedComplete);

        // The one complete shape.
        assert_eq!(
            derive_instrumentation_state(
                &proven_cleanup,
                false,
                false,
                false,
                false,
                0,
                &Some(complete.clone()),
                ProcessCompletion::ExitStatus { code: 0 },
                parent_complete,
                &work,
            ),
            InstrumentationState::ObservedComplete
        );

        // Cleanup dominates every other outcome.
        let dirty_cleanup =
            CleanupRecord { failures: vec!["leftover".to_string()], ..proven_cleanup.clone() };
        assert_eq!(
            derive_instrumentation_state(
                &dirty_cleanup,
                false,
                false,
                false,
                false,
                0,
                &Some(complete.clone()),
                ProcessCompletion::ExitStatus { code: 0 },
                parent_complete,
                &work,
            ),
            InstrumentationState::CleanupFailed
        );

        // A lying terminal never completes.
        assert_eq!(
            derive_instrumentation_state(
                &proven_cleanup,
                false,
                false,
                false,
                false,
                1,
                &Some(complete.clone()),
                ProcessCompletion::ExitStatus { code: 0 },
                parent_complete,
                &work,
            ),
            InstrumentationState::TerminalDisagreement
        );

        // A partial trace stays partial; nothing upgrades it.
        work.complete_rows = 1;
        assert_eq!(
            derive_instrumentation_state(
                &proven_cleanup,
                false,
                false,
                false,
                false,
                0,
                &Some(complete.clone()),
                ProcessCompletion::ExitStatus { code: 0 },
                parent_complete,
                &work,
            ),
            InstrumentationState::TracePartial
        );
        work.complete_rows = 2;

        // A missing terminal frame types the stream malformed.
        let malformed =
            TraceStreamOutcome::Malformed { reason: "missing terminal frame".to_string() };
        assert_eq!(
            derive_instrumentation_state(
                &proven_cleanup,
                false,
                false,
                false,
                false,
                0,
                &Some(malformed),
                ProcessCompletion::ExitStatus { code: 0 },
                parent_complete,
                &work,
            ),
            InstrumentationState::TraceMalformed
        );

        // Missing terminal evidence is not proven, never complete.
        assert_eq!(
            derive_instrumentation_state(
                &proven_cleanup,
                false,
                false,
                false,
                false,
                0,
                &Some(complete.clone()),
                ProcessCompletion::TimedOut { deadline_millis: 1000 },
                Some(DiscoveryObservationState::TimedOut),
                &work,
            ),
            InstrumentationState::NotProven
        );

        // A failed runner never completes even with complete-looking rows.
        assert_eq!(
            derive_instrumentation_state(
                &proven_cleanup,
                false,
                false,
                false,
                false,
                0,
                &Some(complete.clone()),
                ProcessCompletion::ExitStatus { code: 7 },
                Some(DiscoveryObservationState::RunnerFailed),
                &work,
            ),
            InstrumentationState::RunnerFailed
        );

        // An absent parent is a construction failure; contamination types
        // itself.
        assert_eq!(
            derive_instrumentation_state(
                &proven_cleanup,
                true,
                true,
                true,
                false,
                0,
                &None,
                ProcessCompletion::ExitStatus { code: 0 },
                None,
                &work,
            ),
            InstrumentationState::ContaminatedParent
        );
        assert_eq!(
            derive_instrumentation_state(
                &proven_cleanup,
                true,
                false,
                true,
                false,
                0,
                &None,
                ProcessCompletion::ExitStatus { code: 0 },
                None,
                &work,
            ),
            InstrumentationState::ParentConstructionFailed
        );
    }

    #[test]
    fn mandatory_limitations_are_exactly_the_sorted_required_set() {
        let mut expected = vec![
            super::LIMITATION_INSTRUMENTED_NOT_ORDINARY,
            super::LIMITATION_TRACE_NOT_EXECUTION,
            super::LIMITATION_HEADER_IS_PLAN_FRAMING,
            super::LIMITATION_DISPOSABLE_MANIFEST,
        ];
        expected.sort_unstable();
        assert_eq!(required_limitations(), expected);
    }

    #[test]
    fn constants_pin_the_channel_and_tool_identities() {
        assert_eq!(super::TRACE_CHANNEL_BASENAME, ".perl-core-harness-trace.jsonl");
        assert_eq!(super::PATCH_TOOL_IDENTITY, "perl-core-harness/exact-anchor-patch/1");
        assert_eq!(
            super::INSTRUMENTATION_WORK_SCHEMA_VERSION,
            "perl_core_harness.instrumentation_work.v1"
        );
        assert_eq!(super::EXACT_PATCH_SCHEMA_VERSION, "perl_core_harness.exact_runner_patch.v1");
        assert_eq!(
            UPSTREAM_INVOCATION_TRACE_SCHEMA_VERSION,
            "perl_core_harness.upstream_effective_invocation_trace.v1"
        );
    }
}
