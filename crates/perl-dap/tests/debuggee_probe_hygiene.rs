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

#![expect(
    clippy::print_stderr,
    reason = "Integration-test diagnostic output; tracing is not wired into test helpers."
)]
#![allow(unsafe_code)] // required for std::env::set_var/remove_var in Rust 2024 (unsafe fn)

mod common;

use common::{DEBUGGEE_PERL_OVERRIDE_ENV, resolve_debuggee_perl};
use std::fs;

const PROBE_PREFIX: &str = "perl-lsp-dap-debuggee-probe-";

/// Temp entries whose name starts with our prefix AND carries this process's
/// pid token — i.e., workspaces materialized by THIS binary. Matches both
/// the legacy layout (`…-probe-<pid>`, no separator) and the repaired
/// randomized layout (`…-probe-<pid>-<random>`).
fn current_process_probe_artifacts() -> Vec<std::path::PathBuf> {
    let pid_token = std::process::id().to_string();
    fs::read_dir(std::env::temp_dir())
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| {
                    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                        return false;
                    };
                    let Some(tail) = name.strip_prefix(PROBE_PREFIX) else {
                        return false;
                    };
                    let Some(after_pid) = tail.strip_prefix(pid_token.as_str()) else {
                        return false;
                    };
                    // pid 123 must not claim sibling-process workspace
                    // `…-probe-1234-…`; require a delimiter (or end) right
                    // after our pid digits.
                    after_pid.is_empty() || after_pid.starts_with('-') || after_pid.starts_with('.')
                })
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn no_probe_workspace_survives_resolution_sweeps() {
    let before = current_process_probe_artifacts();

    // One guaranteed failed sweep: a nonexistent pin fails at spawn after its
    // workspace has been created, exercising cleanup on the error path that
    // used to leak.
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
        // collapse to the bogus pin alone, its probe materializes a
        // workspace, the spawn fails deterministically, and resolution must
        // report none — on every host, pre- or post-repair.
        assert!(
            resolve_debuggee_perl().is_none(),
            "a nonexistent pinned interpreter must fail resolution outright"
        );
    }

    let after = current_process_probe_artifacts();
    assert!(
        after.is_empty(),
        "probe workspaces must be cleaned up deterministically; leaked \
         artifacts from this run: {after:?}"
    );

    // The scan itself only means something if the baseline wasn't already
    // contaminated by a prior same-pid run (practically impossible across
    // processes since pids are not reused within a boot cycle, but assert the
    // invariant we actually care about: OUR sweep added none).
    assert!(
        !after.iter().any(|leaked| !before.contains(leaked)),
        "this resolution sweep added surviving probe artifacts: {after:?}"
    );
}
