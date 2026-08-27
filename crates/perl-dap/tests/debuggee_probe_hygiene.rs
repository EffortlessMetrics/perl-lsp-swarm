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

#![allow(unsafe_code)] // required for std::env::set_var/remove_var in Rust 2024 (unsafe fn)

mod common;

#[cfg(unix)]
use common::{reset_sigkill_escalation_observation, sigkill_escalation_was_observed};

use common::{
    DEBUGGEE_PERL_OVERRIDE_ENV, probe_debuggee_perl_for_test_with_descendant_pid,
    probe_debuggee_perl_for_test_with_workspace_cleanup_failure, resolve_debuggee_perl,
};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const PROBE_PREFIX: &str = "perl-lsp-dap-debuggee-probe-";

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
    let controls = tempfile::tempdir()?;
    let success = compile_probe_control(
        controls.path(),
        "probe_success",
        "fn main() { println!(\"15\"); }\n",
    )?;
    let no_banner = compile_probe_control(controls.path(), "probe_no_banner", "fn main() {}\n")?;
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
        #[cfg(unix)]
        let descendant = Command::new("sh")
            .args(["-c", "trap '' TERM; while :; do sleep 1; done"])
            .spawn();
        #[cfg(windows)]
        let descendant = {
            let Ok(executable) = env::current_exe() else { return };
            Command::new(executable)
                .env_remove("PERL_LSP_DAP_TEST_DESCENDANT_PID_FILE")
                .spawn()
        };
        let Ok(descendant) = descendant else { return };
        let _ = fs::write(pid_file, descendant.id().to_string());
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
        #[cfg(unix)]
        let descendant = Command::new("sh")
            .args([
                "-c",
                "printf ready > \"$PERL_LSP_DAP_TEST_DESCENDANT_READY_FILE\"; trap '' TERM; while :; do sleep 1; done",
            ])
            .spawn();
        #[cfg(windows)]
        let descendant = {
            let Ok(executable) = env::current_exe() else { return };
            Command::new(executable)
                .env_remove("PERL_LSP_DAP_TEST_DESCENDANT_PID_FILE")
                .spawn()
        };
        let Ok(descendant) = descendant else { return };
        let _ = fs::write(pid_file, descendant.id().to_string());
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
    assert!(
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
        assert!(
            before.iter().any(|path| path == seeded_stale.path()),
            "{label} case lost the seeded stale baseline before probing"
        );

        let result = common::probe_debuggee_perl_for_test(binary, budget, simulate_wait_error);
        assert_eq!(result.is_ok(), should_succeed, "unexpected {label} probe result: {result:?}");
        let after = current_process_probe_artifacts()?;
        let new_artifacts: Vec<_> = after.iter().filter(|path| !before.contains(path)).collect();
        assert!(
            new_artifacts.is_empty(),
            "{label} probe left newly created workspaces: {new_artifacts:?}"
        );
        assert!(
            after.iter().any(|path| path == seeded_stale.path()),
            "{label} probe must not delete the pre-existing stale control"
        );
        assert_eq!(
            common::active_probe_reader_count(),
            0,
            "{label} probe left an active reader thread"
        );
    }

    for (label, simulate_wait_error, budget) in [
        ("timeout-descendant", false, Duration::from_secs(2)),
        ("wait-error-descendant", true, Duration::from_secs(2)),
    ] {
        let before = current_process_probe_artifacts()?;
        let pid_file = controls.path().join(format!("{label}.pid"));
        let binary = hanging.clone();
        let pid_file_for_probe = pid_file.clone();
        let probe = std::thread::spawn(move || {
            probe_debuggee_perl_for_test_with_descendant_pid(
                &binary,
                budget,
                simulate_wait_error,
                &pid_file_for_probe,
            )
        });
        let descendant_pid = wait_for_pid_file(&pid_file, Duration::from_secs(5))?;
        wait_for_marker_file(&pid_file.with_extension("pid.ready"), Duration::from_secs(5))?;
        wait_for_process_start(descendant_pid, Duration::from_secs(5))?;
        let result =
            probe.join().map_err(|_| io::Error::other(format!("{label} probe thread panicked")))?;
        assert!(result.is_err(), "{label} probe must fail through its cleanup path");
        wait_for_process_exit(descendant_pid, Duration::from_secs(5))?;

        let after = current_process_probe_artifacts()?;
        let new_artifacts: Vec<_> = after.iter().filter(|path| !before.contains(path)).collect();
        assert!(
            new_artifacts.is_empty(),
            "{label} probe left newly created workspaces: {new_artifacts:?}"
        );
        assert!(
            after.iter().any(|path| path == seeded_stale.path()),
            "{label} probe must not delete the pre-existing stale control"
        );
        assert_eq!(
            common::active_probe_reader_count(),
            0,
            "{label} probe left an active reader thread"
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
            move || {
                common::probe_debuggee_perl_for_test_with_descendant_pid(
                    &binary,
                    Duration::from_secs(2),
                    false,
                    &pid_file,
                )
            }
        });
        let descendant_pid = wait_for_pid_file(&pid_file, Duration::from_secs(5))?;
        wait_for_marker_file(&pid_file.with_extension("pid.ready"), Duration::from_secs(5))?;
        let result = probe
            .join()
            .map_err(|_| io::Error::other("successful-parent probe thread panicked"))?;
        assert!(result.is_ok(), "successful-parent probe must report success: {result:?}");
        wait_for_process_exit(descendant_pid, Duration::from_secs(5))?;

        let after = current_process_probe_artifacts()?;
        let new_artifacts: Vec<_> = after.iter().filter(|path| !before.contains(path)).collect();
        assert!(
            new_artifacts.is_empty(),
            "successful-parent probe left newly created workspaces: {new_artifacts:?}"
        );
        assert!(
            after.iter().any(|path| path == seeded_stale.path()),
            "successful-parent probe must not delete the pre-existing stale control"
        );
        assert_eq!(
            common::active_probe_reader_count(),
            0,
            "successful-parent probe left an active reader thread"
        );
        #[cfg(unix)]
        assert!(
            sigkill_escalation_was_observed(),
            "SIGTERM-resistant successful-parent descendant did not require SIGKILL escalation"
        );
    }

    let termination_failure = common::probe_debuggee_perl_for_test_with_termination_failure(
        &timeout,
        Duration::from_millis(100),
    );
    let termination_error = match termination_failure {
        Ok(_) => return Err(io::Error::other("termination-command failure was accepted")),
        Err(error) => error,
    };
    assert!(
        termination_error.contains("termination command failed"),
        "termination command failure must be explicit: {termination_error}"
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
    assert!(
        workspace_cleanup_error.contains("probe workspace cleanup failed"),
        "workspace cleanup failure must be explicit: {workspace_cleanup_error}"
    );
    let after = current_process_probe_artifacts()?;
    let new_artifacts: Vec<_> = after.iter().filter(|path| !before.contains(path)).collect();
    assert!(
        new_artifacts.is_empty(),
        "workspace cleanup failure control left newly created workspaces: {new_artifacts:?}"
    );

    #[cfg(windows)]
    {
        let before = current_process_probe_artifacts()?;
        let pid_file = controls.path().join("assignment-failure.pid");
        let assignment_pid_file = pid_file.clone();
        let assignment_binary = hanging.clone();
        let probe = std::thread::spawn(move || {
            common::probe_debuggee_perl_for_test_with_job_assignment_failure(
                &assignment_binary,
                Duration::from_secs(2),
                &assignment_pid_file,
            )
        });
        let descendant_pid = wait_for_pid_file(&pid_file, Duration::from_secs(5))?;
        wait_for_process_start(descendant_pid, Duration::from_secs(5))?;
        let assignment_failure =
            probe.join().map_err(|_| io::Error::other("job assignment probe thread panicked"))?;
        let assignment_error = match assignment_failure {
            Ok(_) => return Err(io::Error::other("job assignment failure was accepted")),
            Err(error) => error,
        };
        assert!(
            assignment_error.contains("job assignment"),
            "job assignment fallback must be explicit: {assignment_error}"
        );
        wait_for_process_exit(descendant_pid, Duration::from_secs(5))?;
        let after = current_process_probe_artifacts()?;
        let new_artifacts: Vec<_> = after.iter().filter(|path| !before.contains(path)).collect();
        assert!(
            new_artifacts.is_empty(),
            "job assignment fallback left artifacts: {new_artifacts:?}"
        );
    }

    {
        struct Guard(Option<std::ffi::OsString>);
        impl Drop for Guard {
            fn drop(&mut self) {
                match self.0.take() {
                    Some(value) => unsafe { std::env::set_var(DEBUGGEE_PERL_OVERRIDE_ENV, value) },
                    None => unsafe { std::env::remove_var(DEBUGGEE_PERL_OVERRIDE_ENV) },
                }
            }
        }
        let _guard = Guard(std::env::var_os(DEBUGGEE_PERL_OVERRIDE_ENV));
        unsafe { std::env::set_var(DEBUGGEE_PERL_OVERRIDE_ENV, "/definitely/not/a/real/perl") };

        // Drive RESOLUTION directly (not the availability gate): candidates
        // collapse to the bogus pin alone and resolution must report none.
        assert!(
            resolve_debuggee_perl().is_none(),
            "a nonexistent pinned interpreter must fail resolution outright"
        );
    }
    Ok(())
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

fn wait_for_process_exit(pid: u32, timeout: Duration) -> io::Result<()> {
    let deadline = std::time::Instant::now() + timeout;
    while process_exists(pid)? {
        if std::time::Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("descendant process {pid} survived probe cleanup"),
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
