//! The #11384 format-on-save family cell catalog.
//!
//! Seven pre-registered cells carry the save-triggered formatting
//! propositions for the pinned Vim + vim-lsp + perllsp subject:
//!
//! | Cell | Classifying action | Proposition |
//! | --- | --- | --- |
//! | `vim.vim_lsp.save.route` | `observe_save_settlement` | the selected save trigger/owner/route |
//! | `vim.vim_lsp.save.invocation_cardinality` | `observe_save_settlement` | one save → one formatting invocation |
//! | `vim.vim_lsp.save.format_applied` | `observe_save_settlement` | an applied save-triggered formatting effect |
//! | `vim.vim_lsp.save.format_no_change` | `observe_save_settlement` | legitimate no-change over an executed save route |
//! | `vim.vim_lsp.save.disabled_or_refused` | `observe_save_settlement` | disabled/refused disposition, incl. configuration-only |
//! | `vim.vim_lsp.save.failure` | `observe_save_settlement` | a formatting failure honestly recorded (never `pass`) |
//! | `vim.vim_lsp.save.stale_result_rejected` | `hold_release_stale_result` | a stale/cancelled result is never applied |
//!
//! Ownership split — consumed, never duplicated:
//!
//! - [`super`] owns the registration model and cross-catalog laws; this module
//!   owns this family's ledger, fixture substrate, vocabulary, cells, and the
//!   family laws [`validate_save_catalog`] adds on top.
//! - `crate::vim_lsp_specialized_driver` (#11380) owns the action vocabulary
//!   and the save false-subject laws: a manual comparator run can never label
//!   itself save-triggered, duplicate owners are not observable as a pass, and
//!   a pre/post digest settlement must observe exactly one configured owner.
//!   This catalog mirrors those laws structurally: the comparator action is
//!   pinned as a control that no save cell may cite as a scenario owner.
//! - #11376 owns the save BDD scenario ledger and #11378 the semantic
//!   fixture/expectation cells; both remain open (`needs-spec`). Until they
//!   land, cells bind the landed #11380 action vocabulary as scenario owners
//!   and the landed #11369 fixture authorities as fixture owners; re-binding
//!   is a reviewed edit that changes every digest visibly.
//!
//! Family laws beyond the shared model (all fail-closed):
//!
//! - the ledger mirrors exactly the landed #11380 save-format actions;
//! - `manual_comparator` is a control: it may never appear as a scenario owner
//!   of a save cell, so manual explicit formatting cannot satisfy this family
//!   even at the registration layer;
//! - every cell's observation class is a landed save-format action and one of
//!   the cell's own scenario owners;
//! - every cell admits `fail` and `not_proven`; the failure cell admits no
//!   `pass` (a clean save is the applied/no-change cells' proposition);
//! - every cell binds `client.pinned_commit`, `server.executable_identity`,
//!   `stage.exact_source_local`, and at least one `generation.*` dimension;
//! - the stage bound is `exact_source_local` only, the vocabulary is exactly
//!   the declared save dispositions, every cell feeds only
//!   `vim_first_class_exact_source`, and the union of scenario owners covers
//!   every landed save-format action except the pinned control.

use anyhow::{Result, ensure};
use std::collections::BTreeSet;

use super::{
    CellCatalog, CellRegistration, CellSubject, CoverageRule, InstrumentEvidence, Scenario,
    ScenarioClass, ScenarioLedger,
};
use crate::editor_client_compat::EvidenceStage;
use crate::vim_lsp_specialized_driver::{ACTIONS, ActionFamily};

pub const SAVE_CATALOG_ID: &str = "vim_lsp_save";
pub const SAVE_LEDGER_ID: &str = "vim.vim_lsp.specialized.save_format.v1";

/// Fixture substrate: the landed #11369 authorities this family binds until
/// #11378 lands its semantic fixture/expectation cells.
pub const SAVE_FIXTURE_SUBSTRATE: &[&str] =
    &["vim-vim-lsp-configuration.v1", "vim-vim-lsp-public-surface.v1", "vim-vim-lsp-subject.v1"];

/// Save dispositions (#11384). Beyond the receipt-serializable generic set,
/// `client_not_exposed`/`configuration_only` are family-level result tokens
/// naming client exposure and configuration-only routes; their receipt
/// mapping is owned by the emit-time slice (#7778 runner / host leaf).
pub const SAVE_RESULT_VOCABULARY: &[&str] = &[
    "pass",
    "fail",
    "partial",
    "client_not_exposed",
    "configuration_only",
    "unsupported",
    "not_proven",
];

pub const SAVE_LIMITATION_VOCABULARY: &[&str] = &[
    "client_not_exposed",
    "configuration_only",
    "not_proven",
    "observation_incomplete",
    "instrument_incomplete",
];

const SAVE_PROFILE: &str = "vim_first_class_exact_source";
const CELL_PREFIX: &str = "vim.vim_lsp.save.";

/// The comparator action: a manual explicit format run exists in the
/// vocabulary as a control only. It can never be a scenario owner of a save
/// cell, so "correct final bytes from the wrong trigger" cannot satisfy this
/// family even before the driver's trigger law runs.
const CONTROL_ACTIONS: &[&str] = &["vim.vim_lsp.specialized.save_format.manual_comparator"];

/// Dimensions every save cell must bind.
const REQUIRED_DIMENSIONS: &[&str] =
    &["client.pinned_commit", "server.executable_identity", "stage.exact_source_local"];
const GENERATION_PREFIX: &str = "generation.";

const SETTLEMENT_FIXTURES: &[&str] =
    &["vim-vim-lsp-configuration.v1", "vim-vim-lsp-public-surface.v1", "vim-vim-lsp-subject.v1"];
const HOLD_RELEASE_FIXTURES: &[&str] = &["vim-vim-lsp-public-surface.v1", "vim-vim-lsp-subject.v1"];

const ROUTE_DIMENSIONS: &[&str] = &[
    "client.pinned_commit",
    "generation.config",
    "save.owner",
    "save.trigger",
    "server.executable_identity",
    "stage.exact_source_local",
];
const CARDINALITY_DIMENSIONS: &[&str] = &[
    "client.pinned_commit",
    "generation.document",
    "save.cardinality",
    "save.owner",
    "save.trigger",
    "server.executable_identity",
    "stage.exact_source_local",
];
const APPLIED_DIMENSIONS: &[&str] = &[
    "client.pinned_commit",
    "generation.document",
    "generation.source",
    "save.owner",
    "save.trigger",
    "server.executable_identity",
    "stage.exact_source_local",
];
const DISABLED_DIMENSIONS: &[&str] = &[
    "client.pinned_commit",
    "generation.config",
    "save.owner",
    "save.trigger",
    "server.executable_identity",
    "stage.exact_source_local",
];
const FAILURE_DIMENSIONS: &[&str] = &[
    "client.pinned_commit",
    "generation.document",
    "save.owner",
    "save.trigger",
    "server.executable_identity",
    "stage.exact_source_local",
];
const STALE_RESULT_DIMENSIONS: &[&str] = &[
    "client.pinned_commit",
    "generation.document",
    "generation.source",
    "save.owner",
    "save.trigger",
    "server.executable_identity",
    "stage.exact_source_local",
];

const SETTLEMENT_INSTRUMENT: &[InstrumentEvidence] = &[
    InstrumentEvidence::CapabilitySnapshot,
    InstrumentEvidence::ClientLog,
    InstrumentEvidence::DriverOutput,
    InstrumentEvidence::ProcessLedger,
];
const STALE_INSTRUMENT: &[InstrumentEvidence] = &[
    InstrumentEvidence::CleanupObservation,
    InstrumentEvidence::ClientLog,
    InstrumentEvidence::DriverOutput,
    InstrumentEvidence::ProcessLedger,
];

const BASE_CLAIM_CEILING: &str = "registration only: pre-registers one exact-subject Vim/vim-lsp save-triggered formatting cell for the generic editor_client_compat.v1 receipt; binds landed #11380 action owners and #11369 fixtures until #11376/#11378 land their owning surfaces; manual explicit formatting and matching final bytes from another owner can never satisfy it; proves no host save/format behavior and awards no support profile";
const FAILURE_CLAIM_CEILING: &str = "registration only: the failure cell never carries pass; a clean save belongs to the applied/no-change cells and a failure disposition records the failure honestly";
const STALE_CLAIM_CEILING: &str = "registration only: a stale or cancelled save-format result must be rejected, never applied; cleanup evidence stays independently load-bearing";

const OBSERVE_SETTLEMENT: &str = "vim.vim_lsp.specialized.save_format.observe_save_settlement";
const HOLD_RELEASE_STALE: &str = "vim.vim_lsp.specialized.save_format.hold_release_stale_result";
const CONFIGURE_OWNER: &str = "vim.vim_lsp.specialized.save_format.configure_single_owner";
const ORDINARY_WRITE: &str = "vim.vim_lsp.specialized.save_format.ordinary_write";

/// Declaration shape for one save row; [`build`] fills the pinned subject,
/// version, stage, limitations, and profile every save cell shares.
struct CellSpec<'a> {
    cell_id: &'a str,
    scenario_owners: &'a [&'a str],
    fixture_owners: &'a [&'a str],
    observation_class: &'a str,
    subject_dimensions: &'a [&'a str],
    instrument_evidence: &'a [InstrumentEvidence],
    allowed_results: &'a [&'a str],
    claim_ceiling: &'a str,
}

fn build(spec: CellSpec<'_>, subject: CellSubject) -> CellRegistration {
    CellRegistration {
        cell_id: spec.cell_id.to_string(),
        cell_version: 1,
        scenario_owners: spec.scenario_owners.iter().map(|value| value.to_string()).collect(),
        fixture_owners: spec.fixture_owners.iter().map(|value| value.to_string()).collect(),
        subject,
        observation_class: spec.observation_class.to_string(),
        subject_dimensions: spec.subject_dimensions.iter().map(|value| value.to_string()).collect(),
        instrument_evidence: spec.instrument_evidence.to_vec(),
        allowed_stages: vec![EvidenceStage::ExactSourceLocal],
        allowed_results: spec.allowed_results.iter().map(|value| value.to_string()).collect(),
        allowed_limitations: SAVE_LIMITATION_VOCABULARY
            .iter()
            .map(|value| value.to_string())
            .collect(),
        allowed_profiles: vec![SAVE_PROFILE.to_string()],
        claim_ceiling: spec.claim_ceiling.to_string(),
    }
}

/// The save-format scenario ledger, derived from the landed #11380
/// save-format action vocabulary (sorted by scenario ID for deterministic
/// aggregation). #11376 owns the BDD scenario ledger; when it lands, this
/// derivation is superseded by a reviewed re-bind.
pub fn save_action_ledger() -> ScenarioLedger {
    let mut scenarios: Vec<Scenario> = ACTIONS
        .iter()
        .filter(|action| action.family == ActionFamily::SaveFormat)
        .map(|action| Scenario { id: action.action_id.to_string(), class: ScenarioClass::Baseline })
        .collect();
    scenarios.sort_by(|left, right| left.id.cmp(&right.id));
    ScenarioLedger {
        ledger_id: SAVE_LEDGER_ID.to_string(),
        owning_authority: "#11380 specialized action vocabulary (PR #12204), save_format family; supersedes pending: #11376 owns the BDD scenario ledger, #11378 the fixture/expectation cells"
            .to_string(),
        scenarios,
    }
}

/// The save-format family catalog registered on this PR (#11384).
pub fn save_catalog() -> CellCatalog {
    let subject = super::vim_vim_lsp_subject();
    CellCatalog {
        catalog_id: SAVE_CATALOG_ID.to_string(),
        catalog_version: 1,
        ledger_id: SAVE_LEDGER_ID.to_string(),
        coverage: CoverageRule::AdditiveFamily,
        fixture_substrate: SAVE_FIXTURE_SUBSTRATE.iter().map(|value| value.to_string()).collect(),
        allowed_stages: vec![EvidenceStage::ExactSourceLocal],
        allowed_result_vocabulary: SAVE_RESULT_VOCABULARY
            .iter()
            .map(|value| value.to_string())
            .collect(),
        core_profile: None,
        cells: vec![
            build(
                CellSpec {
                    cell_id: "vim.vim_lsp.save.route",
                    scenario_owners: &[CONFIGURE_OWNER, ORDINARY_WRITE, OBSERVE_SETTLEMENT],
                    fixture_owners: SETTLEMENT_FIXTURES,
                    observation_class: OBSERVE_SETTLEMENT,
                    subject_dimensions: ROUTE_DIMENSIONS,
                    instrument_evidence: SETTLEMENT_INSTRUMENT,
                    allowed_results: SAVE_RESULT_VOCABULARY,
                    claim_ceiling: BASE_CLAIM_CEILING,
                },
                subject.clone(),
            ),
            build(
                CellSpec {
                    cell_id: "vim.vim_lsp.save.invocation_cardinality",
                    scenario_owners: &[ORDINARY_WRITE, OBSERVE_SETTLEMENT],
                    fixture_owners: SETTLEMENT_FIXTURES,
                    observation_class: OBSERVE_SETTLEMENT,
                    subject_dimensions: CARDINALITY_DIMENSIONS,
                    instrument_evidence: SETTLEMENT_INSTRUMENT,
                    allowed_results: &[
                        "pass",
                        "fail",
                        "partial",
                        "client_not_exposed",
                        "unsupported",
                        "not_proven",
                    ],
                    claim_ceiling: BASE_CLAIM_CEILING,
                },
                subject.clone(),
            ),
            build(
                CellSpec {
                    cell_id: "vim.vim_lsp.save.format_applied",
                    scenario_owners: &[CONFIGURE_OWNER, ORDINARY_WRITE, OBSERVE_SETTLEMENT],
                    fixture_owners: SETTLEMENT_FIXTURES,
                    observation_class: OBSERVE_SETTLEMENT,
                    subject_dimensions: APPLIED_DIMENSIONS,
                    instrument_evidence: SETTLEMENT_INSTRUMENT,
                    allowed_results: &[
                        "pass",
                        "fail",
                        "partial",
                        "client_not_exposed",
                        "unsupported",
                        "not_proven",
                    ],
                    claim_ceiling: BASE_CLAIM_CEILING,
                },
                subject.clone(),
            ),
            build(
                CellSpec {
                    cell_id: "vim.vim_lsp.save.format_no_change",
                    scenario_owners: &[CONFIGURE_OWNER, ORDINARY_WRITE, OBSERVE_SETTLEMENT],
                    fixture_owners: SETTLEMENT_FIXTURES,
                    observation_class: OBSERVE_SETTLEMENT,
                    subject_dimensions: APPLIED_DIMENSIONS,
                    instrument_evidence: SETTLEMENT_INSTRUMENT,
                    allowed_results: &[
                        "pass",
                        "fail",
                        "partial",
                        "client_not_exposed",
                        "unsupported",
                        "not_proven",
                    ],
                    claim_ceiling: BASE_CLAIM_CEILING,
                },
                subject.clone(),
            ),
            build(
                CellSpec {
                    cell_id: "vim.vim_lsp.save.disabled_or_refused",
                    scenario_owners: &[CONFIGURE_OWNER, ORDINARY_WRITE, OBSERVE_SETTLEMENT],
                    fixture_owners: SETTLEMENT_FIXTURES,
                    observation_class: OBSERVE_SETTLEMENT,
                    subject_dimensions: DISABLED_DIMENSIONS,
                    instrument_evidence: SETTLEMENT_INSTRUMENT,
                    allowed_results: SAVE_RESULT_VOCABULARY,
                    claim_ceiling: BASE_CLAIM_CEILING,
                },
                subject.clone(),
            ),
            build(
                CellSpec {
                    cell_id: "vim.vim_lsp.save.failure",
                    scenario_owners: &[ORDINARY_WRITE, OBSERVE_SETTLEMENT],
                    fixture_owners: SETTLEMENT_FIXTURES,
                    observation_class: OBSERVE_SETTLEMENT,
                    subject_dimensions: FAILURE_DIMENSIONS,
                    instrument_evidence: SETTLEMENT_INSTRUMENT,
                    allowed_results: &[
                        "fail",
                        "partial",
                        "client_not_exposed",
                        "unsupported",
                        "not_proven",
                    ],
                    claim_ceiling: FAILURE_CLAIM_CEILING,
                },
                subject.clone(),
            ),
            build(
                CellSpec {
                    cell_id: "vim.vim_lsp.save.stale_result_rejected",
                    scenario_owners: &[HOLD_RELEASE_STALE],
                    fixture_owners: HOLD_RELEASE_FIXTURES,
                    observation_class: HOLD_RELEASE_STALE,
                    subject_dimensions: STALE_RESULT_DIMENSIONS,
                    instrument_evidence: STALE_INSTRUMENT,
                    allowed_results: &[
                        "pass",
                        "fail",
                        "partial",
                        "client_not_exposed",
                        "unsupported",
                        "not_proven",
                    ],
                    claim_ceiling: STALE_CLAIM_CEILING,
                },
                subject,
            ),
        ],
    }
}

/// The landed save-format action IDs, as the family's authority set.
fn save_action_ids() -> BTreeSet<&'static str> {
    ACTIONS
        .iter()
        .filter(|action| action.family == ActionFamily::SaveFormat)
        .map(|action| action.action_id)
        .collect()
}

/// Validate the compiled save-format catalog against the family laws.
pub fn validate_family_laws() -> Result<()> {
    validate_save_catalog(&save_catalog(), &save_action_ledger())
}

/// Validate one save-format-shaped catalog against the family laws. Shared
/// laws run in [`super::validate_registry`]; the laws here are the ones only
/// this family can state.
pub fn validate_save_catalog(catalog: &CellCatalog, ledger: &ScenarioLedger) -> Result<()> {
    ensure!(
        catalog.catalog_id == SAVE_CATALOG_ID,
        "save family catalog must keep its identity {SAVE_CATALOG_ID}, found {}",
        catalog.catalog_id
    );
    ensure!(
        catalog.ledger_id == SAVE_LEDGER_ID && ledger.ledger_id == SAVE_LEDGER_ID,
        "save family must bind ledger {SAVE_LEDGER_ID}"
    );
    ensure!(
        catalog.coverage == CoverageRule::AdditiveFamily,
        "save family catalog is additive, not a baseline-coverage catalog"
    );
    ensure!(
        catalog.core_profile.is_none(),
        "save family assigns no core profile; profiles consume cells, catalogs do not assign them"
    );
    ensure!(
        catalog.allowed_stages.len() == 1
            && catalog.allowed_stages[0] == EvidenceStage::ExactSourceLocal,
        "save family stage bound is exact_source_local only; a maintained/public stage needs its own reviewed stage law"
    );
    let declared: BTreeSet<&str> =
        catalog.allowed_result_vocabulary.iter().map(String::as_str).collect();
    let expected: BTreeSet<&str> = SAVE_RESULT_VOCABULARY.iter().copied().collect();
    ensure!(declared == expected, "save result vocabulary drifted from the #11384 dispositions");

    let actions = save_action_ids();
    let scenarios: BTreeSet<&str> = ledger.scenarios.iter().map(|s| s.id.as_str()).collect();
    ensure!(
        scenarios == actions,
        "save ledger must mirror exactly the landed #11380 save_format actions; ledger has {} rows, vocabulary has {}",
        scenarios.len(),
        actions.len()
    );

    let mut covered: BTreeSet<String> = BTreeSet::new();
    for cell in &catalog.cells {
        ensure!(
            cell.cell_id.starts_with(CELL_PREFIX),
            "cell {} is outside the save family namespace {CELL_PREFIX}",
            cell.cell_id
        );
        ensure!(
            actions.contains(cell.observation_class.as_str()),
            "cell {} observation class {} is not a landed save_format action; another family's action or an invented token cannot classify a save cell",
            cell.cell_id,
            cell.observation_class
        );
        ensure!(
            cell.scenario_owners.contains(&cell.observation_class),
            "cell {} observation class {} must be one of its own scenario owners",
            cell.cell_id,
            cell.observation_class
        );
        for control in CONTROL_ACTIONS {
            ensure!(
                !cell.scenario_owners.iter().any(|owner| owner == control),
                "cell {} cites control action {control}; a manual comparator run can never be save evidence",
                cell.cell_id
            );
        }
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
        ensure!(
            cell.subject_dimensions.iter().any(|token| token.starts_with(GENERATION_PREFIX)),
            "cell {} must bind at least one generation dimension",
            cell.cell_id
        );
        ensure!(
            cell.allowed_profiles.len() == 1 && cell.allowed_profiles[0] == SAVE_PROFILE,
            "cell {} may feed only {SAVE_PROFILE}",
            cell.cell_id
        );
        covered.extend(cell.scenario_owners.iter().cloned());
    }

    // Proposition-specific laws the shared shape cannot state.
    let cell_of = |id: &str| {
        catalog
            .cells
            .iter()
            .find(|cell| cell.cell_id == id)
            .ok_or_else(|| anyhow::anyhow!("save catalog omitted cell {id}"))
    };
    let stale = cell_of("vim.vim_lsp.save.stale_result_rejected")?;
    ensure!(
        stale.subject_dimensions.iter().any(|token| token == "save.trigger")
            && stale.subject_dimensions.iter().any(|token| token == "save.owner"),
        "the stale-result cell must bind the save.trigger and save.owner identities; a held manual or non-save formatting result is not save evidence"
    );
    ensure!(
        stale.instrument_evidence.contains(&InstrumentEvidence::CleanupObservation),
        "the stale-result cell must require cleanup evidence; cleanup stays independently load-bearing and cannot ride a product pass"
    );
    let failure = cell_of("vim.vim_lsp.save.failure")?;
    ensure!(
        !failure.allowed_results.iter().any(|result| result == "pass"),
        "the failure cell must never admit pass; a clean save is the applied/no-change cells' proposition"
    );

    let expected_coverage: BTreeSet<&str> =
        actions.iter().filter(|action| !CONTROL_ACTIONS.contains(action)).copied().collect();
    let uncovered: Vec<&str> =
        expected_coverage.iter().filter(|action| !covered.contains(**action)).copied().collect();
    ensure!(
        uncovered.is_empty(),
        "landed save_format actions without a pre-registered cell: {uncovered:?}"
    );
    Ok(())
}
