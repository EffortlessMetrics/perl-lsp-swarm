//! Contract tests for the #11374 additive Vim/vim-lsp cell-catalog API.
//!
//! Positive proof: the compiled registry (the #11371 baseline catalog plus the
//! #11381 freshness, #11384 save, #11388 expanded-activation, #11386
//! server-generation recovery, and #11387 host-reopen/repeated-session family
//! catalogs) validates, the baseline covers exactly the 23 baseline #11371
//! scenarios, each family covers exactly its landed #11380 action vocabulary,
//! the activation, recovery, and lifecycle denominator mirrors match their
//! landed artifacts, all catalogs bind only fixture authorities that exist on
//! disk, and digests are deterministic.
//!
//! Negative controls: every fail-closed law of the registration model —
//! duplicate/unknown/conflicting cell IDs, unknown or optional scenarios,
//! coverage gaps, absent fixture owners, cross-client subjects, stage
//! escapes, version and vocabulary violations, missing profiles and ceilings —
//! plus the #11381 freshness family laws (landed-action observation classes,
//! ledger/vocabulary mirroring, required dimensions, fail/not_proven
//! expressibility, action coverage), the #11384 save-family laws, the #11388
//! activation family laws (finite #7762 denominator membership,
//! row-aspect completeness, row-dimension identity, semantic honesty on
//! non-perl rows, aspect vocabularies, override authorization boundaries,
//! cleanup evidence), the #11386 recovery family laws (finite
//! recovery-root denominator membership, stage completeness, generation and
//! old-generation bindings, row-identity dimensions, adverse-exit honesty,
//! manual-disposition expressibility, stage vocabularies, cleanup evidence),
//! and the #11387 lifecycle family laws (finite lifecycle-root denominator
//! membership, stage completeness, generation bindings, iff host-replacement /
//! pending-identity / iteration-denominator / cleanup-kind bindings,
//! row-identity dimensions, row-authority digest visibility, pinned stage
//! classes and owner sets, stage vocabularies, cleanup evidence) are executed
//! as mutations of otherwise valid catalogs and must be rejected for their
//! own reason.
//!
//! Forward compatibility: later family-shaped cells register through the same
//! API without changing any earlier catalog identity (additive extension
//! without semantic drift).

use anyhow::{Context, Result, bail, ensure};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use xtask::editor_client_compat::EvidenceStage;
use xtask::vim_lsp_cell_catalog::{
    self as catalog, CellCatalog, CellRegistration, CoverageRule, InstrumentEvidence,
    RegistrySummary, Scenario, ScenarioClass, ScenarioLedger, activation, baseline, freshness,
    lifecycle, recovery, save_format, scenario_ledger,
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

/// The five #11388 activation aspects every denominator row registers, pinned
/// so a rename or an ad hoc aspect cannot slip in as an edit.
const PUBLISHED_ACTIVATION_ASPECTS: &[&str] =
    &["native_filetype", "override", "attachment", "semantic_result", "ambiguity_preserved"];

const ACTIVATION_ACTION_COUNT: usize = 6;
const ACTIVATION_ROW_COUNT: usize = 18;

/// The eight recovery cell IDs #11386 publishes — the spec's final convention
/// registration list verbatim — pinned so a rename, an ad hoc addition, or a
/// relabeled first-launch/host-reopen row cannot slip in as an edit.
const PUBLISHED_RECOVERY_CELL_IDS: &[&str] = &[
    "vim.vim_lsp.recovery.explicit_restart",
    "vim.vim_lsp.recovery.unexpected_exit",
    "vim.vim_lsp.recovery.initialized_new_generation",
    "vim.vim_lsp.recovery.document_replay",
    "vim.vim_lsp.recovery.current_result",
    "vim.vim_lsp.recovery.old_generation_rejected",
    "vim.vim_lsp.recovery.retry_or_manual_disposition",
    "vim.vim_lsp.recovery.shutdown_cleanup",
];

const RECOVERY_ACTION_COUNT: usize = 7;
const RECOVERY_ROW_COUNT: usize = 8;

/// The eight lifecycle cell IDs #11387 publishes — the spec's final convention
/// registration list verbatim — pinned so a rename, an ad hoc addition, a
/// relabeled server-restart row, or a baseline cleanup row cannot slip in as
/// an edit.
const PUBLISHED_LIFECYCLE_CELL_IDS: &[&str] = &[
    "vim.vim_lsp.lifecycle.buffer_reopen",
    "vim.vim_lsp.lifecycle.host_reopen",
    "vim.vim_lsp.lifecycle.workspace_or_session_reopen",
    "vim.vim_lsp.lifecycle.cancellation",
    "vim.vim_lsp.lifecycle.late_result_rejected",
    "vim.vim_lsp.lifecycle.repeated_sessions",
    "vim.vim_lsp.lifecycle.normal_cleanup",
    "vim.vim_lsp.lifecycle.failure_cleanup",
];

const LIFECYCLE_ACTION_COUNT: usize = 7;
const LIFECYCLE_ROW_COUNT: usize = 8;

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
        + PUBLISHED_SAVE_CELL_IDS.len()
        + ACTIVATION_ROW_COUNT * PUBLISHED_ACTIVATION_ASPECTS.len()
        + PUBLISHED_RECOVERY_CELL_IDS.len()
        + PUBLISHED_LIFECYCLE_CELL_IDS.len();
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

    let activation_summary = first
        .catalogs
        .iter()
        .find(|summary| summary.catalog_id == activation::ACTIVATION_CATALOG_ID)
        .context("compiled registry omitted the activation family catalog")?;
    ensure!(
        activation_summary.cell_count == ACTIVATION_ROW_COUNT * PUBLISHED_ACTIVATION_ASPECTS.len(),
        "activation catalog carries {} cells, expected {} denominator rows x {} aspects",
        activation_summary.cell_count,
        ACTIVATION_ROW_COUNT,
        PUBLISHED_ACTIVATION_ASPECTS.len()
    );
    ensure!(
        activation_summary.scenario_ids.len() == ACTIVATION_ACTION_COUNT,
        "activation catalog cites {} scenarios, expected the {ACTIVATION_ACTION_COUNT} landed #11380 activation actions",
        activation_summary.scenario_ids.len()
    );
    let compiled_activation = activation::activation_catalog();
    let mut activation_pairs: BTreeSet<(String, &str)> = BTreeSet::new();
    for cell in &compiled_activation.cells {
        let name = cell
            .cell_id
            .strip_prefix("vim.vim_lsp.activation.")
            .context("activation cell outside its namespace")?;
        let aspect = PUBLISHED_ACTIVATION_ASPECTS
            .iter()
            .copied()
            .find(|aspect| name.ends_with(&format!("_{aspect}")))
            .with_context(|| format!("activation cell {name} carries no published aspect"))?;
        let slug = &name[..name.len() - aspect.len() - 1];
        ensure!(
            activation::ACTIVATION_DENOMINATOR.iter().any(|row| row.slug == slug),
            "activation cell {name} names a row outside the compiled denominator mirror"
        );
        ensure!(
            activation_pairs.insert((slug.to_string(), aspect)),
            "activation row-aspect registered twice: {slug}::{aspect}"
        );
    }
    for row in activation::ACTIVATION_DENOMINATOR {
        for &aspect in PUBLISHED_ACTIVATION_ASPECTS {
            ensure!(
                activation_pairs.contains(&(row.slug.to_string(), aspect)),
                "denominator row {} aspect {aspect} has no registered cell",
                row.slug
            );
        }
    }

    let recovery_summary = first
        .catalogs
        .iter()
        .find(|summary| summary.catalog_id == recovery::RECOVERY_CATALOG_ID)
        .context("compiled registry omitted the recovery family catalog")?;
    ensure!(
        recovery_summary.cell_count == PUBLISHED_RECOVERY_CELL_IDS.len(),
        "recovery catalog carries {} cells, expected the {} #11386 denominator stages",
        recovery_summary.cell_count,
        PUBLISHED_RECOVERY_CELL_IDS.len()
    );
    ensure!(
        recovery_summary.scenario_ids.len() == RECOVERY_ACTION_COUNT,
        "recovery catalog cites {} scenarios, expected the {RECOVERY_ACTION_COUNT} landed #11380 recovery actions",
        recovery_summary.scenario_ids.len()
    );
    let compiled_recovery = recovery::recovery_catalog();
    let recovery_ids: BTreeSet<&str> =
        compiled_recovery.cells.iter().map(|cell| cell.cell_id.as_str()).collect();
    for published in PUBLISHED_RECOVERY_CELL_IDS {
        ensure!(
            recovery_ids.contains(published),
            "published recovery cell id {published} is missing from the compiled catalog"
        );
    }
    ensure!(
        recovery_ids.len() == PUBLISHED_RECOVERY_CELL_IDS.len(),
        "recovery catalog registers cells beyond the published #11386 table"
    );
    for row in recovery::RECOVERY_DENOMINATOR {
        ensure!(
            recovery_ids.contains(&format!("vim.vim_lsp.recovery.{}", row.stage_id).as_str()),
            "denominator stage {} has no registered cell",
            row.stage_id
        );
    }

    let lifecycle_summary = first
        .catalogs
        .iter()
        .find(|summary| summary.catalog_id == lifecycle::LIFECYCLE_CATALOG_ID)
        .context("compiled registry omitted the lifecycle family catalog")?;
    ensure!(
        lifecycle_summary.cell_count == PUBLISHED_LIFECYCLE_CELL_IDS.len(),
        "lifecycle catalog carries {} cells, expected the {} #11387 denominator stages",
        lifecycle_summary.cell_count,
        PUBLISHED_LIFECYCLE_CELL_IDS.len()
    );
    ensure!(
        lifecycle_summary.scenario_ids.len() == LIFECYCLE_ACTION_COUNT,
        "lifecycle catalog cites {} scenarios, expected the {LIFECYCLE_ACTION_COUNT} landed #11380 host-reopen actions",
        lifecycle_summary.scenario_ids.len()
    );
    let compiled_lifecycle = lifecycle::lifecycle_catalog();
    let lifecycle_ids: BTreeSet<&str> =
        compiled_lifecycle.cells.iter().map(|cell| cell.cell_id.as_str()).collect();
    for published in PUBLISHED_LIFECYCLE_CELL_IDS {
        ensure!(
            lifecycle_ids.contains(published),
            "published lifecycle cell id {published} is missing from the compiled catalog"
        );
    }
    ensure!(
        lifecycle_ids.len() == PUBLISHED_LIFECYCLE_CELL_IDS.len(),
        "lifecycle catalog registers cells beyond the published #11387 table"
    );
    for row in lifecycle::LIFECYCLE_DENOMINATOR {
        ensure!(
            lifecycle_ids.contains(&format!("vim.vim_lsp.lifecycle.{}", row.stage_id).as_str()),
            "denominator stage {} has no registered cell",
            row.stage_id
        );
    }
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
    let activation_before = before
        .catalogs
        .iter()
        .find(|summary| summary.catalog_id == activation::ACTIVATION_CATALOG_ID)
        .context("activation summary missing")?
        .digest
        .clone();
    let recovery_before = before
        .catalogs
        .iter()
        .find(|summary| summary.catalog_id == recovery::RECOVERY_CATALOG_ID)
        .context("recovery summary missing")?
        .digest
        .clone();
    let lifecycle_before = before
        .catalogs
        .iter()
        .find(|summary| summary.catalog_id == lifecycle::LIFECYCLE_CATALOG_ID)
        .context("lifecycle summary missing")?
        .digest
        .clone();

    // Registering the save family over the pre-save registry (baseline +
    // freshness + the later #11388 activation, #11386 recovery, and #11387
    // lifecycle families) leaves every prior catalog digest byte-identical.
    let prior_catalogs = vec![
        baseline::baseline_catalog(),
        freshness::freshness_catalog(),
        activation::activation_catalog(),
        recovery::recovery_catalog(),
        lifecycle::lifecycle_catalog(),
    ];
    let prior_ledgers = vec![
        scenario_ledger::vim_bdd_ledger_11371(),
        freshness::freshness_action_ledger(),
        activation::activation_action_ledger(),
        recovery::recovery_action_ledger(),
        lifecycle::lifecycle_action_ledger(),
    ];
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
    let prior_activation = prior
        .catalogs
        .iter()
        .find(|summary| summary.catalog_id == activation::ACTIVATION_CATALOG_ID)
        .context("activation summary missing")?
        .digest
        .clone();
    let prior_recovery = prior
        .catalogs
        .iter()
        .find(|summary| summary.catalog_id == recovery::RECOVERY_CATALOG_ID)
        .context("recovery summary missing")?
        .digest
        .clone();
    let prior_lifecycle = prior
        .catalogs
        .iter()
        .find(|summary| summary.catalog_id == lifecycle::LIFECYCLE_CATALOG_ID)
        .context("lifecycle summary missing")?
        .digest
        .clone();
    ensure!(prior_baseline == baseline_before, "the save family changed the baseline digest");
    ensure!(
        prior_freshness == freshness_before,
        "the save family changed the freshness catalog digest"
    );
    ensure!(
        prior_activation == activation_before,
        "the save family changed the activation catalog digest"
    );
    ensure!(
        prior_recovery == recovery_before,
        "the save family changed the recovery catalog digest"
    );
    ensure!(
        prior_lifecycle == lifecycle_before,
        "the save family changed the lifecycle catalog digest"
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
fn save_stale_result_cell_laws_fail_closed() -> Result<()> {
    // The stale-result cell must bind the save-event trigger identity, so a
    // held manual-format result cannot pose as save evidence.
    assert_save_rejects(
        |catalog| {
            let cell = save_cell_mut(catalog, "vim.vim_lsp.save.stale_result_rejected")?;
            cell.subject_dimensions.retain(|token| token != "save.trigger");
            Ok(())
        },
        "save.trigger and save.owner identities",
    )?;
    // Cleanup evidence is independently load-bearing for stale rejection.
    assert_save_rejects(
        |catalog| {
            let cell = save_cell_mut(catalog, "vim.vim_lsp.save.stale_result_rejected")?;
            cell.instrument_evidence
                .retain(|token| !matches!(token, InstrumentEvidence::CleanupObservation));
            Ok(())
        },
        "must require cleanup evidence",
    )
}

#[test]
fn save_failure_cell_cannot_admit_pass() -> Result<()> {
    // The no-pass law is enforced by the validator, not only by the factory:
    // a save-shaped catalog whose failure cell admits pass fails closed.
    assert_save_rejects(
        |catalog| {
            let cell = save_cell_mut(catalog, "vim.vim_lsp.save.failure")?;
            cell.allowed_results.push("pass".to_string());
            Ok(())
        },
        "the failure cell must never admit pass",
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
// #11388 activation family: landed-authority bindings and family laws
// ---------------------------------------------------------------------------

#[test]
fn activation_ledger_mirrors_the_landed_11380_action_vocabulary() -> Result<()> {
    let ledger = activation::activation_action_ledger();
    let mirrored: BTreeSet<String> =
        ledger.scenarios.iter().map(|scenario| scenario.id.clone()).collect();
    let landed: BTreeSet<String> = xtask::vim_lsp_specialized_driver::ACTIONS
        .iter()
        .filter(|action| {
            action.family == xtask::vim_lsp_specialized_driver::ActionFamily::Activation
        })
        .map(|action| action.action_id.to_string())
        .collect();
    ensure!(
        mirrored == landed && mirrored.len() == ACTIVATION_ACTION_COUNT,
        "activation ledger drifted from the landed #11380 activation action vocabulary"
    );
    Ok(())
}

/// The compiled denominator mirror must match the landed #7762 activation-root
/// artifact row for row (case, path, expectation, source, negative control,
/// override boundary, independent semantic support), and every slug must be a
/// stable reason token equal to the artifact case id whenever that case id is
/// itself a stable reason token.
#[test]
fn activation_denominator_mirror_matches_the_landed_7762_artifact() -> Result<()> {
    let root = repository_root()?;
    let path = root.join(".ci/editor-clients").join("vim-vim-lsp-activation-root.v1.json");
    let bytes = fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    let artifact: serde_json::Value =
        serde_json::from_slice(&bytes).with_context(|| format!("parsing {}", path.display()))?;
    let rows = artifact
        .get("filetypes")
        .and_then(|value| value.as_array())
        .context("activation-root artifact carries no filetypes array")?;
    ensure!(
        rows.len() == ACTIVATION_ROW_COUNT,
        "activation-root artifact carries {} rows, mirror carries {ACTIVATION_ROW_COUNT}",
        rows.len()
    );

    let mut slugs: BTreeSet<&str> = BTreeSet::new();
    for (index, row) in rows.iter().enumerate() {
        let mirror = &activation::ACTIVATION_DENOMINATOR[index];
        let field = |name: &str| row.get(name).and_then(|value| value.as_str()).map(str::to_string);
        let case = field("case").context("artifact row missing case")?;
        ensure!(
            mirror.case_id == case,
            "mirror row {index} case {} drifted from artifact case {case}",
            mirror.case_id
        );
        let path_row = field("path").context("artifact row missing path")?;
        ensure!(
            mirror.path == path_row,
            "mirror row {case} path {} drifted from artifact path {path_row}",
            mirror.path
        );
        let expect = field("expect").context("artifact row missing expect")?;
        ensure!(
            mirror.expect == expect,
            "mirror row {case} expectation {} drifted from artifact expectation {expect}",
            mirror.expect
        );
        let source = field("source");
        ensure!(
            mirror.source == source.as_deref(),
            "mirror row {case} detection source drifted from artifact"
        );
        let negative =
            row.get("negative_control").and_then(|value| value.as_bool()).unwrap_or(false);
        ensure!(
            mirror.negative_control == negative,
            "mirror row {case} negative-control flag drifted from artifact flag {negative}"
        );
        let boundary = field("manual_override");
        ensure!(
            mirror.manual_override == boundary.as_deref(),
            "mirror row {case} manual-override boundary drifted from artifact"
        );
        let semantic = field("semantic_support");
        ensure!(
            mirror.semantic_support == semantic.as_deref(),
            "mirror row {case} independent-semantic-support marker drifted from artifact"
        );

        ensure!(
            xtask::client_compat_fixture::is_reason_token(mirror.slug),
            "row {case} slug {} is not a stable reason token",
            mirror.slug
        );
        ensure!(slugs.insert(mirror.slug), "denominator slug {} is not unique", mirror.slug);
        if xtask::client_compat_fixture::is_reason_token(&case) {
            ensure!(
                mirror.slug == case,
                "row {case} carries slug {} although its case id is already a stable reason token",
                mirror.slug
            );
        }
    }
    // The discriminators that keep ambiguity honest stay negative controls.
    for control in ["pm_xpm", "t_tads"] {
        let row = activation::ACTIVATION_DENOMINATOR
            .iter()
            .find(|row| row.slug == control)
            .with_context(|| format!("negative-control row {control} missing"))?;
        ensure!(row.negative_control, "row {control} lost its negative-control flag");
    }
    Ok(())
}

#[test]
fn activation_fixture_substrate_is_landed_on_disk() -> Result<()> {
    let root = repository_root()?;
    for fixture in activation::ACTIVATION_FIXTURE_SUBSTRATE {
        let path = root.join(".ci/editor-clients").join(format!("{fixture}.json"));
        ensure!(
            path.is_file(),
            "activation fixture substrate id {fixture} has no landed authority artifact at {}",
            path.display()
        );
    }
    Ok(())
}

#[test]
fn activation_family_registration_leaves_earlier_catalogs_byte_identical() -> Result<()> {
    let before = catalog::validate_compiled_registry()?;
    let earlier_digests: BTreeMap<String, String> = before
        .catalogs
        .iter()
        .map(|summary| (summary.catalog_id.clone(), summary.digest.clone()))
        .collect();

    // Registering the activation family over the pre-activation registry
    // (baseline + freshness + save + the later #11386 recovery and #11387
    // lifecycle families) leaves every prior catalog digest byte-identical.
    let prior_catalogs = vec![
        baseline::baseline_catalog(),
        freshness::freshness_catalog(),
        save_format::save_catalog(),
        recovery::recovery_catalog(),
        lifecycle::lifecycle_catalog(),
    ];
    let prior_ledgers = vec![
        scenario_ledger::vim_bdd_ledger_11371(),
        freshness::freshness_action_ledger(),
        save_format::save_action_ledger(),
        recovery::recovery_action_ledger(),
        lifecycle::lifecycle_action_ledger(),
    ];
    let prior = catalog::validate_registry(&prior_catalogs, &prior_ledgers)?;
    for summary in &prior.catalogs {
        let digest = earlier_digests.get(&summary.catalog_id).with_context(|| {
            format!("prior catalog {} missing from compiled registry", summary.catalog_id)
        })?;
        ensure!(
            digest == &summary.digest,
            "the activation family changed the {} catalog digest",
            summary.catalog_id
        );
    }
    ensure!(
        before.cell_count
            == prior.cell_count + ACTIVATION_ROW_COUNT * PUBLISHED_ACTIVATION_ASPECTS.len(),
        "the activation family changed the cell count of an earlier catalog"
    );
    Ok(())
}

/// Positive binding proof: every registered cell is present, typed by a
/// landed activation action, keyed to its own denominator row dimension, and
/// bound to that row's #7762 expectation; semantic affirmation stays where
/// the row claims it.
#[test]
fn activation_cells_bind_row_identity_expectation_and_claim() -> Result<()> {
    let compiled = activation::activation_catalog();
    ensure!(
        compiled.cells.len() == ACTIVATION_ROW_COUNT * PUBLISHED_ACTIVATION_ASPECTS.len(),
        "activation catalog carries {} cells",
        compiled.cells.len()
    );
    let landed: BTreeSet<&str> = xtask::vim_lsp_specialized_driver::ACTIONS
        .iter()
        .filter(|action| {
            action.family == xtask::vim_lsp_specialized_driver::ActionFamily::Activation
        })
        .map(|action| action.action_id)
        .collect();

    for row in activation::ACTIVATION_DENOMINATOR {
        for aspect in PUBLISHED_ACTIVATION_ASPECTS {
            let cell_id = format!("vim.vim_lsp.activation.{}_{}", row.slug, aspect);
            let cell = compiled
                .cells
                .iter()
                .find(|cell| cell.cell_id == cell_id)
                .with_context(|| format!("activation catalog omitted cell {cell_id}"))?;
            ensure!(
                landed.contains(cell.observation_class.as_str()),
                "cell {cell_id} is typed by non-landed action {}",
                cell.observation_class
            );
            ensure!(
                cell.subject_dimensions
                    .iter()
                    .any(|token| token == &format!("activation.row.{}", row.slug)),
                "cell {cell_id} does not bind its own denominator row dimension"
            );
            ensure!(
                cell.subject_dimensions
                    .iter()
                    .any(|token| token == &format!("activation.expect.{}", row.expect)),
                "cell {cell_id} does not bind its row's #7762 expectation",
            );
            let semantic_affirming = cell.allowed_results.iter().any(|result| {
                result == "native_supported" || result == "bounded_override_supported"
            });
            if aspect == &"semantic_result" {
                if row.expect == "perl" {
                    ensure!(
                        semantic_affirming,
                        "perl row {} lost its semantic-support-affirming results",
                        row.slug
                    );
                } else {
                    ensure!(
                        !semantic_affirming
                            && cell
                                .allowed_results
                                .iter()
                                .any(|result| result == "activation_only"),
                        "non-perl row {} semantic cell still affirms semantic support",
                        row.slug
                    );
                }
            }
        }
        // The native-filetype cell binds the row's landed detection route
        // wherever the #7762 artifact declares one.
        let native = compiled
            .cells
            .iter()
            .find(|cell| {
                cell.cell_id == format!("vim.vim_lsp.activation.{}_native_filetype", row.slug)
            })
            .context("native filetype cell missing")?;
        if let Some(source) = row.source {
            ensure!(
                native
                    .subject_dimensions
                    .iter()
                    .any(|token| token == &format!("activation.detection.{source}")),
                "row {} native cell does not bind its #7762 detection source",
                row.slug
            );
        } else {
            ensure!(
                !native
                    .subject_dimensions
                    .iter()
                    .any(|token| token.starts_with("activation.detection.")),
                "row {} native cell invented a detection source its #7762 row does not declare",
                row.slug
            );
        }
        // Override cells of boundary rows keep the authorization limitation.
        if let Some(boundary) = row.manual_override {
            let override_cell = compiled
                .cells
                .iter()
                .find(|cell| {
                    cell.cell_id == format!("vim.vim_lsp.activation.{}_override", row.slug)
                })
                .context("override cell missing")?;
            ensure!(
                override_cell.allowed_limitations.iter().any(|token| token == boundary),
                "row {} override cell lost its {boundary} limitation",
                row.slug
            );
        }
    }
    Ok(())
}

/// Validate a mutated activation family catalog against the family laws and
/// then the shared laws over the compiled sibling catalogs plus the mutated
/// family; both must pass for the mutation to count as accepted.
fn validate_activation_with(
    mutation: impl FnOnce(&mut CellCatalog) -> Result<()>,
) -> Result<RegistrySummary> {
    let mut mutated = activation::activation_catalog();
    mutation(&mut mutated)?;
    activation::validate_activation_catalog(&mutated, &activation::activation_action_ledger())?;
    let mut catalogs = catalog::registry();
    let slot = catalogs
        .iter_mut()
        .find(|candidate| candidate.catalog_id == activation::ACTIVATION_CATALOG_ID)
        .context("compiled registry omitted the activation catalog")?;
    *slot = mutated;
    catalog::validate_registry(&catalogs, &catalog::scenario_ledgers())
}

fn activation_cell_mut<'a>(
    catalog: &'a mut CellCatalog,
    cell_id: &str,
) -> Result<&'a mut CellRegistration> {
    catalog
        .cells
        .iter_mut()
        .find(|cell| cell.cell_id == cell_id)
        .with_context(|| format!("activation catalog omitted cell {cell_id}"))
}

/// Assert that a mutated activation registry is rejected — by the family laws
/// or the shared laws — for a reason containing `needle`.
fn assert_activation_rejects(
    mutation: impl FnOnce(&mut CellCatalog) -> Result<()>,
    needle: &str,
) -> Result<()> {
    let error = match validate_activation_with(mutation) {
        Ok(_) => {
            bail!(
                "mutated activation registry was accepted; expected rejection containing {needle}"
            )
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
fn activation_denominator_membership_fails_closed() -> Result<()> {
    // A cell for a row outside the finite #7762 denominator cannot register,
    // even with a consistent row dimension and expectation.
    assert_activation_rejects(
        |catalog| {
            let mut clone =
                activation_cell_mut(catalog, "vim.vim_lsp.activation.pl_native_filetype")?.clone();
            clone.cell_id = "vim.vim_lsp.activation.scala_native_filetype".to_string();
            clone.subject_dimensions.retain(|token| {
                !token.starts_with("activation.row.") && !token.starts_with("activation.expect.")
            });
            clone.subject_dimensions.push("activation.row.scala".to_string());
            clone.subject_dimensions.push("activation.expect.perl".to_string());
            catalog.cells.push(clone);
            Ok(())
        },
        "outside the finite #7762 activation-root denominator",
    )?;
    // A misspelled row slug is an unknown row, not a new row.
    assert_activation_rejects(
        |catalog| {
            let cell = activation_cell_mut(catalog, "vim.vim_lsp.activation.pl_native_filetype")?;
            cell.cell_id = "vim.vim_lsp.activation.pl_typo_native_filetype".to_string();
            Ok(())
        },
        "outside the finite #7762 activation-root denominator",
    )
}

#[test]
fn activation_row_aspect_completeness_fails_closed() -> Result<()> {
    // Dropping one cell leaves a denominator row-aspect unregistered.
    assert_activation_rejects(
        |catalog| {
            catalog
                .cells
                .retain(|cell| cell.cell_id != "vim.vim_lsp.activation.cpanfile_attachment");
            Ok(())
        },
        "missing from the #11388 activation family",
    )?;
    // Duplicating one row-aspect registration fails closed.
    assert_activation_rejects(
        |catalog| {
            let clone =
                activation_cell_mut(catalog, "vim.vim_lsp.activation.bin_shebang_semantic_result")?
                    .clone();
            catalog.cells.push(clone);
            Ok(())
        },
        "duplicate activation row-aspect registration",
    )
}

#[test]
fn activation_row_identity_dimensions_fail_closed() -> Result<()> {
    // A cell must keep exactly one row dimension.
    assert_activation_rejects(
        |catalog| {
            let cell = activation_cell_mut(catalog, "vim.vim_lsp.activation.pod_override")?;
            cell.subject_dimensions.retain(|token| !token.starts_with("activation.row."));
            Ok(())
        },
        "must bind exactly one activation.row.* dimension",
    )?;
    // A cell cannot inherit another row's identity.
    assert_activation_rejects(
        |catalog| {
            let cell = activation_cell_mut(catalog, "vim.vim_lsp.activation.pl_attachment")?;
            *cell
                .subject_dimensions
                .iter_mut()
                .find(|token| token.as_str() == "activation.row.pl")
                .context("pl cell missing its row dimension")? =
                "activation.row.pm_perl".to_string();
            Ok(())
        },
        "does not match its own row",
    )?;
    // A cell must carry its row's exact #7762 expectation.
    assert_activation_rejects(
        |catalog| {
            let cell = activation_cell_mut(catalog, "vim.vim_lsp.activation.pl_attachment")?;
            *cell
                .subject_dimensions
                .iter_mut()
                .find(|token| token.as_str() == "activation.expect.perl")
                .context("pl cell missing its expectation dimension")? =
                "activation.expect.observe".to_string();
            Ok(())
        },
        "must bind the #7762 expectation dimension",
    )
}

/// The row-binding authority identity makes every #7762 denominator field
/// digest-visible: two rows differing in any single authority field carry
/// different identities, and every compiled cell binds its own row's
/// identity exactly.
#[test]
fn activation_row_authority_identity_is_digest_visible() -> Result<()> {
    use xtask::vim_lsp_cell_catalog::activation::row_binding_identity;
    let base = xtask::vim_lsp_cell_catalog::activation::ActivationDenominatorRow {
        case_id: "pl",
        slug: "pl",
        path: "sample.pl",
        expect: "perl",
        source: Some("native_vim"),
        negative_control: false,
        manual_override: None,
        semantic_support: None,
    };
    let baseline = row_binding_identity(&base);
    // A path-only change moves the identity.
    let edited_path = xtask::vim_lsp_cell_catalog::activation::ActivationDenominatorRow {
        path: "other-sample.pl",
        ..base
    };
    ensure!(
        baseline != row_binding_identity(&edited_path),
        "a fixture-path denominator edit left the row authority identity unchanged"
    );
    // So do control-flag, boundary, semantic-support, expectation, and
    // detection-source edits.
    for edited in [
        xtask::vim_lsp_cell_catalog::activation::ActivationDenominatorRow {
            negative_control: true,
            ..base
        },
        xtask::vim_lsp_cell_catalog::activation::ActivationDenominatorRow {
            manual_override: Some("not_authorized_by_extension_alone"),
            ..base
        },
        xtask::vim_lsp_cell_catalog::activation::ActivationDenominatorRow {
            semantic_support: Some("independent"),
            ..base
        },
        xtask::vim_lsp_cell_catalog::activation::ActivationDenominatorRow {
            expect: "observe",
            ..base
        },
        xtask::vim_lsp_cell_catalog::activation::ActivationDenominatorRow { source: None, ..base },
    ] {
        ensure!(
            baseline != row_binding_identity(&edited),
            "a denominator authority edit left the row authority identity unchanged"
        );
    }
    // Identical fields keep the identity stable.
    ensure!(
        baseline == row_binding_identity(&base),
        "the row authority identity is not deterministic"
    );

    // Every compiled cell binds exactly its own row's authority identity
    // (matched by the cell's parsed row slug, not a name prefix, so e.g. the
    // `pl` row cannot capture `pl_uppercase` cells).
    let compiled = activation::activation_catalog();
    for cell in &compiled.cells {
        let name = cell
            .cell_id
            .strip_prefix("vim.vim_lsp.activation.")
            .context("activation cell outside its namespace")?;
        let aspect = PUBLISHED_ACTIVATION_ASPECTS
            .iter()
            .copied()
            .find(|aspect| name.ends_with(&format!("_{aspect}")))
            .with_context(|| format!("activation cell {name} carries no published aspect"))?;
        let slug = &name[..name.len() - aspect.len() - 1];
        let row = activation::ACTIVATION_DENOMINATOR
            .iter()
            .find(|row| row.slug == slug)
            .with_context(|| format!("activation row {slug} missing from the mirror"))?;
        let identity = row_binding_identity(row);
        let bound = cell
            .subject_dimensions
            .iter()
            .filter(|token| token.starts_with("activation.row_binding."))
            .collect::<Vec<_>>();
        ensure!(
            bound.len() == 1 && bound[0].as_str() == identity,
            "cell {} does not bind its row's authority identity exactly",
            cell.cell_id
        );
    }

    // A stale or hand-copied binding dimension fails the family law.
    assert_activation_rejects(
        |catalog| {
            let cell = activation_cell_mut(catalog, "vim.vim_lsp.activation.pm_perl_override")?;
            let index = cell
                .subject_dimensions
                .iter()
                .position(|token| token.starts_with("activation.row_binding."))
                .context("row binding dimension missing")?;
            cell.subject_dimensions[index] = "activation.row_binding.sha256-0".to_string();
            Ok(())
        },
        "row's authority identity",
    )
}

/// Each aspect is classified by its one pinned action: an attachment
/// observation cannot classify a semantic cell even though the attachment
/// action is one of the semantic cell's own scenario owners.
#[test]
fn activation_aspect_classes_are_pinned_to_their_propositions() -> Result<()> {
    assert_activation_rejects(
        |catalog| {
            let cell = activation_cell_mut(catalog, "vim.vim_lsp.activation.pl_semantic_result")?;
            cell.observation_class =
                "vim.vim_lsp.specialized.activation.observe_service_attachment".to_string();
            Ok(())
        },
        "must be classified by",
    )?;
    // Same law on the filetype aspect: the override stimulus cannot classify
    // the native-filetype proposition.
    assert_activation_rejects(
        |catalog| {
            let cell = activation_cell_mut(catalog, "vim.vim_lsp.activation.pl_native_filetype")?;
            cell.observation_class =
                "vim.vim_lsp.specialized.activation.declared_override_row".to_string();
            Ok(())
        },
        "must be classified by",
    )
}

/// The #11388 law: a successfully attached adjacent-language false subject
/// still fails the semantic and ambiguity cells — attachment can never be
/// relabeled semantic support.
#[test]
fn activation_attached_false_subject_still_fails_semantic_and_ambiguity_cells() -> Result<()> {
    // vim-lsp attaches to Image.pm (XPM negative control): relabeling that
    // attachment as semantic support fails closed.
    assert_activation_rejects(
        |catalog| {
            let cell =
                activation_cell_mut(catalog, "vim.vim_lsp.activation.pm_xpm_semantic_result")?;
            cell.allowed_results.push("native_supported".to_string());
            Ok(())
        },
        "semantic-support-affirming result",
    )?;
    // Same law on the TADS false subject's semantic cell.
    assert_activation_rejects(
        |catalog| {
            let cell =
                activation_cell_mut(catalog, "vim.vim_lsp.activation.t_tads_semantic_result")?;
            cell.allowed_results.push("bounded_override_supported".to_string());
            Ok(())
        },
        "semantic-support-affirming result",
    )?;
    // The ambiguity cell keeps only its preservation disposition: a bounded
    // override cannot stand in for ambiguity preservation either.
    assert_activation_rejects(
        |catalog| {
            let cell =
                activation_cell_mut(catalog, "vim.vim_lsp.activation.t_tads_ambiguity_preserved")?;
            cell.allowed_results.push("bounded_override_supported".to_string());
            Ok(())
        },
        "drifted from the pinned ambiguity_preserved aspect vocabulary",
    )?;
    // And the compiled registry keeps those cells non-affirming by default.
    let compiled = activation::activation_catalog();
    for (row, aspect) in [("pm_xpm", "semantic_result"), ("t_tads", "semantic_result")] {
        let cell_id = format!("vim.vim_lsp.activation.{row}_{aspect}");
        let cell = compiled
            .cells
            .iter()
            .find(|cell| cell.cell_id == cell_id)
            .with_context(|| format!("{cell_id} missing"))?;
        ensure!(
            !cell.allowed_results.iter().any(|result| result == "native_supported"),
            "{cell_id} affirms semantic support by default"
        );
    }
    Ok(())
}

#[test]
fn activation_aspect_vocabularies_cannot_stand_in_for_each_other() -> Result<()> {
    // A filetype cell cannot admit the attachment-only disposition.
    assert_activation_rejects(
        |catalog| {
            let cell = activation_cell_mut(catalog, "vim.vim_lsp.activation.pl_native_filetype")?;
            cell.allowed_results.push("activation_only".to_string());
            Ok(())
        },
        "drifted from the pinned native_filetype aspect vocabulary",
    )?;
    // An attachment cell cannot admit a filetype/override disposition.
    assert_activation_rejects(
        |catalog| {
            let cell = activation_cell_mut(catalog, "vim.vim_lsp.activation.pl_attachment")?;
            cell.allowed_results.push("native_supported".to_string());
            Ok(())
        },
        "drifted from the pinned attachment aspect vocabulary",
    )?;
    // The override cell never admits a native result: an override is not
    // native detection.
    let compiled = activation::activation_catalog();
    for row in activation::ACTIVATION_DENOMINATOR {
        let cell_id = format!("vim.vim_lsp.activation.{}_override", row.slug);
        let cell = compiled
            .cells
            .iter()
            .find(|cell| cell.cell_id == cell_id)
            .with_context(|| format!("{cell_id} missing"))?;
        ensure!(
            !cell.allowed_results.iter().any(|result| result == "native_supported"),
            "{cell_id} admits a native result"
        );
    }
    // Keeping the pinned class but dropping it from the cell's own scenario
    // owners fails the owner-binding law.
    assert_activation_rejects(
        |catalog| {
            let cell = activation_cell_mut(catalog, "vim.vim_lsp.activation.pl_attachment")?;
            cell.scenario_owners.retain(|owner| {
                owner != "vim.vim_lsp.specialized.activation.observe_service_attachment"
            });
            Ok(())
        },
        "must be one of its own scenario owners",
    )
}

#[test]
fn activation_override_boundary_and_cleanup_laws_fail_closed() -> Result<()> {
    // The cgi/fcgi override rows keep their extension-alone authorization
    // boundary.
    assert_activation_rejects(
        |catalog| {
            let cell = activation_cell_mut(catalog, "vim.vim_lsp.activation.cgi_override")?;
            cell.allowed_limitations.retain(|token| token != "not_authorized_by_extension_alone");
            Ok(())
        },
        "must keep the not_authorized_by_extension_alone limitation",
    )?;
    // Cells citing the between-rows reset action keep cleanup evidence.
    assert_activation_rejects(
        |catalog| {
            let cell = activation_cell_mut(catalog, "vim.vim_lsp.activation.pl_override")?;
            cell.instrument_evidence
                .retain(|token| !matches!(token, InstrumentEvidence::CleanupObservation));
            Ok(())
        },
        "must require cleanup evidence",
    )
}

#[test]
fn activation_cannot_be_filled_by_another_family_or_baseline_row() -> Result<()> {
    // A freshness action cannot classify an activation cell.
    assert_activation_rejects(
        |catalog| {
            let cell = activation_cell_mut(catalog, "vim.vim_lsp.activation.pl_native_filetype")?;
            cell.observation_class =
                "vim.vim_lsp.specialized.freshness.observe_route_and_generation".to_string();
            Ok(())
        },
        "is not a landed activation action",
    )?;
    // A save action cannot own an activation cell.
    assert_activation_rejects(
        |catalog| {
            let cell = activation_cell_mut(catalog, "vim.vim_lsp.activation.pl_override")?;
            cell.scenario_owners
                .push("vim.vim_lsp.specialized.save_format.observe_save_settlement".to_string());
            Ok(())
        },
        "absent from ledger",
    )?;
    // A baseline scenario stays owned by the baseline catalog.
    assert_activation_rejects(
        |catalog| {
            let cell = activation_cell_mut(catalog, "vim.vim_lsp.activation.pl_attachment")?;
            cell.scenario_owners.push("vim.bdd.lifecycle.03".to_string());
            Ok(())
        },
        "absent from ledger",
    )
}

#[test]
fn activation_family_vocabulary_stage_profile_and_subject_laws_fail_closed() -> Result<()> {
    // The family vocabulary is pinned.
    assert_activation_rejects(
        |catalog| {
            catalog.allowed_result_vocabulary.push("blanket_override_pass".to_string());
            Ok(())
        },
        "activation result vocabulary drifted",
    )?;
    // Every cell must be able to fail and to stay honestly unproven.
    assert_activation_rejects(
        |catalog| {
            let cell = activation_cell_mut(catalog, "vim.vim_lsp.activation.pl_override")?;
            cell.allowed_results.retain(|token| token != "fail");
            Ok(())
        },
        "must admit fail and not_proven",
    )?;
    // Required dimensions stay load-bearing.
    assert_activation_rejects(
        |catalog| {
            let cell = activation_cell_mut(catalog, "vim.vim_lsp.activation.pl_semantic_result")?;
            cell.subject_dimensions.retain(|token| token != "client.pinned_commit");
            Ok(())
        },
        "must bind required dimension client.pinned_commit",
    )?;
    // Stage escapes stay rejected by the shared bound.
    assert_activation_rejects(
        |catalog| {
            let cell = activation_cell_mut(catalog, "vim.vim_lsp.activation.pl_attachment")?;
            cell.allowed_stages = vec![EvidenceStage::PublicArtifact];
            Ok(())
        },
        "outside catalog",
    )?;
    assert_activation_rejects(
        |catalog| {
            catalog.allowed_stages = vec![EvidenceStage::ReleaseCandidate];
            Ok(())
        },
        "stage bound is exact_source_local only",
    )?;
    // Cells feed only the exact-source profile.
    assert_activation_rejects(
        |catalog| {
            let cell = activation_cell_mut(catalog, "vim.vim_lsp.activation.pl_attachment")?;
            cell.allowed_profiles = vec!["vim_programme_closeout".to_string()];
            Ok(())
        },
        "may feed only vim_first_class_exact_source",
    )?;
    // Cross-client receipts cannot register here.
    for impostor in ["coc", "yegappan/lsp", "neovim", "vimspector"] {
        assert_activation_rejects(
            |catalog| {
                let cell = activation_cell_mut(catalog, "vim.vim_lsp.activation.pl_attachment")?;
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
fn activation_cell_digests_discriminate_binding_edits() -> Result<()> {
    let compiled = activation::activation_catalog();
    let cell_id = "vim.vim_lsp.activation.pm_xpm_ambiguity_preserved";
    let cell = compiled
        .cells
        .iter()
        .find(|cell| cell.cell_id == cell_id)
        .with_context(|| format!("{cell_id} missing"))?;
    let before = catalog::cell_digest(cell)?;
    let catalog_before = catalog::catalog_digest(&compiled)?;
    let registry_before = catalog::validate_compiled_registry()?.digest;

    let mut edited = compiled.clone();
    let cell = edited
        .cells
        .iter_mut()
        .find(|cell| cell.cell_id == cell_id)
        .with_context(|| format!("{cell_id} missing"))?;
    cell.subject_dimensions.push("activation.adjacent.reroll".to_string());
    ensure!(
        before != catalog::cell_digest(cell)?,
        "an activation binding edit did not change the cell digest"
    );
    ensure!(
        catalog_before != catalog::catalog_digest(&edited)?,
        "an activation binding edit did not change the family catalog digest"
    );
    ensure!(
        registry_before.starts_with("sha256:"),
        "registry digest is not a sha256 identity: {registry_before}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// #11386 recovery family: landed-authority bindings and family laws
// ---------------------------------------------------------------------------

#[test]
fn recovery_ledger_mirrors_the_landed_11380_action_vocabulary() -> Result<()> {
    let ledger = recovery::recovery_action_ledger();
    let mirrored: BTreeSet<String> =
        ledger.scenarios.iter().map(|scenario| scenario.id.clone()).collect();
    let landed: BTreeSet<String> = xtask::vim_lsp_specialized_driver::ACTIONS
        .iter()
        .filter(|action| action.family == xtask::vim_lsp_specialized_driver::ActionFamily::Recovery)
        .map(|action| action.action_id.to_string())
        .collect();
    ensure!(
        mirrored == landed && mirrored.len() == RECOVERY_ACTION_COUNT,
        "recovery ledger drifted from the landed #11380 recovery action vocabulary"
    );
    for scenario in &ledger.scenarios {
        ensure!(
            scenario.class == ScenarioClass::Baseline,
            "recovery action {} must stay a baseline-class landed row",
            scenario.id
        );
    }
    Ok(())
}

/// The compiled denominator mirror must match the landed #11386 recovery-root
/// artifact row for row (stage, entry, old-generation requirement,
/// cardinality law, disposition shape), the generation kinds and initialize
/// sequence must match the artifact's `generations` block, and the
/// honest-claim rules must stay armed.
#[test]
fn recovery_denominator_mirror_matches_the_landed_11386_artifact() -> Result<()> {
    let root = repository_root()?;
    let path = root.join(".ci/editor-clients").join("vim-vim-lsp-recovery-root.v1.json");
    let bytes = fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    let artifact: serde_json::Value =
        serde_json::from_slice(&bytes).with_context(|| format!("parsing {}", path.display()))?;
    let stages = artifact
        .get("stages")
        .and_then(|value| value.as_array())
        .context("recovery-root artifact carries no stages array")?;
    ensure!(
        stages.len() == RECOVERY_ROW_COUNT,
        "recovery-root artifact carries {} rows, mirror carries {RECOVERY_ROW_COUNT}",
        stages.len()
    );

    let mut stage_ids: BTreeSet<&str> = BTreeSet::new();
    for (index, row) in stages.iter().enumerate() {
        let mirror = &recovery::RECOVERY_DENOMINATOR[index];
        let field = |name: &str| row.get(name).and_then(|value| value.as_str()).map(str::to_string);
        let stage = field("stage").context("artifact row missing stage")?;
        ensure!(
            mirror.stage_id == stage,
            "mirror row {index} stage {} drifted from artifact stage {stage}",
            mirror.stage_id
        );
        let entry = field("entry").context("artifact row missing entry")?;
        ensure!(
            mirror.entry == entry,
            "mirror row {stage} entry {} drifted from artifact entry {entry}",
            mirror.entry
        );
        let old_generation = row
            .get("old_generation")
            .and_then(|value| value.as_bool())
            .context("artifact row missing old_generation")?;
        ensure!(
            mirror.old_generation == old_generation,
            "mirror row {stage} old-generation requirement drifted from artifact flag {old_generation}"
        );
        let cardinality = field("cardinality").context("artifact row missing cardinality")?;
        ensure!(
            mirror.cardinality == cardinality,
            "mirror row {stage} cardinality {} drifted from artifact cardinality {cardinality}",
            mirror.cardinality
        );
        let disposition = field("disposition").context("artifact row missing disposition")?;
        ensure!(
            mirror.disposition == disposition,
            "mirror row {stage} disposition {} drifted from artifact disposition {disposition}",
            mirror.disposition
        );

        for token in [mirror.stage_id, mirror.entry, mirror.cardinality, mirror.disposition] {
            ensure!(
                xtask::client_compat_fixture::is_reason_token(token),
                "row {stage} authority token {token} is not a stable reason token"
            );
        }
        ensure!(
            stage_ids.insert(mirror.stage_id),
            "denominator stage {} is not unique",
            mirror.stage_id
        );
        ensure!(
            mirror.old_generation,
            "recovery stage {stage} dropped its old-generation requirement; a clean first launch could pose as recovery"
        );
    }

    let generations = artifact
        .get("generations")
        .context("recovery-root artifact carries no generations block")?;
    let kinds: Vec<&str> = generations
        .get("kinds")
        .and_then(|value| value.as_array())
        .context("generations block carries no kinds array")?
        .iter()
        .map(|value| value.as_str())
        .collect::<Option<Vec<_>>>()
        .context("a generation kind is not a string")?;
    ensure!(
        kinds == recovery::GENERATION_KINDS,
        "generation kinds drifted from the artifact generations block: {kinds:?}"
    );
    ensure!(
        generations
            .get("old_new_binding_required")
            .and_then(|value| value.as_bool())
            .context("generations block missing old_new_binding_required")?,
        "the artifact dropped the old/new generation binding requirement"
    );
    let sequence: Vec<&str> = generations
        .get("initialize_sequence")
        .and_then(|value| value.as_array())
        .context("generations block carries no initialize_sequence")?
        .iter()
        .map(|value| value.as_str())
        .collect::<Option<Vec<_>>>()
        .context("an initialize-sequence entry is not a string")?;
    ensure!(
        sequence == recovery::INITIALIZE_SEQUENCE,
        "initialize sequence drifted from the artifact generations block: {sequence:?}"
    );

    let rules = artifact
        .get("claim_rules")
        .context("recovery-root artifact carries no claim_rules block")?;
    for rule in [
        "new_pid_is_not_recovery",
        "process_start_is_not_initialize",
        "clean_first_launch_is_not_recovery",
        "manual_restart_is_not_automatic_recovery",
        "server_restart_is_not_host_reopen",
        "later_correct_answer_requires_current_generation",
        "old_generation_effects_must_be_rejected",
        "initialize_replay_current_result_rejection_cleanup_independent",
    ] {
        ensure!(
            rules.get(rule).and_then(|value| value.as_bool()) == Some(true),
            "claim rule {rule} is missing or disarmed in the artifact"
        );
    }
    Ok(())
}

#[test]
fn recovery_fixture_substrate_is_landed_on_disk() -> Result<()> {
    let root = repository_root()?;
    for fixture in recovery::RECOVERY_FIXTURE_SUBSTRATE {
        let path = root.join(".ci/editor-clients").join(format!("{fixture}.json"));
        ensure!(
            path.is_file(),
            "recovery fixture substrate id {fixture} has no landed authority artifact at {}",
            path.display()
        );
    }
    Ok(())
}

#[test]
fn recovery_family_registration_leaves_earlier_catalogs_byte_identical() -> Result<()> {
    let before = catalog::validate_compiled_registry()?;
    let earlier_digests: BTreeMap<String, String> = before
        .catalogs
        .iter()
        .map(|summary| (summary.catalog_id.clone(), summary.digest.clone()))
        .collect();

    // Registering the recovery family over the pre-recovery registry (baseline
    // + freshness + save + activation + the later #11387 lifecycle family)
    // leaves every prior catalog digest byte-identical.
    let prior_catalogs = vec![
        baseline::baseline_catalog(),
        freshness::freshness_catalog(),
        save_format::save_catalog(),
        activation::activation_catalog(),
        lifecycle::lifecycle_catalog(),
    ];
    let prior_ledgers = vec![
        scenario_ledger::vim_bdd_ledger_11371(),
        freshness::freshness_action_ledger(),
        save_format::save_action_ledger(),
        activation::activation_action_ledger(),
        lifecycle::lifecycle_action_ledger(),
    ];
    let prior = catalog::validate_registry(&prior_catalogs, &prior_ledgers)?;
    for summary in &prior.catalogs {
        let digest = earlier_digests.get(&summary.catalog_id).with_context(|| {
            format!("prior catalog {} missing from compiled registry", summary.catalog_id)
        })?;
        ensure!(
            digest == &summary.digest,
            "the recovery family changed the {} catalog digest",
            summary.catalog_id
        );
    }
    ensure!(
        before.cell_count == prior.cell_count + PUBLISHED_RECOVERY_CELL_IDS.len(),
        "the recovery family changed the cell count of an earlier catalog"
    );
    Ok(())
}

/// Positive binding proof: every published #11386 cell is present with its
/// exact spec ID, typed by a landed recovery action, keyed to its own
/// denominator row/entry/cardinality dimensions, bound to all five generation
/// kinds and the old-generation requirement, and pinned to its row's
/// authority identity.
#[test]
fn recovery_cells_bind_row_identity_generations_and_claim() -> Result<()> {
    let compiled = recovery::recovery_catalog();
    ensure!(
        compiled.cells.len() == RECOVERY_ROW_COUNT,
        "recovery catalog carries {} cells",
        compiled.cells.len()
    );
    let landed: BTreeSet<&str> = xtask::vim_lsp_specialized_driver::ACTIONS
        .iter()
        .filter(|action| action.family == xtask::vim_lsp_specialized_driver::ActionFamily::Recovery)
        .map(|action| action.action_id)
        .collect();

    for row in recovery::RECOVERY_DENOMINATOR {
        let cell_id = format!("vim.vim_lsp.recovery.{}", row.stage_id);
        let cell = compiled
            .cells
            .iter()
            .find(|cell| cell.cell_id == cell_id)
            .with_context(|| format!("recovery catalog omitted cell {cell_id}"))?;
        ensure!(
            landed.contains(cell.observation_class.as_str()),
            "cell {cell_id} is typed by non-landed action {}",
            cell.observation_class
        );
        ensure!(
            cell.subject_dimensions
                .iter()
                .any(|token| token == &format!("recovery.row.{}", row.stage_id)),
            "cell {cell_id} does not bind its own denominator row dimension"
        );
        ensure!(
            cell.subject_dimensions
                .iter()
                .any(|token| token == &format!("recovery.entry.{}", row.entry)),
            "cell {cell_id} does not bind its row's #11386 entry condition",
        );
        ensure!(
            cell.subject_dimensions
                .iter()
                .any(|token| token == &format!("recovery.cardinality.{}", row.cardinality)),
            "cell {cell_id} does not bind its row's #11386 cardinality law",
        );
        for kind in recovery::GENERATION_KINDS {
            ensure!(
                cell.subject_dimensions.iter().any(|token| token == &format!("generation.{kind}")),
                "cell {cell_id} does not bind the generation.{kind} dimension"
            );
        }
        ensure!(
            cell.subject_dimensions.iter().any(|token| token == "recovery.old_generation.required"),
            "cell {cell_id} does not bind the old-generation requirement"
        );
        let identity = recovery::row_binding_identity(row);
        ensure!(
            cell.subject_dimensions.iter().any(|token| token == &identity),
            "cell {cell_id} does not bind its row's authority identity"
        );
        if row.cardinality == "new_generation_initialized_once" {
            ensure!(
                cell.subject_dimensions.iter().any(|token| token
                    == "recovery.initialize_sequence.initialize_initialized_buffer_enabled"),
                "cell {cell_id} does not bind the initialize sequence"
            );
        }
    }

    // The adverse-exit stage stays non-affirming by default; the retry stage
    // keeps the manual disposition; every other stage admits pass only
    // through its full row binding.
    let exit = compiled
        .cells
        .iter()
        .find(|cell| cell.cell_id == "vim.vim_lsp.recovery.unexpected_exit")
        .context("unexpected_exit cell missing")?;
    ensure!(
        !exit.allowed_results.iter().any(|result| result == "pass"),
        "unexpected_exit affirms recovery by default"
    );
    let retry = compiled
        .cells
        .iter()
        .find(|cell| cell.cell_id == "vim.vim_lsp.recovery.retry_or_manual_disposition")
        .context("retry_or_manual_disposition cell missing")?;
    ensure!(
        retry.allowed_results.iter().any(|result| result == "manual_restart_required"),
        "retry_or_manual_disposition lost the manual-restart disposition"
    );
    Ok(())
}

/// Validate a mutated recovery family catalog against the family laws and
/// then the shared laws over the compiled sibling catalogs plus the mutated
/// family; both must pass for the mutation to count as accepted.
fn validate_recovery_with(
    mutation: impl FnOnce(&mut CellCatalog) -> Result<()>,
) -> Result<RegistrySummary> {
    let mut mutated = recovery::recovery_catalog();
    mutation(&mut mutated)?;
    recovery::validate_recovery_catalog(&mutated, &recovery::recovery_action_ledger())?;
    let mut catalogs = catalog::registry();
    let slot = catalogs
        .iter_mut()
        .find(|candidate| candidate.catalog_id == recovery::RECOVERY_CATALOG_ID)
        .context("compiled registry omitted the recovery catalog")?;
    *slot = mutated;
    catalog::validate_registry(&catalogs, &catalog::scenario_ledgers())
}

fn recovery_cell_mut<'a>(
    catalog: &'a mut CellCatalog,
    cell_id: &str,
) -> Result<&'a mut CellRegistration> {
    catalog
        .cells
        .iter_mut()
        .find(|cell| cell.cell_id == cell_id)
        .with_context(|| format!("recovery catalog omitted cell {cell_id}"))
}

/// Assert that a mutated recovery registry is rejected — by the family laws
/// or the shared laws — for a reason containing `needle`.
fn assert_recovery_rejects(
    mutation: impl FnOnce(&mut CellCatalog) -> Result<()>,
    needle: &str,
) -> Result<()> {
    let error = match validate_recovery_with(mutation) {
        Ok(_) => {
            bail!("mutated recovery registry was accepted; expected rejection containing {needle}")
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
fn recovery_denominator_membership_fails_closed() -> Result<()> {
    // A relabeled clean first launch cannot register as a recovery stage,
    // even with a consistent row dimension set.
    assert_recovery_rejects(
        |catalog| {
            let mut clone =
                recovery_cell_mut(catalog, "vim.vim_lsp.recovery.explicit_restart")?.clone();
            clone.cell_id = "vim.vim_lsp.recovery.first_launch".to_string();
            clone.subject_dimensions.retain(|token| {
                !token.starts_with("recovery.row.")
                    && !token.starts_with("recovery.entry.")
                    && !token.starts_with("recovery.cardinality.")
                    && !token.starts_with("recovery.row_binding.")
            });
            clone.subject_dimensions.push("recovery.row.first_launch".to_string());
            clone.subject_dimensions.push("recovery.entry.clean_launch".to_string());
            clone.subject_dimensions.push("recovery.cardinality.new_pid_only".to_string());
            catalog.cells.push(clone);
            Ok(())
        },
        "outside the finite #11386 recovery-root denominator",
    )?;
    // A misspelled stage is an unknown stage, not a new stage.
    assert_recovery_rejects(
        |catalog| {
            let cell = recovery_cell_mut(catalog, "vim.vim_lsp.recovery.current_result")?;
            cell.cell_id = "vim.vim_lsp.recovery.current_results".to_string();
            Ok(())
        },
        "outside the finite #11386 recovery-root denominator",
    )?;
    // A host-reopen row cannot register in the recovery family: server
    // restart is never host reopen (#11387 owns that family).
    assert_recovery_rejects(
        |catalog| {
            let mut clone =
                recovery_cell_mut(catalog, "vim.vim_lsp.recovery.shutdown_cleanup")?.clone();
            clone.cell_id = "vim.vim_lsp.recovery.host_reopen".to_string();
            clone.subject_dimensions.retain(|token| {
                !token.starts_with("recovery.row.")
                    && !token.starts_with("recovery.entry.")
                    && !token.starts_with("recovery.cardinality.")
                    && !token.starts_with("recovery.row_binding.")
            });
            clone.subject_dimensions.push("recovery.row.host_reopen".to_string());
            clone.subject_dimensions.push("recovery.entry.host_instance_changed".to_string());
            clone.subject_dimensions.push("recovery.cardinality.repeated_sessions".to_string());
            catalog.cells.push(clone);
            Ok(())
        },
        "outside the finite #11386 recovery-root denominator",
    )
}

#[test]
fn recovery_stage_completeness_fails_closed() -> Result<()> {
    // Dropping one cell leaves a denominator stage unregistered.
    assert_recovery_rejects(
        |catalog| {
            catalog.cells.retain(|cell| cell.cell_id != "vim.vim_lsp.recovery.document_replay");
            Ok(())
        },
        "denominator stage cells missing from the #11386 recovery family",
    )?;
    // Duplicating one stage registration fails closed.
    assert_recovery_rejects(
        |catalog| {
            let clone =
                recovery_cell_mut(catalog, "vim.vim_lsp.recovery.old_generation_rejected")?.clone();
            catalog.cells.push(clone);
            Ok(())
        },
        "duplicate recovery stage registration",
    )
}

#[test]
fn recovery_row_identity_dimensions_fail_closed() -> Result<()> {
    // A cell must keep exactly one row dimension.
    assert_recovery_rejects(
        |catalog| {
            let cell = recovery_cell_mut(catalog, "vim.vim_lsp.recovery.current_result")?;
            cell.subject_dimensions.retain(|token| !token.starts_with("recovery.row."));
            Ok(())
        },
        "must bind exactly one recovery.row.* dimension",
    )?;
    // A cell cannot inherit another stage's identity.
    assert_recovery_rejects(
        |catalog| {
            let cell = recovery_cell_mut(catalog, "vim.vim_lsp.recovery.document_replay")?;
            *cell
                .subject_dimensions
                .iter_mut()
                .find(|token| token.as_str() == "recovery.row.document_replay")
                .context("document_replay cell missing its row dimension")? =
                "recovery.row.current_result".to_string();
            Ok(())
        },
        "does not match its own stage",
    )?;
    // A cell must carry its row's exact entry condition.
    assert_recovery_rejects(
        |catalog| {
            let cell = recovery_cell_mut(catalog, "vim.vim_lsp.recovery.explicit_restart")?;
            *cell
                .subject_dimensions
                .iter_mut()
                .find(|token| token.as_str() == "recovery.entry.user_public_route")
                .context("explicit_restart cell missing its entry dimension")? =
                "recovery.entry.generation_replacement".to_string();
            Ok(())
        },
        "must bind its row's #11386 entry dimension",
    )?;
    // A cell must carry its row's exact cardinality law.
    assert_recovery_rejects(
        |catalog| {
            let cell = recovery_cell_mut(catalog, "vim.vim_lsp.recovery.shutdown_cleanup")?;
            *cell
                .subject_dimensions
                .iter_mut()
                .find(|token| token.as_str() == "recovery.cardinality.cleanup_settled_once")
                .context("shutdown_cleanup cell missing its cardinality dimension")? =
                "recovery.cardinality.new_generation_initialized_once".to_string();
            Ok(())
        },
        "must bind its row's #11386 cardinality dimension",
    )
}

/// The row-binding authority identity makes every #11386 denominator field
/// digest-visible: two rows differing in any single authority field carry
/// different identities, and every compiled cell binds its own row's
/// identity exactly.
#[test]
fn recovery_row_authority_identity_is_digest_visible() -> Result<()> {
    use xtask::vim_lsp_cell_catalog::recovery::row_binding_identity;
    let base = xtask::vim_lsp_cell_catalog::recovery::RecoveryDenominatorRow {
        stage_id: "current_result",
        entry: "generation_replacement",
        old_generation: true,
        cardinality: "current_result_from_current_generation_only",
        disposition: "current_answer_verified_current_generation",
    };
    let baseline = row_binding_identity(&base);
    // An entry-only change moves the identity.
    let edited_entry = xtask::vim_lsp_cell_catalog::recovery::RecoveryDenominatorRow {
        entry: "user_public_route",
        ..base
    };
    ensure!(
        baseline != row_binding_identity(&edited_entry),
        "an entry-condition denominator edit left the row authority identity unchanged"
    );
    // So do old-generation, cardinality, and disposition edits.
    for edited in [
        xtask::vim_lsp_cell_catalog::recovery::RecoveryDenominatorRow {
            old_generation: false,
            ..base
        },
        xtask::vim_lsp_cell_catalog::recovery::RecoveryDenominatorRow {
            cardinality: "open_documents_root_config_replayed_exact",
            ..base
        },
        xtask::vim_lsp_cell_catalog::recovery::RecoveryDenominatorRow {
            disposition: "old_effect_rejected_not_admitted",
            ..base
        },
    ] {
        ensure!(
            baseline != row_binding_identity(&edited),
            "a denominator authority edit left the row authority identity unchanged"
        );
    }
    // Identical fields keep the identity stable.
    ensure!(
        baseline == row_binding_identity(&base),
        "the row authority identity is not deterministic"
    );

    // Every compiled cell binds exactly its own row's authority identity.
    let compiled = recovery::recovery_catalog();
    for cell in &compiled.cells {
        let stage = cell
            .cell_id
            .strip_prefix("vim.vim_lsp.recovery.")
            .context("recovery cell outside its namespace")?;
        let row = recovery::RECOVERY_DENOMINATOR
            .iter()
            .find(|row| row.stage_id == stage)
            .with_context(|| format!("recovery stage {stage} missing from the mirror"))?;
        let identity = row_binding_identity(row);
        let bound = cell
            .subject_dimensions
            .iter()
            .filter(|token| token.starts_with("recovery.row_binding."))
            .collect::<Vec<_>>();
        ensure!(
            bound.len() == 1 && bound[0].as_str() == identity,
            "cell {} does not bind its row's authority identity exactly",
            cell.cell_id
        );
    }

    // A stale or hand-copied binding dimension fails the family law.
    assert_recovery_rejects(
        |catalog| {
            let cell = recovery_cell_mut(catalog, "vim.vim_lsp.recovery.shutdown_cleanup")?;
            let index = cell
                .subject_dimensions
                .iter()
                .position(|token| token.starts_with("recovery.row_binding."))
                .context("row binding dimension missing")?;
            cell.subject_dimensions[index] = "recovery.row_binding.sha256-0".to_string();
            Ok(())
        },
        "row's authority identity",
    )
}

/// Each stage is classified by its one pinned action: a disposition
/// observation cannot classify an initialize proposition even though the
/// disposition action is a landed recovery action.
#[test]
fn recovery_stage_classes_are_pinned_to_their_propositions() -> Result<()> {
    assert_recovery_rejects(
        |catalog| {
            let cell =
                recovery_cell_mut(catalog, "vim.vim_lsp.recovery.initialized_new_generation")?;
            cell.observation_class =
                "vim.vim_lsp.specialized.recovery.bounded_retry_disposition".to_string();
            Ok(())
        },
        "must be classified by",
    )?;
    // Same law on the exit stage: the restart route cannot classify the
    // adverse-exit disposition.
    assert_recovery_rejects(
        |catalog| {
            let cell = recovery_cell_mut(catalog, "vim.vim_lsp.recovery.unexpected_exit")?;
            cell.observation_class =
                "vim.vim_lsp.specialized.recovery.restart_server_public_route".to_string();
            Ok(())
        },
        "must be classified by",
    )
}

/// The #11386 law: a healthy new process — a new PID that initializes
/// cleanly — still fails recovery when replay/currentness bindings are
/// omitted, and a first launch without an old generation cannot pose as any
/// stage.
#[test]
fn recovery_healthy_new_process_without_replay_currentness_fails_closed() -> Result<()> {
    // Dropping the current-result cardinality law turns the cell into a
    // bare-answer proposition; it must fail.
    assert_recovery_rejects(
        |catalog| {
            let cell = recovery_cell_mut(catalog, "vim.vim_lsp.recovery.current_result")?;
            cell.subject_dimensions.retain(|token| {
                token != "recovery.cardinality.current_result_from_current_generation_only"
            });
            Ok(())
        },
        "must bind its row's #11386 cardinality dimension",
    )?;
    // Dropping the old-generation requirement admits a clean first launch;
    // it must fail.
    assert_recovery_rejects(
        |catalog| {
            let cell = recovery_cell_mut(catalog, "vim.vim_lsp.recovery.current_result")?;
            cell.subject_dimensions.retain(|token| token != "recovery.old_generation.required");
            Ok(())
        },
        "recovery.old_generation.required",
    )?;
    // Dropping the process-generation binding lets another process supply
    // the observation; it must fail.
    assert_recovery_rejects(
        |catalog| {
            let cell = recovery_cell_mut(catalog, "vim.vim_lsp.recovery.current_result")?;
            cell.subject_dimensions.retain(|token| token != "generation.process");
            Ok(())
        },
        "must bind the generation dimension generation.process",
    )?;
    // Dropping the initialize-sequence binding lets a bare process-start
    // event satisfy initialize; it must fail.
    assert_recovery_rejects(
        |catalog| {
            let cell =
                recovery_cell_mut(catalog, "vim.vim_lsp.recovery.initialized_new_generation")?;
            cell.subject_dimensions.retain(|token| {
                token != "recovery.initialize_sequence.initialize_initialized_buffer_enabled"
            });
            Ok(())
        },
        "must bind the initialize-sequence dimension",
    )
}

/// The #11386 honesty laws: the adverse exit never admits `pass`, and the
/// exit/retry stages keep `manual_restart_required` expressible so a manual
/// restart cannot be relabeled automatic recovery.
#[test]
fn recovery_adverse_exit_honesty_and_manual_disposition_fail_closed() -> Result<()> {
    // Relabeling the unexpected exit as a passing recovery observation fails.
    assert_recovery_rejects(
        |catalog| {
            let cell = recovery_cell_mut(catalog, "vim.vim_lsp.recovery.unexpected_exit")?;
            cell.allowed_results.push("pass".to_string());
            Ok(())
        },
        "admits the recovery-affirming result pass",
    )?;
    // Dropping the manual-restart disposition from the retry stage fails.
    assert_recovery_rejects(
        |catalog| {
            let cell =
                recovery_cell_mut(catalog, "vim.vim_lsp.recovery.retry_or_manual_disposition")?;
            cell.allowed_results.retain(|token| token != "manual_restart_required");
            Ok(())
        },
        "must keep the manual_restart_required disposition",
    )?;
    // Same law on the adverse-exit stage.
    assert_recovery_rejects(
        |catalog| {
            let cell = recovery_cell_mut(catalog, "vim.vim_lsp.recovery.unexpected_exit")?;
            cell.allowed_results.retain(|token| token != "manual_restart_required");
            Ok(())
        },
        "must keep the manual_restart_required disposition",
    )
}

#[test]
fn recovery_stage_vocabularies_cannot_stand_in_for_each_other() -> Result<()> {
    // A stage cannot admit a result outside its pinned vocabulary, even an
    // in-family token: the manual disposition cannot leak into a stage that
    // never carries it.
    assert_recovery_rejects(
        |catalog| {
            let cell = recovery_cell_mut(catalog, "vim.vim_lsp.recovery.current_result")?;
            cell.allowed_results.push("manual_restart_required".to_string());
            Ok(())
        },
        "drifted from the pinned current_result stage vocabulary",
    )?;
    // Honest failure must stay expressible.
    assert_recovery_rejects(
        |catalog| {
            let cell = recovery_cell_mut(catalog, "vim.vim_lsp.recovery.document_replay")?;
            cell.allowed_results.retain(|token| token != "fail");
            Ok(())
        },
        "must admit fail and not_proven",
    )?;
    // Keeping the pinned class but dropping it from the cell's own scenario
    // owners fails the owner-binding law.
    assert_recovery_rejects(
        |catalog| {
            let cell = recovery_cell_mut(catalog, "vim.vim_lsp.recovery.old_generation_rejected")?;
            cell.scenario_owners.retain(|owner| {
                owner != "vim.vim_lsp.specialized.recovery.hold_release_old_generation_result"
            });
            Ok(())
        },
        "must be one of its own scenario owners",
    )
}

/// The complete scenario-owner set of each stage is pinned: dropping a
/// non-classifying entry-path owner (the crash/restart paths that
/// distinguish a recovery stage from a clean first launch) or widening the
/// set with another landed recovery action both fail closed, even though the
/// union-coverage law would stay satisfied through other cells.
#[test]
fn recovery_stage_owner_sets_are_pinned_fail_closed() -> Result<()> {
    // Dropping the termination entry path from the initialize stage: the
    // classifying action stays an owner, so only the pinned-set law catches
    // it.
    assert_recovery_rejects(
        |catalog| {
            let cell =
                recovery_cell_mut(catalog, "vim.vim_lsp.recovery.initialized_new_generation")?;
            cell.scenario_owners.retain(|owner| {
                owner != "vim.vim_lsp.specialized.recovery.terminate_server_process"
            });
            Ok(())
        },
        "scenario owners drifted from the pinned initialized_new_generation stage owner set",
    )?;
    // Same law on the stop entry path of the explicit-restart stage.
    assert_recovery_rejects(
        |catalog| {
            let cell = recovery_cell_mut(catalog, "vim.vim_lsp.recovery.explicit_restart")?;
            cell.scenario_owners.retain(|owner| {
                owner != "vim.vim_lsp.specialized.recovery.stop_server_public_route"
            });
            Ok(())
        },
        "scenario owners drifted from the pinned explicit_restart stage owner set",
    )?;
    // Widening is also rejected: a landed recovery action that is not part of
    // the stage's declared owner set cannot be attached to the cell.
    assert_recovery_rejects(
        |catalog| {
            let cell = recovery_cell_mut(catalog, "vim.vim_lsp.recovery.shutdown_cleanup")?;
            cell.scenario_owners
                .push("vim.vim_lsp.specialized.recovery.bounded_retry_disposition".to_string());
            Ok(())
        },
        "scenario owners drifted from the pinned shutdown_cleanup stage owner set",
    )
}

#[test]
fn recovery_cannot_be_filled_by_another_family_or_baseline_row() -> Result<()> {
    // A freshness action cannot classify a recovery cell.
    assert_recovery_rejects(
        |catalog| {
            let cell = recovery_cell_mut(catalog, "vim.vim_lsp.recovery.current_result")?;
            cell.observation_class =
                "vim.vim_lsp.specialized.freshness.observe_route_and_generation".to_string();
            Ok(())
        },
        "is not a landed recovery action",
    )?;
    // A host-reopen action is landed #11380 vocabulary but does not belong to
    // the recovery ledger; it cannot own a recovery cell. The pinned
    // stage-owner-set family law rejects the widened set before the shared
    // ledger law is reached, so the pinned-set reason is the observed
    // rejection surface for owner additions in this family.
    assert_recovery_rejects(
        |catalog| {
            let cell = recovery_cell_mut(catalog, "vim.vim_lsp.recovery.shutdown_cleanup")?;
            cell.scenario_owners
                .push("vim.vim_lsp.specialized.host_reopen.buffer_close_wipe_reopen".to_string());
            Ok(())
        },
        "scenario owners drifted from the pinned shutdown_cleanup stage owner set",
    )?;
    // A baseline scenario stays owned by the baseline catalog.
    assert_recovery_rejects(
        |catalog| {
            let cell = recovery_cell_mut(catalog, "vim.vim_lsp.recovery.explicit_restart")?;
            cell.scenario_owners.push("vim.bdd.lifecycle.03".to_string());
            Ok(())
        },
        "scenario owners drifted from the pinned explicit_restart stage owner set",
    )?;
    // An activation action cannot own a recovery cell either.
    assert_recovery_rejects(
        |catalog| {
            let cell = recovery_cell_mut(catalog, "vim.vim_lsp.recovery.document_replay")?;
            cell.scenario_owners
                .push("vim.vim_lsp.specialized.activation.observe_service_attachment".to_string());
            Ok(())
        },
        "scenario owners drifted from the pinned document_replay stage owner set",
    )
}

#[test]
fn recovery_cleanup_evidence_stays_load_bearing() -> Result<()> {
    // The explicit-restart cell cites the public-route stop; dropping its
    // cleanup evidence fails.
    assert_recovery_rejects(
        |catalog| {
            let cell = recovery_cell_mut(catalog, "vim.vim_lsp.recovery.explicit_restart")?;
            cell.instrument_evidence
                .retain(|token| !matches!(token, InstrumentEvidence::CleanupObservation));
            Ok(())
        },
        "must require cleanup evidence",
    )?;
    // Same law on the shutdown-cleanup cell, which cites the host-shutdown
    // action.
    assert_recovery_rejects(
        |catalog| {
            let cell = recovery_cell_mut(catalog, "vim.vim_lsp.recovery.shutdown_cleanup")?;
            cell.instrument_evidence
                .retain(|token| !matches!(token, InstrumentEvidence::CleanupObservation));
            Ok(())
        },
        "must require cleanup evidence",
    )
}

#[test]
fn recovery_family_vocabulary_stage_profile_and_subject_laws_fail_closed() -> Result<()> {
    // The family vocabulary is pinned.
    assert_recovery_rejects(
        |catalog| {
            catalog.allowed_result_vocabulary.push("automatic_recovery".to_string());
            Ok(())
        },
        "recovery result vocabulary drifted",
    )?;
    // Required dimensions stay load-bearing.
    assert_recovery_rejects(
        |catalog| {
            let cell = recovery_cell_mut(catalog, "vim.vim_lsp.recovery.current_result")?;
            cell.subject_dimensions.retain(|token| token != "client.pinned_commit");
            Ok(())
        },
        "must bind required dimension client.pinned_commit",
    )?;
    // Stage escapes stay rejected by the shared bound.
    assert_recovery_rejects(
        |catalog| {
            let cell = recovery_cell_mut(catalog, "vim.vim_lsp.recovery.current_result")?;
            cell.allowed_stages = vec![EvidenceStage::PublicArtifact];
            Ok(())
        },
        "outside catalog",
    )?;
    assert_recovery_rejects(
        |catalog| {
            catalog.allowed_stages = vec![EvidenceStage::ReleaseCandidate];
            Ok(())
        },
        "stage bound is exact_source_local only",
    )?;
    // Cells feed only the exact-source profile.
    assert_recovery_rejects(
        |catalog| {
            let cell = recovery_cell_mut(catalog, "vim.vim_lsp.recovery.current_result")?;
            cell.allowed_profiles = vec!["vim_programme_closeout".to_string()];
            Ok(())
        },
        "may feed only vim_first_class_exact_source",
    )?;
    // Cross-client receipts cannot register here.
    for impostor in ["coc", "yegappan/lsp", "neovim", "vimspector"] {
        assert_recovery_rejects(
            |catalog| {
                let cell = recovery_cell_mut(catalog, "vim.vim_lsp.recovery.current_result")?;
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
fn recovery_cell_digests_discriminate_binding_edits() -> Result<()> {
    let compiled = recovery::recovery_catalog();
    let cell_id = "vim.vim_lsp.recovery.old_generation_rejected";
    let cell = compiled
        .cells
        .iter()
        .find(|cell| cell.cell_id == cell_id)
        .with_context(|| format!("{cell_id} missing"))?;
    let before = catalog::cell_digest(cell)?;
    let catalog_before = catalog::catalog_digest(&compiled)?;
    let registry_before = catalog::validate_compiled_registry()?.digest;

    let mut edited = compiled.clone();
    let cell = edited
        .cells
        .iter_mut()
        .find(|cell| cell.cell_id == cell_id)
        .with_context(|| format!("{cell_id} missing"))?;
    cell.subject_dimensions.push("generation.session".to_string());
    ensure!(
        before != catalog::cell_digest(cell)?,
        "a recovery binding edit did not change the cell digest"
    );
    ensure!(
        catalog_before != catalog::catalog_digest(&edited)?,
        "a recovery binding edit did not change the family catalog digest"
    );
    ensure!(
        registry_before.starts_with("sha256:"),
        "registry digest is not a sha256 identity: {registry_before}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// #11387 lifecycle family: landed-authority bindings and family laws
// ---------------------------------------------------------------------------

#[test]
fn lifecycle_ledger_mirrors_the_landed_11380_action_vocabulary() -> Result<()> {
    let ledger = lifecycle::lifecycle_action_ledger();
    let mirrored: BTreeSet<String> =
        ledger.scenarios.iter().map(|scenario| scenario.id.clone()).collect();
    let landed: BTreeSet<String> = xtask::vim_lsp_specialized_driver::ACTIONS
        .iter()
        .filter(|action| {
            action.family == xtask::vim_lsp_specialized_driver::ActionFamily::HostReopen
        })
        .map(|action| action.action_id.to_string())
        .collect();
    ensure!(
        mirrored == landed && mirrored.len() == LIFECYCLE_ACTION_COUNT,
        "lifecycle ledger drifted from the landed #11380 host-reopen action vocabulary"
    );
    for scenario in &ledger.scenarios {
        ensure!(
            scenario.class == ScenarioClass::Baseline,
            "lifecycle action {} must stay a baseline-class landed row",
            scenario.id
        );
    }
    Ok(())
}

/// The compiled denominator mirror must match the landed #11387 lifecycle-root
/// artifact row for row (stage, entry, host-replacement requirement,
/// pending-identity requirement, iteration denominator, cleanup kind,
/// cardinality law, disposition shape), the generation kinds and initialize
/// sequence must match the artifact's `generations` block, and the
/// honest-claim rules must stay armed.
#[test]
fn lifecycle_denominator_mirror_matches_the_landed_11387_artifact() -> Result<()> {
    let root = repository_root()?;
    let path = root.join(".ci/editor-clients").join("vim-vim-lsp-lifecycle-root.v1.json");
    let bytes = fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    let artifact: serde_json::Value =
        serde_json::from_slice(&bytes).with_context(|| format!("parsing {}", path.display()))?;
    let stages = artifact
        .get("stages")
        .and_then(|value| value.as_array())
        .context("lifecycle-root artifact carries no stages array")?;
    ensure!(
        stages.len() == LIFECYCLE_ROW_COUNT,
        "lifecycle-root artifact carries {} rows, mirror carries {LIFECYCLE_ROW_COUNT}",
        stages.len()
    );

    let mut stage_ids: BTreeSet<&str> = BTreeSet::new();
    let mut host_replacement_stages: BTreeSet<&str> = BTreeSet::new();
    let mut pending_stages: BTreeSet<&str> = BTreeSet::new();
    let mut repeated_stages: BTreeSet<&str> = BTreeSet::new();
    let mut cleanup_stages: BTreeSet<&str> = BTreeSet::new();
    for (index, row) in stages.iter().enumerate() {
        let mirror = &lifecycle::LIFECYCLE_DENOMINATOR[index];
        let field = |name: &str| row.get(name).and_then(|value| value.as_str()).map(str::to_string);
        let stage = field("stage").context("artifact row missing stage")?;
        ensure!(
            mirror.stage_id == stage,
            "mirror row {index} stage {} drifted from artifact stage {stage}",
            mirror.stage_id
        );
        let entry = field("entry").context("artifact row missing entry")?;
        ensure!(
            mirror.entry == entry,
            "mirror row {stage} entry {} drifted from artifact entry {entry}",
            mirror.entry
        );
        let host_replacement = row
            .get("host_replacement")
            .and_then(|value| value.as_bool())
            .context("artifact row missing host_replacement")?;
        ensure!(
            mirror.host_replacement == host_replacement,
            "mirror row {stage} host-replacement requirement drifted from artifact flag {host_replacement}"
        );
        let pending_identity = row
            .get("pending_identity")
            .and_then(|value| value.as_bool())
            .context("artifact row missing pending_identity")?;
        ensure!(
            mirror.pending_identity == pending_identity,
            "mirror row {stage} pending-identity requirement drifted from artifact flag {pending_identity}"
        );
        let min_iterations = row
            .get("min_iterations")
            .and_then(|value| value.as_u64())
            .context("artifact row missing min_iterations")?;
        ensure!(
            u64::from(mirror.min_iterations) == min_iterations,
            "mirror row {stage} iteration denominator {} drifted from artifact denominator {min_iterations}",
            mirror.min_iterations
        );
        let cleanup = field("cleanup").context("artifact row missing cleanup")?;
        ensure!(
            mirror.cleanup == cleanup,
            "mirror row {stage} cleanup kind {} drifted from artifact kind {cleanup}",
            mirror.cleanup
        );
        let cardinality = field("cardinality").context("artifact row missing cardinality")?;
        ensure!(
            mirror.cardinality == cardinality,
            "mirror row {stage} cardinality {} drifted from artifact cardinality {cardinality}",
            mirror.cardinality
        );
        let disposition = field("disposition").context("artifact row missing disposition")?;
        ensure!(
            mirror.disposition == disposition,
            "mirror row {stage} disposition {} drifted from artifact disposition {disposition}",
            mirror.disposition
        );

        for token in
            [mirror.stage_id, mirror.entry, mirror.cleanup, mirror.cardinality, mirror.disposition]
        {
            ensure!(
                xtask::client_compat_fixture::is_reason_token(token),
                "row {stage} authority token {token} is not a stable reason token"
            );
        }
        ensure!(
            stage_ids.insert(mirror.stage_id),
            "denominator stage {} is not unique",
            mirror.stage_id
        );
        if mirror.host_replacement {
            host_replacement_stages.insert(mirror.stage_id);
        }
        if mirror.pending_identity {
            pending_stages.insert(mirror.stage_id);
        }
        if mirror.min_iterations > 0 {
            repeated_stages.insert(mirror.stage_id);
        }
        if mirror.cleanup != "none" {
            cleanup_stages.insert(mirror.stage_id);
        }
    }

    // The #11387 independence law lives in the flag distribution: exactly the
    // full-host rows require host replacement, exactly the pending rows bind
    // action identity, exactly one row carries the repeated-session
    // denominator, and exactly the two cleanup rows carry a cleanup kind.
    ensure!(
        host_replacement_stages == ["host_reopen", "repeated_sessions"].into_iter().collect(),
        "host-replacement requirement drifted to {host_replacement_stages:?}"
    );
    ensure!(
        pending_stages == ["cancellation", "late_result_rejected"].into_iter().collect(),
        "pending-identity requirement drifted to {pending_stages:?}"
    );
    ensure!(
        repeated_stages == ["repeated_sessions"].into_iter().collect()
            && lifecycle::LIFECYCLE_DENOMINATOR
                .iter()
                .find(|row| row.stage_id == "repeated_sessions")
                .context("repeated_sessions row missing")?
                .min_iterations
                >= lifecycle::MIN_REPEATED_SESSION_ITERATIONS,
        "the repeated-session denominator drifted; one passing run must never pose as repeated use"
    );
    ensure!(
        cleanup_stages == ["normal_cleanup", "failure_cleanup"].into_iter().collect(),
        "cleanup kinds drifted to {cleanup_stages:?}"
    );

    let generations = artifact
        .get("generations")
        .context("lifecycle-root artifact carries no generations block")?;
    let kinds: Vec<&str> = generations
        .get("kinds")
        .and_then(|value| value.as_array())
        .context("generations block carries no kinds array")?
        .iter()
        .map(|value| value.as_str())
        .collect::<Option<Vec<_>>>()
        .context("a generation kind is not a string")?;
    ensure!(
        kinds == lifecycle::GENERATION_KINDS,
        "generation kinds drifted from the artifact generations block: {kinds:?}"
    );
    ensure!(
        generations
            .get("old_new_binding_required")
            .and_then(|value| value.as_bool())
            .context("generations block missing old_new_binding_required")?,
        "the artifact dropped the old/new generation binding requirement"
    );
    let sequence: Vec<&str> = generations
        .get("initialize_sequence")
        .and_then(|value| value.as_array())
        .context("generations block carries no initialize_sequence")?
        .iter()
        .map(|value| value.as_str())
        .collect::<Option<Vec<_>>>()
        .context("an initialize-sequence entry is not a string")?;
    ensure!(
        sequence == lifecycle::INITIALIZE_SEQUENCE,
        "initialize sequence drifted from the artifact generations block: {sequence:?}"
    );

    let rules = artifact
        .get("claim_rules")
        .context("lifecycle-root artifact carries no claim_rules block")?;
    for rule in [
        "server_restart_is_not_host_reopen",
        "buffer_reopen_is_not_host_reopen",
        "single_passing_run_is_not_repeated_use",
        "missing_resource_observation_is_not_zero",
        "client_event_or_force_kill_is_not_clean_cleanup",
        "late_old_result_must_be_rejected",
        "stale_state_cannot_satisfy_new_run",
        "restart_buffer_host_repeated_cleanup_independent",
    ] {
        ensure!(
            rules.get(rule).and_then(|value| value.as_bool()) == Some(true),
            "claim rule {rule} is missing or disarmed in the artifact"
        );
    }
    Ok(())
}

#[test]
fn lifecycle_fixture_substrate_is_landed_on_disk() -> Result<()> {
    let root = repository_root()?;
    for fixture in lifecycle::LIFECYCLE_FIXTURE_SUBSTRATE {
        let path = root.join(".ci/editor-clients").join(format!("{fixture}.json"));
        ensure!(
            path.is_file(),
            "lifecycle fixture substrate id {fixture} has no landed authority artifact at {}",
            path.display()
        );
    }
    Ok(())
}

/// The substrate pin (review finding on #12663): dropping the lifecycle-root
/// denominator artifact from the declared substrate — even consistently,
/// from every cell's fixture owners too, so the shared non-empty-substrate
/// and owner-membership laws stay satisfied — or widening the substrate with
/// an unlanded authority both fail the family law.
#[test]
fn lifecycle_fixture_substrate_is_pinned_fail_closed() -> Result<()> {
    // Dropping the denominator artifact from the substrate and from every
    // cell's fixture owners would leave a shared-law-valid catalog that no
    // longer cites the authority owning its rows; the family pin fails it.
    assert_lifecycle_rejects(
        |catalog| {
            catalog.fixture_substrate.retain(|fixture| fixture != "vim-vim-lsp-lifecycle-root.v1");
            for cell in &mut catalog.cells {
                cell.fixture_owners.retain(|fixture| fixture != "vim-vim-lsp-lifecycle-root.v1");
            }
            Ok(())
        },
        "lifecycle fixture substrate drifted",
    )?;
    // Widening the substrate with an invented authority fails the same pin.
    assert_lifecycle_rejects(
        |catalog| {
            catalog.fixture_substrate.push("vim-vim-lsp-lifecycle-future.v1".to_string());
            Ok(())
        },
        "lifecycle fixture substrate drifted",
    )?;
    // A single cell dropping the denominator artifact from its own fixture
    // owners fails the per-cell pin even while the catalog substrate stays
    // pinned.
    assert_lifecycle_rejects(
        |catalog| {
            let cell = lifecycle_cell_mut(catalog, "vim.vim_lsp.lifecycle.host_reopen")?;
            cell.fixture_owners.retain(|fixture| fixture != "vim-vim-lsp-lifecycle-root.v1");
            Ok(())
        },
        "fixture owners drifted from the pinned #11387 substrate",
    )
}

#[test]
fn lifecycle_family_registration_leaves_earlier_catalogs_byte_identical() -> Result<()> {
    let before = catalog::validate_compiled_registry()?;
    let earlier_digests: BTreeMap<String, String> = before
        .catalogs
        .iter()
        .map(|summary| (summary.catalog_id.clone(), summary.digest.clone()))
        .collect();

    // Registering the lifecycle family over the pre-lifecycle registry
    // (baseline + freshness + save + activation + recovery) leaves every prior
    // catalog digest byte-identical.
    let prior_catalogs = vec![
        baseline::baseline_catalog(),
        freshness::freshness_catalog(),
        save_format::save_catalog(),
        activation::activation_catalog(),
        recovery::recovery_catalog(),
    ];
    let prior_ledgers = vec![
        scenario_ledger::vim_bdd_ledger_11371(),
        freshness::freshness_action_ledger(),
        save_format::save_action_ledger(),
        activation::activation_action_ledger(),
        recovery::recovery_action_ledger(),
    ];
    let prior = catalog::validate_registry(&prior_catalogs, &prior_ledgers)?;
    for summary in &prior.catalogs {
        let digest = earlier_digests.get(&summary.catalog_id).with_context(|| {
            format!("prior catalog {} missing from compiled registry", summary.catalog_id)
        })?;
        ensure!(
            digest == &summary.digest,
            "the lifecycle family changed the {} catalog digest",
            summary.catalog_id
        );
    }
    ensure!(
        before.cell_count == prior.cell_count + PUBLISHED_LIFECYCLE_CELL_IDS.len(),
        "the lifecycle family changed the cell count of an earlier catalog"
    );
    Ok(())
}

/// Positive binding proof: every published #11387 cell is present with its
/// exact spec ID, typed by a landed host-reopen action, keyed to its own
/// denominator row/entry/cardinality dimensions, bound to all five generation
/// kinds, pinned to its row's authority identity, and carrying exactly its
/// row's iff bindings (host replacement, pending identity, iteration
/// denominator, cleanup kind).
#[test]
fn lifecycle_cells_bind_row_identity_generations_and_claim() -> Result<()> {
    let compiled = lifecycle::lifecycle_catalog();
    ensure!(
        compiled.cells.len() == LIFECYCLE_ROW_COUNT,
        "lifecycle catalog carries {} cells",
        compiled.cells.len()
    );
    let landed: BTreeSet<&str> = xtask::vim_lsp_specialized_driver::ACTIONS
        .iter()
        .filter(|action| {
            action.family == xtask::vim_lsp_specialized_driver::ActionFamily::HostReopen
        })
        .map(|action| action.action_id)
        .collect();

    for row in lifecycle::LIFECYCLE_DENOMINATOR {
        let cell_id = format!("vim.vim_lsp.lifecycle.{}", row.stage_id);
        let cell = compiled
            .cells
            .iter()
            .find(|cell| cell.cell_id == cell_id)
            .with_context(|| format!("lifecycle catalog omitted cell {cell_id}"))?;
        ensure!(
            landed.contains(cell.observation_class.as_str()),
            "cell {cell_id} is typed by non-landed action {}",
            cell.observation_class
        );
        ensure!(
            cell.subject_dimensions
                .iter()
                .any(|token| token == &format!("lifecycle.row.{}", row.stage_id)),
            "cell {cell_id} does not bind its own denominator row dimension"
        );
        ensure!(
            cell.subject_dimensions
                .iter()
                .any(|token| token == &format!("lifecycle.entry.{}", row.entry)),
            "cell {cell_id} does not bind its row's #11387 entry condition",
        );
        ensure!(
            cell.subject_dimensions
                .iter()
                .any(|token| token == &format!("lifecycle.cardinality.{}", row.cardinality)),
            "cell {cell_id} does not bind its row's #11387 cardinality law",
        );
        for kind in lifecycle::GENERATION_KINDS {
            ensure!(
                cell.subject_dimensions.iter().any(|token| token == &format!("generation.{kind}")),
                "cell {cell_id} does not bind the generation.{kind} dimension"
            );
        }
        let identity = lifecycle::row_binding_identity(row);
        ensure!(
            cell.subject_dimensions.iter().any(|token| token == &identity),
            "cell {cell_id} does not bind its row's authority identity"
        );
        ensure!(
            cell.subject_dimensions
                .iter()
                .any(|token| token == "lifecycle.host_replacement.required")
                == row.host_replacement,
            "cell {cell_id} host-replacement binding does not match its row"
        );
        ensure!(
            cell.subject_dimensions
                .iter()
                .any(|token| token == "lifecycle.pending_identity.required")
                == row.pending_identity,
            "cell {cell_id} pending-identity binding does not match its row"
        );
        if row.min_iterations > 0 {
            ensure!(
                cell.subject_dimensions
                    .iter()
                    .any(|token| token
                        == &format!("lifecycle.min_iterations.{}", row.min_iterations)),
                "cell {cell_id} does not bind its row's iteration denominator"
            );
            ensure!(
                cell.subject_dimensions
                    .iter()
                    .any(|token| token == "lifecycle.per_iteration_result.required"),
                "cell {cell_id} does not bind the per-iteration result requirement"
            );
        } else {
            ensure!(
                !cell
                    .subject_dimensions
                    .iter()
                    .any(|token| token.starts_with("lifecycle.min_iterations.")),
                "cell {cell_id} of a non-repeated stage binds an iteration denominator"
            );
        }
        if row.cleanup != "none" {
            ensure!(
                cell.subject_dimensions
                    .iter()
                    .any(|token| token == &format!("lifecycle.cleanup.{}", row.cleanup)),
                "cell {cell_id} does not bind its row's cleanup kind"
            );
        } else {
            ensure!(
                !cell
                    .subject_dimensions
                    .iter()
                    .any(|token| token.starts_with("lifecycle.cleanup.")),
                "cell {cell_id} of a non-cleanup stage binds a cleanup kind"
            );
        }
        if row.cardinality == "replacement_host_initialized_once" {
            ensure!(
                cell.subject_dimensions.iter().any(|token| token
                    == "lifecycle.initialize_sequence.initialize_initialized_buffer_enabled"),
                "cell {cell_id} does not bind the initialize sequence"
            );
        }
    }

    // The two baseline lifecycle cells stay baseline rows: they are outside
    // this denominator and cannot register in this family.
    let lifecycle_ids: BTreeSet<&str> =
        compiled.cells.iter().map(|cell| cell.cell_id.as_str()).collect();
    ensure!(
        !lifecycle_ids.contains("vim.vim_lsp.lifecycle.close_reopen"),
        "the baseline lifecycle cell leaked into the #11387 family"
    );
    ensure!(
        !lifecycle_ids.contains("vim.vim_lsp.lifecycle.baseline_cleanup"),
        "the baseline cleanup cell leaked into the #11387 family"
    );
    Ok(())
}

/// Validate a mutated lifecycle family catalog against the family laws and
/// then the shared laws over the compiled sibling catalogs plus the mutated
/// family; both must pass for the mutation to count as accepted.
fn validate_lifecycle_with(
    mutation: impl FnOnce(&mut CellCatalog) -> Result<()>,
) -> Result<RegistrySummary> {
    let mut mutated = lifecycle::lifecycle_catalog();
    mutation(&mut mutated)?;
    lifecycle::validate_lifecycle_catalog(&mutated, &lifecycle::lifecycle_action_ledger())?;
    let mut catalogs = catalog::registry();
    let slot = catalogs
        .iter_mut()
        .find(|candidate| candidate.catalog_id == lifecycle::LIFECYCLE_CATALOG_ID)
        .context("compiled registry omitted the lifecycle catalog")?;
    *slot = mutated;
    catalog::validate_registry(&catalogs, &catalog::scenario_ledgers())
}

fn lifecycle_cell_mut<'a>(
    catalog: &'a mut CellCatalog,
    cell_id: &str,
) -> Result<&'a mut CellRegistration> {
    catalog
        .cells
        .iter_mut()
        .find(|cell| cell.cell_id == cell_id)
        .with_context(|| format!("lifecycle catalog omitted cell {cell_id}"))
}

/// Assert that a mutated lifecycle registry is rejected — by the family laws
/// or the shared laws — for a reason containing `needle`.
fn assert_lifecycle_rejects(
    mutation: impl FnOnce(&mut CellCatalog) -> Result<()>,
    needle: &str,
) -> Result<()> {
    let error = match validate_lifecycle_with(mutation) {
        Ok(_) => {
            bail!("mutated lifecycle registry was accepted; expected rejection containing {needle}")
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
fn lifecycle_denominator_membership_fails_closed() -> Result<()> {
    // A relabeled server restart cannot register as a lifecycle stage, even
    // with a consistent row dimension set: server restart is never host
    // reopen (#11386 owns that family).
    assert_lifecycle_rejects(
        |catalog| {
            let mut clone =
                lifecycle_cell_mut(catalog, "vim.vim_lsp.lifecycle.host_reopen")?.clone();
            clone.cell_id = "vim.vim_lsp.lifecycle.server_restart".to_string();
            clone.subject_dimensions.retain(|token| {
                !token.starts_with("lifecycle.row.")
                    && !token.starts_with("lifecycle.entry.")
                    && !token.starts_with("lifecycle.cardinality.")
                    && !token.starts_with("lifecycle.row_binding.")
            });
            clone.subject_dimensions.push("lifecycle.row.server_restart".to_string());
            clone.subject_dimensions.push("lifecycle.entry.user_public_route".to_string());
            clone
                .subject_dimensions
                .push("lifecycle.cardinality.new_generation_initialized_once".to_string());
            catalog.cells.push(clone);
            Ok(())
        },
        "outside the finite #11387 lifecycle-root denominator",
    )?;
    // A misspelled stage is an unknown stage, not a new stage.
    assert_lifecycle_rejects(
        |catalog| {
            let cell = lifecycle_cell_mut(catalog, "vim.vim_lsp.lifecycle.buffer_reopen")?;
            cell.cell_id = "vim.vim_lsp.lifecycle.buffer_reopens".to_string();
            Ok(())
        },
        "outside the finite #11387 lifecycle-root denominator",
    )?;
    // A baseline cleanup row cannot register in the lifecycle family: the
    // baseline `lifecycle.baseline_cleanup` cell is not a #11387 denominator
    // stage.
    assert_lifecycle_rejects(
        |catalog| {
            let mut clone =
                lifecycle_cell_mut(catalog, "vim.vim_lsp.lifecycle.normal_cleanup")?.clone();
            clone.cell_id = "vim.vim_lsp.lifecycle.baseline_cleanup".to_string();
            clone.subject_dimensions.retain(|token| {
                !token.starts_with("lifecycle.row.")
                    && !token.starts_with("lifecycle.entry.")
                    && !token.starts_with("lifecycle.cardinality.")
                    && !token.starts_with("lifecycle.row_binding.")
            });
            clone.subject_dimensions.push("lifecycle.row.baseline_cleanup".to_string());
            clone.subject_dimensions.push("lifecycle.entry.baseline".to_string());
            clone.subject_dimensions.push("lifecycle.cardinality.baseline_once".to_string());
            catalog.cells.push(clone);
            Ok(())
        },
        "outside the finite #11387 lifecycle-root denominator",
    )
}

#[test]
fn lifecycle_stage_completeness_fails_closed() -> Result<()> {
    // Dropping one cell leaves a denominator stage unregistered.
    assert_lifecycle_rejects(
        |catalog| {
            catalog.cells.retain(|cell| cell.cell_id != "vim.vim_lsp.lifecycle.cancellation");
            Ok(())
        },
        "denominator stage cells missing from the #11387 lifecycle family",
    )?;
    // Duplicating one stage registration fails closed.
    assert_lifecycle_rejects(
        |catalog| {
            let clone =
                lifecycle_cell_mut(catalog, "vim.vim_lsp.lifecycle.repeated_sessions")?.clone();
            catalog.cells.push(clone);
            Ok(())
        },
        "duplicate lifecycle stage registration",
    )
}

#[test]
fn lifecycle_row_identity_dimensions_fail_closed() -> Result<()> {
    // A cell must keep exactly one row dimension.
    assert_lifecycle_rejects(
        |catalog| {
            let cell = lifecycle_cell_mut(catalog, "vim.vim_lsp.lifecycle.host_reopen")?;
            cell.subject_dimensions.retain(|token| !token.starts_with("lifecycle.row."));
            Ok(())
        },
        "must bind exactly one lifecycle.row.* dimension",
    )?;
    // A cell cannot inherit another stage's identity.
    assert_lifecycle_rejects(
        |catalog| {
            let cell = lifecycle_cell_mut(catalog, "vim.vim_lsp.lifecycle.late_result_rejected")?;
            *cell
                .subject_dimensions
                .iter_mut()
                .find(|token| token.as_str() == "lifecycle.row.late_result_rejected")
                .context("late_result_rejected cell missing its row dimension")? =
                "lifecycle.row.cancellation".to_string();
            Ok(())
        },
        "does not match its own stage",
    )?;
    // A cell must carry its row's exact entry condition.
    assert_lifecycle_rejects(
        |catalog| {
            let cell = lifecycle_cell_mut(catalog, "vim.vim_lsp.lifecycle.buffer_reopen")?;
            *cell
                .subject_dimensions
                .iter_mut()
                .find(|token| token.as_str() == "lifecycle.entry.buffer_closed_in_same_host")
                .context("buffer_reopen cell missing its entry dimension")? =
                "lifecycle.entry.host_exit_and_replacement_launch".to_string();
            Ok(())
        },
        "must bind its row's #11387 entry dimension",
    )?;
    // A cell must carry its row's exact cardinality law.
    assert_lifecycle_rejects(
        |catalog| {
            let cell = lifecycle_cell_mut(catalog, "vim.vim_lsp.lifecycle.failure_cleanup")?;
            *cell
                .subject_dimensions
                .iter_mut()
                .find(|token| {
                    token.as_str() == "lifecycle.cardinality.failure_path_cleanup_settled_once"
                })
                .context("failure_cleanup cell missing its cardinality dimension")? =
                "lifecycle.cardinality.normal_exit_cleanup_settled_once".to_string();
            Ok(())
        },
        "must bind its row's #11387 cardinality dimension",
    )
}

/// The row-binding authority identity makes every #11387 denominator field
/// digest-visible: two rows differing in any single authority field carry
/// different identities, and every compiled cell binds its own row's
/// identity exactly.
#[test]
fn lifecycle_row_authority_identity_is_digest_visible() -> Result<()> {
    use xtask::vim_lsp_cell_catalog::lifecycle::row_binding_identity;
    let base = xtask::vim_lsp_cell_catalog::lifecycle::LifecycleDenominatorRow {
        stage_id: "host_reopen",
        entry: "host_exit_and_replacement_launch",
        host_replacement: true,
        pending_identity: false,
        min_iterations: 0,
        cleanup: "none",
        cardinality: "replacement_host_initialized_once",
        disposition: "host_instance_changed_not_server_restart",
    };
    let baseline = row_binding_identity(&base);
    // An entry-only change moves the identity.
    let edited_entry = xtask::vim_lsp_cell_catalog::lifecycle::LifecycleDenominatorRow {
        entry: "buffer_closed_in_same_host",
        ..base
    };
    ensure!(
        baseline != row_binding_identity(&edited_entry),
        "an entry-condition denominator edit left the row authority identity unchanged"
    );
    // So do host-replacement, pending-identity, iteration, cleanup,
    // cardinality, and disposition edits.
    for edited in [
        xtask::vim_lsp_cell_catalog::lifecycle::LifecycleDenominatorRow {
            host_replacement: false,
            ..base
        },
        xtask::vim_lsp_cell_catalog::lifecycle::LifecycleDenominatorRow {
            pending_identity: true,
            ..base
        },
        xtask::vim_lsp_cell_catalog::lifecycle::LifecycleDenominatorRow {
            min_iterations: 2,
            ..base
        },
        xtask::vim_lsp_cell_catalog::lifecycle::LifecycleDenominatorRow {
            cleanup: "normal_exit",
            ..base
        },
        xtask::vim_lsp_cell_catalog::lifecycle::LifecycleDenominatorRow {
            cardinality: "bounded_iterations_each_observed",
            ..base
        },
        xtask::vim_lsp_cell_catalog::lifecycle::LifecycleDenominatorRow {
            disposition: "per_iteration_fresh_not_stale_state",
            ..base
        },
    ] {
        ensure!(
            baseline != row_binding_identity(&edited),
            "a denominator authority edit left the row authority identity unchanged"
        );
    }
    // Identical fields keep the identity stable.
    ensure!(
        baseline == row_binding_identity(&base),
        "the row authority identity is not deterministic"
    );

    // Every compiled cell binds exactly its own row's authority identity.
    let compiled = lifecycle::lifecycle_catalog();
    for cell in &compiled.cells {
        let stage = cell
            .cell_id
            .strip_prefix("vim.vim_lsp.lifecycle.")
            .context("lifecycle cell outside its namespace")?;
        let row = lifecycle::LIFECYCLE_DENOMINATOR
            .iter()
            .find(|row| row.stage_id == stage)
            .with_context(|| format!("lifecycle stage {stage} missing from the mirror"))?;
        let identity = row_binding_identity(row);
        let bound = cell
            .subject_dimensions
            .iter()
            .filter(|token| token.starts_with("lifecycle.row_binding."))
            .collect::<Vec<_>>();
        ensure!(
            bound.len() == 1 && bound[0].as_str() == identity,
            "cell {} does not bind its row's authority identity exactly",
            cell.cell_id
        );
    }

    // A stale or hand-copied binding dimension fails the family law.
    assert_lifecycle_rejects(
        |catalog| {
            let cell = lifecycle_cell_mut(catalog, "vim.vim_lsp.lifecycle.repeated_sessions")?;
            let index = cell
                .subject_dimensions
                .iter()
                .position(|token| token.starts_with("lifecycle.row_binding."))
                .context("row binding dimension missing")?;
            cell.subject_dimensions[index] = "lifecycle.row_binding.sha256-0".to_string();
            Ok(())
        },
        "row's authority identity",
    )
}

/// Each stage is classified by its one pinned action: a buffer-reopen
/// observation cannot classify a full host-replacement proposition, and a
/// forced-failure observation cannot classify the normal-exit cleanup
/// proposition, even though both actions are landed host-reopen actions.
#[test]
fn lifecycle_stage_classes_are_pinned_to_their_propositions() -> Result<()> {
    assert_lifecycle_rejects(
        |catalog| {
            let cell = lifecycle_cell_mut(catalog, "vim.vim_lsp.lifecycle.host_reopen")?;
            cell.observation_class =
                "vim.vim_lsp.specialized.host_reopen.buffer_close_wipe_reopen".to_string();
            Ok(())
        },
        "must be classified by",
    )?;
    // Same law on the cleanup stages: the forced-failure path cannot classify
    // the normal-exit cleanup proposition.
    assert_lifecycle_rejects(
        |catalog| {
            let cell = lifecycle_cell_mut(catalog, "vim.vim_lsp.lifecycle.normal_cleanup")?;
            cell.observation_class =
                "vim.vim_lsp.specialized.host_reopen.forced_failure_path".to_string();
            Ok(())
        },
        "must be classified by",
    )
}

/// The #11387 discriminating mutation: a journey that passes a server
/// restart and a buffer reopen while omitting full host replacement must
/// fail the host-reopen cell. Each mutation below strips exactly the binding
/// that full host replacement supplies; every one fails closed.
#[test]
fn lifecycle_host_reopen_requires_full_host_replacement() -> Result<()> {
    // Dropping the host-generation binding lets a same-host server restart
    // supply the observation; it must fail.
    assert_lifecycle_rejects(
        |catalog| {
            let cell = lifecycle_cell_mut(catalog, "vim.vim_lsp.lifecycle.host_reopen")?;
            cell.subject_dimensions.retain(|token| token != "generation.host");
            Ok(())
        },
        "must bind the generation dimension generation.host",
    )?;
    // Dropping the host-replacement requirement admits a server restart plus
    // buffer reopen as full host reopen; it must fail.
    assert_lifecycle_rejects(
        |catalog| {
            let cell = lifecycle_cell_mut(catalog, "vim.vim_lsp.lifecycle.host_reopen")?;
            cell.subject_dimensions.retain(|token| token != "lifecycle.host_replacement.required");
            Ok(())
        },
        "host-replacement binding must match its row's #11387 requirement",
    )?;
    // Relabeling the classifying action to the buffer-reopen observation
    // turns the cell into a buffer-reopen proposition; it must fail.
    assert_lifecycle_rejects(
        |catalog| {
            let cell = lifecycle_cell_mut(catalog, "vim.vim_lsp.lifecycle.host_reopen")?;
            cell.observation_class =
                "vim.vim_lsp.specialized.host_reopen.buffer_close_wipe_reopen".to_string();
            Ok(())
        },
        "must be classified by",
    )?;
    // Dropping the exit entry path from the owner set lets a server restart
    // (no host exit) satisfy the cell; the pinned-set law must fail it.
    assert_lifecycle_rejects(
        |catalog| {
            let cell = lifecycle_cell_mut(catalog, "vim.vim_lsp.lifecycle.host_reopen")?;
            cell.scenario_owners
                .retain(|owner| owner != "vim.vim_lsp.specialized.host_reopen.exit_host");
            Ok(())
        },
        "scenario owners drifted from the pinned host_reopen stage owner set",
    )?;
    // The same-host buffer reopen cannot bind the host-replacement dimension
    // to upgrade itself into a full host reopen; the iff law must fail it.
    assert_lifecycle_rejects(
        |catalog| {
            let cell = lifecycle_cell_mut(catalog, "vim.vim_lsp.lifecycle.buffer_reopen")?;
            cell.subject_dimensions.push("lifecycle.host_replacement.required".to_string());
            Ok(())
        },
        "host-replacement binding must match its row's #11387 requirement",
    )?;
    // Dropping the initialize-sequence binding lets a replacement process
    // spawn without initialize/readiness satisfy the cell; it must fail.
    assert_lifecycle_rejects(
        |catalog| {
            let cell = lifecycle_cell_mut(catalog, "vim.vim_lsp.lifecycle.host_reopen")?;
            cell.subject_dimensions.retain(|token| {
                token != "lifecycle.initialize_sequence.initialize_initialized_buffer_enabled"
            });
            Ok(())
        },
        "must bind the initialize-sequence dimension",
    )
}

/// The repeated-session denominator law: one passing run can never pose as
/// repeated use, and stale prior state can never satisfy a new iteration.
#[test]
fn lifecycle_repeated_session_denominator_fails_closed() -> Result<()> {
    // Dropping the finite iteration denominator admits one passing run; it
    // must fail.
    assert_lifecycle_rejects(
        |catalog| {
            let cell = lifecycle_cell_mut(catalog, "vim.vim_lsp.lifecycle.repeated_sessions")?;
            cell.subject_dimensions.retain(|token| !token.starts_with("lifecycle.min_iterations."));
            Ok(())
        },
        "must bind exactly one lifecycle.min_iterations.* dimension",
    )?;
    // Dropping the per-iteration result requirement admits a stale prior
    // result as a new iteration; it must fail.
    assert_lifecycle_rejects(
        |catalog| {
            let cell = lifecycle_cell_mut(catalog, "vim.vim_lsp.lifecycle.repeated_sessions")?;
            cell.subject_dimensions
                .retain(|token| token != "lifecycle.per_iteration_result.required");
            Ok(())
        },
        "must bind lifecycle.per_iteration_result.required",
    )?;
    // A single-session row cannot bind an iteration denominator to relabel
    // itself as repeated use; the iff law must fail it.
    assert_lifecycle_rejects(
        |catalog| {
            let cell = lifecycle_cell_mut(catalog, "vim.vim_lsp.lifecycle.buffer_reopen")?;
            cell.subject_dimensions.push("lifecycle.min_iterations.2".to_string());
            Ok(())
        },
        "of non-repeated stage buffer_reopen binds an iteration denominator",
    )
}

/// The pending-action and cleanup honesty laws: cancellation and late-result
/// rejection bind the pending action identity, and terminal cleanup is
/// observed — a client exit event or a force-kill alone is never clean
/// cleanup.
#[test]
fn lifecycle_pending_and_cleanup_honesty_fail_closed() -> Result<()> {
    // Dropping the pending-identity requirement lets any disposed action pose
    // as the cancelled one; it must fail.
    assert_lifecycle_rejects(
        |catalog| {
            let cell = lifecycle_cell_mut(catalog, "vim.vim_lsp.lifecycle.cancellation")?;
            cell.subject_dimensions.retain(|token| token != "lifecycle.pending_identity.required");
            Ok(())
        },
        "pending-identity binding must match its row's #11387 requirement",
    )?;
    // A non-pending row cannot bind the pending-identity requirement.
    assert_lifecycle_rejects(
        |catalog| {
            let cell = lifecycle_cell_mut(catalog, "vim.vim_lsp.lifecycle.normal_cleanup")?;
            cell.subject_dimensions.push("lifecycle.pending_identity.required".to_string());
            Ok(())
        },
        "pending-identity binding must match its row's #11387 requirement",
    )?;
    // Dropping the cleanup kind from the normal-cleanup row erases the
    // observed-cleanup proposition; it must fail.
    assert_lifecycle_rejects(
        |catalog| {
            let cell = lifecycle_cell_mut(catalog, "vim.vim_lsp.lifecycle.normal_cleanup")?;
            cell.subject_dimensions.retain(|token| token != "lifecycle.cleanup.normal_exit");
            Ok(())
        },
        "must bind exactly one lifecycle.cleanup.* dimension",
    )?;
    // Relabeling the forced-failure cleanup kind as a normal-exit cleanup (or
    // vice versa) crosses the two cleanup propositions; it must fail.
    assert_lifecycle_rejects(
        |catalog| {
            let cell = lifecycle_cell_mut(catalog, "vim.vim_lsp.lifecycle.failure_cleanup")?;
            *cell
                .subject_dimensions
                .iter_mut()
                .find(|token| token.as_str() == "lifecycle.cleanup.forced_failure")
                .context("failure_cleanup cell missing its cleanup dimension")? =
                "lifecycle.cleanup.normal_exit".to_string();
            Ok(())
        },
        "equal to its row's #11387 cleanup kind lifecycle.cleanup.forced_failure",
    )?;
    // A client exit event or a force-kill alone is not clean cleanup: the
    // cleanup stages must require observed cleanup evidence.
    for cell_id in ["vim.vim_lsp.lifecycle.normal_cleanup", "vim.vim_lsp.lifecycle.failure_cleanup"]
    {
        assert_lifecycle_rejects(
            |catalog| {
                let cell = lifecycle_cell_mut(catalog, cell_id)?;
                cell.instrument_evidence
                    .retain(|token| !matches!(token, InstrumentEvidence::CleanupObservation));
                Ok(())
            },
            "must require cleanup evidence",
        )
        .with_context(|| format!("cleanup stage {cell_id} dropped its cleanup evidence"))?;
    }
    Ok(())
}

#[test]
fn lifecycle_stage_vocabularies_cannot_stand_in_for_each_other() -> Result<()> {
    // A stage cannot drop a token from its pinned vocabulary: every stage
    // carries the full honest disposition set, so any shrink is a drift.
    assert_lifecycle_rejects(
        |catalog| {
            let cell = lifecycle_cell_mut(catalog, "vim.vim_lsp.lifecycle.host_reopen")?;
            cell.allowed_results.retain(|token| token != "partial");
            Ok(())
        },
        "drifted from the pinned host_reopen stage vocabulary",
    )?;
    // Same law on the cleanup stage: dropping honest non-exposure erases the
    // `where exposed` escape of the chain.
    assert_lifecycle_rejects(
        |catalog| {
            let cell = lifecycle_cell_mut(catalog, "vim.vim_lsp.lifecycle.failure_cleanup")?;
            cell.allowed_results.retain(|token| token != "client_not_exposed");
            Ok(())
        },
        "drifted from the pinned failure_cleanup stage vocabulary",
    )?;
    // Honest failure must stay expressible.
    assert_lifecycle_rejects(
        |catalog| {
            let cell = lifecycle_cell_mut(catalog, "vim.vim_lsp.lifecycle.late_result_rejected")?;
            cell.allowed_results.retain(|token| token != "fail");
            Ok(())
        },
        "must admit fail and not_proven",
    )?;
    // Keeping the pinned class but dropping it from the cell's own scenario
    // owners fails the owner-binding law.
    assert_lifecycle_rejects(
        |catalog| {
            let cell = lifecycle_cell_mut(catalog, "vim.vim_lsp.lifecycle.cancellation")?;
            cell.scenario_owners.retain(|owner| {
                owner != "vim.vim_lsp.specialized.host_reopen.pending_action_start_invalidate"
            });
            Ok(())
        },
        "must be one of its own scenario owners",
    )
}

/// The complete scenario-owner set of each stage is pinned: dropping a
/// non-classifying entry-path owner (the exit path that distinguishes a full
/// host reopen from a server restart) or widening the set with another
/// landed host-reopen action both fail closed, even though the
/// union-coverage law would stay satisfied through other cells.
#[test]
fn lifecycle_stage_owner_sets_are_pinned_fail_closed() -> Result<()> {
    // Widening is rejected: a landed host-reopen action that is not part of
    // the stage's declared owner set cannot be attached to the cell.
    assert_lifecycle_rejects(
        |catalog| {
            let cell = lifecycle_cell_mut(catalog, "vim.vim_lsp.lifecycle.normal_cleanup")?;
            cell.scenario_owners
                .push("vim.vim_lsp.specialized.host_reopen.launch_replacement_host".to_string());
            Ok(())
        },
        "scenario owners drifted from the pinned normal_cleanup stage owner set",
    )?;
    // Same law on the workspace row with the buffer-reopen action.
    assert_lifecycle_rejects(
        |catalog| {
            let cell =
                lifecycle_cell_mut(catalog, "vim.vim_lsp.lifecycle.workspace_or_session_reopen")?;
            cell.scenario_owners
                .push("vim.vim_lsp.specialized.host_reopen.buffer_close_wipe_reopen".to_string());
            Ok(())
        },
        "scenario owners drifted from the pinned workspace_or_session_reopen stage owner set",
    )
}

#[test]
fn lifecycle_cannot_be_filled_by_another_family_or_baseline_row() -> Result<()> {
    // A recovery action (the server restart route) cannot classify a
    // lifecycle cell: server restart is never host reopen.
    assert_lifecycle_rejects(
        |catalog| {
            let cell = lifecycle_cell_mut(catalog, "vim.vim_lsp.lifecycle.host_reopen")?;
            cell.observation_class =
                "vim.vim_lsp.specialized.recovery.restart_server_public_route".to_string();
            Ok(())
        },
        "is not a landed host-reopen action",
    )?;
    // A recovery action is landed #11380 vocabulary but does not belong to
    // the lifecycle ledger; it cannot own a lifecycle cell. The pinned
    // stage-owner-set family law rejects the widened set before the shared
    // ledger law is reached, so the pinned-set reason is the observed
    // rejection surface for owner additions in this family.
    assert_lifecycle_rejects(
        |catalog| {
            let cell = lifecycle_cell_mut(catalog, "vim.vim_lsp.lifecycle.failure_cleanup")?;
            cell.scenario_owners
                .push("vim.vim_lsp.specialized.recovery.host_shutdown_while_pending".to_string());
            Ok(())
        },
        "scenario owners drifted from the pinned failure_cleanup stage owner set",
    )?;
    // A baseline scenario stays owned by the baseline catalog.
    assert_lifecycle_rejects(
        |catalog| {
            let cell = lifecycle_cell_mut(catalog, "vim.vim_lsp.lifecycle.buffer_reopen")?;
            cell.scenario_owners.push("vim.bdd.lifecycle.03".to_string());
            Ok(())
        },
        "scenario owners drifted from the pinned buffer_reopen stage owner set",
    )?;
    // An activation action cannot own a lifecycle cell either.
    assert_lifecycle_rejects(
        |catalog| {
            let cell = lifecycle_cell_mut(catalog, "vim.vim_lsp.lifecycle.cancellation")?;
            cell.scenario_owners
                .push("vim.vim_lsp.specialized.activation.observe_service_attachment".to_string());
            Ok(())
        },
        "scenario owners drifted from the pinned cancellation stage owner set",
    )
}

#[test]
fn lifecycle_cleanup_evidence_stays_load_bearing() -> Result<()> {
    // The host-reopen cell cites the exit action; dropping its cleanup
    // evidence fails.
    assert_lifecycle_rejects(
        |catalog| {
            let cell = lifecycle_cell_mut(catalog, "vim.vim_lsp.lifecycle.host_reopen")?;
            cell.instrument_evidence
                .retain(|token| !matches!(token, InstrumentEvidence::CleanupObservation));
            Ok(())
        },
        "must require cleanup evidence",
    )?;
    // Same law on the repeated-session cell, which cites the
    // repeated-session sequence action.
    assert_lifecycle_rejects(
        |catalog| {
            let cell = lifecycle_cell_mut(catalog, "vim.vim_lsp.lifecycle.repeated_sessions")?;
            cell.instrument_evidence
                .retain(|token| !matches!(token, InstrumentEvidence::CleanupObservation));
            Ok(())
        },
        "must require cleanup evidence",
    )
}

#[test]
fn lifecycle_family_vocabulary_stage_profile_and_subject_laws_fail_closed() -> Result<()> {
    // The family vocabulary is pinned.
    assert_lifecycle_rejects(
        |catalog| {
            catalog.allowed_result_vocabulary.push("host_reopen_magic_pass".to_string());
            Ok(())
        },
        "lifecycle result vocabulary drifted",
    )?;
    // Required dimensions stay load-bearing.
    assert_lifecycle_rejects(
        |catalog| {
            let cell = lifecycle_cell_mut(catalog, "vim.vim_lsp.lifecycle.host_reopen")?;
            cell.subject_dimensions.retain(|token| token != "client.pinned_commit");
            Ok(())
        },
        "must bind required dimension client.pinned_commit",
    )?;
    // Stage escapes stay rejected by the shared bound.
    assert_lifecycle_rejects(
        |catalog| {
            let cell = lifecycle_cell_mut(catalog, "vim.vim_lsp.lifecycle.host_reopen")?;
            cell.allowed_stages = vec![EvidenceStage::PublicArtifact];
            Ok(())
        },
        "outside catalog",
    )?;
    assert_lifecycle_rejects(
        |catalog| {
            catalog.allowed_stages = vec![EvidenceStage::ReleaseCandidate];
            Ok(())
        },
        "stage bound is exact_source_local only",
    )?;
    // Cells feed only the exact-source profile.
    assert_lifecycle_rejects(
        |catalog| {
            let cell = lifecycle_cell_mut(catalog, "vim.vim_lsp.lifecycle.host_reopen")?;
            cell.allowed_profiles = vec!["vim_programme_closeout".to_string()];
            Ok(())
        },
        "may feed only vim_first_class_exact_source",
    )?;
    // Cross-client receipts cannot register here.
    for impostor in ["coc", "yegappan/lsp", "neovim", "vimspector"] {
        assert_lifecycle_rejects(
            |catalog| {
                let cell = lifecycle_cell_mut(catalog, "vim.vim_lsp.lifecycle.host_reopen")?;
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
fn lifecycle_cell_digests_discriminate_binding_edits() -> Result<()> {
    let compiled = lifecycle::lifecycle_catalog();
    let cell_id = "vim.vim_lsp.lifecycle.late_result_rejected";
    let cell = compiled
        .cells
        .iter()
        .find(|cell| cell.cell_id == cell_id)
        .with_context(|| format!("{cell_id} missing"))?;
    let before = catalog::cell_digest(cell)?;
    let catalog_before = catalog::catalog_digest(&compiled)?;
    let registry_before = catalog::validate_compiled_registry()?.digest;

    let mut edited = compiled.clone();
    let cell = edited
        .cells
        .iter_mut()
        .find(|cell| cell.cell_id == cell_id)
        .with_context(|| format!("{cell_id} missing"))?;
    cell.subject_dimensions.push("generation.session".to_string());
    ensure!(
        before != catalog::cell_digest(cell)?,
        "a lifecycle binding edit did not change the cell digest"
    );
    ensure!(
        catalog_before != catalog::catalog_digest(&edited)?,
        "a lifecycle binding edit did not change the family catalog digest"
    );
    ensure!(
        registry_before.starts_with("sha256:"),
        "registry digest is not a sha256 identity: {registry_before}"
    );
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
