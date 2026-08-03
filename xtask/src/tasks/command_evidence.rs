//! Truthful evidence for one direct child-process command (#5246).

use chrono::Utc;
use color_eyre::eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{self, Read};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
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

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProofSetCommand {
    pub id: String,
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub candidate: Option<String>,
    pub timeout_secs: Option<u64>,
    pub out_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProofSetItem {
    pub id: String,
    pub receipt: CommandEvidenceReceipt,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProofSetReceipt {
    pub schema_version: &'static str,
    pub commands: Vec<ProofSetItem>,
    pub result: ResultClass,
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
    let receipt = run_proof_set_receipt(path)?;
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

fn run_proof_set_receipt(path: &Path) -> Result<ProofSetReceipt> {
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
    for command in &spec.commands {
        if command.id.trim().is_empty() {
            bail!("proof-set command id must not be empty");
        }
        if !ids.insert(command.id.clone()) {
            bail!("proof-set command id {:?} is duplicated", command.id);
        }
        if command.program.trim().is_empty() {
            bail!("proof-set command {:?} must define a non-empty program", command.id);
        }
    }

    run_proof_commands(spec.commands)
}

/// Execute a caller-supplied, ordered proof set and return typed evidence
/// without printing or writing a second receipt. Integration callers consume
/// this boundary so command identity and termination state remain intact.
pub fn run_proof_commands(commands: Vec<ProofSetCommand>) -> Result<ProofSetReceipt> {
    if commands.is_empty() {
        bail!("proof-set must contain at least one command");
    }

    let mut ids = HashSet::new();
    for command in &commands {
        if command.id.trim().is_empty() {
            bail!("proof-set command id must not be empty");
        }
        if !ids.insert(command.id.clone()) {
            bail!("proof-set command id {:?} is duplicated", command.id);
        }
        if command.program.trim().is_empty() {
            bail!("proof-set command {:?} must define a non-empty program", command.id);
        }
    }

    let mut items = Vec::with_capacity(commands.len());
    for command in commands {
        let id = command.id.clone();
        let receipt = execute(CommandEvidenceConfig {
            program: command.program.clone(),
            args: command.args.clone(),
            cwd: command.cwd.clone(),
            candidate: command.candidate.clone(),
            timeout: command.timeout_secs.map(Duration::from_secs),
            out_dir: command.out_dir.clone(),
            json_only: true,
        })
        .unwrap_or_else(|error| instrument_failure_receipt(&command, error.to_string()));
        items.push(ProofSetItem { id, receipt });
    }

    let result = items
        .iter()
        .map(|item| item.receipt.result)
        .find(|result| *result != ResultClass::Success)
        .unwrap_or(ResultClass::Success);
    Ok(ProofSetReceipt { schema_version: "command-proof-set.v1", commands: items, result })
}

fn instrument_failure_receipt(command: &ProofSetCommand, error: String) -> CommandEvidenceReceipt {
    let started = Utc::now();
    CommandEvidenceReceipt {
        schema_version: "command-evidence.v1",
        argv: redact_argv(
            &std::iter::once(command.program.clone())
                .chain(command.args.clone())
                .collect::<Vec<_>>(),
        ),
        cwd: command
            .cwd
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
            .display()
            .to_string(),
        candidate_identity: command.candidate.clone(),
        started_at: started.to_rfc3339(),
        ended_at: started.to_rfc3339(),
        duration_ms: 0,
        exit_code: None,
        termination: error,
        stdout_reference: String::new(),
        stderr_reference: String::new(),
        stdout_excerpt: String::new(),
        stderr_excerpt: String::new(),
        result: ResultClass::InstrumentFailure,
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
    let argv = redact_argv(&argv);
    let out_dir = config.out_dir.unwrap_or_else(|| PathBuf::from("target/command-evidence"));
    fs::create_dir_all(&out_dir)
        .with_context(|| format!("failed to create evidence directory {}", out_dir.display()))?;
    let (stdout_path, stderr_path) = reserve_evidence_paths(&out_dir, &config.program, started)?;

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
    let result = classify_result(
        captured.timed_out,
        output_incomplete,
        captured.status.code(),
        terminated_by_signal(&captured.status),
    );
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

fn reserve_evidence_paths(
    out_dir: &Path,
    program: &str,
    started: chrono::DateTime<Utc>,
) -> Result<(PathBuf, PathBuf)> {
    loop {
        let evidence_id = NEXT_EVIDENCE_ID.fetch_add(1, Ordering::Relaxed);
        let stem = format!(
            "{}-{}-{}-{}",
            sanitize_filename(program),
            started.timestamp_millis(),
            std::process::id(),
            evidence_id
        );
        let stdout_path = out_dir.join(format!("{stem}-stdout.log"));
        let stderr_path = out_dir.join(format!("{stem}-stderr.log"));
        match OpenOptions::new().write(true).create_new(true).open(&stdout_path) {
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("failed to reserve {}", stdout_path.display()));
            }
        }
        match OpenOptions::new().write(true).create_new(true).open(&stderr_path) {
            Ok(_) => return Ok((stdout_path, stderr_path)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let _ = fs::remove_file(&stdout_path);
            }
            Err(error) => {
                let _ = fs::remove_file(&stdout_path);
                return Err(error)
                    .with_context(|| format!("failed to reserve {}", stderr_path.display()));
            }
        }
    }
}

fn spawn_and_capture(
    program: &str,
    args: &[String],
    cwd: &Path,
    timeout: Option<Duration>,
) -> std::result::Result<CapturedOutput, SpawnFailure> {
    if !cwd.is_dir() {
        return Err(SpawnFailure {
            result: ResultClass::SpawnFailure,
            message: format!("SPAWN_FAILURE: working directory does not exist: {}", cwd.display()),
        });
    }
    let mut command = Command::new(program);
    command.args(args).current_dir(cwd).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(unix)]
    command.process_group(0);
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
            let (stdout, stderr, timed_out, termination_note) = join_streams_with_deadline(
                stdout,
                stderr,
                &mut child,
                Some(Duration::from_secs(2)),
                Instant::now(),
                true,
            )
            .map_err(|error| SpawnFailure {
                result: ResultClass::OutputIncomplete,
                message: error.to_string(),
            })?;
            return Ok(CapturedOutput { status, stdout, stderr, timed_out, termination_note });
        }
        if timeout.is_some_and(|bound| started.elapsed() >= bound) {
            let termination_note = terminate_process_tree(&mut child);
            let status = child.wait().map_err(|error| SpawnFailure {
                result: ResultClass::InstrumentFailure,
                message: format!("failed waiting after timeout: {error}"),
            })?;
            let (stdout, stderr, _drain_timed_out, drain_note) = join_streams_with_deadline(
                stdout,
                stderr,
                &mut child,
                Some(Duration::from_secs(2)),
                Instant::now(),
                true,
            )
            .map_err(|error| SpawnFailure {
                result: ResultClass::OutputIncomplete,
                message: error.to_string(),
            })?;
            return Ok(CapturedOutput {
                status,
                stdout,
                stderr,
                timed_out: true,
                termination_note: Some(
                    drain_note.unwrap_or_else(|| format!("timeout; {termination_note}")),
                ),
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

fn join_streams_with_deadline(
    stdout: Option<thread::JoinHandle<io::Result<OutputCapture>>>,
    stderr: Option<thread::JoinHandle<io::Result<OutputCapture>>>,
    child: &mut Child,
    timeout: Option<Duration>,
    started: Instant,
    already_reaped: bool,
) -> Result<(OutputCapture, OutputCapture, bool, Option<String>)> {
    let deadline = timeout.map(|bound| started + bound);
    let mut timed_out = false;
    let mut process_reaped = already_reaped;
    let mut termination_note = None;
    let stdout = join_stream_with_deadline(
        stdout,
        "stdout",
        child,
        deadline,
        &mut timed_out,
        &mut process_reaped,
        &mut termination_note,
    )?;
    let stderr = join_stream_with_deadline(
        stderr,
        "stderr",
        child,
        deadline,
        &mut timed_out,
        &mut process_reaped,
        &mut termination_note,
    )?;
    Ok((stdout, stderr, timed_out, termination_note))
}

fn join_stream_with_deadline(
    stream: Option<thread::JoinHandle<io::Result<OutputCapture>>>,
    name: &str,
    child: &mut Child,
    deadline: Option<Instant>,
    timed_out: &mut bool,
    process_reaped: &mut bool,
    termination_note: &mut Option<String>,
) -> Result<OutputCapture> {
    let Some(stream) = stream else {
        return Ok(OutputCapture { bytes: Vec::new(), truncated: false });
    };
    let mut drain_deadline = deadline;
    loop {
        if stream.is_finished() {
            return join_stream(Some(stream), name);
        }
        if !*timed_out && drain_deadline.is_some_and(|bound| Instant::now() >= bound) {
            if *process_reaped {
                return Err(color_eyre::eyre::eyre!(
                    "timed out draining {name} after process reaping"
                ));
            }
            let note = terminate_process_tree(child);
            let _ = child.wait();
            *timed_out = true;
            *process_reaped = true;
            *termination_note = Some(format!("timeout; {note}"));
            drain_deadline = Some(Instant::now() + Duration::from_secs(2));
        } else if *timed_out && drain_deadline.is_some_and(|bound| Instant::now() >= bound) {
            return Err(color_eyre::eyre::eyre!(
                "timed out draining {name} after process termination"
            ));
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn terminate_process_tree(child: &mut Child) -> &'static str {
    let pid = child.id().to_string();
    #[cfg(windows)]
    let tree_termination_confirmed = Command::new("taskkill")
        .args(["/PID", &pid, "/T", "/F"])
        .status()
        .is_ok_and(|status| status.success());
    #[cfg(unix)]
    let tree_termination_confirmed = Command::new("kill")
        .args(["-TERM", &format!("-{pid}")])
        .status()
        .is_ok_and(|status| status.success());
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
    terminated_by_signal: bool,
) -> ResultClass {
    if timed_out {
        return ResultClass::Timeout;
    }
    if output_incomplete {
        return ResultClass::OutputIncomplete;
    }
    if terminated_by_signal {
        return ResultClass::Failure;
    }
    match exit_code {
        Some(0) => ResultClass::Success,
        Some(_) => ResultClass::Failure,
        None => ResultClass::Failure,
    }
}

fn terminated_by_signal(status: &ExitStatus) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        status.signal().is_some()
    }
    #[cfg(not(unix))]
    {
        let _ = status;
        false
    }
}

fn termination(status: &ExitStatus, timed_out: bool) -> String {
    if timed_out {
        return "timeout".to_string();
    }
    match status.code() {
        Some(code) => format!("exit_code:{code}"),
        None => {
            #[cfg(unix)]
            {
                use std::os::unix::process::ExitStatusExt;
                if let Some(signal) = status.signal() {
                    return format!("signal:{signal}");
                }
            }
            "terminated_without_exit_code".to_string()
        }
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

fn redact_argv(argv: &[String]) -> Vec<String> {
    let mut redacted = Vec::with_capacity(argv.len());
    let mut redact_next = false;

    for argument in argv {
        if redact_next {
            redacted.push("<redacted>".to_string());
            redact_next = false;
            continue;
        }

        let sanitized = redact_secrets(argument);
        if is_secret_option(argument) && argument == &sanitized {
            redact_next = true;
        }
        redacted.push(sanitized);
    }

    redacted
}

fn is_secret_option(argument: &str) -> bool {
    let option = argument.trim_start_matches('-').to_ascii_lowercase();
    const SECRET_KEYS: [&str; 8] = [
        "token",
        "password",
        "passwd",
        "secret",
        "authorization",
        "api-key",
        "apikey",
        "credential",
    ];
    option == "p" || SECRET_KEYS.iter().any(|key| option == *key || option.ends_with(key))
}

fn render_argv(argv: &[String]) -> String {
    redact_argv(argv).iter().map(|arg| render_argument(arg)).collect::<Vec<_>>().join(" ")
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
        assert_eq!(classify_result(false, false, Some(0), false), ResultClass::Success);
        assert_eq!(classify_result(false, false, Some(1), false), ResultClass::Failure);
        assert_eq!(classify_result(true, false, None, false), ResultClass::Timeout);
        assert_eq!(classify_result(false, false, None, false), ResultClass::Failure);
        assert_eq!(classify_result(false, true, Some(0), false), ResultClass::OutputIncomplete);
    }

    #[test]
    fn excerpts_are_bounded_and_streams_are_redacted() {
        let text = format!("token=abc password:xyz {}", "x".repeat(3_000));
        let excerpt = redact_excerpt(text.as_bytes());
        assert!(excerpt.contains("token=<redacted>"), "token value must be redacted: {excerpt}");
        assert!(
            excerpt.contains("password:<redacted>"),
            "password value must be redacted: {excerpt}"
        );
        assert!(
            excerpt.chars().count() <= MAX_EXCERPT_CHARS,
            "excerpt must stay within the character bound"
        );
    }

    #[test]
    fn argv_rendering_redacts_secret_like_values() {
        let rendered = render_argv(&["tool".to_string(), "--token=abc".to_string()]);
        assert_eq!(rendered, "tool \"--token=<redacted>\"");
    }

    #[test]
    fn argv_redaction_hides_separate_secret_values() {
        let redacted = redact_argv(&[
            "tool".to_string(),
            "--token".to_string(),
            "abc".to_string(),
            "--auth-token".to_string(),
            "def".to_string(),
            "--api-key".to_string(),
            "ghi".to_string(),
            "--access-token".to_string(),
            "jkl".to_string(),
            "-p".to_string(),
            "mno".to_string(),
            "--mode".to_string(),
            "safe".to_string(),
        ]);
        assert_eq!(
            redacted,
            [
                "tool",
                "--token",
                "<redacted>",
                "--auth-token",
                "<redacted>",
                "--api-key",
                "<redacted>",
                "--access-token",
                "<redacted>",
                "-p",
                "<redacted>",
                "--mode",
                "safe"
            ]
        );
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
        assert!(capture.truncated, "capture must be marked truncated at the byte cap");
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
        let receipt = run_proof_set_receipt(&spec)?;
        assert_eq!(receipt.result, ResultClass::Failure, "a failing entry must fail the proof set");
        assert_eq!(receipt.commands[0].id, "first", "proof entries must retain input order");
        assert_eq!(
            receipt.commands[0].receipt.result,
            ResultClass::Success,
            "first proof entry must retain its successful result"
        );
        assert_eq!(receipt.commands[1].id, "second", "proof entries must retain input order");
        assert_eq!(
            receipt.commands[1].receipt.result,
            ResultClass::Failure,
            "second proof entry must retain its failing result"
        );
        let logs = fs::read_dir(&output)?.count();
        assert_eq!(logs, 4, "each command must retain stdout and stderr evidence");
        Ok(())
    }
}
