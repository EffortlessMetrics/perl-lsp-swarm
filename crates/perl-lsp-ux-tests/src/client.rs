//! LSP client for UX scenario tests.
//!
//! Spawns the real `perl-lsp` binary via stdio and communicates using the
//! JSON-RPC 2.0 / LSP Content-Length framing protocol. All server-initiated
//! messages (`window/showMessage`, `window/logMessage`, diagnostic
//! notifications, requests, etc.) are captured in an event queue so scenarios
//! can assert on user-visible behavior after the fact. Server-initiated
//! requests receive deterministic client responses so modern LSP flows cannot
//! leave the server waiting on the harness.
// Test harness client — eprintln! echoes spawned server stderr for debugging.
#![allow(clippy::print_stderr)]

use crate::{FakeWorkspace, ScenarioConfig};
use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

static NEXT_ID: AtomicU64 = AtomicU64::new(100);

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
    /// Any other server-initiated message.
    Other { method: String, params: Value },
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
    stdin: Arc<Mutex<ChildStdin>>,
    initialize_result: Value,
    /// Events buffered from the server's stdout (notifications, requests, etc.).
    events: Arc<Mutex<VecDeque<Value>>>,
    /// Responses to requests (matched by id).
    responses: Arc<Mutex<VecDeque<Value>>>,
    /// Stderr lines captured from the server process.
    stderr_lines: Arc<Mutex<Vec<String>>>,
    /// First terminal failure observed by the stdout transport loop.
    transport_error: Arc<Mutex<Option<String>>>,
    /// Capability-gating violations observed on server requests.
    capability_violations: Arc<Mutex<Vec<CapabilityViolation>>>,
    /// Set before the best-effort shutdown handshake so normal EOF is not a
    /// transport failure.
    closing: Arc<std::sync::atomic::AtomicBool>,
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
        let client_capabilities = build_client_capabilities(config);

        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("Failed to spawn perl-lsp from {binary_path:?}"))?;

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

        let stdin = Arc::new(Mutex::new(stdin));
        let events: Arc<Mutex<VecDeque<Value>>> = Arc::new(Mutex::new(VecDeque::new()));
        let responses: Arc<Mutex<VecDeque<Value>>> = Arc::new(Mutex::new(VecDeque::new()));
        let stderr_lines: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let transport_error: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let capability_violations: Arc<Mutex<Vec<CapabilityViolation>>> =
            Arc::new(Mutex::new(Vec::new()));
        let closing = Arc::new(std::sync::atomic::AtomicBool::new(false));

        // ── stdout reader thread ──────────────────────────────────────────────
        let stdin_clone = Arc::clone(&stdin);
        let ev_clone = Arc::clone(&events);
        let resp_clone = Arc::clone(&responses);
        let transport_error_clone = Arc::clone(&transport_error);
        let capability_violations_clone = Arc::clone(&capability_violations);
        let client_capabilities_clone = client_capabilities.clone();
        let closing_clone = Arc::clone(&closing);
        let _stdout_thread = std::thread::Builder::new()
            .name("ux-lsp-stdout".into())
            .spawn(move || {
                let mut reader = BufReader::new(stdout);
                loop {
                    let routed = route_next_stdout_message(
                        &mut reader,
                        &stdin_clone,
                        &ev_clone,
                        &resp_clone,
                        &capability_violations_clone,
                        &client_capabilities_clone,
                    );
                    if let Err(error) = routed {
                        record_transport_error(&transport_error_clone, &error, &closing_clone);
                        break;
                    }
                }
            })
            .context("Failed to spawn stdout reader thread")?;

        // ── stderr drain thread ───────────────────────────────────────────────
        let echo = config.echo_stderr;
        let stderr_clone = Arc::clone(&stderr_lines);
        let _stderr_thread = std::thread::Builder::new()
            .name("ux-lsp-stderr".into())
            .spawn(move || {
                let reader = BufReader::new(stderr);
                for l in reader.lines().map_while(Result::ok) {
                    if let Ok(mut guard) = stderr_clone.lock() {
                        guard.push(l.clone());
                    }
                    if echo {
                        eprintln!("[perl-lsp stderr] {l}");
                    }
                }
            })
            .context("Failed to spawn stderr drain thread")?;

        // Allow the server a moment to start before we send initialize.
        std::thread::sleep(Duration::from_millis(50));

        let mut client = Self {
            child: Mutex::new(child),
            stdin,
            initialize_result: Value::Null,
            events,
            responses,
            stderr_lines,
            transport_error,
            capability_violations,
            closing,
            _stdout_thread,
            _stderr_thread,
        };

        // ── LSP handshake ─────────────────────────────────────────────────────
        client.initialize_result =
            client.handshake(workspace, config, &client_capabilities, config.timeout)?;

        Ok(client)
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
            "capabilities": client_capabilities
        });
        if !workspace_folders.is_empty() {
            params["workspaceFolders"] = Value::Array(workspace_folders);
        }
        if !config.initialization_options.is_null() {
            params["initializationOptions"] = config.initialization_options.clone();
        }

        let init_resp = self.request("initialize", params, timeout)?;

        if let Some(err) = init_resp.get("error") {
            return Err(anyhow!("LSP initialize returned error: {err}"));
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
    /// After this call the internal queue is empty. Use `peek_events` if you
    /// need to inspect events without consuming them.
    pub fn drain_events(&self) -> Vec<LspEvent> {
        let raw: Vec<Value> = {
            let mut guard = self.events.lock().unwrap_or_else(|e| e.into_inner());
            guard.drain(..).collect()
        };
        raw.into_iter().map(decode_event).collect()
    }

    /// Clone and decode all buffered events **without** removing them from the
    /// queue. Safe to call before or after `drain_events` / `collect_notifications`.
    pub fn peek_events(&self) -> Vec<LspEvent> {
        let raw: Vec<Value> = {
            let guard = self.events.lock().unwrap_or_else(|e| e.into_inner());
            guard.iter().cloned().collect()
        };
        raw.into_iter().map(decode_event).collect()
    }

    /// Clone raw server-initiated messages without removing them from the queue.
    ///
    /// This preserves server request IDs and registration payloads for protocol
    /// smoke checks that need to assert exact JSON-RPC shapes.
    pub fn peek_raw_events(&self) -> Vec<Value> {
        let guard = self.events.lock().unwrap_or_else(|e| e.into_inner());
        guard.iter().cloned().collect()
    }

    /// Clone raw server-initiated requests without removing them from the queue.
    ///
    /// Requests remain observable after the client has sent its deterministic
    /// response, allowing scenarios to assert method, id, and params together.
    pub fn peek_server_requests(&self) -> Vec<Value> {
        let guard = self.events.lock().unwrap_or_else(|e| e.into_inner());
        guard.iter().filter(|message| is_server_request(message)).cloned().collect()
    }

    /// Clone all stderr lines captured from the server process.
    pub fn peek_stderr_lines(&self) -> Vec<String> {
        self.stderr_lines.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Return the terminal stdout transport failure, if one has occurred.
    ///
    /// Foreground request waits consume the same evidence so malformed frames,
    /// invalid JSON, and response-write failures fail fast instead of timing out.
    pub fn peek_transport_error(&self) -> Option<String> {
        self.transport_error.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Ask the child process to shut down and require a successful exit.
    ///
    /// This is intentionally available to focused process-level tests so they
    /// can distinguish a clean LSP shutdown from a merely dropped child.
    pub fn shutdown_and_wait(&self, timeout: Duration) -> Result<()> {
        self.closing.store(true, Ordering::Release);

        {
            let mut child = self.child.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(status) = child.try_wait().context("Failed to inspect perl-lsp exit")? {
                return require_clean_exit(status);
            }
        }

        self.send_shutdown_messages()?;
        let deadline = Instant::now() + timeout;
        loop {
            let status = {
                let mut child = self.child.lock().unwrap_or_else(|e| e.into_inner());
                child.try_wait().context("Failed to wait for perl-lsp exit")?
            };
            if let Some(status) = status {
                return require_clean_exit(status);
            }
            if Instant::now() >= deadline {
                return Err(anyhow!("Timed out waiting for perl-lsp clean exit"));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Clone capability violations observed by the transport loop.
    pub fn peek_capability_violations(&self) -> Vec<CapabilityViolation> {
        self.capability_violations.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Wait up to `timeout` for any `window/showMessage` containing `needle`.
    pub fn wait_for_message(&self, needle: &str, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            {
                let guard = self.events.lock().unwrap_or_else(|e| e.into_inner());
                for msg in guard.iter() {
                    let method = msg["method"].as_str().unwrap_or("");
                    if method == "window/showMessage" || method == "window/logMessage" {
                        let text = msg["params"]["message"].as_str().unwrap_or("");
                        if text.contains(needle) {
                            return true;
                        }
                    }
                }
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    fn send_raw(&self, msg: &Value) -> Result<()> {
        let mut stdin = self.stdin.lock().unwrap_or_else(|e| e.into_inner());
        write_lsp_message(&mut *stdin, msg)
    }

    fn wait_for_response(&self, id: u64, timeout: Duration) -> Result<Value> {
        wait_for_response_queue(&self.responses, &self.transport_error, id, timeout)
    }

    fn send_shutdown_messages(&self) -> Result<()> {
        let mut stdin = self.stdin.lock().unwrap_or_else(|e| e.into_inner());
        write_lsp_message(
            &mut *stdin,
            &json!({"jsonrpc":"2.0","id":999998,"method":"shutdown","params":{}}),
        )?;
        write_lsp_message(&mut *stdin, &json!({"jsonrpc":"2.0","method":"exit"}))?;
        Ok(())
    }
}

fn wait_for_response_queue(
    responses: &Mutex<VecDeque<Value>>,
    transport_error: &Mutex<Option<String>>,
    id: u64,
    timeout: Duration,
) -> Result<Value> {
    let deadline = Instant::now() + timeout;
    loop {
        {
            let mut guard = responses.lock().unwrap_or_else(|e| e.into_inner());
            let position = guard
                .iter()
                .position(|value| value["id"].as_u64() == Some(id) || value["id"] == json!(id));
            if let Some(msg) = position.and_then(|pos| guard.remove(pos)) {
                return Ok(msg);
            }
        }

        let transport_failure = transport_error.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(error) = transport_failure {
            return Err(anyhow!(
                "LSP stdout transport failed while waiting for response id={id}: {error}"
            ));
        }

        if Instant::now() >= deadline {
            return Err(anyhow!(
                "Timeout waiting for LSP response to id={id} after {}ms",
                timeout.as_millis()
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
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
        let _ = self.shutdown_and_wait(Duration::from_millis(500));
        if let Ok(mut child) = self.child.lock()
            && child.try_wait().ok().flatten().is_none()
        {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

// ── Message framing ───────────────────────────────────────────────────────────

fn read_one_message(reader: &mut impl BufRead) -> Result<Value> {
    // Parse LSP Content-Length headers.
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return Err(NormalEof.into());
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some(rest) = trimmed.to_ascii_lowercase().strip_prefix("content-length") {
            let rest = rest.trim_start_matches(':').trim();
            content_length = rest.parse::<usize>().ok();
        }
    }
    let len = content_length.ok_or_else(|| anyhow!("No Content-Length in LSP message"))?;
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body)?;
    serde_json::from_slice(&body).context("Failed to parse LSP JSON body")
}

#[derive(Debug)]
struct NormalEof;

impl std::fmt::Display for NormalEof {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("normal EOF")
    }
}

impl std::error::Error for NormalEof {}

fn record_transport_error(
    slot: &Mutex<Option<String>>,
    error: &anyhow::Error,
    closing: &std::sync::atomic::AtomicBool,
) {
    if is_normal_eof(error) && closing.load(Ordering::Acquire) {
        return;
    }
    let mut guard = slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if guard.is_none() {
        *guard = Some(format!("{error:#}"));
    }
}

fn is_normal_eof(error: &anyhow::Error) -> bool {
    error.downcast_ref::<NormalEof>().is_some()
}

fn require_clean_exit(status: std::process::ExitStatus) -> Result<()> {
    if status.success() { Ok(()) } else { Err(anyhow!("perl-lsp exited unsuccessfully: {status}")) }
}

fn write_lsp_message(writer: &mut impl Write, message: &Value) -> Result<()> {
    let body = message.to_string();
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    writer.write_all(header.as_bytes()).context("Failed to write LSP header to stdin")?;
    writer.write_all(body.as_bytes()).context("Failed to write LSP body to stdin")?;
    writer.flush().context("Failed to flush LSP stdin")?;
    Ok(())
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

fn route_next_stdout_message<R, W>(
    reader: &mut R,
    stdin: &Arc<Mutex<W>>,
    events: &Arc<Mutex<VecDeque<Value>>>,
    responses: &Arc<Mutex<VecDeque<Value>>>,
    capability_violations: &Arc<Mutex<Vec<CapabilityViolation>>>,
    capabilities: &Value,
) -> Result<()>
where
    R: BufRead,
    W: Write,
{
    let message = read_one_message(reader)?;
    let has_id = message.get("id").is_some_and(|id| !id.is_null());
    let has_result = message.get("result").is_some();
    let has_error = message.get("error").is_some();
    let is_response = has_id && (has_result || has_error);
    if is_response {
        responses.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push_back(message);
        return Ok(());
    }

    let decision = server_request_decision(&message, capabilities);
    let method_name = message.get("method").and_then(Value::as_str).unwrap_or("<missing>");
    let method = method_name.to_owned();
    let id = message.get("id").cloned().unwrap_or(Value::Null);
    events.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push_back(message);

    if let Some(decision) = decision {
        if let Some(violation) = decision.capability_violation {
            capability_violations
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(violation);
        }
        let mut stdin = stdin.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        write_lsp_message(&mut *stdin, &decision.response)
            .with_context(|| format!("Failed to answer server request method={method} id={id}"))?;
    }

    Ok(())
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
    match capabilities.pointer(&pointer) {
        Some(Value::Bool(value)) => *value,
        Some(Value::Object(_)) | Some(Value::Array(_)) => true,
        _ => false,
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
        let Some(capability) = registration_capability_path(method) else {
            return Some(DynamicRegistrationIssue::Malformed(format!(
                "unsupported dynamic registration method {method}"
            )));
        };
        if !capability_is_advertised(capabilities, capability) {
            return Some(DynamicRegistrationIssue::Capability(capability.to_owned()));
        }
    }
    None
}

fn registration_capability_path(method: &str) -> Option<&'static str> {
    let path = match method {
        "workspace/didChangeConfiguration" => {
            "workspace.didChangeConfiguration.dynamicRegistration"
        }
        "workspace/didChangeWatchedFiles" => "workspace.didChangeWatchedFiles.dynamicRegistration",
        "workspace/executeCommand" => "workspace.executeCommand.dynamicRegistration",
        "textDocument/completion" => "textDocument.completion.dynamicRegistration",
        "textDocument/inlineCompletion" => "textDocument.inlineCompletion.dynamicRegistration",
        "textDocument/hover" => "textDocument.hover.dynamicRegistration",
        "textDocument/definition" => "textDocument.definition.dynamicRegistration",
        "textDocument/declaration" => "textDocument.declaration.dynamicRegistration",
        "textDocument/typeDefinition" => "textDocument.typeDefinition.dynamicRegistration",
        "textDocument/implementation" => "textDocument.implementation.dynamicRegistration",
        "textDocument/references" => "textDocument.references.dynamicRegistration",
        "textDocument/documentHighlight" => "textDocument.documentHighlight.dynamicRegistration",
        "textDocument/documentSymbol" => "textDocument.documentSymbol.dynamicRegistration",
        "textDocument/codeAction" => "textDocument.codeAction.dynamicRegistration",
        "textDocument/codeLens" => "textDocument.codeLens.dynamicRegistration",
        "textDocument/documentLink" => "textDocument.documentLink.dynamicRegistration",
        "textDocument/documentColor" => "textDocument.colorProvider.dynamicRegistration",
        "textDocument/formatting" => "textDocument.formatting.dynamicRegistration",
        "textDocument/rangeFormatting" => "textDocument.rangeFormatting.dynamicRegistration",
        "textDocument/onTypeFormatting" => "textDocument.onTypeFormatting.dynamicRegistration",
        "textDocument/rename" => "textDocument.rename.dynamicRegistration",
        "textDocument/publishDiagnostics" => "textDocument.publishDiagnostics.dynamicRegistration",
        "textDocument/signatureHelp" => "textDocument.signatureHelp.dynamicRegistration",
        "textDocument/semanticTokens" => "textDocument.semanticTokens.dynamicRegistration",
        "textDocument/inlayHint" => "textDocument.inlayHint.dynamicRegistration",
        "textDocument/inlineValue" => "textDocument.inlineValue.dynamicRegistration",
        _ => return None,
    };
    Some(path)
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
mod tests {
    use super::*;

    struct BrokenWriter;

    impl Write for BrokenWriter {
        fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "synthetic broken pipe"))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn stdout_router_answers_server_request_preserves_evidence_and_keeps_routing() -> Result<()> {
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
        write_lsp_message(&mut server_stdout, &server_request)?;
        write_lsp_message(&mut server_stdout, &later_response)?;

        let mut reader = BufReader::new(server_stdout.as_slice());
        let stdin = Arc::new(Mutex::new(Vec::new()));
        let events = Arc::new(Mutex::new(VecDeque::new()));
        let responses = Arc::new(Mutex::new(VecDeque::new()));
        let violations = Arc::new(Mutex::new(Vec::new()));
        let capabilities = capabilities_with(json!({
            "workspace": { "configuration": true }
        }));

        route_next_stdout_message(
            &mut reader,
            &stdin,
            &events,
            &responses,
            &violations,
            &capabilities,
        )?;
        route_next_stdout_message(
            &mut reader,
            &stdin,
            &events,
            &responses,
            &violations,
            &capabilities,
        )?;

        let framed_response = stdin.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let mut response_reader = BufReader::new(framed_response.as_slice());
        let client_response = read_one_message(&mut response_reader)?;
        assert_eq!(client_response["id"], "server-17");
        assert_eq!(client_response["result"], json!([null, null]));

        let observed: Vec<Value> =
            events.lock().unwrap_or_else(|e| e.into_inner()).iter().cloned().collect();
        assert_eq!(observed, vec![server_request]);

        let queued: Vec<Value> =
            responses.lock().unwrap_or_else(|e| e.into_inner()).iter().cloned().collect();
        assert_eq!(queued, vec![later_response]);
        assert!(violations.lock().unwrap_or_else(|e| e.into_inner()).is_empty());
        Ok(())
    }

    #[test]
    fn stdout_router_reports_response_write_failure_with_request_identity() -> Result<()> {
        let request = json!({
            "jsonrpc": "2.0",
            "id": 33,
            "method": "window/workDoneProgress/create",
            "params": { "token": "index" }
        });
        let mut server_stdout = Vec::new();
        write_lsp_message(&mut server_stdout, &request)?;
        let mut reader = BufReader::new(server_stdout.as_slice());
        let stdin = Arc::new(Mutex::new(BrokenWriter));
        let events = Arc::new(Mutex::new(VecDeque::new()));
        let responses = Arc::new(Mutex::new(VecDeque::new()));
        let violations = Arc::new(Mutex::new(Vec::new()));
        let capabilities = capabilities_with(json!({
            "window": { "workDoneProgress": true }
        }));

        let failure = match route_next_stdout_message(
            &mut reader,
            &stdin,
            &events,
            &responses,
            &violations,
            &capabilities,
        ) {
            Ok(()) => return Err(anyhow!("broken writer unexpectedly accepted the response")),
            Err(error) => format!("{error:#}"),
        };
        assert!(failure.contains("method=window/workDoneProgress/create id=33"));
        assert!(failure.contains("synthetic broken pipe"));
        Ok(())
    }

    #[test]
    fn response_wait_surfaces_transport_failure_before_timeout() -> Result<()> {
        let responses = Mutex::new(VecDeque::new());
        let transport_error = Mutex::new(Some("Failed to parse LSP JSON body".to_owned()));

        let failure = match wait_for_response_queue(
            &responses,
            &transport_error,
            77,
            Duration::from_secs(30),
        ) {
            Ok(value) => return Err(anyhow!("unexpected response: {value}")),
            Err(error) => error.to_string(),
        };
        assert!(failure.contains("response id=77"));
        assert!(failure.contains("Failed to parse LSP JSON body"));
        Ok(())
    }

    #[test]
    fn normal_eof_is_only_ignored_after_shutdown_begins() -> Result<()> {
        let mut reader = BufReader::new(&b""[..]);
        let error = read_one_message(&mut reader).expect_err("empty stdout should be EOF");
        assert!(is_normal_eof(&error));

        let transport_error = Mutex::new(None);
        let closing = std::sync::atomic::AtomicBool::new(false);
        record_transport_error(&transport_error, &error, &closing);
        assert_eq!(
            transport_error.lock().unwrap_or_else(|e| e.into_inner()).as_deref(),
            Some("normal EOF")
        );

        let transport_error = Mutex::new(None);
        closing.store(true, Ordering::Release);
        record_transport_error(&transport_error, &error, &closing);
        assert!(transport_error.lock().unwrap_or_else(|e| e.into_inner()).is_none());
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

    fn capabilities_with(overrides: Value) -> Value {
        let config =
            ScenarioConfig { client_capability_overrides: overrides, ..ScenarioConfig::default() };
        build_client_capabilities(&config)
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

        write_lsp_message(&mut framed, &response)?;

        let expected = format!("Content-Length: {}\r\n\r\n{body}", body.len());
        assert_eq!(framed, expected.as_bytes());
        Ok(())
    }
}
