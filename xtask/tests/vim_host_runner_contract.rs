// Hermetic actual-Vim host runner contract tests (#10944).
//
// The substrate is consumed progressively by the successor execution leaves
// (#10946 and the #11376/#11378 families); this contract target exercises
// the fail-closed schema, subject, isolation, process, and receipt laws that
// must hold before any of them can trust the substrate. Real-editor launches
// are not unit tests: the canonical real-host proof runs in the dedicated
// workflow (`.github/workflows/vim-hermetic-host.yml`), mirroring how the
// Emacs runner split offline contract tests from actual-host runs.
#![allow(dead_code)]
#![allow(unused_imports)]

// One substrate instance: the same module the xtask binary command uses
// (`xtask::vim_host_run::vim_host_runner`), so contract tests and the CLI
// cannot diverge into two compiled copies with incompatible types.
use xtask::vim_host_run::vim_host_runner;

use anyhow::{Context, Result, bail, ensure};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use vim_host_runner::{
    DRIVER_SCHEMA_VERSION, DriverEvent, DriverEventKind, RUN_PLAN_SCHEMA_VERSION, VimHostPaths,
    VimHostRunIdentity, VimHostRunPlan, VimLspSubjectManifest, WireEvidence, build_vim_command,
    capabilities_from_wire_evidence, diagnostics_from_wire_evidence, extract_wire_evidence,
    is_reason_token, parse_process_snapshot, surviving_processes, validate_driver_events,
    validate_receipt_binding,
};
use xtask::editor_client_compat::{
    ArtifactKind, CANONICAL_EXPECTATION_SET_ID, CapabilityBasis, CleanupResult, DiagnosticMode,
    DiagnosticsIdentity, EditorClientCompatReceipt, EvidenceStage, JourneyCell, ObservationResult,
    PlatformIdentity, PositionEncodingBasis, RegistrationState, WorkspaceFixtureIdentity,
    canonical_expectation_set_digest, fixture_digest,
};
use xtask::vim_host_run::{
    bind_candidate_build_revision, evaluate_observation, load_activation_root_manifest,
    load_configuration_manifest, materialize_harness_fixture, outcome_journey,
    validate_identity_packet, verify_vim_features,
};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(1).unwrap_or(Path::new(".")).to_path_buf()
}

// ---------------------------------------------------------------------------
// Schema identity
// ---------------------------------------------------------------------------

#[test]
fn runner_contract_uses_versioned_run_and_driver_schemas() {
    assert_eq!(RUN_PLAN_SCHEMA_VERSION, "vim_host_run_plan.v1");
    assert_eq!(DRIVER_SCHEMA_VERSION, "vim_host_driver.v1");
}

// ---------------------------------------------------------------------------
// Driver-event laws (including the #7762 native-activation law)
// ---------------------------------------------------------------------------

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

fn registration_digest() -> String {
    format!("sha256:{}", "a".repeat(64))
}

fn complete_events() -> Vec<DriverEvent> {
    vec![
        event(1, DriverEventKind::HostStarted),
        event(2, DriverEventKind::ClientLoaded),
        detail_event(
            3,
            DriverEventKind::RegistrationSelected,
            &[("cmd", "perllsp--stdio"), ("candidate_sha256", &registration_digest())],
        ),
        event(4, DriverEventKind::FixtureOpened),
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
            &[("root_source", "activation_root_marker")],
        ),
        detail_event(9, DriverEventKind::DiagnosticsObserved, &[("mode", "push")]),
        event(10, DriverEventKind::ShutdownStarted),
        event(11, DriverEventKind::ShutdownCompleted),
    ]
}

#[test]
fn complete_driver_event_stream_validates() -> Result<()> {
    ensure!(
        validate_driver_events(&complete_events(), true).is_ok(),
        "complete stream must validate"
    );
    Ok(())
}

#[test]
fn driver_events_require_contiguous_sequence_and_order() -> Result<()> {
    let mut reordered = complete_events();
    reordered[0] = event(2, DriverEventKind::HostStarted);
    ensure_not_valid(&reordered, "non-contiguous sequence must be rejected")?;
    let mut reordered = complete_events();
    reordered.swap(4, 8);
    ensure_not_valid(&reordered, "out-of-order lifecycle events must be rejected")?;
    Ok(())
}

#[test]
fn registration_must_bind_the_canonical_command_and_candidate_digest() -> Result<()> {
    let mut events = complete_events();
    events[2] = detail_event(
        3,
        DriverEventKind::RegistrationSelected,
        &[("cmd", "perllsp--http"), ("candidate_sha256", &registration_digest())],
    );
    ensure_not_valid(&events, "a non-canonical command identity must be rejected")?;
    let mut events = complete_events();
    events[2] =
        detail_event(3, DriverEventKind::RegistrationSelected, &[("cmd", "perllsp--stdio")]);
    ensure_not_valid(&events, "registration without the candidate digest must be rejected")?;
    let mut events = complete_events();
    events[2] = detail_event(
        3,
        DriverEventKind::RegistrationSelected,
        &[("cmd", "perllsp--stdio"), ("candidate_sha256", "deadbeef")],
    );
    ensure_not_valid(&events, "a malformed candidate digest must be rejected")?;
    Ok(())
}

#[test]
fn pre_forced_filetype_cannot_manufacture_activation() -> Result<()> {
    // #7762 law: a buffer attachment whose filetype was not natively
    // detected by Vim is not native activation, no matter what the filetype
    // value says.
    let mut events = complete_events();
    events[5] = detail_event(
        6,
        DriverEventKind::BufferEnabled,
        &[("filetype", "perl"), ("detection", "manual_setf")],
    );
    ensure_not_valid(&events, "a pre-forced filetype must be rejected")?;
    let mut events = complete_events();
    events[5] = detail_event(
        6,
        DriverEventKind::BufferEnabled,
        &[("filetype", "pod"), ("detection", "native_vim")],
    );
    ensure_not_valid(&events, "a non-perl attachment must be rejected")?;
    let mut events = complete_events();
    events[5] = detail_event(
        6,
        DriverEventKind::BufferEnabled,
        &[("filetype", "perl"), ("detection", "unobserved")],
    );
    ensure_not_valid(&events, "an unobserved detection route must be rejected")?;
    Ok(())
}

#[test]
fn driver_failure_is_typed_and_terminal() -> Result<()> {
    let mut events = complete_events();
    events.push(detail_event(12, DriverEventKind::DriverFailed, &[("reason", "attach_timeout")]));
    ensure_not_valid(&events, "a complete run cannot also carry driver_failed")?;
    let mut events = complete_events();
    events.truncate(6);
    events.push(detail_event(8, DriverEventKind::DriverFailed, &[("reason", "budget_exhausted")]));
    ensure_not_valid(&events, "driver_failed before the ordered lifecycle completed is invalid")?;
    Ok(())
}

#[test]
fn thin_adapter_and_driver_never_force_a_filetype_or_second_orchestration() -> Result<()> {
    // Static source law: the thin adapter and driver may not set a filetype
    // (the #7762 native-detection law), may not write receipts, and may not
    // spawn processes themselves (Rust owns supervision).
    let adapter =
        fs::read_to_string(repo_root().join("scripts/test/vim-clients/vim-lsp-adapter.vim"))?;
    let driver = fs::read_to_string(repo_root().join("scripts/test/vim-host-driver.vim"))?;
    for (label, source) in [("adapter", &adapter), ("driver", &driver)] {
        for forbidden in ["setf ", "setlocal filetype", "set filetype", "filetype=perl"] {
            ensure!(
                !source.contains(forbidden),
                "{label} contains forbidden Vimscript `{forbidden}`: {label} must stay a thin \
                 native adapter (no filetype forcing, a thin native adapter may not force a filetype (#7762 native-detection law)"
            );
        }
        let has_system_call = source.lines().any(|line| {
            let code = line.split('"').next().unwrap_or_default();
            code.match_indices("system(").any(|(index, _)| {
                match index.checked_sub(1).and_then(|i| code.as_bytes().get(i)) {
                    Some(byte) if byte.is_ascii_alphanumeric() || *byte == b'_' => false,
                    _ => true,
                }
            })
        });
        ensure!(
            !has_system_call
                && !source.contains("job_start")
                && !source.contains("term_start"),
            "{label} must not spawn processes; the Rust supervisor owns process supervision"
        );
    }
    // The adapter writes no files and emits no receipts: only the driver
    // appends typed events, and receipts are Rust-owned.
    ensure!(
        !adapter.contains("writefile(") && !adapter.contains("json_encode("),
        "adapter must not write artifacts; the Rust supervisor owns receipts and evidence"
    );
    // The adapter registers the exact absolute candidate delivered by the
    // Rust wrapper, never a bare `perllsp` name that ambient PATH could
    // satisfy with a wrong binary.
    ensure!(
        adapter.contains("'cmd': {server_info -> [s:candidate, '--stdio']}"),
        "adapter must launch the exact env-delivered candidate executable"
    );
    // The env boundary must use getenv(), not expand(): expand() returns the
    // literal "$NAME" text for unset variables, so the fail-closed empty
    // checks would never fire through it.
    for (label, source) in [("adapter", &adapter), ("driver", &driver)] {
        ensure!(
            !source.contains("expand('$PERLLSP_VIM_HOST"),
            "{label} must not read the run contract through expand() (unset variables expand              to their literal text, defeating fail-closed checks)"
        );
    }
    ensure!(
        adapter.contains("s:Env('PERLLSP_VIM_HOST_CANDIDATE')"),
        "adapter must resolve the candidate through the wrapper environment boundary"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Hermetic isolation
// ---------------------------------------------------------------------------

fn scratch_plan(root: &Path, timeout_ms: u64) -> Result<VimHostRunPlan> {
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
    let fixture_root = materialize_harness_fixture(&root.join("fixture"))?;
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
                id: "vim_vim_lsp_host_lifecycle_v1".to_string(),
                digest: fixture_digest(&fixture_root)?,
                expectation_set_id: CANONICAL_EXPECTATION_SET_ID.to_string(),
                expectation_set_digest: canonical_expectation_set_digest()?,
            },
            journey_selector: "vim_vim_lsp_host_lifecycle.v1".to_string(),
            platform: PlatformIdentity {
                os: "linux".to_string(),
                os_version: "test".to_string(),
                arch: "x86_64".to_string(),
            },
            registration_state: RegistrationState::ManualClientRegistration,
            timeout_ms,
        },
        paths: VimHostPaths {
            vim_executable: vim,
            vim_lsp_checkout: checkout,
            driver,
            adapter,
            candidate_executable: candidate,
            fixture_root,
            artifact_root: root.join("artifacts"),
        },
    })
}

#[test]
fn validated_plan_passes_and_missing_client_fails_closed() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_plan(dir.path(), 60_000)?;
    ensure!(plan.validate().is_ok(), "constructed scratch plan must validate");
    // A checkout without the plugin entry is a typed plan failure, never a
    // skipped pass.
    fs::remove_file(plan.paths.vim_lsp_checkout.join("plugin/lsp.vim"))?;
    ensure!(plan.validate().is_err(), "missing vim-lsp entry must refuse the plan");
    Ok(())
}

#[test]
fn hermetic_environment_redirects_user_state_and_binds_exact_candidate() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_plan(dir.path(), 60_000)?;
    let layout = vim_host_runner::HermeticVimLayout::prepare(&dir.path().join("hermetic"))?;
    let markers = vec![".perl-lsp.toml".to_string()];
    let command = build_vim_command(&plan, &layout, "perllsp-under-test", &markers)?;

    // The launch line loads no user config: no vimrc, no gvimrc, no swap,
    // no viminfo, headless silent-ex mode, and the driver as the only
    // sourced script.
    let args: Vec<String> =
        command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();
    for pair in [["-Nu", "NONE"], ["-U", "NONE"], ["-i", "NONE"]] {
        let mut window = args.windows(pair.len());
        ensure!(
            window.any(|window| {
                window.iter().map(|value| value.as_str()).collect::<Vec<_>>() == pair
            }),
            "vim launch must pass {} (is: {args:?})",
            pair.join(" ")
        );
    }
    for single in ["-n", "-es"] {
        ensure!(
            args.iter().any(|value| value == single),
            "vim launch must pass {single} (is: {args:?})"
        );
    }
    ensure!(command.get_envs().count() > 0, "hermetic env must be explicit");

    // The environment carries the exact candidate absolute path — the
    // registration source consumes this value, so ambient PATH can never
    // select another perllsp.
    let environment: BTreeMap<String, String> = command
        .get_envs()
        .filter_map(|(key, value)| {
            let key = key.to_str()?.to_string();
            let value = value?.to_str()?.to_string();
            Some((key, value))
        })
        .collect();
    ensure!(
        environment.get("PERLLSP_VIM_HOST_CANDIDATE").map(String::as_str)
            == Some(
                vim_host_runner::vim_path(&plan.paths.candidate_executable).to_str().unwrap_or("")
            ),
        "the wrapper must deliver the exact absolute candidate executable Vim-normalized"
    );
    ensure!(
        !environment
            .get("PERLLSP_VIM_HOST_CANDIDATE")
            .map(|value| value.contains('\\'))
            .unwrap_or(true),
        "editor-bound paths must use forward slashes on every host"
    );
    ensure!(
        environment.get("PERLLSP_VIM_HOST_ROOT_MARKERS").map(String::as_str)
            == Some(".perl-lsp.toml"),
        "the wrapper must deliver the #7762 marker list"
    );
    let home = environment.get("HOME").cloned().unwrap_or_default();
    let isolated_root = vim_host_runner::vim_path(dir.path()).to_string_lossy().into_owned();
    ensure!(
        home.starts_with(&isolated_root),
        "HOME must be redirected into the isolated run root, not the user home ({home})"
    );
    ensure!(
        environment.get("PERLLSP_VIM_HOST_CLIENT_LOG")
            != environment.get("PERLLSP_VIM_HOST_SERVER_TRACE"),
        "client log and server trace must remain separate targets"
    );
    ensure!(
        layout.client_log() != layout.server_trace()
            && layout.client_log() != layout.capability_snapshot(),
        "layout keeps client, server, and capability evidence in separate files"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Pinned-subject verification (offline, against synthetic checkouts)
// ---------------------------------------------------------------------------

fn init_subject_checkout(dir: &Path, blob_manifest: &mut VimLspSubjectManifest) -> Result<()> {
    fs::create_dir_all(dir.join("plugin"))?;
    fs::write(dir.join("plugin/lsp.vim"), "\" pinned plugin entry\n")?;
    let run = |args: &[&str]| -> Result<String> {
        let output = Command::new("git").arg("-C").arg(dir).args(args).output()?;
        ensure!(output.status.success(), "git {} failed in fixture", args.join(" "));
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    };
    run(&["init", "-q"])?;
    run(&["add", "."])?;
    run(&[
        "-c",
        "user.name=fixture",
        "-c",
        "user.email=fixture@example.invalid",
        "commit",
        "-q",
        "-m",
        "subject",
    ])?;
    let head = run(&["rev-parse", "HEAD"])?;
    let tree = run(&["rev-parse", "HEAD^{tree}"])?;
    let blob = run(&["hash-object", "--", "plugin/lsp.vim"])?;
    blob_manifest.upstream.selected_commit = head;
    blob_manifest.upstream.tree_digest = Some(vim_host_runner::VimLspTreeDigest {
        algorithm: "git-tree-sha1".to_string(),
        value: tree,
    });
    blob_manifest.expected_content_identity.entry_files = vec![vim_host_runner::VimLspEntryFile {
        path: "plugin/lsp.vim".to_string(),
        git_blob_sha1: blob,
    }];
    Ok(())
}

fn subject_manifest_for(_dir: &Path) -> VimLspSubjectManifest {
    VimLspSubjectManifest {
        schema_version: "vim_lsp_subject.v1".to_string(),
        upstream: vim_host_runner::VimLspUpstream {
            selected_commit: String::new(),
            tree_digest: None,
        },
        expected_content_identity: vim_host_runner::VimLspExpectedContent { entry_files: vec![] },
    }
}

#[test]
fn checked_subject_manifest_parses_and_pins_the_11369_commit() -> Result<()> {
    let manifest = VimLspSubjectManifest::load(
        &repo_root().join(".ci/editor-clients/vim-vim-lsp-subject.v1.json"),
    )?;
    ensure!(
        manifest.upstream.selected_commit == "e10d186452743beb7b43d2b3427020832f930c2b",
        "the checked subject manifest must still pin the #11369 commit"
    );
    ensure!(
        manifest
            .expected_content_identity
            .entry_files
            .iter()
            .any(|entry| entry.path == "plugin/lsp.vim"),
        "the checked subject manifest must pin the plugin entry"
    );
    Ok(())
}

#[test]
fn subject_manifest_refuses_wrong_schema_and_malformed_identity() -> Result<()> {
    ensure!(
        VimLspSubjectManifest::parse(br#"{"schema_version":"vim_lsp_subject.v2"}"#).is_err(),
        "unknown manifest schema must be refused"
    );
    let malformed = format!(
        r#"{{"schema_version":"vim_lsp_subject.v1","upstream":{{"selected_commit":"{}"}},"expected_content_identity":{{"entry_files":[]}}}}"#,
        "z".repeat(40)
    );
    ensure!(
        VimLspSubjectManifest::parse(malformed.as_bytes()).is_err(),
        "a non-hex pinned commit must be refused"
    );
    Ok(())
}

#[test]
fn checkout_verification_accepts_the_exact_subject_and_refuses_every_drift() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let checkout = dir.path().join("vim-lsp");
    let mut manifest = subject_manifest_for(&checkout);
    init_subject_checkout(&checkout, &mut manifest)?;

    let identity = vim_host_runner::verify_vim_lsp_checkout(&checkout, &manifest)?;
    ensure!(identity.verified_entry_count == 1, "the entry file must be verified");

    // Wrong bytes: tamper the pinned entry -> blob digest mismatch.
    fs::write(checkout.join("plugin/lsp.vim"), "\" tampered entry\n")?;
    ensure!(
        vim_host_runner::verify_vim_lsp_checkout(&checkout, &manifest).is_err(),
        "a tampered entry file must refuse the subject"
    );
    fs::write(checkout.join("plugin/lsp.vim"), "\" pinned plugin entry\n")?;

    // Dirty worktree: uncommitted changes are not the pinned subject.
    fs::write(checkout.join("plugin/extra.vim"), "\" stray\n")?;
    ensure!(
        vim_host_runner::verify_vim_lsp_checkout(&checkout, &manifest).is_err(),
        "a dirty checkout must refuse the subject"
    );
    fs::remove_file(checkout.join("plugin/extra.vim"))?;

    // Wrong head: another commit is a different subject, never this run.
    manifest.upstream.selected_commit = "d".repeat(40);
    ensure!(
        vim_host_runner::verify_vim_lsp_checkout(&checkout, &manifest).is_err(),
        "a checkout at another commit must refuse the subject"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Authority-manifest consumption
// ---------------------------------------------------------------------------

#[test]
fn configuration_manifest_is_consumed_not_copied() -> Result<()> {
    let (manifest, digest) = load_configuration_manifest(&repo_root())?;
    ensure!(digest.starts_with("sha256:"), "configuration digest must be recorded");
    ensure!(
        manifest.registration.command_identity.argv == ["perllsp", "--stdio"],
        "the canonical argv law is consumed from the manifest"
    );
    ensure!(
        manifest.registration.server_name == "perllsp-under-test",
        "the canonical server name is consumed from the manifest"
    );
    Ok(())
}

#[test]
fn activation_root_manifest_is_consumed_for_markers_and_fallback() -> Result<()> {
    let (_manifest, markers, digest) = load_activation_root_manifest(&repo_root())?;
    ensure!(!markers.is_empty(), "markers must be consumed");
    ensure!(markers.contains(&".perl-lsp.toml".to_string()), "the primary marker is present");
    ensure!(digest.starts_with("sha256:"), "activation-root digest must be recorded");
    Ok(())
}

#[test]
fn harness_fixture_materializes_a_root_marker_and_native_perl_entry() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let root = materialize_harness_fixture(&dir.path().join("fixture"))?;
    ensure!(root.join(".perl-lsp.toml").is_file(), "fixture carries the #7762 root marker");
    ensure!(root.join("main.pl").is_file(), "fixture carries the native Perl entry");
    ensure!(root.join("lib/My/Widget.pm").is_file(), "fixture carries the lib tree");
    Ok(())
}

// ---------------------------------------------------------------------------
// Host-feature and identity-packet laws
// ---------------------------------------------------------------------------

#[test]
fn vim_feature_probe_requires_transport_features() -> Result<()> {
    ensure!(
        verify_vim_features("VIM - Vi IMproved 9.2\n+channel +job +timers").is_ok(),
        "a host with all transport features is admitted"
    );
    ensure!(
        verify_vim_features("VIM - Vi IMproved 9.2\n+job +timers -channel").is_err(),
        "a host without +channel is a typed failure"
    );
    ensure!(
        verify_vim_features("VIM - Vi IMproved 8.0\n-channel -job -timers").is_err(),
        "a tiny build without transport features is a typed failure"
    );
    Ok(())
}

#[test]
fn identity_packet_must_be_the_canonical_server_packet() -> Result<()> {
    ensure!(
        validate_identity_packet(
            r#"{"schema_version":"perl_lsp.binary_identity.v1","binary":{"executable":"perllsp","role":"server"}}"#
        )
        .is_ok(),
        "canonical packet validates"
    );
    ensure!(
        validate_identity_packet(
            r#"{"schema_version":"perl_lsp.binary_identity.v1","binary":{"executable":"perl-dap","role":"server"}}"#
        )
        .is_err(),
        "another executable identity must be refused"
    );
    ensure!(
        validate_identity_packet(
            r#"{"schema_version":"perl_lsp.binary_identity.v1","binary":{"executable":"perllsp","role":"client"}}"#
        )
        .is_err(),
        "a non-server role must be refused"
    );
    ensure!(validate_identity_packet("not json").is_err(), "malformed packet must be refused");
    Ok(())
}

#[test]
fn candidate_build_revision_binds_the_executable_to_the_repository() -> Result<()> {
    let commit = "4a7559123d6a251ed99737adab103a8dbbe4e419";
    // The real output shape: the embedded identity rides a later line as a
    // short commit sha, and only the first line is the version string.
    ensure!(
        bind_candidate_build_revision("perllsp 0.17.0\nGit commit: 4a7559123\n", commit).is_ok(),
        "a short embedded commit that prefixes the repository commit binds"
    );
    ensure!(
        bind_candidate_build_revision(
            "perllsp 0.17.0\nGit commit: 4a7559123d6a251ed99737adab103a8dbbe4e419\n",
            commit
        )
        .is_ok(),
        "a full embedded commit that equals the repository commit binds"
    );
    ensure!(
        bind_candidate_build_revision("perllsp 0.17.0\nGit commit: deadbee\n", commit).is_err(),
        "a stale executable whose embedded commit disagrees must be refused"
    );
    ensure!(
        bind_candidate_build_revision("perllsp 0.17.0\nGit tag: v0.18.0\n", commit).is_err(),
        "a tag-identified build cannot serve the exact_source_local stage"
    );
    ensure!(
        bind_candidate_build_revision("perllsp 0.17.0\nGit revision: tarball\n", commit).is_err(),
        "a revision-identified build cannot serve the exact_source_local stage"
    );
    ensure!(
        bind_candidate_build_revision("perllsp 0.17.0\n", commit).is_err(),
        "a version output with no commit identity must be refused"
    );
    ensure!(
        bind_candidate_build_revision("perllsp 0.17.0\nGit commit: xyz\n", commit).is_err(),
        "a non-hex revision token must be refused"
    );
    Ok(())
}

#[test]
fn teardown_deferred_shutdown_passes_only_on_teardown_evidence() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_plan(dir.path(), 60_000)?;
    let digest = plan.identity.candidate_artifact_sha256.clone();
    let deferred_events = {
        let mut events = complete_events_with_digest(&digest);
        events[10] = detail_event(
            11,
            DriverEventKind::ShutdownCompleted,
            &[("server_exited", "0"), ("exit_evidence", "deferred_to_editor_teardown")],
        );
        events
    };
    // The pinned vim-lsp loses the job-exit callback in the stop/kill race;
    // the driver defers to the editor teardown. With the client's own
    // teardown trace (`s:on_exit`) in the post-run log plus an orderly
    // supervisor-observed process boundary, the cell passes with the finding
    // recorded in its limitation.
    let observation =
        observation_with(Some(0), false, CleanupResult::Pass, deferred_events.clone());
    let wire = WireEvidence {
        saw_initialize: true,
        saw_initialized: true,
        saw_client_exit_log: true,
        client_capabilities: Some(serde_json::json!({})),
        ..WireEvidence::default()
    };
    let judgment = evaluate_observation(&plan, &observation, &wire)?;
    ensure!(judgment.result == ObservationResult::Pass, "the substrate judgment stays a pass");
    let journey = outcome_journey(&observation, &wire);
    let Some(shutdown_cell) = journey.iter().find(|cell| cell.id == "shutdown_completed") else {
        bail!("the shutdown_completed cell is missing from the journey");
    };
    ensure!(
        shutdown_cell.result == ObservationResult::Pass,
        "teardown trace plus orderly boundary prove the cell"
    );
    ensure!(
        shutdown_cell.limitation.as_deref().is_some_and(|text| text.contains("stop/kill race")),
        "the passing cell limitation must still name the recorded finding"
    );
    // Without the teardown trace the same run stays not-proven: the finding
    // alone never substitutes for evidence.
    let no_trace = WireEvidence {
        saw_initialize: true,
        saw_initialized: true,
        client_capabilities: Some(serde_json::json!({})),
        ..WireEvidence::default()
    };
    let journey = outcome_journey(&observation, &no_trace);
    let Some(shutdown_cell) = journey.iter().find(|cell| cell.id == "shutdown_completed") else {
        bail!("the shutdown_completed cell is missing from the journey");
    };
    ensure!(
        shutdown_cell.result == ObservationResult::NotProven,
        "without the teardown trace the cell must stay not-proven"
    );
    Ok(())
}

#[test]
fn observed_cleanup_leak_with_orderly_exit_is_a_failure() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_plan(dir.path(), 60_000)?;
    let digest = plan.identity.candidate_artifact_sha256.clone();
    let events = complete_events_with_digest(&digest);
    // Exit 0, complete journey, wire attach identity — but the deterministic
    // after-probe observed a surviving candidate process: a leak is a
    // failure, never a not-proven.
    let observation = observation_with(Some(0), false, CleanupResult::Fail, events);
    let wire = WireEvidence {
        saw_initialize: true,
        saw_initialized: true,
        client_capabilities: Some(serde_json::json!({})),
        ..WireEvidence::default()
    };
    let judgment = evaluate_observation(&plan, &observation, &wire)?;
    ensure!(judgment.result == ObservationResult::Fail, "an observed leak must fail the run");
    ensure!(
        judgment.failure_class == Some(xtask::editor_client_compat::FailureClass::Cleanup),
        "the leak must carry the cleanup failure class"
    );
    Ok(())
}

#[test]
fn exact_path_needle_does_not_attribute_foreign_perllsp_processes() -> Result<()> {
    let before = parse_process_snapshot("10 /unrelated/checkout/perllsp --stdio\n20 vim -es")?;
    let after = parse_process_snapshot(
        "10 /unrelated/checkout/perllsp --stdio\n31 /exact/run/perllsp --stdio",
    )?;
    ensure!(
        surviving_processes(&before, &after, "/exact/run/perllsp").len() == 1,
        "the run's own candidate is still detected"
    );
    ensure!(
        surviving_processes(&before, &after, "/another/checkout/perllsp").is_empty(),
        "a foreign perllsp from another checkout is never attributed to this run"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Wire-evidence mining
// ---------------------------------------------------------------------------

#[test]
fn wire_evidence_mines_initialize_lifecycle_and_diagnostics() -> Result<()> {
    let log = concat!(
        "12:00:00 [client] --> {\n",
        "12:00:01 [client] {\"method\":\"initialize\",\"params\":{\"capabilities\":{\"positionEncoding\":null}}}\n",
        "12:00:02 {\"method\":\"initialized\"}\n",
        "12:00:03 {\"method\":\"textDocument/publishDiagnostics\",\"params\":{}}\n",
        "12:00:04 {\"method\":\"shutdown\"}\n",
        "12:00:05 {\"method\":\"exit\"}\n"
    );
    let evidence = extract_wire_evidence(log.as_bytes());
    ensure!(evidence.saw_initialize, "initialize must be mined");
    ensure!(evidence.saw_initialized, "initialized must be mined");
    ensure!(evidence.saw_shutdown, "shutdown must be mined");
    ensure!(evidence.saw_exit, "exit must be mined");
    ensure!(evidence.saw_publish_diagnostics, "push diagnostics must be mined");
    ensure!(evidence.client_capabilities.is_some(), "client capabilities must be captured");

    let empty = extract_wire_evidence(b"no json here\n");
    ensure!(!empty.saw_initialize, "an empty log proves nothing");
    Ok(())
}

#[test]
fn wire_evidence_mines_the_client_teardown_exit_trace() -> Result<()> {
    let with_trace = concat!(
        "12:00:01 [\"--->\",1,\"perllsp-under-test\",{\"method\":\"initialize\"}]
",
        "12:00:02 [\"<---\",1,\"perllsp-under-test\",{\"response\":{\"method\":\"initialized\"}}]
",
        "12:00:03 [\"s:on_exit\",1,\"perllsp-under-test\",\"exited\",-1]
"
    );
    let evidence = extract_wire_evidence(with_trace.as_bytes());
    ensure!(
        evidence.saw_client_exit_log,
        "the client's own teardown exit trace must be mined from the log"
    );
    ensure!(
        evidence.saw_initialize && evidence.saw_initialized,
        "the wire lifecycle still mines normally alongside the trace"
    );
    let without_trace = concat!(
        "12:00:01 [\"s:on_stdout\",1,\"noise\"]
",
        "12:00:02 [\"s:on_request\",1,{\"id\":2,\"method\":\"workspace/configuration\"}]
"
    );
    let evidence = extract_wire_evidence(without_trace.as_bytes());
    ensure!(
        !evidence.saw_client_exit_log,
        "other lifecycle trace labels must not be mistaken for the exit trace"
    );
    Ok(())
}

#[test]
fn wire_evidence_derives_capability_and_diagnostic_identities() -> Result<()> {
    let offered = WireEvidence {
        client_capabilities: Some(serde_json::json!({"positionEncoding": "utf-8"})),
        ..WireEvidence::default()
    };
    let capabilities =
        capabilities_from_wire_evidence(&offered, Some("sha256:".to_string() + &"a".repeat(64)))?;
    ensure!(capabilities.position_encoding_basis == PositionEncodingBasis::Offered);
    ensure!(capabilities.position_encoding_selected.as_deref() == Some("utf-8"));

    let no_offer = WireEvidence::default();
    let capabilities =
        capabilities_from_wire_evidence(&no_offer, Some("sha256:".to_string() + &"a".repeat(64)))?;
    ensure!(capabilities.position_encoding_basis == PositionEncodingBasis::ProtocolDefault);
    ensure!(capabilities.position_encoding_selected.as_deref() == Some("utf-16"));

    let no_snapshot = capabilities_from_wire_evidence(&no_offer, None)?;
    ensure!(no_snapshot.position_encoding_basis == PositionEncodingBasis::NotProven);

    let push = diagnostics_from_wire_evidence(&WireEvidence {
        saw_publish_diagnostics: true,
        ..WireEvidence::default()
    });
    ensure!(push.advertised_mode == DiagnosticMode::Push);
    ensure!(!push.observed_messages.is_empty());
    let none = diagnostics_from_wire_evidence(&WireEvidence::default());
    ensure!(none.advertised_mode == DiagnosticMode::NotProven);
    Ok(())
}

// ---------------------------------------------------------------------------
// Process-set comparison
// ---------------------------------------------------------------------------

#[test]
fn process_snapshots_parse_deterministically_and_reject_garbage() -> Result<()> {
    let lines = parse_process_snapshot("  10 /usr/bin/perllsp --stdio\n20 vim -es\n\n")?;
    ensure!(lines.len() == 2, "two well-formed lines parse");
    ensure!(lines[0].pid == 10, "lines sort by pid");
    ensure!(
        parse_process_snapshot("not-a-pid args\n").is_err(),
        "an unparseable probe line is evidence failure, not an empty set"
    );
    Ok(())
}

#[test]
fn surviving_process_comparison_catches_an_intentional_leak() -> Result<()> {
    // POSIX needles are the full configured executable path, exactly as the
    // runners pass them; since the component-boundary law (#12794 P1) a bare
    // name never matches mid-path, so fixtures use caller-shaped needles.
    let before = parse_process_snapshot(
        "10 /usr/bin/perllsp --stdio\n20 vim -es\n15 /tmp/x/perllsp --stdio",
    )?;
    let after = parse_process_snapshot(
        "15 /tmp/x/perllsp --stdio\n21 kate settings\n31 /tmp/x/perllsp --stdio",
    )?;
    let survivors = surviving_processes(&before, &after, "/tmp/x/perllsp");
    ensure!(survivors.len() == 1, "the leaked perllsp must be detected");
    ensure!(
        survivors[0].pid == 31,
        "the survivor is the candidate process new since the before-snapshot"
    );

    let clean = parse_process_snapshot("15 /tmp/x/perllsp --stdio")?;
    ensure!(
        surviving_processes(&before, &clean, "/tmp/x/perllsp").is_empty(),
        "a pre-existing process that ends is not a survivor"
    );
    // The needle binds the exact candidate path: unrelated new processes and
    // prefix-sharing helper names never match.
    ensure!(
        surviving_processes(&before, &after, "vim").is_empty(),
        "the comparison is scoped to the candidate needle"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Owned-process bounding
// ---------------------------------------------------------------------------

/// Spawned as a child by `run_owned_process_bounds_and_kills_a_hung_host`.
/// When the sleep variable is unset (a normal test invocation) it returns
/// immediately, so it costs nothing in ordinary runs.
#[test]
#[ignore = "spawned as a sleeper child by the bounded-kill contract test"]
fn sleeper_child_helper() {
    if let Ok(ms) = std::env::var("PERLLSP_VIM_HOST_TEST_SLEEP_MS") {
        let ms: u64 = ms.parse().unwrap_or(30_000);
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }
}

#[test]
fn run_owned_process_bounds_and_kills_a_hung_host() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_plan(dir.path(), 800)?;
    let layout = vim_host_runner::HermeticVimLayout::prepare(&dir.path().join("hermetic"))?;
    let mut command = Command::new(std::env::current_exe()?);
    command
        .args(["--exact", "sleeper_child_helper", "--ignored", "--test-threads=1"])
        .env("PERLLSP_VIM_HOST_TEST_SLEEP_MS", "30000");
    let observation = vim_host_runner::run_owned_process(&mut command, &plan, &layout)?;
    ensure!(observation.timed_out, "a hung host must hit the parent-owned deadline");
    ensure!(observation.kill_requested, "a hung host must be killed by the parent");
    ensure!(
        observation.cleanup != CleanupResult::Pass,
        "a killed host can never claim proven cleanup"
    );
    ensure!(!observation.driver_complete, "a killed host never completed the driver contract");
    ensure!(!observation.passed_process_boundary(), "a killed host fails the process boundary");
    Ok(())
}

// ---------------------------------------------------------------------------
// Outcome judgment (thin-adapter failure is reported, never a pass)
// ---------------------------------------------------------------------------

fn observation_with(
    status: Option<i32>,
    timed_out: bool,
    cleanup: CleanupResult,
    events: Vec<DriverEvent>,
) -> vim_host_runner::ProcessObservation {
    let driver_complete = validate_driver_events(&events, true).is_ok();
    vim_host_runner::ProcessObservation {
        status_code: status,
        timed_out,
        kill_requested: timed_out,
        cleanup,
        cleanup_detail: "test".to_string(),
        events,
        driver_complete,
        artifacts: Vec::new(),
    }
}

#[test]
fn driver_failure_before_receipt_is_reported_not_skipped() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_plan(dir.path(), 60_000)?;
    let events = vec![
        event(1, DriverEventKind::HostStarted),
        detail_event(2, DriverEventKind::DriverFailed, &[("reason", "attach_timeout")]),
    ];
    let observation = observation_with(Some(2), false, CleanupResult::NotProven, events);
    let wire = WireEvidence::default();
    let judgment = evaluate_observation(&plan, &observation, &wire)?;
    ensure!(judgment.result == ObservationResult::Fail, "an instrument failure must fail");
    ensure!(judgment.failure_class.is_some(), "the failure must carry a class");

    // A wrong candidate digest attestation is an environment failure: the
    // run attached to something other than the planned exact candidate.
    let mut events = complete_events();
    events[2] = detail_event(
        3,
        DriverEventKind::RegistrationSelected,
        &[("cmd", "perllsp--stdio"), ("candidate_sha256", &format!("sha256:{}", "b".repeat(64)))],
    );
    let observation = observation_with(Some(0), false, CleanupResult::Pass, events);
    let judgment = evaluate_observation(&plan, &observation, &wire)?;
    ensure!(
        judgment.result != ObservationResult::Pass,
        "a registration digest mismatch can never pass"
    );
    Ok(())
}

#[test]
fn attach_identity_requires_the_wire_initialize_sequence() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_plan(dir.path(), 60_000)?;
    let digest = plan.identity.candidate_artifact_sha256.clone();
    let mut events = complete_events();
    events[2] = detail_event(
        3,
        DriverEventKind::RegistrationSelected,
        &[("cmd", "perllsp--stdio"), ("candidate_sha256", &digest)],
    );
    let observation = observation_with(Some(0), false, CleanupResult::Pass, events);
    let no_wire = WireEvidence::default();
    let judgment = evaluate_observation(&plan, &observation, &no_wire)?;
    ensure!(
        judgment.result != ObservationResult::Pass,
        "without the initialize/initialized wire identity the run is not proven"
    );
    let with_wire = WireEvidence {
        saw_initialize: true,
        saw_initialized: true,
        saw_publish_diagnostics: true,
        client_capabilities: Some(serde_json::json!({})),
        ..WireEvidence::default()
    };
    let judgment = evaluate_observation(&plan, &observation, &with_wire)?;
    ensure!(judgment.result == ObservationResult::Pass, "a complete honest run passes");
    Ok(())
}

// ---------------------------------------------------------------------------
// Receipt composition and the stale-receipt law
// ---------------------------------------------------------------------------

fn minimal_receipt(
    plan: &VimHostRunPlan,
    result: ObservationResult,
) -> Result<EditorClientCompatReceipt> {
    let wire = WireEvidence {
        saw_initialize: true,
        saw_initialized: true,
        saw_publish_diagnostics: true,
        client_capabilities: Some(serde_json::json!({})),
        ..WireEvidence::default()
    };
    let observation = observation_with(
        Some(0),
        false,
        CleanupResult::Pass,
        complete_events_with_digest(&plan.identity.candidate_artifact_sha256),
    );
    let capabilities = capabilities_from_wire_evidence(
        &wire,
        Some(plan.identity.candidate_artifact_sha256.clone()),
    )?;
    let diagnostics = diagnostics_from_wire_evidence(&wire);
    let journey = vec![
        JourneyCell {
            id: "host_started".to_string(),
            capability_basis: CapabilityBasis::NotApplicable,
            observed: true,
            result: ObservationResult::Pass,
            evidence: vec!["vim/driver-events.jsonl".to_string()],
            limitation: None,
        },
        JourneyCell {
            id: "process_boundary".to_string(),
            capability_basis: CapabilityBasis::NotApplicable,
            observed: true,
            result: ObservationResult::Pass,
            evidence: vec!["vim/process-ledger.json".to_string()],
            limitation: None,
        },
    ];
    let mut receipt = vim_host_runner::build_receipt(
        plan,
        &observation,
        capabilities,
        diagnostics,
        journey,
        result,
        None,
        vec!["harness substrate proof only".to_string()],
        "#10944 test receipt".to_string(),
    );
    // Give the pass receipt its required artifact kinds.
    receipt.artifacts = vec![
        artifact(ArtifactKind::ClientLog),
        artifact(ArtifactKind::ServerStderr),
        artifact(ArtifactKind::CapabilitySnapshot),
        artifact(ArtifactKind::ProcessLedger),
    ];
    Ok(receipt)
}

fn artifact(kind: ArtifactKind) -> xtask::editor_client_compat::EvidenceArtifact {
    xtask::editor_client_compat::EvidenceArtifact {
        kind,
        id: format!("vim/{kind:?}"),
        sha256: format!("sha256:{}", "a".repeat(64)),
    }
}

fn complete_events_with_digest(digest: &str) -> Vec<DriverEvent> {
    let mut events = complete_events();
    events[2] = detail_event(
        3,
        DriverEventKind::RegistrationSelected,
        &[("cmd", "perllsp--stdio"), ("candidate_sha256", digest)],
    );
    events
}

#[test]
fn canonical_receipt_composes_and_binds_the_vim_subject() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_plan(dir.path(), 60_000)?;
    let receipt = minimal_receipt(&plan, ObservationResult::Pass)?;
    ensure!(
        receipt.validate().is_ok(),
        "a passing composed receipt must satisfy the generic schema"
    );
    ensure!(receipt.host.product == "vim", "host product is vim");
    ensure!(receipt.host.client_id == "vim-lsp", "client id is the pinned plugin token");
    ensure!(
        receipt.server.launch_command == vec!["perllsp".to_string(), "--stdio".to_string()],
        "the launch command is the canonical argv"
    );
    ensure!(
        validate_receipt_binding(&receipt, &plan).is_ok(),
        "the receipt binds its own run plan"
    );
    Ok(())
}

#[test]
fn stale_receipt_from_another_run_is_rejected() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let plan = scratch_plan(dir.path(), 60_000)?;
    let receipt = minimal_receipt(&plan, ObservationResult::Pass)?;

    // A different run plan (new candidate build, new fixture, new plugin
    // bytes, new host) must refuse the old receipt at every binding seam.
    let mut other = scratch_plan(&dir.path().join("other"), 60_000)?;
    fs::write(&other.paths.candidate_executable, b"different candidate bytes")?;
    other.identity.candidate_artifact_sha256 =
        vim_host_runner::file_sha256(&other.paths.candidate_executable)?;
    other.identity.vim_lsp_commit = "e".repeat(40);
    other.identity.vim_build_sha256 = format!("sha256:{}", "f".repeat(64));
    ensure!(
        validate_receipt_binding(&receipt, &other).is_err(),
        "a receipt from another run must be rejected"
    );

    // Subject mismatch: the generic receipt of another editor family is not
    // this runner's subject.
    let mut foreign = minimal_receipt(&plan, ObservationResult::Pass)?;
    foreign.host.product = "emacs".to_string();
    foreign.host.client_id = "eglot".to_string();
    ensure!(
        validate_receipt_binding(&foreign, &plan).is_err(),
        "a canonical receipt with the wrong subject must be rejected"
    );
    Ok(())
}

#[test]
fn reused_output_root_refuses_the_run() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let out = dir.path().join("out");
    fs::create_dir_all(&out)?;
    ensure!(
        xtask::vim_host_run::ensure_fresh_output_root(&out).is_err(),
        "a prior run's output root must refuse a new run (stale receipts cannot be inherited)"
    );
    ensure!(
        xtask::vim_host_run::ensure_fresh_output_root(&dir.path().join("fresh")).is_ok(),
        "a fresh output root is accepted"
    );
    Ok(())
}

fn ensure_not_valid(events: &[DriverEvent], because: &str) -> Result<()> {
    ensure!(
        validate_driver_events(events, true).is_err(),
        "driver contract violation not rejected: {because}"
    );
    Ok(())
}
