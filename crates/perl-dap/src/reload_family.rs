//! Wire registration for the `perl-lsp/loadedModuleReload` custom DAP family
//! (version 1) — reload train R01B, issue #10138.
//!
//! This module owns the wire format only, under the frozen semantic contract
//! of #10097 (`reload`, ADR-0046) and the classification authority of
//! #6737/#4838 (`.ci/dap/protocol-authority.json`, `project_families`). The
//! frozen contract vocabulary in [`crate::reload`] is projected verbatim;
//! this module adds no semantics, invents no capability, and performs no
//! reload.
//!
//! Registration is transport and compatibility only:
//!
//! - the family is namespaced and versioned, never colliding with a standard
//!   DAP request name (pinned against the adapter's single supported-command
//!   authority, `debug_adapter::SUPPORTED_COMMANDS`);
//! - it is **not dispatched**: no route exists for the family request name,
//!   and a request for it receives the adapter's ordinary unknown-command
//!   response;
//! - it is **unadvertised**: no capability key mentions it until the R04
//!   exact-proof leaf lands;
//! - the wire request payload is the typed, adapter-issued opaque subject
//!   only; raw paths, debugger commands, and Perl expressions are refused;
//! - unknown fields, unknown enum variants, and unknown versions fail
//!   closed under the registry-recorded v1 policy (`reject-closed`);
//! - `indeterminate_possibly_applied` is never flattened to a clean or
//!   empty failure, and a post-boundary unknown outcome still advances the
//!   runtime-module generation on the wire exactly as the contract demands.
//!
//! The Rust wire types here, the generated TypeScript projection
//! (`vscode-extension/src/loadedModuleReloadFamily.generated.ts`), the wire
//! schema (`schemas/loaded_module_reload_family.v1.schema.json`), the
//! registry entry, and the canonical JSON vectors
//! (`.spec/10138-loaded-module-reload-family/fixtures/`) are mechanically
//! synchronized by the tests in this module; none is an independently
//! edited authority.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::reload::{
    IndeterminateCause, LoadedModuleReloadEligibility, LoadedModuleReloadOutcome,
    PreMutationFailureCause, ReloadTransactionPhase, RuntimeModuleGeneration,
    RuntimeModuleGenerationClock,
};

/// The registered custom family identity: a non-empty namespace, the `/`
/// separator, and a non-empty local name (ADR-0046 §6).
pub const LOADED_MODULE_RELOAD_FAMILY: &str = "perl-lsp/loadedModuleReload";

/// The single DAP request name of family version 1. It equals the family
/// identity, is namespaced, and is deliberately absent from
/// `debug_adapter::SUPPORTED_COMMANDS`.
pub const LOADED_MODULE_RELOAD_REQUEST: &str = "perl-lsp/loadedModuleReload";

/// The registered family version.
pub const LOADED_MODULE_RELOAD_FAMILY_VERSION: u32 = 1;

// --- Registry-recorded bounds (enforced before publication) ---------------

/// Maximum serialized request size accepted before any interpretation.
pub const MAX_REQUEST_BYTES: usize = 8192;
/// Maximum length of an opaque identity token (module identity, source URI).
pub const MAX_IDENTITY_CHARS: usize = 256;
/// Maximum length of a saved-source digest token.
pub const MAX_DIGEST_CHARS: usize = 128;
/// Maximum number of bounded reason codes carried on one response.
pub const MAX_REASONS: usize = 16;
/// Maximum length of one reason code.
pub const MAX_REASON_CHARS: usize = 96;
/// Maximum length of a remediation/detail code before redaction.
pub const MAX_DETAIL_CHARS: usize = 256;
/// Maximum retained recent operation identities per session.
pub const MAX_RETAINED_OPERATIONS: usize = 64;
/// Lower deadline bound (milliseconds).
pub const MIN_DEADLINE_MS: u64 = 100;
/// Upper deadline bound (milliseconds).
pub const MAX_DEADLINE_MS: u64 = 60_000;

/// Marker reason code appended when a reason list is clamped to the bound.
pub const REASONS_TRUNCATED_MARKER: &str = "reasons_truncated";
/// Marker remediation code substituted when a detail exceeds the bound.
pub const DETAIL_REDACTED_MARKER: &str = "detail_redacted";

/// The closed request key set; anything else is unknown and, if it names a
/// raw client input channel, raw.
const REQUEST_KEYS: [&str; 6] =
    ["family", "familyVersion", "sessionEpoch", "operationId", "subject", "deadlineMs"];

/// The closed subject key set.
const SUBJECT_KEYS: [&str; 4] =
    ["moduleIdentity", "savedSourceDigest", "logicalSourceUri", "observationGeneration"];

/// Key spellings that can only be attempts to smuggle raw client input
/// (paths, `%INC` keys, package names, commands, Perl, source bytes, or
/// environment) into the typed-subject payload.
const RAW_INPUT_KEYS: [&str; 22] = [
    "path",
    "modulePath",
    "runtimePath",
    "filename",
    "basename",
    "incKey",
    "inc",
    "package",
    "packageName",
    "command",
    "expression",
    "perl",
    "shell",
    "argv",
    "env",
    "environment",
    "source",
    "sourceText",
    "sourceBytes",
    "replacement",
    "replacementSource",
    "debuggerCommand",
];

// --- Frozen vocabulary projections -----------------------------------------

/// Wire projection of the frozen terminal outcome kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WireOutcomeKind {
    /// The runtime accepted and read back the replacement.
    #[serde(rename = "reloaded")]
    Reloaded,
    /// Admission refused before anything was attempted.
    #[serde(rename = "refused")]
    Refused,
    /// Deterministic failure before the mutation boundary.
    #[serde(rename = "failed_before_mutation")]
    FailedBeforeMutation,
    /// Post-boundary unknown outcome; possibly applied, never clean.
    #[serde(rename = "indeterminate_possibly_applied")]
    IndeterminatePossiblyApplied,
}

impl From<&LoadedModuleReloadOutcome> for WireOutcomeKind {
    fn from(outcome: &LoadedModuleReloadOutcome) -> Self {
        match outcome {
            LoadedModuleReloadOutcome::Reloaded => WireOutcomeKind::Reloaded,
            LoadedModuleReloadOutcome::Refused { .. } => WireOutcomeKind::Refused,
            LoadedModuleReloadOutcome::FailedBeforeMutation { .. } => {
                WireOutcomeKind::FailedBeforeMutation
            }
            LoadedModuleReloadOutcome::IndeterminatePossiblyApplied { .. } => {
                WireOutcomeKind::IndeterminatePossiblyApplied
            }
        }
    }
}

impl WireOutcomeKind {
    /// The frozen code, identical to `LoadedModuleReloadOutcome::kind_code`.
    pub const fn as_str(self) -> &'static str {
        match self {
            WireOutcomeKind::Reloaded => "reloaded",
            WireOutcomeKind::Refused => "refused",
            WireOutcomeKind::FailedBeforeMutation => "failed_before_mutation",
            WireOutcomeKind::IndeterminatePossiblyApplied => "indeterminate_possibly_applied",
        }
    }

    /// Whether a body of this kind may carry DAP success. Only `reloaded`
    /// is a clean terminal success; an indeterminate outcome never is.
    pub const fn permits_success(self) -> bool {
        matches!(self, WireOutcomeKind::Reloaded)
    }
}

/// Wire projection of the frozen transaction phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WirePhase {
    /// Eligibility and identity admission.
    #[serde(rename = "admission")]
    Admission,
    /// Readiness, authority, and saved-source preflight.
    #[serde(rename = "preflight")]
    Preflight,
    /// Prepare the bounded transaction (no runtime mutation yet).
    #[serde(rename = "prepare")]
    Prepare,
    /// The boundary: runtime mutation begins.
    #[serde(rename = "runtime_mutation_begins")]
    RuntimeMutationBegins,
    /// Runtime acknowledgement / read-back.
    #[serde(rename = "runtime_acknowledgement_read_back")]
    RuntimeAcknowledgementReadBack,
    /// Commit the runtime-module generation.
    #[serde(rename = "commit_generation")]
    CommitGeneration,
    /// Post-reload reconciliation of invalidated state.
    #[serde(rename = "post_reload_reconciliation")]
    PostReloadReconciliation,
    /// Terminal projection to the client.
    #[serde(rename = "terminal_projection")]
    TerminalProjection,
}

impl From<ReloadTransactionPhase> for WirePhase {
    fn from(phase: ReloadTransactionPhase) -> Self {
        match phase {
            ReloadTransactionPhase::Admission => WirePhase::Admission,
            ReloadTransactionPhase::Preflight => WirePhase::Preflight,
            ReloadTransactionPhase::Prepare => WirePhase::Prepare,
            ReloadTransactionPhase::RuntimeMutationBegins => WirePhase::RuntimeMutationBegins,
            ReloadTransactionPhase::RuntimeAcknowledgementReadBack => {
                WirePhase::RuntimeAcknowledgementReadBack
            }
            ReloadTransactionPhase::CommitGeneration => WirePhase::CommitGeneration,
            ReloadTransactionPhase::PostReloadReconciliation => WirePhase::PostReloadReconciliation,
            ReloadTransactionPhase::TerminalProjection => WirePhase::TerminalProjection,
        }
    }
}

/// Wire projection of the frozen refusal dispositions (the twelve
/// non-admitted eligibility classes; the admitted class is never a refusal).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WireDisposition {
    /// The subject is not present in the current `%INC` observation.
    #[serde(rename = "not_loaded")]
    NotLoaded,
    /// The subject identity is not exact or no longer current.
    #[serde(rename = "source_not_exact_or_stale")]
    SourceNotExactOrStale,
    /// The client-declared source revision does not match the saved disk
    /// source.
    #[serde(rename = "dirty_or_unsaved_source")]
    DirtyOrUnsavedSource,
    /// An active frame is executing in the target module.
    #[serde(rename = "active_frame_in_target")]
    ActiveFrameInTarget,
    /// The subject is the debuggee's main program, not a loadable module.
    #[serde(rename = "main_program_not_module")]
    MainProgramNotModule,
    /// XS or native-linked module.
    #[serde(rename = "xs_or_native_module")]
    XsOrNativeModule,
    /// Source filter or compile-hook boundary module.
    #[serde(rename = "source_filter_or_compile_hook_boundary")]
    SourceFilterOrCompileHookBoundary,
    /// Generated or eval-produced source.
    #[serde(rename = "generated_or_eval_source")]
    GeneratedOrEvalSource,
    /// The runtime mapping cannot bind exactly one subject.
    #[serde(rename = "ambiguous_runtime_mapping")]
    AmbiguousRuntimeMapping,
    /// The resolved subject path lies outside the validated launch root.
    #[serde(rename = "outside_launch_authority")]
    OutsideLaunchAuthority,
    /// The selected runtime does not support the reload mechanism family.
    #[serde(rename = "unsupported_runtime")]
    UnsupportedRuntime,
    /// The debuggee is not stopped or not command-ready.
    #[serde(rename = "not_stopped_or_not_command_ready")]
    NotStoppedOrNotCommandReady,
}

impl TryFrom<LoadedModuleReloadEligibility> for WireDisposition {
    type Error = ();

    fn try_from(eligibility: LoadedModuleReloadEligibility) -> Result<Self, Self::Error> {
        let disposition = match eligibility {
            LoadedModuleReloadEligibility::EligibleSourceBackedPerlModule => {
                return Err(());
            }
            LoadedModuleReloadEligibility::NotLoaded => WireDisposition::NotLoaded,
            LoadedModuleReloadEligibility::SourceNotExactOrStale => {
                WireDisposition::SourceNotExactOrStale
            }
            LoadedModuleReloadEligibility::DirtyOrUnsavedSource => {
                WireDisposition::DirtyOrUnsavedSource
            }
            LoadedModuleReloadEligibility::ActiveFrameInTarget => {
                WireDisposition::ActiveFrameInTarget
            }
            LoadedModuleReloadEligibility::MainProgramNotModule => {
                WireDisposition::MainProgramNotModule
            }
            LoadedModuleReloadEligibility::XsOrNativeModule => WireDisposition::XsOrNativeModule,
            LoadedModuleReloadEligibility::SourceFilterOrCompileHookBoundary => {
                WireDisposition::SourceFilterOrCompileHookBoundary
            }
            LoadedModuleReloadEligibility::GeneratedOrEvalSource => {
                WireDisposition::GeneratedOrEvalSource
            }
            LoadedModuleReloadEligibility::AmbiguousRuntimeMapping => {
                WireDisposition::AmbiguousRuntimeMapping
            }
            LoadedModuleReloadEligibility::OutsideLaunchAuthority => {
                WireDisposition::OutsideLaunchAuthority
            }
            LoadedModuleReloadEligibility::UnsupportedRuntime => {
                WireDisposition::UnsupportedRuntime
            }
            LoadedModuleReloadEligibility::NotStoppedOrNotCommandReady => {
                WireDisposition::NotStoppedOrNotCommandReady
            }
        };
        Ok(disposition)
    }
}

/// Wire projection of the frozen pre-mutation and indeterminate causes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WireFailureCause {
    /// Prepare failed deterministically before any mutation was issued.
    #[serde(rename = "prepare_failed")]
    PrepareFailed,
    /// The operation was cancelled before the boundary.
    #[serde(rename = "cancelled_before_mutation_began")]
    CancelledBeforeMutationBegan,
    /// The framed acknowledgement timed out after mutation began.
    #[serde(rename = "timeout_after_mutation_began")]
    TimeoutAfterMutationBegan,
    /// The transport was lost after mutation began.
    #[serde(rename = "transport_loss_after_mutation_began")]
    TransportLossAfterMutationBegan,
    /// The acknowledgement was ambiguous.
    #[serde(rename = "ambiguous_acknowledgement")]
    AmbiguousAcknowledgement,
    /// The read-back could not establish the post-mutation state.
    #[serde(rename = "read_back_inconclusive")]
    ReadBackInconclusive,
}

impl From<PreMutationFailureCause> for WireFailureCause {
    fn from(cause: PreMutationFailureCause) -> Self {
        match cause {
            PreMutationFailureCause::PrepareFailed => WireFailureCause::PrepareFailed,
            PreMutationFailureCause::CancelledBeforeMutationBegan => {
                WireFailureCause::CancelledBeforeMutationBegan
            }
        }
    }
}

impl From<IndeterminateCause> for WireFailureCause {
    fn from(cause: IndeterminateCause) -> Self {
        match cause {
            IndeterminateCause::TimeoutAfterMutationBegan => {
                WireFailureCause::TimeoutAfterMutationBegan
            }
            IndeterminateCause::TransportLossAfterMutationBegan => {
                WireFailureCause::TransportLossAfterMutationBegan
            }
            IndeterminateCause::AmbiguousAcknowledgement => {
                WireFailureCause::AmbiguousAcknowledgement
            }
            IndeterminateCause::ReadBackInconclusive => WireFailureCause::ReadBackInconclusive,
        }
    }
}

/// Wire projection of the closed typed request-rejection codes (transport
/// admission only; never a transaction outcome).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WireRejectionCode {
    /// The client never negotiated the family for this session.
    #[serde(rename = "family_not_negotiated")]
    FamilyNotNegotiated,
    /// The request names a different family identity.
    #[serde(rename = "family_name_mismatch")]
    FamilyNameMismatch,
    /// No mutually known family version exists.
    #[serde(rename = "family_version_unsupported")]
    FamilyVersionUnsupported,
    /// The request belongs to a replaced session epoch.
    #[serde(rename = "session_stale")]
    SessionStale,
    /// The operation identity was already terminal in this epoch.
    #[serde(rename = "operation_stale")]
    OperationStale,
    /// The operation identity is zero.
    #[serde(rename = "operation_id_invalid")]
    OperationIdInvalid,
    /// An unknown request field failed closed under the v1 policy.
    #[serde(rename = "unknown_field_rejected")]
    UnknownFieldRejected,
    /// An unknown mandatory enum variant failed closed.
    #[serde(rename = "unknown_variant_rejected")]
    UnknownVariantRejected,
    /// The request carried raw client input (path, command, or expression).
    #[serde(rename = "raw_client_input_refused")]
    RawClientInputRefused,
    /// The typed subject identity was missing or incomplete.
    #[serde(rename = "subject_identity_insufficient")]
    SubjectIdentityInsufficient,
    /// The serialized request exceeded the registry byte bound.
    #[serde(rename = "payload_too_large")]
    PayloadTooLarge,
    /// An identity token exceeded the registry length bound.
    #[serde(rename = "identity_too_large")]
    IdentityTooLarge,
    /// A detail exceeded the registry bound and could not be published.
    #[serde(rename = "detail_too_large")]
    DetailTooLarge,
    /// The deadline hint fell outside the registry bounds.
    #[serde(rename = "deadline_out_of_range")]
    DeadlineOutOfRange,
    /// The session has no reload mechanism backing (version 1 always).
    #[serde(rename = "family_not_backed_for_session")]
    FamilyNotBackedForSession,
    /// The request document was not a well-formed family request.
    #[serde(rename = "malformed_request")]
    MalformedRequest,
}

impl WireRejectionCode {
    /// The frozen wire code for this rejection.
    pub const fn as_str(self) -> &'static str {
        match self {
            WireRejectionCode::FamilyNotNegotiated => "family_not_negotiated",
            WireRejectionCode::FamilyNameMismatch => "family_name_mismatch",
            WireRejectionCode::FamilyVersionUnsupported => "family_version_unsupported",
            WireRejectionCode::SessionStale => "session_stale",
            WireRejectionCode::OperationStale => "operation_stale",
            WireRejectionCode::OperationIdInvalid => "operation_id_invalid",
            WireRejectionCode::UnknownFieldRejected => "unknown_field_rejected",
            WireRejectionCode::UnknownVariantRejected => "unknown_variant_rejected",
            WireRejectionCode::RawClientInputRefused => "raw_client_input_refused",
            WireRejectionCode::SubjectIdentityInsufficient => "subject_identity_insufficient",
            WireRejectionCode::PayloadTooLarge => "payload_too_large",
            WireRejectionCode::IdentityTooLarge => "identity_too_large",
            WireRejectionCode::DetailTooLarge => "detail_too_large",
            WireRejectionCode::DeadlineOutOfRange => "deadline_out_of_range",
            WireRejectionCode::FamilyNotBackedForSession => "family_not_backed_for_session",
            WireRejectionCode::MalformedRequest => "malformed_request",
        }
    }
}

// --- Wire documents ---------------------------------------------------------

/// The typed, adapter-issued opaque subject. This is the only admissible
/// request payload shape (ADR-0046 §6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WireSubject {
    /// Opaque loaded-module identity issued by the adapter.
    pub module_identity: String,
    /// Saved-source content digest token.
    pub saved_source_digest: String,
    /// Adapter-issued overlay/editor logical source identity.
    pub logical_source_uri: String,
    /// Loaded-source observation generation the identity was issued under.
    pub observation_generation: u64,
}

/// The family request body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LoadedModuleReloadWireRequest {
    /// Must equal [`LOADED_MODULE_RELOAD_FAMILY`].
    pub family: String,
    /// Must equal the negotiated family version.
    pub family_version: u32,
    /// Adapter-issued session epoch selector.
    pub session_epoch: u64,
    /// Client request and reload operation identity (non-zero, unique per
    /// epoch).
    pub operation_id: u64,
    /// The typed subject.
    pub subject: WireSubject,
    /// Optional cancellation/deadline hint in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline_ms: Option<u64>,
}

/// Runtime-module generation witness carried where permitted. Both terminal
/// mutation outcomes (`reloaded` and `indeterminate_possibly_applied`)
/// advance the generation; the witness states the advance rather than the
/// runtime read-back state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WireGenerationWitness {
    /// The generation before the transaction applied its effect.
    pub previous: u64,
    /// The generation after the transaction applied its effect.
    pub current: u64,
    /// Whether the terminal outcome required an advance.
    pub advanced: bool,
}

/// Reconciliation dispositions carried separately from the terminal
/// outcome. Version 1 registers the surface with the `deferred`
/// disposition; the real dispositions are #10102's (R03) to carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireReconciliation {
    /// Loaded-source refresh disposition.
    #[serde(rename = "loaded_source_refresh")]
    pub loaded_source_refresh: WireReconciliationDisposition,
    /// Inspection invalidation disposition.
    #[serde(rename = "inspection_invalidation")]
    pub inspection_invalidation: WireReconciliationDisposition,
    /// Breakpoint reconciliation disposition (durable desired
    /// configuration is preserved and reconciled, never invalidated).
    #[serde(rename = "breakpoint_reconciliation")]
    pub breakpoint_reconciliation: WireReconciliationDisposition,
}

impl Default for WireReconciliation {
    fn default() -> Self {
        WireReconciliation {
            loaded_source_refresh: WireReconciliationDisposition::Deferred,
            inspection_invalidation: WireReconciliationDisposition::Deferred,
            breakpoint_reconciliation: WireReconciliationDisposition::Deferred,
        }
    }
}

/// Closed v1 reconciliation disposition vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WireReconciliationDisposition {
    /// The disposition surface is registered; the real disposition arrives
    /// with the R03 composition leaf (#10102).
    #[serde(rename = "deferred")]
    Deferred,
}

/// The terminal transaction result body. Field names follow DAP camelCase;
/// the reconciliation sub-object keeps the frozen semantic surface names.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadedModuleReloadOutcomeBody {
    /// The frozen terminal kind, projected verbatim.
    pub kind: WireOutcomeKind,
    /// The transaction phase reached.
    pub phase: WirePhase,
    /// The refusal disposition (present for `refused` only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disposition: Option<WireDisposition>,
    /// The failure cause (present for `failed_before_mutation` and
    /// `indeterminate_possibly_applied`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cause: Option<WireFailureCause>,
    /// Whether the runtime may have mutated. True exactly for
    /// `indeterminate_possibly_applied`.
    pub possibly_applied: bool,
    /// The generation witness, where permitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<WireGenerationWitness>,
    /// Reconciliation dispositions carried separately.
    #[serde(default)]
    pub reconciliation: WireReconciliation,
    /// Bounded reason codes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<String>,
    /// Bounded remediation identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remediation: Option<String>,
}

/// Discriminator for the typed request-rejection body: always
/// `request_rejected`, never a transaction outcome kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WireRejectionMarker {
    /// The body discriminates a typed request rejection.
    #[serde(rename = "request_rejected")]
    RequestRejected,
}

/// The typed request-rejection body (transport admission; never a
/// transaction outcome).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadedModuleReloadRejectionBody {
    /// Discriminator: always `request_rejected`.
    pub kind: WireRejectionMarker,
    /// The typed fail-closed code.
    pub code: WireRejectionCode,
    /// Bounded reason codes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<String>,
}

impl LoadedModuleReloadRejectionBody {
    fn new(code: WireRejectionCode) -> Self {
        LoadedModuleReloadRejectionBody {
            kind: WireRejectionMarker::RequestRejected,
            code,
            reasons: Vec::new(),
        }
    }
}

/// One family response: the DAP success flag, the correlated operation
/// identity, and the typed body. The operation identity is carried on
/// every request/response pair (ADR-0046 §6 correlation requirement and
/// the registry's `operation-id-on-every-request-response-pair` rule).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadedModuleReloadWireResponse {
    /// DAP-level success. Only `reloaded` is success; an indeterminate
    /// outcome is never success even though its body is fully typed.
    pub success: bool,
    /// The correlated client request and reload operation identity.
    pub operation_id: u64,
    /// The typed body.
    pub body: LoadedModuleReloadResponseBody,
}

/// The response body union.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LoadedModuleReloadResponseBody {
    /// A terminal transaction result.
    Outcome(LoadedModuleReloadOutcomeBody),
    /// A typed request rejection.
    Rejected(LoadedModuleReloadRejectionBody),
}

/// Fail-closed client-side classification of a response kind, mirrored by
/// the generated TypeScript projection. Only `reloaded` without
/// `possibly_applied` is clean; unknown or contradictory bodies never are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireTerminalClassification {
    /// Clean terminal success.
    ReloadedClean,
    /// Refusal: clean failure, nothing attempted.
    RefusedCleanFailure,
    /// Pre-mutation failure: clean failure, nothing mutated.
    FailedBeforeMutationCleanFailure,
    /// Possibly applied: never clean, never an ordinary failure.
    PossiblyApplied,
    /// Unknown or contradictory body: fail closed.
    UnknownFailClosed,
}

/// Classify a response kind the way a conforming client must. The kind and
/// the `possibly_applied` flag must agree: a body claiming a clean
/// refusal/pre-mutation failure while asserting `possibly_applied` is
/// contradictory and fails closed, exactly like an unknown kind. The
/// indeterminate kind stays authoritative on its own (the flag is
/// redundant there by construction), so it can never be demoted to an
/// ordinary failure by a lying field.
pub fn classify_wire_terminal(kind: &str, possibly_applied: bool) -> WireTerminalClassification {
    match kind {
        "reloaded" if !possibly_applied => WireTerminalClassification::ReloadedClean,
        "refused" if !possibly_applied => WireTerminalClassification::RefusedCleanFailure,
        "failed_before_mutation" if !possibly_applied => {
            WireTerminalClassification::FailedBeforeMutationCleanFailure
        }
        "indeterminate_possibly_applied" => WireTerminalClassification::PossiblyApplied,
        _ => WireTerminalClassification::UnknownFailClosed,
    }
}

// --- Negotiation ------------------------------------------------------------

/// A client's declared family support (or its absence).
#[derive(Debug, Clone, PartialEq)]
pub struct ClientFamilyDeclaration {
    /// The family the client declares.
    pub family: String,
    /// The versions the client can speak, highest first is not required.
    pub versions: Vec<u32>,
}

/// Why negotiation did not produce a usable family version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FamilyNegotiationRefusal {
    /// The client declared no family support at all.
    FamilyAbsent,
    /// The client declared a different family identity.
    FamilyNameMismatch,
    /// No mutually known version exists.
    NoOverlappingVersion,
}

impl FamilyNegotiationRefusal {
    /// The wire code matching the refusal, for diagnostics and vectors.
    pub const fn wire_code(self) -> WireRejectionCode {
        match self {
            FamilyNegotiationRefusal::FamilyAbsent => WireRejectionCode::FamilyNotNegotiated,
            FamilyNegotiationRefusal::FamilyNameMismatch => WireRejectionCode::FamilyNameMismatch,
            FamilyNegotiationRefusal::NoOverlappingVersion => {
                WireRejectionCode::FamilyVersionUnsupported
            }
        }
    }
}

/// Adapter-side per-session family state. A session restart or replacement
/// constructs a fresh session (new epoch); prior family and operation
/// identities never survive it.
pub struct ReloadFamilySession {
    epoch: u64,
    negotiated_version: Option<u32>,
    backed: bool,
    recent_operations: VecDeque<u64>,
}

impl ReloadFamilySession {
    /// A session with the given epoch and mechanism backing. Production
    /// version 1 constructs sessions with `backed = false`: registration is
    /// not behavior, and no reload mechanism exists yet (#10098 is R02).
    pub fn new(epoch: u64, backed: bool) -> ReloadFamilySession {
        ReloadFamilySession {
            epoch,
            negotiated_version: None,
            backed,
            recent_operations: VecDeque::new(),
        }
    }

    /// Negotiate the family against a client declaration, selecting the
    /// highest mutually known version. Fails closed for an absent
    /// declaration, a different family identity, or no overlapping version.
    pub fn negotiate(
        &mut self,
        declaration: Option<&ClientFamilyDeclaration>,
    ) -> Result<u32, FamilyNegotiationRefusal> {
        let declaration = match declaration {
            Some(declaration) => declaration,
            None => return Err(FamilyNegotiationRefusal::FamilyAbsent),
        };
        if declaration.family != LOADED_MODULE_RELOAD_FAMILY {
            return Err(FamilyNegotiationRefusal::FamilyNameMismatch);
        }
        let mutual = declaration
            .versions
            .iter()
            .copied()
            .filter(|version| *version >= 1 && *version <= LOADED_MODULE_RELOAD_FAMILY_VERSION)
            .max();
        match mutual {
            Some(version) => {
                self.negotiated_version = Some(version);
                Ok(version)
            }
            None => Err(FamilyNegotiationRefusal::NoOverlappingVersion),
        }
    }

    /// Evaluate one wire request against this session, fail-closed at every
    /// gate and before any backend action (of which version 1 has none).
    ///
    /// Gate precedence (registry-recorded): payload bound, family identity,
    /// family version, negotiated presence, session epoch, key set, shape,
    /// subject identity, operation identity, deadline, mechanism backing.
    pub fn evaluate(&mut self, raw: &Value) -> ReloadRequestEvaluation {
        // The operation identity travels on every rejection too (0 when the
        // request carried nothing parseable — such requests correlate via
        // the DAP request sequence instead).
        let operation_id = raw
            .as_object()
            .and_then(|object| object.get("operationId"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let reject = move |code: WireRejectionCode| {
            ReloadRequestEvaluation::Response(LoadedModuleReloadWireResponse {
                success: false,
                operation_id,
                body: LoadedModuleReloadResponseBody::Rejected(
                    LoadedModuleReloadRejectionBody::new(code),
                ),
            })
        };

        if serde_json::to_string(raw).map(|text| text.len()).unwrap_or(MAX_REQUEST_BYTES + 1)
            > MAX_REQUEST_BYTES
        {
            return reject(WireRejectionCode::PayloadTooLarge);
        }

        let object = match raw.as_object() {
            Some(object) => object,
            None => return reject(WireRejectionCode::MalformedRequest),
        };

        let declared_family = match object.get("family").and_then(Value::as_str) {
            Some(family) => family,
            None => return reject(WireRejectionCode::MalformedRequest),
        };
        if declared_family != LOADED_MODULE_RELOAD_FAMILY {
            return reject(WireRejectionCode::FamilyNameMismatch);
        }
        let declared_version = match object.get("familyVersion").and_then(Value::as_u64) {
            Some(version) => version,
            None => return reject(WireRejectionCode::MalformedRequest),
        };
        if declared_version != u64::from(LOADED_MODULE_RELOAD_FAMILY_VERSION) {
            return reject(WireRejectionCode::FamilyVersionUnsupported);
        }
        if self.negotiated_version != Some(LOADED_MODULE_RELOAD_FAMILY_VERSION) {
            return reject(WireRejectionCode::FamilyNotNegotiated);
        }
        let request_epoch = match object.get("sessionEpoch").and_then(Value::as_u64) {
            Some(epoch) => epoch,
            None => return reject(WireRejectionCode::MalformedRequest),
        };
        if request_epoch != self.epoch {
            return reject(WireRejectionCode::SessionStale);
        }

        for key in object.keys() {
            if RAW_INPUT_KEYS.contains(&key.as_str()) {
                return reject(WireRejectionCode::RawClientInputRefused);
            }
            if !REQUEST_KEYS.contains(&key.as_str()) {
                return reject(WireRejectionCode::UnknownFieldRejected);
            }
        }
        let subject_object = match object.get("subject").and_then(Value::as_object) {
            Some(subject) => subject,
            None => return reject(WireRejectionCode::MalformedRequest),
        };
        for key in subject_object.keys() {
            if RAW_INPUT_KEYS.contains(&key.as_str()) {
                return reject(WireRejectionCode::RawClientInputRefused);
            }
            if !SUBJECT_KEYS.contains(&key.as_str()) {
                return reject(WireRejectionCode::UnknownFieldRejected);
            }
        }

        let request = match serde_json::from_value::<LoadedModuleReloadWireRequest>(raw.clone()) {
            Ok(request) => request,
            Err(_) => return reject(WireRejectionCode::MalformedRequest),
        };

        let subject = request.subject;
        if subject.module_identity.trim().is_empty()
            || subject.saved_source_digest.trim().is_empty()
            || subject.logical_source_uri.trim().is_empty()
        {
            return reject(WireRejectionCode::SubjectIdentityInsufficient);
        }
        if subject.module_identity.chars().count() > MAX_IDENTITY_CHARS
            || subject.logical_source_uri.chars().count() > MAX_IDENTITY_CHARS
        {
            return reject(WireRejectionCode::IdentityTooLarge);
        }
        if subject.saved_source_digest.chars().count() > MAX_DIGEST_CHARS {
            return reject(WireRejectionCode::IdentityTooLarge);
        }

        if request.operation_id == 0 {
            return reject(WireRejectionCode::OperationIdInvalid);
        }
        if self.recent_operations.contains(&request.operation_id) {
            return reject(WireRejectionCode::OperationStale);
        }
        if let Some(deadline) = request.deadline_ms
            && !(MIN_DEADLINE_MS..=MAX_DEADLINE_MS).contains(&deadline)
        {
            return reject(WireRejectionCode::DeadlineOutOfRange);
        }

        if !self.backed {
            return reject(WireRejectionCode::FamilyNotBackedForSession);
        }

        self.recent_operations.push_back(request.operation_id);
        if self.recent_operations.len() > MAX_RETAINED_OPERATIONS {
            self.recent_operations.pop_front();
        }
        ReloadRequestEvaluation::Admitted { operation_id: request.operation_id }
    }
}

/// The result of evaluating one wire request.
#[derive(Debug, Clone, PartialEq)]
pub enum ReloadRequestEvaluation {
    /// The request passed every gate and awaits a terminal outcome from the
    /// (R02) mechanism; version 1 production sessions never reach this.
    Admitted {
        /// The admitted operation identity.
        operation_id: u64,
    },
    /// A typed fail-closed response with no backend action.
    Response(LoadedModuleReloadWireResponse),
}

// --- Outcome projection -----------------------------------------------------

/// Project one frozen contract outcome onto the wire body, applying the
/// generation clock exactly as the contract demands.
///
/// The terminal outcome is projected verbatim: `reloaded` is the only
/// success; `indeterminate_possibly_applied` carries
/// `possibly_applied = true`, never DAP success, and the generation
/// advanced; refusals and pre-mutation failures change nothing. Bounded
/// reason codes are clamped (with the `reasons_truncated` marker) and an
/// over-bound remediation detail is replaced by the content-free
/// `detail_redacted` code before publication.
pub fn project_outcome(
    outcome: &LoadedModuleReloadOutcome,
    operation_id: u64,
    clock: &mut RuntimeModuleGenerationClock,
    reasons: &[String],
    remediation: Option<&str>,
) -> Result<LoadedModuleReloadWireResponse, WireProjectionRefusal> {
    let kind = WireOutcomeKind::from(outcome);
    let phase = match outcome {
        LoadedModuleReloadOutcome::Reloaded => WirePhase::TerminalProjection,
        LoadedModuleReloadOutcome::Refused { .. } => WirePhase::Admission,
        LoadedModuleReloadOutcome::FailedBeforeMutation { phase, .. } => WirePhase::from(*phase),
        LoadedModuleReloadOutcome::IndeterminatePossiblyApplied { phase, .. } => {
            WirePhase::from(*phase)
        }
    };

    // Fail closed before the clock moves: an outcome whose phase/kind
    // pairing the frozen contract does not permit (for example a
    // `failed_before_mutation` carrying a phase at or after the mutation
    // boundary) can never be published as a clean pre-mutation failure.
    if !crate::reload::phase_permits_outcome(
        match outcome {
            LoadedModuleReloadOutcome::Reloaded => ReloadTransactionPhase::TerminalProjection,
            LoadedModuleReloadOutcome::Refused { .. } => ReloadTransactionPhase::Admission,
            LoadedModuleReloadOutcome::FailedBeforeMutation { phase, .. } => *phase,
            LoadedModuleReloadOutcome::IndeterminatePossiblyApplied { phase, .. } => *phase,
        },
        outcome,
    ) {
        return Err(WireProjectionRefusal::OutcomePhaseKindMismatch);
    }

    let previous = generation_value(clock.current());
    let advance = clock.apply(outcome);
    let current = generation_value(advance.generation());

    let disposition = match outcome {
        LoadedModuleReloadOutcome::Refused { disposition } => {
            WireDisposition::try_from(*disposition).ok()
        }
        _ => None,
    };
    let cause = match outcome {
        LoadedModuleReloadOutcome::FailedBeforeMutation { cause, .. } => {
            Some(WireFailureCause::from(*cause))
        }
        LoadedModuleReloadOutcome::IndeterminatePossiblyApplied { cause, .. } => {
            Some(WireFailureCause::from(*cause))
        }
        _ => None,
    };

    let possibly_applied =
        matches!(outcome, LoadedModuleReloadOutcome::IndeterminatePossiblyApplied { .. });

    let body = LoadedModuleReloadOutcomeBody {
        kind,
        phase,
        disposition,
        cause,
        possibly_applied,
        generation: Some(WireGenerationWitness { previous, current, advanced: advance.advanced() }),
        reconciliation: WireReconciliation::default(),
        reasons: clamp_reasons(reasons),
        remediation: redact_remediation(remediation),
    };
    Ok(LoadedModuleReloadWireResponse {
        success: kind.permits_success(),
        operation_id,
        body: LoadedModuleReloadResponseBody::Outcome(body),
    })
}

/// Why an outcome cannot be projected onto the wire at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireProjectionRefusal {
    /// The outcome's phase/kind pairing is not permitted by the frozen
    /// contract (`phase_permits_outcome`); publishing it would serialize a
    /// contradictory terminal body.
    OutcomePhaseKindMismatch,
}

impl std::fmt::Display for WireProjectionRefusal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WireProjectionRefusal::OutcomePhaseKindMismatch => {
                formatter.write_str("outcome_phase_kind_mismatch")
            }
        }
    }
}

impl std::error::Error for WireProjectionRefusal {}

/// Read the numeric value of an opaque generation without assuming its
/// representation beyond the contract's monotonic origin.
fn generation_value(generation: RuntimeModuleGeneration) -> u64 {
    RuntimeModuleGeneration::INITIAL.distance_to(generation).unwrap_or_default()
}

/// Whether a bounded code satisfies the registry grammar (lowercase
/// snake_case, non-empty): the only shape a reason or remediation may
/// travel in.
fn is_bounded_code(code: &str, max_chars: usize) -> bool {
    !code.is_empty()
        && code.chars().count() <= max_chars
        && code.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
        })
}

/// Clamp a reason list to the registry bound and grammar, carrying the
/// truncation marker instead of silently dropping overflow, and redacting
/// any entry that is not a bounded code (a raw path, free text, or an
/// over-long value never reaches the wire).
fn clamp_reasons(reasons: &[String]) -> Vec<String> {
    let bounded: Vec<String> = reasons
        .iter()
        .map(|reason| {
            if is_bounded_code(reason, MAX_REASON_CHARS) {
                reason.clone()
            } else {
                DETAIL_REDACTED_MARKER.to_string()
            }
        })
        .collect();
    if bounded.len() <= MAX_REASONS {
        return bounded;
    }
    let kept = bounded.into_iter().take(MAX_REASONS.saturating_sub(1));
    let mut clamped: Vec<String> = kept.collect();
    clamped.push(REASONS_TRUNCATED_MARKER.to_string());
    clamped
}

/// Redact a remediation detail to the content-free marker unless it is a
/// bounded code; the code surface never echoes private paths, source
/// text, or runtime output, however short.
fn redact_remediation(remediation: Option<&str>) -> Option<String> {
    match remediation {
        Some(detail) if !is_bounded_code(detail, MAX_DETAIL_CHARS) => {
            Some(DETAIL_REDACTED_MARKER.to_string())
        }
        other => other.map(str::to_string),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debug_adapter::{DapMessage, DebugAdapter, SUPPORTED_COMMANDS};
    use crate::reload::{
        GenerationEffect, IndeterminateCause, LoadedModuleReloadEligibility,
        LoadedModuleReloadOutcome, PreMutationFailureCause, ReloadTransactionPhase,
        RuntimeModuleGenerationClock,
    };
    use serde_json::Value;
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::{Path, PathBuf};

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn repository_root() -> Result<PathBuf, String> {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .ok_or_else(|| "perl-dap must live below the repository root".to_string())
    }

    fn read_repo(relative: &str) -> Result<String, String> {
        let path = repository_root()?.join(relative);
        fs::read_to_string(&path).map_err(|error| format!("{relative} must be readable: {error}"))
    }

    fn subject_value() -> Value {
        serde_json::json!({
            "moduleIdentity": "opaque-module-token-1a2b",
            "savedSourceDigest": "sha256:0f12e4d6a9b8c7d5e3f1a0b9c8d7e6f5a4b3c2d1e0f9a8b7c6d5e4f3a2b1c0d",
            "logicalSourceUri": "perl-lsp-subject:epoch=7;observation=3",
            "observationGeneration": 3
        })
    }

    fn request_value(operation_id: u64, session_epoch: u64) -> Value {
        serde_json::json!({
            "family": LOADED_MODULE_RELOAD_FAMILY,
            "familyVersion": LOADED_MODULE_RELOAD_FAMILY_VERSION,
            "sessionEpoch": session_epoch,
            "operationId": operation_id,
            "subject": subject_value(),
            "deadlineMs": 5000
        })
    }

    fn negotiated_backed_session(epoch: u64) -> ReloadFamilySession {
        let mut session = ReloadFamilySession::new(epoch, true);
        let declaration = ClientFamilyDeclaration {
            family: LOADED_MODULE_RELOAD_FAMILY.to_string(),
            versions: vec![LOADED_MODULE_RELOAD_FAMILY_VERSION],
        };
        assert_eq!(session.negotiate(Some(&declaration)), Ok(LOADED_MODULE_RELOAD_FAMILY_VERSION));
        session
    }

    fn rejection_code_of(evaluation: &ReloadRequestEvaluation) -> Option<WireRejectionCode> {
        match evaluation {
            ReloadRequestEvaluation::Response(LoadedModuleReloadWireResponse {
                body: LoadedModuleReloadResponseBody::Rejected(rejection),
                ..
            }) => Some(rejection.code),
            _ => None,
        }
    }

    // -----------------------------------------------------------------------
    // Negotiation matrix
    // -----------------------------------------------------------------------

    #[test]
    fn negotiation_matrix_selects_the_highest_mutual_version_or_fails_closed() -> TestResult {
        let declaration = |family: &str, versions: &[u32]| ClientFamilyDeclaration {
            family: family.to_string(),
            versions: versions.to_vec(),
        };

        let mut session = ReloadFamilySession::new(7, true);
        assert_eq!(
            session.negotiate(Some(&declaration(LOADED_MODULE_RELOAD_FAMILY, &[1]))),
            Ok(1),
            "same-version client/adapter must negotiate"
        );
        assert_eq!(
            session.negotiate(None),
            Err(FamilyNegotiationRefusal::FamilyAbsent),
            "a client with no family support must not negotiate"
        );
        assert_eq!(
            session.negotiate(Some(&declaration("perl-lsp/otherFamily", &[1]))),
            Err(FamilyNegotiationRefusal::FamilyNameMismatch),
            "a different family identity must not negotiate"
        );
        assert_eq!(
            session.negotiate(Some(&declaration(LOADED_MODULE_RELOAD_FAMILY, &[2]))),
            Err(FamilyNegotiationRefusal::NoOverlappingVersion),
            "a newer client against this adapter version must fail closed"
        );
        assert_eq!(
            session.negotiate(Some(&declaration(LOADED_MODULE_RELOAD_FAMILY, &[0, 3]))),
            Err(FamilyNegotiationRefusal::NoOverlappingVersion),
            "version zero and unknown versions are not versions"
        );
        let mut fresh = ReloadFamilySession::new(8, true);
        assert_eq!(
            fresh.negotiate(Some(&declaration(LOADED_MODULE_RELOAD_FAMILY, &[0, 1, 2]))),
            Ok(1),
            "the highest mutually known version is selected"
        );
        Ok(())
    }

    #[test]
    fn session_replacement_invalidates_prior_family_and_operation_identities() -> TestResult {
        let mut old = negotiated_backed_session(7);
        assert!(matches!(
            old.evaluate(&request_value(42, 7)),
            ReloadRequestEvaluation::Admitted { .. }
        ));
        // The replacement session has a new epoch; the old negotiated family
        // and the old operation identity do not carry over.
        let mut restarted = ReloadFamilySession::new(8, true);
        assert_eq!(
            rejection_code_of(&restarted.evaluate(&request_value(42, 7))),
            Some(WireRejectionCode::FamilyNotNegotiated),
            "the new session has no negotiation yet"
        );
        let declaration = ClientFamilyDeclaration {
            family: LOADED_MODULE_RELOAD_FAMILY.to_string(),
            versions: vec![1],
        };
        let _ = restarted.negotiate(Some(&declaration));
        assert_eq!(
            rejection_code_of(&restarted.evaluate(&request_value(42, 7))),
            Some(WireRejectionCode::SessionStale),
            "old-epoch requests receive session_stale, never a current result"
        );
        assert!(
            matches!(
                restarted.evaluate(&request_value(42, 8)),
                ReloadRequestEvaluation::Admitted { .. }
            ),
            "operation identity 42 is free again under the new epoch"
        );
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Evaluation gates
    // -----------------------------------------------------------------------

    #[test]
    fn evaluation_gates_fail_closed_in_the_registry_precedence() -> TestResult {
        // Payload bound.
        let mut oversized = request_value(51, 7);
        oversized["subject"]["moduleIdentity"] =
            Value::String("y".repeat(MAX_REQUEST_BYTES).into());
        let mut session = negotiated_backed_session(7);
        assert_eq!(
            rejection_code_of(&session.evaluate(&oversized)),
            Some(WireRejectionCode::PayloadTooLarge)
        );

        // Family identity.
        let mut wrong_family = request_value(52, 7);
        wrong_family["family"] = Value::String("modules".into());
        let mut session = negotiated_backed_session(7);
        assert_eq!(
            rejection_code_of(&session.evaluate(&wrong_family)),
            Some(WireRejectionCode::FamilyNameMismatch)
        );

        // Family version.
        let mut wrong_version = request_value(53, 7);
        wrong_version["familyVersion"] = Value::from(2);
        let mut session = negotiated_backed_session(7);
        assert_eq!(
            rejection_code_of(&session.evaluate(&wrong_version)),
            Some(WireRejectionCode::FamilyVersionUnsupported)
        );

        // No negotiation.
        let mut unnegotiated = ReloadFamilySession::new(7, true);
        assert_eq!(
            rejection_code_of(&unnegotiated.evaluate(&request_value(54, 7))),
            Some(WireRejectionCode::FamilyNotNegotiated)
        );

        // Raw input at the request and subject levels.
        for key in ["path", "command", "expression", "incKey", "packageName", "replacement"] {
            let mut raw_request = request_value(55, 7);
            raw_request[key] = Value::String("/etc/passwd".into());
            let mut session = negotiated_backed_session(7);
            assert_eq!(
                rejection_code_of(&session.evaluate(&raw_request)),
                Some(WireRejectionCode::RawClientInputRefused),
                "request-level key {key} must be refused as raw client input"
            );
            let mut raw_subject = request_value(56, 7);
            raw_subject["subject"][key] = Value::String("delete $INC{'App.pm'}".into());
            let mut session = negotiated_backed_session(7);
            assert_eq!(
                rejection_code_of(&session.evaluate(&raw_subject)),
                Some(WireRejectionCode::RawClientInputRefused),
                "subject-level key {key} must be refused as raw client input"
            );
        }

        // Unknown additive field fails closed under the v1 policy.
        let mut unknown_field = request_value(57, 7);
        unknown_field["hint"] = Value::String("forward-compat candidate".into());
        let mut session = negotiated_backed_session(7);
        assert_eq!(
            rejection_code_of(&session.evaluate(&unknown_field)),
            Some(WireRejectionCode::UnknownFieldRejected)
        );

        // Malformed shape.
        let mut session = negotiated_backed_session(7);
        assert_eq!(
            rejection_code_of(&session.evaluate(&Value::String("not an object".into()))),
            Some(WireRejectionCode::MalformedRequest)
        );

        // Identity bounds and insufficiency.
        let mut long_identity = request_value(58, 7);
        long_identity["subject"]["moduleIdentity"] =
            Value::String("x".repeat(MAX_IDENTITY_CHARS + 1).into());
        let mut session = negotiated_backed_session(7);
        assert_eq!(
            rejection_code_of(&session.evaluate(&long_identity)),
            Some(WireRejectionCode::IdentityTooLarge)
        );
        let mut missing_identity = request_value(59, 7);
        missing_identity["subject"]["logicalSourceUri"] = Value::String("  ".into());
        let mut session = negotiated_backed_session(7);
        assert_eq!(
            rejection_code_of(&session.evaluate(&missing_identity)),
            Some(WireRejectionCode::SubjectIdentityInsufficient)
        );

        // Operation identity.
        let mut zero_operation = request_value(0, 7);
        let mut session = negotiated_backed_session(7);
        assert_eq!(
            rejection_code_of(&session.evaluate(&zero_operation)),
            Some(WireRejectionCode::OperationIdInvalid)
        );
        let mut session = negotiated_backed_session(7);
        session.evaluate(&request_value(42, 7));
        assert_eq!(
            rejection_code_of(&session.evaluate(&request_value(42, 7))),
            Some(WireRejectionCode::OperationStale),
            "a replayed operation identity is stale"
        );

        // Deadline bound.
        for deadline in [MIN_DEADLINE_MS - 1, MAX_DEADLINE_MS + 1] {
            let mut out_of_range = request_value(60, 7);
            out_of_range["deadlineMs"] = Value::from(deadline);
            let mut session = negotiated_backed_session(7);
            assert_eq!(
                rejection_code_of(&session.evaluate(&out_of_range)),
                Some(WireRejectionCode::DeadlineOutOfRange)
            );
        }

        // Mechanism backing: registration is not behavior.
        let declaration = ClientFamilyDeclaration {
            family: LOADED_MODULE_RELOAD_FAMILY.to_string(),
            versions: vec![1],
        };
        let mut unbacked = ReloadFamilySession::new(7, false);
        let _ = unbacked.negotiate(Some(&declaration));
        assert_eq!(
            rejection_code_of(&unbacked.evaluate(&request_value(61, 7))),
            Some(WireRejectionCode::FamilyNotBackedForSession)
        );

        // The happy path admits exactly once per operation identity.
        let mut session = negotiated_backed_session(7);
        assert!(matches!(
            session.evaluate(&request_value(62, 7)),
            ReloadRequestEvaluation::Admitted { operation_id: 62 }
        ));
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Terminal routing
    // -----------------------------------------------------------------------

    fn refusal_dispositions() -> Vec<LoadedModuleReloadEligibility> {
        LoadedModuleReloadEligibility::ALL
            .into_iter()
            .filter(|eligibility| WireDisposition::try_from(*eligibility).is_ok())
            .collect()
    }

    #[test]
    fn every_refusal_disposition_routes_verbatim_without_advancing() -> TestResult {
        assert_eq!(refusal_dispositions().len(), 12, "exactly the twelve refusal classes");
        for disposition in refusal_dispositions() {
            let outcome = LoadedModuleReloadOutcome::Refused { disposition };
            let mut clock = RuntimeModuleGenerationClock::new();
            let response = project_outcome(&outcome, 42, &mut clock, &[], None)?;
            assert!(!response.success, "a refusal is never success");
            let LoadedModuleReloadResponseBody::Outcome(body) = response.body else {
                return Err("a refusal must project an outcome body".into());
            };
            let wire_disposition =
                WireDisposition::try_from(disposition).map_err(|()| "mapping failed")?;
            let projected = serde_json::to_value(
                body.disposition.ok_or("a refusal must carry its disposition")?,
            )?;
            let expected = serde_json::to_value(wire_disposition)?;
            assert_eq!(projected, expected);
            assert_eq!(body.kind.as_str(), "refused");
            assert!(!body.possibly_applied);
            let generation = body.generation.ok_or("the generation witness must be carried")?;
            assert!(!generation.advanced, "a refusal advances nothing");
            assert_eq!(generation.previous, generation.current);
            assert_eq!(outcome.generation_effect(), GenerationEffect::None);
        }
        Ok(())
    }

    #[test]
    fn every_pre_mutation_failure_routes_with_phase_and_cause() -> TestResult {
        for phase in [
            ReloadTransactionPhase::Admission,
            ReloadTransactionPhase::Preflight,
            ReloadTransactionPhase::Prepare,
        ] {
            for cause in PreMutationFailureCause::ALL {
                let outcome = LoadedModuleReloadOutcome::FailedBeforeMutation { phase, cause };
                let mut clock = RuntimeModuleGenerationClock::new();
                let response = project_outcome(&outcome, 42, &mut clock, &[], None)?;
                assert!(!response.success);
                let LoadedModuleReloadResponseBody::Outcome(body) = response.body else {
                    return Err("a pre-mutation failure must project an outcome body".into());
                };
                assert_eq!(body.kind.as_str(), "failed_before_mutation");
                assert_eq!(
                    body.phase,
                    WirePhase::from(phase),
                    "the reached phase must travel verbatim"
                );
                assert_eq!(body.cause, Some(WireFailureCause::from(cause)));
                assert!(!body.possibly_applied);
                assert!(!body.generation.ok_or("witness required")?.advanced);
            }
        }
        Ok(())
    }

    #[test]
    fn every_indeterminate_cause_is_possibly_applied_and_never_clean() -> TestResult {
        for phase in [
            ReloadTransactionPhase::RuntimeMutationBegins,
            ReloadTransactionPhase::RuntimeAcknowledgementReadBack,
            ReloadTransactionPhase::CommitGeneration,
            ReloadTransactionPhase::PostReloadReconciliation,
        ] {
            for cause in IndeterminateCause::ALL {
                let outcome =
                    LoadedModuleReloadOutcome::IndeterminatePossiblyApplied { phase, cause };
                let mut clock = RuntimeModuleGenerationClock::new();
                let response = project_outcome(&outcome, 42, &mut clock, &[], None)?;
                assert!(
                    !response.success,
                    "an indeterminate outcome is never DAP success ({cause:?} at {phase:?})"
                );
                let LoadedModuleReloadResponseBody::Outcome(body) = &response.body else {
                    return Err("an indeterminate outcome must project an outcome body".into());
                };
                assert_eq!(body.kind.as_str(), "indeterminate_possibly_applied");
                assert!(body.possibly_applied, "possibly_applied must be true");
                assert_eq!(body.phase, WirePhase::from(phase));
                assert_eq!(body.cause, Some(WireFailureCause::from(cause)));
                let generation = body.generation.ok_or("witness required")?;
                assert!(generation.advanced, "the generation advances for the unknown outcome");
                assert_eq!(generation.current, generation.previous + 1);

                // The classification surface mirrors the client contract:
                // indeterminate is its own terminal class, never clean, and
                // never an ordinary failure spelling.
                assert_eq!(
                    classify_wire_terminal(body.kind.as_str(), body.possibly_applied),
                    WireTerminalClassification::PossiblyApplied
                );
                // A serialized body round-trips with possibly_applied intact:
                // flattening it to ordinary failure would be visible here.
                let wire = serde_json::to_value(response)?;
                assert_eq!(wire["body"]["possiblyApplied"], Value::Bool(true));
                assert_eq!(
                    wire["body"]["kind"],
                    Value::String("indeterminate_possibly_applied".into())
                );
                assert_eq!(wire["success"], Value::Bool(false));
            }
        }
        Ok(())
    }

    #[test]
    fn both_advancement_kinds_advance_and_only_they_do() -> TestResult {
        let advancing = [
            LoadedModuleReloadOutcome::Reloaded,
            LoadedModuleReloadOutcome::IndeterminatePossiblyApplied {
                phase: ReloadTransactionPhase::RuntimeAcknowledgementReadBack,
                cause: IndeterminateCause::TimeoutAfterMutationBegan,
            },
        ];
        for outcome in advancing {
            let mut clock = RuntimeModuleGenerationClock::new();
            let response = project_outcome(&outcome, 42, &mut clock, &[], None)?;
            let LoadedModuleReloadResponseBody::Outcome(body) = response.body else {
                return Err("advancing outcomes must project outcome bodies".into());
            };
            assert!(body.generation.ok_or("witness required")?.advanced);
        }
        let static_outcomes = [
            LoadedModuleReloadOutcome::Refused {
                disposition: LoadedModuleReloadEligibility::NotLoaded,
            },
            LoadedModuleReloadOutcome::FailedBeforeMutation {
                phase: ReloadTransactionPhase::Preflight,
                cause: PreMutationFailureCause::PrepareFailed,
            },
        ];
        for outcome in static_outcomes {
            let mut clock = RuntimeModuleGenerationClock::new();
            let response = project_outcome(&outcome, 42, &mut clock, &[], None)?;
            let LoadedModuleReloadResponseBody::Outcome(body) = response.body else {
                return Err("static outcomes must project outcome bodies".into());
            };
            assert!(!body.generation.ok_or("witness required")?.advanced);
        }
        Ok(())
    }

    #[test]
    fn only_reloaded_is_clean_and_contradictory_or_unknown_bodies_fail_closed() -> TestResult {
        assert_eq!(
            classify_wire_terminal("reloaded", false),
            WireTerminalClassification::ReloadedClean
        );
        assert_eq!(
            classify_wire_terminal("reloaded", true),
            WireTerminalClassification::UnknownFailClosed,
            "a contradictory reloaded+possibly_applied body must fail closed"
        );
        assert_eq!(
            classify_wire_terminal("refused", true),
            WireTerminalClassification::UnknownFailClosed,
            "a contradictory refused+possibly_applied body must fail closed"
        );
        assert_eq!(
            classify_wire_terminal("failed_before_mutation", true),
            WireTerminalClassification::UnknownFailClosed,
            "a contradictory failed+possibly_applied body must fail closed"
        );
        assert_eq!(
            classify_wire_terminal("runtime_rejected", false),
            WireTerminalClassification::UnknownFailClosed,
            "an unknown mandatory variant is never clean"
        );
        assert_eq!(
            classify_wire_terminal("preflight_failed", false),
            WireTerminalClassification::UnknownFailClosed
        );
        // Unknown mandatory variants also fail deserialization fail-closed.
        let unknown = serde_json::json!({
            "kind": "reloaded_extended_v2",
            "phase": "terminal_projection",
            "possiblyApplied": false
        });
        assert!(serde_json::from_value::<LoadedModuleReloadOutcomeBody>(unknown).is_err());
        Ok(())
    }

    #[test]
    fn bounds_apply_before_publication() -> TestResult {
        // Reason clamping carries the marker instead of dropping overflow.
        let reasons: Vec<String> = (0..20).map(|index| format!("reason_{index:02}")).collect();
        let outcome = LoadedModuleReloadOutcome::Reloaded;
        let mut clock = RuntimeModuleGenerationClock::new();
        let response = project_outcome(&outcome, 42, &mut clock, &reasons, None)?;
        let LoadedModuleReloadResponseBody::Outcome(body) = response.body else {
            return Err("expected an outcome body".into());
        };
        assert_eq!(body.reasons.len(), MAX_REASONS, "the reason list is clamped to the bound");
        assert_eq!(
            *body.reasons.last().ok_or("clamped list is not empty")?,
            REASONS_TRUNCATED_MARKER
        );

        // An over-bound remediation detail is replaced, never echoed.
        let private_detail = format!("/private/path/{}", "y".repeat(400));
        let outcome = LoadedModuleReloadOutcome::Refused {
            disposition: LoadedModuleReloadEligibility::OutsideLaunchAuthority,
        };
        let mut clock = RuntimeModuleGenerationClock::new();
        let response = project_outcome(&outcome, 42, &mut clock, &[], Some(&private_detail))?;
        let LoadedModuleReloadResponseBody::Outcome(body) = &response.body else {
            return Err("expected an outcome body".into());
        };
        assert_eq!(body.remediation.as_deref(), Some(DETAIL_REDACTED_MARKER));
        let wire = serde_json::to_string(&response)?;
        assert!(!wire.contains("/private/path/"), "private detail must not reach the wire");
        Ok(())
    }

    #[test]
    fn non_code_reasons_and_short_non_code_remediation_are_redacted() -> TestResult {
        // Grammar enforcement, not just length: a raw path or free text in
        // the reason list is redacted to the bounded marker before
        // publication, whatever its length.
        let reasons = vec![
            "valid_reason".to_string(),
            "/private/path/App.pm".to_string(),
            "Has-Uppercase".to_string(),
            format!("over_long_{}", "x".repeat(MAX_REASON_CHARS)),
        ];
        let outcome = LoadedModuleReloadOutcome::IndeterminatePossiblyApplied {
            phase: ReloadTransactionPhase::RuntimeAcknowledgementReadBack,
            cause: IndeterminateCause::TimeoutAfterMutationBegan,
        };
        let mut clock = RuntimeModuleGenerationClock::new();
        let response = project_outcome(&outcome, 7, &mut clock, &reasons, None)?;
        let LoadedModuleReloadResponseBody::Outcome(body) = &response.body else {
            return Err("expected an outcome body".into());
        };
        assert_eq!(body.reasons.first().map(String::as_str), Some("valid_reason"));
        for redacted in body.reasons.iter().skip(1) {
            assert_eq!(redacted, DETAIL_REDACTED_MARKER);
        }
        let wire = serde_json::to_string(&response)?;
        assert!(!wire.contains("/private/path/"), "a raw path reason must not reach the wire");

        // A short but non-code remediation is redacted too: the surface
        // carries bounded codes only.
        let outcome = LoadedModuleReloadOutcome::Refused {
            disposition: LoadedModuleReloadEligibility::OutsideLaunchAuthority,
        };
        let mut clock = RuntimeModuleGenerationClock::new();
        let response =
            project_outcome(&outcome, 7, &mut clock, &[], Some("/private/path/secret.pm"))?;
        let LoadedModuleReloadResponseBody::Outcome(body) = &response.body else {
            return Err("expected an outcome body".into());
        };
        assert_eq!(body.remediation.as_deref(), Some(DETAIL_REDACTED_MARKER));
        Ok(())
    }

    #[test]
    fn a_post_boundary_pre_mutation_outcome_refuses_projection_and_moves_nothing() -> TestResult {
        // The frozen contract treats this shape as malformed-but-advancing
        // in its clock; the wire must not serialize it as a clean
        // pre-mutation failure at all — it refuses before the clock moves.
        let outcome = LoadedModuleReloadOutcome::FailedBeforeMutation {
            phase: ReloadTransactionPhase::RuntimeMutationBegins,
            cause: PreMutationFailureCause::PrepareFailed,
        };
        let mut clock = RuntimeModuleGenerationClock::new();
        let refusal =
            project_outcome(&outcome, 42, &mut clock, &[], None).expect_err("must refuse");
        assert_eq!(refusal, WireProjectionRefusal::OutcomePhaseKindMismatch);
        assert_eq!(
            project_outcome(
                &LoadedModuleReloadOutcome::FailedBeforeMutation {
                    phase: ReloadTransactionPhase::TerminalProjection,
                    cause: PreMutationFailureCause::CancelledBeforeMutationBegan,
                },
                42,
                &mut clock,
                &[],
                None
            )
            .expect_err("any post-boundary pairing must refuse"),
            WireProjectionRefusal::OutcomePhaseKindMismatch
        );
        // Nothing was published and the clock never moved.
        let outcome = LoadedModuleReloadOutcome::Reloaded;
        let response = project_outcome(&outcome, 42, &mut clock, &[], None)?;
        let LoadedModuleReloadResponseBody::Outcome(body) = response.body else {
            return Err("expected an outcome body".into());
        };
        assert_eq!(body.generation.ok_or("witness required")?.previous, 0);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Capability and dispatch pins (unadvertised and not dispatched)
    // -----------------------------------------------------------------------

    #[test]
    fn no_capability_advertises_the_family_before_r04() -> TestResult {
        let mut adapter = DebugAdapter::new();
        let init = adapter.handle_request(1, "initialize", None);
        let DapMessage::Response { success: true, command, body: Some(body), .. } = init else {
            return Err("expected a successful initialize response".into());
        };
        if command != "initialize" {
            return Err("expected the initialize command".into());
        }
        let capabilities = body.as_object().ok_or("initialize body must be a capability object")?;
        for key in capabilities.keys() {
            assert!(
                !key.to_lowercase().contains("reload"),
                "the reload family must stay unadvertised, but capability {key:?} mentions it"
            );
        }
        assert!(
            !capabilities.contains_key("supportsLoadedModuleReload"),
            "no invented standard capability spelling may appear"
        );
        Ok(())
    }

    #[test]
    fn the_family_request_is_not_dispatched_and_fails_closed() -> TestResult {
        assert!(
            !SUPPORTED_COMMANDS.contains(&LOADED_MODULE_RELOAD_REQUEST),
            "the custom family must not be a dispatched standard command"
        );
        assert!(!crate::debug_adapter::is_supported_dap_command(LOADED_MODULE_RELOAD_REQUEST));
        let mut adapter = DebugAdapter::new();
        let response = adapter.handle_request(2, LOADED_MODULE_RELOAD_REQUEST, None);
        let DapMessage::Response { success, command, .. } = response else {
            return Err("expected a response".into());
        };
        assert_eq!(command, LOADED_MODULE_RELOAD_REQUEST);
        assert!(
            !success,
            "an undischarged family request must receive the ordinary unknown-command failure"
        );
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Registry, schema, and TypeScript synchronization
    // -----------------------------------------------------------------------

    fn registry_family_entry() -> Result<Value, String> {
        let manifest: Value = serde_json::from_str(&read_repo(".ci/dap/protocol-authority.json")?)
            .map_err(|error| format!("authority manifest must be valid JSON: {error}"))?;
        manifest["project_families"]
            .as_array()
            .and_then(|families| {
                families
                    .iter()
                    .find(|family| family["family"].as_str() == Some(LOADED_MODULE_RELOAD_FAMILY))
            })
            .cloned()
            .ok_or_else(|| {
                format!(
                    "the {LOADED_MODULE_RELOAD_FAMILY} family must be registered in \
                     .ci/dap/protocol-authority.json project_families"
                )
            })
    }

    #[test]
    fn the_canonical_registry_entry_defines_the_family() -> TestResult {
        let entry = registry_family_entry()?;
        assert_eq!(entry["request_name"].as_str(), Some(LOADED_MODULE_RELOAD_REQUEST));
        assert_eq!(entry["version"].as_u64(), Some(u64::from(LOADED_MODULE_RELOAD_FAMILY_VERSION)));
        assert_eq!(entry["classification"].as_str(), Some("custom_dap_extension"));
        assert_eq!(entry["capability_advertisement"].as_str(), Some("unadvertised-until-r04"));
        assert_eq!(entry["dispatched"].as_bool(), Some(false), "no handler routing in R01B");
        assert_eq!(entry["bounds"]["max_request_bytes"].as_u64(), Some(MAX_REQUEST_BYTES as u64));
        assert_eq!(entry["bounds"]["max_identity_chars"].as_u64(), Some(MAX_IDENTITY_CHARS as u64));
        assert_eq!(entry["bounds"]["max_reasons"].as_u64(), Some(MAX_REASONS as u64));
        Ok(())
    }

    fn schema_enums() -> Result<Value, String> {
        let schema: Value =
            serde_json::from_str(&read_repo("schemas/loaded_module_reload_family.v1.schema.json")?)
                .map_err(|error| format!("wire schema must be valid JSON: {error}"))?;
        Ok(schema["$defs"].clone())
    }

    fn enum_codes(defs: &Value, name: &str) -> Result<Vec<String>, String> {
        let codes = defs[name]["enum"]
            .as_array()
            .ok_or_else(|| format!("schema $defs.{name} must declare an enum"))?;
        codes
            .iter()
            .map(|code| {
                code.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| format!("$defs.{name} enum entries must be strings"))
            })
            .collect()
    }

    #[test]
    fn the_wire_schema_enums_mirror_the_frozen_vocabulary_exactly() -> TestResult {
        let defs = schema_enums()?;

        let kinds: BTreeSet<String> = enum_codes(&defs, "outcomeKind")?.into_iter().collect();
        let expected_kinds: BTreeSet<String> =
            ["reloaded", "refused", "failed_before_mutation", "indeterminate_possibly_applied"]
                .into_iter()
                .map(str::to_string)
                .collect();
        assert_eq!(kinds, expected_kinds, "the four frozen terminal kinds only");

        let phases: BTreeSet<String> =
            ReloadTransactionPhase::ALL.iter().map(|phase| phase.as_str().to_string()).collect();
        assert_eq!(
            enum_codes(&defs, "transactionPhase")?.into_iter().collect::<BTreeSet<_>>(),
            phases
        );

        let dispositions: BTreeSet<String> = refusal_dispositions()
            .into_iter()
            .map(|eligibility| eligibility.as_str().to_string())
            .collect();
        assert_eq!(
            enum_codes(&defs, "refusalDisposition")?.into_iter().collect::<BTreeSet<_>>(),
            dispositions,
            "exactly the twelve refusal dispositions, eligible class excluded"
        );

        let mut causes: Vec<String> =
            PreMutationFailureCause::ALL.iter().map(|cause| cause.as_str().to_string()).collect();
        causes.extend(IndeterminateCause::ALL.iter().map(|cause| cause.as_str().to_string()));
        let causes: BTreeSet<String> = causes.into_iter().collect();
        assert_eq!(enum_codes(&defs, "failureCause")?.into_iter().collect::<BTreeSet<_>>(), causes);

        let rejection_codes: BTreeSet<String> =
            ALL_REJECTION_CODES.iter().map(|code| code.to_string()).collect();
        assert_eq!(
            enum_codes(&defs, "rejectionCode")?.into_iter().collect::<BTreeSet<_>>(),
            rejection_codes
        );

        assert_eq!(defs["opaqueIdentity"]["maxLength"].as_u64(), Some(MAX_IDENTITY_CHARS as u64));
        assert_eq!(defs["digestToken"]["maxLength"].as_u64(), Some(MAX_DIGEST_CHARS as u64));
        assert_eq!(defs["reasonCode"]["maxLength"].as_u64(), Some(MAX_REASON_CHARS as u64));
        assert_eq!(defs["remediationCode"]["maxLength"].as_u64(), Some(MAX_DETAIL_CHARS as u64));
        Ok(())
    }

    /// Every rejection code, for the schema synchronization test.
    const ALL_REJECTION_CODES: [&str; 16] = [
        "family_not_negotiated",
        "family_name_mismatch",
        "family_version_unsupported",
        "session_stale",
        "operation_stale",
        "operation_id_invalid",
        "unknown_field_rejected",
        "unknown_variant_rejected",
        "raw_client_input_refused",
        "subject_identity_insufficient",
        "payload_too_large",
        "identity_too_large",
        "detail_too_large",
        "deadline_out_of_range",
        "family_not_backed_for_session",
        "malformed_request",
    ];

    /// Oracle for the TypeScript projection: returns the drift messages
    /// instead of panicking so the failure variants are assertable.
    fn check_typescript_projection(typescript: &str) -> Result<(), String> {
        if !typescript.contains(&format!(
            "LOADED_MODULE_RELOAD_FAMILY_VERSION = {LOADED_MODULE_RELOAD_FAMILY_VERSION}"
        )) {
            return Err("TypeScript feature-family version drifted".into());
        }
        let mut required: Vec<String> = vec![
            LOADED_MODULE_RELOAD_FAMILY.to_string(),
            "request_rejected".to_string(),
            REASONS_TRUNCATED_MARKER.to_string(),
            DETAIL_REDACTED_MARKER.to_string(),
        ];
        required.extend(
            ["reloaded", "refused", "failed_before_mutation", "indeterminate_possibly_applied"]
                .map(str::to_string),
        );
        required.extend(ReloadTransactionPhase::ALL.iter().map(|phase| phase.as_str().to_string()));
        required.extend(
            refusal_dispositions().into_iter().map(|eligibility| eligibility.as_str().to_string()),
        );
        required
            .extend(PreMutationFailureCause::ALL.iter().map(|cause| cause.as_str().to_string()));
        required.extend(IndeterminateCause::ALL.iter().map(|cause| cause.as_str().to_string()));
        required.extend(ALL_REJECTION_CODES.map(str::to_string));
        for literal in required {
            let quoted_single = format!("'{literal}'");
            let quoted_double = format!("\"{literal}\"");
            if !typescript.contains(&quoted_single) && !typescript.contains(&quoted_double) {
                return Err(format!("TypeScript projection is missing literal {literal:?}"));
            }
        }
        if !typescript.contains("| (string & {})") {
            return Err(
                "TypeScript projection lost its bounded unknown-variant representation".into()
            );
        }
        if typescript.contains("modulePath") {
            return Err("TypeScript projection must not model raw path fields".into());
        }
        Ok(())
    }

    #[test]
    fn the_typescript_projection_stays_synchronized() -> TestResult {
        let typescript = read_repo("vscode-extension/src/loadedModuleReloadFamily.generated.ts")?;
        check_typescript_projection(&typescript)?;
        Ok(())
    }

    #[test]
    fn typescript_projection_drift_is_reported() -> TestResult {
        let typescript = read_repo("vscode-extension/src/loadedModuleReloadFamily.generated.ts")?;

        let dropped_kind = typescript
            .replace("'indeterminate_possibly_applied'", "'flattened_failure'")
            .replace("\"indeterminate_possibly_applied\"", "\"flattened_failure\"");
        let error = match check_typescript_projection(&dropped_kind) {
            Err(error) => error,
            Ok(()) => return Err("a dropped terminal kind must be reported as drift".into()),
        };
        assert!(
            error.contains("missing literal"),
            "the drift must name the missing literal, got: {error}"
        );

        let dropped_unknown = typescript.replace("| (string & {})", "");
        let error = match check_typescript_projection(&dropped_unknown) {
            Err(error) => error,
            Ok(()) => return Err("losing the bounded unknown representation must be drift".into()),
        };
        assert_eq!(error, "TypeScript projection lost its bounded unknown-variant representation");
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Canonical vector corpus
    // -----------------------------------------------------------------------

    fn spec_fixtures() -> Result<Vec<(String, Value)>, String> {
        let directory = repository_root()?.join(".spec/10138-loaded-module-reload-family/fixtures");
        let entries = fs::read_dir(&directory)
            .map_err(|error| format!("fixtures must be readable: {error}"))?;
        let mut documents = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| format!("fixture entry: {error}"))?;
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".json") {
                let bytes = fs::read(entry.path()).map_err(|error| format!("{name}: {error}"))?;
                let value = serde_json::from_slice(&bytes)
                    .map_err(|error| format!("{name} must be valid JSON: {error}"))?;
                documents.push((name, value));
            }
        }
        documents.sort_by(|left, right| left.0.cmp(&right.0));
        if documents.is_empty() {
            return Err("the vector corpus must not be empty".to_string());
        }
        Ok(documents)
    }

    fn parse_phase(code: &str) -> Result<ReloadTransactionPhase, String> {
        ReloadTransactionPhase::ALL
            .into_iter()
            .find(|phase| phase.as_str() == code)
            .ok_or_else(|| format!("unknown phase code {code:?}"))
    }

    fn parse_disposition(code: &str) -> Result<LoadedModuleReloadEligibility, String> {
        LoadedModuleReloadEligibility::ALL
            .into_iter()
            .find(|eligibility| eligibility.as_str() == code)
            .ok_or_else(|| format!("unknown disposition code {code:?}"))
    }

    fn parse_pre_cause(code: &str) -> Result<PreMutationFailureCause, String> {
        PreMutationFailureCause::ALL
            .into_iter()
            .find(|cause| cause.as_str() == code)
            .ok_or_else(|| format!("unknown pre-mutation cause {code:?}"))
    }

    fn parse_indeterminate_cause(code: &str) -> Result<IndeterminateCause, String> {
        IndeterminateCause::ALL
            .into_iter()
            .find(|cause| cause.as_str() == code)
            .ok_or_else(|| format!("unknown indeterminate cause {code:?}"))
    }

    fn outcome_from_doc(document: &Value) -> Result<LoadedModuleReloadOutcome, String> {
        let outcome = &document["outcome"];
        let kind = outcome["kind"].as_str().ok_or("vector outcome.kind must be a string")?;
        match kind {
            "reloaded" => Ok(LoadedModuleReloadOutcome::Reloaded),
            "refused" => Ok(LoadedModuleReloadOutcome::Refused {
                disposition: parse_disposition(
                    outcome["disposition"]
                        .as_str()
                        .ok_or("refused vectors must carry a disposition")?,
                )?,
            }),
            "failed_before_mutation" => Ok(LoadedModuleReloadOutcome::FailedBeforeMutation {
                phase: parse_phase(
                    outcome["phase"].as_str().ok_or("failed vectors must carry a phase")?,
                )?,
                cause: parse_pre_cause(
                    outcome["cause"].as_str().ok_or("failed vectors must carry a cause")?,
                )?,
            }),
            "indeterminate_possibly_applied" => {
                Ok(LoadedModuleReloadOutcome::IndeterminatePossiblyApplied {
                    phase: parse_phase(
                        outcome["phase"].as_str().ok_or("indeterminate vectors carry a phase")?,
                    )?,
                    cause: parse_indeterminate_cause(
                        outcome["cause"].as_str().ok_or("indeterminate vectors: cause")?,
                    )?,
                })
            }
            other => Err(format!("unknown vector outcome kind {other:?}")),
        }
    }

    fn seeded_clock(document: &Value) -> Result<RuntimeModuleGenerationClock, String> {
        let mut clock = RuntimeModuleGenerationClock::new();
        let seed = document["generation_before"].as_u64().unwrap_or(0);
        for _ in 0..seed {
            clock.apply(&LoadedModuleReloadOutcome::Reloaded);
        }
        Ok(clock)
    }

    #[test]
    fn the_canonical_vector_corpus_holds_on_the_adapter_side() -> TestResult {
        let fixtures = spec_fixtures()?;
        let mut kinds_covered = BTreeSet::new();
        for (name, document) in fixtures {
            let expect = &document["expect"];

            // Client-classification probes.
            if let Some(expected_class) = expect["classify"].as_str() {
                let probe = &document["response_probe"];
                let kind = probe["kind"]
                    .as_str()
                    .ok_or_else(|| format!("{name}: probe kind must be a string"))?;
                let possibly_applied = probe["possiblyApplied"].as_bool().unwrap_or(false);
                let classification = classify_wire_terminal(kind, possibly_applied);
                let as_code = match classification {
                    WireTerminalClassification::ReloadedClean => "reloaded_clean",
                    WireTerminalClassification::RefusedCleanFailure => "refused_clean_failure",
                    WireTerminalClassification::FailedBeforeMutationCleanFailure => {
                        "failed_before_mutation_clean_failure"
                    }
                    WireTerminalClassification::PossiblyApplied => "possibly_applied",
                    WireTerminalClassification::UnknownFailClosed => "unknown_fail_closed",
                };
                assert_eq!(as_code, expected_class, "{name}: classification mismatch");
                continue;
            }

            let evaluation = if let Some(request) = document.get("request") {
                let mut session = match document.get("negotiation") {
                    Some(negotiation) => {
                        let adapter = &negotiation["adapter"];
                        let epoch = adapter["epoch"].as_u64().unwrap_or(1);
                        let backed = adapter["backed"].as_bool().unwrap_or(false);
                        let mut session = ReloadFamilySession::new(epoch, backed);
                        let declaration = negotiation
                            .get("client")
                            .and_then(|client| client.as_object())
                            .map(|client| ClientFamilyDeclaration {
                                family: client["family"].as_str().unwrap_or_default().to_string(),
                                versions: client["versions"]
                                    .as_array()
                                    .map(|versions| {
                                        versions
                                            .iter()
                                            .filter_map(|version| version.as_u64())
                                            .map(|version| version as u32)
                                            .collect()
                                    })
                                    .unwrap_or_default(),
                            });
                        // An expected negotiation refusal (for example the
                        // client-newer vector) leaves the session
                        // unnegotiated; evaluation must then produce the
                        // typed code on its own, exactly as production
                        // would.
                        let _ = session.negotiate(declaration.as_ref());
                        for operation in document["previously_admitted_operations"]
                            .as_array()
                            .unwrap_or(&Vec::new())
                        {
                            let seeded = serde_json::json!({
                                "family": LOADED_MODULE_RELOAD_FAMILY,
                                "familyVersion": LOADED_MODULE_RELOAD_FAMILY_VERSION,
                                "sessionEpoch": session.epoch,
                                "operationId": operation.as_u64().unwrap_or_default(),
                                "subject": request["subject"],
                            });
                            let _ = session.evaluate(&seeded);
                        }
                        session
                    }
                    None => negotiated_backed_session(7),
                };
                Some(session.evaluate(request))
            } else {
                None
            };

            if let Some(evaluation) = evaluation {
                match (&evaluation, expect["evaluation"].as_str()) {
                    (ReloadRequestEvaluation::Admitted { operation_id }, Some("admitted")) => {
                        assert_eq!(
                            Some(*operation_id),
                            document["request"]["operationId"].as_u64(),
                            "{name}: admitted operation identity"
                        );
                    }
                    (ReloadRequestEvaluation::Response(response), expected) => {
                        if expected == Some("admitted") {
                            return Err(format!("{name}: expected admission, got rejection").into());
                        }
                        let LoadedModuleReloadResponseBody::Rejected(rejection) = &response.body
                        else {
                            return Err(format!("{name}: expected a typed rejection").into());
                        };
                        assert_eq!(
                            rejection.code.as_str(),
                            expect["code"].as_str().unwrap_or_default(),
                            "{name}: rejection code mismatch"
                        );
                        assert!(!response.success);
                        continue;
                    }
                    _ => {}
                }
            }

            // Terminal projection (directly or after admission).
            let outcome =
                outcome_from_doc(&document).map_err(|error| format!("{name}: {error}"))?;
            kinds_covered
                .insert(document["outcome"]["kind"].as_str().unwrap_or("unknown").to_string());
            let mut clock = seeded_clock(&document)?;
            let reasons: Vec<String> = document["oversized_reasons_input"]
                .as_array()
                .map(|values| {
                    values.iter().filter_map(|value| value.as_str().map(str::to_string)).collect()
                })
                .unwrap_or_default();
            let remediation = document["oversized_remediation_input"].as_str();
            let operation_id = document["request"]["operationId"].as_u64().unwrap_or(1);
            let response =
                project_outcome(&outcome, operation_id, &mut clock, &reasons, remediation)
                    .map_err(|refusal| format!("{name}: projection refused: {refusal:?}"))?;
            let wire = serde_json::to_value(&response)?;

            assert_eq!(wire["success"], expect["success"], "{name}: DAP success mismatch");
            if let Some(operation_id) = document["request"]["operationId"].as_u64() {
                assert_eq!(
                    wire["operationId"],
                    Value::from(operation_id),
                    "{name}: the operation identity must travel on the response"
                );
            }
            let body = &wire["body"];
            assert_eq!(body["kind"], expect["kind"], "{name}: kind mismatch");
            if let Some(phase) = expect["phase"].as_str() {
                assert_eq!(body["phase"], Value::String(phase.to_string()), "{name}: phase");
            }
            if let Some(disposition) = expect["disposition"].as_str() {
                assert_eq!(
                    body["disposition"],
                    Value::String(disposition.to_string()),
                    "{name}: disposition"
                );
            }
            if let Some(cause) = expect["cause"].as_str() {
                assert_eq!(body["cause"], Value::String(cause.to_string()), "{name}: cause");
            }
            if let Some(possibly_applied) = expect["possibly_applied"].as_bool() {
                assert_eq!(
                    body["possiblyApplied"],
                    Value::Bool(possibly_applied),
                    "{name}: possibly_applied"
                );
            }
            if expect.get("generation").is_some() {
                let witness = &body["generation"];
                let expected = &expect["generation"];
                assert_eq!(witness["previous"], expected["previous"], "{name}: previous");
                assert_eq!(witness["current"], expected["current"], "{name}: current");
                assert_eq!(witness["advanced"], expected["advanced"], "{name}: advanced");
            }
            if let Some(reconciliation) = expect["reconciliation"].as_object() {
                for (field, disposition) in reconciliation {
                    assert_eq!(
                        body["reconciliation"][field],
                        Value::String(disposition.as_str().unwrap_or_default().to_string()),
                        "{name}: reconciliation {field}"
                    );
                }
            }
            if let Some(count) = expect["reasons_count"].as_u64() {
                assert_eq!(
                    body["reasons"].as_array().map(Vec::len),
                    Some(count as usize),
                    "{name}: clamped reason count"
                );
                assert!(
                    body["reasons"].as_array().is_some_and(|reasons| {
                        reasons.iter().any(|reason| {
                            reason.as_str()
                                == Some(
                                    expect["reasons_truncated_marker"].as_str().unwrap_or_default(),
                                )
                        })
                    }),
                    "{name}: truncation marker"
                );
            }
            if let Some(remediation) = expect["remediation"].as_str() {
                assert_eq!(
                    body["remediation"],
                    Value::String(remediation.to_string()),
                    "{name}: remediation"
                );
            }
        }

        for kind in
            ["reloaded", "refused", "failed_before_mutation", "indeterminate_possibly_applied"]
        {
            assert!(kinds_covered.contains(kind), "the corpus must cover the terminal kind {kind}");
        }
        Ok(())
    }
}
