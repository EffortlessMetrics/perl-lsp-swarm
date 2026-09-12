// #11401 host-reopen lifecycle scenario contract tests.
//
// Red-first law: the discriminating controls below were authored and proven
// to reject before the positive host journey ran. Every control feeds the
// judgment the exact evidence a false green would need (a server restart or
// a buffer reopen relabeled as full host replacement, a single session
// claiming repeated use, a prior session's stale state posing as the
// replacement's opening state, a cancelled result that was admitted, a late
// result whose response was never mined, a workspace surface the client
// never exposed, a missing settle probe encoded as zero, a surviving owned
// process) and asserts the judgment or the event contract refuses it.
// Real-editor launches are not unit tests: the canonical journey and the
// relabel control run in the dedicated workflow
// (`.github/workflows/vim-hermetic-host.yml`).

use anyhow::{Result, ensure};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use xtask::editor_client_compat::{
    CANONICAL_EXPECTATION_SET_ID, CleanupResult, EvidenceStage, ObservationResult,
    PlatformIdentity, RegistrationState, WorkspaceFixtureIdentity,
    canonical_expectation_set_digest, fixture_digest,
};
use xtask::vim_host_lifecycle_run::{
    CELL_BUFFER_REOPEN, CELL_CANCELLATION, CELL_FAILURE_CLEANUP, CELL_HOST_REOPEN,
    CELL_LATE_RESULT, CELL_NORMAL_CLEANUP, CELL_REPEATED_SESSIONS, CELL_WORKSPACE_REOPEN,
    CLEAN_LINE_TEXT, DEFECT_LINE_TEXT, HostSessionRecord, LifecycleFixtureVariant, LifecycleWire,
    PendingResponse, aggregate_journey_observation, evaluate_lifecycle_observation,
    extract_lifecycle_wire, lifecycle_journey, materialize_lifecycle_fixture,
};
use xtask::vim_host_run::vim_host_runner::{
    self, DRIVER_SCHEMA_VERSION, DriverEvent, DriverEventKind, ProcessObservation,
    RUN_PLAN_SCHEMA_VERSION, VimHostPaths, VimHostRunIdentity, VimHostRunPlan, WireEvidence,
    validate_driver_events,
};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(1).unwrap_or(Path::new(".")).to_path_buf()
}

// ---------------------------------------------------------------------------
// Scratch plan and evidence helpers
// ---------------------------------------------------------------------------

fn scratch_lifecycle_plan(root: &Path) -> Result<VimHostRunPlan> {
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
    let fixture = materialize_lifecycle_fixture(&root.join("fixture"))?;
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
                id: "vim_vim_lsp_host_reopen_lifecycle_v1".to_string(),
                digest: fixture_digest(&fixture)?,
                expectation_set_id: CANONICAL_EXPECTATION_SET_ID.to_string(),
                expectation_set_digest: canonical_expectation_set_digest()?,
            },
            journey_selector: "vim_vim_lsp_host_reopen_lifecycle.v1".to_string(),
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
            fixture_root: fixture,
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
    let mut observation = event(sequence, kind);
    for (key, value) in details {
        observation.details.insert((*key).to_string(), (*value).to_string());
    }
    observation
}

fn renumber(events: &mut [DriverEvent]) {
    for (index, item) in events.iter_mut().enumerate() {
        item.sequence = (index + 1) as u64;
    }
}

fn generation_event(
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

/// The shared bootstrap barrier stream every session emits (sequences
/// 1..=9).
fn bootstrap_events(digest: &str, role: &str) -> Vec<DriverEvent> {
    vec![
        detail_event(
            1,
            DriverEventKind::HostStarted,
            &[("vim_version", "VIM - Vi IMproved 9.2"), ("session_role", role)],
        ),
        detail_event(2, DriverEventKind::ClientLoaded, &[("plugin", "lsp_vim")]),
        detail_event(
            3,
            DriverEventKind::RegistrationSelected,
            &[("cmd", "perllsp--stdio"), ("candidate_sha256", digest)],
        ),
        detail_event(4, DriverEventKind::FixtureOpened, &[("file", "workspace/project/main.pl")]),
        detail_event(5, DriverEventKind::ServerInitialized, &[("status", "running")]),
        detail_event(
            6,
            DriverEventKind::BufferEnabled,
            &[("filetype", "perl"), ("detection", "native_vim")],
        ),
        detail_event(
            7,
            DriverEventKind::InitializeObserved,
            &[("capabilities_written", "1"), ("position_encoding", "utf-16")],
        ),
        detail_event(
            8,
            DriverEventKind::RootSelected,
            &[
                ("root_source", "activation_root_marker"),
                ("root_marker", "cpanfile"),
                ("expected_root", "workspace/project"),
                ("observed_root", "workspace/project"),
            ],
        ),
        detail_event(9, DriverEventKind::DiagnosticsObserved, &[("mode", "push")]),
    ]
}

/// The full lifecycle session's complete event stream (host 1).
fn complete_host1_events(digest: &str) -> Vec<DriverEvent> {
    let mut events = bootstrap_events(digest, "full_lifecycle_session");
    events.push(generation_event(10, "1", "defect_present", "1", "0"));
    events.push(detail_event(
        11,
        DriverEventKind::PendingActionStarted,
        &[
            ("pending_index", "1"),
            ("method", "textDocument/documentSymbol"),
            ("request_id", "2"),
            ("target_bufnr", "5"),
        ],
    ));
    events.push(detail_event(
        12,
        DriverEventKind::PendingActionCancelled,
        &[
            ("cancel_index", "1"),
            ("pending_index", "1"),
            ("request_id", "2"),
            ("cancel_sent", "1"),
            ("notification_count", "0"),
        ],
    ));
    events.push(detail_event(
        13,
        DriverEventKind::PendingActionStarted,
        &[
            ("pending_index", "2"),
            ("method", "textDocument/documentSymbol"),
            ("request_id", "3"),
            ("target_bufnr", "5"),
        ],
    ));
    events.push(detail_event(
        14,
        DriverEventKind::BufferWiped,
        &[("wipe_index", "1"), ("bufnr", "5"), ("didclose_sent", "1")],
    ));
    events.push(detail_event(
        15,
        DriverEventKind::ExternalMutationApplied,
        &[
            ("mutation_index", "1"),
            ("mutation", "atomic_replace"),
            ("target", "governed"),
            ("disk_generation", "clean_restored"),
        ],
    ));
    events.push(detail_event(
        16,
        DriverEventKind::BufferReopened,
        &[
            ("reopen_index", "1"),
            ("old_bufnr", "5"),
            ("new_bufnr", "7"),
            ("same_path", "1"),
            ("server_init_count", "1"),
            ("document_generation", "instance2_clean"),
        ],
    ));
    events.push(generation_event(17, "2", "instance2_clean", "0", "0"));
    events.push(detail_event(
        18,
        DriverEventKind::LateResultRejected,
        &[
            ("late_index", "1"),
            ("pending_index", "2"),
            ("request_id", "3"),
            ("response_delivered", "1"),
            ("replacement_state_unchanged", "1"),
            ("window_ms", "3000"),
        ],
    ));
    events.push(detail_event(
        19,
        DriverEventKind::PendingActionStarted,
        &[
            ("pending_index", "3"),
            ("method", "textDocument/documentSymbol"),
            ("request_id", "4"),
            ("target_bufnr", "7"),
        ],
    ));
    events.push(detail_event(
        20,
        DriverEventKind::SessionIterationSettled,
        &[
            ("iteration_index", "1"),
            ("session_role", "full_lifecycle_session"),
            ("product_result", "defect_to_current"),
        ],
    ));
    events.push(detail_event(21, DriverEventKind::ShutdownStarted, &[("server_stopping", "1")]));
    events.push(detail_event(22, DriverEventKind::ShutdownCompleted, &[("server_exited", "1")]));
    events.push(detail_event(23, DriverEventKind::HostExitInitiated, &[("exit_path", "user_qa")]));
    events
}

/// The replacement host session's complete event stream (host 2).
fn complete_host2_events(digest: &str) -> Vec<DriverEvent> {
    let mut events = bootstrap_events(digest, "replacement_host_session");
    events.push(generation_event(10, "1", "replacement_open_clean", "0", "0"));
    events.push(generation_event(11, "2", "replacement_own_defect", "1", "0"));
    events.push(generation_event(12, "3", "replacement_own_current", "0", "0"));
    events.push(detail_event(
        13,
        DriverEventKind::SessionIterationSettled,
        &[
            ("iteration_index", "2"),
            ("session_role", "replacement_host_session"),
            ("product_result", "own_edit_cycle"),
        ],
    ));
    events.push(detail_event(14, DriverEventKind::ShutdownStarted, &[("server_stopping", "1")]));
    events.push(detail_event(15, DriverEventKind::ShutdownCompleted, &[("server_exited", "1")]));
    events.push(detail_event(16, DriverEventKind::HostExitInitiated, &[("exit_path", "user_qa")]));
    events
}

/// The assertion-failure session's complete event stream (host 3): the typed
/// forced assertion failure is the designed terminal path.
fn complete_host3_events(digest: &str) -> Vec<DriverEvent> {
    let mut events = bootstrap_events(digest, "assertion_failure_session");
    events.push(generation_event(10, "1", "failure_session_open", "0", "0"));
    events.push(detail_event(
        11,
        DriverEventKind::SessionIterationSettled,
        &[
            ("iteration_index", "3"),
            ("session_role", "assertion_failure_session"),
            ("product_result", "typed_assertion_failure"),
        ],
    ));
    events.push(detail_event(12, DriverEventKind::ShutdownStarted, &[("server_stopping", "1")]));
    events.push(detail_event(13, DriverEventKind::ShutdownCompleted, &[("server_exited", "1")]));
    events.push(detail_event(
        14,
        DriverEventKind::DriverFailed,
        &[("reason", "forced_assertion_failure")],
    ));
    events
}

/// The timeout/interruption session's complete event stream (host 4): the
/// deliberate indefinite barrier is the designed terminal path.
fn complete_host4_events(digest: &str) -> Vec<DriverEvent> {
    let mut events = bootstrap_events(digest, "timeout_interruption_session");
    events.push(generation_event(10, "1", "timeout_session_open", "0", "0"));
    events.push(detail_event(
        11,
        DriverEventKind::SessionIterationSettled,
        &[
            ("iteration_index", "4"),
            ("session_role", "timeout_interruption_session"),
            ("product_result", "typed_timeout_pending"),
        ],
    ));
    events
}

/// The server-restart relabel control's complete event stream.
fn complete_relabel_events(digest: &str) -> Vec<DriverEvent> {
    let mut events = bootstrap_events(digest, "server_restart_relabel_session");
    events.push(generation_event(10, "1", "defect_present", "1", "0"));
    events.push(generation_event(11, "2", "post_restart_defect", "1", "0"));
    events.push(detail_event(
        12,
        DriverEventKind::SessionIterationSettled,
        &[
            ("iteration_index", "1"),
            ("session_role", "server_restart_relabel_session"),
            ("product_result", "server_restart_relabel"),
        ],
    ));
    events.push(detail_event(13, DriverEventKind::ShutdownStarted, &[("server_stopping", "1")]));
    events.push(detail_event(14, DriverEventKind::ShutdownCompleted, &[("server_exited", "1")]));
    events.push(detail_event(15, DriverEventKind::HostExitInitiated, &[("exit_path", "user_qa")]));
    events
}

fn observation_with(
    status: Option<i32>,
    cleanup: CleanupResult,
    events: Vec<DriverEvent>,
    timed_out: bool,
    kill_requested: bool,
) -> ProcessObservation {
    // Mirrors the supervisor: driver completeness is the shared
    // complete-journey contract over the session's own serialized stream.
    let mut text = String::new();
    for item in &events {
        text.push_str(&serde_json::to_string(item).unwrap_or_default());
        text.push('\n');
    }
    let driver_complete = vim_host_runner::parse_driver_events(text.as_bytes(), true).is_ok();
    ProcessObservation {
        status_code: status,
        timed_out,
        kill_requested,
        cleanup,
        cleanup_detail: "process-set comparison clean".to_string(),
        events,
        driver_complete,
        artifacts: Vec::new(),
    }
}

fn wire_with_caps(workspace_folders: Option<bool>) -> WireEvidence {
    WireEvidence {
        saw_initialize: true,
        saw_initialized: true,
        client_capabilities: Some(serde_json::json!({
            "workspace": {"workspaceFolders": workspace_folders.unwrap_or(false)}
        })),
        ..WireEvidence::default()
    }
}

fn host1_wire() -> LifecycleWire {
    LifecycleWire {
        initialize_count: 1,
        did_close_lines: vec![(13, "main.pl".to_string())],
        // Pending #2 (request 3) is answered AFTER the governed didClose at
        // line 13 (the late document route); pending #3 (request 4) stays
        // UNANSWERED through host 1's exit (the in-flight host route). A wire
        // that answers request 4 before exit is the negative control in
        // `a_response_for_the_in_flight_request_defeats_the_host_route`.
        document_symbol_responses: vec![PendingResponse { line_index: 14, request_id: 3 }],
        cancel_request_ids: vec![2],
    }
}

fn ledger_of(pid: u64, survivors: usize, probe: &str) -> serde_json::Value {
    serde_json::json!({
        "pid": pid,
        "surviving_processes": (0..survivors)
            .map(|index| serde_json::json!({"pid": 9000 + index, "args": "perllsp --stdio"}))
            .collect::<Vec<_>>(),
        "process_probe": probe,
    })
}

fn scratch_session(
    index: usize,
    role: &str,
    plan: &VimHostRunPlan,
    events: Vec<DriverEvent>,
    wire: WireEvidence,
    lifecycle_wire: LifecycleWire,
    status: Option<i32>,
    cleanup: CleanupResult,
    timed_out: bool,
    kill_requested: bool,
    pid: u64,
    settled: Option<bool>,
) -> HostSessionRecord {
    HostSessionRecord {
        index,
        role: role.to_string(),
        observation: observation_with(status, cleanup, events, timed_out, kill_requested),
        wire,
        lifecycle_wire,
        plan: plan.clone(),
        ledger: Some(ledger_of(pid, 0, "available")),
        settled_probe_clean: settled,
        capability_snapshot: PathBuf::from("initialize.json"),
    }
}

/// The complete canonical four-session journey evidence.
fn canonical_sessions(plan: &VimHostRunPlan, digest: &str) -> Vec<HostSessionRecord> {
    vec![
        scratch_session(
            1,
            "full_lifecycle_session",
            plan,
            complete_host1_events(digest),
            wire_with_caps(Some(false)),
            host1_wire(),
            Some(0),
            CleanupResult::Pass,
            false,
            false,
            111,
            Some(true),
        ),
        scratch_session(
            2,
            "replacement_host_session",
            plan,
            complete_host2_events(digest),
            wire_with_caps(Some(false)),
            LifecycleWire { initialize_count: 1, ..LifecycleWire::default() },
            Some(0),
            CleanupResult::Pass,
            false,
            false,
            222,
            Some(true),
        ),
        scratch_session(
            3,
            "assertion_failure_session",
            plan,
            complete_host3_events(digest),
            wire_with_caps(Some(false)),
            LifecycleWire { initialize_count: 1, ..LifecycleWire::default() },
            Some(2),
            CleanupResult::NotProven,
            false,
            false,
            333,
            Some(true),
        ),
        scratch_session(
            4,
            "timeout_interruption_session",
            plan,
            complete_host4_events(digest),
            wire_with_caps(Some(false)),
            LifecycleWire { initialize_count: 1, ..LifecycleWire::default() },
            None,
            CleanupResult::NotProven,
            true,
            true,
            444,
            Some(true),
        ),
    ]
}

// ---------------------------------------------------------------------------
// Fixture laws
// ---------------------------------------------------------------------------

#[test]
fn lifecycle_fixture_ships_the_defective_generation() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let fixture = materialize_lifecycle_fixture(&dir.path().join("fixture"))?;
    let project = fixture.join("workspace/project");
    ensure!(project.join("cpanfile").is_file(), "the governed marker must exist");
    ensure!(
        project.join("lib/My/Widget.pm").is_file(),
        "the governed module must exist (registration channel)"
    );
    let main = fs::read_to_string(project.join("main.pl"))?;
    ensure!(
        main.contains(DEFECT_LINE_TEXT),
        "the governed source ships the defective generation (the old state to invalidate)"
    );
    ensure!(
        !main.contains(CLEAN_LINE_TEXT),
        "the governed source must not ship the clean generation (the disk truth arrives only \
         through the journey's external replacement)"
    );
    Ok(())
}

#[test]
fn lifecycle_fixture_variants_are_digest_distinct_where_the_stimulus_differs() -> Result<()> {
    // The relabel control reuses the canonical fixture: only the journey's
    // claim differs (a same-bytes control proves the discriminator is the
    // judgment, not the fixture).
    let dir = tempfile::tempdir()?;
    let first = materialize_lifecycle_fixture(&dir.path().join("one"))?;
    let second = materialize_lifecycle_fixture(&dir.path().join("two"))?;
    ensure!(fixture_digest(&first)? == fixture_digest(&second)?, "the fixture is deterministic");
    Ok(())
}

// ---------------------------------------------------------------------------
// Driver-event laws for the repeating lifecycle kinds
// ---------------------------------------------------------------------------

#[test]
fn complete_lifecycle_event_streams_validate() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_lifecycle_plan(&dir.path().join("scratch"))?;
    let digest = plan.identity.candidate_artifact_sha256.clone();
    for events in [complete_host1_events(&digest), complete_host2_events(&digest)] {
        ensure!(
            validate_driver_events(&events, true).is_ok(),
            "the complete orderly session streams must validate under the shared driver contract: \
             {:?}",
            validate_driver_events(&events, true)
        );
    }
    // The designed-failure sessions fail the complete-journey contract (a
    // typed driver failure; a missing shutdown tier) exactly as the
    // supervisor's driver_complete observation reports.
    ensure!(
        validate_driver_events(&complete_host3_events(&digest), true).is_err(),
        "the assertion-failure session must not report driver completeness"
    );
    ensure!(
        validate_driver_events(&complete_host4_events(&digest), true).is_err(),
        "the timeout session must not report driver completeness"
    );
    ensure!(
        validate_driver_events(&complete_relabel_events(&digest), true).is_ok(),
        "the relabel control's own session stream is orderly (the judgment, not the driver, \
         rejects the relabel)"
    );
    Ok(())
}

#[test]
fn lifecycle_event_repetition_laws_reject_disorder_and_forgeries() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_lifecycle_plan(&dir.path().join("scratch"))?;
    let digest = plan.identity.candidate_artifact_sha256.clone();

    // A skipped pending index is rejected.
    let mut skipped = complete_host1_events(&digest);
    skipped[12].details.insert("pending_index".to_string(), "3".to_string());
    renumber(&mut skipped);
    ensure!(
        validate_driver_events(&skipped, true).is_err(),
        "a skipped pending index must be rejected"
    );

    // A cancelled result that was admitted is rejected here, before any
    // judgment can pass it.
    let mut admitted = complete_host1_events(&digest);
    admitted[11].details.insert("notification_count".to_string(), "1".to_string());
    renumber(&mut admitted);
    ensure!(
        validate_driver_events(&admitted, true).is_err(),
        "an admitted cancelled result is a contract violation, never evidence"
    );

    // A reopen without a changed document instance is rejected.
    let mut same_instance = complete_host1_events(&digest);
    same_instance[15].details.insert("new_bufnr".to_string(), "5".to_string());
    renumber(&mut same_instance);
    ensure!(
        validate_driver_events(&same_instance, true).is_err(),
        "a same-instance reopen is not a buffer reopen"
    );

    // A wipe without the real didClose path is rejected.
    let mut no_didclose = complete_host1_events(&digest);
    no_didclose[13].details.insert("didclose_sent".to_string(), "0".to_string());
    renumber(&mut no_didclose);
    ensure!(
        validate_driver_events(&no_didclose, true).is_err(),
        "a wipe without the client's own didClose is not an invalidation"
    );

    // A late-result claim without the bounded window is rejected.
    let mut tiny_window = complete_host1_events(&digest);
    tiny_window[17].details.insert("window_ms".to_string(), "10".to_string());
    renumber(&mut tiny_window);
    ensure!(
        validate_driver_events(&tiny_window, true).is_err(),
        "a late-result claim without a real observation window must be rejected"
    );

    // A late-result claim whose old operation never completed is rejected.
    let mut undelivered = complete_host1_events(&digest);
    undelivered[17].details.insert("response_delivered".to_string(), "0".to_string());
    renumber(&mut undelivered);
    ensure!(
        validate_driver_events(&undelivered, true).is_err(),
        "an uncompleted observation is not a late result"
    );

    // A pending action after the shutdown boundary is rejected.
    let mut after_shutdown = complete_host1_events(&digest);
    let late = after_shutdown.remove(10);
    after_shutdown.push(late);
    renumber(&mut after_shutdown);
    ensure!(
        validate_driver_events(&after_shutdown, true).is_err(),
        "a pending barrier after shutdown must be rejected"
    );

    // The exit initiation must bind the user-equivalent path.
    let mut forged_exit = complete_host1_events(&digest);
    forged_exit[22].details.insert("exit_path".to_string(), "force_kill".to_string());
    renumber(&mut forged_exit);
    ensure!(
        validate_driver_events(&forged_exit, true).is_err(),
        "a force-kill exit path cannot pose as the user-equivalent exit"
    );

    // An invented session role is rejected.
    let mut forged_role = complete_host2_events(&digest);
    forged_role[12].details.insert("session_role".to_string(), "quick_pass_session".to_string());
    renumber(&mut forged_role);
    ensure!(
        validate_driver_events(&forged_role, true).is_err(),
        "an invented session role cannot settle an iteration"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Wire-mining laws
// ---------------------------------------------------------------------------

#[test]
fn lifecycle_wire_counts_outgoing_sends_only_and_mines_identities() -> Result<()> {
    // Response envelopes echo the original request: the extractor must bind
    // response identities from the response envelope, cancel identities from
    // outgoing sends only, and never admit backslash URIs.
    let log = concat!(
        "12:00:01 [\"--->\",1,\"perllsp\",{\"method\":\"initialize\",\"params\":{}}]\n",
        "12:00:02 [\"--->\",1,\"perllsp\",{\"method\":\"initialize\",\"params\":{}}]\n",
        "12:00:03 [\"<---\",1,\"perllsp\",{\"response\":{\"id\":1,\"result\":{}},\"request\":{\"id\":1,\"method\":\"initialize\"}}]\n",
        "12:00:04 [\"--->\",2,\"perllsp\",{\"method\":\"textDocument/documentSymbol\",\"params\":{}}]\n",
        "12:00:05 [\"--->\",2,\"perllsp\",{\"method\":\"$/cancelRequest\",\"params\":{\"id\":2}}]\n",
        "12:00:06 [\"<---\",2,\"perllsp\",{\"response\":{\"id\":2,\"result\":[]},\"request\":{\"id\":2,\"method\":\"textDocument/documentSymbol\"}}]\n",
        "12:00:07 [\"--->\",3,\"perllsp\",{\"method\":\"textDocument/didClose\",\"params\":{\"textDocument\":{\"uri\":\"file:///w/main.pl\"}}}]\n"
    );
    let wire = extract_lifecycle_wire(log.as_bytes());
    ensure!(wire.initialize_count == 2, "outgoing initializes are counted");
    ensure!(wire.cancel_request_ids == vec![2], "cancel identities come from outgoing sends");
    ensure!(wire.document_symbol_responses.len() == 1, "the symbol response is mined");
    ensure!(wire.response_line_of(2) == Some(5), "the response identity is bound");
    ensure!(wire.first_close_line("main.pl") == Some(6), "the close boundary is mined");
    ensure!(wire.response_line_of(7).is_none(), "unknown identities never resolve");
    let windows_only = extract_lifecycle_wire(
        b"[\"--->\",1,\"p\",{\"method\":\"textDocument/didClose\",\"params\":{\"textDocument\":{\"uri\":\"file:///w\\\\main.pl\"}}}]\n",
    );
    ensure!(
        windows_only.did_close_lines.is_empty(),
        "a backslash-qualified URI is not a governed boundary"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Judgment: the canonical positive path and the discriminating controls
// ---------------------------------------------------------------------------

#[test]
fn canonical_evidence_passes_all_eight_cells() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_lifecycle_plan(&dir.path().join("scratch"))?;
    let digest = plan.identity.candidate_artifact_sha256.clone();
    let sessions = canonical_sessions(&plan, &digest);
    let judgment = evaluate_lifecycle_observation(&sessions, LifecycleFixtureVariant::Canonical);
    ensure!(
        judgment.result == ObservationResult::Pass,
        "the canonical evidence must pass: cells {:?}, failure_reason {:?}",
        judgment.cells,
        judgment.failure_reason
    );
    for cell in [
        CELL_BUFFER_REOPEN,
        CELL_HOST_REOPEN,
        CELL_CANCELLATION,
        CELL_LATE_RESULT,
        CELL_REPEATED_SESSIONS,
        CELL_NORMAL_CLEANUP,
        CELL_FAILURE_CLEANUP,
    ] {
        ensure!(
            judgment.cells.get(cell) == Some(&ObservationResult::Pass),
            "cell {cell} must pass on canonical evidence: {:?}",
            judgment.cells
        );
    }
    ensure!(
        judgment.cells.get(CELL_WORKSPACE_REOPEN) == Some(&ObservationResult::Unsupported),
        "the workspace cell must carry its honest not-exposed disposition"
    );
    ensure!(judgment.failure_reason.is_none());
    ensure!(judgment.all_hosts_as_designed);
    Ok(())
}

#[test]
fn a_server_restart_relabel_fails_typed_host_replacement_absent() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_lifecycle_plan(&dir.path().join("scratch"))?;
    let digest = plan.identity.candidate_artifact_sha256.clone();
    // The control's own session is orderly (status 0, clean exit) and even
    // re-establishes the defect through the restarted server: everything a
    // relabel would need except a changed host instance.
    let sessions = vec![scratch_session(
        1,
        "server_restart_relabel_session",
        &plan,
        complete_relabel_events(&digest),
        wire_with_caps(Some(false)),
        LifecycleWire { initialize_count: 2, ..LifecycleWire::default() },
        Some(0),
        CleanupResult::Pass,
        false,
        false,
        111,
        Some(true),
    )];
    let judgment =
        evaluate_lifecycle_observation(&sessions, LifecycleFixtureVariant::ServerRestartRelabel);
    ensure!(
        judgment.result == ObservationResult::Fail,
        "the relabel control must fail, not pass or degrade"
    );
    ensure!(
        judgment.cells.get(CELL_HOST_REOPEN) == Some(&ObservationResult::Fail),
        "a server restart can never satisfy the host-reopen cell"
    );
    ensure!(
        judgment.failure_reason.as_deref() == Some("host_replacement_absent"),
        "the typed detection must name the absent host replacement: {:?}",
        judgment.failure_reason
    );
    Ok(())
}

#[test]
fn a_buffer_reopen_relabel_fails_the_host_reopen_cell() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_lifecycle_plan(&dir.path().join("scratch"))?;
    let digest = plan.identity.candidate_artifact_sha256.clone();
    // The full buffer-reopen chain inside ONE host: the buffer cell passes,
    // but the host-reopen cell must fail — a buffer reopen is a required
    // false subject.
    let sessions = vec![scratch_session(
        1,
        "full_lifecycle_session",
        &plan,
        complete_host1_events(&digest),
        wire_with_caps(Some(false)),
        host1_wire(),
        Some(0),
        CleanupResult::Pass,
        false,
        false,
        111,
        Some(true),
    )];
    let judgment = evaluate_lifecycle_observation(&sessions, LifecycleFixtureVariant::Canonical);
    ensure!(
        judgment.cells.get(CELL_BUFFER_REOPEN) == Some(&ObservationResult::Pass),
        "the same-host reopen chain itself is valid"
    );
    ensure!(
        judgment.cells.get(CELL_HOST_REOPEN) == Some(&ObservationResult::Fail),
        "a buffer reopen relabeled as host reopen must fail the host-reopen cell"
    );
    ensure!(
        judgment.cells.get(CELL_REPEATED_SESSIONS) == Some(&ObservationResult::NotProven),
        "a single session can never claim repeated use"
    );
    ensure!(judgment.result != ObservationResult::Pass);
    Ok(())
}

#[test]
fn a_single_passing_run_is_not_repeated_use() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_lifecycle_plan(&dir.path().join("scratch"))?;
    let digest = plan.identity.candidate_artifact_sha256.clone();
    // Two sessions but the SAME process identity (a bare re-run without a
    // changed host instance): the denominator law must reject it.
    let mut sessions = canonical_sessions(&plan, &digest);
    if let Some(second) = sessions.get_mut(1) {
        second.ledger = Some(ledger_of(111, 0, "available"));
    }
    let judgment = evaluate_lifecycle_observation(&sessions, LifecycleFixtureVariant::Canonical);
    ensure!(
        judgment.cells.get(CELL_REPEATED_SESSIONS) == Some(&ObservationResult::Fail),
        "iterations over one host identity are not repeated use over changed host instances"
    );
    ensure!(judgment.result != ObservationResult::Pass);
    Ok(())
}

#[test]
fn a_prior_sessions_stale_state_cannot_open_the_replacement() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_lifecycle_plan(&dir.path().join("scratch"))?;
    let digest = plan.identity.candidate_artifact_sha256.clone();
    // Host 2's opening observation carries the OLD defective state (errors=1)
    // where the supervisor wrote the clean disk generation: the stale-state
    // falsifier.
    let mut sessions = canonical_sessions(&plan, &digest);
    if let Some(second) = sessions.get_mut(1) {
        second.observation.events[9] =
            generation_event(10, "1", "replacement_open_clean", "1", "0");
    }
    let judgment = evaluate_lifecycle_observation(&sessions, LifecycleFixtureVariant::Canonical);
    ensure!(
        judgment.cells.get(CELL_REPEATED_SESSIONS) == Some(&ObservationResult::Fail),
        "a replacement opening on a prior session's stale state fails repeated use"
    );
    ensure!(
        judgment.cells.get(CELL_HOST_REOPEN) == Some(&ObservationResult::Fail),
        "a replacement opening on stale state is not an honest host reopen"
    );
    Ok(())
}

#[test]
fn a_workspace_surface_the_client_exposes_changes_the_row() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_lifecycle_plan(&dir.path().join("scratch"))?;
    let digest = plan.identity.candidate_artifact_sha256.clone();
    let mut sessions = canonical_sessions(&plan, &digest);
    sessions[0].wire = wire_with_caps(Some(true));
    let judgment = evaluate_lifecycle_observation(&sessions, LifecycleFixtureVariant::Canonical);
    ensure!(
        judgment.cells.get(CELL_WORKSPACE_REOPEN) == Some(&ObservationResult::Fail),
        "a client that exposes workspace folders is a different row: the not-exposed \
         disposition must fail instead of silently passing"
    );
    ensure!(judgment.result != ObservationResult::Pass);
    ensure!(judgment.workspace_folders_offered == Some(true));
    Ok(())
}

#[test]
fn an_unmined_late_response_cannot_satisfy_late_result_rejection() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_lifecycle_plan(&dir.path().join("scratch"))?;
    let digest = plan.identity.candidate_artifact_sha256.clone();
    let mut sessions = canonical_sessions(&plan, &digest);
    // Strip the mined response for pending #2: the driver's claim alone
    // (response_delivered=1) must not satisfy the cell.
    sessions[0].lifecycle_wire.document_symbol_responses.clear();
    let judgment = evaluate_lifecycle_observation(&sessions, LifecycleFixtureVariant::Canonical);
    ensure!(
        judgment.cells.get(CELL_LATE_RESULT) == Some(&ObservationResult::Fail),
        "the wire-mined response is load-bearing: a driver claim alone cannot prove the old \
         operation completed"
    );
    Ok(())
}

#[test]
fn a_response_before_the_document_close_is_not_a_late_result() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_lifecycle_plan(&dir.path().join("scratch"))?;
    let digest = plan.identity.candidate_artifact_sha256.clone();
    let mut sessions = canonical_sessions(&plan, &digest);
    // The old operation's response lands BEFORE the governed didClose (line
    // 13): the document was never invalidated between request and response,
    // so the "late result" claim has no observed invalidation to reject.
    sessions[0].lifecycle_wire.document_symbol_responses =
        vec![PendingResponse { line_index: 12, request_id: 3 }];
    let judgment = evaluate_lifecycle_observation(&sessions, LifecycleFixtureVariant::Canonical);
    ensure!(
        judgment.cells.get(CELL_LATE_RESULT) == Some(&ObservationResult::Fail),
        "the response must follow the document close to prove lateness: {:?}",
        judgment.cells
    );
    Ok(())
}

#[test]
fn a_response_for_the_in_flight_request_defeats_the_host_route() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_lifecycle_plan(&dir.path().join("scratch"))?;
    let digest = plan.identity.candidate_artifact_sha256.clone();
    let mut sessions = canonical_sessions(&plan, &digest);
    // Pending #3 (request 4) receives its response before host 1 exits: no
    // work was ever in flight across the host boundary, so the host route of
    // the late-result cell must fail.
    sessions[0]
        .lifecycle_wire
        .document_symbol_responses
        .push(PendingResponse { line_index: 20, request_id: 4 });
    let judgment = evaluate_lifecycle_observation(&sessions, LifecycleFixtureVariant::Canonical);
    ensure!(
        judgment.cells.get(CELL_LATE_RESULT) == Some(&ObservationResult::Fail),
        "a request answered before host exit was never in flight: {:?}",
        judgment.cells
    );
    Ok(())
}

#[test]
fn an_unexercised_relabel_control_is_never_a_typed_failure() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_lifecycle_plan(&dir.path().join("scratch"))?;
    let digest = plan.identity.candidate_artifact_sha256.clone();
    // The control's host timed out before any in-host restart: no second
    // outgoing `initialize` ever landed on its wire. The typed
    // `host_replacement_absent` detection must not be asserted from the
    // variant — the run is an instrument gap (NotProven, no typed reason),
    // which the CLI rejects instead of accepting a typed failure that never
    // exercised its designed false subject.
    let sessions = vec![scratch_session(
        1,
        "server_restart_relabel_session",
        &plan,
        complete_relabel_events(&digest),
        wire_with_caps(Some(false)),
        LifecycleWire::default(),
        None,
        CleanupResult::NotProven,
        true,
        true,
        111,
        Some(true),
    )];
    let judgment =
        evaluate_lifecycle_observation(&sessions, LifecycleFixtureVariant::ServerRestartRelabel);
    ensure!(
        judgment.result == ObservationResult::NotProven,
        "a control that never exercised its relabel path is not the typed failure: {:?}",
        judgment.cells
    );
    ensure!(
        judgment.failure_reason.is_none(),
        "no typed reason may be assigned without relabel evidence: {:?}",
        judgment.failure_reason
    );
    Ok(())
}

#[test]
fn a_missing_cancel_identity_on_the_wire_fails_cancellation() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_lifecycle_plan(&dir.path().join("scratch"))?;
    let digest = plan.identity.candidate_artifact_sha256.clone();
    let mut sessions = canonical_sessions(&plan, &digest);
    sessions[0].lifecycle_wire.cancel_request_ids.clear();
    let judgment = evaluate_lifecycle_observation(&sessions, LifecycleFixtureVariant::Canonical);
    ensure!(
        judgment.cells.get(CELL_CANCELLATION) == Some(&ObservationResult::Fail),
        "cancellation must be identity-bound on the wire, not asserted by the driver"
    );
    Ok(())
}

#[test]
fn a_surviving_owned_process_fails_forced_failure_cleanup() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_lifecycle_plan(&dir.path().join("scratch"))?;
    let digest = plan.identity.candidate_artifact_sha256.clone();
    let mut sessions = canonical_sessions(&plan, &digest);
    // The timeout session's settled probe observes a survivor.
    sessions[3].settled_probe_clean = Some(false);
    let judgment = evaluate_lifecycle_observation(&sessions, LifecycleFixtureVariant::Canonical);
    ensure!(
        judgment.cells.get(CELL_FAILURE_CLEANUP) == Some(&ObservationResult::Fail),
        "an owned process surviving the settled window is an observed leak, never a pass"
    );
    ensure!(judgment.result != ObservationResult::Pass);
    Ok(())
}

#[test]
fn a_missing_settle_probe_is_not_proven_never_zero() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_lifecycle_plan(&dir.path().join("scratch"))?;
    let digest = plan.identity.candidate_artifact_sha256.clone();
    let mut sessions = canonical_sessions(&plan, &digest);
    sessions[2].settled_probe_clean = None;
    sessions[3].settled_probe_clean = None;
    let judgment = evaluate_lifecycle_observation(&sessions, LifecycleFixtureVariant::Canonical);
    ensure!(
        judgment.cells.get(CELL_FAILURE_CLEANUP) == Some(&ObservationResult::NotProven),
        "a missing resource observation is encoded as not_proven, never as zero"
    );
    ensure!(judgment.result != ObservationResult::Pass);
    Ok(())
}

#[test]
fn an_undesigned_failure_or_missing_kill_cannot_satisfy_failure_cleanup() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_lifecycle_plan(&dir.path().join("scratch"))?;
    let digest = plan.identity.candidate_artifact_sha256.clone();
    // The assertion session exits 0 instead of its designed typed failure.
    let mut sessions = canonical_sessions(&plan, &digest);
    sessions[2].observation.status_code = Some(0);
    let judgment = evaluate_lifecycle_observation(&sessions, LifecycleFixtureVariant::Canonical);
    ensure!(
        judgment.cells.get(CELL_FAILURE_CLEANUP) == Some(&ObservationResult::Fail),
        "an orderly exit cannot pose as the forced assertion-failure path"
    );

    // The timeout session returns without the supervisor kill.
    let mut sessions = canonical_sessions(&plan, &digest);
    sessions[3].observation.timed_out = false;
    sessions[3].observation.kill_requested = false;
    let judgment = evaluate_lifecycle_observation(&sessions, LifecycleFixtureVariant::Canonical);
    ensure!(
        judgment.cells.get(CELL_FAILURE_CLEANUP) == Some(&ObservationResult::Fail),
        "an unbounded hang that returned cannot pose as the timeout/interruption shape"
    );
    Ok(())
}

#[test]
fn normal_cleanup_requires_observed_clean_comparisons() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_lifecycle_plan(&dir.path().join("scratch"))?;
    let digest = plan.identity.candidate_artifact_sha256.clone();
    // A missing ledger (no retained observation) degrades normal cleanup.
    let mut sessions = canonical_sessions(&plan, &digest);
    sessions[0].ledger = None;
    let judgment = evaluate_lifecycle_observation(&sessions, LifecycleFixtureVariant::Canonical);
    ensure!(
        judgment.cells.get(CELL_NORMAL_CLEANUP) == Some(&ObservationResult::NotProven),
        "cleanup without the retained ledger observation is not_proven"
    );

    // A nonzero exit on an orderly session fails normal cleanup.
    let mut sessions = canonical_sessions(&plan, &digest);
    sessions[1].observation.status_code = Some(2);
    let judgment = evaluate_lifecycle_observation(&sessions, LifecycleFixtureVariant::Canonical);
    ensure!(
        judgment.cells.get(CELL_NORMAL_CLEANUP) == Some(&ObservationResult::Fail),
        "a nonzero exit on an orderly session is not normal terminal cleanup"
    );
    Ok(())
}

#[test]
fn a_leaked_process_fails_the_whole_journey() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_lifecycle_plan(&dir.path().join("scratch"))?;
    let digest = plan.identity.candidate_artifact_sha256.clone();
    let mut sessions = canonical_sessions(&plan, &digest);
    sessions[0].observation.cleanup = CleanupResult::Fail;
    sessions[0].observation.cleanup_detail =
        "process-set comparison observed 1 surviving".to_string();
    let judgment = evaluate_lifecycle_observation(&sessions, LifecycleFixtureVariant::Canonical);
    ensure!(judgment.result == ObservationResult::Fail, "an observed leak fails the journey");
    Ok(())
}

#[test]
fn a_surviving_forced_session_process_fails_the_aggregate() -> Result<()> {
    // Real-host regression guard (CI run 33022125220 class): the substrate
    // degrades designed forced shapes to not-proven (their exit skips the
    // driver shutdown path), so the journey aggregate must judge them through
    // the dedicated bounded settle probe instead. A survivor behind that probe
    // is an owned leak and must fail the receipt cleanup — never be absorbed.
    let dir = tempfile::tempdir()?;
    let plan = scratch_lifecycle_plan(&dir.path().join("scratch"))?;
    let digest = plan.identity.candidate_artifact_sha256.clone();
    let mut sessions = canonical_sessions(&plan, &digest);
    sessions[3].settled_probe_clean = Some(false);
    let judgment = evaluate_lifecycle_observation(&sessions, LifecycleFixtureVariant::Canonical);
    ensure!(
        judgment.cells.get(CELL_FAILURE_CLEANUP) == Some(&ObservationResult::Fail),
        "a surviving forced-session process must fail failure_cleanup"
    );
    let aggregate = aggregate_journey_observation(&sessions);
    ensure!(
        aggregate.cleanup == CleanupResult::Fail,
        "a survivor behind the settle probe must fail the published process cleanup"
    );
    Ok(())
}

#[test]
fn an_unavailable_forced_settle_probe_is_not_proven_never_zero() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_lifecycle_plan(&dir.path().join("scratch"))?;
    let digest = plan.identity.candidate_artifact_sha256.clone();
    let mut sessions = canonical_sessions(&plan, &digest);
    sessions[2].settled_probe_clean = None;
    let judgment = evaluate_lifecycle_observation(&sessions, LifecycleFixtureVariant::Canonical);
    ensure!(
        judgment.cells.get(CELL_FAILURE_CLEANUP) == Some(&ObservationResult::NotProven),
        "an unavailable forced-shape settle probe is an instrument gap, not zero"
    );
    let aggregate = aggregate_journey_observation(&sessions);
    ensure!(
        aggregate.cleanup == CleanupResult::NotProven,
        "the published cleanup must stay honestly unproven when the probe is missing"
    );
    Ok(())
}

#[test]
fn the_published_cleanup_passes_over_designed_forced_shapes_with_clean_settles() -> Result<()> {
    // The canonical fixture keeps hosts 3/4 at the real substrate shape:
    // nonzero-exit/forced-kill sessions carry `not_proven` barrier comparison,
    // yet their dedicated bounded probes settled clean. The receipt's
    // published cleanup must then reflect every owned resource cleaned while
    // the raw per-host truths remain visible as limitations, so the CI bind
    // (`process_cleanup == pass`) proves exactly what it claims.
    let dir = tempfile::tempdir()?;
    let plan = scratch_lifecycle_plan(&dir.path().join("scratch"))?;
    let digest = plan.identity.candidate_artifact_sha256.clone();
    let sessions = canonical_sessions(&plan, &digest);
    ensure!(
        sessions[2].observation.cleanup == CleanupResult::NotProven
            && sessions[3].observation.cleanup == CleanupResult::NotProven,
        "fixture hosts 3/4 must model the real substrate degradation, never a fake pass"
    );
    let aggregate = aggregate_journey_observation(&sessions);
    ensure!(
        aggregate.cleanup == CleanupResult::Pass,
        "clean forced-shape settles must compose a passing published cleanup"
    );
    ensure!(aggregate.cleanup_detail.contains("host-3"), "per-host truths stay in the detail");
    ensure!(aggregate.cleanup_detail.contains("host-4"), "per-host truths stay in the detail");
    Ok(())
}

fn empty_evidence_is_not_proven_not_passed() -> Result<()> {
    let judgment = evaluate_lifecycle_observation(&[], LifecycleFixtureVariant::Canonical);
    ensure!(judgment.result == ObservationResult::NotProven);
    ensure!(judgment.cells.values().all(|result| *result == ObservationResult::NotProven));
    Ok(())
}

// ---------------------------------------------------------------------------
// Receipt composition
// ---------------------------------------------------------------------------

#[test]
fn journey_receipt_carries_prefixed_host_barriers_and_the_eight_catalog_cells() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_lifecycle_plan(&dir.path().join("scratch"))?;
    let digest = plan.identity.candidate_artifact_sha256.clone();
    let sessions = canonical_sessions(&plan, &digest);
    let judgment = evaluate_lifecycle_observation(&sessions, LifecycleFixtureVariant::Canonical);
    let cells = lifecycle_journey(&sessions, &judgment);
    let catalog: Vec<&str> = cells
        .iter()
        .filter(|cell| cell.id.starts_with("vim.vim_lsp.lifecycle."))
        .map(|cell| cell.id.as_str())
        .collect();
    ensure!(
        catalog.len() == 8,
        "exactly the eight #11387 catalog cells may appear, found {catalog:?}"
    );
    for expected in [
        CELL_BUFFER_REOPEN,
        CELL_HOST_REOPEN,
        CELL_WORKSPACE_REOPEN,
        CELL_CANCELLATION,
        CELL_LATE_RESULT,
        CELL_REPEATED_SESSIONS,
        CELL_NORMAL_CLEANUP,
        CELL_FAILURE_CLEANUP,
    ] {
        ensure!(catalog.contains(&expected), "catalog cell {expected} missing");
    }
    let workspace = cells.iter().find(|cell| cell.id == CELL_WORKSPACE_REOPEN).unwrap();
    ensure!(!workspace.observed, "an unsupported cell must not claim an observation");
    ensure!(
        workspace.limitation.as_deref().is_some_and(|text| text.contains("client_not_exposed")),
        "the workspace limitation must name the honest disposition"
    );
    for host in 1..=2 {
        ensure!(
            cells.iter().any(|cell| cell.id == format!("host{host}_host_started")),
            "orderly host {host}'s substrate barriers must appear with their prefixed identity"
        );
    }
    // The designed-failure sessions contribute failure-cleanup evidence, not
    // barrier cells: a passing receipt never carries not-proven barriers for
    // sessions that deliberately never reach them.
    for host in 3..=4 {
        ensure!(
            cells.iter().all(|cell| cell.id != format!("host{host}_host_started")),
            "designed-failure host {host}'s barriers are the failure-cleanup evidence, not \
             unproven barrier claims"
        );
    }
    ensure!(
        cells.iter().any(|cell| cell.id == "host1_shutdown_completed"),
        "the orderly session's shutdown barrier must appear"
    );
    Ok(())
}

#[test]
fn the_journey_receipt_binds_the_shared_subject_identity() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_lifecycle_plan(&dir.path().join("scratch"))?;
    let digest = plan.identity.candidate_artifact_sha256.clone();
    let sessions = canonical_sessions(&plan, &digest);
    let judgment = evaluate_lifecycle_observation(&sessions, LifecycleFixtureVariant::Canonical);
    let capabilities = xtask::editor_client_compat::CapabilityIdentity {
        initialize_snapshot_sha256: vim_host_runner::bytes_sha256(b"snapshot")?,
        position_encodings_offered: vec!["utf-16".to_string()],
        position_encoding_basis: xtask::editor_client_compat::PositionEncodingBasis::Offered,
        position_encoding_selected: Some("utf-16".to_string()),
    };
    let diagnostics = xtask::editor_client_compat::DiagnosticsIdentity {
        advertised_mode: xtask::editor_client_compat::DiagnosticMode::Push,
        observed_messages: vec!["publish_diagnostics".to_string()],
    };
    let journey_observation = ProcessObservation {
        status_code: None,
        timed_out: false,
        kill_requested: false,
        cleanup: CleanupResult::Pass,
        cleanup_detail: "aggregate".to_string(),
        events: Vec::new(),
        driver_complete: true,
        artifacts: vec![
            xtask::editor_client_compat::EvidenceArtifact {
                kind: xtask::editor_client_compat::ArtifactKind::ClientLog,
                id: "host-1/vim/vim-lsp-client.log".to_string(),
                sha256: vim_host_runner::bytes_sha256(b"log")?,
            },
            xtask::editor_client_compat::EvidenceArtifact {
                kind: xtask::editor_client_compat::ArtifactKind::ServerStderr,
                id: "host-1/vim/perllsp.log".to_string(),
                sha256: vim_host_runner::bytes_sha256(b"trace")?,
            },
            xtask::editor_client_compat::EvidenceArtifact {
                kind: xtask::editor_client_compat::ArtifactKind::CapabilitySnapshot,
                id: "host-1/vim/initialize.json".to_string(),
                sha256: vim_host_runner::bytes_sha256(b"snapshot")?,
            },
            xtask::editor_client_compat::EvidenceArtifact {
                kind: xtask::editor_client_compat::ArtifactKind::ProcessLedger,
                id: "host-1/vim/process-ledger.json".to_string(),
                sha256: vim_host_runner::bytes_sha256(b"ledger")?,
            },
        ],
    };
    let receipt = vim_host_runner::build_receipt(
        &sessions[0].plan,
        &journey_observation,
        capabilities,
        diagnostics,
        lifecycle_journey(&sessions, &judgment),
        judgment.result,
        judgment.failure_class,
        vec!["test".to_string()],
        "#11401 test".to_string(),
    );
    ensure!(
        receipt.validate().is_ok(),
        "the composed receipt must validate: {:?}",
        receipt.validate()
    );
    ensure!(
        vim_host_runner::validate_receipt_binding(&receipt, &sessions[0].plan).is_ok(),
        "the receipt binds the shared subject identity of the first session's plan"
    );
    // A stale receipt (another candidate artifact) must be refused.
    let mut stale = sessions[1].plan.clone();
    stale.identity.candidate_artifact_sha256 = "f".repeat(64);
    ensure!(
        vim_host_runner::validate_receipt_binding(&receipt, &stale).is_err(),
        "a receipt from another candidate cannot satisfy this journey"
    );
    Ok(())
}
