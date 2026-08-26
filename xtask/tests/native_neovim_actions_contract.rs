//! Contract tests for the `native_neovim_actions.v1` native Neovim
//! built-in-LSP action and observation contract (#11409).
//!
//! Positive proof: the compiled vocabulary validates, serves every section of
//! the issue's action contract (the honest denominator — no BDD ledger is
//! landed yet, #10888 owns it), every action admits an honest observation
//! from the deterministic fake backend, and a full ordered run plus the
//! observation-file entry point round-trip.
//!
//! Negative controls: all twelve #11409 falsifiers — fixed sleep, global
//! workspace idle, any-result satisfaction, log text, server-response-only
//! (returned-not-applied), wrong client/renamed server, raw companion
//! relabeled ordinary, stale generation, host action implementing process
//! policy, self-derived expectation, unknown/local action, and
//! unbounded/private evidence — plus vocabulary-level mutations (duplicate or
//! out-of-namespace IDs, grammar escapes, unjustified hooks, zero budgets,
//! missing not_proven) and run-ordering laws.
//!
//! Claim ceiling: these tests prove driver/contract semantics only. Nothing
//! here proves actual Neovim or `perllsp` behavior; no host is launched.

use anyhow::{Context, Result, bail, ensure};
use xtask::native_neovim_actions::{
    self as contract, ACTIONS, ActionFamily, ActionSpec, InputBinding, InputKind,
    SurfaceClassification, fake,
    observation::{
        AnchorPosition, BackendIdentity, EffectClass, EffectStage, EvidenceKind, EvidenceRef,
        ExpectationSource, ObservationPlane, ObservationResult, ObservedRoute, TypedObservation,
    },
    predicate::{PredicateEvidence, PredicateKind, SubstitutionKind},
};

fn action(id: &str) -> Result<&'static ActionSpec> {
    contract::action_by_id(id).with_context(|| format!("vocabulary omitted action {id}"))
}

fn valid_for(id: &str) -> Result<TypedObservation> {
    let spec = action(id)?;
    fake::observation_for(spec, 1, &fake::FakeWorld::settling())
        .map_err(|error| anyhow::anyhow!("fake builder failed for {id}: {error}"))
}

/// Assert that a mutated observation is rejected for a reason containing
/// `needle`.
fn assert_rejects(observation: &TypedObservation, needle: &str) -> Result<()> {
    match contract::validate_observation(observation) {
        Ok(validated) => bail!(
            "observation for {} was accepted with result {:?}; expected rejection containing {needle}",
            observation.action_id,
            validated.result
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
fn compiled_vocabulary_validates_and_serves_the_issue_action_contract() -> Result<()> {
    let summary = contract::validate_compiled_contract()?;
    ensure!(summary.action_count == ACTIONS.len());
    let families: Vec<ActionFamily> =
        summary.family_counts.iter().map(|(family, _)| *family).collect();
    for expected in [
        ActionFamily::HostSession,
        ActionFamily::ClientAttachment,
        ActionFamily::ReadMethods,
        ActionFamily::ConfigEdits,
        ActionFamily::TextSyncLifecycle,
    ] {
        ensure!(families.contains(&expected), "vocabulary omits family {expected:?}");
    }
    for (family, count) in &summary.family_counts {
        ensure!(*count >= 4, "family {family:?} carries only {count} actions");
    }
    // The issue's action-contract sections are the honest denominator until
    // the #10888 BDD ledger lands: every named operation has an action, and
    // the enumeration must cover the complete compiled registry row-for-row.
    let core_denominator: &[&str] = &[
        "neovim.native.host_session.start_isolated_host",
        "neovim.native.host_session.load_canonical_config",
        "neovim.native.host_session.open_buffer",
        "neovim.native.host_session.edit_buffer",
        "neovim.native.host_session.write_buffer",
        "neovim.native.host_session.close_buffer",
        "neovim.native.host_session.reopen_buffer",
        "neovim.native.host_session.stop_client_normal_route",
        "neovim.native.host_session.exit_host_normal",
        "neovim.native.client_attachment.observe_filetype_before_override",
        "neovim.native.client_attachment.identify_client_and_process",
        "neovim.native.client_attachment.observe_initialize_identities",
        "neovim.native.client_attachment.exclude_foreign_clients",
        "neovim.native.read_methods.wait_target_diagnostic_state",
        "neovim.native.read_methods.request_completion",
        "neovim.native.read_methods.accept_completion",
        "neovim.native.read_methods.request_hover",
        "neovim.native.read_methods.drive_definition_navigation",
        "neovim.native.read_methods.request_optional_cells",
        "neovim.native.config_edits.apply_client_settings",
        "neovim.native.config_edits.observe_setting_effect",
        "neovim.native.config_edits.request_document_format",
        "neovim.native.config_edits.request_range_format",
        "neovim.native.config_edits.request_rename",
        "neovim.native.config_edits.request_code_action",
        "neovim.native.config_edits.request_workspace_edits",
        "neovim.native.config_edits.observe_resulting_state",
        "neovim.native.text_sync_lifecycle.ordinary_edit_didchange",
        "neovim.native.text_sync_lifecycle.companion_multichange_control",
        "neovim.native.text_sync_lifecycle.companion_invalid_notification_control",
        "neovim.native.text_sync_lifecycle.full_source_recovery_reopen",
        "neovim.native.text_sync_lifecycle.held_work_barrier",
        "neovim.native.text_sync_lifecycle.root_add_remove",
        "neovim.native.text_sync_lifecycle.post_run_observation_handoff",
    ];
    ensure!(
        core_denominator.len() == ACTIONS.len(),
        "the denominator enumerates {} actions but the registry carries {}; keep them row-for-row",
        core_denominator.len(),
        ACTIONS.len()
    );
    for required in core_denominator {
        ensure!(contract::action_by_id(required).is_some(), "core set omits {required}");
    }
    let again = contract::validate_compiled_contract()?;
    ensure!(summary == again, "contract validation is not deterministic");
    Ok(())
}

#[test]
fn every_action_admits_an_honest_fake_observation() -> Result<()> {
    for (index, spec) in ACTIONS.iter().enumerate() {
        let observation =
            fake::observation_for(spec, index as u64 + 1, &fake::FakeWorld::settling()).map_err(
                |error| anyhow::anyhow!("fake builder failed for {}: {error}", spec.action_id),
            )?;
        let validated = contract::validate_observation(&observation).map_err(|error| {
            anyhow::anyhow!("positive case failed for {}: {error}", spec.action_id)
        })?;
        ensure!(validated.action_id == spec.action_id, "validated the wrong action");
        ensure!(
            spec.allowed_results.contains(&validated.result),
            "fake produced a result outside the admitted vocabulary for {}",
            spec.action_id
        );
        if validated.result.requires_limitation() {
            ensure!(
                validated.limitation_class.is_some(),
                "honest results carry a limitation class"
            );
        }
    }
    Ok(())
}

#[test]
fn full_ordered_run_and_observation_file_round_trip() -> Result<()> {
    let mut run = Vec::new();
    let mut sequence = 0u64;
    for id in [
        "neovim.native.host_session.start_isolated_host",
        "neovim.native.host_session.load_canonical_config",
        "neovim.native.host_session.open_buffer",
        "neovim.native.client_attachment.observe_filetype_before_override",
        "neovim.native.client_attachment.identify_client_and_process",
        "neovim.native.client_attachment.observe_initialize_identities",
        "neovim.native.client_attachment.exclude_foreign_clients",
        "neovim.native.text_sync_lifecycle.ordinary_edit_didchange",
        "neovim.native.read_methods.wait_target_diagnostic_state",
        "neovim.native.read_methods.request_completion",
        "neovim.native.read_methods.accept_completion",
        "neovim.native.read_methods.request_hover",
        "neovim.native.read_methods.drive_definition_navigation",
        "neovim.native.config_edits.request_document_format",
        "neovim.native.config_edits.request_rename",
        "neovim.native.config_edits.observe_resulting_state",
        "neovim.native.text_sync_lifecycle.full_source_recovery_reopen",
        "neovim.native.host_session.stop_client_normal_route",
        "neovim.native.host_session.exit_host_normal",
        "neovim.native.text_sync_lifecycle.post_run_observation_handoff",
    ] {
        sequence += 1;
        let spec = action(id)?;
        let observation = fake::observation_for(spec, sequence, &fake::FakeWorld::settling())
            .map_err(|error| anyhow::anyhow!("{id}: {error}"))?;
        run.push(observation);
    }
    let validated = contract::validate_observation_run(&run)?;
    ensure!(validated == run.len(), "run length mismatch");

    let lines: Vec<String> =
        run.iter().map(serde_json::to_string).collect::<std::result::Result<_, _>>()?;
    let path = std::env::temp_dir().join("plsw_11409_good_run.jsonl");
    std::fs::write(&path, lines.join("\n") + "\n")?;
    ensure!(
        contract::validate_observation_file(&path)? == run.len(),
        "observation file round trip lost observations"
    );

    let empty = std::env::temp_dir().join("plsw_11409_empty_run.jsonl");
    std::fs::write(&empty, "\n\n")?;
    ensure!(
        contract::validate_observation_file(&empty).is_err(),
        "an observations file with no observations must fail closed"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Predicate/wait law
// ---------------------------------------------------------------------------

#[test]
fn timed_out_predicate_is_lawful_but_forces_not_proven() -> Result<()> {
    let spec = action("neovim.native.read_methods.wait_target_diagnostic_state")?;
    let mut world = fake::FakeWorld::settling();
    world.timed_out_predicates = vec![PredicateKind::DiagnosticStateCurrent];
    world.result = Some(ObservationResult::Observed);
    let observation = fake::observation_for(spec, 1, &world).map_err(|e| anyhow::anyhow!("{e}"))?;
    assert_rejects(&observation, "must classify not_proven")?;

    world.result = Some(ObservationResult::NotProven);
    let observation = fake::observation_for(spec, 2, &world).map_err(|e| anyhow::anyhow!("{e}"))?;
    let validated = contract::validate_observation(&observation).map_err(|error| {
        anyhow::anyhow!("a timed-out predicate with not_proven must validate: {error}")
    })?;
    ensure!(validated.result == ObservationResult::NotProven);
    Ok(())
}

#[test]
fn satisfied_predicate_needs_a_named_state_and_a_poll_inside_the_budget() -> Result<()> {
    // No settled-state digest (elapsed time alone): rejected.
    let mut observation = valid_for("neovim.native.read_methods.request_hover")?;
    if let Some(evidence) = observation.predicate_evidence.first_mut() {
        *evidence = PredicateEvidence::Satisfied {
            kind: PredicateKind::HoverResultExact,
            settled_state_digest: "not_a_digest".to_string(),
            settled_generations: observation.generations,
            polls: 2,
            waited_ms: 50,
        };
    }
    assert_rejects(&observation, "never satisfaction")?;

    // Zero polls: satisfaction without polling is manufactured.
    let mut observation = valid_for("neovim.native.read_methods.request_hover")?;
    if let Some(evidence) = observation.predicate_evidence.first_mut() {
        *evidence = PredicateEvidence::Satisfied {
            kind: PredicateKind::HoverResultExact,
            settled_state_digest: fake::fake_digest("settled"),
            settled_generations: observation.generations,
            polls: 0,
            waited_ms: 50,
        };
    }
    assert_rejects(&observation, "without a single poll")?;

    // Satisfaction beyond the declared budget: rejected.
    let mut observation = valid_for("neovim.native.read_methods.request_hover")?;
    if let Some(evidence) = observation.predicate_evidence.first_mut() {
        *evidence = PredicateEvidence::Satisfied {
            kind: PredicateKind::HoverResultExact,
            settled_state_digest: fake::fake_digest("settled"),
            settled_generations: observation.generations,
            polls: 2,
            waited_ms: 60_000,
        };
    }
    assert_rejects(&observation, "beyond the")?;
    Ok(())
}

#[test]
fn fixed_sleep_and_global_idle_substitutions_fail_closed() -> Result<()> {
    for substitution in [SubstitutionKind::FixedSleep, SubstitutionKind::GlobalWorkspaceIdle] {
        let mut observation = valid_for("neovim.native.read_methods.wait_target_diagnostic_state")?;
        for evidence in observation.predicate_evidence.iter_mut() {
            *evidence = PredicateEvidence::Substituted {
                kind: PredicateKind::DiagnosticStateCurrent,
                substitution: substitution.clone(),
            };
        }
        assert_rejects(&observation, "substitution")?;
    }
    Ok(())
}

#[test]
fn log_text_substituting_actual_traffic_fails_closed() -> Result<()> {
    let mut observation = valid_for("neovim.native.text_sync_lifecycle.ordinary_edit_didchange")?;
    for evidence in observation.predicate_evidence.iter_mut() {
        *evidence = PredicateEvidence::Substituted {
            kind: PredicateKind::DocumentGenerationAccepted,
            substitution: SubstitutionKind::LogTextOnly,
        };
    }
    assert_rejects(&observation, "substitution")
}

#[test]
fn predicate_evidence_the_action_does_not_require_fails_closed() -> Result<()> {
    let mut observation = valid_for("neovim.native.read_methods.request_hover")?;
    observation.predicate_evidence.push(PredicateEvidence::Satisfied {
        kind: PredicateKind::DiagnosticStateCurrent,
        settled_state_digest: fake::fake_digest("settled"),
        settled_generations: observation.generations,
        polls: 1,
        waited_ms: 10,
    });
    assert_rejects(&observation, "does not require")
}

#[test]
fn missing_required_predicate_evidence_fails_closed() -> Result<()> {
    let mut observation = valid_for("neovim.native.read_methods.request_hover")?;
    observation.predicate_evidence.clear();
    assert_rejects(&observation, "carries no evidence")
}

// ---------------------------------------------------------------------------
// Falsifier 3 + 10: expected vs observed separation
// ---------------------------------------------------------------------------

#[test]
fn any_result_cannot_satisfy_an_exact_expectation() -> Result<()> {
    // Observed claimed while the observed digest differs from the expected
    // digest: the any-result-satisfies shape.
    let mut observation = valid_for("neovim.native.read_methods.request_completion")?;
    let expectation_digest = observation.expectation.as_ref().map(|e| e.expectation_digest.clone());
    observation.observed.result_digest = fake::fake_digest("some_other_result");
    assert_rejects(&observation, "cannot be satisfied by any result")?;

    // The mismatch label contradicts equal digests.
    let mut observation = valid_for("neovim.native.read_methods.request_completion")?;
    observation.result = ObservationResult::Mismatch;
    observation.limitation_class = Some("expectation_mismatch".to_string());
    if let Some(expectation) = &expectation_digest {
        observation.observed.result_digest = expectation.clone();
    }
    assert_rejects(&observation, "contradicts the bound values")?;

    // Honest mismatch (different digests) validates.
    let mut observation = valid_for("neovim.native.read_methods.request_completion")?;
    observation.result = ObservationResult::Mismatch;
    observation.limitation_class = Some("expectation_mismatch".to_string());
    observation.observed.result_digest = fake::fake_digest("different_result");
    contract::validate_observation(&observation)
        .map_err(|error| anyhow::anyhow!("honest mismatch must validate: {error}"))?;
    Ok(())
}

#[test]
fn self_derived_expectation_fails_closed() -> Result<()> {
    let mut observation = valid_for("neovim.native.read_methods.request_hover")?;
    if let Some(expectation) = observation.expectation.as_mut() {
        expectation.source = ExpectationSource::ObservedOutput;
    }
    assert_rejects(&observation, "derived from the observed output")
}

#[test]
fn missing_expectation_reference_fails_closed() -> Result<()> {
    let mut observation = valid_for("neovim.native.read_methods.request_hover")?;
    observation.expectation = None;
    assert_rejects(&observation, "requires an expected-result reference")
}

// ---------------------------------------------------------------------------
// Falsifier 5: returned-not-applied / server response as buffer state
// ---------------------------------------------------------------------------

#[test]
fn server_response_cannot_substitute_applied_buffer_state() -> Result<()> {
    let spec = action("neovim.native.config_edits.request_document_format")?;
    let mut world = fake::FakeWorld::settling();
    world.stage_override = Some(EffectStage::Returned);
    let observation = fake::observation_for(spec, 1, &world).map_err(|e| anyhow::anyhow!("{e}"))?;
    assert_rejects(&observation, "cannot satisfy an application claim")?;

    // An applied-or-beyond effect must bind its applied digest.
    let mut observation = valid_for("neovim.native.config_edits.request_document_format")?;
    observation.observed.effect_digest = None;
    assert_rejects(&observation, "applied effect digest")
}

// ---------------------------------------------------------------------------
// Falsifier 8: stale generation
// ---------------------------------------------------------------------------

#[test]
fn old_generation_result_cannot_satisfy_a_post_edit_action() -> Result<()> {
    let mut observation = valid_for("neovim.native.read_methods.wait_target_diagnostic_state")?;
    observation.observed.generations.document_generation += 1;
    assert_rejects(&observation, "old-generation")?;

    // Honestly classified not_proven with the stale evidence, it validates.
    let mut observation = valid_for("neovim.native.read_methods.wait_target_diagnostic_state")?;
    observation.observed.generations.document_generation += 1;
    observation.result = ObservationResult::NotProven;
    observation.limitation_class = Some("stale_generation".to_string());
    contract::validate_observation(&observation)
        .map_err(|error| anyhow::anyhow!("honest stale not_proven must validate: {error}"))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 6: wrong client / renamed server
// ---------------------------------------------------------------------------

#[test]
fn foreign_client_and_renamed_server_observations_fail_closed() -> Result<()> {
    let mut observation = valid_for("neovim.native.client_attachment.identify_client_and_process")?;
    observation.subject.client_id = "coc.nvim".to_string();
    assert_rejects(&observation, "pinned neovim host build and canonical config subject")?;

    let mut observation = valid_for("neovim.native.client_attachment.identify_client_and_process")?;
    observation.subject.server_executable = "neovim_named_server".to_string();
    assert_rejects(&observation, "pinned neovim host build and canonical config subject")?;

    let mut observation = valid_for("neovim.native.client_attachment.exclude_foreign_clients")?;
    observation.observed.cardinalities.insert("foreign_clients_attached".to_string(), 1);
    assert_rejects(&observation, "zero foreign clients")?;

    let mut observation = valid_for("neovim.native.client_attachment.exclude_foreign_clients")?;
    observation.observed.cardinalities.insert("pinned_clients_attached".to_string(), 0);
    assert_rejects(&observation, "pinned client attached")
}

// ---------------------------------------------------------------------------
// Falsifier 7: raw companion relabeled ordinary
// ---------------------------------------------------------------------------

#[test]
fn raw_companion_request_relabeled_ordinary_fails_closed() -> Result<()> {
    let mut observation = valid_for("neovim.native.read_methods.request_hover")?;
    observation.route =
        ObservedRoute::CompanionControl { control: "vim.lsp.client.request".to_string() };
    assert_rejects(&observation, "never ordinary Neovim traffic")?;

    let mut observation = valid_for("neovim.native.read_methods.request_hover")?;
    observation.route =
        ObservedRoute::PublicStableApi { api: "vim.lsp.client.request".to_string() };
    assert_rejects(&observation, "does not declare public api")?;

    // The companion control stays lawful on its own companion-class action,
    // on the instrument plane.
    let companion = valid_for("neovim.native.text_sync_lifecycle.companion_multichange_control")?;
    let validated = contract::validate_observation(&companion)
        .map_err(|error| anyhow::anyhow!("honest companion control must validate: {error}"))?;
    ensure!(validated.plane == ObservationPlane::Instrument);
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 9: host action implementing process policy
// ---------------------------------------------------------------------------

#[test]
fn host_handoff_stays_fail_closed_and_owned_by_10894() -> Result<()> {
    // An ordinary action cannot route through a host handoff.
    let mut observation = valid_for("neovim.native.host_session.stop_client_normal_route")?;
    observation.route = ObservedRoute::HostHandoff { handoff: "spawn_with_deadline".to_string() };
    assert_rejects(&observation, "owned by #10894")?;

    // The host handoff actions themselves cannot claim observed results.
    let mut observation = valid_for("neovim.native.host_session.start_isolated_host")?;
    observation.result = ObservationResult::Observed;
    observation.limitation_class = None;
    assert_rejects(&observation, "outside the admitted vocabulary")?;

    // Normal stop is a product action; the post-run handoff is a cleanup
    // plane observation. Swapping planes fails.
    let stop = valid_for("neovim.native.host_session.stop_client_normal_route")?;
    let validated = contract::validate_observation(&stop)
        .map_err(|error| anyhow::anyhow!("normal stop must validate: {error}"))?;
    ensure!(validated.plane == ObservationPlane::Product);
    let handoff = valid_for("neovim.native.text_sync_lifecycle.post_run_observation_handoff")?;
    let validated = contract::validate_observation(&handoff)
        .map_err(|error| anyhow::anyhow!("post-run handoff must validate: {error}"))?;
    ensure!(validated.plane == ObservationPlane::Cleanup);

    let mut swapped = valid_for("neovim.native.host_session.stop_client_normal_route")?;
    swapped.plane = ObservationPlane::Cleanup;
    assert_rejects(&swapped, "does not match action class")
}

// ---------------------------------------------------------------------------
// Falsifier 11: unknown/local action IDs
// ---------------------------------------------------------------------------

#[test]
fn unknown_and_locally_invented_action_ids_fail_closed() -> Result<()> {
    let mut observation = valid_for("neovim.native.read_methods.request_hover")?;
    observation.action_id = "neovim.native.read_methods.private_probe".to_string();
    assert_rejects(&observation, "unknown action id")?;

    let mut observation = valid_for("neovim.native.read_methods.request_hover")?;
    observation.action_id = "neovim.local.read_methods.plausible_action".to_string();
    assert_rejects(&observation, "unknown action id")
}

// ---------------------------------------------------------------------------
// Falsifier 12: unbounded/private evidence
// ---------------------------------------------------------------------------

#[test]
fn unbounded_and_private_data_fails_closed() -> Result<()> {
    let observation = valid_for("neovim.native.read_methods.request_hover")?;

    let mut raw_text = observation.clone();
    raw_text.observed.identity_digests.insert("buffer_text".to_string(), "my $x = 1;".to_string());
    assert_rejects(&raw_text, "bounded digest")?;

    let mut home_path = observation.clone();
    home_path.subject.document.fixture_path = r"C:\Users\steven\secret.pm".to_string();
    assert_rejects(&home_path, "fixture-root-relative")?;

    let mut traversal = observation.clone();
    traversal.subject.document.fixture_path.push_str("/../../escape.pm");
    assert_rejects(&traversal, "fixture-root-relative")?;

    let mut unstable_key = observation.clone();
    unstable_key.observed.cardinalities.insert("Save Requests".to_string(), 1);
    assert_rejects(&unstable_key, "not bounded")?;

    let mut log_dump = observation.clone();
    log_dump.evidence.push(EvidenceRef {
        kind: EvidenceKind::ServerStderr,
        reference: "stderr: [lsp] textDocument/hover my $secret = 1;".to_string(),
    });
    assert_rejects(&log_dump, "bounded digest/path")?;

    let mut bad_backend = observation.clone();
    bad_backend.backend = BackendIdentity::HostAdapter { adapter_digest: "deadbeef".to_string() };
    assert_rejects(&bad_backend, "adapter digest")
}

#[test]
fn unknown_observation_fields_are_rejected_by_the_schema() -> Result<()> {
    let observation = valid_for("neovim.native.read_methods.request_hover")?;
    let json = serde_json::to_string(&observation)?;
    let mut value: serde_json::Value = serde_json::from_str(&json)?;
    value["source_text_snippet"] = serde_json::json!("sub my $x = 1;");
    let tampered = serde_json::to_string(&value)?;
    let parsed: std::result::Result<TypedObservation, _> = serde_json::from_str(&tampered);
    ensure!(parsed.is_err(), "durable observations admitted an unknown (private-data) field");

    // Nested payloads are closed too: an unknown field below the top level
    // (subject, observed effect) must fail deserialization, not ride along.
    let mut nested = serde_json::from_str::<serde_json::Value>(&json)?;
    nested["subject"]["private_log"] = serde_json::json!("[lsp] hover my $secret = 1;");
    let nested = serde_json::to_string(&nested)?;
    let parsed: std::result::Result<TypedObservation, _> = serde_json::from_str(&nested);
    ensure!(parsed.is_err(), "the subject binding admitted an unknown (private-data) field");

    let mut nested = serde_json::from_str::<serde_json::Value>(&json)?;
    nested["observed"]["raw_source"] = serde_json::json!("my $x = 1;");
    let nested = serde_json::to_string(&nested)?;
    let parsed: std::result::Result<TypedObservation, _> = serde_json::from_str(&nested);
    ensure!(parsed.is_err(), "the observed effect admitted an unknown (private-data) field");

    // The internally-tagged route and evidence payloads are closed as well:
    // an extra field inside a tagged arm fails deserialization instead of
    // being silently dropped (verified against the toolchain's serde).
    let mut tagged = serde_json::from_str::<serde_json::Value>(&json)?;
    if let Some(route) = tagged.get_mut("route").and_then(|value| value.as_object_mut()) {
        route.insert("wire_dump".to_string(), serde_json::json!("textDocument/hover ..."));
    }
    let tagged = serde_json::to_string(&tagged)?;
    let parsed: std::result::Result<TypedObservation, _> = serde_json::from_str(&tagged);
    ensure!(parsed.is_err(), "the route payload admitted an unknown (private-data) field");

    let mut evidence = serde_json::from_str::<serde_json::Value>(&json)?;
    if let Some(first) =
        evidence.get_mut("predicate_evidence").and_then(|value| value.as_array_mut())
        && let Some(record) = first.first_mut().and_then(|value| value.as_object_mut())
    {
        record.insert("raw_payload".to_string(), serde_json::json!("my $x = 1;"));
    }
    let evidence = serde_json::to_string(&evidence)?;
    let parsed: std::result::Result<TypedObservation, _> = serde_json::from_str(&evidence);
    ensure!(
        parsed.is_err(),
        "the predicate-evidence payload admitted an unknown (private-data) field"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Review-repair falsifiers (PR #12638 review: subject pin completeness,
// companion identity, closed channels, timeout budget, generation floors,
// bounded collections)
// ---------------------------------------------------------------------------

#[test]
fn unpinned_host_build_and_config_subjects_fail_closed() -> Result<()> {
    // host_version_scope / config_id are load-bearing subject dimensions,
    // not free tokens: an invented build or foreign config cannot observe.
    let mut observation = valid_for("neovim.native.client_attachment.identify_client_and_process")?;
    observation.subject.host_version_scope = "invented_build".to_string();
    assert_rejects(&observation, "pinned neovim host build and canonical config subject")?;

    let mut observation = valid_for("neovim.native.client_attachment.identify_client_and_process")?;
    observation.subject.config_id = "foreign_config".to_string();
    assert_rejects(&observation, "pinned neovim host build and canonical config subject")?;

    // Boundary note (review disposition): root_id and the document path stay
    // per-fixture bounded bindings by design — the fixture/expectation
    // authority (#10903) is not landed and the root_add_remove action
    // exercises a changing root; they are tightened to exact fixture rows by
    // that authority, not pinned here. Their boundedness is still enforced:
    let mut observation = valid_for("neovim.native.client_attachment.identify_client_and_process")?;
    observation.subject.root_id = "root with spaces and capitals".to_string();
    assert_rejects(&observation, "bounded stable token")
}

#[test]
fn companion_control_must_be_the_action_declared_control() -> Result<()> {
    // A control token owned by another action (or invented) cannot satisfy
    // the route even on the companion-class action.
    let mut observation =
        valid_for("neovim.native.text_sync_lifecycle.companion_multichange_control")?;
    observation.route =
        ObservedRoute::CompanionControl { control: "vim.lsp.buf.hover".to_string() };
    assert_rejects(&observation, "does not declare companion control")?;

    let mut observation =
        valid_for("neovim.native.text_sync_lifecycle.companion_multichange_control")?;
    observation.route =
        ObservedRoute::CompanionControl { control: "locally_invented_control".to_string() };
    assert_rejects(&observation, "does not declare companion control")
}

#[test]
fn host_handoff_and_stimulus_channels_are_closed_vocabularies() -> Result<()> {
    let mut observation =
        valid_for("neovim.native.text_sync_lifecycle.post_run_observation_handoff")?;
    observation.route = ObservedRoute::HostHandoff { handoff: "spawn_with_deadline".to_string() };
    assert_rejects(&observation, "closed handoff vocabulary")?;

    // The stimulus channel is class-gated first (no core action is a
    // stimulus; additive families may add them), so a stimulus route on an
    // ordinary action rejects at the class law - and the closed vocabulary
    // the gate protects is pinned here so an additive family cannot invent
    // tokens the fake would happily bind.
    let mut observation =
        valid_for("neovim.native.text_sync_lifecycle.post_run_observation_handoff")?;
    observation.route = ObservedRoute::TestStimulus { stimulus: "deliberate_stimulus".to_string() };
    assert_rejects(&observation, "ordinary action")?;
    ensure!(
        contract::TEST_STIMULUS_TOKENS.contains(&"deliberate_stimulus"),
        "the fake's stimulus token must stay inside the closed vocabulary"
    );
    ensure!(
        !contract::TEST_STIMULUS_TOKENS.contains(&"locally_invented_stimulus"),
        "the closed stimulus vocabulary must not admit invented tokens"
    );
    Ok(())
}

#[test]
fn timeout_beyond_the_declared_budget_fails_closed() -> Result<()> {
    let spec = action("neovim.native.read_methods.request_hover")?;
    let mut world = fake::FakeWorld::settling();
    world.timed_out_predicates = vec![PredicateKind::HoverResultExact];
    world.result = Some(ObservationResult::NotProven);
    let observation = fake::observation_for(spec, 1, &world).map_err(|e| anyhow::anyhow!("{e}"))?;
    // Bounded timeout inside the budget validates.
    contract::validate_observation(&observation)
        .map_err(|error| anyhow::anyhow!("bounded timeout must validate: {error}"))?;

    let mut beyond = observation;
    if let Some(evidence) = beyond.predicate_evidence.first_mut() {
        *evidence = PredicateEvidence::TimedOut {
            kind: PredicateKind::HoverResultExact,
            polls: 999,
            waited_ms: 60_000,
        };
    }
    assert_rejects(&beyond, "beyond the")
}

#[test]
fn stale_predicate_settlement_cannot_prove_a_current_result() -> Result<()> {
    // A predicate settled one document generation before the observation
    // snapshot cannot prove the current-generation result.
    let mut observation = valid_for("neovim.native.read_methods.wait_target_diagnostic_state")?;
    if let Some(PredicateEvidence::Satisfied { settled_generations, .. }) =
        observation.predicate_evidence.first_mut()
    {
        settled_generations.document_generation -= 1;
    }
    assert_rejects(&observation, "stale state cannot prove a current result")?;

    // Settlement at the observation's own generation (the honest default)
    // still validates — the floor is equality, not recency.
    let observation = valid_for("neovim.native.read_methods.wait_target_diagnostic_state")?;
    contract::validate_observation(&observation)
        .map_err(|error| anyhow::anyhow!("current settlement must validate: {error}"))?;
    Ok(())
}

#[test]
fn unbounded_tokens_and_collections_fail_closed() -> Result<()> {
    let observation = valid_for("neovim.native.read_methods.request_hover")?;

    // A megabyte-scale "token" is not bounded evidence.
    let mut long_token = observation.clone();
    long_token.scenario_id = Some(format!("neovim.bdd.{}", "a".repeat(400)));
    assert_rejects(&long_token, "bounded stable token")?;

    // Collection caps: more effect classes than any action may report.
    let mut classes = observation.clone();
    classes.observed.effect_classes = vec![
        EffectClass::BufferState,
        EffectClass::FileState,
        EffectClass::Filetype,
        EffectClass::ClientIdentity,
        EffectClass::DiagnosticState,
        EffectClass::CompletionItems,
        EffectClass::HoverContent,
        EffectClass::NavigationTarget,
        EffectClass::CursorState,
    ];
    assert_rejects(&classes, "the cap is")?;

    // Duplicate evidence references cannot pad durable evidence.
    let mut duplicates = observation.clone();
    let first = duplicates.evidence[0].clone();
    duplicates.evidence.push(first);
    assert_rejects(&duplicates, "duplicate evidence reference")?;

    // Duplicate effect classes are rejected even below the cap.
    let mut duplicate_class = observation.clone();
    let class = duplicate_class.observed.effect_classes[0];
    duplicate_class.observed.effect_classes.push(class);
    assert_rejects(&duplicate_class, "duplicate effect class")
}

#[test]
fn schema_version_drift_fails_closed() -> Result<()> {
    let mut observation = valid_for("neovim.native.read_methods.request_hover")?;
    observation.schema_version = "native_neovim_actions.v0".to_string();
    assert_rejects(&observation, "schema version")
}

// ---------------------------------------------------------------------------
// Routing, surface classification, and shape laws
// ---------------------------------------------------------------------------

#[test]
fn undeclared_api_route_and_native_surface_fail_closed() -> Result<()> {
    let mut observation = valid_for("neovim.native.read_methods.request_hover")?;
    observation.route = ObservedRoute::PublicStableApi { api: "vim.diagnostic.get".to_string() };
    assert_rejects(&observation, "does not declare public api")?;

    let mut observation = valid_for("neovim.native.host_session.open_buffer")?;
    observation.route = ObservedRoute::NativeEditorSurface { surface: ":w".to_string() };
    assert_rejects(&observation, "does not declare native surface")?;

    // Coc plugin APIs fail the spelling grammar outright.
    let mut observation = valid_for("neovim.native.read_methods.request_hover")?;
    observation.route = ObservedRoute::PublicStableApi { api: "coc#request".to_string() };
    assert_rejects(&observation, "outside the grammar")
}

#[test]
fn version_scoped_surfaces_bind_the_exact_scope() -> Result<()> {
    let observation = valid_for("neovim.native.host_session.load_canonical_config")?;
    let validated = contract::validate_observation(&observation)
        .map_err(|error| anyhow::anyhow!("version-scoped positive case failed: {error}"))?;
    ensure!(validated.action_id == "neovim.native.host_session.load_canonical_config");

    let mut wrong_scope = observation.clone();
    wrong_scope.route = ObservedRoute::VersionScopedApi {
        api: "vim.lsp.config".to_string(),
        scope: "neovim-0.9".to_string(),
    };
    assert_rejects(&wrong_scope, "version-scoped")?;

    let mut wrong_class = observation.clone();
    wrong_class.route = ObservedRoute::PublicStableApi { api: "vim.lsp.config".to_string() };
    assert_rejects(&wrong_class, "does not classify")
}

#[test]
fn instrument_hooks_need_their_exact_owner() -> Result<()> {
    let observation = valid_for("neovim.native.text_sync_lifecycle.held_work_barrier")?;
    let validated = contract::validate_observation(&observation)
        .map_err(|error| anyhow::anyhow!("instrument hook positive case failed: {error}"))?;
    // The plane derives from the instrument-only surface even though the
    // action class is observational: how the observation can actually be
    // obtained is load-bearing, not the class alone.
    ensure!(validated.plane == ObservationPlane::Instrument);

    let mut product_plane = observation.clone();
    product_plane.plane = ObservationPlane::Product;
    assert_rejects(&product_plane, "does not match action class")?;

    let mut wrong_owner = observation.clone();
    wrong_owner.route = ObservedRoute::InstrumentHook {
        hook: "vim.lsp.client.request".to_string(),
        owner: "host_leaf_improvisation".to_string(),
    };
    assert_rejects(&wrong_owner, "exact owner")?;

    let mut hook_on_ordinary = valid_for("neovim.native.read_methods.request_hover")?;
    hook_on_ordinary.route = ObservedRoute::InstrumentHook {
        hook: "vim.lsp.log".to_string(),
        owner: "shared_host_execution_10894".to_string(),
    };
    assert_rejects(&hook_on_ordinary, "not classified instrument-only")
}

#[test]
fn effect_classes_must_be_within_the_action_emissions() -> Result<()> {
    let mut observation = valid_for("neovim.native.config_edits.request_document_format")?;
    observation.observed.effect_classes.push(EffectClass::ForeignClientSet);
    assert_rejects(&observation, "outside what")
}

#[test]
fn content_anchors_resolve_or_fail_closed() -> Result<()> {
    let anchors = fake::FakeAnchors::new();
    let resolved = anchors
        .resolve("sub_declaration")
        .map_err(|error| anyhow::anyhow!("known anchor must resolve: {error}"))?;
    ensure!(resolved == AnchorPosition { line: 3, character: 4 });
    ensure!(
        anchors.resolve("invented_anchor").is_err(),
        "an unknown anchor must fail closed before any observation is built"
    );

    let mut observation = valid_for("neovim.native.read_methods.request_hover")?;
    observation.observed.anchor_positions.clear();
    assert_rejects(&observation, "no resolved anchor position")
}

#[test]
fn reporting_plane_is_reserved_for_generic_owners() -> Result<()> {
    let mut observation = valid_for("neovim.native.read_methods.request_hover")?;
    observation.plane = ObservationPlane::Reporting;
    assert_rejects(&observation, "reporting plane")
}

#[test]
fn unsupported_optional_cells_validate_honestly() -> Result<()> {
    let mut observation = valid_for("neovim.native.read_methods.request_optional_cells")?;
    observation.result = ObservationResult::Unsupported;
    observation.limitation_class = Some("cell_not_selected".to_string());
    let validated = contract::validate_observation(&observation)
        .map_err(|error| anyhow::anyhow!("honest unsupported must validate: {error}"))?;
    ensure!(validated.result == ObservationResult::Unsupported);

    let mut observation = valid_for("neovim.native.read_methods.request_optional_cells")?;
    observation.result = ObservationResult::Unsupported;
    observation.limitation_class = None;
    assert_rejects(&observation, "limitation/failure class")
}

#[test]
fn observed_result_cannot_carry_a_limitation_class() -> Result<()> {
    let mut observation = valid_for("neovim.native.read_methods.request_hover")?;
    observation.limitation_class = Some("gratuitous".to_string());
    assert_rejects(&observation, "cannot carry a limitation class")
}

// ---------------------------------------------------------------------------
// Run ordering laws
// ---------------------------------------------------------------------------

#[test]
fn run_ordering_is_strictly_increasing() -> Result<()> {
    let first = action("neovim.native.host_session.open_buffer")?;
    let second = action("neovim.native.read_methods.request_hover")?;
    let a = fake::observation_for(first, 2, &fake::FakeWorld::settling())
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let b = fake::observation_for(second, 1, &fake::FakeWorld::settling())
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    ensure!(
        contract::validate_observation_run(&[a, b]).is_err(),
        "a decreasing sequence must fail closed"
    );

    let first = action("neovim.native.host_session.open_buffer")?;
    let second = action("neovim.native.read_methods.request_hover")?;
    let a = fake::observation_for(first, 1, &fake::FakeWorld::settling())
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let b = fake::observation_for(second, 1, &fake::FakeWorld::settling())
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    ensure!(
        contract::validate_observation_run(&[a, b]).is_err(),
        "a duplicate sequence must fail closed"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Vocabulary-level negative controls (table mutations)
// ---------------------------------------------------------------------------

fn validate_with(
    needle: &str,
    mutation: impl FnOnce(&mut Vec<ActionSpec>) -> Result<()>,
) -> Result<()> {
    let mut table = ACTIONS.to_vec();
    mutation(&mut table)?;
    match contract::validate_table(&table) {
        Ok(_) => bail!("mutated action table was accepted"),
        Err(error) => {
            let reason = error.to_string();
            ensure!(
                reason.contains(needle),
                "wrong table rejection reason: {reason} (wanted something containing {needle})"
            );
            Ok(())
        }
    }
}

// Static mutation payloads: ActionSpec fields are `&'static`, so table
// mutations cite static slices rather than inline temporaries.
static COC_API: &[&str] = &["coc#request"];
static INJECTED_API: &[&str] = &["vim.lsp.rpc; DROP TABLE"];
static BAD_COMMAND: &[&str] = &[":rm -rf /"];
static LONG_KEYS: &[&str] = &["keys <c-n> <cr> much too long for the grammar"];
static UNJUSTIFIED_HOOKS: &[contract::InstrumentHookUse] =
    &[contract::InstrumentHookUse { api: "vim.lsp.log", justification: "", retirement: "" }];
static NO_HOOKS: &[contract::InstrumentHookUse] = &[];
static ZERO_BUDGET: &[contract::predicate::PredicateRequirement] =
    &[contract::predicate::PredicateRequirement {
        kind: PredicateKind::HoverResultExact,
        max_wait_ms: 0,
    }];
static WITH_OBSERVED: &[ObservationResult] =
    &[ObservationResult::Observed, ObservationResult::NotProven];
static OBSERVED_ONLY: &[ObservationResult] = &[ObservationResult::Observed];
static DUPLICATE_INPUTS: &[InputBinding] = &[
    InputBinding { name: "document", kind: InputKind::FixtureDocument },
    InputBinding { name: "document", kind: InputKind::ContentAnchor },
];

#[test]
fn duplicate_and_out_of_namespace_action_ids_fail_closed() -> Result<()> {
    validate_with("duplicate action id", |table| {
        ensure!(table.len() >= 2, "vocabulary too small to duplicate");
        table[1].action_id = table[0].action_id;
        Ok(())
    })?;
    validate_with("outside the", |table| {
        table[0].action_id = "coc.nvim.native.host_session.open_buffer";
        Ok(())
    })?;
    validate_with("does not spell", |table| {
        table[0].action_id = "neovim.native.host_session.open.buffer";
        Ok(())
    })
}

#[test]
fn family_token_mismatch_fails_closed() -> Result<()> {
    validate_with("does not spell", |table| {
        let index = table
            .iter()
            .position(|spec| spec.family == ActionFamily::ReadMethods)
            .context("vocabulary carries no read-methods action")?;
        table[index].action_id = "neovim.native.config_edits.request_hover";
        Ok(())
    })
}

#[test]
fn api_and_native_surface_grammar_escapes_fail_closed() -> Result<()> {
    validate_with("outside the Neovim grammar", |table| {
        table[0].api_uses = COC_API;
        Ok(())
    })?;
    validate_with("outside the Neovim grammar", |table| {
        table[0].api_uses = INJECTED_API;
        Ok(())
    })?;
    validate_with("outside the native editor grammar", |table| {
        table[0].native_surfaces = BAD_COMMAND;
        Ok(())
    })?;
    validate_with("outside the native editor grammar", |table| {
        table[0].native_surfaces = LONG_KEYS;
        Ok(())
    })
}

#[test]
fn instrument_hook_without_justification_fails_closed() -> Result<()> {
    validate_with("without justification and retirement", |table| {
        let index = table
            .iter()
            .position(|spec| !spec.instrument_hooks.is_empty())
            .context("vocabulary carries no instrument hook to mutate")?;
        table[index].instrument_hooks = UNJUSTIFIED_HOOKS;
        Ok(())
    })
}

#[test]
fn surface_classification_laws_fail_closed() -> Result<()> {
    // Version-scoped surface with a non-token scope.
    validate_with("version scope", |table| {
        table[0].surface = SurfaceClassification::PublicVersionScoped { scope: "Neovim 0.11+" };
        Ok(())
    })?;
    // Instrument-only surface without any hook.
    validate_with("cites no instrument hook", |table| {
        table[0].surface =
            SurfaceClassification::InstrumentOnlyHook { owner: "shared_host_execution_10894" };
        table[0].instrument_hooks = NO_HOOKS;
        Ok(())
    })?;
    // Companion surface on a non-companion action.
    validate_with("not a companion-class action", |table| {
        table[0].surface = SurfaceClassification::CompanionProtocolControl;
        Ok(())
    })?;
    // Host handoff that stops failing closed.
    validate_with("fail-closed behind #10894", |table| {
        let index = table
            .iter()
            .position(|spec| spec.class == contract::ActionClass::HostHandoff)
            .context("vocabulary carries no host handoff to mutate")?;
        table[index].surface = SurfaceClassification::PublicStable;
        Ok(())
    })
}

#[test]
fn not_exposed_actions_can_never_admit_observed() -> Result<()> {
    validate_with("can never claim an observed result", |table| {
        let index = table
            .iter()
            .position(|spec| spec.surface == SurfaceClassification::NotExposed)
            .context("vocabulary carries no not-exposed action to mutate")?;
        table[index].allowed_results = WITH_OBSERVED;
        Ok(())
    })
}

#[test]
fn zero_wait_budget_and_missing_not_proven_fail_closed() -> Result<()> {
    validate_with("zero wait budget", |table| {
        let index = table
            .iter()
            .position(|spec| !spec.required_predicates.is_empty())
            .context("vocabulary carries no predicate to mutate")?;
        table[index].required_predicates = ZERO_BUDGET;
        Ok(())
    })?;
    validate_with("must admit not_proven", |table| {
        // Point at a public action: the NotExposed row rejects OBSERVED_ONLY
        // through its own law, which would mask this one.
        let index = table
            .iter()
            .position(|spec| spec.surface == SurfaceClassification::PublicStable)
            .context("vocabulary carries no public-stable action")?;
        table[index].allowed_results = OBSERVED_ONLY;
        Ok(())
    })
}

#[test]
fn duplicate_input_names_fail_closed() -> Result<()> {
    validate_with("duplicate input", |table| {
        table[0].inputs = DUPLICATE_INPUTS;
        Ok(())
    })
}

#[test]
fn additive_family_extension_seam_supports_new_rows() -> Result<()> {
    // The same registration API admits a well-formed additive row: the seam
    // for later families (atomic sync/desync, parser strategies, races,
    // version/platform, first-mile, virtual documents) is one table edit,
    // not a new authority.
    let mut table = ACTIONS.to_vec();
    let base = action("neovim.native.text_sync_lifecycle.ordinary_edit_didchange")?;
    let mut extension = *base;
    extension.action_id = "neovim.native.text_sync_lifecycle.atomic_ranged_change";
    extension.summary = "additive family row: sequential ranged change under the selected envelope";
    table.push(extension);
    let summary = contract::validate_table(&table).map_err(|error| {
        anyhow::anyhow!("additive row must validate through the same API: {error}")
    })?;
    ensure!(summary.action_count == ACTIONS.len() + 1);
    let compiled = contract::validate_compiled_contract()?;
    ensure!(
        summary.vocabulary_digest != compiled.vocabulary_digest,
        "an additive row must change the advertised vocabulary identity"
    );

    // But a locally invented action cannot enter evidence: the observation
    // law only resolves compiled registry IDs.
    let invented = contract::action_by_id("neovim.native.text_sync_lifecycle.atomic_ranged_change");
    ensure!(invented.is_none(), "the compiled registry must not grow at runtime");
    Ok(())
}
