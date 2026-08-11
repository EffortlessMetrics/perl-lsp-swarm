#![allow(dead_code)]

//! Strict real-process LSP test client.
//!
//! Unlike the older compatibility harnesses, this client has one subject:
//! Cargo's exact `perl-lsp` binary for the current integration-test build. It
//! never falls through to PATH, a stale target directory, or `cargo run`.
//! Stdout is parsed as a strict Content-Length stream so an accidental log line
//! is a protocol failure rather than something the test reader silently skips.

use anyhow::{Context, Result, anyhow, bail, ensure};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tempfile::TempDir;

const EXACT_CANDIDATE: Option<&str> = option_env!("CARGO_BIN_EXE_perl-lsp");
const EVENT_CAPACITY: usize = 256;
const PENDING_CAPACITY: usize = 256;
const STDERR_CAPACITY: usize = 200;
const MAX_HEADER_BYTES: usize = 4 * 1024;
const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug)]
enum ProcessEvent {
    Message(Value),
    ProtocolError(String),
    Eof,
}

/// Strict client for the exact Cargo-built `perl-lsp` process.
pub struct RealProcessClient {
    child: Child,
    stdin: Option<BufWriter<ChildStdin>>,
    events: Receiver<ProcessEvent>,
    pending: VecDeque<Value>,
    stderr_lines: Arc<Mutex<VecDeque<String>>>,
    stdout_thread: Option<JoinHandle<()>>,
    stderr_thread: Option<JoinHandle<()>>,
    candidate_path: PathBuf,
    _workspace: TempDir,
    finished: bool,
}

impl RealProcessClient {
    /// Spawn Cargo's exact candidate binary in an isolated working directory.
    pub fn spawn_exact() -> Result<Self> {
        let candidate = EXACT_CANDIDATE.ok_or_else(|| {
            anyhow!(
                "Cargo did not provide CARGO_BIN_EXE_perl-lsp; run this as a perl-lsp-rs integration test"
            )
        })?;
        let candidate_path = PathBuf::from(candidate);
        ensure!(
            candidate_path.is_file(),
            "exact Cargo candidate does not exist or is not a file: {}",
            candidate_path.display()
        );

        let workspace = tempfile::tempdir().context("create isolated LSP process workspace")?;
        let mut child = Command::new(&candidate_path)
            .arg("--stdio")
            .current_dir(workspace.path())
            .env("PERL_LSP_QUIET", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("spawn exact candidate {}", candidate_path.display()))?;

        let stdin = child.stdin.take().ok_or_else(|| anyhow!("candidate stdin was not piped"))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow!("candidate stdout was not piped"))?;
        let stderr = child.stderr.take().ok_or_else(|| anyhow!("candidate stderr was not piped"))?;

        let (event_tx, events) = mpsc::sync_channel(EVENT_CAPACITY);
        let stdout_thread = std::thread::Builder::new()
            .name("lsp-exact-stdout".to_string())
            .spawn(move || read_stdout(stdout, event_tx))
            .context("spawn strict stdout reader")?;

        let stderr_lines = Arc::new(Mutex::new(VecDeque::new()));
        let stderr_sink = Arc::clone(&stderr_lines);
        let stderr_thread = std::thread::Builder::new()
            .name("lsp-exact-stderr".to_string())
            .spawn(move || drain_stderr(stderr, &stderr_sink))
            .context("spawn stderr reader")?;

        Ok(Self {
            child,
            stdin: Some(BufWriter::new(stdin)),
            events,
            pending: VecDeque::new(),
            stderr_lines,
            stdout_thread: Some(stdout_thread),
            stderr_thread: Some(stderr_thread),
            candidate_path,
            _workspace: workspace,
            finished: false,
        })
    }

    /// Exact binary path supplied by Cargo for this test build.
    pub fn candidate_path(&self) -> &Path {
        &self.candidate_path
    }

    /// Encode one JSON-RPC message using LSP Content-Length framing.
    pub fn encode_message(message: &Value) -> Vec<u8> {
        let body = message.to_string();
        let mut frame = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
        frame.extend_from_slice(body.as_bytes());
        frame
    }

    /// Send an already encoded byte sequence to the candidate stdin.
    pub fn send_raw_bytes(&mut self, bytes: &[u8]) -> Result<()> {
        ensure!(!self.finished, "cannot write after candidate exit");
        let stdin = self.stdin.as_mut().ok_or_else(|| anyhow!("candidate stdin is closed"))?;
        stdin.write_all(bytes).context("write candidate stdin")?;
        stdin.flush().context("flush candidate stdin")
    }

    /// Send one frame in caller-selected fragments.
    pub fn send_raw_chunks(&mut self, chunks: &[&[u8]]) -> Result<()> {
        for chunk in chunks {
            self.send_raw_bytes(chunk)?;
        }
        Ok(())
    }

    /// Send a JSON-RPC notification.
    pub fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        let message = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        self.send_raw_bytes(&Self::encode_message(&message))
    }

    /// Send a request with an explicit numeric or string ID and wait for its response.
    pub fn request(
        &mut self,
        id: Value,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value> {
        ensure!(
            id.is_number() || id.is_string(),
            "JSON-RPC request ID must be a number or string, got {id}"
        );
        let message = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let response_id = message["id"].clone();
        self.send_raw_bytes(&Self::encode_message(&message))?;
        self.receive_response(&response_id, timeout)
    }

    /// Wait for a response to a request that was sent through a raw frame.
    pub fn receive_response(&mut self, id: &Value, timeout: Duration) -> Result<Value> {
        if let Some(index) = self.pending.iter().position(|message| is_response_for(message, id)) {
            return self
                .pending
                .remove(index)
                .ok_or_else(|| anyhow!("matched pending response disappeared"));
        }

        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                bail!(
                    "timed out waiting for response id={id}; pending={:?}; stderr={}",
                    self.pending,
                    self.stderr_tail()
                );
            }

            match self.events.recv_timeout(remaining) {
                Ok(ProcessEvent::Message(message)) if is_response_for(&message, id) => {
                    return Ok(message);
                }
                Ok(ProcessEvent::Message(message)) => self.push_pending(message)?,
                Ok(ProcessEvent::ProtocolError(error)) => {
                    bail!("candidate stdout protocol error: {error}; stderr={}", self.stderr_tail())
                }
                Ok(ProcessEvent::Eof) => {
                    bail!(
                        "candidate closed stdout before response id={id}; stderr={}",
                        self.stderr_tail()
                    )
                }
                Err(RecvTimeoutError::Timeout) => {
                    bail!(
                        "timed out waiting for response id={id}; pending={:?}; stderr={}",
                        self.pending,
                        self.stderr_tail()
                    )
                }
                Err(RecvTimeoutError::Disconnected) => {
                    bail!("candidate stdout reader disconnected; stderr={}", self.stderr_tail())
                }
            }
        }
    }

    /// Assert that processing a prior notification did not fabricate a response with `id: null`.
    ///
    /// Call this after a later request has completed; the request acts as an
    /// ordered processing barrier for the prior notification.
    pub fn assert_no_null_id_response_pending(&self) -> Result<()> {
        let unexpected = self.pending.iter().find(|message| {
            is_response(message) && message.get("id").is_some_and(Value::is_null)
        });
        ensure!(
            unexpected.is_none(),
            "server replied to a notification with a null-ID response: {unexpected:?}"
        );
        Ok(())
    }

    /// Wait for the child to exit without silently killing it.
    pub fn wait_for_exit(&mut self, timeout: Duration) -> Result<ExitStatus> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.child.try_wait().context("poll candidate exit")? {
                self.finished = true;
                self.stdin.take();
                self.finish_reader_threads();
                return Ok(status);
            }
            if Instant::now() >= deadline {
                bail!(
                    "candidate did not exit within {timeout:?}; pid={}; stderr={}",
                    self.child.id(),
                    self.stderr_tail()
                );
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Drain terminal reader events and fail if stdout ever stopped being a strict frame stream.
    pub fn assert_transport_clean(&mut self) -> Result<()> {
        loop {
            match self.events.try_recv() {
                Ok(ProcessEvent::Message(message)) => self.push_pending(message)?,
                Ok(ProcessEvent::ProtocolError(error)) => {
                    bail!("candidate stdout protocol error: {error}; stderr={}", self.stderr_tail())
                }
                Ok(ProcessEvent::Eof) => {}
                Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected) => break,
            }
        }
        Ok(())
    }

    /// Bounded stderr tail for failure receipts.
    pub fn stderr_tail(&self) -> String {
        let lines = lock_lines(&self.stderr_lines);
        lines.iter().cloned().collect::<Vec<_>>().join("\n")
    }

    fn push_pending(&mut self, message: Value) -> Result<()> {
        ensure!(
            self.pending.len() < PENDING_CAPACITY,
            "pending server message capacity exceeded ({PENDING_CAPACITY})"
        );
        self.pending.push_back(message);
        Ok(())
    }

    fn finish_reader_threads(&mut self) {
        if let Some(thread) = self.stdout_thread.take() {
            let _ = thread.join();
        }
        if let Some(thread) = self.stderr_thread.take() {
            let _ = thread.join();
        }
    }

    fn force_cleanup(&mut self) {
        self.stdin.take();
        match self.child.try_wait() {
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => {
                let _ = self.child.kill();
                let _ = self.child.wait();
            }
        }
        self.finished = true;
        self.finish_reader_threads();
    }
}

impl Drop for RealProcessClient {
    fn drop(&mut self) {
        if !self.finished {
            self.force_cleanup();
        } else {
            self.finish_reader_threads();
        }
    }
}

fn is_response(message: &Value) -> bool {
    message.get("method").is_none()
        && (message.get("result").is_some() || message.get("error").is_some())
}

fn is_response_for(message: &Value, id: &Value) -> bool {
    is_response(message) && message.get("id") == Some(id)
}

fn read_stdout(stdout: ChildStdout, sender: SyncSender<ProcessEvent>) {
    let mut reader = BufReader::new(stdout);
    loop {
        match read_frame(&mut reader) {
            Ok(Some(message)) => {
                if sender.send(ProcessEvent::Message(message)).is_err() {
                    return;
                }
            }
            Ok(None) => {
                let _ = sender.send(ProcessEvent::Eof);
                return;
            }
            Err(error) => {
                let _ = sender.send(ProcessEvent::ProtocolError(error.to_string()));
                return;
            }
        }
    }
}

fn read_frame(reader: &mut dyn BufRead) -> Result<Option<Value>> {
    let mut content_length = None;
    let mut header_bytes = 0usize;

    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line).context("read candidate stdout header")?;
        if read == 0 {
            return Ok(None);
        }
        header_bytes = header_bytes.saturating_add(read);
        ensure!(
            header_bytes <= MAX_HEADER_BYTES,
            "candidate stdout header exceeded {MAX_HEADER_BYTES} bytes"
        );

        let line = line.trim_end_matches(|ch| ch == '\r' || ch == '\n');
        if line.is_empty() {
            break;
        }

        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| anyhow!("non-header stdout line before frame: {line:?}"))?;
        let name = name.trim();
        let value = value.trim();
        if name.eq_ignore_ascii_case("Content-Length") {
            ensure!(content_length.is_none(), "duplicate Content-Length header from candidate");
            let parsed = value
                .parse::<usize>()
                .with_context(|| format!("invalid Content-Length value {value:?}"))?;
            ensure!(parsed <= MAX_FRAME_BYTES, "candidate frame exceeded {MAX_FRAME_BYTES} bytes");
            content_length = Some(parsed);
        } else if !name.eq_ignore_ascii_case("Content-Type") {
            bail!("unexpected candidate stdout header {name:?}; stdout must contain LSP frames only");
        }
    }

    let length = content_length.ok_or_else(|| anyhow!("candidate frame omitted Content-Length"))?;
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body).context("read candidate stdout frame body")?;
    let message: Value = serde_json::from_slice(&body).with_context(|| {
        format!("candidate emitted non-JSON frame body: {:?}", String::from_utf8_lossy(&body))
    })?;
    ensure!(message.is_object(), "candidate emitted non-object JSON-RPC message: {message}");
    Ok(Some(message))
}

fn drain_stderr(stderr: std::process::ChildStderr, sink: &Arc<Mutex<VecDeque<String>>>) {
    let mut reader = BufReader::new(stderr);
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {
                let line = line.trim_end_matches(|ch| ch == '\r' || ch == '\n').to_string();
                let mut lines = lock_lines(sink);
                if lines.len() == STDERR_CAPACITY {
                    lines.pop_front();
                }
                lines.push_back(line);
            }
        }
    }
}

fn lock_lines(lines: &Arc<Mutex<VecDeque<String>>>) -> std::sync::MutexGuard<'_, VecDeque<String>> {
    match lines.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}
