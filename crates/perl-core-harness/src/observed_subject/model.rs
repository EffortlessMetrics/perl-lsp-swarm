//! Immutable observed runner subject types
//! (`perl_core_harness.observed_runner_subject.v1`, #12287).
//!
//! One closed result model for the pure one-to-one fan-in join between one
//! complete observed upstream discovery receipt (#12281/#12283), its
//! independently reconstructed runner plan (#7737), and one complete
//! effective-invocation trace observation set (#12284/#12285) under the exact
//! ordinary/instrumented transfer relation (#12286) and the caller-supplied
//! exact producer subject identity (#12158).
//!
//! The join never copies source or invocation facts into a second vocabulary:
//! every joined row retains its discovery raw/order identity, its canonical
//! plan projection digest, and its typed join disposition. It performs no
//! upstream runner execution, tracing, compiler invocation, process
//! reconciliation, report assembly, selected production cutover, or
//! accepted-state transition.

use crate::invocation_trace::model::FieldStateCounts;
use crate::observed_discovery::model::{EvidenceClass, LineFraming, RunnerArtifactIdentity};
use crate::runner_model::{DiscoveryFrame, RunnerKind, RunnerPlan, RunnerScheduling};
use serde::{Deserialize, Serialize};

/// Versioned identity of the observed runner subject schema.
pub const OBSERVED_RUNNER_SUBJECT_SCHEMA_VERSION: &str =
    "perl_core_harness.observed_runner_subject.v1";

/// Number of subject-relation validations one strict construction performs.
pub const SUBJECT_VALIDATIONS_PER_CONSTRUCTION: u64 = 6;

/// Fixed claim boundary carried by every observed runner subject.
pub const OBSERVED_SUBJECT_CLAIM_BOUNDARY: &str = "pure one-to-one join of one complete observed discovery receipt, one \
     independently validated runner plan, and one complete effective-invocation \
     observation set under an exact producer subject; upstream execution, \
     tracing, compiler invocation, production selection, and accepted-state \
     transitions remain unproven";

/// Mandatory limitation retained by every observed runner subject.
pub const LIMITATION_JOIN_NOT_EXECUTION: &str =
    "observed_runner_subject_joins_observations_it_never_executes_runners_or_compiler_rows";
/// Mandatory limitation retained by every observed runner subject.
pub const LIMITATION_REFERENCES_ARE_CALLER_SUPPLIED: &str =
    "producer_plan_and_equivalence_identities_are_caller_supplied_references";
/// Mandatory limitation retained by every observed runner subject.
pub const LIMITATION_NO_LOCAL_AUTHORITY: &str =
    "join_performs_no_source_reads_filesystem_scans_direct_probes_or_field_reconstruction";

/// Aggregate result state of one observed runner subject. Exactly one state;
/// a producer label, a complete count, or a direct probe never upgrades it.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservedSubjectState {
    /// Every law holds: complete discovery, valid independent plan, exactly
    /// one complete projected invocation per admitted member, bound transfer
    /// relation, agreed producer subject.
    CompleteCurrent,
    /// At least one admitted member has no invocation observation at all.
    PartialMissingInvocation,
    /// A complete projected invocation exists for a member outside the
    /// admitted discovery membership.
    PartialExtraInvocation,
    /// One member was claimed by more than one complete invocation row
    /// (identical digests stay duplicates inside this category).
    PartialConflictingInvocation,
    /// An invoked member's observation is well-formed but has at least one
    /// behavior-bearing field not `observed`, so it cannot project a plan.
    PartialUnobservedFields,
    /// The observation is instrument-captured and the supplied #12286
    /// ordinary/instrumented relation does not bind it exactly (or is absent),
    /// so the ordinary-runner proposition is not transferred.
    InstrumentedWithoutEquivalence,
    /// Row-level subject bindings disagree with the joined subject even though
    /// receipt-level binding held.
    SubjectMismatch,
    /// Consumer-side freshness judgment: prepared tree moved on. Assigned only
    /// through [`crate::observed_subject::build::observed_subject_freshness`].
    Stale,
    /// The upstream capture was cancelled before a terminal state existed.
    Cancelled,
    /// The instrumentation wrapper failed independently of the runner.
    InstrumentFailure,
    /// Terminal evidence missing, decode failed, discovery partial/failed/truncated,
    /// or trace stream malformed: nothing else can be claimed.
    NotProven,
}

impl ObservedSubjectState {
    /// Only `complete_current` proves the joined subject.
    pub fn is_complete(self) -> bool {
        matches!(self, Self::CompleteCurrent)
    }
}

/// Caller-supplied exact producer subject identity (#12158). Opaque
/// references validated for shape here and bound field-by-field to the
/// discovery observation during the join; equal spelling from another
/// repository candidate, tree, runner artifact, or profile cannot repair a
/// mismatch.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProducerSubjectIdentity {
    /// Commit of the measuring repository.
    pub repository_commit: String,
    /// Resolved upstream Perl source reference.
    pub perl_ref: String,
    /// Prepared-tree identity reference.
    pub prepared_tree_identity: String,
    /// Host Perl interpreter identity reference.
    pub host_perl_identity: String,
    /// Pinned target matrix fingerprint.
    pub matrix_fingerprint: String,
    /// Target contract identity.
    pub target_id: String,
    /// SHA-256 of the pinned target selection contract.
    pub target_contract_digest: String,
    /// Environment-variant target reference when the producer ran a variant.
    pub variant_target_id: Option<String>,
    /// Upstream route the producer owns.
    pub runner: RunnerKind,
    /// Exact ordinary runner artifact/source identity.
    pub runner_artifact: RunnerArtifactIdentity,
    /// Prepared-tree-relative working directory of the producer route.
    pub working_directory: String,
    /// Behavior-bearing environment identity digest of the producer route.
    pub environment_sha256: String,
}

/// Exact ordinary/instrumented transfer-relation references (#12286). Every
/// field must agree with the inputs it binds during the join; a relation that
/// belongs to another patch, Perl ref, target, schema, or artifact pair fails
/// closed as `instrumented_without_equivalence`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrdinaryInstrumentedEquivalenceIdentity {
    /// Instrumentation subject both sides were captured under.
    pub instrumentation_id: String,
    /// SHA-256 of the ordinary runner artifact bytes this relation transfers to.
    pub ordinary_runner_artifact_sha256: String,
    /// SHA-256 of the instrumented derivative artifact bytes this relation
    /// transfers from.
    pub instrumented_runner_artifact_sha256: String,
    /// Retained patch-subject digest of the instrumented derivative.
    pub patch_subject_digest: String,
}

/// The full input set of one observed runner subject join. Runtime-only: never
/// serialized into receipts. Receipts are consumed only through their strict
/// validation adapters, so an internally incoherent input cannot reach the
/// arithmetic.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservedRunnerSubjectInput {
    /// Validated exact producer subject identity (#12158).
    pub producer: ProducerSubjectIdentity,
    /// Independently reconstructed runner plan (#7737); revalidated against
    /// matrix authority and byte-bound to the observed discovery stream.
    pub plan: RunnerPlan,
    /// Observed upstream discovery receipt (#12281/#12283).
    pub discovery: crate::observed_discovery::model::UpstreamDiscoveryReceiptV1,
    /// Effective-invocation trace receipt (#12284/#12285), parent-bound to
    /// `discovery`.
    pub trace: crate::invocation_trace::model::EffectiveInvocationTraceReceiptV1,
    /// Ordinary/instrumented transfer relation (#12286), required whenever the
    /// observation is instrument-captured.
    pub equivalence: Option<OrdinaryInstrumentedEquivalenceIdentity>,
}

/// Per-row join disposition. Categories cannot collapse into each other or
/// disappear behind aggregate completeness.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "detail")]
pub enum SubjectJoinDisposition {
    /// Exactly one complete projected invocation matched this member.
    Joined,
    /// No invocation observation reached this admitted member.
    MissingInvocation,
    /// The member was invoked but its observation cannot project a plan.
    PartialFields {
        /// Wire name of the first behavior-bearing field that is not observed.
        first_missing_field: String,
    },
    /// A complete projected invocation claims a member outside the admission.
    ExtraInvocation {
        /// Invocation sequence of the offending row.
        sequence: u32,
    },
    /// More than one identical complete projection claimed this member.
    DuplicateInvocation {
        /// Invocation sequences claiming the member, in stream order.
        sequences: Vec<u32>,
    },
    /// More than one distinct complete projection claimed this member.
    ConflictingInvocation {
        /// Invocation sequences claiming the member, in stream order.
        sequences: Vec<u32>,
    },
    /// The invocation row itself carries a subject-mismatch state from the
    /// trace contract; it joins as evidence of the mismatch, never as a
    /// member invocation.
    SubjectMismatchRow {
        /// Producer-assigned identity of the offending invocation row.
        row_id: String,
    },
}

/// One joined row. Discovery members keep their original order, raw spelling,
/// framing, and normalized source identity; extra invocation rows keep their
/// invocation identity. Facts are retained by reference, never re-encoded.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedRunnerSubjectRow {
    /// Canonical member identity this row accounts for.
    pub member_path: String,
    /// Zero-based position of the contributing discovery row in original
    /// observed order; `None` for extra invocation rows.
    pub discovery_ordinal: Option<u32>,
    /// Raw discovery row spelling without the line terminator; `None` for
    /// extra invocation rows.
    pub discovery_raw_text: Option<String>,
    /// Line framing observed for the contributing discovery row; `None` for
    /// extra invocation rows.
    pub framing: Option<LineFraming>,
    /// Normalized source item of the contributing discovery row; `None` for
    /// extra invocation rows.
    pub normalized: Option<crate::runner_model::RunnerSourceItem>,
    /// Sequence of the single matching invocation row where exactly one exists.
    pub invocation_sequence: Option<u32>,
    /// Canonical projection digest when the row completed its plan projection.
    pub projection_digest: Option<String>,
    /// Typed per-field observation counts of the matching invocation row.
    pub field_counts: FieldStateCounts,
    /// Typed join disposition assigned to this row.
    pub disposition: SubjectJoinDisposition,
    /// Deterministic fingerprint over every other field of this row.
    pub row_fingerprint: String,
}

/// Named-field diagnostic retained by the payload. Failures name the exact
/// disagreeing field or member; they are never summarized away.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectDiagnostic {
    /// Field or authority whose agreement failed (for example
    /// `plan.raw_discovery_digest`).
    pub field: String,
    /// Member path the diagnostic is scoped to, when member-local.
    pub member_path: Option<String>,
    /// Precise disagreement text.
    pub detail: String,
}

/// Agreed cross-receipt identity snapshot recorded by the join. Every field
/// was checked for exact agreement across all four inputs before joining.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedSubjectBindings {
    /// Commit of the measuring repository shared by all inputs.
    pub repository_commit: String,
    /// Resolved upstream Perl source reference shared by all inputs.
    pub perl_ref: String,
    /// Prepared-tree identity reference shared by all inputs.
    pub prepared_tree_identity: String,
    /// Host Perl interpreter identity reference shared by all inputs.
    pub host_perl_identity: String,
    /// Pinned target matrix fingerprint shared by all inputs.
    pub matrix_fingerprint: String,
    /// Target contract identity shared by all inputs.
    pub target_id: String,
    /// SHA-256 of the pinned target selection contract shared by all inputs.
    pub target_contract_digest: String,
    /// Environment-variant target reference shared by all inputs.
    pub variant_target_id: Option<String>,
    /// Upstream route of the discovery observation and the independent plan.
    pub runner: RunnerKind,
    /// Exact ordinary runner artifact/source identity.
    pub runner_artifact: RunnerArtifactIdentity,
    /// Working directory of the ordinary route.
    pub working_directory: String,
    /// Behavior-bearing environment identity digest of the ordinary route.
    pub environment_sha256: String,
    /// Explicit discovery frame declared by both discovery receipt and plan.
    pub discovery_frame: DiscoveryFrame,
    /// Capture identity of the ordinary discovery process.
    pub discovery_process_nonce: String,
    /// Trace session identity of the joined observation.
    pub trace_session_id: String,
    /// Payload digest of the joined discovery receipt.
    pub discovery_receipt_digest: String,
    /// Payload digest of the joined invocation trace receipt.
    pub trace_receipt_digest: String,
    /// Structural digest of the independently validated runner plan.
    pub plan_digest: String,
}

/// Work accounting proven by this join. Unknown work is never recorded as
/// numeric zero; the five zero invariants are structural properties of the
/// pure join and are re-proven during validation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JoinWork {
    /// Decoded discovery rows examined.
    pub discovery_rows_considered: u64,
    /// Discovery rows inside the accepted membership.
    pub discovery_accepted_rows: u64,
    /// Decoded invocation rows examined.
    pub invocation_rows_considered: u64,
    /// Invocation rows deriving `observed_complete` with an accepted frame.
    pub complete_invocation_rows: u64,
    /// Invocation rows deriving `observed_partial` with an accepted frame.
    pub partial_invocation_rows: u64,
    /// Rows that joined one-to-one.
    pub joined_rows: u64,
    /// Members with no invocation observation.
    pub missing_invocation_rows: u64,
    /// Complete invocations outside the membership.
    pub extra_invocation_rows: u64,
    /// Duplicate (byte-identical projections) member claims.
    pub duplicate_invocation_rows: u64,
    /// Conflicting (distinct projections) member claims.
    pub conflicting_invocation_rows: u64,
    /// Invocation rows carrying `subject_mismatch` states.
    pub subject_mismatch_rows: u64,
    /// Discovery rows with unsupported source forms (never members).
    pub unsupported_source_form_rows: u64,
    /// Runner-plan validations performed and accepted by construction.
    pub plan_validations_accepted: u64,
    /// Structural invariant of this contract: always zero.
    pub source_reads: u64,
    /// Structural invariant of this contract: always zero.
    pub filesystem_scans: u64,
    /// Structural invariant of this contract: always zero.
    pub runner_processes: u64,
    /// Structural invariant of this contract: always zero.
    pub direct_probe_inputs: u64,
    /// Structural invariant of this contract: always zero.
    pub reconstructed_fields: u64,
}

/// Full evidence payload bound by [`ObservedRunnerSubjectV1::payload_digest`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedRunnerSubjectPayload {
    /// Sorted evidence classes actually consumed by this join (`observed_upstream`
    /// plus `instrumented_upstream` when the observation is instrument-captured).
    pub evidence_classes: Vec<EvidenceClass>,
    /// Agreed cross-receipt identity snapshot.
    pub bindings: ObservedSubjectBindings,
    /// Exact producer subject identity bound by this join.
    pub producer: ProducerSubjectIdentity,
    /// Accepted ordinary/instrumented transfer relation, present exactly when
    /// one was supplied and bound exactly.
    pub equivalence: Option<OrdinaryInstrumentedEquivalenceIdentity>,
    /// Runner scheduling facts retained verbatim from the independent plan.
    pub scheduling: RunnerScheduling,
    /// Single derived aggregate state.
    pub state: ObservedSubjectState,
    /// Joined rows: admission order first (original ordinals), then extra
    /// invocation rows in stream order.
    pub rows: Vec<ObservedRunnerSubjectRow>,
    /// Named-field diagnostics covering every non-joined outcome.
    pub diagnostics: Vec<SubjectDiagnostic>,
    /// Proven work accounting.
    pub work: JoinWork,
    /// Mandatory limitations retained verbatim, sorted.
    pub limitations: Vec<String>,
    /// Fixed claim boundary retained verbatim.
    pub claim_boundary: String,
}

/// Immutable versioned observed runner subject (#12287): the fan-in proof that
/// the observed `t/TEST` member set equals the effective invocation denominator.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedRunnerSubjectV1 {
    /// Schema identity; always [`OBSERVED_RUNNER_SUBJECT_SCHEMA_VERSION`].
    pub schema_version: String,
    /// SHA-256 over the canonical serialization of `payload`.
    pub payload_digest: String,
    /// Full evidence payload.
    pub payload: ObservedRunnerSubjectPayload,
}

/// Internal lookup record for one complete projected invocation.
pub(crate) struct ProjectedInvocation {
    pub(crate) sequence: u32,
    pub(crate) digest: String,
}
