//! LSP client for UX scenario tests.
//!
//! Spawns the real `perl-lsp` binary via stdio and communicates using the
//! JSON-RPC 2.0 / LSP Content-Length framing protocol.  All server-initiated
//! messages (`window/showMessage`, `window/logMessage`, diagnostic
//! notifications, etc.) are captured in an event queue so scenarios can
//! assert on user-visible messages after the fact.
// Test harness client — eprintln! echoes spawned server stderr for debugging.
#![allow(clippy::print_stderr)]

use crate::observation::{Inbox, InboxSnapshot, StreamEnd, WaitEnd};
use crate::{FakeWorkspace, ScenarioConfig};
use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
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
        self.wait_for(timeout, |snapshot: &InboxSnapshot| {
            let decoded: Vec<LspEvent> =
                snapshot.events().iter().cloned().map(decode_event).collect();
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
                let mut reader = BufReader::new(stdout);
                let end = loop {
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
                        FrameRead::EndOfStream => break StreamEnd::ServerClosed,
                        FrameRead::Failed(detail) => break StreamEnd::TransportFailure { detail },
                    }
                };
                reader_inbox.close(end);
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
        mut select: impl FnMut(&[Value]) -> Option<T>,
    ) -> Result<T, WaitEnd> {
        self.inbox.wait_for(timeout, |snapshot: &InboxSnapshot| select(snapshot.events()))
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
                .wait_for(remaining, |snapshot: &InboxSnapshot| {
                    snapshot
                        .responses()
                        .iter()
                        .find(|(_, value)| value["id"] == wanted)
                        .map(|(observation, _)| *observation)
                })
                .map_err(|end| anyhow!("No LSP response to id={id}: {}", end.describe()))?;

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
        // Wait for the server's own end-of-stream rather than polling
        // `try_wait` on a timer: the stdout reader records the stream end the
        // moment it happens, so an orderly exit is observed immediately and a
        // wedged server still costs only the bound below.
        // The predicate never matches, so the wait can only end by the stream
        // ending or by the grace bound expiring — exactly the two cases below.
        let closed =
            matches!(self.inbox.wait_for(SHUTDOWN_GRACE, |_| None::<()>), Err(WaitEnd::Ended(_)));
        if closed && let Ok(mut child) = self.child.lock() {
            // The stream ended; reap without forcing.
            let _ = child.wait();
            return;
        }
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
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
    let mut body = vec![0u8; len];
    if let Err(error) = reader.read_exact(&mut body) {
        return FrameRead::Failed(format!(
            "stream ended or failed while reading a {len}-byte LSP body: {error}"
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
