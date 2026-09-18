//! Probe-workspace hygiene proof (#12594 repair r2, finding 2).
//!
//! [`probe_debuggee_perl`] (via the resolver) materializes a temporary
//! workspace — directory plus `pipe_probe.pl` script — under the system temp
//! directory for every candidate attempt. Pre-repair nothing removed it, so
//! every skipped/failing/probing DAP run leaked one directory per process
//! into `std::env::temp_dir()`.
//!
//! This proof drives resolution with a deliberately broken
//! [`DEBUGGEE_PERL_OVERRIDE_ENV`] pin (deterministic instant probe failure on
//! any host, regardless of which perls exist) and then asserts that no
//! probe workspace belonging to THIS test process survives. Directory names
//! embed the creating pid (`perl-lsp-dap-debuggee-probe-<pid>-…`), so the
//! scan cannot confuse artifacts from concurrently running suites.

mod common;

#[cfg(unix)]
use common::{reset_sigkill_escalation_observation, sigkill_escalation_was_observed};

use common::{
    DEBUGGEE_PERL_OVERRIDE_ENV, ProbeThreadSpawnFailure,
    probe_debuggee_perl_for_test_with_descendant_pid,
    probe_debuggee_perl_for_test_with_descendant_pid_publication_barrier,
    probe_debuggee_perl_for_test_with_thread_spawn_failure, resolve_debuggee_perl,
};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const PROBE_PREFIX: &str = "perl-lsp-dap-debuggee-probe-";
const INVALID_PIN_CHILD_MODE: &str = "PERL_LSP_DAP_INVALID_PIN_CHILD";

/// Temp entries whose name starts with our prefix AND carries this process's
/// pid token — i.e., workspaces materialized by THIS binary. Matches both
/// the legacy layout (`…-probe-<pid>`, no separator) and the repaired
/// randomized layout (`…-probe-<pid>-<random>`).
fn current_process_probe_artifacts() -> io::Result<Vec<PathBuf>> {
    let pid_token = std::process::id().to_string();
    let mut artifacts = Vec::new();
    for entry in fs::read_dir(std::env::temp_dir())? {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(tail) = name.strip_prefix(PROBE_PREFIX) else {
            continue;
        };
        let Some(after_pid) = tail.strip_prefix(pid_token.as_str()) else {
            continue;
        };
        // pid 123 must not claim sibling-process workspace
        // `…-probe-1234-…`; require a delimiter (or end) right
        // after our pid digits.
        if after_pid.is_empty() || after_pid.starts_with('-') || after_pid.starts_with('.') {
            artifacts.push(path);
        }
    }
    Ok(artifacts)
}

fn compile_probe_control(directory: &Path, label: &str, body: &str) -> io::Result<PathBuf> {
    let source = directory.join(format!("{label}.rs"));
    let binary =
        directory.join(if cfg!(windows) { format!("{label}.exe") } else { label.to_string() });
    fs::write(&source, body)?;
    let output = Command::new("rustc")
        .args(["--edition", "2024"])
        .arg(&source)
        .arg("-o")
        .arg(&binary)
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "failed to compile {label} probe control: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(binary)
}

#[test]
fn probe_workspace_cleanup_covers_each_child_exit_path() -> io::Result<()> {
    macro_rules! require {
        ($condition:expr, $($arg:tt)+) => {
            if !($condition) {
                return Err(io::Error::other(format!($($arg)+)));
            }
        };
    }

    // Keep the invalid-pin resolver control isolated from this test process.
    // `std::env::set_var`/`remove_var` are unsound in a multithreaded Unix
    // test binary; the child receives the pin at process creation instead.
    if std::env::var_os(INVALID_PIN_CHILD_MODE).is_some() {
        require!(
            resolve_debuggee_perl().is_none(),
            "a nonexistent pinned interpreter must fail resolution outright"
        );
        return Ok(());
    }

    let controls = tempfile::tempdir()?;
    let success = compile_probe_control(
        controls.path(),
        "probe_success",
        "fn main() { println!(\"15\"); }\n",
    )?;
    let no_banner = compile_probe_control(controls.path(), "probe_no_banner", "fn main() {}\n")?;
    let descendant = compile_probe_control(
        controls.path(),
        "probe_descendant",
        r#"
use std::{env, fs, thread, time::Duration};

#[cfg(unix)]
unsafe extern "C" {
    fn signal(signal: i32, handler: usize) -> usize;
}

#[cfg(unix)]
fn ignore_sigterm() {
    // SAFETY: installing SIG_IGN for this dedicated test fixture is process
    // local and intentionally makes escalation to SIGKILL observable.
    unsafe {
        let _ = signal(15, 1);
    }
}

#[cfg(not(unix))]
fn ignore_sigterm() {}

fn main() {
    ignore_sigterm();
    let Some(ready_file) = env::args_os().nth(1) else {
        return;
    };
    if fs::write(ready_file, "ready").is_err() {
        return;
    }
    thread::sleep(Duration::from_secs(60));
}
"#,
    )?;
    let timeout = compile_probe_control(
        controls.path(),
        "probe_timeout",
        "fn main() { std::thread::sleep(std::time::Duration::from_secs(60)); }\n",
    )?;
    let hanging = compile_probe_control(
        controls.path(),
        "probe_hanging",
        r#"
use std::{env, fs, process::Command, thread, time::Duration};

fn main() {
    if env::var_os("PERL_LSP_DAP_TEST_DESCENDANT_PID_FILE").is_none()
        && let Some(ready_file) = env::var_os("PERL_LSP_DAP_TEST_DESCENDANT_READY_FILE")
    {
        let _ = fs::write(ready_file, "ready");
    }
    if let Some(pid_file) = env::var_os("PERL_LSP_DAP_TEST_DESCENDANT_PID_FILE") {
        // Keep a separate receipt for the direct child so the PID-publication
        // failure control can prove that cleanup reaps both process levels.
        let child_pid_file = format!("{}.child", pid_file.to_string_lossy());
        let _ = fs::write(child_pid_file, std::process::id().to_string());
        let descendant_binary = env::var_os("PERL_LSP_DAP_TEST_DESCENDANT_BINARY");
        let Some(ready_file) = env::var_os("PERL_LSP_DAP_TEST_DESCENDANT_READY_FILE") else {
            thread::sleep(Duration::from_secs(60));
            return;
        };
        if let Some(descendant_binary) = descendant_binary {
            let descendant = Command::new(descendant_binary).arg(ready_file).spawn();
            let Ok(descendant) = descendant else {
                thread::sleep(Duration::from_secs(60));
                return;
            };
            let _ = fs::write(pid_file, descendant.id().to_string());
        }
    }
    thread::sleep(Duration::from_secs(60));
}
"#,
    )?;
    let success_with_descendant = compile_probe_control(
        controls.path(),
        "probe_success_with_descendant",
        r#"
use std::{env, fs, process::Command};

fn main() {
    if env::var_os("PERL_LSP_DAP_TEST_DESCENDANT_PID_FILE").is_none()
        && let Some(ready_file) = env::var_os("PERL_LSP_DAP_TEST_DESCENDANT_READY_FILE")
    {
        let _ = fs::write(ready_file, "ready");
    }
    if let Some(pid_file) = env::var_os("PERL_LSP_DAP_TEST_DESCENDANT_PID_FILE") {
        let descendant_binary = env::var_os("PERL_LSP_DAP_TEST_DESCENDANT_BINARY");
        let Some(descendant_binary) = descendant_binary else { return };
        let Some(ready_file) = env::var_os("PERL_LSP_DAP_TEST_DESCENDANT_READY_FILE") else {
            return;
        };
        let descendant = Command::new(descendant_binary).arg(ready_file).spawn();
        let Ok(descendant) = descendant else { return };
        let _ = fs::write(pid_file, descendant.id().to_string());
        let Some(ready_file) = env::var_os("PERL_LSP_DAP_TEST_DESCENDANT_READY_FILE") else {
            return;
        };
        for _ in 0..500 {
            if fs::metadata(&ready_file).is_ok() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        println!("15");
        return;
    }
    std::thread::sleep(std::time::Duration::from_secs(60));
}
"#,
    )?;

    let stale_prefix = format!("{PROBE_PREFIX}{}-seeded-stale-control-", std::process::id());
    let seeded_stale =
        tempfile::Builder::new().prefix(&stale_prefix).tempdir_in(std::env::temp_dir())?;
    let baseline = current_process_probe_artifacts()?;
    require!(
        baseline.contains(&seeded_stale.path().to_path_buf()),
        "seeded same-process stale artifact must be visible in the baseline"
    );

    let cases = [
        ("success", success.as_path(), Duration::from_secs(2), false, true),
        ("no-banner", no_banner.as_path(), Duration::from_secs(2), false, false),
        ("timeout", timeout.as_path(), Duration::from_millis(100), false, false),
    ];
    for (label, binary, budget, simulate_wait_error, should_succeed) in cases {
        let before = current_process_probe_artifacts()?;
        require!(
            before.iter().any(|path| path == seeded_stale.path()),
            "{label} case lost the seeded stale baseline before probing"
        );

        let result = common::probe_debuggee_perl_for_test(binary, budget, simulate_wait_error);
        require!(result.is_ok() == should_succeed, "unexpected {label} probe result: {result:?}");
        let after = current_process_probe_artifacts()?;
        let new_artifacts: Vec<_> = after.iter().filter(|path| !before.contains(path)).collect();
        require!(
            new_artifacts.is_empty(),
            "{label} probe left newly created workspaces: {new_artifacts:?}"
        );
        require!(
            after.iter().any(|path| path == seeded_stale.path()),
            "{label} probe must not delete the pre-existing stale control"
        );
        require!(
            common::active_probe_reader_count() == 0,
            "{label} probe left an active reader thread"
        );
    }

    for (label, simulate_wait_error, budget) in [
        ("timeout-descendant", false, Duration::from_secs(10)),
        ("wait-error-descendant", true, Duration::from_secs(10)),
    ] {
        let before = current_process_probe_artifacts()?;
        let pid_file = controls.path().join(format!("{label}.pid"));
        let binary = hanging.clone();
        let pid_file_for_probe = pid_file.clone();
        let descendant_for_probe = descendant.clone();
        let probe = std::thread::spawn(move || {
            probe_debuggee_perl_for_test_with_descendant_pid(
                &binary,
                budget,
                simulate_wait_error,
                &pid_file_for_probe,
                &descendant_for_probe,
            )
        });
        let descendant_pid = wait_for_pid_file(&pid_file, Duration::from_secs(5))?;
        wait_for_marker_file(&pid_file.with_extension("pid.ready"), Duration::from_secs(5))?;
        if !simulate_wait_error {
            wait_for_process_start(descendant_pid, Duration::from_secs(5))?;
        }
        let result =
            probe.join().map_err(|_| io::Error::other(format!("{label} probe thread panicked")))?;
        require!(result.is_err(), "{label} probe must fail through its cleanup path");
        wait_for_process_exit(label, descendant_pid, Duration::from_secs(5))?;

        let after = current_process_probe_artifacts()?;
        let new_artifacts: Vec<_> = after.iter().filter(|path| !before.contains(path)).collect();
        require!(
            new_artifacts.is_empty(),
            "{label} probe left newly created workspaces: {new_artifacts:?}"
        );
        require!(
            after.iter().any(|path| path == seeded_stale.path()),
            "{label} probe must not delete the pre-existing stale control"
        );
        require!(
            common::active_probe_reader_count() == 0,
            "{label} probe left an active reader thread"
        );
    }

    // The PID receipt itself is deliberately made unwritable. The probe child
    // has already spawned, so returning directly from fs::write would leak a
    // live parent (and potentially its descendant) unless the publication
    // failure uses the same process-tree cleanup boundary as later failures.
    {
        let before = current_process_probe_artifacts()?;
        let pid_file = controls.path().join("pid-receipt-write-failure.pid");
        let receipt_path = common::probe_pid_file_for_test(&pid_file);
        fs::create_dir(&receipt_path)?;
        let child_pid_path = PathBuf::from(format!("{}.child", pid_file.display()));
        let binary = hanging.clone();
        let descendant_binary = descendant.clone();
        let pid_file_for_probe = pid_file.clone();
        let probe = std::thread::spawn(move || {
            probe_debuggee_perl_for_test_with_descendant_pid_publication_barrier(
                &binary,
                Duration::from_secs(10),
                &pid_file_for_probe,
                &descendant_binary,
            )
        });
        let child_pid = wait_for_pid_file(&child_pid_path, Duration::from_secs(5))?;
        let descendant_pid = wait_for_pid_file(&pid_file, Duration::from_secs(5))?;
        wait_for_marker_file(&pid_file.with_extension("pid.ready"), Duration::from_secs(5))?;
        let result =
            probe.join().map_err(|_| io::Error::other("PID receipt failure probe panicked"))?;
        let error = result
            .err()
            .ok_or_else(|| io::Error::other("PID receipt publication failure must be reported"))?;
        require!(
            error.contains("cannot publish probe child PID"),
            "receipt failure must remain the primary error, got: {error}"
        );
        wait_for_process_exit("PID receipt failure child", child_pid, Duration::from_secs(5))?;
        wait_for_process_exit(
            "PID receipt failure descendant",
            descendant_pid,
            Duration::from_secs(5),
        )?;
        require!(
            common::active_probe_reader_count() == 0,
            "PID receipt failure probe left an active reader thread"
        );
        let after = current_process_probe_artifacts()?;
        let new_artifacts: Vec<_> = after.iter().filter(|path| !before.contains(path)).collect();
        require!(
            new_artifacts.is_empty(),
            "PID receipt failure probe left newly created workspaces: {new_artifacts:?}"
        );
    }

    {
        let before = current_process_probe_artifacts()?;
        #[cfg(unix)]
        reset_sigkill_escalation_observation();
        let pid_file = controls.path().join("success-descendant.pid");
        let probe = std::thread::spawn({
            let binary = success_with_descendant.clone();
            let pid_file = pid_file.clone();
            let descendant = descendant.clone();
            move || {
                common::probe_debuggee_perl_for_test_with_descendant_pid(
                    &binary,
                    Duration::from_secs(2),
                    false,
                    &pid_file,
                    &descendant,
                )
            }
        });
        let descendant_pid = wait_for_pid_file(&pid_file, Duration::from_secs(5))?;
        wait_for_marker_file(&pid_file.with_extension("pid.ready"), Duration::from_secs(5))?;
        let result = probe
            .join()
            .map_err(|_| io::Error::other("successful-parent probe thread panicked"))?;
        require!(result.is_ok(), "successful-parent probe must report success: {result:?}");
        wait_for_process_exit("success-descendant", descendant_pid, Duration::from_secs(5))?;

        let after = current_process_probe_artifacts()?;
        let new_artifacts: Vec<_> = after.iter().filter(|path| !before.contains(path)).collect();
        require!(
            new_artifacts.is_empty(),
            "successful-parent probe left newly created workspaces: {new_artifacts:?}"
        );
        require!(
            after.iter().any(|path| path == seeded_stale.path()),
            "successful-parent probe must not delete the pre-existing stale control"
        );
        require!(
            common::active_probe_reader_count() == 0,
            "successful-parent probe left an active reader thread"
        );
        #[cfg(unix)]
        require!(
            sigkill_escalation_was_observed(),
            "SIGTERM-resistant successful-parent descendant did not require SIGKILL escalation"
        );
    }

    let before = current_process_probe_artifacts()?;
    let termination_pid_file = controls.path().join("termination-failure.pid");
    let termination_pid_for_probe = termination_pid_file.clone();
    let termination_binary = hanging.clone();
    let termination_descendant_binary = descendant.clone();
    let termination_probe = std::thread::spawn(move || {
        common::probe_debuggee_perl_for_test_with_termination_failure(
            &termination_binary,
            Duration::from_millis(100),
            &termination_pid_for_probe,
            &termination_descendant_binary,
        )
    });
    let termination_descendant_pid =
        wait_for_pid_file(&termination_pid_file, Duration::from_secs(5))?;
    wait_for_marker_file(
        &termination_pid_file.with_extension("pid.ready"),
        Duration::from_secs(5),
    )?;
    // The injected termination failure can begin tearing down the descendant
    // immediately after it publishes its ready marker.  Requiring tasklist to
    // observe a live descendant here races that intentional cleanup; the PID
    // plus ready marker establish startup, while the later exit check proves
    // the descendant was reaped.
    let termination_probe_pid = wait_for_pid_file(
        &common::probe_pid_file_for_test(&termination_pid_file),
        Duration::from_secs(5),
    )?;
    require!(
        process_exists(termination_probe_pid)?,
        "termination-failure probe child must be live before the injected cleanup failure"
    );
    let termination_failure = termination_probe
        .join()
        .map_err(|_| io::Error::other("termination-failure probe thread panicked"))?;
    let termination_error = match termination_failure {
        Ok(_) => return Err(io::Error::other("termination-command failure was accepted")),
        Err(error) => error,
    };
    require!(
        termination_error.contains("owned process termination failure"),
        "owned termination failure must be explicit: {termination_error}"
    );
    require!(
        process_exists(termination_probe_pid)?,
        "the injected termination failure must return with the owned child retained"
    );
    common::force_cleanup_probe_process_for_test(termination_probe_pid)
        .map_err(io::Error::other)?;
    wait_for_process_exit(
        "termination-failure",
        termination_descendant_pid,
        Duration::from_secs(5),
    )?;
    let after = current_process_probe_artifacts()?;
    let new_artifacts: Vec<_> = after.iter().filter(|path| !before.contains(path)).collect();
    require!(
        new_artifacts.is_empty(),
        "termination-failure probe left newly created workspaces: {new_artifacts:?}"
    );
    require!(
        after.iter().any(|path| path == seeded_stale.path()),
        "termination-failure probe must not delete the pre-existing stale control"
    );
    require!(
        common::active_probe_reader_count() == 0,
        "termination-failure probe left an active reader thread"
    );

    let before = current_process_probe_artifacts()?;
    let workspace_cleanup_failure =
        common::probe_debuggee_perl_for_test_with_workspace_cleanup_failure(
            &success,
            Duration::from_secs(2),
        );
    let workspace_cleanup_error = match workspace_cleanup_failure {
        Ok(_) => return Err(io::Error::other("workspace cleanup failure was accepted")),
        Err(error) => error,
    };
    require!(
        workspace_cleanup_error.contains("probe workspace cleanup failed"),
        "workspace cleanup failure must be explicit: {workspace_cleanup_error}"
    );
    let after = current_process_probe_artifacts()?;
    let new_artifacts: Vec<_> = after.iter().filter(|path| !before.contains(path)).collect();
    require!(
        !new_artifacts.is_empty(),
        "real TempDir::close failure must leave a displaced workspace for explicit cleanup"
    );
    require!(
        after.iter().any(|path| path == seeded_stale.path()),
        "workspace cleanup failure must not delete the pre-existing stale control"
    );
    require!(
        common::active_probe_reader_count() == 0,
        "workspace cleanup failure left an active reader thread"
    );
    for artifact in &new_artifacts {
        fs::remove_dir_all(artifact)?;
    }
    let after_explicit_cleanup = current_process_probe_artifacts()?;
    require!(
        after_explicit_cleanup.iter().all(|path| before.contains(path)),
        "explicit cleanup must remove only the displaced workspace: {after_explicit_cleanup:?}"
    );

    #[cfg(windows)]
    {
        let before = current_process_probe_artifacts()?;
        let descendant_pid_file = controls.path().join("assignment-failure.pid");
        let assignment_binary = hanging.clone();
        let assignment_descendant_binary = descendant.clone();
        let assignment_pid_file = descendant_pid_file.clone();
        let probe = std::thread::spawn(move || {
            common::probe_debuggee_perl_for_test_with_job_assignment_failure(
                &assignment_binary,
                Duration::from_secs(2),
                &assignment_pid_file,
                &assignment_descendant_binary,
            )
        });
        let child_pid = wait_for_pid_file(
            &common::probe_pid_file_for_test(&descendant_pid_file),
            Duration::from_secs(5),
        )?;
        let assignment_failure =
            probe.join().map_err(|_| io::Error::other("job assignment probe thread panicked"))?;
        let assignment_error = match assignment_failure {
            Ok(_) => return Err(io::Error::other("job assignment failure was accepted")),
            Err(error) => error,
        };
        require!(
            assignment_error.contains("job assignment"),
            "job assignment fallback must be explicit: {assignment_error}"
        );
        wait_for_process_exit("job-assignment-child", child_pid, Duration::from_secs(5))?;
        let after = current_process_probe_artifacts()?;
        let new_artifacts: Vec<_> = after.iter().filter(|path| !before.contains(path)).collect();
        require!(
            new_artifacts.is_empty(),
            "job assignment fallback left artifacts: {new_artifacts:?}"
        );
        require!(
            after.iter().any(|path| path == seeded_stale.path()),
            "job assignment fallback must not delete the pre-existing stale control"
        );
        require!(
            common::active_probe_reader_count() == 0,
            "job assignment fallback left an active reader thread"
        );
    }

    for (label, stage) in [
        ("writer-spawn", ProbeThreadSpawnFailure::Writer),
        ("stdout-reader-spawn", ProbeThreadSpawnFailure::StdoutReader),
        ("stderr-reader-spawn", ProbeThreadSpawnFailure::StderrReader),
    ] {
        let before = current_process_probe_artifacts()?;
        let descendant_pid_file = controls.path().join(format!("{label}.pid"));
        let failure_binary = hanging.clone();
        let failure_pid_file = descendant_pid_file.clone();
        let descendant_for_probe = descendant.clone();
        let probe = std::thread::spawn(move || {
            probe_debuggee_perl_for_test_with_thread_spawn_failure(
                &failure_binary,
                Duration::from_secs(2),
                &failure_pid_file,
                &descendant_for_probe,
                stage,
            )
        });
        let descendant_pid = wait_for_pid_file(&descendant_pid_file, Duration::from_secs(5))?;
        wait_for_marker_file(
            &descendant_pid_file.with_extension("pid.ready"),
            Duration::from_secs(5),
        )?;
        // Injected reader/writer-spawn failures can begin cleanup immediately
        // after the descendant publishes its ready marker.  The PID file plus
        // marker prove that the descendant started; the exit check below proves
        // that failure cleanup reaped it without a tasklist race.
        let failure =
            probe.join().map_err(|_| io::Error::other(format!("{label} probe thread panicked")))?;
        let error = match failure {
            Ok(_) => return Err(io::Error::other(format!("{label} failure was accepted"))),
            Err(error) => error,
        };
        require!(error.contains("injected probe"), "{label} failure must be explicit: {error}");
        wait_for_process_exit(label, descendant_pid, Duration::from_secs(5))?;
        let after = current_process_probe_artifacts()?;
        let new_artifacts: Vec<_> = after.iter().filter(|path| !before.contains(path)).collect();
        require!(
            new_artifacts.is_empty(),
            "{label} failure left newly created workspaces: {new_artifacts:?}"
        );
        require!(
            after.iter().any(|path| path == seeded_stale.path()),
            "{label} failure must not delete the pre-existing stale control"
        );
        require!(
            common::active_probe_reader_count() == 0,
            "{label} failure left an active reader thread"
        );
    }

    {
        // Drive RESOLUTION directly (not the availability gate): candidates
        // collapse to the bogus pin alone and resolution must report none.
        // The parent environment remains untouched, including any caller pin.
        let parent_pin = std::env::var_os(DEBUGGEE_PERL_OVERRIDE_ENV);
        let child = Command::new(std::env::current_exe()?)
            .args(["--exact", "probe_workspace_cleanup_covers_each_child_exit_path", "--nocapture"])
            .env(INVALID_PIN_CHILD_MODE, "1")
            .env(DEBUGGEE_PERL_OVERRIDE_ENV, "/definitely/not/a/real/perl")
            .output()?;
        require!(
            child.status.success(),
            "invalid pinned interpreter child failed: status={:?}, stderr={}",
            child.status,
            String::from_utf8_lossy(&child.stderr)
        );
        require!(
            std::env::var_os(DEBUGGEE_PERL_OVERRIDE_ENV) == parent_pin,
            "parent resolver-pin environment changed while testing child override"
        );
    }
    Ok(())
}

#[test]
fn cleanup_command_wait_error_kills_and_reaps_helper() -> io::Result<()> {
    let command = if cfg!(windows) {
        let mut command = Command::new("ping");
        command.args(["127.0.0.1", "-n", "31"]);
        command
    } else {
        let mut command = Command::new("sleep");
        command.arg("30");
        command
    };
    let (pid, result) = common::run_cleanup_command_for_test(command, Duration::from_secs(5))
        .map_err(io::Error::other)?;
    let error = match result {
        Ok(status) => {
            return Err(io::Error::other(format!(
                "injected cleanup wait failure unexpectedly succeeded: {status}"
            )));
        }
        Err(error) => error,
    };
    if !error.contains("injected cleanup command wait failure") {
        return Err(io::Error::other(format!(
            "cleanup helper returned the wrong failure: {error}"
        )));
    }
    wait_for_process_exit("cleanup-command-wait-error", pid, Duration::from_secs(5))
}

fn process_exists(pid: u32) -> io::Result<bool> {
    #[cfg(windows)]
    {
        let output = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "tasklist failed while checking PID {pid}: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        Ok(String::from_utf8_lossy(&output.stdout).lines().any(|line| {
            line.split(',')
                .nth(1)
                .map(|field| field.trim_matches('"') == pid.to_string())
                .unwrap_or(false)
        }))
    }
    #[cfg(unix)]
    {
        Ok(Command::new("kill").args(["-0", &pid.to_string()]).status()?.success())
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = pid;
        Ok(false)
    }
}

fn wait_for_pid_file(path: &Path, timeout: Duration) -> io::Result<u32> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if let Ok(contents) = fs::read_to_string(path)
            && let Ok(pid) = contents.trim().parse::<u32>()
        {
            return Ok(pid);
        }
        if std::time::Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("descendant PID file was not written: {}", path.display()),
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn wait_for_marker_file(path: &Path, timeout: Duration) -> io::Result<()> {
    let deadline = std::time::Instant::now() + timeout;
    while !path.exists() {
        if std::time::Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("marker file was not written: {}", path.display()),
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

fn wait_for_process_exit(label: &str, pid: u32, timeout: Duration) -> io::Result<()> {
    let deadline = std::time::Instant::now() + timeout;
    while process_exists(pid)? {
        if std::time::Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("{label}: descendant process {pid} survived probe cleanup"),
            ));
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    Ok(())
}

fn wait_for_process_start(pid: u32, timeout: Duration) -> io::Result<()> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if process_exists(pid)? {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("descendant process {pid} was not observable after its PID was written"),
            ));
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}
