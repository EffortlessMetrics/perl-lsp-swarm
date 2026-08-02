//! Truthful evidence for one direct child-process command (#5246).

use chrono::Utc;
use color_eyre::eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const MAX_EXCERPT_CHARS: usize = 2_000;
const MAX_CAPTURE_BYTES: usize = 4 * 1024 * 1024;
static NEXT_EVIDENCE_ID: AtomicU64 = AtomicU64::new(1);

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

#[derive(Debug, Deserialize)]
struct ProofSetSpec {
    schema_version: String,
    commands: Vec<ProofSetCommand>,
}

#[derive(Debug, Deserialize)]
struct ProofSetCommand {
    id: String,
    program: String,
    #[serde(default)]
    args: Vec<String>,
    cwd: Option<PathBuf>,
    candidate: Option<String>,
    timeout_secs: Option<u64>,
    out_dir: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
struct ProofSetItem {
    id: String,
    receipt: CommandEvidenceReceipt,
}

#[derive(Debug, Serialize)]
struct ProofSetReceipt {
    schema_version: &'static str,
    commands: Vec<ProofSetItem>,
    result: ResultClass,
}

struct CapturedOutput {
    status: ExitStatus,
    stdout: OutputCapture,
    stderr: OutputCapture,
    timed_out: bool,
    termination_note: Option<String>,
}

struct OutputCapture {
    bytes: Vec<u8>,
    truncated: bool,
}

struct SpawnFailure {
    result: ResultClass,
    message: String,
}

pub fn run(config: CommandEvidenceConfig) -> Result<()> {
    let json_only = config.json_only;
    let receipt = execute(config)?;
    emit_receipt(receipt, json_only)
}

/// Run a bounded proof set serially. Every entry runs once; a failure is
/// retained in its own receipt and does not hide later entries.
pub fn run_proof_set(path: &Path, json_only: bool) -> Result<()> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read proof-set specification {}", path.display()))?;
    let spec: ProofSetSpec = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse proof-set specification {}", path.display()))?;
    if spec.schema_version != "command-proof-set.v1" {
        bail!(
            "unsupported proof-set schema {:?}; expected command-proof-set.v1",
            spec.schema_version
        );
    }
    if spec.commands.is_empty() {
        bail!("proof-set specification must contain at least one command");
    }

    let mut ids = HashSet::new();
    let mut items = Vec::with_capacity(spec.commands.len());
    for command in spec.commands {
        if command.id.trim().is_empty() {
            bail!("proof-set command id must not be empty");
        }
        if !ids.insert(command.id.clone()) {
            bail!("proof-set command id {:?} is duplicated", command.id);
        }
        let receipt = execute(CommandEvidenceConfig {
            program: command.program,
            args: command.args,
            cwd: command.cwd,
            candidate: command.candidate,
            timeout: command.timeout_secs.map(Duration::from_secs),
            out_dir: command.out_dir,
            json_only: true,
        })?;
        items.push(ProofSetItem { id: command.id, receipt });
    }

    let result = items
        .iter()
        .map(|item| item.receipt.result)
        .find(|result| *result != ResultClass::Success)
        .unwrap_or(ResultClass::Success);
    let receipt =
        ProofSetReceipt { schema_version: "command-proof-set.v1", commands: items, result };
    if !json_only {
        for item in &receipt.commands {
            println!("{}: {:?}", item.id, item.receipt.result);
        }
    }
    println!("{}", serde_json::to_string_pretty(&receipt)?);
    if receipt.result == ResultClass::Success {
        Ok(())
    } else {
        bail!("proof-set result: {:?}", receipt.result)
    }
}

fn execute(config: CommandEvidenceConfig) -> Result<CommandEvidenceReceipt> {
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
    let evidence_id = NEXT_EVIDENCE_ID.fetch_add(1, Ordering::Relaxed);
    let stem = format!(
        "{}-{}-{}-{}",
        sanitize_filename(&config.program),
        started.timestamp_millis(),
        std::process::id(),
        evidence_id
    );
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
            return Ok(receipt);
        }
    };
    let ended = Utc::now();
    let duration_ms = started_instant.elapsed().as_millis();
    fs::write(&stdout_path, &captured.stdout.bytes)
        .with_context(|| format!("failed to write {}", stdout_path.display()))?;
    fs::write(&stderr_path, &captured.stderr.bytes)
        .with_context(|| format!("failed to write {}", stderr_path.display()))?;

    let output_incomplete = captured.stdout.truncated || captured.stderr.truncated;
    let result = classify_result(captured.timed_out, output_incomplete, captured.status.code());
    let receipt = CommandEvidenceReceipt {
        schema_version: "command-evidence.v1",
        argv,
        cwd: cwd.display().to_string(),
        candidate_identity: config.candidate,
        started_at: started.to_rfc3339(),
        ended_at: ended.to_rfc3339(),
        duration_ms,
        exit_code: captured.status.code(),
        termination: captured
            .termination_note
            .unwrap_or_else(|| termination(&captured.status, captured.timed_out)),
        stdout_reference: stdout_path.display().to_string(),
        stderr_reference: stderr_path.display().to_string(),
        stdout_excerpt: redact_excerpt(&captured.stdout.bytes),
        stderr_excerpt: redact_excerpt(&captured.stderr.bytes),
        result,
    };

    Ok(receipt)
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
                termination_note: None,
            });
        }
        if timeout.is_some_and(|bound| started.elapsed() >= bound) {
            let termination_note = terminate_process_tree(&mut child);
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
                termination_note: Some(format!("timeout; {termination_note}")),
            });
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn read_stream(
    mut stream: impl Read + Send + 'static,
) -> thread::JoinHandle<io::Result<OutputCapture>> {
    thread::spawn(move || {
        let mut output = Vec::with_capacity(MAX_CAPTURE_BYTES.min(64 * 1024));
        let mut buffer = [0_u8; 64 * 1024];
        let mut truncated = false;
        loop {
            let read = stream.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            let remaining = MAX_CAPTURE_BYTES.saturating_sub(output.len());
            if remaining == 0 {
                truncated = true;
            } else if read <= remaining {
                output.extend_from_slice(&buffer[..read]);
            } else {
                output.extend_from_slice(&buffer[..remaining]);
                truncated = true;
            }
        }
        Ok(OutputCapture { bytes: output, truncated })
    })
}

fn join_stream(
    stream: Option<thread::JoinHandle<io::Result<OutputCapture>>>,
    name: &str,
) -> Result<OutputCapture> {
    let Some(stream) = stream else {
        return Ok(OutputCapture { bytes: Vec::new(), truncated: false });
    };
    stream
        .join()
        .map_err(|_| color_eyre::eyre::eyre!("{name} reader panicked"))?
        .with_context(|| format!("failed reading {name}"))
}

fn terminate_process_tree(child: &mut Child) -> &'static str {
    let pid = child.id().to_string();
    #[cfg(windows)]
    let tree_termination_confirmed = Command::new("taskkill")
        .args(["/PID", &pid, "/T", "/F"])
        .status()
        .is_ok_and(|status| status.success());
    #[cfg(unix)]
    let tree_termination_confirmed =
        Command::new("kill").args(["-TERM", &pid]).status().is_ok_and(|status| status.success());
    #[cfg(not(any(windows, unix)))]
    let tree_termination_confirmed = false;
    let _ = child.kill();
    if tree_termination_confirmed {
        "process_tree_termination_confirmed"
    } else {
        "process_tree_termination_unconfirmed"
    }
}

fn classify_result(
    timed_out: bool,
    output_incomplete: bool,
    exit_code: Option<i32>,
) -> ResultClass {
    if timed_out {
        return ResultClass::Timeout;
    }
    if output_incomplete {
        return ResultClass::OutputIncomplete;
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
        let mut start = 0;
        while start < result.len() {
            let lower = result.to_ascii_lowercase();
            let Some(relative) = lower[start..].find(key) else { break };
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
                start = value_start + "<redacted>".len();
                if start >= result.len() {
                    break;
                }
                continue;
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
    argv.iter().map(|arg| render_argument(&redact_secrets(arg))).collect::<Vec<_>>().join(" ")
}

fn render_argument(argument: &str) -> String {
    if argument.chars().all(|ch| ch.is_ascii_alphanumeric() || "-_.:/\\".contains(ch)) {
        argument.to_string()
    } else {
        format!("\"{}\"", argument.replace('"', "\\\""))
    }
}

fn sanitize_filename(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' { ch } else { '_' })
        .take(80)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn result_classes_preserve_exit_and_timeout() {
        assert_eq!(classify_result(false, false, Some(0)), ResultClass::Success);
        assert_eq!(classify_result(false, false, Some(1)), ResultClass::Failure);
        assert_eq!(classify_result(true, false, None), ResultClass::Timeout);
        assert_eq!(classify_result(false, false, None), ResultClass::Cancelled);
        assert_eq!(classify_result(false, true, Some(0)), ResultClass::OutputIncomplete);
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
        assert_eq!(rendered, "tool \"--token=<redacted>\"");
    }

    #[test]
    fn argv_rendering_preserves_display_boundaries() {
        let rendered = render_argv(&[
            "tool".to_string(),
            "path with spaces".to_string(),
            "quote\"value".to_string(),
        ]);
        assert_eq!(rendered, "tool \"path with spaces\" \"quote\\\"value\"");
    }

    #[test]
    fn repeated_secret_keys_are_all_redacted() {
        let excerpt = redact_excerpt(b"token=one token=two");
        assert_eq!(excerpt, "token=<redacted> token=<redacted>");
    }

    #[test]
    fn stream_capture_is_bounded_and_marks_incomplete_output() -> Result<()> {
        let input = vec![b'x'; MAX_CAPTURE_BYTES + 1];
        let capture = join_stream(Some(read_stream(Cursor::new(input))), "stdout")?;
        assert_eq!(capture.bytes.len(), MAX_CAPTURE_BYTES);
        assert!(capture.truncated);
        Ok(())
    }

    #[test]
    fn filename_component_is_bounded_for_long_windows_paths() {
        let filename = sanitize_filename(&"C:\\very-long-program-name".repeat(20));
        assert_eq!(filename.len(), 80);
    }

    #[test]
    fn proof_set_runs_entries_in_order_and_preserves_each_result() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let spec = dir.path().join("proof-set.json");
        let output = dir.path().join("evidence");
        let program = if cfg!(windows) { "cmd" } else { "sh" };
        let first_args = if cfg!(windows) {
            vec!["/C".to_string(), "exit".to_string(), "0".to_string()]
        } else {
            vec!["-c".to_string(), "exit 0".to_string()]
        };
        let second_args = if cfg!(windows) {
            vec!["/C".to_string(), "exit".to_string(), "7".to_string()]
        } else {
            vec!["-c".to_string(), "exit 7".to_string()]
        };
        let document = serde_json::json!({
            "schema_version": "command-proof-set.v1",
            "commands": [
                {"id": "first", "program": program, "args": first_args, "out_dir": output},
                {"id": "second", "program": program, "args": second_args, "out_dir": output}
            ]
        });
        fs::write(&spec, serde_json::to_vec(&document)?)?;
        assert!(run_proof_set(&spec, true).is_err());
        let logs = fs::read_dir(&output)?.count();
        assert_eq!(logs, 4, "each command must retain stdout and stderr evidence");
        Ok(())
    }
}
