//! Truthful evidence for one direct child-process command (#5246).

use chrono::Utc;
use color_eyre::eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const MAX_EXCERPT_CHARS: usize = 2_000;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ResultClass {
    Success,
    Failure,
    Timeout,
    Cancelled,
    MissingExecutable,
    SpawnFailure,
    OutputIncomplete,
    EnvironmentOrCapacityBlocked,
    InstrumentFailure,
    NotProven,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandEvidenceReceipt {
    pub schema_version: &'static str,
    pub argv: Vec<String>,
    pub cwd: String,
    pub candidate_identity: Option<String>,
    pub started_at: String,
    pub ended_at: String,
    pub duration_ms: u128,
    pub exit_code: Option<i32>,
    pub termination: String,
    pub stdout_reference: String,
    pub stderr_reference: String,
    pub stdout_excerpt: String,
    pub stderr_excerpt: String,
    pub result: ResultClass,
}

pub struct CommandEvidenceConfig {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub candidate: Option<String>,
    pub timeout: Option<Duration>,
    pub out_dir: Option<PathBuf>,
    pub json_only: bool,
}

struct CapturedOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    timed_out: bool,
}

struct SpawnFailure {
    result: ResultClass,
    message: String,
}

pub fn run(config: CommandEvidenceConfig) -> Result<()> {
    if config.program.trim().is_empty() {
        bail!("program must not be empty");
    }

    let cwd = config
        .cwd
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let started = Utc::now();
    let started_instant = Instant::now();
    let argv =
        std::iter::once(config.program.clone()).chain(config.args.clone()).collect::<Vec<_>>();
    let out_dir = config.out_dir.unwrap_or_else(|| PathBuf::from("target/command-evidence"));
    let stem = format!("{}-{}", sanitize_filename(&config.program), started.timestamp_millis());
    let stdout_path = out_dir.join(format!("{stem}-stdout.log"));
    let stderr_path = out_dir.join(format!("{stem}-stderr.log"));
    fs::create_dir_all(&out_dir)
        .with_context(|| format!("failed to create evidence directory {}", out_dir.display()))?;

    let captured = match spawn_and_capture(&config.program, &config.args, &cwd, config.timeout) {
        Ok(captured) => captured,
        Err(failure) => {
            fs::write(&stdout_path, [])
                .with_context(|| format!("failed to write {}", stdout_path.display()))?;
            fs::write(&stderr_path, failure.message.as_bytes())
                .with_context(|| format!("failed to write {}", stderr_path.display()))?;
            let receipt = CommandEvidenceReceipt {
                schema_version: "command-evidence.v1",
                argv,
                cwd: cwd.display().to_string(),
                candidate_identity: config.candidate,
                started_at: started.to_rfc3339(),
                ended_at: Utc::now().to_rfc3339(),
                duration_ms: started_instant.elapsed().as_millis(),
                exit_code: None,
                termination: failure.message,
                stdout_reference: stdout_path.display().to_string(),
                stderr_reference: stderr_path.display().to_string(),
                stdout_excerpt: String::new(),
                stderr_excerpt: String::new(),
                result: failure.result,
            };
            return emit_receipt(receipt, config.json_only);
        }
    };
    let ended = Utc::now();
    let duration_ms = started_instant.elapsed().as_millis();
    fs::write(&stdout_path, &captured.stdout)
        .with_context(|| format!("failed to write {}", stdout_path.display()))?;
    fs::write(&stderr_path, &captured.stderr)
        .with_context(|| format!("failed to write {}", stderr_path.display()))?;

    let result = classify_result(captured.timed_out, captured.status.code());
    let receipt = CommandEvidenceReceipt {
        schema_version: "command-evidence.v1",
        argv,
        cwd: cwd.display().to_string(),
        candidate_identity: config.candidate,
        started_at: started.to_rfc3339(),
        ended_at: ended.to_rfc3339(),
        duration_ms,
        exit_code: captured.status.code(),
        termination: termination(&captured.status, captured.timed_out),
        stdout_reference: stdout_path.display().to_string(),
        stderr_reference: stderr_path.display().to_string(),
        stdout_excerpt: redact_excerpt(&captured.stdout),
        stderr_excerpt: redact_excerpt(&captured.stderr),
        result,
    };

    emit_receipt(receipt, config.json_only)
}

fn emit_receipt(receipt: CommandEvidenceReceipt, json_only: bool) -> Result<()> {
    if !json_only {
        println!("result: {:?}", receipt.result);
        println!("command: {}", render_argv(&receipt.argv));
        println!("cwd: {}", receipt.cwd);
        if let Some(candidate) = &receipt.candidate_identity {
            println!("candidate: {candidate}");
        }
        println!("termination: {}", receipt.termination);
        println!("stdout: {}", receipt.stdout_reference);
        println!("stderr: {}", receipt.stderr_reference);
        println!("stdout excerpt: {}", receipt.stdout_excerpt);
        println!("stderr excerpt: {}", receipt.stderr_excerpt);
    }
    println!("{}", serde_json::to_string_pretty(&receipt)?);
    if receipt.result == ResultClass::Success {
        Ok(())
    } else {
        bail!("command evidence result: {:?}", receipt.result)
    }
}

fn spawn_and_capture(
    program: &str,
    args: &[String],
    cwd: &Path,
    timeout: Option<Duration>,
) -> std::result::Result<CapturedOutput, SpawnFailure> {
    let mut command = Command::new(program);
    command.args(args).current_dir(cwd).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| {
        let class = if error.kind() == io::ErrorKind::NotFound {
            ResultClass::MissingExecutable
        } else if error.kind() == io::ErrorKind::WouldBlock {
            ResultClass::EnvironmentOrCapacityBlocked
        } else {
            ResultClass::SpawnFailure
        };
        SpawnFailure {
            result: class,
            message: format!("{class:?}: failed to spawn {program}: {error}"),
        }
    })?;
    let stdout = child.stdout.take().map(read_stream);
    let stderr = child.stderr.take().map(read_stream);
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(|error| SpawnFailure {
            result: ResultClass::InstrumentFailure,
            message: format!("failed waiting for child process: {error}"),
        })? {
            return Ok(CapturedOutput {
                status,
                stdout: join_stream(stdout, "stdout").map_err(|error| SpawnFailure {
                    result: ResultClass::OutputIncomplete,
                    message: error.to_string(),
                })?,
                stderr: join_stream(stderr, "stderr").map_err(|error| SpawnFailure {
                    result: ResultClass::OutputIncomplete,
                    message: error.to_string(),
                })?,
                timed_out: false,
            });
        }
        if timeout.is_some_and(|bound| started.elapsed() >= bound) {
            terminate_process_tree(&mut child);
            let status = child.wait().map_err(|error| SpawnFailure {
                result: ResultClass::InstrumentFailure,
                message: format!("failed waiting after timeout: {error}"),
            })?;
            return Ok(CapturedOutput {
                status,
                stdout: join_stream(stdout, "stdout").map_err(|error| SpawnFailure {
                    result: ResultClass::OutputIncomplete,
                    message: error.to_string(),
                })?,
                stderr: join_stream(stderr, "stderr").map_err(|error| SpawnFailure {
                    result: ResultClass::OutputIncomplete,
                    message: error.to_string(),
                })?,
                timed_out: true,
            });
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn read_stream(mut stream: impl Read + Send + 'static) -> thread::JoinHandle<io::Result<Vec<u8>>> {
    thread::spawn(move || {
        let mut output = Vec::new();
        stream.read_to_end(&mut output).map(|_| output)
    })
}

fn join_stream(
    stream: Option<thread::JoinHandle<io::Result<Vec<u8>>>>,
    name: &str,
) -> Result<Vec<u8>> {
    let Some(stream) = stream else { return Ok(Vec::new()) };
    stream
        .join()
        .map_err(|_| color_eyre::eyre::eyre!("{name} reader panicked"))?
        .with_context(|| format!("failed reading {name}"))
}

fn terminate_process_tree(child: &mut Child) {
    let pid = child.id().to_string();
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill").args(["/PID", &pid, "/T", "/F"]).status();
    }
    #[cfg(unix)]
    {
        let _ = Command::new("kill").args(["-TERM", &pid]).status();
    }
    let _ = child.kill();
}

fn classify_result(timed_out: bool, exit_code: Option<i32>) -> ResultClass {
    if timed_out {
        return ResultClass::Timeout;
    }
    match exit_code {
        Some(0) => ResultClass::Success,
        Some(_) => ResultClass::Failure,
        None => ResultClass::Cancelled,
    }
}

fn termination(status: &ExitStatus, timed_out: bool) -> String {
    if timed_out {
        return "timeout".to_string();
    }
    match status.code() {
        Some(code) => format!("exit_code:{code}"),
        None => "terminated_without_exit_code".to_string(),
    }
}

fn redact_excerpt(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let excerpt = text.chars().take(MAX_EXCERPT_CHARS).collect::<String>();
    redact_secrets(&excerpt).chars().take(MAX_EXCERPT_CHARS).collect()
}

fn redact_secrets(text: &str) -> String {
    let mut result = text.to_string();
    for key in ["token", "password", "secret", "authorization"] {
        let lower = result.to_ascii_lowercase();
        let mut start = 0;
        while let Some(relative) = lower[start..].find(key) {
            let key_start = start + relative;
            let after_key = key_start + key.len();
            let suffix = result[after_key..].chars().next();
            if matches!(suffix, Some('=') | Some(':')) {
                let value_start = after_key + 1;
                let value_end = result[value_start..]
                    .find(char::is_whitespace)
                    .map(|offset| value_start + offset)
                    .unwrap_or(result.len());
                result.replace_range(value_start..value_end, "<redacted>");
            }
            start = after_key;
            if start >= result.len() {
                break;
            }
        }
    }
    result
}

fn render_argv(argv: &[String]) -> String {
    argv.iter().map(|arg| redact_secrets(arg)).collect::<Vec<_>>().join(" ")
}

fn sanitize_filename(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' { ch } else { '_' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn result_classes_preserve_exit_and_timeout() {
        assert_eq!(classify_result(false, Some(0)), ResultClass::Success);
        assert_eq!(classify_result(false, Some(1)), ResultClass::Failure);
        assert_eq!(classify_result(true, None), ResultClass::Timeout);
        assert_eq!(classify_result(false, None), ResultClass::Cancelled);
    }

    #[test]
    fn excerpts_are_bounded_and_streams_are_redacted() {
        let text = format!("token=abc password:xyz {}", "x".repeat(3_000));
        let excerpt = redact_excerpt(text.as_bytes());
        assert!(excerpt.contains("token=<redacted>"));
        assert!(excerpt.contains("password:<redacted>"));
        assert!(excerpt.chars().count() <= MAX_EXCERPT_CHARS);
    }

    #[test]
    fn argv_rendering_redacts_secret_like_values() {
        let rendered = render_argv(&["tool".to_string(), "--token=abc".to_string()]);
        assert_eq!(rendered, "tool --token=<redacted>");
    }
}
