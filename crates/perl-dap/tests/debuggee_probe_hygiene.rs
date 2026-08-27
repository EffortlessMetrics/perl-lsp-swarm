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

use common::{DEBUGGEE_PERL_OVERRIDE_ENV, last_probe_pid_for_test, resolve_debuggee_perl};
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
        "fn main() { std::thread::sleep(std::time::Duration::from_secs(60)); }\n",
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
        ("wait-error", hanging.as_path(), Duration::from_secs(2), true, false),
    ];
    for (label, binary, budget, simulate_wait_error, should_succeed) in cases {
        let before = current_process_probe_artifacts()?;
        assert!(
            before.iter().any(|path| path == seeded_stale.path()),
            "{label} case lost the seeded stale baseline before probing"
        );

        let result = common::probe_debuggee_perl_for_test(binary, budget, simulate_wait_error);
        assert_eq!(result.is_ok(), should_succeed, "unexpected {label} probe result: {result:?}");
        if label == "wait-error" {
            let pid = last_probe_pid_for_test()
                .ok_or_else(|| io::Error::other("wait-error probe did not record a child pid"))?;
            assert!(
                !process_exists(pid),
                "wait-error probe child {pid} was not terminated and reaped"
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

fn process_exists(pid: u32) -> bool {
    #[cfg(windows)]
    {
        Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output()
            .map(|output| String::from_utf8_lossy(&output.stdout).contains(&pid.to_string()))
            .unwrap_or(false)
    }
    #[cfg(unix)]
    {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = pid;
        false
    }
}
