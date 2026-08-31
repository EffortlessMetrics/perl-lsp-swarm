//! Deterministic fake-host fixture for the shared Emacs runner (#8734).
//!
//! Re-enters this test binary so every scenario drives the real
//! `run_owned_process` / artifact / receipt seam. There is no second
//! test-only supervisor.

use super::{
    DRIVER_SCHEMA_VERSION, EmacsClientKind, EmacsHostPaths, EmacsHostRunIdentity, EmacsHostRunPlan,
    HermeticLayout, MAX_CAPTURE_BYTES, RUN_PLAN_SCHEMA_VERSION,
};
use anyhow::{Context, Result, bail, ensure};
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;
use xtask::editor_client_compat::{
    CANONICAL_EXPECTATION_SET_ID, ClientSourceState, EvidenceStage, PlatformIdentity,
    RegistrationState, WorkspaceFixtureIdentity, canonical_expectation_set_digest, fixture_digest,
};

/// Environment switch selecting fake-host behavior. Without it the child is
/// an ordinary harness invocation.
pub const FAKE_HOST_MODE_ENV: &str = "PERL_LSP_FAKE_HOST_MODE";
const FAKE_HOST_DESCENDANT_READY_ENV: &str = "PERL_LSP_FAKE_DESCENDANT_READY";
const FAKE_HOST_ENTRY_ENV: &str = "PERL_LSP_FAKE_ENTRY_TEST";
const DESCENDANT_LIFETIME_CAP_MS: u64 = 120_000;

fn synthetic_sha256(seed: u8) -> String {
    format!("sha256:{}", [seed; 64].iter().map(|byte| format!("{byte:x}")).collect::<String>())
}

fn standalone_forty_hex(seed: u8) -> String {
    [seed; 40].iter().map(|byte| format!("{byte:x}")).collect()
}

/// Build a hermetic run plan whose candidate identity is unique per `tag` so
/// concurrent scenarios cannot attribute each other's processes. Production
/// `plan.validate()` still pins `perllsp[.exe]`; this fixture skips that
/// filename law so Windows image-name probes stay isolated.
pub fn supervision_plan(
    root: &Path,
    tag: &str,
    timeout_ms: u64,
) -> Result<(EmacsHostRunPlan, HermeticLayout)> {
    ensure!(root.is_absolute(), "supervision root must be absolute");
    ensure!(super::is_reason_token(tag), "supervision tag must be a stable reason token");
    let layout = HermeticLayout::prepare(root)?;
    let bin = root.join("bin");
    fs::create_dir_all(&bin).context("creating supervision bin directory")?;
    let fixture_root = root.join("fixture");
    fs::create_dir_all(&fixture_root).context("creating supervision fixture directory")?;
    fs::write(fixture_root.join("probe.pl"), b"print qq{ok\\n};\n")
        .context("writing supervision fixture file")?;

    let executable_suffix = if cfg!(windows) { ".exe" } else { "" };
    let candidate_executable = bin.join(format!("perllsp-{tag}{executable_suffix}"));
    let emacs_executable = bin.join("host-fixture");
    let client_source = bin.join("client-source.el");
    let driver = bin.join("host-driver.el");
    let adapter = bin.join("client-adapter.el");
    let configuration = bin.join("client-config.el");
    for (path, bytes) in [
        (&candidate_executable, &b"supervised candidate"[..]),
        (&emacs_executable, &b"supervised host"[..]),
        (&client_source, &b";; supervised client"[..]),
        (&driver, &b";; supervised driver"[..]),
        (&adapter, &b";; supervised adapter"[..]),
        (&configuration, &b";; supervised configuration"[..]),
    ] {
        fs::write(path, bytes)
            .with_context(|| format!("writing supervision input {}", path.display()))?;
    }

    let identity = EmacsHostRunIdentity {
        schema_version: RUN_PLAN_SCHEMA_VERSION.to_string(),
        stage: EvidenceStage::ExactSourceLocal,
        repository: "repo/supervision".to_string(),
        candidate_sha: standalone_forty_hex(1),
        emacs_version: "GNU Emacs 30.1 (supervision fixture)".to_string(),
        emacs_build_sha256: synthetic_sha256(2),
        client: super::ClientSubject {
            client_id: format!("fake_eglot_{tag}"),
            kind: EmacsClientKind::BundledEglot,
            version: "1.17.30".to_string(),
            source_state: ClientSourceState::Bundled,
            source_ref: "fixture".to_string(),
            source_sha256: synthetic_sha256(3),
            package_sha256: None,
        },
        driver_sha256: synthetic_sha256(4),
        adapter_sha256: synthetic_sha256(5),
        configuration_sha256: synthetic_sha256(6),
        candidate_version: "perllsp supervision".to_string(),
        candidate_build_revision: standalone_forty_hex(7),
        candidate_artifact_sha256: synthetic_sha256(8),
        fixture: WorkspaceFixtureIdentity {
            id: format!("fixture_{tag}"),
            digest: fixture_digest(&fixture_root)?,
            expectation_set_id: CANONICAL_EXPECTATION_SET_ID.to_string(),
            expectation_set_digest: canonical_expectation_set_digest()?,
        },
        journey_selector: "supervision_lifecycle.v1".to_string(),
        platform: PlatformIdentity {
            os: "supervision".to_string(),
            os_version: "fixture".to_string(),
            arch: "fixture".to_string(),
        },
        registration_state: RegistrationState::ManualClientRegistration,
        timeout_ms,
    };
    let plan = EmacsHostRunPlan {
        identity,
        paths: EmacsHostPaths {
            emacs_executable,
            client_source,
            client_package: None,
            driver,
            adapter,
            configuration,
            candidate_executable,
            fixture_root,
            artifact_root: layout.artifact_directory.clone(),
        },
    };
    Ok((plan, layout))
}

/// Same environment surface `build_emacs_command` applies, pointed at the
/// fake-host re-entry instead of a real Emacs binary.
pub fn supervision_command(
    host_executable: &Path,
    entry_test: &str,
    plan: &EmacsHostRunPlan,
    layout: &HermeticLayout,
    mode: &str,
) -> Result<Command> {
    let mut command = Command::new(host_executable);
    command.arg("--exact").arg(entry_test).arg("--nocapture").arg("--test-threads=1");
    for (key, value) in layout.environment(plan)? {
        command.env(key, value);
    }
    command.env(FAKE_HOST_MODE_ENV, mode);
    command.env(FAKE_HOST_ENTRY_ENV, entry_test);
    command.env_remove("RUST_TEST_ARGS");
    Ok(command)
}

pub fn stop_test_descendant(pid: u32) {
    if cfg!(windows) {
        let _ = Command::new("taskkill")
            .args(["/F", "/PID", &pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    } else {
        let _ = Command::new("kill")
            .args(["-9", &pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

fn child_required_env(name: &str) -> Result<PathBuf> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .with_context(|| format!("supervision child missing {name}"))
}

fn child_emit(
    event_file: &Path,
    sequence: &mut u64,
    event: &str,
    details: &[(&str, &str)],
) -> Result<()> {
    *sequence += 1;
    let mut detail_map = BTreeMap::new();
    for (key, value) in details {
        detail_map.insert((*key).to_string(), (*value).to_string());
    }
    let payload = serde_json::json!({
        "schema_version": DRIVER_SCHEMA_VERSION,
        "sequence": sequence,
        "event": event,
        "details": detail_map,
    });
    use std::io::Write as _;
    let mut file = fs::OpenOptions::new().create(true).append(true).open(event_file)?;
    writeln!(file, "{payload}")?;
    Ok(())
}

fn child_emit_lifecycle(
    event_file: &Path,
    sequence: &mut u64,
    stop_after_barrier: Option<&str>,
) -> Result<()> {
    let ladder: [(&str, Vec<(&str, &str)>); 11] = [
        ("host_started", vec![("subject", "emacs"), ("client_kind", "bundled_eglot")]),
        ("client_loaded", vec![]),
        ("registration_selected", vec![]),
        ("initialize_observed", vec![]),
        ("workspace_ready", vec![]),
        ("buffer_opened", vec![]),
        ("host_action_started", vec![("action_id", "rename_module")]),
        ("host_action_completed", vec![("action_id", "rename_module")]),
        ("edit_applied", vec![]),
        ("shutdown_started", vec![]),
        ("shutdown_completed", vec![]),
    ];
    for (name, details) in ladder {
        child_emit(event_file, sequence, name, &details)?;
        if stop_after_barrier == Some(name) {
            return Ok(());
        }
    }
    Ok(())
}

/// Copy this test binary onto the unique candidate path and spawn a
/// descendant that stays alive under `descendant_sleep`. Callers must
/// reap it (`stop_test_descendant`). Used by the leak-while-host-runs
/// scenario and by the pre-existing-before-launch discriminator so both
/// drive the same spawn path.
pub fn spawn_preexisting_candidate(
    candidate: &Path,
    ready_marker: &Path,
    entry_test: &str,
) -> Result<u32> {
    let self_exe = std::env::current_exe().context("locating supervision fixture exe")?;
    fs::copy(&self_exe, candidate)
        .with_context(|| format!("staging descendant image at {}", candidate.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(candidate)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(candidate, permissions)?;
    }
    if let Some(parent) = ready_marker.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let mut descendant = Command::new(candidate);
    descendant
        .args(["--exact", entry_test, "--nocapture", "--test-threads=1"])
        .env(FAKE_HOST_MODE_ENV, "descendant_sleep")
        .env(FAKE_HOST_DESCENDANT_READY_ENV, ready_marker)
        .env(FAKE_HOST_ENTRY_ENV, entry_test)
        // Isolated `cargo test --exact …` puts the parent filter in
        // RUST_TEST_ARGS; inheriting it would re-run the parent test instead
        // of the fake-host entry.
        .env_remove("RUST_TEST_ARGS");
    let stderr_log = ready_marker.with_extension("stderr");
    descendant.stderr(
        fs::File::create(&stderr_log)
            .with_context(|| format!("creating descendant stderr log {}", stderr_log.display()))?,
    );
    let descendant_pid = spawn_surviving_descendant(descendant)?;
    let mut became_ready = ready_marker.is_file();
    for _ in 0..400 {
        if became_ready {
            break;
        }
        thread::sleep(Duration::from_millis(50));
        became_ready = ready_marker.is_file();
    }
    if !became_ready {
        let stderr_head = fs::read_to_string(&stderr_log).unwrap_or_default();
        bail!(
            "leak-scenario descendant {descendant_pid} never signaled readiness; stderr head: {}",
            stderr_head.chars().take(400).collect::<String>()
        );
    }
    Ok(descendant_pid)
}

fn spawn_surviving_descendant(mut command: Command) -> Result<u32> {
    command.stdin(Stdio::null()).stdout(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB
        // | CREATE_NO_WINDOW. The descendant must outlive the status-0 host.
        const DETACHED: u32 = 0x00000008 | 0x00000200 | 0x01000000 | 0x08000000;
        command.creation_flags(DETACHED);
        if let Ok(child) = command.spawn() {
            let pid = child.id();
            std::mem::forget(child);
            return Ok(pid);
        }
        const WEAKER: u32 = 0x00000008 | 0x00000200 | 0x08000000;
        command.creation_flags(WEAKER);
    }
    let child = command.spawn().context("spawning leak-scenario descendant")?;
    let pid = child.id();
    std::mem::forget(child);
    Ok(pid)
}

fn child_write_stdout(line: &str) -> Result<()> {
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{line}").context("writing fake-host stdout")?;
    stdout.flush().context("flushing fake-host stdout")?;
    Ok(())
}

fn child_write_stderr(line: &str) -> Result<()> {
    let mut stderr = io::stderr().lock();
    writeln!(stderr, "{line}").context("writing fake-host stderr")?;
    stderr.flush().context("flushing fake-host stderr")?;
    Ok(())
}

/// Fixture entry: never returns. The supervised process boundary observes
/// the fixture's real exit status.
pub fn run_fake_host_entry(mode: &str) -> ! {
    match run_fake_host_mode(mode) {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            let _ = child_write_stderr(&format!("supervision fixture failed: {error:#}"));
            std::process::exit(9);
        }
    }
}

fn run_fake_host_mode(mode: &str) -> Result<i32> {
    if mode == "descendant_sleep" {
        let ready = child_required_env(FAKE_HOST_DESCENDANT_READY_ENV)?;
        fs::write(&ready, format!("ready pid={}", std::process::id()).as_bytes())
            .with_context(|| format!("writing {}", ready.display()))?;
        for _ in 0..(DESCENDANT_LIFETIME_CAP_MS / 50) {
            thread::sleep(Duration::from_millis(50));
        }
        return Ok(0);
    }
    let event_file = child_required_env("PERL_LSP_EMACS_EVENT_FILE")?;
    let mut sequence = 0_u64;
    match mode {
        "clean" => {
            for (name_env, content) in [
                ("PERL_LSP_EMACS_CLIENT_LOG", "client log supervision capture distinct"),
                ("PERL_LSP_EMACS_SERVER_STDERR", "server stderr supervision capture distinct"),
                ("PERL_LSP_EMACS_CAPABILITY_SNAPSHOT", "{\"capabilities\":{}}"),
            ] {
                fs::write(child_required_env(name_env)?, content)
                    .with_context(|| format!("writing distinct capture for {name_env}"))?;
            }
            child_write_stdout("clean supervision stdout")?;
            child_emit_lifecycle(&event_file, &mut sequence, None)?;
            Ok(0)
        }
        "chatty_paths" => {
            let home = std::env::var_os("HOME")
                .map(|value| value.to_string_lossy().into_owned())
                .unwrap_or_default();
            child_write_stdout(&home)?;
            child_write_stdout("/home/observer/.netrc")?;
            child_write_stdout("C:\\Users\\observer\\secret-token.txt")?;
            child_write_stdout("\\Users\\observer\\secret-token.txt")?;
            child_emit_lifecycle(&event_file, &mut sequence, None)?;
            Ok(0)
        }
        "oversize_output" => {
            use std::io::Write as _;
            let stdout = std::io::stdout();
            let mut lock = stdout.lock();
            let filler = [b'x'; 4096];
            let total = MAX_CAPTURE_BYTES + 4096;
            let mut written = 0_usize;
            while written < total {
                let chunk = usize::min(filler.len(), total - written);
                lock.write_all(&filler[..chunk]).map_err(anyhow::Error::from)?;
                written += chunk;
            }
            writeln!(lock, "/home/observer/.netrc leaked past bound")?;
            lock.flush()?;
            child_emit_lifecycle(&event_file, &mut sequence, None)?;
            Ok(0)
        }
        "garbage_events" => {
            use std::io::Write as _;
            let mut file = fs::OpenOptions::new().create(true).append(true).open(&event_file)?;
            write!(file, "{{\"schema_version\":\"{DRIVER_SCHEMA_VERSION}\"")?;
            drop(file);
            child_emit(&event_file, &mut sequence, "host_started", &[("subject", "emacs")])?;
            Ok(0)
        }
        "hang_after_workspace_ready" => {
            child_emit_lifecycle(&event_file, &mut sequence, Some("workspace_ready"))?;
            for _ in 0..6000 {
                thread::sleep(Duration::from_millis(100));
            }
            Ok(7)
        }
        "driver_failed_exit3" => {
            child_emit_lifecycle(&event_file, &mut sequence, Some("buffer_opened"))?;
            child_emit(
                &event_file,
                &mut sequence,
                "driver_failed",
                &[("reason", "candidate_refused")],
            )?;
            Ok(3)
        }
        "leak_descendant_clean_exit" => {
            let candidate = child_required_env("PERL_LSP_EMACS_CANDIDATE")?;
            let ready_marker = event_file
                .parent()
                .context("event file must have a parent directory")?
                .join(format!("descendant-ready-{}", std::process::id()));
            let entry_test = std::env::var(FAKE_HOST_ENTRY_ENV)
                .context("supervision child missing entry test name")?;
            let _descendant_pid =
                spawn_preexisting_candidate(&candidate, &ready_marker, &entry_test)?;
            child_emit_lifecycle(&event_file, &mut sequence, None)?;
            Ok(0)
        }
        "leak_descendant_then_hang" => {
            let candidate = child_required_env("PERL_LSP_EMACS_CANDIDATE")?;
            let ready_marker = event_file
                .parent()
                .context("event file must have a parent directory")?
                .join(format!("descendant-ready-{}", std::process::id()));
            let entry_test = std::env::var(FAKE_HOST_ENTRY_ENV)
                .context("supervision child missing entry test name")?;
            let _descendant_pid =
                spawn_preexisting_candidate(&candidate, &ready_marker, &entry_test)?;
            child_emit_lifecycle(&event_file, &mut sequence, Some("workspace_ready"))?;
            for _ in 0..6000 {
                thread::sleep(Duration::from_millis(100));
            }
            Ok(7)
        }
        "partial_event_then_hang" => {
            child_emit_lifecycle(&event_file, &mut sequence, Some("workspace_ready"))?;
            use std::io::Write as _;
            let mut file = fs::OpenOptions::new().create(true).append(true).open(&event_file)?;
            write!(file, "{{\"schema_version\":\"{DRIVER_SCHEMA_VERSION}\",\"sequence\":")?;
            drop(file);
            for _ in 0..6000 {
                thread::sleep(Duration::from_millis(100));
            }
            Ok(7)
        }
        other => bail!("unknown supervision fixture mode: {other}"),
    }
}
