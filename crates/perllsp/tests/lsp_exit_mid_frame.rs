//! Real-process proof for a response that is still being written when `exit`
//! is received without a preceding `shutdown`.

#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.

use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::collections::{BTreeSet, VecDeque};
use std::io::{Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tempfile::TempDir;

const IO_TIMEOUT: Duration = Duration::from_secs(30);
const SYMBOL_COUNT: usize = 500;
const SYMBOL_PADDING: usize = 4_096;
const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;
const STDERR_BYTES: usize = 32 * 1024;

#[derive(Debug)]
struct ReadRequest(usize);

struct ProcessGuard {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout_requests: Option<SyncSender<ReadRequest>>,
    stdout_responses: Receiver<std::io::Result<Vec<u8>>>,
    stdout_thread: Option<JoinHandle<()>>,
    stderr_thread: Option<JoinHandle<Vec<u8>>>,
    _workspace: TempDir,
}

impl ProcessGuard {
    fn spawn() -> Result<Self> {
        let workspace = tempfile::tempdir().context("create isolated workspace")?;
        let mut child = Command::new(env!("CARGO_BIN_EXE_perllsp"))
            .arg("--stdio")
            .current_dir(workspace.path())
            .env("PERL_LSP_QUIET", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("spawn Cargo's exact perllsp binary")?;
        let stdin = child.stdin.take().context("perllsp stdin unavailable")?;
        let mut stdout = child.stdout.take().context("perllsp stdout unavailable")?;
        let stderr = child.stderr.take().context("perllsp stderr unavailable")?;
        let (stdout_requests, requests) = mpsc::sync_channel(1);
        let (responses, stdout_responses) = mpsc::sync_channel(1);
        let stdout_thread = thread::spawn(move || {
            while let Ok(ReadRequest(length)) = requests.recv() {
                let mut bytes = vec![0; length];
                let result = stdout.read_exact(&mut bytes).map(|()| bytes);
                if responses.send(result).is_err() {
                    break;
                }
            }
        });
        let stderr_thread = thread::spawn(move || {
            let mut tail = VecDeque::with_capacity(STDERR_BYTES);
            let mut chunk = [0; 4096];
            let mut stderr = stderr;
            loop {
                match stderr.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => {
                        for byte in chunk.iter().take(count) {
                            if tail.len() == STDERR_BYTES {
                                let _ = tail.pop_front();
                            }
                            tail.push_back(*byte);
                        }
                    }
                }
            }
            tail.into_iter().collect()
        });
        Ok(Self {
            child,
            stdin: Some(stdin),
            stdout_requests: Some(stdout_requests),
            stdout_responses,
            stdout_thread: Some(stdout_thread),
            stderr_thread: Some(stderr_thread),
            _workspace: workspace,
        })
    }

    fn send(&mut self, message: &Value) -> Result<()> {
        let body = serde_json::to_vec(message).context("encode LSP message")?;
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        let stdin = self.stdin.as_mut().context("perllsp stdin closed")?;
        stdin.write_all(header.as_bytes()).context("write LSP header")?;
        stdin.write_all(&body).context("write LSP body")?;
        stdin.flush().context("flush LSP message")
    }

    fn read_frame(&mut self, deadline: Instant) -> Result<Value> {
        let body = self.read_frame_bytes(deadline)?;
        serde_json::from_slice(&body).context("parse LSP response JSON")
    }

    fn read_frame_bytes(&mut self, deadline: Instant) -> Result<Vec<u8>> {
        let header = self.read_header(deadline)?;
        let length = header
            .strip_prefix("Content-Length:")
            .context("response omitted Content-Length")?
            .trim()
            .parse::<usize>()
            .context("invalid Content-Length")?;
        ensure!(length <= MAX_FRAME_BYTES, "response body exceeded {MAX_FRAME_BYTES} bytes");
        ensure!(length > 0, "response body was empty");
        read_exact_with_deadline(self, length, deadline)
    }

    fn read_header(&mut self, deadline: Instant) -> Result<String> {
        let mut bytes = Vec::new();
        loop {
            let byte = read_exact_with_deadline(self, 1, deadline)?;
            bytes.push(byte.first().copied().context("stdout reader returned no byte")?);
            if bytes.ends_with(b"\r\n\r\n") {
                let text = String::from_utf8(bytes).context("response header was not UTF-8")?;
                return Ok(text
                    .strip_suffix("\r\n\r\n")
                    .context("header terminator disappeared")?
                    .to_string());
            }
            ensure!(bytes.len() <= 4096, "response headers exceeded bound");
        }
    }

    fn finish(mut self, expected_code: i32) -> Result<()> {
        let deadline = Instant::now() + IO_TIMEOUT;
        while self.child.try_wait().context("poll perllsp exit")?.is_none() {
            ensure!(Instant::now() < deadline, "perllsp did not exit within deadline");
            thread::sleep(Duration::from_millis(10));
        }
        let status = self.child.wait().context("wait for perllsp")?;
        ensure!(
            status.code() == Some(expected_code),
            "expected exit {expected_code}, got {status}"
        );
        self.stdout_requests.take();
        if let Some(thread) = self.stdout_thread.take() {
            thread.join().map_err(|e| anyhow::anyhow!("stdout reader panicked: {e:?}"))?;
        }
        let stderr = self
            .stderr_thread
            .take()
            .context("stderr reader already joined")?
            .join()
            .map_err(|e| anyhow::anyhow!("stderr reader panicked: {e:?}"))?;
        ensure!(stderr.len() <= 32 * 1024, "stderr exceeded bounded capture");
        Ok(())
    }
}

impl Drop for ProcessGuard {
    fn drop(&mut self) {
        let _ = self.stdin.take();
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        self.stdout_requests.take();
        if let Some(thread) = self.stdout_thread.take() {
            let _ = thread.join();
        }
        if let Some(thread) = self.stderr_thread.take() {
            let _ = thread.join();
        }
    }
}

fn read_exact_with_deadline(
    process: &mut ProcessGuard,
    length: usize,
    deadline: Instant,
) -> Result<Vec<u8>> {
    ensure!(length <= MAX_FRAME_BYTES, "read request exceeded {MAX_FRAME_BYTES} bytes");
    let requests = process.stdout_requests.as_ref().context("stdout reader stopped")?;
    requests.send(ReadRequest(length)).context("request stdout bytes")?;
    let remaining = deadline.saturating_duration_since(Instant::now());
    ensure!(!remaining.is_zero(), "stdio read deadline expired");
    match process.stdout_responses.recv_timeout(remaining) {
        Ok(result) => result.context("read complete LSP frame"),
        Err(RecvTimeoutError::Timeout) => bail!("stdio read deadline expired"),
        Err(RecvTimeoutError::Disconnected) => bail!("stdout reader disconnected"),
    }
}

fn initialize(process: &mut ProcessGuard) -> Result<()> {
    process.send(&json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "processId": null,
            "rootUri": null,
            "capabilities": {}
        }
    }))?;
    let response = process.read_frame(Instant::now() + IO_TIMEOUT)?;
    ensure!(response.get("id") == Some(&json!(1)), "initialize response mismatch: {response}");
    process.send(&json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }))
}

fn large_symbol_document() -> String {
    let padding = "x".repeat(SYMBOL_PADDING);
    (0..SYMBOL_COUNT)
        .map(|index| format!("sub symbol_{index:05}_{padding} {{ return {index}; }}\n"))
        .collect()
}

fn request_symbol(process: &mut ProcessGuard, id: u64) -> Result<()> {
    process.send(&json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "textDocument/documentSymbol",
        "params": { "textDocument": { "uri": "file:///exit-mid-frame-symbols.pl" } }
    }))
}

fn request_large_symbol_frame(process: &mut ProcessGuard) -> Result<(usize, u64)> {
    initialize(process)?;
    let uri = "file:///exit-mid-frame-symbols.pl";
    process.send(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": { "textDocument": {
            "uri": uri,
            "languageId": "perl",
            "version": 1,
            "text": large_symbol_document()
        }}
    }))?;
    request_symbol(process, 2)?;

    // Consume complete preceding notifications, then stop after the target
    // header so the real OS pipe applies backpressure to the body writer.
    let deadline = Instant::now() + IO_TIMEOUT;
    let mut request_id = 2u64;
    for _attempt in 0..20u64 {
        let header = process.read_header(deadline)?;
        let length = header
            .strip_prefix("Content-Length:")
            .context("response omitted Content-Length")?
            .trim()
            .parse::<usize>()?;
        ensure!(length <= MAX_FRAME_BYTES, "response body exceeded {MAX_FRAME_BYTES} bytes");
        if length > 64 * 1024 {
            return Ok((length, request_id));
        }
        let body = read_exact_with_deadline(process, length, deadline)?;
        let message: Value = serde_json::from_slice(&body).context("parse preceding response")?;
        if message.get("id") == Some(&json!(request_id)) {
            request_id += 1;
            request_symbol(process, request_id)?;
            thread::sleep(Duration::from_millis(100));
        }
    }
    bail!("documentSymbol did not produce a pipe-sized response after 20 attempts")
}

fn assert_symbol_response(body: &[u8], expected_id: u64) -> Result<()> {
    let response: Value =
        serde_json::from_slice(body).context("parse complete documentSymbol response")?;
    ensure!(
        response.get("id") == Some(&json!(expected_id)),
        "documentSymbol response mismatch: {response}"
    );
    let symbols = response
        .get("result")
        .and_then(Value::as_array)
        .context("documentSymbol response lacked an array result")?;
    let mut names = Vec::new();
    collect_symbol_names(symbols, &mut names);
    ensure!(names.len() == SYMBOL_COUNT, "symbol count mismatch: {}", names.len());
    let expected = (0..SYMBOL_COUNT)
        .map(|index| format!("symbol_{index:05}_{}", "x".repeat(SYMBOL_PADDING)))
        .collect::<BTreeSet<_>>();
    let actual = names.into_iter().collect::<BTreeSet<_>>();
    ensure!(actual == expected, "documentSymbol names did not match the complete fixture set");
    Ok(())
}

fn collect_symbol_names(values: &[Value], names: &mut Vec<String>) {
    for value in values {
        if let Some(name) = value.get("name").and_then(Value::as_str)
            && name.starts_with("symbol_")
        {
            names.push(name.to_string());
        }
        if let Some(children) = value.get("children").and_then(Value::as_array) {
            collect_symbol_names(children, names);
        }
    }
}

fn read_response_id(process: &mut ProcessGuard, id: &Value) -> Result<Value> {
    let deadline = Instant::now() + IO_TIMEOUT;
    loop {
        let message = process.read_frame(deadline)?;
        if message.get("id") == Some(id) {
            return Ok(message);
        }
    }
}

#[test]
fn document_symbols_complete_when_exit_arrives_after_header() -> Result<()> {
    let mut process = ProcessGuard::spawn()?;
    let (length, response_id) = request_large_symbol_frame(&mut process)?;

    process.send(&json!({ "jsonrpc": "2.0", "method": "exit", "params": null }))?;
    let deadline = Instant::now() + IO_TIMEOUT;
    let body = read_exact_with_deadline(&mut process, length, deadline)
        .with_context(|| format!("read complete {length}-byte documentSymbol body after exit"))?;
    assert_symbol_response(&body, response_id)?;
    process.finish(1)
}

#[test]
fn document_symbols_large_frame_is_complete_without_exit_race() -> Result<()> {
    let mut process = ProcessGuard::spawn()?;
    let (length, response_id) = request_large_symbol_frame(&mut process)?;
    let body = read_exact_with_deadline(&mut process, length, Instant::now() + IO_TIMEOUT)?;
    assert_symbol_response(&body, response_id)?;
    process.send(&json!({ "jsonrpc": "2.0", "id": 3, "method": "shutdown", "params": null }))?;
    let shutdown = read_response_id(&mut process, &json!(3))?;
    ensure!(shutdown.get("id") == Some(&json!(3)), "shutdown response mismatch: {shutdown}");
    ensure!(
        shutdown.get("result") == Some(&Value::Null),
        "shutdown response was not null: {shutdown}"
    );
    ensure!(shutdown.get("error").is_none(), "shutdown response contained an error: {shutdown}");
    process.send(&json!({ "jsonrpc": "2.0", "method": "exit", "params": null }))?;
    process.finish(0)
}
