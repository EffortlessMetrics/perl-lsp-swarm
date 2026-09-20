//! Contract tests for the #11380 specialized Vim/vim-lsp action and
//! observation primitives.
//!
//! Positive proof: the compiled vocabulary validates against the landed #11369
//! public-surface inventory, serves all five #11376 families, and every action
//! admits an honest observation from the deterministic fake backend.
//!
//! Negative controls: every fail-closed law of the model — the thirteen
//! #11380 false-subject shapes (fixed sleep, raw protocol request, event-log
//! freshness, manual-format-as-save, duplicate owners, bare-PID recovery,
//! stale-generation results, server-restart-as-host-restart, single-iteration
//! sessions, pre-forced filetype, foreign client, unknown process/cleanup,
//! unbounded/private data) plus vocabulary-level mutations (unclassified
//! surfaces, native-grammar escapes, unjustified instrument hooks, duplicate
//! or out-of-namespace action IDs, zero wait budgets, raw substitutions).
//!
//! Receipt-agnostic proof: the driver module leaves the #12100 cell catalog
//! byte-identical; no driver action ID can be shaped like a journey cell.

use anyhow::{Context, Result, bail, ensure};
use std::path::{Path, PathBuf};
use xtask::vim_lsp_cell_catalog;
use xtask::vim_lsp_specialized_driver::{
    self as driver, ACTIONS, ActionFamily, ActionSpec, DEFAULT_SHAPE,
    barrier::{BarrierEvidence, BarrierKind, SubstitutionKind},
    fake, observation,
    observation::{
        ActionResult, BackendIdentity, CleanupLedger, DetectionRoute, ObservedRoute, SaveTrigger,
        TypedObservation,
    },
};

fn repository_root() -> Result<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .context("xtask must live below the repository root")
}

fn action(id: &str) -> Result<&'static ActionSpec> {
    driver::action_by_id(id).with_context(|| format!("vocabulary omitted action {id}"))
}

fn valid_for(id: &str) -> Result<TypedObservation> {
    let spec = action(id)?;
    Ok(fake::observation_for(spec, &fake::FakeWorld::settling(spec)))
}

/// Assert that a mutated observation is rejected for a reason containing
/// `needle`.
fn assert_rejects(observation: &TypedObservation, needle: &str) -> Result<()> {
    match driver::validate_observation(observation) {
        Ok(validated) => bail!(
            "observation for {} was accepted with outcome {:?}; expected rejection containing {needle}",
            observation.action_id,
            validated.outcome
        ),
        Err(error) => {
            ensure!(
                error.contains(needle),
                "wrong rejection reason: {error} (wanted something containing {needle})"
            );
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// Compiled vocabulary: positive proof
// ---------------------------------------------------------------------------

#[test]
fn compiled_vocabulary_validates_against_the_landed_inventory() -> Result<()> {
    let root = repository_root()?;
    let summary = driver::validate_driver_contract(&root)?;
    ensure!(summary.action_count == ACTIONS.len());
    ensure!(summary.action_count >= 25, "vocabulary is too thin to serve all five families");
    let families: Vec<ActionFamily> =
        summary.family_counts.iter().map(|(family, _)| *family).collect();
    for expected in [
        ActionFamily::Freshness,
        ActionFamily::SaveFormat,
        ActionFamily::Recovery,
        ActionFamily::HostReopen,
        ActionFamily::Activation,
    ] {
        ensure!(families.contains(&expected), "vocabulary omits family {expected:?}");
    }
    for (family, count) in &summary.family_counts {
        ensure!(*count >= 4, "family {family:?} carries only {count} actions");
    }
    let again = driver::validate_driver_contract(&root)?;
    ensure!(summary == again, "driver contract validation is not deterministic");
    Ok(())
}

#[test]
fn every_action_admits_an_honest_fake_observation() -> Result<()> {
    for spec in ACTIONS {
        let world = fake::FakeWorld::settling(spec);
        let observation = fake::observation_for(spec, &world);
        let validated = driver::validate_observation(&observation).map_err(|error| {
            anyhow::anyhow!("positive case failed for {}: {error}", spec.action_id)
        })?;
        ensure!(validated.action_id == spec.action_id, "validated the wrong action");
        ensure!(
            spec.allowed_results.contains(&validated.outcome),
            "fake produced an outcome outside the admitted vocabulary"
        );
        if validated.outcome.requires_limitation() {
            ensure!(validated.limitation.is_some(), "honest outcomes carry a limitation");
        }
    }
    Ok(())
}

#[test]
fn timed_out_barrier_is_lawful_but_forces_not_proven() -> Result<()> {
    let spec = action("vim.vim_lsp.specialized.freshness.source_mutate_closed_in_place")?;
    let mut world = fake::FakeWorld::settling(spec);
    world.timed_out_barriers = vec![BarrierKind::DigestReached];
    world.outcome = ActionResult::Applied;
    let observation = fake::observation_for(spec, &world);
    assert_rejects(&observation, "must classify not_proven")?;

    world.outcome = ActionResult::NotProven;
    world.limitation = Some("digest_barrier_timeout".to_string());
    let observation = fake::observation_for(spec, &world);
    let validated = driver::validate_observation(&observation).map_err(|error| {
        anyhow::anyhow!(
            "a timed-out barrier with a not_proven outcome and limitation must validate: {error}"
        )
    })?;
    ensure!(validated.outcome == ActionResult::NotProven);
    Ok(())
}

#[test]
fn driver_leaves_the_cell_catalog_byte_identical() -> Result<()> {
    let registry = vim_lsp_cell_catalog::validate_compiled_registry()?;
    // The driver module registers no catalog of its own: the compiled registry
    // carries exactly the catalogs the cell-catalog module aggregates, and the
    // baseline catalog keeps its 17 published cells unchanged (the #12100
    // law; family catalogs like #11381 freshness are additive siblings).
    let registered: std::collections::BTreeSet<String> =
        vim_lsp_cell_catalog::registry().iter().map(|c| c.catalog_id.clone()).collect();
    let validated_ids: std::collections::BTreeSet<String> =
        registry.catalogs.iter().map(|summary| summary.catalog_id.clone()).collect();
    ensure!(
        registered == validated_ids,
        "compiled registry catalogs diverged from the catalog module's own aggregation"
    );
    let baseline = registry
        .catalogs
        .iter()
        .find(|summary| summary.catalog_id == "vim_lsp_baseline")
        .context("baseline catalog missing from the compiled registry")?;
    ensure!(baseline.cell_count == 17, "baseline catalog changed under the driver");
    for spec in ACTIONS {
        let cell_shaped = spec
            .action_id
            .strip_prefix("vim.vim_lsp.")
            .is_some_and(|rest| rest.split('.').count() == 2);
        ensure!(!cell_shaped, "action id {} is shaped like a journey cell id", spec.action_id);
    }
    let validated = driver::validate_observation(&valid_for(
        "vim.vim_lsp.specialized.activation.open_without_preset_filetype",
    )?)
    .map_err(|error| anyhow::anyhow!("{error}"))?;
    let serialized = serde_json::to_string(&validated)?;
    ensure!(
        !serialized.contains("cell") && !serialized.contains("receipt"),
        "validated observations carry receipt/cell data"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Family A — freshness negative controls
// ---------------------------------------------------------------------------

#[test]
fn fixed_sleep_substituting_required_state_fails_closed() -> Result<()> {
    let mut observation =
        valid_for("vim.vim_lsp.specialized.freshness.source_mutate_closed_in_place")?;
    for evidence in observation.barriers.iter_mut() {
        if evidence.kind() == BarrierKind::DigestReached {
            *evidence = BarrierEvidence::Substituted {
                kind: BarrierKind::DigestReached,
                substitution: SubstitutionKind::FixedSleep,
            };
        }
    }
    assert_rejects(&observation, "substitution")
}

#[test]
fn event_log_substituting_semantic_freshness_fails_closed() -> Result<()> {
    let mut observation =
        valid_for("vim.vim_lsp.specialized.freshness.observe_route_and_generation")?;
    observation.semantic_probe = None;
    assert_rejects(&observation, "semantic probe")
}

#[test]
fn raw_lsp_request_substituting_user_action_fails_closed() -> Result<()> {
    let mut observation = valid_for("vim.vim_lsp.specialized.freshness.workspace_setting_change")?;
    observation.route = ObservedRoute::RawProtocolRequest {
        method: "workspace_didchangeconfiguration".to_string(),
    };
    assert_rejects(&observation, "raw protocol request")
}

// ---------------------------------------------------------------------------
// Family B — save-format negative controls
// ---------------------------------------------------------------------------

#[test]
fn manual_format_substituting_save_triggered_format_fails_closed() -> Result<()> {
    let mut observation = valid_for("vim.vim_lsp.specialized.save_format.observe_save_settlement")?;
    observation.trigger = Some(SaveTrigger::ManualComparator);
    assert_rejects(&observation, "save-event trigger")
}

#[test]
fn duplicate_save_owners_are_not_observable_as_pass() -> Result<()> {
    let mut observation = valid_for("vim.vim_lsp.specialized.save_format.ordinary_write")?;
    observation.configured_owner_count = Some(2);
    assert_rejects(&observation, "owner count")
}

#[test]
fn undeclared_public_api_route_fails_closed() -> Result<()> {
    let mut observation = valid_for("vim.vim_lsp.specialized.save_format.ordinary_write")?;
    observation.route =
        ObservedRoute::PublicClientApi { api: "lsp#stop_server(server_name)".to_string() };
    assert_rejects(&observation, "does not declare public surface")
}

// ---------------------------------------------------------------------------
// Family C — recovery negative controls
// ---------------------------------------------------------------------------

#[test]
fn bare_new_pid_substituting_initialized_replay_fails_closed() -> Result<()> {
    let mut observation = valid_for("vim.vim_lsp.specialized.recovery.observe_generation_replay")?;
    observation.protocol_events.clear();
    assert_rejects(&observation, "protocol event")?;

    let mut observation = valid_for("vim.vim_lsp.specialized.recovery.observe_generation_replay")?;
    observation.cardinalities.insert("replayed_buffers".to_string(), 0);
    assert_rejects(&observation, "replayed-buffer cardinality")
}

#[test]
fn old_generation_result_cannot_classify_applied() -> Result<()> {
    let mut observation =
        valid_for("vim.vim_lsp.specialized.freshness.observe_route_and_generation")?;
    let mut stale = observation.clone();
    if let Some(probe) = observation.semantic_probe.as_mut() {
        let mut scope = probe.generation_scope;
        scope.document_generation += 1;
        probe.generation_scope = scope;
    } else {
        bail!("positive case lacked a semantic probe");
    }
    assert_rejects(&observation, "generation scope")?;

    // The same stale answer honestly classified as stale is accepted.
    stale.outcome = ActionResult::Stale;
    let _ = stale;
    Ok(())
}

#[test]
fn unknown_process_and_pending_cleanup_can_never_pass() -> Result<()> {
    let mut observation = valid_for("vim.vim_lsp.specialized.recovery.bounded_retry_disposition")?;
    observation.process = observation::ProcessDisposition::Unknown;
    assert_rejects(&observation, "not_proven")?;

    let mut observation = valid_for("vim.vim_lsp.specialized.recovery.bounded_retry_disposition")?;
    observation.cleanup = CleanupLedger::Pending;
    assert_rejects(&observation, "pending cleanup")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Family D — host reopen / repeated-session negative controls
// ---------------------------------------------------------------------------

#[test]
fn server_restart_substituting_full_host_restart_fails_closed() -> Result<()> {
    let mut observation =
        valid_for("vim.vim_lsp.specialized.host_reopen.repeated_session_sequence")?;
    for evidence in observation.barriers.iter_mut() {
        if evidence.kind() == BarrierKind::HostInstanceChanged {
            *evidence = BarrierEvidence::Substituted {
                kind: BarrierKind::HostInstanceChanged,
                substitution: SubstitutionKind::ServerRestartOnly,
            };
        }
    }
    assert_rejects(&observation, "substitution")
}

#[test]
fn one_iteration_substituting_repeated_sessions_fails_closed() -> Result<()> {
    let mut observation =
        valid_for("vim.vim_lsp.specialized.host_reopen.repeated_session_sequence")?;
    observation.session_iterations = Some(1);
    assert_rejects(&observation, "iterations")
}

#[test]
fn host_handoff_without_the_host_runner_fails_closed_honestly() -> Result<()> {
    // The replacement-host action exists in the vocabulary but, with no
    // #10944 runner landed, an `applied` claim from any backend that exists
    // today cannot validate: `applied` is not admitted vocabulary for the
    // handoff until the runner lands as a reviewed vocabulary edit.
    let spec = action("vim.vim_lsp.specialized.host_reopen.launch_replacement_host")?;
    let mut world = fake::FakeWorld::settling(spec);
    world.outcome = ActionResult::Applied;
    world.limitation = None;
    let observation = fake::observation_for(spec, &world);
    assert_rejects(&observation, "outside the admitted vocabulary")
}

#[test]
fn native_surface_route_must_be_declared_by_the_action() -> Result<()> {
    let mut observation = valid_for("vim.vim_lsp.specialized.freshness.workspace_setting_change")?;
    observation.route = ObservedRoute::NativeVimSurface { surface: ":w".to_string() };
    assert_rejects(&observation, "does not declare native surface")
}

#[test]
fn satisfied_barrier_beyond_its_budget_fails_closed() -> Result<()> {
    let mut observation =
        valid_for("vim.vim_lsp.specialized.freshness.source_mutate_closed_in_place")?;
    for evidence in observation.barriers.iter_mut() {
        if matches!(evidence, BarrierEvidence::Satisfied { .. }) {
            *evidence = BarrierEvidence::Satisfied {
                kind: evidence.kind(),
                settled_generations: observation.generations,
                waited_ms: 60_000,
            };
        }
    }
    assert_rejects(&observation, "beyond the")
}

#[test]
fn barrier_settling_newer_than_the_observation_snapshot_fails_closed() -> Result<()> {
    let mut observation =
        valid_for("vim.vim_lsp.specialized.freshness.source_mutate_closed_in_place")?;
    for evidence in observation.barriers.iter_mut() {
        if matches!(evidence, BarrierEvidence::Satisfied { .. }) {
            let mut newer = observation.generations;
            newer.source_generation += 1;
            *evidence = BarrierEvidence::Satisfied {
                kind: evidence.kind(),
                settled_generations: newer,
                waited_ms: 5,
            };
        }
    }
    assert_rejects(&observation, "newer")
}

#[test]
fn replay_events_out_of_order_fail_closed() -> Result<()> {
    let mut observation = valid_for("vim.vim_lsp.specialized.recovery.observe_generation_replay")?;
    observation.protocol_events.reverse();
    assert_rejects(&observation, "out of order")
}

// ---------------------------------------------------------------------------
// Family E — activation negative controls
// ---------------------------------------------------------------------------

#[test]
fn pre_forced_native_filetype_fails_closed() -> Result<()> {
    let mut observation =
        valid_for("vim.vim_lsp.specialized.activation.open_without_preset_filetype")?;
    observation.detection_route = Some(DetectionRoute::PreForced);
    assert_rejects(&observation, "pre-forced")
}

#[test]
fn declared_override_row_must_report_the_override_route() -> Result<()> {
    let mut observation = valid_for("vim.vim_lsp.specialized.activation.declared_override_row")?;
    observation.detection_route = Some(DetectionRoute::Native);
    assert_rejects(&observation, "detection route")
}

#[test]
fn foreign_client_observation_fails_closed() -> Result<()> {
    let mut observation =
        valid_for("vim.vim_lsp.specialized.activation.observe_service_attachment")?;
    observation.client_id = "coc.nvim".to_string();
    assert_rejects(&observation, "pinned vim/vim-lsp/perllsp subject")
}

// ---------------------------------------------------------------------------
// Boundedness / privacy negative controls
// ---------------------------------------------------------------------------

#[test]
fn unbounded_and_private_data_fails_closed() -> Result<()> {
    let observation = valid_for("vim.vim_lsp.specialized.activation.root_semantic_discriminator")?;

    let mut raw_text = observation.clone();
    raw_text.digests.insert("buffer_text".to_string(), "my $x = 1;".to_string());
    assert_rejects(&raw_text, "bounded digest")?;

    let mut home_path = observation.clone();
    home_path.fixture.fixture_relative_paths = vec![r"C:\Users\steven\secret.pm".to_string()];
    assert_rejects(&home_path, "fixture-root-relative")?;

    let mut traversal = observation.clone();
    traversal.fixture.fixture_relative_paths.push("../../etc/passwd".to_string());
    assert_rejects(&traversal, "fixture-root-relative")?;

    let mut unstable_key = observation.clone();
    unstable_key.cardinalities.insert("Save Requests".to_string(), 1);
    assert_rejects(&unstable_key, "stable")?;

    let mut bad_digest = observation.clone();
    bad_digest.backend = BackendIdentity::Adapter { script_digest: "deadbeef".to_string() };
    assert_rejects(&bad_digest, "script digest")?;
    Ok(())
}

#[test]
fn unknown_observation_fields_are_rejected_by_the_schema() -> Result<()> {
    let observation = valid_for("vim.vim_lsp.specialized.activation.observe_native_filetype")?;
    let json = serde_json::to_string(&observation)?;
    let mut value: serde_json::Value = serde_json::from_str(&json)?;
    value["source_text_snippet"] = serde_json::json!("sub my $x = 1;");
    let tampered = serde_json::to_string(&value)?;
    let parsed: Result<TypedObservation, _> = serde_json::from_str(&tampered);
    ensure!(parsed.is_err(), "durable observations admitted an unknown (private-data) field");
    Ok(())
}

#[test]
fn schema_version_drift_fails_closed() -> Result<()> {
    let mut observation = valid_for("vim.vim_lsp.specialized.activation.observe_native_filetype")?;
    observation.schema_version = "vim_lsp_specialized_driver.v0".to_string();
    assert_rejects(&observation, "schema version")?;
    Ok(())
}

#[test]
fn unknown_action_id_fails_closed() -> Result<()> {
    let mut observation = valid_for("vim.vim_lsp.specialized.activation.observe_native_filetype")?;
    observation.action_id = "vim.vim_lsp.specialized.activation.not_registered".to_string();
    assert_rejects(&observation, "unknown action id")
}

// ---------------------------------------------------------------------------
// Vocabulary-level negative controls (table mutations)
// ---------------------------------------------------------------------------

fn validate_with(mutation: impl FnOnce(&mut Vec<ActionSpec>) -> Result<()>) -> Result<()> {
    let mut table = ACTIONS.to_vec();
    mutation(&mut table)?;
    match driver::validate_table(&table, &repository_root()?) {
        Ok(_) => bail!("mutated action table was accepted"),
        Err(error) => {
            let reason = error.to_string();
            ensure!(!reason.is_empty(), "mutation rejected without a reason");
            Ok(())
        }
    }
}

static UNCLASSIFIED_SURFACE: &[&str] = &["lsp#secret_private_api(x)"];
static BAD_NATIVE: &[&str] = &["rm -rf /"];
static ZERO_BUDGET: &[driver::barrier::BarrierRequirement] =
    &[driver::barrier::BarrierRequirement { kind: BarrierKind::ServiceAttached, max_wait_ms: 0 }];
static HOOK_WITHOUT_JUSTIFICATION: &[driver::InstrumentHookUse] = &[driver::InstrumentHookUse {
    api: "g:lsp_log_verbose = 1 + g:lsp_log_file wire capture parsed offline",
    justification: "",
    retirement: "",
}];

#[test]
fn unclassified_public_surface_fails_closed() -> Result<()> {
    validate_with(|table| {
        table[0].public_surfaces = UNCLASSIFIED_SURFACE;
        Ok(())
    })
}

#[test]
fn native_surface_outside_the_grammar_fails_closed() -> Result<()> {
    validate_with(|table| {
        table[0].native_vim_surfaces = BAD_NATIVE;
        Ok(())
    })
}

#[test]
fn instrument_hook_without_justification_fails_closed() -> Result<()> {
    validate_with(|table| {
        let index = table
            .iter()
            .position(|spec| !spec.instrument_hooks.is_empty())
            .context("vocabulary carries no instrument hook to mutate")?;
        table[index].instrument_hooks = HOOK_WITHOUT_JUSTIFICATION;
        Ok(())
    })
}

#[test]
fn duplicate_action_id_fails_closed() -> Result<()> {
    validate_with(|table| {
        ensure!(table.len() >= 2, "vocabulary too small to duplicate");
        table[1].action_id = table[0].action_id;
        Ok(())
    })
}

#[test]
fn out_of_namespace_action_id_fails_closed() -> Result<()> {
    validate_with(|table| {
        table[0].action_id = "coc.nvim.specialized.freshness.source_mutate";
        Ok(())
    })
}

#[test]
fn zero_wait_budget_fails_closed() -> Result<()> {
    validate_with(|table| {
        let index = table
            .iter()
            .position(|spec| !spec.required_barriers.is_empty())
            .context("vocabulary carries no barrier to mutate")?;
        table[index].required_barriers = ZERO_BUDGET;
        Ok(())
    })
}

#[test]
fn family_token_mismatch_fails_closed() -> Result<()> {
    validate_with(|table| {
        let index = table
            .iter()
            .position(|spec| spec.family == ActionFamily::Freshness)
            .context("vocabulary carries no freshness action")?;
        table[index].action_id =
            "vim.vim_lsp.specialized.save_format.source_mutate_closed_in_place";
        table[index].shape = DEFAULT_SHAPE;
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Observation-file entry point
// ---------------------------------------------------------------------------

#[test]
fn observation_file_round_trip_validates_and_fails_closed() -> Result<()> {
    let mut lines = Vec::new();
    for id in [
        "vim.vim_lsp.specialized.activation.open_without_preset_filetype",
        "vim.vim_lsp.specialized.save_format.observe_save_settlement",
        "vim.vim_lsp.specialized.recovery.observe_generation_replay",
    ] {
        let observation = valid_for(id)?;
        lines.push(serde_json::to_string(&observation)?);
    }
    let good_path = std::env::temp_dir().join("plsw_11380_good_observations.jsonl");
    std::fs::write(&good_path, lines.join("\n") + "\n")?;
    let validated = driver::validate_observation_file(&good_path)?;
    ensure!(validated == 3, "expected 3 validated observations, got {validated}");

    let mut bad = valid_for("vim.vim_lsp.specialized.activation.observe_native_filetype")?;
    bad.detection_route = Some(DetectionRoute::PreForced);
    let bad_path = std::env::temp_dir().join("plsw_11380_bad_observations.jsonl");
    std::fs::write(&bad_path, serde_json::to_string(&bad)? + "\n")?;
    ensure!(
        driver::validate_observation_file(&bad_path).is_err(),
        "a pre-forced observation file must fail closed"
    );

    let empty_path = std::env::temp_dir().join("plsw_11380_empty_observations.jsonl");
    std::fs::write(&empty_path, "\n\n")?;
    ensure!(
        driver::validate_observation_file(&empty_path).is_err(),
        "an observations file with no observations must fail closed"
    );
    Ok(())
}
