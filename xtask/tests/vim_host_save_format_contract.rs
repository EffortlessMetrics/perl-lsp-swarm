// #11396 save-format scenario contract tests.
//
// Red-first law: the negative controls below were authored and proven to
// reject before the positive host journey ran. Every discriminating control
// feeds the judgment the exact evidence a false green would need (a manual
// comparator relabeled as the save trigger, a duplicate invocation, correct
// final bytes without the exact file state, a no-change claim whose route
// never executed, a refusal flattened into no-change, a failure without its
// error response, a stale result that applies, a client offering
// willSaveWaitUntil, a wrong-root same-name source, a registration for
// another candidate, a negative variant that wrongly passes) and asserts the
// slice refuses it. Real-editor launches are not unit tests: the canonical
// journeys run in the dedicated workflow (`.github/workflows/vim-hermetic-host.yml`).

use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use xtask::editor_client_compat::{
    CANONICAL_EXPECTATION_SET_ID, CleanupResult, EvidenceStage, ObservationResult,
    PlatformIdentity, RegistrationState, WorkspaceFixtureIdentity,
    canonical_expectation_set_digest, fixture_digest,
};
use xtask::vim_host_run::vim_host_runner::{
    self, DRIVER_SCHEMA_VERSION, DriverEvent, DriverEventKind, RUN_PLAN_SCHEMA_VERSION,
    VimHostPaths, VimHostRunIdentity, VimHostRunPlan, WireEvidence, validate_driver_events,
};
use xtask::vim_host_save_format_run::save_format_journey;
use xtask::vim_host_save_format_run::{
    CELL_APPLIED, CELL_CARDINALITY, CELL_DISABLED, CELL_FAILURE, CELL_NO_CHANGE, CELL_ROUTE,
    CELL_STALE, ROOT_MARKER, SaveFormatFixtureVariant, SaveWire, evaluate_save_format_observation,
    extract_save_wire, materialize_save_format_fixture, text_sha256,
};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(1).unwrap_or(Path::new(".")).to_path_buf()
}

// ---------------------------------------------------------------------------
// Scratch plan and evidence helpers
// ---------------------------------------------------------------------------

fn scratch_save_format_plan(root: &Path) -> Result<VimHostRunPlan> {
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
    let fixture = materialize_save_format_fixture(
        &root.join("fixture"),
        SaveFormatFixtureVariant::Canonical,
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
            candidate_identity_packet_sha256: vim_host_runner::bytes_sha256(b"packet")?,
            fixture: WorkspaceFixtureIdentity {
                id: "vim_vim_lsp_save_format_v1".to_string(),
                digest: fixture_digest(&fixture.root)?,
                expectation_set_id: CANONICAL_EXPECTATION_SET_ID.to_string(),
                expectation_set_digest: canonical_expectation_set_digest()?,
            },
            journey_selector: "vim_vim_lsp_save_format.v1".to_string(),
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

/// A syntactically valid scratch digest for streams that do not bind a real
/// plan.
fn scratch_digest() -> String {
    format!("sha256:{}", "a".repeat(64))
}

fn settlement_details(
    save_index: &str,
    trigger: &str,
    owner_count: &str,
    disposition: &str,
    requests_before: &str,
    requests_after: &str,
    response_kind: &str,
    bytes_sha: &str,
) -> Vec<(&'static str, String)> {
    vec![
        ("save_index", save_index.to_string()),
        ("trigger", trigger.to_string()),
        ("owner_count", owner_count.to_string()),
        ("disposition", disposition.to_string()),
        ("requests_before", requests_before.to_string()),
        ("requests_after", requests_after.to_string()),
        ("response_kind", response_kind.to_string()),
        ("buffer_sha256", bytes_sha.to_string()),
        ("file_sha256", bytes_sha.to_string()),
    ]
}

fn owned_settlement(sequence: u64, details: Vec<(&'static str, String)>) -> DriverEvent {
    let mut observation = event(sequence, DriverEventKind::SaveSettlementObserved);
    for (key, value) in details {
        observation.details.insert(key.to_string(), value);
    }
    observation
}

/// The complete canonical save-format journey event stream (30 barriers), in
/// the exact order the driver emits them.
fn complete_save_format_events(digest: &str) -> Result<Vec<DriverEvent>> {
    let canonical_sha = text_sha256(&xtask::vim_host_save_format_run::canonical_source_text())?;
    let non_canonical_sha =
        text_sha256(&xtask::vim_host_save_format_run::non_canonical_source_text())?;
    let bulk_sha = text_sha256(&xtask::vim_host_save_format_run::bulk_non_canonical_text())?;
    let owner = |index: &str, count: &str, timeout: &str| {
        vec![
            ("owner_index", index.to_string()),
            ("owner_count", count.to_string()),
            (
                "route",
                if count == "0" { "none".to_string() } else { "bufwritepre_autocmd".to_string() },
            ),
            ("action", "lsp_document_format_sync".to_string()),
            ("timeout_ms", timeout.to_string()),
        ]
    };
    let save =
        |index: &str, disposition: &str, before: &str, after: &str, kind: &str, sha: &str| {
            vec![
                ("save_index", index.to_string()),
                ("trigger", "bufwritepre_save".to_string()),
                (
                    "owner_count",
                    if disposition == "disabled" { "0".to_string() } else { "1".to_string() },
                ),
                ("disposition", disposition.to_string()),
                ("requests_before", before.to_string()),
                ("requests_after", after.to_string()),
                ("response_kind", kind.to_string()),
                ("buffer_sha256", sha.to_string()),
                ("file_sha256", sha.to_string()),
            ]
        };
    let mut events = vec![
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
        detail_event(
            7,
            DriverEventKind::InitializeObserved,
            &[
                ("capabilities_written", "1"),
                ("position_encoding", "utf-16"),
                ("document_formatting_advertised", "1"),
            ],
        ),
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
        owned_settlement(10, owner("1", "1", "30000")),
        owned_settlement(11, save("1", "applied", "0", "1", "edits", &canonical_sha)),
        owned_settlement(12, save("2", "no_change", "1", "2", "empty", &canonical_sha)),
        owned_settlement(13, owner("2", "1", "1")),
        detail_event(
            14,
            DriverEventKind::StaleResultHoldObserved,
            &[
                ("hold_index", "1"),
                ("window_ms", "5000"),
                ("requests_before", "2"),
                ("requests_after", "3"),
                ("bytes_held", "1"),
                ("late_response_rejected", "1"),
            ],
        ),
        owned_settlement(15, save("3", "stale_rejected", "2", "3", "edits", &bulk_sha)),
        owned_settlement(16, owner("3", "1", "30000")),
        owned_settlement(17, save("4", "no_change", "3", "4", "empty", &canonical_sha)),
        owned_settlement(18, owner("4", "0", "30000")),
        detail_event(
            19,
            DriverEventKind::ExternalMutationApplied,
            &[
                ("mutation_index", "1"),
                ("mutation", "in_place"),
                ("target", "governed"),
                ("disk_generation", "g1_non_canonical_restored"),
            ],
        ),
        detail_event(
            20,
            DriverEventKind::ClientMaterializationApplied,
            &[
                ("materialization_index", "1"),
                ("materialization", "client_close_reopen"),
                ("picks_generation", "g1_non_canonical_restored"),
            ],
        ),
        owned_settlement(21, save("5", "disabled", "4", "4", "absent", &non_canonical_sha)),
        owned_settlement(22, owner("5", "1", "30000")),
        detail_event(
            23,
            DriverEventKind::ExternalMutationApplied,
            &[
                ("mutation_index", "2"),
                ("mutation", "in_place"),
                ("target", "project_config"),
                ("disk_generation", "toml_formatting_off"),
            ],
        ),
        detail_event(
            24,
            DriverEventKind::ClientMaterializationApplied,
            &[
                ("materialization_index", "2"),
                ("materialization", "server_restart"),
                ("picks_generation", "toml_formatting_off"),
            ],
        ),
        owned_settlement(25, save("6", "refused", "4", "5", "empty", &non_canonical_sha)),
        detail_event(
            26,
            DriverEventKind::ExternalMutationApplied,
            &[
                ("mutation_index", "3"),
                ("mutation", "in_place"),
                ("target", "project_config"),
                ("disk_generation", "toml_external_missing_profile"),
            ],
        ),
        detail_event(
            27,
            DriverEventKind::ClientMaterializationApplied,
            &[
                ("materialization_index", "3"),
                ("materialization", "server_restart"),
                ("picks_generation", "toml_external_missing_profile"),
            ],
        ),
        owned_settlement(28, save("7", "failure", "5", "6", "error", &non_canonical_sha)),
        event(29, DriverEventKind::ShutdownStarted),
        event(30, DriverEventKind::ShutdownCompleted),
    ];
    // The owned_settlement name is misleading for owner events; rebuild them
    // with the right kind.
    for sequence in [10_u64, 13, 16, 18, 22] {
        if let Some(found) = events.iter_mut().find(|item| item.sequence == sequence) {
            found.kind = DriverEventKind::SaveOwnerConfigured;
        }
    }
    Ok(events)
}

/// The canonical save wire: six formatting requests (saves 1, 2, 3, 4, 6, 7;
/// the disabled save issues none), six settled responses (edits, empty,
/// edits, empty, empty, error).
fn canonical_save_wire() -> SaveWire {
    SaveWire {
        request_lines: vec![10, 12, 20, 22, 26, 28],
        response_lines: vec![11, 13, 21, 23, 27, 29],
        error_response_lines: vec![29],
        empty_response_lines: vec![13, 23, 27],
        edits_response_lines: vec![11, 21],
        cancel_request_lines: Vec::new(),
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
                "capabilities": {
                    "textDocument": {
                        "synchronization": {
                            "didSave": true,
                            "willSave": false,
                            "willSaveWaitUntil": false,
                       },
                    },
                },
            }
        })),
        client_capabilities: Some(serde_json::json!({
            "textDocument": {
                "synchronization": {"didSave": true, "willSave": false, "willSaveWaitUntil": false},
            },
        })),
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
    save_wire: SaveWire,
    variant: SaveFormatFixtureVariant,
) -> xtask::vim_host_save_format_run::SaveFormatJudgment {
    let observation = observation_with(Some(0), CleanupResult::Pass, events);
    evaluate_save_format_observation(plan, &observation, &wire, &save_wire, variant)
}

fn plan_digest(plan: &VimHostRunPlan) -> String {
    plan.identity.candidate_artifact_sha256.clone()
}

// ---------------------------------------------------------------------------
// Fixture laws
// ---------------------------------------------------------------------------

#[test]
fn canonical_fixture_carries_marker_decoy_and_both_generations() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let fixture = materialize_save_format_fixture(
        &dir.path().join("fixture"),
        SaveFormatFixtureVariant::Canonical,
    )?;
    let root = &fixture.root;
    ensure!(root.join("workspace/project/cpanfile").is_file());
    ensure!(!root.join("workspace/cpanfile").exists());
    ensure!(root.join("workspace/main.pl").is_file());
    let governed = fs::read_to_string(root.join("workspace/project/main.pl"))?;
    ensure!(governed.contains("sub compute{"), "governed source must start non-canonical");
    ensure!(!governed.contains("sub compute {"));
    let bulk = fs::read_to_string(root.join("workspace/project/bulk.pl"))?;
    ensure!(bulk.lines().count() > 800, "bulk stale document must be large");
    ensure!(!root.join("workspace/project/.perl-lsp.toml").exists());
    Ok(())
}

#[test]
fn wrong_root_variant_moves_exactly_the_marker() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let canonical = materialize_save_format_fixture(
        &dir.path().join("canonical"),
        SaveFormatFixtureVariant::Canonical,
    )?;
    let decoy = materialize_save_format_fixture(
        &dir.path().join("decoy"),
        SaveFormatFixtureVariant::WrongRootDecoy,
    )?;
    ensure!(canonical.root.join("workspace/project/cpanfile").is_file());
    ensure!(!decoy.root.join("workspace/project/cpanfile").exists());
    ensure!(decoy.root.join("workspace/cpanfile").is_file());
    // Every other file is byte-identical between the variants.
    for relative in ["workspace/project/main.pl", "workspace/project/bulk.pl", "workspace/main.pl"]
    {
        ensure!(
            fs::read(canonical.root.join(relative))? == fs::read(decoy.root.join(relative))?,
            "{relative} drifted between variants"
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Event-stream shape laws
// ---------------------------------------------------------------------------

#[test]
fn complete_save_event_stream_validates() -> Result<()> {
    let events = complete_save_format_events(&scratch_digest())?;
    validate_driver_events(&events, true)?;
    Ok(())
}

#[test]
fn save_event_repetition_laws_reject_disorder_and_forgeries() -> Result<()> {
    let digest = scratch_digest();
    let mut events = complete_save_format_events(&digest)?;
    ensure!(validate_driver_events(&events, true).is_ok());

    // A settlement whose index skips is rejected.
    let mut skipped = events.clone();
    if let Some(found) = skipped.iter_mut().find(|item| item.sequence == 12) {
        found.details.insert("save_index".to_string(), "3".to_string());
    }
    ensure!(validate_driver_events(&skipped, true).is_err());

    // A settlement without exact byte identities is rejected.
    let mut shaless = events.clone();
    if let Some(found) = shaless.iter_mut().find(|item| item.sequence == 11) {
        found.details.remove("file_sha256");
    }
    ensure!(validate_driver_events(&shaless, true).is_err());

    // An owner event without the canonical action is rejected.
    let mut misdelegated = events.clone();
    if let Some(found) = misdelegated.iter_mut().find(|item| item.sequence == 10) {
        found.details.insert("action".to_string(), "manual_format_command".to_string());
    }
    ensure!(validate_driver_events(&misdelegated, true).is_err());

    // A hold below the minimum window is rejected.
    let mut short_hold = events.clone();
    if let Some(found) = short_hold.iter_mut().find(|item| item.sequence == 14) {
        found.details.insert("window_ms".to_string(), "100".to_string());
    }
    ensure!(validate_driver_events(&short_hold, true).is_err());

    // An unknown trigger token is rejected.
    let mut forged_trigger = events;
    if let Some(found) = forged_trigger.iter_mut().find(|item| item.sequence == 11) {
        found.details.insert("trigger".to_string(), "did_save_post".to_string());
    }
    ensure!(validate_driver_events(&forged_trigger, true).is_err());
    Ok(())
}

// ---------------------------------------------------------------------------
// Wire mining laws
// ---------------------------------------------------------------------------

#[test]
fn save_wire_mines_real_wire_artifacts_without_forging_requests() -> Result<()> {
    // Real-envelope artifacts: prefixed lines, response echoes of the request
    // (the #12660 finding), and an unrelated didSave notification.
    let log = concat!(
        "08/25/2026 10:00:00:[\"--->\",3,\"perl\",{\"method\":\"initialize\",\"params\":{}}]\n",
        "08/25/2026 10:00:00:[\"<---\",3,\"perl\",{\"response\":{\"id\":1,\"result\":{\"capabilities\":{}}},\"request\":{\"method\":\"initialize\",\"id\":1}}]\n",
        "08/25/2026 10:00:01:[\"--->\",3,\"perl\",{\"method\":\"textDocument/didSave\",\"params\":{}}]\n",
        "08/25/2026 10:00:02:[\"--->\",3,\"perl\",{\"method\":\"textDocument/formatting\",\"id\":7,\"params\":{}}]\n",
        "08/25/2026 10:00:02:[\"<---\",3,\"perl\",{\"response\":{\"id\":7,\"result\":[{\"range\":{\"start\":{\"line\":0}}}]},\"request\":{\"method\":\"textDocument/formatting\",\"id\":7}}]\n",
        "08/25/2026 10:00:03:[\"--->\",3,\"perl\",{\"method\":\"textDocument/formatting\",\"id\":8,\"params\":{}}]\n",
        "08/25/2026 10:00:03:[\"<---\",3,\"perl\",{\"response\":{\"id\":8,\"error\":{\"code\":-32603,\"message\":\"Formatting failed\"}},\"request\":{\"method\":\"textDocument/formatting\",\"id\":8}}]\n",
    );
    let wire = extract_save_wire(log.as_bytes());
    assert_eq!(wire.request_count(), 2, "response echoes must not count as requests");
    assert_eq!(wire.response_count(), 2);
    assert_eq!(wire.edits_response_lines, vec![4]);
    assert_eq!(wire.error_response_lines, vec![6]);
    ensure!(wire.empty_response_lines.is_empty());
    Ok(())
}

// ---------------------------------------------------------------------------
// Judgment laws
// ---------------------------------------------------------------------------

#[test]
fn canonical_evidence_proves_all_seven_cells() -> Result<()> {
    let plan = scratch_save_format_plan(&tempfile::tempdir()?.path().join("p"))?;
    let events = complete_save_format_events(&plan_digest(&plan))?;
    let judgment = canonical_judgment(
        &plan,
        events,
        canonical_wire(),
        canonical_save_wire(),
        SaveFormatFixtureVariant::Canonical,
    );
    assert_eq!(judgment.result, ObservationResult::Pass);
    for (cell, result) in &judgment.cells {
        assert_eq!(*result, ObservationResult::Pass, "cell {cell} must pass on canonical evidence");
    }
    assert_eq!(judgment.client_will_save_wait_until, Some(false));
    assert_eq!(judgment.server_formatting_advertised, Some(true));
    Ok(())
}

#[test]
fn an_ownerless_manual_comparator_cannot_satisfy_the_route_cell() -> Result<()> {
    let plan = scratch_save_format_plan(&tempfile::tempdir()?.path().join("p"))?;
    let canonical_sha = text_sha256(&xtask::vim_host_save_format_run::canonical_source_text())?;
    let mut events = complete_save_format_events(&plan_digest(&plan))?;
    // The faithful manual_comparator_only stream: the bootstrap barriers,
    // one ownerless owner event, the manual-comparator settlement with
    // correct final bytes, then the typed failure. A journey that never arms
    // the save owner cannot claim the save-triggered route.
    events.truncate(9);
    events.push(detail_event(
        10,
        DriverEventKind::SaveOwnerConfigured,
        &[
            ("owner_index", "1"),
            ("owner_count", "0"),
            ("route", "none"),
            ("action", "lsp_document_format_sync"),
            ("timeout_ms", "30000"),
        ],
    ));
    events.push(owned_settlement(
        11,
        settlement_details(
            "1",
            "manual_comparator",
            "0",
            "applied",
            "0",
            "1",
            "edits",
            &canonical_sha,
        ),
    ));
    events.push(event(12, DriverEventKind::ShutdownStarted));
    events.push(event(13, DriverEventKind::ShutdownCompleted));
    events.push(detail_event(
        14,
        DriverEventKind::DriverFailed,
        &[("reason", "save_trigger_absent")],
    ));
    renumber(&mut events);
    let mut wire = canonical_save_wire();
    wire.request_lines = vec![10];
    wire.response_lines = vec![11];
    wire.edits_response_lines = vec![11];
    wire.empty_response_lines = Vec::new();
    wire.error_response_lines = Vec::new();
    let observation = observation_with(Some(0), CleanupResult::Pass, events);
    let judgment = evaluate_save_format_observation(
        &plan,
        &observation,
        &canonical_wire(),
        &wire,
        SaveFormatFixtureVariant::ManualComparatorOnly,
    );
    assert_eq!(judgment.cells.get(CELL_ROUTE), Some(&ObservationResult::Fail));
    assert_eq!(judgment.result, ObservationResult::Fail);
    assert_eq!(judgment.driver_failure_reason.as_deref(), Some("save_trigger_absent"));
    Ok(())
}

#[test]
fn a_duplicate_invocation_breaks_cardinality() -> Result<()> {
    let plan = scratch_save_format_plan(&tempfile::tempdir()?.path().join("p"))?;
    let mut events = complete_save_format_events(&plan_digest(&plan))?;
    if let Some(found) = events.iter_mut().find(|item| item.sequence == 10) {
        found.details.insert("owner_count".to_string(), "2".to_string());
    }
    if let Some(found) = events.iter_mut().find(|item| item.sequence == 11) {
        found.details.insert("owner_count".to_string(), "2".to_string());
        found.details.insert("requests_after".to_string(), "2".to_string());
    }
    // Renumber the accounting: every later settlement saw one extra request.
    for sequence in [12_u64, 14, 15, 17, 21, 25, 28] {
        if let Some(found) = events.iter_mut().find(|item| item.sequence == sequence) {
            for key in ["requests_before", "requests_after"] {
                if let Some(value) = found.details.get(key).cloned() {
                    let bumped: u32 = value.parse::<u32>()? + 1;
                    found.details.insert(key.to_string(), bumped.to_string());
                }
            }
        }
    }
    let mut observation = observation_with(Some(0), CleanupResult::Pass, events);
    observation.events.push(detail_event(
        31,
        DriverEventKind::DriverFailed,
        &[("reason", "duplicate_invocation_observed")],
    ));
    renumber(&mut observation.events);
    let judgment = evaluate_save_format_observation(
        &plan,
        &observation,
        &canonical_wire(),
        &canonical_save_wire(),
        SaveFormatFixtureVariant::DuplicateOwner,
    );
    assert_eq!(judgment.cells.get(CELL_CARDINALITY), Some(&ObservationResult::Fail));
    assert_eq!(judgment.result, ObservationResult::Fail);
    assert_eq!(judgment.driver_failure_reason.as_deref(), Some("duplicate_invocation_observed"));
    Ok(())
}

#[test]
fn applied_without_the_exact_file_bytes_is_rejected() -> Result<()> {
    let plan = scratch_save_format_plan(&tempfile::tempdir()?.path().join("p"))?;
    let mut events = complete_save_format_events(&plan_digest(&plan))?;
    // Falsifier 4: the request succeeded but the FILE state is wrong (the
    // buffer alone matching cannot satisfy the applied cell).
    let wrong_sha = text_sha256("use strict;\nuse warnings;\n# wrong bytes\n")?;
    if let Some(found) = events.iter_mut().find(|item| item.sequence == 11) {
        found.details.insert("file_sha256".to_string(), wrong_sha);
    }
    let judgment = canonical_judgment(
        &plan,
        events,
        canonical_wire(),
        canonical_save_wire(),
        SaveFormatFixtureVariant::Canonical,
    );
    assert_eq!(judgment.cells.get(CELL_APPLIED), Some(&ObservationResult::Fail));
    assert_eq!(judgment.result, ObservationResult::NotProven);
    Ok(())
}

#[test]
fn a_no_change_without_route_execution_is_rejected() -> Result<()> {
    let plan = scratch_save_format_plan(&tempfile::tempdir()?.path().join("p"))?;
    let mut events = complete_save_format_events(&plan_digest(&plan))?;
    // Falsifier 5: already-canonical bytes called no-change although the
    // route never executed (zero requests, absent response).
    if let Some(found) = events.iter_mut().find(|item| item.sequence == 12) {
        found.details.insert("requests_after".to_string(), "1".to_string());
        found.details.insert("response_kind".to_string(), "absent".to_string());
    }
    let judgment = canonical_judgment(
        &plan,
        events,
        canonical_wire(),
        canonical_save_wire(),
        SaveFormatFixtureVariant::Canonical,
    );
    assert_eq!(judgment.cells.get(CELL_NO_CHANGE), Some(&ObservationResult::Fail));
    assert_eq!(judgment.result, ObservationResult::NotProven);
    Ok(())
}

#[test]
fn a_refusal_flattened_into_no_change_is_rejected() -> Result<()> {
    let plan = scratch_save_format_plan(&tempfile::tempdir()?.path().join("p"))?;
    let mut events = complete_save_format_events(&plan_digest(&plan))?;
    // Falsifier 6: the refused leg relabeled no-change. The label law and the
    // authored byte oracle (non-canonical bytes) both refuse it.
    if let Some(found) = events.iter_mut().find(|item| item.sequence == 25) {
        found.details.insert("disposition".to_string(), "no_change".to_string());
    }
    let judgment = canonical_judgment(
        &plan,
        events,
        canonical_wire(),
        canonical_save_wire(),
        SaveFormatFixtureVariant::Canonical,
    );
    // The disabled/refused cell is the discrimination channel: its refused
    // label law fails. The no-change cell reads only the honest saves 2 and
    // 4, so the relabeling cannot manufacture a false no-change pass either.
    assert_eq!(judgment.cells.get(CELL_DISABLED), Some(&ObservationResult::Fail));
    assert_eq!(judgment.cells.get(CELL_NO_CHANGE), Some(&ObservationResult::Pass));
    assert_eq!(judgment.result, ObservationResult::NotProven);
    Ok(())
}

#[test]
fn a_failure_without_an_error_response_is_rejected() -> Result<()> {
    let plan = scratch_save_format_plan(&tempfile::tempdir()?.path().join("p"))?;
    let mut events = complete_save_format_events(&plan_digest(&plan))?;
    // The failure disposition must rest on an observed error response, never
    // on an empty result a refusal or no-change could carry.
    if let Some(found) = events.iter_mut().find(|item| item.sequence == 28) {
        found.details.insert("response_kind".to_string(), "empty".to_string());
    }
    let judgment = canonical_judgment(
        &plan,
        events,
        canonical_wire(),
        canonical_save_wire(),
        SaveFormatFixtureVariant::Canonical,
    );
    assert_eq!(judgment.cells.get(CELL_FAILURE), Some(&ObservationResult::Fail));
    assert_eq!(judgment.result, ObservationResult::NotProven);
    Ok(())
}

#[test]
fn a_stale_result_that_applies_is_rejected() -> Result<()> {
    let plan = scratch_save_format_plan(&tempfile::tempdir()?.path().join("p"))?;
    let canonical_sha = text_sha256(&xtask::vim_host_save_format_run::canonical_source_text())?;
    let mut events = complete_save_format_events(&plan_digest(&plan))?;
    // Falsifier 7: the stale result applied anyway — the bulk bytes moved to
    // the formatted form inside the window.
    if let Some(found) = events.iter_mut().find(|item| item.sequence == 15) {
        found.details.insert("buffer_sha256".to_string(), canonical_sha.clone());
        found.details.insert("file_sha256".to_string(), canonical_sha.clone());
    }
    if let Some(found) = events.iter_mut().find(|item| item.sequence == 14) {
        found.details.insert("bytes_held".to_string(), "0".to_string());
    }
    let judgment = canonical_judgment(
        &plan,
        events,
        canonical_wire(),
        canonical_save_wire(),
        SaveFormatFixtureVariant::Canonical,
    );
    assert_eq!(judgment.cells.get(CELL_STALE), Some(&ObservationResult::Fail));
    assert_eq!(judgment.result, ObservationResult::NotProven);
    Ok(())
}

#[test]
fn a_missing_stale_hold_cannot_pass_the_stale_cell() -> Result<()> {
    let plan = scratch_save_format_plan(&tempfile::tempdir()?.path().join("p"))?;
    let mut events = complete_save_format_events(&plan_digest(&plan))?;
    events.retain(|item| item.kind != DriverEventKind::StaleResultHoldObserved);
    renumber(&mut events);
    let judgment = canonical_judgment(
        &plan,
        events,
        canonical_wire(),
        canonical_save_wire(),
        SaveFormatFixtureVariant::Canonical,
    );
    assert_eq!(judgment.cells.get(CELL_STALE), Some(&ObservationResult::Fail));
    assert_eq!(judgment.result, ObservationResult::NotProven);
    Ok(())
}

#[test]
fn a_will_save_wait_until_client_changes_the_route_row() -> Result<()> {
    let plan = scratch_save_format_plan(&tempfile::tempdir()?.path().join("p"))?;
    let events = complete_save_format_events(&plan_digest(&plan))?;
    let mut wire = canonical_wire();
    // A client that exposes the native save-format surface is a different
    // route row: the autocmd classification must not silently stay a pass.
    wire.client_capabilities = Some(serde_json::json!({
        "textDocument": {"synchronization": {"willSave": false, "willSaveWaitUntil": true}},
    }));
    let judgment = canonical_judgment(
        &plan,
        events,
        wire,
        canonical_save_wire(),
        SaveFormatFixtureVariant::Canonical,
    );
    assert_eq!(judgment.cells.get(CELL_ROUTE), Some(&ObservationResult::Fail));
    assert_eq!(judgment.client_will_save_wait_until, Some(true));
    assert_eq!(judgment.result, ObservationResult::NotProven);
    Ok(())
}

#[test]
fn a_server_that_does_not_advertise_formatting_changes_the_route_row() -> Result<()> {
    let plan = scratch_save_format_plan(&tempfile::tempdir()?.path().join("p"))?;
    let mut events = complete_save_format_events(&plan_digest(&plan))?;
    if let Some(found) = events.iter_mut().find(|item| item.sequence == 7) {
        found.details.insert("document_formatting_advertised".to_string(), "0".to_string());
    }
    let judgment = canonical_judgment(
        &plan,
        events,
        canonical_wire(),
        canonical_save_wire(),
        SaveFormatFixtureVariant::Canonical,
    );
    assert_eq!(judgment.cells.get(CELL_ROUTE), Some(&ObservationResult::Fail));
    assert_eq!(judgment.result, ObservationResult::NotProven);
    Ok(())
}

#[test]
fn a_negative_variant_that_reaches_a_pass_is_an_oracle_violation() -> Result<()> {
    let plan = scratch_save_format_plan(&tempfile::tempdir()?.path().join("p"))?;
    let events = complete_save_format_events(&plan_digest(&plan))?;
    for variant in [
        SaveFormatFixtureVariant::ManualComparatorOnly,
        SaveFormatFixtureVariant::DuplicateOwner,
        SaveFormatFixtureVariant::WrongRootDecoy,
    ] {
        let judgment = canonical_judgment(
            &plan,
            events.clone(),
            canonical_wire(),
            canonical_save_wire(),
            variant,
        );
        assert_eq!(
            judgment.result,
            ObservationResult::Fail,
            "a negative variant reaching all-seven-pass must report the oracle violation"
        );
    }
    Ok(())
}

#[test]
fn a_wrong_root_fails_the_journey_and_the_route_cell() -> Result<()> {
    let plan = scratch_save_format_plan(&tempfile::tempdir()?.path().join("p"))?;
    let mut events = complete_save_format_events(&plan_digest(&plan))?;
    if let Some(found) = events.iter_mut().find(|item| item.sequence == 8) {
        found.details.insert("observed_root".to_string(), "workspace".to_string());
    }
    let mut observation = observation_with(Some(0), CleanupResult::Pass, events);
    observation.events.push(detail_event(
        31,
        DriverEventKind::DriverFailed,
        &[("reason", "root_mismatch")],
    ));
    renumber(&mut observation.events);
    let judgment = evaluate_save_format_observation(
        &plan,
        &observation,
        &canonical_wire(),
        &canonical_save_wire(),
        SaveFormatFixtureVariant::WrongRootDecoy,
    );
    assert_eq!(judgment.cells.get(CELL_ROUTE), Some(&ObservationResult::Fail));
    assert_eq!(judgment.driver_failure_reason.as_deref(), Some("root_mismatch"));
    assert_eq!(judgment.result, ObservationResult::Fail);
    Ok(())
}

#[test]
fn a_registration_for_another_candidate_cannot_own_the_result() -> Result<()> {
    let plan = scratch_save_format_plan(&tempfile::tempdir()?.path().join("p"))?;
    let mut events = complete_save_format_events(&plan_digest(&plan))?;
    if let Some(found) = events.iter_mut().find(|item| item.sequence == 3) {
        found.details.insert("candidate_sha256".to_string(), "sha256:b".repeat(8).to_string());
    }
    let judgment = canonical_judgment(
        &plan,
        events,
        canonical_wire(),
        canonical_save_wire(),
        SaveFormatFixtureVariant::Canonical,
    );
    assert_eq!(judgment.cells.get(CELL_ROUTE), Some(&ObservationResult::Fail));
    assert_eq!(judgment.result, ObservationResult::NotProven);
    Ok(())
}

// ---------------------------------------------------------------------------
// Thin-script source laws (#12763 review threads)
//
// The judgment above can only be as honest as the thin scripts that produce
// its events, so these laws pin the script-side honesty mechanically: no
// fabricated response-kind labels, a duplicate-owner control that fails the
// instrument unless the falsifier was actually observed, a stale rejection
// classified from the wire, digests that keep trailing-newline states
// distinct, and a CI trigger that actually fires on formatter changes.
// ---------------------------------------------------------------------------

fn save_format_driver_source() -> Result<String> {
    fs::read_to_string(repo_root().join("scripts/test/vim-host-save-format-driver.vim"))
        .context("reading scripts/test/vim-host-save-format-driver.vim")
}

fn vim_lsp_adapter_source() -> Result<String> {
    fs::read_to_string(repo_root().join("scripts/test/vim-clients/vim-lsp-adapter.vim"))
        .context("reading scripts/test/vim-clients/vim-lsp-adapter.vim")
}

#[test]
fn every_save_settlement_carries_a_classified_or_declared_absent_response_kind() -> Result<()> {
    // A settled formatting response must never be pre-labeled: an empty or
    // error result recorded as edits would let unchanged bytes satisfy a cell
    // that claims a real edit result was observed and judged (#12763 P1).
    let driver = save_format_driver_source()?;
    for fabricated in
        ["'response_kind': 'edits'", "'response_kind': 'empty'", "'response_kind': 'error'"]
    {
        ensure!(
            !driver.contains(fabricated),
            "driver hardcodes {fabricated}: every settlement response_kind must come from \
             classifying the actual wire counters (or the declared 'absent')"
        );
    }
    Ok(())
}

#[test]
fn duplicate_owner_control_fails_closed_when_the_falsifier_was_never_observed() -> Result<()> {
    // `duplicate_invocation_observed` is the typed reason the CLI accepts as a
    // successful negative control, so it may only be emitted after the two
    // requests and responses were actually observed; an unobserved wait is an
    // instrument failure with its own reason (#12763 thread 3864145196).
    let driver = save_format_driver_source()?;
    let accepted_control_hits = driver.matches("'duplicate_invocation_observed'").count();
    ensure!(
        accepted_control_hits == 1,
        "the accepted negative-control reason must appear exactly once (observed branch), found \
         {accepted_control_hits}"
    );
    ensure!(
        driver.contains("'duplicate_invocations_never_observed'"),
        "a failed observation of the two-invocation falsifier must emit the distinct instrument \
         failure reason, never the accepted negative-control reason"
    );
    Ok(())
}

#[test]
fn stale_leg_rejection_is_classified_from_the_wire_and_requires_edits() -> Result<()> {
    // The held-bytes window cannot distinguish "an edit result arrived late
    // and was rejected" from "nothing arrived to reject", so the stale leg
    // must classify the specific settled response through the same
    // direction-aware counters as ordinary saves and fail closed when the
    // settled response carries no edit result (#12763 thread 3864145173).
    let driver = save_format_driver_source()?;
    ensure!(
        driver.contains("s:Fail('stale_late_response_not_edits')"),
        "a settled non-edits response in the stale leg must be an instrument failure, not a \
         rejection claim"
    );
    // Ordinary saves classify once; the stale leg adds its own classification,
    // so each counter must appear at least twice.
    for counter in [
        "VimLspHostWireErrorResponseCount(",
        "VimLspHostWireEmptyResponseCount(",
        "VimLspHostWireEditsResponseCount(",
    ] {
        let hits = driver.matches(counter).count();
        ensure!(
            hits >= 2,
            "{counter} must back at least the ordinary-save and stale-leg classifications, found \
             {hits}"
        );
    }
    ensure!(
        !driver.contains("'late_response_rejected': '1'"),
        "late_response_rejected must be gated on the classified response kind, never hardcoded"
    );
    Ok(())
}

#[test]
fn exact_byte_digests_keep_trailing_blank_line_and_final_newline_states_distinct() -> Result<()> {
    // Collapsing a trailing empty item before appending one newline maps two
    // different byte texts (with and without one more blank line) onto one
    // digest, so both exact-byte checks can pass despite different bytes
    // (#12763 thread 3864145199).
    let adapter = vim_lsp_adapter_source()?;
    ensure!(
        !adapter.contains("remove(l:lines"),
        "digest helpers must not strip line items before hashing: stripping collapses trailing \
         newline states into one identity"
    );
    // The buffer carries content lines only; the document-final newline lives
    // in end-of-line state, so the buffer identity unconditionally joins and
    // appends the terminator.
    ensure!(
        adapter.contains(r#"sha256(join(a:lines, "\n") . "\n")"#),
        "buffer digest must hash the joined content lines plus their terminator verbatim"
    );
    // Binary-mode reads encode the final-newline state as a trailing empty
    // item, so the file identity reconstructs the exact bytes from that item.
    ensure!(
        adapter.contains("readfile(a:path, 'b')") && adapter.contains(r#"join(l:lines, "\n")"#),
        "file digest must hash raw binary-mode bytes reconstructed exactly"
    );
    Ok(())
}

#[test]
fn hermetic_host_ci_triggers_on_the_production_formatter_crate() -> Result<()> {
    // perl-lsp-rs-core depends on perl-lsp-perltidy and invokes its native
    // formatter, so a PR changing only the formatter crate must re-run the
    // end-to-end save proof (#12763 thread 3864145182).
    let workflow = fs::read_to_string(repo_root().join(".github/workflows/vim-hermetic-host.yml"))
        .context("reading .github/workflows/vim-hermetic-host.yml")?;
    // The ratchet is coverage of the formatter crate, not one spelling of it.
    // A blanket `crates/**` covers it strictly more broadly than the named
    // entry; narrowing to any filter that reaches neither still fails here.
    let covered =
        workflow.contains("\"crates/perl-lsp-perltidy/**\"") || workflow.contains("\"crates/**\"");
    ensure!(
        covered,
        "the hermetic host workflow path filter must reach crates/perl-lsp-perltidy \
         (via \"crates/perl-lsp-perltidy/**\" or a covering \"crates/**\")"
    );
    Ok(())
}

#[test]
fn the_failure_cell_carries_the_family_disposition_token() -> Result<()> {
    let plan = scratch_save_format_plan(&tempfile::tempdir()?.path().join("p"))?;
    let events = complete_save_format_events(&plan_digest(&plan))?;
    let observation = observation_with(Some(0), CleanupResult::Pass, events);
    let judgment = canonical_judgment(
        &plan,
        observation.events.clone(),
        canonical_wire(),
        canonical_save_wire(),
        SaveFormatFixtureVariant::Canonical,
    );
    assert_eq!(judgment.result, ObservationResult::Pass);
    let journey = save_format_journey(&observation, &judgment, &canonical_wire());
    let failure_cell = journey
        .iter()
        .find(|cell| cell.id == CELL_FAILURE)
        .context("the receipt journey must carry the failure cell")?;
    // The generic receipt admits no fail cell in a passing run, so the cell's
    // generic result is the proven honest-record claim while the limitation
    // carries the #11384 family vocabulary token the CI binding checks.
    assert_eq!(failure_cell.result, ObservationResult::Pass);
    ensure!(failure_cell.observed, "the honestly recorded failure is an observation");
    ensure!(
        failure_cell
            .limitation
            .as_deref()
            .is_some_and(|text| text.contains("observed_disposition=fail")
                && text.contains("never carries pass")),
        "the failure cell limitation must carry the family disposition token and the no-pass law"
    );
    for cell in [
        &CELL_ROUTE,
        &CELL_CARDINALITY,
        &CELL_APPLIED,
        &CELL_NO_CHANGE,
        &CELL_DISABLED,
        &CELL_STALE,
    ] {
        let found = journey.iter().find(|item| &item.id == cell).context("missing cell")?;
        assert_eq!(found.result, ObservationResult::Pass);
    }
    Ok(())
}
