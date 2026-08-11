#![allow(dead_code)]

//! Strict real-process LSP test client.
//!
//! This client runs only Cargo's exact `perl-lsp` integration-test binary. It
//! never falls through to PATH, a target directory, or `cargo run`. Stdout is a
//! bounded, strict Content-Length stream; stderr is retained separately as a
//! bounded byte tail.

use anyhow::{Context, Result, anyhow, bail, ensure};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{
    self, Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError,
};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tempfile::TempDir;

const EXACT_CANDIDATE: Option<&str> = option_env!("CARGO_BIN_EXE_perl-lsp");
const EVENT_CAPACITY: usize = 256;
const PENDING_CAPACITY: usize = 256;
const STDERR_BYTE_CAPACITY: usize = 32 * 1024;
const MAX_HEADER_BYTES: usize = 4 * 1024;
const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug)]
enum ProcessEvent {
    Message(Value),
    ProtocolError(String),
    Eof,
}

pub struct RealProcessClient {
    child: Child,
    stdin: Option<BufWriter<ChildStdin>>,
    events: Receiver<ProcessEvent>,
    pending: VecDeque<Value>,
    event_overflow: Arc<AtomicBool>,
    stderr_bytes: Arc<Mutex<VecDeque<u8>>>,
    stderr_truncated: Arc<AtomicBool>,
    stdout_thread: Option<JoinHandle<()>>,
    stderr_thread: Option<JoinHandle<()>>,
    candidate_path: PathBuf,
    _workspace: TempDir,
    finished: bool,
}

impl RealProcessClient {
    pub fn spawn_exact() -> Result<Self> {
        let candidate = EXACT_CANDIDATE.ok_or_else(|| {
            anyhow!(
                "Cargo did not provide CARGO_BIN_EXE_perl-lsp; \
                 run this as a perl-lsp-rs integration test"
            )
        })?;
        let candidate_path = PathBuf::from(candidate);
        ensure!(
            candidate_path.is_file(),
            "exact Cargo candidate is not a file: {}",
            candidate_path.display()
        );

        let workspace = tempfile::tempdir()
            .context("create isolated LSP process workspace")?;
        let mut child = Command::new(&candidate_path)
            .arg("--stdio")
            .current_dir(workspace.path())
            .env("PERL_LSP_QUIET", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| {
                format!("spawn exact candidate {}", candidate_path.display())
            })?;

        let stdin = take_pipe(&mut child, |child| child.stdin.take(), "stdin")?;
        let stdout = take_pipe(&mut child, |child| child.stdout.take(), "stdout")?;
        let stderr = take_pipe(&mut child, |child| child.stderr.take(), "stderr")?;

        let (event_tx, events) = mpsc::sync_channel(EVENT_CAPACITY);
        let event_overflow = Arc::new(AtomicBool::new(false));
        let reader_overflow = Arc::clone(&event_overflow);
        let stdout_thread = match std::thread::Builder::new()
            .name("lsp-exact-stdout".to_string())
            .spawn(move || read_stdout(stdout, event_tx, &reader_overflow))
        {
            Ok(thread) => thread,
            Err(error) => {
                terminate_child(&mut child);
                return Err(error).context("spawn strict stdout reader");
            }
        };

        let stderr_bytes = Arc::new(Mutex::new(VecDeque::new()));
        let stderr_sink = Arc::clone(&stderr_bytes);
        let stderr_truncated = Arc::new(AtomicBool::new(false));
        let truncated_flag = Arc::clone(&stderr_truncated);
        let stderr_thread = match std::thread::Builder::new()
            .name("lsp-exact-stderr".to_string())
            .spawn(move || {
                drain_stderr(stderr, &stderr_sink, &truncated_flag);
            })
        {
            Ok(thread) => thread,
            Err(error) => {
                terminate_child(&mut child);
                let _ = stdout_thread.join();
                return Err(error).context("spawn stderr reader");
            }
        };

        Ok(Self {
            child,
            stdin: Some(BufWriter::new(stdin)),
            events,
            pending: VecDeque::new(),
            event_overflow,
            stderr_bytes,
            stderr_truncated,
            stdout_thread: Some(stdout_thread),
            stderr_thread: Some(stderr_thread),
            candidate_path,
            _workspace: workspace,
            finished: false,
        })
    }

    pub fn candidate_path(&self) -> &Path {
        &self.candidate_path
    }

    pub fn encode_message(message: &Value) -> Vec<u8> {
        let body = message.to_string();
        let mut frame =
            format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
        frame.extend_from_slice(body.as_bytes());
        frame
    }

    pub fn parse_stdout_frame_for_test(bytes: &[u8]) -> Result<Option<Value>> {
        let cursor = std::io::Cursor::new(bytes);
        let mut reader = BufReader::new(cursor);
        read_frame(&mut reader)
    }

    pub fn is_valid_server_notification_for_test(message: &Value) -> bool {
        is_server_notification(message)
    }

    pub fn send_raw_bytes(&mut self, bytes: &[u8]) -> Result<()> {
        ensure!(!self.finished, "cannot write after candidate exit");
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| anyhow!("candidate stdin is closed"))?;
        stdin.write_all(bytes).context("write candidate stdin")?;
        stdin.flush().context("flush candidate stdin")
    }

    pub fn send_raw_chunks(&mut self, chunks: &[&[u8]]) -> Result<()> {
        for chunk in chunks {
            self.send_raw_bytes(chunk)?;
        }
        Ok(())
    }

    pub fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        let message = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        self.send_raw_bytes(&Self::encode_message(&message))
    }

    pub fn request(
        &mut self,
        id: Value,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value> {
        ensure!(
            id.is_number() || id.is_string(),
            "request ID must be a number or string, got {id}"
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

    pub fn respond(&mut self, id: Value, result: Value) -> Result<()> {
        ensure!(
            id.is_number() || id.is_string(),
            "server request ID must be a number or string, got {id}"
        );
        let response = json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        });
        self.send_raw_bytes(&Self::encode_message(&response))
    }

    pub fn receive_response(
        &mut self,
        id: &Value,
        timeout: Duration,
    ) -> Result<Value> {
        self.ensure_event_queue_intact()?;
        if let Some(index) = self
            .pending
            .iter()
            .position(|message| is_response_for(message, id))
        {
            return self
                .pending
                .remove(index)
                .ok_or_else(|| anyhow!("matched pending response disappeared"));
        }

        let deadline = Instant::now() + timeout;
        loop {
            let message = self.receive_message_until(deadline, "response", id)?;
            if is_response_for(&message, id) {
                return Ok(message);
            }
            self.push_pending(message)?;
        }
    }

    pub fn receive_response_and_retain(
        &mut self,
        id: &Value,
        timeout: Duration,
    ) -> Result<Value> {
        let response = self.receive_response(id, timeout)?;
        self.push_pending(response.clone())?;
        Ok(response)
    }

    pub fn receive_server_request(
        &mut self,
        method: &str,
        timeout: Duration,
    ) -> Result<Value> {
        if let Some(index) = self
            .pending
            .iter()
            .position(|message| is_server_request_for(message, method))
        {
            return self
                .pending
                .remove(index)
                .ok_or_else(|| anyhow!("matched server request disappeared"));
        }

        let deadline = Instant::now() + timeout;
        let marker = Value::String(method.to_string());
        loop {
            let message =
                self.receive_message_until(deadline, "server request", &marker)?;
            if is_server_request_for(&message, method) {
                return Ok(message);
            }
            self.push_pending(message)?;
        }
    }

    pub fn assert_no_response_pending(&self) -> Result<()> {
        let unexpected = self
            .pending
            .iter()
            .find(|message| is_response_like(message));
        ensure!(
            unexpected.is_none(),
            "notification produced an unmatched response: {unexpected:?}"
        );
        Ok(())
    }

    pub fn event_queue_overflowed(&self) -> bool {
        self.event_overflow.load(Ordering::Acquire)
    }

    pub fn wait_for_exit(&mut self, timeout: Duration) -> Result<ExitStatus> {
        let deadline = Instant::now() + timeout;
        loop {
            self.ensure_event_queue_intact()?;
            if let Some(status) = self.child.try_wait().context("poll exit")? {
                self.finished = true;
                let _ = self.stdin.take();
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

    pub fn assert_transport_clean(&mut self) -> Result<()> {
        self.ensure_event_queue_intact()?;
        self.drain_available_events()?;
        self.ensure_event_queue_intact()?;

        if let Some(unexpected) = self
            .pending
            .iter()
            .find(|message| !is_server_notification(message))
        {
            bail!(
                "unconsumed terminal message: {unexpected}; \
                 pending={:?}; stderr={}",
                self.pending,
                self.stderr_tail()
            );
        }
        Ok(())
    }

    pub fn stderr_tail(&self) -> String {
        let bytes = lock_bytes(&self.stderr_bytes);
        let collected = bytes.iter().copied().collect::<Vec<_>>();
        let tail = String::from_utf8_lossy(&collected).into_owned();
        if self.stderr_truncated.load(Ordering::Acquire) {
            format!(
                "[stderr truncated to last {STDERR_BYTE_CAPACITY} bytes]\n{tail}"
            )
        } else {
            tail
        }
    }

    fn receive_message_until(
        &mut self,
        deadline: Instant,
        expected_kind: &str,
        marker: &Value,
    ) -> Result<Value> {
        self.ensure_event_queue_intact()?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            bail!(
                "timed out waiting for {expected_kind} {marker}; \
                 pending={:?}; stderr={}",
                self.pending,
                self.stderr_tail()
            );
        }

        match self.events.recv_timeout(remaining) {
            Ok(ProcessEvent::Message(message)) => Ok(message),
            Ok(ProcessEvent::ProtocolError(error)) => {
                bail!("stdout protocol error: {error}; stderr={}", self.stderr_tail())
            }
            Ok(ProcessEvent::Eof) => {
                bail!(
                    "stdout closed before {expected_kind} {marker}; stderr={}",
                    self.stderr_tail()
                )
            }
            Err(RecvTimeoutError::Timeout) => {
                bail!(
                    "timed out waiting for {expected_kind} {marker}; \
                     pending={:?}; stderr={}",
                    self.pending,
                    self.stderr_tail()
                )
            }
            Err(RecvTimeoutError::Disconnected) => {
                bail!("stdout reader disconnected; stderr={}", self.stderr_tail())
            }
        }
    }

    fn drain_available_events(&mut self) -> Result<()> {
        loop {
            match self.events.try_recv() {
                Ok(ProcessEvent::Message(message)) => self.push_pending(message)?,
                Ok(ProcessEvent::ProtocolError(error)) => {
                    bail!(
                        "stdout protocol error: {error}; stderr={}",
                        self.stderr_tail()
                    )
                }
                Ok(ProcessEvent::Eof) => {}
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }
        Ok(())
    }

    fn push_pending(&mut self, message: Value) -> Result<()> {
        ensure!(
            self.pending.len() < PENDING_CAPACITY,
            "pending message capacity exceeded ({PENDING_CAPACITY})"
        );
        self.pending.push_back(message);
        Ok(())
    }

    fn ensure_event_queue_intact(&self) -> Result<()> {
        ensure!(
            !self.event_overflow.load(Ordering::Acquire),
            "stdout event queue overflowed; stderr={}",
            self.stderr_tail()
        );
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
        let _ = self.stdin.take();
        terminate_child(&mut self.child);
        self.finished = true;
        self.finish_reader_threads();
    }
}

impl Drop for RealProcessClient {
    fn drop(&mut self) {
        if self.finished {
            self.finish_reader_threads();
        } else {
            self.force_cleanup();
        }
    }
}

fn take_pipe<T>(
    child: &mut Child,
    take: impl FnOnce(&mut Child) -> Option<T>,
    name: &str,
) -> Result<T> {
    match take(child) {
        Some(pipe) => Ok(pipe),
        None => {
            terminate_child(child);
            bail!("candidate {name} was not piped")
        }
    }
}

fn is_jsonrpc_2(message: &Value) -> bool {
    message.get("jsonrpc").and_then(Value::as_str) == Some("2.0")
}

fn is_response_like(message: &Value) -> bool {
    message.get("method").is_none()
        && (message.get("result").is_some() || message.get("error").is_some())
}

fn is_response(message: &Value) -> bool {
    is_jsonrpc_2(message)
        && is_response_like(message)
        && message.get("id").is_some_and(|id| {
            id.is_number() || id.is_string() || id.is_null()
        })
        && (message.get("result").is_some() ^ message.get("error").is_some())
}

fn is_response_for(message: &Value, id: &Value) -> bool {
    is_response(message) && message.get("id") == Some(id)
}

fn is_server_request_for(message: &Value, method: &str) -> bool {
    is_jsonrpc_2(message)
        && message.get("method").and_then(Value::as_str) == Some(method)
        && message
            .get("id")
            .is_some_and(|id| id.is_number() || id.is_string())
}

fn is_server_notification(message: &Value) -> bool {
    is_jsonrpc_2(message)
        && message.get("method").and_then(Value::as_str).is_some()
        && message.get("id").is_none()
}

fn read_stdout(
    stdout: ChildStdout,
    sender: SyncSender<ProcessEvent>,
    overflow: &AtomicBool,
) {
    let mut reader = BufReader::new(stdout);
    loop {
        let event = match read_frame(&mut reader) {
            Ok(Some(message)) => ProcessEvent::Message(message),
            Ok(None) => ProcessEvent::Eof,
            Err(error) => ProcessEvent::ProtocolError(error.to_string()),
        };
        let terminal = matches!(
            event,
            ProcessEvent::ProtocolError(_) | ProcessEvent::Eof
        );
        if !try_publish_event(&sender, overflow, event) || terminal {
            return;
        }
    }
}

fn try_publish_event(
    sender: &SyncSender<ProcessEvent>,
    overflow: &AtomicBool,
    event: ProcessEvent,
) -> bool {
    match sender.try_send(event) {
        Ok(()) => true,
        Err(TrySendError::Full(_)) => {
            overflow.store(true, Ordering::Release);
            false
        }
        Err(TrySendError::Disconnected(_)) => false,
    }
}

fn read_frame(reader: &mut dyn BufRead) -> Result<Option<Value>> {
    let mut content_length = None;
    let mut header_bytes = 0usize;

    loop {
        let remaining = MAX_HEADER_BYTES.saturating_sub(header_bytes);
        let line = match read_bounded_header_line(reader, remaining)? {
            Some(line) => line,
            None if header_bytes == 0 => return Ok(None),
            None => bail!("stdout ended inside a frame header block"),
        };
        header_bytes = header_bytes.saturating_add(line.len());

        let line = std::str::from_utf8(&line)
            .context("stdout header was not valid UTF-8")?;
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }

        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| anyhow!("non-header stdout line: {line:?}"))?;
        let name = name.trim();
        let value = value.trim();
        if name.eq_ignore_ascii_case("Content-Length") {
            ensure!(
                content_length.is_none(),
                "duplicate Content-Length header"
            );
            let parsed = value
                .parse::<usize>()
                .with_context(|| format!("invalid Content-Length {value:?}"))?;
            ensure!(
                parsed <= MAX_FRAME_BYTES,
                "frame exceeded {MAX_FRAME_BYTES} bytes"
            );
            content_length = Some(parsed);
        } else if !name.eq_ignore_ascii_case("Content-Type") {
            bail!("unexpected stdout header {name:?}");
        }
    }

    let length = content_length
        .ok_or_else(|| anyhow!("frame omitted Content-Length"))?;
    let mut body = vec![0u8; length];
    reader
        .read_exact(&mut body)
        .context("read stdout frame body")?;
    let message: Value = serde_json::from_slice(&body).with_context(|| {
        format!(
            "stdout frame was not JSON: {:?}",
            String::from_utf8_lossy(&body)
        )
    })?;
    ensure!(message.is_object(), "stdout JSON-RPC message was not an object");
    Ok(Some(message))
}

fn read_bounded_header_line(
    reader: &mut dyn BufRead,
    max_bytes: usize,
) -> Result<Option<Vec<u8>>> {
    let mut line = Vec::with_capacity(max_bytes.min(256));
    loop {
        let available = reader.fill_buf().context("read stdout header buffer")?;
        if available.is_empty() {
            if line.is_empty() {
                return Ok(None);
            }
            bail!("stdout ended inside a frame header line");
        }

        let newline = available.iter().position(|byte| *byte == b'\n');
        let take = newline.map_or(available.len(), |index| index + 1);
        ensure!(
            line.len().saturating_add(take) <= max_bytes,
            "stdout header exceeded {MAX_HEADER_BYTES} bytes"
        );
        line.extend_from_slice(&available[..take]);
        reader.consume(take);
        if newline.is_some() {
            return Ok(Some(line));
        }
    }
}

fn drain_stderr(
    mut stderr: std::process::ChildStderr,
    sink: &Arc<Mutex<VecDeque<u8>>>,
    truncated: &AtomicBool,
) {
    let mut chunk = [0u8; 1024];
    loop {
        let read = match stderr.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(read) => read,
        };
        let mut bytes = lock_bytes(sink);
        for byte in &chunk[..read] {
            if bytes.len() == STDERR_BYTE_CAPACITY {
                let _ = bytes.pop_front();
                truncated.store(true, Ordering::Release);
            }
            bytes.push_back(*byte);
        }
    }
}

fn terminate_child(child: &mut Child) {
    match child.try_wait() {
        Ok(Some(_)) => {}
        Ok(None) | Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn lock_bytes(bytes: &Arc<Mutex<VecDeque<u8>>>) -> MutexGuard<'_, VecDeque<u8>> {
    match bytes.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}
