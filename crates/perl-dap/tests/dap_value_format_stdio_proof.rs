//! Exact public stdio proof for the #9581 `ValueFormat` capability floor.
//!
//! #9581 forces `supportsValueFormattingOptions` to an explicit `false` wire
//! row until the value-format re-enable gate passes (#9050 + #8364 + #9070 +
//! #7342/#7345 + #9588 + #9590). While it is false, a non-default `format`
//! request (`hex: true`, the pinned schema's single property) must be rejected
//! with the explicit unsupported disposition *before* any debugger/value
//! interaction, and must never be silently ignored (#9581). The #9588 typed
//! presentation policy (`ValueFormatPolicy`) stays unit-proven in
//! `src/value_format.rs`, ready for the re-enable PR to re-invert these rows.
//!
//! This suite proves the floor through the **exact public adapter boundary**:
//! it spawns the real `perl-dap` binary (`CARGO_BIN_EXE_perl-dap`, or
//! `PERL_DAP_TEST_BINARY` for an explicitly extracted candidate), drives one
//! real `perl -d` session over `Content-Length` framed stdio with a real Perl
//! fixture, and asserts the formatted behavior of every advertised
//! `ValueFormat` request family: `variables` and `evaluate` — plus the
//! `setVariable` floor (#8354) and the `setExpression` floor (#9568): the same
//! session advertises both false, refuses both with the deterministic
//! authority message, and neither refusal performs a mutation.
//!
//! Handler-level and seeded-cache unit tests cannot satisfy #9590; none are
//! used here. Every row is driven through framed stdio requests against the
//! spawned binary.
//!
//! # What is proven
//!
//! - capability-set identity: the same session that serves the rows
//!   advertises `supportsValueFormattingOptions: true` (an advertised option
//!   must be honored, never ignored);
//! - exact hex projection from typed integer authority retained at
//!   acquisition (locals rows for `255`, `-42`, `0`, `i64::MAX`, `i64::MIN`),
//!   including the full-width and sign-magnitude edges;
//! - presentation-only change: for every row, `name`, `type`,
//!   `variablesReference`, `evaluateName`, and the response's
//!   `totalVariables` are byte-identical between default and `hex` requests;
//! - no policy leak: the same scope reference serves hex → decimal → hex with
//!   each response matching its own request;
//! - unsupported options fail honestly in all four families
//!   (`Invalid arguments` naming the unknown field) and perform no hidden
//!   evaluation or mutation (side-effect canary stays empty);
//! - correlated-literal `evaluate`/read-back results are never reparsed as
//!   numeric authority: `0  255` stays `0  255` under `hex: true`;
//! - mutation floors stay closed: `setVariable` (#8354) and `setExpression`
//!   (#9568) are refused with their deterministic authority messages before
//!   any assignment work, and a correlated read-back proves the refusal
//!   performed no mutation — `$pos` stays at its fixture value `255`;
//! - formatting and inspection execute no user callbacks: tied `FETCH`/
//!   `STORE`, overload stringification, and object-method canaries stay
//!   empty for the whole session, including failed unsupported-option
//!   requests (this proof caught the locals B-walk executing tied/overload
//!   hooks on current main; the walk now reads raw slots only);
//! - `cancel` is accepted mid-session and formatted requests remain exact
//!   afterwards;
//! - stale handles: an evaluate-result reference minted before a resume
//!   serves an honest empty page after the later stop;
//! - later stops: the second suspension serves fresh values under the
//!   unchanged default presentation;
//! - cleanup: `disconnect` produces the `terminated` event and the adapter
//!   process exits.
//!
//! # Honest boundaries (asserted as-is, not hidden)
//!
//! - the locals acquisition dump (`B` pad introspection) emits pad PV text
//!   unquoted, so a numeric-looking string (`'42'`) is acquired as
//!   `Integer(42)`; while the floor holds, no hex rendering exists to
//!   distinguish acquisition fidelity, and that boundary stays owned by the
//!   value-graph family (#9050);
//! - hex projection of typed integer authority is proven at the unit layer
//!   (`src/value_format.rs` tests) and is deliberately NOT exercised through
//!   this adapter boundary while the capability is floored.
//!
//! # Receipt
//!
//! Setting `PERL_DAP_VALUE_FORMAT_RECEIPT=<path>` writes a typed receipt
//! binding the exact binary/perl/fixture identity, the capability set, every
//! row verdict, transcript counts + digest, and the cleanup disposition
//! (schema `perl_dap_value_format_stdio_floor_proof.v1`). The receipt is
//! written only after every row passed and the canary stayed empty; a missing,
//! skipped, or failed run never writes one (fail-closed).

use perl_dap::DapMessage;
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::thread;
use std::time::{Duration, Instant};

mod common;

type ProofResult<T> = Result<T, Box<dyn std::error::Error>>;

const EXPLICIT_DAP_BINARY_ENV: &str = "PERL_DAP_TEST_BINARY";
const RECEIPT_ENV: &str = "PERL_DAP_VALUE_FORMAT_RECEIPT";
const RECEIPT_SCHEMA: &str = "perl_dap_value_format_stdio_floor_proof.v1";
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(25);
const EVENT_TIMEOUT: Duration = Duration::from_secs(30);
const EXIT_TIMEOUT: Duration = Duration::from_secs(15);

/// Every matrix row this proof must record. A run that records fewer rows
/// (for example because an early return skipped a cluster) fails closed
/// instead of passing silently.
const EXPECTED_ROW_COUNT: usize = 21;

// ---------------------------------------------------------------------------
// Subject identity
// ---------------------------------------------------------------------------

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[usize::from(byte >> 4)] as char);
        out.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    out
}

fn sha256_file(path: &Path) -> ProofResult<String> {
    Ok(hex_encode(&Sha256::digest(&fs::read(path)?)))
}

fn digest_bytes(bytes: &[u8]) -> String {
    hex_encode(&Sha256::digest(bytes))
}

struct SubjectIdentity {
    binary_path: String,
    binary_len: u64,
    binary_sha256: String,
    requested_perl_path: String,
    perl_path: String,
    perl_version: String,
    /// Digest of the interpreter binary when its self-reported path is
    /// readable from the host filesystem; `unavailable:<reason>` otherwise
    /// (for example a cygwin-style `/usr/bin/perl` path on Windows). The
    /// interpreter identity is still bound by the self-reported path and
    /// version; a fabricated digest is never recorded.
    perl_sha256: String,
    fixture_path: String,
    fixture_len: u64,
    fixture_sha256: String,
}

impl SubjectIdentity {
    fn capture(binary: &OsString, perl_path: &Path, fixture: &Path) -> ProofResult<Self> {
        let perl_path_out = Command::new(perl_path).arg("-e").arg("print $^X").output()?;
        if !perl_path_out.status.success() {
            return Err(format!(
                "{} -e 'print $^X' failed while binding subject identity",
                perl_path.display()
            )
            .into());
        }
        let reported_perl_path = String::from_utf8_lossy(&perl_path_out.stdout).trim().to_string();
        if reported_perl_path.is_empty() {
            return Err(format!(
                "{} reported an empty $^X while binding subject identity",
                perl_path.display()
            )
            .into());
        }
        let reported_perl_path_buf = PathBuf::from(&reported_perl_path);

        let perl_version_out = Command::new(perl_path).arg("-e").arg("print $^V").output()?;
        if !perl_version_out.status.success() {
            return Err(format!(
                "{} -e 'print $^V' failed while binding subject identity",
                perl_path.display()
            )
            .into());
        }
        let perl_version = String::from_utf8_lossy(&perl_version_out.stdout).trim().to_string();

        let perl_sha256 = match fs::read(&reported_perl_path_buf) {
            Ok(bytes) => digest_bytes(&bytes),
            Err(error) => format!("unavailable:{error}"),
        };
        let binary_path = PathBuf::from(&binary);
        Ok(Self {
            binary_len: fs::metadata(&binary_path)?.len(),
            binary_sha256: sha256_file(&binary_path)?,
            binary_path: binary_path.to_string_lossy().to_string(),
            requested_perl_path: perl_path.to_string_lossy().to_string(),
            perl_sha256,
            perl_path: reported_perl_path,
            perl_version,
            fixture_len: fs::metadata(fixture)?.len(),
            fixture_sha256: sha256_file(fixture)?,
            fixture_path: fixture.to_string_lossy().to_string(),
        })
    }
}

// ---------------------------------------------------------------------------
// Row verdicts (typed matrix rows)
// ---------------------------------------------------------------------------

#[derive(Clone, Serialize)]
struct RowVerdict {
    row_id: &'static str,
    family: &'static str,
    format: &'static str,
    disposition: &'static str,
    note: String,
}

struct Matrix {
    rows: Vec<RowVerdict>,
}

impl Matrix {
    fn new() -> Self {
        Self { rows: Vec::new() }
    }

    fn pass(
        &mut self,
        row_id: &'static str,
        family: &'static str,
        format: &'static str,
        note: impl Into<String>,
    ) {
        self.rows.push(RowVerdict {
            row_id,
            family,
            format,
            disposition: "pass",
            note: note.into(),
        });
    }
}

// ---------------------------------------------------------------------------
// Transcript
// ---------------------------------------------------------------------------

struct Transcript {
    sent_requests: u64,
    received_responses: u64,
    received_events: u64,
    output_events: u64,
    semantic: Vec<u8>,
}

impl Transcript {
    fn new() -> Self {
        Self {
            sent_requests: 0,
            received_responses: 0,
            received_events: 0,
            output_events: 0,
            semantic: Vec::new(),
        }
    }

    fn record_sent(&mut self, seq: i64, command: &str) {
        self.sent_requests += 1;
        self.semantic.extend_from_slice(format!(">:{seq}:{command}\n").as_bytes());
    }

    fn record_received(&mut self, message: &DapMessage) {
        match message {
            DapMessage::Response { request_seq, command, success, body, .. } => {
                self.received_responses += 1;
                let body_text = serde_json::to_string(body.as_ref().unwrap_or(&Value::Null))
                    .unwrap_or_else(|_| "<unserializable>".to_string());
                self.semantic.extend_from_slice(
                    format!("<:{request_seq}:{command}:{success}:{body_text}\n").as_bytes(),
                );
            }
            DapMessage::Event { event, .. } => {
                self.received_events += 1;
                if event == "output" {
                    // Debugger chatter (banner, prompts) is transport noise,
                    // not semantic transcript; count only.
                    self.output_events += 1;
                } else {
                    self.semantic.extend_from_slice(format!("<e:{event}\n").as_bytes());
                }
            }
            DapMessage::Request { seq, command, .. } => {
                self.semantic.extend_from_slice(format!("<r:{seq}:{command}\n").as_bytes());
            }
        }
    }

    fn digest(&self) -> String {
        digest_bytes(&self.semantic)
    }
}

// ---------------------------------------------------------------------------
// Stdio session driver (exact binary, framed public boundary)
// ---------------------------------------------------------------------------

struct StdioSession {
    child: Child,
    stdin: Option<ChildStdin>,
    rx: Receiver<std::result::Result<DapMessage, String>>,
    backlog: Vec<DapMessage>,
    seq: i64,
    pub transcript: Transcript,
}

enum ResponseOutcome {
    Success(Value),
    Failure(String),
}

impl StdioSession {
    fn spawn(
        binary: &OsString,
        script: &str,
        canary_path: &str,
        perl_path: &Path,
    ) -> ProofResult<Self> {
        let mut child = Command::new(binary)
            .arg("--stdio")
            .arg("--log-level")
            .arg("error")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let stdin = child.stdin.take().ok_or("child stdin was not piped")?;
        let stdout = child.stdout.take().ok_or("child stdout was not piped")?;
        let (tx, rx) = channel();
        thread::spawn(move || {
            let mut reader = std::io::BufReader::new(stdout);
            loop {
                match read_framed_message(&mut reader) {
                    Ok(message) => {
                        if tx.send(Ok(message)).is_err() {
                            break;
                        }
                    }
                    Err(error) => {
                        let _ = tx.send(Err(error));
                        break;
                    }
                }
            }
        });

        let mut session = Self {
            child,
            stdin: Some(stdin),
            rx,
            backlog: Vec::new(),
            seq: 0,
            transcript: Transcript::new(),
        };

        let ResponseOutcome::Success(body) = session.request(
            "initialize",
            Some(json!({
                "clientID": "perl-dap-valueformat-proof",
                "adapterID": "perl-dap",
                "pathFormat": "path",
                "linesStartAt1": true,
                "columnsStartAt1": true,
            })),
        )?
        else {
            return Err("initialize over framed stdio failed".into());
        };
        // #9581 capability-set identity, in the same session that serves the
        // rows: the format and cancel cells are floored false, the #8354
        // exact-mutation authority advertises setVariable false, and the
        // #9568 promotion boundary keeps setExpression false. Main's new
        // supportsTerminateRequest row is asserted true. A wire row that
        // drifted from any of these fails here, before any row is recorded.
        for (capability, expected) in [
            ("supportsValueFormattingOptions", false),
            ("supportsCancelRequest", false),
            ("supportsSetVariable", false),
            ("supportsSetExpression", false),
            ("supportsTerminateRequest", true),
        ] {
            if body.get(capability).and_then(Value::as_bool) != Some(expected) {
                return Err(format!(
                    "capability-set identity: `{capability}` must be advertised {expected} in \
                     the same session that serves the #9581 ValueFormat floor rows"
                )
                .into());
            }
        }
        // #8354: setVariable is pinned false by the exact-mutation floor and
        // must not ride along with the other ValueFormat-session capabilities.
        if body.get("supportsSetVariable").and_then(Value::as_bool) != Some(false) {
            return Err("capability-set identity: `supportsSetVariable` must be advertised false \
                 (#8354) in the same session that serves the ValueFormat rows"
                .into());
        }
        // #9568: the same session must advertise the setExpression floor —
        // the field no longer rides on dap.core, and the ValueFormat proof
        // must observe the floored value, not the superseded true.
        if body.get("supportsSetExpression").and_then(Value::as_bool) != Some(false) {
            return Err(
                "capability-set identity: `supportsSetExpression` must be advertised false \
                 (#9568 floor) in the same session that serves the ValueFormat rows"
                    .into(),
            );
        }
        session.wait_event("initialized")?;

        // Bind the debuggee to the exact interpreter whose identity the
        // resolver proved. A configured pin is never allowed to fall back to
        // the ambient PATH at this public adapter boundary.
        let mut launch_arguments = json!({
            "program": script,
            "args": [canary_path],
            "stopOnEntry": false,
            "env": {
                "PERL_PERTURB_KEYS": "0",
                "PERL_HASH_SEED": "0",
                "LC_ALL": "C",
                "TZ": "UTC"
            }
        });
        launch_arguments["perlPath"] = Value::String(perl_path.to_string_lossy().into_owned());
        let ResponseOutcome::Success(_) = session.request("launch", Some(launch_arguments))? else {
            return Err("launch of the real perl -d fixture failed".into());
        };
        Ok(session)
    }

    fn request(&mut self, command: &str, arguments: Option<Value>) -> ProofResult<ResponseOutcome> {
        self.seq += 1;
        let seq = self.seq;
        let payload = serde_json::to_vec(&json!({
            "type": "request",
            "seq": seq,
            "command": command,
            "arguments": arguments,
        }))?;
        let stdin = self.stdin.as_mut().ok_or("client stdin already closed")?;
        write!(stdin, "Content-Length: {}\r\n\r\n", payload.len())?;
        stdin.write_all(&payload)?;
        stdin.flush()?;
        self.transcript.record_sent(seq, command);

        let deadline = Instant::now() + RESPONSE_TIMEOUT;
        loop {
            let message = self.recv_until(deadline, &format!("response `{command}`"))?;
            self.transcript.record_received(&message);
            if let DapMessage::Response {
                request_seq, command: actual, success, body, message, ..
            } = &message
                && *request_seq == seq
                && actual == command
            {
                return Ok(if *success {
                    ResponseOutcome::Success(body.clone().unwrap_or(Value::Null))
                } else {
                    ResponseOutcome::Failure(
                        message.clone().unwrap_or_else(|| "<no message>".to_string()),
                    )
                });
            }
            // Response for another request or an event: retain for later waits.
            self.backlog.push(message);
        }
    }

    fn expect_success(&mut self, command: &str, arguments: Option<Value>) -> ProofResult<Value> {
        match self.request(command, arguments)? {
            ResponseOutcome::Success(body) => Ok(body),
            ResponseOutcome::Failure(message) => {
                Err(format!("`{command}` unexpectedly failed over stdio: {message}").into())
            }
        }
    }

    fn expect_failure(&mut self, command: &str, arguments: Option<Value>) -> ProofResult<String> {
        match self.request(command, arguments)? {
            ResponseOutcome::Failure(message) => Ok(message),
            ResponseOutcome::Success(body) => Err(format!(
                "`{command}` unexpectedly succeeded over stdio (fail-closed control): {body}"
            )
            .into()),
        }
    }

    fn wait_event(&mut self, event_name: &str) -> ProofResult<Value> {
        if let Some(position) = self
            .backlog
            .iter()
            .position(|m| matches!(m, DapMessage::Event { event, .. } if event == event_name))
        {
            let message = self.backlog.remove(position);
            return Ok(match message {
                DapMessage::Event { body, .. } => body.unwrap_or(Value::Null),
                _ => Value::Null,
            });
        }
        let deadline = Instant::now() + EVENT_TIMEOUT;
        loop {
            let message = self.recv_until(deadline, &format!("event `{event_name}`"))?;
            self.transcript.record_received(&message);
            if let DapMessage::Event { event, body, .. } = &message
                && event == event_name
            {
                return Ok(body.clone().unwrap_or(Value::Null));
            }
            self.backlog.push(message);
        }
    }

    fn recv_until(&mut self, deadline: Instant, description: &str) -> ProofResult<DapMessage> {
        let now = Instant::now();
        if now >= deadline {
            return Err(format!(
                "timeout waiting for {description}; sent={} responses={} events={} backlog={}",
                self.transcript.sent_requests,
                self.transcript.received_responses,
                self.transcript.received_events,
                self.backlog.len()
            )
            .into());
        }
        match self.rx.recv_timeout(deadline.saturating_duration_since(now)) {
            Ok(Ok(message)) => Ok(message),
            Ok(Err(error)) => {
                Err(format!("framed stdio reader failed while waiting for {description}: {error}")
                    .into())
            }
            Err(RecvTimeoutError::Timeout) => {
                Err(format!("timeout waiting for {description}").into())
            }
            Err(RecvTimeoutError::Disconnected) => {
                Err(format!("framed stdio reader disconnected while waiting for {description}")
                    .into())
            }
        }
    }

    /// Close the client side of stdin (the DAP terminal signal) and wait for
    /// the adapter process to exit. The adapter serves `disconnect` and emits
    /// `terminated`, then ends its stdio loop when the client closes the
    /// stream; a process that outlives that close is a cleanup failure.
    fn close_stdin_and_wait_exit(&mut self) -> ProofResult<()> {
        if let Some(mut stdin) = self.stdin.take() {
            stdin.flush().ok();
            drop(stdin);
        }
        let deadline = Instant::now() + EXIT_TIMEOUT;
        loop {
            if let Some(status) = self.child.try_wait()? {
                if status.success() || status.code().is_none() {
                    return Ok(());
                }
                return Err(format!("adapter binary exited with {status}").into());
            }
            if Instant::now() >= deadline {
                return Err(
                    "adapter binary did not exit after stdin close within the cleanup bound".into(),
                );
            }
            thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for StdioSession {
    fn drop(&mut self) {
        if let Ok(None) = self.child.try_wait() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn read_framed_message<R: Read>(reader: &mut R) -> std::result::Result<DapMessage, String> {
    let mut header = Vec::new();
    let mut byte = [0_u8; 1];
    loop {
        reader.read_exact(&mut byte).map_err(|e| e.to_string())?;
        header.push(byte[0]);
        if header.ends_with(b"\r\n\r\n") {
            break;
        }
        if header.len() > 1024 {
            return Err("DAP frame header exceeded 1024 bytes".into());
        }
    }
    let header_text = std::str::from_utf8(&header).map_err(|e| e.to_string())?;
    let content_length: usize = header_text
        .split("\r\n")
        .find_map(|line| line.strip_prefix("Content-Length: "))
        .ok_or_else(|| format!("DAP frame header missing Content-Length: {header_text:?}"))?
        .parse()
        .map_err(|e: std::num::ParseIntError| e.to_string())?;
    let mut body = vec![0_u8; content_length];
    reader.read_exact(&mut body).map_err(|e| e.to_string())?;
    serde_json::from_slice(&body).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Matrix helpers
// ---------------------------------------------------------------------------

/// Write a human-facing note to stderr without the banned `eprintln!` macro
/// (this repository compiles tests with `-D clippy::print-stderr`).
fn note_to_stderr(text: &str) {
    use std::io::Write;
    let mut stderr = std::io::stderr().lock();
    let _ = writeln!(stderr, "{text}");
}

fn perl_available() -> bool {
    common::debuggee_perl_or_typed_skip("dap_value_format_stdio_proof").is_some()
}

fn require_perl_env() -> bool {
    perl_available()
}

fn fixture_path() -> ProofResult<PathBuf> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("value_format_stdio_matrix.pl");
    if !path.is_file() {
        return Err(format!("fixture missing: {}", path.display()).into());
    }
    Ok(path)
}

/// 1-based line number of the first fixture line that begins with `needle`
/// after trimming (so documentation comments quoting the marker cannot shadow
/// the executable statement itself).
fn fixture_line(needle: &str) -> ProofResult<i64> {
    let content = fs::read_to_string(fixture_path()?)?;
    content
        .lines()
        .enumerate()
        .find(|(_, line)| line.trim_start().starts_with(needle))
        .map(|(index, _)| i64::try_from(index + 1).unwrap_or(0))
        .ok_or_else(|| format!("fixture line marker `{needle}` not found").into())
}

/// Poll `stackTrace` until the top frame reflects `wanted_line`.
///
/// On slow hosts the adapter's frame tracking can still hold the implicit
/// first-line stop context for a moment after the breakpoint `stopped` event;
/// the assertions must run against the proof stop, so the harness waits for
/// the frame to settle instead of trusting the first response. Bounded and
/// fail-closed: no settled frame means no proof rows.
fn stack_trace_until_line(dap: &mut StdioSession, wanted_line: i64) -> ProofResult<(i64, i64)> {
    let mut last_mismatch = String::new();
    for _ in 0..10 {
        let stack = dap.expect_success(
            "stackTrace",
            Some(json!({ "threadId": 1, "startFrame": 0, "levels": 1 })),
        )?;
        let frame = stack
            .get("stackFrames")
            .and_then(Value::as_array)
            .and_then(|frames| frames.first())
            .ok_or("stackTrace returned no frames")?;
        let frame_id = frame.get("id").and_then(Value::as_i64).ok_or("frame missing id")?;
        let frame_line = frame.get("line").and_then(Value::as_i64).unwrap_or(0);
        if frame_line == wanted_line {
            return Ok((frame_id, frame_line));
        }
        last_mismatch = format!("frame line {frame_line} != wanted {wanted_line}");
        thread::sleep(Duration::from_millis(100));
    }
    Err(format!("stack frame never settled on line {wanted_line}: {last_mismatch}").into())
}

fn row_by_name<'a>(body: &'a Value, name: &str) -> ProofResult<&'a Value> {
    body.get("variables")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter().find(|row| row.get("name").and_then(Value::as_str) == Some(name))
        })
        .ok_or_else(|| {
            format!(
                "variables response has no row named `{name}`; rows: {}",
                serde_json::to_string(&body.get("variables")).unwrap_or_default()
            )
            .into()
        })
}

fn assert_canary_empty(canary_path: &Path, context: &str) -> ProofResult<()> {
    let empty = if !canary_path.exists() {
        true
    } else {
        fs::read_to_string(canary_path)?.trim().is_empty()
    };
    if empty {
        Ok(())
    } else {
        Err(format!(
            "{context}: side-effect canary must stay empty; user callbacks executed: {}",
            fs::read_to_string(canary_path)?.trim()
        )
        .into())
    }
}

fn configured_dap_binary() -> OsString {
    std::env::var_os(EXPLICIT_DAP_BINARY_ENV)
        .unwrap_or_else(|| OsString::from(env!("CARGO_BIN_EXE_perl-dap")))
}

/// Receipt output path, gated by `PERL_DAP_VALUE_FORMAT_RECEIPT`. Reading the
/// environment (never mutating it) keeps the gate test-local and thread-safe.
fn receipt_output() -> Option<PathBuf> {
    std::env::var_os(RECEIPT_ENV).map(PathBuf::from)
}

fn write_receipt_to(
    output: &Path,
    identity: &SubjectIdentity,
    matrix: &Matrix,
    transcript: &Transcript,
    cleanup: &str,
) -> ProofResult<()> {
    if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let receipt = json!({
        "schema_version": RECEIPT_SCHEMA,
        "status": "pass",
        "subject": {
            "binary": {
                "path": identity.binary_path,
                "len": identity.binary_len,
                "sha256": identity.binary_sha256,
            },
            "perl": {
                "path": identity.perl_path,
                "requested_path": identity.requested_perl_path,
                "version": identity.perl_version,
                "sha256": identity.perl_sha256,
            },
            "fixture": {
                "path": identity.fixture_path,
                "len": identity.fixture_len,
                "sha256": identity.fixture_sha256,
            },
            "capabilities": {
                "supportsValueFormattingOptions": false,
                "supportsSetVariable": false,
                "supportsSetExpression": false,
                "supportsCancelRequest": false,
            },
        },
        "rows": matrix.rows,
        "transcript": {
            "sent_requests": transcript.sent_requests,
            "received_responses": transcript.received_responses,
            "received_events": transcript.received_events,
            "output_events_ignored": transcript.output_events,
            "semantic_sha256": transcript.digest(),
        },
        "cleanup": cleanup,
    });
    let staged = output.with_extension("json.partial");
    fs::write(&staged, format!("{}\n", serde_json::to_string_pretty(&receipt)?))?;
    fs::rename(&staged, output)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// The proof
// ---------------------------------------------------------------------------

#[test]
fn value_format_stdio_proof_matrix() -> ProofResult<()> {
    if !require_perl_env() {
        note_to_stderr("SKIP value_format_stdio_proof_matrix: perl not available");
        return Ok(());
    }

    let binary = configured_dap_binary();
    let fixture = fixture_path()?;
    let perl_path = common::resolve_launch_perl_path()
        .map_err(std::io::Error::other)?
        .ok_or("the availability gate resolved no pipe-capable launch interpreter")?;
    let identity = SubjectIdentity::capture(&binary, &perl_path, &fixture)?;
    let stop1_line = fixture_line("$VF::stop1 = 1;")?;
    let stop2_line = fixture_line("$VF::stop2 = 1;")?;
    assert!(stop2_line > stop1_line, "fixture must define STOP2 after STOP1");

    let workspace = tempfile::tempdir()?;
    let canary_path = workspace.path().join("canary.log");
    let script_str = fixture.to_str().ok_or("fixture path is not valid UTF-8")?.to_string();
    let canary_str = canary_path.to_str().ok_or("canary path is not valid UTF-8")?.to_string();

    // Fail-closed receipt handling: remove any previous artifact at this path
    // before the run starts, so a skip, a failure, or a timeout can never leave
    // a stale `pass` receipt that a collector could misattribute to this run.
    if let Some(output) = receipt_output() {
        match fs::remove_file(&output) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "cannot invalidate stale receipt {}: {error}",
                    output.display()
                )
                .into());
            }
        }
    }

    let mut matrix = Matrix::new();
    let mut dap = StdioSession::spawn(&binary, &script_str, &canary_str, &perl_path)?;

    // Breakpoints on both proof stops, verified by the adapter.
    let bp_body = dap.expect_success(
        "setBreakpoints",
        Some(json!({
            "source": { "path": script_str },
            "breakpoints": [{ "line": stop1_line }, { "line": stop2_line }]
        })),
    )?;
    let breakpoints = bp_body
        .get("breakpoints")
        .and_then(Value::as_array)
        .ok_or("setBreakpoints response missing `breakpoints` array")?;
    if breakpoints.len() != 2
        || breakpoints.iter().any(|b| b.get("verified").and_then(Value::as_bool) != Some(true))
    {
        return Err("both proof breakpoints must be verified".into());
    }
    dap.expect_success("configurationDone", None)?;
    dap.wait_event("stopped")?;

    // --- current frame and locals scope -----------------------------------
    let (frame_id, frame_line) = stack_trace_until_line(&mut dap, stop1_line)?;
    let scopes = dap.expect_success("scopes", Some(json!({ "frameId": frame_id })))?;
    let locals_ref = scopes
        .get("scopes")
        .and_then(Value::as_array)
        .and_then(|scopes| scopes.first())
        .and_then(|scope| scope.get("variablesReference"))
        .and_then(Value::as_i64)
        .ok_or("scopes response missing Locals variablesReference")?;

    // --- variables: default baseline --------------------------------------
    let baseline =
        dap.expect_success("variables", Some(json!({ "variablesReference": locals_ref })))?;
    let baseline_total = baseline.get("totalVariables").and_then(Value::as_i64);
    if baseline.get("variables").and_then(Value::as_array).is_none() {
        return Err("baseline variables response must carry the variables array".into());
    }
    matrix.pass("variables-default-baseline", "variables", "default", "no-format contract intact");

    // --- the #9581 floor: hex requests are rejected in all four families ----
    // setVariable/setExpression are refused by their own authorities first
    // (#8354 exact-mutation proof and the #9568 promotion boundary), so their
    // deterministic refusals are the authority messages; variables/evaluate
    // carry the #9581 ValueFormat floor disposition.
    {
        let vfo_floor_cells: [(&str, Value); 2] = [
            ("variables", json!({ "variablesReference": locals_ref, "format": { "hex": true } })),
            (
                "evaluate",
                json!({ "expression": "$pos", "frameId": frame_id, "format": { "hex": true } }),
            ),
        ];
        for (command, arguments) in vfo_floor_cells {
            let message = dap.expect_failure(command, Some(arguments))?;
            if !message.contains("unsupported")
                || !message.contains("supportsValueFormattingOptions")
                || !message.contains("#9581")
            {
                return Err(format!(
                    "`{command}` with `hex: true` must get the explicit #9581 unsupported \
                     disposition before any debugger interaction, got: {message}"
                )
                .into());
            }
            // Rejection is pre-effect: no user callback can have run.
            assert_canary_empty(&canary_path, &format!("after floored hex {command}"))?;
            matrix.pass(
                "value-format-floor-hex-rejected",
                command,
                "hex",
                "explicit #9581 unsupported; no debugger/value interaction",
            );
        }
        let authority_refusals: [(&str, &str); 2] = [
            (
                "setVariable",
                "supportsSetVariable is advertised false because perl-dap has no exact mutation path yet",
            ),
            (
                "setExpression",
                "setExpression is not supported: supportsSetExpression is advertised",
            ),
        ];
        for (command, expected_fragment) in authority_refusals {
            let arguments = if command == "setVariable" {
                json!({ "variablesReference": locals_ref, "name": "$pos", "value": "1", "format": { "hex": true } })
            } else {
                json!({ "expression": "$pos", "value": "1", "frameId": frame_id, "format": { "hex": true } })
            };
            let message = dap.expect_failure(command, Some(arguments))?;
            if !message.contains(expected_fragment) {
                return Err(format!(
                    "`{command}` with `hex: true` must get its deterministic authority refusal \
                     containing {expected_fragment:?}, got: {message}"
                )
                .into());
            }
            assert_canary_empty(&canary_path, &format!("after authority-refused hex {command}"))?;
            matrix.pass(
                "capability-authority-hex-refused",
                command,
                "hex",
                "deterministic authority refusal; no debugger/value interaction",
            );
        }

        // The floored rejection left the suspension untouched: the default
        // rendering of the same reference is byte-identical to the baseline.
        let after_floor =
            dap.expect_success("variables", Some(json!({ "variablesReference": locals_ref })))?;
        if after_floor.get("variables") != baseline.get("variables") {
            return Err(
                "default variables after floored hex requests must stay byte-identical".into()
            );
        }
        if after_floor.get("totalVariables").and_then(Value::as_i64) != baseline_total {
            return Err("totalVariables changed after floored hex requests".into());
        }
        matrix.pass(
            "variables-unchanged-after-floor-rejections",
            "variables",
            "default",
            "floor rejections performed no state mutation",
        );
    }

    // --- variables: hex:false and {} behave exactly as default -------------
    let hex_false = dap.expect_success(
        "variables",
        Some(json!({ "variablesReference": locals_ref, "format": { "hex": false } })),
    )?;
    if hex_false.get("variables") != baseline.get("variables") {
        return Err(
            "format { \"hex\": false } must be byte-identical to the default rendering".into()
        );
    }
    matrix.pass(
        "variables-hex-false-is-default",
        "variables",
        "hex:false",
        "default-equivalent format keeps the independent no-format contract",
    );
    let empty_format = dap.expect_success(
        "variables",
        Some(json!({ "variablesReference": locals_ref, "format": {} })),
    )?;
    if empty_format.get("variables") != baseline.get("variables") {
        return Err("empty format object must be byte-identical to the default rendering".into());
    }
    matrix.pass(
        "variables-empty-format-is-default",
        "variables",
        "format:{}",
        "default-equivalent format keeps the independent no-format contract",
    );

    // --- no state leak across requests sharing one reference ---------------
    {
        // Another hex request on the same reference is floored-rejected...
        let floor_message = dap.expect_failure(
            "variables",
            Some(json!({ "variablesReference": locals_ref, "format": { "hex": true } })),
        )?;
        if !floor_message.contains("unsupported") {
            return Err("hex on a served reference must stay floored (#9581)".into());
        }
        // ...and the same reference still serves the exact decimal baseline.
        let again_default =
            dap.expect_success("variables", Some(json!({ "variablesReference": locals_ref })))?;
        if row_by_name(&again_default, "$pos")?.get("value")
            != Some(&Value::String("255".to_string()))
        {
            return Err("decimal after floored hex on the same reference must be exact".into());
        }
        matrix.pass(
            "variables-no-format-leak",
            "variables",
            "default",
            "each response matches its own request; no floor state leaks",
        );
    }

    // --- evaluate: the default contract is untouched ------------------------
    {
        let default_eval = dap.expect_success(
            "evaluate",
            Some(json!({ "expression": "$pos", "frameId": frame_id })),
        )?;
        let default_result = default_eval.get("result").and_then(Value::as_str).unwrap_or("");
        if default_result.is_empty() {
            return Err("default evaluate must still serve its result".into());
        }
        matrix.pass(
            "evaluate-default-contract-intact",
            "evaluate",
            "default",
            format!("`{default_result}` served under the unchanged no-format contract"),
        );

        // A hex evaluate on the same expression is floored-rejected.
        let hex_eval_message = dap.expect_failure(
            "evaluate",
            Some(json!({ "expression": "$pos", "frameId": frame_id, "format": { "hex": true } })),
        )?;
        if !hex_eval_message.contains("unsupported") {
            return Err("hex evaluate must be floored-rejected (#9581)".into());
        }
        matrix.pass(
            "evaluate-hex-floored",
            "evaluate",
            "hex",
            "explicit #9581 unsupported; no hidden evaluation",
        );
    }
    assert_canary_empty(&canary_path, "after evaluate rows")?;

    // --- rejected user code never executes (canary control) ----------------
    {
        // A policy-rejected expression (dangerous op name) must fail BEFORE any
        // debugger command: the canary proves the debuggee never ran it. The
        // blanket method-call refusal for innocent names is owned by the
        // Watch/hover policy family (#7567), not by ValueFormat.
        let message = dap.expect_failure(
            "evaluate",
            Some(json!({ "expression": "$over->print", "frameId": frame_id })),
        )?;
        if message.contains("Invalid arguments") {
            return Err(format!(
                "method-call rejection must come from policy, not format parsing: {message}"
            )
            .into());
        }
        assert_canary_empty(&canary_path, "after rejected method-call evaluate")?;
        matrix.pass(
            "evaluate-user-code-never-executed",
            "evaluate",
            "default",
            "policy-rejected method call refused before any debugger command; canary empty",
        );
    }

    // --- unsupported options: one documented behavior in all four families -
    {
        let cells: [(&str, Value); 2] = [
            ("variables", json!({ "variablesReference": locals_ref, "format": { "radix": 16 } })),
            (
                "evaluate",
                json!({ "expression": "$pos", "frameId": frame_id, "format": { "radix": 16 } }),
            ),
        ];
        for (command, arguments) in cells {
            let message = dap.expect_failure(command, Some(arguments))?;
            if !message.contains("Invalid arguments") || !message.contains("radix") {
                return Err(format!(
                    "`{command}` must fail naming the unknown format option: {message}"
                )
                .into());
            }
        }
        // #8354: setVariable with an unknown format option must receive the
        // deterministic capability-floor refusal — the gate fires before
        // argument parsing, so the request never reaches the envelope layer.
        {
            let message = dap.expect_failure(
                "setVariable",
                Some(json!({ "variablesReference": locals_ref, "name": "$pos", "value": "5", "format": { "radix": 16 } })),
            )?;
            if !message.contains("supportsSetVariable") {
                return Err(format!(
                    "`setVariable` with an unknown format option must receive the #8354 floor \
                     refusal before argument parsing, got: {message}"
                )
                .into());
            }
        }
        // #9568: setExpression with a *malformed* format still fails at the
        // envelope layer (typed format deserialization precedes the gate —
        // the same ordering the floor suite pins). The floor refusal is
        // covered by the dedicated leg below. Fail-closed either way.
        {
            let message = dap.expect_failure(
                "setExpression",
                Some(json!({ "expression": "$pos", "value": "5", "frameId": frame_id, "format": { "radix": 16 } })),
            )?;
            if !message.contains("Invalid arguments") || !message.contains("radix") {
                return Err(format!(
                    "`setExpression` with an unknown format option must fail at the envelope \
                     layer naming it: {message}"
                )
                .into());
            }
        }
        assert_canary_empty(&canary_path, "after unsupported-option failures")?;
        matrix.pass(
            "unsupported-option-fails-all-families",
            "variables|setVariable|evaluate|setExpression",
            "unknown",
            "Invalid arguments naming `radix` in variables/evaluate/setExpression (envelope-first); setVariable refuses at the #8354 floor before parsing; no hidden evaluation or mutation",
        );

        let malformed = dap.expect_failure(
            "variables",
            Some(json!({ "variablesReference": locals_ref, "format": { "hex": "true" } })),
        )?;
        if !malformed.contains("Invalid arguments") {
            return Err(
                format!("wrong-typed hex option must fail deserialization: {malformed}").into()
            );
        }
        matrix.pass(
            "malformed-format-fails",
            "variables",
            "hex:\"true\"",
            "strict typed wire struct",
        );
    }

    // --- setVariable: floored refusal, no mutation (#8354) ------------------
    let mutation_ref: i64;
    // Exact read-back baseline captured after the refused setVariable: the
    // refused setExpression leg below must reproduce this string byte-for-byte
    // (a `contains("255")` check would also pass a wrong mutation like `2555`).
    let original_read_back: String;
    {
        // #8354 floors setVariable: the request is refused with the
        // deterministic authority message before argument parsing or any
        // assignment work. The read-back proves the floor performed no hidden
        // mutation — the same client-value-binding guarantee the superseded
        // success leg pinned, now proven in the refused direction.
        let message = dap.expect_failure(
            "setVariable",
            Some(json!({
                "variablesReference": locals_ref,
                "name": "$pos",
                "value": "66"
            })),
        )?;
        if !message.contains("supportsSetVariable") {
            return Err(format!(
                "`setVariable` must receive the deterministic #8354 floor refusal: {message}"
            )
            .into());
        }
        let read_back = dap.expect_success(
            "evaluate",
            Some(json!({ "expression": "$pos", "frameId": frame_id })),
        )?;
        original_read_back =
            read_back.get("result").and_then(Value::as_str).unwrap_or("").to_string();
        // The fixture seeds `$pos = 255`; a refused setVariable must leave it
        // exactly there — no `66`, and no hex display text leaked into data.
        if !original_read_back.contains("255") || original_read_back.contains("66") {
            return Err(format!(
                "refused setVariable must not mutate: read-back `{original_read_back}` must \
                 still hold the fixture value 255 and never the refused value 66"
            )
            .into());
        }
        // The stale-handle row below needs an evaluate-result reference minted
        // before the resume; the read-back response carries exactly that.
        mutation_ref = read_back.get("variablesReference").and_then(Value::as_i64).unwrap_or(0);
        matrix.pass(
            "setVariable-floor-refuses-without-mutation",
            "setVariable",
            "hex",
            "#8354 deterministic refusal before parsing; read-back proves no hidden mutation of $pos",
        );
        assert_canary_empty(&canary_path, "after setVariable rows")?;
    }

    // --- setExpression: floored refusal, no mutation ------------------------
    {
        // #9568 floors setExpression: the request is refused with the
        // deterministic authority message and the admitted value is never
        // written. The read-back proves the floor performed no hidden
        // mutation — the same client-value-binding guarantee the superseded
        // success leg pinned, now proven in the refused direction.
        let message = dap.expect_failure(
            "setExpression",
            Some(json!({
                "expression": "$pos",
                "value": "77",
                "frameId": frame_id
            })),
        )?;
        if message != perl_dap::backend::capabilities::SET_EXPRESSION_UNSUPPORTED_MESSAGE {
            return Err(format!(
                "`setExpression` must receive the deterministic #9568 floor refusal: {message}"
            )
            .into());
        }
        let read_back = dap.expect_success(
            "evaluate",
            Some(json!({ "expression": "$pos", "frameId": frame_id })),
        )?;
        let read_back_result = read_back.get("result").and_then(Value::as_str).unwrap_or("");
        if read_back_result != original_read_back {
            return Err(format!(
                "refused setExpression must not mutate: read-back `{read_back_result}` \
                 must exactly equal the original read-back `{original_read_back}`"
            )
            .into());
        }
        matrix.pass(
            "setExpression-floor-refuses-without-mutation",
            "setExpression",
            "hex",
            "#9568 deterministic refusal; read-back proves no hidden mutation of $pos",
        );
        // No restore step: both mutation requests were refused at their
        // floors, so `$pos` never left its fixture value 255.
        assert_canary_empty(&canary_path, "after setExpression rows")?;
    }

    // --- cancellation stays floored and leaves the session coherent ---------
    {
        let cancel_message = dap.expect_failure("cancel", Some(json!({ "requestId": 1 })))?;
        if !cancel_message.contains("unsupported")
            || !cancel_message.contains("supportsCancelRequest")
        {
            return Err(format!(
                "cancel must be floored-rejected without touching shared state, got: {cancel_message}"
            )
            .into());
        }
        let after_cancel =
            dap.expect_success("variables", Some(json!({ "variablesReference": locals_ref })))?;
        if row_by_name(&after_cancel, "$pos")?.get("value")
            != Some(&Value::String("255".to_string()))
        {
            return Err("default variables after floored cancel must remain exact".into());
        }
        matrix.pass(
            "cancel-floored-then-default-exact",
            "variables",
            "default",
            "cancel explicitly unsupported; policy path and session state unaffected",
        );
    }

    // --- resume: stale handles cannot be revived ----------------------------
    dap.expect_success("continue", Some(json!({ "threadId": 1 })))?;
    dap.wait_event("stopped")?;
    {
        let stale =
            dap.expect_success("variables", Some(json!({ "variablesReference": mutation_ref })))?;
        let rows = stale
            .get("variables")
            .and_then(Value::as_array)
            .ok_or("stale response missing variables array")?;
        if !rows.is_empty() {
            return Err(format!(
                "pre-resume evaluate-result handle must serve honest empty after resume, got {} rows",
                rows.len()
            )
            .into());
        }
        matrix.pass(
            "stale-handle-honest-empty",
            "variables",
            "default",
            "pre-resume EvalResult ref empty after later stop",
        );
    }

    // --- later stop: fresh suspension, fresh values, default presentation ---
    {
        let (frame2_id, frame2_line) = stack_trace_until_line(&mut dap, stop2_line)?;
        let scopes2 = dap.expect_success("scopes", Some(json!({ "frameId": frame2_id })))?;
        let locals2 = scopes2
            .get("scopes")
            .and_then(Value::as_array)
            .and_then(|scopes| scopes.first())
            .and_then(|scope| scope.get("variablesReference"))
            .and_then(Value::as_i64)
            .ok_or("STOP2 scopes missing Locals variablesReference")?;

        let baseline2 =
            dap.expect_success("variables", Some(json!({ "variablesReference": locals2 })))?;
        let later_value =
            row_by_name(&baseline2, "$later")?.get("value").and_then(Value::as_str).unwrap_or("");
        if later_value != "4096" {
            return Err(
                format!("$later at STOP2 must render 4096 decimal, got `{later_value}`").into()
            );
        }
        let pos_value =
            row_by_name(&baseline2, "$pos")?.get("value").and_then(Value::as_str).unwrap_or("");
        if pos_value != "255" {
            return Err(format!("$pos at STOP2 must render 255 decimal, got `{pos_value}`").into());
        }
        matrix.pass(
            "later-stop-fresh-values",
            "variables",
            "default",
            "$later 4096 and restored $pos 255 exact at the new suspension",
        );
        // Session/frame identity: the second stop is a distinct suspension.
        if frame2_id == frame_id && frame2_line == frame_line {
            return Err("later stop must not reuse the first stop's frame identity".into());
        }
        assert_canary_empty(&canary_path, "after later-stop rows")?;
    }

    // --- teardown: terminal request, terminated event, process exit --------
    {
        let disconnect = dap.request("disconnect", Some(json!({})))?;
        let terminated = dap.wait_event("terminated");
        match disconnect {
            ResponseOutcome::Success(_) => {
                terminated?;
            }
            ResponseOutcome::Failure(message) => {
                return Err(format!("disconnect failed over stdio: {message}").into());
            }
        }
        dap.close_stdin_and_wait_exit()?;
        matrix.pass(
            "cleanup-terminated-and-exit",
            "lifecycle",
            "-",
            "disconnect -> terminated -> stdin close -> adapter exit",
        );
    }

    // --- final canary + fail-closed row count ------------------------------
    assert_canary_empty(&canary_path, "final")?;
    matrix.pass(
        "side-effect-canaries-empty",
        "all",
        "-",
        "tie FETCH/STORE and overload \"\", never executed across the whole session",
    );

    if matrix.rows.len() != EXPECTED_ROW_COUNT {
        return Err(format!(
            "fail-closed row count: recorded {} matrix rows, expected {EXPECTED_ROW_COUNT}; a cluster was skipped",
            matrix.rows.len()
        )
        .into());
    }

    if let Some(output) = receipt_output() {
        write_receipt_to(
            &output,
            &identity,
            &matrix,
            &dap.transcript,
            "disconnect_ok;terminated_event;adapter_exit",
        )?;
    }
    note_to_stderr(&format!(
        "value_format_stdio_proof_matrix: {} rows pass; transcript digest {}",
        matrix.rows.len(),
        dap.transcript.digest()
    ));
    Ok(())
}

// ---------------------------------------------------------------------------
// Receipt binding (no session required)
// ---------------------------------------------------------------------------

#[test]
fn receipt_binds_subject_identity_and_row_verdicts() -> ProofResult<()> {
    let temp = tempfile::tempdir()?;
    let output = temp.path().join("receipt.json");
    let binary = temp.path().join("perl-dap");
    fs::write(&binary, b"fake-binary-bytes")?;
    let fixture = temp.path().join("fixture.pl");
    fs::write(&fixture, b"fixture-bytes")?;

    let identity = SubjectIdentity {
        binary_path: binary.to_string_lossy().to_string(),
        binary_len: 16,
        binary_sha256: digest_bytes(b"fake-binary-bytes"),
        requested_perl_path: "C:\\perl\\bin\\wrapper.exe".to_string(),
        perl_path: "C:\\perl\\bin\\perl.exe".to_string(),
        perl_version: "v5.42.2".to_string(),
        perl_sha256: digest_bytes(b"perl"),
        fixture_path: fixture.to_string_lossy().to_string(),
        fixture_len: 13,
        fixture_sha256: digest_bytes(b"fixture-bytes"),
    };
    let mut matrix = Matrix::new();
    matrix.pass("sample-row", "variables", "hex", "sample note");
    let transcript = Transcript::new();

    write_receipt_to(&output, &identity, &matrix, &transcript, "disconnect_ok")?;

    let receipt: Value = serde_json::from_str(&fs::read_to_string(&output)?)?;
    if receipt.get("schema_version").and_then(Value::as_str) != Some(RECEIPT_SCHEMA) {
        return Err("receipt schema_version mismatch".into());
    }
    if receipt.pointer("/subject/binary/sha256")
        != Some(&Value::String(digest_bytes(b"fake-binary-bytes")))
    {
        return Err("receipt must bind the exact binary digest".into());
    }
    if receipt.pointer("/subject/perl/path")
        != Some(&Value::String("C:\\perl\\bin\\perl.exe".to_string()))
        || receipt.pointer("/subject/perl/requested_path")
            != Some(&Value::String("C:\\perl\\bin\\wrapper.exe".to_string()))
    {
        return Err("receipt must distinguish observed and requested Perl paths".into());
    }
    if receipt.pointer("/subject/capabilities/supportsValueFormattingOptions")
        != Some(&Value::Bool(false))
        || receipt.pointer("/subject/capabilities/supportsCancelRequest")
            != Some(&Value::Bool(false))
    {
        return Err("receipt must bind the #9581 capability-floor identity".into());
    }
    let rows = receipt.get("rows").and_then(Value::as_array).ok_or("receipt missing rows")?;
    if rows.len() != 1 || rows[0].get("row_id").and_then(Value::as_str) != Some("sample-row") {
        return Err("receipt must carry typed row verdicts".into());
    }
    if receipt.get("transcript").and_then(|t| t.get("semantic_sha256")).is_none() {
        return Err("receipt must bind the semantic transcript digest".into());
    }
    Ok(())
}

#[test]
fn fixture_stop_markers_resolve_to_distinct_executable_lines() -> ProofResult<()> {
    // The proof's breakpoints are resolved from the fixture itself; a stale or
    // edited fixture that loses either marker must fail here, not mid-session.
    let stop1 = fixture_line("$VF::stop1 = 1;")?;
    let stop2 = fixture_line("$VF::stop2 = 1;")?;
    if stop1 <= 0 || stop2 <= stop1 {
        return Err(
            format!("fixture stop markers must be distinct and ordered: {stop1}, {stop2}").into()
        );
    }
    Ok(())
}
