// The hermetic runner substrate is consumed progressively by the sibling
// actual-host verdict targets (#7126/#7721/#7727); this contract target
// exercises only the schema/event/receipt core, so under-consumed substrate
// items are expected rather than drift.
#![allow(dead_code)]
#![allow(unused_imports)]

#[path = "support/emacs_host_runner.rs"]
mod emacs_host_runner;

use anyhow::{Result, ensure};
use emacs_host_runner::{
    DRIVER_SCHEMA_VERSION, DriverEvent, DriverEventKind, RUN_PLAN_SCHEMA_VERSION,
    default_not_proven_diagnostics, validate_driver_events,
};
use std::collections::BTreeMap;
use xtask::editor_client_compat::{DiagnosticMode, DiagnosticsIdentity};

fn event(sequence: u64, kind: DriverEventKind) -> DriverEvent {
    DriverEvent {
        schema_version: DRIVER_SCHEMA_VERSION.to_string(),
        sequence,
        kind,
        details: BTreeMap::new(),
    }
}

/// `validate_driver_events` pairs host actions by their `action_id` detail, so
/// an action event without one is rejected before any ordering rule is reached.
/// Building the accepted trace without these details did not merely fail the
/// positive test: it also made every negative test below pass for the wrong
/// reason, since a missing `action_id` rejects the trace on its own.
fn action_event(sequence: u64, kind: DriverEventKind, action_id: &str) -> DriverEvent {
    let mut observation = event(sequence, kind);
    observation.details.insert("action_id".to_string(), action_id.to_string());
    observation
}

fn complete_events() -> Vec<DriverEvent> {
    vec![
        event(1, DriverEventKind::HostStarted),
        event(2, DriverEventKind::ClientLoaded),
        event(3, DriverEventKind::RegistrationSelected),
        event(4, DriverEventKind::InitializeObserved),
        event(5, DriverEventKind::WorkspaceReady),
        event(6, DriverEventKind::BufferOpened),
        action_event(7, DriverEventKind::HostActionStarted, "rename_module"),
        action_event(8, DriverEventKind::HostActionCompleted, "rename_module"),
        event(9, DriverEventKind::EditApplied),
        event(10, DriverEventKind::ShutdownStarted),
        event(11, DriverEventKind::ShutdownCompleted),
    ]
}

#[test]
fn runner_contract_uses_versioned_run_and_driver_schemas() {
    assert_eq!(RUN_PLAN_SCHEMA_VERSION, "emacs_host_run_plan.v1");
    assert_eq!(DRIVER_SCHEMA_VERSION, "emacs_host_driver.v1");
}

#[test]
fn diagnostics_contract_uses_the_canonical_type() -> Result<()> {
    let diagnostics: DiagnosticsIdentity = default_not_proven_diagnostics();
    ensure!(
        diagnostics.advertised_mode == DiagnosticMode::NotProven,
        "default diagnostics must fail closed"
    );
    ensure!(
        diagnostics.observed_messages.is_empty(),
        "not-proven diagnostics cannot manufacture observations"
    );
    Ok(())
}

#[test]
fn ordered_complete_driver_trace_is_accepted() -> Result<()> {
    validate_driver_events(&complete_events(), true)
}

#[test]
fn unidentified_and_mismatched_host_actions_are_rejected() {
    let mut missing_action_id = complete_events();
    missing_action_id[6].details.clear();
    assert!(validate_driver_events(&missing_action_id, true).is_err());

    let mut mismatched_action_id = complete_events();
    mismatched_action_id[7].details.insert("action_id".to_string(), "other_action".to_string());
    assert!(validate_driver_events(&mismatched_action_id, true).is_err());
}

#[test]
fn sequence_gap_and_missing_shutdown_are_rejected() {
    let mut sequence_gap = complete_events();
    sequence_gap[3].sequence = 99;
    assert!(validate_driver_events(&sequence_gap, true).is_err());

    let mut missing_shutdown = complete_events();
    missing_shutdown.pop();
    assert!(validate_driver_events(&missing_shutdown, true).is_err());
}

#[test]
fn lifecycle_reordering_and_unclosed_action_are_rejected() {
    let mut reordered = complete_events();
    reordered.swap(1, 2);
    for (index, observation) in reordered.iter_mut().enumerate() {
        observation.sequence = (index + 1) as u64;
    }
    assert!(validate_driver_events(&reordered, true).is_err());

    let mut unclosed_action = complete_events();
    unclosed_action.remove(7);
    for (index, observation) in unclosed_action.iter_mut().enumerate() {
        observation.sequence = (index + 1) as u64;
    }
    assert!(validate_driver_events(&unclosed_action, true).is_err());
}

#[test]
fn schema_drift_is_rejected() {
    let mut drifted = complete_events();
    drifted[0].schema_version = "emacs_host_driver.v2".to_string();
    assert!(validate_driver_events(&drifted, true).is_err());
}

// ---------------------------------------------------------------------------
// Bundled-Eglot client subject (#7778 slice 1): the adapter, the subject
// registry, and the run-plan builder are checked-in surfaces, so CI can
// discriminate their structure without an Emacs host. Real-host execution
// runs through `cargo xtask integration emacs host-run` on provisioned
// hosts only; a host that is not provided is never reported green.
// ---------------------------------------------------------------------------

use anyhow::Context as _;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use xtask::editor_client_compat::fixture_digest;
use xtask::emacs_host_run as host_run_task;

fn workspace_root() -> Result<PathBuf> {
    Ok(Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .context("xtask must live directly under the workspace root")?
        .to_path_buf())
}

fn read_checked(relative: &str) -> Result<String> {
    let path = workspace_root()?.join(relative);
    fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))
}

/// A minimal one-row subject manifest whose declared digest matches the
/// fake bundled eglot bytes, so plan-builder mechanics stay hermetic while
/// the bundled path is digest-validated through the subject resolver
/// (#11744). The checked manifest's real rows stay pinned by the subject
/// manifest contract tests.
fn fixture_subject_manifest() -> Result<xtask::emacs_subject_manifest::SubjectManifest> {
    use sha2::{Digest as ShaDigest, Sha256};
    use xtask::editor_client_compat::ClientSourceState;
    use xtask::emacs_subject_manifest::{
        DigestAudit, MANIFEST_SCHEMA_VERSION, MaterializationMethod, SubjectClientKind,
        SubjectManifest, SubjectRow,
    };
    let mut hasher = Sha256::new();
    hasher.update(b";; fake bundled eglot.el\n");
    let digest = format!(
        "sha256:{}",
        hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect::<String>()
    );
    Ok(SubjectManifest {
        schema_version: MANIFEST_SCHEMA_VERSION.to_string(),
        subjects: vec![SubjectRow {
            subject_id: "bundled_eglot_emacs_30_1".to_string(),
            client_kind: SubjectClientKind::BundledEglot,
            source_state: ClientSourceState::Bundled,
            emacs_release_tag: "emacs-30.1".to_string(),
            emacs_version_token: "30.1".to_string(),
            client_version_hint: "1.17.30".to_string(),
            client_source_relative_path: "lisp/progmodes/eglot.el".to_string(),
            client_source_sha256: digest,
            materialization: MaterializationMethod::InstallationRootResolution,
            client_library_forms: vec!["eglot.el".to_string()],
            external_package: None,
            source_tree: None,
            digest_audit: DigestAudit {
                gnu_tarball_url: "https://ftp.gnu.org/gnu/emacs/fixture.tar.xz".to_string(),
                gnu_tarball_sha256: format!("sha256:{}", "0".repeat(64)),
                observed_client_version_header: "1.17.30".to_string(),
            },
        }],
    })
}

const ADAPTER: &str = "scripts/test/emacs-clients/eglot-bundled.el";
const CONFIGURATION: &str = "scripts/test/emacs-clients/eglot-bundled-config.el";

#[test]
fn bundled_adapter_defines_the_driver_entrypoint_and_bundled_resolution() -> Result<()> {
    let adapter = read_checked(ADAPTER)?;
    assert!(
        adapter.contains("(defun perl-lsp-test-client-run"),
        "the adapter must define the entrypoint the common driver calls"
    );
    assert!(adapter.contains("(require 'eglot)"), "the adapter must load Eglot itself");
    // Bundled proof: resolution inside the running build, not an ambient
    // archive or cache that merely carries a matching version header.
    assert!(adapter.contains("(locate-library \"eglot\")"));
    assert!(adapter.contains("invocation-directory"));
    assert!(
        adapter.contains("(string-prefix-p emacs-root library)"),
        "the resolved library must be proven to live inside the running Emacs build"
    );
    assert!(
        adapter.contains("(secure-hash 'sha256"),
        "the loaded file digest must be emitted as runtime identity evidence"
    );
    assert!(
        adapter.contains("(insert-file-contents-literally"),
        "digests must be computed over raw bytes: the decoded insert-file-contents performs \
         coding-system and line-ending translation that silently changes the hash"
    );
    assert!(
        adapter.contains("(lm-version"),
        "the loaded file's own version header must be observed"
    );
    Ok(())
}

#[test]
fn bundled_adapter_registers_exactly_one_manual_candidate_row() -> Result<()> {
    let adapter = read_checked(ADAPTER)?;
    assert!(
        adapter.contains("(setq eglot-server-programs"),
        "the adapter must own the registration table"
    );
    assert!(
        adapter.contains("(perl-mode . ,contact)") && adapter.contains("(cperl-mode . ,contact)"),
        "the manual candidate contact must be the whole table for both Perl modes"
    );
    assert!(
        adapter.contains("(list candidate \"--stdio\")"),
        "the single manual contact must launch the exact candidate over stdio"
    );
    for forbidden in [
        "package-initialize",
        "package-archives",
        "package-install",
        "package-refresh-contents",
        "use-package",
        "add-to-list 'eglot-server-programs",
        "add-to-list #'eglot-server-programs",
    ] {
        assert!(
            !adapter.contains(forbidden),
            "the hermetic adapter must never touch ambient package state: {forbidden}"
        );
    }
    Ok(())
}

#[test]
fn bundled_adapter_binds_the_observed_server_process_to_the_candidate() -> Result<()> {
    let adapter = read_checked(ADAPTER)?;
    assert!(
        adapter.contains("(process-command"),
        "the adapter must observe the program the live server process was started as"
    );
    assert!(
        adapter.contains("PERL_LSP_EMACS_CANDIDATE"),
        "the candidate identity comes from the run plan environment"
    );
    assert!(
        adapter.contains("non-candidate server program"),
        "an observed program that differs from the candidate must fail the run"
    );
    Ok(())
}

#[test]
fn bundled_adapter_keeps_client_log_and_server_stderr_separate() -> Result<()> {
    let adapter = read_checked(ADAPTER)?;
    assert!(adapter.contains("jsonrpc-events-buffer"));
    assert!(adapter.contains("jsonrpc-stderr-buffer"));
    assert!(adapter.contains("PERL_LSP_EMACS_CLIENT_LOG"));
    assert!(adapter.contains("PERL_LSP_EMACS_SERVER_STDERR"));
    // The two exports must be distinct writes: a single combined sink would
    // conflate client events with server output, which the runner's negative
    // controls forbid.
    let client_index = adapter.find("PERL_LSP_EMACS_CLIENT_LOG");
    let stderr_index = adapter.find("PERL_LSP_EMACS_SERVER_STDERR");
    assert!(client_index.is_some() && stderr_index.is_some());
    assert_ne!(client_index, stderr_index);
    Ok(())
}

#[test]
fn bundled_adapter_emits_the_full_lifecycle_barrier_ladder() -> Result<()> {
    let adapter = read_checked(ADAPTER)?;
    for barrier in [
        "client_loaded",
        "registration_selected",
        "initialize_observed",
        "workspace_ready",
        "buffer_opened",
        "shutdown_started",
        "shutdown_completed",
    ] {
        assert!(
            adapter.contains(&format!("\"{barrier}\"")),
            "the adapter must emit the {barrier} driver barrier"
        );
    }
    assert!(
        adapter.contains("PERL_LSP_EMACS_CAPABILITY_SNAPSHOT"),
        "the initialize capability snapshot must be written through the run plan path"
    );
    Ok(())
}

/// `eglot--connect` performs no class defaulting of its own: a nil class
/// reaches `make-instance` and breaks the run. The bundled adapter must pin
/// the stock class explicitly.
#[test]
fn bundled_adapter_pins_the_connect_class_explicitly() -> Result<()> {
    let adapter = read_checked(ADAPTER)?;
    assert!(
        adapter.contains("'eglot-lsp-server contact"),
        "the adapter must pass the stock eglot-lsp-server class to eglot--connect"
    );
    assert!(
        !adapter.contains("nil contact"),
        "a nil class argument breaks make-instance in eglot--connect"
    );
    Ok(())
}

/// `eglot-shutdown`'s optionals are (SERVER _INTERACTIVE TIMEOUT
/// PRESERVE-BUFFERS): passing the timeout first would silently leave the
/// buffers to be killed before the exports run. The adapter must pass the
/// ignored interactive slot explicitly and preserve the evidence buffers.
#[test]
fn bundled_adapter_shuts_down_with_preserved_evidence_buffers() -> Result<()> {
    let adapter = read_checked(ADAPTER)?;
    assert!(
        adapter.contains("(eglot-shutdown server nil"),
        "the adapter must skip the ignored interactive slot of eglot-shutdown"
    );
    assert!(
        !adapter.contains("(eglot-shutdown server perl-lsp-test-bundled-shutdown-deadline t)"),
        "the timeout must not land in the ignored interactive slot"
    );
    Ok(())
}

#[test]
fn bundled_configuration_carries_only_client_behavior_settings() -> Result<()> {
    let configuration = read_checked(CONFIGURATION)?;
    for setting in ["eglot-sync-connect", "eglot-autoreconnect", "eglot-autoshutdown"] {
        assert!(configuration.contains(setting), "the checked configuration must pin {setting}");
    }
    for forbidden in ["package-", "eglot-server-programs", "load-file"] {
        assert!(
            !configuration.contains(forbidden),
            "the checked configuration must not touch {forbidden} state"
        );
    }
    Ok(())
}

#[test]
fn subject_registry_pins_each_exact_client_subject_immutable() -> Result<()> {
    assert_eq!(
        host_run_task::EmacsClientSubject::known_ids(),
        &[
            "bundled_eglot_emacs_29_4",
            "bundled_eglot_emacs_30_1",
            "released_eglot_gnu_elpa_1_23",
            "released_eglot_gnu_elpa_1_24",
            "source_eglot_emacs_c1ad9d27",
            "released_lsp_mode_melpa_stable_10_0_0",
            "source_lsp_mode_github_6bfc593",
        ],
        "each subject is an immutable registry row; new releases are new rows, never silent \
         replacements"
    );
    let manifest = xtask::emacs_subject_manifest::SubjectManifest::load(&workspace_root()?)?;
    let subject = host_run_task::EmacsClientSubject::BundledEglotEmacs301.client_identity(
        &manifest,
        format!("sha256:{}", "0".repeat(64)),
        None,
    )?;
    assert_eq!(subject.client_id, "bundled_eglot_emacs_30_1");
    assert_eq!(subject.kind, host_run_task::emacs_host_runner::EmacsClientKind::BundledEglot);
    assert_eq!(subject.version, "1.17.30");
    assert_eq!(subject.source_ref, "emacs-30.1");
    assert_eq!(subject.source_state, xtask::editor_client_compat::ClientSourceState::Bundled);
    assert!(
        subject.package_sha256.is_none(),
        "a bundled subject cannot carry a separate package identity"
    );
    let emacs_294 = host_run_task::EmacsClientSubject::BundledEglotEmacs294.client_identity(
        &manifest,
        format!("sha256:{}", "9".repeat(64)),
        None,
    )?;
    assert_eq!(emacs_294.client_id, "bundled_eglot_emacs_29_4");
    assert_eq!(emacs_294.version, "1.12.29");
    assert_eq!(emacs_294.source_ref, "emacs-29.4");
    assert_eq!(
        host_run_task::EmacsClientSubject::BundledEglotEmacs294.pinned_emacs_version_token(),
        "29.4"
    );

    let released = host_run_task::EmacsClientSubject::ReleasedEglotGnuElpa123.client_identity(
        &manifest,
        format!("sha256:{}", "1".repeat(64)),
        Some(format!("sha256:{}", "2".repeat(64))),
    )?;
    assert_eq!(released.client_id, "released_eglot_gnu_elpa_1_23");
    assert_eq!(released.kind, host_run_task::emacs_host_runner::EmacsClientKind::ExternalEglot);
    assert_eq!(released.version, "1.23");
    assert_eq!(released.source_ref, "gnu-elpa-eglot-1.23");
    assert_eq!(released.source_state, xtask::editor_client_compat::ClientSourceState::Released);
    assert!(
        released.package_sha256.is_some(),
        "a released subject must carry an exact package identity"
    );
    // Subject dispatch surfaces: the released row points at its own adapter
    // and configuration, never the bundled ones.
    assert_eq!(
        host_run_task::EmacsClientSubject::ReleasedEglotGnuElpa123.adapter_relative_path(),
        "scripts/test/emacs-clients/eglot-released.el"
    );
    assert_eq!(
        host_run_task::EmacsClientSubject::ReleasedEglotGnuElpa123.configuration_relative_path(),
        "scripts/test/emacs-clients/eglot-released-config.el"
    );
    assert_eq!(
        host_run_task::EmacsClientSubject::ReleasedEglotGnuElpa123.journey_selector(),
        "released_eglot_lifecycle.v1"
    );
    assert!(host_run_task::EmacsClientSubject::ReleasedEglotGnuElpa123.requires_client_package());
    assert!(!host_run_task::EmacsClientSubject::BundledEglotEmacs301.requires_client_package());
    Ok(())
}

#[test]
fn fixture_materialization_is_deterministic_and_bounded() -> Result<()> {
    let first = tempfile::tempdir()?;
    let second = tempfile::tempdir()?;
    let first_root = host_run_task::materialize_client_subject_fixture(&first.path().join("f"))?;
    let second_root = host_run_task::materialize_client_subject_fixture(&second.path().join("f"))?;
    let first_digest = fixture_digest(&first_root)?;
    let second_digest = fixture_digest(&second_root)?;
    ensure!(first_digest == second_digest, "fixture materialization must be deterministic");
    ensure!(first_root.join("script/probe.pl").is_file());
    ensure!(first_root.join("lib/My/Thing.pm").is_file());
    Ok(())
}

fn fake_exact_inputs(root: &Path) -> Result<(PathBuf, PathBuf, PathBuf)> {
    let candidate_name = if cfg!(windows) { "perllsp.exe" } else { "perllsp" };
    let emacs = root.join("emacs-exe");
    let candidate = root.join(candidate_name);
    let client_source = root.join("eglot.el");
    fs::write(&emacs, b"fake exact emacs executable bytes")?;
    fs::write(&candidate, b"fake exact perllsp candidate bytes")?;
    fs::write(&client_source, b";; fake bundled eglot.el\n")?;
    Ok((emacs, candidate, client_source))
}

#[test]
fn run_plan_builder_fails_closed_when_the_exact_host_is_absent() -> Result<()> {
    let root = tempfile::tempdir()?;
    let (_emacs, candidate, client_source) = fake_exact_inputs(root.path())?;
    let missing_emacs = root.path().join("absent-emacs");
    let run = host_run_task::EmacsHostRunInputs {
        emacs_executable: missing_emacs.clone(),
        candidate_executable: candidate,
        client_source,
        client_package: None,
        out_root: root.path().join("out-missing"),
        timeout_ms: 0,
    };
    let error = host_run_task::build_client_subject_run_plan(
        &workspace_root()?,
        host_run_task::EmacsClientSubject::BundledEglotEmacs301,
        &run,
        &"0".repeat(40),
        "perllsp fake",
        "GNU Emacs 30.1 (fake)",
        &fixture_subject_manifest()?,
    )
    .err()
    .context("a missing exact host must not produce a runnable plan")?;
    assert!(
        error.to_string().contains("absent-emacs"),
        "the failure must name the missing exact input: {error}"
    );
    assert!(!missing_emacs.exists(), "the missing host must not have been created");
    Ok(())
}

#[test]
fn run_plan_builder_validates_over_the_checked_tree_with_exact_fake_inputs() -> Result<()> {
    let root = tempfile::tempdir()?;
    let (emacs, candidate, client_source) = fake_exact_inputs(root.path())?;
    let run = host_run_task::EmacsHostRunInputs {
        emacs_executable: emacs,
        candidate_executable: candidate.clone(),
        client_source,
        client_package: None,
        out_root: root.path().join("out"),
        timeout_ms: 0,
    };
    let (plan, layout) = host_run_task::build_client_subject_run_plan(
        &workspace_root()?,
        host_run_task::EmacsClientSubject::BundledEglotEmacs301,
        &run,
        "0123456789abcdef0123456789abcdef01234567",
        "perllsp fake",
        "GNU Emacs 30.1 (fake)",
        &fixture_subject_manifest()?,
    )?;
    ensure!(
        plan.identity.timeout_ms == 180_000,
        "timeout_ms=0 must fall back to the bounded default"
    );
    ensure!(plan.identity.journey_selector == "bundled_eglot_lifecycle.v1");

    let command = host_run_task::emacs_host_runner::build_emacs_command(&plan, &layout)?;
    let argv: Vec<String> =
        command.get_args().map(|argument| argument.to_string_lossy().into_owned()).collect();
    let driver_index = argv
        .iter()
        .position(|argument| argument.ends_with("emacs-host-driver.el"))
        .context("driver must be on the command line")?;
    let adapter_index = argv
        .iter()
        .position(|argument| argument.ends_with("eglot-bundled.el"))
        .context("adapter must be on the command line")?;
    let funcall_index = argv
        .iter()
        .position(|argument| argument == "perl-lsp-test-run")
        .context("the driver entrypoint must be funcalled")?;
    ensure!(driver_index < adapter_index, "the driver must load before the adapter");
    ensure!(adapter_index < funcall_index, "the entrypoint runs after both loads");
    ensure!(argv.contains(&"-Q".to_string()));
    ensure!(argv.contains(&"--no-site-file".to_string()));
    ensure!(argv.contains(&"--batch".to_string()));

    let environment: std::collections::BTreeMap<&OsStr, &OsStr> =
        command.get_envs().filter_map(|(name, value)| value.map(|value| (name, value))).collect();
    let candidate_binding = environment
        .get(OsStr::new("PERL_LSP_EMACS_CANDIDATE"))
        .copied()
        .context("candidate binding must be present")?;
    ensure!(candidate_binding == candidate.as_os_str());
    let home = environment.get(OsStr::new("HOME")).copied().context("hermetic HOME must be set")?;
    ensure!(home == layout.home.as_os_str());
    Ok(())
}

#[test]
fn subject_helpers_are_exported_for_the_cli_surface() -> Result<()> {
    // The CLI resolves the bundled library inside the exact installation;
    // ambiguity and absence are typed errors, never silent choices.
    let root = tempfile::tempdir()?;
    let bin = root.path().join("bin");
    fs::create_dir_all(&bin)?;
    let emacs = bin.join("emacs");
    fs::write(&emacs, b"fake")?;
    let error = host_run_task::resolve_bundled_client_source(&emacs)
        .err()
        .context("an empty installation must not resolve a client source")?;
    assert!(
        error.to_string().contains("no bundled Eglot library"),
        "absence must be a typed identity error, got: {error}"
    );
    Ok(())
}

/// Installed Emacs builds commonly load `eglot.elc` while shipping the
/// source as `eglot.el` and/or `eglot.el.gz`. Resolution must accept the
/// compiled form when the source is absent, prefer the source when present,
/// and fail closed on a same-form identity ambiguity.
#[test]
fn bundled_library_resolution_handles_real_installation_forms() -> Result<()> {
    let root = tempfile::tempdir()?;
    let progmodes = root.path().join("lisp/progmodes");
    fs::create_dir_all(&progmodes)?;
    let emacs = root.path().join("bin/emacs");
    fs::create_dir_all(emacs.parent().context("emacs path must have a parent")?)?;
    fs::write(&emacs, b"fake")?;

    let compiled = progmodes.join("eglot.elc");
    fs::write(&compiled, b"bytecode")?;
    let resolved = host_run_task::resolve_bundled_client_source(&emacs)?;
    ensure!(
        fs::canonicalize(&resolved)? == fs::canonicalize(&compiled)?,
        "compiled-only installation must resolve the .elc"
    );

    let source = progmodes.join("eglot.el");
    fs::write(&source, b";; source")?;
    let resolved = host_run_task::resolve_bundled_client_source(&emacs)?;
    ensure!(
        fs::canonicalize(&resolved)? == fs::canonicalize(&source)?,
        "source form must win over the compiled form"
    );

    let second_tree = root.path().join("share/emacs/30.1/lisp/progmodes");
    fs::create_dir_all(&second_tree)?;
    fs::write(second_tree.join("eglot.el"), b";; duplicate")?;
    let error = host_run_task::resolve_bundled_client_source(&emacs)
        .err()
        .context("two same-form libraries must be an identity defect")?;
    assert!(
        error.to_string().contains("ambiguous bundled eglot.el identity"),
        "same-form duplication must fail closed: {error}"
    );
    Ok(())
}

/// The subject pins the exact host build; a different Emacs is a different
/// subject and must be refused before anything is launched.
#[test]
fn pinned_host_version_is_enforced_before_launch() -> Result<()> {
    host_run_task::EmacsClientSubject::BundledEglotEmacs301
        .ensure_pinned_host_version("GNU Emacs 30.1 (build 1, x86_64)")?;
    let wrong = host_run_task::EmacsClientSubject::BundledEglotEmacs301
        .ensure_pinned_host_version("GNU Emacs 29.4")
        .err()
        .context("an unpinned host must be refused")?;
    assert!(
        wrong.to_string().contains("does not match the pinned subject"),
        "the refusal must name the subject pin: {wrong}"
    );
    Ok(())
}

/// A reused output directory concatenates driver event streams and
/// misattributes stale artifacts; the run must refuse it.
#[test]
fn host_run_refuses_a_reused_output_directory() -> Result<()> {
    let root = tempfile::tempdir()?;
    let out = root.path().join("out");
    fs::create_dir_all(&out)?;
    let error = host_run_task::ensure_fresh_output_root(&out)
        .err()
        .context("a reused output root must be refused")?;
    assert!(
        error.to_string().contains("use a fresh directory"),
        "the refusal must demand a fresh directory: {error}"
    );
    host_run_task::ensure_fresh_output_root(&root.path().join("fresh-out"))?;
    Ok(())
}

/// A build revision is a standalone 40-hex run only; longer or shorter hex
/// runs are not commit identities.
#[test]
fn commit_like_tokens_are_standalone_forty_hex_runs() {
    let token = "0123456789abcdef0123456789abcdef01234567";
    assert_eq!(
        host_run_task::extract_commit_like_token(&format!("perllsp 1.2.3 ({token})")),
        Some(token.to_string())
    );
    assert_eq!(host_run_task::extract_commit_like_token("perllsp 1.2.3"), None);
    let forty_one = format!("{:0>41}", "0123456789abcdef0123456789abcdef012345678");
    assert_eq!(
        host_run_task::extract_commit_like_token(&format!("hash {forty_one}")),
        None,
        "a 41-hex run is not a commit identity"
    );
}

// ---------------------------------------------------------------------------
// Released-Eglot client subject (#7778 slice 2): the GNU ELPA 1.23 adapter,
// its package-identity binding, and the released-subject fail-closed laws.
// ---------------------------------------------------------------------------

const RELEASED_ADAPTER: &str = "scripts/test/emacs-clients/eglot-released.el";
const RELEASED_CONFIGURATION: &str = "scripts/test/emacs-clients/eglot-released-config.el";

#[test]
fn released_adapter_defines_entrypoint_and_declared_package_resolution() -> Result<()> {
    let adapter = read_checked(RELEASED_ADAPTER)?;
    assert!(
        adapter.contains("(defun perl-lsp-test-client-run"),
        "the released adapter must define the entrypoint the common driver calls"
    );
    // The declared package file is the only resolution source: its directory
    // is pushed onto load-path and the resolution is then proven equal to
    // the declared file, so the bundled copy or an ambient cache entry that
    // answered instead fails the run.
    assert!(adapter.contains("PERL_LSP_EMACS_CLIENT_SOURCE"));
    assert!(adapter.contains("PERL_LSP_EMACS_CLIENT_PACKAGE"));
    assert!(adapter.contains("(add-to-list 'load-path (file-name-directory library))"));
    assert!(
        adapter.contains("(string-equal (file-truename resolved)\n                            (file-truename library))"),
        "the resolved library must be proven to be the declared package file"
    );
    assert!(
        adapter.contains("did not resolve to the declared client file"),
        "a foreign resolution must fail the run with a typed reason"
    );
    // Released identity requires the version header, not just the digest.
    assert!(adapter.contains("(lm-version"));
    assert!(
        adapter.contains("carries no version header"),
        "an unreadable version header must fail a released run"
    );
    assert!(
        adapter.contains("(secure-hash 'sha256"),
        "the loaded file digest must be emitted as runtime identity evidence"
    );
    // A top-level `(require 'eglot)' would load the Emacs build's bundled
    // copy before the declared package directory owns `load-path'; the only
    // require must come after the load-path manipulation.
    let require_index = adapter
        .find("(require 'eglot)")
        .context("the adapter must require eglot exactly once, after owning load-path")?;
    let load_path_index = adapter
        .find("(add-to-list 'load-path (file-name-directory library))")
        .context("the adapter must push the declared package directory onto load-path")?;
    ensure!(
        load_path_index < require_index,
        "the eglot require must come after the declared package directory is on load-path"
    );
    ensure!(
        adapter.matches("(require 'eglot)").count() == 1,
        "eglot must be required exactly once so no earlier load can satisfy it"
    );
    assert!(
        adapter.contains("(insert-file-contents-literally"),
        "digests must be computed over raw bytes, including the binary package archive"
    );
    assert!(
        adapter.contains("external Eglot library did not resolve after require"),
        "an unresolvable library must fail with a typed reason, not a nil file-truename error"
    );
    Ok(())
}

#[test]
fn released_adapter_registers_exactly_one_manual_candidate_row() -> Result<()> {
    let adapter = read_checked(RELEASED_ADAPTER)?;
    assert!(
        adapter.contains("(setq eglot-server-programs"),
        "the adapter must own the registration table"
    );
    assert!(
        adapter.contains("(perl-mode . ,contact)") && adapter.contains("(cperl-mode . ,contact)"),
        "the manual candidate contact must be the whole table for both Perl modes"
    );
    assert!(
        adapter.contains("(list candidate \"--stdio\")"),
        "the single manual contact must launch the exact candidate over stdio"
    );
    for forbidden in [
        "package-initialize",
        "package-archives",
        "package-install",
        "package-refresh-contents",
        "use-package",
        "add-to-list 'eglot-server-programs",
        "add-to-list #'eglot-server-programs",
    ] {
        assert!(
            !adapter.contains(forbidden),
            "the hermetic released adapter must never touch ambient package state: {forbidden}"
        );
    }
    Ok(())
}

#[test]
fn released_adapter_binds_process_log_and_lifecycle_like_the_bundled_one() -> Result<()> {
    let adapter = read_checked(RELEASED_ADAPTER)?;
    assert!(adapter.contains("(process-command"));
    assert!(adapter.contains("non-candidate server program"));
    assert!(adapter.contains("jsonrpc-events-buffer"));
    assert!(adapter.contains("jsonrpc-stderr-buffer"));
    assert!(adapter.contains("PERL_LSP_EMACS_CLIENT_LOG"));
    assert!(adapter.contains("PERL_LSP_EMACS_SERVER_STDERR"));
    for barrier in [
        "client_loaded",
        "registration_selected",
        "initialize_observed",
        "workspace_ready",
        "buffer_opened",
        "shutdown_started",
        "shutdown_completed",
    ] {
        assert!(
            adapter.contains(&format!("\"{barrier}\"")),
            "the released adapter must emit the {barrier} driver barrier"
        );
    }
    // The slice-1 review repairs are pinned here too: explicit connect
    // class and shutdown optional order.
    assert!(adapter.contains("'eglot-lsp-server contact"));
    assert!(adapter.contains("(eglot-shutdown server nil"));
    Ok(())
}

#[test]
fn released_configuration_carries_only_client_behavior_settings() -> Result<()> {
    let configuration = read_checked(RELEASED_CONFIGURATION)?;
    for setting in ["eglot-sync-connect", "eglot-autoreconnect", "eglot-autoshutdown"] {
        assert!(configuration.contains(setting), "the checked configuration must pin {setting}");
    }
    for forbidden in ["package-", "eglot-server-programs", "load-file"] {
        assert!(
            !configuration.contains(forbidden),
            "the checked configuration must not touch {forbidden} state"
        );
    }
    Ok(())
}

#[test]
fn released_run_plan_requires_and_binds_package_identity() -> Result<()> {
    let root = tempfile::tempdir()?;
    let (_emacs, candidate, client_source) = fake_exact_inputs(root.path())?;
    let package = root.path().join("eglot-1.23.tar");
    fs::write(&package, b"fake released package bytes")?;
    let run = host_run_task::EmacsHostRunInputs {
        emacs_executable: root.path().join("emacs-exe"),
        candidate_executable: candidate,
        client_source,
        client_package: None,
        out_root: root.path().join("out-no-package"),
        timeout_ms: 0,
    };
    let missing = host_run_task::build_client_subject_run_plan(
        &workspace_root()?,
        host_run_task::EmacsClientSubject::ReleasedEglotGnuElpa123,
        &run,
        "0123456789abcdef0123456789abcdef01234567",
        "perllsp fake",
        "GNU Emacs 30.1 (fake)",
        // The released row predates the subject manifest and does not
        // consult its rows; the checked manifest is the honest authority.
        &xtask::emacs_subject_manifest::SubjectManifest::load(&workspace_root()?)?,
    )
    .err()
    .context("a released subject without its package file must not produce a plan")?;
    assert!(
        missing.to_string().contains("requires an exact client package file"),
        "the failure must name the missing package identity: {missing}"
    );

    let run = host_run_task::EmacsHostRunInputs {
        emacs_executable: root.path().join("emacs-exe"),
        candidate_executable: root.path().join(if cfg!(windows) {
            "perllsp.exe"
        } else {
            "perllsp"
        }),
        client_source: root.path().join("eglot.el"),
        client_package: Some(package.clone()),
        out_root: root.path().join("out"),
        timeout_ms: 0,
    };
    let (plan, layout) = host_run_task::build_client_subject_run_plan(
        &workspace_root()?,
        host_run_task::EmacsClientSubject::ReleasedEglotGnuElpa123,
        &run,
        "0123456789abcdef0123456789abcdef01234567",
        "perllsp fake",
        "GNU Emacs 30.1 (fake)",
        &xtask::emacs_subject_manifest::SubjectManifest::load(&workspace_root()?)?,
    )?;
    ensure!(
        plan.identity.client.package_sha256.is_some(),
        "the released plan must carry the package digest"
    );
    ensure!(plan.paths.client_package.as_deref() == Some(package.as_path()));
    ensure!(plan.identity.journey_selector == "released_eglot_lifecycle.v1");
    let command = host_run_task::emacs_host_runner::build_emacs_command(&plan, &layout)?;
    let argv: Vec<String> =
        command.get_args().map(|argument| argument.to_string_lossy().into_owned()).collect();
    ensure!(
        argv.iter().any(|argument| argument.ends_with("eglot-released.el")),
        "the released adapter must be the one loaded"
    );
    let environment: std::collections::BTreeMap<&OsStr, &OsStr> =
        command.get_envs().filter_map(|(name, value)| value.map(|value| (name, value))).collect();
    ensure!(
        environment
            .get(OsStr::new("PERL_LSP_EMACS_CLIENT_SOURCE"))
            .copied()
            .is_some_and(|value| value == root.path().join("eglot.el").as_os_str()),
        "the declared client source must reach the adapter environment"
    );
    ensure!(
        environment
            .get(OsStr::new("PERL_LSP_EMACS_CLIENT_PACKAGE"))
            .copied()
            .is_some_and(|value| value == package.as_os_str()),
        "the declared package file must reach the adapter environment"
    );
    Ok(())
}

#[test]
fn bundled_subject_rejects_a_package_identity() -> Result<()> {
    let root = tempfile::tempdir()?;
    let (_emacs, candidate, client_source) = fake_exact_inputs(root.path())?;
    let package = root.path().join("eglot-1.23.tar");
    fs::write(&package, b"fake released package bytes")?;
    let run = host_run_task::EmacsHostRunInputs {
        emacs_executable: root.path().join("emacs-exe"),
        candidate_executable: candidate,
        client_source,
        client_package: Some(package),
        out_root: root.path().join("out"),
        timeout_ms: 0,
    };
    let error = host_run_task::build_client_subject_run_plan(
        &workspace_root()?,
        host_run_task::EmacsClientSubject::BundledEglotEmacs301,
        &run,
        "0123456789abcdef0123456789abcdef01234567",
        "perllsp fake",
        "GNU Emacs 30.1 (fake)",
        &fixture_subject_manifest()?,
    )
    .err()
    .context("a bundled subject must not accept a package identity")?;
    assert!(
        error.to_string().contains("cannot carry a separate package identity"),
        "the failure must name the identity conflict: {error}"
    );
    Ok(())
}

#[test]
fn released_subject_never_searches_the_host_installation() -> Result<()> {
    let unknown = host_run_task::EmacsClientSubject::from_id("made_up_subject")
        .err()
        .context("an unknown subject must be a typed error")?;
    assert!(
        unknown.to_string().contains("unknown client subject"),
        "unknown ids must fail closed: {unknown}"
    );
    ensure!(
        !host_run_task::EmacsClientSubject::ReleasedEglotGnuElpa123
            .resolves_client_source_from_installation(),
        "a released subject resolves its source only through declared package inputs"
    );
    Ok(())
}

/// A directly observed leaked `perllsp` fails the run even when the host
/// itself exited 0 through no driver fault — the cleanup facet outranks the
/// not-proven fall-through, mirroring the Vim evaluator's law (#12794
/// review: "Promote observed Emacs cleanup leaks to failure").
#[test]
fn observed_cleanup_leak_fails_an_orderly_run() -> Result<()> {
    use host_run_task::emacs_host_runner as runner_types;
    use runner_types::{DriverEventKind, ProcessObservation};
    use std::collections::BTreeMap as Details;

    let subject_digest = format!("sha256:{}", "c".repeat(64));
    let plan = host_run_task::emacs_host_runner::EmacsHostRunPlan {
        identity: host_run_task::emacs_host_runner::EmacsHostRunIdentity {
            schema_version: RUN_PLAN_SCHEMA_VERSION.to_string(),
            stage: xtask::editor_client_compat::EvidenceStage::ExactSourceLocal,
            repository: "repo".into(),
            candidate_sha: format!("sha256:{}", "a".repeat(64)),
            emacs_version: "GNU Emacs 30.1".into(),
            emacs_build_sha256: format!("sha256:{}", "b".repeat(64)),
            client: host_run_task::emacs_host_runner::ClientSubject {
                client_id: "eglot".into(),
                kind: host_run_task::emacs_host_runner::EmacsClientKind::BundledEglot,
                version: "30.1".into(),
                source_state: xtask::editor_client_compat::ClientSourceState::UpstreamSource,
                source_ref: "bundled".into(),
                source_sha256: subject_digest.clone(),
                package_sha256: None,
            },
            driver_sha256: subject_digest,
            adapter_sha256: format!("sha256:{}", "d".repeat(64)),
            configuration_sha256: format!("sha256:{}", "e".repeat(64)),
            candidate_version: "perl-lsp 0.1.0".into(),
            candidate_build_revision: "f".repeat(40),
            candidate_artifact_sha256: format!("sha256:{}", "0".repeat(64)),
            fixture: xtask::editor_client_compat::WorkspaceFixtureIdentity {
                id: "fixture".into(),
                digest: format!("sha256:{}", "1".repeat(64)),
                expectation_set_id: "set".into(),
                expectation_set_digest: format!("sha256:{}", "2".repeat(64)),
            },
            journey_selector: "selector".into(),
            platform: xtask::editor_client_compat::PlatformIdentity {
                os: "test".into(),
                os_version: "0".into(),
                arch: "x".into(),
            },
            registration_state:
                xtask::editor_client_compat::RegistrationState::ManualClientRegistration,
            timeout_ms: 60_000,
        },
        paths: host_run_task::emacs_host_runner::EmacsHostPaths {
            emacs_executable: std::path::PathBuf::from("emacs"),
            client_source: std::path::PathBuf::from("eglot.el"),
            client_package: None,
            driver: std::path::PathBuf::from("driver.el"),
            adapter: std::path::PathBuf::from("adapter.el"),
            configuration: std::path::PathBuf::from("conf.el"),
            candidate_executable: std::path::PathBuf::from("perllsp"),
            fixture_root: std::path::PathBuf::from("fixture"),
            artifact_root: std::path::PathBuf::from("artifacts"),
        },
    };
    let mut client_loaded_details = Details::new();
    client_loaded_details.insert("source_sha256".to_string(), "c".repeat(64));
    let events = vec![{
        let mut event = runner_types::DriverEvent {
            schema_version: RUN_PLAN_SCHEMA_VERSION.to_string(),
            sequence: 1,
            kind: runner_types::DriverEventKind::ClientLoaded,
            details: client_loaded_details,
        };
        event
    }];
    let observation = ProcessObservation {
        status_code: Some(0),
        timed_out: false,
        kill_requested: false,
        cleanup: xtask::editor_client_compat::CleanupResult::Fail,
        cleanup_detail: "process-set comparison observed 1 surviving candidate process(es)".into(),
        events,
        driver_complete: true,
        artifacts: Vec::new(),
    };
    let judgment = host_run_task::evaluate_observation(&plan, &observation)?;
    ensure!(
        judgment.result == xtask::editor_client_compat::ObservationResult::Fail,
        "an observed leak must fail an otherwise orderly run, got {:?}",
        judgment.result
    );
    ensure!(
        judgment.failure_class == Some(xtask::editor_client_compat::FailureClass::Cleanup),
        "the leak must classify as Cleanup, got {:?}",
        judgment.failure_class
    );
    Ok(())
}
