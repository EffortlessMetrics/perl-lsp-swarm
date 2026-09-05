// #11398 server-generation recovery scenario contract tests.
//
// Red-first law: the negative controls below were authored and proven to
// reject before the positive host journey ran. Every discriminating control
// feeds the judgment the exact evidence a false green would need (a healthy
// respawn without initialize/readiness, a clean first launch relabeled
// recovery, an old-generation result republished after the replacement, a
// manual restart relabeled automatic, a stimulus that never landed, a
// shutdown-during-recovery without the pending observation) and asserts the
// slice refuses it. Real-editor launches are not unit tests: the canonical
// journeys run in the dedicated workflow (`.github/workflows/vim-hermetic-host.yml`).

use anyhow::{Result, ensure};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use xtask::editor_client_compat::{
    CANONICAL_EXPECTATION_SET_ID, CleanupResult, EvidenceStage, ObservationResult,
    PlatformIdentity, RegistrationState, WorkspaceFixtureIdentity,
    canonical_expectation_set_digest, fixture_digest,
};
use xtask::vim_host_recovery_run::recovery_journey;
use xtask::vim_host_recovery_run::{
    CELL_CURRENT_RESULT, CELL_DOCUMENT_REPLAY, CELL_EXPLICIT_RESTART,
    CELL_INITIALIZED_NEW_GENERATION, CELL_OLD_GENERATION_REJECTED, CELL_RETRY_OR_MANUAL,
    CELL_SHUTDOWN_CLEANUP, CELL_UNEXPECTED_EXIT, RecoveryFixtureVariant, StimulusRecord,
    evaluate_recovery_observation, extract_recovery_wire, materialize_recovery_fixture,
    stimulus_ledger_is_complete,
};
use xtask::vim_host_run::vim_host_runner::{
    self, DRIVER_SCHEMA_VERSION, DriverEvent, DriverEventKind, RUN_PLAN_SCHEMA_VERSION,
    VimHostPaths, VimHostRunIdentity, VimHostRunPlan, WireEvidence, validate_driver_events,
};

fn valid_digest() -> String {
    format!("sha256:{}", "a".repeat(64))
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(1).unwrap_or(Path::new(".")).to_path_buf()
}

// ---------------------------------------------------------------------------
// Scratch plan and evidence helpers
// ---------------------------------------------------------------------------

fn scratch_recovery_plan(root: &Path) -> Result<VimHostRunPlan> {
    fs::create_dir_all(root)?;
    let vim = root.join("vim.exe");
    let candidate = root.join("perllsp.exe");
    let driver = root.join("driver.vim");
    let adapter = root.join("adapter.vim");
    let checkout = root.join("vim-lsp");
    fs::write(&vim, b"fake vim binary")?;
    fs::write(&candidate, b"fake perllsp binary")?;
    fs::write(&driver, b"\" driver")?;
    fs::write(&adapter, b"\" adapter")?;
    fs::create_dir_all(checkout.join("plugin"))?;
    fs::write(checkout.join("plugin/lsp.vim"), b"\" plugin entry")?;
    fs::create_dir_all(checkout.join(".git"))?;
    fs::write(checkout.join(".git/HEAD"), b"ref: refs/heads/main\n")?;
    let fixture =
        materialize_recovery_fixture(&root.join("fixture"), RecoveryFixtureVariant::Canonical)?;
    Ok(VimHostRunPlan {
        identity: VimHostRunIdentity {
            schema_version: RUN_PLAN_SCHEMA_VERSION.to_string(),
            stage: EvidenceStage::ExactSourceLocal,
            repository: "EffortlessMetrics/perl-lsp-swarm".to_string(),
            candidate_sha: "a".repeat(40),
            vim_version: "VIM - Vi IMproved 9.2".to_string(),
            vim_build_sha256: vim_host_runner::file_sha256(&vim)?,
            vim_feature_digest: vim_host_runner::bytes_sha256(b"features")?,
            vim_lsp_commit: "b".repeat(40),
            vim_lsp_tree_digest: "c".repeat(40),
            vim_lsp_plugin_entry_sha256: vim_host_runner::file_sha256(
                &checkout.join("plugin/lsp.vim"),
            )?,
            driver_sha256: vim_host_runner::file_sha256(&driver)?,
            adapter_sha256: vim_host_runner::file_sha256(&adapter)?,
            configuration_sha256: vim_host_runner::file_sha256(&driver)?,
            activation_root_sha256: vim_host_runner::file_sha256(&adapter)?,
            subject_manifest_sha256: vim_host_runner::file_sha256(&driver)?,
            candidate_version: "perllsp 0.17.0".to_string(),
            candidate_build_revision: "a".repeat(40),
            candidate_artifact_sha256: vim_host_runner::file_sha256(&candidate)?,
            candidate_identity_packet_sha256: vim_host_runner::bytes_sha256(b"packet")?,
            fixture: WorkspaceFixtureIdentity {
                id: "vim_vim_lsp_recovery_generations_v1".to_string(),
                digest: fixture_digest(&fixture.root)?,
                expectation_set_id: CANONICAL_EXPECTATION_SET_ID.to_string(),
                expectation_set_digest: canonical_expectation_set_digest()?,
            },
            journey_selector: "vim_vim_lsp_recovery_generations.v1".to_string(),
            platform: PlatformIdentity {
                os: "linux".to_string(),
                os_version: "test".to_string(),
                arch: "x86_64".to_string(),
            },
            registration_state: RegistrationState::ManualClientRegistration,
            timeout_ms: 240_000,
        },
        paths: VimHostPaths {
            vim_executable: vim,
            vim_lsp_checkout: checkout,
            driver,
            adapter,
            candidate_executable: candidate,
            fixture_root: fixture.root,
            artifact_root: root.join("artifacts"),
        },
    })
}

fn event(sequence: u64, kind: DriverEventKind) -> DriverEvent {
    DriverEvent {
        schema_version: DRIVER_SCHEMA_VERSION.to_string(),
        sequence,
        kind,
        details: BTreeMap::new(),
    }
}

fn detail_event(sequence: u64, kind: DriverEventKind, details: &[(&str, &str)]) -> DriverEvent {
    let mut built = event(sequence, kind);
    for (key, value) in details {
        built.details.insert((*key).to_string(), (*value).to_string());
    }
    built
}

fn current_event(
    sequence: u64,
    index: &str,
    generation: &str,
    errors: &str,
    warnings: &str,
) -> DriverEvent {
    detail_event(
        sequence,
        DriverEventKind::GenerationCurrentObserved,
        &[
            ("generation_index", index),
            ("generation", generation),
            ("state_source", "client_state"),
            ("barrier", "diagnostics_event_and_wire"),
            ("errors", errors),
            ("warnings", warnings),
        ],
    )
}

/// The complete canonical recovery journey event stream (28 barriers), in
/// the exact order the driver emits them.
fn complete_recovery_events(digest: &str) -> Vec<DriverEvent> {
    vec![
        event(1, DriverEventKind::HostStarted),
        event(2, DriverEventKind::ClientLoaded),
        detail_event(
            3,
            DriverEventKind::RegistrationSelected,
            &[("cmd", "perllsp--stdio"), ("candidate_sha256", digest)],
        ),
        detail_event(4, DriverEventKind::FixtureOpened, &[("file", "workspace/project/main.pl")]),
        event(5, DriverEventKind::ServerInitialized),
        detail_event(
            6,
            DriverEventKind::BufferEnabled,
            &[("filetype", "perl"), ("detection", "native_vim")],
        ),
        event(7, DriverEventKind::InitializeObserved),
        detail_event(
            8,
            DriverEventKind::RootSelected,
            &[
                ("root_source", "activation_root_marker"),
                ("root_marker", "cpanfile"),
                ("expected_root", "workspace/project"),
                ("observed_root", "workspace/project"),
                ("decoy_root", "workspace"),
            ],
        ),
        detail_event(9, DriverEventKind::DiagnosticsObserved, &[("mode", "push")]),
        current_event(10, "1", "g1_defect_current", "1", "0"),
        detail_event(
            11,
            DriverEventKind::ServerRestartApplied,
            &[
                ("restart_index", "1"),
                ("route", "public_stop_reopen"),
                ("old_init_generation", "1"),
                ("new_init_generation", "2"),
            ],
        ),
        detail_event(
            12,
            DriverEventKind::GenerationReplayObserved,
            &[
                ("replay_index", "1"),
                ("initialize_generation", "2"),
                ("document", "main.pl"),
                ("root", "workspace/project"),
                ("did_open_replayed", "1"),
                ("client_init_events", "2"),
                ("buffer_enabled_events", "2"),
            ],
        ),
        current_event(13, "2", "g2_recomputed_defect", "1", "0"),
        detail_event(
            14,
            DriverEventKind::RecoveryStimulusApplied,
            &[
                ("stimulus_index", "1"),
                ("stimulus", "terminate_server_process"),
                ("marker", "kill-1.req"),
                ("serving_generation", "2"),
            ],
        ),
        detail_event(
            15,
            DriverEventKind::RecoveryDispositionObserved,
            &[
                ("disposition_index", "1"),
                ("stimulus", "unexpected_exit"),
                ("disposition", "manual_restart_required"),
                ("retry_count", "0"),
                ("window_ms", "5000"),
                ("exit_observed", "1"),
            ],
        ),
        detail_event(
            16,
            DriverEventKind::ServerRestartApplied,
            &[
                ("restart_index", "2"),
                ("route", "manual_reopen_after_exit"),
                ("old_init_generation", "2"),
                ("new_init_generation", "3"),
            ],
        ),
        detail_event(
            17,
            DriverEventKind::GenerationReplayObserved,
            &[
                ("replay_index", "2"),
                ("initialize_generation", "3"),
                ("document", "main.pl"),
                ("root", "workspace/project"),
                ("did_open_replayed", "1"),
                ("client_init_events", "3"),
                ("buffer_enabled_events", "3"),
            ],
        ),
        current_event(18, "3", "g3_manual_recovery_defect", "1", "0"),
        detail_event(
            19,
            DriverEventKind::RecoveryStimulusApplied,
            &[
                ("stimulus_index", "2"),
                ("stimulus", "terminate_server_process"),
                ("marker", "kill-2.req"),
                ("serving_generation", "3"),
            ],
        ),
        detail_event(
            20,
            DriverEventKind::RecoveryDispositionObserved,
            &[
                ("disposition_index", "2"),
                ("stimulus", "unexpected_exit"),
                ("disposition", "manual_restart_required"),
                ("retry_count", "0"),
                ("window_ms", "5000"),
                ("exit_observed", "1"),
            ],
        ),
        detail_event(
            21,
            DriverEventKind::ServerRestartApplied,
            &[
                ("restart_index", "3"),
                ("route", "manual_reopen_after_exit"),
                ("old_init_generation", "3"),
                ("new_init_generation", "4"),
            ],
        ),
        detail_event(
            22,
            DriverEventKind::GenerationReplayObserved,
            &[
                ("replay_index", "3"),
                ("initialize_generation", "4"),
                ("document", "main.pl"),
                ("root", "workspace/project"),
                ("did_open_replayed", "1"),
                ("client_init_events", "4"),
                ("buffer_enabled_events", "4"),
            ],
        ),
        current_event(23, "4", "g4_clean_current", "0", "0"),
        detail_event(
            24,
            DriverEventKind::OldGenerationRejected,
            &[
                ("rejection_index", "1"),
                ("held_generation", "g3_manual_recovery_defect"),
                ("released_after_generation", "g4_clean_current"),
                ("held_result", "defect_error_signature"),
                ("old_signature_settled", "0"),
            ],
        ),
        detail_event(
            25,
            DriverEventKind::RecoveryStimulusApplied,
            &[
                ("stimulus_index", "3"),
                ("stimulus", "terminate_server_process"),
                ("marker", "kill-3.req"),
                ("serving_generation", "4"),
            ],
        ),
        detail_event(
            26,
            DriverEventKind::ShutdownDuringPendingObserved,
            &[
                ("old_generation_dead", "1"),
                ("new_generation_started", "0"),
                ("recovery_route", "pending_manual_reopen"),
            ],
        ),
        detail_event(27, DriverEventKind::ShutdownStarted, &[("server_stopping", "1")]),
        detail_event(28, DriverEventKind::ShutdownCompleted, &[("server_exited", "1")]),
    ]
}

/// The canonical recovery wire: four initialize generations with their
/// initialized notifications, the governed document's didOpen per
/// generation, the registration-scoped config push, and each generation's
/// settled publish (the defect through generation 3, the clean generation
/// after the source replacement).
fn canonical_wire_lines() -> Vec<String> {
    vec![
        // generation 1
        "{\"method\":\"initialize\",\"params\":{\"rootUri\":\"file:///w/workspace/project\"}}".to_string(),
        "{\"method\":\"initialized\",\"params\":{}}".to_string(),
        "{\"method\":\"textDocument/didOpen\",\"params\":{\"textDocument\":{\"uri\":\"file:///w/workspace/project/main.pl\"}}}".to_string(),
        "{\"method\":\"workspace/didChangeConfiguration\",\"params\":{}}".to_string(),
        "{\"method\":\"textDocument/publishDiagnostics\",\"params\":{\"uri\":\"file:///w/workspace/project/main.pl\",\"diagnostics\":[{\"severity\":1}]}}".to_string(),
        "{\"method\":\"textDocument/didClose\",\"params\":{\"textDocument\":{\"uri\":\"file:///w/workspace/project/main.pl\"}}}".to_string(),
        // generation 2 (explicit restart; the close preceded the stop)
        "{\"method\":\"initialize\",\"params\":{\"rootUri\":\"file:///w/workspace/project\"}}".to_string(),
        "{\"method\":\"initialized\",\"params\":{}}".to_string(),
        "{\"method\":\"textDocument/didOpen\",\"params\":{\"textDocument\":{\"uri\":\"file:///w/workspace/project/main.pl\"}}}".to_string(),
        "{\"method\":\"textDocument/publishDiagnostics\",\"params\":{\"uri\":\"file:///w/workspace/project/main.pl\",\"diagnostics\":[{\"severity\":1}]}}".to_string(),
        // generation 3 (manual recovery after the crash stimulus; the dead
        // server received no didClose)
        "{\"method\":\"initialize\",\"params\":{\"rootUri\":\"file:///w/workspace/project\"}}".to_string(),
        "{\"method\":\"initialized\",\"params\":{}}".to_string(),
        "{\"method\":\"textDocument/didOpen\",\"params\":{\"textDocument\":{\"uri\":\"file:///w/workspace/project/main.pl\"}}}".to_string(),
        "{\"method\":\"textDocument/publishDiagnostics\",\"params\":{\"uri\":\"file:///w/workspace/project/main.pl\",\"diagnostics\":[{\"severity\":1}]}}".to_string(),
        // generation 4 (replacement generation over the clean source)
        "{\"method\":\"initialize\",\"params\":{\"rootUri\":\"file:///w/workspace/project\"}}".to_string(),
        "{\"method\":\"initialized\",\"params\":{}}".to_string(),
        "{\"method\":\"textDocument/didOpen\",\"params\":{\"textDocument\":{\"uri\":\"file:///w/workspace/project/main.pl\"}}}".to_string(),
        "{\"method\":\"textDocument/publishDiagnostics\",\"params\":{\"uri\":\"file:///w/workspace/project/main.pl\",\"diagnostics\":[]}}".to_string(),
    ]
}

fn canonical_wire() -> xtask::vim_host_recovery_run::RecoveryWire {
    let text = canonical_wire_lines().join("\n");
    extract_recovery_wire(text.as_bytes())
}

fn landed_stimulus_records() -> Vec<StimulusRecord> {
    (1..=3)
        .map(|index| StimulusRecord {
            marker: format!("kill-{index}.req"),
            pids: vec![1000 + index],
            killed_at: "2026-08-26T00:00:00Z".to_string(),
            outcome: "terminated 1 exact candidate process(es)".to_string(),
        })
        .collect()
}

fn substrate_wire_evidence() -> WireEvidence {
    let request = serde_json::json!({"params": {"rootUri": "file:///w/workspace/project"}});
    WireEvidence {
        saw_initialize: true,
        saw_initialized: true,
        initialize_request: Some(request),
        ..Default::default()
    }
}

fn observation_with(
    status: Option<i32>,
    cleanup: CleanupResult,
    events: Vec<DriverEvent>,
) -> vim_host_runner::ProcessObservation {
    let driver_complete = validate_driver_events(&events, true).is_ok();
    vim_host_runner::ProcessObservation {
        status_code: status,
        timed_out: false,
        kill_requested: false,
        cleanup,
        cleanup_detail: "test".to_string(),
        events,
        driver_complete,
        artifacts: Vec::new(),
    }
}

fn canonical_judgment(
    plan: &VimHostRunPlan,
    events: Vec<DriverEvent>,
    wire: &xtask::vim_host_recovery_run::RecoveryWire,
    records: &[StimulusRecord],
) -> xtask::vim_host_recovery_run::RecoveryJudgment {
    let observation = observation_with(Some(0), CleanupResult::Pass, events);
    evaluate_recovery_observation(
        plan,
        &observation,
        &substrate_wire_evidence(),
        wire,
        records,
        RecoveryFixtureVariant::Canonical,
    )
}

// ---------------------------------------------------------------------------
// Fixture and variant laws
// ---------------------------------------------------------------------------

#[test]
fn canonical_fixture_carries_marker_decoy_and_defect_generation() -> Result<()> {
    let root = repo_root().join("target/test-recovery-contract/fixture-canonical");
    let _ = fs::remove_dir_all(&root);
    let fixture = materialize_recovery_fixture(&root, RecoveryFixtureVariant::Canonical)?;
    ensure!(fixture.root.join("workspace/project/cpanfile").is_file());
    ensure!(fixture.root.join("workspace/project/main.pl").is_file());
    ensure!(fixture.root.join("workspace/main.pl").is_file());
    ensure!(!fixture.root.join("workspace/cpanfile").exists());
    let governed = fs::read_to_string(fixture.root.join("workspace/project/main.pl"))?;
    ensure!(governed.contains("my $value = scheduled_maintenance()\n"));
    ensure!(!governed.contains("scheduled_maintenance();"));
    let decoy = fs::read_to_string(fixture.root.join("workspace/main.pl"))?;
    ensure!(decoy.contains("outer decoy"));
    Ok(())
}

#[test]
fn wrong_root_decoy_moves_the_marker_to_the_outer_root() -> Result<()> {
    let root = repo_root().join("target/test-recovery-contract/fixture-wrong-root");
    let _ = fs::remove_dir_all(&root);
    let fixture = materialize_recovery_fixture(&root, RecoveryFixtureVariant::WrongRootDecoy)?;
    ensure!(fixture.root.join("workspace/cpanfile").is_file());
    ensure!(!fixture.root.join("workspace/project/cpanfile").exists());
    Ok(())
}

#[test]
fn stimulus_channel_stays_inside_the_perllsp_vim_host_namespace() -> Result<()> {
    let env = xtask::vim_host_recovery_run::recovery_env(
        Path::new("/tmp/fixture"),
        Path::new("/tmp/out/stimulus"),
        RecoveryFixtureVariant::Canonical,
    );
    ensure!(!env.is_empty());
    for (key, value) in &env {
        let key = key.to_string_lossy();
        ensure!(key.starts_with("PERLLSP_VIM_HOST_"), "journey extra escaped the channel: {key}");
        ensure!(!value.to_string_lossy().is_empty(), "journey extra {key} is empty");
    }
    ensure!(env.iter().any(|(key, value)| {
        key.to_string_lossy() == "PERLLSP_VIM_HOST_RECOVERY_VARIANT"
            && value.to_string_lossy() == "canonical"
    }));
    ensure!(env.iter().any(|(key, value)| {
        key.to_string_lossy() == "PERLLSP_VIM_HOST_STIMULUS_DIR"
            && value.to_string_lossy() == "/tmp/out/stimulus"
    }));
    Ok(())
}

// ---------------------------------------------------------------------------
// Event-stream laws
// ---------------------------------------------------------------------------

#[test]
fn complete_recovery_event_stream_validates() -> Result<()> {
    let events = complete_recovery_events(&valid_digest());
    validate_driver_events(&events, true)?;
    Ok(())
}

#[test]
fn recovery_event_repetition_laws_reject_disorder_and_forgeries() -> Result<()> {
    let digest = "sha256:aa";
    let reject = |events: Vec<DriverEvent>| {
        ensure!(
            validate_driver_events(&events, false).is_err(),
            "forged recovery stream must be rejected"
        );
        anyhow::Ok(())
    };

    // index gap: the first restart is dropped, so restart_index starts at 2
    let mut events = complete_recovery_events(digest);
    let mut second = events.remove(11);
    let _ = events.remove(10);
    second.sequence = 11;
    events.insert(10, second);
    for (index, item) in events.iter_mut().enumerate() {
        item.sequence = (index + 1) as u64;
    }
    reject(events)?;

    // a private launch route is not a restart route
    let mut events = complete_recovery_events(digest);
    events[10].details.insert("route".to_string(), "raw_process_launch".to_string());
    reject(events)?;

    // a new generation that does not exceed the old one (first launch)
    let mut events = complete_recovery_events(digest);
    events[10].details.insert("old_init_generation".to_string(), "0".to_string());
    reject(events)?;

    // a disposition window below the honest minimum
    let mut events = complete_recovery_events(digest);
    events[14].details.insert("window_ms".to_string(), "10".to_string());
    reject(events)?;

    // an invented disposition token
    let mut events = complete_recovery_events(digest);
    events[14].details.insert("disposition".to_string(), "recovered_automatically".to_string());
    reject(events)?;

    // a non-numeric retry count
    let mut events = complete_recovery_events(digest);
    events[14].details.insert("retry_count".to_string(), "zero".to_string());
    reject(events)?;

    // a stimulus event without its marker identity
    let mut events = complete_recovery_events(digest);
    events[13].details.remove("marker");
    reject(events)?;
    Ok(())
}

#[test]
fn replay_without_readiness_counts_is_rejected() -> Result<()> {
    let mut events = complete_recovery_events(&valid_digest());
    events[11].details.insert("client_init_events".to_string(), "1".to_string());
    ensure!(validate_driver_events(&events, false).is_err());
    Ok(())
}

#[test]
fn an_admitted_old_generation_signature_cannot_even_be_stated() -> Result<()> {
    let mut events = complete_recovery_events(&valid_digest());
    events[23].details.insert("old_signature_settled".to_string(), "1".to_string());
    ensure!(validate_driver_events(&events, false).is_err());
    Ok(())
}

#[test]
fn shutdown_pending_observation_cannot_repeat_or_lie() -> Result<()> {
    let mut events = complete_recovery_events(&valid_digest());
    // a second pending observation
    let mut second = detail_event(
        29,
        DriverEventKind::ShutdownDuringPendingObserved,
        &[
            ("old_generation_dead", "1"),
            ("new_generation_started", "0"),
            ("recovery_route", "pending_manual_reopen"),
        ],
    );
    second.sequence = events.len() as u64 + 1;
    events.push(second);
    ensure!(validate_driver_events(&events, false).is_err());

    // a pending observation that claims the replacement already started
    let mut events = complete_recovery_events(&valid_digest());
    events[25].details.insert("new_generation_started".to_string(), "1".to_string());
    ensure!(validate_driver_events(&events, false).is_err());
    Ok(())
}

#[test]
fn shutdown_pending_must_precede_the_shutdown_barriers() -> Result<()> {
    let mut events = complete_recovery_events(&valid_digest());
    let pending = events.remove(25);
    events.insert(27, pending);
    for (index, item) in events.iter_mut().enumerate() {
        item.sequence = (index + 1) as u64;
    }
    ensure!(validate_driver_events(&events, false).is_err());
    Ok(())
}

// ---------------------------------------------------------------------------
// Stimulus matcher laws
// ---------------------------------------------------------------------------

#[test]
fn the_stimulus_matcher_binds_only_the_serving_server_process() -> Result<()> {
    use xtask::vim_host_recovery_run::unix_args_match_serving_server;
    let needle = "/ws/target/debug/perllsp".to_string();
    // The serving server: argv[0] is the exact candidate, stdio transport.
    ensure!(
        unix_args_match_serving_server("/ws/target/debug/perllsp --stdio", &needle),
        "the serving server must match"
    );
    // The supervising cargo run carries the same path as the --candidate
    // argument: a substring match would kill the supervisor (bot P1,
    // review #12831).
    ensure!(
        !unix_args_match_serving_server(
            "cargo run --quiet --locked -p xtask -- editor-compat vim run --candidate              /ws/target/debug/perllsp --out /ws/out",
            &needle
        ),
        "the supervising cargo command line must not match"
    );
    // The xtask harness itself carries the path too.
    ensure!(
        !unix_args_match_serving_server(
            "/ws/target/debug/xtask editor-compat vim run --candidate /ws/target/debug/perllsp",
            &needle
        ),
        "the harness command line must not match"
    );
    // Another checkout's candidate at a different path never matches.
    ensure!(
        !unix_args_match_serving_server("/other/checkout/target/debug/perllsp --stdio", &needle),
        "another checkout's server must not match"
    );
    // The exact candidate without the stdio transport is not the serving
    // registration command.
    ensure!(
        !unix_args_match_serving_server("/ws/target/debug/perllsp --tcp", &needle),
        "a non-stdio launch of the candidate must not match"
    );
    Ok(())
}

#[test]
fn the_stimulus_matcher_handles_quoted_windows_paths_and_exe_suffixes() -> Result<()> {
    use xtask::vim_host_recovery_run::unix_args_match_serving_server;

    let needle = "c:/work tree/target/debug/perllsp";
    ensure!(unix_args_match_serving_server(
        r#""C:\work tree\target\debug\perllsp.exe" --stdio"#,
        needle,
    ));
    ensure!(
        unix_args_match_serving_server(r#"C:\work tree\target\debug\perllsp --stdio"#, needle,)
    );
    ensure!(!unix_args_match_serving_server(
        r#""C:\work tree\target\debug\perllsp.exe" --tcp"#,
        needle,
    ));
    ensure!(!unix_args_match_serving_server(
        r#"C:\other tree\target\debug\perllsp.exe --stdio"#,
        needle,
    ));
    // Rejoining the leading tokens is what lets an unquoted spaced path
    // match, so the anchor has to be proven to survive it: a supervisor
    // that carries the same spaced candidate path as a mid-line argument
    // must still not match, quoted or not. Without the start anchor this
    // widening would kill the supervising process (bot P1, review #12831).
    ensure!(!unix_args_match_serving_server(
        r#"cargo run -p xtask -- editor-compat vim run --candidate "C:\work tree\target\debug\perllsp.exe" --stdio"#,
        needle,
    ));
    ensure!(!unix_args_match_serving_server(
        r#"C:\tools\xtask.exe --candidate C:\work tree\target\debug\perllsp --stdio"#,
        needle,
    ));
    Ok(())
}

// ---------------------------------------------------------------------------
// Wire mining laws
// ---------------------------------------------------------------------------

#[test]
fn canonical_wire_mines_four_generations_and_windows() -> Result<()> {
    let wire = canonical_wire();
    ensure!(wire.initialize_lines == vec![0, 6, 10, 14], "recovery contract equality failed");
    ensure!(wire.initialized_lines.len() == 4, "recovery contract equality failed");
    ensure!(wire.opens_of("main.pl") == vec![2, 8, 12, 16], "recovery contract equality failed");
    ensure!(wire.closes_of("main.pl") == vec![5], "recovery contract equality failed");
    ensure!(wire.did_change_configuration_lines == vec![3], "recovery contract equality failed");
    // each generation window settles exactly as authored
    let batches = wire.batches_of("main.pl");
    ensure!(batches.len() == 4, "recovery contract equality failed");
    ensure!(batches[0].error_severity_count == 1, "recovery contract equality failed");
    ensure!(batches[3].error_severity_count == 0, "recovery contract equality failed");
    // no old-signature publish after the replacement generation's initialize
    let replacement_initialize = wire
        .initialize_line_of(4)
        .ok_or_else(|| anyhow::anyhow!("synthetic wire lost the replacement initialize"))?;
    let after = wire.batches_after("main.pl", replacement_initialize);
    ensure!(after.iter().all(|batch| batch.error_severity_count == 0));
    Ok(())
}

#[test]
fn initialize_generation_counting_ignores_response_echoes() -> Result<()> {
    // Response envelopes embed the original request: a naive count would
    // double every initialize.
    let log = concat!(
        "[\"--->\", 1, \"perllsp\", {\"method\":\"initialize\",\"params\":{}}]\n",
        "[\"<---\", 1, \"perllsp\", {\"request\":{\"method\":\"initialize\"},\"response\":{}}]\n",
        "[\"--->\", 1, \"perllsp\", {\"method\":\"initialized\",\"params\":{}}]\n",
        "[\"--->\", 2, \"perllsp\", {\"method\":\"initialize\",\"params\":{}}]\n",
        "[\"<---\", 2, \"perllsp\", {\"request\":{\"method\":\"initialize\"},\"response\":{}}]\n"
    );
    let wire = extract_recovery_wire(log.as_bytes());
    ensure!(wire.initialize_lines == vec![0, 3], "recovery contract equality failed");
    ensure!(wire.initialized_lines == vec![2], "recovery contract equality failed");
    Ok(())
}

// ---------------------------------------------------------------------------
// Judgment laws
// ---------------------------------------------------------------------------

#[test]
fn canonical_evidence_passes_affirming_cells_with_partial_adverse_exit() -> Result<()> {
    let root = repo_root().join("target/test-recovery-contract/judgment-canonical");
    let _ = fs::remove_dir_all(&root);
    let plan = scratch_recovery_plan(&root)?;
    let events = complete_recovery_events(&plan.identity.candidate_artifact_sha256);
    let wire = canonical_wire();
    let judgment = canonical_judgment(&plan, events, &wire, &landed_stimulus_records());
    ensure!(
        judgment.cells.get(CELL_EXPLICIT_RESTART) == Some(&ObservationResult::Pass),
        "recovery contract equality failed"
    );
    ensure!(
        judgment.cells.get(CELL_INITIALIZED_NEW_GENERATION) == Some(&ObservationResult::Pass),
        "recovery contract equality failed"
    );
    ensure!(
        judgment.cells.get(CELL_DOCUMENT_REPLAY) == Some(&ObservationResult::Pass),
        "recovery contract equality failed"
    );
    ensure!(
        judgment.cells.get(CELL_CURRENT_RESULT) == Some(&ObservationResult::Pass),
        "recovery contract equality failed"
    );
    ensure!(
        judgment.cells.get(CELL_OLD_GENERATION_REJECTED) == Some(&ObservationResult::Pass),
        "recovery contract equality failed"
    );
    ensure!(
        judgment.cells.get(CELL_RETRY_OR_MANUAL) == Some(&ObservationResult::Pass),
        "recovery contract equality failed"
    );
    ensure!(
        judgment.cells.get(CELL_SHUTDOWN_CLEANUP) == Some(&ObservationResult::Pass),
        "recovery contract equality failed"
    );
    // The adverse-exit cell never passes (#11386 family law).
    ensure!(
        judgment.cells.get(CELL_UNEXPECTED_EXIT) == Some(&ObservationResult::Partial),
        "recovery contract equality failed"
    );
    // The honest top-line is partial, never a forced pass.
    ensure!(judgment.result == ObservationResult::Partial, "recovery contract equality failed");
    ensure!(judgment.failure_class.is_none());
    Ok(())
}

#[test]
fn a_respawn_without_initialize_readiness_cannot_pass() -> Result<()> {
    let root = repo_root().join("target/test-recovery-contract/judgment-respawn");
    let _ = fs::remove_dir_all(&root);
    let plan = scratch_recovery_plan(&root)?;
    let events = complete_recovery_events(&plan.identity.candidate_artifact_sha256);
    // The wire never carried the replacement generations' initialize: a new
    // PID and a restart event alone are not initialize/readiness.
    let log = concat!(
        "{\"method\":\"initialize\",\"params\":{\"rootUri\":\"file:///w/workspace/project\"}}\n",
        "{\"method\":\"initialized\",\"params\":{}}\n",
        "{\"method\":\"textDocument/didOpen\",\"params\":{\"textDocument\":{\"uri\":\"file:///w/workspace/project/main.pl\"}}}\n",
        "{\"method\":\"textDocument/publishDiagnostics\",\"params\":{\"uri\":\"file:///w/workspace/project/main.pl\",\"diagnostics\":[{\"severity\":1}]}}\n"
    );
    let wire = extract_recovery_wire(log.as_bytes());
    let judgment = canonical_judgment(&plan, events, &wire, &landed_stimulus_records());
    ensure!(
        judgment.cells.get(CELL_EXPLICIT_RESTART) == Some(&ObservationResult::Fail),
        "recovery contract equality failed"
    );
    ensure!(
        judgment.cells.get(CELL_INITIALIZED_NEW_GENERATION) == Some(&ObservationResult::NotProven),
        "recovery contract equality failed"
    );
    ensure!(
        judgment.cells.get(CELL_UNEXPECTED_EXIT) == Some(&ObservationResult::Fail),
        "recovery contract equality failed"
    );
    ensure!(judgment.result == ObservationResult::Fail, "recovery contract equality failed");
    Ok(())
}

#[test]
fn a_clean_first_launch_relabeled_recovery_is_rejected() -> Result<()> {
    let root = repo_root().join("target/test-recovery-contract/judgment-first-launch");
    let _ = fs::remove_dir_all(&root);
    let plan = scratch_recovery_plan(&root)?;
    // A stream with no restarts, stimuli, dispositions, replays, or
    // rejection: only the first attach exists. No recovery cell may pass.
    let digest = plan.identity.candidate_artifact_sha256.clone();
    let events: Vec<DriverEvent> = vec![
        event(1, DriverEventKind::HostStarted),
        event(2, DriverEventKind::ClientLoaded),
        detail_event(
            3,
            DriverEventKind::RegistrationSelected,
            &[("cmd", "perllsp--stdio"), ("candidate_sha256", digest.as_str())],
        ),
        detail_event(4, DriverEventKind::FixtureOpened, &[("file", "workspace/project/main.pl")]),
        event(5, DriverEventKind::ServerInitialized),
        detail_event(
            6,
            DriverEventKind::BufferEnabled,
            &[("filetype", "perl"), ("detection", "native_vim")],
        ),
        event(7, DriverEventKind::InitializeObserved),
        detail_event(
            8,
            DriverEventKind::RootSelected,
            &[
                ("root_source", "activation_root_marker"),
                ("root_marker", "cpanfile"),
                ("expected_root", "workspace/project"),
                ("observed_root", "workspace/project"),
                ("decoy_root", "workspace"),
            ],
        ),
        detail_event(9, DriverEventKind::DiagnosticsObserved, &[("mode", "push")]),
        current_event(10, "1", "g1_defect_current", "1", "0"),
        detail_event(11, DriverEventKind::ShutdownStarted, &[("server_stopping", "1")]),
        detail_event(12, DriverEventKind::ShutdownCompleted, &[("server_exited", "1")]),
    ];
    let log = concat!(
        "{\"method\":\"initialize\",\"params\":{\"rootUri\":\"file:///w/workspace/project\"}}\n",
        "{\"method\":\"initialized\",\"params\":{}}\n",
        "{\"method\":\"textDocument/didOpen\",\"params\":{\"textDocument\":{\"uri\":\"file:///w/workspace/project/main.pl\"}}}\n",
        "{\"method\":\"textDocument/publishDiagnostics\",\"params\":{\"uri\":\"file:///w/workspace/project/main.pl\",\"diagnostics\":[{\"severity\":1}]}}\n"
    );
    let wire = extract_recovery_wire(log.as_bytes());
    let judgment = canonical_judgment(&plan, events, &wire, &[]);
    ensure!(
        judgment.cells.get(CELL_EXPLICIT_RESTART) == Some(&ObservationResult::NotProven),
        "recovery contract equality failed"
    );
    ensure!(
        judgment.cells.get(CELL_UNEXPECTED_EXIT) == Some(&ObservationResult::NotProven),
        "recovery contract equality failed"
    );
    ensure!(
        judgment.cells.get(CELL_RETRY_OR_MANUAL) == Some(&ObservationResult::NotProven),
        "recovery contract equality failed"
    );
    ensure!(judgment.result == ObservationResult::NotProven, "recovery contract equality failed");
    Ok(())
}

#[test]
fn an_old_generation_publish_after_replacement_fails_rejection_and_current() -> Result<()> {
    let root = repo_root().join("target/test-recovery-contract/judgment-late-old-result");
    let _ = fs::remove_dir_all(&root);
    let plan = scratch_recovery_plan(&root)?;
    let events = complete_recovery_events(&plan.identity.candidate_artifact_sha256);
    // The held old result republishes after the replacement generation's
    // initialize: a later clean batch cannot hide it.
    let mut lines = canonical_wire_lines();
    lines.push(
        "{\"method\":\"textDocument/publishDiagnostics\",\"params\":{\"uri\":\"file:///w/workspace/project/main.pl\",\"diagnostics\":[{\"severity\":1}]}}"
            .to_string(),
    );
    let wire = extract_recovery_wire(lines.join("\n").as_bytes());
    let judgment = canonical_judgment(&plan, events, &wire, &landed_stimulus_records());
    ensure!(
        judgment.cells.get(CELL_OLD_GENERATION_REJECTED) == Some(&ObservationResult::Fail),
        "recovery contract equality failed"
    );
    ensure!(
        judgment.cells.get(CELL_CURRENT_RESULT) == Some(&ObservationResult::Fail),
        "recovery contract equality failed"
    );
    ensure!(judgment.result == ObservationResult::Fail, "recovery contract equality failed");
    Ok(())
}

#[test]
fn a_stimulus_that_never_landed_fails_the_recovery_cells() -> Result<()> {
    let root = repo_root().join("target/test-recovery-contract/judgment-missed-stimulus");
    let _ = fs::remove_dir_all(&root);
    let plan = scratch_recovery_plan(&root)?;
    let events = complete_recovery_events(&plan.identity.candidate_artifact_sha256);
    let wire = canonical_wire();
    // The watcher killed only two of the three stimuli.
    let mut records = landed_stimulus_records();
    records.truncate(2);
    let judgment = canonical_judgment(&plan, events, &wire, &records);
    ensure!(
        judgment.cells.get(CELL_UNEXPECTED_EXIT) == Some(&ObservationResult::Fail),
        "recovery contract equality failed"
    );
    ensure!(
        judgment.cells.get(CELL_RETRY_OR_MANUAL) == Some(&ObservationResult::Fail),
        "recovery contract equality failed"
    );
    ensure!(
        judgment.cells.get(CELL_SHUTDOWN_CLEANUP) == Some(&ObservationResult::Fail),
        "recovery contract equality failed"
    );
    ensure!(judgment.result == ObservationResult::Fail, "recovery contract equality failed");
    Ok(())
}

#[test]
fn stimulus_ledger_law_requires_a_landed_kill_per_marker() -> Result<()> {
    let events = complete_recovery_events(&valid_digest());
    ensure!(stimulus_ledger_is_complete(&events, &landed_stimulus_records())?);
    // a missing record
    ensure!(!stimulus_ledger_is_complete(&events, &[])?);
    // a record whose kill found no process
    let hollow = vec![StimulusRecord {
        marker: "kill-1.req".to_string(),
        pids: Vec::new(),
        killed_at: "2026-08-26T00:00:00Z".to_string(),
        outcome: "no exact candidate process found".to_string(),
    }];
    ensure!(!stimulus_ledger_is_complete(&events, &hollow)?);
    Ok(())
}

#[test]
fn a_manual_restart_relabeled_automatic_recovery_is_an_oracle_violation() -> Result<()> {
    let root = repo_root().join("target/test-recovery-contract/judgment-auto-claim");
    let _ = fs::remove_dir_all(&root);
    let plan = scratch_recovery_plan(&root)?;
    let events = complete_recovery_events(&plan.identity.candidate_artifact_sha256);
    let wire = canonical_wire();
    let observation = observation_with(Some(0), CleanupResult::Pass, events);
    // The auto_recovery_claimed control reaches the full canonical evidence:
    // the journey must refuse to call it a green outcome.
    let judgment = evaluate_recovery_observation(
        &plan,
        &observation,
        &substrate_wire_evidence(),
        &wire,
        &landed_stimulus_records(),
        RecoveryFixtureVariant::AutoRecoveryClaimed,
    );
    ensure!(judgment.result == ObservationResult::Fail, "recovery contract equality failed");
    Ok(())
}

#[test]
fn a_wrong_root_fails_the_journey_typed() -> Result<()> {
    let root = repo_root().join("target/test-recovery-contract/judgment-wrong-root");
    let _ = fs::remove_dir_all(&root);
    let plan = scratch_recovery_plan(&root)?;
    let mut events = complete_recovery_events(&plan.identity.candidate_artifact_sha256);
    // The root observation reports the decoy root and the driver fails
    // typed before any recovery phase runs.
    events.clear();
    let digest = plan.identity.candidate_artifact_sha256.clone();
    let failed: Vec<DriverEvent> = vec![
        event(1, DriverEventKind::HostStarted),
        event(2, DriverEventKind::ClientLoaded),
        detail_event(
            3,
            DriverEventKind::RegistrationSelected,
            &[("cmd", "perllsp--stdio"), ("candidate_sha256", digest.as_str())],
        ),
        detail_event(4, DriverEventKind::FixtureOpened, &[("file", "workspace/project/main.pl")]),
        event(5, DriverEventKind::ServerInitialized),
        detail_event(
            6,
            DriverEventKind::BufferEnabled,
            &[("filetype", "perl"), ("detection", "native_vim")],
        ),
        event(7, DriverEventKind::InitializeObserved),
        detail_event(
            8,
            DriverEventKind::RootSelected,
            &[
                ("root_source", "activation_root_marker"),
                ("root_marker", "cpanfile"),
                ("expected_root", "workspace/project"),
                ("observed_root", "workspace"),
                ("decoy_root", "workspace"),
            ],
        ),
        detail_event(
            9,
            DriverEventKind::DiagnosticsObserved,
            &[("mode", "push"), ("skipped", "root_mismatch")],
        ),
        detail_event(10, DriverEventKind::DriverFailed, &[("reason", "root_mismatch")]),
        detail_event(11, DriverEventKind::ShutdownStarted, &[("server_stopping", "1")]),
        detail_event(12, DriverEventKind::ShutdownCompleted, &[("server_exited", "1")]),
    ];
    events.extend(failed);
    let observation = observation_with(Some(2), CleanupResult::NotProven, events);
    let judgment = evaluate_recovery_observation(
        &plan,
        &observation,
        &substrate_wire_evidence(),
        &canonical_wire(),
        &[],
        RecoveryFixtureVariant::WrongRootDecoy,
    );
    ensure!(
        judgment.driver_failure_reason.as_deref() == Some("root_mismatch"),
        "recovery contract equality failed"
    );
    ensure!(judgment.result == ObservationResult::Fail, "recovery contract equality failed");
    Ok(())
}

#[test]
fn a_replay_skipped_claim_fails_the_journey_typed() -> Result<()> {
    let root = repo_root().join("target/test-recovery-contract/judgment-replay-skipped");
    let _ = fs::remove_dir_all(&root);
    let plan = scratch_recovery_plan(&root)?;
    let digest = plan.identity.candidate_artifact_sha256.clone();
    let events: Vec<DriverEvent> = vec![
        event(1, DriverEventKind::HostStarted),
        event(2, DriverEventKind::ClientLoaded),
        detail_event(
            3,
            DriverEventKind::RegistrationSelected,
            &[("cmd", "perllsp--stdio"), ("candidate_sha256", digest.as_str())],
        ),
        detail_event(4, DriverEventKind::FixtureOpened, &[("file", "workspace/project/main.pl")]),
        event(5, DriverEventKind::ServerInitialized),
        detail_event(
            6,
            DriverEventKind::BufferEnabled,
            &[("filetype", "perl"), ("detection", "native_vim")],
        ),
        event(7, DriverEventKind::InitializeObserved),
        detail_event(
            8,
            DriverEventKind::RootSelected,
            &[
                ("root_source", "activation_root_marker"),
                ("root_marker", "cpanfile"),
                ("expected_root", "workspace/project"),
                ("observed_root", "workspace/project"),
                ("decoy_root", "workspace"),
            ],
        ),
        detail_event(9, DriverEventKind::DiagnosticsObserved, &[("mode", "push")]),
        current_event(10, "1", "g1_defect_current", "1", "0"),
        detail_event(
            11,
            DriverEventKind::ServerRestartApplied,
            &[
                ("restart_index", "1"),
                ("route", "public_stop_reopen"),
                ("old_init_generation", "1"),
                ("new_init_generation", "2"),
            ],
        ),
        detail_event(12, DriverEventKind::DriverFailed, &[("reason", "document_replay_absent")]),
        detail_event(13, DriverEventKind::ShutdownStarted, &[("server_stopping", "1")]),
        detail_event(14, DriverEventKind::ShutdownCompleted, &[("server_exited", "1")]),
    ];
    let observation = observation_with(Some(2), CleanupResult::NotProven, events);
    let judgment = evaluate_recovery_observation(
        &plan,
        &observation,
        &substrate_wire_evidence(),
        &canonical_wire(),
        &[],
        RecoveryFixtureVariant::ReplaySkippedClaimed,
    );
    ensure!(
        judgment.driver_failure_reason.as_deref() == Some("document_replay_absent"),
        "recovery contract equality failed"
    );
    ensure!(judgment.result == ObservationResult::Fail, "recovery contract equality failed");
    Ok(())
}

#[test]
fn a_leaked_process_cannot_keep_the_shutdown_cell() -> Result<()> {
    let root = repo_root().join("target/test-recovery-contract/judgment-leak");
    let _ = fs::remove_dir_all(&root);
    let plan = scratch_recovery_plan(&root)?;
    let events = complete_recovery_events(&plan.identity.candidate_artifact_sha256);
    let observation = observation_with(Some(0), CleanupResult::Fail, events);
    let judgment = evaluate_recovery_observation(
        &plan,
        &observation,
        &substrate_wire_evidence(),
        &canonical_wire(),
        &landed_stimulus_records(),
        RecoveryFixtureVariant::Canonical,
    );
    ensure!(
        judgment.cells.get(CELL_SHUTDOWN_CLEANUP) == Some(&ObservationResult::Fail),
        "recovery contract equality failed"
    );
    ensure!(judgment.result == ObservationResult::Fail, "recovery contract equality failed");
    Ok(())
}

// ---------------------------------------------------------------------------
// Receipt journey laws
// ---------------------------------------------------------------------------

#[test]
fn receipt_journey_cites_the_recovery_catalog_cells_with_honest_results() -> Result<()> {
    let root = repo_root().join("target/test-recovery-contract/journey");
    let _ = fs::remove_dir_all(&root);
    let plan = scratch_recovery_plan(&root)?;
    let events = complete_recovery_events(&plan.identity.candidate_artifact_sha256);
    let observation = observation_with(Some(0), CleanupResult::Pass, events);
    let judgment = canonical_judgment_with_plan(&plan, &observation);
    let journey = recovery_journey(&observation, &judgment, &substrate_wire_evidence());
    for cell in [
        CELL_EXPLICIT_RESTART,
        CELL_UNEXPECTED_EXIT,
        CELL_INITIALIZED_NEW_GENERATION,
        CELL_DOCUMENT_REPLAY,
        CELL_CURRENT_RESULT,
        CELL_OLD_GENERATION_REJECTED,
        CELL_RETRY_OR_MANUAL,
        CELL_SHUTDOWN_CLEANUP,
    ] {
        let found = journey.iter().find(|item| item.id == cell);
        ensure!(
            found.is_some(),
            "journey omitted catalog cell {cell}; journey carries {:?}",
            journey.iter().map(|item| item.id.as_str()).collect::<Vec<_>>()
        );
        let found = match found {
            Some(item) => item,
            None => return Err(anyhow::anyhow!("unreachable: ensured above")),
        };
        ensure!(found.observed, "catalog cell {cell} claims no observation");
        ensure!(
            found.limitation.is_some(),
            "catalog cell {cell} must carry its route/disposition limitation"
        );
        if cell == CELL_UNEXPECTED_EXIT {
            ensure!(
                found.result == ObservationResult::Partial,
                "recovery contract equality failed"
            );
        } else {
            ensure!(found.result == ObservationResult::Pass, "recovery contract equality failed");
        }
    }
    // No sibling-family cell may appear in this journey's semantic surface.
    for item in &journey {
        ensure!(
            !item.id.starts_with("vim.vim_lsp.freshness."),
            "recovery journey cited a freshness cell: {}",
            item.id
        );
        ensure!(
            !item.id.starts_with("vim.vim_lsp.save."),
            "recovery journey cited a save cell: {}",
            item.id
        );
    }
    Ok(())
}

fn canonical_judgment_with_plan(
    plan: &VimHostRunPlan,
    observation: &vim_host_runner::ProcessObservation,
) -> xtask::vim_host_recovery_run::RecoveryJudgment {
    evaluate_recovery_observation(
        plan,
        observation,
        &substrate_wire_evidence(),
        &canonical_wire(),
        &landed_stimulus_records(),
        RecoveryFixtureVariant::Canonical,
    )
}
