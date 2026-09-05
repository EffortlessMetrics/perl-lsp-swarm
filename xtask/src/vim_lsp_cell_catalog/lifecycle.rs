//! The #11387 host-reopen and repeated-session family cell catalog.
//!
//! Every cell is keyed by the finite #11387 lifecycle-chain denominator: the 8
//! stage rows of `.ci/editor-clients/vim-vim-lsp-lifecycle-root.v1.json`,
//! mirrored in [`LIFECYCLE_DENOMINATOR`] and checked against that artifact by
//! tests so the mirror cannot drift from the landed authority. Each row
//! pre-registers exactly the #11387 convention cell of the same name — one
//! independently visible host-reopen/repeated-session proposition per chain
//! stage:
//!
//! | Cell (== denominator row) | Classifying action | Proposition |
//! | --- | --- | --- |
//! | `buffer_reopen` | `buffer_close_wipe_reopen` | the same-host close/wipe+reopen of the exact buffer — never a full host replacement or a server restart |
//! | `host_reopen` | `launch_replacement_host` | full Vim exit plus replacement-host launch binds a changed host instance — a server restart, a buffer reopen, or a bare new PID is not host reopen |
//! | `workspace_or_session_reopen` | `replace_workspace_session` | workspace/session replacement only where the client exposes the concept — never relabeled from another surface |
//! | `cancellation` | `pending_action_start_invalidate` | a started pending action is cancelled by identity — the held action never applies after invalidation |
//! | `late_result_rejected` | `pending_action_start_invalidate` | a late old-host/document/process result is rejected, not admitted |
//! | `repeated_sessions` | `repeated_session_sequence` | the finite repeated-session denominator binds a bounded iteration count over changed host instances with a fresh per-iteration result — one passing run is not repeated use |
//! | `normal_cleanup` | `exit_host` | normal-exit terminal cleanup settles observably — a client exit event or a force-kill alone is not clean cleanup |
//! | `failure_cleanup` | `forced_failure_path` | forced-failure/timeout terminal cleanup settles observably |
//!
//! The landed cell-ID grammar admits exactly `vim.vim_lsp.<family>.<name>`
//! (two stable reason-token segments) and the #11387 spec's registered names
//! are already in that final convention form, so — like the #11386 recovery
//! family — every stage registers under its exact spec name: row == cell, and
//! the denominator laws below bind them. The `lifecycle` family segment is
//! shared with the two #11371 baseline cells (`lifecycle.close_reopen`,
//! `lifecycle.baseline_cleanup`); cell IDs stay globally unique and those
//! baseline rows stay outside this denominator, so a baseline cleanup
//! observation can never register as a lifecycle stage.
//!
//! Ownership split — consumed, never duplicated:
//!
//! - [`super`] owns the registration model and cross-catalog laws; this module
//!   owns this family's ledger, denominator mirror, fixture substrate,
//!   vocabulary, cells, and the family laws [`validate_lifecycle_catalog`]
//!   adds on top.
//! - `crate::vim_lsp_specialized_driver` (#11380) owns the action vocabulary;
//!   the scenario ledger is *derived* from the landed host-reopen actions, so
//!   the binding cannot drift from the vocabulary.
//! - `.ci/editor-clients/vim-vim-lsp-lifecycle-root.v1.json` (#11387) owns the
//!   lifecycle-chain denominator bytes: stage entries, host-replacement and
//!   pending-identity requirements, iteration denominator, cleanup kinds,
//!   cardinality laws, and honest-claim rules. The mirror here is checked
//!   against that file by tests; a denominator change is a reviewed edit that
//!   changes every affected digest visibly.
//! - #11376 owns the lifecycle BDD scenarios and #11378 the lifecycle
//!   fixtures; both remain pending. Until they land, cells bind the landed
//!   #11380 action vocabulary as scenario owners and the landed #11369
//!   fixture authorities as fixture owners; re-binding is a reviewed edit.
//!
//! Family laws beyond the shared model (all fail-closed):
//!
//! - the ledger mirrors exactly the landed #11380 host-reopen actions;
//! - the declared fixture substrate is pinned to the lifecycle-root
//!   denominator artifact plus the #11369 authorities, and every cell cites
//!   that complete substrate as its fixture owners, so the catalog cannot
//!   stop citing the authority that owns its rows through a reviewed edit;
//! - the registered cells are exactly the 8 denominator stages: a missing
//!   stage cell, a duplicate stage registration, or a cell outside the finite
//!   #11387 denominator (a relabeled server restart, a baseline cleanup row,
//!   an invented stage) is rejected;
//! - every cell binds the five `generation.*` dimensions of the artifact's
//!   generation kinds, so a receipt must name the old/new process, host,
//!   document, root, and config generations and another process, host, or
//!   provider cannot supply the observation;
//! - every cell binds exactly one `lifecycle.row.*` dimension that matches its
//!   own cell-ID stage, its row's `lifecycle.entry.*` entry condition, its
//!   row's `lifecycle.cardinality.*` cardinality law, and its row's
//!   `lifecycle.row_binding.*` authority identity (a sha256 over every #11387
//!   denominator field), so one stage's observation cannot inherit another
//!   stage's identity and a denominator edit of any authority field is
//!   digest-visible;
//! - the host-replacement requirement binds exactly the host-replacement rows
//!   (`host_reopen`, `repeated_sessions`): a server restart or a buffer reopen
//!   passing while full host replacement is omitted fails the host-reopen
//!   cell, and a same-host row binding the host-replacement dimension to
//!   relabel itself fails closed;
//! - the pending-action identity requirement binds exactly the pending rows
//!   (`cancellation`, `late_result_rejected`), so a late result is rejected by
//!   identity, not by disposition re-serialization;
//! - the repeated-session row binds its finite iteration denominator (>= 2
//!   changed host instances) and a fresh per-iteration result, so one passing
//!   run can never relabel repeated use and stale prior
//!   receipt/profile/temp state can never satisfy a new iteration;
//! - the cleanup rows bind their cleanup kind and require observed cleanup
//!   evidence, so a client exit event or a force-kill alone is never clean
//!   cleanup;
//! - the replacement-host stage binds the artifact's mirrored
//!   initialize/initialized/buffer-enabled sequence dimension, so a replacement
//!   process spawn without initialize/readiness cannot satisfy it;
//! - each stage is classified by its one pinned #11380 action, and each
//!   stage's complete scenario-owner set is pinned to its declared set
//!   (fail-closed both ways: dropped and widened), so the exit path that
//!   distinguishes a full host reopen from a server restart cannot be dropped
//!   from the `host_reopen` cell and another family's action cannot own a
//!   lifecycle cell;
//! - each stage's allowed result set is pinned, every cell admits `fail` and
//!   `not_proven`;
//! - the stage bound is `exact_source_local` only and every cell feeds only
//!   `vim_first_class_exact_source`.

use anyhow::{Context as _, Result, ensure};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeSet;

use super::{
    CellCatalog, CellRegistration, CellSubject, CoverageRule, InstrumentEvidence, Scenario,
    ScenarioClass, ScenarioLedger,
};
use crate::editor_client_compat::EvidenceStage;
use crate::vim_lsp_specialized_driver::{ACTIONS, ActionFamily};

pub const LIFECYCLE_CATALOG_ID: &str = "vim_lsp_lifecycle";
pub const LIFECYCLE_LEDGER_ID: &str = "vim.vim_lsp.specialized.host_reopen.v1";

/// Fixture substrate: the landed #11369 authorities plus the #11387
/// lifecycle-root denominator this family binds until #11376/#11378 land
/// their owning surfaces. Tests verify each ID resolves to
/// `.ci/editor-clients/<id>.json`, so an absent authority fails closed.
pub const LIFECYCLE_FIXTURE_SUBSTRATE: &[&str] = &[
    "vim-vim-lsp-lifecycle-root.v1",
    "vim-vim-lsp-configuration.v1",
    "vim-vim-lsp-public-surface.v1",
    "vim-vim-lsp-subject.v1",
];

/// Lifecycle dispositions (#11387): exactly the receipt-serializable
/// disposition set the family's propositions can honestly carry.
pub const LIFECYCLE_RESULT_VOCABULARY: &[&str] =
    &["pass", "fail", "partial", "client_not_exposed", "unsupported", "not_proven"];

pub const LIFECYCLE_LIMITATION_VOCABULARY: &[&str] = &[
    "client_not_exposed",
    "not_proven",
    "session_iterations_incomplete",
    "observation_incomplete",
    "instrument_incomplete",
];

const LIFECYCLE_PROFILE: &str = "vim_first_class_exact_source";
const CELL_PREFIX: &str = "vim.vim_lsp.lifecycle.";
const ROW_DIMENSION_PREFIX: &str = "lifecycle.row.";
const ENTRY_DIMENSION_PREFIX: &str = "lifecycle.entry.";
const CARDINALITY_DIMENSION_PREFIX: &str = "lifecycle.cardinality.";
const ROW_BINDING_PREFIX: &str = "lifecycle.row_binding.";
const HOST_REPLACEMENT_DIMENSION: &str = "lifecycle.host_replacement.required";
const PENDING_IDENTITY_DIMENSION: &str = "lifecycle.pending_identity.required";
const MIN_ITERATIONS_DIMENSION_PREFIX: &str = "lifecycle.min_iterations.";
const PER_ITERATION_RESULT_DIMENSION: &str = "lifecycle.per_iteration_result.required";
const CLEANUP_DIMENSION_PREFIX: &str = "lifecycle.cleanup.";
const GENERATION_DIMENSION_PREFIX: &str = "generation.";

/// The five generation kinds every lifecycle cell binds old/new (the #11387
/// required binding: process, host, document, root, config generations),
/// mirrored from the artifact's `generations.kinds` array.
pub const GENERATION_KINDS: &[&str] = &["process", "host", "document", "root", "config"];

/// The initialize/readiness sequence a replacement host must complete before a
/// host-reopen observation may classify beyond `not_proven`, mirrored from the
/// artifact's `generations.initialize_sequence`.
pub const INITIALIZE_SEQUENCE: &[&str] = &["initialize", "initialized", "buffer_enabled"];

/// Dimensions every lifecycle cell must bind: the pinned client/server/stage
/// identity.
const REQUIRED_DIMENSIONS: &[&str] =
    &["client.pinned_commit", "server.executable_identity", "stage.exact_source_local"];

/// The cleanup kind of rows that carry no terminal-cleanup proposition.
const NO_CLEANUP: &str = "none";

/// The minimum repeated-session iterations the #11387 denominator admits; a
/// single passing run can never pose as repeated use.
pub const MIN_REPEATED_SESSION_ITERATIONS: u32 = 2;

/// One row of the finite #11387 lifecycle-chain denominator, mirrored from
/// `.ci/editor-clients/vim-vim-lsp-lifecycle-root.v1.json` in artifact order.
/// Tests check every field against the artifact, so the mirror cannot drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LifecycleDenominatorRow {
    /// Verbatim artifact `stage` identity; also the cell-ID name segment.
    pub stage_id: &'static str,
    /// Verbatim artifact entry condition (how this chain stage is entered).
    pub entry: &'static str,
    /// Verbatim artifact host-replacement requirement: the stage's proposition
    /// needs a changed host instance, so a server restart or a buffer reopen
    /// can never fill it.
    pub host_replacement: bool,
    /// Verbatim artifact pending-identity requirement: the stage binds a
    /// pending action/request identity (cancellation, late-result rejection).
    pub pending_identity: bool,
    /// Verbatim artifact iteration denominator; zero on non-repeated rows.
    pub min_iterations: u32,
    /// Verbatim artifact cleanup kind (`none`, `normal_exit`,
    /// `forced_failure`).
    pub cleanup: &'static str,
    /// Verbatim artifact cardinality law this stage binds.
    pub cardinality: &'static str,
    /// Verbatim artifact honest-disposition shape of the stage.
    pub disposition: &'static str,
}

/// The finite #11387-backed denominator: the 8 lifecycle-chain stages in
/// artifact order. A cell outside this set cannot register (family law), and
/// each stage registers exactly one cell, so the catalog carries 8 cells.
pub const LIFECYCLE_DENOMINATOR: &[LifecycleDenominatorRow] = &[
    LifecycleDenominatorRow {
        stage_id: "buffer_reopen",
        entry: "buffer_closed_in_same_host",
        host_replacement: false,
        pending_identity: false,
        min_iterations: 0,
        cleanup: "none",
        cardinality: "buffer_reopened_same_host_once",
        disposition: "same_host_reopen_not_host_replacement",
    },
    LifecycleDenominatorRow {
        stage_id: "host_reopen",
        entry: "host_exit_and_replacement_launch",
        host_replacement: true,
        pending_identity: false,
        min_iterations: 0,
        cleanup: "none",
        cardinality: "replacement_host_initialized_once",
        disposition: "host_instance_changed_not_server_restart",
    },
    LifecycleDenominatorRow {
        stage_id: "workspace_or_session_reopen",
        entry: "workspace_or_session_replacement",
        host_replacement: false,
        pending_identity: false,
        min_iterations: 0,
        cleanup: "none",
        cardinality: "workspace_replacement_observed_once",
        disposition: "replacement_where_exposed_never_relabeled",
    },
    LifecycleDenominatorRow {
        stage_id: "cancellation",
        entry: "pending_action_invalidated",
        host_replacement: false,
        pending_identity: true,
        min_iterations: 0,
        cleanup: "none",
        cardinality: "pending_action_cancelled_bounded_once",
        disposition: "held_action_never_applies_after_invalidation",
    },
    LifecycleDenominatorRow {
        stage_id: "late_result_rejected",
        entry: "pending_action_invalidated",
        host_replacement: false,
        pending_identity: true,
        min_iterations: 0,
        cleanup: "none",
        cardinality: "late_result_rejected_not_admitted",
        disposition: "late_old_host_result_rejected",
    },
    LifecycleDenominatorRow {
        stage_id: "repeated_sessions",
        entry: "finite_repeated_session_sequence",
        host_replacement: true,
        pending_identity: false,
        min_iterations: MIN_REPEATED_SESSION_ITERATIONS,
        cleanup: "none",
        cardinality: "bounded_iterations_each_observed",
        disposition: "per_iteration_fresh_not_stale_state",
    },
    LifecycleDenominatorRow {
        stage_id: "normal_cleanup",
        entry: "normal_host_exit",
        host_replacement: false,
        pending_identity: false,
        min_iterations: 0,
        cleanup: "normal_exit",
        cardinality: "normal_exit_cleanup_settled_once",
        disposition: "cleanup_observed_not_client_event_or_force_kill",
    },
    LifecycleDenominatorRow {
        stage_id: "failure_cleanup",
        entry: "forced_failure_or_timeout",
        host_replacement: false,
        pending_identity: false,
        min_iterations: 0,
        cleanup: "forced_failure",
        cardinality: "failure_path_cleanup_settled_once",
        disposition: "forced_failure_cleanup_observed",
    },
];

const BUFFER_CLOSE_WIPE_REOPEN: &str =
    "vim.vim_lsp.specialized.host_reopen.buffer_close_wipe_reopen";
const EXIT_HOST: &str = "vim.vim_lsp.specialized.host_reopen.exit_host";
const LAUNCH_REPLACEMENT_HOST: &str = "vim.vim_lsp.specialized.host_reopen.launch_replacement_host";
const REPLACE_WORKSPACE_SESSION: &str =
    "vim.vim_lsp.specialized.host_reopen.replace_workspace_session";
const PENDING_ACTION_START_INVALIDATE: &str =
    "vim.vim_lsp.specialized.host_reopen.pending_action_start_invalidate";
const REPEATED_SESSION_SEQUENCE: &str =
    "vim.vim_lsp.specialized.host_reopen.repeated_session_sequence";
const FORCED_FAILURE_PATH: &str = "vim.vim_lsp.specialized.host_reopen.forced_failure_path";

/// The classifying action of one denominator stage (`LIFECYCLE_DENOMINATOR`
/// order): the validator enforces the mapping, not only the factory, so a
/// buffer-reopen or server-restart observation can never classify a full
/// host-replacement proposition even through a reviewed row edit.
const STAGE_OBSERVATION_CLASSES: &[&str] = &[
    BUFFER_CLOSE_WIPE_REOPEN,
    LAUNCH_REPLACEMENT_HOST,
    REPLACE_WORKSPACE_SESSION,
    PENDING_ACTION_START_INVALIDATE,
    PENDING_ACTION_START_INVALIDATE,
    REPEATED_SESSION_SEQUENCE,
    EXIT_HOST,
    FORCED_FAILURE_PATH,
];

/// Scenario owners of one denominator stage (`LIFECYCLE_DENOMINATOR` order):
/// the classifying action plus the chain actions that must be citable for the
/// stage's proposition to be meaningful. The full-host-reopen stage cites the
/// exit action as well as the replacement launch: the exit path is what
/// distinguishes a full host reopen from a server restart, and the pinned-set
/// law keeps it from being dropped.
const STAGE_SCENARIO_OWNERS: &[&[&str]] = &[
    &[BUFFER_CLOSE_WIPE_REOPEN],
    &[EXIT_HOST, LAUNCH_REPLACEMENT_HOST],
    &[REPLACE_WORKSPACE_SESSION],
    &[PENDING_ACTION_START_INVALIDATE],
    &[PENDING_ACTION_START_INVALIDATE],
    &[REPEATED_SESSION_SEQUENCE],
    &[EXIT_HOST],
    &[FORCED_FAILURE_PATH],
];

/// Instrument/reporting/cleanup evidence of one denominator stage
/// (`LIFECYCLE_DENOMINATOR` order).
const STAGE_INSTRUMENT: &[&[InstrumentEvidence]] = &[
    &[
        InstrumentEvidence::CapabilitySnapshot,
        InstrumentEvidence::ClientLog,
        InstrumentEvidence::DriverOutput,
        InstrumentEvidence::ProcessLedger,
    ],
    &[
        InstrumentEvidence::CapabilitySnapshot,
        InstrumentEvidence::CleanupObservation,
        InstrumentEvidence::ClientLog,
        InstrumentEvidence::DriverOutput,
        InstrumentEvidence::ProcessLedger,
    ],
    &[
        InstrumentEvidence::CapabilitySnapshot,
        InstrumentEvidence::ClientLog,
        InstrumentEvidence::DriverOutput,
    ],
    &[
        InstrumentEvidence::ClientLog,
        InstrumentEvidence::DriverOutput,
        InstrumentEvidence::ProcessLedger,
    ],
    &[
        InstrumentEvidence::ClientLog,
        InstrumentEvidence::DriverOutput,
        InstrumentEvidence::ProcessLedger,
        InstrumentEvidence::ServerStderr,
    ],
    &[
        InstrumentEvidence::CleanupObservation,
        InstrumentEvidence::ClientLog,
        InstrumentEvidence::DriverOutput,
        InstrumentEvidence::ProcessLedger,
    ],
    &[
        InstrumentEvidence::CleanupObservation,
        InstrumentEvidence::ClientLog,
        InstrumentEvidence::DriverOutput,
        InstrumentEvidence::ProcessLedger,
    ],
    &[
        InstrumentEvidence::CleanupObservation,
        InstrumentEvidence::ClientLog,
        InstrumentEvidence::DriverOutput,
        InstrumentEvidence::FailureDiagnostics,
        InstrumentEvidence::ProcessLedger,
    ],
];

/// The pinned result set of every denominator stage: honest failure, honest
/// incompleteness, honest non-exposure, and honest non-support are always
/// expressible alongside `pass`/`partial`.
const STAGE_RESULTS: &[&str] =
    &["pass", "fail", "partial", "client_not_exposed", "unsupported", "not_proven"];

/// Actions whose citation makes cleanup evidence independently load-bearing:
/// the host exit, the repeated-session sequence, and the forced-failure path
/// all settle process exit and cleanup, so cells citing them cannot drop that
/// evidence.
const CLEANUP_REQUIRING_OWNERS: &[&str] =
    &[EXIT_HOST, REPEATED_SESSION_SEQUENCE, FORCED_FAILURE_PATH];

/// The cardinality law that binds the artifact's initialize sequence, so the
/// replacement-host stage stays pinned to initialize/readiness.
const INITIALIZE_CARDINALITY: &str = "replacement_host_initialized_once";

const BASE_CLAIM_CEILING: &str = "registration only: pre-registers one exact-subject Vim/vim-lsp host-reopen/repeated-session cell for the generic editor_client_compat.v1 receipt, keyed by the finite #11387 lifecycle-root denominator; binds landed #11380 action owners and #11369 fixtures until #11376/#11378 land their owning surfaces; proves no host behavior and awards no support profile";
const BUFFER_REOPEN_CLAIM_CEILING: &str = "registration only: a buffer reopen is the same-host close/wipe+reopen of the exact buffer; a full host replacement or a server restart can never satisfy it";
const HOST_REOPEN_CLAIM_CEILING: &str = "registration only: a full host reopen binds the exit of the exact host plus a replacement-host launch with a changed host instance through the complete initialize sequence; a server restart, a buffer reopen, or a bare new PID can never satisfy it";
const WORKSPACE_CLAIM_CEILING: &str = "registration only: workspace/session replacement counts only where the client exposes the concept; a non-exposed client stays client_not_exposed, never relabeled";
const CANCELLATION_CLAIM_CEILING: &str = "registration only: a cancelled pending action is cancelled by identity; the held action never applies after invalidation";
const LATE_REJECTION_CLAIM_CEILING: &str = "registration only: a late old-host/document/process result must be rejected; admitting it while later answers look correct fails";
const REPEATED_SESSIONS_CLAIM_CEILING: &str = "registration only: repeated use binds a finite iteration denominator over changed host instances with a fresh per-iteration result; one passing run is not repeated use and stale prior state can never satisfy a new iteration";
const NORMAL_CLEANUP_CLAIM_CEILING: &str = "registration only: normal-exit terminal cleanup settles observably; a client exit event or a force-kill alone is not clean cleanup";
const FAILURE_CLEANUP_CLAIM_CEILING: &str = "registration only: forced-failure/timeout terminal cleanup settles observably; an unsettled or unobserved cleanup stays not_proven";

const STAGE_CLAIM_CEILINGS: &[&str] = &[
    BUFFER_REOPEN_CLAIM_CEILING,
    HOST_REOPEN_CLAIM_CEILING,
    WORKSPACE_CLAIM_CEILING,
    CANCELLATION_CLAIM_CEILING,
    LATE_REJECTION_CLAIM_CEILING,
    REPEATED_SESSIONS_CLAIM_CEILING,
    NORMAL_CLEANUP_CLAIM_CEILING,
    FAILURE_CLEANUP_CLAIM_CEILING,
];

const HEX: &[u8; 16] = b"0123456789abcdef";

/// The stable authority identity of one denominator row: a sha256 over every
/// authority field the #11387 artifact carries for the row (stage, entry,
/// host-replacement and pending-identity requirements, iteration denominator,
/// cleanup kind, cardinality law, disposition shape). Every cell of the row
/// binds it as a `lifecycle.row_binding.sha256-<hex>` dimension, so an
/// artifact edit of *any* row field changes that row's cell digest and the
/// catalog digest: denominator edits stay digest-visible, never silent.
pub fn row_binding_identity(row: &LifecycleDenominatorRow) -> String {
    let canonical = format!(
        "stage={}|entry={}|host_replacement={}|pending_identity={}|min_iterations={}|cleanup={}|cardinality={}|disposition={}",
        row.stage_id,
        row.entry,
        row.host_replacement,
        row.pending_identity,
        row.min_iterations,
        row.cleanup,
        row.cardinality,
        row.disposition,
    );
    let digest = Sha256::digest(canonical.as_bytes());
    let mut identity = String::with_capacity(ROW_BINDING_PREFIX.len() + "sha256-".len() + 64);
    identity.push_str(ROW_BINDING_PREFIX);
    identity.push_str("sha256-");
    for byte in digest {
        identity.push(HEX[(byte >> 4) as usize] as char);
        identity.push(HEX[(byte & 0x0f) as usize] as char);
    }
    identity
}

/// Look up one denominator row by its cell-ID stage segment.
fn row_by_stage(stage: &str) -> Option<(usize, &'static LifecycleDenominatorRow)> {
    LIFECYCLE_DENOMINATOR.iter().enumerate().find(|(_, row)| row.stage_id == stage)
}

/// The row-scoped dimensions every lifecycle cell binds: its denominator stage
/// identity, its row's artifact entry condition, its row's cardinality law,
/// and the row's full authority identity (see [`row_binding_identity`]), so a
/// receipt must name the exact chain stage, cannot inherit another stage's
/// entry or cardinality, and a denominator edit of any authority field is
/// digest-visible.
fn row_dimensions(row: &LifecycleDenominatorRow) -> Vec<String> {
    vec![
        format!("{}{}", ROW_DIMENSION_PREFIX, row.stage_id),
        format!("{}{}", ENTRY_DIMENSION_PREFIX, row.entry),
        format!("{}{}", CARDINALITY_DIMENSION_PREFIX, row.cardinality),
        row_binding_identity(row),
    ]
}

/// Build one lifecycle cell for a denominator stage.
fn build_cell(
    row: &LifecycleDenominatorRow,
    index: usize,
    subject: CellSubject,
) -> CellRegistration {
    let mut dimensions =
        REQUIRED_DIMENSIONS.iter().map(|value| value.to_string()).collect::<Vec<_>>();
    for kind in GENERATION_KINDS {
        dimensions.push(format!("{GENERATION_DIMENSION_PREFIX}{kind}"));
    }
    dimensions.extend(row_dimensions(row));
    // The iff laws of the family: the host-replacement requirement binds
    // exactly the host-replacement rows, the pending-identity requirement
    // exactly the pending rows, the iteration denominator exactly the
    // repeated-session row, and the cleanup kind exactly the cleanup rows.
    if row.host_replacement {
        dimensions.push(HOST_REPLACEMENT_DIMENSION.to_string());
    }
    if row.pending_identity {
        dimensions.push(PENDING_IDENTITY_DIMENSION.to_string());
    }
    if row.min_iterations > 0 {
        dimensions.push(format!("{MIN_ITERATIONS_DIMENSION_PREFIX}{}", row.min_iterations));
        dimensions.push(PER_ITERATION_RESULT_DIMENSION.to_string());
    }
    if row.cleanup != NO_CLEANUP {
        dimensions.push(format!("{CLEANUP_DIMENSION_PREFIX}{}", row.cleanup));
    }
    // The replacement-host stage binds the artifact's mirrored sequence, so a
    // replacement process spawn without initialize/readiness cannot satisfy it.
    if row.cardinality == INITIALIZE_CARDINALITY {
        dimensions.push(format!("lifecycle.initialize_sequence.{}", INITIALIZE_SEQUENCE.join("_")));
    }
    CellRegistration {
        cell_id: format!("{CELL_PREFIX}{}", row.stage_id),
        cell_version: 1,
        scenario_owners: STAGE_SCENARIO_OWNERS[index]
            .iter()
            .map(|value| value.to_string())
            .collect(),
        fixture_owners: LIFECYCLE_FIXTURE_SUBSTRATE.iter().map(|value| value.to_string()).collect(),
        subject,
        observation_class: STAGE_OBSERVATION_CLASSES[index].to_string(),
        subject_dimensions: dimensions,
        instrument_evidence: STAGE_INSTRUMENT[index].to_vec(),
        allowed_stages: vec![EvidenceStage::ExactSourceLocal],
        allowed_results: STAGE_RESULTS.iter().map(|value| value.to_string()).collect(),
        allowed_limitations: LIFECYCLE_LIMITATION_VOCABULARY
            .iter()
            .map(|value| value.to_string())
            .collect(),
        allowed_profiles: vec![LIFECYCLE_PROFILE.to_string()],
        claim_ceiling: format!("{BASE_CLAIM_CEILING}; {}", STAGE_CLAIM_CEILINGS[index]),
    }
}

/// The host-reopen/repeated-session scenario ledger, derived from the landed
/// #11380 host-reopen action vocabulary: one baseline scenario per action ID,
/// sorted for deterministic aggregation. #11376 owns the BDD scenario ledger;
/// when it lands, this derivation is superseded by a reviewed re-bind.
pub fn lifecycle_action_ledger() -> ScenarioLedger {
    let mut scenarios: Vec<Scenario> = ACTIONS
        .iter()
        .filter(|action| action.family == ActionFamily::HostReopen)
        .map(|action| Scenario { id: action.action_id.to_string(), class: ScenarioClass::Baseline })
        .collect();
    scenarios.sort_by(|left, right| left.id.cmp(&right.id));
    ScenarioLedger {
        ledger_id: LIFECYCLE_LEDGER_ID.to_string(),
        owning_authority: "#11380 specialized action vocabulary (PR #12204), host-reopen family; denominator: #11387 vim-vim-lsp-lifecycle-root.v1; supersedes pending: #11376 owns the BDD scenario ledger, #11378 the fixture/expectation cells"
            .to_string(),
        scenarios,
    }
}

/// The host-reopen/repeated-session family catalog registered on this PR
/// (#11387): the finite denominator stages, one exact convention cell each.
pub fn lifecycle_catalog() -> CellCatalog {
    let subject = super::vim_vim_lsp_subject();
    let mut cells = Vec::with_capacity(LIFECYCLE_DENOMINATOR.len());
    for (index, row) in LIFECYCLE_DENOMINATOR.iter().enumerate() {
        cells.push(build_cell(row, index, subject.clone()));
    }
    CellCatalog {
        catalog_id: LIFECYCLE_CATALOG_ID.to_string(),
        catalog_version: 1,
        ledger_id: LIFECYCLE_LEDGER_ID.to_string(),
        coverage: CoverageRule::AdditiveFamily,
        fixture_substrate: LIFECYCLE_FIXTURE_SUBSTRATE
            .iter()
            .map(|value| value.to_string())
            .collect(),
        allowed_stages: vec![EvidenceStage::ExactSourceLocal],
        allowed_result_vocabulary: LIFECYCLE_RESULT_VOCABULARY
            .iter()
            .map(|value| value.to_string())
            .collect(),
        core_profile: None,
        cells,
    }
}

/// The landed host-reopen action IDs, as the family's authority set.
fn host_reopen_action_ids() -> BTreeSet<&'static str> {
    ACTIONS
        .iter()
        .filter(|action| action.family == ActionFamily::HostReopen)
        .map(|action| action.action_id)
        .collect()
}

/// Validate the compiled lifecycle catalog against the family laws.
pub fn validate_family_laws() -> Result<()> {
    validate_lifecycle_catalog(&lifecycle_catalog(), &lifecycle_action_ledger())
}

/// Validate one lifecycle-shaped catalog against the family laws. Shared-model
/// laws (subject pin, stage bound, duplicate IDs, ledger membership,
/// cross-catalog ownership) run in [`super::validate_registry`]; the laws here
/// are the ones only this family can state.
pub fn validate_lifecycle_catalog(catalog: &CellCatalog, ledger: &ScenarioLedger) -> Result<()> {
    ensure!(
        catalog.catalog_id == LIFECYCLE_CATALOG_ID,
        "lifecycle family catalog must keep its identity {LIFECYCLE_CATALOG_ID}, found {}",
        catalog.catalog_id
    );
    ensure!(
        catalog.ledger_id == LIFECYCLE_LEDGER_ID && ledger.ledger_id == LIFECYCLE_LEDGER_ID,
        "lifecycle family must bind ledger {LIFECYCLE_LEDGER_ID}"
    );
    ensure!(
        catalog.coverage == CoverageRule::AdditiveFamily,
        "lifecycle family catalog is additive, not a baseline-coverage catalog"
    );
    ensure!(
        catalog.core_profile.is_none(),
        "lifecycle family assigns no core profile; profiles consume cells, catalogs do not assign them"
    );
    ensure!(
        catalog.allowed_stages.len() == 1
            && catalog.allowed_stages[0] == EvidenceStage::ExactSourceLocal,
        "lifecycle family stage bound is exact_source_local only; an exact-source cell cannot inherit a maintained/public stage"
    );
    let declared: BTreeSet<&str> =
        catalog.allowed_result_vocabulary.iter().map(String::as_str).collect();
    let expected: BTreeSet<&str> = LIFECYCLE_RESULT_VOCABULARY.iter().copied().collect();
    ensure!(
        declared == expected,
        "lifecycle result vocabulary drifted from the #11387 dispositions"
    );
    // The declared substrate is pinned: the lifecycle-root denominator
    // artifact and the #11369 authorities cannot be dropped (the catalog
    // would stop citing the authority that owns its rows) or widened (an
    // unlanded authority would enter through a reviewed edit) — the shared
    // non-empty-substrate law alone cannot see either drift.
    let declared_substrate: BTreeSet<&str> =
        catalog.fixture_substrate.iter().map(String::as_str).collect();
    let pinned_substrate: BTreeSet<&str> = LIFECYCLE_FIXTURE_SUBSTRATE.iter().copied().collect();
    ensure!(
        declared_substrate == pinned_substrate,
        "lifecycle fixture substrate drifted from the pinned #11387 substrate; the lifecycle-root denominator artifact and the #11369 authorities cannot be dropped or widened"
    );

    let actions = host_reopen_action_ids();
    let scenarios: BTreeSet<&str> = ledger.scenarios.iter().map(|s| s.id.as_str()).collect();
    ensure!(
        scenarios == actions,
        "lifecycle ledger must mirror exactly the landed #11380 host-reopen actions; ledger has {} rows, vocabulary has {}",
        scenarios.len(),
        actions.len()
    );

    // The registered stages must be exactly the denominator stages.
    let mut registered: BTreeSet<String> = BTreeSet::new();
    let mut covered: BTreeSet<String> = BTreeSet::new();
    for cell in &catalog.cells {
        ensure!(
            cell.cell_id.starts_with(CELL_PREFIX),
            "cell {} is outside the lifecycle family namespace {CELL_PREFIX}",
            cell.cell_id
        );
        let stage = &cell.cell_id[CELL_PREFIX.len()..];
        let (index, row) = row_by_stage(stage).with_context(|| {
            format!(
                "cell {} names stage {stage} outside the finite #11387 lifecycle-root denominator; a relabeled server restart, a baseline cleanup row, or an invented stage cannot register here",
                cell.cell_id
            )
        })?;
        ensure!(
            registered.insert(stage.to_string()),
            "duplicate lifecycle stage registration {stage}"
        );

        ensure!(
            actions.contains(cell.observation_class.as_str()),
            "cell {} observation class {} is not a landed host-reopen action; another family's action or an invented token cannot classify a lifecycle cell",
            cell.cell_id,
            cell.observation_class
        );
        // Each stage is classified by its one pinned action — not merely a
        // landed action the cell cites — so a buffer-reopen or server-restart
        // observation can never classify a full host-replacement proposition
        // even through a reviewed row edit.
        let required_class = STAGE_OBSERVATION_CLASSES[index];
        ensure!(
            cell.observation_class == required_class,
            "cell {} must be classified by {required_class} for stage {stage}, found {}; the wrong lifecycle proposition cannot satisfy this cell",
            cell.cell_id,
            cell.observation_class
        );
        ensure!(
            cell.scenario_owners.contains(&cell.observation_class),
            "cell {} observation class {} must be one of its own scenario owners",
            cell.cell_id,
            cell.observation_class
        );
        // The stage's complete scenario-owner set is pinned, not only its
        // classifying action: the entry-path owners that distinguish a stage
        // from its neighbors (for example the exit path that distinguishes a
        // full host reopen from a server restart) cannot be dropped while the
        // union-coverage law stays satisfied through other cells.
        let expected_owners: BTreeSet<&str> =
            STAGE_SCENARIO_OWNERS[index].iter().copied().collect();
        let declared_owners: BTreeSet<&str> =
            cell.scenario_owners.iter().map(String::as_str).collect();
        ensure!(
            declared_owners == expected_owners,
            "cell {} scenario owners drifted from the pinned {stage} stage owner set; the lifecycle-entry paths that distinguish this stage from a server restart or a buffer reopen cannot be dropped or widened",
            cell.cell_id
        );
        // Every cell cites the complete pinned substrate as its fixture
        // owners, so no cell can stop citing the lifecycle-root denominator
        // artifact while the catalog-level substrate stays pinned.
        let declared_fixture_owners: BTreeSet<&str> =
            cell.fixture_owners.iter().map(String::as_str).collect();
        ensure!(
            declared_fixture_owners == pinned_substrate,
            "cell {} fixture owners drifted from the pinned #11387 substrate; a cell that stops citing the lifecycle-root denominator artifact or a #11369 authority fails closed",
            cell.cell_id
        );
        ensure!(
            cell.allowed_results.iter().any(|result| result == "fail")
                && cell.allowed_results.iter().any(|result| result == "not_proven"),
            "cell {} must admit fail and not_proven; honest failure and honest incompleteness are always expressible",
            cell.cell_id
        );
        for dimension in REQUIRED_DIMENSIONS {
            ensure!(
                cell.subject_dimensions.iter().any(|token| token == dimension),
                "cell {} must bind required dimension {dimension}",
                cell.cell_id
            );
        }

        // Generation law: the old/new process, host, document, root, and
        // config generations are bound by every cell, so another process,
        // host, or provider cannot supply the observation.
        for kind in GENERATION_KINDS {
            let dimension = format!("{GENERATION_DIMENSION_PREFIX}{kind}");
            ensure!(
                cell.subject_dimensions.iter().any(|token| token == &dimension),
                "cell {} must bind the generation dimension {dimension} of the #11387 required binding",
                cell.cell_id
            );
        }

        // Host-replacement law (iff): exactly the host-replacement rows bind
        // the changed-host-instance requirement, so a server restart or a
        // buffer reopen passing while full host replacement is omitted fails
        // the host-reopen cell, and a same-host row cannot relabel itself.
        let binds_host_replacement =
            cell.subject_dimensions.iter().any(|token| token == HOST_REPLACEMENT_DIMENSION);
        ensure!(
            binds_host_replacement == row.host_replacement,
            "cell {} host-replacement binding must match its row's #11387 requirement ({}); a server restart or a buffer reopen can never satisfy a full host replacement, and a same-host stage can never bind one",
            cell.cell_id,
            row.host_replacement
        );

        // Pending-identity law (iff): exactly the pending rows bind the
        // pending action/request identity.
        let binds_pending_identity =
            cell.subject_dimensions.iter().any(|token| token == PENDING_IDENTITY_DIMENSION);
        ensure!(
            binds_pending_identity == row.pending_identity,
            "cell {} pending-identity binding must match its row's #11387 requirement ({})",
            cell.cell_id,
            row.pending_identity
        );

        // Repeated-session denominator law: the repeated row binds its finite
        // iteration denominator and a fresh per-iteration result; no other row
        // binds an iteration dimension, so one passing run can never pose as
        // repeated use.
        let iteration_dimensions: Vec<&String> = cell
            .subject_dimensions
            .iter()
            .filter(|token| token.starts_with(MIN_ITERATIONS_DIMENSION_PREFIX))
            .collect();
        if row.min_iterations > 0 {
            let expected = format!("{MIN_ITERATIONS_DIMENSION_PREFIX}{}", row.min_iterations);
            ensure!(
                iteration_dimensions.len() == 1 && iteration_dimensions[0].as_str() == expected,
                "cell {} must bind exactly one {MIN_ITERATIONS_DIMENSION_PREFIX}* dimension equal to its row's #11387 iteration denominator {expected}",
                cell.cell_id
            );
            ensure!(
                row.min_iterations >= MIN_REPEATED_SESSION_ITERATIONS,
                "cell {} iteration denominator {} is below the #11387 minimum of {MIN_REPEATED_SESSION_ITERATIONS}; one passing run is not repeated use",
                cell.cell_id,
                row.min_iterations
            );
            ensure!(
                cell.subject_dimensions.iter().any(|token| token == PER_ITERATION_RESULT_DIMENSION),
                "cell {} must bind {PER_ITERATION_RESULT_DIMENSION}; a stale prior result can never satisfy a new iteration",
                cell.cell_id
            );
        } else {
            ensure!(
                iteration_dimensions.is_empty(),
                "cell {} of non-repeated stage {stage} binds an iteration denominator; only the repeated-session stage carries one",
                cell.cell_id
            );
        }

        // Cleanup law (iff): exactly the cleanup rows bind their cleanup kind
        // and must observe cleanup — a client exit event or a force-kill
        // alone is never clean cleanup.
        let cleanup_dimensions: Vec<&String> = cell
            .subject_dimensions
            .iter()
            .filter(|token| token.starts_with(CLEANUP_DIMENSION_PREFIX))
            .collect();
        if row.cleanup != NO_CLEANUP {
            let expected = format!("{CLEANUP_DIMENSION_PREFIX}{}", row.cleanup);
            ensure!(
                cleanup_dimensions.len() == 1 && cleanup_dimensions[0].as_str() == expected,
                "cell {} must bind exactly one {CLEANUP_DIMENSION_PREFIX}* dimension equal to its row's #11387 cleanup kind {expected}",
                cell.cell_id
            );
            ensure!(
                cell.instrument_evidence.contains(&InstrumentEvidence::CleanupObservation),
                "cell {} of cleanup stage {stage} must require cleanup evidence; a client exit event or a force-kill alone is not clean cleanup",
                cell.cell_id
            );
        } else {
            ensure!(
                cleanup_dimensions.is_empty(),
                "cell {} of non-cleanup stage {stage} binds a cleanup kind dimension",
                cell.cell_id
            );
        }

        // Row identity: exactly one lifecycle.row.* dimension, matching the
        // cell's own stage, plus the row's artifact entry condition and
        // cardinality law — one stage's observation cannot inherit another
        // stage's identity.
        let row_dimensions: Vec<&String> = cell
            .subject_dimensions
            .iter()
            .filter(|token| token.starts_with(ROW_DIMENSION_PREFIX))
            .collect();
        ensure!(
            row_dimensions.len() == 1,
            "cell {} must bind exactly one {ROW_DIMENSION_PREFIX}* dimension",
            cell.cell_id
        );
        ensure!(
            row_dimensions[0].as_str() == format!("{ROW_DIMENSION_PREFIX}{stage}"),
            "cell {} binds row dimension {} which does not match its own stage",
            cell.cell_id,
            row_dimensions[0]
        );
        ensure!(
            cell.subject_dimensions
                .iter()
                .any(|token| token == &format!("{}{}", ENTRY_DIMENSION_PREFIX, row.entry)),
            "cell {} must bind its row's #11387 entry dimension {ENTRY_DIMENSION_PREFIX}{}",
            cell.cell_id,
            row.entry
        );
        ensure!(
            cell.subject_dimensions
                .iter()
                .any(|token| token
                    == &format!("{}{}", CARDINALITY_DIMENSION_PREFIX, row.cardinality)),
            "cell {} must bind its row's #11387 cardinality dimension {CARDINALITY_DIMENSION_PREFIX}{}; the reopen/denominator/cleanup cardinality cannot be omitted",
            cell.cell_id,
            row.cardinality
        );
        // Row authority identity: exactly one binding dimension, equal to the
        // digest over the row's full #11387 authority content, so denominator
        // edits of any field are digest-visible.
        let binding = row_binding_identity(row);
        let bindings: Vec<&String> = cell
            .subject_dimensions
            .iter()
            .filter(|token| token.starts_with(ROW_BINDING_PREFIX))
            .collect();
        ensure!(
            bindings.len() == 1 && bindings[0].as_str() == binding,
            "cell {} must bind exactly one {ROW_BINDING_PREFIX}* dimension equal to its row's authority identity {binding}; a #11387 denominator edit cannot stay digest-invisible",
            cell.cell_id
        );
        // The replacement-host stage binds the mirrored sequence dimension.
        if row.cardinality == INITIALIZE_CARDINALITY {
            let sequence =
                format!("lifecycle.initialize_sequence.{}", INITIALIZE_SEQUENCE.join("_"));
            ensure!(
                cell.subject_dimensions.iter().any(|token| token == &sequence),
                "cell {} must bind the initialize-sequence dimension {sequence}; a replacement process spawn without initialize/readiness cannot satisfy stage {stage}",
                cell.cell_id
            );
        }

        // Stage vocabularies are pinned: the chain stages' dispositions
        // cannot stand in for each other.
        let expected_results: BTreeSet<&str> = STAGE_RESULTS.iter().copied().collect();
        let declared_results: BTreeSet<&str> =
            cell.allowed_results.iter().map(String::as_str).collect();
        ensure!(
            declared_results == expected_results,
            "cell {} allowed results drifted from the pinned {stage} stage vocabulary of the #11387 lifecycle chain",
            cell.cell_id
        );

        // Cells citing the exit, repeated-session, or forced-failure actions
        // keep cleanup evidence independently load-bearing.
        for owner in CLEANUP_REQUIRING_OWNERS {
            if cell.scenario_owners.iter().any(|token| token == owner) {
                ensure!(
                    cell.instrument_evidence.contains(&InstrumentEvidence::CleanupObservation),
                    "cell {} cites the cleanup-settling action {owner} and must require cleanup evidence; lifecycle stages cannot contaminate each other silently",
                    cell.cell_id
                );
            }
        }

        ensure!(
            cell.allowed_profiles.len() == 1 && cell.allowed_profiles[0] == LIFECYCLE_PROFILE,
            "cell {} may feed only {LIFECYCLE_PROFILE}",
            cell.cell_id
        );
        covered.extend(cell.scenario_owners.iter().cloned());
    }

    let expected_stages: BTreeSet<String> =
        LIFECYCLE_DENOMINATOR.iter().map(|row| row.stage_id.to_string()).collect();
    let missing: Vec<String> = expected_stages.difference(&registered).cloned().collect();
    ensure!(
        missing.is_empty(),
        "denominator stage cells missing from the #11387 lifecycle family: {missing:?}"
    );

    let uncovered: Vec<&str> =
        actions.iter().filter(|action| !covered.contains(**action)).copied().collect();
    ensure!(
        uncovered.is_empty(),
        "landed host-reopen actions without a pre-registered cell: {uncovered:?}"
    );
    Ok(())
}
