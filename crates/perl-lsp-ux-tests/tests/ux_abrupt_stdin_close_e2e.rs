//! Pin the abrupt client-transport death path: initialize and open a document,
//! then close stdin without `shutdown`/`exit`. The server must exit zero within
//! a bounded time (#16623 item 1).

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
const EXIT_TIMEOUT: Duration = Duration::from_secs(5);
const READER_TIMEOUT: Duration = Duration::from_secs(5);
const STDERR_TAIL_LINES: usize = 40;

struct AbruptCloseProcess {
    child: Child,
    stdin: Option<ChildStdin>,
    messages: Receiver<Result<Value, String>>,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
    stdout_thread: Option<JoinHandle<()>>,
    stderr_thread: Option<JoinHandle<()>>,
}

impl AbruptCloseProcess {
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

    fn response(&self, id: &Value, timeout: Duration) -> Result<Value> {
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
                    if message.get("id") == Some(id)
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
                    "server did not exit within {}ms after abrupt stdin close\n{}",
                    timeout.as_millis(),
                    self.render_stderr_tail()
                );
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn join_readers(&mut self, timeout: Duration) -> Result<()> {
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

impl Drop for AbruptCloseProcess {
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

fn file_uri(path: &Path) -> Result<String> {
    Url::from_file_path(path)
        .map(|uri| uri.to_string())
        .map_err(|()| anyhow!("failed to convert {} to a file URI", path.display()))
}

#[test]
fn stdio_server_exits_zero_on_abrupt_stdin_close_without_shutdown() -> Result<()> {
    ensure!(
        binary_available(),
        "perllsp binary is not available; build it first (`cargo build -p perllsp`)"
    );

    let binary = resolve_binary().context("UX binary became unavailable after preflight")?;
    let workspace = TempDir::new().context("failed to create isolated workspace")?;
    let source = workspace.path().join("main.pl");
    std::fs::write(&source, "use strict;\nuse warnings;\n1;\n").context("write fixture")?;
    let root_uri = file_uri(workspace.path())?;
    let source_uri = file_uri(&source)?;

    let mut server = AbruptCloseProcess::spawn(&binary, workspace.path())?;

    server.send(&json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "processId": null,
            "rootUri": root_uri,
            "capabilities": {}
        }
    }))?;
    let initialize_id = json!(1);
    let initialize = server.response(&initialize_id, INITIALIZE_TIMEOUT)?;
    ensure!(
        initialize.get("error").is_none_or(Value::is_null),
        "initialize returned an error: {initialize:#}"
    );

    server.send(&json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    }))?;

    server.send(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": source_uri,
                "languageId": "perl",
                "version": 1,
                "text": "use strict;\nuse warnings;\n1;\n"
            }
        }
    }))?;

    server.close_stdin();
    let status = server.wait_for_exit(EXIT_TIMEOUT)?;
    server.join_readers(READER_TIMEOUT)?;
    let stderr_tail = server.render_stderr_tail();
    ensure!(
        status.success(),
        "server exited unsuccessfully after abrupt stdin close: {status}\n{stderr_tail}"
    );

    Ok(())
}
