//! POSIX gate shell launcher (#18011).
//!
//! Gate commands run under non-login Bash so an explicitly selected `PATH` from
//! the caller survives the wrapper, timeout, and fallback branches. Login
//! startup (`bash -lc`) may reset `PATH` on hosted agents; non-login
//! (`bash -c`) inherits the caller's environment.

use std::os::unix::process::CommandExt;
use std::process::Command;

pub(crate) fn shell_command_process(command: &str, timeout_secs: u64) -> Command {
    let mut cmd = Command::new("bash");
    let timeout_arg = format!("{timeout_secs}s");
    cmd.arg("-c")
        .arg(
            "if command -v timeout >/dev/null 2>&1; then \
             exec timeout --signal=TERM --kill-after=60s \"$1\" bash -c \"$2\"; \
             else exec bash -c \"$2\"; fi",
        )
        .arg("xtask-gate-timeout")
        .arg(timeout_arg)
        .arg(command);
    cmd.process_group(0);
    cmd
}
