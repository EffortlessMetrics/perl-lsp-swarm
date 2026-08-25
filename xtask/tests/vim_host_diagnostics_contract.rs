// #10946 bootstrap/diagnostics scenario contract tests.
//
// Red-first law: the negative controls below were authored and proven to
// reject before the positive host journey ran. Every discriminating control
// feeds the judgment the exact evidence a false green would need (a wrong
// root, an absent defect, a server-log-only claim, an unrelated diagnostic,
// a reused pre-edit push, a stale state) and asserts the slice refuses it.
// Real-editor launches are not unit tests: the canonical journeys run in the
// dedicated workflow (`.github/workflows/vim-hermetic-host.yml`).
#![allow(dead_code)]

use anyhow::{Result, ensure};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use xtask::editor_client_compat::{
    CANONICAL_EXPECTATION_SET_ID, CleanupResult, EvidenceStage, ObservationResult,
    PlatformIdentity, RegistrationState, WorkspaceFixtureIdentity,
    canonical_expectation_set_digest, fixture_digest,
};
use xtask::vim_host_diagnostics_run::{
    CELL_BASELINE_CLEANUP, CELL_BOOTSTRAP, CELL_CURRENTNESS, CELL_DIAGNOSTICS, CELL_ROOT,
    DECOY_ROOT_REL, DECOY_SAME_NAME_FILE_REL, DEFECT_LINE, DiagnosticsFixtureVariant,
    EXPECTED_ROOT_REL, OPENED_FILE_REL, materialize_diagnostics_fixture,
};
use xtask::vim_host_run::vim_host_runner::{
    self, DRIVER_SCHEMA_VERSION, DriverEvent, DriverEventKind, RUN_PLAN_SCHEMA_VERSION,
    VimHostPaths, VimHostRunIdentity, VimHostRunPlan, WireEvidence, validate_driver_events,
};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(1).unwrap_or(Path::new(".")).to_path_buf()
}

// ---------------------------------------------------------------------------
// Scratch plan and observation helpers
// ---------------------------------------------------------------------------

fn scratch_diagnostics_plan(root: &Path) -> Result<VimHostRunPlan> {
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
    let fixture = materialize_diagnostics_fixture(
        &root.join("fixture"),
        DiagnosticsFixtureVariant::Canonical,
    )?;
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
            candidate_identity_packet_sha256: vim_host_runner::bytes_sha256(b"{}")?,
            fixture: WorkspaceFixtureIdentity {
                id: xtask::vim_host_diagnostics_run::DIAGNOSTICS_FIXTURE_ID.to_string(),
                digest: fixture_digest(&fixture.root)?,
                expectation_set_id: CANONICAL_EXPECTATION_SET_ID.to_string(),
                expectation_set_digest: canonical_expectation_set_digest()?,
            },
            journey_selector: xtask::vim_host_diagnostics_run::DIAGNOSTICS_JOURNEY_SELECTOR
                .to_string(),
            platform: PlatformIdentity {
                os: "linux".to_string(),
                os_version: "test".to_string(),
                arch: "x86_64".to_string(),
            },
            registration_state: RegistrationState::ManualClientRegistration,
            timeout_ms: 60_000,
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
    let mut observation = event(sequence, kind);
    for (key, value) in details {
        observation.details.insert((*key).to_string(), (*value).to_string());
    }
    observation
}

/// Re-number a mutated stream's sequence field after reordering so the
/// ordering law is tested on lifecycle order alone, not on sequence
/// contiguity (which is validated first).
fn renumber(events: &mut [DriverEvent]) {
    for (index, item) in events.iter_mut().enumerate() {
        item.sequence = (index + 1) as u64;
    }
}

fn complete_diagnostics_events(digest: &str) -> Vec<DriverEvent> {
    vec![
        event(1, DriverEventKind::HostStarted),
        event(2, DriverEventKind::ClientLoaded),
        detail_event(
            3,
            DriverEventKind::RegistrationSelected,
            &[("cmd", "perllsp--stdio"), ("candidate_sha256", digest)],
        ),
        detail_event(4, DriverEventKind::FixtureOpened, &[("file", OPENED_FILE_REL)]),
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
                ("root_marker", ".perl-lsp.toml"),
                ("expected_root", EXPECTED_ROOT_REL),
                ("observed_root", EXPECTED_ROOT_REL),
                ("decoy_root", DECOY_ROOT_REL),
            ],
        ),
        detail_event(9, DriverEventKind::DiagnosticsObserved, &[("mode", "push")]),
        detail_event(
            10,
            DriverEventKind::DefectStateObserved,
            &[("state_source", "client_state"), ("errors", "1")],
        ),
        detail_event(11, DriverEventKind::DefectFixApplied, &[("edit_path", "buffer_did_change")]),
        detail_event(
            12,
            DriverEventKind::CurrentStateObserved,
            &[
                ("state_source", "client_state"),
                ("errors", "0"),
                ("discriminator_absent", "1"),
                ("barrier", "diagnostics_event_and_wire"),
            ],
        ),
        event(13, DriverEventKind::ShutdownStarted),
        event(14, DriverEventKind::ShutdownCompleted),
    ]
}

fn canonical_wire(root_uri_tail: &str) -> WireEvidence {
    WireEvidence {
        saw_initialize: true,
        saw_initialized: true,
        saw_publish_diagnostics: true,
        did_change_line: Some(7),
        publish_diagnostics_batches: vec![
            vim_host_runner::PublishDiagnosticsBatch {
                line_index: 5,
                uri_file: "main.pl".to_string(),
                diagnostics_count: 1,
                error_severity_count: 1,
                parser_code_count: 1,
            },
            vim_host_runner::PublishDiagnosticsBatch {
                line_index: 9,
                uri_file: "main.pl".to_string(),
                diagnostics_count: 0,
                error_severity_count: 0,
                parser_code_count: 0,
            },
        ],
        initialize_request: Some(serde_json::json!({
            "method": "initialize",
            "params": {
                "rootUri": format!("file:///hermetic-run/{root_uri_tail}"),
                "capabilities": {},
            }
        })),
        client_capabilities: Some(serde_json::json!({})),
        ..WireEvidence::default()
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

fn judgment_with(
    plan: &VimHostRunPlan,
    events: Vec<DriverEvent>,
    wire: WireEvidence,
    variant: DiagnosticsFixtureVariant,
) -> xtask::vim_host_diagnostics_run::DiagnosticsJudgment {
    let observation = observation_with(Some(0), CleanupResult::Pass, events);
    xtask::vim_host_diagnostics_run::evaluate_diagnostics_observation(
        plan,
        &observation,
        &wire,
        variant,
    )
}

// ---------------------------------------------------------------------------
// Fixture laws
// ---------------------------------------------------------------------------

#[test]
fn canonical_fixture_carries_marker_decoy_and_governed_defect() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let fixture = materialize_diagnostics_fixture(
        &dir.path().join("fixture"),
        DiagnosticsFixtureVariant::Canonical,
    )?;
    let project = fixture.root.join("workspace/project");
    ensure!(
        project.join(".perl-lsp.toml").is_file(),
        "the governed project must carry the #7762 marker"
    );
    ensure!(
        !fixture.root.join("workspace/.perl-lsp.toml").exists(),
        "the outer decoy must not carry a marker in the canonical variant"
    );
    ensure!(
        fixture.root.join(DECOY_SAME_NAME_FILE_REL).is_file(),
        "the same-named decoy file must exist at the outer root"
    );
    let main = fs::read_to_string(project.join("main.pl"))?;
    let defect_line = main.lines().nth(DEFECT_LINE - 1).unwrap_or_default();
    ensure!(
        defect_line == xtask::vim_host_diagnostics_run::DEFECT_LINE_TEXT,
        "the governed defect line must be exactly the Rust-authored defective text"
    );
    ensure!(
        !main.contains(xtask::vim_host_diagnostics_run::FIXED_LINE_TEXT),
        "the canonical fixture must not already contain the fixed line"
    );
    ensure!(project.join("lib/My/Widget.pm").is_file(), "the lib definition target exists");
    Ok(())
}

#[test]
fn negative_variants_change_exactly_the_governed_stimulus() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let canonical = materialize_diagnostics_fixture(
        &dir.path().join("canonical"),
        DiagnosticsFixtureVariant::Canonical,
    )?;
    let defect_absent = materialize_diagnostics_fixture(
        &dir.path().join("defect_absent"),
        DiagnosticsFixtureVariant::DefectAbsent,
    )?;
    let wrong_root = materialize_diagnostics_fixture(
        &dir.path().join("wrong_root"),
        DiagnosticsFixtureVariant::WrongRootDecoy,
    )?;

    // defect_absent: the same layout, the fixed line shipped.
    let main = fs::read_to_string(defect_absent.root.join("workspace/project/main.pl"))?;
    ensure!(
        main.lines().nth(DEFECT_LINE - 1).unwrap_or_default()
            == xtask::vim_host_diagnostics_run::FIXED_LINE_TEXT,
        "defect_absent must ship the already-fixed governed line"
    );
    ensure!(
        defect_absent.root.join("workspace/project/.perl-lsp.toml").is_file(),
        "defect_absent keeps the governed marker: only the defect is absent"
    );

    // wrong_root_decoy: the same broken file, marker moved to the decoy.
    let main = fs::read_to_string(wrong_root.root.join("workspace/project/main.pl"))?;
    ensure!(
        main.lines().nth(DEFECT_LINE - 1).unwrap_or_default()
            == xtask::vim_host_diagnostics_run::DEFECT_LINE_TEXT,
        "wrong_root_decoy keeps the governed defect: only the root is wrong"
    );
    ensure!(
        wrong_root.root.join("workspace/.perl-lsp.toml").is_file(),
        "wrong_root_decoy plants the marker at the decoy root"
    );
    ensure!(
        !wrong_root.root.join("workspace/project/.perl-lsp.toml").exists(),
        "wrong_root_decoy removes the governed marker so native resolution selects the decoy"
    );

    // Every variant is a different fixture identity: no variant can inherit
    // another's receipt.
    let digests: Vec<String> = [&canonical, &defect_absent, &wrong_root]
        .iter()
        .map(|fixture| fixture_digest(&fixture.root))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        digests[0] != digests[1] && digests[0] != digests[2] && digests[1] != digests[2],
        "fixture variants must be digest-distinct"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Driver-event laws for the diagnostics lifecycle
// ---------------------------------------------------------------------------

#[test]
fn complete_diagnostics_event_stream_validates_and_laws_reject_forgeries() -> Result<()> {
    let digest = format!("sha256:{}", "a".repeat(64));
    let events = complete_diagnostics_events(&digest);
    ensure!(
        validate_driver_events(&events, true).is_ok(),
        "the complete diagnostics journey must validate under the shared driver contract"
    );

    // The diagnostics lifecycle tier carries a strict order: a fix edit
    // before the defect observation, or a lifecycle barrier after shutdown,
    // is rejected (#10946 review repair: dedicated match arms enforce the
    // same ordering law the fallback arm enforces).
    let mut reordered = complete_diagnostics_events(&digest);
    let fix = reordered.remove(10);
    reordered.insert(9, fix);
    renumber(&mut reordered);
    ensure!(
        validate_driver_events(&reordered, true).is_err(),
        "a fix edit before the defect state observation must be rejected"
    );
    let mut after_shutdown = complete_diagnostics_events(&digest);
    let current = after_shutdown.remove(11);
    after_shutdown.push(current);
    renumber(&mut after_shutdown);
    ensure!(
        validate_driver_events(&after_shutdown, true).is_err(),
        "a currentness observation after shutdown must be rejected"
    );

    // A defect-state claim that does not come from the client's own state is
    // rejected before any judgment runs.
    let mut forged = complete_diagnostics_events(&digest);
    forged[9] = detail_event(
        10,
        DriverEventKind::DefectStateObserved,
        &[("state_source", "server_log"), ("errors", "1")],
    );
    ensure!(
        validate_driver_events(&forged, true).is_err(),
        "a server-log-sourced defect claim must be rejected by the event contract"
    );

    // An edit that did not ride the real buffer/didChange path is rejected.
    let mut forged = complete_diagnostics_events(&digest);
    forged[10] =
        detail_event(11, DriverEventKind::DefectFixApplied, &[("edit_path", "synthetic_request")]);
    ensure!(
        validate_driver_events(&forged, true).is_err(),
        "a synthetic edit path must be rejected by the event contract"
    );

    // A current-state claim without the proven-absent discriminator or the
    // barrier binding is rejected.
    let mut forged = complete_diagnostics_events(&digest);
    forged[11] = detail_event(
        12,
        DriverEventKind::CurrentStateObserved,
        &[("state_source", "client_state"), ("errors", "0"), ("barrier", "fixed_sleep")],
    );
    ensure!(
        validate_driver_events(&forged, true).is_err(),
        "a currentness claim without discriminator_absent is rejected"
    );
    let mut forged = complete_diagnostics_events(&digest);
    forged[11] = detail_event(
        12,
        DriverEventKind::CurrentStateObserved,
        &[
            ("state_source", "client_state"),
            ("errors", "1"),
            ("discriminator_absent", "0"),
            ("barrier", "diagnostics_event_and_wire"),
        ],
    );
    ensure!(
        validate_driver_events(&forged, true).is_err(),
        "a currentness claim that still carries the discriminator is rejected"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Wire-mining laws
// ---------------------------------------------------------------------------

#[test]
fn wire_evidence_mines_batches_and_did_change_ordering() -> Result<()> {
    let log = concat!(
        "12:00:01 {\"method\":\"initialize\",\"params\":{\"rootUri\":\"file:///w/workspace/project\",\"capabilities\":{}}}\n",
        "12:00:02 {\"method\":\"initialized\"}\n",
        "12:00:03 {\"method\":\"textDocument/didChange\",\"params\":{\"textDocument\":{\"uri\":\"file:///w/workspace/project/main.pl\"}}}\n",
        "12:00:04 {\"method\":\"textDocument/publishDiagnostics\",\"params\":{\"uri\":\"file:///w/workspace/project/main.pl\",\"diagnostics\":[{\"severity\":1,\"code\":\"PL002\",\"message\":\"syntax error\"}]}}\n",
        "12:00:05 {\"method\":\"textDocument/publishDiagnostics\",\"params\":{\"uri\":\"file:///w/workspace/project/main.pl\",\"diagnostics\":[]}}\n"
    );
    let evidence = vim_host_runner::extract_wire_evidence(log.as_bytes());
    ensure!(evidence.saw_initialize && evidence.saw_initialized);
    ensure!(evidence.did_change_line == Some(2), "the didChange line index is mined");
    ensure!(
        evidence.publish_diagnostics_batches.len() == 2,
        "both publishDiagnostics batches are mined"
    );
    let first = &evidence.publish_diagnostics_batches[0];
    ensure!(first.uri_file == "main.pl", "the batch binds the governed file token");
    ensure!(first.error_severity_count == 1 && first.parser_code_count == 1);
    let second = &evidence.publish_diagnostics_batches[1];
    ensure!(second.diagnostics_count == 0 && second.parser_code_count == 0);
    ensure!(
        second.line_index > evidence.did_change_line.unwrap_or(usize::MAX),
        "wire ordering distinguishes the post-edit batch"
    );

    // An unrelated diagnostic (severity 1 but a non-parser code) is mined
    // without ever satisfying the governed-defect discriminator.
    let unrelated = "12:00:01 {\"method\":\"textDocument/publishDiagnostics\",\"params\":{\"uri\":\"file:///w/workspace/project/main.pl\",\"diagnostics\":[{\"severity\":1,\"code\":\"PL100\",\"message\":\"missing strict\"}]}}\n";
    let evidence = vim_host_runner::extract_wire_evidence(unrelated.as_bytes());
    ensure!(!xtask::vim_host_diagnostics_run::governed_defect_batch(&evidence));
    Ok(())
}

// ---------------------------------------------------------------------------
// Judgment: positive path sanity
// ---------------------------------------------------------------------------

#[test]
fn canonical_honest_journey_passes_all_four_cells() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_diagnostics_plan(dir.path())?;
    let digest = plan.identity.candidate_artifact_sha256.clone();
    let judgment = judgment_with(
        &plan,
        complete_diagnostics_events(&digest),
        canonical_wire("workspace/project"),
        DiagnosticsFixtureVariant::Canonical,
    );
    ensure!(
        judgment.result == ObservationResult::Pass,
        "an honest complete journey must pass: {:?}",
        judgment.cells
    );
    ensure!(judgment.failure_class.is_none());
    for cell in
        [CELL_BOOTSTRAP, CELL_ROOT, CELL_DIAGNOSTICS, CELL_CURRENTNESS, CELL_BASELINE_CLEANUP]
    {
        ensure!(
            judgment.cells.get(cell) == Some(&ObservationResult::Pass),
            "cell {cell} must pass on the honest journey"
        );
    }
    ensure!(
        !judgment.wrong_initialize_root,
        "the initialize rootUri agrees with the governed root"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Judgment: the red-first negative controls
// ---------------------------------------------------------------------------

/// Wrong-root control (#10946 root discriminator): the driver observed the
/// decoy root, failed with the typed reason, and skipped the diagnostics
/// lifecycle. The slice must reject it even though every other barrier was
/// on the canonical journey.
#[test]
fn wrong_root_journey_is_rejected_even_with_diagnostics_available() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_diagnostics_plan(dir.path())?;
    let digest = plan.identity.candidate_artifact_sha256.clone();
    let mut events = complete_diagnostics_events(&digest);
    events[7] = detail_event(
        8,
        DriverEventKind::RootSelected,
        &[
            ("root_source", "activation_root_marker"),
            ("root_marker", ".perl-lsp.toml"),
            ("expected_root", EXPECTED_ROOT_REL),
            ("observed_root", DECOY_ROOT_REL),
            ("decoy_root", DECOY_ROOT_REL),
        ],
    );
    // The server started from the wrong root; the lifecycle barriers are
    // skipped (the driver fails at the root check), and diagnostics that
    // might still have appeared cannot rescue the run.
    events.truncate(9);
    events.push(detail_event(10, DriverEventKind::ShutdownStarted, &[("server_stopping", "1")]));
    events.push(detail_event(11, DriverEventKind::ShutdownCompleted, &[("server_exited", "1")]));
    events.push(detail_event(12, DriverEventKind::DriverFailed, &[("reason", "root_mismatch")]));
    let judgment = judgment_with(
        &plan,
        events,
        canonical_wire(DECOY_ROOT_REL),
        DiagnosticsFixtureVariant::WrongRootDecoy,
    );
    ensure!(
        judgment.result == ObservationResult::Fail,
        "a server answering from the decoy root must fail the slice"
    );
    ensure!(
        judgment.cells.get(CELL_ROOT) != Some(&ObservationResult::Pass),
        "the root cell cannot pass on the decoy root"
    );
    ensure!(
        judgment.cells.get(CELL_DIAGNOSTICS) != Some(&ObservationResult::Pass),
        "diagnostics cells cannot pass without the lifecycle barriers"
    );
    ensure!(judgment.driver_failure_reason.as_deref() == Some("root_mismatch"));
    Ok(())
}

/// Defect-absent control: the governed defect never became visible; the run
/// fails with the typed reason and the diagnostics cell stays red.
#[test]
fn defect_absent_journey_is_rejected() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_diagnostics_plan(dir.path())?;
    let digest = plan.identity.candidate_artifact_sha256.clone();
    let mut events = complete_diagnostics_events(&digest);
    // The defect-state barrier never emitted; the driver failed fast.
    events.truncate(10);
    events.remove(9);
    events.push(detail_event(10, DriverEventKind::ShutdownStarted, &[("server_stopping", "1")]));
    events.push(detail_event(11, DriverEventKind::ShutdownCompleted, &[("server_exited", "1")]));
    events.push(detail_event(
        12,
        DriverEventKind::DriverFailed,
        &[("reason", "defect_state_absent")],
    ));
    // The wire carries only an empty push: the server honestly published a
    // clean state.
    let mut wire = canonical_wire(EXPECTED_ROOT_REL);
    wire.publish_diagnostics_batches = vec![vim_host_runner::PublishDiagnosticsBatch {
        line_index: 5,
        uri_file: "main.pl".to_string(),
        diagnostics_count: 0,
        error_severity_count: 0,
        parser_code_count: 0,
    }];
    let judgment = judgment_with(&plan, events, wire, DiagnosticsFixtureVariant::DefectAbsent);
    ensure!(
        judgment.result == ObservationResult::Fail,
        "an absent governed defect must fail the slice"
    );
    ensure!(
        judgment.cells.get(CELL_DIAGNOSTICS) != Some(&ObservationResult::Pass),
        "the diagnostics cell cannot pass without the governed defect"
    );
    ensure!(
        judgment.driver_failure_reason.as_deref() == Some("defect_state_absent"),
        "the typed negative reason is retained"
    );
    Ok(())
}

/// Server-log-only control: the client state event claims the defect, but
/// the client's own wire record carries no publishDiagnostics batch at all.
/// A claim without client-side corroboration fails closed — this is also the
/// synthesized-receipt shape.
#[test]
fn client_claim_without_wire_record_cannot_pass() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_diagnostics_plan(dir.path())?;
    let digest = plan.identity.candidate_artifact_sha256.clone();
    let mut wire = canonical_wire(EXPECTED_ROOT_REL);
    wire.publish_diagnostics_batches = Vec::new();
    wire.saw_publish_diagnostics = false;
    let judgment = judgment_with(
        &plan,
        complete_diagnostics_events(&digest),
        wire,
        DiagnosticsFixtureVariant::Canonical,
    );
    ensure!(
        judgment.result != ObservationResult::Pass,
        "a diagnostics claim the client's own wire never recorded cannot pass"
    );
    ensure!(
        judgment.cells.get(CELL_DIAGNOSTICS) != Some(&ObservationResult::Pass),
        "the diagnostics cell requires the client wire record"
    );
    Ok(())
}

/// Stale-reuse control: the post-edit state is claimed, but every wire batch
/// sits before the didChange — a reused pre-edit generation can never
/// satisfy currentness.
#[test]
fn reused_pre_edit_push_cannot_satisfy_currentness() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_diagnostics_plan(dir.path())?;
    let digest = plan.identity.candidate_artifact_sha256.clone();
    let mut wire = canonical_wire(EXPECTED_ROOT_REL);
    wire.did_change_line = Some(9);
    wire.publish_diagnostics_batches = vec![
        vim_host_runner::PublishDiagnosticsBatch {
            line_index: 3,
            uri_file: "main.pl".to_string(),
            diagnostics_count: 1,
            error_severity_count: 1,
            parser_code_count: 1,
        },
        vim_host_runner::PublishDiagnosticsBatch {
            line_index: 5,
            uri_file: "main.pl".to_string(),
            diagnostics_count: 0,
            error_severity_count: 0,
            parser_code_count: 0,
        },
    ];
    let judgment = judgment_with(
        &plan,
        complete_diagnostics_events(&digest),
        wire,
        DiagnosticsFixtureVariant::Canonical,
    );
    ensure!(
        judgment.result != ObservationResult::Pass,
        "a pre-edit push reused as post-edit evidence cannot pass"
    );
    ensure!(
        judgment.cells.get(CELL_CURRENTNESS) != Some(&ObservationResult::Pass),
        "the currentness cell requires a wire batch ordered after the didChange"
    );
    Ok(())
}

/// Unrelated-diagnostic control: an error-severity diagnostic without a
/// parser-family code never satisfies the governed-defect cell.
#[test]
fn unrelated_diagnostic_never_satisfies_the_governed_defect() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_diagnostics_plan(dir.path())?;
    let digest = plan.identity.candidate_artifact_sha256.clone();
    let mut wire = canonical_wire(EXPECTED_ROOT_REL);
    wire.publish_diagnostics_batches = vec![
        vim_host_runner::PublishDiagnosticsBatch {
            line_index: 5,
            uri_file: "main.pl".to_string(),
            diagnostics_count: 1,
            error_severity_count: 1,
            parser_code_count: 0,
        },
        vim_host_runner::PublishDiagnosticsBatch {
            line_index: 9,
            uri_file: "main.pl".to_string(),
            diagnostics_count: 0,
            error_severity_count: 0,
            parser_code_count: 0,
        },
    ];
    let judgment = judgment_with(
        &plan,
        complete_diagnostics_events(&digest),
        wire,
        DiagnosticsFixtureVariant::Canonical,
    );
    ensure!(
        judgment.result != ObservationResult::Pass,
        "an unrelated diagnostic accepted as the governed defect cannot pass"
    );
    ensure!(
        judgment.cells.get(CELL_DIAGNOSTICS) != Some(&ObservationResult::Pass),
        "the governed-defect cell requires the parser-code discriminator"
    );
    Ok(())
}

/// Initialize-root consistency: a driver-observed canonical root with an
/// initialize rootUri pointing elsewhere (a decoy server identity) fails.
#[test]
fn initialize_root_uri_disagreement_cannot_pass() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_diagnostics_plan(dir.path())?;
    let digest = plan.identity.candidate_artifact_sha256.clone();
    let judgment = judgment_with(
        &plan,
        complete_diagnostics_events(&digest),
        canonical_wire("some/other/root"),
        DiagnosticsFixtureVariant::Canonical,
    );
    ensure!(judgment.wrong_initialize_root, "the rootUri disagreement must be detected");
    ensure!(
        judgment.cells.get(CELL_ROOT) != Some(&ObservationResult::Pass),
        "the root cell cannot pass on an inconsistent root identity"
    );
    ensure!(judgment.result != ObservationResult::Pass);
    Ok(())
}

/// Negative-variant oracle guard: a negative fixture variant whose journey
/// reaches an otherwise-complete pass is reported as a failure, never a
/// green run on a wrong fixture.
#[test]
fn negative_variant_reaching_a_pass_is_reported_as_fail() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_diagnostics_plan(dir.path())?;
    let digest = plan.identity.candidate_artifact_sha256.clone();
    let judgment = judgment_with(
        &plan,
        complete_diagnostics_events(&digest),
        canonical_wire(EXPECTED_ROOT_REL),
        DiagnosticsFixtureVariant::DefectAbsent,
    );
    ensure!(
        judgment.result == ObservationResult::Fail,
        "a pass-shaped journey on a negative variant is an oracle violation and must fail"
    );
    Ok(())
}

/// Leak control: a complete journey with an orderly exit that still leaked
/// the candidate fails with the cleanup class.
#[test]
fn leaked_candidate_fails_the_cleanup_cell() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_diagnostics_plan(dir.path())?;
    let digest = plan.identity.candidate_artifact_sha256.clone();
    let observation =
        observation_with(Some(0), CleanupResult::Fail, complete_diagnostics_events(&digest));
    let judgment = xtask::vim_host_diagnostics_run::evaluate_diagnostics_observation(
        &plan,
        &observation,
        &canonical_wire(EXPECTED_ROOT_REL),
        DiagnosticsFixtureVariant::Canonical,
    );
    ensure!(judgment.result == ObservationResult::Fail, "a leak is a failure");
    ensure!(
        judgment.cells.get(CELL_BASELINE_CLEANUP) == Some(&ObservationResult::Fail),
        "the cleanup cell fails on an observed survivor"
    );
    ensure!(judgment.failure_class == Some(xtask::editor_client_compat::FailureClass::Cleanup));
    Ok(())
}

// ---------------------------------------------------------------------------
// Receipt composition
// ---------------------------------------------------------------------------

#[test]
fn diagnostics_receipt_composes_with_catalog_cells_and_binds_its_plan() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_diagnostics_plan(dir.path())?;
    let digest = plan.identity.candidate_artifact_sha256.clone();
    let observation =
        observation_with(Some(0), CleanupResult::Pass, complete_diagnostics_events(&digest));
    let wire = canonical_wire(EXPECTED_ROOT_REL);
    let judgment = xtask::vim_host_diagnostics_run::evaluate_diagnostics_observation(
        &plan,
        &observation,
        &wire,
        DiagnosticsFixtureVariant::Canonical,
    );
    ensure!(judgment.result == ObservationResult::Pass);

    let journey =
        xtask::vim_host_diagnostics_run::diagnostics_journey(&observation, &wire, &judgment);
    let mut ids = std::collections::BTreeSet::new();
    for cell in &journey {
        ensure!(ids.insert(cell.id.as_str()), "duplicate journey cell id {}", cell.id);
        if cell.result == ObservationResult::Pass {
            ensure!(cell.observed, "a passing cell must be observed: {}", cell.id);
        }
    }
    for cell in
        [CELL_BOOTSTRAP, CELL_ROOT, CELL_DIAGNOSTICS, CELL_CURRENTNESS, CELL_BASELINE_CLEANUP]
    {
        ensure!(ids.contains(cell), "the receipt journey must cite catalog cell {cell}");
    }

    let capabilities =
        vim_host_runner::capabilities_from_wire_evidence(&wire, Some(digest.clone()))?;
    let diagnostics = vim_host_runner::diagnostics_from_wire_evidence(&wire);
    let mut receipt = vim_host_runner::build_receipt(
        &plan,
        &observation,
        capabilities,
        diagnostics,
        journey,
        judgment.result,
        None,
        vec!["contract-test limitations".to_string()],
        "#10946 contract test receipt".to_string(),
    );
    receipt.artifacts = vec![
        evidence_artifact(xtask::editor_client_compat::ArtifactKind::ClientLog),
        evidence_artifact(xtask::editor_client_compat::ArtifactKind::ServerStderr),
        evidence_artifact(xtask::editor_client_compat::ArtifactKind::CapabilitySnapshot),
        evidence_artifact(xtask::editor_client_compat::ArtifactKind::ProcessLedger),
    ];
    ensure!(receipt.validate().is_ok(), "the composed receipt satisfies the generic schema");
    ensure!(
        vim_host_runner::validate_receipt_binding(&receipt, &plan).is_ok(),
        "the receipt binds its own diagnostics run plan"
    );

    // A receipt from this journey cannot satisfy another plan: the fixture
    // digest is variant-bound.
    let other_root = dir.path().join("other-fixture");
    let other_fixture =
        materialize_diagnostics_fixture(&other_root, DiagnosticsFixtureVariant::DefectAbsent)?;
    let mut other_plan = scratch_diagnostics_plan(&dir.path().join("other-plan"))?;
    other_plan.identity.fixture.digest = fixture_digest(&other_fixture.root)?;
    ensure!(
        vim_host_runner::validate_receipt_binding(&receipt, &other_plan).is_err(),
        "a receipt from another fixture variant is stale evidence"
    );
    Ok(())
}

fn evidence_artifact(
    kind: xtask::editor_client_compat::ArtifactKind,
) -> xtask::editor_client_compat::EvidenceArtifact {
    xtask::editor_client_compat::EvidenceArtifact {
        kind,
        id: format!("vim/{kind:?}"),
        sha256: format!("sha256:{}", "a".repeat(64)),
    }
}

// ---------------------------------------------------------------------------
// Static thinness laws for the diagnostics driver and the extended adapter
// ---------------------------------------------------------------------------

#[test]
fn diagnostics_driver_and_adapter_stay_thin_and_native() -> Result<()> {
    let adapter =
        fs::read_to_string(repo_root().join("scripts/test/vim-clients/vim-lsp-adapter.vim"))?;
    let driver =
        fs::read_to_string(repo_root().join("scripts/test/vim-host-diagnostics-driver.vim"))?;
    for (label, source) in [("adapter", &adapter), ("driver", &driver)] {
        for forbidden in ["setf ", "setlocal filetype", "set filetype", "filetype=perl"] {
            ensure!(
                !source.contains(forbidden),
                "{label} contains forbidden Vimscript `{forbidden}`: no filetype forcing (#7762)"
            );
        }
        ensure!(
            !source.contains("system(")
                && !source.contains("job_start")
                && !source.contains("term_start"),
            "{label} must not spawn processes; the Rust supervisor owns supervision"
        );
        ensure!(
            !source.contains("expand('$PERLLSP_VIM_HOST"),
            "{label} must read the run contract through getenv(), not expand()"
        );
    }
    ensure!(
        !adapter.contains("writefile(") && !adapter.contains("json_encode("),
        "adapter must not write artifacts; the Rust supervisor owns receipts"
    );
    // The diagnostics state observation must ride the classified public
    // surface, never a synthesized protocol substitute.
    ensure!(
        adapter.contains("lsp#get_buffer_diagnostics_counts()"),
        "the adapter must observe diagnostics through the classified public counts surface"
    );
    ensure!(
        !adapter.contains("lsp#diagnostics#"),
        "the adapter must not reach into non-classified internal diagnostics autoloads"
    );
    // The fix edit rides the real buffer/change path.
    ensure!(
        adapter.contains("doautocmd <nomodeline> TextChanged"),
        "the adapter flushes edits through the real TextChanged -> didChange path"
    );
    // The driver applies the Rust-authored expectation only.
    ensure!(
        driver.contains("VimLspHostSetLineAndFlush"),
        "the governed fix edit must ride the adapter's real buffer-edit path"
    );
    Ok(())
}
