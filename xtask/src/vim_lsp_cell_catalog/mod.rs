//! Checked additive cell-catalog API for Vim/vim-lsp `editor_client_compat.v1`
//! journey cells (#11374).
//!
//! Ownership split — consumed, never duplicated:
//!
//! - `editor_client_compat.v1` (`crate::editor_client_compat`) owns the generic
//!   actual-editor receipt contract, including the free-form journey-cell
//!   grammar every client family shares.
//! - This module owns the *pre-registration* of which Vim/vim-lsp cells may
//!   ever exist: one checked registration model binding every admitted cell to
//!   its stable cell ID/version, BDD scenario owner, fixture owners, exact
//!   host/client subject, required action/observation class, required exact
//!   subject dimensions, required instrument/reporting/cleanup evidence,
//!   allowed evidence stages, allowed result/limitation vocabulary, allowed
//!   support-profile consumers, and a claim ceiling.
//! - `.spec/11371-vim-bdd-journeys/` owns the stable `vim.bdd.*` scenario IDs.
//!   The ledger mirror in [`scenario_ledger`] is checked against those files by
//!   tests so the mirror cannot drift from the landed authority.
//! - `.ci/editor-clients/vim-vim-lsp-*.json` (#11369 / #7762 substrate) own the
//!   exact subject/configuration/public-surface/activation-root bytes; the
//!   baseline catalog binds them by fixture-owner ID and tests verify those
//!   files exist, so an absent fixture authority fails closed.
//!
//! Fail-closed laws enforced by [`validate_registry`]:
//!
//! - unknown cell ID shape, duplicate cell ID (within or across catalogs), or
//!   a cell ID outside the `vim.vim_lsp.<family>.<name>` namespace is rejected;
//! - a cell citing a scenario absent from its catalog's declared ledger, a
//!   scenario already owned by a different catalog, an optional scenario from
//!   inside a baseline-coverage catalog, or an uncovered baseline scenario is
//!   rejected;
//! - a required fixture owner absent from the catalog's declared substrate is
//!   rejected;
//! - a cell bound to any subject other than the pinned
//!   `Vim + prabirshrestha/vim-lsp + perllsp --stdio` subject is rejected, so a
//!   Coc/yegappan/Neovim/DAP observation cannot be registered here;
//! - a cell admitting an evidence stage outside its catalog's stage bound is
//!   rejected (the baseline catalog admits `exact_source_local` only, so an
//!   exact-source cell cannot grow a public-artifact stage by editing a row);
//! - result tokens outside the catalog's declared vocabulary, an empty result
//!   set, a limitation-requiring result with no admitted limitation vocabulary,
//!   a profile outside the known #11371 profile set, or a baseline cell that
//!   does not feed `vim_actual_client_core` are rejected;
//! - empty subject dimensions, instrument evidence, or claim ceiling are
//!   rejected.
//!
//! The baseline catalog compiled into [`baseline`] is the complete #11371
//! baseline registry consumed by the #10962 fan-in. Additive family catalogs
//! (#11381 freshness in [`freshness`], #11384 format-on-save in
//! [`save_format`], #11388 expanded activation in [`activation`], #11386
//! server-generation recovery in [`recovery`], #11387 host-reopen/repeated
//! session in [`lifecycle`])
//! register through this same API as sibling modules: they declare their own
//! scenario ledger, fixture substrate, result vocabulary, and stage bound, and
//! they can neither steal a baseline scenario nor shift a baseline cell's
//! meaning — a family addition changes the registry digest but leaves every
//! baseline cell digest and the baseline catalog digest byte-identical. Each
//! family module also owns family laws beyond the shared model (its ledger
//! mirrors its landed #11380 action vocabulary, its observation classes are
//! landed actions, its vocabulary and stage bounds are pinned);
//! [`validate_compiled_registry`] runs every registered family's laws so no
//! consumer can validate the compiled registry without them.

pub mod activation;
pub mod baseline;
pub mod freshness;
pub mod lifecycle;
pub mod recovery;
pub mod save_format;
pub mod scenario_ledger;

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use crate::client_compat_fixture::is_reason_token;
use crate::editor_client_compat::EvidenceStage;

/// Identity of this registration model, for receipts and indexes that need to
/// name the catalog semantics they were validated against.
pub const CELL_CATALOG_SCHEMA_VERSION: &str = "vim_lsp_cell_catalog.v1";

/// The only cell-ID namespace this registry admits. A separate exact-subject
/// family (for example a yegappan/lsp client, owner #7717) registers through a
/// separate registry module with its own namespace; it never extends this one.
pub const CELL_ID_PREFIX: &str = "vim.vim_lsp.";

/// Support-profile vocabulary owned by the #11371 claim-profile table. A cell
/// may only feed one of these profiles; new profiles arrive as reviewed
/// registry edits, never as a family-side token invention.
pub const KNOWN_PROFILES: &[&str] = &[
    "vim_configuration_documented",
    "vim_actual_client_core",
    "vim_first_class_exact_source",
    "vim_public_artifact",
    "vim_programme_closeout",
];

/// Result tokens that always require an admitted limitation vocabulary,
/// mirroring the `editor_client_compat.v1` journey-cell law so a catalog row
/// can never promise a disposition the generic receipt cannot carry honestly.
pub const LIMITATION_REQUIRING_RESULTS: &[&str] =
    &["partial", "not_proven", "unsupported", "client_not_exposed"];

/// The one exact subject this registry admits, mirroring the #11369 pin
/// (#12050): Vim host, the pinned `prabirshrestha/vim-lsp` client plugin,
/// `perllsp --stdio` server, generic-LSP integration.
///
/// `client_id` uses the receipt-valid stable token `vim-lsp`: the plugin's
/// repository path (`prabirshrestha/vim-lsp`, which contains `/` and so is
/// not a valid `HostIdentity.client_id` reason token) is identified by this
/// documentation and bound through the pinned bytes in
/// `.ci/editor-clients/vim-vim-lsp-subject.v1.json` plus the
/// `client.pinned_commit` subject dimension, never by a second pin here.
pub fn vim_vim_lsp_subject() -> CellSubject {
    CellSubject {
        host_product: "vim".to_string(),
        client_id: "vim-lsp".to_string(),
        server_executable: "perllsp".to_string(),
        launch_command: vec!["perllsp".to_string(), "--stdio".to_string()],
        integration_mode: "generic_lsp".to_string(),
    }
}

/// Classification of one scenario inside its ledger. `Optional` scenarios are
/// `consumes_if_available` inputs (#10858): they may seed a future optional
/// family but can never enter a baseline-coverage catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScenarioClass {
    Baseline,
    Optional,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scenario {
    pub id: String,
    pub class: ScenarioClass,
}

/// A landed BDD scenario ledger: the stable scenario IDs one family of cells
/// may cite. The current landed ledger is the #11371 mirror in
/// [`scenario_ledger`]; #11376-class extensions arrive as new ledger constants
/// alongside their family catalogs, never by editing #11371's rows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScenarioLedger {
    pub ledger_id: String,
    /// Owning authority (issue/spec reference) recorded for review; not
    /// validated beyond being a non-empty identity.
    pub owning_authority: String,
    pub scenarios: Vec<Scenario>,
}

/// The exact host/client/server subject a cell's observations must come from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CellSubject {
    pub host_product: String,
    pub client_id: String,
    pub server_executable: String,
    pub launch_command: Vec<String>,
    pub integration_mode: String,
}

/// Instrument/reporting/cleanup evidence a cell requires before its product
/// disposition can be anything but `not_proven`. Mirrors the #7777/#10894
/// product/instrument/reporting/cleanup split: artifact kinds are the generic
/// receipt vocabulary; `CleanupObservation` names the independently
/// load-bearing cleanup disposition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstrumentEvidence {
    ClientLog,
    ServerStderr,
    DriverOutput,
    CapabilitySnapshot,
    ProcessLedger,
    FailureDiagnostics,
    CleanupObservation,
}

/// One pre-registered cell. Every field is load-bearing at validation time;
/// digests cover all of them, so any semantic edit is a visible identity
/// change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CellRegistration {
    /// Stable ID in the `vim.vim_lsp.<family>.<name>` namespace.
    pub cell_id: String,
    /// Registration version of this cell; bumped by reviewed edits.
    pub cell_version: u32,
    /// BDD scenario owners from the catalog's declared ledger.
    pub scenario_owners: Vec<String>,
    /// Fixture/expectation owners from the catalog's declared substrate.
    pub fixture_owners: Vec<String>,
    /// Allowed host/client subject; must equal [`vim_vim_lsp_subject`].
    pub subject: CellSubject,
    /// Required action/observation class token.
    pub observation_class: String,
    /// Exact subject dimensions a receipt for this cell must bind.
    pub subject_dimensions: Vec<String>,
    /// Required instrument/reporting/cleanup evidence.
    pub instrument_evidence: Vec<InstrumentEvidence>,
    /// Evidence stages this cell may ever be evidenced at.
    pub allowed_stages: Vec<EvidenceStage>,
    /// Allowed result vocabulary (subset of the catalog's declared vocabulary).
    pub allowed_results: Vec<String>,
    /// Allowed limitation vocabulary.
    pub allowed_limitations: Vec<String>,
    /// Support-profile consumers permitted to cite this cell.
    pub allowed_profiles: Vec<String>,
    /// Claim ceiling: what a registration of this cell proves and never proves.
    pub claim_ceiling: String,
}

/// Coverage obligation of a catalog. The baseline catalog must cover every
/// baseline scenario of its ledger exactly; an additive family catalog carries
/// no baseline obligation and may cite baseline or optional scenarios of its
/// own ledger (subject to the global one-catalog-per-scenario ownership law).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverageRule {
    ExactLedgerBaseline,
    AdditiveFamily,
}

/// One declarative cell-family catalog: a set of rows sharing one scenario
/// ledger, fixture substrate, stage bound, result vocabulary, and (for the
/// baseline) one core profile every cell must feed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CellCatalog {
    pub catalog_id: String,
    pub catalog_version: u32,
    pub ledger_id: String,
    pub coverage: CoverageRule,
    /// Fixture/expectation owner IDs this catalog's cells may cite. The
    /// baseline substrate is the landed #11369/#7762 fixture set; a family
    /// catalog names its own landed-or-authoritative fixture owners here.
    pub fixture_substrate: Vec<String>,
    /// Stage bound: no cell of this catalog may admit a stage outside it.
    pub allowed_stages: Vec<EvidenceStage>,
    /// Result-token vocabulary this catalog's cells may draw from.
    pub allowed_result_vocabulary: Vec<String>,
    /// When set, every cell of this catalog must feed this support profile.
    pub core_profile: Option<String>,
    pub cells: Vec<CellRegistration>,
}

/// Validated summary of one catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogSummary {
    pub catalog_id: String,
    pub cell_count: usize,
    pub scenario_ids: std::collections::BTreeSet<String>,
    pub digest: String,
}

/// Validated summary of one whole registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistrySummary {
    pub catalogs: Vec<CatalogSummary>,
    pub cell_count: usize,
    pub digest: String,
}

/// The ledgers current main registers. A family PR appends its landed ledger
/// constant here and to [`registry`] — rows never leave their own module.
pub fn scenario_ledgers() -> Vec<ScenarioLedger> {
    vec![
        scenario_ledger::vim_bdd_ledger_11371(),
        freshness::freshness_action_ledger(),
        save_format::save_action_ledger(),
        activation::activation_action_ledger(),
        recovery::recovery_action_ledger(),
        lifecycle::lifecycle_action_ledger(),
    ]
}

/// The catalogs current main registers. The aggregation point is code: each
/// additive family is one module plus one line here, never a hand-edited
/// merged row list.
pub fn registry() -> Vec<CellCatalog> {
    vec![
        baseline::baseline_catalog(),
        freshness::freshness_catalog(),
        save_format::save_catalog(),
        activation::activation_catalog(),
        recovery::recovery_catalog(),
        lifecycle::lifecycle_catalog(),
    ]
}

/// Validate the compiled registry of current main: the shared cross-catalog
/// laws, then every registered family's own laws.
pub fn validate_compiled_registry() -> Result<RegistrySummary> {
    let summary = validate_registry(&registry(), &scenario_ledgers())?;
    freshness::validate_family_laws()?;
    save_format::validate_family_laws()?;
    activation::validate_family_laws()?;
    recovery::validate_family_laws()?;
    lifecycle::validate_family_laws()?;
    Ok(summary)
}

/// Validate a whole registry: every catalog against its declared ledger, then
/// the cross-catalog laws (unique cell IDs, one owning catalog per scenario,
/// unique catalog/ledger identities).
pub fn validate_registry(
    catalogs: &[CellCatalog],
    ledgers: &[ScenarioLedger],
) -> Result<RegistrySummary> {
    let mut ledger_by_id = BTreeMap::new();
    for ledger in ledgers {
        validate_ledger(ledger)?;
        ensure!(
            ledger_by_id.insert(ledger.ledger_id.as_str(), ledger).is_none(),
            "duplicate scenario ledger id: {}",
            ledger.ledger_id
        );
    }
    ensure!(!ledgers.is_empty(), "a registry requires at least one scenario ledger");
    ensure!(!catalogs.is_empty(), "a registry requires at least one cell catalog");

    let mut summaries = Vec::new();
    let mut catalog_ids = BTreeSet::new();
    let mut cell_owner_by_id: BTreeMap<String, String> = BTreeMap::new();
    let mut catalog_by_scenario: BTreeMap<String, String> = BTreeMap::new();

    for catalog in catalogs {
        ensure!(
            catalog_ids.insert(catalog.catalog_id.as_str()),
            "duplicate catalog id: {}",
            catalog.catalog_id
        );
        ensure!(
            catalog.catalog_version >= 1,
            "catalog {} must carry a positive version",
            catalog.catalog_id
        );
        let ledger = ledger_by_id.get(catalog.ledger_id.as_str()).copied().with_context(|| {
            format!(
                "catalog {} references unknown scenario ledger {}",
                catalog.catalog_id, catalog.ledger_id
            )
        })?;
        let summary = validate_catalog(catalog, ledger)?;
        for scenario in &summary.scenario_ids {
            if let Some(owner) = catalog_by_scenario.get(scenario) {
                bail!(
                    "scenario {scenario} is claimed by catalogs {owner} and {}",
                    catalog.catalog_id
                );
            }
            catalog_by_scenario.insert(scenario.clone(), catalog.catalog_id.clone());
        }
        for cell in &catalog.cells {
            if let Some(owner) = cell_owner_by_id.get(&cell.cell_id) {
                bail!(
                    "cell id {} is registered by catalogs {owner} and {}",
                    cell.cell_id,
                    catalog.catalog_id
                );
            }
            cell_owner_by_id.insert(cell.cell_id.clone(), catalog.catalog_id.clone());
        }
        summaries.push(summary);
    }

    let cell_count: usize = summaries.iter().map(|summary| summary.cell_count).sum();
    Ok(RegistrySummary {
        digest: registry_digest_from(&summaries)?,
        catalogs: summaries,
        cell_count,
    })
}

/// Structural checks on one ledger independent of any catalog.
fn validate_ledger(ledger: &ScenarioLedger) -> Result<()> {
    ensure!(
        is_reason_token(&ledger.ledger_id),
        "ledger id must be a stable reason token: {}",
        ledger.ledger_id
    );
    ensure!(
        !ledger.owning_authority.trim().is_empty(),
        "ledger {} must record its owning authority",
        ledger.ledger_id
    );
    ensure!(
        !ledger.scenarios.is_empty(),
        "ledger {} must contain at least one scenario",
        ledger.ledger_id
    );
    let mut seen = BTreeSet::new();
    for scenario in &ledger.scenarios {
        ensure!(
            is_reason_token(&scenario.id),
            "scenario id must be a stable reason token: {}",
            scenario.id
        );
        ensure!(
            seen.insert(scenario.id.as_str()),
            "duplicate scenario id in ledger {}: {}",
            ledger.ledger_id,
            scenario.id
        );
    }
    Ok(())
}

/// Validate one catalog (its own laws) and return its summary with digest.
pub fn validate_catalog(catalog: &CellCatalog, ledger: &ScenarioLedger) -> Result<CatalogSummary> {
    ensure!(
        catalog.ledger_id == ledger.ledger_id,
        "catalog {} declares ledger {} but was validated against ledger {}",
        catalog.catalog_id,
        catalog.ledger_id,
        ledger.ledger_id
    );
    ensure!(
        !catalog.fixture_substrate.is_empty(),
        "catalog {} must declare a non-empty fixture substrate",
        catalog.catalog_id
    );
    let mut substrate = BTreeSet::new();
    for fixture in &catalog.fixture_substrate {
        ensure!(
            is_reason_token(fixture),
            "fixture substrate id must be a stable reason token: {fixture}"
        );
        ensure!(substrate.insert(fixture.as_str()), "duplicate fixture substrate id: {fixture}");
    }
    ensure!(
        !catalog.allowed_stages.is_empty(),
        "catalog {} must declare a non-empty stage bound",
        catalog.catalog_id
    );
    let mut stage_bound = BTreeSet::new();
    for stage in &catalog.allowed_stages {
        ensure!(
            stage_bound.insert(*stage),
            "duplicate stage in bound of catalog {}",
            catalog.catalog_id
        );
    }
    ensure!(
        !catalog.allowed_result_vocabulary.is_empty(),
        "catalog {} must declare a non-empty result vocabulary",
        catalog.catalog_id
    );
    let mut vocabulary = BTreeSet::new();
    for token in &catalog.allowed_result_vocabulary {
        ensure!(
            is_reason_token(token),
            "result vocabulary token must be a stable reason token: {token}"
        );
        ensure!(vocabulary.insert(token.as_str()), "duplicate result vocabulary token: {token}");
    }
    if let Some(profile) = &catalog.core_profile {
        ensure!(
            KNOWN_PROFILES.contains(&profile.as_str()),
            "catalog {} names unknown core profile {profile}",
            catalog.catalog_id
        );
    }

    ensure!(
        !catalog.cells.is_empty(),
        "catalog {} must register at least one cell",
        catalog.catalog_id
    );
    let scenario_by_id: BTreeMap<&str, &Scenario> =
        ledger.scenarios.iter().map(|scenario| (scenario.id.as_str(), scenario)).collect();
    let pinned_subject = vim_vim_lsp_subject();

    let mut seen_cells = BTreeSet::new();
    let mut cited_scenarios = std::collections::BTreeSet::new();
    for cell in &catalog.cells {
        validate_cell(cell, catalog, &scenario_by_id, &substrate, &vocabulary, &pinned_subject)?;
        ensure!(
            seen_cells.insert(cell.cell_id.as_str()),
            "duplicate cell id in catalog {}: {}",
            catalog.catalog_id,
            cell.cell_id
        );
        cited_scenarios.extend(cell.scenario_owners.iter().cloned());
    }

    if catalog.coverage == CoverageRule::ExactLedgerBaseline {
        for scenario in &ledger.scenarios {
            if scenario.class == ScenarioClass::Baseline {
                ensure!(
                    cited_scenarios.contains(&scenario.id),
                    "baseline scenario {} of ledger {} is not covered by catalog {}",
                    scenario.id,
                    ledger.ledger_id,
                    catalog.catalog_id
                );
            }
        }
    }

    let cell_count = catalog.cells.len();
    Ok(CatalogSummary {
        catalog_id: catalog.catalog_id.clone(),
        cell_count,
        scenario_ids: cited_scenarios,
        digest: catalog_digest(catalog)?,
    })
}

/// Validate one cell registration against its catalog and ledger.
fn validate_cell(
    cell: &CellRegistration,
    catalog: &CellCatalog,
    scenario_by_id: &BTreeMap<&str, &Scenario>,
    substrate: &BTreeSet<&str>,
    vocabulary: &BTreeSet<&str>,
    pinned_subject: &CellSubject,
) -> Result<()> {
    let Some(rest) = cell.cell_id.strip_prefix(CELL_ID_PREFIX) else {
        bail!("cell id {} is outside the {CELL_ID_PREFIX} namespace", cell.cell_id);
    };
    let segments: Vec<&str> = rest.split('.').collect();
    ensure!(
        segments.len() == 2 && segments.iter().all(|segment| is_reason_token(segment)),
        "cell id {} must be {CELL_ID_PREFIX}<family>.<name> with stable tokens",
        cell.cell_id
    );
    ensure!(cell.cell_version >= 1, "cell {} must carry a positive version", cell.cell_id);

    ensure!(
        !cell.scenario_owners.is_empty(),
        "cell {} must cite at least one BDD scenario owner",
        cell.cell_id
    );
    let mut scenarios = BTreeSet::new();
    for scenario in &cell.scenario_owners {
        ensure!(
            is_reason_token(scenario),
            "scenario owner must be a stable reason token: {scenario}"
        );
        ensure!(
            scenarios.insert(scenario.as_str()),
            "duplicate scenario owner {scenario} in cell {}",
            cell.cell_id
        );
        let landed = scenario_by_id.get(scenario.as_str()).with_context(|| {
            format!(
                "cell {} cites scenario {scenario} absent from ledger {}",
                cell.cell_id, catalog.ledger_id
            )
        })?;
        ensure!(
            !(catalog.coverage == CoverageRule::ExactLedgerBaseline
                && landed.class == ScenarioClass::Optional),
            "cell {} cites optional scenario {scenario} from inside a baseline-coverage catalog",
            cell.cell_id
        );
    }

    ensure!(
        !cell.fixture_owners.is_empty(),
        "cell {} must cite at least one fixture owner",
        cell.cell_id
    );
    let mut fixtures = BTreeSet::new();
    for fixture in &cell.fixture_owners {
        ensure!(
            fixtures.insert(fixture.as_str()),
            "duplicate fixture owner {fixture} in cell {}",
            cell.cell_id
        );
        ensure!(
            substrate.contains(fixture.as_str()),
            "cell {} requires fixture owner {fixture} absent from catalog {} substrate",
            cell.cell_id,
            catalog.catalog_id
        );
    }

    ensure!(
        cell.subject == *pinned_subject,
        "cell {} binds subject {} which is not the pinned Vim + vim-lsp + perllsp --stdio subject",
        cell.cell_id,
        cell.subject.client_id
    );

    ensure!(
        is_reason_token(&cell.observation_class),
        "cell {} observation class must be a stable reason token: {}",
        cell.cell_id,
        cell.observation_class
    );

    ensure!(
        !cell.subject_dimensions.is_empty(),
        "cell {} must require at least one exact subject dimension",
        cell.cell_id
    );
    let mut dimensions = BTreeSet::new();
    for dimension in &cell.subject_dimensions {
        ensure!(
            is_reason_token(dimension),
            "subject dimension must be a stable reason token: {dimension}"
        );
        ensure!(
            dimensions.insert(dimension.as_str()),
            "duplicate subject dimension {dimension} in cell {}",
            cell.cell_id
        );
    }

    ensure!(
        !cell.instrument_evidence.is_empty(),
        "cell {} must require at least one instrument/reporting/cleanup evidence",
        cell.cell_id
    );
    let mut instruments = BTreeSet::new();
    for instrument in &cell.instrument_evidence {
        ensure!(
            instruments.insert(*instrument),
            "duplicate instrument evidence {instrument:?} in cell {}",
            cell.cell_id
        );
    }

    ensure!(
        !cell.allowed_stages.is_empty(),
        "cell {} must admit at least one evidence stage",
        cell.cell_id
    );
    let mut stages = BTreeSet::new();
    for stage in &cell.allowed_stages {
        ensure!(
            stages.insert(*stage),
            "duplicate allowed stage {stage:?} in cell {}",
            cell.cell_id
        );
        ensure!(
            catalog.allowed_stages.contains(stage),
            "cell {} admits stage {stage:?} outside catalog {} stage bound",
            cell.cell_id,
            catalog.catalog_id
        );
    }

    ensure!(
        !cell.allowed_results.is_empty(),
        "cell {} must admit at least one result",
        cell.cell_id
    );
    let mut results = BTreeSet::new();
    for result in &cell.allowed_results {
        ensure!(is_reason_token(result), "result token must be a stable reason token: {result}");
        ensure!(
            results.insert(result.as_str()),
            "duplicate allowed result {result} in cell {}",
            cell.cell_id
        );
        ensure!(
            vocabulary.contains(result.as_str()),
            "cell {} admits result {result} outside catalog {} vocabulary",
            cell.cell_id,
            catalog.catalog_id
        );
    }
    let limitation_required = cell
        .allowed_results
        .iter()
        .any(|result| LIMITATION_REQUIRING_RESULTS.contains(&result.as_str()));
    ensure!(
        !limitation_required || !cell.allowed_limitations.is_empty(),
        "cell {} admits a limitation-requiring result but no limitation vocabulary",
        cell.cell_id
    );
    let mut limitations = BTreeSet::new();
    for limitation in &cell.allowed_limitations {
        ensure!(
            is_reason_token(limitation),
            "limitation token must be a stable reason token: {limitation}"
        );
        ensure!(
            limitations.insert(limitation.as_str()),
            "duplicate allowed limitation {limitation} in cell {}",
            cell.cell_id
        );
    }

    ensure!(
        !cell.allowed_profiles.is_empty(),
        "cell {} must name at least one support-profile consumer",
        cell.cell_id
    );
    let mut profiles = BTreeSet::new();
    for profile in &cell.allowed_profiles {
        ensure!(is_reason_token(profile), "profile token must be a stable reason token: {profile}");
        ensure!(
            profiles.insert(profile.as_str()),
            "duplicate allowed profile {profile} in cell {}",
            cell.cell_id
        );
        ensure!(
            KNOWN_PROFILES.contains(&profile.as_str()),
            "cell {} names unknown support profile {profile}",
            cell.cell_id
        );
    }
    if let Some(core) = &catalog.core_profile {
        ensure!(
            profiles.contains(core.as_str()),
            "cell {} does not feed core profile {core} of catalog {}",
            cell.cell_id,
            catalog.catalog_id
        );
    }

    ensure!(
        !cell.claim_ceiling.trim().is_empty(),
        "cell {} must record a claim ceiling",
        cell.cell_id
    );
    Ok(())
}

/// Stable digest of one cell's full binding. Order-insensitive over the owner,
/// dimension, evidence, and vocabulary lists; sensitive to every identity,
/// version, binding, and boundary field.
pub fn cell_digest(cell: &CellRegistration) -> Result<String> {
    let view = CellDigestView {
        cell_id: cell.cell_id.clone(),
        cell_version: cell.cell_version,
        scenario_owners: sorted(cell.scenario_owners.clone()),
        fixture_owners: sorted(cell.fixture_owners.clone()),
        subject: cell.subject.clone(),
        observation_class: cell.observation_class.clone(),
        subject_dimensions: sorted(cell.subject_dimensions.clone()),
        instrument_evidence: sorted_wire(&cell.instrument_evidence)?,
        allowed_stages: sorted_wire(&cell.allowed_stages)?,
        allowed_results: sorted(cell.allowed_results.clone()),
        allowed_limitations: sorted(cell.allowed_limitations.clone()),
        allowed_profiles: sorted(cell.allowed_profiles.clone()),
        claim_ceiling: cell.claim_ceiling.clone(),
    };
    let canonical = serde_json::to_string(&view)
        .with_context(|| format!("serializing cell binding for digest: {}", cell.cell_id))?;
    digest_of(canonical.as_bytes())
}

/// Stable digest of one catalog: its identity/version plus every cell digest,
/// order-insensitive over cells.
pub fn catalog_digest(catalog: &CellCatalog) -> Result<String> {
    let mut digests = Vec::new();
    for cell in &catalog.cells {
        digests.push(cell_digest(cell)?);
    }
    digests.sort_unstable();
    let canonical = serde_json::to_string(&CatalogDigestView {
        catalog_id: catalog.catalog_id.clone(),
        catalog_version: catalog.catalog_version,
        ledger_id: catalog.ledger_id.clone(),
        coverage: wire(&catalog.coverage)?,
        fixture_substrate: sorted(catalog.fixture_substrate.clone()),
        allowed_stages: sorted_wire(&catalog.allowed_stages)?,
        allowed_result_vocabulary: sorted(catalog.allowed_result_vocabulary.clone()),
        core_profile: catalog.core_profile.clone(),
        cell_digests: digests,
    })
    .with_context(|| format!("serializing catalog digest: {}", catalog.catalog_id))?;
    digest_of(canonical.as_bytes())
}

fn registry_digest_from(summaries: &[CatalogSummary]) -> Result<String> {
    let mut digests: Vec<&str> = summaries.iter().map(|summary| summary.digest.as_str()).collect();
    digests.sort_unstable();
    let canonical = serde_json::to_string(&digests).context("serializing registry digest")?;
    digest_of(canonical.as_bytes())
}

fn digest_of(bytes: &[u8]) -> Result<String> {
    // Same spelling rule as `client_compat_fixture::digest_identity`: the
    // byte-wise hex walk keeps the identity stable across sha2 versions.
    let mut identity = String::with_capacity("sha256:".len() + 64);
    identity.push_str("sha256:");
    for byte in Sha256::digest(bytes) {
        write!(&mut identity, "{byte:02x}")?;
    }
    Ok(identity)
}

fn sorted(mut values: Vec<String>) -> Vec<String> {
    values.sort_unstable();
    values
}

fn sorted_wire<T: Serialize>(values: &[T]) -> Result<Vec<String>> {
    let mut wired = Vec::new();
    for value in values {
        wired.push(wire(value)?);
    }
    wired.sort_unstable();
    Ok(wired)
}

/// The wire spelling of an enum, taken from its own serialization so a digest
/// can never drift from what the contract actually writes. Same rule as
/// `editor_client_compat`'s private helper, kept local so this module does not
/// reach into another contract's internals.
fn wire(value: &impl Serialize) -> Result<String> {
    match serde_json::to_value(value)? {
        serde_json::Value::String(text) => Ok(text),
        other => bail!("expected a string wire spelling, found {other}"),
    }
}

/// Canonical digest projection of a cell binding.
#[derive(Serialize)]
struct CellDigestView {
    cell_id: String,
    cell_version: u32,
    scenario_owners: Vec<String>,
    fixture_owners: Vec<String>,
    subject: CellSubject,
    observation_class: String,
    subject_dimensions: Vec<String>,
    instrument_evidence: Vec<String>,
    allowed_stages: Vec<String>,
    allowed_results: Vec<String>,
    allowed_limitations: Vec<String>,
    allowed_profiles: Vec<String>,
    claim_ceiling: String,
}

#[derive(Serialize)]
/// Canonical digest projection of a catalog binding: identity and version,
/// every catalog-level semantic (ledger, coverage rule, fixture substrate,
/// stage bound, result vocabulary, core profile), and every cell digest. A
/// change to any binding surface with the version held fixed therefore
/// changes the advertised identity.
struct CatalogDigestView {
    catalog_id: String,
    catalog_version: u32,
    ledger_id: String,
    coverage: String,
    fixture_substrate: Vec<String>,
    allowed_stages: Vec<String>,
    allowed_result_vocabulary: Vec<String>,
    core_profile: Option<String>,
    cell_digests: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_spellings_are_snake_case() -> Result<()> {
        ensure!(
            wire(&InstrumentEvidence::CapabilitySnapshot)? == "capability_snapshot",
            "instrument wire spelling drifted"
        );
        ensure!(
            wire(&ScenarioClass::Optional)? == "optional",
            "scenario class wire spelling drifted"
        );
        Ok(())
    }
}
