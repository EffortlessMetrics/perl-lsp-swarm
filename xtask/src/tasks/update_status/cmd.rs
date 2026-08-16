//! Command helpers for status regeneration.

use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use color_eyre::eyre::Result;

/// Run a command with a timeout, returning combined stdout+stderr or empty string on failure.
fn stream_reader<R: Read>(reader: R, log_prefix: &'static str) -> String {
    let mut captured = String::new();
    let mut buf = BufReader::new(reader);
    let mut line = String::new();
    loop {
        line.clear();
        let Ok(bytes) = buf.read_line(&mut line) else {
            break;
        };
        if bytes == 0 {
            break;
        }
        eprint!("[{log_prefix}] {line}");
        captured.push_str(&line);
    }
    captured
}

pub(super) fn run_cmd(root: &Path, args: &[&str], timeout: Duration) -> String {
    let Some((&program, rest)) = args.split_first() else {
        return String::new();
    };

    eprintln!("[update-status] running: {}", args.join(" "));
    let result = Command::new(program)
        .args(rest)
        .current_dir(root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();

    let mut child = match result {
        Ok(c) => c,
        Err(err) => {
            eprintln!("[update-status] failed to start `{}`: {err}", args.join(" "));
            return String::new();
        }
    };
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let out_handle = stdout.map(|out| std::thread::spawn(move || stream_reader(out, "stdout")));
    let err_handle = stderr.map(|err| std::thread::spawn(move || stream_reader(err, "stderr")));

    let started_at = Instant::now();
    let mut last_heartbeat = started_at;
    let command_name = args.join(" ");
    let mut observation_failed = false;

    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if started_at.elapsed() >= timeout {
                    observation_failed = true;
                    eprintln!(
                        "[update-status] command timed out after {timeout:?}: {command_name}"
                    );
                    let _ = child.kill();
                    break child.wait().ok();
                }
                if last_heartbeat.elapsed() >= Duration::from_secs(30) {
                    eprintln!("[update-status] still running (heartbeat): {command_name}");
                    last_heartbeat = Instant::now();
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(err) => {
                observation_failed = true;
                eprintln!("[update-status] failed to poll `{command_name}`: {err}");
                let _ = child.kill();
                break child.wait().ok();
            }
        }
    };

    if observation_failed {
        // `kill` reaches the direct child only. A grandchild — every `rustc`
        // under a `cargo` invocation — inherits the stdout/stderr write ends
        // and keeps them open, so joining the reader threads here would block
        // until those descendants exit and defeat the very bound this timeout
        // exists to enforce. Detach the readers and fail closed instead: a
        // command that outlived its deadline, or that we stopped being able to
        // observe, has no acceptable output regardless of what the final reap
        // reports.
        drop(out_handle);
        drop(err_handle);
        return String::new();
    }

    let mut combined = String::new();
    if let Some(handle) = out_handle {
        combined.push_str(&handle.join().unwrap_or_default());
    }
    if let Some(handle) = err_handle {
        combined.push_str(&handle.join().unwrap_or_default());
    }
    if status.is_none_or(|status| !status.success()) {
        if let Some(status) = status {
            eprintln!("[update-status] command exited with {status}: {}", args.join(" "));
        }
        return String::new();
    }
    combined
}

/// Like `run_cmd` but merges stderr into stdout in one temporary file.
///
/// Essential for `cargo test -- --list`: cargo writes crate headers to stderr and test
/// names to stdout, so separate pipes lose the ordering needed for crate attribution.
/// The command is spawned directly and the file is read without joining pipe readers;
/// after a timeout, descendants that inherited the file cannot extend the bound.
pub(super) fn run_cmd_merged(root: &Path, args: &[&str], timeout: Duration) -> String {
    let Some((&program, rest)) = args.split_first() else {
        return String::new();
    };
    eprintln!("[update-status] running: {}", args.join(" "));
    let Ok(mut merged) = tempfile::tempfile() else {
        eprintln!("[update-status] failed to create merged-output file");
        return String::new();
    };
    let Ok(stdout) = merged.try_clone() else {
        eprintln!("[update-status] failed to clone merged-output file");
        return String::new();
    };
    let Ok(stderr) = merged.try_clone() else {
        eprintln!("[update-status] failed to clone merged-output file");
        return String::new();
    };
    let child = Command::new(program)
        .args(rest)
        .current_dir(root)
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn();
    let Ok(mut child) = child else {
        eprintln!("[update-status] failed to start `{}`", args.join(" "));
        return String::new();
    };

    let started_at = Instant::now();
    let mut observation_failed = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started_at.elapsed() >= timeout => {
                observation_failed = true;
                eprintln!(
                    "[update-status] command timed out after {timeout:?}: {}",
                    args.join(" ")
                );
                let _ = child.kill();
                break child.wait().ok();
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(err) => {
                observation_failed = true;
                eprintln!("[update-status] failed to poll `{}`: {err}", args.join(" "));
                let _ = child.kill();
                break child.wait().ok();
            }
        }
    };
    if !merged_output_is_acceptable(
        observation_failed,
        status.as_ref().map(|status| status.success()),
    ) {
        return String::new();
    }
    if merged.seek(SeekFrom::Start(0)).is_err() {
        return String::new();
    }
    let mut output = String::new();
    if merged.read_to_string(&mut output).is_err() {
        return String::new();
    }
    output
}

fn merged_output_is_acceptable(observation_failed: bool, status_success: Option<bool>) -> bool {
    !observation_failed && status_success == Some(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_cmd_merged_discards_failed_command_output() -> color_eyre::eyre::Result<()> {
        let output = run_cmd_merged(
            Path::new("."),
            &["rustc", "--definitely-invalid-update-status-option"],
            Duration::from_secs(10),
        );
        assert_eq!(
            output, "",
            "failed merged command output must not be treated as valid discovery data"
        );
        Ok(())
    }

    #[test]
    fn run_cmd_merged_rejects_successful_reap_after_terminal_observation_failure() {
        assert!(
            !merged_output_is_acceptable(true, Some(true)),
            "timeout or poll failure must remain terminal when kill races or fails and reap succeeds"
        );
    }

    /// `run_cmd` must honor its deadline even when a descendant inherits the
    /// output pipes.
    ///
    /// `kill` reaches only the direct child, so joining the reader threads on
    /// the timeout path waits for every grandchild to close the pipe write
    /// ends. Before this was fixed the call blocked for the full 5s sleep
    /// against a 100ms bound. `cargo check` under `count_missing_docs_perl_parser`
    /// is exactly this shape: cargo dies, its `rustc` children do not.
    #[cfg(unix)]
    #[test]
    fn run_cmd_timeout_does_not_wait_for_inherited_output_handles() {
        let started_at = Instant::now();
        let output =
            run_cmd(Path::new("."), &["sh", "-c", "sleep 5 & wait"], Duration::from_millis(100));

        assert_eq!(output, "");
        assert!(
            started_at.elapsed() < Duration::from_secs(2),
            "run_cmd blocked for {:?} past its 100ms bound",
            started_at.elapsed()
        );
    }

    #[cfg(unix)]
    #[test]
    fn run_cmd_merged_timeout_does_not_wait_for_inherited_output_handles() {
        let started_at = Instant::now();
        let output = run_cmd_merged(
            Path::new("."),
            &["sh", "-c", "sleep 5 & wait"],
            Duration::from_millis(100),
        );
        assert_eq!(output, "");
        assert!(started_at.elapsed() < Duration::from_secs(2));
    }
}

pub(super) fn run_subsystem<T>(
    name: &str,
    repro: &str,
    action: impl FnOnce() -> Result<T>,
) -> Result<T> {
    eprintln!("[update-status] starting subsystem: {name}");
    let result = action();
    match result {
        Ok(value) => {
            eprintln!("[update-status] completed subsystem: {name}");
            Ok(value)
        }
        Err(err) => {
            eprintln!("[update-status] subsystem failed: {name}");
            eprintln!("[update-status] repro: {repro}");
            Err(err)
        }
    }
}
