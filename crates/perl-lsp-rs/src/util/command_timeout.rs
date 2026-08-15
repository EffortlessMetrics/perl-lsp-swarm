//! Timeout-safe command execution wrapper.
//!
//! Provides a helper to run `std::process::Command` with a wall-clock
//! timeout enforced by polling child process state. When the timeout
//! expires, the child process is terminated.

use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// Execute `cmd` with a wall-clock `timeout_secs` limit.
///
/// Returns `Ok(Output)` if the command finishes within the timeout, or
/// `Err(String)` with a human-readable message if it times out or fails
/// to spawn. A `timeout_secs` value of `0` disables timeout enforcement.
pub fn run_command_with_timeout(mut cmd: Command, timeout_secs: u64) -> Result<Output, String> {
    let timeout = (timeout_secs > 0).then(|| Duration::from_secs(timeout_secs));
    let start = Instant::now();
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|error| format!("command failed to start: {error}"))?;

    loop {
        // Check completion before the deadline so a process that finishes
        // exactly at the deadline boundary is never reported as timed out.
        if let Some(_status) =
            child.try_wait().map_err(|error| format!("failed waiting for command: {error}"))?
        {
            return child
                .wait_with_output()
                .map_err(|error| format!("failed collecting command output: {error}"));
        }

        if timeout.is_some_and(|timeout| start.elapsed() >= timeout) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("command timed out after {} seconds", timeout_secs));
        }

        thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn slow_command() -> Command {
        #[cfg(windows)]
        {
            let mut cmd = Command::new("powershell");
            cmd.args(["-NoProfile", "-Command", "Start-Sleep -Seconds 10"]);
            cmd
        }

        #[cfg(not(windows))]
        {
            let mut cmd = Command::new("sleep");
            cmd.arg("10");
            cmd
        }
    }

    fn fast_command() -> Command {
        #[cfg(windows)]
        {
            let mut cmd = Command::new("powershell");
            cmd.args(["-NoProfile", "-Command", "[Console]::Out.Write('hello')"]);
            cmd
        }

        #[cfg(not(windows))]
        {
            let mut cmd = Command::new("sh");
            cmd.args(["-c", "printf hello"]);
            cmd
        }
    }

    fn guaranteed_nonzero_exit_command() -> Command {
        #[cfg(windows)]
        {
            let mut cmd = Command::new("cmd");
            cmd.args(["/C", "exit", "7"]);
            cmd
        }

        #[cfg(not(windows))]
        {
            let mut cmd = Command::new("sh");
            cmd.args(["-c", "exit 7"]);
            cmd
        }
    }

    fn nonexistent_command() -> Command {
        Command::new("__perl_lsp_nonexistent_command__")
    }

    #[test]
    fn unit_timeout_fires_for_slow_command() {
        let start = Instant::now();
        let result = run_command_with_timeout(slow_command(), 1);
        let elapsed = start.elapsed();

        assert!(result.is_err(), "expected timeout error");
        // Should take approximately 1s, allow up to 4s for slow CI
        assert!(elapsed.as_secs() < 4, "timeout took too long: {}ms", elapsed.as_millis());
    }

    #[test]
    fn unit_fast_command_succeeds() {
        let result = run_command_with_timeout(fast_command(), 10);

        assert!(result.is_ok(), "expected success, got: {:?}", result.err());
        if let Ok(output) = result {
            assert!(output.status.success());
            assert_eq!(output.stdout, b"hello");
            assert!(output.stderr.is_empty());
        }
    }

    fn stderr_command() -> Command {
        #[cfg(windows)]
        {
            let mut cmd = Command::new("powershell");
            cmd.args(["-NoProfile", "-Command", "[Console]::Error.Write('diagnostic')"]);
            cmd
        }

        #[cfg(not(windows))]
        {
            let mut cmd = Command::new("sh");
            cmd.args(["-c", "printf diagnostic >&2"]);
            cmd
        }
    }

    #[test]
    fn unit_command_captures_stderr_without_losing_successful_completion() {
        let result = run_command_with_timeout(stderr_command(), 10);

        assert!(result.is_ok(), "expected command output, got: {:?}", result.err());
        if let Ok(output) = result {
            assert!(output.status.success());
            assert!(output.stdout.is_empty());
            assert_eq!(output.stderr, b"diagnostic");
        }
    }

    #[test]
    fn unit_zero_timeout_disables_deadline() {
        let result = run_command_with_timeout(fast_command(), 0);

        assert!(result.is_ok(), "expected zero-timeout command to run to completion");
    }

    #[test]
    fn unit_nonzero_exit_is_returned_as_output() {
        let result = run_command_with_timeout(guaranteed_nonzero_exit_command(), 10);

        assert!(result.is_ok(), "process should run and exit");
        if let Ok(output) = result {
            assert_eq!(output.status.code(), Some(7));
            assert!(!output.status.success());
            assert!(output.stdout.is_empty());
            assert!(output.stderr.is_empty());
        }
    }

    #[test]
    fn unit_spawn_failure_surfaces_start_error() {
        let result = run_command_with_timeout(nonexistent_command(), 10);

        assert!(result.is_err(), "expected spawn error for nonexistent command");
        if let Err(message) = result {
            assert!(message.contains("command failed to start"));
        }
    }
}
