//! Immutable observed upstream-discovery receipt types (`upstream_runner_discovery.v1`).
//!
//! One closed result model for one upstream runner discovery operation. The
//! receipt retains byte-exact raw stdout/stderr envelopes, the typed terminal
//! state of the upstream process, frame-aware decoded source rows resolved
//! through the one current runner-plan normalizer, membership dispositions,
//! and a deterministic payload digest. It never spawns processes, walks the
//! filesystem, or repairs missing evidence.

use crate::runner_model::{DiscoveryFrame, RunnerKind, RunnerSourceItem};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Versioned identity of the observed-discovery receipt schema.
pub const UPSTREAM_DISCOVERY_SCHEMA_VERSION: &str =
    "perl_core_harness.upstream_runner_discovery.v1";

/// Hard upper bound for one retained raw stream envelope.
pub const MAX_RAW_STREAM_BYTES: usize = 1024 * 1024;

/// Hard upper bound for decoded rows in one observed stream.
pub const MAX_DECODED_ROWS: usize = 100_000;

/// Number of subject-relation validations one strict construction performs.
pub const SUBJECT_VALIDATIONS_PER_CONSTRUCTION: u64 = 3;

/// Fixed claim boundary carried by every observed discovery receipt.
pub const OBSERVED_DISCOVERY_CLAIM_BOUNDARY: &str = "byte-exact raw upstream runner discovery output decoded under explicit frames with typed terminal state; production selection, acceptance, compiler results, and effective per-file invocations remain unproven";

/// Mandatory limitation retained by every observed discovery receipt.
pub const LIMITATION_MEMBERSHIP_NOT_SELECTED: &str =
    "decoded_membership_is_observed_not_selected_or_accepted_production_membership";
/// Mandatory limitation retained by every observed discovery receipt.
pub const LIMITATION_REFERENCES_ARE_CALLER_SUPPLIED: &str =
    "repository_perl_preparation_and_environment_identities_are_caller_supplied_references";
/// Mandatory limitation retained by every observed discovery receipt.
pub const LIMITATION_NO_LOCAL_DISCOVERY: &str =
    "decoder_performs_no_filesystem_discovery_or_direct_probing";

/// Evidence-class law. Equal bytes, membership, order, argv, or digest never
/// collapse these classes.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceClass {
    /// Caller-declared input that no upstream runner produced.
    DeclaredInput,
    /// Membership reconstructed from matrix authority alone.
    ReconstructedExpected,
    /// Byte-exact output captured from an admitted upstream runner route.
    ObservedUpstream,
    /// Output captured under an instrumentation subject.
    InstrumentedUpstream,
    /// Historical evidence recorded without today's identity boundaries.
    HistoricalUnbound,
    /// Direct diagnostic output outside the runner route authority.
    DirectDiagnostic,
}

/// Terminal and completeness vocabulary for one observed discovery operation.
/// States cannot collapse: the strict derivation assigns exactly one state
/// from the supplied envelope facts.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryObservationState {
    /// Complete terminal, complete bounded raw output, strict decode complete,
    /// all rows frame-resolved, no unowned duplicate/conflict, no subject
    /// mismatch.
    ObservedComplete,
    /// Terminal admitted but at least one retained row is not accepted
    /// membership (duplicate, conflict, out-of-target, unsupported form).
    ObservedPartial,
    /// Upstream process exited nonzero or died by signal.
    RunnerFailed,
    /// Capture or runner cancelled before a terminal state existed.
    Cancelled,
    /// Runner exceeded its capture deadline.
    TimedOut,
    /// Producer flagged capture truncation on either retained stream.
    OutputTruncated,
    /// Strict decode failed: bad encoding, framing, or control bytes.
    MalformedOutput,
    /// The instrumentation wrapper failed independently of the runner.
    InstrumentFailed,
    /// Recorded subject references disagree (runner/artifact binding, target
    /// contract digest, or capture-identity pairing).
    SubjectMismatch,
    /// Consumer-side freshness judgment against the current prepared tree;
    /// never written into a receipt by this module.
    Stale,
    /// Missing terminal evidence; nothing else can be claimed.
    NotProven,
}

impl DiscoveryObservationState {
    /// Only `observed_complete` proves a complete observation.
    pub fn is_complete(self) -> bool {
        matches!(self, Self::ObservedComplete)
    }
}

/// Typed terminal outcome of the upstream runner process. `Unknown` records
/// missing terminal evidence and always derives `not_proven`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessCompletion {
    /// Process exited with the recorded status code.
    ExitStatus {
        /// Raw exit status observed for the upstream process.
        code: i32,
    },
    /// Process died from the recorded signal number.
    Signalled {
        /// Raw signal number observed for the upstream process.
        signal: u32,
    },
    /// Operation was cancelled before reaching a terminal state.
    Cancelled,
    /// Runner missed its capture deadline.
    TimedOut {
        /// Deadline that expired, in milliseconds.
        deadline_millis: u64,
    },
    /// The instrumentation wrapper failed around the runner.
    InstrumentFailed,
    /// No terminal evidence was supplied.
    Unknown,
}

/// Byte-exact raw stream envelope retained by the receipt. Bytes are stored as
/// lower-case hexadecimal so arbitrary binary output survives JSON losslessly.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawStreamEnvelope {
    /// Capture identity shared by both streams and the terminal observation
    /// of the same process.
    pub process_nonce: String,
    /// Exact captured bytes, lower-case hexadecimal.
    pub bytes_hex: String,
    /// Producer assertion that capture was cut at the retention bound before
    /// the upstream process finished writing this stream.
    pub truncated: bool,
}

impl RawStreamEnvelope {
    /// Decoded captured bytes.
    pub fn bytes(&self) -> Result<Vec<u8>, String> {
        hex_decode(&self.bytes_hex)
    }

    /// Captured byte count.
    pub fn byte_len(&self) -> usize {
        self.bytes_hex.len() / 2
    }
}

/// Exact upstream runner artifact/source identity retained by the subject.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerArtifactIdentity {
    /// Prepared-tree-relative runner artifact path (for example `t/TEST`).
    pub canonical_path: String,
    /// SHA-256 of the artifact bytes as captured for this observation.
    pub content_sha256: String,
}

/// Behavior-bearing environment identity. The map is retained sorted and the
/// digest is always recomputed from it during validation.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentIdentity {
    /// Behavior-bearing variables, sorted by key.
    pub variables: BTreeMap<String, String>,
    /// SHA-256 over `key=value\n` lines in sorted key order.
    pub sha256: String,
}

/// Validated subject references for one observed discovery operation. These
/// are opaque references, not a second harness-subject vocabulary.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoverySubjectIdentity {
    /// Commit of the measuring repository.
    pub repository_commit: String,
    /// Resolved upstream Perl source reference.
    pub perl_ref: String,
    /// Caller-supplied prepared-tree identity reference.
    pub prepared_tree_identity: String,
    /// Caller-supplied host Perl interpreter identity reference.
    pub host_perl_identity: String,
    /// Pinned target matrix fingerprint binding the target references below.
    pub matrix_fingerprint: String,
    /// Target contract identity the discovery was executed for.
    pub target_id: String,
    /// SHA-256 of the pinned target selection contract.
    pub target_contract_digest: String,
    /// Environment-variant target when the observation ran a variant.
    pub variant_target_id: Option<String>,
    /// Instrumentation subject when the observation ran instrumented.
    pub instrumentation_id: Option<String>,
}

/// Exact invocation identity of the observed upstream runner operation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvocationObservation {
    /// Admitted upstream runner kind for this observation.
    pub runner: RunnerKind,
    /// Exact runner artifact/source identity.
    pub runner_artifact: RunnerArtifactIdentity,
    /// Relative argv of the upstream process; absolute paths are rejected.
    pub argv: Vec<String>,
    /// Prepared-tree-relative working directory of the upstream process.
    pub working_directory: String,
    /// Behavior-bearing environment identity.
    pub environment: EnvironmentIdentity,
}

/// Typed terminal observation retained by the receipt.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalObservation {
    /// Capture identity shared with both raw stream envelopes.
    pub process_nonce: String,
    /// Typed terminal outcome of the upstream process.
    pub completion: ProcessCompletion,
}

/// Frame spelling of one decoded raw row.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineFraming {
    /// Terminated by LF.
    Lf,
    /// Terminated by CRLF.
    Crlf,
    /// Final row without a terminating newline.
    Eof,
}

/// Frame-aware decoded source row. Resolution always goes through the one
/// current runner-plan normalizer; unsupported forms and malformed rows are
/// preserved and typed, never dropped or repaired.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedDiscoveryRow {
    /// Zero-based position in the original observed order.
    pub ordinal: u32,
    /// Raw row spelling without the line terminator.
    pub raw_text: String,
    /// Line framing observed for this row.
    pub framing: LineFraming,
    /// Explicit discovery frame declared for the observed stream.
    pub discovery_frame: DiscoveryFrame,
    /// Membership disposition assigned to this row.
    pub disposition: MemberDisposition,
    /// Present exactly when the row normalized successfully.
    pub normalized: Option<RunnerSourceItem>,
}

impl ObservedDiscoveryRow {
    /// Canonical repository-relative source identity when normalized.
    pub fn canonical_path(&self) -> Option<&str> {
        self.normalized.as_ref().map(|item| item.canonical_path.as_str())
    }

    /// True when the row entered the accepted membership.
    pub fn is_accepted(&self) -> bool {
        matches!(self.disposition, MemberDisposition::Accepted)
    }
}

/// Membership disposition of one decoded row. Every row receives exactly one
/// disposition; duplicates, conflicts, out-of-target rows, unsupported forms,
/// and malformed rows are retained explicitly.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "detail")]
pub enum MemberDisposition {
    /// Row normalized, matched the declared forms, and sits inside the target
    /// selection as the first contributor of its canonical identity.
    Accepted,
    /// Same canonical identity observed again from an identical raw spelling.
    DuplicateOfCanonical {
        /// Canonical path shared with the first contributing row.
        canonical_path: String,
    },
    /// Same canonical identity reached from a different raw spelling or frame.
    ConflictingCanonical {
        /// Canonical path shared with the first contributing row.
        canonical_path: String,
    },
    /// Normalized identity lies outside the declared target selection.
    OutsideTargetSelection,
    /// Source form or discovery root is not part of the receipt vocabulary.
    UnsupportedSourceForm,
    /// Control bytes, empty framing, or another unresolvable row defect.
    MalformedRow,
}

/// Outcome of strictly decoding one raw stream.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "outcome", content = "reason")]
pub enum StreamDecodeOutcome {
    /// Every row was framed and classified strictly.
    Complete,
    /// Stream-level malformation (for example invalid UTF-8) prevented any
    /// row reconstruction.
    Malformed {
        /// Reason the strict decode failed.
        reason: String,
    },
}

/// Work accounting proven by this decoder. Unknown work is never recorded as
/// numeric zero; the two zero invariants are structural properties of the
/// decoder and are re-proven during validation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecoderWork {
    /// Retained stdout bytes consumed by the decoder.
    pub raw_stdout_bytes: u64,
    /// Retained stderr bytes carried as evidence only.
    pub raw_stderr_bytes: u64,
    /// Rows reconstructed from the retained stdout bytes.
    pub decoded_rows: u64,
    /// Rows entering the accepted membership.
    pub accepted_rows: u64,
    /// Rows rejected as malformed.
    pub malformed_rows: u64,
    /// Rows recorded as duplicates of an earlier canonical identity.
    pub duplicate_rows: u64,
    /// Rows recorded as canonical conflicts with an earlier raw spelling.
    pub conflicting_rows: u64,
    /// Rows outside the declared target selection.
    pub out_of_target_rows: u64,
    /// Rows with unsupported source form or root.
    pub unsupported_source_form_rows: u64,
    /// Rows that reached the current runner-plan normalizer.
    pub normalization_operations: u64,
    /// Subject-relation checks performed during construction.
    pub terminal_subject_validations: u64,
    /// Structural invariant of this decoder: always zero.
    pub filesystem_discovery_operations: u64,
    /// Structural invariant of this decoder: always zero.
    pub direct_probe_rows_consumed: u64,
}

/// Full evidence payload bound by [`UpstreamDiscoveryReceiptV1::payload_digest`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoveryPayload {
    /// Validated subject references for the observation.
    pub subject: DiscoverySubjectIdentity,
    /// Exact invocation identity of the upstream process.
    pub invocation: InvocationObservation,
    /// Explicit discovery frame declared for the observed stdout stream.
    pub discovery_frame: DiscoveryFrame,
    /// Typed terminal observation of the upstream process.
    pub terminal: TerminalObservation,
    /// Byte-exact raw stdout envelope.
    pub stdout: RawStreamEnvelope,
    /// Byte-exact raw stderr envelope.
    pub stderr: RawStreamEnvelope,
    /// Outcome of strictly decoding the stdout envelope.
    pub stdout_decode: StreamDecodeOutcome,
    /// Frame-aware decoded rows in original observed order.
    pub rows: Vec<ObservedDiscoveryRow>,
    /// Single derived terminal/completeness state.
    pub state: DiscoveryObservationState,
    /// Proven decoder work accounting.
    pub work: DecoderWork,
    /// Mandatory limitations retained verbatim.
    pub limitations: Vec<String>,
    /// Fixed claim boundary retained verbatim.
    pub claim_boundary: String,
}

/// Immutable versioned observed upstream-discovery receipt.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpstreamDiscoveryReceiptV1 {
    /// Schema identity; always [`UPSTREAM_DISCOVERY_SCHEMA_VERSION`].
    pub schema_version: String,
    /// Always `observed_upstream`; every other class is rejected on validation.
    pub evidence_class: EvidenceClass,
    /// SHA-256 over the canonical serialization of `payload`.
    pub payload_digest: String,
    /// Full evidence payload.
    pub payload: DiscoveryPayload,
}

/// Freshness judgment for consumers comparing a receipt against the current
/// prepared tree. Never written into the receipt itself.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReceiptFreshness {
    /// Receipt's prepared-tree reference matches the current reference.
    Current,
    /// Prepared tree moved on after this observation was captured.
    Stale,
}

/// Supplied terminal process/raw-output envelope plus validated subject and
/// invocation references for one observed discovery construction. This is the
/// only input shape a receipt can be built from: bare declared bytes cannot
/// select the `observed_upstream` evidence class.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservedDiscoveryInput {
    /// Validated subject references for the observation.
    pub subject: DiscoverySubjectIdentity,
    /// Admitted upstream routes only (`test`, `harness`).
    pub runner: RunnerKind,
    /// Exact runner artifact/source identity.
    pub runner_artifact: RunnerArtifactIdentity,
    /// Relative argv of the upstream process.
    pub argv: Vec<String>,
    /// Prepared-tree-relative working directory.
    pub working_directory: String,
    /// Behavior-bearing environment variables retained sorted by key.
    pub environment: BTreeMap<String, String>,
    /// Explicit frame for every row of the observed stdout stream.
    pub discovery_frame: DiscoveryFrame,
    /// Typed terminal outcome of the upstream process.
    pub completion: ProcessCompletion,
    /// Capture identity shared across both streams and the terminal record.
    pub process_nonce: String,
    /// Exact raw stdout bytes captured from the upstream process.
    pub stdout_bytes: Vec<u8>,
    /// Whether stdout capture was cut at the retention bound.
    pub stdout_truncated: bool,
    /// Exact raw stderr bytes captured from the upstream process.
    pub stderr_bytes: Vec<u8>,
    /// Whether stderr capture was cut at the retention bound.
    pub stderr_truncated: bool,
}

pub(crate) fn hex_decode(value: &str) -> Result<Vec<u8>, String> {
    if !value.len().is_multiple_of(2) {
        return Err("hex stream must have even length".to_string());
    }
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(value.len() / 2);
    for pair in bytes.chunks(2) {
        let high =
            hex_nibble(pair[0]).ok_or_else(|| "hex stream contains a non-hex byte".to_string())?;
        let low =
            hex_nibble(pair[1]).ok_or_else(|| "hex stream contains a non-hex byte".to_string())?;
        out.push((high << 4) | low);
    }
    Ok(out)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

pub(crate) fn hex_encode(bytes: &[u8]) -> String {
    const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX_DIGITS[usize::from(byte >> 4)] as char);
        out.push(HEX_DIGITS[usize::from(byte & 0x0f)] as char);
    }
    out
}
