//! Command helpers for status regeneration.

use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::Path;
use std::process::{Child, Command, Stdio};
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

    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if started_at.elapsed() >= timeout {
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
                eprintln!("[update-status] failed to poll `{command_name}`: {err}");
                let _ = child.kill();
                break child.wait().ok();
            }
        }
    };

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

#[cfg(unix)]
fn configure_merged_command(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(not(unix))]
fn configure_merged_command(_command: &mut Command) {}

/// Terminate the direct child and the descendants it created.
///
/// Unix commands run in a dedicated process group, so signalling the negative
/// child PID reaches the whole group. Windows uses `taskkill /T` to terminate
/// the process tree rooted at the child PID. `Child::kill` remains a direct
/// fallback if the platform tree command is unavailable or races with exit.
fn terminate_merged_process_tree(child: &mut Child) {
    #[cfg(unix)]
    {
        let process_group = format!("-{}", child.id());
        let _ = Command::new("kill")
            .args(["-TERM", &process_group])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        std::thread::sleep(Duration::from_millis(100));
        let _ = Command::new("kill")
            .args(["-KILL", &process_group])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(windows)]
    {
        let pid = child.id().to_string();
        let _ = Command::new("taskkill")
            .args(["/PID", &pid, "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
}

/// Like `run_cmd` but merges stderr into stdout in one temporary file.
///
/// Essential for `cargo test -- --list`: cargo writes crate headers to stderr and test
/// names to stdout, so separate pipes lose the ordering needed for crate attribution.
/// The command is spawned directly and the file is read without joining pipe readers.
/// Timeout and poll-failure paths terminate the command's process tree; the read is
/// limited to the file length observed after reap so surviving handles cannot extend it.
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
    let mut command = Command::new(program);
    command.args(rest).current_dir(root).stdout(Stdio::from(stdout)).stderr(Stdio::from(stderr));
    configure_merged_command(&mut command);
    let child = command.spawn();
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
                terminate_merged_process_tree(&mut child);
                break child.wait().ok();
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(err) => {
                observation_failed = true;
                eprintln!("[update-status] failed to poll `{}`: {err}", args.join(" "));
                terminate_merged_process_tree(&mut child);
                break child.wait().ok();
            }
        }
    };
    if !merged_output_is_acceptable(
        observation_failed,
        status.as_ref().map(|status| status.success()),
    ) {
        if let Some(status) = status {
            eprintln!("[update-status] command exited with {status}: {}", args.join(" "));
        }
        return String::new();
    }

    let Ok(output_len) = merged.metadata().map(|metadata| metadata.len()) else {
        return String::new();
    };
    if merged.seek(SeekFrom::Start(0)).is_err() {
        return String::new();
    }
    let mut output = String::new();
    let mut snapshot = (&mut merged).take(output_len);
    if snapshot.read_to_string(&mut output).is_err() {
        return String::new();
    }
    output
}

fn merged_output_is_acceptable(observation_failed: bool, status_success: Option<bool>) -> bool {
    !observation_failed && status_success == Some(true)
}

#[cfg(test)]
mod tests;

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
