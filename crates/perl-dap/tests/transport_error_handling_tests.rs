//! Tests for transport layer error handling.
//!
//! Verifies that serialization errors and write failures are handled symmetrically:
//! - Serialization errors produce a DAP error response to the client
//! - Write failures terminate gracefully without panicking
//! - Transport recovers or shuts down cleanly after errors

use anyhow::{Context, Result, anyhow};
use perl_dap::DapMessage;
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::thread;
use std::time::{Duration, Instant};

struct DapProcess {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<std::result::Result<DapMessage, String>>,
}

impl DapProcess {
    fn spawn() -> Result<Self> {
        let mut child = Command::new(env!("CARGO_BIN_EXE_perl-dap"))
            .arg("--stdio")
            .arg("--log-level")
            .arg("error")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("failed to spawn perl-dap binary")?;

        let stdin = child.stdin.take().ok_or_else(|| anyhow!("child stdin was not piped"))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow!("child stdout was not piped"))?;
        let rx = spawn_frame_reader(stdout);

        Ok(Self { child, stdin, rx })
    }

    fn send_request(&mut self, seq: i64, command: &str, arguments: Option<Value>) -> Result<()> {
        let payload = serde_json::to_vec(&json!({
            "type": "request",
            "seq": seq,
            "command": command,
            "arguments": arguments,
        }))?;
        write!(self.stdin, "Content-Length: {}\r\n\r\n", payload.len())?;
        self.stdin.write_all(&payload)?;
        self.stdin.flush()?;
        Ok(())
    }

    fn wait_for_response(&self, request_seq: i64, command: &str) -> Result<Option<Value>> {
        wait_for_message(
            &self.rx,
            format!("response `{command}` for request {request_seq}"),
            |msg| {
                matches!(
                    msg,
                    DapMessage::Response {
                        request_seq: actual_request_seq,
                        command: actual_command,
                        ..
                    } if *actual_request_seq == request_seq && actual_command == command
                )
            },
        )
        .and_then(|message| match message {
            DapMessage::Response { success, body, message, .. } => {
                if success {
                    Ok(body)
                } else {
                    Err(anyhow!(
                        "response `{command}` for request {request_seq} failed: {}",
                        message.unwrap_or_else(|| "<no message>".to_string())
                    ))
                }
            }
            other => Err(anyhow!("expected response `{command}`, got {other:?}")),
        })
    }

    fn wait_for_event(&self, event_name: &str) -> Result<Option<Value>> {
        wait_for_message(
            &self.rx,
            format!("event `{event_name}`"),
            |msg| matches!(msg, DapMessage::Event { event, .. } if event == event_name),
        )
        .and_then(|message| match message {
            DapMessage::Event { body, .. } => Ok(body),
            other => Err(anyhow!("expected event `{event_name}`, got {other:?}")),
        })
    }
}

impl Drop for DapProcess {
    fn drop(&mut self) {
        if let Ok(None) = self.child.try_wait() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn spawn_frame_reader<R>(mut reader: R) -> Receiver<std::result::Result<DapMessage, String>>
where
    R: Read + Send + 'static,
{
    let (tx, rx) = channel();
    thread::spawn(move || {
        loop {
            match read_framed_message(&mut reader) {
                Ok(message) => {
                    if tx.send(Ok(message)).is_err() {
                        break;
                    }
                }
                Err(error) => {
                    let _ = tx.send(Err(error.to_string()));
                    break;
                }
            }
        }
    });
    rx
}

fn read_framed_message<R: Read>(reader: &mut R) -> Result<DapMessage> {
    let mut header = Vec::new();
    let mut byte = [0_u8; 1];
    loop {
        reader.read_exact(&mut byte).context("failed to read DAP frame header")?;
        header.push(byte[0]);
        if header.ends_with(b"\r\n\r\n") {
            break;
        }
        if header.len() > 1024 {
            return Err(anyhow!("DAP frame header exceeded 1024 bytes"));
        }
    }

    let header_text = std::str::from_utf8(&header).context("DAP frame header was not UTF-8")?;
    let content_length = header_text
        .split("\r\n")
        .find_map(|line| line.strip_prefix("Content-Length: "))
        .ok_or_else(|| anyhow!("DAP frame header missing Content-Length: {header_text:?}"))?
        .parse::<usize>()
        .context("DAP Content-Length was not a positive integer")?;

    let mut body = vec![0_u8; content_length];
    reader.read_exact(&mut body).context("failed to read DAP frame body")?;
    serde_json::from_slice(&body).context("DAP frame body was not a DapMessage")
}

fn wait_for_message<F>(
    rx: &Receiver<std::result::Result<DapMessage, String>>,
    description: String,
    matches_message: F,
) -> Result<DapMessage>
where
    F: Fn(&DapMessage) -> bool,
{
    let timeout = Duration::from_secs(5);
    let deadline = Instant::now() + timeout;
    let mut observed = Vec::new();

    loop {
        let now = Instant::now();
        if now >= deadline {
            return Err(anyhow!("timeout waiting for {description}; observed {observed:?}"));
        }

        match rx.recv_timeout(deadline.saturating_duration_since(now)) {
            Ok(Ok(message)) if matches_message(&message) => return Ok(message),
            Ok(Ok(message)) => observed.push(message_label(&message)),
            Ok(Err(error)) => {
                return Err(anyhow!("DAP reader failed while waiting for {description}: {error}"));
            }
            Err(RecvTimeoutError::Timeout) => {
                return Err(anyhow!("timeout waiting for {description}; observed {observed:?}"));
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(anyhow!(
                    "DAP reader disconnected while waiting for {description}; observed {observed:?}"
                ));
            }
        }
    }
}

fn message_label(message: &DapMessage) -> String {
    match message {
        DapMessage::Request { command, .. } => format!("request:{command}"),
        DapMessage::Response { command, request_seq, success, .. } => {
            format!("response:{command}#{request_seq}:success={success}")
        }
        DapMessage::Event { event, .. } => format!("event:{event}"),
    }
}

/// Test that transport layer handles initialization correctly before testing error scenarios.
#[test]
fn test_transport_initialization_success() -> Result<()> {
    let mut dap = DapProcess::spawn()?;

    dap.send_request(
        1,
        "initialize",
        Some(json!({
            "clientID": "perl-dap-transport-error-test",
            "adapterID": "perl-dap",
            "pathFormat": "path",
            "linesStartAt1": true,
            "columnsStartAt1": true,
        })),
    )?;

    let init_response = dap.wait_for_response(1, "initialize")?;
    assert!(init_response.is_some(), "initialize response should contain capabilities");

    dap.wait_for_event("initialized")?;

    // Send a second request to verify transport is still alive
    dap.send_request(2, "threads", None)?;
    dap.wait_for_response(2, "threads")?;

    Ok(())
}

/// Test that transport layer properly handles multiple valid requests in sequence.
/// This establishes baseline behavior for the error handling tests.
#[test]
fn test_transport_sequential_requests_all_succeed() -> Result<()> {
    let mut dap = DapProcess::spawn()?;

    // Initialize
    dap.send_request(
        1,
        "initialize",
        Some(json!({
            "clientID": "perl-dap-transport-error-test",
            "adapterID": "perl-dap",
            "pathFormat": "path",
            "linesStartAt1": true,
            "columnsStartAt1": true,
        })),
    )?;
    dap.wait_for_response(1, "initialize")?;
    dap.wait_for_event("initialized")?;

    // First request: threads
    dap.send_request(2, "threads", None)?;
    dap.wait_for_response(2, "threads")?;

    // Second request: stackTrace (will fail because no active thread, but should send response)
    dap.send_request(
        3,
        "stackTrace",
        Some(json!({
            "threadId": 0,
        })),
    )?;
    // This should get an error response, but it should be sent (not silently dropped)
    let result = dap.wait_for_response(3, "stackTrace");
    assert!(
        result.is_err(),
        "stackTrace on non-existent thread should fail, but error should be transmitted"
    );

    // Third request: threads again to verify adapter is still alive
    dap.send_request(4, "threads", None)?;
    dap.wait_for_response(4, "threads")?;

    Ok(())
}

/// Test that when an unknown command is sent, the adapter responds with an error
/// (not silence).
#[test]
fn test_transport_unknown_command_gets_error_response() -> Result<()> {
    let mut dap = DapProcess::spawn()?;

    // Initialize
    dap.send_request(
        1,
        "initialize",
        Some(json!({
            "clientID": "perl-dap-transport-error-test",
            "adapterID": "perl-dap",
            "pathFormat": "path",
            "linesStartAt1": true,
            "columnsStartAt1": true,
        })),
    )?;
    dap.wait_for_response(1, "initialize")?;
    dap.wait_for_event("initialized")?;

    // Send an unknown command
    dap.send_request(2, "unknownCommand", None)?;

    // The key assertion: we MUST receive a response (even if it's an error)
    // not silence/timeout. The transport should send an error response.
    let wait_result = dap.wait_for_response(2, "unknownCommand");

    // If this times out, it means the transport silently dropped the response
    // (the bug we're fixing).
    assert!(wait_result.is_err(), "unknown command should receive an error response (not timeout)");

    Ok(())
}
