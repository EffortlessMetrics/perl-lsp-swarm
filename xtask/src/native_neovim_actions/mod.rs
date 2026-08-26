//! Checked native Neovim built-in-LSP action and observation contract
//! (`native_neovim_actions.v1`, #11409).
//!
//! Ownership split — consumed, never duplicated:
//!
//! - This module owns the native built-in-LSP action vocabulary and its
//!   observation shapes: which actions exist, their typed inputs and emitted
//!   effect classes, their public-API-boundary classification (public stable,
//!   public but version-scoped, checked companion protocol control,
//!   instrument-only hook with exact owner, not exposed), the bounded
//!   observable predicates asynchronous actions wait on, and the typed,
//!   bounded, plane-separated observations that come back. Rust/xtask owns
//!   orchestration, identity, boundedness, and classification policy.
//! - #10888 owns the BDD scenario ledger; #10903 owns the semantic
//!   fixture/expectation manifest. This contract retains scenario/fixture/
//!   cell references on observations and validates them as stable tokens,
//!   but never registers, mirrors, or re-oracles them.
//! - #11406 owns the exact Neovim host bytes/build; #10502/#7768 own the
//!   canonical config. The subject binding cites these by identity token and
//!   fails closed on any other subject; exact bytes are never re-pinned here.
//! - #10894 owns process spawn/deadline/ledger/cleanup. Host-lifecycle
//!   actions are bounded handoffs that stay fail-closed (`not_proven`/
//!   `unsupported`) until that authority lands; no action of this vocabulary
//!   implements a process timeout or cleanup policy.
//! - #10503 owns the thin real-host adapter; the native receipt adapter owns
//!   projection into `editor_client_compat.v1`. This module is
//!   receipt-agnostic: it validates and classifies, it never writes a
//!   receipt or decides support.
//!
//! Fail-closed laws enforced by [`validate_observation`] and
//! [`validate_table`]:
//!
//! - a fixed sleep, global workspace idle, any-result satisfaction, log
//!   text, server response where buffer/UI state is claimed, or a raw
//!   companion request relabeled ordinary is a hard validation failure
//!   ([`SubstitutionKind`]);
//! - a satisfied predicate names the state that settled it (bounded digest);
//!   elapsed time alone can never be satisfaction; a timed-out predicate is
//!   lawful but forces `not_proven`;
//! - expected results come from the fixture authority; an expectation
//!   derived from the observed output is rejected;
//! - a returned result cannot satisfy an application claim;
//!   requested/returned/applied/visible-current stay distinct, and an
//!   `observed` result must be current at the observation's own generation;
//! - the subject must be the pinned Neovim + built-in `vim.lsp` + `perllsp`
//!   subject; a Coc/other-client or renamed-server observation is rejected;
//! - companion protocol controls are lawful only as companion-class,
//!   instrument-plane evidence and can never label themselves ordinary
//!   Neovim traffic;
//! - host handoffs stay fail-closed until #10894 lands;
//! - observations are bounded and privacy-safe: stable tokens, `sha256:`
//!   digests, fixture-relative paths, grammar-checked spellings, no unknown
//!   fields;
//! - unknown action IDs fail closed; a host leaf cannot register a plausible
//!   private action while producing evidence.

pub mod fake;
pub mod observation;
pub mod predicate;

use anyhow::{Context, Result, ensure};
use serde::Serialize;
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use observation::{
    BackendIdentity, EffectClass, EffectStage, ExpectationSource, ObservationPlane,
    ObservationResult, ObservedRoute, TypedObservation,
};
use predicate::{PredicateEvidence, PredicateKind, PredicateRequirement};

/// Identity of this contract, for consumers that need to name the action and
/// observation semantics they validated against.
pub const CONTRACT_SCHEMA_VERSION: &str = "native_neovim_actions.v1";

/// The action-ID namespace: `neovim.native.<family>.<name>`.
pub const ACTION_ID_PREFIX: &str = "neovim.native.";

/// The five action families of the #11409 action contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionFamily {
    HostSession,
    ClientAttachment,
    ReadMethods,
    ConfigEdits,
    TextSyncLifecycle,
}

impl ActionFamily {
    /// Stable family token used inside action IDs.
    pub fn token(self) -> &'static str {
        match self {
            ActionFamily::HostSession => "host_session",
            ActionFamily::ClientAttachment => "client_attachment",
            ActionFamily::ReadMethods => "read_methods",
            ActionFamily::ConfigEdits => "config_edits",
            ActionFamily::TextSyncLifecycle => "text_sync_lifecycle",
        }
    }
}

/// How one action's execution is owned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionClass {
    /// An ordinary editor/client user action through real public surfaces.
    UserAction,
    /// A read-only observation of current client/server/editor state.
    Observation,
    /// A checked companion protocol control: deliberate negative/control
    /// traffic through the client's raw request surface, never ordinary
    /// behavior.
    CompanionControl,
    /// A deliberate test stimulus, never labeled product behavior.
    TestStimulus,
    /// A process/session operation owned by #10894; this contract only emits
    /// the bounded fail-closed handoff observation.
    HostHandoff,
}

/// The #11409 public-API-boundary classification of one action's helper
/// surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceClassification {
    /// Supported public Neovim APIs and user-shaped actions.
    PublicStable,
    /// Public but scoped to pinned Neovim versions; the exact build is
    /// owned by #11406.
    PublicVersionScoped { scope: &'static str },
    /// A checked companion protocol control (deliberate negative control
    /// only; cannot substitute for ordinary actual-client behavior).
    CompanionProtocolControl,
    /// An instrument-only hook with its exact owner.
    InstrumentOnlyHook { owner: &'static str },
    /// Not exposed: no public Neovim surface exists for this helper; the
    /// action stays fail-closed and can never claim `observed`.
    NotExposed,
}

/// A justified instrument-only hook citation.
#[derive(Debug, Clone, Copy)]
pub struct InstrumentHookUse {
    /// Neovim API spelling of the hook.
    pub api: &'static str,
    pub justification: &'static str,
    pub retirement: &'static str,
}

/// Typed input parameter kinds. Actions declare named bindings; the fake
/// backend and the real adapter route parameters through these kinds, so a
/// free-text parameter channel cannot appear.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InputKind {
    /// Fixture/substrate owner token (authority #10903 when landed).
    FixtureOwner,
    /// Fixture-root-relative document path.
    FixtureDocument,
    /// Named content anchor resolved through the fixture authority.
    ContentAnchor,
    /// Bounded user key sequence (insert-mode editing paths).
    KeySequence,
    /// Governed setting name token.
    SettingName,
    /// `sha256:` digest over the bounded setting value.
    SettingValueDigest,
    /// Expected-result reference (authority #10903).
    ExpectationRef,
    /// The bounded observable predicate spec this action waits on.
    PredicateSpec,
    /// Currentness generation floor the wait is tied to.
    GenerationFloor,
    /// Optional-cell selector (semantic tokens / inlay / code lens / …).
    OptionalCell,
    /// Foreign client id an exclusion control must rule out.
    ForeignClientId,
    /// Companion protocol control selector.
    CompanionControlSpec,
}

/// One typed input binding.
#[derive(Debug, Clone, Copy)]
pub struct InputBinding {
    pub name: &'static str,
    pub kind: InputKind,
}

/// Per-action observation shape rules beyond the shared laws.
#[derive(Debug, Clone, Copy)]
pub struct ShapeRules {
    /// Identity digests the observed effect must bind (capability, process,
    /// filetype, root identities, …).
    pub required_identity_digests: &'static [&'static str],
    /// The observation must bind pinned/foreign client cardinalities that
    /// prove client exclusion.
    pub requires_client_exclusion_cardinalities: bool,
}

/// The no-extra-shape baseline used by most rows.
pub const DEFAULT_SHAPE: ShapeRules =
    ShapeRules { required_identity_digests: &[], requires_client_exclusion_cardinalities: false };

/// One registered action. Every field is load-bearing at validation time;
/// the vocabulary digest covers all of them, so any semantic edit is a
/// visible identity change.
#[derive(Debug, Clone, Copy)]
pub struct ActionSpec {
    /// Stable ID in the `neovim.native.<family>.<name>` namespace.
    pub action_id: &'static str,
    pub family: ActionFamily,
    pub class: ActionClass,
    pub surface: SurfaceClassification,
    pub summary: &'static str,
    /// Public Neovim APIs the action routes through (grammar-checked).
    pub api_uses: &'static [&'static str],
    /// Native editor surfaces (Ex commands, autocmd events, user keys).
    pub native_surfaces: &'static [&'static str],
    /// Instrument-only hooks cited by this action, each with justification
    /// and retirement condition.
    pub instrument_hooks: &'static [InstrumentHookUse],
    /// Typed inputs.
    pub inputs: &'static [InputBinding],
    /// Effect classes the action may emit.
    pub emits: &'static [EffectClass],
    /// Bounded observable predicates asynchronous settlement waits on.
    pub required_predicates: &'static [PredicateRequirement],
    /// The minimum honest effect stage for an `observed` result.
    pub claim: EffectStage,
    pub shape: ShapeRules,
    /// Result vocabulary this action may report.
    pub allowed_results: &'static [ObservationResult],
}

impl ActionSpec {
    /// True when this action's inputs require an expected-result reference
    /// (#11409: expected results stay independent of production output).
    pub fn requires_expectation(&self) -> bool {
        self.inputs.iter().any(|input| input.kind == InputKind::ExpectationRef)
    }

    /// True when this action takes a content anchor input.
    pub fn requires_anchor(&self) -> bool {
        self.inputs.iter().any(|input| input.kind == InputKind::ContentAnchor)
    }
}

/// A validated observation: the classified result, with no receipt data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ValidatedObservation {
    pub action_id: String,
    pub family: ActionFamily,
    pub plane: ObservationPlane,
    pub result: ObservationResult,
    pub limitation_class: Option<String>,
    pub backend: BackendIdentity,
}

const fn predicate(kind: PredicateKind, max_wait_ms: u64) -> PredicateRequirement {
    PredicateRequirement { kind, max_wait_ms }
}

/// The full #11409 result vocabulary, admitted by ordinary product actions.
const FULL_RESULTS: &[ObservationResult] = &[
    ObservationResult::Observed,
    ObservationResult::Mismatch,
    ObservationResult::Unsupported,
    ObservationResult::NotProven,
    ObservationResult::InstrumentFailed,
];

/// Fail-closed vocabulary for host handoffs: with no #10894 shared
/// host-execution authority landed, no backend that exists today can
/// honestly produce an `observed` handoff. Admitting it is a reviewed
/// vocabulary edit that lands with the runner.
const HANDOFF_RESULTS: &[ObservationResult] =
    &[ObservationResult::NotProven, ObservationResult::Unsupported];

const INIT_SEQ_BUDGET_MS: u64 = 30_000;
const SETTLE_BUDGET_MS: u64 = 15_000;
/// Process/client teardown of a workspace-indexing server can legitimately
/// take tens of seconds under load; the bound stays explicit and typed.
const EXIT_BUDGET_MS: u64 = 45_000;

const GET_CLIENTS: &str = "vim.lsp.get_clients";
const CLIENT_CONFIG: &str = "vim.lsp.client.config";
const CLIENT_STOP: &str = "vim.lsp.client.stop";
const CLIENT_REQUEST: &str = "vim.lsp.client.request";
const LSP_CONFIG: &str = "vim.lsp.config";
const LSP_ENABLE: &str = "vim.lsp.enable";
const OMNIFUNC: &str = "vim.lsp.omnifunc";
const DIAG_GET: &str = "vim.diagnostic.get";
const BUF_HOVER: &str = "vim.lsp.buf.hover";
const BUF_DEFINITION: &str = "vim.lsp.buf.definition";
const BUF_FORMAT: &str = "vim.lsp.buf.format";
const BUF_RENAME: &str = "vim.lsp.buf.rename";
const BUF_CODE_ACTION: &str = "vim.lsp.buf.code_action";
const APPLY_WORKSPACE_EDIT: &str = "vim.lsp.util.apply_workspace_edit";
const SEMANTIC_TOKENS_START: &str = "vim.lsp.semantic_tokens.start";
const INLAY_HINT_ENABLE: &str = "vim.lsp.inlay_hint.enable";
const BO_FILETYPE: &str = "vim.bo.filetype";
const FN_GETCURPOS: &str = "vim.fn.getcurpos";
const FN_GETPOS: &str = "vim.fn.getpos";
const FN_MKDIR: &str = "vim.fn.mkdir";
const FN_DELETE: &str = "vim.fn.delete";

const WIRE_CAPTURE: &str = "vim.lsp.log";
const INFLIGHT_TABLE: &str = "vim.lsp.client.request";

/// The published action vocabulary. Rows are frozen once landed; semantic
/// edits bump the vocabulary digest visibly.
pub const ACTIONS: &[ActionSpec] = &[
    // -----------------------------------------------------------------
    // Family A — host/session
    // -----------------------------------------------------------------
    ActionSpec {
        action_id: "neovim.native.host_session.start_isolated_host",
        family: ActionFamily::HostSession,
        class: ActionClass::HostHandoff,
        surface: SurfaceClassification::NotExposed,
        summary: "start an isolated Neovim subject through the #10894 shared host-execution handoff",
        api_uses: &[],
        native_surfaces: &[],
        instrument_hooks: &[],
        inputs: &[InputBinding { name: "subject", kind: InputKind::FixtureOwner }],
        emits: &[EffectClass::HandoffState],
        required_predicates: &[],
        claim: EffectStage::Requested,
        shape: DEFAULT_SHAPE,
        allowed_results: HANDOFF_RESULTS,
    },
    ActionSpec {
        action_id: "neovim.native.host_session.load_canonical_config",
        family: ActionFamily::HostSession,
        class: ActionClass::UserAction,
        surface: SurfaceClassification::PublicVersionScoped { scope: "neovim-0.11" },
        summary: "load the exact canonical config and enable the perllsp client through the public config API",
        api_uses: &[LSP_CONFIG, LSP_ENABLE],
        native_surfaces: &[],
        instrument_hooks: &[],
        inputs: &[InputBinding { name: "config_owner", kind: InputKind::FixtureOwner }],
        emits: &[EffectClass::ConfigState],
        required_predicates: &[],
        claim: EffectStage::Applied,
        shape: DEFAULT_SHAPE,
        allowed_results: FULL_RESULTS,
    },
    ActionSpec {
        action_id: "neovim.native.host_session.open_buffer",
        family: ActionFamily::HostSession,
        class: ActionClass::UserAction,
        surface: SurfaceClassification::PublicStable,
        summary: "open the exact fixture document in a new buffer",
        api_uses: &[],
        native_surfaces: &[":e"],
        instrument_hooks: &[],
        inputs: &[
            InputBinding { name: "fixture", kind: InputKind::FixtureOwner },
            InputBinding { name: "document", kind: InputKind::FixtureDocument },
        ],
        emits: &[EffectClass::BufferState],
        required_predicates: &[],
        claim: EffectStage::Applied,
        shape: DEFAULT_SHAPE,
        allowed_results: FULL_RESULTS,
    },
    ActionSpec {
        action_id: "neovim.native.host_session.edit_buffer",
        family: ActionFamily::HostSession,
        class: ActionClass::UserAction,
        surface: SurfaceClassification::PublicStable,
        summary: "edit the open buffer through ordinary insert-mode user keys",
        api_uses: &[],
        native_surfaces: &["keys"],
        instrument_hooks: &[],
        inputs: &[
            InputBinding { name: "document", kind: InputKind::FixtureDocument },
            InputBinding { name: "anchor", kind: InputKind::ContentAnchor },
            InputBinding { name: "keys", kind: InputKind::KeySequence },
        ],
        emits: &[EffectClass::BufferState],
        required_predicates: &[],
        claim: EffectStage::Applied,
        shape: DEFAULT_SHAPE,
        allowed_results: FULL_RESULTS,
    },
    ActionSpec {
        action_id: "neovim.native.host_session.write_buffer",
        family: ActionFamily::HostSession,
        class: ActionClass::UserAction,
        surface: SurfaceClassification::PublicStable,
        summary: "write the buffer to its file through the ordinary write command",
        api_uses: &[],
        native_surfaces: &[":w"],
        instrument_hooks: &[],
        inputs: &[InputBinding { name: "document", kind: InputKind::FixtureDocument }],
        emits: &[EffectClass::BufferState, EffectClass::FileState],
        required_predicates: &[],
        claim: EffectStage::Applied,
        shape: DEFAULT_SHAPE,
        allowed_results: FULL_RESULTS,
    },
    ActionSpec {
        action_id: "neovim.native.host_session.close_buffer",
        family: ActionFamily::HostSession,
        class: ActionClass::UserAction,
        surface: SurfaceClassification::PublicStable,
        summary: "close and wipe the buffer",
        api_uses: &[],
        native_surfaces: &[":bwipeout"],
        instrument_hooks: &[],
        inputs: &[InputBinding { name: "document", kind: InputKind::FixtureDocument }],
        emits: &[EffectClass::BufferState],
        required_predicates: &[],
        claim: EffectStage::Applied,
        shape: DEFAULT_SHAPE,
        allowed_results: FULL_RESULTS,
    },
    ActionSpec {
        action_id: "neovim.native.host_session.reopen_buffer",
        family: ActionFamily::HostSession,
        class: ActionClass::UserAction,
        surface: SurfaceClassification::PublicStable,
        summary: "reopen the closed document and wait for the exact client to re-attach",
        api_uses: &[],
        native_surfaces: &[":e"],
        instrument_hooks: &[],
        inputs: &[InputBinding { name: "document", kind: InputKind::FixtureDocument }],
        emits: &[EffectClass::BufferState],
        required_predicates: &[predicate(
            PredicateKind::ClientInitializedExactProcess,
            INIT_SEQ_BUDGET_MS,
        )],
        claim: EffectStage::Applied,
        shape: DEFAULT_SHAPE,
        allowed_results: FULL_RESULTS,
    },
    ActionSpec {
        action_id: "neovim.native.host_session.stop_client_normal_route",
        family: ActionFamily::HostSession,
        class: ActionClass::UserAction,
        surface: SurfaceClassification::PublicStable,
        summary: "stop the exact LSP client through its normal public route",
        api_uses: &[CLIENT_STOP],
        native_surfaces: &[],
        instrument_hooks: &[],
        inputs: &[],
        emits: &[EffectClass::TerminalState],
        required_predicates: &[predicate(PredicateKind::ClientTerminalState, EXIT_BUDGET_MS)],
        claim: EffectStage::Applied,
        shape: DEFAULT_SHAPE,
        allowed_results: FULL_RESULTS,
    },
    ActionSpec {
        action_id: "neovim.native.host_session.exit_host_normal",
        family: ActionFamily::HostSession,
        class: ActionClass::UserAction,
        surface: SurfaceClassification::PublicStable,
        summary: "exit the Neovim host normally and wait for the client terminal state",
        api_uses: &[],
        native_surfaces: &[":qa"],
        instrument_hooks: &[],
        inputs: &[],
        emits: &[EffectClass::HostSessionState, EffectClass::TerminalState],
        required_predicates: &[predicate(PredicateKind::ClientTerminalState, EXIT_BUDGET_MS)],
        claim: EffectStage::Applied,
        shape: DEFAULT_SHAPE,
        allowed_results: FULL_RESULTS,
    },
    // -----------------------------------------------------------------
    // Family B — client/attachment
    // -----------------------------------------------------------------
    ActionSpec {
        action_id: "neovim.native.client_attachment.observe_filetype_before_override",
        family: ActionFamily::ClientAttachment,
        class: ActionClass::Observation,
        surface: SurfaceClassification::PublicStable,
        summary: "observe the native filetype detection before any override is applied",
        api_uses: &[BO_FILETYPE],
        native_surfaces: &[],
        instrument_hooks: &[],
        inputs: &[InputBinding { name: "document", kind: InputKind::FixtureDocument }],
        emits: &[EffectClass::Filetype],
        required_predicates: &[],
        claim: EffectStage::VisibleCurrent,
        shape: ShapeRules { required_identity_digests: &["observed_filetype"], ..DEFAULT_SHAPE },
        allowed_results: FULL_RESULTS,
    },
    ActionSpec {
        action_id: "neovim.native.client_attachment.identify_client_and_process",
        family: ActionFamily::ClientAttachment,
        class: ActionClass::Observation,
        surface: SurfaceClassification::PublicStable,
        summary: "identify the exact built-in-LSP client and the selected perllsp process",
        api_uses: &[GET_CLIENTS, CLIENT_CONFIG],
        native_surfaces: &["autocmd LspAttach"],
        instrument_hooks: &[],
        inputs: &[InputBinding { name: "document", kind: InputKind::FixtureDocument }],
        emits: &[EffectClass::ClientIdentity],
        required_predicates: &[predicate(
            PredicateKind::ClientInitializedExactProcess,
            INIT_SEQ_BUDGET_MS,
        )],
        claim: EffectStage::VisibleCurrent,
        shape: ShapeRules {
            required_identity_digests: &["client_config_cmd", "server_process_identity"],
            ..DEFAULT_SHAPE
        },
        allowed_results: FULL_RESULTS,
    },
    ActionSpec {
        action_id: "neovim.native.client_attachment.observe_initialize_identities",
        family: ActionFamily::ClientAttachment,
        class: ActionClass::Observation,
        surface: SurfaceClassification::PublicStable,
        summary: "observe the initialize/capability/root/workspace identities of the exact client",
        api_uses: &[GET_CLIENTS, CLIENT_CONFIG],
        native_surfaces: &["autocmd LspAttach"],
        instrument_hooks: &[],
        inputs: &[InputBinding { name: "root", kind: InputKind::FixtureOwner }],
        emits: &[EffectClass::InitializeIdentity],
        required_predicates: &[predicate(
            PredicateKind::ClientInitializedExactProcess,
            INIT_SEQ_BUDGET_MS,
        )],
        claim: EffectStage::VisibleCurrent,
        shape: ShapeRules {
            required_identity_digests: &[
                "client_capabilities",
                "server_capabilities",
                "workspace_root",
                "workspace_folders",
            ],
            ..DEFAULT_SHAPE
        },
        allowed_results: FULL_RESULTS,
    },
    ActionSpec {
        action_id: "neovim.native.client_attachment.exclude_foreign_clients",
        family: ActionFamily::ClientAttachment,
        class: ActionClass::Observation,
        surface: SurfaceClassification::PublicStable,
        summary: "rule out Coc and any other LSP client/server supplying the observation",
        api_uses: &[GET_CLIENTS],
        native_surfaces: &[],
        instrument_hooks: &[],
        inputs: &[InputBinding { name: "foreign", kind: InputKind::ForeignClientId }],
        emits: &[EffectClass::ForeignClientSet],
        required_predicates: &[],
        claim: EffectStage::VisibleCurrent,
        shape: ShapeRules {
            required_identity_digests: &["attached_client_ids"],
            requires_client_exclusion_cardinalities: true,
        },
        allowed_results: FULL_RESULTS,
    },
    // -----------------------------------------------------------------
    // Family C — diagnostics and read methods
    // -----------------------------------------------------------------
    ActionSpec {
        action_id: "neovim.native.read_methods.wait_target_diagnostic_state",
        family: ActionFamily::ReadMethods,
        class: ActionClass::Observation,
        surface: SurfaceClassification::PublicStable,
        summary: "wait for the exact target diagnostic code/range at the current document generation",
        api_uses: &[DIAG_GET],
        native_surfaces: &[],
        instrument_hooks: &[],
        inputs: &[
            InputBinding { name: "document", kind: InputKind::FixtureDocument },
            InputBinding { name: "expectation", kind: InputKind::ExpectationRef },
            InputBinding { name: "predicate", kind: InputKind::PredicateSpec },
            InputBinding { name: "floor", kind: InputKind::GenerationFloor },
        ],
        emits: &[EffectClass::DiagnosticState],
        required_predicates: &[predicate(PredicateKind::DiagnosticStateCurrent, SETTLE_BUDGET_MS)],
        claim: EffectStage::VisibleCurrent,
        shape: DEFAULT_SHAPE,
        allowed_results: FULL_RESULTS,
    },
    ActionSpec {
        action_id: "neovim.native.read_methods.request_completion",
        family: ActionFamily::ReadMethods,
        class: ActionClass::UserAction,
        surface: SurfaceClassification::PublicStable,
        summary: "request completion through the actual omnifunc insert-mode path",
        api_uses: &[OMNIFUNC],
        native_surfaces: &["keys"],
        instrument_hooks: &[InstrumentHookUse {
            api: WIRE_CAPTURE,
            justification: "request/consumption cardinality is not exposed by any public Neovim surface; the classified trace log is parsed read-only offline",
            retirement: "retire when Neovim exposes a public request telemetry surface",
        }],
        inputs: &[
            InputBinding { name: "document", kind: InputKind::FixtureDocument },
            InputBinding { name: "anchor", kind: InputKind::ContentAnchor },
            InputBinding { name: "expectation", kind: InputKind::ExpectationRef },
        ],
        emits: &[EffectClass::CompletionItems],
        required_predicates: &[predicate(PredicateKind::CompletionResultExact, SETTLE_BUDGET_MS)],
        claim: EffectStage::Returned,
        shape: DEFAULT_SHAPE,
        allowed_results: FULL_RESULTS,
    },
    ActionSpec {
        action_id: "neovim.native.read_methods.accept_completion",
        family: ActionFamily::ReadMethods,
        class: ActionClass::UserAction,
        surface: SurfaceClassification::PublicStable,
        summary: "accept the selected completion through the actual Neovim insert path",
        api_uses: &[],
        native_surfaces: &["keys"],
        instrument_hooks: &[],
        inputs: &[
            InputBinding { name: "document", kind: InputKind::FixtureDocument },
            InputBinding { name: "anchor", kind: InputKind::ContentAnchor },
            InputBinding { name: "expectation", kind: InputKind::ExpectationRef },
        ],
        emits: &[EffectClass::CompletionApplied],
        required_predicates: &[predicate(PredicateKind::AppliedBufferDigest, SETTLE_BUDGET_MS)],
        claim: EffectStage::VisibleCurrent,
        shape: DEFAULT_SHAPE,
        allowed_results: FULL_RESULTS,
    },
    ActionSpec {
        action_id: "neovim.native.read_methods.request_hover",
        family: ActionFamily::ReadMethods,
        class: ActionClass::UserAction,
        surface: SurfaceClassification::PublicStable,
        summary: "request hover at the anchor and observe the exact result",
        api_uses: &[BUF_HOVER],
        native_surfaces: &[],
        instrument_hooks: &[],
        inputs: &[
            InputBinding { name: "document", kind: InputKind::FixtureDocument },
            InputBinding { name: "anchor", kind: InputKind::ContentAnchor },
            InputBinding { name: "expectation", kind: InputKind::ExpectationRef },
        ],
        emits: &[EffectClass::HoverContent],
        required_predicates: &[predicate(PredicateKind::HoverResultExact, SETTLE_BUDGET_MS)],
        claim: EffectStage::Returned,
        shape: DEFAULT_SHAPE,
        allowed_results: FULL_RESULTS,
    },
    ActionSpec {
        action_id: "neovim.native.read_methods.drive_definition_navigation",
        family: ActionFamily::ReadMethods,
        class: ActionClass::UserAction,
        surface: SurfaceClassification::PublicStable,
        summary: "drive definition/navigation and observe the exact target role/content",
        api_uses: &[BUF_DEFINITION],
        native_surfaces: &[],
        instrument_hooks: &[],
        inputs: &[
            InputBinding { name: "document", kind: InputKind::FixtureDocument },
            InputBinding { name: "anchor", kind: InputKind::ContentAnchor },
            InputBinding { name: "expectation", kind: InputKind::ExpectationRef },
        ],
        emits: &[EffectClass::NavigationTarget],
        required_predicates: &[predicate(PredicateKind::NavigationResultExact, SETTLE_BUDGET_MS)],
        claim: EffectStage::VisibleCurrent,
        shape: DEFAULT_SHAPE,
        allowed_results: FULL_RESULTS,
    },
    ActionSpec {
        action_id: "neovim.native.read_methods.request_optional_cells",
        family: ActionFamily::ReadMethods,
        class: ActionClass::UserAction,
        surface: SurfaceClassification::PublicVersionScoped { scope: "neovim-0.10" },
        summary: "request selected optional cells (semantic tokens/inlay/code-lens) where selected",
        api_uses: &[SEMANTIC_TOKENS_START, INLAY_HINT_ENABLE],
        native_surfaces: &[],
        instrument_hooks: &[],
        inputs: &[
            InputBinding { name: "document", kind: InputKind::FixtureDocument },
            InputBinding { name: "cell", kind: InputKind::OptionalCell },
            InputBinding { name: "expectation", kind: InputKind::ExpectationRef },
        ],
        emits: &[EffectClass::OptionalCellResult],
        required_predicates: &[],
        claim: EffectStage::Returned,
        shape: DEFAULT_SHAPE,
        allowed_results: FULL_RESULTS,
    },
    // -----------------------------------------------------------------
    // Family D — configuration and edits
    // -----------------------------------------------------------------
    ActionSpec {
        action_id: "neovim.native.config_edits.apply_client_settings",
        family: ActionFamily::ConfigEdits,
        class: ActionClass::UserAction,
        surface: SurfaceClassification::PublicVersionScoped { scope: "neovim-0.11" },
        summary: "apply safe ClientConfig settings through the public config API",
        api_uses: &[LSP_CONFIG],
        native_surfaces: &[],
        instrument_hooks: &[],
        inputs: &[
            InputBinding { name: "setting", kind: InputKind::SettingName },
            InputBinding { name: "value", kind: InputKind::SettingValueDigest },
        ],
        emits: &[EffectClass::SettingEffect],
        required_predicates: &[],
        claim: EffectStage::Applied,
        shape: DEFAULT_SHAPE,
        allowed_results: FULL_RESULTS,
    },
    ActionSpec {
        action_id: "neovim.native.config_edits.observe_setting_effect",
        family: ActionFamily::ConfigEdits,
        class: ActionClass::Observation,
        surface: SurfaceClassification::PublicStable,
        summary: "observe the behavior-backed effect of a selected setting",
        api_uses: &[DIAG_GET],
        native_surfaces: &[],
        instrument_hooks: &[],
        inputs: &[
            InputBinding { name: "setting", kind: InputKind::SettingName },
            InputBinding { name: "expectation", kind: InputKind::ExpectationRef },
        ],
        emits: &[EffectClass::SettingEffect],
        required_predicates: &[predicate(PredicateKind::DiagnosticStateCurrent, SETTLE_BUDGET_MS)],
        claim: EffectStage::VisibleCurrent,
        shape: DEFAULT_SHAPE,
        allowed_results: FULL_RESULTS,
    },
    ActionSpec {
        action_id: "neovim.native.config_edits.request_document_format",
        family: ActionFamily::ConfigEdits,
        class: ActionClass::UserAction,
        surface: SurfaceClassification::PublicStable,
        summary: "request and apply document formatting through the public format API",
        api_uses: &[BUF_FORMAT],
        native_surfaces: &[],
        instrument_hooks: &[],
        inputs: &[
            InputBinding { name: "document", kind: InputKind::FixtureDocument },
            InputBinding { name: "expectation", kind: InputKind::ExpectationRef },
        ],
        emits: &[EffectClass::BufferState],
        required_predicates: &[predicate(PredicateKind::AppliedBufferDigest, SETTLE_BUDGET_MS)],
        claim: EffectStage::Applied,
        shape: DEFAULT_SHAPE,
        allowed_results: FULL_RESULTS,
    },
    ActionSpec {
        action_id: "neovim.native.config_edits.request_range_format",
        family: ActionFamily::ConfigEdits,
        class: ActionClass::UserAction,
        surface: SurfaceClassification::PublicStable,
        summary: "request and apply range formatting at the selected anchor",
        api_uses: &[BUF_FORMAT],
        native_surfaces: &[],
        instrument_hooks: &[],
        inputs: &[
            InputBinding { name: "document", kind: InputKind::FixtureDocument },
            InputBinding { name: "anchor", kind: InputKind::ContentAnchor },
            InputBinding { name: "expectation", kind: InputKind::ExpectationRef },
        ],
        emits: &[EffectClass::BufferState],
        required_predicates: &[predicate(PredicateKind::AppliedBufferDigest, SETTLE_BUDGET_MS)],
        claim: EffectStage::Applied,
        shape: DEFAULT_SHAPE,
        allowed_results: FULL_RESULTS,
    },
    ActionSpec {
        action_id: "neovim.native.config_edits.request_rename",
        family: ActionFamily::ConfigEdits,
        class: ActionClass::UserAction,
        surface: SurfaceClassification::PublicStable,
        summary: "request and apply a rename through the public rename API",
        api_uses: &[BUF_RENAME],
        native_surfaces: &[],
        instrument_hooks: &[],
        inputs: &[
            InputBinding { name: "document", kind: InputKind::FixtureDocument },
            InputBinding { name: "anchor", kind: InputKind::ContentAnchor },
            InputBinding { name: "expectation", kind: InputKind::ExpectationRef },
        ],
        emits: &[EffectClass::BufferState, EffectClass::FileState],
        required_predicates: &[predicate(PredicateKind::AppliedFileDigest, SETTLE_BUDGET_MS)],
        claim: EffectStage::Applied,
        shape: DEFAULT_SHAPE,
        allowed_results: FULL_RESULTS,
    },
    ActionSpec {
        action_id: "neovim.native.config_edits.request_code_action",
        family: ActionFamily::ConfigEdits,
        class: ActionClass::UserAction,
        surface: SurfaceClassification::PublicStable,
        summary: "request and apply a code action through the public code-action API",
        api_uses: &[BUF_CODE_ACTION],
        native_surfaces: &[],
        instrument_hooks: &[],
        inputs: &[
            InputBinding { name: "document", kind: InputKind::FixtureDocument },
            InputBinding { name: "anchor", kind: InputKind::ContentAnchor },
            InputBinding { name: "expectation", kind: InputKind::ExpectationRef },
        ],
        emits: &[EffectClass::BufferState],
        required_predicates: &[predicate(PredicateKind::AppliedBufferDigest, SETTLE_BUDGET_MS)],
        claim: EffectStage::Applied,
        shape: DEFAULT_SHAPE,
        allowed_results: FULL_RESULTS,
    },
    ActionSpec {
        action_id: "neovim.native.config_edits.request_workspace_edits",
        family: ActionFamily::ConfigEdits,
        class: ActionClass::UserAction,
        surface: SurfaceClassification::PublicStable,
        summary: "apply a workspace edit through the public apply helper",
        api_uses: &[APPLY_WORKSPACE_EDIT],
        native_surfaces: &[],
        instrument_hooks: &[],
        inputs: &[
            InputBinding { name: "document", kind: InputKind::FixtureDocument },
            InputBinding { name: "expectation", kind: InputKind::ExpectationRef },
        ],
        emits: &[EffectClass::BufferState, EffectClass::FileState],
        required_predicates: &[predicate(PredicateKind::AppliedFileDigest, SETTLE_BUDGET_MS)],
        claim: EffectStage::Applied,
        shape: DEFAULT_SHAPE,
        allowed_results: FULL_RESULTS,
    },
    ActionSpec {
        action_id: "neovim.native.config_edits.observe_resulting_state",
        family: ActionFamily::ConfigEdits,
        class: ActionClass::Observation,
        surface: SurfaceClassification::PublicStable,
        summary: "observe the exact resulting buffers/files/cursors/selections",
        api_uses: &[FN_GETCURPOS, FN_GETPOS],
        native_surfaces: &[],
        instrument_hooks: &[],
        inputs: &[
            InputBinding { name: "document", kind: InputKind::FixtureDocument },
            InputBinding { name: "expectation", kind: InputKind::ExpectationRef },
        ],
        emits: &[
            EffectClass::BufferState,
            EffectClass::FileState,
            EffectClass::CursorState,
            EffectClass::SelectionState,
        ],
        required_predicates: &[],
        claim: EffectStage::VisibleCurrent,
        shape: DEFAULT_SHAPE,
        allowed_results: FULL_RESULTS,
    },
    // -----------------------------------------------------------------
    // Family E — text synchronization and lifecycle
    // -----------------------------------------------------------------
    ActionSpec {
        action_id: "neovim.native.text_sync_lifecycle.ordinary_edit_didchange",
        family: ActionFamily::TextSyncLifecycle,
        class: ActionClass::UserAction,
        surface: SurfaceClassification::PublicStable,
        summary: "make an ordinary buffer edit and observe the actual didChange acceptance",
        api_uses: &[],
        native_surfaces: &["keys"],
        instrument_hooks: &[InstrumentHookUse {
            api: WIRE_CAPTURE,
            justification: "didChange request/consumption cardinality is not exposed by any public Neovim surface; the classified trace log is parsed read-only offline",
            retirement: "retire when Neovim exposes a public sync telemetry surface",
        }],
        inputs: &[
            InputBinding { name: "document", kind: InputKind::FixtureDocument },
            InputBinding { name: "anchor", kind: InputKind::ContentAnchor },
            InputBinding { name: "keys", kind: InputKind::KeySequence },
        ],
        emits: &[EffectClass::BufferState, EffectClass::DidChangeTraffic],
        required_predicates: &[predicate(
            PredicateKind::DocumentGenerationAccepted,
            SETTLE_BUDGET_MS,
        )],
        claim: EffectStage::Applied,
        shape: ShapeRules {
            required_identity_digests: &["accepted_document_generation"],
            ..DEFAULT_SHAPE
        },
        allowed_results: FULL_RESULTS,
    },
    ActionSpec {
        action_id: "neovim.native.text_sync_lifecycle.companion_multichange_control",
        family: ActionFamily::TextSyncLifecycle,
        class: ActionClass::CompanionControl,
        surface: SurfaceClassification::CompanionProtocolControl,
        summary: "checked companion multi-change raw control where the BDD selects it",
        api_uses: &[CLIENT_REQUEST],
        native_surfaces: &[],
        instrument_hooks: &[],
        inputs: &[
            InputBinding { name: "document", kind: InputKind::FixtureDocument },
            InputBinding { name: "control", kind: InputKind::CompanionControlSpec },
        ],
        emits: &[EffectClass::CompanionResult],
        required_predicates: &[],
        claim: EffectStage::Requested,
        shape: DEFAULT_SHAPE,
        allowed_results: FULL_RESULTS,
    },
    ActionSpec {
        action_id: "neovim.native.text_sync_lifecycle.companion_invalid_notification_control",
        family: ActionFamily::TextSyncLifecycle,
        class: ActionClass::CompanionControl,
        surface: SurfaceClassification::CompanionProtocolControl,
        summary: "checked companion invalid-notification desync control where the BDD selects it",
        api_uses: &[CLIENT_REQUEST],
        native_surfaces: &[],
        instrument_hooks: &[],
        inputs: &[
            InputBinding { name: "document", kind: InputKind::FixtureDocument },
            InputBinding { name: "control", kind: InputKind::CompanionControlSpec },
        ],
        emits: &[EffectClass::CompanionResult],
        required_predicates: &[],
        claim: EffectStage::Requested,
        shape: DEFAULT_SHAPE,
        allowed_results: FULL_RESULTS,
    },
    ActionSpec {
        action_id: "neovim.native.text_sync_lifecycle.full_source_recovery_reopen",
        family: ActionFamily::TextSyncLifecycle,
        class: ActionClass::UserAction,
        surface: SurfaceClassification::PublicStable,
        summary: "recover from desync through full-source replace/reopen and re-acceptance",
        api_uses: &[],
        native_surfaces: &[":e"],
        instrument_hooks: &[],
        inputs: &[
            InputBinding { name: "document", kind: InputKind::FixtureDocument },
            InputBinding { name: "expectation", kind: InputKind::ExpectationRef },
        ],
        emits: &[EffectClass::RecoveryState, EffectClass::BufferState],
        required_predicates: &[
            predicate(PredicateKind::DocumentGenerationAccepted, SETTLE_BUDGET_MS),
            predicate(PredicateKind::AppliedBufferDigest, SETTLE_BUDGET_MS),
        ],
        claim: EffectStage::Applied,
        shape: DEFAULT_SHAPE,
        allowed_results: FULL_RESULTS,
    },
    ActionSpec {
        action_id: "neovim.native.text_sync_lifecycle.held_work_barrier",
        family: ActionFamily::TextSyncLifecycle,
        class: ActionClass::Observation,
        surface: SurfaceClassification::InstrumentOnlyHook { owner: "shared_host_execution_10894" },
        summary: "hold/release pending work at the generic held-work barrier (supplied by generic owners)",
        api_uses: &[],
        native_surfaces: &[],
        instrument_hooks: &[InstrumentHookUse {
            api: INFLIGHT_TABLE,
            justification: "in-flight request settlement is not exposed by any public Neovim surface; the classified client request table is inspected read-only",
            retirement: "retire when Neovim exposes a public in-flight request surface",
        }],
        inputs: &[InputBinding { name: "expectation", kind: InputKind::ExpectationRef }],
        emits: &[EffectClass::HeldWorkDisposition],
        required_predicates: &[predicate(PredicateKind::ParserEffectTicket, SETTLE_BUDGET_MS)],
        claim: EffectStage::Returned,
        shape: DEFAULT_SHAPE,
        allowed_results: FULL_RESULTS,
    },
    ActionSpec {
        action_id: "neovim.native.text_sync_lifecycle.root_add_remove",
        family: ActionFamily::TextSyncLifecycle,
        class: ActionClass::UserAction,
        surface: SurfaceClassification::PublicStable,
        summary: "add/remove a workspace root where selected and observe re-acceptance",
        api_uses: &[FN_MKDIR, FN_DELETE],
        native_surfaces: &[],
        instrument_hooks: &[],
        inputs: &[InputBinding { name: "root", kind: InputKind::FixtureOwner }],
        emits: &[EffectClass::RootChange],
        required_predicates: &[predicate(
            PredicateKind::DocumentGenerationAccepted,
            SETTLE_BUDGET_MS,
        )],
        claim: EffectStage::Applied,
        shape: DEFAULT_SHAPE,
        allowed_results: FULL_RESULTS,
    },
    ActionSpec {
        action_id: "neovim.native.text_sync_lifecycle.post_run_observation_handoff",
        family: ActionFamily::TextSyncLifecycle,
        class: ActionClass::HostHandoff,
        surface: SurfaceClassification::NotExposed,
        summary: "hand post-run observations to the #10894/generic receipt owners",
        api_uses: &[],
        native_surfaces: &[],
        instrument_hooks: &[],
        inputs: &[],
        emits: &[EffectClass::HandoffState],
        required_predicates: &[],
        claim: EffectStage::Requested,
        shape: DEFAULT_SHAPE,
        allowed_results: HANDOFF_RESULTS,
    },
];

/// Look up one action by ID. Unknown IDs are the caller's typed error.
pub fn action_by_id(action_id: &str) -> Option<&'static ActionSpec> {
    ACTIONS.iter().find(|action| action.action_id == action_id)
}

/// The pinned subject identity tokens of this contract. The exact Neovim
/// build bytes are #11406's authority and the canonical config bytes are
/// #10502/#7768's: both are cited by registered identity tokens here and
/// fail closed on any other token, while the exact bytes stay with their
/// owning authorities. `root_id` and the document binding are per-fixture
/// run bindings (fixture/expectation authority #10903; the workspace-root
/// model is deliberately per-run because the `root_add_remove` action
/// exercises changing it), validated as bounded tokens/paths and tightened
/// to exact fixture rows when #10903 lands.
pub const PINNED_HOST_PRODUCT: &str = "neovim";
pub const PINNED_CLIENT_ID: &str = "vim.lsp";
pub const PINNED_SERVER_EXECUTABLE: &str = "perllsp";
/// Identity token citing the pinned Neovim host build (authority #11406).
pub const PINNED_HOST_VERSION_SCOPE: &str = "neovim_host_build_11406";
/// Identity token citing the exact canonical config subject (#10502/#7768).
pub const PINNED_CONFIG_ID: &str = "canonical_config_10502";

/// The closed host-handoff channel vocabulary: handoffs are bounded
/// references to the #10894 authority, never free-form process policy.
pub const HOST_HANDOFF_TOKENS: &[&str] = &["host_process_handoff", "post_run_observation_handoff"];

/// The closed test-stimulus channel vocabulary.
pub const TEST_STIMULUS_TOKENS: &[&str] = &["deliberate_stimulus"];

/// The currentness dimensions each predicate kind must settle at the
/// observation's own generation: an older-generation settlement can never
/// prove a current result (#11409 falsifier 8).
pub fn predicate_floor_dimensions(
    kind: PredicateKind,
) -> &'static [predicate::GenerationDimension] {
    use predicate::GenerationDimension as Dimension;
    match kind {
        PredicateKind::ClientInitializedExactProcess | PredicateKind::ClientTerminalState => {
            &[Dimension::Host, Dimension::Process]
        }
        PredicateKind::DiagnosticStateCurrent
        | PredicateKind::CompletionResultExact
        | PredicateKind::HoverResultExact
        | PredicateKind::NavigationResultExact
        | PredicateKind::AppliedBufferDigest
        | PredicateKind::DocumentGenerationAccepted
        | PredicateKind::ParserEffectTicket => &[Dimension::Document],
        PredicateKind::AppliedFileDigest => &[Dimension::Document, Dimension::Source],
    }
}

/// Grammar for public Neovim API spellings: dotted lowercase Lua paths under
/// the built-in `vim.lsp`/`vim.diagnostic`/`vim.fn`/`vim.bo` roots. Nothing
/// else may ride in this channel; a Coc plugin API (`coc#...`) fails the
/// grammar outright.
pub fn is_neovim_api_spelling(spelling: &str) -> bool {
    for root in ["vim.lsp", "vim.diagnostic", "vim.fn", "vim.bo"] {
        let prefix = format!("{root}.");
        if let Some(rest) = spelling.strip_prefix(&prefix) {
            return !rest.is_empty()
                && rest.len() <= 60
                && rest.split('.').all(|segment| {
                    !segment.is_empty()
                        && segment.bytes().all(|byte| {
                            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'
                        })
                });
        }
    }
    false
}

/// Grammar for native editor surface spellings: an Ex command (`:e`,
/// `:bwipeout`, `:qa!`), a `LspAttach`-style autocmd event
/// (`autocmd LspAttach`), or the insert-mode user-key channel (`keys` or
/// `keys <c-x><c-o>`).
pub fn is_native_editor_surface(spelling: &str) -> bool {
    if let Some(command) = spelling.strip_prefix(':') {
        return !command.is_empty()
            && command.len() <= 12
            && command.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'!');
    }
    if let Some(event) = spelling.strip_prefix("autocmd ") {
        return !event.is_empty()
            && event.len() <= 24
            && event.bytes().all(|byte| byte.is_ascii_alphabetic());
    }
    if spelling == "keys" {
        return true;
    }
    if let Some(keys) = spelling.strip_prefix("keys ") {
        return !keys.is_empty()
            && keys.len() <= 24
            && keys.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'<' | b'>' | b'-' | b'_')
            });
    }
    false
}

/// The channel token of a native surface spelling (the first
/// whitespace-separated token).
fn native_channel(surface: &str) -> &str {
    surface.split(' ').next().unwrap_or(surface)
}

/// Validate one observation against its action's laws. Returns the classified
/// result or a precise violation; never mutates receipt or catalog state.
pub fn validate_observation(
    observation: &TypedObservation,
) -> Result<ValidatedObservation, String> {
    if observation.schema_version != CONTRACT_SCHEMA_VERSION {
        return Err(format!(
            "schema version {} does not match {CONTRACT_SCHEMA_VERSION}",
            observation.schema_version
        ));
    }
    let action = action_by_id(&observation.action_id)
        .ok_or_else(|| format!("unknown action id: {}", observation.action_id))?;

    observation::validate_bounded(observation)?;

    // Subject pin: only the exact Neovim + built-in vim.lsp + perllsp
    // subject, on the registered host-build identity (#11406) and canonical
    // config identity (#10502/#7768), may observe. A Coc/other client, a
    // renamed-server workaround, an invented host build, or a foreign config
    // is rejected. Root/document stay bounded per-fixture bindings (#10903).
    let subject = &observation.subject;
    if subject.host_product != PINNED_HOST_PRODUCT
        || subject.client_id != PINNED_CLIENT_ID
        || subject.server_executable != PINNED_SERVER_EXECUTABLE
        || subject.host_version_scope != PINNED_HOST_VERSION_SCOPE
        || subject.config_id != PINNED_CONFIG_ID
    {
        return Err(format!(
            "observation subject {}/{}/{}/{}/{} is not the pinned neovim host build and canonical config subject",
            subject.host_product,
            subject.client_id,
            subject.server_executable,
            subject.host_version_scope,
            subject.config_id
        ));
    }

    // Plane law: product, instrument, and cleanup observations stay separate;
    // the reporting plane is reserved for the generic reporting/receipt
    // owners and can never be emitted by this contract's actions.
    let expected_plane =
        if matches!(action.surface, SurfaceClassification::InstrumentOnlyHook { .. }) {
            // Route-derived refinement: an action whose only helper surface is
            // an instrument-only hook emits instrument-plane evidence even
            // though its class is observational — the plane must reflect how
            // the observation can actually be obtained, not the class alone.
            ObservationPlane::Instrument
        } else {
            match action.class {
                ActionClass::UserAction | ActionClass::Observation => ObservationPlane::Product,
                ActionClass::CompanionControl | ActionClass::TestStimulus => {
                    ObservationPlane::Instrument
                }
                ActionClass::HostHandoff => ObservationPlane::Cleanup,
            }
        };
    if observation.plane == ObservationPlane::Reporting {
        return Err(
            "the reporting plane is reserved for the generic reporting/receipt owners".to_string()
        );
    }
    if observation.plane != expected_plane {
        return Err(format!(
            "plane {:?} does not match action class {:?} (expected {expected_plane:?})",
            observation.plane, action.class
        ));
    }

    // Route/surface law: the executed route must match how the action's
    // helper surface is classified; a companion control can never label
    // itself ordinary Neovim traffic and an instrument hook must name its
    // exact owner.
    match &observation.route {
        ObservedRoute::PublicStableApi { api } => {
            if action.surface != SurfaceClassification::PublicStable {
                return Err(format!(
                    "action {} does not classify {} as a public stable api",
                    action.action_id, api
                ));
            }
            if !action.api_uses.contains(&api.as_str()) {
                return Err(format!(
                    "action {} does not declare public api {api}",
                    action.action_id
                ));
            }
        }
        ObservedRoute::VersionScopedApi { api, scope } => {
            let SurfaceClassification::PublicVersionScoped { scope: pinned_scope } = action.surface
            else {
                return Err(format!(
                    "action {} does not classify {} as a version-scoped api",
                    action.action_id, api
                ));
            };
            if scope.as_str() != pinned_scope {
                return Err(format!(
                    "action {} is version-scoped to {pinned_scope}, not {scope}; the exact scope is load-bearing",
                    action.action_id
                ));
            }
            if !action.api_uses.contains(&api.as_str()) {
                return Err(format!(
                    "action {} does not declare public api {api}",
                    action.action_id
                ));
            }
        }
        ObservedRoute::NativeEditorSurface { surface } => {
            if !matches!(
                action.surface,
                SurfaceClassification::PublicStable
                    | SurfaceClassification::PublicVersionScoped { .. }
            ) {
                return Err(format!(
                    "action {} does not classify a public native editor surface",
                    action.action_id
                ));
            }
            if !action.native_surfaces.iter().any(|declared| {
                *declared == surface.as_str() || native_channel(surface) == *declared
            }) {
                return Err(format!(
                    "action {} does not declare native surface {surface}",
                    action.action_id
                ));
            }
        }
        ObservedRoute::CompanionControl { control } => {
            if action.class != ActionClass::CompanionControl
                || action.surface != SurfaceClassification::CompanionProtocolControl
            {
                return Err(format!(
                    "companion control {control} offered as the route of {}; a raw companion request is never ordinary Neovim traffic",
                    action.action_id
                ));
            }
            // Exact membership, same as public APIs: a control token owned by
            // another action or invented inline can never satisfy the route.
            if !action.api_uses.contains(&control.as_str()) {
                return Err(format!(
                    "action {} does not declare companion control {control}",
                    action.action_id
                ));
            }
        }
        ObservedRoute::InstrumentHook { hook, owner } => {
            let SurfaceClassification::InstrumentOnlyHook { owner: pinned_owner } = action.surface
            else {
                return Err(format!(
                    "instrument hook {hook} offered as the route of {} which is not classified instrument-only",
                    action.action_id
                ));
            };
            if owner != pinned_owner {
                return Err(format!(
                    "instrument hook owner {owner} does not match the exact owner {pinned_owner}"
                ));
            }
            if !action.instrument_hooks.iter().any(|use_| use_.api == hook) {
                return Err(format!(
                    "action {} does not declare instrument hook {hook}",
                    action.action_id
                ));
            }
        }
        ObservedRoute::TestStimulus { stimulus } => {
            if action.class != ActionClass::TestStimulus {
                return Err(format!(
                    "test stimulus {stimulus} offered as the route of ordinary action {}",
                    action.action_id
                ));
            }
            if !TEST_STIMULUS_TOKENS.contains(&stimulus.as_str()) {
                return Err(format!(
                    "test stimulus {stimulus} is outside the closed stimulus vocabulary"
                ));
            }
        }
        ObservedRoute::HostHandoff { handoff } => {
            if action.class != ActionClass::HostHandoff {
                return Err(format!(
                    "host handoff {handoff} offered as the route of {}; process spawn/deadline/cleanup is owned by #10894, not a host action",
                    action.action_id
                ));
            }
            if !HOST_HANDOFF_TOKENS.contains(&handoff.as_str()) {
                return Err(format!(
                    "host handoff {handoff} is outside the closed handoff vocabulary; a handoff is a bounded reference to #10894, never free-form process policy"
                ));
            }
        }
    }

    // Predicate law: every required predicate must be Satisfied within its
    // budget over generations no newer than the observation's own snapshot;
    // evidence for a predicate the action does not require is a routing
    // violation; a TimedOut predicate forces not_proven; a Substituted
    // predicate is dishonest.
    let required: BTreeMap<PredicateKind, u64> = action
        .required_predicates
        .iter()
        .map(|requirement| (requirement.kind, requirement.max_wait_ms))
        .collect();
    let mut seen_kinds = BTreeSet::new();
    for evidence in &observation.predicate_evidence {
        let kind = evidence.kind();
        let budget = required.get(&kind).copied().ok_or_else(|| {
            format!(
                "observation for {} carries {kind:?} evidence the action does not require",
                action.action_id
            )
        })?;
        if !seen_kinds.insert(kind) {
            return Err(format!(
                "duplicate predicate evidence for {kind:?} in {}",
                action.action_id
            ));
        }
        match evidence {
            PredicateEvidence::Satisfied {
                settled_state_digest,
                settled_generations,
                polls,
                waited_ms,
                ..
            } => {
                if !observation::is_bounded_digest(settled_state_digest) {
                    return Err(format!(
                        "predicate {kind:?} satisfaction does not name its settled state; elapsed time alone is never satisfaction"
                    ));
                }
                if *polls == 0 {
                    return Err(format!(
                        "predicate {kind:?} claims satisfaction without a single poll"
                    ));
                }
                if *waited_ms > budget {
                    return Err(format!(
                        "predicate {kind:?} claims satisfaction after {waited_ms}ms, beyond the {budget}ms budget"
                    ));
                }
                for dimension in predicate::GENERATION_DIMENSIONS {
                    let settled = settled_generations.dimension(*dimension);
                    let observed = observation.generations.dimension(*dimension);
                    if settled > observed {
                        return Err(format!(
                            "predicate {kind:?} settled at a newer {dimension:?} generation ({settled}) than the observation snapshot ({observed})"
                        ));
                    }
                }
                // Generation floor: on the dimensions the predicate kind is
                // about, the settlement must be at the observation's own
                // generation — an older-generation settlement can never
                // prove a current result (#11409 falsifier 8).
                for dimension in predicate_floor_dimensions(kind) {
                    let settled = settled_generations.dimension(*dimension);
                    let observed = observation.generations.dimension(*dimension);
                    if settled < observed {
                        return Err(format!(
                            "predicate {kind:?} settled at an older {dimension:?} generation ({settled}) than the observation snapshot ({observed}); stale state cannot prove a current result"
                        ));
                    }
                }
            }
            PredicateEvidence::TimedOut { polls, waited_ms, .. } => {
                if *polls == 0 || *waited_ms == 0 {
                    return Err(format!(
                        "predicate {kind:?} timeout must record its bounded polls and wait"
                    ));
                }
                if *waited_ms > budget {
                    return Err(format!(
                        "predicate {kind:?} timed out after {waited_ms}ms, beyond the {budget}ms budget; a run that violated the declared deadline is not bounded evidence"
                    ));
                }
                if observation.result != ObservationResult::NotProven {
                    return Err(format!(
                        "predicate {kind:?} timed out but result is {:?}; a timeout must classify not_proven",
                        observation.result
                    ));
                }
            }
            PredicateEvidence::Substituted { substitution, .. } => {
                return Err(format!(
                    "predicate {kind:?} evidence is a {substitution:?} substitution; substitutions are never state"
                ));
            }
        }
    }
    for kind in required.keys() {
        if !seen_kinds.contains(kind) {
            return Err(format!(
                "action {} requires predicate {kind:?} but the observation carries no evidence for it",
                action.action_id
            ));
        }
    }

    // Effect routing law: the reported effect classes must be within the
    // action's declared emissions.
    let effect = &observation.observed;
    if effect.effect_classes.is_empty() {
        return Err(format!("observation for {} reports no effect class", action.action_id));
    }
    for class in &effect.effect_classes {
        if !action.emits.contains(class) {
            return Err(format!(
                "effect class {class:?} is outside what {} may emit",
                action.action_id
            ));
        }
    }

    // Application/currentness law: requested/returned/applied/visible-current
    // stay distinct. An `observed` result requires the claimed minimum
    // stage, an applied-or-beyond effect must bind its effect digest, and
    // the effect must be current at the observation's own generation — an
    // old-generation result can never satisfy a post-edit action.
    if observation.result == ObservationResult::Observed {
        if effect.stage < action.claim {
            return Err(format!(
                "effect stopped at {:?} but {} claims {:?}; a returned result cannot satisfy an application claim",
                effect.stage, action.action_id, action.claim
            ));
        }
        if effect.generations != observation.generations {
            return Err(format!(
                "observed result computed against generation {:?} while the observation settled at {:?}; an old-generation result must classify mismatch or not_proven",
                effect.generations, observation.generations
            ));
        }
    }
    if effect.stage >= EffectStage::Applied && effect.effect_digest.is_none() {
        return Err("an applied-or-beyond effect must bind its applied effect digest".to_string());
    }

    // Expectation law: expected results come from the fixture authority; a
    // self-derived expectation (captured from the production output) is
    // always rejected, and expectation-requiring actions must carry one.
    if let Some(expectation) = &observation.expectation
        && expectation.source == ExpectationSource::ObservedOutput
    {
        return Err(
            "expectation derived from the observed output; expected results are independent fixture-owned facts"
                .to_string(),
        );
    }
    if action.requires_expectation() && observation.expectation.is_none() {
        return Err(format!("action {} requires an expected-result reference", action.action_id));
    }
    // Comparison law: where an exact expectation is required, the result
    // token is the comparison of the two bound values. `observed` with
    // differing digests is an any-result-satisfies shape; `mismatch` with
    // equal digests contradicts the bound values. Both channels stay
    // structurally separate — the law only constrains their coherence.
    if action.requires_expectation()
        && let Some(expectation) = &observation.expectation
    {
        if observation.result == ObservationResult::Observed
            && effect.result_digest != expectation.expectation_digest
        {
            return Err(
                "result claims observed but the observed result digest differs from the expected-result digest; an exact expectation cannot be satisfied by any result"
                    .to_string(),
            );
        }
        if observation.result == ObservationResult::Mismatch
            && effect.result_digest == expectation.expectation_digest
        {
            return Err(
                "result claims mismatch while the observed digest equals the expected digest; the comparison outcome contradicts the bound values"
                    .to_string(),
            );
        }
    }

    // Anchor law: actions that take a content anchor input must bind its
    // resolved position.
    if action.requires_anchor() && effect.anchor_positions.is_empty() {
        return Err(format!(
            "action {} takes a content anchor input but binds no resolved anchor position",
            action.action_id
        ));
    }

    // Shape laws.
    for key in action.shape.required_identity_digests {
        if !effect.identity_digests.contains_key(*key) {
            return Err(format!("action {} must bind the {key} identity digest", action.action_id));
        }
    }
    if action.shape.requires_client_exclusion_cardinalities {
        let pinned = effect.cardinalities.get("pinned_clients_attached").copied();
        let foreign = effect.cardinalities.get("foreign_clients_attached").copied();
        if pinned.unwrap_or(0) == 0 || foreign.unwrap_or(0) != 0 {
            return Err(format!(
                "client exclusion must observe the pinned client attached ({pinned:?}) and zero foreign clients ({foreign:?})"
            ));
        }
    }

    // Result vocabulary and limitation/failure-class law.
    if !action.allowed_results.contains(&observation.result) {
        return Err(format!(
            "result {:?} is outside the admitted vocabulary of {}",
            observation.result, action.action_id
        ));
    }
    if observation.result.requires_limitation() && observation.limitation_class.is_none() {
        return Err(format!("result {:?} requires a limitation/failure class", observation.result));
    }
    if observation.result == ObservationResult::Observed && observation.limitation_class.is_some() {
        return Err("an observed result cannot carry a limitation class".to_string());
    }

    Ok(ValidatedObservation {
        action_id: action.action_id.to_string(),
        family: action.family,
        plane: observation.plane,
        result: observation.result,
        limitation_class: observation.limitation_class.clone(),
        backend: observation.backend.clone(),
    })
}

/// Validate one run's observations: each observation individually valid, and
/// the ordered action identity strictly increasing and unique.
pub fn validate_observation_run(observations: &[TypedObservation]) -> Result<usize> {
    ensure!(!observations.is_empty(), "a run must carry at least one observation");
    let mut last_sequence = 0u64;
    for observation in observations {
        validate_observation(observation).map_err(|error| {
            anyhow::anyhow!(
                "observation {} (sequence {}) failed validation: {error}",
                observation.action_id,
                observation.sequence
            )
        })?;
        ensure!(
            observation.sequence > last_sequence,
            "sequence {} for {} does not increase past {last_sequence}; ordered action identity is load-bearing",
            observation.sequence,
            observation.action_id
        );
        last_sequence = observation.sequence;
    }
    Ok(observations.len())
}

/// Validate an observations file (one JSON observation per line) as one run.
pub fn validate_observation_file(path: &Path) -> Result<usize> {
    let bytes =
        std::fs::read(path).with_context(|| format!("reading observations {}", path.display()))?;
    let text = String::from_utf8(bytes).context("observations file must be UTF-8")?;
    let mut observations = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let observation: TypedObservation = serde_json::from_str(trimmed)
            .with_context(|| format!("parsing observation at line {}", index + 1))?;
        observations.push(observation);
    }
    validate_observation_run(&observations)
        .with_context(|| format!("validating run in {}", path.display()))
}

/// Summary of a successful contract validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractSummary {
    pub action_count: usize,
    pub family_counts: Vec<(ActionFamily, usize)>,
    pub vocabulary_digest: String,
}

/// Validate the compiled vocabulary of current main.
pub fn validate_compiled_contract() -> Result<ContractSummary> {
    validate_table(ACTIONS)
}

/// Validate an arbitrary action table (the compiled vocabulary, or a mutated
/// copy under test) against the fail-closed table laws.
pub fn validate_table(actions: &[ActionSpec]) -> Result<ContractSummary> {
    let mut seen = BTreeSet::new();
    for action in actions {
        ensure!(seen.insert(action.action_id), "duplicate action id {}", action.action_id);
        let suffix = action.action_id.strip_prefix(ACTION_ID_PREFIX).with_context(|| {
            format!("action id {} outside the {ACTION_ID_PREFIX} namespace", action.action_id)
        })?;
        let segments: Vec<&str> = suffix.split('.').collect();
        ensure!(
            segments.len() == 2
                && segments[0] == action.family.token()
                && crate::client_compat_fixture::is_reason_token(segments[0])
                && crate::client_compat_fixture::is_reason_token(segments[1]),
            "action id {} does not spell neovim.native.<family>.<name> for family {}",
            action.action_id,
            action.family.token()
        );
        for api in action.api_uses {
            ensure!(
                is_neovim_api_spelling(api),
                "action {} cites api {api} outside the Neovim grammar",
                action.action_id
            );
        }
        for surface in action.native_surfaces {
            ensure!(
                is_native_editor_surface(surface),
                "action {} cites native surface {surface} outside the native editor grammar",
                action.action_id
            );
        }
        for hook in action.instrument_hooks {
            ensure!(
                is_neovim_api_spelling(hook.api),
                "action {} cites instrument hook {} outside the Neovim grammar",
                action.action_id,
                hook.api
            );
            ensure!(
                !hook.justification.trim().is_empty() && !hook.retirement.trim().is_empty(),
                "action {} cites an instrument hook without justification and retirement condition",
                action.action_id
            );
        }
        match action.surface {
            SurfaceClassification::PublicStable => {}
            SurfaceClassification::PublicVersionScoped { scope } => {
                ensure!(
                    crate::client_compat_fixture::is_reason_token(scope),
                    "action {} declares a version scope that is not a stable token: {scope}",
                    action.action_id
                );
            }
            SurfaceClassification::CompanionProtocolControl => {
                ensure!(
                    action.class == ActionClass::CompanionControl,
                    "action {} classifies companion protocol control but is not a companion-class action",
                    action.action_id
                );
                ensure!(
                    action.claim <= EffectStage::Requested,
                    "action {} is a companion control and cannot claim an application stage",
                    action.action_id
                );
            }
            SurfaceClassification::InstrumentOnlyHook { owner } => {
                ensure!(
                    crate::client_compat_fixture::is_reason_token(owner),
                    "action {} declares an instrument hook owner that is not a stable token: {owner}",
                    action.action_id
                );
                ensure!(
                    !action.instrument_hooks.is_empty(),
                    "action {} classifies instrument-only but cites no instrument hook",
                    action.action_id
                );
            }
            SurfaceClassification::NotExposed => {
                ensure!(
                    !action.allowed_results.contains(&ObservationResult::Observed),
                    "action {} is not exposed and can never claim an observed result",
                    action.action_id
                );
            }
        }
        if action.class == ActionClass::HostHandoff {
            ensure!(
                action.surface == SurfaceClassification::NotExposed,
                "host handoff action {} must stay fail-closed behind #10894",
                action.action_id
            );
        }
        if action.class == ActionClass::CompanionControl {
            ensure!(
                action.surface == SurfaceClassification::CompanionProtocolControl,
                "companion-class action {} must classify its companion protocol control",
                action.action_id
            );
        }
        let mut input_names = BTreeSet::new();
        for input in action.inputs {
            ensure!(
                crate::client_compat_fixture::is_reason_token(input.name),
                "action {} input name {} is not a stable token",
                action.action_id,
                input.name
            );
            ensure!(
                input_names.insert(input.name),
                "action {} declares duplicate input {}",
                action.action_id,
                input.name
            );
        }
        ensure!(!action.emits.is_empty(), "action {} emits nothing", action.action_id);
        for requirement in action.required_predicates {
            ensure!(
                requirement.max_wait_ms > 0,
                "action {} declares a zero wait budget for predicate {:?}",
                action.action_id,
                requirement.kind
            );
        }
        ensure!(
            !action.allowed_results.is_empty(),
            "action {} admits no result vocabulary",
            action.action_id
        );
        ensure!(
            action.allowed_results.contains(&ObservationResult::NotProven),
            "action {} must admit not_proven; honest failure is always possible",
            action.action_id
        );
    }

    let mut family_counts: BTreeMap<ActionFamily, usize> = BTreeMap::new();
    for action in actions {
        *family_counts.entry(action.family).or_default() += 1;
    }
    Ok(ContractSummary {
        action_count: actions.len(),
        family_counts: family_counts.into_iter().collect(),
        vocabulary_digest: table_digest(actions)?,
    })
}

/// Stable digest of the published action vocabulary, so any semantic edit is
/// a visible identity change for downstream consumers.
pub fn contract_vocabulary_digest() -> Result<String> {
    table_digest(ACTIONS)
}

/// Write one action's full binding into the canonical digest buffer.
fn write_action_binding(canonical: &mut String, action: &ActionSpec) -> Result<()> {
    let _ = writeln!(canonical, "{}", action.action_id);
    let _ = writeln!(canonical, "family:{}", action.family.token());
    let _ = writeln!(canonical, "class:{:?}", action.class);
    match action.surface {
        SurfaceClassification::PublicStable => {
            let _ = writeln!(canonical, "surface:public_stable");
        }
        SurfaceClassification::PublicVersionScoped { scope } => {
            let _ = writeln!(canonical, "surface:public_version_scoped:{scope}");
        }
        SurfaceClassification::CompanionProtocolControl => {
            let _ = writeln!(canonical, "surface:companion_protocol_control");
        }
        SurfaceClassification::InstrumentOnlyHook { owner } => {
            let _ = writeln!(canonical, "surface:instrument_only_hook:{owner}");
        }
        SurfaceClassification::NotExposed => {
            let _ = writeln!(canonical, "surface:not_exposed");
        }
    }
    let _ = writeln!(canonical, "summary:{}", action.summary);
    for api in action.api_uses {
        let _ = writeln!(canonical, "api:{api}");
    }
    for surface in action.native_surfaces {
        let _ = writeln!(canonical, "native:{surface}");
    }
    for hook in action.instrument_hooks {
        let _ = writeln!(canonical, "hook:{}|{}|{}", hook.api, hook.justification, hook.retirement);
    }
    for input in action.inputs {
        let _ = writeln!(canonical, "input:{}:{:?}", input.name, input.kind);
    }
    for class in action.emits {
        let _ = writeln!(canonical, "emit:{class:?}");
    }
    for requirement in action.required_predicates {
        let _ = writeln!(canonical, "predicate:{:?}:{}", requirement.kind, requirement.max_wait_ms);
    }
    let _ = writeln!(canonical, "claim:{:?}", action.claim);
    for key in action.shape.required_identity_digests {
        let _ = writeln!(canonical, "identity:{key}");
    }
    let _ = writeln!(
        canonical,
        "exclusion_cardinalities:{}",
        action.shape.requires_client_exclusion_cardinalities
    );
    for result in action.allowed_results {
        let _ = writeln!(canonical, "result:{result:?}");
    }
    Ok(())
}

/// Hash the canonical buffer with the shared `sha256:`-prefixed spelling.
fn digest_canonical(canonical: &str) -> Result<String> {
    let mut identity = String::with_capacity("sha256:".len() + 64);
    identity.push_str("sha256:");
    for byte in Sha256::digest(canonical.as_bytes()) {
        write!(&mut identity, "{byte:02x}")?;
    }
    Ok(identity)
}

/// Digest one action table (the compiled vocabulary or a mutated copy).
fn table_digest(actions: &[ActionSpec]) -> Result<String> {
    let mut canonical = String::new();
    for action in actions {
        write_action_binding(&mut canonical, action)?;
    }
    digest_canonical(&canonical)
}
