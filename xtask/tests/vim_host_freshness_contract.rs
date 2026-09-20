// #11390 freshness-generations scenario contract tests.
//
// Red-first law: the negative controls below were authored and proven to
// reject before the positive host journey ran. Every discriminating control
// feeds the judgment the exact evidence a false green would need (a watcher
// surface the client never exposed, an old generation repopulating state, a
// settings push that never reached the wire, a wrong-root same-name source
// supplying the result, a config generation that never restarted, a negative
// variant that wrongly passes) and asserts the slice refuses it. Real-editor
// launches are not unit tests: the canonical journeys run in the dedicated
// workflow (`.github/workflows/vim-hermetic-host.yml`).

use anyhow::{Result, ensure};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use xtask::editor_client_compat::{
    CANONICAL_EXPECTATION_SET_ID, CleanupResult, EvidenceStage, ObservationResult,
    PlatformIdentity, RegistrationState, WorkspaceFixtureIdentity,
    canonical_expectation_set_digest, fixture_digest,
};
use xtask::vim_host_freshness_run::freshness_journey;
use xtask::vim_host_freshness_run::{
    CELL_CLIENT_SETTINGS, CELL_EXTERNAL_SOURCE, CELL_PROJECT_CONFIG, CELL_PROVIDER_OWNERSHIP,
    CELL_ROUTE, CELL_STALE_GENERATION, CONFIG_TOKEN, FreshnessBatch, FreshnessFixtureVariant,
    FreshnessWire, MAIN_TOKEN, ROOT_MARKER, SETTINGS_TOKEN, evaluate_freshness_observation,
    extract_freshness_wire, materialize_freshness_fixture,
};
use xtask::vim_host_run::vim_host_runner::{
    self, DRIVER_SCHEMA_VERSION, DriverEvent, DriverEventKind, RUN_PLAN_SCHEMA_VERSION,
    VimHostPaths, VimHostRunIdentity, VimHostRunPlan, WireEvidence, validate_driver_events,
};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(1).unwrap_or(Path::new(".")).to_path_buf()
}

// ---------------------------------------------------------------------------
// Scratch plan and evidence helpers
// ---------------------------------------------------------------------------

fn scratch_freshness_plan(root: &Path) -> Result<VimHostRunPlan> {
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
        materialize_freshness_fixture(&root.join("fixture"), FreshnessFixtureVariant::Canonical)?;
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
                id: "vim_vim_lsp_freshness_generations_v1".to_string(),
                digest: fixture_digest(&fixture.root)?,
                expectation_set_id: CANONICAL_EXPECTATION_SET_ID.to_string(),
                expectation_set_digest: canonical_expectation_set_digest()?,
            },
            journey_selector: "vim_vim_lsp_freshness_generations.v1".to_string(),
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

/// The complete canonical freshness journey event stream (42 barriers), in
/// the exact order the driver emits them.
fn complete_freshness_events(digest: &str) -> Vec<DriverEvent> {
    let generation = |index: &'static str,
                      token: &'static str,
                      errors: &'static str,
                      warnings: &'static str|
     -> Vec<(&'static str, &'static str)> {
        vec![
            ("generation_index", index),
            ("generation", token),
            ("state_source", "client_state"),
            ("barrier", "diagnostics_event_and_wire"),
            ("errors", errors),
            ("warnings", warnings),
        ]
    };
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
                ("root_marker", ROOT_MARKER),
                ("expected_root", "workspace/project"),
                ("observed_root", "workspace/project"),
                ("decoy_root", "workspace"),
            ],
        ),
        detail_event(9, DriverEventKind::DiagnosticsObserved, &[("mode", "push")]),
        detail_event(
            10,
            DriverEventKind::GenerationCurrentObserved,
            &generation("1", "g1_clean", "0", "0"),
        ),
        detail_event(
            11,
            DriverEventKind::ExternalMutationApplied,
            &[
                ("mutation_index", "1"),
                ("mutation", "in_place"),
                ("target", "governed"),
                ("disk_generation", "g2_defect"),
            ],
        ),
        detail_event(
            12,
            DriverEventKind::StaleGenerationHeld,
            &[
                ("hold_index", "1"),
                ("held_generation", "g2_defect"),
                ("current_generation", "g1_clean"),
                ("window_ms", "5000"),
                ("state_held", "1"),
            ],
        ),
        detail_event(
            13,
            DriverEventKind::ClientMaterializationApplied,
            &[
                ("materialization_index", "1"),
                ("materialization", "client_close_reopen"),
                ("picks_generation", "g2_defect"),
            ],
        ),
        detail_event(
            14,
            DriverEventKind::GenerationCurrentObserved,
            &generation("2", "g2_defect", "1", "0"),
        ),
        detail_event(
            15,
            DriverEventKind::ExternalMutationApplied,
            &[
                ("mutation_index", "2"),
                ("mutation", "atomic_replace"),
                ("target", "governed"),
                ("disk_generation", "g3_old_clean"),
            ],
        ),
        detail_event(
            16,
            DriverEventKind::StaleGenerationHeld,
            &[
                ("hold_index", "2"),
                ("held_generation", "g3_old_clean"),
                ("current_generation", "g2_defect"),
                ("window_ms", "5000"),
                ("state_held", "1"),
            ],
        ),
        detail_event(
            17,
            DriverEventKind::ClientMaterializationApplied,
            &[
                ("materialization_index", "2"),
                ("materialization", "client_close_reopen"),
                ("picks_generation", "g3_old_clean"),
            ],
        ),
        detail_event(
            18,
            DriverEventKind::GenerationCurrentObserved,
            &generation("3", "g3_old_clean", "0", "0"),
        ),
        detail_event(
            19,
            DriverEventKind::ExternalMutationApplied,
            &[
                ("mutation_index", "3"),
                ("mutation", "in_place"),
                ("target", "decoy"),
                ("disk_generation", "decoy_defect"),
            ],
        ),
        detail_event(
            20,
            DriverEventKind::ClientMaterializationApplied,
            &[
                ("materialization_index", "3"),
                ("materialization", "client_close_reopen"),
                ("picks_generation", "g3_old_clean"),
            ],
        ),
        detail_event(
            21,
            DriverEventKind::GenerationCurrentObserved,
            &generation("4", "g3_decoy_control", "0", "0"),
        ),
        detail_event(
            22,
            DriverEventKind::GenerationCurrentObserved,
            &generation("5", "settings_pl701_present", "0", "1"),
        ),
        detail_event(
            23,
            DriverEventKind::ClientMaterializationApplied,
            &[
                ("materialization_index", "4"),
                ("materialization", "client_close_reopen"),
                ("picks_generation", "settings_control_present"),
            ],
        ),
        detail_event(
            24,
            DriverEventKind::GenerationCurrentObserved,
            &generation("6", "settings_control_present", "0", "1"),
        ),
        detail_event(
            25,
            DriverEventKind::ClientMaterializationApplied,
            &[
                ("materialization_index", "5"),
                ("materialization", "settings_push"),
                ("picks_generation", "settings_post_push"),
            ],
        ),
        detail_event(
            26,
            DriverEventKind::ClientMaterializationApplied,
            &[
                ("materialization_index", "6"),
                ("materialization", "client_close_reopen"),
                ("picks_generation", "settings_push_cleared"),
            ],
        ),
        detail_event(
            27,
            DriverEventKind::GenerationCurrentObserved,
            &generation("7", "settings_push_cleared", "0", "0"),
        ),
        detail_event(
            28,
            DriverEventKind::GenerationCurrentObserved,
            &generation("8", "config_critic_present", "0", "1"),
        ),
        detail_event(
            29,
            DriverEventKind::ExternalMutationApplied,
            &[
                ("mutation_index", "4"),
                ("mutation", "in_place"),
                ("target", "project_config"),
                ("disk_generation", "toml_exclude_created"),
            ],
        ),
        detail_event(
            30,
            DriverEventKind::StaleGenerationHeld,
            &[
                ("hold_index", "3"),
                ("held_generation", "toml_exclude_created"),
                ("current_generation", "config_critic_present"),
                ("window_ms", "5000"),
                ("state_held", "1"),
            ],
        ),
        detail_event(
            31,
            DriverEventKind::ClientMaterializationApplied,
            &[
                ("materialization_index", "7"),
                ("materialization", "server_restart"),
                ("picks_generation", "config_exclude_active"),
            ],
        ),
        detail_event(
            32,
            DriverEventKind::GenerationCurrentObserved,
            &generation("9", "config_exclude_active", "0", "0"),
        ),
        detail_event(
            33,
            DriverEventKind::ExternalMutationApplied,
            &[
                ("mutation_index", "5"),
                ("mutation", "atomic_replace"),
                ("target", "project_config"),
                ("disk_generation", "toml_malformed"),
            ],
        ),
        detail_event(
            34,
            DriverEventKind::StaleGenerationHeld,
            &[
                ("hold_index", "4"),
                ("held_generation", "toml_malformed"),
                ("current_generation", "config_exclude_active"),
                ("window_ms", "5000"),
                ("state_held", "1"),
            ],
        ),
        detail_event(
            35,
            DriverEventKind::ClientMaterializationApplied,
            &[
                ("materialization_index", "8"),
                ("materialization", "server_restart"),
                ("picks_generation", "config_malformed_rejected"),
            ],
        ),
        detail_event(
            36,
            DriverEventKind::GenerationCurrentObserved,
            &generation("10", "config_malformed_rejected", "0", "1"),
        ),
        detail_event(
            37,
            DriverEventKind::ExternalMutationApplied,
            &[
                ("mutation_index", "6"),
                ("mutation", "atomic_replace"),
                ("target", "project_config"),
                ("disk_generation", "toml_exclude_repaired"),
            ],
        ),
        detail_event(
            38,
            DriverEventKind::StaleGenerationHeld,
            &[
                ("hold_index", "5"),
                ("held_generation", "toml_exclude_repaired"),
                ("current_generation", "config_malformed_rejected"),
                ("window_ms", "5000"),
                ("state_held", "1"),
            ],
        ),
        detail_event(
            39,
            DriverEventKind::ClientMaterializationApplied,
            &[
                ("materialization_index", "9"),
                ("materialization", "server_restart"),
                ("picks_generation", "config_exclude_repaired"),
            ],
        ),
        detail_event(
            40,
            DriverEventKind::GenerationCurrentObserved,
            &generation("11", "config_exclude_repaired", "0", "0"),
        ),
        event(41, DriverEventKind::ShutdownStarted),
        event(42, DriverEventKind::ShutdownCompleted),
    ]
}

fn batch(
    line: usize,
    token: &str,
    errors: usize,
    warnings: usize,
    pl701: usize,
    critic: usize,
) -> FreshnessBatch {
    FreshnessBatch {
        line_index: line,
        uri_file: token.to_string(),
        error_severity_count: errors,
        warning_severity_count: warnings,
        pl701_count: pl701,
        critic_policy_count: critic,
    }
}

/// The canonical freshness wire: four main windows (clean, defect, restored
/// clean, decoy control), three settings windows (baseline warning, control
/// warning, post-push clean), four config windows (critic warning, excluded
/// clean, malformed warning, repaired clean), two configuration pushes (the
/// registration's startup push and the journey's settings push), and four
/// initialize requests (the initial server plus three restarts).
fn canonical_freshness_wire() -> FreshnessWire {
    FreshnessWire {
        batches: vec![
            batch(2, MAIN_TOKEN, 0, 0, 0, 0),
            batch(12, MAIN_TOKEN, 1, 0, 0, 0),
            batch(20, MAIN_TOKEN, 0, 0, 0, 0),
            batch(27, MAIN_TOKEN, 0, 0, 0, 0),
            batch(32, SETTINGS_TOKEN, 0, 1, 1, 0),
            batch(36, SETTINGS_TOKEN, 0, 1, 1, 0),
            batch(42, SETTINGS_TOKEN, 0, 0, 0, 0),
            batch(52, CONFIG_TOKEN, 0, 1, 0, 1),
            batch(62, CONFIG_TOKEN, 0, 0, 0, 0),
            batch(72, CONFIG_TOKEN, 0, 1, 0, 1),
            batch(82, CONFIG_TOKEN, 0, 0, 0, 0),
        ],
        did_open_lines: vec![
            (1, MAIN_TOKEN.to_string()),
            (10, MAIN_TOKEN.to_string()),
            (18, MAIN_TOKEN.to_string()),
            (25, MAIN_TOKEN.to_string()),
            (30, SETTINGS_TOKEN.to_string()),
            (35, SETTINGS_TOKEN.to_string()),
            (40, SETTINGS_TOKEN.to_string()),
            (50, CONFIG_TOKEN.to_string()),
            (60, CONFIG_TOKEN.to_string()),
            (70, CONFIG_TOKEN.to_string()),
            (80, CONFIG_TOKEN.to_string()),
        ],
        did_close_lines: Vec::new(),
        did_change_configuration_lines: vec![3, 38],
        initialize_count: 4,
    }
}

fn canonical_wire() -> WireEvidence {
    WireEvidence {
        saw_initialize: true,
        saw_initialized: true,
        saw_publish_diagnostics: true,
        initialize_request: Some(serde_json::json!({
            "method": "initialize",
            "params": {
                "rootUri": "file:///hermetic-run/workspace/project",
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

fn canonical_judgment(
    plan: &VimHostRunPlan,
    events: Vec<DriverEvent>,
    wire: WireEvidence,
    freshness_wire: FreshnessWire,
    variant: FreshnessFixtureVariant,
) -> xtask::vim_host_freshness_run::FreshnessJudgment {
    let observation = observation_with(Some(0), CleanupResult::Pass, events);
    evaluate_freshness_observation(plan, &observation, &wire, &freshness_wire, variant)
}

fn plan_digest(plan: &VimHostRunPlan) -> String {
    plan.identity.candidate_artifact_sha256.clone()
}

// ---------------------------------------------------------------------------
// Fixture laws
// ---------------------------------------------------------------------------

#[test]
fn canonical_fixture_carries_cpanfile_marker_decoy_and_clean_generations() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let fixture = materialize_freshness_fixture(
        &dir.path().join("fixture"),
        FreshnessFixtureVariant::Canonical,
    )?;
    let project = fixture.root.join("workspace/project");
    ensure!(
        project.join(ROOT_MARKER).is_file(),
        "the governed project must carry the cpanfile root marker"
    );
    ensure!(
        !fixture.root.join("workspace/cpanfile").exists(),
        "the outer decoy must not carry a marker in the canonical variant"
    );
    ensure!(
        !project.join(".perl-lsp.toml").exists(),
        "no project config may exist initially: the config generation is created by the journey"
    );
    ensure!(
        fixture.root.join("workspace/main.pl").is_file(),
        "the same-named decoy file must exist at the outer root"
    );
    ensure!(
        project.join("lib/My/Widget.pm").is_file()
            && project.join("vendor/My/Vendor/Extra.pm").is_file(),
        "both module roots exist: lib through the registration channel, vendor only through \
         the settings channel"
    );
    let main = fs::read_to_string(project.join("main.pl"))?;
    ensure!(
        main.contains("my $value = My::Widget::answer();"),
        "the governed source ships the clean generation"
    );
    ensure!(
        !main.contains("My::Widget::answer()\n"),
        "the governed source must not ship the defect"
    );
    let settings = fs::read_to_string(project.join("settings.pl"))?;
    ensure!(
        settings.contains("use My::Vendor::Extra;"),
        "the settings file must depend on the vendor-resolvable module"
    );
    let config = fs::read_to_string(project.join("config.pl"))?;
    ensure!(
        config.contains("eval { handled(); 1; };"),
        "the config file must carry the block eval"
    );
    ensure!(
        config.contains("if ($@) {"),
        "the config file must carry the immediate $@ condition (the distinct          stale-dollar-at discriminator)"
    );
    Ok(())
}

#[test]
fn freshness_variants_change_exactly_the_governed_stimulus() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let canonical = materialize_freshness_fixture(
        &dir.path().join("canonical"),
        FreshnessFixtureVariant::Canonical,
    )?;
    let wrong_root = materialize_freshness_fixture(
        &dir.path().join("wrong_root"),
        FreshnessFixtureVariant::WrongRootDecoy,
    )?;
    let claimed = materialize_freshness_fixture(
        &dir.path().join("claimed"),
        FreshnessFixtureVariant::LiveReloadClaimed,
    )?;
    let ambient = materialize_freshness_fixture(
        &dir.path().join("ambient"),
        FreshnessFixtureVariant::AmbientPathOnly,
    )?;

    // wrong_root_decoy: the marker moves to the decoy root; everything else
    // stays canonical.
    ensure!(
        wrong_root.root.join("workspace/cpanfile").is_file(),
        "wrong_root_decoy plants the marker at the decoy root"
    );
    ensure!(
        !wrong_root.root.join("workspace/project/cpanfile").exists(),
        "wrong_root_decoy removes the governed marker so native resolution selects the decoy"
    );

    // live_reload_claimed and ambient_path_only ship the canonical fixture:
    // only the journey's claim (or the pushed channel content) differs.
    ensure!(
        claimed.root.join("workspace/project/cpanfile").is_file()
            && ambient.root.join("workspace/project/cpanfile").is_file(),
        "the claim variants keep the governed marker"
    );

    let digests: Vec<String> = [&canonical, &wrong_root, &claimed, &ambient]
        .iter()
        .map(|fixture| fixture_digest(&fixture.root))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        digests[0] != digests[1],
        "the wrong_root fixture must be digest-distinct from canonical"
    );
    ensure!(
        digests[0] == digests[2] && digests[0] == digests[3],
        "the claim variants ship exactly the canonical fixture bytes"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Driver-event laws for the repeating freshness kinds
// ---------------------------------------------------------------------------

#[test]
fn complete_freshness_event_stream_validates() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_freshness_plan(&dir.path().join("scratch"))?;
    let events = complete_freshness_events(&plan_digest(&plan));
    ensure!(
        validate_driver_events(&events, true).is_ok(),
        "the complete freshness journey must validate under the shared driver contract"
    );
    Ok(())
}

#[test]
fn freshness_event_repetition_laws_reject_disorder_and_forgeries() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_freshness_plan(&dir.path().join("scratch"))?;
    let digest = plan_digest(&plan);

    // A repeated index is rejected (monotone, gap-free).
    let mut duplicated = complete_freshness_events(&digest);
    duplicated[13] = duplicated[9].clone();
    renumber(&mut duplicated);
    ensure!(
        validate_driver_events(&duplicated, true).is_err(),
        "a repeated generation index must be rejected"
    );

    // A skipped index is rejected.
    let mut skipped = complete_freshness_events(&digest);
    skipped[13].details.insert("generation_index".to_string(), "3".to_string());
    renumber(&mut skipped);
    ensure!(
        validate_driver_events(&skipped, true).is_err(),
        "a skipped generation index must be rejected"
    );

    // A hold without the honest window is rejected.
    let mut tiny_window = complete_freshness_events(&digest);
    tiny_window[11].details.insert("window_ms".to_string(), "10".to_string());
    ensure!(
        validate_driver_events(&tiny_window, true).is_err(),
        "a stale hold without a real observation window must be rejected"
    );

    // A hold whose state claim did not hold is rejected.
    let mut broken_hold = complete_freshness_events(&digest);
    broken_hold[11].details.insert("state_held".to_string(), "0".to_string());
    ensure!(
        validate_driver_events(&broken_hold, true).is_err(),
        "a hold whose state claim did not hold must be rejected"
    );

    // A mutation without an exact mode is rejected.
    let mut forged = complete_freshness_events(&digest);
    forged[10].details.insert("mutation".to_string(), "synthetic_buffer_edit".to_string());
    ensure!(
        validate_driver_events(&forged, true).is_err(),
        "a synthetic mutation mode must be rejected"
    );

    // A materialization without an exact client route is rejected.
    let mut forged = complete_freshness_events(&digest);
    forged[12].details.insert("materialization".to_string(), "restart_free_hot_reload".to_string());
    ensure!(
        validate_driver_events(&forged, true).is_err(),
        "an invented materialization route must be rejected"
    );

    // A current generation claim that does not come from the client's own
    // state through the deterministic barrier is rejected.
    let mut forged = complete_freshness_events(&digest);
    forged[9].details.insert("state_source".to_string(), "server_log".to_string());
    ensure!(
        validate_driver_events(&forged, true).is_err(),
        "a server-log-sourced generation claim must be rejected"
    );

    // Freshness events may not cross the shutdown boundary.
    let mut after_shutdown = complete_freshness_events(&digest);
    let late = after_shutdown.remove(9);
    after_shutdown.push(late);
    renumber(&mut after_shutdown);
    ensure!(
        validate_driver_events(&after_shutdown, true).is_err(),
        "a freshness barrier after shutdown must be rejected"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Wire-mining laws
// ---------------------------------------------------------------------------

#[test]
fn freshness_wire_mines_prefix_tolerance_and_discriminators() -> Result<()> {
    let log = concat!(
        "12:00:01 {\"method\":\"initialize\",\"params\":{}}\n",
        "12:00:02 {\"method\":\"textDocument/didOpen\",\"params\":{\"textDocument\":{\"uri\":\"file:///w/workspace/project/settings.pl\"}}}\n",
        "12:00:03 {\"method\":\"textDocument/publishDiagnostics\",\"params\":{\"uri\":\"file:///w/workspace/project/settings.pl\",\"diagnostics\":[{\"severity\":2,\"code\":\"PL701\",\"message\":\"Module 'My::Vendor::Extra' not found\"}]}}\n",
        "12:00:04 {\"method\":\"workspace/didChangeConfiguration\",\"params\":{\"settings\":{}}}\n",
        "12:00:05 {\"method\":\"textDocument/didClose\",\"params\":{\"textDocument\":{\"uri\":\"file:///w/workspace/project/settings.pl\"}}}\n",
        "12:00:06 {\"method\":\"textDocument/didOpen\",\"params\":{\"textDocument\":{\"uri\":\"file:///w/workspace/project/config.pl\"}}}\n",
        "12:00:07 {\"method\":\"textDocument/publishDiagnostics\",\"params\":{\"uri\":\"file:///w/workspace/project/config.pl\",\"diagnostics\":[{\"severity\":2,\"code\":\"native.common.stale_dollar_at\"}]}}\n"
    );
    let wire = extract_freshness_wire(log.as_bytes());
    ensure!(wire.initialize_count == 1, "initialize requests are counted");
    ensure!(
        wire.did_open_lines.len() == 2 && wire.did_close_lines.len() == 1,
        "didOpen/didClose positions are mined"
    );
    ensure!(wire.did_change_configuration_lines == vec![3]);
    let settings = wire.settled_batch_after_open(SETTINGS_TOKEN, 0).unwrap();
    ensure!(settings.pl701_count == 1 && settings.warning_severity_count == 1);
    let config = wire.settled_batch_after_open(CONFIG_TOKEN, 0).unwrap();
    ensure!(config.critic_policy_count == 1 && config.warning_severity_count == 1);
    // Backslash URIs are never admitted as governed tokens.
    let windows_only = extract_freshness_wire(
        b"{\"method\":\"textDocument/didOpen\",\"params\":{\"textDocument\":{\"uri\":\"file:///w\\\\main.pl\"}}}\n",
    );
    ensure!(
        windows_only.did_open_lines.is_empty(),
        "a backslash-qualified URI is not a governed token"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Judgment: the canonical positive path and the discriminating controls
// ---------------------------------------------------------------------------

#[test]
fn real_wire_artifacts_do_not_forge_or_break_generations() -> Result<()> {
    // The real host wire carries two transport artifacts this judgment must
    // model honestly: the per-didOpen leading empty publish and the empty
    // clearing publish the server sends on didClose. Both land inside
    // generation windows; settled semantics (last batch before the close)
    // must ignore the clearing artifact while still catching a forged
    // settled generation.
    let mut wire = canonical_freshness_wire();
    // A leading empty publish inside the G2 window and a clearing publish at
    // its close boundary (after the didClose, before the next didOpen).
    wire.did_close_lines = vec![(16, MAIN_TOKEN.to_string()), (24, MAIN_TOKEN.to_string())];
    wire.batches.insert(1, batch(11, MAIN_TOKEN, 0, 0, 0, 0));
    wire.batches.insert(3, batch(17, MAIN_TOKEN, 0, 0, 0, 0));
    wire.batches.sort_by_key(|batch| batch.line_index);
    // The settled G2 batch is still the defective one; the settled decoy
    // window batch is still clean.
    ensure!(
        wire.settled_batch_after_open(MAIN_TOKEN, 1).unwrap().error_severity_count >= 1,
        "the clearing artifact must not displace the defective settled generation"
    );
    ensure!(
        wire.settled_batch_after_open(MAIN_TOKEN, 3).unwrap().is_clean(),
        "the decoy window must stay settled-clean"
    );

    // But a genuinely re-settled clean generation inside the G2 window (the
    // old generation repopulating and SETTLING before the close) is caught.
    let mut forged = canonical_freshness_wire();
    forged.did_close_lines = vec![(16, MAIN_TOKEN.to_string())];
    forged.batches.retain(|batch| batch.line_index != 12);
    forged.batches.push(batch(15, MAIN_TOKEN, 0, 0, 0, 0));
    forged.batches.sort_by_key(|batch| batch.line_index);
    ensure!(
        forged.settled_batch_after_open(MAIN_TOKEN, 1).unwrap().is_clean(),
        "a re-settled clean generation is the forged evidence the judgment must reject"
    );
    Ok(())
}

#[test]
fn initialize_restarts_count_outgoing_sends_only() -> Result<()> {
    // The real client log echoes the original request inside its response
    // envelope, so a naive method walker counts each initialize twice. The
    // extractor must count client-originated sends only.
    let log = concat!(
        "12:00:01 [\"--->\",1,\"perllsp-under-test\",{\"method\":\"initialize\",\"params\":{}}]
",
        "12:00:02 [\"<---\",1,\"perllsp-under-test\",{\"response\":{\"id\":1,\"result\":{}},\"request\":{\"id\":1,\"method\":\"initialize\"}}]
",
        "12:00:03 [\"--->\",1,\"perllsp-under-test\",{\"method\":\"textDocument/didOpen\",\"params\":{\"textDocument\":{\"uri\":\"file:///w/main.pl\"}}}]
",
        "12:00:04 [\"<---\",1,\"perllsp-under-test\",{\"response\":{\"method\":\"textDocument/publishDiagnostics\",\"params\":{\"uri\":\"file:///w/main.pl\",\"diagnostics\":[]}}}]
"
    );
    let wire = extract_freshness_wire(log.as_bytes());
    ensure!(wire.initialize_count == 1, "response echoes must not count as restarts");
    ensure!(wire.did_open_lines.len() == 1, "outgoing didOpen is mined");
    ensure!(wire.batches.len() == 1, "incoming publishDiagnostics is mined");
    ensure!(
        wire.did_change_configuration_lines.is_empty(),
        "echoed requests must not count as configuration pushes"
    );
    Ok(())
}

#[test]
fn teardown_deferred_shutdown_journey_needs_the_real_wire() -> Result<()> {
    // The pinned client can lose the job-exit callback in the stop/kill
    // race; the receipt's shutdown cell then relies on the client's own
    // teardown trace in the real mined wire (the #10944 substrate law). The
    // journey must be composed from that wire — an empty one would degrade
    // the cell to not-proven on a canonical run.
    let dir = tempfile::tempdir()?;
    let plan = scratch_freshness_plan(&dir.path().join("scratch"))?;
    let mut events = complete_freshness_events(&plan_digest(&plan));
    let last = events.len();
    events[last - 1] = detail_event(
        last as u64,
        DriverEventKind::ShutdownCompleted,
        &[("server_exited", "0"), ("exit_evidence", "deferred_to_editor_teardown")],
    );
    let observation = observation_with(Some(0), CleanupResult::Pass, events);
    let judgment = evaluate_freshness_observation(
        &plan,
        &observation,
        &canonical_wire(),
        &canonical_freshness_wire(),
        FreshnessFixtureVariant::Canonical,
    );
    let mut real_wire = canonical_wire();
    real_wire.saw_client_exit_log = true;
    let with_real_wire = freshness_journey(&observation, &judgment, &real_wire);
    let shutdown_cell = with_real_wire
        .iter()
        .find(|cell| cell.id == "shutdown_completed")
        .unwrap_or_else(|| panic!("shutdown_completed cell missing"));
    ensure!(
        shutdown_cell.result == ObservationResult::Pass,
        "the teardown-deferred shutdown must pass on the client's own teardown trace"
    );
    let with_empty_wire = freshness_journey(&observation, &judgment, &WireEvidence::default());
    let empty_cell = with_empty_wire
        .iter()
        .find(|cell| cell.id == "shutdown_completed")
        .unwrap_or_else(|| panic!("shutdown_completed cell missing"));
    ensure!(
        empty_cell.result == ObservationResult::NotProven,
        "an empty wire cannot prove the teardown-deferred exit"
    );
    Ok(())
}

#[test]
fn canonical_evidence_passes_all_six_cells() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_freshness_plan(&dir.path().join("scratch"))?;
    let judgment = canonical_judgment(
        &plan,
        complete_freshness_events(&plan_digest(&plan)),
        canonical_wire(),
        canonical_freshness_wire(),
        FreshnessFixtureVariant::Canonical,
    );
    ensure!(
        judgment.result == ObservationResult::Pass,
        "the canonical evidence must pass: cells {:?}, driver_failure {:?}",
        judgment.cells,
        judgment.driver_failure_reason
    );
    for cell in [
        CELL_ROUTE,
        CELL_EXTERNAL_SOURCE,
        CELL_PROJECT_CONFIG,
        CELL_CLIENT_SETTINGS,
        CELL_STALE_GENERATION,
        CELL_PROVIDER_OWNERSHIP,
    ] {
        ensure!(
            judgment.cells.get(cell) == Some(&ObservationResult::Pass),
            "cell {cell} must pass on canonical evidence: {:?}",
            judgment.cells
        );
    }
    ensure!(!judgment.client_watcher_exposed, "the pinned client exposes no watcher");
    Ok(())
}

#[test]
fn a_client_watcher_surface_changes_the_route_row_instead_of_passing() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_freshness_plan(&dir.path().join("scratch"))?;
    let mut wire = canonical_wire();
    wire.client_capabilities = Some(serde_json::json!({
        "workspace": {"didChangeWatchedFiles": {"dynamicRegistration": true}}
    }));
    let judgment = canonical_judgment(
        &plan,
        complete_freshness_events(&plan_digest(&plan)),
        wire,
        canonical_freshness_wire(),
        FreshnessFixtureVariant::Canonical,
    );
    ensure!(
        judgment.cells.get(CELL_ROUTE) == Some(&ObservationResult::Fail),
        "a client that exposes the watcher surface is a different route row: the route cell \
         must fail, not silently pass"
    );
    ensure!(judgment.result != ObservationResult::Pass);
    Ok(())
}

#[test]
fn an_old_generation_repopulation_is_rejected() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_freshness_plan(&dir.path().join("scratch"))?;
    // A clean batch sneaks into the defect window: the released old
    // generation "spontaneously" cleared the state without any
    // materialization. The stale cell must reject it.
    let mut freshness_wire = canonical_freshness_wire();
    freshness_wire.batches.insert(2, batch(15, MAIN_TOKEN, 0, 0, 0, 0));
    // Also drop the genuine defect batch so the state claim still matches:
    // the wire alone must convict.
    freshness_wire.batches.retain(|batch| batch.line_index != 12);
    let judgment = canonical_judgment(
        &plan,
        complete_freshness_events(&plan_digest(&plan)),
        canonical_wire(),
        freshness_wire,
        FreshnessFixtureVariant::Canonical,
    );
    ensure!(
        judgment.cells.get(CELL_STALE_GENERATION) == Some(&ObservationResult::Fail),
        "an old generation repopulating the wire must fail the stale cell: {:?}",
        judgment.cells
    );
    ensure!(judgment.result != ObservationResult::Pass);
    Ok(())
}

#[test]
fn a_missing_stale_hold_observation_cannot_pass_the_stale_cell() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_freshness_plan(&dir.path().join("scratch"))?;
    let mut events = complete_freshness_events(&plan_digest(&plan));
    // Remove both source holds and renumber the remaining ones (the index
    // law is gap-free from 1; the point here is the missing observations,
    // not the index shape).
    let mut held = 0;
    events.retain(|event| {
        if event.kind == DriverEventKind::StaleGenerationHeld && held < 2 {
            held += 1;
            false
        } else {
            true
        }
    });
    let mut next_index = 1_u32;
    for item in &mut events {
        if item.kind == DriverEventKind::StaleGenerationHeld {
            item.details.insert("hold_index".to_string(), next_index.to_string());
            next_index += 1;
        }
    }
    renumber(&mut events);
    ensure!(validate_driver_events(&events, true).is_ok());
    let judgment = canonical_judgment(
        &plan,
        events,
        canonical_wire(),
        canonical_freshness_wire(),
        FreshnessFixtureVariant::Canonical,
    );
    ensure!(
        judgment.cells.get(CELL_STALE_GENERATION) != Some(&ObservationResult::Pass),
        "a journey that never held a generation open cannot pass stale rejection"
    );
    ensure!(judgment.result != ObservationResult::Pass);
    Ok(())
}

#[test]
fn a_settings_push_that_never_reached_the_wire_is_rejected() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_freshness_plan(&dir.path().join("scratch"))?;
    // Remove the journey's settings push from the wire: the effect cleared
    // "spontaneously" — a registration/log-only claim must not pass.
    let mut freshness_wire = canonical_freshness_wire();
    freshness_wire.did_change_configuration_lines = vec![3];
    let judgment = canonical_judgment(
        &plan,
        complete_freshness_events(&plan_digest(&plan)),
        canonical_wire(),
        freshness_wire,
        FreshnessFixtureVariant::Canonical,
    );
    ensure!(
        judgment.cells.get(CELL_CLIENT_SETTINGS) == Some(&ObservationResult::Fail),
        "a settings effect without the push on the wire must fail: {:?}",
        judgment.cells
    );
    ensure!(judgment.result != ObservationResult::Pass);
    Ok(())
}

#[test]
fn a_settings_effect_without_the_control_observation_is_rejected() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_freshness_plan(&dir.path().join("scratch"))?;
    // The control reopen cleared the warning too (the reopen alone would
    // suffice): attribution to the push is lost and the cell must fail.
    let mut freshness_wire = canonical_freshness_wire();
    freshness_wire.batches.retain(|batch| batch.line_index != 36);
    freshness_wire.batches.push(batch(37, SETTINGS_TOKEN, 0, 0, 0, 0));
    freshness_wire.batches.sort_by_key(|batch| batch.line_index);
    let judgment = canonical_judgment(
        &plan,
        complete_freshness_events(&plan_digest(&plan)),
        canonical_wire(),
        freshness_wire,
        FreshnessFixtureVariant::Canonical,
    );
    ensure!(
        judgment.cells.get(CELL_CLIENT_SETTINGS) == Some(&ObservationResult::Fail),
        "a settings cell without the in-journey control must fail: {:?}",
        judgment.cells
    );
    ensure!(judgment.result != ObservationResult::Pass);
    Ok(())
}

#[test]
fn a_decoy_supplied_result_is_rejected() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_freshness_plan(&dir.path().join("scratch"))?;
    // The decoy control window carries the defect: the wrong-root same-name
    // source supplied the changed result.
    let mut freshness_wire = canonical_freshness_wire();
    freshness_wire.batches.retain(|batch| batch.line_index != 27);
    freshness_wire.batches.push(batch(26, MAIN_TOKEN, 1, 0, 0, 0));
    freshness_wire.batches.sort_by_key(|batch| batch.line_index);
    let judgment = canonical_judgment(
        &plan,
        complete_freshness_events(&plan_digest(&plan)),
        canonical_wire(),
        freshness_wire,
        FreshnessFixtureVariant::Canonical,
    );
    ensure!(
        judgment.cells.get(CELL_EXTERNAL_SOURCE) == Some(&ObservationResult::Fail),
        "a decoy-supplied result must fail the external source cell: {:?}",
        judgment.cells
    );
    ensure!(
        judgment.cells.get(CELL_PROVIDER_OWNERSHIP) == Some(&ObservationResult::Fail),
        "a decoy-supplied result must fail the provider ownership cell"
    );
    ensure!(judgment.result != ObservationResult::Pass);
    Ok(())
}

#[test]
fn a_config_generation_without_its_restart_is_rejected() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_freshness_plan(&dir.path().join("scratch"))?;
    // Only two initialize requests on the wire: the repair restart never
    // happened, so the repaired config generation cannot be current.
    let mut freshness_wire = canonical_freshness_wire();
    freshness_wire.initialize_count = 2;
    let judgment = canonical_judgment(
        &plan,
        complete_freshness_events(&plan_digest(&plan)),
        canonical_wire(),
        freshness_wire,
        FreshnessFixtureVariant::Canonical,
    );
    ensure!(
        judgment.cells.get(CELL_PROJECT_CONFIG) == Some(&ObservationResult::Fail),
        "a config generation without its restart must fail: {:?}",
        judgment.cells
    );
    ensure!(judgment.result != ObservationResult::Pass);
    Ok(())
}

#[test]
fn a_malformed_config_that_stays_silent_is_rejected() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_freshness_plan(&dir.path().join("scratch"))?;
    // The malformed restart window stays clean: the server silently kept the
    // exclude. The honest rejection (warning returns) is the discriminating
    // expectation; a silent carryover must fail.
    let mut freshness_wire = canonical_freshness_wire();
    freshness_wire.batches.retain(|batch| batch.line_index != 72);
    freshness_wire.batches.push(batch(71, CONFIG_TOKEN, 0, 0, 0, 0));
    freshness_wire.batches.sort_by_key(|batch| batch.line_index);
    let judgment = canonical_judgment(
        &plan,
        complete_freshness_events(&plan_digest(&plan)),
        canonical_wire(),
        freshness_wire,
        FreshnessFixtureVariant::Canonical,
    );
    ensure!(
        judgment.cells.get(CELL_PROJECT_CONFIG) == Some(&ObservationResult::Fail),
        "a malformed config generation that never restored the discriminator must fail: {:?}",
        judgment.cells
    );
    ensure!(judgment.result != ObservationResult::Pass);
    Ok(())
}

#[test]
fn a_negative_variant_that_reaches_a_pass_is_an_oracle_violation() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_freshness_plan(&dir.path().join("scratch"))?;
    // All three negative variants, fed the complete canonical evidence,
    // must not be reported as a pass: the oracle violation rule converts it
    // to a fail so a wrong green can never hide.
    for variant in [
        FreshnessFixtureVariant::WrongRootDecoy,
        FreshnessFixtureVariant::LiveReloadClaimed,
        FreshnessFixtureVariant::AmbientPathOnly,
    ] {
        let judgment = canonical_judgment(
            &plan,
            complete_freshness_events(&plan_digest(&plan)),
            canonical_wire(),
            canonical_freshness_wire(),
            variant,
        );
        ensure!(
            judgment.result == ObservationResult::Fail,
            "a negative variant reaching a pass must be reported as a fail ({variant:?})"
        );
    }
    Ok(())
}

#[test]
fn a_wrong_root_fails_the_journey_and_the_ownership_cell() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_freshness_plan(&dir.path().join("scratch"))?;
    let mut events = complete_freshness_events(&plan_digest(&plan));
    events[7].details.insert("observed_root".to_string(), "workspace".to_string());
    events.push(detail_event(43, DriverEventKind::DriverFailed, &[("reason", "root_mismatch")]));
    let judgment = canonical_judgment(
        &plan,
        events,
        canonical_wire(),
        canonical_freshness_wire(),
        FreshnessFixtureVariant::Canonical,
    );
    ensure!(
        judgment.cells.get(CELL_PROVIDER_OWNERSHIP) != Some(&ObservationResult::Pass),
        "a server answering from the wrong root cannot pass provider ownership"
    );
    ensure!(judgment.result != ObservationResult::Pass);
    Ok(())
}

#[test]
fn a_registration_for_another_candidate_cannot_own_the_result() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_freshness_plan(&dir.path().join("scratch"))?;
    let mut events = complete_freshness_events(&plan_digest(&plan));
    events[2].details.insert("candidate_sha256".to_string(), format!("sha256:{}", "f".repeat(64)));
    let judgment = canonical_judgment(
        &plan,
        events,
        canonical_wire(),
        canonical_freshness_wire(),
        FreshnessFixtureVariant::Canonical,
    );
    ensure!(
        judgment.cells.get(CELL_PROVIDER_OWNERSHIP) == Some(&ObservationResult::Fail),
        "a registration bound to another candidate digest must fail ownership: {:?}",
        judgment.cells
    );
    ensure!(judgment.result != ObservationResult::Pass);
    Ok(())
}
