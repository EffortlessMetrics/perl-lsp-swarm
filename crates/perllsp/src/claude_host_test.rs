//! Explicit opt-in Claude host-test diagnostic contract and CLI admission.
//!
//! This module owns the versioned plan/result/failure model and the no-surprise
//! admission seam for `perllsp doctor --client claude --host-test`. It does not
//! invoke Claude, run semantic host smoke, install a plugin, or mutate
//! compatibility or support authority.

use perllsp::claude_compat::{
    CompatibilityReason, CompatibilityResult, SCHEMA_VERSION as COMPAT_SCHEMA,
};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

/// Versioned host-test result schema.
pub const RESULT_SCHEMA_VERSION: &str = "claude_host_test.v1";
/// Versioned host-test plan schema.
pub const PLAN_SCHEMA_VERSION: &str = "claude_host_test_plan.v1";
/// Structural inspection schema consumed as a plan input (#7829).
pub const STRUCTURAL_STATUS_SCHEMA: &str = "perllsp.claude_status.v1";
/// Canonical user-action identity for the reviewed CLI shape.
pub const USER_ACTION_IDENTITY: &str = "perllsp.doctor.client.claude.host_test";
/// Usage/cost disclosure required before any host-test executor effect.
pub const USAGE_DISCLOSURE: &str = "This diagnostic invokes actual authenticated Claude Code and may consume user entitlement/usage. No project source is selected. Host testing is never run by ordinary doctor, status, setup, or completion.";

const PRIVATE_CANARY_MARKERS: [&str; 6] =
    ["sk-ant-", "ANTHROPIC_API_KEY=", "/home/", "\\Users\\", "PROMPT_CANARY", "SOURCE_CANARY"];

/// Explicit user-action identity. Host testing is never an implicit default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserActionIdentity {
    /// `perllsp doctor --client claude --host-test`.
    DoctorClientClaudeHostTest,
}

impl UserActionIdentity {
    /// Stable machine-readable spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DoctorClientClaudeHostTest => USER_ACTION_IDENTITY,
        }
    }
}

/// Reviewed bounded scenario. Arbitrary prompt/profile strings are not representable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundedScenario {
    /// Isolated native-LSP slice reserved for later host-session/semantic leaves.
    BoundedNativeLspSliceV1,
}

impl BoundedScenario {
    /// Stable machine-readable spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BoundedNativeLspSliceV1 => "bounded_native_lsp_slice.v1",
        }
    }
}

/// Isolated fixture policy. User repositories cannot be selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixturePolicy {
    /// Operation-created temporary fixture independent of the caller's project.
    OperationCreatedTemporaryFixture,
}

impl FixturePolicy {
    /// Stable machine-readable spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OperationCreatedTemporaryFixture => "operation_created_temporary_fixture",
        }
    }
}

/// Host/model instrument bounds identity. No arbitrary process/prompt authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstrumentBoundsIdentity {
    /// Minimum tools/context for the bounded native-LSP slice.
    BoundedNativeLspInstrumentV1,
}

impl InstrumentBoundsIdentity {
    /// Stable machine-readable spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BoundedNativeLspInstrumentV1 => "bounded_native_lsp_instrument.v1",
        }
    }
}

/// Reviewed deadline/retry policy identity. Fields here grant no extra process authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeadlineRetryPolicyIdentity {
    /// Single bounded operation with no retry loop.
    SingleBoundedAttemptV1,
}

impl DeadlineRetryPolicyIdentity {
    /// Stable machine-readable spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SingleBoundedAttemptV1 => "single_bounded_attempt.v1",
        }
    }
}

/// Cleanup and public-safe redaction policy identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupRedactionPolicyIdentity {
    /// Operation-owned temp cleanup plus public-safe redaction.
    OperationOwnedCleanupPublicSafeRedactionV1,
}

impl CleanupRedactionPolicyIdentity {
    /// Stable machine-readable spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OperationOwnedCleanupPublicSafeRedactionV1 => {
                "operation_owned_cleanup_public_safe_redaction.v1"
            }
        }
    }
}

/// Policy for a known incompatible #7885 pair. Silent coercion is not representable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IncompatiblePairPolicy {
    /// Refuse the host test and retain the incompatible identity.
    Refuse,
    /// Explicit reviewed diagnostic override. Not exposed by the current CLI.
    ExplicitReviewedDiagnosticOverride,
}

impl IncompatiblePairPolicy {
    /// Stable machine-readable spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Refuse => "refuse",
            Self::ExplicitReviewedDiagnosticOverride => "explicit_reviewed_diagnostic_override",
        }
    }
}

/// Preflight disposition before any actual-host executor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreflightDisposition {
    /// Plan is admitted for a later actual-host executor.
    Admitted,
    /// #7829 structural inspection requires action first.
    StructuralActionRequired,
    /// Known incompatible pair without an explicit reviewed override.
    CompatibilityIncompatibleRequiresExplicitOverrideOrRefusal,
    /// Compatibility remains not proven and is recorded rather than coerced.
    CompatibilityNotProven,
    /// Claude host or authentication is unavailable.
    HostOrAuthUnavailable,
    /// Supported noninteractive/test surface is unavailable.
    NoninteractiveSurfaceUnavailable,
    /// Platform or reviewed profile is unsupported.
    UnsupportedPlatformOrProfile,
    /// Request is structurally invalid or hostile.
    InvalidRequest,
    /// Required host/instrument observation is not proven in this leaf.
    InstrumentNotProven,
}

impl PreflightDisposition {
    /// Stable machine-readable spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Admitted => "admitted",
            Self::StructuralActionRequired => "structural_action_required",
            Self::CompatibilityIncompatibleRequiresExplicitOverrideOrRefusal => {
                "compatibility_incompatible_requires_explicit_override_or_refusal"
            }
            Self::CompatibilityNotProven => "compatibility_not_proven",
            Self::HostOrAuthUnavailable => "host_or_auth_unavailable",
            Self::NoninteractiveSurfaceUnavailable => "noninteractive_surface_unavailable",
            Self::UnsupportedPlatformOrProfile => "unsupported_platform_or_profile",
            Self::InvalidRequest => "invalid_request",
            Self::InstrumentNotProven => "instrument_not_proven",
        }
    }

    const fn exit_code(self) -> u8 {
        match self {
            Self::InvalidRequest | Self::InstrumentNotProven => 1,
            Self::Admitted
            | Self::CompatibilityNotProven
            | Self::StructuralActionRequired
            | Self::CompatibilityIncompatibleRequiresExplicitOverrideOrRefusal
            | Self::HostOrAuthUnavailable
            | Self::NoninteractiveSurfaceUnavailable
            | Self::UnsupportedPlatformOrProfile => 2,
        }
    }
}

/// Terminal result. Kept separate from component dispositions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalResult {
    /// Every required cell observed pass and cleanup succeeded.
    Pass,
    /// Product/session failure distinct from instrument failure.
    Fail,
    /// Required evidence was not established.
    NotProven,
    /// Host/model instrument failed; not a server/product failure.
    InstrumentFailure,
}

impl TerminalResult {
    /// Stable machine-readable spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::NotProven => "not_proven",
            Self::InstrumentFailure => "instrument_failure",
        }
    }
}

/// One component or method-cell disposition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellDisposition {
    /// Cell was not attempted.
    NotAttempted,
    /// Cell was observed passing.
    ObservedPass,
    /// Cell was observed failing as product/session evidence.
    #[expect(
        dead_code,
        reason = "reserved #16872 vocabulary for later host-session/semantic leaves"
    )]
    ObservedFail,
    /// Cell remains not proven.
    NotProven,
    /// Instrument/model could not drive the cell.
    InstrumentFailure,
    /// Cleanup of operation-owned state failed.
    CleanupFailed,
}

impl CellDisposition {
    /// Stable machine-readable spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotAttempted => "not_attempted",
            Self::ObservedPass => "observed_pass",
            Self::ObservedFail => "observed_fail",
            Self::NotProven => "not_proven",
            Self::InstrumentFailure => "instrument_failure",
            Self::CleanupFailed => "cleanup_failed",
        }
    }
}

/// Stable typed failure reasons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureReason {
    /// Structural prerequisite from #7829 failed.
    StructuralPrerequisiteFailed,
    /// Compatibility catalog says incompatible.
    CompatibilityIncompatible,
    /// Compatibility remains not proven.
    CompatibilityNotProven,
    /// Claude host is not installed or cannot be observed.
    HostNotInstalledOrUnavailable,
    /// Authentication is unavailable.
    #[expect(
        dead_code,
        reason = "reserved #16872 vocabulary for later host-session/semantic leaves"
    )]
    AuthUnavailable,
    /// Noninteractive host surface is unavailable.
    NoninteractiveHostSurfaceUnavailable,
    /// Plugin failed to activate.
    #[expect(
        dead_code,
        reason = "reserved #16872 vocabulary for later host-session/semantic leaves"
    )]
    PluginActivationFailed,
    /// Observed plugin subject is not the intended one.
    WrongPluginSubject,
    /// Observed server subject is not the intended one.
    WrongServerSubject,
    /// Protocol or startup failed.
    #[expect(
        dead_code,
        reason = "reserved #16872 vocabulary for later host-session/semantic leaves"
    )]
    ProtocolOrStartupFailed,
    /// Host/model instrument failed.
    HostInstrumentFailed,
    /// Required native LSP tool call was not observed.
    RequiredLspToolCallNotObserved,
    /// Semantic mismatch against independent expectations.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "reserved #16872 vocabulary for later host-session/semantic leaves"
        )
    )]
    SemanticMismatch,
    /// Stale result after edit/re-query.
    #[expect(
        dead_code,
        reason = "reserved #16872 vocabulary for later host-session/semantic leaves"
    )]
    StalePostEditResult,
    /// Shutdown left an orphaned process.
    #[expect(
        dead_code,
        reason = "reserved #16872 vocabulary for later host-session/semantic leaves"
    )]
    ShutdownOrOrphanFailed,
    /// Cleanup of operation-owned state failed.
    CleanupFailed,
    /// Public-safe redaction failed.
    #[expect(
        dead_code,
        reason = "reserved #16872 vocabulary for later host-session/semantic leaves"
    )]
    RedactionFailed,
    /// Request carried hostile client authority.
    InvalidRequest,
    /// This leaf has no actual-host instrument.
    InstrumentNotProven,
}

impl FailureReason {
    /// Stable machine-readable spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StructuralPrerequisiteFailed => "structural_prerequisite_failed",
            Self::CompatibilityIncompatible => "compatibility_incompatible",
            Self::CompatibilityNotProven => "compatibility_not_proven",
            Self::HostNotInstalledOrUnavailable => "host_not_installed_or_unavailable",
            Self::AuthUnavailable => "auth_unavailable",
            Self::NoninteractiveHostSurfaceUnavailable => "noninteractive_host_surface_unavailable",
            Self::PluginActivationFailed => "plugin_activation_failed",
            Self::WrongPluginSubject => "wrong_plugin_subject",
            Self::WrongServerSubject => "wrong_server_subject",
            Self::ProtocolOrStartupFailed => "protocol_or_startup_failed",
            Self::HostInstrumentFailed => "host_instrument_failed",
            Self::RequiredLspToolCallNotObserved => "required_lsp_tool_call_not_observed",
            Self::SemanticMismatch => "semantic_mismatch",
            Self::StalePostEditResult => "stale_post_edit_result",
            Self::ShutdownOrOrphanFailed => "shutdown_or_orphan_failed",
            Self::CleanupFailed => "cleanup_failed",
            Self::RedactionFailed => "redaction_failed",
            Self::InvalidRequest => "invalid_request",
            Self::InstrumentNotProven => "instrument_not_proven",
        }
    }

    fn from_preflight(disposition: PreflightDisposition) -> Option<Self> {
        match disposition {
            PreflightDisposition::Admitted => None,
            PreflightDisposition::StructuralActionRequired => {
                Some(Self::StructuralPrerequisiteFailed)
            }
            PreflightDisposition::CompatibilityIncompatibleRequiresExplicitOverrideOrRefusal => {
                Some(Self::CompatibilityIncompatible)
            }
            PreflightDisposition::CompatibilityNotProven => Some(Self::CompatibilityNotProven),
            PreflightDisposition::HostOrAuthUnavailable => {
                Some(Self::HostNotInstalledOrUnavailable)
            }
            PreflightDisposition::NoninteractiveSurfaceUnavailable => {
                Some(Self::NoninteractiveHostSurfaceUnavailable)
            }
            PreflightDisposition::UnsupportedPlatformOrProfile => Some(Self::InvalidRequest),
            PreflightDisposition::InvalidRequest => Some(Self::InvalidRequest),
            PreflightDisposition::InstrumentNotProven => Some(Self::InstrumentNotProven),
        }
    }
}

/// Evidence class. Injected and contract-admission results cannot become actual-host evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceClass {
    /// CLI/contract admission without an actual-host session.
    ContractAdmission,
    /// Test-only injected executor. Cannot satisfy actual-host behavior.
    InjectedTestExecutor,
    /// Later actual-host executor. Not produced by this leaf.
    ActualHost,
}

impl EvidenceClass {
    /// Stable machine-readable spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ContractAdmission => "contract_admission",
            Self::InjectedTestExecutor => "injected_test_executor",
            Self::ActualHost => "actual_host",
        }
    }
}

/// Claim ceiling: this result is a support diagnostic observation only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClaimCeiling {
    /// Never #7238/#7242 release evidence.
    pub release_or_support_harness_evidence: bool,
    /// Never mutates #7885.
    pub updates_compatibility_authority: bool,
    /// Never promotes #7122.
    pub promotes_support_registry: bool,
    /// True only for a later actual-host executor with observed native LSP evidence.
    pub actual_host_evidence: bool,
}

impl ClaimCeiling {
    /// Ceiling enforced by this contract leaf.
    pub const fn diagnostic_observation(evidence_class: EvidenceClass) -> Self {
        Self {
            release_or_support_harness_evidence: false,
            updates_compatibility_authority: false,
            promotes_support_registry: false,
            actual_host_evidence: matches!(evidence_class, EvidenceClass::ActualHost),
        }
    }
}

/// #7829 structural-status identity consumed by the plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuralStatusIdentity {
    /// Structural schema identity.
    pub schema_version: String,
    /// Structural verdict spelling.
    pub verdict: String,
    /// Host presence spelling: present, missing, error, unobserved.
    pub host_state: String,
}

impl StructuralStatusIdentity {
    /// Unobserved structural input: this leaf does not probe Claude.
    pub fn unobserved() -> Self {
        Self {
            schema_version: STRUCTURAL_STATUS_SCHEMA.to_string(),
            verdict: "unobserved".to_string(),
            host_state: "unobserved".to_string(),
        }
    }
}

/// #7885 compatibility identity recorded at plan/result time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompatibilityIdentity {
    /// Compatibility schema identity.
    pub schema_version: String,
    /// Distinct compatible / incompatible / not_proven result.
    pub result: CompatibilityResult,
    /// Reason the disposition was reached.
    pub reason: CompatibilityReason,
}

impl CompatibilityIdentity {
    /// Current embedded catalog identity. Empty catalogs remain not_proven.
    pub fn from_embedded_catalog() -> Self {
        Self {
            schema_version: COMPAT_SCHEMA.to_string(),
            result: CompatibilityResult::NotProven,
            reason: CompatibilityReason::ExactPairNotEstablished,
        }
    }
}

/// Observed host capability for preflight. Absence is not_proven, not a pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostCapability {
    /// Authenticated noninteractive surface is available.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "reserved #16872 preflight observation for later host-session leaves"
        )
    )]
    Available,
    /// Host or authentication is unavailable.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "reserved #16872 preflight observation for later host-session leaves"
        )
    )]
    HostOrAuthUnavailable,
    /// Noninteractive/test surface is unavailable.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "reserved #16872 preflight observation for later host-session leaves"
        )
    )]
    NoninteractiveUnavailable,
}

impl HostCapability {
    /// Stable machine-readable spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::HostOrAuthUnavailable => "host_or_auth_unavailable",
            Self::NoninteractiveUnavailable => "noninteractive_unavailable",
        }
    }
}

/// Hostile client-supplied authority. Any populated field fails closed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ClientSuppliedAuthority {
    /// Arbitrary Claude/perllsp command.
    pub command: Option<String>,
    /// Prompt or generated-prose authority.
    pub prompt: Option<String>,
    /// Caller workspace or project path.
    pub workspace_path: Option<String>,
    /// Raw environment map.
    pub environment: Option<BTreeMap<String, String>>,
    /// Credential or token material.
    pub credential: Option<String>,
    /// Executable override.
    pub executable_override: Option<String>,
    /// Attempted compatibility-table mutation.
    pub compatibility_mutation: Option<String>,
}

impl ClientSuppliedAuthority {
    /// No extra client authority.
    pub fn none() -> Self {
        Self::default()
    }

    fn hostile_field(&self) -> Option<&'static str> {
        if self.command.is_some() {
            Some("command")
        } else if self.prompt.is_some() {
            Some("prompt")
        } else if self.workspace_path.is_some() {
            Some("workspace_path")
        } else if self.environment.is_some() {
            Some("environment")
        } else if self.credential.is_some() {
            Some("credential")
        } else if self.executable_override.is_some() {
            Some("executable_override")
        } else if self.compatibility_mutation.is_some() {
            Some("compatibility_mutation")
        } else {
            None
        }
    }
}

/// Transport-neutral host-test request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostTestRequest {
    /// Explicit user-action identity.
    pub user_action: UserActionIdentity,
    /// Current #7829 structural identity, if observed.
    pub structural_status: Option<StructuralStatusIdentity>,
    /// Current #7885 compatibility identity.
    pub compatibility: CompatibilityIdentity,
    /// Requested bounded scenario.
    pub scenario: BoundedScenario,
    /// Isolated fixture policy.
    pub fixture_policy: FixturePolicy,
    /// Host/model instrument bounds.
    pub instrument_bounds: InstrumentBoundsIdentity,
    /// Deadline/retry policy identity.
    pub deadline_retry_policy: DeadlineRetryPolicyIdentity,
    /// Cleanup/redaction policy identity.
    pub cleanup_redaction_policy: CleanupRedactionPolicyIdentity,
    /// Policy for a known incompatible pair.
    pub incompatible_pair_policy: IncompatiblePairPolicy,
    /// Observed host capability, if any.
    pub host_capability: Option<HostCapability>,
    /// Whether the requested platform/profile is supported.
    pub platform_supported: bool,
    /// Client-supplied extra authority. Must remain empty.
    pub client_authority: ClientSuppliedAuthority,
}

impl HostTestRequest {
    /// CLI admission request. Does not probe Claude or accept extra authority.
    pub fn from_cli() -> Self {
        Self {
            user_action: UserActionIdentity::DoctorClientClaudeHostTest,
            structural_status: None,
            compatibility: CompatibilityIdentity::from_embedded_catalog(),
            scenario: BoundedScenario::BoundedNativeLspSliceV1,
            fixture_policy: FixturePolicy::OperationCreatedTemporaryFixture,
            instrument_bounds: InstrumentBoundsIdentity::BoundedNativeLspInstrumentV1,
            deadline_retry_policy: DeadlineRetryPolicyIdentity::SingleBoundedAttemptV1,
            cleanup_redaction_policy:
                CleanupRedactionPolicyIdentity::OperationOwnedCleanupPublicSafeRedactionV1,
            incompatible_pair_policy: IncompatiblePairPolicy::Refuse,
            host_capability: None,
            platform_supported: true,
            client_authority: ClientSuppliedAuthority::none(),
        }
    }
}

/// Validated host-test plan. Contains no prompt, credential, env dump, or path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostTestPlan {
    /// Plan schema identity.
    pub schema_version: String,
    /// Explicit user-action identity.
    pub user_action: UserActionIdentity,
    /// #7829 structural identity.
    pub structural_status: StructuralStatusIdentity,
    /// #7885 compatibility identity.
    pub compatibility: CompatibilityIdentity,
    /// Requested bounded scenario.
    pub scenario: BoundedScenario,
    /// Isolated fixture policy.
    pub fixture_policy: FixturePolicy,
    /// Expected exact subject classes.
    pub expected_subject_classes: Vec<String>,
    /// Host/model instrument bounds.
    pub instrument_bounds: InstrumentBoundsIdentity,
    /// Deadline/retry policy identity.
    pub deadline_retry_policy: DeadlineRetryPolicyIdentity,
    /// Cleanup/redaction policy identity.
    pub cleanup_redaction_policy: CleanupRedactionPolicyIdentity,
    /// Claim ceiling recorded on the plan.
    pub claim_ceiling: ClaimCeiling,
    /// Compatibility plan state retained rather than coerced.
    pub compatibility_plan_state: PreflightDisposition,
}

/// Startup/plugin/server/semantic/currentness/cleanup dispositions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComponentDispositions {
    /// Host/plugin/server startup.
    pub startup: CellDisposition,
    /// Plugin activation.
    pub plugin: CellDisposition,
    /// Server launch identity.
    pub server: CellDisposition,
    /// Semantic native-LSP cells.
    pub semantic: CellDisposition,
    /// Post-edit currentness.
    pub currentness: CellDisposition,
    /// Cleanup of operation-owned state.
    pub cleanup: CellDisposition,
    /// Host/model instrument.
    pub host_instrument: CellDisposition,
}

impl ComponentDispositions {
    fn not_attempted() -> Self {
        Self {
            startup: CellDisposition::NotAttempted,
            plugin: CellDisposition::NotAttempted,
            server: CellDisposition::NotAttempted,
            semantic: CellDisposition::NotAttempted,
            currentness: CellDisposition::NotAttempted,
            cleanup: CellDisposition::NotAttempted,
            host_instrument: CellDisposition::NotProven,
        }
    }
}

/// Observed subject classes. Wrong subjects cannot be represented as pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubjectObservation {
    /// Intended plugin subject observed.
    pub intended_plugin: bool,
    /// Intended server subject observed.
    pub intended_server: bool,
}

impl SubjectObservation {
    fn unobserved() -> Self {
        Self { intended_plugin: false, intended_server: false }
    }
}

/// Cells supplied by a test-only injected executor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InjectedExecution {
    /// Component dispositions.
    pub dispositions: ComponentDispositions,
    /// Subject observation.
    pub subjects: SubjectObservation,
    /// Method cells attempted.
    pub methods_attempted: Vec<String>,
    /// Method cells observed.
    pub methods_observed: Vec<String>,
    /// Optional explicit failure reason.
    pub failure_reason: Option<FailureReason>,
}

impl InjectedExecution {
    /// All-pass injected cells. Still cannot be labelled actual-host evidence.
    #[cfg(test)]
    fn all_observed_pass() -> Self {
        Self {
            dispositions: ComponentDispositions {
                startup: CellDisposition::ObservedPass,
                plugin: CellDisposition::ObservedPass,
                server: CellDisposition::ObservedPass,
                semantic: CellDisposition::ObservedPass,
                currentness: CellDisposition::ObservedPass,
                cleanup: CellDisposition::ObservedPass,
                host_instrument: CellDisposition::ObservedPass,
            },
            subjects: SubjectObservation { intended_plugin: true, intended_server: true },
            methods_attempted: vec![
                "definition".to_string(),
                "references".to_string(),
                "hover_or_document_symbols".to_string(),
                "edit_requery".to_string(),
            ],
            methods_observed: vec![
                "definition".to_string(),
                "references".to_string(),
                "hover_or_document_symbols".to_string(),
                "edit_requery".to_string(),
            ],
            failure_reason: None,
        }
    }
}

/// Versioned `claude_host_test.v1` result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostTestResult {
    /// Result schema identity.
    pub schema_version: String,
    /// Plan schema identity.
    pub plan_schema_version: String,
    /// Operation identity.
    pub operation_identity: String,
    /// #7829 structural identity.
    pub structural_status: StructuralStatusIdentity,
    /// #7885 compatibility identity at start.
    pub compatibility: CompatibilityIdentity,
    /// Platform family. Not a HOME/PATH dump.
    pub platform: String,
    /// Architecture family.
    pub architecture: String,
    /// Fixture/profile identity.
    pub fixture_policy: FixturePolicy,
    /// Bounded scenario identity.
    pub scenario: BoundedScenario,
    /// Host/model instrument bounds.
    pub instrument_bounds: InstrumentBoundsIdentity,
    /// Deadline/retry policy identity.
    pub deadline_retry_policy: DeadlineRetryPolicyIdentity,
    /// Cleanup/redaction policy identity.
    pub cleanup_redaction_policy: CleanupRedactionPolicyIdentity,
    /// Incompatible-pair policy retained rather than coerced.
    pub incompatible_pair_policy: IncompatiblePairPolicy,
    /// Observed host capability, if any. CLI admission leaves this unobserved.
    pub host_capability: Option<HostCapability>,
    /// Expected exact subject classes from the plan.
    pub expected_subject_classes: Vec<String>,
    /// Host/plugin/server identities filled only by a later actual-host leaf.
    pub later_observed: LaterObservedIdentities,
    /// Method cells attempted.
    pub methods_attempted: Vec<String>,
    /// Method cells observed.
    pub methods_observed: Vec<String>,
    /// Component dispositions.
    pub dispositions: ComponentDispositions,
    /// Terminal result, separate from component dispositions.
    pub terminal: TerminalResult,
    /// Typed failure reason, when applicable.
    pub failure_reason: Option<FailureReason>,
    /// Preflight disposition that produced this result.
    pub preflight: PreflightDisposition,
    /// Evidence class.
    pub evidence_class: EvidenceClass,
    /// Claim ceiling.
    pub claim_ceiling: ClaimCeiling,
    /// Public-safe redaction state.
    pub redaction_state: String,
    /// Usage/cost disclosure captured before executor effect.
    pub usage_disclosure: String,
    /// Honest limitations.
    pub limitations: Vec<String>,
}

impl HostTestResult {
    /// Attempting to relabel non-actual-host evidence always fails.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "contract API consumed by #16872 tests and later host-session leaves"
        )
    )]
    pub fn relabel_as_actual_host(&self) -> Result<Self, &'static str> {
        Err("injected or contract-admission results cannot be relabelled actual-host evidence")
    }
}

/// Host/plugin/server identities observed only by a later actual-host executor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaterObservedIdentities {
    /// Claude host version, when later observed.
    pub claude_host_version: Option<String>,
    /// Claude host source class, when later observed.
    pub claude_host_source_class: Option<String>,
    /// Plugin slug, when later observed.
    pub plugin_slug: Option<String>,
    /// Plugin source, when later observed.
    pub plugin_source: Option<String>,
    /// Plugin version, when later observed.
    pub plugin_version: Option<String>,
    /// Plugin tree identity, when later observed.
    pub plugin_tree: Option<String>,
    /// Plugin package identity, when later observed.
    pub plugin_package: Option<String>,
    /// perllsp path-role, when later observed.
    pub server_path_role: Option<String>,
    /// perllsp version, when later observed.
    pub server_version: Option<String>,
    /// perllsp build identity, when later observed.
    pub server_build: Option<String>,
    /// perllsp digest, when later observed.
    pub server_digest: Option<String>,
}

impl LaterObservedIdentities {
    fn unobserved() -> Self {
        Self {
            claude_host_version: None,
            claude_host_source_class: None,
            plugin_slug: None,
            plugin_source: None,
            plugin_version: None,
            plugin_tree: None,
            plugin_package: None,
            server_path_role: None,
            server_version: None,
            server_build: None,
            server_digest: None,
        }
    }

    fn is_unobserved(&self) -> bool {
        self.claude_host_version.is_none()
            && self.claude_host_source_class.is_none()
            && self.plugin_slug.is_none()
            && self.plugin_source.is_none()
            && self.plugin_version.is_none()
            && self.plugin_tree.is_none()
            && self.plugin_package.is_none()
            && self.server_path_role.is_none()
            && self.server_version.is_none()
            && self.server_build.is_none()
            && self.server_digest.is_none()
    }
}

/// Kind of host-test executor. Actual-host is reserved for a later leaf.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutorKind {
    /// Production reserved executor: no Claude invocation.
    Reserved,
    /// Test-only injected executor.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "injected executor is constructed only by #16872 contract tests"
        )
    )]
    Injected,
}

/// Test-only or reserved executor. Production never invokes Claude.
pub trait HostTestExecutor {
    /// Executor classification. Actual-host is not representable here.
    fn kind(&self) -> ExecutorKind;
    /// Execute an admitted plan. Must not be called unless usage was disclosed.
    fn execute(&mut self, plan: &HostTestPlan) -> InjectedExecution;
}

/// Production executor. Returns not-proven cells and never invokes Claude.
#[derive(Debug, Default)]
pub struct ReservedExecutor;

impl HostTestExecutor for ReservedExecutor {
    fn kind(&self) -> ExecutorKind {
        ExecutorKind::Reserved
    }

    fn execute(&mut self, _plan: &HostTestPlan) -> InjectedExecution {
        InjectedExecution {
            dispositions: ComponentDispositions::not_attempted(),
            subjects: SubjectObservation::unobserved(),
            methods_attempted: Vec::new(),
            methods_observed: Vec::new(),
            failure_reason: Some(FailureReason::InstrumentNotProven),
        }
    }
}

/// Product surfaces that must not select host testing by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProductSurface {
    /// Ordinary shared doctor.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "ordinary-surface negative control reserved by the #16872 contract"
        )
    )]
    OrdinaryDoctor,
    /// Ordinary Claude setup.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "ordinary-surface negative control reserved by the #16872 contract"
        )
    )]
    OrdinarySetup,
    /// Ordinary status.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "ordinary-surface negative control reserved by the #16872 contract"
        )
    )]
    OrdinaryStatus,
    /// Shell completion.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "ordinary-surface negative control reserved by the #16872 contract"
        )
    )]
    ShellCompletion,
    /// Read-only Claude doctor without `--host-test`.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "ordinary-surface negative control reserved by the #16872 contract"
        )
    )]
    DoctorClientClaude,
    /// Explicit host-test admission.
    DoctorClientClaudeHostTest {
        /// JSON projection requested.
        json: bool,
    },
}

/// Outcome of product-surface dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchOutcome {
    /// Number of host-test executor calls.
    pub executor_calls: usize,
    /// Whether usage/cost disclosure was emitted before any executor call.
    pub usage_disclosed_before_executor: bool,
    /// Result when this surface is the host-test command.
    pub result: Option<HostTestResult>,
    /// Rendered human or JSON projection.
    pub rendered: String,
    /// Process exit code.
    pub exit_code: u8,
}

/// Admit a request into a validated plan or typed refusal.
pub fn admit(request: &HostTestRequest) -> (PreflightDisposition, Option<HostTestPlan>) {
    if request.client_authority.hostile_field().is_some() {
        return (PreflightDisposition::InvalidRequest, None);
    }
    if !request.platform_supported {
        return (PreflightDisposition::UnsupportedPlatformOrProfile, None);
    }

    let structural =
        request.structural_status.clone().unwrap_or_else(StructuralStatusIdentity::unobserved);

    match structural.verdict.as_str() {
        "action_required" => {
            return (
                PreflightDisposition::StructuralActionRequired,
                Some(plan_from(request, structural)),
            );
        }
        "unsupported" => {
            return (
                PreflightDisposition::UnsupportedPlatformOrProfile,
                Some(plan_from(request, structural)),
            );
        }
        "instrument_error" => {
            return (
                PreflightDisposition::InstrumentNotProven,
                Some(plan_from(request, structural)),
            );
        }
        "degraded" | "ready" | "unobserved" => {}
        _ => {
            return (
                PreflightDisposition::InstrumentNotProven,
                Some(plan_from(request, structural)),
            );
        }
    }
    match request.host_capability {
        Some(HostCapability::HostOrAuthUnavailable) => {
            return (
                PreflightDisposition::HostOrAuthUnavailable,
                Some(plan_from(request, structural)),
            );
        }
        Some(HostCapability::NoninteractiveUnavailable) => {
            return (
                PreflightDisposition::NoninteractiveSurfaceUnavailable,
                Some(plan_from(request, structural)),
            );
        }
        Some(HostCapability::Available) => {}
        None => {
            if structural.host_state == "missing" {
                return (
                    PreflightDisposition::HostOrAuthUnavailable,
                    Some(plan_from(request, structural)),
                );
            }
            return (
                PreflightDisposition::InstrumentNotProven,
                Some(plan_from(request, structural)),
            );
        }
    }

    match request.compatibility.result {
        CompatibilityResult::Incompatible
            if request.incompatible_pair_policy == IncompatiblePairPolicy::Refuse =>
        {
            (
                PreflightDisposition::CompatibilityIncompatibleRequiresExplicitOverrideOrRefusal,
                Some(plan_from(request, structural)),
            )
        }
        CompatibilityResult::Incompatible
        | CompatibilityResult::NotProven
        | CompatibilityResult::Compatible => {
            (PreflightDisposition::Admitted, Some(plan_from(request, structural)))
        }
    }
}

fn plan_from(request: &HostTestRequest, structural: StructuralStatusIdentity) -> HostTestPlan {
    let compatibility_plan_state = match request.compatibility.result {
        CompatibilityResult::Compatible => PreflightDisposition::Admitted,
        CompatibilityResult::NotProven => PreflightDisposition::CompatibilityNotProven,
        CompatibilityResult::Incompatible => {
            if request.incompatible_pair_policy
                == IncompatiblePairPolicy::ExplicitReviewedDiagnosticOverride
            {
                PreflightDisposition::Admitted
            } else {
                PreflightDisposition::CompatibilityIncompatibleRequiresExplicitOverrideOrRefusal
            }
        }
    };
    HostTestPlan {
        schema_version: PLAN_SCHEMA_VERSION.to_string(),
        user_action: request.user_action,
        structural_status: structural,
        compatibility: request.compatibility.clone(),
        scenario: request.scenario,
        fixture_policy: request.fixture_policy,
        expected_subject_classes: expected_subject_classes(),
        instrument_bounds: request.instrument_bounds,
        deadline_retry_policy: request.deadline_retry_policy,
        cleanup_redaction_policy: request.cleanup_redaction_policy,
        claim_ceiling: ClaimCeiling::diagnostic_observation(EvidenceClass::ContractAdmission),
        compatibility_plan_state,
    }
}

/// Dispatch a product surface against a host-test executor.
pub fn dispatch<E: HostTestExecutor>(surface: ProductSurface, executor: &mut E) -> DispatchOutcome {
    let ProductSurface::DoctorClientClaudeHostTest { json } = surface else {
        return DispatchOutcome {
            executor_calls: 0,
            usage_disclosed_before_executor: false,
            result: None,
            rendered: String::new(),
            exit_code: 0,
        };
    };

    let request = HostTestRequest::from_cli();
    dispatch_request(&request, json, executor)
}

/// Dispatch an explicit request. Usage disclosure is recorded before any executor call.
pub fn dispatch_request<E: HostTestExecutor>(
    request: &HostTestRequest,
    json: bool,
    executor: &mut E,
) -> DispatchOutcome {
    let usage_disclosed_before_executor = true;
    let (disposition, plan) = admit(request);
    if disposition != PreflightDisposition::Admitted {
        let result = refusal_result(request, disposition, plan.as_ref());
        let rendered = render(&result, json);
        return DispatchOutcome {
            executor_calls: 0,
            usage_disclosed_before_executor,
            result: Some(result),
            rendered,
            exit_code: disposition.exit_code(),
        };
    }

    let plan = match plan {
        Some(plan) => plan,
        None => {
            let result = refusal_result(request, PreflightDisposition::InvalidRequest, None);
            let rendered = render(&result, json);
            return DispatchOutcome {
                executor_calls: 0,
                usage_disclosed_before_executor,
                result: Some(result),
                rendered,
                exit_code: PreflightDisposition::InvalidRequest.exit_code(),
            };
        }
    };

    let execution = executor.execute(&plan);
    let executor_calls = 1;
    let result = result_from_execution(&plan, request, execution, executor.kind());
    let rendered = render(&result, json);
    let exit_code = match result.terminal {
        TerminalResult::Pass => 0,
        TerminalResult::Fail | TerminalResult::NotProven => 2,
        TerminalResult::InstrumentFailure => 1,
    };
    DispatchOutcome {
        executor_calls,
        usage_disclosed_before_executor,
        result: Some(result),
        rendered,
        exit_code,
    }
}

fn refusal_result(
    request: &HostTestRequest,
    disposition: PreflightDisposition,
    plan: Option<&HostTestPlan>,
) -> HostTestResult {
    let structural = plan
        .map(|plan| plan.structural_status.clone())
        .or_else(|| request.structural_status.clone())
        .unwrap_or_else(StructuralStatusIdentity::unobserved);
    let compatibility = plan
        .map(|plan| plan.compatibility.clone())
        .unwrap_or_else(|| request.compatibility.clone());
    let terminal = match disposition {
        PreflightDisposition::InstrumentNotProven => TerminalResult::NotProven,
        PreflightDisposition::InvalidRequest => TerminalResult::Fail,
        PreflightDisposition::UnsupportedPlatformOrProfile
        | PreflightDisposition::StructuralActionRequired
        | PreflightDisposition::CompatibilityIncompatibleRequiresExplicitOverrideOrRefusal
        | PreflightDisposition::HostOrAuthUnavailable
        | PreflightDisposition::NoninteractiveSurfaceUnavailable => TerminalResult::Fail,
        PreflightDisposition::CompatibilityNotProven | PreflightDisposition::Admitted => {
            TerminalResult::NotProven
        }
    };
    HostTestResult {
        schema_version: RESULT_SCHEMA_VERSION.to_string(),
        plan_schema_version: PLAN_SCHEMA_VERSION.to_string(),
        operation_identity: USER_ACTION_IDENTITY.to_string(),
        structural_status: structural,
        compatibility,
        platform: std::env::consts::OS.to_string(),
        architecture: std::env::consts::ARCH.to_string(),
        fixture_policy: request.fixture_policy,
        scenario: request.scenario,
        instrument_bounds: request.instrument_bounds,
        deadline_retry_policy: request.deadline_retry_policy,
        cleanup_redaction_policy: request.cleanup_redaction_policy,
        incompatible_pair_policy: request.incompatible_pair_policy,
        host_capability: request.host_capability,
        expected_subject_classes: plan
            .map(|plan| plan.expected_subject_classes.clone())
            .unwrap_or_else(expected_subject_classes),
        later_observed: LaterObservedIdentities::unobserved(),
        methods_attempted: Vec::new(),
        methods_observed: Vec::new(),
        dispositions: ComponentDispositions::not_attempted(),
        terminal,
        failure_reason: FailureReason::from_preflight(disposition),
        preflight: disposition,
        evidence_class: EvidenceClass::ContractAdmission,
        claim_ceiling: ClaimCeiling::diagnostic_observation(EvidenceClass::ContractAdmission),
        redaction_state: "public_safe".to_string(),
        usage_disclosure: USAGE_DISCLOSURE.to_string(),
        limitations: vec![
            "actual Claude host session is not implemented in this contract leaf".to_string(),
            "result cannot update compatibility or support authority".to_string(),
        ],
    }
}

fn result_from_execution(
    plan: &HostTestPlan,
    request: &HostTestRequest,
    execution: InjectedExecution,
    kind: ExecutorKind,
) -> HostTestResult {
    let evidence_class = match kind {
        ExecutorKind::Reserved => EvidenceClass::ContractAdmission,
        ExecutorKind::Injected => EvidenceClass::InjectedTestExecutor,
    };
    let mut failure_reason = execution.failure_reason;
    if !execution.subjects.intended_plugin {
        failure_reason = Some(FailureReason::WrongPluginSubject);
    } else if !execution.subjects.intended_server {
        failure_reason = Some(FailureReason::WrongServerSubject);
    } else if execution.dispositions.cleanup == CellDisposition::CleanupFailed {
        failure_reason = Some(FailureReason::CleanupFailed);
    } else if execution.dispositions.host_instrument == CellDisposition::InstrumentFailure {
        failure_reason = Some(FailureReason::HostInstrumentFailed);
    } else if execution.dispositions.host_instrument != CellDisposition::ObservedPass {
        failure_reason = failure_reason.or(Some(FailureReason::InstrumentNotProven));
    } else if !required_methods_observed(&execution)
        || execution.dispositions.semantic == CellDisposition::NotAttempted
        || execution.dispositions.semantic == CellDisposition::NotProven
    {
        failure_reason = failure_reason.or(Some(FailureReason::RequiredLspToolCallNotObserved));
    }

    let terminal = derive_terminal(&execution, evidence_class, failure_reason);
    let mut limitations = vec![
        "result cannot update compatibility or support authority".to_string(),
        "result is not release or support-harness evidence".to_string(),
    ];
    if evidence_class != EvidenceClass::ActualHost {
        limitations.push("result is not actual-host evidence".to_string());
    }
    if plan.compatibility_plan_state == PreflightDisposition::CompatibilityNotProven {
        limitations.push("compatibility remains not_proven and was not coerced".to_string());
    }

    HostTestResult {
        schema_version: RESULT_SCHEMA_VERSION.to_string(),
        plan_schema_version: plan.schema_version.clone(),
        operation_identity: request.user_action.as_str().to_string(),
        structural_status: plan.structural_status.clone(),
        compatibility: plan.compatibility.clone(),
        platform: std::env::consts::OS.to_string(),
        architecture: std::env::consts::ARCH.to_string(),
        fixture_policy: plan.fixture_policy,
        scenario: plan.scenario,
        instrument_bounds: plan.instrument_bounds,
        deadline_retry_policy: plan.deadline_retry_policy,
        cleanup_redaction_policy: plan.cleanup_redaction_policy,
        incompatible_pair_policy: request.incompatible_pair_policy,
        host_capability: request.host_capability,
        expected_subject_classes: plan.expected_subject_classes.clone(),
        later_observed: LaterObservedIdentities::unobserved(),
        methods_attempted: execution
            .methods_attempted
            .iter()
            .map(|value| redact_text(value))
            .collect(),
        methods_observed: execution
            .methods_observed
            .iter()
            .map(|value| redact_text(value))
            .collect(),
        dispositions: execution.dispositions,
        terminal,
        failure_reason,
        preflight: PreflightDisposition::Admitted,
        evidence_class,
        claim_ceiling: ClaimCeiling::diagnostic_observation(evidence_class),
        redaction_state: "public_safe".to_string(),
        usage_disclosure: USAGE_DISCLOSURE.to_string(),
        limitations,
    }
}

fn derive_terminal(
    execution: &InjectedExecution,
    evidence_class: EvidenceClass,
    failure_reason: Option<FailureReason>,
) -> TerminalResult {
    if execution.dispositions.host_instrument == CellDisposition::InstrumentFailure
        || failure_reason == Some(FailureReason::HostInstrumentFailed)
    {
        return TerminalResult::InstrumentFailure;
    }
    if !execution.subjects.intended_plugin
        || !execution.subjects.intended_server
        || execution.dispositions.cleanup == CellDisposition::CleanupFailed
        || execution.dispositions.semantic != CellDisposition::ObservedPass
        || execution.dispositions.currentness != CellDisposition::ObservedPass
        || execution.dispositions.startup != CellDisposition::ObservedPass
        || execution.dispositions.plugin != CellDisposition::ObservedPass
        || execution.dispositions.server != CellDisposition::ObservedPass
    {
        if evidence_class == EvidenceClass::ContractAdmission {
            return TerminalResult::NotProven;
        }
        if execution.dispositions.semantic == CellDisposition::NotAttempted
            || execution.dispositions.cleanup == CellDisposition::NotAttempted
        {
            return TerminalResult::NotProven;
        }
        return TerminalResult::Fail;
    }
    if execution.dispositions.host_instrument != CellDisposition::ObservedPass
        || !required_methods_observed(execution)
        || (failure_reason.is_some() && failure_reason != Some(FailureReason::HostInstrumentFailed))
    {
        if evidence_class == EvidenceClass::ContractAdmission {
            return TerminalResult::NotProven;
        }
        if failure_reason.is_some()
            && execution.dispositions.host_instrument != CellDisposition::InstrumentFailure
        {
            return TerminalResult::Fail;
        }
        return TerminalResult::NotProven;
    }
    if evidence_class == EvidenceClass::ActualHost {
        TerminalResult::Pass
    } else if execution.dispositions.semantic == CellDisposition::ObservedPass
        && execution.subjects.intended_plugin
        && execution.subjects.intended_server
        && execution.dispositions.cleanup == CellDisposition::ObservedPass
    {
        // Injected executors may derive a terminal pass for contract tests, but
        // claim_ceiling.actual_host_evidence remains false.
        TerminalResult::Pass
    } else {
        TerminalResult::NotProven
    }
}

/// Render the human or JSON projection of a result.
pub fn render(result: &HostTestResult, json: bool) -> String {
    if json {
        match serde_json::to_string_pretty(&result_to_json(result)) {
            Ok(rendered) => rendered,
            Err(_) => json!({
                "schema_version": RESULT_SCHEMA_VERSION,
                "terminal": "instrument_failure",
                "failure_reason": "redaction_failed",
                "usage_disclosure": USAGE_DISCLOSURE,
            })
            .to_string(),
        }
    } else {
        render_human(result)
    }
}

fn render_human(result: &HostTestResult) -> String {
    let mut lines = Vec::new();
    lines.push(result.usage_disclosure.clone());
    lines.push(format!("Claude host test ({})", result.schema_version));
    lines.push(format!("  Preflight:   {}", result.preflight.as_str()));
    lines.push(format!("  Terminal:    {}", result.terminal.as_str()));
    lines.push(format!("  Evidence:    {}", result.evidence_class.as_str()));
    lines.push(format!("  Compatibility: {}", result.compatibility.result.as_str()));
    if let Some(reason) = result.failure_reason {
        lines.push(format!("  Failure:     {}", reason.as_str()));
    }
    lines.push(format!(
        "  Claim ceiling: release={} compat={} support={} actual_host={}",
        result.claim_ceiling.release_or_support_harness_evidence,
        result.claim_ceiling.updates_compatibility_authority,
        result.claim_ceiling.promotes_support_registry,
        result.claim_ceiling.actual_host_evidence
    ));
    for limitation in &result.limitations {
        lines.push(format!("  Limitation:  {}", redact_text(limitation)));
    }
    redact_text(&lines.join("\n"))
}

/// Deterministic JSON object independent of map insertion order.
pub fn result_to_json(result: &HostTestResult) -> Value {
    let mut object = Map::new();
    insert(&mut object, "schema_version", json!(result.schema_version));
    insert(&mut object, "plan_schema_version", json!(result.plan_schema_version));
    insert(&mut object, "operation_identity", json!(result.operation_identity));
    insert(&mut object, "usage_disclosure", json!(result.usage_disclosure));
    insert(
        &mut object,
        "structural_status",
        json!({
            "schema_version": result.structural_status.schema_version,
            "verdict": result.structural_status.verdict,
            "host_state": result.structural_status.host_state,
        }),
    );
    insert(
        &mut object,
        "compatibility",
        json!({
            "schema_version": result.compatibility.schema_version,
            "result": result.compatibility.result.as_str(),
            "reason": result.compatibility.reason.as_str(),
        }),
    );
    insert(&mut object, "platform", json!(result.platform));
    insert(&mut object, "architecture", json!(result.architecture));
    insert(&mut object, "fixture_policy", json!(result.fixture_policy.as_str()));
    insert(&mut object, "scenario", json!(result.scenario.as_str()));
    insert(&mut object, "instrument_bounds", json!(result.instrument_bounds.as_str()));
    insert(&mut object, "deadline_retry_policy", json!(result.deadline_retry_policy.as_str()));
    insert(
        &mut object,
        "cleanup_redaction_policy",
        json!(result.cleanup_redaction_policy.as_str()),
    );
    insert(
        &mut object,
        "incompatible_pair_policy",
        json!(result.incompatible_pair_policy.as_str()),
    );
    insert(
        &mut object,
        "host_capability",
        match result.host_capability {
            Some(capability) => json!(capability.as_str()),
            None => json!("unobserved"),
        },
    );
    insert(&mut object, "expected_subject_classes", json!(result.expected_subject_classes));
    insert(&mut object, "later_observed", later_observed_json(&result.later_observed));
    insert(&mut object, "methods_attempted", json!(result.methods_attempted));
    insert(&mut object, "methods_observed", json!(result.methods_observed));
    insert(
        &mut object,
        "dispositions",
        json!({
            "startup": result.dispositions.startup.as_str(),
            "plugin": result.dispositions.plugin.as_str(),
            "server": result.dispositions.server.as_str(),
            "semantic": result.dispositions.semantic.as_str(),
            "currentness": result.dispositions.currentness.as_str(),
            "cleanup": result.dispositions.cleanup.as_str(),
            "host_instrument": result.dispositions.host_instrument.as_str(),
        }),
    );
    insert(&mut object, "terminal", json!(result.terminal.as_str()));
    insert(
        &mut object,
        "failure_reason",
        match result.failure_reason {
            Some(reason) => json!(reason.as_str()),
            None => Value::Null,
        },
    );
    insert(&mut object, "preflight", json!(result.preflight.as_str()));
    insert(&mut object, "evidence_class", json!(result.evidence_class.as_str()));
    insert(
        &mut object,
        "claim_ceiling",
        json!({
            "release_or_support_harness_evidence": result.claim_ceiling.release_or_support_harness_evidence,
            "updates_compatibility_authority": result.claim_ceiling.updates_compatibility_authority,
            "promotes_support_registry": result.claim_ceiling.promotes_support_registry,
            "actual_host_evidence": result.claim_ceiling.actual_host_evidence,
        }),
    );
    insert(&mut object, "redaction_state", json!(result.redaction_state));
    insert(
        &mut object,
        "limitations",
        json!(result.limitations.iter().map(|line| redact_text(line)).collect::<Vec<_>>()),
    );
    Value::Object(object)
}

fn insert(object: &mut Map<String, Value>, key: &str, value: Value) {
    object.insert(key.to_string(), value);
}

fn expected_subject_classes() -> Vec<String> {
    vec![
        "claude_code_host".to_string(),
        "plugin:perl-lsp-rs".to_string(),
        "server:perllsp".to_string(),
        "operation_created_temporary_fixture".to_string(),
    ]
}

const REQUIRED_METHOD_CELLS: [&str; 4] =
    ["definition", "references", "hover_or_document_symbols", "edit_requery"];

fn required_methods_observed(execution: &InjectedExecution) -> bool {
    REQUIRED_METHOD_CELLS
        .iter()
        .all(|method| execution.methods_observed.iter().any(|observed| observed == method))
}

fn later_observed_json(identities: &LaterObservedIdentities) -> Value {
    let state = if identities.is_unobserved() { "unobserved" } else { "observed" };
    json!({
        "state": state,
        "claude_host": {
            "version": identities.claude_host_version,
            "source_class": identities.claude_host_source_class,
        },
        "plugin": {
            "slug": identities.plugin_slug,
            "source": identities.plugin_source,
            "version": identities.plugin_version,
            "tree": identities.plugin_tree,
            "package": identities.plugin_package,
        },
        "server": {
            "path_role": identities.server_path_role,
            "version": identities.server_version,
            "build": identities.server_build,
            "digest": identities.server_digest,
        },
    })
}

fn redact_text(value: &str) -> String {
    let mut redacted = value.to_string();
    for marker in PRIVATE_CANARY_MARKERS {
        if redacted.contains(marker) {
            redacted = redacted.replace(marker, "[redacted]");
        }
    }
    redacted
}

/// True when a rendered projection still contains a private canary.
#[cfg(test)]
fn contains_private_canary(value: &str) -> bool {
    PRIVATE_CANARY_MARKERS.iter().any(|marker| value.contains(marker))
}

/// Plan JSON used to prove the plan never carries hostile authority fields.
#[cfg(test)]
fn plan_to_json(plan: &HostTestPlan) -> Value {
    json!({
        "schema_version": plan.schema_version,
        "user_action": plan.user_action.as_str(),
        "structural_status": {
            "schema_version": plan.structural_status.schema_version,
            "verdict": plan.structural_status.verdict,
            "host_state": plan.structural_status.host_state,
        },
        "compatibility": {
            "schema_version": plan.compatibility.schema_version,
            "result": plan.compatibility.result.as_str(),
            "reason": plan.compatibility.reason.as_str(),
        },
        "scenario": plan.scenario.as_str(),
        "fixture_policy": plan.fixture_policy.as_str(),
        "expected_subject_classes": plan.expected_subject_classes,
        "instrument_bounds": plan.instrument_bounds.as_str(),
        "deadline_retry_policy": plan.deadline_retry_policy.as_str(),
        "cleanup_redaction_policy": plan.cleanup_redaction_policy.as_str(),
        "compatibility_plan_state": plan.compatibility_plan_state.as_str(),
        "claim_ceiling": {
            "release_or_support_harness_evidence": plan.claim_ceiling.release_or_support_harness_evidence,
            "updates_compatibility_authority": plan.claim_ceiling.updates_compatibility_authority,
            "promotes_support_registry": plan.claim_ceiling.promotes_support_registry,
            "actual_host_evidence": plan.claim_ceiling.actual_host_evidence,
        },
    })
}

#[cfg(test)]
mod claude_host_test_contract {
    use super::*;
    use perl_test_must::must_some_with;
    use perllsp::claude_compat::{CompatibilityReason, CompatibilityResult};
    use std::collections::BTreeMap;

    struct CountingExecutor {
        kind: ExecutorKind,
        calls: usize,
        last_plan: Option<HostTestPlan>,
        execution: InjectedExecution,
    }

    impl CountingExecutor {
        fn reserved() -> Self {
            Self {
                kind: ExecutorKind::Reserved,
                calls: 0,
                last_plan: None,
                execution: InjectedExecution {
                    dispositions: ComponentDispositions::not_attempted(),
                    subjects: SubjectObservation::unobserved(),
                    methods_attempted: Vec::new(),
                    methods_observed: Vec::new(),
                    failure_reason: Some(FailureReason::InstrumentNotProven),
                },
            }
        }

        fn injected(execution: InjectedExecution) -> Self {
            Self { kind: ExecutorKind::Injected, calls: 0, last_plan: None, execution }
        }
    }

    impl HostTestExecutor for CountingExecutor {
        fn kind(&self) -> ExecutorKind {
            self.kind
        }

        fn execute(&mut self, plan: &HostTestPlan) -> InjectedExecution {
            self.calls += 1;
            self.last_plan = Some(plan.clone());
            self.execution.clone()
        }
    }

    fn admitted_request(result: CompatibilityResult) -> HostTestRequest {
        let mut request = HostTestRequest::from_cli();
        request.structural_status = Some(StructuralStatusIdentity {
            schema_version: STRUCTURAL_STATUS_SCHEMA.to_string(),
            verdict: "degraded".to_string(),
            host_state: "present".to_string(),
        });
        request.host_capability = Some(HostCapability::Available);
        request.compatibility = CompatibilityIdentity {
            schema_version: COMPAT_SCHEMA.to_string(),
            result,
            reason: match result {
                CompatibilityResult::Compatible => CompatibilityReason::ExactEvidence,
                CompatibilityResult::Incompatible => CompatibilityReason::ExactKnownBad,
                CompatibilityResult::NotProven => CompatibilityReason::ExactPairNotEstablished,
            },
        };
        request
    }

    #[test]
    fn ordinary_surfaces_invoke_zero_host_test_executor_calls() {
        let mut executor = CountingExecutor::reserved();
        for surface in [
            ProductSurface::OrdinaryDoctor,
            ProductSurface::OrdinarySetup,
            ProductSurface::OrdinaryStatus,
            ProductSurface::ShellCompletion,
            ProductSurface::DoctorClientClaude,
        ] {
            let outcome = dispatch(surface, &mut executor);
            assert_eq!(outcome.executor_calls, 0, "{surface:?}");
            assert!(!outcome.usage_disclosed_before_executor, "{surface:?}");
            assert!(outcome.result.is_none(), "{surface:?}");
        }
        assert_eq!(executor.calls, 0);
    }

    #[test]
    fn explicit_host_test_invokes_exactly_one_admitted_plan() {
        let mut executor = CountingExecutor::injected(InjectedExecution::all_observed_pass());
        let outcome = dispatch_request(
            &admitted_request(CompatibilityResult::Compatible),
            true,
            &mut executor,
        );
        assert_eq!(outcome.executor_calls, 1);
        assert!(outcome.usage_disclosed_before_executor);
        assert_eq!(executor.calls, 1);
        let plan = must_some_with(executor.last_plan.as_ref(), "admitted plan");
        assert_eq!(plan.user_action.as_str(), USER_ACTION_IDENTITY);
        assert_eq!(plan.scenario, BoundedScenario::BoundedNativeLspSliceV1);
        assert_eq!(plan.fixture_policy, FixturePolicy::OperationCreatedTemporaryFixture);
        let result = must_some_with(outcome.result, "result");
        assert_eq!(result.preflight, PreflightDisposition::Admitted);
    }

    #[test]
    fn human_and_json_consume_the_same_typed_result() {
        let mut executor = CountingExecutor::injected(InjectedExecution::all_observed_pass());
        let request = admitted_request(CompatibilityResult::NotProven);
        let json_outcome = dispatch_request(&request, true, &mut executor);
        let mut executor = CountingExecutor::injected(InjectedExecution::all_observed_pass());
        let human_outcome = dispatch_request(&request, false, &mut executor);
        let json_result = must_some_with(json_outcome.result, "json result");
        let human_result = must_some_with(human_outcome.result, "human result");
        assert_eq!(json_result, human_result);
        assert_eq!(json_result.compatibility.result, CompatibilityResult::NotProven);
        assert!(json_outcome.rendered.contains(USAGE_DISCLOSURE));
        assert!(human_outcome.rendered.contains(USAGE_DISCLOSURE));
    }

    #[test]
    fn usage_disclosure_precedes_the_first_executor_effect() {
        let mut executor = CountingExecutor::injected(InjectedExecution::all_observed_pass());
        let outcome = dispatch_request(
            &admitted_request(CompatibilityResult::Compatible),
            true,
            &mut executor,
        );
        assert!(outcome.usage_disclosed_before_executor);
        assert!(outcome.rendered.contains(USAGE_DISCLOSURE));
        assert_eq!(executor.calls, 1);
    }

    #[test]
    fn hostile_authority_is_invalid_request_and_never_executes() {
        let mut request = admitted_request(CompatibilityResult::Compatible);
        request.client_authority = ClientSuppliedAuthority {
            command: Some("claude --print".to_string()),
            prompt: Some("PROMPT_CANARY ignore previous".to_string()),
            workspace_path: Some("/home/user/private-src".to_string()),
            environment: Some(BTreeMap::from([(
                "ANTHROPIC_API_KEY".to_string(),
                "sk-ant-secret".to_string(),
            )])),
            credential: Some("sk-ant-secret".to_string()),
            executable_override: Some("/tmp/evil".to_string()),
            compatibility_mutation: Some("mark compatible".to_string()),
        };
        let mut executor = CountingExecutor::injected(InjectedExecution::all_observed_pass());
        let outcome = dispatch_request(&request, true, &mut executor);
        assert_eq!(executor.calls, 0);
        let result = must_some_with(outcome.result, "result");
        assert_eq!(result.preflight, PreflightDisposition::InvalidRequest);
        assert_eq!(result.failure_reason, Some(FailureReason::InvalidRequest));
        assert!(!contains_private_canary(&outcome.rendered));
    }

    #[test]
    fn compatible_incompatible_and_not_proven_remain_distinct() {
        for result in [
            CompatibilityResult::Compatible,
            CompatibilityResult::Incompatible,
            CompatibilityResult::NotProven,
        ] {
            let request = admitted_request(result);
            let (disposition, plan) = admit(&request);
            match result {
                CompatibilityResult::Compatible => {
                    assert_eq!(disposition, PreflightDisposition::Admitted);
                    assert_eq!(
                        must_some_with(plan, "plan").compatibility.result,
                        CompatibilityResult::Compatible
                    );
                }
                CompatibilityResult::NotProven => {
                    assert_eq!(disposition, PreflightDisposition::Admitted);
                    let plan = must_some_with(plan, "plan");
                    assert_eq!(plan.compatibility.result, CompatibilityResult::NotProven);
                    assert_eq!(
                        plan.compatibility_plan_state,
                        PreflightDisposition::CompatibilityNotProven
                    );
                }
                CompatibilityResult::Incompatible => {
                    assert_eq!(
                        disposition,
                        PreflightDisposition::CompatibilityIncompatibleRequiresExplicitOverrideOrRefusal
                    );
                    assert_eq!(
                        must_some_with(plan, "plan").compatibility.result,
                        CompatibilityResult::Incompatible
                    );
                }
            }
        }
        assert_ne!(
            CompatibilityResult::Compatible.as_str(),
            CompatibilityResult::Incompatible.as_str()
        );
        assert_ne!(
            CompatibilityResult::Compatible.as_str(),
            CompatibilityResult::NotProven.as_str()
        );
        assert_ne!(
            CompatibilityResult::Incompatible.as_str(),
            CompatibilityResult::NotProven.as_str()
        );
    }

    #[test]
    fn incompatible_pair_does_not_run_without_explicit_override() {
        let mut executor = CountingExecutor::injected(InjectedExecution::all_observed_pass());
        let outcome = dispatch_request(
            &admitted_request(CompatibilityResult::Incompatible),
            true,
            &mut executor,
        );
        assert_eq!(executor.calls, 0);
        let result = must_some_with(outcome.result, "result");
        assert_eq!(
            result.preflight,
            PreflightDisposition::CompatibilityIncompatibleRequiresExplicitOverrideOrRefusal
        );
        assert_eq!(result.failure_reason, Some(FailureReason::CompatibilityIncompatible));
    }

    #[test]
    fn explicit_incompatible_override_admits_without_coercing_compatibility() {
        let mut request = admitted_request(CompatibilityResult::Incompatible);
        request.incompatible_pair_policy =
            IncompatiblePairPolicy::ExplicitReviewedDiagnosticOverride;
        let (disposition, plan) = admit(&request);
        assert_eq!(disposition, PreflightDisposition::Admitted);
        let plan = must_some_with(plan, "plan");
        assert_eq!(plan.compatibility.result, CompatibilityResult::Incompatible);
        assert_eq!(plan.compatibility_plan_state, PreflightDisposition::Admitted);
    }

    #[test]
    fn instrument_failure_is_not_product_failure_or_pass() {
        let mut execution = InjectedExecution::all_observed_pass();
        execution.dispositions.host_instrument = CellDisposition::InstrumentFailure;
        execution.failure_reason = Some(FailureReason::HostInstrumentFailed);
        let mut executor = CountingExecutor::injected(execution);
        let outcome = dispatch_request(
            &admitted_request(CompatibilityResult::Compatible),
            true,
            &mut executor,
        );
        let result = must_some_with(outcome.result, "result");
        assert_eq!(result.terminal, TerminalResult::InstrumentFailure);
        assert_ne!(result.terminal, TerminalResult::Fail);
        assert_ne!(result.terminal, TerminalResult::Pass);
        assert_eq!(result.failure_reason, Some(FailureReason::HostInstrumentFailed));
    }

    #[test]
    fn missing_semantic_or_cleanup_cells_cannot_pass() {
        let mut missing_semantic = InjectedExecution::all_observed_pass();
        missing_semantic.dispositions.semantic = CellDisposition::NotAttempted;
        let mut executor = CountingExecutor::injected(missing_semantic);
        let outcome = dispatch_request(
            &admitted_request(CompatibilityResult::Compatible),
            true,
            &mut executor,
        );
        assert_ne!(must_some_with(outcome.result, "result").terminal, TerminalResult::Pass);

        let mut cleanup_failed = InjectedExecution::all_observed_pass();
        cleanup_failed.dispositions.cleanup = CellDisposition::CleanupFailed;
        let mut executor = CountingExecutor::injected(cleanup_failed);
        let outcome = dispatch_request(
            &admitted_request(CompatibilityResult::Compatible),
            true,
            &mut executor,
        );
        let result = must_some_with(outcome.result, "result");
        assert_ne!(result.terminal, TerminalResult::Pass);
        assert_eq!(result.failure_reason, Some(FailureReason::CleanupFailed));
    }

    #[test]
    fn wrong_subject_cannot_be_represented_pass() {
        let mut wrong_plugin = InjectedExecution::all_observed_pass();
        wrong_plugin.subjects.intended_plugin = false;
        let mut executor = CountingExecutor::injected(wrong_plugin);
        let outcome = dispatch_request(
            &admitted_request(CompatibilityResult::Compatible),
            true,
            &mut executor,
        );
        let result = must_some_with(outcome.result, "result");
        assert_ne!(result.terminal, TerminalResult::Pass);
        assert_eq!(result.failure_reason, Some(FailureReason::WrongPluginSubject));

        let mut wrong_server = InjectedExecution::all_observed_pass();
        wrong_server.subjects.intended_server = false;
        let mut executor = CountingExecutor::injected(wrong_server);
        let outcome = dispatch_request(
            &admitted_request(CompatibilityResult::Compatible),
            true,
            &mut executor,
        );
        let result = must_some_with(outcome.result, "result");
        assert_ne!(result.terminal, TerminalResult::Pass);
        assert_eq!(result.failure_reason, Some(FailureReason::WrongServerSubject));
    }

    #[test]
    fn result_serialization_is_deterministic_independent_of_map_order() {
        let mut executor = CountingExecutor::injected(InjectedExecution::all_observed_pass());
        let outcome = dispatch_request(
            &admitted_request(CompatibilityResult::NotProven),
            true,
            &mut executor,
        );
        let result = must_some_with(outcome.result, "result");
        let first =
            must_some_with(serde_json::to_string(&result_to_json(&result)).ok(), "first json");
        let second =
            must_some_with(serde_json::to_string(&result_to_json(&result)).ok(), "second json");
        assert_eq!(first, second);
        let value: Value = must_some_with(serde_json::from_str(&first).ok(), "parsed json");
        assert_eq!(value["schema_version"], RESULT_SCHEMA_VERSION);
        assert_eq!(value["compatibility"]["result"], "not_proven");
    }

    #[test]
    fn private_canaries_do_not_enter_human_or_json_output() {
        let mut request = admitted_request(CompatibilityResult::Compatible);
        request.client_authority.prompt = Some("PROMPT_CANARY secret prose".to_string());
        request.client_authority.credential = Some("sk-ant-secret-token".to_string());
        request.client_authority.workspace_path = Some("/home/user/SOURCE_CANARY".to_string());
        request.client_authority.environment =
            Some(BTreeMap::from([("ANTHROPIC_API_KEY".to_string(), "sk-ant-secret".to_string())]));
        let mut executor = CountingExecutor::injected(InjectedExecution::all_observed_pass());
        let json_outcome = dispatch_request(&request, true, &mut executor);
        let mut executor = CountingExecutor::injected(InjectedExecution::all_observed_pass());
        let human_outcome = dispatch_request(&request, false, &mut executor);
        assert!(!contains_private_canary(&json_outcome.rendered));
        assert!(!contains_private_canary(&human_outcome.rendered));
        assert!(!json_outcome.rendered.contains("PROMPT_CANARY"));
        assert!(!human_outcome.rendered.contains("SOURCE_CANARY"));
    }

    #[test]
    fn injected_result_cannot_be_relabelled_actual_host_evidence() {
        let mut executor = CountingExecutor::injected(InjectedExecution::all_observed_pass());
        let outcome = dispatch_request(
            &admitted_request(CompatibilityResult::Compatible),
            true,
            &mut executor,
        );
        let result = must_some_with(outcome.result, "result");
        assert_eq!(result.evidence_class.as_str(), "injected_test_executor");
        assert!(!result.claim_ceiling.actual_host_evidence);
        assert!(result.relabel_as_actual_host().is_err());
        let ceiling = ClaimCeiling::diagnostic_observation(result.evidence_class);
        assert!(!ceiling.release_or_support_harness_evidence);
        assert!(!ceiling.updates_compatibility_authority);
        assert!(!ceiling.promotes_support_registry);
    }

    #[test]
    fn reserved_executor_never_claims_actual_host_or_promotes_support() {
        let mut executor = ReservedExecutor;
        let outcome =
            dispatch(ProductSurface::DoctorClientClaudeHostTest { json: true }, &mut executor);
        let result = must_some_with(outcome.result, "result");
        assert_eq!(result.preflight, PreflightDisposition::InstrumentNotProven);
        assert_eq!(result.evidence_class.as_str(), "contract_admission");
        assert_eq!(result.terminal, TerminalResult::NotProven);
        assert!(!result.claim_ceiling.actual_host_evidence);
        assert!(!result.claim_ceiling.updates_compatibility_authority);
        assert!(!result.claim_ceiling.promotes_support_registry);
        assert!(!result.claim_ceiling.release_or_support_harness_evidence);
        assert!(result.later_observed.is_unobserved());
        assert_eq!(result.host_capability, None);
        assert!(outcome.rendered.contains(USAGE_DISCLOSURE));
        assert_eq!(outcome.executor_calls, 0);
        let value = result_to_json(&result);
        assert_eq!(value["later_observed"]["state"], "unobserved");
        assert_eq!(value["host_capability"], "unobserved");
        assert_eq!(value["scenario"], BoundedScenario::BoundedNativeLspSliceV1.as_str());
    }

    #[test]
    fn plan_rejects_prompt_credential_env_and_path_authority() {
        let request = admitted_request(CompatibilityResult::Compatible);
        let (_, plan) = admit(&request);
        let plan = must_some_with(plan, "plan");
        let json = plan_to_json(&plan).to_string();
        assert!(!json.contains("prompt"));
        assert!(!json.contains("credential"));
        assert!(!json.contains("ANTHROPIC_API_KEY"));
        assert!(!json.contains("workspace_path"));
        assert!(!json.contains("executable_override"));
    }

    #[test]
    fn structural_and_capability_refusals_are_distinct() {
        let mut structural = admitted_request(CompatibilityResult::Compatible);
        structural.structural_status = Some(StructuralStatusIdentity {
            schema_version: STRUCTURAL_STATUS_SCHEMA.to_string(),
            verdict: "action_required".to_string(),
            host_state: "present".to_string(),
        });
        let (disposition, _) = admit(&structural);
        assert_eq!(disposition, PreflightDisposition::StructuralActionRequired);

        let mut host = admitted_request(CompatibilityResult::Compatible);
        host.host_capability = Some(HostCapability::HostOrAuthUnavailable);
        let (disposition, _) = admit(&host);
        assert_eq!(disposition, PreflightDisposition::HostOrAuthUnavailable);

        let mut noninteractive = admitted_request(CompatibilityResult::Compatible);
        noninteractive.host_capability = Some(HostCapability::NoninteractiveUnavailable);
        let (disposition, _) = admit(&noninteractive);
        assert_eq!(disposition, PreflightDisposition::NoninteractiveSurfaceUnavailable);

        let mut unsupported = admitted_request(CompatibilityResult::Compatible);
        unsupported.platform_supported = false;
        let (disposition, _) = admit(&unsupported);
        assert_eq!(disposition, PreflightDisposition::UnsupportedPlatformOrProfile);

        let mut structural_unsupported = admitted_request(CompatibilityResult::Compatible);
        structural_unsupported.structural_status = Some(StructuralStatusIdentity {
            schema_version: STRUCTURAL_STATUS_SCHEMA.to_string(),
            verdict: "unsupported".to_string(),
            host_state: "present".to_string(),
        });
        let (disposition, _) = admit(&structural_unsupported);
        assert_eq!(disposition, PreflightDisposition::UnsupportedPlatformOrProfile);

        let mut instrument_error = admitted_request(CompatibilityResult::Compatible);
        instrument_error.structural_status = Some(StructuralStatusIdentity {
            schema_version: STRUCTURAL_STATUS_SCHEMA.to_string(),
            verdict: "instrument_error".to_string(),
            host_state: "error".to_string(),
        });
        let mut executor = CountingExecutor::injected(InjectedExecution::all_observed_pass());
        let outcome = dispatch_request(&instrument_error, true, &mut executor);
        assert_eq!(executor.calls, 0);
        assert_eq!(
            must_some_with(outcome.result, "result").preflight,
            PreflightDisposition::InstrumentNotProven
        );
    }

    #[test]
    fn missing_method_or_instrument_evidence_cannot_pass() {
        let mut missing_methods = InjectedExecution::all_observed_pass();
        missing_methods.methods_observed.clear();
        let mut executor = CountingExecutor::injected(missing_methods);
        let outcome = dispatch_request(
            &admitted_request(CompatibilityResult::Compatible),
            true,
            &mut executor,
        );
        let result = must_some_with(outcome.result, "result");
        assert_ne!(result.terminal, TerminalResult::Pass);
        assert_eq!(result.failure_reason, Some(FailureReason::RequiredLspToolCallNotObserved));

        let mut unproven_instrument = InjectedExecution::all_observed_pass();
        unproven_instrument.dispositions.host_instrument = CellDisposition::NotProven;
        let mut executor = CountingExecutor::injected(unproven_instrument);
        let outcome = dispatch_request(
            &admitted_request(CompatibilityResult::Compatible),
            true,
            &mut executor,
        );
        let result = must_some_with(outcome.result, "result");
        assert_ne!(result.terminal, TerminalResult::Pass);
        assert_eq!(result.failure_reason, Some(FailureReason::InstrumentNotProven));
        assert_ne!(result.terminal, TerminalResult::InstrumentFailure);

        let mut explicit_fail = InjectedExecution::all_observed_pass();
        explicit_fail.failure_reason = Some(FailureReason::SemanticMismatch);
        let mut executor = CountingExecutor::injected(explicit_fail);
        let outcome = dispatch_request(
            &admitted_request(CompatibilityResult::Compatible),
            true,
            &mut executor,
        );
        let result = must_some_with(outcome.result, "result");
        assert_ne!(result.terminal, TerminalResult::Pass);
        assert_eq!(result.failure_reason, Some(FailureReason::SemanticMismatch));
    }

    #[test]
    fn executor_provided_private_methods_are_redacted() {
        let mut execution = InjectedExecution::all_observed_pass();
        execution.methods_observed[0] = "definition PROMPT_CANARY /home/user".to_string();
        let mut executor = CountingExecutor::injected(execution);
        let outcome = dispatch_request(
            &admitted_request(CompatibilityResult::Compatible),
            true,
            &mut executor,
        );
        assert!(!contains_private_canary(&outcome.rendered));
        let result = must_some_with(outcome.result, "result");
        assert!(!result.methods_observed.iter().any(|method| contains_private_canary(method)));
    }
}
