//! LSP client for UX scenario tests.
//!
//! Spawns the real `perl-lsp` binary via stdio and communicates using the
//! JSON-RPC 2.0 / LSP Content-Length framing protocol.  All server-initiated
//! messages (`window/showMessage`, `window/logMessage`, diagnostic
//! notifications, etc.) are captured in an event queue so scenarios can
//! assert on user-visible messages after the fact.
// Test harness client — eprintln! echoes spawned server stderr for debugging.
#![allow(clippy::print_stderr)]

use crate::{FakeWorkspace, ScenarioConfig};
use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use server_request_script::{ObservedServerRequest, ScriptedServerRequest, ServerRequestScript};
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

pub mod server_request_script;

/// A server-initiated event captured during a scenario.
#[derive(Debug, Clone)]
pub enum LspEvent {
    /// `window/showMessage` — user-visible modal notification.
    WindowMessage {
        /// LSP MessageType (1=Error, 2=Warning, 3=Info, 4=Log).
        message_type: u32,
        /// The user-visible message text.
        message: String,
    },
    /// `window/logMessage` — IDE output panel message.
    LogMessage {
        /// LSP MessageType (1=Error, 2=Warning, 3=Info, 4=Log).
        message_type: u32,
        /// The output-panel message text.
        message: String,
    },
    /// `textDocument/publishDiagnostics` — diagnostic update.
    Diagnostics {
        /// The document URI.
        uri: String,
        /// The document version, when supplied.
        version: Option<i64>,
        /// The diagnostics reported for the document.
        diagnostics: Vec<Value>,
    },
    /// Any other server-initiated notification.
    Other {
        /// The notification method.
        method: String,
        /// The notification parameters.
        params: Value,
    },
}

/// A lightweight LSP client that speaks directly to a spawned perl-lsp process.
pub struct UxClient {
    child: Mutex<Child>,
    stdin: Arc<Mutex<Option<ChildStdin>>>,
    initialize_result: Value,
    /// Events buffered from the server's stdout (notifications, etc.)
    events: Arc<Mutex<VecDeque<Value>>>,
    /// Responses to requests (matched by id).
    responses: Arc<Mutex<VecDeque<Value>>>,
    /// Stderr lines captured from the server process.
    stderr_lines: Arc<Mutex<Vec<String>>>,
    script: Option<ServerRequestScript>,
    stdout_thread: Option<std::thread::JoinHandle<()>>,
    stderr_thread: Option<std::thread::JoinHandle<()>>,
}

impl UxClient {
    /// Spawn the perl-lsp binary and perform the LSP handshake
    /// (`initialize` + `initialized`).
    pub fn spawn(
        binary_path: &str,
        workspace: &FakeWorkspace,
        config: &ScenarioConfig,
    ) -> Result<Self> {
        let process = spawn_process(binary_path, config, None)?;
        let mut client = Self::from_process(process);

        // ── LSP handshake ─────────────────────────────────────────────────────
        client.initialize_result = client.handshake(workspace, config, config.timeout)?;

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
        let process = spawn_process(binary_path, &config, Some(script))?;
        let mut client = Self::from_process(process);
        client.initialize_result = client.scripted_handshake(root_uri, timeout)?;
        Ok(client)
    }

    fn from_process(process: SpawnedProcess) -> Self {
        Self {
            child: Mutex::new(process.child),
            stdin: process.stdin,
            initialize_result: Value::Null,
            events: process.events,
            responses: process.responses,
            stderr_lines: process.stderr_lines,
            script: process.script,
            stdout_thread: Some(process.stdout_thread),
            stderr_thread: Some(process.stderr_thread),
        }
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
        let raw: Vec<Value> = {
            let mut guard = self.events.lock().unwrap_or_else(|e| e.into_inner());
            guard.drain(..).collect()
        };
        raw.into_iter().map(decode_event).collect()
    }

    /// Clone and decode all buffered events **without** removing them from the
    /// queue.  Safe to call before or after `drain_events` / `collect_notifications`.
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
        let stdin = stdin.as_mut().ok_or_else(|| anyhow!("LSP client stdin is already closed"))?;
        write_framed(stdin, msg)
    }

    fn wait_for_response(&self, id: u64, timeout: Duration) -> Result<Value> {
        let deadline = Instant::now() + timeout;
        loop {
            {
                let mut guard = self.responses.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(pos) =
                    guard.iter().position(|v| v["id"].as_u64() == Some(id) || v["id"] == json!(id))
                {
                    // pos is valid since we just found it
                    if let Some(msg) = guard.remove(pos) {
                        return Ok(msg);
                    }
                }
            }
            if Instant::now() >= deadline {
                return Err(anyhow!(
                    "Timeout waiting for LSP response to id={} after {}ms",
                    id,
                    timeout.as_millis()
                ));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

struct SpawnedProcess {
    child: Child,
    stdin: Arc<Mutex<Option<ChildStdin>>>,
    events: Arc<Mutex<VecDeque<Value>>>,
    responses: Arc<Mutex<VecDeque<Value>>>,
    stderr_lines: Arc<Mutex<Vec<String>>>,
    script: Option<ServerRequestScript>,
    stdout_thread: std::thread::JoinHandle<()>,
    stderr_thread: std::thread::JoinHandle<()>,
}

fn spawn_process(
    binary_path: &str,
    config: &ScenarioConfig,
    scripted_requests: Option<Vec<ScriptedServerRequest>>,
) -> Result<SpawnedProcess> {
    let mut cmd = build_command(binary_path, config)?;
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("Failed to spawn perl-lsp from {:?}", binary_path))?;
    let stdin = Arc::new(Mutex::new(Some(
        child.stdin.take().ok_or_else(|| anyhow!("perl-lsp stdin not available after spawn"))?,
    )));
    let stdout =
        child.stdout.take().ok_or_else(|| anyhow!("perl-lsp stdout not available after spawn"))?;
    let stderr =
        child.stderr.take().ok_or_else(|| anyhow!("perl-lsp stderr not available after spawn"))?;
    let events: Arc<Mutex<VecDeque<Value>>> = Arc::new(Mutex::new(VecDeque::new()));
    let responses: Arc<Mutex<VecDeque<Value>>> = Arc::new(Mutex::new(VecDeque::new()));
    let stderr_lines: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let (script, observer) = match scripted_requests {
        Some(script) => {
            let (script, observer) = ServerRequestScript::new(stdin.clone(), script)?;
            (Some(script), Some(observer))
        }
        None => (None, None),
    };

    let event_queue = events.clone();
    let response_queue = responses.clone();
    let stdout_thread = std::thread::Builder::new()
        .name("ux-lsp-stdout".into())
        .spawn(move || {
            let mut reader = BufReader::new(stdout);
            while let Ok(message) = read_one_message(&mut reader) {
                let has_id = message.get("id").is_some_and(|id| !id.is_null());
                let is_response =
                    has_id && (message.get("result").is_some() || message.get("error").is_some());
                if is_response {
                    if let Ok(mut queue) = response_queue.lock() {
                        queue.push_back(message);
                    }
                } else {
                    if has_id
                        && message.get("method").and_then(Value::as_str).is_some()
                        && let Some(observer) = observer.as_ref()
                    {
                        observer.observe(&message);
                    }
                    if let Ok(mut queue) = event_queue.lock() {
                        queue.push_back(message);
                    }
                }
            }
        })
        .context("Failed to spawn stdout reader thread")?;

    let echo = config.echo_stderr;
    let captured_stderr = stderr_lines.clone();
    let stderr_thread = std::thread::Builder::new()
        .name("ux-lsp-stderr".into())
        .spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if let Ok(mut lines) = captured_stderr.lock() {
                    lines.push(line.clone());
                }
                if echo {
                    eprintln!("[perl-lsp stderr] {}", line);
                }
            }
        })
        .context("Failed to spawn stderr drain thread")?;

    // Allow the server a moment to start before we send initialize.
    std::thread::sleep(Duration::from_millis(50));

    Ok(SpawnedProcess {
        child,
        stdin,
        events,
        responses,
        stderr_lines,
        script,
        stdout_thread,
        stderr_thread,
    })
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
        // Best-effort graceful shutdown.
        if let Ok(mut stdin) = self.stdin.lock()
            && let Some(stdin) = stdin.as_mut()
        {
            let shutdown = json!({
                "jsonrpc": "2.0",
                "id": 999998,
                "method": "shutdown",
                "params": {}
            });
            let exit = json!({"jsonrpc": "2.0", "method": "exit"});
            let _ = write_framed(stdin, &shutdown);
            let _ = write_framed(stdin, &exit);
        }
        if let Ok(mut stdin) = self.stdin.lock() {
            let _ = stdin.take();
        }
        // Wait briefly for graceful exit then force-kill.
        let mut exited = false;
        for _ in 0..50 {
            if let Ok(mut child) = self.child.lock()
                && child.try_wait().ok().flatten().is_some()
            {
                exited = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if !exited && let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(thread) = self.stdout_thread.take() {
            let _ = thread.join();
        }
        if let Some(thread) = self.stderr_thread.take() {
            let _ = thread.join();
        }
    }
}

// ── Message framing ───────────────────────────────────────────────────────────

fn write_framed(stdin: &mut ChildStdin, message: &Value) -> Result<()> {
    let body = message.to_string();
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    stdin.write_all(header.as_bytes()).context("Failed to write LSP header")?;
    stdin.write_all(body.as_bytes()).context("Failed to write LSP body")?;
    stdin.flush().context("Failed to flush LSP message")
}

fn read_one_message(reader: &mut impl BufRead) -> Result<Value> {
    // Parse LSP Content-Length headers.
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return Err(anyhow!("EOF reading LSP headers"));
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
