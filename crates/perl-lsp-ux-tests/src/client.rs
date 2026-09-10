//! LSP client for UX scenario tests.
//!
//! Spawns the real `perl-lsp` binary via stdio and communicates using the
//! JSON-RPC 2.0 / LSP Content-Length framing protocol.  All server-initiated
//! messages (`window/showMessage`, `window/logMessage`, diagnostic
//! notifications, etc.) are captured in an event queue so scenarios can
//! assert on user-visible messages after the fact.
// Test harness client — eprintln! echoes spawned server stderr for debugging.
#![allow(clippy::print_stderr)]

use crate::observation::{Inbox, StreamEnd, WaitEnd};
use crate::{FakeWorkspace, ScenarioConfig};
use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

static NEXT_ID: AtomicU64 = AtomicU64::new(100);

/// Outer bound on an orderly server exit after `shutdown`/`exit` are sent.
/// Reaching it means the server did not close its stream and is force-killed.
const SHUTDOWN_GRACE: Duration = Duration::from_millis(500);

fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// A server-initiated event captured during a scenario.
#[derive(Debug, Clone)]
pub enum LspEvent {
    /// `window/showMessage` — user-visible modal notification.
    WindowMessage {
        /// LSP MessageType (1=Error, 2=Warning, 3=Info, 4=Log).
        message_type: u32,
        message: String,
    },
    /// `window/logMessage` — IDE output panel message.
    LogMessage { message_type: u32, message: String },
    /// `textDocument/publishDiagnostics` — diagnostic update.
    Diagnostics { uri: String, version: Option<i64>, diagnostics: Vec<Value> },
    /// Any other server-initiated notification.
    Other { method: String, params: Value },
}

/// A source of decoded server-initiated events that a harness wait can block on.
///
/// This is the seam that keeps higher-level waiters (diagnostics, index
/// readiness, active-document readiness) event-driven without each of them
/// re-implementing a wait. Taking this rather than a `impl FnMut() -> Vec<LspEvent>`
/// poll provider is what removes their timers: a provider can only be sampled,
/// whereas a source can be *waited on*.
pub trait EventSource {
    /// Block until `select` matches over every buffered decoded event, the
    /// stream ends, or `timeout` expires.
    ///
    /// # Errors
    ///
    /// Returns the typed [`WaitEnd`] describing which bound ended the wait.
    fn wait_for_events<T>(
        &self,
        timeout: Duration,
        select: impl FnMut(&[LspEvent]) -> Option<T>,
    ) -> Result<T, WaitEnd>;
}

impl EventSource for Inbox {
    fn wait_for_events<T>(
        &self,
        timeout: Duration,
        mut select: impl FnMut(&[LspEvent]) -> Option<T>,
    ) -> Result<T, WaitEnd> {
        self.wait_for_raw_events(timeout, |events| {
            let decoded: Vec<LspEvent> = events.iter().cloned().map(decode_event).collect();
            select(&decoded)
        })
    }
}

impl EventSource for UxClient {
    fn wait_for_events<T>(
        &self,
        timeout: Duration,
        select: impl FnMut(&[LspEvent]) -> Option<T>,
    ) -> Result<T, WaitEnd> {
        self.wait_decoded(timeout, select)
    }
}

/// Guarantees the stdout reader records *some* stream end when it stops.
///
/// The reader normally names the end itself. If it instead unwinds — a panic
/// anywhere in the read path — this still closes the inbox, so waiters get a
/// typed failure rather than an indefinite silence that only ever expires as a
/// deadline. `Inbox::close` keeps the first end recorded, so the honest reason
/// always wins over this fallback.
struct ReaderExit {
    inbox: Inbox,
}

impl ReaderExit {
    const fn new(inbox: Inbox) -> Self {
        Self { inbox }
    }

    /// Record the reader's own reason for stopping.
    fn record(&mut self, end: StreamEnd) {
        self.inbox.close(end);
    }
}

impl Drop for ReaderExit {
    fn drop(&mut self) {
        // No-ops when `record` already ran; only an unwind reaches this first.
        self.inbox.close(StreamEnd::TransportFailure {
            detail: "the stdout reader thread stopped without reporting a reason".to_string(),
        });
    }
}

/// A lightweight LSP client that speaks directly to a spawned perl-lsp process.
pub struct UxClient {
    child: Mutex<Child>,
    stdin: Mutex<ChildStdin>,
    initialize_result: Value,
    /// The single observation substrate: buffered events, buffered responses,
    /// and the typed reason the server's output stream ended. Every wait in the
    /// harness blocks on this rather than sleeping on a wall-clock timer.
    inbox: Inbox,
    /// Stderr lines captured from the server process.
    stderr_lines: Arc<Mutex<Vec<String>>>,
    _stdout_thread: std::thread::JoinHandle<()>,
    _stderr_thread: std::thread::JoinHandle<()>,
}

impl UxClient {
    /// Spawn the perl-lsp binary and perform the LSP handshake
    /// (`initialize` + `initialized`).
    pub fn spawn(
        binary_path: &str,
        workspace: &FakeWorkspace,
        config: &ScenarioConfig,
    ) -> Result<Self> {
        let mut cmd = build_command(binary_path, config)?;

        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("Failed to spawn perl-lsp from {:?}", binary_path))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("perl-lsp stdin not available after spawn"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("perl-lsp stdout not available after spawn"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("perl-lsp stderr not available after spawn"))?;

        let inbox = Inbox::new();
        let stderr_lines: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

        // ── stdout reader thread ──────────────────────────────────────────────
        // Publishes into the inbox and, on exit, records *why* the stream ended
        // so a waiter can tell an orderly shutdown from a broken transport
        // instead of both surfacing as an unexplained timeout.
        let reader_inbox = inbox.clone();
        let _stdout_thread = std::thread::Builder::new()
            .name("ux-lsp-stdout".into())
            .spawn(move || {
                // Records the stream end even if this thread unwinds, so a
                // waiter can never be left unable to distinguish a dead reader
                // from a merely silent server.
                let mut exit = ReaderExit::new(reader_inbox.clone());
                let mut reader = BufReader::new(stdout);
                loop {
                    match read_one_frame(&mut reader) {
                        FrameRead::Message(msg) => {
                            let has_id = msg.get("id").is_some() && !msg["id"].is_null();
                            let is_response = has_id
                                && (msg.get("result").is_some() || msg.get("error").is_some());
                            if is_response {
                                reader_inbox.push_response(msg);
                            } else {
                                reader_inbox.push_event(msg);
                            }
                        }
                        FrameRead::EndOfStream => return exit.record(StreamEnd::ServerClosed),
                        FrameRead::Failed(detail) => {
                            return exit.record(StreamEnd::TransportFailure { detail });
                        }
                    }
                }
            })
            .context("Failed to spawn stdout reader thread")?;

        // ── stderr drain thread ───────────────────────────────────────────────
        let echo = config.echo_stderr;
        let stderr_clone = stderr_lines.clone();
        let _stderr_thread = std::thread::Builder::new()
            .name("ux-lsp-stderr".into())
            .spawn(move || {
                let reader = BufReader::new(stderr);
                for l in reader.lines().map_while(Result::ok) {
                    if let Ok(mut guard) = stderr_clone.lock() {
                        guard.push(l.clone());
                    }
                    if echo {
                        eprintln!("[perl-lsp stderr] {}", l);
                    }
                }
            })
            .context("Failed to spawn stderr drain thread")?;

        // No startup sleep: `initialize` is written into the child's stdin
        // pipe, which buffers it whether or not the server has reached its read
        // loop yet. Readiness is then established by the server's own
        // `initialize` response, which the handshake waits for — an observable
        // signal rather than a guess about process startup latency.
        let mut client = Self {
            child: Mutex::new(child),
            stdin: Mutex::new(stdin),
            initialize_result: Value::Null,
            inbox,
            stderr_lines,
            _stdout_thread,
            _stderr_thread,
        };

        // ── LSP handshake ─────────────────────────────────────────────────────
        client.initialize_result = client.handshake(workspace, config, config.timeout)?;

        Ok(client)
    }

    fn handshake(
        &self,
        workspace: &FakeWorkspace,
        config: &ScenarioConfig,
        timeout: Duration,
    ) -> Result<Value> {
        let workspace_folders = config
            .workspace_folders
            .iter()
            .map(|(relative_path, name)| {
                Ok(json!({
                    "uri": workspace.dir_uri(relative_path)?,
                    "name": name,
                }))
            })
            .collect::<Result<Vec<Value>>>()?;

        let root_uri = if workspace_folders.is_empty() {
            Value::String(workspace.root_uri.clone())
        } else {
            Value::Null
        };

        let mut params = json!({
            "processId": null,
            "rootUri": root_uri,
            "capabilities": {
                "general": {
                    "positionEncodings": ["utf-16"]
                },
                "textDocument": {
                    "hover": {
                        "contentFormat": ["markdown", "plaintext"]
                    },
                    "completion": {
                        "completionItem": {
                            "snippetSupport": true
                        }
                    },
                    "formatting": {},
                    "definition": {},
                    "publishDiagnostics": {
                        "relatedInformation": true
                    }
                },
                "workspace": {
                    "workspaceFolders": true
                },
                "window": {
                    "showMessage": {}
                }
            }
        });
        if !workspace_folders.is_empty() {
            params["workspaceFolders"] = Value::Array(workspace_folders);
        }
        if !config.initialization_options.is_null() {
            params["initializationOptions"] = config.initialization_options.clone();
        }

        merge_json(&mut params["capabilities"], &config.client_capability_overrides);

        let init_resp = self.request("initialize", params, timeout)?;

        if let Some(err) = init_resp.get("error") {
            return Err(anyhow!("LSP initialize returned error: {}", err));
        }

        self.notify("initialized", json!({}))?;

        Ok(init_resp)
    }

    /// Clone the initialize response captured during handshake.
    pub fn initialize_result(&self) -> Value {
        self.initialize_result.clone()
    }

    /// Send a JSON-RPC request and wait for the matching response.
    pub fn request(&self, method: &str, params: Value, timeout: Duration) -> Result<Value> {
        let id = next_id();
        let msg = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });
        self.send_raw(&msg)?;
        self.wait_for_response(id, timeout)
    }

    /// Send a JSON-RPC notification (no response expected).
    pub fn notify(&self, method: &str, params: Value) -> Result<()> {
        let msg = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        });
        self.send_raw(&msg)
    }

    /// Send `textDocument/didOpen` using the provided language identifier.
    pub fn did_open_with_language_id(
        &self,
        uri: &str,
        text: &str,
        language_id: &str,
    ) -> Result<()> {
        self.notify(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": language_id,
                    "version": 1,
                    "text": text
                }
            }),
        )
    }

    /// Send `textDocument/didChange` with a full-document replacement.
    pub fn did_change_full(&self, uri: &str, version: i32, text: &str) -> Result<()> {
        self.notify(
            "textDocument/didChange",
            json!({
                "textDocument": {
                    "uri": uri,
                    "version": version
                },
                "contentChanges": [
                    {
                        "text": text
                    }
                ]
            }),
        )
    }

    /// Send `textDocument/didOpen` using Perl as the language identifier.
    pub fn did_open(&self, uri: &str, text: &str) -> Result<()> {
        self.did_open_with_language_id(uri, text, "perl")
    }

    /// Send `textDocument/didChange` with explicit version and content changes.
    pub fn did_change(&self, uri: &str, version: i32, content_changes: Vec<Value>) -> Result<()> {
        self.notify(
            "textDocument/didChange",
            json!({
                "textDocument": {
                    "uri": uri,
                    "version": version
                },
                "contentChanges": content_changes
            }),
        )
    }

    /// Drain all buffered server-initiated events and decode them.
    ///
    /// After this call the internal queue is empty.  Use `peek_events` if you
    /// need to inspect events without consuming them.
    pub fn drain_events(&self) -> Vec<LspEvent> {
        self.inbox.drain_events().into_iter().map(decode_event).collect()
    }

    /// Clone and decode all buffered events **without** removing them from the
    /// queue.  Safe to call before or after `drain_events` / `collect_notifications`.
    pub fn peek_events(&self) -> Vec<LspEvent> {
        self.inbox.snapshot().events().iter().cloned().map(decode_event).collect()
    }

    /// Clone raw server-initiated messages without removing them from the queue.
    ///
    /// This preserves server request IDs and registration payloads for protocol
    /// smoke checks that need to assert exact JSON-RPC shapes.
    pub fn peek_raw_events(&self) -> Vec<Value> {
        self.inbox.snapshot().events().to_vec()
    }

    /// Clone all stderr lines captured from the server process.
    pub fn peek_stderr_lines(&self) -> Vec<String> {
        self.stderr_lines.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Wait up to `timeout` for any `window/showMessage` or `window/logMessage`
    /// containing `needle`.
    pub fn wait_for_message(&self, needle: &str, timeout: Duration) -> bool {
        self.wait_for_raw_events(timeout, |events| {
            events
                .iter()
                .any(|msg| {
                    let method = msg["method"].as_str().unwrap_or("");
                    (method == "window/showMessage" || method == "window/logMessage")
                        && msg["params"]["message"].as_str().unwrap_or("").contains(needle)
                })
                .then_some(())
        })
        .is_ok()
    }

    /// Block until `select` matches over every buffered raw server-initiated
    /// message, the server's stream ends, or `timeout` expires.
    ///
    /// The predicate receives the whole buffer oldest-first and runs with no
    /// harness lock held. Events are never consumed by a wait, so a later
    /// waiter still observes everything an earlier one matched on.
    ///
    /// # Errors
    ///
    /// Returns the typed [`WaitEnd`]: an orderly server close and a transport
    /// failure are reported as such rather than as an expired deadline.
    pub fn wait_for_raw_events<T>(
        &self,
        timeout: Duration,
        select: impl FnMut(&[Value]) -> Option<T>,
    ) -> Result<T, WaitEnd> {
        self.inbox.wait_for_raw_events(timeout, select)
    }

    /// Block until `select` matches over every buffered *decoded* event.
    ///
    /// This is the event-driven replacement for polling [`Self::peek_events`]
    /// on a timer; see [`Self::wait_for_raw_events`] for the outcome contract.
    ///
    /// # Errors
    ///
    /// Returns the typed [`WaitEnd`].
    pub fn wait_for_events<T>(
        &self,
        timeout: Duration,
        select: impl FnMut(&[LspEvent]) -> Option<T>,
    ) -> Result<T, WaitEnd> {
        self.wait_decoded(timeout, select)
    }

    /// The single decoded-event wait body.
    ///
    /// Both the inherent [`Self::wait_for_events`] and the [`EventSource`]
    /// implementation delegate here under a distinct name, so neither can
    /// accidentally resolve to the other and recurse.
    fn wait_decoded<T>(
        &self,
        timeout: Duration,
        mut select: impl FnMut(&[LspEvent]) -> Option<T>,
    ) -> Result<T, WaitEnd> {
        self.wait_for_raw_events(timeout, move |raw| {
            let decoded: Vec<LspEvent> = raw.iter().cloned().map(decode_event).collect();
            select(&decoded)
        })
    }

    /// The typed reason the server's output stream ended, if it has.
    #[must_use]
    pub fn stream_end(&self) -> Option<StreamEnd> {
        self.inbox.stream_end()
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    fn send_raw(&self, msg: &Value) -> Result<()> {
        let body = msg.to_string();
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        let mut stdin = self.stdin.lock().unwrap_or_else(|e| e.into_inner());
        stdin.write_all(header.as_bytes()).context("Failed to write LSP header to stdin")?;
        stdin.write_all(body.as_bytes()).context("Failed to write LSP body to stdin")?;
        stdin.flush().context("Failed to flush LSP stdin")?;
        Ok(())
    }

    /// Explain a wait outcome, folding in the child's real exit status.
    fn explain(&self, end: &WaitEnd) -> String {
        let exit = self.child.lock().ok().and_then(|mut child| child.try_wait().ok().flatten());
        describe_end_with_exit(end, exit)
    }

    fn wait_for_response(&self, id: u64, timeout: Duration) -> Result<Value> {
        // Exact JSON-RPC id identity: numeric `1` and string `"1"` are distinct
        // subjects and must never complete each other's wait.
        let wanted = Value::from(id);
        let deadline = Instant::now() + timeout;
        loop {
            // An unconsumable match costs a retry, never a fresh full budget.
            let remaining = deadline.saturating_duration_since(Instant::now());
            let observation = self
                .inbox
                .wait_for_responses(remaining, |responses| {
                    responses
                        .iter()
                        .find(|(_, value)| value["id"] == wanted)
                        .map(|(observation, _)| *observation)
                })
                .map_err(|end| anyhow!("No LSP response to id={id}: {}", self.explain(&end)))?;

            // Consume by observation identity, not by id, so unrelated traffic
            // can never hand this caller a different message. A `None` here
            // means another waiter took it first — re-evaluate rather than
            // assume a match.
            if let Some(message) = self.inbox.take_response(observation) {
                return Ok(message);
            }
        }
    }
}

fn merge_json(target: &mut Value, overlay: &Value) {
    let (Some(target_obj), Some(overlay_obj)) = (target.as_object_mut(), overlay.as_object())
    else {
        if !overlay.is_null() {
            *target = overlay.clone();
        }
        return;
    };

    for (key, value) in overlay_obj {
        match target_obj.get_mut(key) {
            Some(existing) => merge_json(existing, value),
            None => {
                target_obj.insert(key.clone(), value.clone());
            }
        }
    }
}

impl Drop for UxClient {
    fn drop(&mut self) {
        // Best-effort graceful shutdown.
        let shutdown = r#"{"jsonrpc":"2.0","id":999998,"method":"shutdown","params":{}}"#;
        let exit = r#"{"jsonrpc":"2.0","method":"exit"}"#;
        if let Ok(mut stdin) = self.stdin.lock() {
            for body in [shutdown, exit] {
                let hdr = format!("Content-Length: {}\r\n\r\n", body.len());
                let _ = stdin.write_all(hdr.as_bytes());
                let _ = stdin.write_all(body.as_bytes());
                let _ = stdin.flush();
            }
        }
        // Give the server its grace period by waiting for its own end-of-stream
        // rather than polling `try_wait` on a timer: the reader records the
        // stream end the moment it happens, so an orderly exit is observed
        // immediately instead of on the next poll tick. The predicate never
        // matches, so this can only end at the stream end or the bound.
        let _ = self.inbox.wait_for(SHUTDOWN_GRACE, |_| None::<()>);

        // End of stream is NOT proof the child exited. A server can close its
        // stdout, or corrupt its framing, and keep running — and after a
        // `TransportFailure` the reader has stopped draining stdout entirely.
        // So the exit decision stays bounded and always terminates.
        if let Ok(mut child) = self.child.lock() {
            reap_or_kill(&mut child);
        }
    }
}

/// Finish with the child process without ever blocking indefinitely.
///
/// `wait()` is only called on a process already known to have exited, or after
/// `kill()` — which sends an uncatchable signal, so the subsequent reap
/// returns promptly. A server that closed its output but is still running is
/// killed rather than waited on forever.
/// Describe why a wait ended, consulting the child's actual exit status.
///
/// `StreamEnd::ServerClosed` is an honest statement about the *stream* — EOF at
/// a message boundary — but on its own it reads as an orderly shutdown even
/// when the server crashed or exited nonzero without emitting a partial frame.
/// Reporting that as "orderly" is exactly the kind of misleading outcome this
/// substrate exists to prevent, so the process status is folded in here.
fn describe_end_with_exit(end: &WaitEnd, exit: Option<ExitStatus>) -> String {
    let WaitEnd::Ended(StreamEnd::ServerClosed) = end else {
        // A transport failure and a plain deadline already say what happened.
        return end.describe();
    };
    match exit {
        Some(status) if !status.success() => format!(
            "the server's output ended at a message boundary and the process exited \
             unsuccessfully ({status}) — this is a server failure, not an orderly shutdown"
        ),
        Some(status) => {
            format!("server closed its output stream and exited successfully ({status})")
        }
        // Still running, or the status could not be read: report only what is known.
        None => format!("{} (the process had not exited when this was reported)", end.describe()),
    }
}

fn reap_or_kill(child: &mut Child) {
    match child.try_wait() {
        // Already exited: just collect it.
        Ok(Some(_)) => {}
        // Still running, or its status could not be determined — force it.
        Ok(None) | Err(_) => {
            if child.kill().is_ok() {
                // The signal landed and is uncatchable, so this reap returns.
                let _ = child.wait();
                return;
            }
            // `kill` failed. For a child we spawned this essentially only
            // happens when it has already exited and been reaped, which
            // `try_wait` confirms cheaply. Never fall through to a blocking
            // `wait` here: leaking a process is recoverable, hanging every
            // remaining test is not.
            let _ = child.try_wait();
        }
    }
}

// ── Message framing ───────────────────────────────────────────────────────────

/// The outcome of reading one LSP frame.
///
/// The split between [`FrameRead::EndOfStream`] and [`FrameRead::Failed`] is
/// what lets a waiter distinguish "the server shut down as expected" from "the
/// transport broke". Collapsing them would make every abnormal exit look like
/// an ordinary timeout.
#[derive(Debug)]
enum FrameRead {
    /// A complete, well-formed message.
    Message(Value),
    /// Clean end of stream *at a message boundary* — the orderly shutdown path.
    EndOfStream,
    /// Malformed framing, a truncated frame, unparsable JSON, or an I/O error.
    Failed(String),
}

/// Largest LSP body this harness will buffer for one frame.
///
/// `Content-Length` arrives from the child under test, so it is untrusted:
/// without a bound, a live server that declares gigabytes and keeps its
/// stream open would grow the reader's buffer without limit. The bound is a
/// fail-closed ceiling, not a protocol claim about legitimate payload sizes:
/// 64 MiB exceeds any plausible single LSP frame (diagnostics batches and
/// symbol payloads included) by orders of magnitude while keeping a corrupt
/// or malicious declaration a prompt, bounded rejection.
const MAX_LSP_BODY_BYTES: usize = 64 * 1024 * 1024;

fn read_one_frame(reader: &mut impl BufRead) -> FrameRead {
    // Parse LSP Content-Length headers.
    let mut content_length: Option<usize> = None;
    let mut started_frame = false;
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            // EOF before any header byte is an orderly end of stream; EOF part
            // way through a header block truncated a frame that had begun.
            Ok(0) if started_frame => {
                return FrameRead::Failed(
                    "stream ended part way through an LSP message header block".to_string(),
                );
            }
            Ok(0) => return FrameRead::EndOfStream,
            Ok(_) => {}
            Err(error) => {
                return FrameRead::Failed(format!("I/O error reading LSP headers: {error}"));
            }
        }
        started_frame = true;
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some(rest) = trimmed.to_ascii_lowercase().strip_prefix("content-length") {
            let rest = rest.trim_start_matches(':').trim();
            content_length = rest.parse::<usize>().ok();
        }
    }
    let Some(len) = content_length else {
        return FrameRead::Failed(
            "LSP message header block carried no usable Content-Length".to_string(),
        );
    };
    // `Content-Length` is untrusted input from the child: reject an absurd
    // declaration before reading a single body byte. Without this bound a
    // live server that keeps its stream open would grow `body` without limit
    // (and block the reader until EOF) — the 64 MiB ceiling keeps that
    // fail-closed and prompt without constraining legitimate frames.
    if len > MAX_LSP_BODY_BYTES {
        return FrameRead::Failed(format!(
            "LSP message declared {len} body bytes, above the {MAX_LSP_BODY_BYTES}-byte bound"
        ));
    }
    // Read the body incrementally rather than pre-allocating `len` bytes.
    // `Content-Length` is untrusted input: a corrupted header near `usize::MAX`
    // would make `vec![0u8; len]` panic on capacity overflow, and a merely huge
    // one would abort the process on allocation failure. Either kills this
    // reader thread without recording a stream end, which would leave every
    // waiter unable to tell a dead reader from a silent server — exactly the
    // ambiguity this module exists to remove. `take` caps the read at `len`
    // and stops at EOF, so a bogus length becomes a truncated frame.
    let mut body = Vec::new();
    if let Err(error) = reader.take(len as u64).read_to_end(&mut body) {
        return FrameRead::Failed(format!(
            "stream ended or failed while reading a {len}-byte LSP body: {error}"
        ));
    }
    if body.len() != len {
        return FrameRead::Failed(format!(
            "LSP message declared {len} body bytes but the stream ended after {}",
            body.len()
        ));
    }
    match serde_json::from_slice(&body) {
        Ok(value) => FrameRead::Message(value),
        Err(error) => FrameRead::Failed(format!("LSP message body was not valid JSON: {error}")),
    }
}

// ── Event decoding ────────────────────────────────────────────────────────────

fn decode_event(v: Value) -> LspEvent {
    let method = v["method"].as_str().unwrap_or("").to_string();
    match method.as_str() {
        "window/showMessage" => {
            let message_type = v["params"]["type"].as_u64().unwrap_or(0) as u32;
            let message = v["params"]["message"].as_str().unwrap_or("").to_string();
            LspEvent::WindowMessage { message_type, message }
        }
        "window/logMessage" => {
            let message_type = v["params"]["type"].as_u64().unwrap_or(0) as u32;
            let message = v["params"]["message"].as_str().unwrap_or("").to_string();
            LspEvent::LogMessage { message_type, message }
        }
        "textDocument/publishDiagnostics" => {
            let uri = v["params"]["uri"].as_str().unwrap_or("").to_string();
            let version = v["params"]["version"].as_i64();
            let diagnostics = v["params"]["diagnostics"].as_array().cloned().unwrap_or_default();
            LspEvent::Diagnostics { uri, version, diagnostics }
        }
        _ => LspEvent::Other { method, params: v["params"].clone() },
    }
}

// ── Command construction ──────────────────────────────────────────────────────

fn build_command(binary_path: &str, config: &ScenarioConfig) -> Result<Command> {
    let mut cmd = Command::new(binary_path);
    cmd.arg("--stdio");

    // Apply restricted PATH if requested.
    if let Some(ref dirs) = config.path_restriction {
        use crate::env::RestrictedPath;
        let restricted = RestrictedPath::only(dirs.clone());
        cmd.env("PATH", restricted.build_path());
    }

    // Apply extra env vars / unsets.
    for (key, value) in &config.extra_env {
        match value {
            Some(v) => {
                cmd.env(key, v);
            }
            None => {
                cmd.env_remove(key);
            }
        }
    }

    Ok(cmd)
}

#[cfg(test)]
mod framing_tests {
    use super::{FrameRead, ReaderExit, read_one_frame};
    use crate::observation::{Inbox, StreamEnd};
    use std::io::BufReader;

    fn read(input: &str) -> FrameRead {
        read_one_frame(&mut BufReader::new(input.as_bytes()))
    }

    /// A corrupted `Content-Length` must be reported, never allocated.
    ///
    /// `vec![0u8; len]` on this header panics with a capacity overflow (or
    /// aborts the process on a merely huge value), killing the reader thread
    /// without recording any stream end — which would leave every waiter
    /// unable to tell a dead reader from a silent server.
    #[test]
    fn an_absurd_content_length_is_a_transport_failure_not_an_allocation() -> anyhow::Result<()> {
        let outcome = read("Content-Length: 18446744073709551615\r\n\r\n{}");

        let FrameRead::Failed(detail) = outcome else {
            anyhow::bail!("an undeliverable body length must fail the frame, got {outcome:?}");
        };
        anyhow::ensure!(
            detail.contains("body bytes"),
            "the failure must name the truncated body: {detail}"
        );
        Ok(())
    }

    /// The same guarantee for a large-but-plausible length: the frame is
    /// truncated, so it is a transport failure, not an orderly close.
    #[test]
    fn a_body_shorter_than_its_declared_length_is_a_transport_failure() -> anyhow::Result<()> {
        let outcome = read("Content-Length: 4096\r\n\r\n{\"a\":1}");

        anyhow::ensure!(
            matches!(outcome, FrameRead::Failed(_)),
            "a short body must not be reported as an orderly close, got {outcome:?}"
        );
        Ok(())
    }

    #[test]
    fn a_well_formed_frame_still_parses() -> anyhow::Result<()> {
        let body = r#"{"jsonrpc":"2.0","id":1,"result":{}}"#;
        let outcome = read(&format!("Content-Length: {}\r\n\r\n{body}", body.len()));

        let FrameRead::Message(value) = outcome else {
            anyhow::bail!("a well-formed frame must parse, got {outcome:?}");
        };
        anyhow::ensure!(
            value.get("id") == Some(&serde_json::json!(1)),
            "well-formed frame must retain numeric id 1, got {value:?}"
        );
        Ok(())
    }

    #[test]
    fn a_clean_end_of_stream_is_not_a_failure() -> anyhow::Result<()> {
        anyhow::ensure!(
            matches!(read(""), FrameRead::EndOfStream),
            "empty input must be an orderly end of stream"
        );
        Ok(())
    }

    /// If the reader stops without naming a reason, waiters must still get a
    /// typed stream end rather than silence that only expires as a deadline.
    #[test]
    fn an_unreported_reader_exit_still_closes_the_inbox() -> anyhow::Result<()> {
        let inbox = Inbox::new();
        drop(ReaderExit::new(inbox.clone()));

        anyhow::ensure!(
            matches!(inbox.stream_end(), Some(StreamEnd::TransportFailure { .. })),
            "an unexplained reader exit must still record a stream end"
        );
        Ok(())
    }

    /// The reader's own reason outranks the fallback.
    #[test]
    fn a_reported_reason_wins_over_the_fallback() -> anyhow::Result<()> {
        let inbox = Inbox::new();
        let mut exit = ReaderExit::new(inbox.clone());
        exit.record(StreamEnd::ServerClosed);
        drop(exit);

        anyhow::ensure!(
            matches!(inbox.stream_end(), Some(StreamEnd::ServerClosed)),
            "the honest reason must survive the drop fallback"
        );
        Ok(())
    }

    /// A stream held open by a writer that never delivers the declared body.
    ///
    /// `recv` blocks while the writer lives, so without the body-size bound
    /// this read would block until EOF (forever, here). The bound must fail
    /// the frame before a single body byte is awaited.
    struct LiveWriter {
        rx: std::sync::mpsc::Receiver<Vec<u8>>,
        buf: Vec<u8>,
        pos: usize,
    }

    impl std::io::Read for LiveWriter {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            while self.pos >= self.buf.len() {
                match self.rx.recv() {
                    Ok(chunk) => self.buf.extend_from_slice(&chunk),
                    Err(_) => return Ok(0),
                }
            }
            let end = (self.pos + out.len()).min(self.buf.len());
            out[..end - self.pos].copy_from_slice(&self.buf[self.pos..end]);
            let advanced = end - self.pos;
            self.pos = end;
            Ok(advanced)
        }
    }

    #[test]
    fn a_huge_declared_length_fails_without_waiting_for_the_body() -> anyhow::Result<()> {
        use std::time::{Duration, Instant};

        let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
        tx.send(format!("Content-Length: {}\r\n\r\n", 1024 * 1024 * 1024).into_bytes())
            .map_err(|_| anyhow::anyhow!("header send failed"))?;
        // Hold the stream open: a live server that never sends the body. The
        // parked thread dies with the test process; nothing here joins it.
        std::thread::spawn(move || {
            let _held = tx;
            std::thread::park();
        });

        let start = Instant::now();
        let outcome =
            read_one_frame(&mut BufReader::new(LiveWriter { rx, buf: Vec::new(), pos: 0 }));
        let elapsed = start.elapsed();

        let FrameRead::Failed(detail) = outcome else {
            anyhow::bail!("an over-bound declaration must fail the frame, got {outcome:?}");
        };
        anyhow::ensure!(detail.contains("above the"), "the failure must name the bound: {detail}");
        anyhow::ensure!(
            elapsed < Duration::from_secs(10),
            "the bound must fail before any body wait, took {elapsed:?}"
        );
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod shutdown_tests {
    // Unix-only by construction: these drive a POSIX shell. On other platforms
    // the module is absent, so the suite reports them as not run rather than
    // silently green.
    use super::reap_or_kill;
    use crate::observation::{StreamEnd, WaitEnd};
    use std::process::{Child, Command, ExitStatus, Stdio};
    use std::time::{Duration, Instant};

    /// Spawn a POSIX shell running `script`.
    ///
    /// This module is `cfg(unix)`, so `/bin/sh` is present by contract and a
    /// spawn failure is a real failure. Earlier revisions returned quietly when
    /// the spawn failed, which made these controls *pass* on a platform where
    /// they had proved nothing — the precise dishonesty this crate's harness
    /// exists to remove.
    fn spawn_shell(script: &str) -> anyhow::Result<Child> {
        use anyhow::Context;
        Command::new("/bin/sh")
            .args(["-c", script])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("/bin/sh must be spawnable on a unix host")
    }

    fn exit_status_of(script: &str) -> anyhow::Result<ExitStatus> {
        use anyhow::Context;
        spawn_shell(script)?.wait().context("the fixture shell must be reapable")
    }

    /// A crashed server must not be described as an orderly shutdown.
    ///
    /// EOF at a message boundary is an honest statement about the *stream*, but
    /// on its own it reads as a clean exit even when the process died. The
    /// reported reason folds in the real exit status.
    #[test]
    fn a_nonzero_exit_is_not_described_as_an_orderly_shutdown() -> anyhow::Result<()> {
        let status = exit_status_of("exit 3")?;
        anyhow::ensure!(!status.success(), "fixture must exit nonzero");

        let described =
            super::describe_end_with_exit(&WaitEnd::Ended(StreamEnd::ServerClosed), Some(status));

        anyhow::ensure!(
            described.contains("server failure"),
            "a nonzero exit must be reported as a failure: {described}"
        );
        anyhow::ensure!(
            !described.contains("orderly shutdown)"),
            "it must not read as an orderly shutdown: {described}"
        );
        Ok(())
    }

    #[test]
    fn a_successful_exit_is_still_described_as_an_orderly_close() -> anyhow::Result<()> {
        let status = exit_status_of("exit 0")?;

        let described =
            super::describe_end_with_exit(&WaitEnd::Ended(StreamEnd::ServerClosed), Some(status));

        anyhow::ensure!(
            described.contains("exited successfully"),
            "a clean exit must stay orderly: {described}"
        );
        Ok(())
    }

    /// A transport failure already names itself; the exit status must not
    /// overwrite that more specific reason.
    #[test]
    fn a_transport_failure_keeps_its_own_reason() -> anyhow::Result<()> {
        let described = super::describe_end_with_exit(
            &WaitEnd::Ended(StreamEnd::TransportFailure { detail: "bad header".to_string() }),
            None,
        );
        anyhow::ensure!(
            described.contains("bad header"),
            "the framing detail must survive: {described}"
        );
        Ok(())
    }

    /// Regression control for the hazard that end-of-stream is not process
    /// exit: a server may close (or corrupt) its stdout and keep running.
    ///
    /// Waiting unconditionally on such a child hangs the whole test run, so
    /// this asserts the bounded path — the child is force-killed and reaped
    /// well inside its own sleep.
    #[test]
    fn a_child_that_closes_stdout_but_keeps_running_is_not_waited_on_forever() -> anyhow::Result<()>
    {
        use std::io::Read;
        // A shell builtin waits on the still-open stdin pipe; no sleeping
        // descendant survives a failed assertion or a broken cleanup helper.
        let mut child = Command::new("/bin/sh")
            .args(["-c", "exec 1>&-; read ignored"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let result = (|| -> anyhow::Result<()> {
            let mut stdout =
                child.stdout.take().ok_or_else(|| anyhow::anyhow!("missing child stdout"))?;
            let (sender, receiver) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let mut bytes = Vec::new();
                let _ = sender.send(stdout.read_to_end(&mut bytes));
            });
            let bytes = receiver.recv_timeout(Duration::from_secs(10))??;
            anyhow::ensure!(bytes == 0, "fixture stdout must close without output");
            anyhow::ensure!(child.try_wait()?.is_none(), "fixture must still be alive after EOF");
            let started = Instant::now();
            reap_or_kill(&mut child);
            anyhow::ensure!(
                started.elapsed() < Duration::from_secs(10),
                "cleanup exceeded its bound"
            );
            anyhow::ensure!(child.try_wait()?.is_some(), "cleanup must leave a reaped child");
            Ok(())
        })();
        // Preserve the fixture even when a no-op cleanup mutation is detected.
        if child.try_wait()?.is_none() {
            child.kill()?;
            child.wait()?;
        }
        result
    }

    /// The ordinary path: a child that already exited is collected without
    /// being killed.
    #[test]
    fn an_already_exited_child_is_reaped_without_forcing() -> anyhow::Result<()> {
        let mut child = spawn_shell("exit 0")?;
        // Let it finish on its own terms before deciding.
        let _ = child.wait();

        let started = Instant::now();
        reap_or_kill(&mut child);

        anyhow::ensure!(
            started.elapsed() < Duration::from_secs(5),
            "reaping an exited child must be immediate"
        );
        Ok(())
    }
}
