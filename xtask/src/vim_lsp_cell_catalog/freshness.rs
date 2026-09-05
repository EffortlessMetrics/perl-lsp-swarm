//! The #11381 freshness family cell catalog.
//!
//! Six pre-registered cells carry the external source/configuration freshness
//! propositions for the pinned Vim + vim-lsp + perllsp subject:
//!
//! | Cell | Classifying action | Proposition |
//! | --- | --- | --- |
//! | `vim.vim_lsp.freshness.route` | `observe_route_and_generation` | which freshness route was selected, and whether explicit reload/restart is part of it |
//! | `vim.vim_lsp.freshness.external_source` | `observe_route_and_generation` | closed external source changes reach semantic results |
//! | `vim.vim_lsp.freshness.project_config` | `observe_route_and_generation` | governed `.perl-lsp.toml` lifecycle/currentness incl. malformed→repair |
//! | `vim.vim_lsp.freshness.client_settings` | `observe_route_and_generation` | Vim/vim-lsp workspace-setting currentness |
//! | `vim.vim_lsp.freshness.stale_generation_rejected` | `hold_release_old_generation` | an old source/config/root-generation result is never promoted current |
//! | `vim.vim_lsp.freshness.provider_ownership` | `observe_route_and_generation` | the selected client/provider/service owns the observed results |
//!
//! Ownership split — consumed, never duplicated:
//!
//! - [`super`] owns the registration model and cross-catalog laws; this module
//!   owns this family's ledger, fixture substrate, vocabulary, cells, and the
//!   family laws [`validate_freshness_catalog`] adds on top.
//! - `crate::vim_lsp_specialized_driver` (#11380) owns the action vocabulary.
//!   This family's scenario ledger is *derived* from the landed freshness
//!   actions (one scenario per action ID), so the binding cannot drift from
//!   the vocabulary and no hand-maintained scenario list exists here.
//! - #11376 owns the freshness BDD scenario ledger and #11378 the semantic
//!   fixture/expectation cells; both remain open (`needs-spec`). Until they
//!   land, cells bind the landed #11380 action vocabulary as scenario owners
//!   and the landed #11369 fixture authorities as fixture owners; re-binding
//!   to #11376/#11378 owners is a reviewed edit that changes every digest
//!   visibly. No cell needing only-#11376/#11378 surfaces is registered here.
//!
//! Family laws beyond the shared model (all fail-closed):
//!
//! - the ledger mirrors exactly the landed #11380 freshness actions — an
//!   invented scenario or a dropped action is rejected;
//! - every cell's observation class is a landed freshness action *and* one of
//!   the cell's own scenario owners, so a watcher/registration/log token or
//!   another family's action can never classify a freshness cell;
//! - every cell admits `fail` and `not_proven` (honest failure and honest
//!   incompleteness are always expressible);
//! - every cell binds `client.pinned_commit`, `server.executable_identity`,
//!   `stage.exact_source_local`, and at least one `generation.*` dimension;
//! - the catalog's stage bound is `exact_source_local` only, its vocabulary is
//!   exactly the declared freshness dispositions, and every cell feeds only
//!   `vim_first_class_exact_source`;
//! - the union of the cells' scenario owners covers every landed freshness
//!   action, so every planned freshness host observation has exactly this
//!   catalog as its pre-registration surface and a host leaf cannot add cells
//!   ad hoc.

use anyhow::{Result, ensure};
use std::collections::BTreeSet;

use super::{
    CellCatalog, CellRegistration, CellSubject, CoverageRule, InstrumentEvidence, Scenario,
    ScenarioClass, ScenarioLedger,
};
use crate::editor_client_compat::EvidenceStage;
use crate::vim_lsp_specialized_driver::{ACTIONS, ActionFamily};

pub const FRESHNESS_CATALOG_ID: &str = "vim_lsp_freshness";
pub const FRESHNESS_LEDGER_ID: &str = "vim.vim_lsp.specialized.freshness.v1";

/// Fixture substrate: the landed #11369 authorities this family binds until
/// #11378 lands its semantic fixture/expectation cells. Tests verify each ID
/// resolves to `.ci/editor-clients/<id>.json`, so an absent authority fails
/// closed.
pub const FRESHNESS_FIXTURE_SUBSTRATE: &[&str] =
    &["vim-vim-lsp-configuration.v1", "vim-vim-lsp-public-surface.v1", "vim-vim-lsp-subject.v1"];

/// Freshness dispositions (#11381). Beyond the receipt-serializable generic
/// set, `explicit_reload_required`/`restart_required`/`client_not_exposed`
/// are family-level result tokens: they name route shape and client exposure,
/// never semantic freshness, and their receipt mapping is owned by the
/// emit-time slice (#7778 runner / host leaf), not by this registration.
pub const FRESHNESS_RESULT_VOCABULARY: &[&str] = &[
    "pass",
    "fail",
    "partial",
    "client_not_exposed",
    "explicit_reload_required",
    "restart_required",
    "unsupported",
    "not_proven",
];

pub const FRESHNESS_LIMITATION_VOCABULARY: &[&str] = &[
    "explicit_reload_required",
    "restart_required",
    "not_proven",
    "observation_incomplete",
    "instrument_incomplete",
];

const FRESHNESS_PROFILE: &str = "vim_first_class_exact_source";
const CELL_PREFIX: &str = "vim.vim_lsp.freshness.";

/// Dimensions every freshness cell must bind: the pinned client/server/stage
/// identity plus at least one tracked generation dimension.
const REQUIRED_DIMENSIONS: &[&str] =
    &["client.pinned_commit", "server.executable_identity", "stage.exact_source_local"];
const GENERATION_PREFIX: &str = "generation.";

const SUBJECT_CONFIG_FIXTURES: &[&str] =
    &["vim-vim-lsp-configuration.v1", "vim-vim-lsp-subject.v1"];
const SUBJECT_SURFACE_FIXTURES: &[&str] =
    &["vim-vim-lsp-public-surface.v1", "vim-vim-lsp-subject.v1"];
const ALL_THREE_FIXTURES: &[&str] =
    &["vim-vim-lsp-configuration.v1", "vim-vim-lsp-public-surface.v1", "vim-vim-lsp-subject.v1"];

const ROUTE_DIMENSIONS: &[&str] = &[
    "client.pinned_commit",
    "generation.config",
    "generation.root",
    "generation.source",
    "route.selection",
    "server.executable_identity",
    "stage.exact_source_local",
];
const EXTERNAL_SOURCE_DIMENSIONS: &[&str] = &[
    "client.pinned_commit",
    "generation.document",
    "generation.source",
    "server.executable_identity",
    "stage.exact_source_local",
];
const PROJECT_CONFIG_DIMENSIONS: &[&str] = &[
    "client.pinned_commit",
    "generation.config",
    "generation.root",
    "server.executable_identity",
    "stage.exact_source_local",
];
const CLIENT_SETTINGS_DIMENSIONS: &[&str] = &[
    "client.pinned_commit",
    "generation.config",
    "server.executable_identity",
    "stage.exact_source_local",
];
const STALE_GENERATION_DIMENSIONS: &[&str] = &[
    "client.pinned_commit",
    "generation.config",
    "generation.process",
    "generation.root",
    "generation.source",
    "server.executable_identity",
    "stage.exact_source_local",
];
const PROVIDER_OWNERSHIP_DIMENSIONS: &[&str] = &[
    "client.pinned_commit",
    "generation.process",
    "root.selection",
    "server.executable_identity",
    "service.provider_identity",
    "stage.exact_source_local",
];

const SEMANTIC_INSTRUMENT: &[InstrumentEvidence] = &[
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

const BASE_CLAIM_CEILING: &str = "registration only: pre-registers one exact-subject Vim/vim-lsp freshness cell for the generic editor_client_compat.v1 receipt; binds landed #11380 action owners and #11369 fixtures until #11376/#11378 land their owning surfaces; proves no host reload/configuration behavior and awards no support profile";
const ROUTE_CLAIM_CEILING: &str = "registration only: a freshness-route classification (including explicit_reload_required/restart_required dispositions) is route shape, never an automatic semantic pass";
const STALE_CLAIM_CEILING: &str = "registration only: a stale source/config/root-generation promotion must classify fail, never pass; cleanup evidence stays independently load-bearing";
const PROVIDER_CLAIM_CEILING: &str = "registration only: naming the selected client/provider/service owner is an identity proposition, never semantic freshness";

const OBSERVE: &str = "vim.vim_lsp.specialized.freshness.observe_route_and_generation";
const HOLD_RELEASE: &str = "vim.vim_lsp.specialized.freshness.hold_release_old_generation";

/// Declaration shape for one freshness row; [`build`] fills the pinned
/// subject, version, stage, and profile every freshness cell shares.
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
        allowed_limitations: FRESHNESS_LIMITATION_VOCABULARY
            .iter()
            .map(|value| value.to_string())
            .collect(),
        allowed_profiles: vec![FRESHNESS_PROFILE.to_string()],
        claim_ceiling: spec.claim_ceiling.to_string(),
    }
}

/// The freshness scenario ledger, derived from the landed #11380 freshness
/// action vocabulary: one baseline scenario per action ID. #11376 owns the
/// BDD scenario ledger; when it lands, this derivation is superseded by a
/// reviewed re-bind, never by a local edit of scenario rows.
pub fn freshness_action_ledger() -> ScenarioLedger {
    let mut scenarios: Vec<Scenario> = ACTIONS
        .iter()
        .filter(|action| action.family == ActionFamily::Freshness)
        .map(|action| Scenario { id: action.action_id.to_string(), class: ScenarioClass::Baseline })
        .collect();
    // Deterministic aggregation: the derived ledger is sorted by scenario ID
    // so a future reorder of the compiled vocabulary cannot change the
    // ledger's serialized shape.
    scenarios.sort_by(|left, right| left.id.cmp(&right.id));
    ScenarioLedger {
        ledger_id: FRESHNESS_LEDGER_ID.to_string(),
        owning_authority: "#11380 specialized action vocabulary (PR #12204), freshness family; supersedes pending: #11376 owns the BDD scenario ledger, #11378 the fixture/expectation cells"
            .to_string(),
        scenarios,
    }
}

/// The freshness family catalog registered on this PR (#11381).
pub fn freshness_catalog() -> CellCatalog {
    let subject = super::vim_vim_lsp_subject();
    CellCatalog {
        catalog_id: FRESHNESS_CATALOG_ID.to_string(),
        catalog_version: 1,
        ledger_id: FRESHNESS_LEDGER_ID.to_string(),
        coverage: CoverageRule::AdditiveFamily,
        fixture_substrate: FRESHNESS_FIXTURE_SUBSTRATE
            .iter()
            .map(|value| value.to_string())
            .collect(),
        allowed_stages: vec![EvidenceStage::ExactSourceLocal],
        allowed_result_vocabulary: FRESHNESS_RESULT_VOCABULARY
            .iter()
            .map(|value| value.to_string())
            .collect(),
        core_profile: None,
        cells: vec![
            build(
                CellSpec {
                    cell_id: "vim.vim_lsp.freshness.route",
                    scenario_owners: &[
                        OBSERVE,
                        "vim.vim_lsp.specialized.freshness.explicit_reload_or_restart",
                    ],
                    fixture_owners: ALL_THREE_FIXTURES,
                    observation_class: OBSERVE,
                    subject_dimensions: ROUTE_DIMENSIONS,
                    instrument_evidence: SEMANTIC_INSTRUMENT,
                    allowed_results: FRESHNESS_RESULT_VOCABULARY,
                    claim_ceiling: ROUTE_CLAIM_CEILING,
                },
                subject.clone(),
            ),
            build(
                CellSpec {
                    cell_id: "vim.vim_lsp.freshness.external_source",
                    scenario_owners: &[
                        "vim.vim_lsp.specialized.freshness.source_mutate_closed_in_place",
                        "vim.vim_lsp.specialized.freshness.source_atomic_replace",
                        "vim.vim_lsp.specialized.freshness.source_create_delete_rename",
                        OBSERVE,
                    ],
                    fixture_owners: SUBJECT_SURFACE_FIXTURES,
                    observation_class: OBSERVE,
                    subject_dimensions: EXTERNAL_SOURCE_DIMENSIONS,
                    instrument_evidence: SEMANTIC_INSTRUMENT,
                    allowed_results: FRESHNESS_RESULT_VOCABULARY,
                    claim_ceiling: BASE_CLAIM_CEILING,
                },
                subject.clone(),
            ),
            build(
                CellSpec {
                    cell_id: "vim.vim_lsp.freshness.project_config",
                    scenario_owners: &[
                        "vim.vim_lsp.specialized.freshness.config_file_lifecycle",
                        "vim.vim_lsp.specialized.freshness.config_file_malformed",
                        "vim.vim_lsp.specialized.freshness.config_file_repair",
                        OBSERVE,
                    ],
                    fixture_owners: SUBJECT_CONFIG_FIXTURES,
                    observation_class: OBSERVE,
                    subject_dimensions: PROJECT_CONFIG_DIMENSIONS,
                    instrument_evidence: SEMANTIC_INSTRUMENT,
                    allowed_results: FRESHNESS_RESULT_VOCABULARY,
                    claim_ceiling: BASE_CLAIM_CEILING,
                },
                subject.clone(),
            ),
            build(
                CellSpec {
                    cell_id: "vim.vim_lsp.freshness.client_settings",
                    scenario_owners: &[
                        "vim.vim_lsp.specialized.freshness.workspace_setting_change",
                        OBSERVE,
                    ],
                    fixture_owners: ALL_THREE_FIXTURES,
                    observation_class: OBSERVE,
                    subject_dimensions: CLIENT_SETTINGS_DIMENSIONS,
                    instrument_evidence: SEMANTIC_INSTRUMENT,
                    allowed_results: FRESHNESS_RESULT_VOCABULARY,
                    claim_ceiling: BASE_CLAIM_CEILING,
                },
                subject.clone(),
            ),
            build(
                CellSpec {
                    cell_id: "vim.vim_lsp.freshness.stale_generation_rejected",
                    scenario_owners: &[HOLD_RELEASE],
                    fixture_owners: SUBJECT_SURFACE_FIXTURES,
                    observation_class: HOLD_RELEASE,
                    subject_dimensions: STALE_GENERATION_DIMENSIONS,
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
                subject.clone(),
            ),
            build(
                CellSpec {
                    cell_id: "vim.vim_lsp.freshness.provider_ownership",
                    scenario_owners: &[OBSERVE],
                    fixture_owners: SUBJECT_SURFACE_FIXTURES,
                    observation_class: OBSERVE,
                    subject_dimensions: PROVIDER_OWNERSHIP_DIMENSIONS,
                    instrument_evidence: SEMANTIC_INSTRUMENT,
                    allowed_results: &[
                        "pass",
                        "fail",
                        "partial",
                        "client_not_exposed",
                        "unsupported",
                        "not_proven",
                    ],
                    claim_ceiling: PROVIDER_CLAIM_CEILING,
                },
                subject,
            ),
        ],
    }
}

/// The landed freshness action IDs, as the family's authority set.
fn freshness_action_ids() -> BTreeSet<&'static str> {
    ACTIONS
        .iter()
        .filter(|action| action.family == ActionFamily::Freshness)
        .map(|action| action.action_id)
        .collect()
}

/// Validate the compiled freshness catalog against the family laws.
pub fn validate_family_laws() -> Result<()> {
    validate_freshness_catalog(&freshness_catalog(), &freshness_action_ledger())
}

/// Validate one freshness-shaped catalog against the family laws. Shared-model
/// laws (subject pin, stage bound, duplicate IDs, ledger membership,
/// cross-catalog ownership) run in [`super::validate_registry`]; the laws here
/// are the ones only this family can state.
pub fn validate_freshness_catalog(catalog: &CellCatalog, ledger: &ScenarioLedger) -> Result<()> {
    ensure!(
        catalog.catalog_id == FRESHNESS_CATALOG_ID,
        "freshness family catalog must keep its identity {}, found {}",
        FRESHNESS_CATALOG_ID,
        catalog.catalog_id
    );
    ensure!(
        catalog.ledger_id == FRESHNESS_LEDGER_ID && ledger.ledger_id == FRESHNESS_LEDGER_ID,
        "freshness family must bind ledger {FRESHNESS_LEDGER_ID}"
    );
    ensure!(
        catalog.coverage == CoverageRule::AdditiveFamily,
        "freshness family catalog is additive, not a baseline-coverage catalog"
    );
    ensure!(
        catalog.core_profile.is_none(),
        "freshness family assigns no core profile; profiles consume cells, catalogs do not assign them"
    );
    ensure!(
        catalog.allowed_stages.len() == 1
            && catalog.allowed_stages[0] == EvidenceStage::ExactSourceLocal,
        "freshness family stage bound is exact_source_local only; a maintained/public stage needs its own reviewed stage law"
    );
    let declared: BTreeSet<&str> =
        catalog.allowed_result_vocabulary.iter().map(String::as_str).collect();
    let expected: BTreeSet<&str> = FRESHNESS_RESULT_VOCABULARY.iter().copied().collect();
    ensure!(
        declared == expected,
        "freshness result vocabulary drifted from the #11381 dispositions"
    );

    let actions = freshness_action_ids();
    let scenarios: BTreeSet<&str> = ledger.scenarios.iter().map(|s| s.id.as_str()).collect();
    ensure!(
        scenarios == actions,
        "freshness ledger must mirror exactly the landed #11380 freshness actions; ledger has {} rows, vocabulary has {}",
        scenarios.len(),
        actions.len()
    );

    let mut covered: BTreeSet<String> = BTreeSet::new();
    for cell in &catalog.cells {
        ensure!(
            cell.cell_id.starts_with(CELL_PREFIX),
            "cell {} is outside the freshness family namespace {CELL_PREFIX}",
            cell.cell_id
        );
        ensure!(
            actions.contains(cell.observation_class.as_str()),
            "cell {} observation class {} is not a landed freshness action; watcher/registration/event/log tokens and other families' actions cannot classify freshness",
            cell.cell_id,
            cell.observation_class
        );
        ensure!(
            cell.scenario_owners.iter().any(|owner| owner == &cell.observation_class),
            "cell {} observation class {} must be one of its own scenario owners",
            cell.cell_id,
            cell.observation_class
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
        ensure!(
            cell.subject_dimensions.iter().any(|token| token.starts_with(GENERATION_PREFIX)),
            "cell {} must bind at least one generation dimension",
            cell.cell_id
        );
        ensure!(
            cell.allowed_profiles.len() == 1 && cell.allowed_profiles[0] == FRESHNESS_PROFILE,
            "cell {} may feed only {FRESHNESS_PROFILE}",
            cell.cell_id
        );
        covered.extend(cell.scenario_owners.iter().cloned());
    }
    let uncovered: Vec<&str> =
        actions.iter().filter(|action| !covered.contains(**action)).copied().collect();
    ensure!(
        uncovered.is_empty(),
        "landed freshness actions without a pre-registered cell: {uncovered:?}"
    );
    Ok(())
}
