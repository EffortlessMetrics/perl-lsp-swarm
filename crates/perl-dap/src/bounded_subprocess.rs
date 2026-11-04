//! Wall-clock bounded `std::process::Command` execution for shipping paths.
//!
//! DAP launch-time probes must not inherit the adapter's stdin, block on full
//! pipes, or leave owned descendants running after a deadline (#2316, #15538).

use std::io::{self, Read};
use std::process::{Command, ExitStatus, Output, Stdio};
use std::sync::mpsc::Receiver;
use std::thread;
use std::time::{Duration, Instant};

const POLL_INTERVAL: Duration = Duration::from_millis(20);

fn spawn_pipe_drain<R: Read + Send + 'static>(pipe: Option<R>) -> Option<Receiver<Vec<u8>>> {
    pipe.map(|mut pipe| {
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = pipe.read_to_end(&mut bytes);
            let _ = sender.send(bytes);
        });
        receiver
    })
}

fn join_pipe_drain_within(drain: Option<Receiver<Vec<u8>>>, budget: Duration) -> Vec<u8> {
    drain.and_then(|receiver| receiver.recv_timeout(budget).ok()).unwrap_or_default()
}

/// Run `command` with a wall-clock `timeout`, killing the owned process tree on
/// expiry. Returns [`io::ErrorKind::TimedOut`] when the deadline is exceeded.
///
/// Stdout and stderr are drained on helper threads while polling so a verbose
/// child cannot fill a pipe and deadlock before the deadline. The child's stdin
/// is the null device (#17305).
pub fn output_with_timeout(mut command: Command, timeout: Duration) -> io::Result<Output> {
    crate::process_tree::prepare_owned_command(&mut command);
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn()?;

    let stdout_drain = spawn_pipe_drain(child.stdout.take());
    let stderr_drain = spawn_pipe_drain(child.stderr.take());
    let deadline = Instant::now() + timeout;

    let status: ExitStatus = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = crate::process_tree::terminate_tree_and_reap(&mut child);
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        format!("subprocess exceeded {timeout:?}"),
                    ));
                }
                thread::sleep(POLL_INTERVAL);
            }
            Err(error) => {
                let _ = crate::process_tree::terminate_tree_and_reap(&mut child);
                return Err(error);
            }
        }
    };

    let remaining = deadline.saturating_duration_since(Instant::now());
    let stdout = join_pipe_drain_within(stdout_drain, remaining);
    let stderr = join_pipe_drain_within(stderr_drain, remaining);

    Ok(Output { status, stdout, stderr })
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

    fn verbose_stdout_command() -> Command {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "dd if=/dev/zero bs=4096 count=32 2>/dev/null"]);
        cmd
    }

    #[test]
    fn output_with_timeout_kills_hanging_process() {
        let start = Instant::now();
        let result = output_with_timeout(slow_command(), Duration::from_millis(250));

        assert_eq!(
            result.as_ref().err().map(|err| err.kind()),
            Some(io::ErrorKind::TimedOut),
            "expected timed-out subprocess, got {result:?}",
        );
        assert!(
            start.elapsed() < Duration::from_secs(3),
            "deadline should fire quickly, took {:?}",
            start.elapsed(),
        );
    }

    #[test]
    #[cfg(unix)]
    fn output_with_timeout_drains_verbose_stdout_before_deadline() {
        let result = output_with_timeout(verbose_stdout_command(), Duration::from_secs(5));

        assert!(
            result.as_ref().is_ok_and(|output| output.status.success()),
            "verbose stdout must not deadlock into a timeout: {result:?}",
        );
        assert!(
            result.as_ref().is_ok_and(|output| output.stdout.len() >= 128 * 1024),
            "expected captured stdout bytes",
        );
    }
}
