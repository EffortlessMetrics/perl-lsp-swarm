//! Deterministic first-class action and observation primitives for the
//! specialized Vim/vim-lsp driver (#11380).
//!
//! Ownership split — consumed, never duplicated:
//!
//! - This module owns the specialized action vocabulary for the five #11376
//!   families (freshness, save-format, server-generation recovery, host
//!   reopen/repeated sessions, expanded activation): which actions exist, what
//!   each one executes, which observable-state barriers must settle, which
//!   #11369-classified client surfaces the real adapter may call, and which
//!   typed observations come back. Rust/xtask owns orchestration, identity,
//!   boundedness, and classification policy.
//! - `.ci/editor-clients/vim-vim-lsp-public-surface.v1.json` (#11369) owns the
//!   classified public API inventory. Every vim-lsp surface this vocabulary
//!   cites must appear there; anything else is `client_not_exposed`, never a
//!   synthesized raw protocol substitute.
//! - `crate::vim_lsp_cell_catalog` (#11374/#12100) owns journey-cell
//!   registration. This driver is receipt-agnostic: it validates and
//!   classifies observations, it never registers a cell, writes a receipt, or
//!   decides support. #11376 will land the family journey scenarios and #11378
//!   the semantic fixture expectations; until then the vocabulary binds only
//!   the landed #11369 substrate and the #11371 baseline ledger stays frozen.
//! - #10894/#10944 own parent processes, deadlines, process ledgers, cleanup
//!   guards, and hermetic host startup. This driver emits/consumes bounded
//!   handoff observations and fails closed (`not_proven`) where those
//!   authorities have not landed.
//!
//! Fail-closed laws enforced by [`validate_observation`] and
//! [`validate_driver_contract`]:
//!
//! - a fixed sleep, event-log artifact, raw protocol response, bare process
//!   existence, server restart, manual format, or pre-forced filetype offered
//!   as required state is a hard validation failure ([`SubstitutionKind`]);
//! - a timed-out barrier is lawful typed evidence but forces `not_proven`;
//! - a semantic result computed against a stale generation snapshot cannot
//!   classify `applied` (it must be `stale` or `not_proven`);
//! - a save-format settlement must observe exactly one configured owner and a
//!   save-event trigger; a manual comparator run can never label itself
//!   save-triggered;
//! - a recovery generation-replay observation must bind the
//!   initialize/initialized/buffer-enabled sequence, not a bare new PID;
//! - a repeated-session observation must carry at least the declared minimum
//!   iterations over changed host instances;
//! - an activation row must report native (or, for the declared override row,
//!   override) detection — a pre-forced filetype is rejected;
//! - unknown process/cleanup state forces `not_proven`;
//! - observations are bounded and privacy-safe: stable tokens,
//!   `sha256:`-prefixed digests, fixture-relative paths, no unknown fields;
//! - the observation subject must be the pinned Vim + vim-lsp + perllsp
//!   subject; another client's observation is rejected.

pub mod barrier;
pub mod fake;
pub mod observation;

use anyhow::{Context, Result, ensure};
use serde::Serialize;
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use crate::vim_lsp_cell_catalog::vim_vim_lsp_subject;
use barrier::{BarrierKind, BarrierRequirement};
use observation::{
    ActionResult, BackendIdentity, CleanupLedger, DetectionRoute, ObservedRoute,
    ProcessDisposition, SaveTrigger, TypedObservation,
};

/// Identity of this driver model, for consumers that need to name the
/// vocabulary semantics they validated against.
pub const DRIVER_SCHEMA_VERSION: &str = "vim_lsp_specialized_driver.v1";

/// The action-ID namespace. Three segments below the shared
/// `vim.vim_lsp.` root (`specialized.<family>.<name>`) keep driver action IDs
/// structurally distinct from the two-segment `vim.vim_lsp.<family>.<name>`
/// journey-cell IDs the cell catalog admits, so an action can never be
/// mistaken for a registered cell.
pub const ACTION_ID_PREFIX: &str = "vim.vim_lsp.specialized.";

/// Path of the #11369 classified public-surface inventory, relative to the
/// repository root. Read by [`validate_driver_contract`]; an absent or
/// unreadable inventory fails closed.
pub const PUBLIC_SURFACE_FIXTURE: &str = ".ci/editor-clients/vim-vim-lsp-public-surface.v1.json";

/// The five #11376 first-class families this driver serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionFamily {
    Freshness,
    SaveFormat,
    Recovery,
    HostReopen,
    Activation,
}

impl ActionFamily {
    /// Stable family token used inside action IDs.
    pub fn token(self) -> &'static str {
        match self {
            ActionFamily::Freshness => "freshness",
            ActionFamily::SaveFormat => "save_format",
            ActionFamily::Recovery => "recovery",
            ActionFamily::HostReopen => "host_reopen",
            ActionFamily::Activation => "activation",
        }
    }
}

/// How one action's execution is owned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionClass {
    /// An ordinary editor/client user action through real public surfaces.
    UserAction,
    /// A read-only observation of current client/server state.
    Observation,
    /// A deliberate test stimulus that must never be labeled product behavior.
    TestStimulus,
    /// A process/session operation owned by #10894/#10944; this driver only
    /// emits/consumes the bounded handoff observation and fails closed until
    /// those authorities land.
    HostHandoff,
}

/// Per-action observation shape rules beyond the shared laws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShapeRules {
    /// The observation must bind how the format run was triggered.
    pub requires_save_trigger: bool,
    /// The trigger must be the save event (ordinary write / settlement rows).
    pub save_trigger_must_be_save_event: bool,
    /// The settlement must observe exactly one configured save-format owner.
    pub requires_single_configured_owner: bool,
    /// The observation must carry a bounded semantic probe so an event log
    /// alone can never substitute for the semantic result.
    pub requires_semantic_probe: bool,
    /// The observation must bind the initialize/initialized/buffer-enabled
    /// replay sequence with cardinalities.
    pub requires_generation_replay_sequence: bool,
    /// The observation must name the answering provider/service owner.
    pub requires_provider_owner: bool,
    /// Minimum session iterations a repeated-session observation must carry.
    pub min_session_iterations: Option<u32>,
    /// The detection route an activation row must report.
    pub expected_detection_route: Option<DetectionRoute>,
}

/// The no-extra-shape baseline used by most rows of the vocabulary.
pub const DEFAULT_SHAPE: ShapeRules = ShapeRules {
    requires_save_trigger: false,
    save_trigger_must_be_save_event: false,
    requires_single_configured_owner: false,
    requires_semantic_probe: false,
    requires_generation_replay_sequence: false,
    requires_provider_owner: false,
    min_session_iterations: None,
    expected_detection_route: None,
};

/// One specialized action of the vocabulary. Every field is load-bearing at
/// validation time; the vocabulary digest covers all of them.
#[derive(Debug, Clone, Copy)]
pub struct ActionSpec {
    /// Stable ID in the `vim.vim_lsp.specialized.<family>.<name>` namespace.
    pub action_id: &'static str,
    pub family: ActionFamily,
    pub class: ActionClass,
    pub summary: &'static str,
    /// #11369-classified vim-lsp surfaces the real adapter may call here.
    /// Entries must appear in the public-surface inventory.
    pub public_surfaces: &'static [&'static str],
    /// Native Vim surfaces (commands/options/autocmd events) the action uses.
    /// Vim itself owns these spellings; the contract test enforces a narrow
    /// grammar so this cannot become a free-text channel.
    pub native_vim_surfaces: &'static [&'static str],
    /// Instrument-only hooks (#11369 classification) cited by this action,
    /// each with its justification and retirement condition.
    pub instrument_hooks: &'static [InstrumentHookUse],
    /// Barriers that must be Satisfied before the outcome may classify beyond
    /// `not_proven`.
    pub required_barriers: &'static [BarrierRequirement],
    pub shape: ShapeRules,
    /// Result vocabulary this action may report.
    pub allowed_results: &'static [ActionResult],
}

/// A justified instrument-only hook citation, mirroring the #11380 public API
/// law: instrument hooks are allowed only with an exact subject binding, a
/// read-only scope, a reason the public surface cannot expose the observation,
/// and a retirement condition.
#[derive(Debug, Clone, Copy)]
pub struct InstrumentHookUse {
    pub api: &'static str,
    pub justification: &'static str,
    pub retirement: &'static str,
}

/// A validated observation: the classified result, with no receipt data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ValidatedObservation {
    pub action_id: String,
    pub family: ActionFamily,
    pub outcome: ActionResult,
    pub limitation: Option<String>,
    pub backend: BackendIdentity,
}

const fn barrier(kind: BarrierKind, max_wait_ms: u64) -> BarrierRequirement {
    BarrierRequirement { kind, max_wait_ms }
}

const SETTLED_RESULTS: &[ActionResult] = &[
    ActionResult::Applied,
    ActionResult::NoChange,
    ActionResult::Disabled,
    ActionResult::Refused,
    ActionResult::Failure,
    ActionResult::Stale,
    ActionResult::Cancelled,
    ActionResult::NotProven,
    ActionResult::Unsupported,
];

const RESULTS_WITH_CLIENT_NOT_EXPOSED: &[ActionResult] = &[
    ActionResult::Applied,
    ActionResult::NoChange,
    ActionResult::Disabled,
    ActionResult::Refused,
    ActionResult::Failure,
    ActionResult::Stale,
    ActionResult::Cancelled,
    ActionResult::NotProven,
    ActionResult::Unsupported,
    ActionResult::ClientNotExposed,
];

const INIT_SEQ_BUDGET_MS: u64 = 30_000;
const SETTLE_BUDGET_MS: u64 = 15_000;
/// Process teardown of a workspace-indexing server can legitimately take
/// tens of seconds under load; the bound stays explicit and typed.
const EXIT_BUDGET_MS: u64 = 45_000;

const GENERIC_REQUEST: &str = "lsp#send_request(server_name, request)";
const STATUS: &str = "lsp#get_server_status(...)";
const RUNNING: &str = "lsp#is_server_running(name)";
const STOP: &str = "lsp#stop_server(server_name)";
const BUFFER_LIFECYCLE: &str =
    "native Vim filetype/autocmd behavior plus lsp#enable() buffer tracking";
const APPLY_TEXT_EDITS: &str = "lsp#utils#text_edit#apply_text_edits(uri, text_edits)";
const WIRE_CAPTURE: &str = "g:lsp_log_verbose = 1 + g:lsp_log_file wire capture parsed offline";
const AUTOCMD_INIT: &str = "User autocmd lsp_server_init";
const AUTOCMD_BUFFER: &str = "User autocmd lsp_buffer_enabled";
const UPDATE_CONFIG: &str = "lsp#update_workspace_config(server_name, workspace_config)";
const POSITION: &str = "lsp#get_position()";

/// The published specialized action vocabulary. Rows are frozen once landed;
/// semantic edits bump the vocabulary digest visibly.
pub const ACTIONS: &[ActionSpec] = &[
    // -----------------------------------------------------------------
    // Family A — external source/config freshness operations
    // -----------------------------------------------------------------
    ActionSpec {
        action_id: "vim.vim_lsp.specialized.freshness.source_mutate_closed_in_place",
        family: ActionFamily::Freshness,
        class: ActionClass::UserAction,
        summary: "edit an already-closed source file in place through an ordinary Vim write",
        public_surfaces: &[BUFFER_LIFECYCLE],
        native_vim_surfaces: &[":e", ":w"],
        instrument_hooks: &[],
        required_barriers: &[barrier(BarrierKind::DigestReached, SETTLE_BUDGET_MS)],
        shape: DEFAULT_SHAPE,
        allowed_results: SETTLED_RESULTS,
    },
    ActionSpec {
        action_id: "vim.vim_lsp.specialized.freshness.source_atomic_replace",
        family: ActionFamily::Freshness,
        class: ActionClass::UserAction,
        summary: "atomically replace a source file (write new bytes, rename over the old path)",
        public_surfaces: &[BUFFER_LIFECYCLE],
        native_vim_surfaces: &[":rename", ":w"],
        instrument_hooks: &[],
        required_barriers: &[barrier(BarrierKind::DigestReached, SETTLE_BUDGET_MS)],
        shape: DEFAULT_SHAPE,
        allowed_results: SETTLED_RESULTS,
    },
    ActionSpec {
        action_id: "vim.vim_lsp.specialized.freshness.source_create_delete_rename",
        family: ActionFamily::Freshness,
        class: ActionClass::UserAction,
        summary: "create, delete, or rename a source file where the selected route permits",
        public_surfaces: &[BUFFER_LIFECYCLE],
        native_vim_surfaces: &[":e", ":w", ":delete", ":rename"],
        instrument_hooks: &[],
        required_barriers: &[barrier(BarrierKind::DigestReached, SETTLE_BUDGET_MS)],
        shape: DEFAULT_SHAPE,
        allowed_results: SETTLED_RESULTS,
    },
    ActionSpec {
        action_id: "vim.vim_lsp.specialized.freshness.config_file_lifecycle",
        family: ActionFamily::Freshness,
        class: ActionClass::UserAction,
        summary: "create, modify, replace, or delete a governed .perl-lsp.toml",
        public_surfaces: &[BUFFER_LIFECYCLE],
        native_vim_surfaces: &[":e", ":w", ":delete"],
        instrument_hooks: &[],
        required_barriers: &[barrier(BarrierKind::DigestReached, SETTLE_BUDGET_MS)],
        shape: DEFAULT_SHAPE,
        allowed_results: SETTLED_RESULTS,
    },
    ActionSpec {
        action_id: "vim.vim_lsp.specialized.freshness.config_file_malformed",
        family: ActionFamily::Freshness,
        class: ActionClass::TestStimulus,
        summary: "write a malformed .perl-lsp.toml as a deliberate test stimulus",
        public_surfaces: &[],
        native_vim_surfaces: &[":w"],
        instrument_hooks: &[],
        required_barriers: &[],
        shape: DEFAULT_SHAPE,
        allowed_results: SETTLED_RESULTS,
    },
    ActionSpec {
        action_id: "vim.vim_lsp.specialized.freshness.config_file_repair",
        family: ActionFamily::Freshness,
        class: ActionClass::UserAction,
        summary: "repair a malformed .perl-lsp.toml back to governed bytes",
        public_surfaces: &[BUFFER_LIFECYCLE],
        native_vim_surfaces: &[":e", ":w"],
        instrument_hooks: &[],
        required_barriers: &[barrier(BarrierKind::DigestReached, SETTLE_BUDGET_MS)],
        shape: DEFAULT_SHAPE,
        allowed_results: SETTLED_RESULTS,
    },
    ActionSpec {
        action_id: "vim.vim_lsp.specialized.freshness.workspace_setting_change",
        family: ActionFamily::Freshness,
        class: ActionClass::UserAction,
        summary: "change a governed Vim/vim-lsp workspace setting through the public config API",
        public_surfaces: &[UPDATE_CONFIG],
        native_vim_surfaces: &[],
        instrument_hooks: &[],
        required_barriers: &[barrier(BarrierKind::DocumentGenerationAccepted, SETTLE_BUDGET_MS)],
        shape: DEFAULT_SHAPE,
        allowed_results: SETTLED_RESULTS,
    },
    ActionSpec {
        action_id: "vim.vim_lsp.specialized.freshness.observe_route_and_generation",
        family: ActionFamily::Freshness,
        class: ActionClass::Observation,
        summary: "observe the selected freshness route, current generation, and semantic result",
        public_surfaces: &[STATUS, "lsp#get_buffer_diagnostics_counts()"],
        native_vim_surfaces: &[],
        instrument_hooks: &[],
        required_barriers: &[barrier(BarrierKind::DocumentGenerationAccepted, SETTLE_BUDGET_MS)],
        shape: ShapeRules {
            requires_semantic_probe: true,
            requires_provider_owner: true,
            ..DEFAULT_SHAPE
        },
        allowed_results: SETTLED_RESULTS,
    },
    ActionSpec {
        action_id: "vim.vim_lsp.specialized.freshness.explicit_reload_or_restart",
        family: ActionFamily::Freshness,
        class: ActionClass::UserAction,
        summary: "invoke an explicit reload or restart only where the product contract requires it",
        public_surfaces: &[STOP, UPDATE_CONFIG],
        native_vim_surfaces: &[],
        instrument_hooks: &[],
        required_barriers: &[barrier(BarrierKind::ServerGenerationInitialized, INIT_SEQ_BUDGET_MS)],
        shape: DEFAULT_SHAPE,
        allowed_results: SETTLED_RESULTS,
    },
    ActionSpec {
        action_id: "vim.vim_lsp.specialized.freshness.hold_release_old_generation",
        family: ActionFamily::Freshness,
        class: ActionClass::Observation,
        summary: "hold/release an old source/config/root-generation callback where instrumentable",
        public_surfaces: &[STATUS],
        native_vim_surfaces: &[],
        instrument_hooks: &[InstrumentHookUse {
            api: WIRE_CAPTURE,
            justification: "old-generation callback settlement is not exposed by any public vim-lsp surface; the classified wire capture is parsed read-only offline",
            retirement: "retire when vim-lsp exposes a public config-change notification surface",
        }],
        required_barriers: &[barrier(BarrierKind::ProcessGenerationDisposed, SETTLE_BUDGET_MS)],
        shape: DEFAULT_SHAPE,
        allowed_results: SETTLED_RESULTS,
    },
    // -----------------------------------------------------------------
    // Family B — save-format operations
    // -----------------------------------------------------------------
    ActionSpec {
        action_id: "vim.vim_lsp.specialized.save_format.configure_single_owner",
        family: ActionFamily::SaveFormat,
        class: ActionClass::UserAction,
        summary: "configure exactly one selected save-format owner/route for the session",
        public_surfaces: &[GENERIC_REQUEST, APPLY_TEXT_EDITS],
        native_vim_surfaces: &["autocmd bufwritepre"],
        instrument_hooks: &[],
        required_barriers: &[],
        shape: ShapeRules { requires_single_configured_owner: true, ..DEFAULT_SHAPE },
        allowed_results: SETTLED_RESULTS,
    },
    ActionSpec {
        action_id: "vim.vim_lsp.specialized.save_format.ordinary_write",
        family: ActionFamily::SaveFormat,
        class: ActionClass::UserAction,
        summary: "perform an ordinary Vim write/save action and let the configured owner run",
        public_surfaces: &[APPLY_TEXT_EDITS],
        native_vim_surfaces: &[":w", "autocmd bufwritepre"],
        instrument_hooks: &[],
        required_barriers: &[barrier(BarrierKind::SaveEventAndOwnerSettled, SETTLE_BUDGET_MS)],
        shape: ShapeRules {
            requires_save_trigger: true,
            save_trigger_must_be_save_event: true,
            requires_single_configured_owner: true,
            ..DEFAULT_SHAPE
        },
        allowed_results: SETTLED_RESULTS,
    },
    ActionSpec {
        action_id: "vim.vim_lsp.specialized.save_format.observe_save_settlement",
        family: ActionFamily::SaveFormat,
        class: ActionClass::Observation,
        summary: "observe save event, request cardinality, owner identity, and pre/post digests",
        public_surfaces: &[STATUS],
        native_vim_surfaces: &["autocmd bufwritepre", "autocmd bufwritepost"],
        instrument_hooks: &[InstrumentHookUse {
            api: WIRE_CAPTURE,
            justification: "request/invocation cardinality is not exposed by any public vim-lsp surface; the classified wire capture is parsed read-only offline",
            retirement: "retire when vim-lsp exposes a public request-count/telemetry surface",
        }],
        required_barriers: &[
            barrier(BarrierKind::SaveEventAndOwnerSettled, SETTLE_BUDGET_MS),
            barrier(BarrierKind::DigestReached, SETTLE_BUDGET_MS),
        ],
        shape: ShapeRules {
            requires_save_trigger: true,
            save_trigger_must_be_save_event: true,
            requires_single_configured_owner: true,
            ..DEFAULT_SHAPE
        },
        allowed_results: SETTLED_RESULTS,
    },
    ActionSpec {
        action_id: "vim.vim_lsp.specialized.save_format.manual_comparator",
        family: ActionFamily::SaveFormat,
        class: ActionClass::UserAction,
        summary: "run an explicit manual format as comparator only, never labeled save-triggered",
        public_surfaces: &[GENERIC_REQUEST, APPLY_TEXT_EDITS, POSITION],
        native_vim_surfaces: &[],
        instrument_hooks: &[],
        required_barriers: &[barrier(BarrierKind::PendingActionSettled, SETTLE_BUDGET_MS)],
        shape: ShapeRules { requires_save_trigger: true, ..DEFAULT_SHAPE },
        allowed_results: SETTLED_RESULTS,
    },
    ActionSpec {
        action_id: "vim.vim_lsp.specialized.save_format.hold_release_stale_result",
        family: ActionFamily::SaveFormat,
        class: ActionClass::Observation,
        summary: "hold/release one stale format result where the client exposes it",
        public_surfaces: &[STATUS],
        native_vim_surfaces: &[],
        instrument_hooks: &[InstrumentHookUse {
            api: WIRE_CAPTURE,
            justification: "a held stale format response is not exposed by any public vim-lsp surface; the classified wire capture is parsed read-only offline",
            retirement: "retire when vim-lsp exposes a public in-flight request surface",
        }],
        required_barriers: &[barrier(BarrierKind::PendingActionSettled, SETTLE_BUDGET_MS)],
        shape: DEFAULT_SHAPE,
        allowed_results: RESULTS_WITH_CLIENT_NOT_EXPOSED,
    },
    // -----------------------------------------------------------------
    // Family C — server-generation recovery operations
    // -----------------------------------------------------------------
    ActionSpec {
        action_id: "vim.vim_lsp.specialized.recovery.stop_server_public_route",
        family: ActionFamily::Recovery,
        class: ActionClass::UserAction,
        summary: "request an ordinary vim-lsp server stop through the current public route",
        public_surfaces: &[STOP],
        native_vim_surfaces: &[],
        instrument_hooks: &[],
        required_barriers: &[barrier(BarrierKind::ProcessExitedCleanupSettled, EXIT_BUDGET_MS)],
        shape: DEFAULT_SHAPE,
        allowed_results: SETTLED_RESULTS,
    },
    ActionSpec {
        action_id: "vim.vim_lsp.specialized.recovery.restart_server_public_route",
        family: ActionFamily::Recovery,
        class: ActionClass::UserAction,
        summary: "request an ordinary vim-lsp server restart through the current public route",
        public_surfaces: &[STOP, BUFFER_LIFECYCLE],
        native_vim_surfaces: &[],
        instrument_hooks: &[],
        required_barriers: &[
            barrier(BarrierKind::ServerGenerationInitialized, INIT_SEQ_BUDGET_MS),
            barrier(BarrierKind::BufferEnabled, INIT_SEQ_BUDGET_MS),
        ],
        shape: DEFAULT_SHAPE,
        allowed_results: SETTLED_RESULTS,
    },
    ActionSpec {
        action_id: "vim.vim_lsp.specialized.recovery.terminate_server_process",
        family: ActionFamily::Recovery,
        class: ActionClass::TestStimulus,
        summary: "unexpectedly terminate the exact perllsp process as a bounded test stimulus",
        public_surfaces: &[],
        native_vim_surfaces: &[],
        instrument_hooks: &[],
        required_barriers: &[barrier(BarrierKind::ProcessGenerationDisposed, SETTLE_BUDGET_MS)],
        shape: DEFAULT_SHAPE,
        allowed_results: &[
            ActionResult::NotProven,
            ActionResult::Unsupported,
            ActionResult::Failure,
        ],
    },
    ActionSpec {
        action_id: "vim.vim_lsp.specialized.recovery.observe_generation_replay",
        family: ActionFamily::Recovery,
        class: ActionClass::Observation,
        summary: "observe old/new process generations, init sequence, and replay cardinality",
        public_surfaces: &[AUTOCMD_INIT, AUTOCMD_BUFFER, STATUS, RUNNING],
        native_vim_surfaces: &[],
        instrument_hooks: &[],
        required_barriers: &[
            barrier(BarrierKind::ServerGenerationInitialized, INIT_SEQ_BUDGET_MS),
            barrier(BarrierKind::BufferEnabled, INIT_SEQ_BUDGET_MS),
            barrier(BarrierKind::ProcessGenerationDisposed, SETTLE_BUDGET_MS),
        ],
        shape: ShapeRules { requires_generation_replay_sequence: true, ..DEFAULT_SHAPE },
        allowed_results: SETTLED_RESULTS,
    },
    ActionSpec {
        action_id: "vim.vim_lsp.specialized.recovery.hold_release_old_generation_result",
        family: ActionFamily::Recovery,
        class: ActionClass::Observation,
        summary: "hold/release one old-generation result or effect",
        public_surfaces: &[STATUS],
        native_vim_surfaces: &[],
        instrument_hooks: &[InstrumentHookUse {
            api: WIRE_CAPTURE,
            justification: "an in-flight old-generation response is not exposed by any public vim-lsp surface; the classified wire capture is parsed read-only offline",
            retirement: "retire when vim-lsp exposes a public in-flight request surface",
        }],
        required_barriers: &[barrier(BarrierKind::PendingActionSettled, SETTLE_BUDGET_MS)],
        shape: DEFAULT_SHAPE,
        allowed_results: RESULTS_WITH_CLIENT_NOT_EXPOSED,
    },
    ActionSpec {
        action_id: "vim.vim_lsp.specialized.recovery.bounded_retry_disposition",
        family: ActionFamily::Recovery,
        class: ActionClass::Observation,
        summary: "observe the bounded retry/manual-recovery/unsupported disposition",
        public_surfaces: &[STATUS, RUNNING],
        native_vim_surfaces: &[],
        instrument_hooks: &[],
        required_barriers: &[barrier(BarrierKind::PendingActionSettled, SETTLE_BUDGET_MS)],
        shape: DEFAULT_SHAPE,
        allowed_results: SETTLED_RESULTS,
    },
    ActionSpec {
        action_id: "vim.vim_lsp.specialized.recovery.host_shutdown_while_pending",
        family: ActionFamily::Recovery,
        class: ActionClass::UserAction,
        summary: "trigger host shutdown while recovery is pending and observe cleanup",
        public_surfaces: &[],
        native_vim_surfaces: &[":qa!"],
        instrument_hooks: &[],
        required_barriers: &[barrier(BarrierKind::ProcessExitedCleanupSettled, EXIT_BUDGET_MS)],
        shape: DEFAULT_SHAPE,
        allowed_results: SETTLED_RESULTS,
    },
    // -----------------------------------------------------------------
    // Family D — host reopen / repeated-session operations
    // -----------------------------------------------------------------
    ActionSpec {
        action_id: "vim.vim_lsp.specialized.host_reopen.buffer_close_wipe_reopen",
        family: ActionFamily::HostReopen,
        class: ActionClass::UserAction,
        summary: "close/wipe the exact buffer and reopen it in the same host",
        public_surfaces: &[BUFFER_LIFECYCLE, AUTOCMD_BUFFER],
        native_vim_surfaces: &[":bwipeout", ":e"],
        instrument_hooks: &[],
        required_barriers: &[
            barrier(BarrierKind::ServiceAttached, INIT_SEQ_BUDGET_MS),
            barrier(BarrierKind::BufferEnabled, INIT_SEQ_BUDGET_MS),
        ],
        shape: DEFAULT_SHAPE,
        allowed_results: SETTLED_RESULTS,
    },
    ActionSpec {
        action_id: "vim.vim_lsp.specialized.host_reopen.exit_host",
        family: ActionFamily::HostReopen,
        class: ActionClass::UserAction,
        summary: "exit the exact Vim host normally",
        public_surfaces: &[],
        native_vim_surfaces: &[":qa!"],
        instrument_hooks: &[],
        required_barriers: &[barrier(BarrierKind::ProcessExitedCleanupSettled, EXIT_BUDGET_MS)],
        shape: DEFAULT_SHAPE,
        allowed_results: SETTLED_RESULTS,
    },
    ActionSpec {
        action_id: "vim.vim_lsp.specialized.host_reopen.launch_replacement_host",
        family: ActionFamily::HostReopen,
        class: ActionClass::HostHandoff,
        summary: "launch a replacement exact host through the #10944/#10894 handoff",
        public_surfaces: &[],
        native_vim_surfaces: &[],
        instrument_hooks: &[],
        required_barriers: &[
            barrier(BarrierKind::HostInstanceChanged, INIT_SEQ_BUDGET_MS),
            barrier(BarrierKind::ServerGenerationInitialized, INIT_SEQ_BUDGET_MS),
        ],
        shape: DEFAULT_SHAPE,
        // Fail-closed until the #10894/#10944 host runner lands: no backend
        // that exists today can honestly produce an applied replacement-host
        // observation, so `applied` is not admitted vocabulary. Admitting it
        // is a reviewed vocabulary edit that lands with the runner.
        allowed_results: &[ActionResult::NotProven, ActionResult::Unsupported],
    },
    ActionSpec {
        action_id: "vim.vim_lsp.specialized.host_reopen.replace_workspace_session",
        family: ActionFamily::HostReopen,
        class: ActionClass::UserAction,
        summary: "replace the workspace/session where the client exposes a public concept",
        public_surfaces: &[UPDATE_CONFIG],
        native_vim_surfaces: &[],
        instrument_hooks: &[],
        required_barriers: &[],
        shape: DEFAULT_SHAPE,
        allowed_results: RESULTS_WITH_CLIENT_NOT_EXPOSED,
    },
    ActionSpec {
        action_id: "vim.vim_lsp.specialized.host_reopen.pending_action_start_invalidate",
        family: ActionFamily::HostReopen,
        class: ActionClass::UserAction,
        summary: "start a pending completion/navigation/diagnostic/edit action and invalidate it",
        public_surfaces: &[GENERIC_REQUEST, BUFFER_LIFECYCLE],
        native_vim_surfaces: &[":bwipeout"],
        instrument_hooks: &[],
        required_barriers: &[barrier(BarrierKind::PendingActionSettled, SETTLE_BUDGET_MS)],
        shape: DEFAULT_SHAPE,
        allowed_results: SETTLED_RESULTS,
    },
    ActionSpec {
        action_id: "vim.vim_lsp.specialized.host_reopen.repeated_session_sequence",
        family: ActionFamily::HostReopen,
        class: ActionClass::UserAction,
        summary: "run a finite repeated-session sequence over replacement host instances",
        public_surfaces: &[AUTOCMD_INIT, STATUS],
        native_vim_surfaces: &["autocmd vimenter"],
        instrument_hooks: &[],
        required_barriers: &[
            barrier(BarrierKind::HostInstanceChanged, INIT_SEQ_BUDGET_MS),
            barrier(BarrierKind::ProcessExitedCleanupSettled, EXIT_BUDGET_MS),
        ],
        shape: ShapeRules { min_session_iterations: Some(2), ..DEFAULT_SHAPE },
        allowed_results: SETTLED_RESULTS,
    },
    ActionSpec {
        action_id: "vim.vim_lsp.specialized.host_reopen.forced_failure_path",
        family: ActionFamily::HostReopen,
        class: ActionClass::TestStimulus,
        summary: "trigger one forced assertion/timeout/failure path and observe terminal cleanup",
        public_surfaces: &[],
        native_vim_surfaces: &[":cquit"],
        instrument_hooks: &[],
        required_barriers: &[barrier(BarrierKind::ProcessExitedCleanupSettled, EXIT_BUDGET_MS)],
        shape: DEFAULT_SHAPE,
        allowed_results: SETTLED_RESULTS,
    },
    // -----------------------------------------------------------------
    // Family E — expanded activation operations
    // -----------------------------------------------------------------
    ActionSpec {
        action_id: "vim.vim_lsp.specialized.activation.open_without_preset_filetype",
        family: ActionFamily::Activation,
        class: ActionClass::UserAction,
        summary: "open the exact fixture without pre-setting filetype",
        public_surfaces: &[BUFFER_LIFECYCLE],
        native_vim_surfaces: &[":e"],
        instrument_hooks: &[],
        required_barriers: &[barrier(BarrierKind::NativeFiletypeDetected, SETTLE_BUDGET_MS)],
        shape: ShapeRules {
            expected_detection_route: Some(DetectionRoute::Native),
            ..DEFAULT_SHAPE
        },
        allowed_results: SETTLED_RESULTS,
    },
    ActionSpec {
        action_id: "vim.vim_lsp.specialized.activation.observe_native_filetype",
        family: ActionFamily::Activation,
        class: ActionClass::Observation,
        summary: "observe native &filetype and the detection result after open",
        public_surfaces: &[STATUS],
        native_vim_surfaces: &["&filetype"],
        instrument_hooks: &[],
        required_barriers: &[barrier(BarrierKind::NativeFiletypeDetected, SETTLE_BUDGET_MS)],
        shape: ShapeRules {
            expected_detection_route: Some(DetectionRoute::Native),
            ..DEFAULT_SHAPE
        },
        allowed_results: SETTLED_RESULTS,
    },
    ActionSpec {
        action_id: "vim.vim_lsp.specialized.activation.declared_override_row",
        family: ActionFamily::Activation,
        class: ActionClass::TestStimulus,
        summary: "apply the narrowly declared test/user-equivalent override for the override row only",
        public_surfaces: &[BUFFER_LIFECYCLE],
        native_vim_surfaces: &[":setf"],
        instrument_hooks: &[],
        required_barriers: &[barrier(BarrierKind::ServiceAttached, INIT_SEQ_BUDGET_MS)],
        shape: ShapeRules {
            expected_detection_route: Some(DetectionRoute::DeclaredOverride),
            ..DEFAULT_SHAPE
        },
        allowed_results: SETTLED_RESULTS,
    },
    ActionSpec {
        action_id: "vim.vim_lsp.specialized.activation.observe_service_attachment",
        family: ActionFamily::Activation,
        class: ActionClass::Observation,
        summary: "observe vim-lsp service attachment: languageId, root, process, server status",
        public_surfaces: &[STATUS, RUNNING, AUTOCMD_INIT],
        native_vim_surfaces: &[],
        instrument_hooks: &[],
        required_barriers: &[
            barrier(BarrierKind::ServiceAttached, INIT_SEQ_BUDGET_MS),
            barrier(BarrierKind::ServerGenerationInitialized, INIT_SEQ_BUDGET_MS),
        ],
        shape: ShapeRules { requires_provider_owner: true, ..DEFAULT_SHAPE },
        allowed_results: SETTLED_RESULTS,
    },
    ActionSpec {
        action_id: "vim.vim_lsp.specialized.activation.root_semantic_discriminator",
        family: ActionFamily::Activation,
        class: ActionClass::Observation,
        summary: "invoke a small root/semantic discriminator only where required",
        public_surfaces: &[GENERIC_REQUEST, POSITION],
        native_vim_surfaces: &[],
        instrument_hooks: &[],
        required_barriers: &[barrier(BarrierKind::PendingActionSettled, SETTLE_BUDGET_MS)],
        shape: ShapeRules {
            requires_semantic_probe: true,
            requires_provider_owner: true,
            ..DEFAULT_SHAPE
        },
        allowed_results: SETTLED_RESULTS,
    },
    ActionSpec {
        action_id: "vim.vim_lsp.specialized.activation.close_reset_between_rows",
        family: ActionFamily::Activation,
        class: ActionClass::UserAction,
        summary: "close/reset state between activation rows so rows cannot contaminate each other",
        public_surfaces: &[BUFFER_LIFECYCLE],
        native_vim_surfaces: &[":bwipeout"],
        instrument_hooks: &[],
        required_barriers: &[],
        shape: DEFAULT_SHAPE,
        allowed_results: SETTLED_RESULTS,
    },
];

/// Look up one action by ID.
pub fn action_by_id(action_id: &str) -> Option<&'static ActionSpec> {
    ACTIONS.iter().find(|action| action.action_id == action_id)
}

/// Validate one observation against its action's laws. Returns the classified
/// result or a precise violation; never mutates receipt or catalog state.
pub fn validate_observation(
    observation: &TypedObservation,
) -> Result<ValidatedObservation, String> {
    if observation.schema_version != DRIVER_SCHEMA_VERSION {
        return Err(format!(
            "schema version {} does not match {DRIVER_SCHEMA_VERSION}",
            observation.schema_version
        ));
    }
    let action = action_by_id(&observation.action_id)
        .ok_or_else(|| format!("unknown action id: {}", observation.action_id))?;

    observation::validate_bounded(observation)?;

    // Subject pin: only the exact Vim + vim-lsp + perllsp subject may observe.
    let pinned = vim_vim_lsp_subject();
    if observation.host_product != pinned.host_product
        || observation.client_id != pinned.client_id
        || observation.server_executable != pinned.server_executable
    {
        return Err(format!(
            "observation subject {}/{} is not the pinned vim/vim-lsp/perllsp subject",
            observation.host_product, observation.client_id
        ));
    }

    // Route-class law: the executed route must match how the action is owned,
    // and a raw protocol request is never lawful.
    if let ObservedRoute::RawProtocolRequest { method } = &observation.route {
        return Err(format!(
            "raw protocol request {method} offered as the action route; a raw substitute is never a user action"
        ));
    }
    let native_or_public = matches!(
        observation.route,
        ObservedRoute::NativeVimSurface { .. } | ObservedRoute::PublicClientApi { .. }
    );
    let route_lawful = (native_or_public
        && matches!(action.class, ActionClass::UserAction | ActionClass::Observation))
        || (matches!(observation.route, ObservedRoute::TestStimulus { .. })
            && action.class == ActionClass::TestStimulus)
        || (matches!(observation.route, ObservedRoute::HostHandoff { .. })
            && matches!(action.class, ActionClass::TestStimulus | ActionClass::HostHandoff));
    if !route_lawful {
        return Err(format!(
            "route {:?} does not match action class {:?}",
            observation.route, action.class
        ));
    }
    if let ObservedRoute::PublicClientApi { api } = &observation.route
        && !action.public_surfaces.contains(&api.as_str())
    {
        return Err(format!("action {} does not declare public surface {api}", action.action_id));
    }
    if let ObservedRoute::NativeVimSurface { surface } = &observation.route
        && !action.native_vim_surfaces.contains(&surface.as_str())
    {
        return Err(format!(
            "action {} does not declare native surface {surface}",
            action.action_id
        ));
    }

    // Barrier law: every required barrier must be Satisfied within its
    // budget over generations no newer than the observation's own snapshot;
    // a TimedOut barrier forces not_proven; a Substituted barrier is
    // dishonest.
    for requirement in action.required_barriers {
        let Some(evidence) = observation.barrier(requirement.kind) else {
            return Err(format!(
                "action {} requires barrier {:?} but the observation carries no evidence for it",
                action.action_id, requirement.kind
            ));
        };
        match evidence {
            barrier::BarrierEvidence::Satisfied { settled_generations, waited_ms, .. } => {
                if *waited_ms > requirement.max_wait_ms {
                    return Err(format!(
                        "barrier {:?} claims satisfaction after {waited_ms}ms, beyond the {}ms budget",
                        requirement.kind, requirement.max_wait_ms
                    ));
                }
                for dimension in barrier::GENERATION_DIMENSIONS {
                    let settled = settled_generations.dimension(*dimension);
                    let observed = observation.generations.dimension(*dimension);
                    if settled > observed {
                        return Err(format!(
                            "barrier {:?} settled at a newer {dimension:?} generation ({settled}) than the observation snapshot ({observed}); settlement cannot be newer than the observation",
                            requirement.kind
                        ));
                    }
                }
            }
            barrier::BarrierEvidence::TimedOut { waited_ms, .. } => {
                if observation.outcome != ActionResult::NotProven {
                    return Err(format!(
                        "barrier {:?} timed out after {waited_ms}ms but outcome is {:?}; a timeout must classify not_proven",
                        requirement.kind, observation.outcome
                    ));
                }
            }
            barrier::BarrierEvidence::Substituted { substitution, .. } => {
                return Err(format!(
                    "barrier {:?} evidence is a {substitution:?} substitution; substitutions are never state",
                    requirement.kind
                ));
            }
        }
    }

    // Result vocabulary and limitation law.
    if !action.allowed_results.contains(&observation.outcome) {
        return Err(format!(
            "outcome {:?} is outside the admitted vocabulary of {}",
            observation.outcome, action.action_id
        ));
    }
    if observation.outcome.requires_limitation() && observation.limitation.is_none() {
        return Err(format!(
            "outcome {:?} requires an admitted limitation token",
            observation.outcome
        ));
    }

    // Process/cleanup law: unknown state can never pass; pending cleanup can
    // never report applied.
    if (matches!(observation.process, ProcessDisposition::Unknown)
        || matches!(observation.cleanup, CleanupLedger::Unknown))
        && observation.outcome != ActionResult::NotProven
    {
        return Err(format!(
            "unknown process/cleanup state must classify not_proven, not {:?}",
            observation.outcome
        ));
    }
    if matches!(observation.cleanup, CleanupLedger::Pending)
        && observation.outcome == ActionResult::Applied
    {
        return Err("pending cleanup cannot classify applied".to_string());
    }

    // Stale-generation law: a semantic answer from an old generation cannot
    // classify applied.
    if let Some(probe) = &observation.semantic_probe
        && probe.generation_scope != observation.generations
        && matches!(
            observation.outcome,
            ActionResult::Applied | ActionResult::NoChange | ActionResult::Cancelled
        )
    {
        return Err(format!(
            "semantic probe answered against generation scope {:?} while the observation settled at {:?}; an old-generation result must classify stale or not_proven",
            probe.generation_scope, observation.generations
        ));
    }

    // Family shape laws.
    let shape = action.shape;
    if shape.requires_save_trigger {
        let Some(trigger) = observation.trigger else {
            return Err(format!("action {} must bind its save trigger", action.action_id));
        };
        if shape.save_trigger_must_be_save_event && trigger != SaveTrigger::SaveEvent {
            return Err(format!(
                "action {} requires a save-event trigger; a {trigger:?} run cannot label itself save-triggered",
                action.action_id
            ));
        }
    }
    if shape.requires_single_configured_owner {
        match observation.configured_owner_count {
            Some(1) => {}
            Some(count) => {
                return Err(format!(
                    "duplicate save-format owners are not observable: configured owner count is {count}"
                ));
            }
            None => {
                return Err(format!(
                    "action {} must observe the configured save-format owner count",
                    action.action_id
                ));
            }
        }
        if observation.owner.is_none() {
            return Err(format!(
                "action {} must bind the selected owner identity",
                action.action_id
            ));
        }
    }
    if shape.requires_semantic_probe && observation.semantic_probe.is_none() {
        return Err(format!(
            "action {} requires a bounded semantic probe; event/log artifacts alone are not a semantic result",
            action.action_id
        ));
    }
    if shape.requires_provider_owner
        && !matches!(&observation.owner, Some(owner) if owner.owner_class == "service_provider")
    {
        return Err(format!(
            "action {} must name the answering service provider identity",
            action.action_id
        ));
    }
    if shape.requires_generation_replay_sequence {
        // The initialize sequence must appear in protocol-event ORDER: the
        // server generation initializes before the buffer is enabled. The
        // events are the ones the pinned client exposes publicly (its
        // `lsp_server_init`/`lsp_buffer_enabled` User autocmds); a bare new
        // PID carries neither, so the negative control stays discriminating
        // without requiring an instrument-only wire capture.
        let position = |class: &str| {
            observation.protocol_events.iter().position(|event| event.event_class == class)
        };
        let (Some(init_at), Some(enabled_at)) =
            (position("lsp_server_init"), position("lsp_buffer_enabled"))
        else {
            return Err(
                "generation replay must bind the lsp_server_init and lsp_buffer_enabled protocol events in order; a bare new PID is not an initialized/replayed generation"
                    .to_string(),
            );
        };
        if enabled_at < init_at {
            return Err(
                "generation replay events are out of order: lsp_buffer_enabled precedes lsp_server_init"
                    .to_string(),
            );
        }
        let replay_cardinality =
            observation.cardinalities.get("replayed_buffers").copied().unwrap_or(0);
        if replay_cardinality == 0 {
            return Err("generation replay must bind the replayed-buffer cardinality".to_string());
        }
    }
    if let Some(min_iterations) = shape.min_session_iterations {
        let iterations = observation.session_iterations.unwrap_or(0);
        if iterations < min_iterations {
            return Err(format!(
                "repeated-session observation carries {iterations} iterations; at least {min_iterations} distinct host sessions are required"
            ));
        }
        let host_changed = matches!(
            observation.barrier(BarrierKind::HostInstanceChanged),
            Some(barrier::BarrierEvidence::Satisfied { .. })
        );
        if !host_changed {
            return Err(
                "a repeated-session sequence must satisfy the host-instance-changed barrier; one iteration cannot substitute for repeated sessions"
                    .to_string(),
            );
        }
    }
    if let Some(expected) = shape.expected_detection_route {
        match observation.detection_route {
            Some(route) if route == expected => {}
            Some(DetectionRoute::PreForced) => {
                return Err(
                    "native filetype was pre-forced; pre-forcing is never activation detection"
                        .to_string(),
                );
            }
            Some(other) => {
                return Err(format!(
                    "activation row requires detection route {expected:?} but observed {other:?}"
                ));
            }
            None => {
                return Err(format!("action {} must bind its detection route", action.action_id));
            }
        }
    }

    Ok(ValidatedObservation {
        action_id: action.action_id.to_string(),
        family: action.family,
        outcome: observation.outcome,
        limitation: observation.limitation.clone(),
        backend: observation.backend.clone(),
    })
}

/// Grammar for native Vim surface spellings: an Ex command (`:w`, `:qa!`), an
/// option (`&filetype`), or a lowercase autocmd event row
/// (`autocmd bufwritepre`). Nothing else may ride in this channel.
pub fn is_native_vim_surface(spelling: &str) -> bool {
    if let Some(command) = spelling.strip_prefix(':') {
        return !command.is_empty()
            && command.len() <= 12
            && command.bytes().all(|byte| byte.is_ascii_lowercase() || byte == b'!');
    }
    if let Some(option) = spelling.strip_prefix('&') {
        return !option.is_empty() && option.bytes().all(|byte| byte.is_ascii_lowercase());
    }
    if let Some(event) = spelling.strip_prefix("autocmd ") {
        return !event.is_empty() && event.bytes().all(|byte| byte.is_ascii_lowercase());
    }
    false
}

/// Stable digest of the published action vocabulary, so any semantic edit is
/// a visible identity change for downstream consumers.
pub fn driver_vocabulary_digest() -> Result<String> {
    table_digest(ACTIONS)
}

/// Write one action's full binding into the canonical digest buffer.
fn write_action_binding(canonical: &mut String, action: &ActionSpec) -> Result<()> {
    let _ = writeln!(canonical, "{}", action.action_id);
    let _ = writeln!(canonical, "class:{:?}", action.class);
    let _ = writeln!(canonical, "family:{}", action.family.token());
    let _ = writeln!(canonical, "summary:{}", action.summary);
    for surface in action.public_surfaces {
        let _ = writeln!(canonical, "surface:{surface}");
    }
    for native in action.native_vim_surfaces {
        let _ = writeln!(canonical, "native:{native}");
    }
    for hook in action.instrument_hooks {
        let _ = writeln!(canonical, "hook:{}|{}|{}", hook.api, hook.justification, hook.retirement);
    }
    for requirement in action.required_barriers {
        let _ = writeln!(canonical, "barrier:{:?}:{}", requirement.kind, requirement.max_wait_ms);
    }
    let shape = &action.shape;
    let _ = writeln!(
        canonical,
        "shape:{},{},{},{},{},{},{},{:?}",
        shape.requires_save_trigger,
        shape.save_trigger_must_be_save_event,
        shape.requires_single_configured_owner,
        shape.requires_semantic_probe,
        shape.requires_generation_replay_sequence,
        shape.requires_provider_owner,
        shape.min_session_iterations.unwrap_or(0),
        shape.expected_detection_route,
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

/// Summary of a successful driver-contract validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverContractSummary {
    pub action_count: usize,
    pub family_counts: Vec<(ActionFamily, usize)>,
    pub vocabulary_digest: String,
}

/// Validate the compiled vocabulary against the landed #11369 inventory on
/// disk. Fails closed when the fixture is absent, unreadable, or when any
/// action cites a surface the inventory does not classify.
pub fn validate_driver_contract(repository_root: &Path) -> Result<DriverContractSummary> {
    validate_table(ACTIONS, repository_root)
}

/// Validate an arbitrary table of actions (the compiled vocabulary, or a
/// mutated copy under test) against the landed #11369 inventory.
pub fn validate_table(
    actions: &[ActionSpec],
    repository_root: &Path,
) -> Result<DriverContractSummary> {
    let fixture_path = repository_root.join(PUBLIC_SURFACE_FIXTURE);
    let fixture_bytes = std::fs::read(&fixture_path)
        .with_context(|| format!("reading public-surface inventory {}", fixture_path.display()))?;
    let fixture: serde_json::Value = serde_json::from_slice(&fixture_bytes)
        .with_context(|| format!("parsing public-surface inventory {}", fixture_path.display()))?;

    let mut inventory: BTreeSet<String> = BTreeSet::new();
    let mut instrument_only: BTreeSet<String> = BTreeSet::new();
    if let Some(surfaces) = fixture.get("surfaces").and_then(|value| value.as_array()) {
        for surface in surfaces {
            let classification =
                surface.get("classification").and_then(|value| value.as_str()).unwrap_or_default();
            if let Some(apis) = surface.get("api").and_then(|value| value.as_array()) {
                for api in apis {
                    if let Some(api) = api.as_str() {
                        inventory.insert(api.to_string());
                        if classification == "instrument_only_hook_requiring_justification" {
                            instrument_only.insert(api.to_string());
                        }
                    }
                }
            }
        }
    }
    ensure!(!inventory.is_empty(), "public-surface inventory carried no classified APIs");

    let mut seen = BTreeSet::new();
    for action in actions {
        ensure!(seen.insert(action.action_id), "duplicate action id {}", action.action_id);
        let suffix = action.action_id.strip_prefix(ACTION_ID_PREFIX).with_context(|| {
            format!("action id {} outside the {ACTION_ID_PREFIX} namespace", action.action_id)
        })?;
        let segments: Vec<&str> = suffix.split('.').collect();
        ensure!(
            segments.len() == 2 && segments[0] == action.family.token(),
            "action id {} does not spell specialized.<family>.<name> for family {}",
            action.action_id,
            action.family.token()
        );
        for surface in action.public_surfaces {
            ensure!(
                inventory.contains(*surface),
                "action {} cites public surface {surface} absent from the #11369 inventory",
                action.action_id
            );
        }
        for native in action.native_vim_surfaces {
            ensure!(
                is_native_vim_surface(native),
                "action {} cites native surface {native} outside the native Vim grammar",
                action.action_id
            );
        }
        for hook in action.instrument_hooks {
            ensure!(
                instrument_only.contains(hook.api),
                "action {} cites {api} as instrument-only but the inventory classifies it differently",
                action.action_id,
                api = hook.api
            );
            ensure!(
                !hook.justification.trim().is_empty() && !hook.retirement.trim().is_empty(),
                "action {} cites an instrument hook without justification and retirement condition",
                action.action_id
            );
        }
        for requirement in action.required_barriers {
            ensure!(
                requirement.max_wait_ms > 0,
                "action {} declares a zero wait budget for barrier {:?}",
                action.action_id,
                requirement.kind
            );
        }
        ensure!(
            !action.allowed_results.is_empty(),
            "action {} admits no result vocabulary",
            action.action_id
        );
    }

    let mut family_counts: BTreeMap<ActionFamily, usize> = BTreeMap::new();
    for action in actions {
        *family_counts.entry(action.family).or_default() += 1;
    }
    Ok(DriverContractSummary {
        action_count: actions.len(),
        family_counts: family_counts.into_iter().collect(),
        vocabulary_digest: table_digest(actions)?,
    })
}

/// Digest one action-table (the compiled vocabulary or a mutated copy).
fn table_digest(actions: &[ActionSpec]) -> Result<String> {
    let mut canonical = String::new();
    for action in actions {
        write_action_binding(&mut canonical, action)?;
    }
    digest_canonical(&canonical)
}

/// Validate an observations file (one JSON observation per line) against the
/// vocabulary. Returns the number of validated observations; any invalid line
/// is an error naming the line and the violated law.
pub fn validate_observation_file(path: &Path) -> Result<usize> {
    let bytes =
        std::fs::read(path).with_context(|| format!("reading observations {}", path.display()))?;
    let text = String::from_utf8(bytes).context("observations file must be UTF-8")?;
    let mut validated = 0usize;
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let observation: TypedObservation = serde_json::from_str(trimmed)
            .with_context(|| format!("parsing observation at line {}", index + 1))?;
        validate_observation(&observation).map_err(|error| {
            anyhow::anyhow!(
                "observation at line {} ({}) failed validation: {error}",
                index + 1,
                observation.action_id
            )
        })?;
        validated += 1;
    }
    ensure!(validated > 0, "observations file carried no observations");
    Ok(validated)
}
