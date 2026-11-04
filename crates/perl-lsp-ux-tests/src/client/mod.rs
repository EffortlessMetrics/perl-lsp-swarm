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
use crate::{ChildExit, FakeWorkspace, ScenarioConfig, poll_child_exit};
use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use server_request_script::{
    ObservedServerRequest, ScriptedServerRequest, ServerRequestObserver, ServerRequestScript,
};
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub mod server_request_script;

static NEXT_ID: AtomicU64 = AtomicU64::new(100);
const SHUTDOWN_RUNNING: u8 = 0;
const SHUTDOWN_STARTED: u8 = 1;
const SHUTDOWN_EXIT_SENT: u8 = 2;
const SHUTDOWN_COMPLETE: u8 = 3;
const STDERR_TAIL_LINES: usize = 20;

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

/// Evidence returned only after a protocol-correct, zero-status shutdown.
#[derive(Debug)]
pub struct UxGracefulShutdown {
    /// Exact process exit status observed after the `exit` notification.
    pub status: ExitStatus,
}

/// Evidence that a server request required a capability the scenario did not
/// advertise.
#[derive(Debug, Clone, PartialEq)]
pub struct CapabilityViolation {
    /// Exact JSON-RPC request id supplied by the server.
    pub id: Value,
    /// Server method that was rejected.
    pub method: String,
    /// Capability path that was absent or false.
    pub capability: String,
}

/// A lightweight LSP client that speaks directly to a spawned perl-lsp process.
pub struct UxClient {
    child: Mutex<Child>,
    stdin: Arc<Mutex<Option<ChildStdin>>>,
    initialize_result: Value,
    /// The single observation substrate: buffered events, buffered responses,
    /// and the typed reason the server's output stream ended. Every wait in the
    /// harness blocks on this rather than sleeping on a wall-clock timer.
    inbox: Inbox,
    /// Append-only server-request evidence, independent of drainable events.
    server_requests: Arc<Mutex<Vec<Value>>>,
    /// Capability-gating violations observed on server requests.
    capability_violations: Arc<Mutex<Vec<CapabilityViolation>>>,
    /// Stderr lines captured from the server process.
    stderr_lines: Arc<Mutex<Vec<String>>>,
    script: Option<ServerRequestScript>,
    shutdown_state: AtomicU8,
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
        let mut client = Self::spawn_process(binary_path, config, None)?;
        let capabilities = build_client_capabilities(config);
        client.initialize_result =
            client.handshake(workspace, config, &capabilities, config.timeout)?;
        Ok(client)
    }

    /// Spawn the fixture binary and install scripted responses for its
    /// server-initiated requests.
    pub fn spawn_scripted(
        binary_path: &str,
        root_uri: &str,
        script: Vec<ScriptedServerRequest>,
        timeout: Duration,
    ) -> Result<Self> {
        let config = ScenarioConfig::default();
        let mut client = Self::spawn_process(binary_path, &config, Some(script))?;
        client.initialize_result = client.scripted_handshake(root_uri, timeout)?;
        Ok(client)
    }

    fn spawn_process(
        binary_path: &str,
        config: &ScenarioConfig,
        scripted_requests: Option<Vec<ScriptedServerRequest>>,
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
        let stdin = Arc::new(Mutex::new(Some(stdin)));
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("perl-lsp stdout not available after spawn"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("perl-lsp stderr not available after spawn"))?;

        let inbox = Inbox::new();
        // The stdout reader also answers server-initiated requests, so the
        // writer handle is shared with that thread rather than owned alone.
        let stderr_lines: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let server_requests: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
        let capability_violations: Arc<Mutex<Vec<CapabilityViolation>>> =
            Arc::new(Mutex::new(Vec::new()));
        // One response authority per request: when a script is installed it
        // owns every server-request answer, so the conservative default
        // answering is disabled rather than racing the script.
        let answering_capabilities = if scripted_requests.is_some() {
            None
        } else {
            Some(build_client_capabilities(config))
        };
        let (script, observer) = match scripted_requests {
            Some(script) => {
                let (script, observer) = ServerRequestScript::new(stdin.clone(), script)?;
                (Some(script), Some(observer))
            }
            None => (None, None),
        };

        // ── stdout reader thread ──────────────────────────────────────────────
        // Publishes into the inbox and, on exit, records *why* the stream ended
        // so a waiter can tell an orderly shutdown from a broken transport
        // instead of both surfacing as an unexplained timeout.
        let reader_inbox = inbox.clone();
        let stdin_for_reader = Arc::clone(&stdin);
        let server_requests_for_reader = Arc::clone(&server_requests);
        let capability_violations_for_reader = Arc::clone(&capability_violations);
        let answering_capabilities_for_reader = answering_capabilities.clone();
        let _stdout_thread = std::thread::Builder::new()
            .name("ux-lsp-stdout".into())
            .spawn(move || {
                // Records the stream end even if this thread unwinds, so a
                // waiter can never be left unable to distinguish a dead reader
                // from a merely silent server.
                let mut exit = ReaderExit::new(reader_inbox.clone());
                let mut reader = BufReader::new(stdout);
                let observer: Option<ServerRequestObserver> = observer;
                // The answering loop writes through the shared optional stdin
                // handle, failing closed once that handle has been taken over.
                let stdin_writer = Mutex::new(SharedStdinWriter(stdin_for_reader));
                loop {
                    match read_and_route(
                        &mut reader,
                        &stdin_writer,
                        &reader_inbox,
                        &server_requests_for_reader,
                        &capability_violations_for_reader,
                        answering_capabilities_for_reader.as_ref(),
                        observer.as_ref(),
                    ) {
                        Ok(true) => {}
                        Ok(false) => return exit.record(StreamEnd::ServerClosed),
                        Err(detail) => return exit.record(StreamEnd::TransportFailure { detail }),
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
        let client = Self {
            child: Mutex::new(child),
            stdin,
            initialize_result: Value::Null,
            inbox,
            server_requests,
            capability_violations,
            stderr_lines,
            script,
            shutdown_state: AtomicU8::new(SHUTDOWN_RUNNING),
            _stdout_thread,
            _stderr_thread,
        };

        Ok(client)
    }

    fn scripted_handshake(&self, root_uri: &str, timeout: Duration) -> Result<Value> {
        let init_resp = self.request(
            "initialize",
            json!({
                "processId": null,
                "rootUri": root_uri,
                "capabilities": {}
            }),
            timeout,
        )?;
        if let Some(err) = init_resp.get("error") {
            return Err(anyhow!("LSP initialize returned error: {}", err));
        }
        self.notify("initialized", json!({}))?;
        Ok(init_resp)
    }

    fn handshake(
        &self,
        workspace: &FakeWorkspace,
        config: &ScenarioConfig,
        client_capabilities: &Value,
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
            "capabilities": client_capabilities.clone(),
        });
        if !workspace_folders.is_empty() {
            params["workspaceFolders"] = Value::Array(workspace_folders);
        }
        if !config.initialization_options.is_null() {
            params["initializationOptions"] = config.initialization_options.clone();
        }

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

    /// Clone raw server-initiated requests without removing them from the queue.
    ///
    /// Requests remain observable after the client has sent its deterministic
    /// response, allowing scenarios to assert method, id, and params together.
    pub fn peek_server_requests(&self) -> Vec<Value> {
        self.server_requests.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Clone capability violations observed by the transport loop.
    pub fn peek_capability_violations(&self) -> Vec<CapabilityViolation> {
        self.capability_violations.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Return the recorded stdout transport failure, if the reader has ended
    /// for a framing, JSON, or response-write reason.
    ///
    /// Foreground request waits fold the same evidence into their errors, so
    /// malformed frames and write failures fail fast instead of timing out.
    pub fn peek_transport_error(&self) -> Option<String> {
        match self.inbox.stream_end() {
            Some(StreamEnd::TransportFailure { detail }) => Some(detail),
            _ => None,
        }
    }

    /// Clone all stderr lines captured from the server process.
    pub fn peek_stderr_lines(&self) -> Vec<String> {
        self.stderr_lines.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Wait for all scripted server requests to be observed and answered.
    pub fn wait_for_script(&self, timeout: Duration) -> Result<Vec<ObservedServerRequest>> {
        self.script
            .as_ref()
            .ok_or_else(|| anyhow!("client has no scripted server-request script"))?
            .wait(timeout)
    }

    /// Fail if the server sent a server-initiated request not in the script.
    pub fn assert_no_unscripted_requests(&self) -> Result<()> {
        self.script
            .as_ref()
            .ok_or_else(|| anyhow!("client has no scripted server-request script"))?
            .assert_no_unscripted()
    }

    /// Complete the legal LSP lifecycle and prove a zero-status process exit.
    ///
    /// A successful result requires a matching JSON-RPC shutdown response with
    /// an explicit `null` result, followed by `exit`, followed by bounded
    /// zero-status process termination. A second call fails before emitting a
    /// second normal lifecycle sequence. Destructor cleanup remains fallback,
    /// not evidence.
    pub fn shutdown_and_exit(&self, timeout: Duration) -> Result<UxGracefulShutdown> {
        self.shutdown_state
            .compare_exchange(
                SHUTDOWN_RUNNING,
                SHUTDOWN_STARTED,
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .map_err(|state| {
                anyhow!("explicit LSP shutdown already started or completed (state={state})")
            })?;

        let response = self
            .request("shutdown", Value::Null, timeout)
            .context("explicit LSP shutdown request failed")?;
        validate_shutdown_response(&response)?;

        self.notify("exit", Value::Null)
            .context("failed to send LSP exit notification after accepted shutdown")?;
        self.shutdown_state.store(SHUTDOWN_EXIT_SENT, Ordering::SeqCst);

        // Await process exit, not stream end: the server may close its output
        // before its final cleanup finishes, so stream end must never be
        // mistaken for exit. The poll itself lives in the governed harness
        // root, where its declared quantum keeps this substrate sleep-free.
        let deadline = Instant::now() + timeout;
        match poll_child_exit(&self.child, deadline)? {
            ChildExit::Exited(status) => {
                self.shutdown_state.store(SHUTDOWN_COMPLETE, Ordering::SeqCst);
                if !status.success() {
                    return Err(anyhow!(
                        "perl-lsp exited unsuccessfully after legal shutdown: {status}; stderr={}",
                        self.stderr_tail()
                    ));
                }
                Ok(UxGracefulShutdown { status })
            }
            ChildExit::TimedOut => Err(anyhow!(
                "perl-lsp did not exit within {}ms after accepted shutdown; stderr={}",
                timeout.as_millis(),
                self.stderr_tail()
            )),
        }
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
        let mut stdin = self.stdin.lock().unwrap_or_else(|e| e.into_inner());
        let stdin = stdin.as_mut().ok_or_else(|| anyhow!("LSP client stdin is already closed"))?;
        write_framed_to(stdin, msg)
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

    fn stderr_tail(&self) -> String {
        let lines = self.stderr_lines.lock().unwrap_or_else(|e| e.into_inner());
        format_stderr_tail(&lines)
    }
}

fn format_stderr_tail(lines: &[String]) -> String {
    lines
        .iter()
        .skip(lines.len().saturating_sub(STDERR_TAIL_LINES))
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join("\n")
}

fn validate_shutdown_response(response: &Value) -> Result<()> {
    if response.get("jsonrpc") != Some(&json!("2.0")) {
        return Err(anyhow!("shutdown response omitted JSON-RPC 2.0: {response}"));
    }
    if let Some(error) = response.get("error") {
        return Err(anyhow!("shutdown returned JSON-RPC error: {error}"));
    }
    let result = response
        .get("result")
        .ok_or_else(|| anyhow!("shutdown response omitted result: {response}"))?;
    if !result.is_null() {
        return Err(anyhow!("shutdown result must be null, got: {response}"));
    }
    Ok(())
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
        if let Some(script) = self.script.take() {
            script.settle();
        }

        let shutdown_state = self.shutdown_state.load(Ordering::SeqCst);
        let mut stdin = self.stdin.lock().unwrap_or_else(|error| error.into_inner());
        finish_stdin(&mut stdin, shutdown_state == SHUTDOWN_RUNNING);
        if shutdown_state == SHUTDOWN_COMPLETE {
            return;
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

/// Emit the best-effort `shutdown`/`exit` pair, then close the pipe.
///
/// Closing must happen after the write: the frames are what let the server
/// exit on its own terms, and an early close turns that into an EOF kill.
fn finish_stdin<W: Write>(slot: &mut Option<W>, send_shutdown: bool) {
    if send_shutdown && let Some(stdin) = slot.as_mut() {
        for message in [
            json!({"jsonrpc": "2.0", "id": 999998, "method": "shutdown", "params": {}}),
            json!({"jsonrpc": "2.0", "method": "exit"}),
        ] {
            let _ = write_framed_to(stdin, &message);
        }
    }
    slot.take();
}

fn write_framed_to<W: Write>(stdin: &mut W, message: &Value) -> Result<()> {
    let body = message.to_string();
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    stdin.write_all(header.as_bytes()).context("Failed to write LSP header to stdin")?;
    stdin.write_all(body.as_bytes()).context("Failed to write LSP body to stdin")?;
    stdin.flush().context("Failed to flush LSP stdin")
}

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

// ── Server-request routing ───────────────────────────────────────────────────

/// Read one frame and route it through the shared observation substrate.
///
/// `Ok(true)` = one message routed; `Ok(false)` = the server's stream ended at
/// a message boundary; `Err(detail)` = a framing, JSON, or response-write
/// failure that the caller must record as a transport failure.
fn read_and_route<R, W>(
    reader: &mut R,
    stdin: &Mutex<W>,
    inbox: &Inbox,
    server_requests: &Mutex<Vec<Value>>,
    capability_violations: &Mutex<Vec<CapabilityViolation>>,
    capabilities: Option<&Value>,
    observer: Option<&ServerRequestObserver>,
) -> Result<bool, String>
where
    R: BufRead,
    W: Write,
{
    let message = match read_one_frame(reader) {
        FrameRead::Message(message) => message,
        FrameRead::EndOfStream => return Ok(false),
        FrameRead::Failed(detail) => return Err(detail),
    };
    route_message(
        &message,
        stdin,
        inbox,
        server_requests,
        capability_violations,
        capabilities,
        observer,
    )?;
    Ok(true)
}

/// Route one decoded stdout message: responses feed response waits, server
/// requests are answered conservatively while their evidence is retained, and
/// everything else is buffered as an observable event.
fn route_message<W>(
    message: &Value,
    stdin: &Mutex<W>,
    inbox: &Inbox,
    server_requests: &Mutex<Vec<Value>>,
    capability_violations: &Mutex<Vec<CapabilityViolation>>,
    capabilities: Option<&Value>,
    observer: Option<&ServerRequestObserver>,
) -> Result<(), String>
where
    W: Write,
{
    if let Some(observer) = observer {
        observer.observe(message);
    }
    let has_id = message.get("id").is_some_and(|id| !id.is_null());
    let is_response = has_id && (message.get("result").is_some() || message.get("error").is_some());
    if is_response {
        inbox.push_response(message.clone());
        return Ok(());
    }

    if is_server_request(message) {
        server_requests
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(message.clone());
        if let Some(decision) = capabilities.and_then(|caps| server_request_decision(message, caps))
        {
            if let Some(violation) = decision.capability_violation {
                capability_violations
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(violation);
            }
            let method =
                message.get("method").and_then(Value::as_str).unwrap_or("<missing>").to_owned();
            let id = message.get("id").cloned().unwrap_or(Value::Null);
            let mut stdin = stdin.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            write_framed_to(&mut *stdin, &decision.response).map_err(|error| {
                format!("failed to answer server request method={method} id={id}: {error:#}")
            })?;
        }
    }

    inbox.push_event(message.clone());
    Ok(())
}

/// A `Write` adapter over the shared optional stdin handle.
///
/// Each write re-locks and fails closed if the handle was already taken over,
/// so a scripted or finished client can never hand the answering loop a stale
/// writer.
struct SharedStdinWriter(Arc<Mutex<Option<ChildStdin>>>);

impl Write for SharedStdinWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut guard = self.0.lock().unwrap_or_else(|error| error.into_inner());
        let stdin = guard
            .as_mut()
            .ok_or_else(|| std::io::Error::other("LSP client stdin is already closed"))?;
        stdin.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let mut guard = self.0.lock().unwrap_or_else(|error| error.into_inner());
        match guard.as_mut() {
            Some(stdin) => stdin.flush(),
            None => Err(std::io::Error::other("LSP client stdin is already closed")),
        }
    }
}

fn build_client_capabilities(config: &ScenarioConfig) -> Value {
    let mut capabilities = json!({
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
    });
    merge_json(&mut capabilities, &config.client_capability_overrides);
    capabilities
}
fn is_server_request(message: &Value) -> bool {
    message.get("id").is_some_and(|id| !id.is_null())
        && message.get("method").and_then(Value::as_str).is_some()
}

struct ServerRequestDecision {
    response: Value,
    capability_violation: Option<CapabilityViolation>,
}

fn server_request_decision(message: &Value, capabilities: &Value) -> Option<ServerRequestDecision> {
    if !is_server_request(message) {
        return None;
    }

    let id = message.get("id")?.clone();
    let method = message.get("method")?.as_str()?;
    let required_capability = match method {
        "workspace/applyEdit" => Some("workspace.applyEdit"),
        "workspace/configuration" => Some("workspace.configuration"),
        "window/showMessageRequest" => Some("window.showMessage"),
        "window/showDocument" => Some("window.showDocument.support"),
        "window/workDoneProgress/create" => Some("window.workDoneProgress"),
        "workspace/codeLens/refresh" => Some("workspace.codeLens.refreshSupport"),
        "workspace/semanticTokens/refresh" => Some("workspace.semanticTokens.refreshSupport"),
        "workspace/inlayHint/refresh" => Some("workspace.inlayHint.refreshSupport"),
        "workspace/inlineValue/refresh" => Some("workspace.inlineValue.refreshSupport"),
        "workspace/diagnostic/refresh" => Some("workspace.diagnostics.refreshSupport"),
        "workspace/foldingRange/refresh" => Some("workspace.foldingRange.refreshSupport"),
        "workspace/textDocumentContent/refresh" => {
            Some("workspace.textDocumentContent.refreshSupport")
        }
        "client/registerCapability" | "client/unregisterCapability" => {
            let field = if method == "client/registerCapability" {
                "registrations"
            } else {
                "unregisterations"
            };
            if let Some(issue) = dynamic_registration_issue(message, field, capabilities) {
                return Some(match issue {
                    DynamicRegistrationIssue::Capability(capability) => {
                        capability_violation(message, method, &capability)
                    }
                    DynamicRegistrationIssue::Malformed(reason) => {
                        invalid_params(message, method, &reason)
                    }
                });
            }
            None
        }
        _ => None,
    };

    if let Some(capability) = required_capability
        && !capability_is_advertised(capabilities, capability)
    {
        return Some(capability_violation(message, method, capability));
    }

    let result = match method {
        "workspace/applyEdit" => json!({
            "applied": false,
            "failureReason": "UX test client does not apply workspace edits automatically"
        }),
        "workspace/configuration" => {
            let item_count =
                message.pointer("/params/items").and_then(Value::as_array).map_or(0, Vec::len);
            Value::Array(vec![Value::Null; item_count])
        }
        "window/showMessageRequest" => Value::Null,
        "window/showDocument" => json!({ "success": false }),
        "client/registerCapability"
        | "client/unregisterCapability"
        | "window/workDoneProgress/create"
        | "workspace/codeLens/refresh"
        | "workspace/semanticTokens/refresh"
        | "workspace/inlayHint/refresh"
        | "workspace/inlineValue/refresh"
        | "workspace/diagnostic/refresh"
        | "workspace/foldingRange/refresh"
        | "workspace/textDocumentContent/refresh" => Value::Null,
        _ => {
            return Some(ServerRequestDecision {
                response: json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {
                        "code": -32601,
                        "message": format!("Method not found: {method}")
                    }
                }),
                capability_violation: None,
            });
        }
    };

    Some(ServerRequestDecision {
        response: json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result
        }),
        capability_violation: None,
    })
}

#[cfg(test)]
fn server_request_response(message: &Value, capabilities: &Value) -> Option<Value> {
    server_request_decision(message, capabilities).map(|decision| decision.response)
}

fn capability_violation(message: &Value, method: &str, capability: &str) -> ServerRequestDecision {
    let id = message.get("id").cloned().unwrap_or(Value::Null);
    ServerRequestDecision {
        response: json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": -32601,
                "message": format!("Client capability not advertised: {capability} for {method}")
            }
        }),
        capability_violation: Some(CapabilityViolation {
            id,
            method: method.to_owned(),
            capability: capability.to_owned(),
        }),
    }
}

fn invalid_params(message: &Value, method: &str, reason: &str) -> ServerRequestDecision {
    ServerRequestDecision {
        response: json!({
            "jsonrpc": "2.0",
            "id": message.get("id").cloned().unwrap_or(Value::Null),
            "error": {
                "code": -32602,
                "message": format!("Invalid params for {method}: {reason}")
            }
        }),
        capability_violation: None,
    }
}

fn capability_is_advertised(capabilities: &Value, path: &str) -> bool {
    if path == "workspace.diagnostics.refreshSupport"
        && capability_is_advertised(capabilities, "workspace.diagnostic.refreshSupport")
    {
        return true;
    }

    let pointer = format!("/{}", path.replace('.', "/"));
    let value = capabilities.pointer(&pointer);
    if path == "window.showMessage" {
        value.is_some_and(Value::is_object)
    } else {
        value.and_then(Value::as_bool) == Some(true)
    }
}

enum DynamicRegistrationIssue {
    Capability(String),
    Malformed(String),
}

fn dynamic_registration_issue(
    message: &Value,
    field: &str,
    capabilities: &Value,
) -> Option<DynamicRegistrationIssue> {
    let Some(registrations) =
        message.pointer(&format!("/params/{field}")).and_then(Value::as_array)
    else {
        return Some(DynamicRegistrationIssue::Malformed(format!("missing /params/{field} array")));
    };
    if registrations.is_empty() {
        return Some(DynamicRegistrationIssue::Malformed(
            "registration array must not be empty".to_owned(),
        ));
    }
    for registration in registrations {
        let Some(id) = registration.get("id").and_then(Value::as_str) else {
            return Some(DynamicRegistrationIssue::Malformed(
                "every registration must include a string id".to_owned(),
            ));
        };
        if id.is_empty() {
            return Some(DynamicRegistrationIssue::Malformed(
                "every registration id must be non-empty".to_owned(),
            ));
        }
        let Some(method) = registration.get("method").and_then(Value::as_str) else {
            return Some(DynamicRegistrationIssue::Malformed(
                "every registration must include a string method".to_owned(),
            ));
        };
        let Some(capabilities_required) = registration_capability_paths(method) else {
            return Some(DynamicRegistrationIssue::Malformed(format!(
                "unsupported dynamic registration method {method}"
            )));
        };
        for capability in capabilities_required {
            if !capability_is_advertised(capabilities, capability) {
                return Some(DynamicRegistrationIssue::Capability((*capability).to_owned()));
            }
        }
    }
    None
}

fn registration_capability_paths(method: &str) -> Option<&'static [&'static str]> {
    let paths: &[&str] = match method {
        "workspace/didChangeConfiguration" => {
            &["workspace.didChangeConfiguration.dynamicRegistration"]
        }
        "workspace/didChangeWatchedFiles" => {
            &["workspace.didChangeWatchedFiles.dynamicRegistration"]
        }
        "workspace/didChangeWorkspaceFolders" => &["workspace.workspaceFolders"],
        "workspace/executeCommand" => &["workspace.executeCommand.dynamicRegistration"],
        "workspace/symbol" => &["workspace.symbol.dynamicRegistration"],
        "workspace/didCreateFiles" => {
            &["workspace.fileOperations.dynamicRegistration", "workspace.fileOperations.didCreate"]
        }
        "workspace/willCreateFiles" => {
            &["workspace.fileOperations.dynamicRegistration", "workspace.fileOperations.willCreate"]
        }
        "workspace/didRenameFiles" => {
            &["workspace.fileOperations.dynamicRegistration", "workspace.fileOperations.didRename"]
        }
        "workspace/willRenameFiles" => {
            &["workspace.fileOperations.dynamicRegistration", "workspace.fileOperations.willRename"]
        }
        "workspace/didDeleteFiles" => {
            &["workspace.fileOperations.dynamicRegistration", "workspace.fileOperations.didDelete"]
        }
        "workspace/willDeleteFiles" => {
            &["workspace.fileOperations.dynamicRegistration", "workspace.fileOperations.willDelete"]
        }
        "textDocument/completion" => &["textDocument.completion.dynamicRegistration"],
        "textDocument/didOpen"
        | "textDocument/didClose"
        | "textDocument/didChange"
        | "textDocument/willSave"
        | "textDocument/willSaveWaitUntil"
        | "textDocument/didSave" => &["textDocument.synchronization.dynamicRegistration"],
        "textDocument/inlineCompletion" => &["textDocument.inlineCompletion.dynamicRegistration"],
        "textDocument/hover" => &["textDocument.hover.dynamicRegistration"],
        "textDocument/definition" => &["textDocument.definition.dynamicRegistration"],
        "textDocument/declaration" => &["textDocument.declaration.dynamicRegistration"],
        "textDocument/typeDefinition" => &["textDocument.typeDefinition.dynamicRegistration"],
        "textDocument/implementation" => &["textDocument.implementation.dynamicRegistration"],
        "textDocument/references" => &["textDocument.references.dynamicRegistration"],
        "textDocument/documentHighlight" => &["textDocument.documentHighlight.dynamicRegistration"],
        "textDocument/documentSymbol" => &["textDocument.documentSymbol.dynamicRegistration"],
        "textDocument/codeAction" => &["textDocument.codeAction.dynamicRegistration"],
        "textDocument/codeLens" => &["textDocument.codeLens.dynamicRegistration"],
        "textDocument/documentLink" => &["textDocument.documentLink.dynamicRegistration"],
        "textDocument/documentColor" => &["textDocument.colorProvider.dynamicRegistration"],
        "textDocument/formatting" => &["textDocument.formatting.dynamicRegistration"],
        "textDocument/rangeFormatting" => &["textDocument.rangeFormatting.dynamicRegistration"],
        "textDocument/onTypeFormatting" => &["textDocument.onTypeFormatting.dynamicRegistration"],
        "textDocument/rename" => &["textDocument.rename.dynamicRegistration"],
        "textDocument/publishDiagnostics" => {
            &["textDocument.publishDiagnostics.dynamicRegistration"]
        }
        "textDocument/signatureHelp" => &["textDocument.signatureHelp.dynamicRegistration"],
        "textDocument/semanticTokens" => &["textDocument.semanticTokens.dynamicRegistration"],
        "textDocument/inlayHint" => &["textDocument.inlayHint.dynamicRegistration"],
        "textDocument/inlineValue" => &["textDocument.inlineValue.dynamicRegistration"],
        _ => return None,
    };
    Some(paths)
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
    use super::{FrameRead, ReaderExit, finish_stdin, read_one_frame};
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

    #[test]
    fn finish_stdin_writes_shutdown_before_closing() -> anyhow::Result<()> {
        let mut bytes = Vec::new();
        let mut slot = Some(&mut bytes);
        finish_stdin(&mut slot, true);
        let framed = String::from_utf8(bytes)?;
        anyhow::ensure!(
            framed.matches("Content-Length: ").count() == 2,
            "shutdown and exit must each have a Content-Length header: {framed:?}"
        );
        anyhow::ensure!(
            framed.contains("\"method\":\"shutdown\""),
            "shutdown frame must be written before closing: {framed:?}"
        );
        anyhow::ensure!(
            framed.contains("\"method\":\"exit\""),
            "exit frame must be written before closing: {framed:?}"
        );

        let mut closed_bytes = Vec::new();
        let mut closed = Some(&mut closed_bytes);
        finish_stdin(&mut closed, false);
        anyhow::ensure!(closed.is_none(), "finish_stdin must close the pipe");
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

#[cfg(test)]
mod shutdown_response_tests {
    use super::{format_stderr_tail, validate_shutdown_response};
    use anyhow::{Result, ensure};
    use serde_json::json;

    #[test]
    fn shutdown_response_requires_jsonrpc_null_result_and_no_error() -> Result<()> {
        ensure!(
            validate_shutdown_response(&json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": null
            }))
            .is_ok(),
            "JSON-RPC 2.0 shutdown response with null result must be accepted"
        );

        for invalid in [
            json!({"id": 1, "result": null}),
            json!({"jsonrpc": "2.0", "id": 1}),
            json!({"jsonrpc": "2.0", "id": 1, "result": {}}),
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "error": {"code": -32603, "message": "failed"}
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": null,
                "error": {"code": -32603, "message": "failed"}
            }),
        ] {
            ensure!(
                validate_shutdown_response(&invalid).is_err(),
                "invalid shutdown response was accepted: {invalid}"
            );
        }
        Ok(())
    }

    #[test]
    fn shutdown_stderr_tail_preserves_last_twenty_lines_in_order() -> Result<()> {
        ensure!(format_stderr_tail(&[]).is_empty(), "empty stderr must have an empty tail");
        let short = vec!["first".to_string(), "second".to_string(), "last".to_string()];
        ensure!(
            format_stderr_tail(&short) == "first\nsecond\nlast",
            "short stderr must retain every line in order"
        );
        let long: Vec<String> = (0..25).map(|line| line.to_string()).collect();
        ensure!(
            format_stderr_tail(&long)
                == "5\n6\n7\n8\n9\n10\n11\n12\n13\n14\n15\n16\n17\n18\n19\n20\n21\n22\n23\n24",
            "long stderr must retain exactly the final twenty lines in order"
        );
        Ok(())
    }
}

#[cfg(test)]
mod server_request_tests {
    use super::{
        CapabilityViolation, FrameRead, Inbox, ServerRequestDecision, build_client_capabilities,
        is_server_request, read_one_frame, route_message, server_request_decision,
        server_request_response, write_framed_to,
    };
    use crate::ScenarioConfig;
    use anyhow::{Result, anyhow};
    use serde_json::{Value, json};
    use std::io::{BufReader, Write};
    use std::sync::{Arc, Mutex};

    struct BrokenWriter;

    impl Write for BrokenWriter {
        fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "synthetic broken pipe"))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn capabilities_with(overrides: Value) -> Value {
        let config =
            ScenarioConfig { client_capability_overrides: overrides, ..ScenarioConfig::default() };
        build_client_capabilities(&config)
    }

    fn first_frame(reader: &mut impl std::io::BufRead) -> Result<Value> {
        match read_one_frame(reader) {
            FrameRead::Message(message) => Ok(message),
            other => Err(anyhow!("expected a framed message, got {other:?}")),
        }
    }

    #[test]
    fn router_answers_server_request_preserves_evidence_and_keeps_routing() -> Result<()> {
        let server_request = json!({
            "jsonrpc": "2.0",
            "id": "server-17",
            "method": "workspace/configuration",
            "params": {
                "items": [
                    { "section": "perl" },
                    { "section": "perl.formatting" }
                ]
            }
        });
        let later_response = json!({
            "jsonrpc": "2.0",
            "id": 101,
            "result": { "ok": true }
        });
        let mut server_stdout = Vec::new();
        write_framed_to(&mut server_stdout, &server_request)?;
        write_framed_to(&mut server_stdout, &later_response)?;

        let mut reader = BufReader::new(server_stdout.as_slice());
        let stdin = Arc::new(Mutex::new(Vec::new()));
        let inbox = Inbox::new();
        let server_requests = Mutex::new(Vec::new());
        let violations = Mutex::new(Vec::new());
        let capabilities = capabilities_with(json!({
            "workspace": { "configuration": true }
        }));

        for _ in 0..2 {
            let message = first_frame(&mut reader)?;
            route_message(
                &message,
                &stdin,
                &inbox,
                &server_requests,
                &violations,
                Some(&capabilities),
                None,
            )
            .map_err(|error| anyhow!("{error}"))?;
        }

        let framed_response = stdin.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let mut response_reader = BufReader::new(framed_response.as_slice());
        let client_response = first_frame(&mut response_reader)?;
        anyhow::ensure!(
            client_response["id"] == "server-17"
                && client_response["result"] == json!([null, null]),
            "the answered request must keep its id and per-item null results: {client_response}"
        );

        let observed = inbox.snapshot().events().to_vec();
        anyhow::ensure!(
            observed == vec![server_request.clone()],
            "the request must remain observable as an event: {observed:?}"
        );
        let recorded = server_requests.lock().unwrap_or_else(|e| e.into_inner()).clone();
        anyhow::ensure!(
            recorded == vec![server_request],
            "server request evidence must be preserved separately"
        );
        let responses: Vec<Value> =
            inbox.snapshot().responses().iter().map(|(_, value)| value.clone()).collect();
        anyhow::ensure!(
            responses == vec![later_response],
            "the later response must feed response waits: {responses:?}"
        );
        anyhow::ensure!(
            violations.lock().unwrap_or_else(|e| e.into_inner()).is_empty(),
            "an admitted request must not record a violation"
        );
        Ok(())
    }

    #[test]
    fn a_response_write_failure_is_a_typed_transport_failure_with_evidence() -> Result<()> {
        let request = json!({
            "jsonrpc": "2.0",
            "id": 33,
            "method": "window/workDoneProgress/create",
            "params": { "token": "index" }
        });
        let stdin = Arc::new(Mutex::new(BrokenWriter));
        let inbox = Inbox::new();
        let server_requests = Mutex::new(Vec::new());
        let violations = Mutex::new(Vec::new());
        let capabilities = capabilities_with(json!({
            "window": { "workDoneProgress": true }
        }));

        let failure = route_message(
            &request,
            &stdin,
            &inbox,
            &server_requests,
            &violations,
            Some(&capabilities),
            None,
        )
        .err()
        .ok_or_else(|| anyhow!("broken writer unexpectedly accepted the response"))?;
        anyhow::ensure!(
            failure.contains("method=window/workDoneProgress/create id=33")
                && failure.contains("synthetic broken pipe"),
            "the failure must name the request and the underlying write error: {failure}"
        );
        anyhow::ensure!(
            !server_requests.lock().unwrap_or_else(|e| e.into_inner()).is_empty(),
            "the request evidence must survive the failed answer"
        );
        Ok(())
    }

    #[test]
    fn partial_header_eof_remains_a_transport_failure() -> Result<()> {
        for input in [
            "Content-Length: 10\r\n",
            "Content-Length: 10",
            "Content-Type: application/vscode-jsonrpc\r\n",
        ] {
            let mut reader = BufReader::new(input.as_bytes());
            match read_one_frame(&mut reader) {
                FrameRead::Failed(detail) => {
                    anyhow::ensure!(
                        detail.contains("part way through"),
                        "a truncated header block must be a framing failure: {detail}"
                    );
                }
                other => {
                    return Err(anyhow!(
                        "partial header must never read as an orderly close: {other:?}"
                    ));
                }
            }
        }
        Ok(())
    }

    #[test]
    fn known_server_requests_receive_results() {
        let capabilities = capabilities_with(json!({
            "workspace": {
                "applyEdit": true,
                "configuration": true,
                "codeLens": { "refreshSupport": true },
                "semanticTokens": { "refreshSupport": true },
                "inlayHint": { "refreshSupport": true },
                "inlineValue": { "refreshSupport": true },
                "diagnostics": { "refreshSupport": true },
                "foldingRange": { "refreshSupport": true },
                "textDocumentContent": { "refreshSupport": true }
            },
            "textDocument": {
                "completion": { "dynamicRegistration": true }
            },
            "window": {
                "showDocument": { "support": true },
                "workDoneProgress": true
            }
        }));
        for method in [
            "workspace/applyEdit",
            "workspace/configuration",
            "client/registerCapability",
            "client/unregisterCapability",
            "window/showMessageRequest",
            "window/showDocument",
            "window/workDoneProgress/create",
            "workspace/codeLens/refresh",
            "workspace/semanticTokens/refresh",
            "workspace/inlayHint/refresh",
            "workspace/inlineValue/refresh",
            "workspace/diagnostic/refresh",
            "workspace/foldingRange/refresh",
            "workspace/textDocumentContent/refresh",
        ] {
            let params = match method {
                "client/registerCapability" => json!({
                    "registrations": [{
                        "id": "completion",
                        "method": "textDocument/completion"
                    }]
                }),
                "client/unregisterCapability" => json!({
                    "unregisterations": [{
                        "id": "completion",
                        "method": "textDocument/completion"
                    }]
                }),
                _ => json!({ "items": [] }),
            };
            let request = json!({
                "jsonrpc": "2.0",
                "id": "server-request-1",
                "method": method,
                "params": params
            });
            let response = server_request_response(&request, &capabilities).unwrap_or(Value::Null);

            assert_eq!(response["jsonrpc"], "2.0", "method={method}");
            assert_eq!(response["id"], "server-request-1", "method={method}");
            assert!(response.get("result").is_some(), "method={method}");
            assert!(response.get("error").is_none(), "method={method}");
        }
    }

    #[test]
    fn workspace_configuration_preserves_result_cardinality() {
        let request = json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "workspace/configuration",
            "params": {
                "items": [
                    { "section": "perl" },
                    { "section": "perl.formatting" }
                ]
            }
        });
        let capabilities = capabilities_with(json!({
            "workspace": { "configuration": true }
        }));
        let response = server_request_response(&request, &capabilities).unwrap_or(Value::Null);

        assert_eq!(response["result"], json!([null, null]));
    }

    #[test]
    fn workspace_apply_edit_is_refused_without_hidden_mutation() {
        let request = json!({
            "jsonrpc": "2.0",
            "id": 8,
            "method": "workspace/applyEdit",
            "params": { "edit": { "changes": {} } }
        });
        let capabilities = capabilities_with(json!({
            "workspace": { "applyEdit": true }
        }));
        let response = server_request_response(&request, &capabilities).unwrap_or(Value::Null);

        assert_eq!(response["result"]["applied"], false);
        assert_eq!(
            response["result"]["failureReason"],
            "UX test client does not apply workspace edits automatically"
        );
    }

    #[test]
    fn unknown_server_request_receives_method_not_found() {
        let request = json!({
            "jsonrpc": "2.0",
            "id": "extension-3",
            "method": "experimental/clientPrompt",
            "params": {}
        });
        let response = server_request_response(
            &request,
            &build_client_capabilities(&ScenarioConfig::default()),
        )
        .unwrap_or(Value::Null);

        assert_eq!(response["id"], "extension-3");
        assert_eq!(response["error"]["code"], -32601);
        assert_eq!(response["error"]["message"], "Method not found: experimental/clientPrompt");
    }

    #[test]
    fn notification_is_not_misclassified_as_server_request() {
        let notification = json!({
            "jsonrpc": "2.0",
            "method": "workspace/semanticTokens/refresh",
            "params": {}
        });

        assert!(!is_server_request(&notification));
        assert!(
            server_request_response(
                &notification,
                &build_client_capabilities(&ScenarioConfig::default())
            )
            .is_none()
        );
    }

    #[test]
    fn known_but_unadvertised_capability_is_rejected_and_recorded() {
        let request = json!({
            "jsonrpc": "2.0",
            "id": 14,
            "method": "workspace/semanticTokens/refresh",
            "params": {}
        });
        let decision = server_request_decision(
            &request,
            &build_client_capabilities(&ScenarioConfig::default()),
        )
        .unwrap_or(ServerRequestDecision { response: Value::Null, capability_violation: None });

        assert_eq!(decision.response["id"], 14);
        assert_eq!(decision.response["error"]["code"], -32601);
        assert!(
            decision.response["error"]["message"]
                .as_str()
                .is_some_and(|message| message.contains("workspace.semanticTokens.refreshSupport"))
        );
        assert_eq!(
            decision.capability_violation,
            Some(CapabilityViolation {
                id: json!(14),
                method: "workspace/semanticTokens/refresh".to_owned(),
                capability: "workspace.semanticTokens.refreshSupport".to_owned(),
            })
        );
    }

    #[test]
    fn advertised_capability_allows_known_request() {
        let request = json!({
            "jsonrpc": "2.0",
            "id": "refresh-1",
            "method": "workspace/semanticTokens/refresh",
            "params": {}
        });
        let capabilities = capabilities_with(json!({
            "workspace": {
                "semanticTokens": { "refreshSupport": true }
            }
        }));
        let response = server_request_response(&request, &capabilities).unwrap_or(Value::Null);

        assert_eq!(response["id"], "refresh-1");
        assert_eq!(response["result"], Value::Null);
        assert!(response.get("error").is_none());
    }

    #[test]
    fn inline_completion_dynamic_registration_is_admitted() {
        let request = json!({
            "jsonrpc": "2.0",
            "id": "inline-registration",
            "method": "client/registerCapability",
            "params": {
                "registrations": [{
                    "id": "inline-completion",
                    "method": "textDocument/inlineCompletion"
                }]
            }
        });
        let capabilities = capabilities_with(json!({
            "textDocument": {
                "inlineCompletion": { "dynamicRegistration": true }
            }
        }));
        let response = server_request_response(&request, &capabilities).unwrap_or(Value::Null);

        assert_eq!(response["id"], "inline-registration");
        assert_eq!(response["result"], Value::Null);
        assert!(response.get("error").is_none());
    }

    #[test]
    fn singular_diagnostic_refresh_capability_is_admitted() {
        let request = json!({
            "jsonrpc": "2.0",
            "id": "diagnostic-refresh",
            "method": "workspace/diagnostic/refresh",
            "params": {}
        });
        let capabilities = capabilities_with(json!({
            "workspace": {
                "diagnostic": { "refreshSupport": true }
            }
        }));
        let response = server_request_response(&request, &capabilities).unwrap_or(Value::Null);

        assert_eq!(response["id"], "diagnostic-refresh");
        assert_eq!(response["result"], Value::Null);
        assert!(response.get("error").is_none());
    }

    #[test]
    fn standard_dynamic_registration_paths_are_admitted() {
        let request = json!({
            "jsonrpc": "2.0",
            "id": "standard-registration",
            "method": "client/registerCapability",
            "params": {
                "registrations": [
                    { "id": "sync", "method": "textDocument/didChange" },
                    { "id": "symbols", "method": "workspace/symbol" },
                    { "id": "files", "method": "workspace/didCreateFiles",
                      "registerOptions": { "filters": [{ "pattern": { "glob": "**/*.pl" } }] } }
                ]
            }
        });
        let capabilities = capabilities_with(json!({
            "textDocument": {
                "synchronization": { "dynamicRegistration": true }
            },
            "workspace": {
                "symbol": { "dynamicRegistration": true },
                "fileOperations": {
                    "dynamicRegistration": true,
                    "didCreate": true
                }
            }
        }));
        let response = server_request_response(&request, &capabilities).unwrap_or(Value::Null);

        assert_eq!(response["id"], "standard-registration");
        assert_eq!(response["result"], Value::Null);
        assert!(response.get("error").is_none());
    }

    #[test]
    fn file_operation_registration_requires_both_capabilities() -> Result<()> {
        for operation in
            ["didCreate", "willCreate", "didRename", "willRename", "didDelete", "willDelete"]
        {
            for (dynamic, supported, expected_missing) in [
                (Some(true), Some(true), None),
                (Some(true), None, Some(operation)),
                (Some(true), Some(false), Some(operation)),
                (None, Some(true), Some("dynamicRegistration")),
                (Some(false), Some(true), Some("dynamicRegistration")),
            ] {
                let mut file_operations = serde_json::Map::new();
                if let Some(value) = dynamic {
                    file_operations.insert("dynamicRegistration".to_owned(), json!(value));
                }
                if let Some(value) = supported {
                    file_operations.insert(operation.to_owned(), json!(value));
                }
                let request = json!({
                    "jsonrpc": "2.0",
                    "id": "file-operation",
                    "method": "client/registerCapability",
                    "params": { "registrations": [{
                        "id": "files", "method": format!("workspace/{operation}Files"),
                        "registerOptions": { "filters": [{ "pattern": { "glob": "**/*.pl" } }] }
                    }] }
                });
                let capabilities = capabilities_with(json!({
                    "workspace": { "fileOperations": file_operations }
                }));
                let response = server_request_response(&request, &capabilities)
                    .ok_or_else(|| anyhow!("missing registration response for {operation}"))?;
                if response.get("id") != Some(&json!("file-operation")) {
                    return Err(anyhow!("registration response lost its request identity"));
                }
                if let Some(missing) = expected_missing {
                    let path = format!("workspace.fileOperations.{missing}");
                    if response.pointer("/error/code") != Some(&json!(-32601))
                        || !response
                            .pointer("/error/message")
                            .and_then(Value::as_str)
                            .is_some_and(|message| message.contains(&path))
                    {
                        return Err(anyhow!("{operation} must reject missing {path}: {response}"));
                    }
                } else if response.get("result") != Some(&Value::Null)
                    || response.get("error").is_some()
                {
                    return Err(anyhow!("{operation} must admit both capabilities: {response}"));
                }
            }
        }
        Ok(())
    }

    #[test]
    fn boolean_capability_gates_reject_malformed_advertisements() -> Result<()> {
        let file_registration = json!({ "registrations": [{
            "id": "files", "method": "workspace/didCreateFiles",
            "registerOptions": { "filters": [{ "pattern": { "glob": "**/*.pl" } }] }
        }] });
        for (method, path, params) in [
            ("workspace/configuration", "workspace.configuration", json!({ "items": [] })),
            ("workspace/codeLens/refresh", "workspace.codeLens.refreshSupport", Value::Null),
            ("window/workDoneProgress/create", "window.workDoneProgress", json!({ "token": "t" })),
            (
                "window/showDocument",
                "window.showDocument.support",
                json!({ "uri": "file:///tmp/a.pl" }),
            ),
            (
                "client/registerCapability",
                "textDocument.completion.dynamicRegistration",
                json!({
                    "registrations": [{ "id": "completion", "method": "textDocument/completion",
                        "registerOptions": { "documentSelector": [{ "language": "perl" }] } }]
                }),
            ),
            (
                "client/registerCapability",
                "workspace.fileOperations.dynamicRegistration",
                file_registration.clone(),
            ),
            ("client/registerCapability", "workspace.fileOperations.didCreate", file_registration),
        ] {
            for value in [
                None,
                Some(json!(true)),
                Some(json!(false)),
                Some(json!({})),
                Some(json!([])),
                Some(Value::Null),
                Some(json!("true")),
                Some(json!(1)),
            ] {
                let mut capabilities = match path {
                    "workspace.fileOperations.dynamicRegistration" => {
                        json!({ "workspace": { "fileOperations": { "didCreate": true } } })
                    }
                    "workspace.fileOperations.didCreate" => {
                        json!({ "workspace": { "fileOperations": { "dynamicRegistration": true } } })
                    }
                    _ => json!({}),
                };
                let allowed = value == Some(json!(true));
                if let Some(leaf) = value {
                    let nested = path.rsplit('.').fold(leaf, |child, key| {
                        let mut object = serde_json::Map::new();
                        object.insert(key.to_owned(), child);
                        Value::Object(object)
                    });
                    super::merge_json(&mut capabilities, &nested);
                }
                let request = json!({ "jsonrpc": "2.0", "id": "typed-capability",
                    "method": method, "params": params });
                let decision = server_request_decision(&request, &capabilities)
                    .ok_or_else(|| anyhow!("missing decision for {method}"))?;
                if allowed {
                    if decision.capability_violation.is_some()
                        || decision.response.get("result").is_none()
                    {
                        return Err(anyhow!(
                            "literal true must permit {path}: {}",
                            decision.response
                        ));
                    }
                } else {
                    let violation_on_path = decision
                        .capability_violation
                        .as_ref()
                        .is_some_and(|v| v.capability == path);
                    if !violation_on_path
                        || decision.response.pointer("/error/code") != Some(&json!(-32601))
                    {
                        return Err(anyhow!(
                            "non-true advertisement must reject {path}: {}",
                            decision.response
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    #[test]
    fn show_message_capability_requires_a_structured_object() -> Result<()> {
        let request = json!({ "jsonrpc": "2.0", "id": "prompt", "method": "window/showMessageRequest",
            "params": { "type": 3, "message": "Continue?" } });
        for value in [json!({}), json!([]), json!(true), json!(false), Value::Null, json!("yes")] {
            let allowed = value.is_object();
            let capabilities = json!({ "window": { "showMessage": value } });
            let decision = server_request_decision(&request, &capabilities)
                .ok_or_else(|| anyhow!("missing prompt decision"))?;
            if allowed {
                if decision.response.get("result") != Some(&Value::Null)
                    || decision.capability_violation.is_some()
                {
                    return Err(anyhow!(
                        "structured prompt capability must permit conservative null response"
                    ));
                }
            } else if decision.capability_violation.is_none() {
                return Err(anyhow!("malformed structured prompt capability must be rejected"));
            }
        }
        Ok(())
    }

    #[test]
    fn dynamic_registration_requires_a_non_empty_id() {
        let request = json!({
            "jsonrpc": "2.0",
            "id": 15,
            "method": "client/registerCapability",
            "params": {
                "registrations": [{ "method": "textDocument/completion" }]
            }
        });
        let capabilities = capabilities_with(json!({
            "textDocument": { "completion": { "dynamicRegistration": true } }
        }));
        let response = server_request_response(&request, &capabilities).unwrap_or(Value::Null);

        assert_eq!(response["id"], 15);
        assert_eq!(response["error"]["code"], -32602);
        assert_eq!(
            response["error"]["message"],
            "Invalid params for client/registerCapability: every registration must include a string id"
        );
    }

    #[test]
    fn server_response_uses_lsp_content_length_framing() -> Result<()> {
        let response = json!({
            "jsonrpc": "2.0",
            "id": 11,
            "result": null
        });
        let body = response.to_string();
        let mut framed = Vec::new();

        write_framed_to(&mut framed, &response)?;

        let expected = format!("Content-Length: {}\r\n\r\n{body}", body.len());
        assert_eq!(framed, expected.as_bytes());
        Ok(())
    }
}
