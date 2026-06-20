//! Transport loop tests for handling non-request DAP messages.
//!
//! Verifies that Response and Event messages from the client are handled
//! gracefully without silent drops. These tests ensure the adapter can
//! receive bidirectional messages as permitted by the DAP protocol.
//!
//! Related: Issue #1608

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
            .arg("debug")
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

    fn send_raw_message(&mut self, payload: &[u8]) -> Result<()> {
        write!(self.stdin, "Content-Length: {}\r\n\r\n", payload.len())?;
        self.stdin.write_all(payload)?;
        self.stdin.flush()?;
        Ok(())
    }

    fn send_request(&mut self, seq: i64, command: &str, arguments: Option<Value>) -> Result<()> {
        let payload = serde_json::to_vec(&json!({
            "type": "request",
            "seq": seq,
            "command": command,
            "arguments": arguments,
        }))?;
        self.send_raw_message(&payload)
    }

    fn send_response(
        &mut self,
        seq: i64,
        request_seq: i64,
        command: &str,
        success: bool,
    ) -> Result<()> {
        let payload = serde_json::to_vec(&json!({
            "type": "response",
            "seq": seq,
            "request_seq": request_seq,
            "command": command,
            "success": success,
        }))?;
        self.send_raw_message(&payload)
    }

    fn send_event(&mut self, seq: i64, event: &str) -> Result<()> {
        let payload = serde_json::to_vec(&json!({
            "type": "event",
            "seq": seq,
            "event": event,
        }))?;
        self.send_raw_message(&payload)
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

/// Test that a Response message from the client does not crash the adapter.
/// The adapter should handle it gracefully (log it but continue accepting requests).
#[test]
fn transport_receives_response_message_without_crashing() -> Result<()> {
    let mut dap = DapProcess::spawn()?;

    // Initialize first so the adapter is in a known state
    dap.send_request(
        1,
        "initialize",
        Some(json!({
            "clientID": "test",
            "adapterID": "perl-dap",
            "pathFormat": "path",
            "linesStartAt1": true,
            "columnsStartAt1": true,
        })),
    )?;
    dap.wait_for_response(1, "initialize")?;
    dap.wait_for_event("initialized")?;

    // Now send a Response message (which normally comes from client as a reply to a server request).
    // This should NOT crash the adapter. We expect it to silently handle or log.
    dap.send_response(2, 999, "someServerInitiatedRequest", true)?;

    // Wait a short time to verify adapter doesn't crash
    thread::sleep(Duration::from_millis(100));

    // Send a normal request to verify adapter is still alive and responsive.
    dap.send_request(3, "threads", None)?;
    let threads_resp = dap.wait_for_response(3, "threads")?;
    assert!(
        threads_resp.is_some(),
        "adapter should still respond after receiving a Response message"
    );

    // Verify we can disconnect cleanly
    dap.send_request(4, "disconnect", Some(json!({})))?;
    dap.wait_for_response(4, "disconnect")?;

    Ok(())
}

/// Test that an Event message from the client does not crash the adapter.
/// The adapter should handle it gracefully (log it but continue accepting requests).
#[test]
fn transport_receives_event_message_without_crashing() -> Result<()> {
    let mut dap = DapProcess::spawn()?;

    // Initialize first
    dap.send_request(
        1,
        "initialize",
        Some(json!({
            "clientID": "test",
            "adapterID": "perl-dap",
            "pathFormat": "path",
            "linesStartAt1": true,
            "columnsStartAt1": true,
        })),
    )?;
    dap.wait_for_response(1, "initialize")?;
    dap.wait_for_event("initialized")?;

    // Send an Event message from the client (normally events flow from adapter to client,
    // but the DAP spec permits bidirectional flow for advanced features).
    dap.send_event(2, "clientEvent")?;

    // Wait a short time to verify adapter doesn't crash
    thread::sleep(Duration::from_millis(100));

    // Send a normal request to verify adapter is still alive and responsive.
    dap.send_request(3, "threads", None)?;
    let threads_resp = dap.wait_for_response(3, "threads")?;
    assert!(
        threads_resp.is_some(),
        "adapter should still respond after receiving an Event message"
    );

    // Verify we can disconnect cleanly
    dap.send_request(4, "disconnect", Some(json!({})))?;
    dap.wait_for_response(4, "disconnect")?;

    Ok(())
}

/// Test that Response and Event messages can be received in sequence without crashing.
#[test]
fn transport_receives_mixed_non_request_messages_without_crashing() -> Result<()> {
    let mut dap = DapProcess::spawn()?;

    // Initialize
    dap.send_request(
        1,
        "initialize",
        Some(json!({
            "clientID": "test",
            "adapterID": "perl-dap",
            "pathFormat": "path",
            "linesStartAt1": true,
            "columnsStartAt1": true,
        })),
    )?;
    dap.wait_for_response(1, "initialize")?;
    dap.wait_for_event("initialized")?;

    // Send multiple non-request messages
    dap.send_response(2, 999, "request1", true)?;
    dap.send_event(3, "event1")?;
    dap.send_response(4, 1000, "request2", false)?;
    dap.send_event(5, "event2")?;

    // Wait a bit for processing
    thread::sleep(Duration::from_millis(100));

    // Send a normal request and verify adapter responds
    dap.send_request(6, "threads", None)?;
    let threads_resp = dap.wait_for_response(6, "threads")?;
    assert!(
        threads_resp.is_some(),
        "adapter should still respond after receiving multiple non-request messages"
    );

    // Clean disconnect
    dap.send_request(7, "disconnect", Some(json!({})))?;
    dap.wait_for_response(7, "disconnect")?;

    Ok(())
}
