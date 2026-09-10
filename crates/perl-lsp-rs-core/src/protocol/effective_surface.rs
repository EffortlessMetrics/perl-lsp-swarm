//! Typed [`EffectiveLspSurface`] — the single deterministic final-surface
//! authority (#9665, #8032 train stage S02).
//!
//! One pure model consumes typed inputs from their canonical owners
//! (normalized client capability facts, the selected feature profile,
//! accepted configuration generation, reviewed runtime availability,
//! position/text contracts, command descriptors, explicit compatibility
//! exceptions) and produces the complete intended final surface for exactly
//! one client/build/profile/configuration subject:
//!
//! - the exact final `InitializeResult.serverCapabilities` projection;
//! - one dynamic registration/unregistration **plan** (never active state);
//! - the effective feature/method/command identities;
//! - typed suppression/downgrade/compatibility reasons;
//! - the selected position and text-sync contracts.
//!
//! Governing distinctions preserved here (#8032): client capability input,
//! server implementation capability, static advertisement, planned dynamic
//! registration, pending reverse request, active client-accepted
//! registration, provider/runtime readiness, semantic result success and
//! public support claims stay separate types. A planned registration is not
//! active; a capability may honestly advertise while a later semantic request
//! returns typed unavailable; absence/false/malformed/unknown-future client
//! inputs remain distinct classes.
//!
//! Train boundary (S02): the model is deterministic and pure — it never reads
//! ambient process state, probes tools, parses raw initialize JSON, inspects
//! provider source files, or infers support from dependency presence. Live
//! `handle_initialize`, the static builder
//! ([`crate::protocol::capabilities`]) and registration execution remain the
//! shipped routes until S03/S04 cut over; until then this module is the
//! intended authority proven equivalent through independent parity tests
//! (this module's `tests`, plus the `perl-lsp-rs` lifecycle parity module).

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::features::flags::BuildFlags;
use crate::features::policy::FeatureProfile;

/// Schema version of this model's inputs/output binding.
pub const EFFECTIVE_SURFACE_SCHEMA_VERSION: u64 = 1;

/// Controlling leaf issue for this model.
pub const EFFECTIVE_SURFACE_ISSUE: &str = "#9665";

/// Parent architecture train controller.
pub const TRAIN_CONTROLLER: &str = "#8032";

/// A normalized client capability fact for one consumed selector.
///
/// These classes are deliberately distinct: collapsing them loses the
/// ability to prove why a surface was withheld (#9665 rule 6). The runtime
/// parser currently collapses every non-supported class to "false"; the
/// model records the richer class while producing the same final surface.
///
/// Two predicates matter and stay separate:
/// - [`ClientFact::is_present`] — the selector appeared on the wire at all
///   (presence gates, e.g. `textDocument/inlineCompletion`);
/// - [`ClientFact::is_supported`] — the client affirmatively declared
///   support (boolean gates, e.g. `dynamicRegistration`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum ClientFact {
    /// The client did not send the key at all.
    #[default]
    Absent,
    /// The client sent the key with an explicit `false`.
    DeclaredFalse,
    /// The key was present but had the wrong shape/type (e.g. string where a
    /// boolean is specified). Never treated as support.
    Malformed,
    /// The key was present with a recognized-but-unhandled future variant
    /// payload. Never treated as support.
    UnsupportedFuture,
    /// The client declared support in the expected shape (or, for
    /// presence-only selectors, sent the capability object).
    Supported,
}

impl ClientFact {
    /// Whether the selector appeared on the wire in any shape.
    ///
    /// Presence gates (inline-completion declaration, pull-diagnostics
    /// declaration) admit every non-`Absent` class, mirroring the shipped
    /// wire rule while keeping the richer class recorded.
    #[must_use]
    pub fn is_present(self) -> bool {
        !matches!(self, Self::Absent)
    }

    /// Whether this fact admits the corresponding client behavior.
    ///
    /// Only [`ClientFact::Supported`] counts; absence/false/malformed/
    /// unknown-future never collapse to true.
    #[must_use]
    pub fn is_supported(self) -> bool {
        matches!(self, Self::Supported)
    }
}

/// File-operation participation facts the client declared for initialize
/// (#7682). Each operation is negotiated independently.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct FileOperationFacts {
    /// `workspace.fileOperations.willCreate`.
    pub will_create: ClientFact,
    /// `workspace.fileOperations.didCreate`.
    pub did_create: ClientFact,
    /// `workspace.fileOperations.willRename`.
    pub will_rename: ClientFact,
    /// `workspace.fileOperations.didRename`.
    pub did_rename: ClientFact,
    /// `workspace.fileOperations.willDelete`.
    pub will_delete: ClientFact,
    /// `workspace.fileOperations.didDelete`.
    pub did_delete: ClientFact,
}

/// Refresh-request support facts (`workspace/*/refreshSupport`).
///
/// Spelling deviations (spec-plural `diagnostics` vs client-deviant
/// `diagnostic`) are resolved upstream of this model by the #6735
/// normalization owner; exactly one fact per family arrives here.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct RefreshSupportFacts {
    /// `workspace.codeLens.refreshSupport`.
    pub code_lens: ClientFact,
    /// `workspace.semanticTokens.refreshSupport`.
    pub semantic_tokens: ClientFact,
    /// `workspace.inlayHint.refreshSupport`.
    pub inlay_hint: ClientFact,
    /// `workspace.inlineValue.refreshSupport`.
    pub inline_value: ClientFact,
    /// `workspace.diagnostics.refreshSupport` (either accepted spelling).
    pub diagnostic: ClientFact,
    /// `workspace.foldingRange.refreshSupport`.
    pub folding_range: ClientFact,
}

/// Reviewed runtime availability inputs.
///
/// Native formatter/critic capability does not depend on optional external
/// Perl::Tidy/Perl::Critic presence (`FeatureProfile::runtime_flags` ignores
/// tool availability), so there is deliberately no tool-availability input:
/// the model performs no ambient PATH/tool probe by construction. The fields
/// below are the only reviewed runtime inputs that legitimately affect the
/// final surface today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct RuntimeAvailability {
    /// Runtime tuning gate for dynamic file-watcher registration
    /// (`runtime_tuning.file_watchers`); false suppresses the watcher
    /// registration plan without touching any advertised capability.
    pub file_watchers_enabled: bool,
}

impl Default for RuntimeAvailability {
    fn default() -> Self {
        Self { file_watchers_enabled: true }
    }
}

/// Compatibility exceptions admitted by bounded client-identity evidence.
///
/// Evidence, reason and expiry are intrinsic, exact constants of each known
/// exception — an exception cannot exist without them (#9665 negative
/// control). Admission is an explicit typed input; the model applies only
/// admitted exceptions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum KnownException {
    /// OpenCode declares `textDocument.diagnostic` but relies on push
    /// diagnostics; pull gating is suppressed so diagnostics are not lost.
    OpenCodePushDiagnosticsRetention,
    /// JetBrains-family clients claim dynamic watcher registration but their
    /// registration flow is unreliable; it is force-disabled (#4630-era).
    JetBrainsWatcherForceDisable,
}

impl KnownException {
    /// Exact subject predicate (client identity evidence) for admission.
    #[must_use]
    pub fn subject_evidence(self) -> &'static str {
        match self {
            Self::OpenCodePushDiagnosticsRetention => {
                "clientInfo.name =~ /opencode/i && textDocument.diagnostic advertised"
            }
            Self::JetBrainsWatcherForceDisable => "clientInfo.name =~ /(jetbrains|intellij|idea)/i",
        }
    }

    /// Why the exception exists.
    #[must_use]
    pub fn reason(self) -> &'static str {
        match self {
            Self::OpenCodePushDiagnosticsRetention => {
                "OpenCode relies on push publishDiagnostics even while declaring \
                 textDocument.diagnostic; pull gating suppressed to avoid losing diagnostics"
            }
            Self::JetBrainsWatcherForceDisable => {
                "dynamic watcher registration flow is unreliable and degrades startup; \
                 forced off regardless of declaration"
            }
        }
    }

    /// Explicit exit condition; never silently permanent.
    #[must_use]
    pub fn expiry(self) -> &'static str {
        match self {
            Self::OpenCodePushDiagnosticsRetention => {
                "revisit under #6735 negotiation matrix when OpenCode consumes pull diagnostics"
            }
            Self::JetBrainsWatcherForceDisable => {
                "until EffectiveLspSurface cutover (S03/S04) retires the runtime override \
                 with typed compatibility policy (#9665)"
            }
        }
    }
}

/// A capability family tracked by the model.
///
/// One family has exactly one [`FamilyOutcome`] per subject; incompatible
/// static+dynamic coexistence for one selector is unrepresentable because
/// selection yields a single outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum CapabilityFamily {
    /// `completionProvider`.
    Completion,
    /// `hoverProvider`.
    Hover,
    /// `definitionProvider`.
    Definition,
    /// `typeDefinitionProvider`.
    TypeDefinition,
    /// `implementationProvider`.
    Implementation,
    /// `referencesProvider`.
    References,
    /// `documentSymbolProvider`.
    DocumentSymbol,
    /// `documentHighlightProvider`.
    DocumentHighlight,
    /// `workspaceSymbolProvider` (simple or resolve-options shape).
    WorkspaceSymbol,
    /// `notebookDocumentSync`.
    NotebookDocumentSync,
    /// `foldingRangeProvider`.
    FoldingRange,
    /// `inlayHintProvider`.
    InlayHint,
    /// `diagnosticProvider` (pull diagnostics).
    PullDiagnostic,
    /// `semanticTokensProvider`.
    SemanticTokens,
    /// `codeActionProvider` (provider object and kinds).
    CodeAction,
    /// `codeActionProvider.documentation` insertion (client-gated sub-surface).
    CodeActionDocumentation,
    /// `executeCommandProvider` (host targets only).
    ExecuteCommand,
    /// `documentFormattingProvider`.
    DocumentFormatting,
    /// `documentRangeFormattingProvider` (+LSP 3.18 `rangesSupport`).
    RangeFormatting,
    /// `renameProvider`.
    Rename,
    /// `documentOnTypeFormattingProvider`.
    OnTypeFormatting,
    /// `linkedEditingRangeProvider`.
    LinkedEditingRange,
    /// `signatureHelpProvider`.
    SignatureHelp,
    /// `codeLensProvider`.
    CodeLens,
    /// `inlineValueProvider`.
    InlineValue,
    /// `monikerProvider`.
    Moniker,
    /// `colorProvider` (document color).
    DocumentColor,
    /// `callHierarchyProvider`.
    CallHierarchy,
    /// `declarationProvider`.
    Declaration,
    /// `documentLinkProvider`.
    DocumentLink,
    /// `selectionRangeProvider`.
    SelectionRange,
    /// `typeHierarchyProvider` (experimental marker + top-level LSP 3.18).
    TypeHierarchy,
    /// `inlineCompletionProvider` (LSP 3.18; static/dynamic arbitration).
    InlineCompletion,
    /// `positionEncoding` (v0.18 UTF-16-only wire encoding).
    PositionEncoding,
    /// `textDocumentSync` (runtime-owned authoritative shape).
    TextDocumentSync,
    /// `workspace.*` (folders, file operations, textDocumentContent).
    Workspace,
    /// `experimental.perlInlineCompletionStream` custom-request advertisement.
    ExperimentalInlineCompletionStream,
}

/// Why a family was suppressed despite being compiled in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum SuppressionReason {
    /// An accepted configuration generation disabled the feature ID
    /// (`initializationOptions.disabledFeatures`).
    DisabledByConfiguration {
        /// Canonical `lsp.*` feature ID as supplied by configuration.
        feature_id: String,
    },
    /// A reviewed runtime availability input withheld the family. No input
    /// currently reaches this state (see [`RuntimeAvailability`]); the
    /// variant exists so runtime-mode withholding stays distinct from
    /// configuration and client causes.
    RuntimeUnavailable {
        /// Reviewed input that would withhold the family.
        input: &'static str,
    },
}

/// Why a family's outcome was downgraded from the default transport/shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum DowngradeReason {
    /// An admitted compatibility exception changed the outcome.
    CompatibilityException {
        /// The admitted exception.
        exception: KnownException,
    },
    /// Negotiated position encoding is stored but advertisement stays pinned
    /// to UTF-16 until providers compute positions in the negotiated units.
    PositionEncodingPin {
        /// Server-selected negotiated preference, when any.
        negotiated_preference: Option<PositionEncoding>,
    },
    /// Static advertisement for the selector is withdrawn because planned
    /// dynamic registration owns it for this client.
    DynamicRegistrationPreferred {
        /// Protocol selector the plan owns.
        selector: &'static str,
    },
}

/// Why the file-watcher registration was withheld for one subject.
///
/// Exactly one cause is recorded, evaluated in admission-conjunction order,
/// so a runtime-tuning withholding stays distinct from a client or
/// compatibility cause (#9665 rule 6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum WatcherWithholdReason {
    /// The client did not affirmatively declare dynamic file-watcher support.
    ClientUnsupported,
    /// No active workspace-symbol surface remains for the watcher to serve.
    WorkspaceSurfaceInactive,
    /// A reviewed runtime availability input withheld the plan.
    RuntimeUnavailable {
        /// Reviewed input that withheld the plan (`runtime_tuning.*`).
        input: &'static str,
    },
    /// An admitted compatibility exception force-disables the flow.
    CompatibilityException {
        /// The admitted exception.
        exception: KnownException,
    },
}

/// Typed file-watcher registration decision for one subject.
///
/// Still a plan-level decision: activation belongs to S04.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum WatcherPlanDecision {
    /// The watcher registration is part of the plan.
    Planned,
    /// The plan omits the watcher registration for exactly one reviewed cause.
    Withheld(WatcherWithholdReason),
}

/// Planned dynamic registration descriptor (a plan — never active state).
///
/// Active registration state requires accepted client success through the
/// #6722/#6724 terminal request authorities (S04); this model cannot express
/// activation by construction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct PlannedDynamic {
    /// Stable registration ID (e.g. `perl-didChangeWatchedFiles`).
    pub registration_id: &'static str,
    /// LSP method being registered.
    pub method: &'static str,
    /// Typed shape of the register-options payload.
    pub options_shape: RegistrationOptionsShape,
}

/// Typed register-options shapes producible by the plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum RegistrationOptionsShape {
    /// File-watcher registration; `relative_pattern` selects RelativePattern
    /// objects vs legacy string globs.
    Watchers {
        /// Client declared `relativePatternSupport`.
        relative_pattern: bool,
    },
    /// Inline-completion registration with the perl/perl5 document selector.
    InlineCompletionDocumentSelector,
}

/// One family's final outcome for one subject.
///
/// Exactly one variant applies; static and planned-dynamic cannot coexist
/// for one selector because selection returns a single outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum FamilyOutcome {
    /// Advertised statically in the final `serverCapabilities`.
    Static,
    /// Delivered through the planned `client/registerCapability` request.
    PlannedDynamic(PlannedDynamic),
    /// Not advertised: excluded by build profile (not compiled in), or —
    /// for client-presence-gated selectors — withheld because the client
    /// gave no declaring signal.
    UnadvertisedUnsupported,
    /// Compiled in but withheld by configuration or reviewed runtime input.
    Suppressed(SuppressionReason),
    /// Advertised/plan changed with a typed reason and a retained variant.
    Downgraded(DowngradeReason, Box<FamilyOutcome>),
}

impl FamilyOutcome {
    /// Whether the family reaches the final wire surface somehow (statically
    /// or through the registration plan).
    #[must_use]
    pub fn is_effectively_advertised(&self) -> bool {
        match self {
            Self::Static | Self::PlannedDynamic(_) => true,
            Self::Downgraded(_, inner) => inner.is_effectively_advertised(),
            Self::UnadvertisedUnsupported | Self::Suppressed(_) => false,
        }
    }
}

/// Selected position/text contracts (#2298 consumers).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct PositionEncodingContract {
    /// Encoding actually advertised. Pinned to UTF-16 on current main.
    pub advertised: PositionEncoding,
    /// Server-selected client preference, stored but not advertised.
    pub negotiated_preference: Option<PositionEncoding>,
}

/// Supported position encodings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum PositionEncoding {
    /// UTF-8 code units.
    Utf8,
    /// UTF-16 code units (mandatory LSP default).
    Utf16,
}

/// Authoritative text-sync contract (runtime-owned shape, #4995).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct TextSyncContract {
    /// `openClose` — didOpen/didClose are tracked.
    pub open_close: bool,
    /// `change` kind: 1 = FULL (the server reparses on every didChange).
    pub change_kind_full: bool,
    /// `willSave` — the server handles didSave-adjacent flows.
    pub will_save: bool,
    /// `willSaveWaitUntil` — formatter-owned; withdrawn (#11955).
    pub will_save_wait_until: bool,
    /// `save.includeText`.
    pub save_include_text: bool,
}

/// How pull/push diagnostic transport is selected for the subject.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum DiagnosticTransport {
    /// Client declared `textDocument.diagnostic`, the final surface
    /// advertises the pull family, and no compatibility exception retains
    /// push publishing; push is suppressed.
    PullPreferred,
    /// Push publishing remains the transport.
    PushOnly(PushTransportReason),
}

/// Why push publishing remains active.
///
/// Every cause is a reviewed final-selection fact, never an ambient probe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum PushTransportReason {
    /// Client gave no `textDocument.diagnostic` signal.
    NoClientSignal,
    /// A compatibility exception retains push publishing.
    ClientCompatibility(KnownException),
    /// The final surface does not advertise pull diagnostics (profile
    /// exclusion or configuration suppression), so the client was never
    /// offered the transport whose absence would justify suppressing push.
    PullNotAdvertised,
}

/// Refresh-request families tracked by the plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum RefreshFamily {
    /// `workspace/codeLens/refresh`.
    CodeLens,
    /// `workspace/semanticTokens/refresh`.
    SemanticTokens,
    /// `workspace/inlayHint/refresh`.
    InlayHint,
    /// `workspace/inlineValue/refresh`.
    InlineValue,
    /// `workspace/diagnostic(s)/refresh`.
    Diagnostic,
    /// `workspace/foldingRange/refresh`.
    FoldingRange,
    /// `workspace/textDocumentContent/refresh`.
    TextDocumentContent,
}

/// A planned refresh decision (still a plan; execution belongs to S04).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct RefreshDecision {
    /// Client declared the corresponding refreshSupport fact.
    pub client_supports_refresh: bool,
    /// The owning feature is active in the final surface.
    pub feature_active: bool,
    /// The refresh request is part of the plan (support ∧ active).
    pub planned: bool,
}

/// The complete registration plan for one subject.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct RegistrationPlan {
    /// Registrations the server will send after legal lifecycle state.
    pub registrations: Vec<PlannedDynamic>,
    /// Unregistrations; no producer exists on current main, the typed slot
    /// is owned by S04.
    pub unregistrations: Vec<PlannedDynamic>,
}

/// Normalized client capability evidence steering final-surface policy.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct ClientSurfaceEvidence {
    /// `textDocument/inlineCompletion` declared (presence semantics).
    pub inline_completion: ClientFact,
    /// `textDocument/inlineCompletion/dynamicRegistration`.
    pub inline_completion_dynamic_registration: ClientFact,
    /// `workspace/didChangeWatchedFiles/dynamicRegistration` (post-policy).
    pub dynamic_file_watcher_registration: ClientFact,
    /// `workspace/didChangeWatchedFiles/relativePatternSupport`.
    pub file_watcher_relative_pattern: ClientFact,
    /// `textDocument/codeAction/documentationSupport`.
    pub code_action_documentation: ClientFact,
    /// `workspace/workspaceFolders`.
    pub workspace_folders: ClientFact,
    /// `textDocument/diagnostic` declared (presence semantics).
    pub diagnostic_pull: ClientFact,
    /// Per-operation file-operation participation facts.
    pub file_operations: FileOperationFacts,
    /// Refresh-request support facts.
    pub refresh_supports: RefreshSupportFacts,
    /// Server-selected negotiated position encoding, when any.
    pub negotiated_position_encoding: Option<PositionEncoding>,
}

/// Typed inputs for one exact final-surface subject.
///
/// Every field comes from a canonical owner (#6735 client normalization,
/// #8043/#8078/#8050 profile/feature selection, #6736 configuration
/// generation, #2298 position/text contracts, #8285 command descriptors,
/// explicit compatibility policy). The model rejects inputs that violate
/// provenance instead of silently repairing them.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SurfaceInputs {
    /// Selected build/profile policy. Provenance-checked against `build_flags`.
    pub profile: FeatureProfile,
    /// Raw profile flags; must equal `profile.build_flags()` exactly.
    pub build_flags: BuildFlags,
    /// Accepted configuration generation: disabled feature IDs (`lsp.*`),
    /// deduplicated. Unknown IDs are preserved distinctly in the output.
    pub disabled_feature_ids: BTreeSet<String>,
    /// Bounded client-identity-admitted compatibility exceptions.
    pub compatibility_exceptions: BTreeSet<KnownException>,
    /// Normalized client capability facts consumed by final-surface policy.
    pub client: ClientSurfaceEvidence,
    /// Command descriptors (#8285) eligible for executeCommand advertisement.
    pub command_ids: Vec<String>,
    /// Reviewed runtime availability inputs (currently none).
    pub runtime: RuntimeAvailability,
}

impl SurfaceInputs {
    /// A maximally-default subject for one profile: no client signal, no
    /// configuration generation, no compatibility exceptions, canonical
    /// command descriptors. Richer subjects are produced by mutating the
    /// returned value's fields (the type is `#[non_exhaustive]`, so callers
    /// outside this crate construct subjects through this constructor).
    #[must_use]
    pub fn new_subject(profile: FeatureProfile) -> Self {
        Self {
            profile,
            build_flags: profile.build_flags(),
            disabled_feature_ids: BTreeSet::new(),
            compatibility_exceptions: BTreeSet::new(),
            client: ClientSurfaceEvidence::default(),
            command_ids: super::capabilities::get_supported_commands()
                .into_iter()
                .map(|command| command.to_string())
                .collect(),
            runtime: RuntimeAvailability::default(),
        }
    }

    /// Validate provenance invariants before the model accepts the subject.
    fn validate(&self) -> Result<(), SurfaceBuildError> {
        let mut problems = Vec::new();
        if self.build_flags != self.profile.build_flags() {
            problems.push(
                "build_flags diverge from profile.build_flags(): raw build flags must not \
                 inject a capability the profile does not admit"
                    .to_string(),
            );
        }
        let mut seen_commands = BTreeSet::new();
        for command in &self.command_ids {
            if !seen_commands.insert(command.as_str()) {
                problems.push(format!("duplicate command descriptor: {command}"));
            }
        }
        if problems.is_empty() { Ok(()) } else { Err(SurfaceBuildError { problems }) }
    }

    /// Deterministic digest binding model output to exact inputs
    /// (schema/version/digest receipt binding, #9665 item 8).
    fn input_digest(&self) -> String {
        let disabled: Vec<&String> = self.disabled_feature_ids.iter().collect();
        let exceptions: Vec<String> = self
            .compatibility_exceptions
            .iter()
            .map(|exception| serde_json::to_string(exception).unwrap_or_default())
            .collect();
        let payload = serde_json::json!({
            "schema_version": EFFECTIVE_SURFACE_SCHEMA_VERSION,
            "issue": EFFECTIVE_SURFACE_ISSUE,
            "profile": self.profile.as_str(),
            "build_flag_feature_ids": self.build_flags.to_feature_ids(),
            "disabled_feature_ids": disabled,
            "compatibility_exceptions": exceptions,
            "client": self.client,
            "command_ids": self.command_ids,
            "runtime": self.runtime,
        });
        let serialized = serde_json::to_string(&payload).unwrap_or_default();
        let digest_bytes = Sha256::digest(serialized.as_bytes());
        let hex: String = digest_bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        format!("sha256:{hex}")
    }
}

/// Deterministic model construction failure; rendering refused rather than
/// shipping a partially-derived surface.
#[derive(Debug, Clone)]
pub struct SurfaceBuildError {
    /// Human-readable problems; deterministic order.
    pub problems: Vec<String>,
}

impl std::fmt::Display for SurfaceBuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "effective-surface construction refused:")?;
        for problem in &self.problems {
            writeln!(f, "  - {problem}")?;
        }
        Ok(())
    }
}

impl std::error::Error for SurfaceBuildError {}

/// The one deterministic final-surface authority output for one subject.
///
/// Construct exclusively through [`EffectiveLspSurface::build`]; fields are
/// exposed read-only for projections, tests and later exact-process
/// receipts (S05).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[non_exhaustive]
pub struct EffectiveLspSurface {
    /// Model schema version.
    pub schema_version: u64,
    /// Controlling leaf issue.
    pub controlling_issue: &'static str,
    /// Parent train controller.
    pub train_controller: &'static str,
    /// Profile label for the subject.
    pub profile: &'static str,
    /// Digest binding this output to its exact inputs.
    pub input_digest: String,
    /// One outcome per capability family.
    pub families: BTreeMap<CapabilityFamily, FamilyOutcome>,
    /// Exact final `InitializeResult.serverCapabilities` projection,
    /// serialized only from the selections above.
    pub server_capabilities: serde_json::Value,
    /// The one dynamic registration/unregistration plan.
    pub registration_plan: RegistrationPlan,
    /// Typed file-watcher plan decision (planned, or the exact reviewed
    /// withholding cause).
    pub watcher_registration_decision: WatcherPlanDecision,
    /// Planned refresh-request decisions.
    pub refresh_plan: BTreeMap<RefreshFamily, RefreshDecision>,
    /// Effective advertised feature IDs, derived after suppression.
    pub advertised_feature_ids: Vec<&'static str>,
    /// Effective executeCommand identities (empty when not advertised).
    pub command_ids: Vec<String>,
    /// Selected position contract.
    pub position_contract: PositionEncodingContract,
    /// Selected text-sync contract.
    pub text_sync: TextSyncContract,
    /// Selected diagnostic transport.
    pub diagnostic_transport: DiagnosticTransport,
    /// Configuration-supplied IDs the model could not recognize (preserved
    /// distinctly instead of silently dropped).
    pub unrecognized_disabled_feature_ids: Vec<String>,
    /// Exceptions actually applied while selecting this surface.
    pub compatibility_exceptions_applied: Vec<KnownException>,
}

impl EffectiveLspSurface {
    /// Build the final surface for one exact subject.
    ///
    /// Deterministic: identical inputs produce identical output including
    /// the input digest. Refuses invalid provenance instead of repairing it.
    pub fn build(inputs: &SurfaceInputs) -> Result<Self, SurfaceBuildError> {
        inputs.validate()?;
        let digest = inputs.input_digest();

        // Configuration generation: apply disabled feature IDs to a copy of
        // the profile flags, preserving unknown IDs distinctly.
        let mut flags = inputs.build_flags.clone();
        let mut unrecognized = Vec::new();
        for id in &inputs.disabled_feature_ids {
            if !apply_disabled_feature_id_model(&mut flags, id) {
                unrecognized.push(id.clone());
            }
        }
        unrecognized.sort();

        let client = inputs.client;
        let opencode_retained = inputs
            .compatibility_exceptions
            .contains(&KnownException::OpenCodePushDiagnosticsRetention);
        let jetbrains_watcher_off =
            inputs.compatibility_exceptions.contains(&KnownException::JetBrainsWatcherForceDisable);
        let mut applied: Vec<KnownException> = inputs
            .compatibility_exceptions
            .iter()
            .copied()
            .filter(|exception| match exception {
                KnownException::OpenCodePushDiagnosticsRetention => true,
                KnownException::JetBrainsWatcherForceDisable => {
                    client.dynamic_file_watcher_registration.is_supported()
                }
            })
            .collect();
        applied.sort();

        // ---- static flag families --------------------------------------
        let mut families = BTreeMap::new();
        for (family, base_flag) in flag_family_table(&inputs.build_flags) {
            let post_config = post_config_flag(&flags, family);
            let outcome = if !base_flag {
                FamilyOutcome::UnadvertisedUnsupported
            } else if !post_config {
                FamilyOutcome::Suppressed(SuppressionReason::DisabledByConfiguration {
                    feature_id: family.feature_id().to_string(),
                })
            } else {
                FamilyOutcome::Static
            };
            families.insert(family, outcome);
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let base_flag = inputs.build_flags.execute_command;
            let post_config = flags.execute_command;
            let outcome = if !base_flag {
                FamilyOutcome::UnadvertisedUnsupported
            } else if !post_config {
                FamilyOutcome::Suppressed(SuppressionReason::DisabledByConfiguration {
                    feature_id: CapabilityFamily::ExecuteCommand.feature_id().to_string(),
                })
            } else {
                FamilyOutcome::Static
            };
            families.insert(CapabilityFamily::ExecuteCommand, outcome);
        }

        // ---- client/runtime-shaped families -----------------------------
        // Workspace-symbol shape: the resolve variant overrides the simple
        // boolean when both survive; suppressing `lsp.workspace_symbol`
        // alone cannot withdraw advertisement while resolve remains (the
        // shipped builder behaves identically).
        let ws_base =
            inputs.build_flags.workspace_symbol || inputs.build_flags.workspace_symbol_resolve;
        let ws_post = flags.workspace_symbol || flags.workspace_symbol_resolve;
        let ws_outcome = if !ws_base {
            FamilyOutcome::UnadvertisedUnsupported
        } else if !ws_post {
            FamilyOutcome::Suppressed(SuppressionReason::DisabledByConfiguration {
                feature_id: CapabilityFamily::WorkspaceSymbol.feature_id().to_string(),
            })
        } else {
            FamilyOutcome::Static
        };
        families.insert(CapabilityFamily::WorkspaceSymbol, ws_outcome);

        // Inline completion: static vs planned-dynamic arbitration. Exactly
        // one outcome for the selector; the static advertisement is
        // withdrawn when the plan owns it (LSP 3.18 alternate mode).
        let inline_outcome = if !inputs.build_flags.inline_completion {
            FamilyOutcome::UnadvertisedUnsupported
        } else if !flags.inline_completion {
            FamilyOutcome::Suppressed(SuppressionReason::DisabledByConfiguration {
                feature_id: CapabilityFamily::InlineCompletion.feature_id().to_string(),
            })
        } else if client.inline_completion.is_present()
            && client.inline_completion_dynamic_registration.is_supported()
        {
            FamilyOutcome::Downgraded(
                DowngradeReason::DynamicRegistrationPreferred {
                    selector: "inlineCompletionProvider",
                },
                Box::new(FamilyOutcome::PlannedDynamic(PlannedDynamic {
                    registration_id: "perl-inlineCompletion",
                    method: "textDocument/inlineCompletion",
                    options_shape: RegistrationOptionsShape::InlineCompletionDocumentSelector,
                })),
            )
        } else if client.inline_completion.is_present() {
            FamilyOutcome::Static
        } else {
            // No client signal: the static provider object is withheld for
            // this subject (the runtime tri-state removes it).
            FamilyOutcome::UnadvertisedUnsupported
        };
        let mut inline_planned = None;
        if let FamilyOutcome::Downgraded(_, boxed) = &inline_outcome
            && let FamilyOutcome::PlannedDynamic(plan) = boxed.as_ref()
        {
            inline_planned = Some(plan.clone());
        }
        families.insert(CapabilityFamily::InlineCompletion, inline_outcome);

        // CodeAction.documentation sub-surface: client-gated insertion.
        let documentation_outcome =
            if flags.code_actions && client.code_action_documentation.is_supported() {
                FamilyOutcome::Static
            } else {
                FamilyOutcome::UnadvertisedUnsupported
            };
        families.insert(CapabilityFamily::CodeActionDocumentation, documentation_outcome);

        // Experimental custom-request advertisement.
        let stream_outcome = if flags.inline_completion && client.inline_completion.is_present() {
            FamilyOutcome::Static
        } else {
            FamilyOutcome::UnadvertisedUnsupported
        };
        families.insert(CapabilityFamily::ExperimentalInlineCompletionStream, stream_outcome);

        // Position encoding: negotiated preference stored, advertisement
        // pinned to UTF-16 until the #9282 coordinate cutover.
        families.insert(
            CapabilityFamily::PositionEncoding,
            FamilyOutcome::Downgraded(
                DowngradeReason::PositionEncodingPin {
                    negotiated_preference: client.negotiated_position_encoding,
                },
                Box::new(FamilyOutcome::Static),
            ),
        );

        // Text sync and workspace are unconditional runtime-owned surfaces.
        families.insert(CapabilityFamily::TextDocumentSync, FamilyOutcome::Static);
        families.insert(CapabilityFamily::Workspace, FamilyOutcome::Static);

        // ---- registration plan ------------------------------------------
        let mut registrations = Vec::new();
        let watcher_admitted = client.dynamic_file_watcher_registration.is_supported()
            && flags.workspace_symbol
            && inputs.runtime.file_watchers_enabled
            && !jetbrains_watcher_off;
        let watcher_registration_decision = if watcher_admitted {
            WatcherPlanDecision::Planned
        } else if !client.dynamic_file_watcher_registration.is_supported() {
            WatcherPlanDecision::Withheld(WatcherWithholdReason::ClientUnsupported)
        } else if !flags.workspace_symbol {
            WatcherPlanDecision::Withheld(WatcherWithholdReason::WorkspaceSurfaceInactive)
        } else if !inputs.runtime.file_watchers_enabled {
            WatcherPlanDecision::Withheld(WatcherWithholdReason::RuntimeUnavailable {
                input: "runtime_tuning.file_watchers",
            })
        } else {
            WatcherPlanDecision::Withheld(WatcherWithholdReason::CompatibilityException {
                exception: KnownException::JetBrainsWatcherForceDisable,
            })
        };
        if watcher_admitted {
            registrations.push(PlannedDynamic {
                registration_id: "perl-didChangeWatchedFiles",
                method: "workspace/didChangeWatchedFiles",
                options_shape: RegistrationOptionsShape::Watchers {
                    relative_pattern: client.file_watcher_relative_pattern.is_supported(),
                },
            });
        }
        if let Some(plan) = inline_planned {
            registrations.push(plan);
        }
        let registration_plan = RegistrationPlan { registrations, unregistrations: Vec::new() };

        // ---- refresh plan -------------------------------------------------
        let refresh_plan = build_refresh_plan(&flags, &client.refresh_supports);

        // ---- effective identities (derived AFTER suppression) -------------
        // Identities come from the final family outcomes (#9665 item 5),
        // never the pre-client flag set: a family withheld for any reviewed
        // reason contributes no advertised ID. Families without a canonical
        // `lsp.*` configuration identity (position encoding, text sync,
        // workspace, experimental stream) contribute none either.
        let advertised_feature_ids: Vec<&'static str> = {
            let mut ids = BTreeSet::new();
            for (family, outcome) in &families {
                let feature_id = family.feature_id();
                if feature_id != "n/a" && outcome.is_effectively_advertised() {
                    ids.insert(feature_id);
                }
            }
            ids.into_iter().collect()
        };
        #[allow(unused_mut)]
        let mut command_ids =
            if flags.execute_command { inputs.command_ids.clone() } else { Vec::new() };
        #[cfg(target_arch = "wasm32")]
        {
            command_ids.clear();
        }

        // ---- contracts -----------------------------------------------------
        let position_contract = PositionEncodingContract {
            advertised: PositionEncoding::Utf16,
            negotiated_preference: client.negotiated_position_encoding,
        };
        let text_sync = TextSyncContract {
            open_close: true,
            change_kind_full: true,
            will_save: true,
            will_save_wait_until: false,
            save_include_text: true,
        };

        // ---- diagnostic transport ------------------------------------------
        // Pull is preferred only when the final surface actually advertises
        // the pull family (the same predicate that projects
        // `diagnosticProvider`); suppressing push for a transport the client
        // was never offered would lose diagnostics.
        let diagnostic_transport = if !projects_static(&families, CapabilityFamily::PullDiagnostic)
        {
            DiagnosticTransport::PushOnly(PushTransportReason::PullNotAdvertised)
        } else if client.diagnostic_pull.is_present() {
            if opencode_retained {
                DiagnosticTransport::PushOnly(PushTransportReason::ClientCompatibility(
                    KnownException::OpenCodePushDiagnosticsRetention,
                ))
            } else {
                DiagnosticTransport::PullPreferred
            }
        } else {
            DiagnosticTransport::PushOnly(PushTransportReason::NoClientSignal)
        };

        let server_capabilities = project_server_capabilities(
            &families,
            &client,
            &flags,
            &command_ids,
            &position_contract,
            &text_sync,
        );

        Ok(Self {
            schema_version: EFFECTIVE_SURFACE_SCHEMA_VERSION,
            controlling_issue: EFFECTIVE_SURFACE_ISSUE,
            train_controller: TRAIN_CONTROLLER,
            profile: inputs.profile.as_str(),
            input_digest: digest,
            families,
            server_capabilities,
            registration_plan,
            watcher_registration_decision,
            refresh_plan,
            advertised_feature_ids,
            command_ids,
            position_contract,
            text_sync,
            diagnostic_transport,
            unrecognized_disabled_feature_ids: unrecognized,
            compatibility_exceptions_applied: applied,
        })
    }

    /// Families whose final outcome carries a typed suppression/downgrade
    /// reason (review ledger; plain supported/unsupported outcomes excluded).
    #[must_use]
    pub fn suppressed_or_downgraded(&self) -> BTreeMap<CapabilityFamily, &FamilyOutcome> {
        self.families
            .iter()
            .filter(|(_, outcome)| {
                !matches!(outcome, FamilyOutcome::Static | FamilyOutcome::UnadvertisedUnsupported)
            })
            .map(|(family, outcome)| (*family, outcome))
            .collect()
    }
}

/// Flag-driven families with their base (pre-configuration) flag values, in
/// deterministic order. Host-only [`CapabilityFamily::ExecuteCommand`] and
/// the client/runtime-shaped families are handled by the builder directly.
fn flag_family_table(base: &BuildFlags) -> Vec<(CapabilityFamily, bool)> {
    vec![
        (CapabilityFamily::Completion, base.completion),
        (CapabilityFamily::Hover, base.hover),
        (CapabilityFamily::Definition, base.definition),
        (CapabilityFamily::TypeDefinition, base.type_definition),
        (CapabilityFamily::Implementation, base.implementation),
        (CapabilityFamily::References, base.references),
        (CapabilityFamily::DocumentSymbol, base.document_symbol),
        (CapabilityFamily::WorkspaceSymbol, base.workspace_symbol),
        (CapabilityFamily::DocumentHighlight, base.document_highlight),
        (CapabilityFamily::NotebookDocumentSync, base.notebook_document_sync),
        (CapabilityFamily::FoldingRange, base.folding_range),
        (CapabilityFamily::InlayHint, base.inlay_hints),
        (CapabilityFamily::PullDiagnostic, base.pull_diagnostics),
        (CapabilityFamily::SemanticTokens, base.semantic_tokens),
        (CapabilityFamily::CodeAction, base.code_actions),
        (CapabilityFamily::DocumentFormatting, base.formatting),
        (CapabilityFamily::RangeFormatting, base.range_formatting),
        (CapabilityFamily::Rename, base.rename),
        (CapabilityFamily::OnTypeFormatting, base.on_type_formatting),
        (CapabilityFamily::LinkedEditingRange, base.linked_editing),
        (CapabilityFamily::SignatureHelp, base.signature_help),
        (CapabilityFamily::CodeLens, base.code_lens),
        (CapabilityFamily::InlineValue, base.inline_values),
        (CapabilityFamily::Moniker, base.moniker),
        (CapabilityFamily::DocumentColor, base.document_color),
        (CapabilityFamily::CallHierarchy, base.call_hierarchy),
        (CapabilityFamily::Declaration, base.declaration),
        (CapabilityFamily::DocumentLink, base.document_links),
        (CapabilityFamily::SelectionRange, base.selection_ranges),
        (CapabilityFamily::TypeHierarchy, base.type_hierarchy),
        (CapabilityFamily::InlineCompletion, base.inline_completion),
    ]
}

/// Post-configuration flag for a family, read from the suppressed flag set.
fn post_config_flag(flags: &BuildFlags, family: CapabilityFamily) -> bool {
    match family {
        CapabilityFamily::Completion => flags.completion,
        CapabilityFamily::Hover => flags.hover,
        CapabilityFamily::Definition => flags.definition,
        CapabilityFamily::TypeDefinition => flags.type_definition,
        CapabilityFamily::Implementation => flags.implementation,
        CapabilityFamily::References => flags.references,
        CapabilityFamily::DocumentSymbol => flags.document_symbol,
        CapabilityFamily::WorkspaceSymbol => flags.workspace_symbol,
        CapabilityFamily::DocumentHighlight => flags.document_highlight,
        CapabilityFamily::NotebookDocumentSync => flags.notebook_document_sync,
        CapabilityFamily::FoldingRange => flags.folding_range,
        CapabilityFamily::InlayHint => flags.inlay_hints,
        CapabilityFamily::PullDiagnostic => flags.pull_diagnostics,
        CapabilityFamily::SemanticTokens => flags.semantic_tokens,
        CapabilityFamily::CodeAction | CapabilityFamily::CodeActionDocumentation => {
            flags.code_actions
        }
        CapabilityFamily::ExecuteCommand => flags.execute_command,
        CapabilityFamily::DocumentFormatting => flags.formatting,
        CapabilityFamily::RangeFormatting => flags.range_formatting,
        CapabilityFamily::Rename => flags.rename,
        CapabilityFamily::OnTypeFormatting => flags.on_type_formatting,
        CapabilityFamily::LinkedEditingRange => flags.linked_editing,
        CapabilityFamily::SignatureHelp => flags.signature_help,
        CapabilityFamily::CodeLens => flags.code_lens,
        CapabilityFamily::InlineValue => flags.inline_values,
        CapabilityFamily::Moniker => flags.moniker,
        CapabilityFamily::DocumentColor => flags.document_color,
        CapabilityFamily::CallHierarchy => flags.call_hierarchy,
        CapabilityFamily::Declaration => flags.declaration,
        CapabilityFamily::DocumentLink => flags.document_links,
        CapabilityFamily::SelectionRange => flags.selection_ranges,
        CapabilityFamily::TypeHierarchy => flags.type_hierarchy,
        CapabilityFamily::InlineCompletion => flags.inline_completion,
        CapabilityFamily::PositionEncoding
        | CapabilityFamily::TextDocumentSync
        | CapabilityFamily::Workspace
        | CapabilityFamily::ExperimentalInlineCompletionStream => true,
    }
}

impl CapabilityFamily {
    /// Canonical `lsp.*` feature ID whose configuration suppression zeroes
    /// this family.
    #[must_use]
    pub fn feature_id(self) -> &'static str {
        match self {
            Self::Completion => "lsp.completion",
            Self::Hover => "lsp.hover",
            Self::Definition => "lsp.definition",
            Self::TypeDefinition => "lsp.type_definition",
            Self::Implementation => "lsp.implementation",
            Self::References => "lsp.references",
            Self::DocumentSymbol => "lsp.document_symbol",
            Self::DocumentHighlight => "lsp.document_highlight",
            Self::WorkspaceSymbol => "lsp.workspace_symbol",
            Self::NotebookDocumentSync => "lsp.notebook_document_sync",
            Self::FoldingRange => "lsp.folding_range",
            Self::InlayHint => "lsp.inlay_hint",
            Self::PullDiagnostic => "lsp.pull_diagnostics",
            Self::SemanticTokens => "lsp.semantic_tokens",
            Self::CodeAction | Self::CodeActionDocumentation => "lsp.code_action",
            Self::ExecuteCommand => "lsp.execute_command",
            Self::DocumentFormatting => "lsp.formatting",
            Self::RangeFormatting => "lsp.ranges_formatting",
            Self::Rename => "lsp.rename",
            Self::OnTypeFormatting => "lsp.on_type_formatting",
            Self::LinkedEditingRange => "lsp.linked_editing_range",
            Self::SignatureHelp => "lsp.signature_help",
            Self::CodeLens => "lsp.code_lens",
            Self::InlineValue => "lsp.inline_value",
            Self::Moniker => "lsp.moniker",
            Self::DocumentColor => "lsp.document_color",
            Self::CallHierarchy => "lsp.call_hierarchy",
            Self::Declaration => "lsp.declaration",
            Self::DocumentLink => "lsp.document_link",
            Self::SelectionRange => "lsp.selection_range",
            Self::TypeHierarchy => "lsp.type_hierarchy",
            Self::InlineCompletion => "lsp.inline_completion",
            Self::PositionEncoding
            | Self::TextDocumentSync
            | Self::Workspace
            | Self::ExperimentalInlineCompletionStream => "n/a",
        }
    }

    /// Wire pointer prefixes owned by this family's static projection; used
    /// by ownership controls proving no emitted field lacks a family.
    #[must_use]
    pub fn wire_prefixes(self) -> &'static [&'static str] {
        match self {
            Self::Completion => &["completionProvider"],
            Self::Hover => &["hoverProvider"],
            Self::Definition => &["definitionProvider"],
            Self::TypeDefinition => &["typeDefinitionProvider"],
            Self::Implementation => &["implementationProvider"],
            Self::References => &["referencesProvider"],
            Self::DocumentSymbol => &["documentSymbolProvider"],
            Self::WorkspaceSymbol => &["workspaceSymbolProvider"],
            Self::DocumentHighlight => &["documentHighlightProvider"],
            Self::NotebookDocumentSync => &["notebookDocumentSync"],
            Self::FoldingRange => &["foldingRangeProvider"],
            Self::InlayHint => &["inlayHintProvider"],
            Self::PullDiagnostic => &["diagnosticProvider"],
            Self::SemanticTokens => &["semanticTokensProvider"],
            Self::CodeAction => &["codeActionProvider"],
            Self::CodeActionDocumentation => &[],
            Self::ExecuteCommand => &["executeCommandProvider", "commands[]"],
            Self::DocumentFormatting => &["documentFormattingProvider"],
            Self::RangeFormatting => &["documentRangeFormattingProvider"],
            Self::Rename => &["renameProvider"],
            Self::OnTypeFormatting => &["documentOnTypeFormattingProvider"],
            Self::LinkedEditingRange => &["linkedEditingRangeProvider"],
            Self::SignatureHelp => &["signatureHelpProvider"],
            Self::CodeLens => &["codeLensProvider"],
            Self::InlineValue => &["inlineValueProvider"],
            Self::Moniker => &["monikerProvider"],
            Self::DocumentColor => &["colorProvider"],
            Self::CallHierarchy => &["callHierarchyProvider"],
            Self::Declaration => &["declarationProvider"],
            Self::DocumentLink => &["documentLinkProvider"],
            Self::SelectionRange => &["selectionRangeProvider"],
            Self::TypeHierarchy => &["typeHierarchyProvider", "experimental.typeHierarchyProvider"],
            Self::InlineCompletion => &["inlineCompletionProvider"],
            Self::PositionEncoding => &["positionEncoding"],
            Self::TextDocumentSync => &["textDocumentSync"],
            Self::Workspace => &["workspace"],
            Self::ExperimentalInlineCompletionStream => {
                &["experimental.perlInlineCompletionStream"]
            }
        }
    }

    /// Map an S01 suppression row input name to the family its
    /// configuration suppression zeroes, when one exists.
    #[must_use]
    pub fn feature_id_for_suppression(input: &str) -> Option<Self> {
        let id = input.strip_prefix("initializationOptions.disabledFeatures:")?;
        let all = [
            (Self::Completion, "lsp.completion"),
            (Self::Hover, "lsp.hover"),
            (Self::Definition, "lsp.definition"),
            (Self::TypeDefinition, "lsp.type_definition"),
            (Self::Implementation, "lsp.implementation"),
            (Self::References, "lsp.references"),
            (Self::DocumentSymbol, "lsp.document_symbol"),
            (Self::WorkspaceSymbol, "lsp.workspace_symbol"),
            (Self::NotebookDocumentSync, "lsp.notebook_document_sync"),
            (Self::FoldingRange, "lsp.folding_range"),
            (Self::InlayHint, "lsp.inlay_hint"),
            (Self::PullDiagnostic, "lsp.pull_diagnostics"),
            (Self::SemanticTokens, "lsp.semantic_tokens"),
            (Self::CodeAction, "lsp.code_action"),
            (Self::ExecuteCommand, "lsp.execute_command"),
            (Self::DocumentFormatting, "lsp.formatting"),
            (Self::RangeFormatting, "lsp.range_formatting"),
            (Self::Rename, "lsp.rename"),
            (Self::OnTypeFormatting, "lsp.on_type_formatting"),
            (Self::LinkedEditingRange, "lsp.linked_editing_range"),
            (Self::SignatureHelp, "lsp.signature_help"),
            (Self::CodeLens, "lsp.code_lens"),
            (Self::InlineValue, "lsp.inline_value"),
            (Self::Moniker, "lsp.moniker"),
            (Self::DocumentColor, "lsp.document_color"),
            (Self::CallHierarchy, "lsp.call_hierarchy"),
            (Self::Declaration, "lsp.declaration"),
            (Self::DocumentLink, "lsp.document_link"),
            (Self::SelectionRange, "lsp.selection_range"),
            (Self::TypeHierarchy, "lsp.type_hierarchy"),
            (Self::InlineCompletion, "lsp.inline_completion"),
        ];
        all.into_iter().find(|(_, candidate)| *candidate == id).map(|(family, _)| family)
    }
}

/// Model-side disabled-feature application — the canonical configuration
/// generation table (#9665).
///
/// Returns whether the ID was recognized. The runtime twin
/// (`perl-lsp-rs …lifecycle/capabilities.rs::apply_disabled_feature_id`) is
/// pinned to this table by a lifecycle parity test until S03 removes the
/// duplicate path and executes this authority directly.
pub fn apply_disabled_feature_id_model(flags: &mut BuildFlags, id: &str) -> bool {
    match id {
        "lsp.completion" => flags.completion = false,
        "lsp.hover" => flags.hover = false,
        "lsp.definition" => flags.definition = false,
        "lsp.declaration" => flags.declaration = false,
        "lsp.references" => flags.references = false,
        "lsp.document_symbol" => flags.document_symbol = false,
        "lsp.workspace_symbol" => flags.workspace_symbol = false,
        "lsp.code_action" => flags.code_actions = false,
        "lsp.code_lens" => flags.code_lens = false,
        "lsp.rename" => flags.rename = false,
        "lsp.folding_range" => flags.folding_range = false,
        "lsp.selection_range" => flags.selection_ranges = false,
        "lsp.linked_editing_range" => flags.linked_editing = false,
        "lsp.inlay_hint" => flags.inlay_hints = false,
        "lsp.semantic_tokens" => flags.semantic_tokens = false,
        "lsp.call_hierarchy" => flags.call_hierarchy = false,
        "lsp.type_hierarchy" => flags.type_hierarchy = false,
        "lsp.pull_diagnostics" => flags.pull_diagnostics = false,
        "lsp.document_color" => flags.document_color = false,
        "lsp.signature_help" => flags.signature_help = false,
        "lsp.document_highlight" => flags.document_highlight = false,
        "lsp.formatting" => flags.formatting = false,
        "lsp.range_formatting" | "lsp.ranges_formatting" => flags.range_formatting = false,
        "lsp.on_type_formatting" => flags.on_type_formatting = false,
        "lsp.document_link" => flags.document_links = false,
        "lsp.inline_completion" => flags.inline_completion = false,
        "lsp.inline_value" => flags.inline_values = false,
        "lsp.notebook_document_sync" => flags.notebook_document_sync = false,
        "lsp.notebook_cell_execution" => flags.notebook_cell_execution = false,
        "lsp.implementation" => flags.implementation = false,
        "lsp.type_definition" => flags.type_definition = false,
        "lsp.execute_command" => flags.execute_command = false,
        "lsp.moniker" => flags.moniker = false,
        _ => return false,
    }
    true
}

/// Planned refresh decisions from feature state and client facts.
fn build_refresh_plan(
    flags: &BuildFlags,
    refreshes: &RefreshSupportFacts,
) -> BTreeMap<RefreshFamily, RefreshDecision> {
    let rows = [
        (RefreshFamily::CodeLens, refreshes.code_lens.is_supported(), flags.code_lens),
        (
            RefreshFamily::SemanticTokens,
            refreshes.semantic_tokens.is_supported(),
            flags.semantic_tokens,
        ),
        (RefreshFamily::InlayHint, refreshes.inlay_hint.is_supported(), flags.inlay_hints),
        (RefreshFamily::InlineValue, refreshes.inline_value.is_supported(), flags.inline_values),
        (RefreshFamily::Diagnostic, refreshes.diagnostic.is_supported(), flags.pull_diagnostics),
        (RefreshFamily::FoldingRange, refreshes.folding_range.is_supported(), flags.folding_range),
        // textDocumentContent refresh has no client refreshSupport gate; the
        // owning surface (perldoc schemes) is always active when advertised.
        (RefreshFamily::TextDocumentContent, true, true),
    ];
    rows.into_iter()
        .map(|(family, supported, active)| {
            (
                family,
                RefreshDecision {
                    client_supports_refresh: supported,
                    feature_active: active,
                    planned: supported && active,
                },
            )
        })
        .collect()
}

/// Whether a family projects its static wire shape (plain Static, or a
/// downgrade whose retained variant is Static).
fn projects_static(
    families: &BTreeMap<CapabilityFamily, FamilyOutcome>,
    family: CapabilityFamily,
) -> bool {
    match families.get(&family) {
        Some(FamilyOutcome::Static) => true,
        Some(FamilyOutcome::Downgraded(_, inner)) => {
            matches!(inner.as_ref(), FamilyOutcome::Static)
        }
        _ => false,
    }
}

/// Project the exact final `serverCapabilities` from family selections only.
///
/// Raw build flags never reach this renderer; every emitted pointer traces
/// to a family outcome, the contracts, or the normalized client facts.
fn project_server_capabilities(
    families: &BTreeMap<CapabilityFamily, FamilyOutcome>,
    client: &ClientSurfaceEvidence,
    flags: &BuildFlags,
    command_ids: &[String],
    position: &PositionEncodingContract,
    text_sync: &TextSyncContract,
) -> serde_json::Value {
    let mut caps = serde_json::Map::new();

    // Position encoding (pin policy: advertise the contract value).
    let advertised_encoding = match position.advertised {
        PositionEncoding::Utf8 => "utf-8",
        PositionEncoding::Utf16 => "utf-16",
    };
    caps.insert("positionEncoding".into(), serde_json::json!(advertised_encoding));

    // Text sync: the authoritative runtime shape (#4995).
    caps.insert(
        "textDocumentSync".into(),
        serde_json::json!({
            "openClose": text_sync.open_close,
            "change": u8::from(text_sync.change_kind_full),
            "willSave": text_sync.will_save,
            "willSaveWaitUntil": text_sync.will_save_wait_until,
            "save": { "includeText": text_sync.save_include_text },
        }),
    );

    let simple_true = serde_json::Value::Bool(true);

    if projects_static(families, CapabilityFamily::Hover) {
        caps.insert("hoverProvider".into(), simple_true.clone());
    }
    if projects_static(families, CapabilityFamily::CallHierarchy) {
        caps.insert("callHierarchyProvider".into(), simple_true.clone());
    }
    if projects_static(families, CapabilityFamily::Declaration) {
        caps.insert("declarationProvider".into(), simple_true.clone());
    }
    if projects_static(families, CapabilityFamily::Definition) {
        caps.insert("definitionProvider".into(), simple_true.clone());
    }
    if projects_static(families, CapabilityFamily::TypeDefinition) {
        caps.insert("typeDefinitionProvider".into(), simple_true.clone());
    }
    if projects_static(families, CapabilityFamily::Implementation) {
        caps.insert("implementationProvider".into(), simple_true.clone());
    }
    if projects_static(families, CapabilityFamily::References) {
        caps.insert("referencesProvider".into(), simple_true.clone());
    }
    if projects_static(families, CapabilityFamily::DocumentSymbol) {
        caps.insert("documentSymbolProvider".into(), simple_true.clone());
    }
    if projects_static(families, CapabilityFamily::DocumentHighlight) {
        caps.insert("documentHighlightProvider".into(), simple_true.clone());
    }
    if projects_static(families, CapabilityFamily::WorkspaceSymbol) {
        if flags.workspace_symbol_resolve {
            caps.insert(
                "workspaceSymbolProvider".into(),
                serde_json::json!({ "resolveProvider": true }),
            );
        } else {
            caps.insert("workspaceSymbolProvider".into(), simple_true.clone());
        }
    }
    if projects_static(families, CapabilityFamily::FoldingRange) {
        caps.insert("foldingRangeProvider".into(), simple_true.clone());
    }
    if projects_static(families, CapabilityFamily::InlayHint) {
        caps.insert("inlayHintProvider".into(), serde_json::json!({ "resolveProvider": true }));
    }
    if projects_static(families, CapabilityFamily::PullDiagnostic) {
        caps.insert(
            "diagnosticProvider".into(),
            serde_json::json!({
                "interFileDependencies": false,
                "workspaceDiagnostics": true,
                "identifier": "perl-lsp",
            }),
        );
    }
    if projects_static(families, CapabilityFamily::SemanticTokens) {
        caps.insert(
            "semanticTokensProvider".into(),
            serde_json::json!({
                "legend": {
                    "tokenTypes": SEMANTIC_TOKEN_TYPES,
                    "tokenModifiers": SEMANTIC_TOKEN_MODIFIERS,
                },
                "range": true,
                "full": { "delta": true },
            }),
        );
    }
    if projects_static(families, CapabilityFamily::CodeAction) {
        let mut code_action = serde_json::Map::new();
        code_action.insert(
            "codeActionKinds".into(),
            serde_json::json!([
                "quickfix",
                "refactor",
                "refactor.extract",
                "refactor.rewrite",
                "source.fixAll",
                "source.modernize",
            ]),
        );
        code_action.insert("resolveProvider".into(), serde_json::Value::Bool(true));
        if projects_static(families, CapabilityFamily::CodeActionDocumentation) {
            code_action.insert(
                "documentation".into(),
                crate::protocol::command::code_action_documentation_entries(),
            );
        }
        caps.insert("codeActionProvider".into(), serde_json::Value::Object(code_action));
    }
    if projects_static(families, CapabilityFamily::ExecuteCommand) {
        caps.insert(
            "executeCommandProvider".into(),
            serde_json::json!({ "commands": command_ids }),
        );
    }
    if projects_static(families, CapabilityFamily::DocumentFormatting) {
        caps.insert("documentFormattingProvider".into(), simple_true.clone());
    }
    if projects_static(families, CapabilityFamily::RangeFormatting) {
        caps.insert(
            "documentRangeFormattingProvider".into(),
            serde_json::json!({ "rangesSupport": true }),
        );
    }
    if projects_static(families, CapabilityFamily::Rename) {
        caps.insert("renameProvider".into(), serde_json::json!({ "prepareProvider": true }));
    }
    if projects_static(families, CapabilityFamily::OnTypeFormatting) {
        caps.insert(
            "documentOnTypeFormattingProvider".into(),
            serde_json::json!({
                "firstTriggerCharacter": "}",
                "moreTriggerCharacter": [";", "\n"],
            }),
        );
    }
    if projects_static(families, CapabilityFamily::LinkedEditingRange) {
        caps.insert("linkedEditingRangeProvider".into(), simple_true.clone());
    }
    if projects_static(families, CapabilityFamily::SignatureHelp) {
        caps.insert(
            "signatureHelpProvider".into(),
            serde_json::json!({
                "triggerCharacters": ["(", ","],
                "retriggerCharacters": [",", "@", "%", "{", "["],
            }),
        );
    }
    if projects_static(families, CapabilityFamily::Completion) {
        caps.insert(
            "completionProvider".into(),
            serde_json::json!({
                "resolveProvider": true,
                "triggerCharacters": COMPLETION_TRIGGER_CHARACTERS,
                "completionItem": {
                    "labelDetailsSupport": true,
                    "insertTextModes": [1, 2],
                },
            }),
        );
    }
    if projects_static(families, CapabilityFamily::DocumentLink) {
        caps.insert("documentLinkProvider".into(), serde_json::json!({ "resolveProvider": true }));
    }
    if projects_static(families, CapabilityFamily::SelectionRange) {
        caps.insert("selectionRangeProvider".into(), simple_true.clone());
    }
    if projects_static(families, CapabilityFamily::CodeLens) {
        caps.insert("codeLensProvider".into(), serde_json::json!({ "resolveProvider": true }));
    }
    if projects_static(families, CapabilityFamily::InlineValue) {
        caps.insert("inlineValueProvider".into(), simple_true.clone());
    }
    if projects_static(families, CapabilityFamily::Moniker) {
        caps.insert("monikerProvider".into(), simple_true.clone());
    }
    if projects_static(families, CapabilityFamily::DocumentColor) {
        caps.insert("colorProvider".into(), simple_true.clone());
    }
    if projects_static(families, CapabilityFamily::NotebookDocumentSync) {
        caps.insert(
            "notebookDocumentSync".into(),
            serde_json::json!({
                "notebookSelector": [
                    {
                        "notebook": "jupyter-notebook",
                        "cells": [{ "language": "perl" }],
                    }
                ],
                "save": true,
            }),
        );
    }
    if projects_static(families, CapabilityFamily::TypeHierarchy) {
        caps.insert(
            "typeHierarchyProvider".into(),
            serde_json::json!({ "workDoneProgressOptions": {} }),
        );
    }

    // Inline completion static provider: present only for a plain-Static
    // outcome (planned-dynamic withdraws it; withheld subjects omit it).
    if projects_static(families, CapabilityFamily::InlineCompletion) {
        caps.insert("inlineCompletionProvider".into(), serde_json::json!({}));
    }

    // Workspace surface (folders, textDocumentContent, file operations).
    let workspace_folders_supported = client.workspace_folders.is_supported();
    // Mirrors `perl-lsp-rs` lifecycle/watchers.rs PERL_WATCH_PATTERNS: the
    // catch-all advertisement keeps extensionless shebang scripts observable;
    // handler-side classification stays the Perl-source authority (#13308).
    let perl_globs = ["**/*"];
    let filters: Vec<serde_json::Value> =
        perl_globs.iter().map(|glob| serde_json::json!({ "pattern": { "glob": glob } })).collect();
    let mut file_operations = serde_json::Map::new();
    for (fact, name) in [
        (client.file_operations.will_create, "willCreate"),
        (client.file_operations.did_create, "didCreate"),
        (client.file_operations.will_rename, "willRename"),
        (client.file_operations.did_rename, "didRename"),
        (client.file_operations.will_delete, "willDelete"),
        (client.file_operations.did_delete, "didDelete"),
    ] {
        if fact.is_supported() {
            file_operations.insert(name.to_string(), serde_json::json!({ "filters": filters }));
        }
    }
    let mut workspace = serde_json::Map::new();
    workspace.insert(
        "workspaceFolders".into(),
        serde_json::json!({
            "supported": workspace_folders_supported,
            "changeNotifications": true,
        }),
    );
    workspace.insert("textDocumentContent".into(), serde_json::json!({ "schemes": ["perldoc"] }));
    if !file_operations.is_empty() {
        workspace.insert("fileOperations".into(), serde_json::Value::Object(file_operations));
    }
    caps.insert("workspace".into(), serde_json::Value::Object(workspace));

    // Experimental surface: typeHierarchy marker + custom stream request.
    if projects_static(families, CapabilityFamily::TypeHierarchy) {
        insert_experimental(&mut caps, "typeHierarchyProvider", serde_json::Value::Bool(true));
    }
    if projects_static(families, CapabilityFamily::ExperimentalInlineCompletionStream) {
        insert_experimental(&mut caps, "perlInlineCompletionStream", serde_json::Value::Bool(true));
    }

    serde_json::Value::Object(caps)
}

/// Insert a key under `experimental`, creating the object when absent.
fn insert_experimental(
    caps: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
    value: serde_json::Value,
) {
    let experimental =
        caps.entry("experimental".to_string()).or_insert_with(|| serde_json::json!({}));
    if let Some(map) = experimental.as_object_mut() {
        map.insert(key.to_string(), value);
    }
}

/// Semantic token legend types; index position is the wire format.
const SEMANTIC_TOKEN_TYPES: &[&str] = &[
    "namespace",
    "type",
    "class",
    "interface",
    "enum",
    "enumMember",
    "typeParameter",
    "function",
    "method",
    "property",
    "macro",
    "variable",
    "parameter",
    "keyword",
    "modifier",
    "comment",
    "string",
    "number",
    "regexp",
    "operator",
    "sql_string",
    "sql_heredoc_keyword",
    "json_heredoc_key",
    "label",
];

/// Semantic token legend modifiers; bitmask index is the wire format.
const SEMANTIC_TOKEN_MODIFIERS: &[&str] = &[
    "declaration",
    "definition",
    "readonly",
    "static",
    "deprecated",
    "abstract",
    "async",
    "modification",
    "documentation",
    "defaultLibrary",
    "scalarVariable",
    "arrayVariable",
    "hashVariable",
];

/// Completion trigger characters; multi-character Perl operators advertise
/// their component characters.
const COMPLETION_TRIGGER_CHARACTERS: &[&str] =
    &["$", "@", "%", "-", ">", ":", ".", "/", "\\", "\"", "'"];

#[cfg(test)]
mod tests;
