#[path = "support/editor_client_compat.rs"]
mod editor_client_compat;
#[path = "support/emacs_host_runner.rs"]
mod emacs_host_runner;

use anyhow::{Context, Result, ensure};
use editor_client_compat::{
    ArtifactKind, CANONICAL_EXPECTATION_SET_ID, CapabilityIdentity, ClientSourceState,
    DiagnosticMode, DiagosticsIdentity, EvidenceStage, FailureClass, JourneyCell,
    ObservationResult, PlatformIdentity, PositionEncodingBasis, RegistrationState,
    WorkspaceFixtureIdentity, canonical_expectation_set_digest, fixture_digest,
};
use emacs_host_runner::{
    ClientSubject, DRIVER_SCHEMA_VERSION, DriverEvent, DriverEventKind, EmacsClientKind,
    EmacsHostPaths, EmacsHostRunIdentity, EmacsHostRunPlan, HermeticLayout,
    RUN_PLAN_SCHEMA_VERSION, build_emacs_command, build_receipt, default_not_proven_diagnostics,
    file_sha256, parse_driver_events, run_owned_process, validate_driver_events,
};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

fn repository_root() -> Result<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .context("xtask must live below repository root")
}

fn fixture_root() -> Result<PathBuf> {
    Ok(repository_root()?.join("crates/perl-lsp-ux-tests/fixtures/agent-client-compat"))
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, bytes)?;
    Ok(())
}

struct PlanFixture {
    _inputs: TempDir,
    _run: TempDir,
    plan: EmacsHostRunPlan,
    layout: HermeticLayout,
}

fn plan_fixture(timeout_ms: u64) -> Result<PlanFixture> {
    let inputs = TempDir::new()?;
    let run = TempDir::new()?;
    let current_exe = std::env::current_exe()?;
    let client_source = inputs.path().join("eglot.el");
    let client_package = inputs.path().join("eglot-1.23.tar");
    let adapter = inputs.path().join("eglot-adapter.el");
    let configuration = inputs.path().join("manual-registration.el");
    let candidate = inputs.path().join(if cfg!(windows) { "perllsp.exe" } else { "perllsp" });
    write_file(&client_source, b";;; exact Eglot subject\n")?;
    write_file(&client_package, b"exact package archive\n")?;
    write_file(&adapter, b";; exact client adapter\n")?;
    write_file(&configuration, b";; exact client configuration\n")?;
    write_file(&candidate, b"exact perllsp candidate\n")?;

    let driver = repository_root()?.join("scripts/test/emacs-host-driver.el");
    let fixture = fixture_root()?;
    let layout = HermeticLayout::prepare(run.path())?;
    let client = ClientSubject {
        client_id: "emacs-eglot-1.23".to_string(),
        kind: EmacsClientKind::ExternalEglot,
        version: "1.23".to_string(),
        source_state: ClientSourceState::Released,
        source_ref: "gnu-elpa/eglot-1.23".to_string(),
        source_sha256: file_sha256(&client_source)?,
        package_sha256: Some(file_sha256(&client_package)?),
    };
    let plan = EmacsHostRunPlan {
        identity: EmacsHostRunIdentity {
            schema_version: RUN_PLAN_SCHEMA_VERSION.to_string(),
            stage: EvidenceStage::ExactSourceLocal,
            repository: "EffortlessMetrics/perl-lsp-swarm".to_string(),
            candidate_sha: "a".repeat(40),
            emacs_version: "31.1".to_string(),
            emacs_build_sha256: file_sha256(&current_exe)?,
            client,
            driver_sha256: file_sha256(&driver)?,
            adapter_sha256: file_sha256(&adapter)?,
            configuration_sha256: file_sha256(&configuration)?,
            candidate_version: "0.18.0-dev".to_string(),
            candidate_build_revision: "a".repeat(40),
            candidate_artifact_sha256: file_sha256(&candidate)?,
            fixture: WorkspaceFixtureIdentity {
                id: "perl-agent-client-v1".to_string(),
                digest: fixture_digest(&fixture)?,
                expectation_set_id: CANONICAL_EXPECTATION_SET_ID.to_string(),
                expectation_set_digest: canonical_expectation_set_digest()?,
            },
            journey_selector: "emacs_runner_contract".to_string(),
            platform: PlatformIdentity {
                os: std::env::consts::OS.to_string(),
                os_version: "test-host".to_string(),
                arch: std::env::consts::ARCH.to_string(),
            },
            registration_state: RegistrationState::Manual,
            timeout_ms,
        },
        paths: EmacsHostPaths {
            emacs_executable: current_exe,
            client_source,
            client_package: Some(client_package),
            driver,
            adapter,
            configuration,
            candidate_executable: candidate,
            fixture_root: fixture,
            artifact_root: run.path().to_path_buf(),
        },
    };
    Ok(PlanFixture { _inputs: inputs, _run: run, plan, layout })
}

fn details(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
    entries.iter().map(|(key, value)| ((*key).to_string(), (*value).to_string())).collect()
}

fn complete_events() -> Vec<DriverEvent> {
    vec![
        DriverEvent {
            schema_version: DRIVER_SCHEMA_VERSION.to_string(),
            sequence: 1,
            kind: DriverEventKind::HostStarted,
            details: details(&[("subject", "emacs")]),
        },
        DriverEvent {
            schema_version: DRIVER_SCHEMA_VERSION.to_string(),
            sequence: 2,
            kind: DriverEventKind::ClientLoaded,
            details: details(&[("client", "eglot-1.23")]),
        },
        DriverEvent {
            schema_version: DRIVER_SCHEMA_VERSION.to_string(),
            sequence: 3,
            kind: DriverEventKind::RegistrationSelected,
            details: details(&[("registration", "manual")]),
        },
        DriverEvent {
            schema_version: DRIVER_SCHEMA_VERSION.to_string(),
            sequence: 4,
            kind: DriverEventKind::InitializeObserved,
            details: details(&[("encoding", "utf-16")]),
        },
        DriverEvent {
            schema_version: DRIVER_SCHEMA_VERSION.to_string(),
            sequence: 5,
            kind: DriverEventKind::WorkspaceReady,
            details: details(&[("workspace", "fixture")]),
        },
        DriverEvent {
            schema_version: DRIVER_SCHEMA_VERSION.to_string(),
            sequence: 6,
            kind: DriverEventKind::BufferOpened,
            details: details(&[("buffer", "app.pl")]),
        },
        DriverEvent {
            schema_version: DRIVER_SCHEMA_VERSION.to_string(),
            sequence: 7,
            kind: DriverEventKind::HostActionStarted,
            details: details(&[("action_id", "definition")]),
        },
        DriverEvent {
            schema_version: DRIVER_SCHEMA_VERSION.to_string(),
            sequence: 8,
            kind: DriverEventKind::HostActionCompleted,
            details: details(&[("action_id", "definition")]),
        },
        DriverEvent {
            schema_version: DRIVER_SCHEMA_VERSION.to_string(),
            sequence: 9,
            kind: DriverEventKind::EditApplied,
            details: details(&[("edit", "semantic")]),
        },
        DriverEvent {
            schema_version: DRIVER_SCHEMA_VERSION.to_string(),
            sequence: 10,
            kind: DriverEventKind::ShutdownStarted,
            details: BTreeMap::new(),
        },
        DriverEvent {
            schema_version: DRIVER_SCHEMA_VERSION.to_string(),
            sequence: 11,
            kind: DriverEventKind::ShutdownCompleted,
            details: BTreeMap::new(),
        },
    ]
}

#[test]
fn run_plan_binds_exact_subjects_and_files() -> Result<()> {
    let fixture = plan_fixture(10_000)?;
    fixture.plan.validate()?;

    let encoded = serde_json::to_string_pretty(&fixture.plan.identity)?;
    let decoded: EmacsHostRunIdentity = serde_json::from_str(&encoded)?;
    ensure!(decoded == fixture.plan.identity, "run identity did not round-trip");

    let mut wrong_candidate = fixture.plan.clone();
    wrong_candidate.paths.candidate_executable = wrong_candidate.paths.adapter.clone();
    ensure!(wrong_candidate.validate().is_err(), "runner accepted a non-perllsp candidate path");

    let mut wrong_hash = fixture.plan.clone();
    wrong_hash.identity.client.source_sha256 = format!("sha256:{}", "f".repeat(64));
    ensure!(wrong_hash.validate().is_err(), "runner accepted a mismatched client source");

    let mut wrong_source_state = fixture.plan.clone();
    wrong_source_state.identity.client.kind = EmacsClientKind::BundledEglot;
    ensure!(
        wrong_source_state.validate().is_err(),
        "bundled Eglot accepted released package identity"
    );
    Ok(())
}

#[test]
fn command_and_environment_are_hermetic_and_exact() -> Result<()> {
    let fixture = plan_fixture(10_000)?;
    let command = build_emacs_command(&fixture.plan, &fixture.layout)?;
    ensure!(
        command.get_program() == fixture.plan.paths.emacs_executable.as_os_str(),
        "command selected the wrong Emacs executable"
    );
    let args = command
        .get_args()
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    for required in ["-Q", "--no-site-file", "--batch", "--load", "perl-lsp-test-run"] {
        ensure!(
            args.iter().any(|argument| argument == required),
            "Emacs command omitted {required}"
        );
    }
    let driver = fixture.plan.paths.driver.to_string_lossy().into_owned();
    let adapter = fixture.plan.paths.adapter.to_string_lossy().into_owned();
    ensure!(
        args.iter().any(|argument| argument == &driver),
        "Emacs command omitted the exact common driver"
    );
    ensure!(
        args.iter().any(|argument| argument == &adapter),
        "Emacs command omitted the exact client adapter"
    );

    let environment = fixture.layout.environment(&fixture.plan)?;
    ensure!(
        environment.get(OsStr::new("HOME")) == Some(&fixture.layout.home.as_os_str().to_owned()),
        "HOME was not isolated"
    );
    ensure!(
        environment.get(OsStr::new("XDG_CACHE_HOME"))
            == Some(&fixture.layout.xdg_cache_home.as_os_str().to_owned()),
        "XDG cache was not isolated"
    );
    for forbidden in ["EMACSLOADPATH", "EMACSDATA", "EMACSDOC", "EMACSPATH", "CLAUDE_CONFIG_DIR"] {
        ensure!(
            !environment.contains_key(OsStr::new(forbidden)),
            "ambient variable {forbidden} leaked into the host"
        );
    }
    ensure!(
        environment.get(OsStr::new("PERL_LSP_EMACS_CANDIDATE"))
            == Some(&fixture.plan.paths.candidate_executable.as_os_str().to_owned()),
        "candidate identity was not passed exactly"
    );
    Ok(())
}

#[test]
fn driver_protocol_requires_ordered_complete_host_evidence() -> Result<()> {
    let events = complete_events();
    validate_driver_events(&events, true)?;
    let encoded = events
        .iter()
        .map(serde_json::to_string)
        .collect::<std::result::Result<Vec<_>, _>>()?
        .join("\n");
    ensure!(
        parse_driver_events(encoded.as_bytes(), true)? == events,
        "driver event stream changed during parsing"
    );

    let mut wrong_sequence = events.clone();
    wrong_sequence[3].sequence = 99;
    ensure!(
        validate_driver_events(&wrong_sequence, true).is_err(),
        "non-contiguous driver sequence was accepted"
    );

    let mut wrong_order = events.clone();
    wrong_order.swap(1, 2);
    for (index, event) in wrong_order.iter_mut().enumerate() {
        event.sequence = (index + 1) as u64;
    }
    ensure!(
        validate_driver_events(&wrong_order, true).is_err(),
        "out-of-order lifecycle was accepted"
    );

    let mut incomplete_action = events.clone();
    incomplete_action.remove(7);
    for (index, event) in incomplete_action.iter_mut().enumerate() {
        event.sequence = (index + 1) as u64;
    }
    ensure!(
        validate_driver_events(&incomplete_action, true).is_err(),
        "unclosed host action was accepted"
    );

    let mut missing_shutdown = events;
    missing_shutdown.pop();
    ensure!(
        validate_driver_events(&missing_shutdown, true).is_err(),
        "missing shutdown completion was accepted"
    );
    Ok(())
}

#[test]
fn owned_process_captures_receiptable_evidence_and_redacts_paths() -> Result<()> {
    let fixture = plan_fixture(10_000)?;
    let mut command = helper_command(&fixture, "pass")?;
    let observation = run_owned_process(&mut command, &fixture.plan, &fixture.layout)?;
    ensure!(
        observation.passed_process_boundary(),
        "successful helper did not satisfy the process boundary"
    );
    let artifact_kinds =
        observation.artifacts.iter().map(|artifact| artifact.kind).collect::<BTreeSet<_>>();
    for required in [
        ArtifactKind::ClientLog,
        ArtifactKind::ServerStderr,
        ArtifactKind::CapabilitySnapshot,
        ArtifactKind::ProcessLedger,
    ] {
        ensure!(artifact_kinds.contains(&required), "missing {required:?}");
    }

    let captured_stdout =
        fs::read_to_string(fixture.layout.artifact_directory.join("emacs/driver-stdout.log"))?;
    ensure!(
        captured_stdout.contains("<RUN_ROOT>") && captured_stdout.contains("<CANDIDATE>"),
        "known private paths were not redacted"
    );
    ensure!(
        !captured_stdout.contains(&fixture.layout.root.to_string_lossy().into_owned()),
        "run root leaked into retained stdout"
    );

    let capabilities = CapabilityIdentity {
        initialize_snapshot_sha256: observation
            .artifacts
            .iter()
            .find(|artifact| artifact.kind == ArtifactKind::CapabilitySnapshot)
            .context("capability artifact missing")?
            .sha256
            .clone(),
        position_encodings_offered: vec!["utf-16".to_string()],
        position_encoding_basis: PositionEncodingBasis::Offered,
        position_encoding_selected: Some("utf-16".to_string()),
    };
    let diagnostics = DiagnosticsIdentity {
        advertised_mode: DiagnosticMode::Pull,
        observed_messages: vec![
            "text_document_diagnostic".to_string(),
            "flymake_rendered".to_string(),
        ],
    };
    let receipt = build_receipt(
        &fixture.plan,
        &observation,
        capabilities,
        diagnostics,
        vec![JourneyCell {
            id: "lifecycle.shutdown".to_string(),
            result: ObservationResult::Pass,
            evidence: vec!["driver.shutdown_completed".to_string()],
            limitation: None,
        }],
        ObservationResult::Pass,
        None,
        Vec::new(),
        "Hermetic runner process, artifact, and cleanup cells only.".to_string(),
    );
    receipt.validate()?;
    Ok(())
}

#[test]
fn timeout_is_reaped_but_cannot_become_a_passing_host_receipt() -> Result<()> {
    let fixture = plan_fixture(50)?;
    let mut command = helper_command(&fixture, "timeout")?;
    let observation = run_owned_process(&mut command, &fixture.plan, &fixture.layout)?;
    ensure!(observation.timed_out, "timeout was not recorded");
    ensure!(observation.kill_requested, "timed-out process was not killed");
    ensure!(!observation.passed_process_boundary(), "timed-out process was reported as passed");

    let receipt = build_receipt(
        &fixture.plan,
        &observation,
        CapabilityIdentity {
            initialize_snapshot_sha256: format!("sha256:{}", "0".repeat(64)),
            position_encodings_offered: Vec::new(),
            position_encoding_basis: PositionEncodingBasis::NotProven,
            position_encoding_selected: None,
        },
        default_not_proven_diagnostics(),
        vec![JourneyCell {
            id: "lifecycle.shutdown".to_string(),
            result: ObservationResult::NotProven,
            evidence: Vec::new(),
            limitation: Some("host timed out before shutdown".to_string()),
        }],
        ObservationResult::NotProven,
        Some(FailureClass::Instrument),
        vec!["host timed out and exact client behavior is not proven".to_string()],
        "No actual-host support cells are promoted.".to_string(),
    );
    receipt.validate()?;
    Ok(())
}

fn helper_command(fixture: &PlanFixture, mode: &str) -> Result<Command> {
    let mut command = Command::new(std::env::current_exe()?);
    command
        .arg("--exact")
        .arg("emacs_host_runner_helper_process")
        .arg("--nocapture")
        .arg("--test-threads=1")
        .env("EMACS_HOST_RUNNER_HELPER_MODE", mode)
        .env("PERL_LSP_EMACS_EVENT_FILE", fixture.layout.event_file())
        .env("PERL_LSP_EMACS_CLIENT_LOG", fixture.layout.client_log())
        .env("PERL_LSP_EMACS_SERVER_STDERR", fixture.layout.server_stderr())
        .env("PERL_LSP_EMACS_CAPABILITY_SNAPSHOT", fixture.layout.capability_snapshot())
        .env("EMACS_HOST_RUNNER_PRIVATE_ROOT", &fixture.layout.root)
        .env("EMACS_HOST_RUNNER_CANDIDATE", &fixture.plan.paths.candidate_executable);
    Ok(command)
}

#[test]
fn emacs_host_runner_helper_process() -> Result<()> {
    let Some(mode) = std::env::var_os("EMACS_HOST_RUNNER_HELPER_MODE") else {
        return Ok(());
    };
    if mode == OsStr::new("timeout") {
        thread::sleep(Duration::from_secs(5));
        return Ok(());
    }
    ensure!(mode == OsStr::new("pass"), "unknown helper mode");

    let event_file = required_env_path("PERL_LSP_EMACS_EVENT_FILE")?;
    let events = complete_events();
    let encoded = events
        .iter()
        .map(serde_json::to_string)
        .collect::<std::result::Result<Vec<_>, _>>()?
        .join("\n");
    write_file(&event_file, format!("{encoded}\n").as_bytes())?;
    write_file(
        &required_env_path("PERL_LSP_EMACS_CLIENT_LOG")?,
        b"client selected exact perllsp\n",
    )?;
    write_file(
        &required_env_path("PERL_LSP_EMACS_SERVER_STDERR")?,
        b"server stderr remains separate\n",
    )?;
    write_file(
        &required_env_path("PERL_LSP_EMACS_CAPABILITY_SNAPSHOT")?,
        serde_json::to_string_pretty(&json!({
            "clientInfo": {"name": "Eglot", "version": "1.23"},
            "general": {"positionEncodings": ["utf-16"]}
        }))?
        .as_bytes(),
    )?;

    let private_root = required_env_path("EMACS_HOST_RUNNER_PRIVATE_ROOT")?;
    let candidate = required_env_path("EMACS_HOST_RUNNER_CANDIDATE")?;
    println!("helper root={} candidate={}", private_root.display(), candidate.display());
    eprintln!("helper stderr remains driver-owned");
    Ok(())
}

fn required_env_path(key: &str) -> Result<PathBuf> {
    std::env::var_os(key)
        .map(PathBuf::from)
        .with_context(|| format!("missing helper environment {key}"))
}
