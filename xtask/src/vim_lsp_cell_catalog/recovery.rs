//! The #11386 server-generation recovery family cell catalog.
//!
//! Every cell is keyed by the finite #11386 recovery-chain denominator: the 8
//! stage rows of `.ci/editor-clients/vim-vim-lsp-recovery-root.v1.json`,
//! mirrored in [`RECOVERY_DENOMINATOR`] and checked against that artifact by
//! tests so the mirror cannot drift from the landed authority. Each row
//! pre-registers exactly the #11386 convention cell of the same name — one
//! independently visible recovery proposition per chain stage:
//!
//! | Cell (== denominator row) | Classifying action | Proposition |
//! | --- | --- | --- |
//! | `explicit_restart` | `restart_server_public_route` | the user-initiated public-route stop+start of the exact server — never a first launch, a host reopen, or another client's restart |
//! | `unexpected_exit` | `terminate_server_process` | the unexpected-exit disposition — an adverse-event classification that never admits `pass`; a clean first launch or a deliberate stop relabeled as unexpected exit fails |
//! | `initialized_new_generation` | `observe_generation_replay` | new-generation initialize/readiness binds the initialize/initialized/buffer-enabled sequence — a bare new PID or process-start event is not initialize |
//! | `document_replay` | `observe_generation_replay` | open-document/root/config replay binds exact replay cardinality — initialize without the required replay never passes |
//! | `current_result` | `hold_release_old_generation_result` | a current result is an answer produced by the current generation — a later correct answer with an old-generation effect still admitted fails |
//! | `old_generation_rejected` | `hold_release_old_generation_result` | a late old-generation publication/effect is rejected, not admitted |
//! | `retry_or_manual_disposition` | `bounded_retry_disposition` | the bounded retry/manual-recovery disposition as observed — a manual restart is never relabeled automatic recovery |
//! | `shutdown_cleanup` | `host_shutdown_while_pending` | shutdown-during-recovery cleanup settles observably — an unsettled or unobserved cleanup stays `not_proven` |
//!
//! The landed cell-ID grammar admits exactly `vim.vim_lsp.<family>.<name>`
//! (two stable reason-token segments) and the #11386 spec's registered names
//! are already in that final convention form, so — unlike the #11388
//! activation family, whose illustrated `<row>.<aspect>` IDs needed a
//! convention-equivalent slug munge — every stage registers under its exact
//! spec name: row == cell, and the denominator laws below bind them.
//!
//! Ownership split — consumed, never duplicated:
//!
//! - [`super`] owns the registration model and cross-catalog laws; this module
//!   owns this family's ledger, denominator mirror, fixture substrate,
//!   vocabulary, cells, and the family laws [`validate_recovery_catalog`]
//!   adds on top.
//! - `crate::vim_lsp_specialized_driver` (#11380) owns the action vocabulary;
//!   the scenario ledger is *derived* from the landed recovery actions, so
//!   the binding cannot drift from the vocabulary.
//! - `.ci/editor-clients/vim-vim-lsp-recovery-root.v1.json` (#11386) owns the
//!   recovery-chain denominator bytes: stage entries, old-generation
//!   requirements, generation kinds, cardinality laws, and honest-claim
//!   rules. The mirror here is checked against that file by tests; a
//!   denominator change is a reviewed edit that changes every affected
//!   digest visibly.
//! - #11376 owns the recovery BDD scenarios and #11378 the recovery
//!   fixtures; both remain pending. Until they land, cells bind the landed
//!   #11380 action vocabulary as scenario owners and the landed #11369
//!   fixture authorities as fixture owners; re-binding is a reviewed edit.
//!
//! Family laws beyond the shared model (all fail-closed):
//!
//! - the ledger mirrors exactly the landed #11380 recovery actions;
//! - the registered cells are exactly the 8 denominator stages: a missing
//!   stage cell, a duplicate stage registration, or a cell outside the finite
//!   #11386 denominator (a relabeled clean first launch, a host-reopen row,
//!   an invented stage) is rejected;
//! - every cell binds the five `generation.*` dimensions of the artifact's
//!   generation kinds plus `recovery.old_generation.required` — a receipt
//!   must name the old/new process, host, document, root, and config
//!   generations, so a clean first launch can never pose as recovery;
//! - every cell binds exactly one `recovery.row.*` dimension that matches its
//!   own cell-ID stage, its row's `recovery.entry.*` entry condition, its
//!   row's `recovery.cardinality.*` cardinality law, and its row's
//!   `recovery.row_binding.*` authority identity (a sha256 over every #11386
//!   denominator field), so one stage's observation cannot inherit another
//!   stage's identity and a denominator edit of any authority field is
//!   digest-visible;
//! - the initialize-sequence stages bind the artifact's mirrored
//!   initialize/initialized/buffer-enabled sequence dimension, so process
//!   spawn without initialize/readiness cannot satisfy them;
//! - each stage is classified by its one pinned #11380 action, so a
//!   disposition observation can never classify an initialize proposition,
//!   and each stage's complete scenario-owner set is pinned to its declared
//!   entry paths, so the crash/restart owners that distinguish a stage from
//!   a clean first launch cannot be dropped or widened;
//! - the adverse-exit stage never admits `pass` (an unexpected exit is never
//!   a passing recovery observation), and the exit/retry stages keep
//!   `manual_restart_required` (a manual-restart client cannot be relabeled
//!   automatic recovery by dropping the honest disposition);
//! - each stage's allowed result set is pinned (the stage dispositions cannot
//!   stand in for each other), every cell admits `fail` and `not_proven`;
//! - cells citing the public-route stop or the host-shutdown actions require
//!   cleanup evidence, so recovery stages cannot contaminate each other
//!   silently;
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

pub const RECOVERY_CATALOG_ID: &str = "vim_lsp_recovery";
pub const RECOVERY_LEDGER_ID: &str = "vim.vim_lsp.specialized.recovery.v1";

/// Fixture substrate: the landed #11369 authorities plus the #11386
/// recovery-root denominator this family binds until #11376/#11378 land
/// their owning surfaces. Tests verify each ID resolves to
/// `.ci/editor-clients/<id>.json`, so an absent authority fails closed.
pub const RECOVERY_FIXTURE_SUBSTRATE: &[&str] = &[
    "vim-vim-lsp-recovery-root.v1",
    "vim-vim-lsp-configuration.v1",
    "vim-vim-lsp-public-surface.v1",
    "vim-vim-lsp-subject.v1",
];

/// Recovery dispositions (#11386): exactly the spec's preserved disposition
/// set. `manual_restart_required` is the family-level token naming the honest
/// adverse-path disposition — never automatic recovery.
pub const RECOVERY_RESULT_VOCABULARY: &[&str] = &[
    "pass",
    "fail",
    "partial",
    "manual_restart_required",
    "client_not_exposed",
    "unsupported",
    "not_proven",
];

pub const RECOVERY_LIMITATION_VOCABULARY: &[&str] = &[
    "manual_restart_required",
    "client_not_exposed",
    "not_proven",
    "replay_currentness_incomplete",
    "observation_incomplete",
    "instrument_incomplete",
];

const RECOVERY_PROFILE: &str = "vim_first_class_exact_source";
const CELL_PREFIX: &str = "vim.vim_lsp.recovery.";
const ROW_DIMENSION_PREFIX: &str = "recovery.row.";
const ENTRY_DIMENSION_PREFIX: &str = "recovery.entry.";
const CARDINALITY_DIMENSION_PREFIX: &str = "recovery.cardinality.";
const ROW_BINDING_PREFIX: &str = "recovery.row_binding.";
const OLD_GENERATION_DIMENSION: &str = "recovery.old_generation.required";
const GENERATION_DIMENSION_PREFIX: &str = "generation.";

/// The five generation kinds every recovery cell binds old/new (the #11386
/// required binding: process, host, document, root, config generations),
/// mirrored from the artifact's `generations.kinds` array.
pub const GENERATION_KINDS: &[&str] = &["process", "host", "document", "root", "config"];

/// The initialize/readiness sequence a new generation must complete before a
/// recovery observation may classify beyond `not_proven`, mirrored from the
/// artifact's `generations.initialize_sequence`.
pub const INITIALIZE_SEQUENCE: &[&str] = &["initialize", "initialized", "buffer_enabled"];

/// Dimensions every recovery cell must bind: the pinned client/server/stage
/// identity plus the old-generation requirement of the whole chain.
const REQUIRED_DIMENSIONS: &[&str] =
    &["client.pinned_commit", "server.executable_identity", "stage.exact_source_local"];

/// One row of the finite #11386 recovery-chain denominator, mirrored from
/// `.ci/editor-clients/vim-vim-lsp-recovery-root.v1.json` in artifact order.
/// Tests check every field against the artifact, so the mirror cannot drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecoveryDenominatorRow {
    /// Verbatim artifact `stage` identity; also the cell-ID name segment.
    pub stage_id: &'static str,
    /// Verbatim artifact entry condition (how this chain stage is entered).
    pub entry: &'static str,
    /// Verbatim artifact old-generation requirement: recovery observations
    /// always need an old generation — a clean first launch cannot pose as
    /// any stage of this chain.
    pub old_generation: bool,
    /// Verbatim artifact cardinality law (initialize/replay/current-result
    /// cardinality this stage binds).
    pub cardinality: &'static str,
    /// Verbatim artifact honest-disposition shape of the stage.
    pub disposition: &'static str,
}

/// The finite #11386-backed denominator: the 8 recovery-chain stages in
/// artifact order. A cell outside this set cannot register (family law), and
/// each stage registers exactly one cell, so the catalog carries 8 cells.
pub const RECOVERY_DENOMINATOR: &[RecoveryDenominatorRow] = &[
    RecoveryDenominatorRow {
        stage_id: "explicit_restart",
        entry: "user_public_route",
        old_generation: true,
        cardinality: "new_generation_initialized_once",
        disposition: "restart_executed_via_public_route",
    },
    RecoveryDenominatorRow {
        stage_id: "unexpected_exit",
        entry: "unexpected_process_exit",
        old_generation: true,
        cardinality: "exit_disposition_bounded_once",
        disposition: "exit_classified_never_first_launch",
    },
    RecoveryDenominatorRow {
        stage_id: "initialized_new_generation",
        entry: "generation_replacement",
        old_generation: true,
        cardinality: "new_generation_initialized_once",
        disposition: "initialize_readiness_not_bare_pid",
    },
    RecoveryDenominatorRow {
        stage_id: "document_replay",
        entry: "generation_replacement",
        old_generation: true,
        cardinality: "open_documents_root_config_replayed_exact",
        disposition: "replay_cardinality_observed",
    },
    RecoveryDenominatorRow {
        stage_id: "current_result",
        entry: "generation_replacement",
        old_generation: true,
        cardinality: "current_result_from_current_generation_only",
        disposition: "current_answer_verified_current_generation",
    },
    RecoveryDenominatorRow {
        stage_id: "old_generation_rejected",
        entry: "generation_replacement",
        old_generation: true,
        cardinality: "old_generation_effect_rejected",
        disposition: "old_effect_rejected_not_admitted",
    },
    RecoveryDenominatorRow {
        stage_id: "retry_or_manual_disposition",
        entry: "recovery_disposition",
        old_generation: true,
        cardinality: "retry_bounded_or_manual_classified",
        disposition: "manual_restart_never_relabeled_automatic",
    },
    RecoveryDenominatorRow {
        stage_id: "shutdown_cleanup",
        entry: "host_shutdown_while_pending",
        old_generation: true,
        cardinality: "cleanup_settled_once",
        disposition: "cleanup_settled_during_pending_recovery",
    },
];

const STOP_SERVER: &str = "vim.vim_lsp.specialized.recovery.stop_server_public_route";
const RESTART_SERVER: &str = "vim.vim_lsp.specialized.recovery.restart_server_public_route";
const TERMINATE_SERVER: &str = "vim.vim_lsp.specialized.recovery.terminate_server_process";
const OBSERVE_GENERATION_REPLAY: &str =
    "vim.vim_lsp.specialized.recovery.observe_generation_replay";
const HOLD_RELEASE_OLD_GENERATION: &str =
    "vim.vim_lsp.specialized.recovery.hold_release_old_generation_result";
const BOUNDED_RETRY_DISPOSITION: &str =
    "vim.vim_lsp.specialized.recovery.bounded_retry_disposition";
const HOST_SHUTDOWN_WHILE_PENDING: &str =
    "vim.vim_lsp.specialized.recovery.host_shutdown_while_pending";

/// The classifying action of one denominator stage (`RECOVERY_DENOMINATOR`
/// order): the validator enforces the mapping, not only the factory, so a
/// disposition observation can never classify an initialize proposition even
/// through a reviewed row edit.
const STAGE_OBSERVATION_CLASSES: &[&str] = &[
    RESTART_SERVER,
    TERMINATE_SERVER,
    OBSERVE_GENERATION_REPLAY,
    OBSERVE_GENERATION_REPLAY,
    HOLD_RELEASE_OLD_GENERATION,
    HOLD_RELEASE_OLD_GENERATION,
    BOUNDED_RETRY_DISPOSITION,
    HOST_SHUTDOWN_WHILE_PENDING,
];

/// Scenario owners of one denominator stage (`RECOVERY_DENOMINATOR` order):
/// the classifying action plus the chain actions that must be citable for the
/// stage's proposition to be meaningful.
const STAGE_SCENARIO_OWNERS: &[&[&str]] = &[
    &[STOP_SERVER, RESTART_SERVER],
    &[TERMINATE_SERVER],
    &[RESTART_SERVER, TERMINATE_SERVER, OBSERVE_GENERATION_REPLAY],
    &[TERMINATE_SERVER, OBSERVE_GENERATION_REPLAY],
    &[OBSERVE_GENERATION_REPLAY, HOLD_RELEASE_OLD_GENERATION],
    &[HOLD_RELEASE_OLD_GENERATION],
    &[TERMINATE_SERVER, BOUNDED_RETRY_DISPOSITION],
    &[HOST_SHUTDOWN_WHILE_PENDING],
];

/// Instrument/reporting/cleanup evidence of one denominator stage
/// (`RECOVERY_DENOMINATOR` order).
const STAGE_INSTRUMENT: &[&[InstrumentEvidence]] = &[
    &[
        InstrumentEvidence::CapabilitySnapshot,
        InstrumentEvidence::CleanupObservation,
        InstrumentEvidence::ClientLog,
        InstrumentEvidence::DriverOutput,
        InstrumentEvidence::ProcessLedger,
    ],
    &[
        InstrumentEvidence::ClientLog,
        InstrumentEvidence::DriverOutput,
        InstrumentEvidence::FailureDiagnostics,
        InstrumentEvidence::ProcessLedger,
    ],
    &[
        InstrumentEvidence::CapabilitySnapshot,
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
        InstrumentEvidence::CapabilitySnapshot,
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
        InstrumentEvidence::ClientLog,
        InstrumentEvidence::DriverOutput,
        InstrumentEvidence::FailureDiagnostics,
        InstrumentEvidence::ProcessLedger,
    ],
    &[
        InstrumentEvidence::CleanupObservation,
        InstrumentEvidence::ClientLog,
        InstrumentEvidence::DriverOutput,
        InstrumentEvidence::ProcessLedger,
    ],
];

/// The pinned result set of one denominator stage (`RECOVERY_DENOMINATOR`
/// order). Every stage admits honest failure and honest incompleteness; the
/// adverse-exit stage admits no `pass`; the exit/retry stages keep
/// `manual_restart_required`.
const AFFIRMING_RESULTS: &[&str] =
    &["pass", "fail", "partial", "client_not_exposed", "unsupported", "not_proven"];
const ADVERSE_EXIT_RESULTS: &[&str] = &[
    "fail",
    "partial",
    "manual_restart_required",
    "client_not_exposed",
    "unsupported",
    "not_proven",
];
const RETRY_DISPOSITION_RESULTS: &[&str] = &[
    "pass",
    "fail",
    "partial",
    "manual_restart_required",
    "client_not_exposed",
    "unsupported",
    "not_proven",
];

const STAGE_RESULTS: &[&[&str]] = &[
    AFFIRMING_RESULTS,
    ADVERSE_EXIT_RESULTS,
    AFFIRMING_RESULTS,
    AFFIRMING_RESULTS,
    AFFIRMING_RESULTS,
    AFFIRMING_RESULTS,
    RETRY_DISPOSITION_RESULTS,
    AFFIRMING_RESULTS,
];

/// The stages whose proposition is the adverse exit disposition: an
/// unexpected exit is never a passing recovery observation, so `pass` is
/// forbidden there no matter what a reviewed row edit claims.
const PASS_FORBIDDEN_STAGES: &[&str] = &["unexpected_exit"];

/// The stages that must keep `manual_restart_required` expressible: a
/// manual-restart client cannot be relabeled automatic recovery by dropping
/// the honest disposition from the exit or retry stage.
const MANUAL_DISPOSITION_STAGES: &[&str] = &["unexpected_exit", "retry_or_manual_disposition"];

/// Actions whose citation makes cleanup evidence independently load-bearing:
/// the public-route stop and the host-shutdown-while-pending actions settle
/// process exit and cleanup, so cells citing them cannot drop that evidence.
const CLEANUP_REQUIRING_OWNERS: &[&str] = &[STOP_SERVER, HOST_SHUTDOWN_WHILE_PENDING];

/// The cardinality law that binds the artifact's initialize sequence, so the
/// stages a new generation must initialize through stay pinned.
const INITIALIZE_CARDINALITY: &str = "new_generation_initialized_once";

const BASE_CLAIM_CEILING: &str = "registration only: pre-registers one exact-subject Vim/vim-lsp server-generation recovery cell for the generic editor_client_compat.v1 receipt, keyed by the finite #11386 recovery-root denominator; binds landed #11380 action owners and #11369 fixtures until #11376/#11378 land their owning surfaces; proves no host recovery behavior and awards no support profile";
const EXPLICIT_RESTART_CLAIM_CEILING: &str = "registration only: an explicit restart is the user-initiated public-route stop+start of the exact server; a first launch, a host reopen, or another client's restart can never satisfy it";
const UNEXPECTED_EXIT_CLAIM_CEILING: &str = "registration only: the unexpected-exit disposition is an adverse-event classification, never a passing recovery observation; a clean first launch or a deliberate stop relabeled as unexpected exit fails";
const INITIALIZED_CLAIM_CEILING: &str = "registration only: new-generation initialize/readiness binds the initialize/initialized/buffer-enabled sequence; a bare new PID or process-start event is not initialize";
const REPLAY_CLAIM_CEILING: &str = "registration only: open-document/root/config replay binds exact replay cardinality; initialize without the required replay never passes this cell";
const CURRENT_RESULT_CLAIM_CEILING: &str = "registration only: a current result is an answer produced by the current generation; a later correct answer with an old-generation effect still admitted fails, and a clean-launch answer is not a recovery result";
const OLD_REJECTION_CLAIM_CEILING: &str = "registration only: a late old-generation publication/effect must be rejected; admitting it while later answers look correct fails";
const RETRY_CLAIM_CEILING: &str = "registration only: the bounded retry/manual-recovery disposition is classified as observed; a manual restart is never relabeled automatic recovery";
const SHUTDOWN_CLAIM_CEILING: &str = "registration only: shutdown-during-recovery cleanup settles process exit and cleanup observably; an unsettled or unobserved cleanup stays not_proven";

const STAGE_CLAIM_CEILINGS: &[&str] = &[
    EXPLICIT_RESTART_CLAIM_CEILING,
    UNEXPECTED_EXIT_CLAIM_CEILING,
    INITIALIZED_CLAIM_CEILING,
    REPLAY_CLAIM_CEILING,
    CURRENT_RESULT_CLAIM_CEILING,
    OLD_REJECTION_CLAIM_CEILING,
    RETRY_CLAIM_CEILING,
    SHUTDOWN_CLAIM_CEILING,
];

const HEX: &[u8; 16] = b"0123456789abcdef";

/// The stable authority identity of one denominator row: a sha256 over every
/// authority field the #11386 artifact carries for the row (stage, entry,
/// old-generation requirement, cardinality law, disposition shape). Every
/// cell of the row binds it as a `recovery.row_binding.sha256-<hex>`
/// dimension, so an artifact edit of *any* row field changes that row's cell
/// digest and the catalog digest: denominator edits stay digest-visible,
/// never silent.
pub fn row_binding_identity(row: &RecoveryDenominatorRow) -> String {
    let canonical = format!(
        "stage={}|entry={}|old_generation={}|cardinality={}|disposition={}",
        row.stage_id, row.entry, row.old_generation, row.cardinality, row.disposition,
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
fn row_by_stage(stage: &str) -> Option<(usize, &'static RecoveryDenominatorRow)> {
    RECOVERY_DENOMINATOR.iter().enumerate().find(|(_, row)| row.stage_id == stage)
}

/// The row-scoped dimensions every recovery cell binds: its denominator stage
/// identity, its row's artifact entry condition, its row's cardinality law,
/// and the row's full authority identity (see [`row_binding_identity`]), so a
/// receipt must name the exact chain stage, cannot inherit another stage's
/// entry or cardinality, and a denominator edit of any authority field is
/// digest-visible.
fn row_dimensions(row: &RecoveryDenominatorRow) -> Vec<String> {
    vec![
        format!("{}{}", ROW_DIMENSION_PREFIX, row.stage_id),
        format!("{}{}", ENTRY_DIMENSION_PREFIX, row.entry),
        format!("{}{}", CARDINALITY_DIMENSION_PREFIX, row.cardinality),
        row_binding_identity(row),
    ]
}

/// Build one recovery cell for a denominator stage.
fn build_cell(
    row: &RecoveryDenominatorRow,
    index: usize,
    subject: CellSubject,
) -> CellRegistration {
    let mut dimensions =
        REQUIRED_DIMENSIONS.iter().map(|value| value.to_string()).collect::<Vec<_>>();
    for kind in GENERATION_KINDS {
        dimensions.push(format!("{GENERATION_DIMENSION_PREFIX}{kind}"));
    }
    dimensions.push(OLD_GENERATION_DIMENSION.to_string());
    dimensions.extend(row_dimensions(row));
    // The initialize-sequence stages bind the artifact's mirrored sequence,
    // so process spawn without initialize/readiness cannot satisfy them.
    if row.cardinality == INITIALIZE_CARDINALITY {
        dimensions.push(format!("recovery.initialize_sequence.{}", INITIALIZE_SEQUENCE.join("_")));
    }
    CellRegistration {
        cell_id: format!("{CELL_PREFIX}{}", row.stage_id),
        cell_version: 1,
        scenario_owners: STAGE_SCENARIO_OWNERS[index]
            .iter()
            .map(|value| value.to_string())
            .collect(),
        fixture_owners: RECOVERY_FIXTURE_SUBSTRATE.iter().map(|value| value.to_string()).collect(),
        subject,
        observation_class: STAGE_OBSERVATION_CLASSES[index].to_string(),
        subject_dimensions: dimensions,
        instrument_evidence: STAGE_INSTRUMENT[index].to_vec(),
        allowed_stages: vec![EvidenceStage::ExactSourceLocal],
        allowed_results: STAGE_RESULTS[index].iter().map(|value| value.to_string()).collect(),
        allowed_limitations: RECOVERY_LIMITATION_VOCABULARY
            .iter()
            .map(|value| value.to_string())
            .collect(),
        allowed_profiles: vec![RECOVERY_PROFILE.to_string()],
        claim_ceiling: format!("{BASE_CLAIM_CEILING}; {}", STAGE_CLAIM_CEILINGS[index]),
    }
}

/// The server-generation recovery scenario ledger, derived from the landed
/// #11380 recovery action vocabulary: one baseline scenario per action ID,
/// sorted for deterministic aggregation. #11376 owns the BDD scenario ledger;
/// when it lands, this derivation is superseded by a reviewed re-bind.
pub fn recovery_action_ledger() -> ScenarioLedger {
    let mut scenarios: Vec<Scenario> = ACTIONS
        .iter()
        .filter(|action| action.family == ActionFamily::Recovery)
        .map(|action| Scenario { id: action.action_id.to_string(), class: ScenarioClass::Baseline })
        .collect();
    scenarios.sort_by(|left, right| left.id.cmp(&right.id));
    ScenarioLedger {
        ledger_id: RECOVERY_LEDGER_ID.to_string(),
        owning_authority: "#11380 specialized action vocabulary (PR #12204), recovery family; denominator: #11386 vim-vim-lsp-recovery-root.v1; supersedes pending: #11376 owns the BDD scenario ledger, #11378 the fixture/expectation cells"
            .to_string(),
        scenarios,
    }
}

/// The server-generation recovery family catalog registered on this PR
/// (#11386): the finite denominator stages, one exact convention cell each.
pub fn recovery_catalog() -> CellCatalog {
    let subject = super::vim_vim_lsp_subject();
    let mut cells = Vec::with_capacity(RECOVERY_DENOMINATOR.len());
    for (index, row) in RECOVERY_DENOMINATOR.iter().enumerate() {
        cells.push(build_cell(row, index, subject.clone()));
    }
    CellCatalog {
        catalog_id: RECOVERY_CATALOG_ID.to_string(),
        catalog_version: 1,
        ledger_id: RECOVERY_LEDGER_ID.to_string(),
        coverage: CoverageRule::AdditiveFamily,
        fixture_substrate: RECOVERY_FIXTURE_SUBSTRATE
            .iter()
            .map(|value| value.to_string())
            .collect(),
        allowed_stages: vec![EvidenceStage::ExactSourceLocal],
        allowed_result_vocabulary: RECOVERY_RESULT_VOCABULARY
            .iter()
            .map(|value| value.to_string())
            .collect(),
        core_profile: None,
        cells,
    }
}

/// The landed recovery action IDs, as the family's authority set.
fn recovery_action_ids() -> BTreeSet<&'static str> {
    ACTIONS
        .iter()
        .filter(|action| action.family == ActionFamily::Recovery)
        .map(|action| action.action_id)
        .collect()
}

/// Validate the compiled recovery catalog against the family laws.
pub fn validate_family_laws() -> Result<()> {
    validate_recovery_catalog(&recovery_catalog(), &recovery_action_ledger())
}

/// Validate one recovery-shaped catalog against the family laws. Shared-model
/// laws (subject pin, stage bound, duplicate IDs, ledger membership,
/// cross-catalog ownership) run in [`super::validate_registry`]; the laws here
/// are the ones only this family can state.
pub fn validate_recovery_catalog(catalog: &CellCatalog, ledger: &ScenarioLedger) -> Result<()> {
    ensure!(
        catalog.catalog_id == RECOVERY_CATALOG_ID,
        "recovery family catalog must keep its identity {RECOVERY_CATALOG_ID}, found {}",
        catalog.catalog_id
    );
    ensure!(
        catalog.ledger_id == RECOVERY_LEDGER_ID && ledger.ledger_id == RECOVERY_LEDGER_ID,
        "recovery family must bind ledger {RECOVERY_LEDGER_ID}"
    );
    ensure!(
        catalog.coverage == CoverageRule::AdditiveFamily,
        "recovery family catalog is additive, not a baseline-coverage catalog"
    );
    ensure!(
        catalog.core_profile.is_none(),
        "recovery family assigns no core profile; profiles consume cells, catalogs do not assign them"
    );
    ensure!(
        catalog.allowed_stages.len() == 1
            && catalog.allowed_stages[0] == EvidenceStage::ExactSourceLocal,
        "recovery family stage bound is exact_source_local only; an exact-source cell cannot inherit a maintained/public stage"
    );
    let declared: BTreeSet<&str> =
        catalog.allowed_result_vocabulary.iter().map(String::as_str).collect();
    let expected: BTreeSet<&str> = RECOVERY_RESULT_VOCABULARY.iter().copied().collect();
    ensure!(
        declared == expected,
        "recovery result vocabulary drifted from the #11386 dispositions"
    );

    let actions = recovery_action_ids();
    let scenarios: BTreeSet<&str> = ledger.scenarios.iter().map(|s| s.id.as_str()).collect();
    ensure!(
        scenarios == actions,
        "recovery ledger must mirror exactly the landed #11380 recovery actions; ledger has {} rows, vocabulary has {}",
        scenarios.len(),
        actions.len()
    );

    // The registered stages must be exactly the denominator stages.
    let mut registered: BTreeSet<String> = BTreeSet::new();
    let mut covered: BTreeSet<String> = BTreeSet::new();
    for cell in &catalog.cells {
        ensure!(
            cell.cell_id.starts_with(CELL_PREFIX),
            "cell {} is outside the recovery family namespace {CELL_PREFIX}",
            cell.cell_id
        );
        let stage = &cell.cell_id[CELL_PREFIX.len()..];
        let (index, row) = row_by_stage(stage).with_context(|| {
            format!(
                "cell {} names stage {stage} outside the finite #11386 recovery-root denominator; a relabeled first launch, a host-reopen row, or an invented stage cannot register here",
                cell.cell_id
            )
        })?;
        ensure!(
            registered.insert(stage.to_string()),
            "duplicate recovery stage registration {stage}"
        );

        ensure!(
            actions.contains(cell.observation_class.as_str()),
            "cell {} observation class {} is not a landed recovery action; another family's action or an invented token cannot classify a recovery cell",
            cell.cell_id,
            cell.observation_class
        );
        // Each stage is classified by its one pinned action — not merely a
        // landed action the cell cites — so a disposition observation can
        // never classify an initialize proposition even through a reviewed
        // row edit.
        let required_class = STAGE_OBSERVATION_CLASSES[index];
        ensure!(
            cell.observation_class == required_class,
            "cell {} must be classified by {required_class} for stage {stage}, found {}; the wrong recovery proposition cannot satisfy this cell",
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
        // from a clean first launch (for example the restart/termination
        // owners of `initialized_new_generation`) cannot be dropped while the
        // union-coverage law stays satisfied through other cells.
        let expected_owners: BTreeSet<&str> =
            STAGE_SCENARIO_OWNERS[index].iter().copied().collect();
        let declared_owners: BTreeSet<&str> =
            cell.scenario_owners.iter().map(String::as_str).collect();
        ensure!(
            declared_owners == expected_owners,
            "cell {} scenario owners drifted from the pinned {stage} stage owner set; the recovery-entry paths that distinguish this stage from a first launch cannot be dropped or widened",
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
        // host, or provider cannot supply the observation and a clean first
        // launch has no old generation to name.
        for kind in GENERATION_KINDS {
            let dimension = format!("{GENERATION_DIMENSION_PREFIX}{kind}");
            ensure!(
                cell.subject_dimensions.iter().any(|token| token == &dimension),
                "cell {} must bind the generation dimension {dimension} of the #11386 required binding",
                cell.cell_id
            );
        }
        if row.old_generation {
            ensure!(
                cell.subject_dimensions.iter().any(|token| token == OLD_GENERATION_DIMENSION),
                "cell {} must bind {OLD_GENERATION_DIMENSION}; a clean first launch cannot pose as recovery stage {stage}",
                cell.cell_id
            );
        }

        // Row identity: exactly one recovery.row.* dimension, matching the
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
            "cell {} must bind its row's #11386 entry dimension {ENTRY_DIMENSION_PREFIX}{}",
            cell.cell_id,
            row.entry
        );
        ensure!(
            cell.subject_dimensions
                .iter()
                .any(|token| token
                    == &format!("{}{}", CARDINALITY_DIMENSION_PREFIX, row.cardinality)),
            "cell {} must bind its row's #11386 cardinality dimension {CARDINALITY_DIMENSION_PREFIX}{}; the initialize/replay/current-result cardinality cannot be omitted",
            cell.cell_id,
            row.cardinality
        );
        // Row authority identity: exactly one binding dimension, equal to the
        // digest over the row's full #11386 authority content, so denominator
        // edits of any field are digest-visible.
        let binding = row_binding_identity(row);
        let bindings: Vec<&String> = cell
            .subject_dimensions
            .iter()
            .filter(|token| token.starts_with(ROW_BINDING_PREFIX))
            .collect();
        ensure!(
            bindings.len() == 1 && bindings[0].as_str() == binding,
            "cell {} must bind exactly one {ROW_BINDING_PREFIX}* dimension equal to its row's authority identity {binding}; a #11386 denominator edit cannot stay digest-invisible",
            cell.cell_id
        );
        // Initialize-sequence stages bind the mirrored sequence dimension.
        if row.cardinality == INITIALIZE_CARDINALITY {
            let sequence =
                format!("recovery.initialize_sequence.{}", INITIALIZE_SEQUENCE.join("_"));
            ensure!(
                cell.subject_dimensions.iter().any(|token| token == &sequence),
                "cell {} must bind the initialize-sequence dimension {sequence}; process spawn without initialize/readiness cannot satisfy stage {stage}",
                cell.cell_id
            );
        }

        // Adverse-exit honesty: an unexpected exit is never a passing
        // recovery observation.
        if PASS_FORBIDDEN_STAGES.contains(&stage) {
            ensure!(
                !cell.allowed_results.iter().any(|result| result == "pass"),
                "cell {} of adverse-exit stage {stage} admits the recovery-affirming result pass; an unexpected exit, a new PID, or a clean first launch can never be a passing recovery observation",
                cell.cell_id
            );
        }
        // Manual-restart honesty: the exit/retry stages keep the honest
        // manual disposition expressible.
        if MANUAL_DISPOSITION_STAGES.contains(&stage) {
            ensure!(
                cell.allowed_results.iter().any(|result| result == "manual_restart_required"),
                "cell {} must keep the manual_restart_required disposition; a manual restart cannot be relabeled automatic recovery by dropping it",
                cell.cell_id
            );
        }

        // Stage vocabularies are pinned: the chain stages' dispositions
        // cannot stand in for each other.
        let expected_results: BTreeSet<&str> = STAGE_RESULTS[index].iter().copied().collect();
        let declared_results: BTreeSet<&str> =
            cell.allowed_results.iter().map(String::as_str).collect();
        ensure!(
            declared_results == expected_results,
            "cell {} allowed results drifted from the pinned {stage} stage vocabulary of the #11386 recovery chain",
            cell.cell_id
        );

        // Cells citing the public-route stop or the host-shutdown action keep
        // cleanup evidence independently load-bearing.
        for owner in CLEANUP_REQUIRING_OWNERS {
            if cell.scenario_owners.iter().any(|token| token == owner) {
                ensure!(
                    cell.instrument_evidence.contains(&InstrumentEvidence::CleanupObservation),
                    "cell {} cites the cleanup-settling action {owner} and must require cleanup evidence; recovery stages cannot contaminate each other silently",
                    cell.cell_id
                );
            }
        }

        ensure!(
            cell.allowed_profiles.len() == 1 && cell.allowed_profiles[0] == RECOVERY_PROFILE,
            "cell {} may feed only {RECOVERY_PROFILE}",
            cell.cell_id
        );
        covered.extend(cell.scenario_owners.iter().cloned());
    }

    let expected_stages: BTreeSet<String> =
        RECOVERY_DENOMINATOR.iter().map(|row| row.stage_id.to_string()).collect();
    let missing: Vec<String> = expected_stages.difference(&registered).cloned().collect();
    ensure!(
        missing.is_empty(),
        "denominator stage cells missing from the #11386 recovery family: {missing:?}"
    );

    let uncovered: Vec<&str> =
        actions.iter().filter(|action| !covered.contains(**action)).copied().collect();
    ensure!(
        uncovered.is_empty(),
        "landed recovery actions without a pre-registered cell: {uncovered:?}"
    );
    Ok(())
}
