//! Contract tests for the #11374 additive Vim/vim-lsp cell-catalog API.
//!
//! Positive proof: the compiled registry (the #11371 baseline catalog plus the
//! #11381 freshness family catalog) validates, the baseline covers exactly the
//! 23 baseline #11371 scenarios, the freshness family covers exactly its
//! landed #11380 action vocabulary, both bind only fixture authorities that
//! exist on disk, and digests are deterministic.
//!
//! Negative controls: every fail-closed law of the registration model —
//! duplicate/unknown/conflicting cell IDs, unknown or optional scenarios,
//! coverage gaps, absent fixture owners, cross-client subjects, stage
//! escapes, version and vocabulary violations, missing profiles and ceilings —
//! plus the #11381 freshness family laws (landed-action observation classes,
//! ledger/vocabulary mirroring, required dimensions, fail/not_proven
//! expressibility, action coverage) are executed as mutations of otherwise
//! valid catalogs and must be rejected for their own reason.
//!
//! Forward compatibility: #11384-shaped save-family cells register through
//! the same API without changing any baseline or freshness identity (additive
//! extension without semantic drift).

use anyhow::{Context, Result, bail, ensure};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use xtask::editor_client_compat::EvidenceStage;
use xtask::vim_lsp_cell_catalog::{
    self as catalog, CellCatalog, CellRegistration, CoverageRule, InstrumentEvidence,
    RegistrySummary, Scenario, ScenarioClass, ScenarioLedger, baseline, freshness, save_format,
    scenario_ledger,
};

fn repository_root() -> Result<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .context("xtask must live below the repository root")
}

/// The 17 cell IDs #11374 publishes as the baseline catalog, pinned so a
/// rename cannot slip in as an edit.
const PUBLISHED_BASELINE_CELL_IDS: &[&str] = &[
    "vim.vim_lsp.core.activation",
    "vim.vim_lsp.core.root",
    "vim.vim_lsp.core.bootstrap",
    "vim.vim_lsp.core.diagnostics",
    "vim.vim_lsp.completion.accept_plain",
    "vim.vim_lsp.navigation.hover",
    "vim.vim_lsp.navigation.definition",
    "vim.vim_lsp.navigation.references",
    "vim.vim_lsp.edit.rename",
    "vim.vim_lsp.edit.workspace_edit",
    "vim.vim_lsp.edit.format_explicit",
    "vim.vim_lsp.config.workspace_effect",
    "vim.vim_lsp.position.non_bmp",
    "vim.vim_lsp.sync.did_change",
    "vim.vim_lsp.currentness.post_edit",
    "vim.vim_lsp.lifecycle.close_reopen",
    "vim.vim_lsp.lifecycle.baseline_cleanup",
];

const BASELINE_SCENARIO_COUNT: usize = 23;

/// The six freshness cell IDs #11381 publishes, pinned so a rename or an ad
/// hoc addition cannot slip in as an edit.
const PUBLISHED_FRESHNESS_CELL_IDS: &[&str] = &[
    "vim.vim_lsp.freshness.route",
    "vim.vim_lsp.freshness.external_source",
    "vim.vim_lsp.freshness.project_config",
    "vim.vim_lsp.freshness.client_settings",
    "vim.vim_lsp.freshness.stale_generation_rejected",
    "vim.vim_lsp.freshness.provider_ownership",
];

const FRESHNESS_ACTION_COUNT: usize = 10;

/// The seven save cell IDs #11384 publishes, pinned so a rename or an ad hoc
/// addition cannot slip in as an edit.
const PUBLISHED_SAVE_CELL_IDS: &[&str] = &[
    "vim.vim_lsp.save.route",
    "vim.vim_lsp.save.invocation_cardinality",
    "vim.vim_lsp.save.format_applied",
    "vim.vim_lsp.save.format_no_change",
    "vim.vim_lsp.save.disabled_or_refused",
    "vim.vim_lsp.save.failure",
    "vim.vim_lsp.save.stale_result_rejected",
];

const SAVE_ACTION_COUNT: usize = 5;

fn validate_baseline_with(
    mutation: impl FnOnce(&mut CellCatalog) -> Result<()>,
) -> Result<RegistrySummary> {
    let mut mutated = baseline::baseline_catalog();
    mutation(&mut mutated)?;
    catalog::validate_registry(&[mutated], &[scenario_ledger::vim_bdd_ledger_11371()])
}

fn cell_mut<'a>(catalog: &'a mut CellCatalog, cell_id: &str) -> Result<&'a mut CellRegistration> {
    catalog
        .cells
        .iter_mut()
        .find(|cell| cell.cell_id == cell_id)
        .with_context(|| format!("baseline catalog omitted cell {cell_id}"))
}

/// Assert that a mutated baseline registry is rejected, for a reason
/// containing `needle`.
fn assert_rejects(
    mutation: impl FnOnce(&mut CellCatalog) -> Result<()>,
    needle: &str,
) -> Result<()> {
    let error = match validate_baseline_with(mutation) {
        Ok(_) => {
            bail!("mutated baseline registry was accepted; expected rejection containing {needle}")
        }
        Err(error) => error.to_string(),
    };
    ensure!(
        error.contains(needle),
        "wrong rejection reason: {error} (wanted something containing {needle})"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Compiled registry: positive proof
// ---------------------------------------------------------------------------

#[test]
fn compiled_registry_validates_covers_baseline_and_is_deterministic() -> Result<()> {
    let first = catalog::validate_compiled_registry()?;
    let second = catalog::validate_compiled_registry()?;
    ensure!(first == second, "registry validation is not deterministic across runs");
    let expected_cells = PUBLISHED_BASELINE_CELL_IDS.len()
        + PUBLISHED_FRESHNESS_CELL_IDS.len()
        + PUBLISHED_SAVE_CELL_IDS.len();
    ensure!(
        first.cell_count == expected_cells,
        "compiled registry registers {} cells, expected {expected_cells}",
        first.cell_count
    );

    let baseline_summary = first
        .catalogs
        .iter()
        .find(|summary| summary.catalog_id == baseline::BASELINE_CATALOG_ID)
        .context("compiled registry omitted the baseline catalog")?;
    ensure!(
        baseline_summary.cell_count == PUBLISHED_BASELINE_CELL_IDS.len(),
        "baseline catalog carries {} cells, expected {}",
        baseline_summary.cell_count,
        PUBLISHED_BASELINE_CELL_IDS.len()
    );
    ensure!(
        baseline_summary.scenario_ids.len() == BASELINE_SCENARIO_COUNT,
        "baseline catalog cites {} scenarios, expected {BASELINE_SCENARIO_COUNT}",
        baseline_summary.scenario_ids.len()
    );
    for scenario in &baseline_summary.scenario_ids {
        ensure!(
            !scenario.starts_with("vim.bdd.opt."),
            "optional scenario {scenario} entered the baseline catalog"
        );
    }

    let compiled = baseline::baseline_catalog();
    let registered: BTreeSet<&str> =
        compiled.cells.iter().map(|cell| cell.cell_id.as_str()).collect();
    for published in PUBLISHED_BASELINE_CELL_IDS {
        ensure!(
            registered.contains(published),
            "published baseline cell id {published} is missing from the compiled catalog"
        );
    }
    ensure!(
        registered.len() == PUBLISHED_BASELINE_CELL_IDS.len(),
        "compiled catalog registers extra cells beyond the published table"
    );

    let freshness_summary = first
        .catalogs
        .iter()
        .find(|summary| summary.catalog_id == freshness::FRESHNESS_CATALOG_ID)
        .context("compiled registry omitted the freshness family catalog")?;
    ensure!(
        freshness_summary.cell_count == PUBLISHED_FRESHNESS_CELL_IDS.len(),
        "freshness catalog carries {} cells, expected {}",
        freshness_summary.cell_count,
        PUBLISHED_FRESHNESS_CELL_IDS.len()
    );
    ensure!(
        freshness_summary.scenario_ids.len() == FRESHNESS_ACTION_COUNT,
        "freshness catalog cites {} scenarios, expected the {FRESHNESS_ACTION_COUNT} landed #11380 freshness actions",
        freshness_summary.scenario_ids.len()
    );
    let compiled_freshness = freshness::freshness_catalog();
    let freshness_ids: BTreeSet<&str> =
        compiled_freshness.cells.iter().map(|cell| cell.cell_id.as_str()).collect();
    for published in PUBLISHED_FRESHNESS_CELL_IDS {
        ensure!(
            freshness_ids.contains(published),
            "published freshness cell id {published} is missing from the compiled catalog"
        );
    }
    ensure!(
        freshness_ids.len() == PUBLISHED_FRESHNESS_CELL_IDS.len(),
        "freshness catalog registers cells beyond the published table"
    );

    let save_summary = first
        .catalogs
        .iter()
        .find(|summary| summary.catalog_id == save_format::SAVE_CATALOG_ID)
        .context("compiled registry omitted the save family catalog")?;
    ensure!(
        save_summary.cell_count == PUBLISHED_SAVE_CELL_IDS.len(),
        "save catalog carries {} cells, expected {}",
        save_summary.cell_count,
        PUBLISHED_SAVE_CELL_IDS.len()
    );
    ensure!(
        save_summary.scenario_ids.len() == SAVE_ACTION_COUNT - 1,
        "save catalog cites {} scenarios, expected the {} owned landed #11380 save actions",
        save_summary.scenario_ids.len(),
        SAVE_ACTION_COUNT - 1
    );
    let compiled_save = save_format::save_catalog();
    let save_ids: BTreeSet<&str> =
        compiled_save.cells.iter().map(|cell| cell.cell_id.as_str()).collect();
    for published in PUBLISHED_SAVE_CELL_IDS {
        ensure!(
            save_ids.contains(published),
            "published save cell id {published} is missing from the compiled catalog"
        );
    }
    ensure!(
        save_ids.len() == PUBLISHED_SAVE_CELL_IDS.len(),
        "save catalog registers cells beyond the published table"
    );
    Ok(())
}

#[test]
fn baseline_result_vocabulary_matches_the_documented_baseline_dispositions() -> Result<()> {
    let compiled = baseline::baseline_catalog();
    let vocabulary: BTreeSet<&str> =
        compiled.allowed_result_vocabulary.iter().map(|token| token.as_str()).collect();
    // Exactly the dispositions the generic `ObservationResult` can serialize.
    let expected: BTreeSet<&str> =
        ["pass", "fail", "partial", "not_proven", "unsupported"].into_iter().collect();
    ensure!(
        vocabulary == expected,
        "baseline result vocabulary drifted from the receipt-serializable dispositions: {vocabulary:?}"
    );
    ensure!(
        !vocabulary.contains("instrument_failed"),
        "instrument_failed is a receipt-level failure class (#7777), not a baseline cell result"
    );
    // Exposure states ride as limitation tokens, never as baseline results.
    let limitation_carried = compiled
        .cells
        .iter()
        .any(|cell| cell.allowed_limitations.iter().any(|token| token == "client_not_exposed"));
    ensure!(limitation_carried, "client_not_exposed must remain an admitted limitation token");
    Ok(())
}

// ---------------------------------------------------------------------------
// Landed-authority bindings: ledger mirror and fixture substrate
// ---------------------------------------------------------------------------

/// Scan one #11371 spec file for concrete `vim.bdd.<family>.<nn>` tokens.
fn scan_bdd_ids(path: &Path) -> Result<BTreeSet<String>> {
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut ids = BTreeSet::new();
    let mut rest = text.as_str();
    while let Some(index) = rest.find("vim.bdd.") {
        rest = &rest[index..];
        let token: String = rest
            .chars()
            .take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '.')
            .collect();
        let valid = token
            .strip_prefix("vim.bdd.")
            .and_then(|suffix| suffix.split_once('.'))
            .is_some_and(|(family, number)| {
                matches!(family, "attach" | "nav" | "edit" | "lifecycle" | "opt")
                    && number.len() == 2
                    && number.bytes().all(|byte| byte.is_ascii_digit())
            });
        if valid {
            ids.insert(token.clone());
        }
        rest = &rest[token.len()..];
    }
    Ok(ids)
}

#[test]
fn ledger_mirror_matches_the_landed_11371_spec_files() -> Result<()> {
    let root = repository_root()?;
    let mut landed = scan_bdd_ids(&root.join(".spec/11371-vim-bdd-journeys/context.md"))?;
    landed.extend(scan_bdd_ids(&root.join(".spec/11371-vim-bdd-journeys/acceptance.md"))?);
    let mirror: BTreeSet<String> = scenario_ledger::vim_bdd_ledger_11371()
        .scenarios
        .iter()
        .map(|scenario| scenario.id.clone())
        .collect();
    ensure!(
        landed.len() == 30,
        "expected to find the 30 published scenario ids in the spec files, found {}",
        landed.len()
    );
    ensure!(landed == mirror, "ledger mirror drifted from the landed spec files");

    for scenario in &scenario_ledger::vim_bdd_ledger_11371().scenarios {
        let optional = scenario.id.starts_with("vim.bdd.opt.");
        ensure!(
            (optional && scenario.class == ScenarioClass::Optional)
                || (!optional && scenario.class == ScenarioClass::Baseline),
            "scenario {} carries a class that contradicts the opt-family split",
            scenario.id
        );
    }
    Ok(())
}

#[test]
fn baseline_fixture_substrate_is_landed_on_disk() -> Result<()> {
    let root = repository_root()?;
    for fixture in baseline::BASELINE_FIXTURE_SUBSTRATE {
        let path = root.join(".ci/editor-clients").join(format!("{fixture}.json"));
        ensure!(
            path.is_file(),
            "baseline fixture substrate id {fixture} has no landed authority artifact at {}",
            path.display()
        );
    }
    ensure!(
        baseline::BASELINE_FIXTURE_SUBSTRATE.len() == 4,
        "expected the four landed vim-vim-lsp fixture authorities in the substrate"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// #11381 freshness family: landed-authority bindings and family laws
// ---------------------------------------------------------------------------

#[test]
fn freshness_ledger_mirrors_the_landed_11380_action_vocabulary() -> Result<()> {
    let ledger = freshness::freshness_action_ledger();
    let mirrored: BTreeSet<String> =
        ledger.scenarios.iter().map(|scenario| scenario.id.clone()).collect();
    let landed: BTreeSet<String> = xtask::vim_lsp_specialized_driver::ACTIONS
        .iter()
        .filter(|action| {
            action.family == xtask::vim_lsp_specialized_driver::ActionFamily::Freshness
        })
        .map(|action| action.action_id.to_string())
        .collect();
    ensure!(
        mirrored == landed && mirrored.len() == FRESHNESS_ACTION_COUNT,
        "freshness ledger drifted from the landed #11380 freshness action vocabulary"
    );
    for scenario in &ledger.scenarios {
        ensure!(
            scenario.class == ScenarioClass::Baseline,
            "freshness action {} must stay a baseline-class landed row",
            scenario.id
        );
    }
    Ok(())
}

#[test]
fn freshness_fixture_substrate_is_landed_on_disk() -> Result<()> {
    let root = repository_root()?;
    for fixture in freshness::FRESHNESS_FIXTURE_SUBSTRATE {
        let path = root.join(".ci/editor-clients").join(format!("{fixture}.json"));
        ensure!(
            path.is_file(),
            "freshness fixture substrate id {fixture} has no landed authority artifact at {}",
            path.display()
        );
    }
    Ok(())
}

#[test]
fn freshness_family_registration_leaves_the_baseline_byte_identical() -> Result<()> {
    let baseline_only = catalog::validate_registry(
        &[baseline::baseline_catalog()],
        &[scenario_ledger::vim_bdd_ledger_11371()],
    )?;
    let baseline_digest = baseline_only
        .catalogs
        .iter()
        .find(|summary| summary.catalog_id == baseline::BASELINE_CATALOG_ID)
        .context("baseline summary missing")?
        .digest
        .clone();
    let baseline_cell_digests: Vec<String> = baseline::baseline_catalog()
        .cells
        .iter()
        .map(catalog::cell_digest)
        .collect::<Result<_>>()?;

    let compiled = catalog::validate_compiled_registry()?;
    let compiled_baseline = compiled
        .catalogs
        .iter()
        .find(|summary| summary.catalog_id == baseline::BASELINE_CATALOG_ID)
        .context("baseline summary missing from the compiled registry")?;
    ensure!(
        compiled_baseline.digest == baseline_digest,
        "the freshness family changed the baseline catalog digest"
    );
    let after: Vec<String> = baseline::baseline_catalog()
        .cells
        .iter()
        .map(catalog::cell_digest)
        .collect::<Result<_>>()?;
    ensure!(after == baseline_cell_digests, "the freshness family changed a baseline cell digest");
    ensure!(
        compiled.digest != baseline_only.digest,
        "registry digest ignored the freshness family"
    );
    Ok(())
}

/// Validate a mutated freshness family catalog against the family laws and
/// then the shared laws over baseline plus the mutated family; both must pass
/// for the mutation to count as accepted.
fn validate_freshness_with(
    mutation: impl FnOnce(&mut CellCatalog) -> Result<()>,
) -> Result<RegistrySummary> {
    let mut mutated = freshness::freshness_catalog();
    mutation(&mut mutated)?;
    freshness::validate_freshness_catalog(&mutated, &freshness::freshness_action_ledger())?;
    catalog::validate_registry(
        &[baseline::baseline_catalog(), mutated],
        &catalog::scenario_ledgers(),
    )
}

fn freshness_cell_mut<'a>(
    catalog: &'a mut CellCatalog,
    cell_id: &str,
) -> Result<&'a mut CellRegistration> {
    catalog
        .cells
        .iter_mut()
        .find(|cell| cell.cell_id == cell_id)
        .with_context(|| format!("freshness catalog omitted cell {cell_id}"))
}

/// Assert that a mutated freshness registry is rejected — by the family laws
/// or the shared laws — for a reason containing `needle`.
fn assert_freshness_rejects(
    mutation: impl FnOnce(&mut CellCatalog) -> Result<()>,
    needle: &str,
) -> Result<()> {
    let error = match validate_freshness_with(mutation) {
        Ok(_) => {
            bail!("mutated freshness registry was accepted; expected rejection containing {needle}")
        }
        Err(error) => error.to_string(),
    };
    ensure!(
        error.contains(needle),
        "wrong rejection reason: {error} (wanted something containing {needle})"
    );
    Ok(())
}

#[test]
fn freshness_event_or_registration_shortcuts_fail_closed() -> Result<()> {
    // A watcher/registration/event/log token is not a landed freshness action
    // and can never classify a freshness cell.
    assert_freshness_rejects(
        |catalog| {
            let cell = freshness_cell_mut(catalog, "vim.vim_lsp.freshness.external_source")?;
            cell.observation_class = "watcher.registration_event".to_string();
            Ok(())
        },
        "is not a landed freshness action",
    )
}

#[test]
fn freshness_cannot_be_filled_by_another_family_or_baseline_row() -> Result<()> {
    // A save-family action cannot classify or own a freshness cell.
    assert_freshness_rejects(
        |catalog| {
            let cell = freshness_cell_mut(catalog, "vim.vim_lsp.freshness.route")?;
            cell.observation_class =
                "vim.vim_lsp.specialized.save_format.observe_save_settlement".to_string();
            Ok(())
        },
        "is not a landed freshness action",
    )?;
    assert_freshness_rejects(
        |catalog| {
            let cell = freshness_cell_mut(catalog, "vim.vim_lsp.freshness.route")?;
            cell.scenario_owners
                .push("vim.vim_lsp.specialized.save_format.observe_save_settlement".to_string());
            Ok(())
        },
        "absent from ledger",
    )?;
    // A baseline scenario stays owned by the baseline catalog.
    assert_freshness_rejects(
        |catalog| {
            let cell = freshness_cell_mut(catalog, "vim.vim_lsp.freshness.project_config")?;
            cell.scenario_owners.push("vim.bdd.lifecycle.03".to_string());
            Ok(())
        },
        "absent from ledger",
    )
}

#[test]
fn freshness_family_vocabulary_and_stage_laws_fail_closed() -> Result<()> {
    // A cell admitting a result outside the family vocabulary fails closed.
    assert_freshness_rejects(
        |catalog| {
            let cell = freshness_cell_mut(catalog, "vim.vim_lsp.freshness.route")?;
            cell.allowed_results.push("stale_promoted_current".to_string());
            Ok(())
        },
        "outside catalog",
    )?;
    // The family vocabulary itself is pinned.
    assert_freshness_rejects(
        |catalog| {
            catalog.allowed_result_vocabulary.push("route_magic_pass".to_string());
            Ok(())
        },
        "vocabulary drifted",
    )?;
    // A cell must be able to fail and to stay honestly unproven.
    assert_freshness_rejects(
        |catalog| {
            let cell = freshness_cell_mut(catalog, "vim.vim_lsp.freshness.external_source")?;
            cell.allowed_results.retain(|token| token != "fail");
            Ok(())
        },
        "must admit fail",
    )?;
    // Stage escapes stay rejected by the shared bound.
    assert_freshness_rejects(
        |catalog| {
            let cell = freshness_cell_mut(catalog, "vim.vim_lsp.freshness.client_settings")?;
            cell.allowed_stages = vec![EvidenceStage::PublicArtifact];
            Ok(())
        },
        "outside catalog",
    )?;
    assert_freshness_rejects(
        |catalog| {
            catalog.allowed_stages = vec![EvidenceStage::ReleaseCandidate];
            Ok(())
        },
        "stage bound is exact_source_local only",
    )
}

#[test]
fn freshness_dimension_and_profile_laws_fail_closed() -> Result<()> {
    assert_freshness_rejects(
        |catalog| {
            let cell = freshness_cell_mut(catalog, "vim.vim_lsp.freshness.external_source")?;
            cell.subject_dimensions.retain(|token| token != "stage.exact_source_local");
            Ok(())
        },
        "required dimension stage.exact_source_local",
    )?;
    assert_freshness_rejects(
        |catalog| {
            let cell = freshness_cell_mut(catalog, "vim.vim_lsp.freshness.provider_ownership")?;
            cell.subject_dimensions.retain(|token| !token.starts_with("generation."));
            Ok(())
        },
        "generation dimension",
    )?;
    assert_freshness_rejects(
        |catalog| {
            let cell = freshness_cell_mut(catalog, "vim.vim_lsp.freshness.route")?;
            cell.allowed_profiles = vec!["vim_public_artifact".to_string()];
            Ok(())
        },
        "may feed only vim_first_class_exact_source",
    )
}

#[test]
fn freshness_coverage_and_identity_laws_fail_closed() -> Result<()> {
    // Every landed freshness action must keep a pre-registered owning cell.
    assert_freshness_rejects(
        |catalog| {
            let cell =
                freshness_cell_mut(catalog, "vim.vim_lsp.freshness.stale_generation_rejected")?;
            cell.scenario_owners =
                vec!["vim.vim_lsp.specialized.freshness.observe_route_and_generation".to_string()];
            cell.observation_class =
                "vim.vim_lsp.specialized.freshness.observe_route_and_generation".to_string();
            Ok(())
        },
        "without a pre-registered cell",
    )?;
    // Duplicate registration inside the family fails closed.
    assert_freshness_rejects(
        |catalog| {
            let clone = catalog.cells[0].clone();
            catalog.cells.push(clone);
            Ok(())
        },
        "duplicate cell id",
    )?;
    // The family assigns no core profile and stays additive.
    assert_freshness_rejects(
        |catalog| {
            catalog.core_profile = Some("vim_actual_client_core".to_string());
            Ok(())
        },
        "assigns no core profile",
    )?;
    assert_freshness_rejects(
        |catalog| {
            catalog.coverage = CoverageRule::ExactLedgerBaseline;
            Ok(())
        },
        "additive",
    )
}

#[test]
fn freshness_cross_client_subjects_fail_closed() -> Result<()> {
    for impostor in ["coc", "yegappan/lsp", "neovim", "vimspector"] {
        assert_freshness_rejects(
            |catalog| {
                let cell = freshness_cell_mut(catalog, "vim.vim_lsp.freshness.route")?;
                cell.subject.client_id = impostor.to_string();
                Ok(())
            },
            "not the pinned Vim + vim-lsp + perllsp --stdio subject",
        )
        .with_context(|| format!("cross-client subject {impostor} was accepted"))?;
    }
    Ok(())
}

#[test]
fn freshness_cell_digests_discriminate_binding_edits() -> Result<()> {
    let compiled = freshness::freshness_catalog();
    let route = compiled
        .cells
        .iter()
        .find(|cell| cell.cell_id == "vim.vim_lsp.freshness.route")
        .context("route cell missing")?;
    let before = catalog::cell_digest(route)?;
    let catalog_before = catalog::catalog_digest(&compiled)?;
    let registry_before = catalog::validate_compiled_registry()?.digest;

    let mut edited = compiled.clone();
    let route = edited
        .cells
        .iter_mut()
        .find(|cell| cell.cell_id == "vim.vim_lsp.freshness.route")
        .context("route cell missing")?;
    route.subject_dimensions.push("generation.host".to_string());
    ensure!(
        before != catalog::cell_digest(route)?,
        "a freshness binding edit did not change the cell digest"
    );
    ensure!(
        catalog_before != catalog::catalog_digest(&edited)?,
        "a freshness binding edit did not change the family catalog digest"
    );
    ensure!(
        registry_before.starts_with("sha256:"),
        "registry digest is not a sha256 identity: {registry_before}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// #11384 save family: landed-authority bindings and family laws
// ---------------------------------------------------------------------------

#[test]
fn save_ledger_mirrors_the_landed_11380_action_vocabulary() -> Result<()> {
    let ledger = save_format::save_action_ledger();
    let mirrored: BTreeSet<String> =
        ledger.scenarios.iter().map(|scenario| scenario.id.clone()).collect();
    let landed: BTreeSet<String> = xtask::vim_lsp_specialized_driver::ACTIONS
        .iter()
        .filter(|action| {
            action.family == xtask::vim_lsp_specialized_driver::ActionFamily::SaveFormat
        })
        .map(|action| action.action_id.to_string())
        .collect();
    ensure!(
        mirrored == landed && mirrored.len() == SAVE_ACTION_COUNT,
        "save ledger drifted from the landed #11380 save_format action vocabulary"
    );
    Ok(())
}

#[test]
fn save_fixture_substrate_is_landed_on_disk() -> Result<()> {
    let root = repository_root()?;
    for fixture in save_format::SAVE_FIXTURE_SUBSTRATE {
        let path = root.join(".ci/editor-clients").join(format!("{fixture}.json"));
        ensure!(
            path.is_file(),
            "save fixture substrate id {fixture} has no landed authority artifact at {}",
            path.display()
        );
    }
    Ok(())
}

#[test]
fn save_family_registration_leaves_earlier_catalogs_byte_identical() -> Result<()> {
    let before = catalog::validate_compiled_registry()?;
    let baseline_before = before
        .catalogs
        .iter()
        .find(|summary| summary.catalog_id == baseline::BASELINE_CATALOG_ID)
        .context("baseline summary missing")?
        .digest
        .clone();
    let freshness_before = before
        .catalogs
        .iter()
        .find(|summary| summary.catalog_id == freshness::FRESHNESS_CATALOG_ID)
        .context("freshness summary missing")?
        .digest
        .clone();

    // Registering the save family over the pre-save registry (baseline +
    // freshness) leaves both prior catalog digests byte-identical.
    let prior_catalogs = vec![baseline::baseline_catalog(), freshness::freshness_catalog()];
    let prior_ledgers =
        vec![scenario_ledger::vim_bdd_ledger_11371(), freshness::freshness_action_ledger()];
    let prior = catalog::validate_registry(&prior_catalogs, &prior_ledgers)?;
    let prior_baseline = prior
        .catalogs
        .iter()
        .find(|summary| summary.catalog_id == baseline::BASELINE_CATALOG_ID)
        .context("baseline summary missing")?
        .digest
        .clone();
    let prior_freshness = prior
        .catalogs
        .iter()
        .find(|summary| summary.catalog_id == freshness::FRESHNESS_CATALOG_ID)
        .context("freshness summary missing")?
        .digest
        .clone();
    ensure!(prior_baseline == baseline_before, "the save family changed the baseline digest");
    ensure!(
        prior_freshness == freshness_before,
        "the save family changed the freshness catalog digest"
    );
    ensure!(before.cell_count == prior.cell_count + PUBLISHED_SAVE_CELL_IDS.len());
    Ok(())
}

/// Validate a mutated save family catalog against the family laws and then
/// the shared laws over the compiled sibling catalogs plus the mutated
/// family; both must pass for the mutation to count as accepted.
fn validate_save_with(
    mutation: impl FnOnce(&mut CellCatalog) -> Result<()>,
) -> Result<RegistrySummary> {
    let mut mutated = save_format::save_catalog();
    mutation(&mut mutated)?;
    save_format::validate_save_catalog(&mutated, &save_format::save_action_ledger())?;
    let mut catalogs = catalog::registry();
    let slot = catalogs
        .iter_mut()
        .find(|candidate| candidate.catalog_id == save_format::SAVE_CATALOG_ID)
        .context("compiled registry omitted the save catalog")?;
    *slot = mutated;
    catalog::validate_registry(&catalogs, &catalog::scenario_ledgers())
}

fn save_cell_mut<'a>(
    catalog: &'a mut CellCatalog,
    cell_id: &str,
) -> Result<&'a mut CellRegistration> {
    catalog
        .cells
        .iter_mut()
        .find(|cell| cell.cell_id == cell_id)
        .with_context(|| format!("save catalog omitted cell {cell_id}"))
}

/// Assert that a mutated save registry is rejected — by the family laws or
/// the shared laws — for a reason containing `needle`.
fn assert_save_rejects(
    mutation: impl FnOnce(&mut CellCatalog) -> Result<()>,
    needle: &str,
) -> Result<()> {
    let error = match validate_save_with(mutation) {
        Ok(_) => {
            bail!("mutated save registry was accepted; expected rejection containing {needle}")
        }
        Err(error) => error.to_string(),
    };
    ensure!(
        error.contains(needle),
        "wrong rejection reason: {error} (wanted something containing {needle})"
    );
    Ok(())
}

#[test]
fn save_manual_comparator_cannot_own_or_classify_save_evidence() -> Result<()> {
    let comparator = "vim.vim_lsp.specialized.save_format.manual_comparator";
    // Citing the comparator as a scenario owner of a save cell fails closed:
    // manual explicit formatting cannot satisfy this family.
    assert_save_rejects(
        |catalog| {
            let cell = save_cell_mut(catalog, "vim.vim_lsp.save.format_applied")?;
            cell.scenario_owners.push(comparator.to_string());
            Ok(())
        },
        "manual comparator run can never be save evidence",
    )?;
    // Classifying via the comparator action fails closed: it is a landed
    // save action, so the owner-binding law is the one that rejects it — a
    // comparator run is nobody's save evidence.
    assert_save_rejects(
        |catalog| {
            let cell = save_cell_mut(catalog, "vim.vim_lsp.save.route")?;
            cell.observation_class = comparator.to_string();
            Ok(())
        },
        "must be one of its own scenario owners",
    )
}

#[test]
fn save_cannot_be_filled_by_another_family_row() -> Result<()> {
    // A freshness action cannot classify or own a save cell.
    assert_save_rejects(
        |catalog| {
            let cell = save_cell_mut(catalog, "vim.vim_lsp.save.route")?;
            cell.observation_class =
                "vim.vim_lsp.specialized.freshness.observe_route_and_generation".to_string();
            Ok(())
        },
        "is not a landed save_format action",
    )?;
    assert_save_rejects(
        |catalog| {
            let cell = save_cell_mut(catalog, "vim.vim_lsp.save.format_applied")?;
            cell.scenario_owners.push(
                "vim.vim_lsp.specialized.freshness.source_mutate_closed_in_place".to_string(),
            );
            Ok(())
        },
        "absent from ledger",
    )
}

#[test]
fn save_family_vocabulary_stage_and_result_laws_fail_closed() -> Result<()> {
    assert_save_rejects(
        |catalog| {
            let cell = save_cell_mut(catalog, "vim.vim_lsp.save.route")?;
            cell.allowed_results.push("manual_format_pass".to_string());
            Ok(())
        },
        "outside catalog",
    )?;
    assert_save_rejects(
        |catalog| {
            catalog.allowed_result_vocabulary.push("manual_format_pass".to_string());
            Ok(())
        },
        "vocabulary drifted",
    )?;
    // The failure cell never admits pass.
    let compiled = save_format::save_catalog();
    let failure = compiled
        .cells
        .iter()
        .find(|cell| cell.cell_id == "vim.vim_lsp.save.failure")
        .context("failure cell missing")?;
    ensure!(
        !failure.allowed_results.iter().any(|result| result == "pass"),
        "the failure cell must never admit pass"
    );
    // Every cell must be able to fail and to stay honestly unproven.
    assert_save_rejects(
        |catalog| {
            let cell = save_cell_mut(catalog, "vim.vim_lsp.save.format_no_change")?;
            cell.allowed_results.retain(|token| token != "not_proven");
            Ok(())
        },
        "must admit fail and not_proven",
    )?;
    assert_save_rejects(
        |catalog| {
            let cell = save_cell_mut(catalog, "vim.vim_lsp.save.format_applied")?;
            cell.allowed_stages = vec![EvidenceStage::ReleaseCandidate];
            Ok(())
        },
        "outside catalog",
    )?;
    assert_save_rejects(
        |catalog| {
            catalog.allowed_stages = vec![EvidenceStage::PublicArtifact];
            Ok(())
        },
        "stage bound is exact_source_local only",
    )
}

#[test]
fn save_dimension_profile_and_coverage_laws_fail_closed() -> Result<()> {
    assert_save_rejects(
        |catalog| {
            let cell = save_cell_mut(catalog, "vim.vim_lsp.save.invocation_cardinality")?;
            cell.subject_dimensions.retain(|token| !token.starts_with("generation."));
            Ok(())
        },
        "generation dimension",
    )?;
    assert_save_rejects(
        |catalog| {
            let cell = save_cell_mut(catalog, "vim.vim_lsp.save.disabled_or_refused")?;
            cell.allowed_profiles = vec!["vim_programme_closeout".to_string()];
            Ok(())
        },
        "may feed only vim_first_class_exact_source",
    )?;
    // Dropping the last owner of a settled action must fail coverage.
    assert_save_rejects(
        |catalog| {
            let cell = save_cell_mut(catalog, "vim.vim_lsp.save.stale_result_rejected")?;
            cell.scenario_owners =
                vec!["vim.vim_lsp.specialized.save_format.observe_save_settlement".to_string()];
            cell.observation_class =
                "vim.vim_lsp.specialized.save_format.observe_save_settlement".to_string();
            Ok(())
        },
        "without a pre-registered cell",
    )?;
    assert_save_rejects(
        |catalog| {
            let clone = catalog.cells[0].clone();
            catalog.cells.push(clone);
            Ok(())
        },
        "duplicate cell id",
    )
}

#[test]
fn save_cross_client_subjects_fail_closed() -> Result<()> {
    for impostor in ["coc", "yegappan/lsp", "neovim"] {
        assert_save_rejects(
            |catalog| {
                let cell = save_cell_mut(catalog, "vim.vim_lsp.save.format_applied")?;
                cell.subject.client_id = impostor.to_string();
                Ok(())
            },
            "not the pinned Vim + vim-lsp + perllsp --stdio subject",
        )
        .with_context(|| format!("cross-client subject {impostor} was accepted"))?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Registration fail-closed negative controls
// ---------------------------------------------------------------------------

#[test]
fn duplicate_cell_ids_fail_closed() -> Result<()> {
    assert_rejects(
        |catalog| {
            let clone = catalog.cells[0].clone();
            catalog.cells.push(clone);
            Ok(())
        },
        "duplicate cell id in catalog",
    )
}

#[test]
fn unknown_or_optional_scenarios_fail_closed() -> Result<()> {
    assert_rejects(
        |catalog| {
            let cell = cell_mut(catalog, "vim.vim_lsp.core.activation")?;
            cell.scenario_owners = vec!["vim.bdd.attach.99".to_string()];
            Ok(())
        },
        "absent from ledger",
    )?;
    assert_rejects(
        |catalog| {
            let cell = cell_mut(catalog, "vim.vim_lsp.core.activation")?;
            cell.scenario_owners = vec!["vim.bdd.opt.01".to_string()];
            Ok(())
        },
        "cites optional scenario",
    )
}

#[test]
fn baseline_coverage_gaps_fail_closed() -> Result<()> {
    assert_rejects(
        |catalog| {
            catalog.cells.retain(|cell| cell.cell_id != "vim.vim_lsp.lifecycle.close_reopen");
            Ok(())
        },
        "is not covered by catalog",
    )
}

#[test]
fn absent_fixture_owners_fail_closed() -> Result<()> {
    assert_rejects(
        |catalog| {
            let cell = cell_mut(catalog, "vim.vim_lsp.core.bootstrap")?;
            cell.fixture_owners = vec!["vim-vim-lsp-freshness-fixture.v1".to_string()];
            Ok(())
        },
        "absent from catalog",
    )
}

#[test]
fn cross_client_subjects_fail_closed() -> Result<()> {
    for impostor in ["coc", "yegappan/lsp", "neovim", "vimspector"] {
        assert_rejects(
            |catalog| {
                let cell = cell_mut(catalog, "vim.vim_lsp.core.bootstrap")?;
                cell.subject.client_id = impostor.to_string();
                Ok(())
            },
            "not the pinned Vim + vim-lsp + perllsp --stdio subject",
        )
        .with_context(|| format!("cross-client subject {impostor} was accepted"))?;
    }
    assert_rejects(
        |catalog| {
            let cell = cell_mut(catalog, "vim.vim_lsp.core.bootstrap")?;
            cell.subject.host_product = "neovim".to_string();
            Ok(())
        },
        "not the pinned Vim + vim-lsp + perllsp --stdio subject",
    )
}

#[test]
fn stage_escapes_fail_closed() -> Result<()> {
    for escape in [EvidenceStage::ReleaseCandidate, EvidenceStage::PublicArtifact] {
        assert_rejects(
            |catalog| {
                let cell = cell_mut(catalog, "vim.vim_lsp.core.bootstrap")?;
                cell.allowed_stages = vec![escape];
                Ok(())
            },
            "outside catalog",
        )
        .with_context(|| format!("stage escape {escape:?} was accepted"))?;
    }
    Ok(())
}

#[test]
fn versioning_and_vocabulary_violations_fail_closed() -> Result<()> {
    assert_rejects(
        |catalog| {
            let cell = cell_mut(catalog, "vim.vim_lsp.core.bootstrap")?;
            cell.cell_version = 0;
            Ok(())
        },
        "positive version",
    )?;
    assert_rejects(
        |catalog| {
            catalog.catalog_version = 0;
            Ok(())
        },
        "positive version",
    )?;
    // Exposure states are limitation tokens in the baseline family: admitting
    // client_not_exposed as a baseline result must fail closed.
    assert_rejects(
        |catalog| {
            let cell = cell_mut(catalog, "vim.vim_lsp.core.bootstrap")?;
            cell.allowed_results = vec!["client_not_exposed".to_string()];
            Ok(())
        },
        "outside catalog",
    )?;
    assert_rejects(
        |catalog| {
            let cell = cell_mut(catalog, "vim.vim_lsp.core.bootstrap")?;
            cell.allowed_results = Vec::new();
            Ok(())
        },
        "at least one result",
    )?;
    assert_rejects(
        |catalog| {
            let cell = cell_mut(catalog, "vim.vim_lsp.core.bootstrap")?;
            cell.allowed_results = vec!["explicit_reload_required".to_string()];
            Ok(())
        },
        "outside catalog",
    )?;
    assert_rejects(
        |catalog| {
            let cell = cell_mut(catalog, "vim.vim_lsp.core.bootstrap")?;
            cell.allowed_results = vec!["partial".to_string()];
            cell.allowed_limitations = Vec::new();
            Ok(())
        },
        "no limitation vocabulary",
    )
}

#[test]
fn profile_and_ceiling_violations_fail_closed() -> Result<()> {
    assert_rejects(
        |catalog| {
            let cell = cell_mut(catalog, "vim.vim_lsp.core.bootstrap")?;
            cell.allowed_profiles = vec!["vim_actual_client_ultra".to_string()];
            Ok(())
        },
        "unknown support profile",
    )?;
    assert_rejects(
        |catalog| {
            let cell = cell_mut(catalog, "vim.vim_lsp.core.bootstrap")?;
            cell.allowed_profiles = vec!["vim_first_class_exact_source".to_string()];
            Ok(())
        },
        "does not feed core profile",
    )?;
    assert_rejects(
        |catalog| {
            let cell = cell_mut(catalog, "vim.vim_lsp.core.bootstrap")?;
            cell.claim_ceiling = "  ".to_string();
            Ok(())
        },
        "claim ceiling",
    )
}

#[test]
fn binding_minimums_and_id_namespace_fail_closed() -> Result<()> {
    assert_rejects(
        |catalog| {
            let cell = cell_mut(catalog, "vim.vim_lsp.core.bootstrap")?;
            cell.instrument_evidence = Vec::new();
            Ok(())
        },
        "instrument/reporting/cleanup evidence",
    )?;
    assert_rejects(
        |catalog| {
            let cell = cell_mut(catalog, "vim.vim_lsp.core.bootstrap")?;
            cell.subject_dimensions = Vec::new();
            Ok(())
        },
        "exact subject dimension",
    )?;
    assert_rejects(
        |catalog| {
            let cell = cell_mut(catalog, "vim.vim_lsp.core.bootstrap")?;
            cell.cell_id = "emacs.coc.activation".to_string();
            Ok(())
        },
        "outside the vim.vim_lsp. namespace",
    )?;
    assert_rejects(
        |catalog| {
            let cell = cell_mut(catalog, "vim.vim_lsp.core.bootstrap")?;
            cell.cell_id = "vim.vim_lsp.core.activation.deep".to_string();
            Ok(())
        },
        "must be vim.vim_lsp.<family>.<name>",
    )?;
    assert_rejects(
        |catalog| {
            catalog.ledger_id = "vim.bdd.20999".to_string();
            Ok(())
        },
        "unknown scenario ledger",
    )?;
    assert_rejects(
        |catalog| {
            catalog.fixture_substrate = Vec::new();
            Ok(())
        },
        "non-empty fixture substrate",
    )
}

// ---------------------------------------------------------------------------
// Forward compatibility: #11381 / #11384 family shapes through the same API
// ---------------------------------------------------------------------------

const FUTURE_LEDGER_ID: &str = "vim.bdd.11376.preview";

fn future_family_ledger() -> ScenarioLedger {
    let mut scenarios = Vec::new();
    for number in 1..=6 {
        scenarios.push(Scenario {
            id: format!("vim.bdd.freshness.{number:02}"),
            class: ScenarioClass::Baseline,
        });
    }
    for number in 1..=7 {
        scenarios.push(Scenario {
            id: format!("vim.bdd.save.{number:02}"),
            class: ScenarioClass::Baseline,
        });
    }
    ScenarioLedger {
        ledger_id: FUTURE_LEDGER_ID.to_string(),
        owning_authority: "#11376/#11381/#11384 preview shapes (not landed)".to_string(),
        scenarios,
    }
}

fn future_cell(
    cell_id: &str,
    scenario: &str,
    observation_class: &str,
    results: &[&str],
    limitations: &[&str],
    instrument: Vec<InstrumentEvidence>,
) -> CellRegistration {
    CellRegistration {
        cell_id: cell_id.to_string(),
        cell_version: 1,
        scenario_owners: vec![scenario.to_string()],
        fixture_owners: vec!["vim-vim-lsp-freshness-fixture.v1".to_string()],
        subject: xtask::vim_lsp_cell_catalog::vim_vim_lsp_subject(),
        observation_class: observation_class.to_string(),
        subject_dimensions: vec![
            "client.pinned_commit".to_string(),
            "config.root_generation".to_string(),
            "route.selection".to_string(),
            "server.executable_identity".to_string(),
            "stage.exact_source_local".to_string(),
        ],
        instrument_evidence: instrument,
        allowed_stages: vec![EvidenceStage::ExactSourceLocal],
        allowed_results: results.iter().map(|token| token.to_string()).collect(),
        allowed_limitations: limitations.iter().map(|token| token.to_string()).collect(),
        allowed_profiles: vec!["vim_first_class_exact_source".to_string()],
        claim_ceiling: "future family shape: registration only".to_string(),
    }
}

fn freshness_catalog() -> CellCatalog {
    CellCatalog {
        catalog_id: "vim_lsp_freshness".to_string(),
        catalog_version: 1,
        ledger_id: FUTURE_LEDGER_ID.to_string(),
        coverage: CoverageRule::AdditiveFamily,
        fixture_substrate: vec!["vim-vim-lsp-freshness-fixture.v1".to_string()],
        allowed_stages: vec![EvidenceStage::ExactSourceLocal],
        allowed_result_vocabulary: [
            "pass",
            "fail",
            "partial",
            "client_not_exposed",
            "explicit_reload_required",
            "restart_required",
            "unsupported",
            "not_proven",
            "instrument_failed",
        ]
        .iter()
        .map(|token| token.to_string())
        .collect(),
        core_profile: None,
        cells: vec![future_cell(
            "vim.vim_lsp.freshness.route",
            "vim.bdd.freshness.01",
            "freshness.route_classification",
            &[
                "pass",
                "partial",
                "client_not_exposed",
                "explicit_reload_required",
                "restart_required",
                "unsupported",
                "not_proven",
            ],
            &[
                "explicit_reload_required",
                "restart_required",
                "not_proven",
                "observation_incomplete",
            ],
            vec![
                InstrumentEvidence::CapabilitySnapshot,
                InstrumentEvidence::ClientLog,
                InstrumentEvidence::ProcessLedger,
                InstrumentEvidence::ServerStderr,
            ],
        )],
    }
}

fn save_catalog() -> CellCatalog {
    CellCatalog {
        catalog_id: "vim_lsp_save".to_string(),
        catalog_version: 1,
        ledger_id: FUTURE_LEDGER_ID.to_string(),
        coverage: CoverageRule::AdditiveFamily,
        fixture_substrate: vec!["vim-vim-lsp-freshness-fixture.v1".to_string()],
        allowed_stages: vec![EvidenceStage::ExactSourceLocal],
        allowed_result_vocabulary: [
            "pass",
            "fail",
            "partial",
            "client_not_exposed",
            "configuration_only",
            "unsupported",
            "not_proven",
        ]
        .iter()
        .map(|token| token.to_string())
        .collect(),
        core_profile: None,
        cells: vec![future_cell(
            "vim.vim_lsp.save.route",
            "vim.bdd.save.01",
            "save.trigger_route",
            &[
                "pass",
                "partial",
                "client_not_exposed",
                "configuration_only",
                "unsupported",
                "not_proven",
            ],
            &["configuration_only", "not_proven", "observation_incomplete"],
            vec![
                InstrumentEvidence::CapabilitySnapshot,
                InstrumentEvidence::ClientLog,
                InstrumentEvidence::ProcessLedger,
                InstrumentEvidence::CleanupObservation,
            ],
        )],
    }
}

#[test]
fn future_family_shapes_register_without_baseline_drift() -> Result<()> {
    let ledgers = vec![scenario_ledger::vim_bdd_ledger_11371(), future_family_ledger()];

    let baseline_only = catalog::validate_registry(&[baseline::baseline_catalog()], &ledgers)?;
    let baseline_digest = baseline_only
        .catalogs
        .iter()
        .find(|summary| summary.catalog_id == baseline::BASELINE_CATALOG_ID)
        .context("baseline summary missing")?
        .digest
        .clone();
    let baseline_cell_digests: Vec<String> = baseline::baseline_catalog()
        .cells
        .iter()
        .map(catalog::cell_digest)
        .collect::<Result<_>>()?;

    let extended = catalog::validate_registry(
        &[baseline::baseline_catalog(), freshness_catalog(), save_catalog()],
        &ledgers,
    )?;
    ensure!(
        extended.cell_count == baseline_only.cell_count + 2,
        "extended registry did not admit the two future family cells"
    );
    ensure!(
        extended.digest != baseline_only.digest,
        "registry digest ignored the additive families"
    );

    let extended_baseline = extended
        .catalogs
        .iter()
        .find(|summary| summary.catalog_id == baseline::BASELINE_CATALOG_ID)
        .context("baseline summary missing from extended registry")?;
    ensure!(
        extended_baseline.digest == baseline_digest,
        "an additive family changed the baseline catalog digest"
    );
    let after: Vec<String> = baseline::baseline_catalog()
        .cells
        .iter()
        .map(catalog::cell_digest)
        .collect::<Result<_>>()?;
    ensure!(after == baseline_cell_digests, "an additive family changed a baseline cell digest");

    let compiled = baseline::baseline_catalog();
    let ids: Vec<&str> = compiled.cells.iter().map(|cell| cell.cell_id.as_str()).collect();
    ensure!(
        !ids.contains(&"vim.vim_lsp.freshness.route"),
        "future family cell leaked into the baseline catalog"
    );
    Ok(())
}

#[test]
fn cross_catalog_collisions_fail_closed() -> Result<()> {
    let ledgers = vec![scenario_ledger::vim_bdd_ledger_11371(), future_family_ledger()];

    // Duplicate cell ID across catalogs.
    let mut stolen_id = freshness_catalog();
    stolen_id.cells[0].cell_id = "vim.vim_lsp.core.activation".to_string();
    let error = catalog::validate_registry(&[baseline::baseline_catalog(), stolen_id], &ledgers)
        .err()
        .context("cross-catalog duplicate cell id was accepted")?;
    ensure!(
        error.to_string().contains("is registered by catalogs"),
        "wrong rejection for duplicate cell id across catalogs: {error}"
    );

    // A scenario already owned by the baseline catalog cannot be claimed by a
    // family catalog, even when that family declares the same landed ledger.
    let mut stolen_scenario = freshness_catalog();
    stolen_scenario.ledger_id = scenario_ledger::VIM_BDD_LEDGER_ID.to_string();
    stolen_scenario.cells[0].scenario_owners = vec!["vim.bdd.lifecycle.03".to_string()];
    let error =
        catalog::validate_registry(&[baseline::baseline_catalog(), stolen_scenario], &ledgers)
            .err()
            .context("cross-catalog scenario claim was accepted")?;
    ensure!(
        error.to_string().contains("is claimed by catalogs"),
        "wrong rejection for cross-catalog scenario claim: {error}"
    );

    // A future ledger cannot substitute for the landed #11371 ledger.
    let mut substituted = freshness_catalog();
    substituted.cells[0].scenario_owners = vec!["vim.bdd.lifecycle.03".to_string()];
    let error = catalog::validate_registry(&[baseline::baseline_catalog(), substituted], &ledgers)
        .err()
        .context("cross-ledger scenario substitution was accepted")?;
    ensure!(
        error.to_string().contains("absent from ledger"),
        "wrong rejection for cross-ledger substitution: {error}"
    );
    Ok(())
}

#[test]
fn cell_digests_discriminate_binding_edits() -> Result<()> {
    let compiled = baseline::baseline_catalog();
    let bootstrap = compiled
        .cells
        .iter()
        .find(|cell| cell.cell_id == "vim.vim_lsp.core.bootstrap")
        .context("bootstrap cell missing")?;
    let before = catalog::cell_digest(bootstrap)?;
    let catalog_before = catalog::catalog_digest(&compiled)?;

    let mut edited = compiled.clone();
    let bootstrap = edited
        .cells
        .iter_mut()
        .find(|cell| cell.cell_id == "vim.vim_lsp.core.bootstrap")
        .context("bootstrap cell missing")?;
    bootstrap.subject_dimensions.push("server.build_revision".to_string());
    let after = catalog::cell_digest(bootstrap)?;
    ensure!(before != after, "a binding edit did not change the cell digest");
    ensure!(
        catalog_before != catalog::catalog_digest(&edited)?,
        "a binding edit did not change the catalog digest"
    );
    ensure!(
        before.starts_with("sha256:") && before.len() == "sha256:".len() + 64,
        "cell digest is not a sha256 identity: {before}"
    );
    Ok(())
}

#[test]
fn direct_catalog_validator_requires_the_declared_ledger() -> Result<()> {
    let catalog = baseline::baseline_catalog();
    let other = future_family_ledger();
    let error = catalog::validate_catalog(&catalog, &other)
        .err()
        .context("validate_catalog accepted a ledger other than the catalog's declared ledger")?;
    ensure!(
        error.to_string().contains("declares ledger"),
        "wrong rejection for a mismatched ledger: {error}"
    );
    catalog::validate_catalog(&catalog, &baseline::baseline_ledger())?;
    Ok(())
}

#[test]
fn catalog_digest_covers_catalog_level_semantics() -> Result<()> {
    let compiled = baseline::baseline_catalog();
    let before = catalog::catalog_digest(&compiled)?;

    let mut widened = compiled.clone();
    widened.fixture_substrate.push("vim-vim-lsp-freshness-fixture.v1".to_string());
    ensure!(
        before != catalog::catalog_digest(&widened)?,
        "a fixture-substrate change left the catalog digest unchanged"
    );

    let mut devocabularied = compiled.clone();
    devocabularied.allowed_result_vocabulary.push("explicit_reload_required".to_string());
    ensure!(
        before != catalog::catalog_digest(&devocabularied)?,
        "a result-vocabulary change left the catalog digest unchanged"
    );

    let mut reledgered = compiled.clone();
    reledgered.ledger_id = "vim.bdd.11371.other".to_string();
    ensure!(
        before != catalog::catalog_digest(&reledgered)?,
        "a ledger change left the catalog digest unchanged"
    );
    Ok(())
}
