//! Immutable effective-invocation trace types
//! (`upstream_effective_invocation_trace.v1`).
//!
//! One closed representation for what an instrumented upstream runner
//! (`t/TEST`/`t/harness`) actually decided and invoked for one discovered
//! member. The contract consumes supplied trace bytes and exact parent
//! subjects: it decodes one strict independently framed stream, assigns every
//! behavior-bearing field a typed observation state, re-binds each row to its
//! parent discovery receipt/member/process subject, and projects complete
//! observations into a canonical invocation plan projection. It never patches,
//! executes, or reads the upstream runner, the prepared tree, or project code.

use crate::observed_discovery::model::{
    EnvironmentIdentity, EvidenceClass, ProcessCompletion, RunnerArtifactIdentity,
    UpstreamDiscoveryReceiptV1,
};
use crate::runner_model::{RunnerKind, RunnerScheduling, SourceForm};
use serde::{Deserialize, Serialize};

/// Versioned identity of the effective-invocation trace stream and receipt.
pub const UPSTREAM_INVOCATION_TRACE_SCHEMA_VERSION: &str =
    "perl_core_harness.upstream_effective_invocation_trace.v1";

/// Hard upper bound for one retained trace stream envelope.
pub const MAX_TRACE_STREAM_BYTES: usize = 1024 * 1024;

/// Hard upper bound for decoded rows in one trace stream.
pub const MAX_TRACE_ROWS: usize = 100_000;

/// Number of subject-relation validations one strict construction performs.
pub const SUBJECT_VALIDATIONS_PER_CONSTRUCTION: u64 = 4;

/// Fixed claim boundary carried by every effective-invocation trace receipt.
pub const INVOCATION_TRACE_CLAIM_BOUNDARY: &str = "strictly framed effective per-file invocation \
                                                    observation under typed field states; \
                                                    upstream instrumentation, process execution, \
                                                    plan equivalence, and production execution \
                                                    remain unproven";

/// Mandatory limitation retained by every trace receipt.
pub const LIMITATION_OBSERVATION_NOT_EXECUTION: &str =
    "trace_rows_are_observations_of_invocation_decisions_not_executed_results";
/// Mandatory limitation retained by every trace receipt.
pub const LIMITATION_PARENT_RECEIPT_CALLER_SUPPLIED: &str =
    "parent_discovery_receipt_and_subject_identities_are_caller_supplied_references";
/// Mandatory limitation retained by every trace receipt.
pub const LIMITATION_NO_RUNNER_INTERACTION: &str =
    "decoder_and_adapter_neither_patch_execute_nor_read_the_upstream_runner_or_prepared_tree";
/// Mandatory limitation retained by every trace receipt.
pub const LIMITATION_PARTIAL_ROWS_NEVER_PLANS: &str =
    "partial_ambiguous_or_failed_rows_never_project_an_authoritative_invocation_plan";

/// Byte markers whose presence in an ordinary runner result stream proves the
/// trace channel contaminated the runner output and the transport contract is
/// violated. Construction refuses such parents outright.
pub const TRACE_CONTAMINATION_MARKERS: [&str; 5] = [
    "perl_core_harness.upstream_effective_invocation_trace.v1",
    "\"frame\": \"header\"",
    "\"frame\":\"header\"",
    "\"frame\": \"terminal\"",
    "\"frame\":\"terminal\"",
];

/// Typed observation state of one behavior-bearing invocation field. `None`
/// collapse is structurally impossible: not-applicable, missing, ambiguous,
/// malformed, and instrument-failure evidence each keep their own state and
/// reason.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state", content = "payload")]
pub enum EffectiveInvocationField<T> {
    /// The instrument captured this field's exact value at invocation time.
    Observed {
        /// Captured value.
        value: T,
    },
    /// The field has no meaning for this invocation shape.
    NotApplicable {
        /// Why the field cannot apply.
        reason: String,
    },
    /// The field applies but the instrument captured nothing for it.
    NotObserved {
        /// Why nothing was captured.
        reason: String,
    },
    /// The instrument captured more than one candidate and cannot choose.
    Ambiguous {
        /// Retained candidate values in capture order.
        candidates: Vec<T>,
        /// Why the candidates could not be resolved.
        reason: String,
    },
    /// The instrument captured bytes for this field that do not decode.
    Malformed {
        /// Why the captured value is malformed.
        reason: String,
    },
    /// The instrument itself failed while capturing this field.
    InstrumentFailure {
        /// How the instrument failed.
        reason: String,
    },
}

impl<T> EffectiveInvocationField<T> {
    /// True only for [`Self::Observed`].
    pub fn is_observed(&self) -> bool {
        matches!(self, Self::Observed { .. })
    }

    /// The captured value when observed.
    pub fn observed(&self) -> Option<&T> {
        match self {
            Self::Observed { value } => Some(value),
            _ => None,
        }
    }

    /// True when the instrument failed while capturing this field.
    pub fn is_instrument_failure(&self) -> bool {
        matches!(self, Self::InstrumentFailure { .. })
    }
}

impl<T> Default for EffectiveInvocationField<T> {
    /// The honest default: nothing was observed for this field. Used only for
    /// malformed frames whose field map could not be decoded at all.
    fn default() -> Self {
        Self::NotObserved { reason: "field was not decoded from this frame".to_string() }
    }
}

/// Reviewed TestInit/bootstrap classes retained verbatim from upstream
/// scheduling vocabulary. Semantics stay owned by upstream; the contract only
/// keeps the distinctions load-bearing.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TestInitClass {
    /// Standard bootstrap without a special TestInit class.
    Standard,
    /// Upstream `U1` class.
    U1,
    /// Upstream `U2` class.
    U2,
    /// Upstream `U2T` (threaded) class.
    U2t,
    /// Upstream `A` (ascii) class.
    A,
    /// Upstream `NC` class.
    Nc,
}

/// Shebang/interpreter taint handling of one invocation. `-t` and `-T` are
/// distinct modes and never collapse.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaintMode {
    /// No taint switch was applied.
    None,
    /// Weak taint mode (`-t`): taint warnings without full taint mode.
    TaintWarnings,
    /// Full taint mode (`-T`).
    TaintMode,
}

/// UTF/source-mode handling of one invocation.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Utf8Switch {
    /// No UTF-8 handling was applied to this invocation.
    None,
    /// The runner applied UTF-8 handling (for example `PERL_UNICODE`/`-C`).
    Utf8,
}

/// Upstream operation point at which the frame was captured.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapturePoint {
    /// Member classification inside the upstream scan.
    MemberScan,
    /// The invocation decision for one member.
    InvocationDecision,
    /// The spawned interpreter/wrapper process.
    ProcessSpawn,
}

/// Script/source role the upstream scan selected the member into. The
/// `base`/`comp`/`run` invocation families stay distinct.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptRole {
    /// Selected into the `base` invocation family.
    Base,
    /// Selected into the `comp` invocation family.
    Comp,
    /// Selected into the `run` invocation family.
    Run,
    /// Selected into another reviewed population.
    Other,
}

/// Every behavior-bearing field of one invocation observation. Each field
/// carries its own typed state; producer labels and field counts never
/// upgrade a partial row.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectiveInvocationFields {
    /// Canonical member identity from the parent discovery receipt.
    pub member_identity: EffectiveInvocationField<String>,
    /// Canonical source form (`.t` versus `test.pl`); never collapsed.
    pub source_form: EffectiveInvocationField<SourceForm>,
    /// Effective script path the runner invoked.
    pub script_path: EffectiveInvocationField<String>,
    /// Script/source role the upstream scan selected the member into.
    pub script_role: EffectiveInvocationField<ScriptRole>,
    /// Working directory the invocation ran in.
    pub run_cwd: EffectiveInvocationField<String>,
    /// Directory the runner returns to after the invocation, when distinct.
    pub return_directory: EffectiveInvocationField<String>,
    /// Ordered interpreter/wrapper switches before the script.
    pub interpreter_switches: EffectiveInvocationField<Vec<String>>,
    /// Ordered `-I` include roots in application order.
    pub include_roots: EffectiveInvocationField<Vec<String>>,
    /// TestInit/bootstrap class of the invocation.
    pub test_init: EffectiveInvocationField<TestInitClass>,
    /// Shebang/interpreter taint mode.
    pub taint_mode: EffectiveInvocationField<TaintMode>,
    /// UTF/source-mode switch state.
    pub utf8_mode: EffectiveInvocationField<Utf8Switch>,
    /// Other admitted wrapper arguments.
    pub wrapper_arguments: EffectiveInvocationField<Vec<String>>,
    /// Arguments after the script path.
    pub script_arguments: EffectiveInvocationField<Vec<String>>,
    /// Behavior-bearing environment/capability identity.
    pub environment: EffectiveInvocationField<EnvironmentIdentity>,
    /// Runner scheduling/ordering facts available at invocation.
    pub scheduling: EffectiveInvocationField<RunnerScheduling>,
    /// Capture point inside the upstream operation.
    pub capture_point: EffectiveInvocationField<CapturePoint>,
    /// Upstream operation identity (for example the scan loop) as captured.
    pub upstream_operation: EffectiveInvocationField<String>,
}

/// Field vocabulary key used for typed rejections and comparisons.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldKey {
    /// `member_identity`
    MemberIdentity,
    /// `source_form`
    SourceForm,
    /// `script_path`
    ScriptPath,
    /// `script_role`
    ScriptRole,
    /// `run_cwd`
    RunCwd,
    /// `return_directory`
    ReturnDirectory,
    /// `interpreter_switches`
    InterpreterSwitches,
    /// `include_roots`
    IncludeRoots,
    /// `test_init`
    TestInit,
    /// `taint_mode`
    TaintMode,
    /// `utf8_mode`
    Utf8Mode,
    /// `wrapper_arguments`
    WrapperArguments,
    /// `script_arguments`
    ScriptArguments,
    /// `environment`
    Environment,
    /// `scheduling`
    Scheduling,
    /// `capture_point`
    CapturePoint,
    /// `upstream_operation`
    UpstreamOperation,
}

impl FieldKey {
    /// Every field key in declaration order.
    pub const ALL: [FieldKey; 17] = [
        FieldKey::MemberIdentity,
        FieldKey::SourceForm,
        FieldKey::ScriptPath,
        FieldKey::ScriptRole,
        FieldKey::RunCwd,
        FieldKey::ReturnDirectory,
        FieldKey::InterpreterSwitches,
        FieldKey::IncludeRoots,
        FieldKey::TestInit,
        FieldKey::TaintMode,
        FieldKey::Utf8Mode,
        FieldKey::WrapperArguments,
        FieldKey::ScriptArguments,
        FieldKey::Environment,
        FieldKey::Scheduling,
        FieldKey::CapturePoint,
        FieldKey::UpstreamOperation,
    ];

    /// Wire name of the field.
    pub fn wire_name(self) -> &'static str {
        match self {
            FieldKey::MemberIdentity => "member_identity",
            FieldKey::SourceForm => "source_form",
            FieldKey::ScriptPath => "script_path",
            FieldKey::ScriptRole => "script_role",
            FieldKey::RunCwd => "run_cwd",
            FieldKey::ReturnDirectory => "return_directory",
            FieldKey::InterpreterSwitches => "interpreter_switches",
            FieldKey::IncludeRoots => "include_roots",
            FieldKey::TestInit => "test_init",
            FieldKey::TaintMode => "taint_mode",
            FieldKey::Utf8Mode => "utf8_mode",
            FieldKey::WrapperArguments => "wrapper_arguments",
            FieldKey::ScriptArguments => "script_arguments",
            FieldKey::Environment => "environment",
            FieldKey::Scheduling => "scheduling",
            FieldKey::CapturePoint => "capture_point",
            FieldKey::UpstreamOperation => "upstream_operation",
        }
    }
}

/// Count of fields per observation state inside one row.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldStateCounts {
    /// Fields in the `observed` state.
    pub observed: u64,
    /// Fields in the `not_applicable` state.
    pub not_applicable: u64,
    /// Fields in the `not_observed` state.
    pub not_observed: u64,
    /// Fields in the `ambiguous` state.
    pub ambiguous: u64,
    /// Fields in the `malformed` state.
    pub malformed: u64,
    /// Fields in the `instrument_failure` state.
    pub instrument_failure: u64,
}

impl EffectiveInvocationFields {
    /// Count fields per observation state.
    pub fn state_counts(&self) -> FieldStateCounts {
        let mut counts = FieldStateCounts::default();
        for key in FieldKey::ALL {
            match self.state_of(key) {
                FieldStateRef::Observed => counts.observed += 1,
                FieldStateRef::NotApplicable => counts.not_applicable += 1,
                FieldStateRef::NotObserved => counts.not_observed += 1,
                FieldStateRef::Ambiguous => counts.ambiguous += 1,
                FieldStateRef::Malformed => counts.malformed += 1,
                FieldStateRef::InstrumentFailure => counts.instrument_failure += 1,
            }
        }
        counts
    }

    /// Typed state summary of one field, without carrying its value type.
    pub fn state_of(&self, key: FieldKey) -> FieldStateRef {
        macro_rules! summarize {
            ($field:expr) => {
                match &$field {
                    EffectiveInvocationField::Observed { .. } => FieldStateRef::Observed,
                    EffectiveInvocationField::NotApplicable { .. } => FieldStateRef::NotApplicable,
                    EffectiveInvocationField::NotObserved { .. } => FieldStateRef::NotObserved,
                    EffectiveInvocationField::Ambiguous { .. } => FieldStateRef::Ambiguous,
                    EffectiveInvocationField::Malformed { .. } => FieldStateRef::Malformed,
                    EffectiveInvocationField::InstrumentFailure { .. } => {
                        FieldStateRef::InstrumentFailure
                    }
                }
            };
        }
        match key {
            FieldKey::MemberIdentity => summarize!(self.member_identity),
            FieldKey::SourceForm => summarize!(self.source_form),
            FieldKey::ScriptPath => summarize!(self.script_path),
            FieldKey::ScriptRole => summarize!(self.script_role),
            FieldKey::RunCwd => summarize!(self.run_cwd),
            FieldKey::ReturnDirectory => summarize!(self.return_directory),
            FieldKey::InterpreterSwitches => summarize!(self.interpreter_switches),
            FieldKey::IncludeRoots => summarize!(self.include_roots),
            FieldKey::TestInit => summarize!(self.test_init),
            FieldKey::TaintMode => summarize!(self.taint_mode),
            FieldKey::Utf8Mode => summarize!(self.utf8_mode),
            FieldKey::WrapperArguments => summarize!(self.wrapper_arguments),
            FieldKey::ScriptArguments => summarize!(self.script_arguments),
            FieldKey::Environment => summarize!(self.environment),
            FieldKey::Scheduling => summarize!(self.scheduling),
            FieldKey::CapturePoint => summarize!(self.capture_point),
            FieldKey::UpstreamOperation => summarize!(self.upstream_operation),
        }
    }

    /// True when every behavior-bearing field is `observed`.
    pub fn all_observed(&self) -> bool {
        self.state_counts().observed == FieldKey::ALL.len() as u64
    }

    /// True when any field records an instrument failure.
    pub fn any_instrument_failure(&self) -> bool {
        self.state_counts().instrument_failure > 0
    }

    /// First field (declaration order) that is not `observed`.
    pub fn first_not_observed(&self) -> Option<FieldKey> {
        FieldKey::ALL.into_iter().find(|key| self.state_of(*key) != FieldStateRef::Observed)
    }
}

/// Value-free summary of one field's observation state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FieldStateRef {
    /// `observed`
    Observed,
    /// `not_applicable`
    NotApplicable,
    /// `not_observed`
    NotObserved,
    /// `ambiguous`
    Ambiguous,
    /// `malformed`
    Malformed,
    /// `instrument_failure`
    InstrumentFailure,
}

/// Exact subject binding one row claims. Every element must agree with the
/// receipt subject and the parent discovery receipt or the row is typed
/// `subject_mismatch`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RowSubjectBinding {
    /// Trace session identity shared by the stream header.
    pub trace_session_id: String,
    /// Parent discovery receipt payload digest this row belongs to.
    pub parent_receipt_digest: String,
    /// Canonical member identity inside the parent receipt's accepted rows.
    pub parent_member_path: String,
    /// Runner route the row was captured under.
    pub runner: RunnerKind,
    /// Target the row was captured for.
    pub target_id: String,
    /// Environment-variant target when the row ran a variant.
    pub variant_target_id: Option<String>,
    /// Instrumentation subject when the row ran instrumented.
    pub instrumentation_id: Option<String>,
}

/// Framing disposition of one decoded row frame. Duplicates, out-of-order
/// sequences, cross-run frames, and malformed frames are retained and typed,
/// never dropped, sorted, or repaired.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "detail")]
pub enum TraceRowDisposition {
    /// Frame is strictly well-formed and unique in identity and sequence.
    Accepted,
    /// Row identity was already contributed by an earlier frame; the first
    /// row is retained, never replaced.
    DuplicateRowId {
        /// Duplicated row identity.
        row_id: String,
    },
    /// Sequence number broke strict zero-based ordering.
    OutOfOrderSequence {
        /// Expected sequence number.
        expected: u32,
        /// Observed sequence number.
        actual: u32,
    },
    /// Frame carries a trace session identity foreign to the stream header.
    CrossRunInterleaved {
        /// Foreign session identity.
        session_id: String,
    },
    /// Frame bytes do not decode under the exact frame vocabulary.
    MalformedFrame {
        /// Reason the frame is malformed.
        reason: String,
    },
}

impl TraceRowDisposition {
    /// True only for [`Self::Accepted`].
    pub fn is_accepted(&self) -> bool {
        matches!(self, Self::Accepted)
    }

    /// True when the disposition records a framing conflict.
    pub fn is_conflicting(&self) -> bool {
        matches!(
            self,
            Self::DuplicateRowId { .. }
                | Self::OutOfOrderSequence { .. }
                | Self::CrossRunInterleaved { .. }
        )
    }
}

/// Row-level observation result. Exactly one state per row; a producer label
/// or field count never upgrades a partial row.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvocationObservationState {
    /// Every behavior-bearing field observed, frame accepted, subjects bound,
    /// terminal evidence complete.
    ObservedComplete,
    /// Well-formed frame with at least one field not `observed`.
    ObservedPartial,
    /// The traced runner process failed (nonzero exit or signal).
    RunnerFailed,
    /// The instrument itself failed around the runner.
    InstrumentFailed,
    /// Row subject binding disagrees with the receipt/parent subject.
    SubjectMismatch,
    /// Consumer-side freshness judgment; never written into a receipt.
    Stale,
    /// Terminal evidence missing, frame malformed, or identity unusable.
    NotProven,
}

/// Authority vocabulary of the canonical plan projection. Direct probes carry
/// a distinct authority and can never appear here.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvocationAuthority {
    /// The upstream `t/TEST` route.
    UpstreamTest,
    /// The upstream `t/harness` route.
    UpstreamHarness,
}

/// Canonical invocation plan projection built from one complete observation.
/// Every value came from the row's own `observed` fields; order is retained
/// verbatim. This is the observation-side input the #8492/#4827 canonical
/// plan authority consumes — it is not a second invocation model, and it can
/// never be produced from a partial observation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalInvocationProjection {
    /// Upstream route authority of the projected invocation.
    pub authority: InvocationAuthority,
    /// Canonical member identity inside the parent discovery receipt.
    pub member_path: String,
    /// Canonical source form.
    pub source_form: SourceForm,
    /// Effective script path.
    pub script_path: String,
    /// Script/source role the upstream scan selected.
    pub script_role: ScriptRole,
    /// Working directory of the invocation.
    pub run_cwd: String,
    /// Directory the runner returns to after the invocation.
    pub return_directory: String,
    /// Ordered interpreter/wrapper switches, order retained.
    pub interpreter_switches: Vec<String>,
    /// Ordered `-I` include roots, application order retained.
    pub include_roots: Vec<String>,
    /// TestInit/bootstrap class.
    pub test_init: TestInitClass,
    /// Shebang/interpreter taint mode.
    pub taint_mode: TaintMode,
    /// UTF/source-mode switch state.
    pub utf8_mode: Utf8Switch,
    /// Other admitted wrapper arguments, order retained.
    pub wrapper_arguments: Vec<String>,
    /// Arguments after the script path, order retained.
    pub script_arguments: Vec<String>,
    /// Behavior-bearing environment identity digest (reference).
    pub environment_sha256: String,
    /// Scheduling/ordering facts retained from the observation.
    pub scheduling: RunnerScheduling,
}

/// Deterministic digest binding one projection's exact content. Include-root
/// and switch order change the digest; host checkout spelling cannot enter
/// it.
pub fn canonical_projection_digest(
    projection: &CanonicalInvocationProjection,
) -> Result<String, String> {
    let bytes = serde_json::to_vec(projection)
        .map_err(|error| format!("serializing canonical invocation projection: {error}"))?;
    Ok(crate::build::sha256_bytes(&bytes))
}

/// Kind vocabulary of a typed projection rejection, retained per row.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionRejectionKind {
    /// The row is not `observed_complete`.
    ObservationNotComplete,
    /// The row's frame was not accepted.
    FrameNotAccepted,
    /// A behavior-bearing field is not `observed`.
    FieldNotObserved,
    /// An observed value failed its independent validation law.
    InvalidObservedValue,
    /// The row's subject binding disagrees with the expected subject.
    SubjectMismatch,
    /// Direct-probe routes never project through this contract.
    DirectProbeAuthority,
}

/// Per-row record of the canonical plan projection attempted at
/// construction. The pure adapter can always recompute the full projection;
/// the record retains the typed outcome and, when accepted, its digest.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "outcome")]
pub enum ProjectionRecord {
    /// The projection was accepted; `digest` binds its exact content.
    Projected {
        /// Digest of the accepted projection.
        digest: String,
    },
    /// The projection was rejected with one typed kind.
    Rejected {
        /// Typed rejection kind.
        reason: ProjectionRejectionKind,
    },
}

impl InvocationObservationState {
    /// Only `observed_complete` proves a complete observation.
    pub fn is_complete(self) -> bool {
        matches!(self, Self::ObservedComplete)
    }
}

/// One decoded effective-invocation observation row in original order.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectiveInvocationRow {
    /// Zero-based sequence position in the observed stream.
    pub sequence: u32,
    /// Producer-assigned stable row identity.
    pub row_id: String,
    /// Exact frame line text without the LF terminator.
    pub raw_line: String,
    /// Subject binding claimed by the frame.
    pub subject: RowSubjectBinding,
    /// Typed per-field observation states.
    pub fields: EffectiveInvocationFields,
    /// Framing disposition.
    pub disposition: TraceRowDisposition,
    /// Derived row-level result.
    pub state: InvocationObservationState,
    /// Deterministic fingerprint over the exact frame line bytes.
    pub row_fingerprint: String,
    /// Typed record of the canonical plan projection attempted for this row.
    pub projection: ProjectionRecord,
}

/// Outcome of strictly decoding one trace stream.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "outcome")]
pub enum TraceStreamOutcome {
    /// Stream decoded strictly: one header, ordered unique frames, terminal
    /// frame present and consistent.
    Complete,
    /// Stream-level malformation (invalid UTF-8, partial final row, missing or
    /// inconsistent terminal frame, contamination-grade framing drift).
    Malformed {
        /// Reason the strict decode failed.
        reason: String,
    },
}

impl TraceStreamOutcome {
    /// True when the stream decoded strictly.
    pub fn is_complete(&self) -> bool {
        matches!(self, Self::Complete)
    }
}

/// Decoded header frame facts retained by the receipt.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraceHeader {
    /// Stream schema identity; always
    /// [`UPSTREAM_INVOCATION_TRACE_SCHEMA_VERSION`].
    pub schema_version: String,
    /// Trace session identity binding every frame of this stream.
    pub trace_session_id: String,
    /// Capture identity of the parent runner process.
    pub parent_process_nonce: String,
    /// Parent discovery receipt payload digest.
    pub parent_receipt_digest: String,
    /// Producer-declared expected row count.
    pub expected_row_count: u32,
    /// Declared encoding; only `utf-8` is admitted.
    pub encoding: String,
    /// Declared newline policy; only `lf` is admitted.
    pub newline: String,
}

/// Decoded terminal frame facts retained by the receipt.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraceTerminal {
    /// Producer-declared row count.
    pub row_count: u32,
    /// SHA-256 over the concatenated raw row frame lines including LF.
    pub integrity_sha256: String,
    /// Typed terminal outcome of the traced runner process.
    pub completion: ProcessCompletion,
}

/// Validated subject references for one trace observation. These bind the
/// exact parent discovery subject; equal spelling from another run never
/// satisfies the relation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraceSubjectIdentity {
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
    /// Target contract identity the trace was captured for.
    pub target_id: String,
    /// SHA-256 of the pinned target selection contract.
    pub target_contract_digest: String,
    /// Environment-variant target when instrumented capture ran a variant.
    pub variant_target_id: Option<String>,
    /// Instrumentation subject of the capture.
    pub instrumentation_id: Option<String>,
    /// Trace session identity.
    pub trace_session_id: String,
    /// Capture identity of the parent runner process.
    pub parent_process_nonce: String,
    /// Parent discovery receipt payload digest.
    pub parent_receipt_digest: String,
}

/// Retained trace stream envelope. Bytes are lower-case hexadecimal so
/// arbitrary binary output survives JSON losslessly.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraceStreamEnvelope {
    /// Exact captured trace bytes, lower-case hexadecimal.
    pub bytes_hex: String,
    /// Producer assertion that capture was cut at the retention bound before
    /// the stream finished.
    pub truncated: bool,
}

impl TraceStreamEnvelope {
    /// Decoded captured bytes.
    pub fn bytes(&self) -> Result<Vec<u8>, String> {
        crate::observed_discovery::model::hex_decode(&self.bytes_hex)
    }

    /// Captured byte count.
    pub fn byte_len(&self) -> usize {
        self.bytes_hex.len() / 2
    }
}

/// Work accounting proven by this decoder and adapter. Unknown work is never
/// recorded as numeric zero; the four zero invariants are structural
/// properties re-proven during validation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraceWork {
    /// Retained trace bytes consumed by the decoder.
    pub trace_bytes_consumed: u64,
    /// Frames (header, rows, terminal) consumed by the decoder.
    pub trace_frames_consumed: u64,
    /// Row frames consumed by the decoder.
    pub trace_rows_consumed: u64,
    /// Rows deriving `observed_complete`.
    pub complete_rows: u64,
    /// Rows deriving `observed_partial`.
    pub partial_rows: u64,
    /// Rows with malformed frames.
    pub malformed_rows: u64,
    /// Rows with conflicting framing (duplicate, out-of-order, interleaved).
    pub conflicting_rows: u64,
    /// Rows deriving `runner_failed`.
    pub runner_failed_rows: u64,
    /// Rows deriving `instrument_failed`.
    pub instrument_failed_rows: u64,
    /// Rows deriving `subject_mismatch`.
    pub subject_mismatch_rows: u64,
    /// Rows deriving `not_proven`.
    pub not_proven_rows: u64,
    /// Fields observed across all rows.
    pub fields_observed: u64,
    /// Fields not applicable across all rows.
    pub fields_not_applicable: u64,
    /// Fields not observed across all rows.
    pub fields_not_observed: u64,
    /// Fields ambiguous across all rows.
    pub fields_ambiguous: u64,
    /// Fields malformed across all rows.
    pub fields_malformed: u64,
    /// Fields instrument-failed across all rows.
    pub fields_instrument_failed: u64,
    /// Canonical plan projections attempted by construction.
    pub canonical_plan_projections_attempted: u64,
    /// Canonical plan projections accepted by construction.
    pub canonical_plan_projections_accepted: u64,
    /// Canonical plan projections rejected by construction.
    pub canonical_plan_projections_rejected: u64,
    /// Structural invariant of this contract: always zero.
    pub source_reads: u64,
    /// Structural invariant of this contract: always zero.
    pub filesystem_scans: u64,
    /// Structural invariant of this contract: always zero.
    pub runner_processes: u64,
    /// Structural invariant of this contract: always zero.
    pub direct_probe_inputs: u64,
}

/// Full evidence payload bound by
/// [`EffectiveInvocationTraceReceiptV1::payload_digest`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TracePayload {
    /// Validated subject references for the observation.
    pub subject: TraceSubjectIdentity,
    /// Admitted upstream runner route of the traced process.
    pub runner: RunnerKind,
    /// Exact runner artifact/source identity.
    pub runner_artifact: RunnerArtifactIdentity,
    /// Decoded header frame facts.
    pub header: TraceHeader,
    /// Decoded terminal frame facts.
    pub terminal: Option<TraceTerminal>,
    /// Retained raw trace stream envelope.
    pub trace: TraceStreamEnvelope,
    /// Outcome of strictly decoding the trace stream.
    pub trace_decode: TraceStreamOutcome,
    /// Decoded rows in original observed order.
    pub rows: Vec<EffectiveInvocationRow>,
    /// Proven work accounting.
    pub work: TraceWork,
    /// Mandatory limitations retained verbatim.
    pub limitations: Vec<String>,
    /// Fixed claim boundary retained verbatim.
    pub claim_boundary: String,
}

/// Immutable versioned effective-invocation trace receipt.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectiveInvocationTraceReceiptV1 {
    /// Schema identity; always [`UPSTREAM_INVOCATION_TRACE_SCHEMA_VERSION`].
    pub schema_version: String,
    /// Always `instrumented_upstream`; every other class is rejected on
    /// validation.
    pub evidence_class: EvidenceClass,
    /// SHA-256 over the canonical serialization of `payload`.
    pub payload_digest: String,
    /// Full evidence payload.
    pub payload: TracePayload,
}

/// Supplied trace bytes plus validated subject references for one trace
/// construction. This is the only input shape a receipt can be built from:
/// the parent discovery receipt must itself pass subject-binding validation,
/// share the exact subject, own the parent process identity, and stay free of
/// trace contamination in its result streams.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservedInvocationTraceInput {
    /// Validated subject references for the observation.
    pub subject: TraceSubjectIdentity,
    /// Admitted upstream routes only (`test`, `harness`).
    pub runner: RunnerKind,
    /// Exact runner artifact/source identity.
    pub runner_artifact: RunnerArtifactIdentity,
    /// Parent observed-discovery receipt binding this trace's subject.
    pub parent_receipt: UpstreamDiscoveryReceiptV1,
    /// Exact raw trace bytes captured from the instrumented runner.
    pub trace_bytes: Vec<u8>,
    /// Whether trace capture was cut at the retention bound.
    pub trace_truncated: bool,
}
