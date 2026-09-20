//! The complete #11371 baseline cell catalog compiled on this PR (#11374).
//!
//! Seventeen pre-registered cells carry the whole 23-scenario baseline
//! journey: every baseline scenario of the #11371 ledger is owned here and no
//! optional scenario enters. Cell meaning is one-directional — a cell names
//! the scenario journeys that evidence it; a scenario is cited only inside
//! this one catalog, so no future family can substitute itself for a baseline
//! proposition.
//!
//! Two cells cite `vim.bdd.edit.01` deliberately: the rename proposition
//! (`edit.rename`, the request and its occurrence-exactness) and the edit
//! application mechanism (`edit.workspace_edit`, the client's WorkspaceEdit
//! application), matching the #10955 slice's request/application split. Three
//! cells' worth of configuration journey (`vim.bdd.edit.03`–`05`) land in one
//! `config.workspace_effect` cell because #11371 frames them as one governed
//! workspace-configuration effect with its security guard.

use super::{CellCatalog, CellRegistration, CellSubject, CoverageRule, InstrumentEvidence};
use crate::editor_client_compat::EvidenceStage;

pub const BASELINE_CATALOG_ID: &str = "vim_lsp_baseline";

/// Fixture substrate: the landed #11369/#7762 authority artifacts. Tests
/// verify each ID resolves to `.ci/editor-clients/<id>.json`, so a catalog row
/// can never bind a fixture authority that is absent from the tree.
pub const BASELINE_FIXTURE_SUBSTRATE: &[&str] = &[
    "vim-vim-lsp-activation-root.v1",
    "vim-vim-lsp-configuration.v1",
    "vim-vim-lsp-public-surface.v1",
    "vim-vim-lsp-subject.v1",
];

/// Baseline result vocabulary (#11374): exactly the dispositions the generic
/// `editor_client_compat.v1` `ObservationResult` can serialize
/// (`pass`/`fail`/`partial`/`not_proven`/`unsupported`). Exposure states such
/// as `client_not_exposed` ride as admitted limitation tokens on
/// limitation-requiring results instead of result tokens, so a catalog-valid
/// cell is always encodable in the receipt it pre-registers for;
/// `instrument_failed` stays a receipt-level failure class (#7777), not a
/// cell result, in the baseline family. Future families whose owning
/// contracts permit richer dispositions declare their own vocabulary.
pub const BASELINE_RESULT_VOCABULARY: &[&str] =
    &["pass", "fail", "partial", "not_proven", "unsupported"];

const SUBJECT_CONFIG_FIXTURES: &[&str] =
    &["vim-vim-lsp-subject.v1", "vim-vim-lsp-configuration.v1"];
const ACTIVATION_ROOT_FIXTURES: &[&str] = &["vim-vim-lsp-activation-root.v1"];
const SYNC_FIXTURES: &[&str] =
    &["vim-vim-lsp-subject.v1", "vim-vim-lsp-configuration.v1", "vim-vim-lsp-public-surface.v1"];

const CORE_DIMENSIONS: &[&str] = &[
    "client.pinned_commit",
    "integration.configuration_sha256",
    "integration.driver_sha256",
    "server.executable_identity",
    "stage.exact_source_local",
];
const ROOT_DIMENSIONS: &[&str] = &[
    "client.pinned_commit",
    "root.contract_digest",
    "server.executable_identity",
    "stage.exact_source_local",
];
const BOOTSTRAP_DIMENSIONS: &[&str] = &[
    "capabilities.initialize_snapshot_sha256",
    "client.pinned_commit",
    "host.executable_sha256",
    "host.version",
    "integration.configuration_sha256",
    "integration.driver_sha256",
    "server.artifact_sha256",
    "server.build_revision",
    "server.executable_identity",
    "stage.exact_source_local",
];
const POSITION_DIMENSIONS: &[&str] = &[
    "capabilities.position_encoding_selected",
    "client.pinned_commit",
    "integration.driver_sha256",
    "server.executable_identity",
    "stage.exact_source_local",
];

const PRODUCT_INSTRUMENT: &[InstrumentEvidence] = &[
    InstrumentEvidence::CapabilitySnapshot,
    InstrumentEvidence::ClientLog,
    InstrumentEvidence::ProcessLedger,
];
const BOOTSTRAP_INSTRUMENT: &[InstrumentEvidence] = &[
    InstrumentEvidence::CapabilitySnapshot,
    InstrumentEvidence::ClientLog,
    InstrumentEvidence::ProcessLedger,
    InstrumentEvidence::ServerStderr,
];
const CLEANUP_INSTRUMENT: &[InstrumentEvidence] = &[
    InstrumentEvidence::CapabilitySnapshot,
    InstrumentEvidence::CleanupObservation,
    InstrumentEvidence::ClientLog,
    InstrumentEvidence::ProcessLedger,
];

const CAPABILITY_LIMITATIONS: &[&str] =
    &["capability_not_advertised", "client_not_exposed", "not_proven", "observation_incomplete"];
const BOOTSTRAP_LIMITATIONS: &[&str] =
    &["instrument_incomplete", "not_proven", "observation_incomplete"];
const CONFIG_LIMITATIONS: &[&str] =
    &["client_not_exposed", "configuration_only", "not_proven", "observation_incomplete"];
const CLEANUP_LIMITATIONS: &[&str] = &["not_proven", "observation_incomplete"];

const BASELINE_CLAIM_CEILING: &str = "registration only: binds one pre-registered exact-subject Vim/vim-lsp proposition for the generic editor_client_compat.v1 receipt; proves no host behavior and awards no support profile";
const CLEANUP_CLAIM_CEILING: &str = "registration only: binds the independently load-bearing process-cleanup disposition; cleanup evidence can never be rewritten as a product pass, and its absence blocks any passing receipt";

/// Declaration shape for one baseline row; [`build`] fills the pinned
/// subject, version, stage, profile, and claim ceiling every baseline cell
/// shares.
struct CellSpec<'a> {
    cell_id: &'a str,
    scenario_owners: &'a [&'a str],
    fixture_owners: &'a [&'a str],
    observation_class: &'a str,
    subject_dimensions: &'a [&'a str],
    instrument_evidence: &'a [InstrumentEvidence],
    allowed_results: &'a [&'a str],
    allowed_limitations: &'a [&'a str],
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
        allowed_limitations: spec
            .allowed_limitations
            .iter()
            .map(|value| value.to_string())
            .collect(),
        allowed_profiles: vec!["vim_actual_client_core".to_string()],
        claim_ceiling: spec.claim_ceiling.to_string(),
    }
}

/// The baseline catalog. The #10962 fan-in and the #11381/#11384/#11386/
/// #11387/#11388 family catalogs consume this through the shared validation
/// API; its digest is the stable identity a baseline receipt binds.
pub fn baseline_catalog() -> CellCatalog {
    let subject = super::vim_vim_lsp_subject();
    CellCatalog {
        catalog_id: BASELINE_CATALOG_ID.to_string(),
        catalog_version: 1,
        ledger_id: super::scenario_ledger::VIM_BDD_LEDGER_ID.to_string(),
        coverage: CoverageRule::ExactLedgerBaseline,
        fixture_substrate: BASELINE_FIXTURE_SUBSTRATE
            .iter()
            .map(|value| value.to_string())
            .collect(),
        allowed_stages: vec![EvidenceStage::ExactSourceLocal],
        allowed_result_vocabulary: BASELINE_RESULT_VOCABULARY
            .iter()
            .map(|value| value.to_string())
            .collect(),
        core_profile: Some("vim_actual_client_core".to_string()),
        cells: vec![
            build(
                CellSpec {
                    cell_id: "vim.vim_lsp.core.activation",
                    scenario_owners: &["vim.bdd.attach.01"],
                    fixture_owners: ACTIVATION_ROOT_FIXTURES,
                    observation_class: "activation.filetype_detection",
                    subject_dimensions: ROOT_DIMENSIONS,
                    instrument_evidence: PRODUCT_INSTRUMENT,
                    allowed_results: BASELINE_RESULT_VOCABULARY,
                    allowed_limitations: CAPABILITY_LIMITATIONS,
                    claim_ceiling: BASELINE_CLAIM_CEILING,
                },
                subject.clone(),
            ),
            build(
                CellSpec {
                    cell_id: "vim.vim_lsp.core.root",
                    scenario_owners: &["vim.bdd.attach.04", "vim.bdd.attach.05"],
                    fixture_owners: ACTIVATION_ROOT_FIXTURES,
                    observation_class: "root.selection",
                    subject_dimensions: ROOT_DIMENSIONS,
                    instrument_evidence: PRODUCT_INSTRUMENT,
                    allowed_results: BASELINE_RESULT_VOCABULARY,
                    allowed_limitations: CAPABILITY_LIMITATIONS,
                    claim_ceiling: BASELINE_CLAIM_CEILING,
                },
                subject.clone(),
            ),
            build(
                CellSpec {
                    cell_id: "vim.vim_lsp.core.bootstrap",
                    scenario_owners: &["vim.bdd.attach.02", "vim.bdd.attach.03"],
                    fixture_owners: SUBJECT_CONFIG_FIXTURES,
                    observation_class: "session.bootstrap",
                    subject_dimensions: BOOTSTRAP_DIMENSIONS,
                    instrument_evidence: BOOTSTRAP_INSTRUMENT,
                    allowed_results: BASELINE_RESULT_VOCABULARY,
                    allowed_limitations: BOOTSTRAP_LIMITATIONS,
                    claim_ceiling: BASELINE_CLAIM_CEILING,
                },
                subject.clone(),
            ),
            build(
                CellSpec {
                    cell_id: "vim.vim_lsp.core.diagnostics",
                    scenario_owners: &["vim.bdd.attach.06", "vim.bdd.attach.07"],
                    fixture_owners: SUBJECT_CONFIG_FIXTURES,
                    observation_class: "diagnostics.visible_state",
                    subject_dimensions: CORE_DIMENSIONS,
                    instrument_evidence: BOOTSTRAP_INSTRUMENT,
                    allowed_results: BASELINE_RESULT_VOCABULARY,
                    allowed_limitations: CAPABILITY_LIMITATIONS,
                    claim_ceiling: BASELINE_CLAIM_CEILING,
                },
                subject.clone(),
            ),
            build(
                CellSpec {
                    cell_id: "vim.vim_lsp.completion.accept_plain",
                    scenario_owners: &["vim.bdd.nav.01", "vim.bdd.nav.02", "vim.bdd.nav.03"],
                    fixture_owners: SUBJECT_CONFIG_FIXTURES,
                    observation_class: "completion.acceptance",
                    subject_dimensions: CORE_DIMENSIONS,
                    instrument_evidence: PRODUCT_INSTRUMENT,
                    allowed_results: BASELINE_RESULT_VOCABULARY,
                    allowed_limitations: CAPABILITY_LIMITATIONS,
                    claim_ceiling: BASELINE_CLAIM_CEILING,
                },
                subject.clone(),
            ),
            build(
                CellSpec {
                    cell_id: "vim.vim_lsp.navigation.hover",
                    scenario_owners: &["vim.bdd.nav.04"],
                    fixture_owners: SUBJECT_CONFIG_FIXTURES,
                    observation_class: "navigation.hover",
                    subject_dimensions: CORE_DIMENSIONS,
                    instrument_evidence: PRODUCT_INSTRUMENT,
                    allowed_results: BASELINE_RESULT_VOCABULARY,
                    allowed_limitations: CAPABILITY_LIMITATIONS,
                    claim_ceiling: BASELINE_CLAIM_CEILING,
                },
                subject.clone(),
            ),
            build(
                CellSpec {
                    cell_id: "vim.vim_lsp.navigation.definition",
                    scenario_owners: &["vim.bdd.nav.05"],
                    fixture_owners: SUBJECT_CONFIG_FIXTURES,
                    observation_class: "navigation.definition",
                    subject_dimensions: CORE_DIMENSIONS,
                    instrument_evidence: PRODUCT_INSTRUMENT,
                    allowed_results: BASELINE_RESULT_VOCABULARY,
                    allowed_limitations: CAPABILITY_LIMITATIONS,
                    claim_ceiling: BASELINE_CLAIM_CEILING,
                },
                subject.clone(),
            ),
            build(
                CellSpec {
                    cell_id: "vim.vim_lsp.navigation.references",
                    scenario_owners: &["vim.bdd.nav.06"],
                    fixture_owners: SUBJECT_CONFIG_FIXTURES,
                    observation_class: "navigation.references",
                    subject_dimensions: CORE_DIMENSIONS,
                    instrument_evidence: PRODUCT_INSTRUMENT,
                    allowed_results: BASELINE_RESULT_VOCABULARY,
                    allowed_limitations: CAPABILITY_LIMITATIONS,
                    claim_ceiling: BASELINE_CLAIM_CEILING,
                },
                subject.clone(),
            ),
            build(
                CellSpec {
                    cell_id: "vim.vim_lsp.edit.rename",
                    scenario_owners: &["vim.bdd.edit.01"],
                    fixture_owners: SUBJECT_CONFIG_FIXTURES,
                    observation_class: "edit.rename_request",
                    subject_dimensions: CORE_DIMENSIONS,
                    instrument_evidence: PRODUCT_INSTRUMENT,
                    allowed_results: BASELINE_RESULT_VOCABULARY,
                    allowed_limitations: CAPABILITY_LIMITATIONS,
                    claim_ceiling: BASELINE_CLAIM_CEILING,
                },
                subject.clone(),
            ),
            build(
                CellSpec {
                    cell_id: "vim.vim_lsp.edit.workspace_edit",
                    scenario_owners: &["vim.bdd.edit.01"],
                    fixture_owners: SUBJECT_CONFIG_FIXTURES,
                    observation_class: "edit.workspace_edit_application",
                    subject_dimensions: CORE_DIMENSIONS,
                    instrument_evidence: PRODUCT_INSTRUMENT,
                    allowed_results: BASELINE_RESULT_VOCABULARY,
                    allowed_limitations: CAPABILITY_LIMITATIONS,
                    claim_ceiling: BASELINE_CLAIM_CEILING,
                },
                subject.clone(),
            ),
            build(
                CellSpec {
                    cell_id: "vim.vim_lsp.edit.format_explicit",
                    scenario_owners: &["vim.bdd.edit.02"],
                    fixture_owners: SUBJECT_CONFIG_FIXTURES,
                    observation_class: "edit.format_application",
                    subject_dimensions: CORE_DIMENSIONS,
                    instrument_evidence: PRODUCT_INSTRUMENT,
                    allowed_results: BASELINE_RESULT_VOCABULARY,
                    allowed_limitations: CAPABILITY_LIMITATIONS,
                    claim_ceiling: BASELINE_CLAIM_CEILING,
                },
                subject.clone(),
            ),
            build(
                CellSpec {
                    cell_id: "vim.vim_lsp.config.workspace_effect",
                    scenario_owners: &["vim.bdd.edit.03", "vim.bdd.edit.04", "vim.bdd.edit.05"],
                    fixture_owners: SUBJECT_CONFIG_FIXTURES,
                    observation_class: "config.workspace_effect",
                    subject_dimensions: CORE_DIMENSIONS,
                    instrument_evidence: PRODUCT_INSTRUMENT,
                    allowed_results: BASELINE_RESULT_VOCABULARY,
                    allowed_limitations: CONFIG_LIMITATIONS,
                    claim_ceiling: BASELINE_CLAIM_CEILING,
                },
                subject.clone(),
            ),
            build(
                CellSpec {
                    cell_id: "vim.vim_lsp.position.non_bmp",
                    scenario_owners: &["vim.bdd.lifecycle.01"],
                    fixture_owners: SUBJECT_CONFIG_FIXTURES,
                    observation_class: "position.non_bmp_resolution",
                    subject_dimensions: POSITION_DIMENSIONS,
                    instrument_evidence: PRODUCT_INSTRUMENT,
                    allowed_results: BASELINE_RESULT_VOCABULARY,
                    allowed_limitations: CAPABILITY_LIMITATIONS,
                    claim_ceiling: BASELINE_CLAIM_CEILING,
                },
                subject.clone(),
            ),
            build(
                CellSpec {
                    cell_id: "vim.vim_lsp.sync.did_change",
                    scenario_owners: &["vim.bdd.lifecycle.02"],
                    fixture_owners: SYNC_FIXTURES,
                    observation_class: "sync.did_change_observation",
                    subject_dimensions: CORE_DIMENSIONS,
                    instrument_evidence: PRODUCT_INSTRUMENT,
                    allowed_results: BASELINE_RESULT_VOCABULARY,
                    allowed_limitations: CAPABILITY_LIMITATIONS,
                    claim_ceiling: BASELINE_CLAIM_CEILING,
                },
                subject.clone(),
            ),
            build(
                CellSpec {
                    cell_id: "vim.vim_lsp.currentness.post_edit",
                    scenario_owners: &["vim.bdd.lifecycle.03"],
                    fixture_owners: SUBJECT_CONFIG_FIXTURES,
                    observation_class: "currentness.post_edit_generation",
                    subject_dimensions: CORE_DIMENSIONS,
                    instrument_evidence: PRODUCT_INSTRUMENT,
                    allowed_results: BASELINE_RESULT_VOCABULARY,
                    allowed_limitations: CAPABILITY_LIMITATIONS,
                    claim_ceiling: BASELINE_CLAIM_CEILING,
                },
                subject.clone(),
            ),
            build(
                CellSpec {
                    cell_id: "vim.vim_lsp.lifecycle.close_reopen",
                    scenario_owners: &["vim.bdd.lifecycle.04"],
                    fixture_owners: SUBJECT_CONFIG_FIXTURES,
                    observation_class: "lifecycle.close_reopen",
                    subject_dimensions: CORE_DIMENSIONS,
                    instrument_evidence: PRODUCT_INSTRUMENT,
                    allowed_results: BASELINE_RESULT_VOCABULARY,
                    allowed_limitations: CAPABILITY_LIMITATIONS,
                    claim_ceiling: BASELINE_CLAIM_CEILING,
                },
                subject.clone(),
            ),
            build(
                CellSpec {
                    cell_id: "vim.vim_lsp.lifecycle.baseline_cleanup",
                    scenario_owners: &["vim.bdd.lifecycle.05"],
                    fixture_owners: SUBJECT_CONFIG_FIXTURES,
                    observation_class: "cleanup.process",
                    subject_dimensions: CORE_DIMENSIONS,
                    instrument_evidence: CLEANUP_INSTRUMENT,
                    allowed_results: BASELINE_RESULT_VOCABULARY,
                    allowed_limitations: CLEANUP_LIMITATIONS,
                    claim_ceiling: CLEANUP_CLAIM_CEILING,
                },
                subject,
            ),
        ],
    }
}

/// Convenience accessor for the ledger this catalog binds, so family modules
/// and tests can validate a baseline-plus-family registry without
/// reconstructing the ledger by hand.
pub fn baseline_ledger() -> super::ScenarioLedger {
    super::scenario_ledger::vim_bdd_ledger_11371()
}
