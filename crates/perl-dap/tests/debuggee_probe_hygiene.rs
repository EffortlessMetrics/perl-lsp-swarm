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

use common::{DEBUGGEE_PERL_OVERRIDE_ENV, resolve_debuggee_perl};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const PROBE_PREFIX: &str = "perl-lsp-dap-debuggee-probe-";
const PROBE_PID_FILE_ENV: &str = "PERL_LSP_DAP_PROBE_PID_FILE";

struct EnvGuard {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &Path) -> Self {
        let previous = std::env::var_os(key);
        unsafe { std::env::set_var(key, value) };
        Self { key, previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(value) => unsafe { std::env::set_var(self.key, value) },
            None => unsafe { std::env::remove_var(self.key) },
        }
    }
}

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

fn wait_for_pid_file(path: &Path) -> io::Result<u32> {
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        if let Ok(contents) = fs::read_to_string(path)
            && let Ok(pid) = contents.trim().parse::<u32>()
        {
            return Ok(pid);
        }
        if std::time::Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("probe control did not publish its pid at {}", path.display()),
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn process_is_alive(pid: u32) -> io::Result<bool> {
    let pid_text = pid.to_string();
    #[cfg(unix)]
    {
        return Ok(Command::new("kill").args(["-0", &pid_text]).status()?.success());
    }
    #[cfg(windows)]
    {
        let output = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid_text}"), "/NH"])
            .output()?;
        return Ok(output.status.success()
            && String::from_utf8_lossy(&output.stdout)
                .lines()
                .any(|line| line.split_whitespace().nth(1) == Some(pid_text.as_str())));
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        Err(io::Error::other("process liveness is unsupported on this platform"))
    }
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
        r#"fn main() {
    if let Ok(path) = std::env::var("PERL_LSP_DAP_PROBE_PID_FILE") {
        let _ = std::fs::write(path, std::process::id().to_string());
    }
    println!("15");
    std::thread::sleep(std::time::Duration::from_secs(60));
}
"#,
    )?;
    let pid_file = controls.path().join("probe-child.pid");
    let _pid_file_env = EnvGuard::set(PROBE_PID_FILE_ENV, &pid_file);

    // Seed a same-process artifact before taking the baseline. A proof that
    // merely asserts the entire prefix is empty would confuse this legitimate
    // stale entry with a leak; the production sweep must be judged by its
    // baseline delta instead.
    let stale_prefix = format!("{PROBE_PREFIX}{}-seeded-stale-control-", std::process::id());
    let seeded_stale =
        tempfile::Builder::new().prefix(&stale_prefix).tempdir_in(std::env::temp_dir())?;
    let baseline = current_process_probe_artifacts()?;
    assert!(
        baseline.contains(&seeded_stale.path().to_path_buf()),
        "seeded same-process stale artifact must be visible in the baseline"
    );

    // Run each production cleanup branch through the real probe implementation.
    // The last argument injects a deterministic `try_wait` error after spawn;
    // the short budget keeps the timeout case bounded without weakening the
    // production ten-second deadline.
    let cases = [
        ("success", success.as_path(), Duration::from_secs(2), false, true),
        ("no-banner", no_banner.as_path(), Duration::from_secs(2), false, false),
        ("timeout", timeout.as_path(), Duration::from_secs(2), false, false),
        ("wait-error", timeout.as_path(), Duration::from_secs(2), true, false),
    ];
    for (label, binary, budget, simulate_wait_error, should_succeed) in cases {
        let tracks_child = label == "timeout" || label == "wait-error";
        if tracks_child {
            let _ = fs::remove_file(&pid_file);
        }
        let before = current_process_probe_artifacts()?;
        assert!(
            before.iter().any(|path| path == seeded_stale.path()),
            "{label} case lost the seeded stale baseline before probing"
        );

        let result = common::probe_debuggee_perl_for_test(binary, budget, simulate_wait_error);
        assert_eq!(result.is_ok(), should_succeed, "unexpected {label} probe result: {result:?}");

        if tracks_child {
            let pid = wait_for_pid_file(&pid_file)?;
            assert!(
                !process_is_alive(pid)?,
                "{label} probe child {pid} remained alive after the production cleanup path"
            );
        }

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
    }

    // Keep the pin environment restoration proof from the original sweep: the
    // helper above exercises the direct probe, while this final resolution
    // path confirms the resolver still rejects an explicitly broken pin.
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
