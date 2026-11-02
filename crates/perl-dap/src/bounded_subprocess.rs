//! Wall-clock bounded `std::process::Command` execution for shipping paths.
//!
//! DAP launch-time probes must not inherit the adapter's stdin or block
//! indefinitely when a misbehaving interpreter hangs (#2316).

use std::io;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// Run `command` with a wall-clock `timeout`, killing the child on expiry.
///
/// Returns [`io::ErrorKind::TimedOut`] when the deadline is exceeded. The child's
/// stdin is the null device so an inheriting child cannot block on the DAP
/// transport (#17305).
pub fn output_with_timeout(mut command: Command, timeout: Duration) -> io::Result<Output> {
    let mut child =
        command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    let start = Instant::now();
    let poll_interval = Duration::from_millis(20);

    loop {
        match child.try_wait()? {
            Some(_) => return child.wait_with_output(),
            None => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        format!("subprocess exceeded {timeout:?}"),
                    ));
                }
                thread::sleep(poll_interval);
            }
        }
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
}
