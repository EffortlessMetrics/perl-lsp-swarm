//! Exact-binary real-session matrix harness and typed receipt (#7565, anchor
//! A01 of #9415).
//!
//! The instrument: one command drives the exact configured `perl-dap` binary
//! over real `Content-Length` framed stdio, correlating requests, responses,
//! and events with bounded waits, and binding every matrix row to the exact
//! subject identity (candidate SHA, binary digest, debuggee runtime identity,
//! fixture digests, runner identity). Verdicts fail closed: missing, skipped,
//! timed-out, wrong-subject, or instrument-failed evidence is never `pass`.
//!
//! Receipt: set `PERL_DAP_MATRIX_RECEIPT` to write the versioned
//! `perl_dap_exact_matrix.v1` receipt for the installed consumer (#6694) and
//! the scorecard.
//!
//! Claim boundary: the deterministic rows prove transport/handshake/lifecycle
//! shape over the exact binary; the launch row proves real `perl -d` session
//! reachability where a debuggee runtime resolves, and records an honest
//! `not_proven` environment boundary where it does not. Backend dispatch
//! cutover, ptkdb integration, and installed-VSix journeys stay out of scope.

mod common;

use anyhow::{Context, Result, anyhow};
use common::{DebuggeePerl, debuggee_perl_or_typed_skip};
use perl_dap::DapMessage;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::thread;
use std::time::{Duration, Instant};

const MATRIX_SCHEMA_VERSION: &str = "perl_dap_exact_matrix.v1";
const MATRIX_RECEIPT_ENV: &str = "PERL_DAP_MATRIX_RECEIPT";
const PROTOCOL_AUTHORITY: &str = "DAP 1.66 (protocol authority #6737)";
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(30);

/// One bounded wait budget shared by every deterministic row.
const ROW_TIMEOUT: Duration = Duration::from_secs(15);

/// Bounded teardown budget: a child that survives this long after kill is a
/// cleanup failure (#7565 review).
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(5);

// ---------------------------------------------------------------------------
// Subject identity
// ---------------------------------------------------------------------------

fn sha256_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("reading {path:?} for digest"))?;
    Ok(format!("sha256:{}", hex(&Sha256::digest(&bytes))))
}

fn sha256_text(text: &str) -> String {
    format!("sha256:{}", hex(&Sha256::digest(text.as_bytes())))
}

fn hex(digest: &[u8]) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn resolve_matrix_binary() -> Result<PathBuf> {
    if let Some(explicit) = std::env::var_os("PERL_DAP_TEST_BINARY") {
        let path = PathBuf::from(&explicit);
        if !path.is_file() {
            return Err(anyhow!("PERL_DAP_TEST_BINARY does not point at a file: {explicit:?}"));
        }
        return Ok(path);
    }
    // The Cargo test binary's sibling target tree holds the built perl-dap
    // binary; fall back to locating it through CARGO_BIN_EXE-style layout.
    let current = std::env::current_exe().context("resolving the test executable")?;
    // <target>/debug/deps/<test>-<hash>.exe -> <target>/debug/perl-dap.exe
    let profile_dir = current
        .parent()
        .and_then(Path::parent)
        .context("test executable has no profile directory")?;
    let binary_name = if cfg!(windows) { "perl-dap.exe" } else { "perl-dap" };
    let candidate = profile_dir.join(binary_name);
    if candidate.is_file() {
        return Ok(candidate);
    }
    Err(anyhow!(
        "no exact perl-dap binary found: set PERL_DAP_TEST_BINARY (looked at {})",
        candidate.display()
    ))
}

fn git(args: &[&str]) -> Result<String> {
    let output =
        Command::new("git").args(args).output().with_context(|| format!("running git {args:?}"))?;
    if !output.status.success() {
        return Err(anyhow!("git {args:?} failed: {}", String::from_utf8_lossy(&output.stderr)));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// The exact subject identity every authoritative run binds (#7565
/// "Exact subject identity"). Field-for-field, with digests over the actual
/// bytes of the binary and fixtures — never paths alone.
#[derive(Debug)]
struct SubjectIdentity {
    schema_version: &'static str,
    candidate_sha: String,
    tree_clean: bool,
    binary_path: String,
    binary_size_bytes: u64,
    binary_sha256: String,
    runner_os: &'static str,
    runner_arch: &'static str,
    protocol_authority: &'static str,
    runtime_identity: Option<String>,
    runtime_path: Option<String>,
    fixtures: Vec<(String, String)>,
}

impl SubjectIdentity {
    fn resolve(
        binary: &Path,
        runtime: Option<&DebuggeePerl>,
        fixtures: &[(String, PathBuf)],
    ) -> Result<Self> {
        let candidate_sha = git(&["rev-parse", "HEAD"]).context("binding the candidate SHA")?;
        let dirty =
            git(&["status", "--porcelain"]).map(|status| !status.is_empty()).unwrap_or(true);
        let binary_sha256 = sha256_file(binary)?;
        let binary_size_bytes = fs::metadata(binary)
            .with_context(|| format!("stating the exact binary {binary:?}"))?
            .len();
        let fixtures = fixtures
            .iter()
            .map(|(id, path)| Ok((id.clone(), sha256_file(path)?)))
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            schema_version: MATRIX_SCHEMA_VERSION,
            candidate_sha,
            tree_clean: !dirty,
            binary_path: binary.display().to_string(),
            binary_size_bytes,
            binary_sha256,
            runner_os: std::env::consts::OS,
            runner_arch: std::env::consts::ARCH,
            protocol_authority: PROTOCOL_AUTHORITY,
            runtime_identity: runtime.map(|perl| perl.identity.clone()),
            runtime_path: runtime.map(|perl| perl.binary.display().to_string()),
            fixtures,
        })
    }

    /// Stable digest over the whole subject: two runs on different binaries,
    /// candidates, runtimes, or fixtures can never share a receipt identity.
    fn digest(&self) -> String {
        let payload = json!({
            "schema_version": self.schema_version,
            "candidate_sha": self.candidate_sha,
            "tree_clean": self.tree_clean,
            "binary_path": self.binary_path,
            "binary_size_bytes": self.binary_size_bytes,
            "binary_sha256": self.binary_sha256,
            "runner_os": self.runner_os,
            "runner_arch": self.runner_arch,
            "protocol_authority": self.protocol_authority,
            "runtime_identity": self.runtime_identity,
            "runtime_path": self.runtime_path,
            "fixtures": self.fixtures,
        });
        sha256_text(&payload.to_string())
    }
}

// ---------------------------------------------------------------------------
// Matrix contract
// ---------------------------------------------------------------------------

/// Row verdicts (#7565). Anything that is not a proven positive observation
/// is explicitly not pass. The enum carries the full receipt contract; this
/// slice does not construct every variant yet.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(dead_code, reason = "the typed receipt contract is complete; leaves fill the rest")]
enum RowVerdict {
    Pass,
    Failed { failure_class: FailureClass, detail: String },
    Unsupported { reason: String },
    NotProven { reason: String },
    InstrumentFailure { detail: String },
}

/// Failure taxonomy (#7565) — kept distinct so a timeout or fixture failure
/// is never reported as a debugger defect by default. The taxonomy is the
/// full contract; this slice does not construct every class yet.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(dead_code, reason = "the taxonomy is contract-complete; leaves fill the rest")]
enum FailureClass {
    ProductFailure,
    ProtocolShapeFailure,
    EventOrderFailure,
    FixtureFailure,
    EnvironmentFailure,
    Timeout,
    WrongSubject,
    CleanupFailure,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Disposition {
    Positive,
    Negative,
}

/// One matrix row: identity, obligations, and the observed verdict with its
/// evidence digest over the transcript.
#[derive(Debug, Clone)]
struct MatrixRow {
    id: &'static str,
    owner_issue: u32,
    family: &'static str,
    disposition: Disposition,
    timeout: Duration,
    verdict: RowVerdict,
    /// Normalized transcript summary digest; empty until the row executes.
    evidence_digest: String,
    claim_boundary: &'static str,
}

impl MatrixRow {
    fn new(
        id: &'static str,
        owner_issue: u32,
        family: &'static str,
        disposition: Disposition,
        timeout: Duration,
        claim_boundary: &'static str,
    ) -> Self {
        Self {
            id,
            owner_issue,
            family,
            disposition,
            timeout,
            verdict: RowVerdict::NotProven { reason: "row never executed".to_string() },
            evidence_digest: String::new(),
            claim_boundary,
        }
    }

    fn to_value(&self) -> Value {
        let (verdict_key, detail) = match &self.verdict {
            RowVerdict::Pass => ("pass", json!(null)),
            RowVerdict::Failed { failure_class, detail } => (
                "failed",
                json!({ "failure_class": format!("{failure_class:?}"), "detail": detail }),
            ),
            RowVerdict::Unsupported { reason } => ("unsupported", json!({ "reason": reason })),
            RowVerdict::NotProven { reason } => ("not_proven", json!({ "reason": reason })),
            RowVerdict::InstrumentFailure { detail } => {
                ("instrument_failure", json!({ "detail": detail }))
            }
        };
        json!({
            "id": self.id,
            "owner_issue": format!("#{}", self.owner_issue),
            "family": self.family,
            "disposition": format!("{:?}", self.disposition),
            "timeout_seconds": self.timeout.as_secs(),
            "verdict": verdict_key,
            "detail": detail,
            "evidence_digest": self.evidence_digest,
            "claim_boundary": self.claim_boundary,
        })
    }
}

/// The versioned receipt: subject identity plus one typed row per family.
#[derive(Debug)]
struct MatrixReceipt {
    subject: SubjectIdentity,
    rows: Vec<MatrixRow>,
}

impl MatrixReceipt {
    /// Fail-closed receipt verification: the subject must match exactly —
    /// a receipt from another SHA, binary, runtime, or fixture set is never
    /// this run's evidence.
    fn verify_against(&self, other: &SubjectIdentity) -> Result<()> {
        if self.subject.digest() != other.digest() {
            return Err(anyhow!(
                "receipt subject {} does not match run subject {}",
                self.subject.digest(),
                other.digest()
            ));
        }
        Ok(())
    }

    fn write(&self, output: &Path) -> Result<()> {
        if let Some(parent) = output.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating receipt directory {parent:?}"))?;
        }
        let receipt = json!({
            "schema_version": MATRIX_SCHEMA_VERSION,
            "subject": {
                "schema_version": self.subject.schema_version,
                "candidate_sha": self.subject.candidate_sha,
                "tree_clean": self.subject.tree_clean,
                "binary_path": self.subject.binary_path,
                "binary_size_bytes": self.subject.binary_size_bytes,
                "binary_sha256": self.subject.binary_sha256,
                "runner_os": self.subject.runner_os,
                "runner_arch": self.subject.runner_arch,
                "protocol_authority": self.subject.protocol_authority,
                "runtime_identity": self.subject.runtime_identity,
                "runtime_path": self.subject.runtime_path,
                "fixtures": self.subject.fixtures,
                "subject_digest": self.subject.digest(),
            },
            "rows": self.rows.iter().map(MatrixRow::to_value).collect::<Vec<_>>(),
            "claim_boundary": "deterministic rows prove transport/handshake/lifecycle shape \
                over the exact binary; the launch row proves real perl -d reachability where a \
                debuggee runtime resolves; everything else is an explicit boundary, not a pass",
        });
        let rendered = serde_json::to_string_pretty(&receipt)?;
        fs::write(output, format!("{rendered}\n"))
            .with_context(|| format!("writing receipt {output:?}"))
    }
}

// ---------------------------------------------------------------------------
// Session driver
// ---------------------------------------------------------------------------

/// One normalized transcript entry: direction plus a semantic summary with
/// unstable fields (seq numbers) already removed.
#[derive(Debug, Clone)]
struct TranscriptEntry {
    direction: &'static str,
    summary: String,
}

/// The exact-binary session: real stdio framing, seq-correlated requests,
/// events retained independently of request completion, bounded everywhere.
struct ExactSession {
    child: Child,
    stdin: Option<ChildStdin>,
    rx: Receiver<std::result::Result<DapMessage, String>>,
    seq: i64,
    transcript: Vec<TranscriptEntry>,
    /// Events observed incidentally (while awaiting a response or another
    /// event). They are retained, never dropped: event independence from
    /// request completion is a harness obligation, and an event racing a
    /// response must still be observable (#7565).
    pending_events: Vec<(String, Option<Value>)>,
}

impl ExactSession {
    fn spawn(binary: &Path) -> Result<Self> {
        let mut child = Command::new(binary)
            .arg("--stdio")
            .arg("--log-level")
            .arg("error")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("spawning the exact binary {binary:?}"))?;
        let stdin = child.stdin.take().context("exact binary has no stdin pipe")?;
        let stdout = child.stdout.take().context("exact binary has no stdout pipe")?;
        let (tx, rx) = channel();
        thread::spawn(move || {
            let mut reader = stdout;
            loop {
                match read_framed_message_or_eof(&mut reader) {
                    Ok(Some(message)) => {
                        if tx.send(Ok(message)).is_err() {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(error) => {
                        let _ = tx.send(Err(format!("{error:#}")));
                        break;
                    }
                }
            }
        });
        Ok(Self {
            child,
            stdin: Some(stdin),
            rx,
            seq: 0,
            transcript: Vec::new(),
            pending_events: Vec::new(),
        })
    }

    fn write_frame(&mut self, message: &DapMessage) -> Result<()> {
        let body = serde_json::to_vec(message).context("serializing the DAP request")?;
        let stdin = self.stdin.as_mut().ok_or_else(|| anyhow!("DAP stdin is closed"))?;
        write!(stdin, "Content-Length: {}\r\n\r\n", body.len())
            .context("writing the DAP frame header")?;
        stdin.write_all(&body).context("writing the DAP frame body")?;
        stdin.flush().context("flushing the DAP frame")
    }

    /// Send one request and wait for ITS response by seq correlation; events
    /// observed on the way are retained in the transcript, never dropped and
    /// never mistaken for the response.
    fn request(
        &mut self,
        command: &str,
        arguments: Option<Value>,
        budget: Duration,
    ) -> Result<perl_dap::Response> {
        self.seq += 1;
        let request_seq = self.seq;
        let request =
            DapMessage::Request { seq: request_seq, command: command.to_string(), arguments };
        self.write_frame(&request)?;
        self.transcript.push(TranscriptEntry {
            direction: "client->adapter",
            summary: format!("request {command}"),
        });
        let deadline = Instant::now() + ROW_TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(anyhow!(
                    "timed out waiting for the {command} response within {budget:?}"
                ));
            }
            match self.rx.recv_timeout(remaining) {
                Ok(Ok(DapMessage::Response {
                    seq,
                    request_seq: response_seq,
                    success,
                    command,
                    message,
                    body,
                    ..
                })) => {
                    self.transcript.push(TranscriptEntry {
                        direction: "adapter->client",
                        summary: format!("response {command} success={success}"),
                    });
                    if response_seq == request_seq {
                        return Ok(perl_dap::Response {
                            seq,
                            msg_type: "response".to_string(),
                            request_seq: response_seq,
                            success,
                            command,
                            message,
                            body,
                        });
                    }
                    // A response to a different seq is a protocol-shape
                    // observation worth keeping, but not this request's.
                }
                Ok(Ok(DapMessage::Event { event, body, .. })) => {
                    self.transcript.push(TranscriptEntry {
                        direction: "adapter->client",
                        summary: format!("event {event}"),
                    });
                    // Events stay independent of request completion: an
                    // event racing the response is retained, never dropped.
                    self.pending_events.push((event, body));
                }
                Ok(Ok(other)) => {
                    self.transcript.push(TranscriptEntry {
                        direction: "adapter->client",
                        summary: format!("message {}", message_kind(&other)),
                    });
                }
                Ok(Err(error)) => {
                    return Err(anyhow!("framing error while waiting for {command}: {error}"));
                }
                Err(RecvTimeoutError::Timeout) => {
                    return Err(anyhow!(
                        "timed out waiting for the {command} response within {budget:?}"
                    ));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(anyhow!("adapter stdout closed while waiting for {command}"));
                }
            }
        }
    }

    /// Drain buffered events for one named event within a bound.
    fn wait_for_event(&mut self, event_name: &str, bound: Duration) -> Result<perl_dap::Event> {
        // Events captured incidentally satisfy waits first.
        if let Some(index) = self.pending_events.iter().position(|(event, _)| event == event_name) {
            let (event, body) = self.pending_events.remove(index);
            return Ok(perl_dap::Event { seq: 0, msg_type: "event".to_string(), event, body });
        }
        let deadline = Instant::now() + bound;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(anyhow!("timed out waiting for the {event_name} event"));
            }
            match self.rx.recv_timeout(remaining) {
                Ok(Ok(DapMessage::Event { event, body, .. })) => {
                    self.transcript.push(TranscriptEntry {
                        direction: "adapter->client",
                        summary: format!("event {event}"),
                    });
                    if event == event_name {
                        return Ok(perl_dap::Event {
                            seq: 0,
                            msg_type: "event".to_string(),
                            event,
                            body,
                        });
                    }
                    self.pending_events.push((event, body));
                }
                Ok(Ok(DapMessage::Response { success, command, .. })) => {
                    self.transcript.push(TranscriptEntry {
                        direction: "adapter->client",
                        summary: format!("response {command} success={success}"),
                    });
                }
                Ok(Ok(other)) => {
                    self.transcript.push(TranscriptEntry {
                        direction: "adapter->client",
                        summary: format!("message {}", message_kind(&other)),
                    });
                }
                Ok(Err(error)) => {
                    return Err(anyhow!("framing error while waiting for {event_name}: {error}"));
                }
                Err(RecvTimeoutError::Timeout) => {
                    return Err(anyhow!("timed out waiting for the {event_name} event"));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(anyhow!("adapter stdout closed while waiting for {event_name}"));
                }
            }
        }
    }

    /// Teardown obligation: the adapter process must not outlive the
    /// session, and the wait is BOUNDED — a kill that fails to reap within
    /// the cleanup deadline is a cleanup failure, never an indefinite block
    /// that exceeds every row timeout (#7565 review).
    fn teardown(&mut self) -> Result<()> {
        if let Err(error) = self.child.kill() {
            // Already-exited children surface as invalid-input kills; only
            // re-raise when the child still reports running.
            if !matches!(self.child.try_wait(), Ok(Some(_))) {
                return Err(anyhow!("killing the exact binary failed: {error}"));
            }
        }
        let deadline = Instant::now() + CLEANUP_TIMEOUT;
        loop {
            match self.child.try_wait().context("polling the exact binary exit")? {
                Some(_status) => return Ok(()),
                None if Instant::now() >= deadline => {
                    return Err(anyhow!(
                        "the exact binary survived the cleanup deadline; teardown is unbounded"
                    ));
                }
                None => thread::sleep(Duration::from_millis(25)),
            }
        }
    }

    fn require_natural_exit(&mut self, bound: Duration) -> Result<()> {
        self.stdin.take();
        if self.pending_events.iter().any(|(event, _)| event == "terminated") {
            return Err(anyhow!("natural-exit session emitted duplicate terminated event"));
        }
        let deadline = Instant::now() + bound;
        let mut exit_status = None;
        let mut stdout_closed = false;
        loop {
            if exit_status.is_none() {
                exit_status = self.child.try_wait().context("polling natural adapter exit")?;
                if let Some(status) = exit_status
                    && !status.success()
                {
                    return Err(anyhow!("adapter exited unsuccessfully: {status}"));
                }
            }
            if exit_status.is_some() && stdout_closed {
                break;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(anyhow!("adapter did not exit naturally within {bound:?}"));
            }
            match self.rx.recv_timeout(remaining.min(Duration::from_millis(25))) {
                Ok(Ok(DapMessage::Event { event, body, .. })) => {
                    self.transcript.push(TranscriptEntry {
                        direction: "adapter->client",
                        summary: format!("event {event}"),
                    });
                    if event == "terminated" {
                        return Err(anyhow!(
                            "natural-exit session emitted duplicate terminated event"
                        ));
                    }
                    self.pending_events.push((event, body));
                }
                Ok(Ok(DapMessage::Response { command, success, .. })) => {
                    self.transcript.push(TranscriptEntry {
                        direction: "adapter->client",
                        summary: format!("response {command} success={success}"),
                    });
                }
                Ok(Ok(other)) => {
                    self.transcript.push(TranscriptEntry {
                        direction: "adapter->client",
                        summary: format!("message {}", message_kind(&other)),
                    });
                }
                Ok(Err(error)) => {
                    return Err(anyhow!("framing error after natural exit: {error}"));
                }
                Err(RecvTimeoutError::Disconnected) => stdout_closed = true,
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
        if self.pending_events.iter().any(|(event, _)| event == "terminated") {
            return Err(anyhow!("natural-exit session emitted duplicate terminated event"));
        }
        Ok(())
    }

    fn evidence_digest(&self) -> String {
        let rendered: Vec<String> = self
            .transcript
            .iter()
            .map(|entry| format!("{}: {}", entry.direction, entry.summary))
            .collect();
        sha256_text(&rendered.join("\n"))
    }
}

fn message_kind(message: &DapMessage) -> &'static str {
    match message {
        DapMessage::Request { .. } => "request",
        DapMessage::Response { .. } => "response",
        DapMessage::Event { .. } => "event",
    }
}

/// Distinguish a clean stream EOF from a truncated DAP frame. A clean EOF is
/// the only normal completion signal; every partially-read header or body is
/// an explicit framing failure.
fn read_framed_message_or_eof<R: Read>(reader: &mut R) -> Result<Option<DapMessage>> {
    let mut first = [0_u8; 1];
    match reader.read(&mut first).context("failed to read DAP frame header")? {
        0 => return Ok(None),
        1 => {}
        _ => return Err(anyhow!("DAP reader returned an invalid one-byte read length")),
    }
    let mut header = vec![first[0]];
    let mut byte = [0_u8; 1];
    loop {
        reader.read_exact(&mut byte).context("failed to read DAP frame header")?;
        header.push(byte[0]);
        if header.ends_with(b"\r\n\r\n") {
            break;
        }
        if header.len() > 1024 {
            return Err(anyhow!("DAP frame header exceeded 1024 bytes"));
        }
    }
    let header_text = std::str::from_utf8(&header).context("DAP frame header was not UTF-8")?;
    let content_length = header_text
        .split("\r\n")
        .find_map(|line| line.strip_prefix("Content-Length: "))
        .ok_or_else(|| anyhow!("DAP frame header missing Content-Length: {header_text:?}"))?
        .parse::<usize>()
        .context("DAP Content-Length was not a positive integer")?;
    if content_length > 16 * 1024 * 1024 {
        return Err(anyhow!("DAP frame body exceeded the 16 MiB message budget"));
    }
    let mut body = vec![0_u8; content_length];
    reader.read_exact(&mut body).context("failed to read DAP frame body")?;
    Ok(Some(serde_json::from_slice(&body).context("DAP frame body was not a DapMessage")?))
}

#[test]
fn exact_reader_accepts_clean_eof_but_rejects_partial_frames() -> Result<()> {
    let mut empty = Cursor::new(Vec::<u8>::new());
    if read_framed_message_or_eof(&mut empty)?.is_some() {
        return Err(anyhow!("empty stdout was mistaken for a DAP frame"));
    }

    let mut partial_header = Cursor::new(b"Content-Length: 4".to_vec());
    if read_framed_message_or_eof(&mut partial_header).is_ok() {
        return Err(anyhow!("partial DAP header was accepted as clean EOF"));
    }

    let mut partial_body = Cursor::new(b"Content-Length: 5\r\n\r\n{\"x".to_vec());
    if read_framed_message_or_eof(&mut partial_body).is_ok() {
        return Err(anyhow!("partial DAP body was accepted as clean EOF"));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

fn run_initialize_handshake(session: &mut ExactSession, budget: Duration) -> Result<()> {
    let response = session.request(
        "initialize",
        Some(json!({ "adapterID": "perl-dap-matrix", "clientID": "exact-session-matrix" })),
        budget,
    )?;
    if !response.success {
        return Err(anyhow!("initialize failed: {:?}", response.message));
    }
    session.wait_for_event("initialized", budget)?;
    Ok(())
}

fn run_threads_shape(session: &mut ExactSession, budget: Duration) -> Result<()> {
    let response = session.request("threads", None, budget)?;
    if !response.success {
        return Err(anyhow!("threads failed: {:?}", response.message));
    }
    let Some(body) = response.body else {
        return Err(anyhow!("threads response carried no body"));
    };
    if body.get("threads").and_then(Value::as_array).is_none() {
        return Err(anyhow!("threads body has no threads array: {body}"));
    }
    Ok(())
}

fn run_disconnect_without_session(session: &mut ExactSession, budget: Duration) -> Result<()> {
    let response =
        session.request("disconnect", Some(json!({ "terminateDebuggee": true })), budget)?;
    if !response.success {
        return Err(anyhow!("disconnect failed: {:?}", response.message));
    }
    if session.pending_events.iter().any(|(event, _)| event == "terminated") {
        return Err(anyhow!("pre-launch disconnect fabricated a terminated event"));
    }
    // Close the client side of stdio and require the adapter to honor the
    // successful no-session disconnect naturally before fallback teardown.
    session.stdin.take();
    let deadline = Instant::now() + budget;
    loop {
        match session.child.try_wait().context("polling pre-launch disconnect exit")? {
            Some(status) if status.success() => break,
            Some(status) => {
                return Err(anyhow!("pre-launch disconnect exited unsuccessfully: {status}"));
            }
            None if Instant::now() >= deadline => {
                return Err(anyhow!("pre-launch disconnect did not exit within {budget:?}"));
            }
            None => thread::sleep(Duration::from_millis(25)),
        }
    }
    Ok(())
}

/// Fold a row's protocol outcome AND its teardown into one verdict: failed
/// process cleanup can never produce a passing row (#7565 review).
fn classify_with_cleanup(
    outcome: Result<()>,
    teardown: Result<()>,
    failure_class: FailureClass,
) -> RowVerdict {
    match (outcome, teardown) {
        (Ok(()), Ok(())) => RowVerdict::Pass,
        (Err(error), _) => classify(Err(error), failure_class),
        (Ok(()), Err(cleanup)) => RowVerdict::Failed {
            failure_class: FailureClass::CleanupFailure,
            detail: format!("{cleanup:#}"),
        },
    }
}

/// The deterministic transport/handshake/lifecycle rows over the exact
/// binary, plus the wrong-subject negative control. The negative control
/// uses git — a binary that speaks no DAP framing — and distinguishes HOW
/// the fake fails: a lookalike answering initialize is a broken control, an
/// unobservable run (timeout) is an instrument failure, and only an
/// honestly observed non-answer passes (#7565 review).
fn run_deterministic_rows(binary: &Path, rows: &mut [MatrixRow]) -> Result<()> {
    // Row: initialize handshake.
    {
        let mut session = ExactSession::spawn(binary)?;
        let outcome = run_initialize_handshake(&mut session, rows[0].timeout);
        rows[0].evidence_digest = session.evidence_digest();
        let teardown = session.teardown();
        rows[0].verdict =
            classify_with_cleanup(outcome, teardown, FailureClass::ProtocolShapeFailure);
    }
    // Row: threads shape (fresh session keeps rows independent).
    {
        let mut session = ExactSession::spawn(binary)?;
        let mut outcome = run_initialize_handshake(&mut session, rows[1].timeout);
        if outcome.is_ok() {
            outcome = run_threads_shape(&mut session, rows[1].timeout);
        }
        rows[1].evidence_digest = session.evidence_digest();
        let teardown = session.teardown();
        rows[1].verdict =
            classify_with_cleanup(outcome, teardown, FailureClass::ProtocolShapeFailure);
    }
    // Row: successful pre-launch disconnect must not fabricate a terminated
    // debuggee event; launched rows own that event contract.
    {
        let mut session = ExactSession::spawn(binary)?;
        let mut outcome = run_initialize_handshake(&mut session, rows[2].timeout);
        if outcome.is_ok() {
            outcome = run_disconnect_without_session(&mut session, rows[2].timeout);
        }
        rows[2].evidence_digest = session.evidence_digest();
        let teardown = session.teardown();
        rows[2].verdict = classify_with_cleanup(outcome, teardown, FailureClass::EventOrderFailure);
    }
    // Row: wrong-subject negative control.
    {
        let fake = PathBuf::from("git");
        let mut session = match ExactSession::spawn(&fake) {
            Ok(session) => session,
            // A control that could not run proved nothing: never a pass.
            Err(error) => {
                rows[5].verdict = RowVerdict::InstrumentFailure {
                    detail: format!("the negative control could not spawn its fake: {error:#}"),
                };
                rows[5].evidence_digest = sha256_text(&format!("spawn-refused: {error:#}"));
                return Ok(());
            }
        };
        let outcome = run_initialize_handshake(&mut session, rows[5].timeout);
        rows[5].evidence_digest = session.evidence_digest();
        let teardown = session.teardown();
        rows[5].verdict = match outcome {
            Err(error) if error.to_string().contains("timed out") => {
                RowVerdict::InstrumentFailure {
                    detail: format!("the negative control could not observe the fake: {error:#}"),
                }
            }
            Err(_) => {
                if let Err(cleanup) = teardown {
                    RowVerdict::Failed {
                        failure_class: FailureClass::CleanupFailure,
                        detail: format!(
                            "the fake failed the handshake but cleanup broke: {cleanup:#}"
                        ),
                    }
                } else {
                    RowVerdict::Pass
                }
            }
            Ok(()) => RowVerdict::Failed {
                failure_class: FailureClass::WrongSubject,
                detail: "the fake backend answered the DAP handshake; the negative control \
                         cannot hold"
                    .to_string(),
            },
        };
    }
    Ok(())
}

fn classify(outcome: Result<()>, failure_class: FailureClass) -> RowVerdict {
    match outcome {
        Ok(()) => RowVerdict::Pass,
        Err(error) if error.to_string().contains("timed out") => RowVerdict::Failed {
            // A timeout is recorded as a TIMEOUT, never as a debugger
            // behavior class: it does not prove conformance and does not
            // accuse the adapter (#7565 taxonomy).
            failure_class: FailureClass::Timeout,
            detail: format!("{error:#}"),
        },
        Err(error) => RowVerdict::Failed { failure_class, detail: format!("{error:#}") },
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// The anchor acceptance: one command drives the exact binary through the
/// deterministic matrix, every row lands typed, and negative controls hold.
#[test]
fn exact_session_matrix_conforms_and_fails_closed() -> Result<()> {
    let binary = resolve_matrix_binary()?;
    let mut rows = vec![
        MatrixRow::new(
            "dap.initialize-handshake.v1",
            7565,
            "handshake",
            Disposition::Positive,
            ROW_TIMEOUT,
            "initialize response plus initialized event over real framing; no launch semantics",
        ),
        MatrixRow::new(
            "dap.threads-shape.v1",
            7565,
            "threads",
            Disposition::Positive,
            ROW_TIMEOUT,
            "threads response body shape after handshake; thread values are not identity claims",
        ),
        MatrixRow::new(
            "dap.disconnect-lifecycle.v1",
            7565,
            "lifecycle",
            Disposition::Positive,
            LAUNCH_TIMEOUT,
            "successful pre-launch disconnect response with no fabricated terminated event",
        ),
        MatrixRow::new(
            "dap.launch-perl-stop-on-entry.v1",
            7565,
            "launch",
            Disposition::Positive,
            LAUNCH_TIMEOUT,
            "real perl -d fixture launch reaching a stop; requires a resolvable debuggee runtime",
        ),
        MatrixRow::new(
            "dap.breakpoint-later-source.v1",
            7566,
            "later-source-breakpoint",
            Disposition::Positive,
            LAUNCH_TIMEOUT,
            "real perl -d later executable-line breakpoint with engine ID and exact stack source",
        ),
        MatrixRow::new(
            "dap.wrong-binary-subject.v1",
            7565,
            "negative-control",
            Disposition::Negative,
            ROW_TIMEOUT,
            "a non-DAP fake backend must not complete the handshake; the harness refuses fake \
             success",
        ),
    ];

    let runtime = debuggee_perl_or_typed_skip("exact_session_matrix_conforms_and_fails_closed");
    run_deterministic_rows(&binary, &mut rows)?;

    // The launch row executes only where a debuggee runtime resolved; it is
    // an honest not_proven environment boundary otherwise, never a silent
    // skip recorded as pass. The fixture path stays alive for the receipt.
    let mut fixtures_owned: Vec<(String, tempfile::TempDir, PathBuf)> = Vec::new();
    match runtime {
        Some(perl) => {
            let (guard, fixture_file) = write_fixture()?;
            let mut session = ExactSession::spawn(&binary)?;
            let outcome = run_launch_row(&mut session, perl, &fixture_file, rows[3].timeout);
            rows[3].evidence_digest = session.evidence_digest();
            let teardown = session.teardown();
            rows[3].verdict =
                classify_with_cleanup(outcome, teardown, FailureClass::FixtureFailure);
            fixtures_owned.push(("matrix-fixture-plain".to_string(), guard, fixture_file));
            let (later_guard, later_file) = write_later_breakpoint_fixture()?;
            let mut later_session = ExactSession::spawn(&binary)?;
            let later_outcome = run_later_source_breakpoint_row(
                &mut later_session,
                perl,
                &later_file,
                rows[4].timeout,
                true,
            );
            rows[4].evidence_digest = later_session.evidence_digest();
            let later_teardown = later_session.teardown();
            rows[4].verdict =
                classify_with_cleanup(later_outcome, later_teardown, FailureClass::FixtureFailure);
            fixtures_owned.push((
                "matrix-fixture-later-breakpoint".to_string(),
                later_guard,
                later_file,
            ));
        }
        None => {
            rows[3].verdict = RowVerdict::NotProven {
                reason: "no debuggee perl runtime resolved on this runner; the launch row is an \
                         environment boundary, not conformance evidence"
                    .to_string(),
            };
            rows[4].verdict = RowVerdict::NotProven {
                reason: "no debuggee perl runtime resolved on this runner; the later breakpoint row is an environment boundary, not conformance evidence".to_string(),
            };
        }
    }

    // The typed receipt is written BEFORE the aggregation asserts, so a
    // failing row is still receipted with its typed verdict — the scorecard
    // must see failures too, not only successes (#7565 review).
    let fixtures: Vec<(String, PathBuf)> =
        fixtures_owned.iter().map(|(name, _, path)| (name.clone(), path.clone())).collect();
    write_receipt_if_configured(&binary, runtime, &rows, &fixtures)?;

    // Fail-closed aggregation: deterministic rows must pass, the negative
    // control must hold, and nothing may end in instrument failure.
    for row in &rows {
        match &row.verdict {
            RowVerdict::Pass | RowVerdict::NotProven { .. } => {}
            RowVerdict::Failed { failure_class, detail } => {
                if row.disposition == Disposition::Negative {
                    // Negative rows pass when they honestly hold (set above);
                    // a Failed negative means the control broke.
                    return Err(anyhow!(
                        "negative control {} broke: {failure_class:?} {detail}",
                        row.id
                    ));
                }
                return Err(anyhow!("matrix row {} failed: {failure_class:?} {detail}", row.id));
            }
            RowVerdict::Unsupported { reason } => {
                return Err(anyhow!("matrix row {} unsupported: {reason}", row.id));
            }
            RowVerdict::InstrumentFailure { detail } => {
                return Err(anyhow!("matrix row {} instrument failure: {detail}", row.id));
            }
        }
    }
    Ok(())
}

/// The launch row: real `perl -d` over the exact binary, fixture identity
/// bound by digest, and an ENTRY-reason stop observed before any
/// conformance claim — the first stopped event alone does not prove the
/// debuggee reached entry (#7565 review).
fn run_launch_row(
    session: &mut ExactSession,
    perl: &DebuggeePerl,
    fixture: &Path,
    budget: Duration,
) -> Result<()> {
    let response = session.request(
        "initialize",
        Some(json!({ "adapterID": "perl-dap-matrix", "clientID": "exact-session-matrix" })),
        budget,
    )?;
    if !response.success {
        return Err(anyhow!("initialize failed: {:?}", response.message));
    }
    session.wait_for_event("initialized", budget)?;
    let response = session.request(
        "launch",
        Some(json!({
            "program": fixture.display().to_string(),
            "perlPath": perl.binary.display().to_string(),
            "stopOnEntry": true,
        })),
        budget,
    )?;
    if !response.success {
        return Err(anyhow!("launch failed: {:?}", response.message));
    }
    let stopped = session.wait_for_event("stopped", budget)?;
    let reason = stopped.body.as_ref().and_then(|body| body.get("reason")).and_then(Value::as_str);
    if reason != Some("entry") {
        return Err(anyhow!(
            "the stop event does not prove stop-on-entry (reason {reason:?}); the launch row \
             cannot claim the fixture was reached"
        ));
    }
    let response =
        session.request("disconnect", Some(json!({ "terminateDebuggee": true })), budget)?;
    if !response.success {
        return Err(anyhow!("disconnect failed: {:?}", response.message));
    }
    session.wait_for_event("terminated", budget)?;
    Ok(())
}

/// Prove a later executable source breakpoint through the public stdio
/// protocol.  The launch deliberately starts with `stopOnEntry: false`; the
/// breakpoint is installed before configurationDone, then the stopped event
/// must identify the same breakpoint and the stack must identify this exact
/// fixture and line (#7566).
fn set_later_source_breakpoint(
    session: &mut ExactSession,
    fixture: &Path,
    budget: Duration,
    expected_verified: bool,
) -> Result<i64> {
    let set = session.request(
        "setBreakpoints",
        Some(json!({
            "source": { "path": fixture.display().to_string() },
            "breakpoints": [{ "line": 4 }],
        })),
        budget,
    )?;
    if !set.success {
        return Err(anyhow!("setBreakpoints failed: {:?}", set.message));
    }
    let set_body = set.body.ok_or_else(|| anyhow!("setBreakpoints returned no body"))?;
    let breakpoints = set_body
        .get("breakpoints")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("setBreakpoints body has no breakpoints array: {set_body}"))?;
    let breakpoint =
        breakpoints.first().ok_or_else(|| anyhow!("setBreakpoints returned no entries"))?;
    if breakpoint.get("verified").and_then(Value::as_bool) != Some(expected_verified) {
        return Err(anyhow!("breakpoint verification was unexpected: {breakpoint}"));
    }
    if breakpoint.get("line").and_then(Value::as_i64) != Some(4) {
        return Err(anyhow!("later breakpoint resolved to unexpected line: {breakpoint}"));
    }
    breakpoint
        .get("id")
        .and_then(Value::as_i64)
        .ok_or_else(|| anyhow!("breakpoint had no numeric id: {breakpoint}"))
}

fn run_later_source_breakpoint_row(
    session: &mut ExactSession,
    perl: &DebuggeePerl,
    fixture: &Path,
    budget: Duration,
    prelaunch: bool,
) -> Result<()> {
    let initialized = session.request(
        "initialize",
        Some(
            json!({ "adapterID": "perl-dap-later-breakpoint", "clientID": "exact-session-matrix" }),
        ),
        budget,
    )?;
    if !initialized.success {
        return Err(anyhow!("initialize failed: {:?}", initialized.message));
    }
    session.wait_for_event("initialized", budget)?;

    // Admit the source breakpoint before launch. It must remain pending until
    // the launched debugger acknowledges the exact source and line.
    let prelaunch_id = if prelaunch {
        Some(set_later_source_breakpoint(session, fixture, budget, false)?)
    } else {
        None
    };

    let launch = session.request(
        "launch",
        Some(json!({
            "program": fixture.display().to_string(),
            "perlPath": perl.binary.display().to_string(),
            "stopOnEntry": false,
        })),
        budget,
    )?;
    if !launch.success {
        return Err(anyhow!("launch failed: {:?}", launch.message));
    }

    let breakpoint_id = if let Some(id) = prelaunch_id {
        id
    } else {
        set_later_source_breakpoint(session, fixture, budget, true)?
    };

    if prelaunch_id.is_some() {
        let changed = session.wait_for_event("breakpoint", budget)?;
        let changed_body =
            changed.body.ok_or_else(|| anyhow!("breakpoint changed event had no body"))?;
        let changed_breakpoint = changed_body.get("breakpoint").ok_or_else(|| {
            anyhow!("breakpoint changed event had no breakpoint body: {changed_body}")
        })?;
        if changed_body.get("reason").and_then(Value::as_str) != Some("changed")
            || changed_breakpoint.get("id").and_then(Value::as_i64) != Some(breakpoint_id)
            || changed_breakpoint.get("verified").and_then(Value::as_bool) != Some(true)
        {
            return Err(anyhow!(
                "pre-launch breakpoint was not verified with its original ID: {changed_body}"
            ));
        }
    }

    let configured = session.request("configurationDone", None, budget)?;
    if !configured.success {
        return Err(anyhow!("configurationDone failed: {:?}", configured.message));
    }
    let stopped = session.wait_for_event("stopped", budget)?;
    let stopped_body = stopped.body.ok_or_else(|| anyhow!("stopped event had no body"))?;
    if stopped_body.get("reason").and_then(Value::as_str) != Some("breakpoint") {
        return Err(anyhow!("later breakpoint stop had wrong reason: {stopped_body}"));
    }
    let hit_ids = stopped_body
        .get("hitBreakpointIds")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("breakpoint stop had no hitBreakpointIds: {stopped_body}"))?;
    if hit_ids.len() != 1 || hit_ids.first().and_then(Value::as_i64) != Some(breakpoint_id) {
        return Err(anyhow!(
            "stop must identify exactly breakpoint {breakpoint_id}, got {hit_ids:?}"
        ));
    }
    let thread_id = stopped_body
        .get("threadId")
        .and_then(Value::as_i64)
        .ok_or_else(|| anyhow!("breakpoint stop had no threadId: {stopped_body}"))?;

    let stack = session.request("stackTrace", Some(json!({ "threadId": thread_id })), budget)?;
    if !stack.success {
        return Err(anyhow!("stackTrace failed: {:?}", stack.message));
    }
    let stack_body = stack.body.ok_or_else(|| anyhow!("stackTrace returned no body"))?;
    let frame = stack_body
        .get("stackFrames")
        .and_then(Value::as_array)
        .and_then(|frames| frames.first())
        .ok_or_else(|| anyhow!("stackTrace returned no frames: {stack_body}"))?;
    let frame_path = frame
        .get("source")
        .and_then(|source| source.get("path"))
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("top frame had no source path: {frame}"))?;
    if Path::new(frame_path) != fixture {
        return Err(anyhow!("top frame source {frame_path:?} did not match fixture {fixture:?}"));
    }
    let frame_line = frame
        .get("line")
        .and_then(Value::as_i64)
        .ok_or_else(|| anyhow!("top frame had no line: {frame}"))?;
    if frame_line != 4 {
        return Err(anyhow!("top frame stopped at line {frame_line}, expected 4"));
    }

    let disconnected =
        session.request("disconnect", Some(json!({ "terminateDebuggee": true })), budget)?;
    if !disconnected.success {
        return Err(anyhow!("disconnect failed: {:?}", disconnected.message));
    }
    session.wait_for_event("terminated", budget)?;
    if session.pending_events.iter().any(|(event, _)| event == "terminated") {
        return Err(anyhow!("later breakpoint emitted duplicate terminated events"));
    }
    session.require_natural_exit(budget)
}

#[test]
fn exact_session_later_source_breakpoint_is_verified() -> Result<()> {
    let binary = resolve_matrix_binary()?;
    let Some(perl) =
        debuggee_perl_or_typed_skip("exact_session_later_source_breakpoint_is_verified")
    else {
        return Ok(());
    };
    let (guard, fixture) = write_later_breakpoint_fixture()?;
    let mut session = ExactSession::spawn(&binary)?;
    let outcome =
        run_later_source_breakpoint_row(&mut session, perl, &fixture, LAUNCH_TIMEOUT, true);
    let teardown = session.teardown();
    drop(guard);
    outcome.and(teardown)
}

#[test]
fn exact_session_later_source_breakpoint_after_launch_remains_verified() -> Result<()> {
    let binary = resolve_matrix_binary()?;
    let Some(perl) = debuggee_perl_or_typed_skip(
        "exact_session_later_source_breakpoint_after_launch_remains_verified",
    ) else {
        return Ok(());
    };
    let (guard, fixture) = write_later_breakpoint_fixture()?;
    let mut session = ExactSession::spawn(&binary)?;
    let outcome =
        run_later_source_breakpoint_row(&mut session, perl, &fixture, LAUNCH_TIMEOUT, false);
    let teardown = session.teardown();
    drop(guard);
    outcome.and(teardown)
}

/// Deterministic fixture content with a fixed line anchor, written to a
/// per-run tempdir; the digest lands in the subject identity.
fn write_fixture() -> Result<(tempfile::TempDir, PathBuf)> {
    // The guard is returned to the caller: the fixture lives exactly as long
    // as the test, and nothing is leaked past teardown (#7565 review).
    let dir = tempfile::tempdir().context("creating the fixture tempdir")?;
    let path = dir.path().join("matrix_fixture.pl");
    fs::write(&path, "# matrix fixture (fixed content)\nmy $anchor = 1;\nprint \"ok\\n\";\n")
        .context("writing the fixture")?;
    Ok((dir, path))
}

fn write_later_breakpoint_fixture() -> Result<(tempfile::TempDir, PathBuf)> {
    let dir = tempfile::tempdir().context("creating the later-breakpoint fixture tempdir")?;
    let path = dir.path().join("later_breakpoint_fixture.pl");
    fs::write(&path, "# later breakpoint fixture\nmy $before = 1;\nmy $also_before = 2;\nmy $target = 3;\nprint \"ok\\n\";\n")
        .context("writing the later-breakpoint fixture")?;
    Ok((dir, path))
}

fn write_receipt_if_configured(
    binary: &Path,
    runtime: Option<&DebuggeePerl>,
    rows: &[MatrixRow],
    fixtures: &[(String, PathBuf)],
) -> Result<()> {
    let Some(output) = std::env::var_os(MATRIX_RECEIPT_ENV) else {
        return Ok(());
    };
    let subject = SubjectIdentity::resolve(binary, runtime, fixtures)?;
    let receipt = MatrixReceipt { subject, rows: rows.to_vec() };
    // The verification subject is rebuilt independently from the same
    // inputs; a digest over the ACTUAL fixture files (not a hard-coded
    // constant) must match what the receipt embedded (#7565 review).
    let rebuilt = SubjectIdentity::resolve(binary, runtime, fixtures)?;
    receipt.verify_against(&rebuilt)?;
    receipt.write(Path::new(&output))
}

/// Negative control: a receipt bound to another subject is refused — never
/// accepted as this run's evidence.
#[test]
fn stale_receipt_subject_is_refused() -> Result<()> {
    let binary = resolve_matrix_binary()?;
    let runtime = debuggee_perl_or_typed_skip("stale_receipt_subject_is_refused");
    let subject = SubjectIdentity::resolve(&binary, runtime, &[])?;
    let row = MatrixRow::new(
        "dap.initialize-handshake.v1",
        7565,
        "handshake",
        Disposition::Positive,
        ROW_TIMEOUT,
        "receipt-only fixture row",
    );
    let receipt = MatrixReceipt {
        subject: SubjectIdentity::resolve(&binary, runtime, &[])?,
        rows: vec![row],
    };

    // Same subject verifies.
    receipt.verify_against(&subject)?;
    // A different candidate SHA must refuse.
    let mut other = SubjectIdentity::resolve(&binary, runtime, &[])?;
    other.candidate_sha = "0".repeat(40);
    assert!(
        receipt.verify_against(&other).is_err(),
        "a receipt from another candidate must be refused"
    );
    // A different binary digest must refuse.
    let mut other = SubjectIdentity::resolve(&binary, runtime, &[])?;
    other.binary_sha256 = "sha256:other".to_string();
    assert!(
        receipt.verify_against(&other).is_err(),
        "a receipt from another binary must be refused"
    );
    Ok(())
}
