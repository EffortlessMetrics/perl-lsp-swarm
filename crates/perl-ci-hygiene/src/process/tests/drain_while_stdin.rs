//! Bounded drain-while-stdin proof for `command_with_input_with_status`.
//!
//! The child oracle is the same `process_fixture_child` used by wrapper
//! contracts. A driver re-exec of this test binary invokes the helper (or the
//! sequential write-then-wait falsifier). The parent test is a watchdog that
//! kills and reaps the driver on deadline so a pipe deadlock cannot hang CI.

use super::super::{
    child_exit_code, combined_lossy_output, command_with_input_with_status, configure_child,
    join_stream, stream_join_bytes,
};
use super::wrapper_contracts::{
    FIXTURE_SCENARIO_ENV, PIPE_PRESSURE_BYTES, STDERR_BEFORE_STDIN_MARKER, STDIN_BYTES_MARKER,
    STDIN_DIGEST_MARKER, STDIN_EOF_MARKER, STDOUT_BEFORE_STDIN_MARKER, fixture_args,
    fixture_command, stdin_digest,
};
use super::{TempDir, TestResult};
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const DRIVER_SCENARIO_ENV: &str = "PERL_CI_HYGIENE_PROCESS_DRIVER";
const DRIVER_FILTER: &str = "process::tests::drain_while_stdin::process_input_drain_driver";
const DRIVER_RESULT_PREFIX: &str = "__PERL_CI_HYGIENE_DRIVER__:";
const WATCHDOG_DEADLINE: Duration = Duration::from_secs(10);

fn pipe_pressure_stdin() -> String {
    "I".repeat(PIPE_PRESSURE_BYTES)
}

fn fixture_env(scenario: &'static str) -> [(&'static str, &'static str); 1] {
    [(FIXTURE_SCENARIO_ENV, scenario)]
}

/// Historical write-then-wait ordering. Kept as a test-only falsifier so the
/// watchdog can still observe deadlock after the production helper drains.
fn sequential_write_then_wait(
    repo_root: &Path,
    command: &str,
    args: &[&str],
    env_vars: &[(&str, &str)],
    stdin_payload: &str,
) -> color_eyre::eyre::Result<(i32, String)> {
    let mut child = configure_child(command, repo_root, args, env_vars)?;
    child.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = child.spawn()?;
    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| color_eyre::eyre::eyre!("failed to open stdin for command {command}"))?;
        stdin.write_all(stdin_payload.as_bytes())?;
    }
    let output = child.wait_with_output()?;
    Ok((child_exit_code(output.status), combined_lossy_output(&output)))
}

fn report_helper_result(result: color_eyre::eyre::Result<(i32, String)>) -> String {
    match result {
        Ok((status, combined)) => {
            let stdout_before = u8::from(combined.contains(STDOUT_BEFORE_STDIN_MARKER));
            let stderr_before = u8::from(combined.contains(STDERR_BEFORE_STDIN_MARKER));
            let stdout_bulk = combined.bytes().filter(|byte| *byte == b'~').count();
            let stderr_bulk = combined.bytes().filter(|byte| *byte == b'^').count();
            let digest_line = combined.lines().find_map(|line| {
                line.strip_prefix(&format!("{STDIN_DIGEST_MARKER}:")).map(str::to_owned)
            });
            let bytes_line = combined.lines().find_map(|line| {
                line.strip_prefix(&format!("{STDIN_BYTES_MARKER}:")).map(str::to_owned)
            });
            let eof = u8::from(combined.contains(&format!("{STDIN_EOF_MARKER}:1")));
            format!(
                "ok:status={status}:bytes={}:digest={}:eof={eof}:stdout={stdout_before}:stderr={stderr_before}:stdout_bulk={stdout_bulk}:stderr_bulk={stderr_bulk}",
                bytes_line.unwrap_or_else(|| "missing".to_owned()),
                digest_line.unwrap_or_else(|| "missing".to_owned()),
            )
        }
        Err(error) => {
            let message = format!("{error:#}").replace('\n', " ");
            if message.contains("writing to stdin") {
                format!("err:write:{message}")
            } else {
                format!("err:other:{message}")
            }
        }
    }
}

fn run_driver_scenario(scenario: &str) -> TestResult<String> {
    let temp = TempDir::new("drain-driver")?;
    let command = fixture_command()?;
    let args = fixture_args();
    let payload = pipe_pressure_stdin();
    let report = match scenario {
        "concurrent-write-before-read" => report_helper_result(command_with_input_with_status(
            temp.path(),
            &command,
            &args,
            &fixture_env("write-before-read"),
            &payload,
        )),
        "sequential-write-before-read" => report_helper_result(sequential_write_then_wait(
            temp.path(),
            &command,
            &args,
            &fixture_env("write-before-read"),
            &payload,
        )),
        "early-exit" => report_helper_result(command_with_input_with_status(
            temp.path(),
            &command,
            &args,
            &fixture_env("early-exit-without-stdin"),
            &payload,
        )),
        other => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unknown drain driver scenario: {other}"),
            )
            .into());
        }
    };
    Ok(report)
}

/// Exact libtest-owned driver. Inert unless `PERL_CI_HYGIENE_PROCESS_DRIVER` is set.
#[test]
fn process_input_drain_driver() -> TestResult {
    let Ok(scenario) = std::env::var(DRIVER_SCENARIO_ENV) else {
        return Ok(());
    };
    let report = run_driver_scenario(&scenario)?;
    println!("{DRIVER_RESULT_PREFIX}{report}");
    io::stdout().flush()?;
    Ok(())
}

enum DriverDisposition {
    Completed { status: ExitStatus, stdout: String, stderr: String },
    WatchdogDeadlock { stdout: String, stderr: String },
}

fn observe_driver(scenario: &str) -> TestResult<DriverDisposition> {
    let exe = fixture_command()?;
    let mut child = Command::new(&exe)
        .args([DRIVER_FILTER, "--exact", "--nocapture"])
        .env(DRIVER_SCENARIO_ENV, scenario)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdout_pipe = child.stdout.take();
    let mut stderr_pipe = child.stderr.take();
    thread::scope(|scope| {
        let stdout_thread = scope.spawn(|| {
            let mut buffer = String::new();
            if let Some(pipe) = stdout_pipe.as_mut() {
                let _ = pipe.read_to_string(&mut buffer);
            }
            buffer
        });
        let stderr_thread = scope.spawn(|| {
            let mut buffer = String::new();
            if let Some(pipe) = stderr_pipe.as_mut() {
                let _ = pipe.read_to_string(&mut buffer);
            }
            buffer
        });
        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) if started.elapsed() >= WATCHDOG_DEADLINE => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                Ok(None) => thread::sleep(Duration::from_millis(20)),
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(error.into());
                }
            }
        };
        let stdout = match stdout_thread.join() {
            Ok(text) => text,
            Err(_) => String::from("<stdout collector panicked>"),
        };
        let stderr = match stderr_thread.join() {
            Ok(text) => text,
            Err(_) => String::from("<stderr collector panicked>"),
        };
        Ok(match status {
            Some(status) => DriverDisposition::Completed { status, stdout, stderr },
            None => DriverDisposition::WatchdogDeadlock { stdout, stderr },
        })
    })
}

fn driver_result_line(stdout: &str) -> Option<&str> {
    stdout.lines().find_map(|line| line.strip_prefix(DRIVER_RESULT_PREFIX))
}

#[test]
fn sequential_write_then_wait_deadlocks_when_child_writes_before_reading() -> TestResult {
    match observe_driver("sequential-write-before-read")? {
        DriverDisposition::WatchdogDeadlock { .. } => Ok(()),
        DriverDisposition::Completed { status, stdout, stderr } => Err(io::Error::other(format!(
            "sequential write-then-wait was expected to deadlock; driver exited {status:?}; stdout={stdout}; stderr={stderr}"
        ))
        .into()),
    }
}

#[test]
fn command_with_input_drains_while_delivering_stdin_when_child_writes_first() -> TestResult {
    let disposition = observe_driver("concurrent-write-before-read")?;
    let (status, stdout, stderr) = match disposition {
        DriverDisposition::Completed { status, stdout, stderr } => (status, stdout, stderr),
        DriverDisposition::WatchdogDeadlock { stdout, stderr } => {
            return Err(io::Error::other(format!(
                "production helper deadlocked under watchdog; stdout={stdout}; stderr={stderr}"
            ))
            .into());
        }
    };
    assert!(status.success(), "drain driver failed: {status:?}; stdout={stdout}; stderr={stderr}");
    let report = driver_result_line(&stdout).ok_or_else(|| {
        io::Error::other(format!(
            "driver did not emit a result line; stdout={stdout}; stderr={stderr}"
        ))
    })?;
    let expected_digest = format!("{:016x}", stdin_digest(pipe_pressure_stdin().as_bytes()));
    let expected = format!(
        "ok:status=0:bytes={PIPE_PRESSURE_BYTES}:digest={expected_digest}:eof=1:stdout=1:stderr=1:stdout_bulk={PIPE_PRESSURE_BYTES}:stderr_bulk={PIPE_PRESSURE_BYTES}"
    );
    assert_eq!(report, expected, "driver stderr={stderr}");
    Ok(())
}

#[test]
fn early_child_exit_during_stdin_write_is_a_typed_error() -> TestResult {
    let disposition = observe_driver("early-exit")?;
    let (status, stdout, stderr) = match disposition {
        DriverDisposition::Completed { status, stdout, stderr } => (status, stdout, stderr),
        DriverDisposition::WatchdogDeadlock { stdout, stderr } => {
            return Err(io::Error::other(format!(
                "early-exit helper deadlocked under watchdog; stdout={stdout}; stderr={stderr}"
            ))
            .into());
        }
    };
    assert!(
        status.success(),
        "early-exit driver failed: {status:?}; stdout={stdout}; stderr={stderr}"
    );
    let report = driver_result_line(&stdout).ok_or_else(|| {
        io::Error::other(format!(
            "driver did not emit a result line; stdout={stdout}; stderr={stderr}"
        ))
    })?;
    assert!(
        report.starts_with("err:write:") && report.contains("writing to stdin"),
        "expected typed stdin write error, got {report}; stderr={stderr}"
    );
    assert!(
        report.contains("child exit 3"),
        "write error should retain child status, got {report}"
    );
    Ok(())
}

#[test]
fn sequential_helper_completes_when_write_before_read_fits_in_the_pipe() -> TestResult {
    let temp = TempDir::new("sequential-small")?;
    let command = fixture_command()?;
    let args = fixture_args();
    let payload = "small-stdin-payload";
    let (status, combined) = sequential_write_then_wait(
        temp.path(),
        &command,
        &args,
        &fixture_env("write-small-before-read"),
        payload,
    )?;
    assert_eq!(status, 0);
    assert!(combined.contains(STDOUT_BEFORE_STDIN_MARKER));
    assert!(combined.contains(STDERR_BEFORE_STDIN_MARKER));
    assert!(combined.contains(&format!("{STDIN_BYTES_MARKER}:{}", payload.len())));
    assert!(
        combined
            .contains(&format!("{STDIN_DIGEST_MARKER}:{:016x}", stdin_digest(payload.as_bytes())))
    );
    let stdout_at = combined
        .find(STDOUT_BEFORE_STDIN_MARKER)
        .ok_or_else(|| io::Error::other("small sequential output omitted stdout marker"))?;
    let stderr_at = combined
        .find(STDERR_BEFORE_STDIN_MARKER)
        .ok_or_else(|| io::Error::other("small sequential output omitted stderr marker"))?;
    assert!(stdout_at < stderr_at, "post-process concatenation must keep stdout before stderr");
    Ok(())
}

#[test]
fn empty_stdin_still_closes_and_preserves_nonzero_status() -> TestResult {
    let temp = TempDir::new("empty-stdin")?;
    let command = fixture_command()?;
    let args = fixture_args();
    let (status, combined) = command_with_input_with_status(
        temp.path(),
        &command,
        &args,
        &fixture_env("stdin-exit-7"),
        "",
    )?;
    assert_eq!(status, 7);
    assert!(combined.contains(&format!("{STDIN_BYTES_MARKER}:0")));
    assert!(combined.contains(&format!("{STDIN_EOF_MARKER}:1")));
    Ok(())
}

#[test]
fn collector_panic_is_an_error_not_success() -> TestResult {
    let result = thread::scope(|scope| {
        stream_join_bytes(
            "stdout",
            join_stream(scope.spawn(|| -> io::Result<Vec<u8>> {
                #[expect(
                    clippy::panic,
                    reason = "test-only fixture for collector join mapping a thread panic to Err"
                )]
                {
                    panic!("stdin-drain collector panic fixture")
                }
            })),
        )
    });
    match result {
        Ok(_) => Err(io::Error::other("panicking collector must not become Ok").into()),
        Err(error) => {
            assert!(
                error.to_string().contains("collector thread panicked"),
                "panic must stay distinguishable, got {error}"
            );
            Ok(())
        }
    }
}
