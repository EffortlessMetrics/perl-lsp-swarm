//! Real-process lifecycle proof for the canonical UX test lane.
//!
//! The shared harness deliberately force-kills a server that does not exit during
//! `Drop`. That is safe cleanup, but it cannot prove that the product honors the
//! normal LSP `shutdown` -> `exit` lifecycle. This test owns the process directly
//! and treats forced termination as failure cleanup only.

use anyhow::{Context, Result, anyhow, bail, ensure};
use perl_lsp_ux_tests::{binary_available, resolve_binary};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tempfile::TempDir;
use url::Url;

const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(30);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const EXIT_TIMEOUT: Duration = Duration::from_secs(5);
const READER_TIMEOUT: Duration = Duration::from_secs(5);
const STDERR_TAIL_LINES: usize = 40;

struct LifecycleProcess {
    child: Child,
    stdin: Option<ChildStdin>,
    messages: Receiver<Result<Value, String>>,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
    stdout_thread: Option<JoinHandle<()>>,
    stderr_thread: Option<JoinHandle<()>>,
}

impl LifecycleProcess {
    fn spawn(binary_path: &str, workspace: &Path) -> Result<Self> {
        let executable = canonical_executable(binary_path)?;
        let mut child = Command::new(&executable)
            .arg("--stdio")
            .current_dir(workspace)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("failed to spawn {} --stdio", executable.display()))?;

        let stdin = child.stdin.take().context("spawned server did not expose stdin")?;
        let stdout = child.stdout.take().context("spawned server did not expose stdout")?;
        let stderr = child.stderr.take().context("spawned server did not expose stderr")?;

        let (message_tx, messages) = mpsc::channel();
        let stdout_thread = thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                match read_message(&mut reader) {
                    Ok(Some(message)) => {
                        if message_tx.send(Ok(message)).is_err() {
                            return;
                        }
                    }
                    Ok(None) => return,
                    Err(error) => {
                        let _ = message_tx.send(Err(error.to_string()));
                        return;
                    }
                }
            }
        });

        let stderr_tail = Arc::new(Mutex::new(VecDeque::new()));
        let stderr_tail_for_thread = Arc::clone(&stderr_tail);
        let stderr_thread = thread::spawn(move || {
            for line in BufReader::new(stderr).lines() {
                let Ok(line) = line else {
                    return;
                };
                let mut tail =
                    stderr_tail_for_thread.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                if tail.len() >= STDERR_TAIL_LINES {
                    let _ = tail.pop_front();
                }
                tail.push_back(line);
            }
        });

        Ok(Self {
            child,
            stdin: Some(stdin),
            messages,
            stderr_tail,
            stdout_thread: Some(stdout_thread),
            stderr_thread: Some(stderr_thread),
        })
    }

    fn send(&mut self, message: &Value) -> Result<()> {
        let body = message.to_string();
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        let stdin = self.stdin.as_mut().context("server stdin is already closed")?;
        stdin.write_all(header.as_bytes()).context("failed to write LSP header")?;
        stdin.write_all(body.as_bytes()).context("failed to write LSP body")?;
        stdin.flush().context("failed to flush LSP message")
    }

    fn response(&self, id: u64, timeout: Duration) -> Result<Value> {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                bail!(
                    "timed out after {}ms waiting for response id={id}\n{}",
                    timeout.as_millis(),
                    self.render_stderr_tail()
                );
            }

            match self.messages.recv_timeout(remaining.min(Duration::from_millis(250))) {
                Ok(Ok(message))
                    if message.get("id").and_then(Value::as_u64) == Some(id)
                        && (message.get("result").is_some() || message.get("error").is_some()) =>
                {
                    ensure!(
                        message.get("jsonrpc").and_then(Value::as_str) == Some("2.0"),
                        "response id={id} did not carry the JSON-RPC 2.0 envelope: \
                         {message:#}\n{}",
                        self.render_stderr_tail()
                    );
                    return Ok(message);
                }
                Ok(Ok(_)) => {}
                Ok(Err(error)) => {
                    bail!(
                        "server stdout reader failed before response id={id}: {error}\n{}",
                        self.render_stderr_tail()
                    );
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    bail!(
                        "server stdout reader disconnected before response id={id}\n{}",
                        self.render_stderr_tail()
                    );
                }
            }
        }
    }

    fn strict_response(&self, id: u64, timeout: Duration) -> Result<Value> {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                bail!(
                    "timed out after {}ms waiting for strict response id={id}\n{}",
                    timeout.as_millis(),
                    self.render_stderr_tail()
                );
            }

            match self.messages.recv_timeout(remaining.min(Duration::from_millis(250))) {
                Ok(Ok(message))
                    if message.get("result").is_some() || message.get("error").is_some() =>
                {
                    let observed_id = message.get("id").and_then(Value::as_u64);
                    ensure!(
                        observed_id == Some(id),
                        "unexpected terminal response while waiting for id={id}: {message:#}"
                    );
                    ensure!(
                        message.get("jsonrpc").and_then(Value::as_str) == Some("2.0"),
                        "response id={id} did not carry the JSON-RPC 2.0 envelope: \
                         {message:#}\n{}",
                        self.render_stderr_tail()
                    );
                    return Ok(message);
                }
                Ok(Ok(_)) => {}
                Ok(Err(error)) => {
                    bail!(
                        "server stdout reader failed before strict response id={id}: {error}\n{}",
                        self.render_stderr_tail()
                    );
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    bail!(
                        "server stdout reader disconnected before strict response id={id}\n{}",
                        self.render_stderr_tail()
                    );
                }
            }
        }
    }

    fn notification_for_uri_version(
        &self,
        method: &str,
        uri: &str,
        version: i64,
        timeout: Duration,
    ) -> Result<Value> {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                bail!(
                    "timed out after {}ms waiting for {method} for {uri}\n{}",
                    timeout.as_millis(),
                    self.render_stderr_tail()
                );
            }
            match self.messages.recv_timeout(remaining.min(Duration::from_millis(250))) {
                Ok(Ok(message))
                    if message.get("method").and_then(Value::as_str) == Some(method)
                        && message.pointer("/params/uri").and_then(Value::as_str) == Some(uri)
                        && message.pointer("/params/version").and_then(Value::as_i64)
                            == Some(version) =>
                {
                    return Ok(message);
                }
                Ok(Ok(message))
                    if message.get("result").is_some() || message.get("error").is_some() =>
                {
                    bail!("unexpected terminal response while waiting for {method}: {message:#}");
                }
                Ok(Ok(_)) => {}
                Ok(Err(error)) => {
                    bail!("server stdout reader failed waiting for {method}: {error}")
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    bail!("server stdout reader disconnected waiting for {method}");
                }
            }
        }
    }

    fn close_stdin(&mut self) {
        let _ = self.stdin.take();
    }

    fn wait_for_exit(&mut self, timeout: Duration) -> Result<ExitStatus> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) =
                self.child.try_wait().context("failed to inspect server exit status")?
            {
                return Ok(status);
            }
            if Instant::now() >= deadline {
                bail!(
                    "server did not exit within {}ms after the exit notification\n{}",
                    timeout.as_millis(),
                    self.render_stderr_tail()
                );
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn join_readers(&mut self, timeout: Duration) -> Result<()> {
        self.join_reader_threads(timeout)?;

        while let Ok(message) = self.messages.try_recv() {
            if let Err(error) = message {
                bail!("server emitted invalid LSP output: {error}\n{}", self.render_stderr_tail());
            }
        }
        Ok(())
    }

    fn join_readers_strict(&mut self, timeout: Duration) -> Result<()> {
        self.join_reader_threads(timeout)?;

        while let Ok(message) = self.messages.try_recv() {
            match message {
                Ok(message)
                    if message.get("result").is_some() || message.get("error").is_some() =>
                {
                    bail!("unexpected terminal response after expected responses: {message:#}");
                }
                Ok(_) => {}
                Err(error) => {
                    bail!(
                        "server emitted invalid LSP output: {error}\n{}",
                        self.render_stderr_tail()
                    );
                }
            }
        }
        Ok(())
    }

    fn join_reader_threads(&mut self, timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;
        loop {
            let stdout_finished =
                self.stdout_thread.as_ref().is_none_or(|handle| handle.is_finished());
            let stderr_finished =
                self.stderr_thread.as_ref().is_none_or(|handle| handle.is_finished());
            if stdout_finished && stderr_finished {
                break;
            }
            if Instant::now() >= deadline {
                bail!(
                    "server exited but its stdio reader threads did not finish within {}ms",
                    timeout.as_millis()
                );
            }
            thread::sleep(Duration::from_millis(10));
        }
        if let Some(handle) = self.stdout_thread.take() {
            handle.join().map_err(|_| anyhow!("stdout reader thread panicked"))?;
        }
        if let Some(handle) = self.stderr_thread.take() {
            handle.join().map_err(|_| anyhow!("stderr reader thread panicked"))?;
        }
        Ok(())
    }

    fn render_stderr_tail(&self) -> String {
        let tail = self.stderr_tail.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if tail.is_empty() {
            return "server stderr tail: <empty>".to_string();
        }

        let mut rendered = String::from("server stderr tail:\n");
        for line in tail.iter() {
            rendered.push_str("  | ");
            rendered.push_str(line);
            rendered.push('\n');
        }
        rendered
    }
}

impl Drop for LifecycleProcess {
    fn drop(&mut self) {
        let _ = self.stdin.take();
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn canonical_executable(binary_path: &str) -> Result<PathBuf> {
    let candidate = Path::new(binary_path);
    if candidate.components().count() == 1
        && let Ok(path) = which::which(candidate)
    {
        return Ok(path);
    }
    std::fs::canonicalize(candidate)
        .with_context(|| format!("failed to resolve perl-lsp binary path {binary_path}"))
}

fn read_message(reader: &mut impl BufRead) -> Result<Option<Value>> {
    let mut content_length = None;
    let mut saw_header = false;
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line).context("failed to read LSP header")?;
        if read == 0 {
            if saw_header {
                bail!("EOF while reading LSP headers");
            }
            return Ok(None);
        }
        saw_header = true;

        let header = line.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            content_length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .with_context(|| format!("invalid Content-Length header: {header}"))?,
            );
        }
    }

    let length = content_length.context("LSP frame omitted Content-Length")?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body).context("failed to read complete LSP body")?;
    serde_json::from_slice(&body).map(Some).context("failed to parse LSP JSON body")
}

#[test]
fn stdio_lifecycle_exits_zero_after_shutdown() -> Result<()> {
    // This test owns the lifecycle proof for the clean stdio exit claim, so a
    // missing binary must fail loudly instead of silently skipping: a green
    // result here is only meaningful if the product binary actually ran.
    ensure!(
        binary_available(),
        "perllsp binary is not available; the stdio lifecycle proof cannot run. \
         Build it first (`cargo build -p perllsp`) or run `just ux-tests`."
    );

    let binary = resolve_binary().context("UX binary became unavailable after preflight")?;
    let workspace = TempDir::new().context("failed to create isolated lifecycle workspace")?;
    let root_uri = Url::from_directory_path(workspace.path())
        .map_err(|()| anyhow!("failed to convert workspace path to file URI"))?
        .to_string();
    let mut server = LifecycleProcess::spawn(&binary, workspace.path())?;

    server.send(&json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "processId": null,
            "rootUri": root_uri,
            "workspaceFolders": null,
            "capabilities": {}
        }
    }))?;
    let initialize = server.response(1, INITIALIZE_TIMEOUT)?;
    ensure!(
        initialize.get("error").is_none_or(Value::is_null),
        "initialize returned an error: {initialize:#}"
    );
    ensure!(
        initialize.pointer("/result/capabilities").is_some_and(Value::is_object),
        "initialize did not return a capabilities object: {initialize:#}"
    );

    server.send(&json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    }))?;
    server.send(&json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "shutdown",
        "params": null
    }))?;

    let shutdown = server.response(2, REQUEST_TIMEOUT)?;
    ensure!(
        shutdown.get("error").is_none_or(Value::is_null),
        "shutdown returned an error: {shutdown:#}"
    );
    ensure!(
        shutdown.get("result").is_some_and(Value::is_null),
        "shutdown must return a null result: {shutdown:#}"
    );

    server.send(&json!({
        "jsonrpc": "2.0",
        "method": "exit",
        "params": null
    }))?;

    // Hold stdin open until the child has actually terminated. A conformant
    // server must exit on the `exit` notification itself; closing stdin here
    // would let a server that only exits on stdin EOF pass as clean exit.
    // `Drop` still closes stdin during failed-test cleanup.
    let status = server.wait_for_exit(EXIT_TIMEOUT)?;
    server.close_stdin();
    server.join_readers(READER_TIMEOUT)?;
    let stderr_tail = server.render_stderr_tail();
    ensure!(
        status.success(),
        "server exited unsuccessfully after shutdown -> exit: {status}\n{stderr_tail}"
    );

    Ok(())
}

const NAVIGATION_MODULE_V1: &str = "package Target;\nsub old_target { 1 }\n1;\n";
const NAVIGATION_MODULE_V2: &str = "package Target;\n\nsub old_target { 1 }\n1;\n";
const NAVIGATION_CLIENT_V1: &str = "use lib 'lib';\nuse Target;\nTarget::old_target();\n";

fn file_uri(path: &Path) -> Result<String> {
    Url::from_file_path(path)
        .map(|uri| uri.to_string())
        .map_err(|()| anyhow!("failed to convert {} to a file URI", path.display()))
}

fn exact_definition(response: &Value, expected_uri: &str, expected_line: u64) -> Result<()> {
    ensure!(response.get("error").is_none_or(Value::is_null), "definition failed: {response:#}");
    let result = response
        .get("result")
        .and_then(Value::as_array)
        .context("definition response result was not an array")?;
    ensure!(result.len() == 1, "expected one definition, got {result:#?}");
    let location = result.first().context("definition result was empty")?;
    let uri = location
        .pointer("/uri")
        .and_then(Value::as_str)
        .context("definition result omitted uri")?;
    let line = location
        .pointer("/range/start/line")
        .and_then(Value::as_u64)
        .context("definition result omitted range start line")?;
    let start_character = location
        .pointer("/range/start/character")
        .and_then(Value::as_u64)
        .context("definition result omitted range start character")?;
    let end_character = location
        .pointer("/range/end/character")
        .and_then(Value::as_u64)
        .context("definition result omitted range end character")?;
    let end_line = location
        .pointer("/range/end/line")
        .and_then(Value::as_u64)
        .context("definition result omitted range end line")?;
    ensure!(uri == expected_uri, "definition URI drifted: expected {expected_uri}, got {uri}");
    ensure!(line == expected_line, "definition line drifted: expected {expected_line}, got {line}");
    ensure!(
        end_line == expected_line,
        "definition end line drifted: expected {expected_line}, got {end_line}"
    );
    ensure!(
        start_character == 0,
        "definition start column drifted: expected 0, got {start_character} ({location:#})"
    );
    ensure!(
        end_character == 20,
        "definition end column drifted: expected 20, got {end_character} ({location:#})"
    );
    Ok(())
}

fn definition_after_readiness(
    server: &mut LifecycleProcess,
    client_uri: &str,
    module_uri: &str,
    expected_line: u64,
    first_request_id: u64,
) -> Result<()> {
    for attempt in 0_u64..8 {
        let request_id = first_request_id + attempt;
        server.send(&json!({
            "jsonrpc": "2.0", "id": request_id, "method": "textDocument/definition", "params": {
                "textDocument": { "uri": client_uri }, "position": { "line": 2, "character": 10 }
            }
        }))?;
        let response = server.strict_response(request_id, REQUEST_TIMEOUT)?;
        let transient = response.pointer("/error/code").and_then(Value::as_i64) == Some(-32800)
            || response.pointer("/result").and_then(Value::as_array).is_some_and(Vec::is_empty);
        if transient {
            thread::sleep(Duration::from_millis(100));
            continue;
        }
        exact_definition(&response, module_uri, expected_line)?;
        return Ok(());
    }
    bail!("definition never produced the expected current result for {client_uri} -> {module_uri}")
}

#[test]
fn stdio_navigation_matches_exact_request_and_current_edit() -> Result<()> {
    ensure!(
        binary_available(),
        "perllsp binary is not available; build it before running exact-process proof"
    );
    let configured_binary = std::env::var("PERL_LSP_BIN").context(
        "exact-process navigation proof requires PERL_LSP_BIN to name the candidate binary",
    )?;
    ensure!(
        !configured_binary.trim().is_empty(),
        "PERL_LSP_BIN must not be empty for exact-process navigation proof"
    );
    let binary = canonical_executable(&configured_binary)?;
    ensure!(
        binary.is_file(),
        "PERL_LSP_BIN does not identify a regular executable: {}",
        binary.display()
    );
    let binary_path = binary.to_str().context("candidate binary path was not valid UTF-8")?;
    let workspace = TempDir::new().context("failed to create navigation workspace")?;
    let lib = workspace.path().join("lib");
    std::fs::create_dir_all(&lib).context("failed to create navigation lib directory")?;
    let module = lib.join("Target.pm");
    let client = workspace.path().join("main.pl");
    std::fs::write(&module, NAVIGATION_MODULE_V1).context("failed to write module fixture")?;
    std::fs::write(&client, NAVIGATION_CLIENT_V1).context("failed to write client fixture")?;
    let root_uri = file_uri(workspace.path())?;
    let module_uri = file_uri(&module)?;
    let client_uri = file_uri(&client)?;
    let mut server = LifecycleProcess::spawn(binary_path, workspace.path())?;

    server.send(&json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": { "processId": null, "rootUri": root_uri, "workspaceFolders": null, "capabilities": {} }
    }))?;
    let initialize = server.response(1, INITIALIZE_TIMEOUT)?;
    ensure!(
        initialize.get("error").is_none_or(Value::is_null),
        "initialize failed: {initialize:#}"
    );
    server.send(&json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }))?;
    server.send(&json!({
        "jsonrpc": "2.0", "method": "textDocument/didOpen", "params": {
            "textDocument": { "uri": module_uri, "languageId": "perl", "version": 1, "text": NAVIGATION_MODULE_V1 }
        }
    }))?;
    server.notification_for_uri_version(
        "textDocument/publishDiagnostics",
        &module_uri,
        1,
        REQUEST_TIMEOUT,
    )?;
    server.send(&json!({
        "jsonrpc": "2.0", "method": "textDocument/didOpen", "params": {
            "textDocument": { "uri": client_uri, "languageId": "perl", "version": 1, "text": NAVIGATION_CLIENT_V1 }
        }
    }))?;
    server.notification_for_uri_version(
        "textDocument/publishDiagnostics",
        &client_uri,
        1,
        REQUEST_TIMEOUT,
    )?;
    definition_after_readiness(&mut server, &client_uri, &module_uri, 1, 2)?;

    server.send(&json!({
        "jsonrpc": "2.0", "method": "textDocument/didChange", "params": {
            "textDocument": { "uri": module_uri, "version": 2 },
            "contentChanges": [{ "text": NAVIGATION_MODULE_V2 }]
        }
    }))?;
    server.notification_for_uri_version(
        "textDocument/publishDiagnostics",
        &module_uri,
        2,
        REQUEST_TIMEOUT,
    )?;
    let expected_current_line = 2;
    definition_after_readiness(&mut server, &client_uri, &module_uri, expected_current_line, 10)?;

    server.send(&json!({ "jsonrpc": "2.0", "id": 100, "method": "shutdown", "params": null }))?;
    let shutdown = server.strict_response(100, REQUEST_TIMEOUT)?;
    ensure!(shutdown.get("error").is_none_or(Value::is_null), "shutdown failed: {shutdown:#}");
    server.send(&json!({ "jsonrpc": "2.0", "method": "exit", "params": null }))?;
    let status = server.wait_for_exit(EXIT_TIMEOUT)?;
    server.close_stdin();
    server.join_readers_strict(READER_TIMEOUT)?;
    ensure!(status.success(), "navigation server exited unsuccessfully: {status}");
    Ok(())
}
