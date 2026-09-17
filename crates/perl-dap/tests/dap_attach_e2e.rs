//! End-to-end DAP attach smoke test using a loopback TCP debugger.

// Tests use panic! as structured test failure reporters.
#![allow(clippy::panic)]

mod common;

use perl_dap::{DapMessage, DebugAdapter};
use perl_lsp_rs_core::transport::framing::frame;
use serde_json::{Value, json};
use std::error::Error;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc::{Receiver, TryRecvError, sync_channel};
use std::thread;
use std::time::{Duration, Instant};

type TestResult = Result<(), Box<dyn Error>>;

fn smoke_timeout() -> Duration {
    if std::env::var_os("LLVM_PROFILE_FILE").is_some()
        || std::env::var_os("CARGO_LLVM_COV").is_some()
    {
        Duration::from_mins(1)
    } else {
        Duration::from_secs(10)
    }
}

fn wait_for_event(
    rx: &Receiver<DapMessage>,
    event_name: &str,
    timeout: Duration,
) -> Result<DapMessage, String> {
    common::wait_for_event(rx, event_name, timeout)
}

fn response_success(response: DapMessage, command: &str) -> Result<Option<Value>, String> {
    match response {
        DapMessage::Response { success, command: actual, body, message, .. } => {
            if actual != command {
                return Err(format!("expected `{command}` response, got `{actual}`"));
            }
            if !success {
                return Err(format!(
                    "command `{command}` failed: {}",
                    message.unwrap_or_else(|| "<no message>".to_string())
                ));
            }
            Ok(body)
        }
        _ => Err(format!("expected response message for `{command}`")),
    }
}

fn response_failure_message(response: DapMessage, command: &str) -> Result<String, String> {
    match response {
        DapMessage::Response { success, command: actual, message, .. } => {
            if actual != command {
                return Err(format!("expected `{command}` response, got `{actual}`"));
            }
            if success {
                return Err(format!("expected `{command}` failure response"));
            }
            message.ok_or_else(|| format!("`{command}` failure response missing message"))
        }
        _ => Err(format!("expected response message for `{command}`")),
    }
}

fn event_body(message: &DapMessage) -> Option<&Value> {
    match message {
        DapMessage::Event { body, .. } => body.as_ref(),
        _ => None,
    }
}

#[test]
fn dap_attach_e2e_tcp_loopback() -> TestResult {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();

    let server_handle = thread::spawn(move || -> Result<(), Box<dyn Error + Send + Sync>> {
        let (mut socket, _) = listener.accept()?;

        let stopped_event = json!({
            "type": "event",
            "seq": 1,
            "event": "stopped",
            "body": {
                "reason": "breakpoint",
                "threadId": 7,
                "allThreadsStopped": true
            }
        })
        .to_string();
        socket.write_all(&frame(stopped_event.as_bytes()))?;
        socket.flush()?;

        let mut buf = [0u8; 512];
        loop {
            match socket.read(&mut buf) {
                Ok(0) => break,
                Ok(_) => continue,
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(err) => return Err(Box::new(err)),
            }
        }

        Ok(())
    });

    let timeout = smoke_timeout();
    let mut adapter = DebugAdapter::new();
    let (tx, rx) = sync_channel(64);
    adapter.set_event_sender(tx);

    let init_body = response_success(adapter.handle_request(1, "initialize", None), "initialize")?;
    let capabilities = init_body.ok_or("initialize response missing capability body")?;
    // #9581: restart is a floored secondary capability in attach mode too —
    // the TCP attach surface shares the native initialize rows and each is an
    // explicit `false` until its own gate passes.
    assert!(
        !capabilities.get("supportsRestartRequest").and_then(|v| v.as_bool()).unwrap_or(true),
        "supportsRestartRequest must be false under the #9581 floor (attach mode)"
    );
    let _initialized = wait_for_event(&rx, "initialized", timeout)?;

    response_success(
        adapter.handle_request(
            2,
            "attach",
            Some(json!({
                "host": "127.0.0.1",
                "port": port,
                "timeout": 2000
            })),
        ),
        "attach",
    )?;

    let threads_body = response_success(adapter.handle_request(3, "threads", None), "threads")?
        .ok_or("threads response missing body")?;
    let threads = threads_body
        .get("threads")
        .and_then(Value::as_array)
        .ok_or("threads response missing thread list")?;
    assert_eq!(threads.len(), 1);
    assert_eq!(threads[0]["id"], 1);
    assert_eq!(threads[0]["name"], "TCP Attached Thread");

    let stopped = wait_for_event(&rx, "stopped", timeout)?;
    let stopped_body = event_body(&stopped).ok_or("stopped event missing body")?;
    assert_eq!(stopped_body.get("reason").and_then(Value::as_str), Some("breakpoint"));
    // #8294/#14787: TCP attach exposes one synthetic execution context; the
    // peer thread id (7) must not leak through to the stopped event.
    assert_eq!(stopped_body.get("threadId").and_then(Value::as_i64), Some(1));

    response_success(adapter.handle_request(4, "disconnect", Some(json!({}))), "disconnect")?;
    let _terminated = wait_for_event(&rx, "terminated", timeout)?;

    server_handle
        .join()
        .map_err(|_| std::io::Error::other("fake TCP debugger server panicked"))?
        .map_err(std::io::Error::other)?;
    Ok(())
}

#[test]
fn dap_attach_e2e_tcp_loopback_peer_pause_is_forwarded() -> TestResult {
    tcp_loopback_peer_event("pause")
}

#[test]
fn dap_attach_e2e_tcp_loopback_peer_entry_is_forwarded() -> TestResult {
    tcp_loopback_peer_event("entry")
}

fn tcp_loopback_peer_event(peer_reason: &'static str) -> TestResult {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    listener.set_nonblocking(true)?;
    let (release_tx, release_rx) = sync_channel(1);

    let server_handle = thread::spawn(move || -> Result<(), Box<dyn Error + Send + Sync>> {
        let deadline = Instant::now() + Duration::from_secs(15);
        let (mut socket, _) = loop {
            match listener.accept() {
                Ok(connection) => break connection,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return Err(Box::new(std::io::Error::new(
                            std::io::ErrorKind::TimedOut,
                            "fake TCP debugger server accept timed out",
                        )));
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => return Err(Box::new(error)),
            }
        };
        socket.set_nonblocking(false)?;
        socket.set_read_timeout(Some(Duration::from_secs(15)))?;
        socket.set_write_timeout(Some(Duration::from_secs(15)))?;

        release_rx.recv_timeout(Duration::from_secs(15))?;

        let stopped_event = json!({
            "type": "event",
            "seq": 1,
            "event": "stopped",
            "body": {
                "reason": peer_reason,
                "threadId": 19,
                "allThreadsStopped": true
            }
        })
        .to_string();
        socket.write_all(&frame(stopped_event.as_bytes()))?;
        socket.flush()?;

        let mut buf = [0u8; 512];
        loop {
            match socket.read(&mut buf) {
                Ok(0) => break,
                Ok(_) => continue,
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(err) => return Err(Box::new(err)),
            }
        }

        Ok(())
    });

    let timeout = smoke_timeout();
    let mut adapter = DebugAdapter::new();
    let (tx, rx) = sync_channel(64);
    adapter.set_event_sender(tx);

    response_success(adapter.handle_request(1, "initialize", None), "initialize")?;
    let _initialized = wait_for_event(&rx, "initialized", timeout)?;

    let attach = response_success(
        adapter.handle_request(
            2,
            "attach",
            Some(json!({
                "host": "127.0.0.1",
                "port": port,
                "timeout": 2000,
                "stopOnEntry": false
            })),
        ),
        "attach",
    );
    if let Err(error) = attach {
        let _ = release_tx.send(());
        server_handle
            .join()
            .map_err(|_| std::io::Error::other("fake TCP debugger server panicked"))?
            .map_err(std::io::Error::other)?;
        return Err(error.into());
    }

    let mut failure = None;
    loop {
        match rx.try_recv() {
            Err(TryRecvError::Empty) => break,
            Ok(DapMessage::Event { ref event, .. }) if event == "stopped" => {
                failure = Some("attach emitted a stop before the peer did".into());
                break;
            }
            Ok(_) => {}
            Err(TryRecvError::Disconnected) => {
                failure = Some("event channel disconnected before peer stop".into());
                break;
            }
        }
    }

    let _ = release_tx.send(());
    if failure.is_none() {
        let deadline = Instant::now() + timeout;
        let mut peer_stop_seen = false;
        while !peer_stop_seen && failure.is_none() {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                failure = Some("timed out waiting for peer stop".into());
                break;
            }
            match rx.recv_timeout(remaining) {
                Ok(message) => match message {
                    DapMessage::Event { ref event, .. } if event == "stopped" => {
                        peer_stop_seen = true;
                        match event_body(&message) {
                            Some(stopped_body) => {
                                if stopped_body.get("reason").and_then(Value::as_str)
                                    != Some(peer_reason)
                                {
                                    failure = Some(format!(
                                        "peer stop reason was not {peer_reason}: {stopped_body:?}"
                                    ));
                                } else if stopped_body.get("threadId").and_then(Value::as_i64)
                                    != Some(1)
                                {
                                    failure = Some(format!(
                                        "peer thread id was not normalized: {stopped_body:?}"
                                    ));
                                }
                            }
                            None => failure = Some("stopped event missing body".into()),
                        }
                    }
                    DapMessage::Event { ref event, .. } if event == "terminated" => {
                        failure = Some("terminated arrived before peer stopped event".into());
                    }
                    _ => {}
                },
                Err(error) => {
                    failure = Some(format!("event stream failed before peer stop: {error}"));
                    break;
                }
            }
        }
    }

    let disconnect =
        response_success(adapter.handle_request(3, "disconnect", Some(json!({}))), "disconnect");
    if failure.is_none() {
        if let Err(error) = disconnect {
            failure = Some(error);
        } else {
            let deadline = Instant::now() + timeout;
            let mut terminated_seen = false;
            while !terminated_seen && failure.is_none() {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    failure = Some("timed out waiting for terminated".into());
                    break;
                }
                match rx.recv_timeout(remaining) {
                    Ok(message) => match message {
                        DapMessage::Event { ref event, .. } if event == "stopped" => {
                            failure =
                                Some(format!("stopped event arrived after peer stop: {message:?}"));
                        }
                        DapMessage::Event { ref event, .. } if event == "terminated" => {
                            terminated_seen = true;
                        }
                        _ => {}
                    },
                    Err(error) => {
                        failure = Some(format!("event stream failed before terminated: {error}"));
                    }
                }
            }
        }
    }

    let server_result = server_handle
        .join()
        .map_err(|_| std::io::Error::other("fake TCP debugger server panicked"))
        .and_then(|result| result.map_err(std::io::Error::other));
    if let Some(error) = failure {
        return Err(error.into());
    }
    server_result?;
    Ok(())
}

#[test]
fn dap_attach_e2e_tcp_stop_on_entry_is_rejected_before_connect() -> TestResult {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    listener.set_nonblocking(true)?;

    let mut adapter = DebugAdapter::new();
    let (tx, rx) = sync_channel(64);
    adapter.set_event_sender(tx);
    response_success(adapter.handle_request(1, "initialize", None), "initialize")?;
    let _initialized = wait_for_event(&rx, "initialized", smoke_timeout())?;

    let attach_response = adapter.handle_request(
        2,
        "attach",
        Some(json!({
            "host": "127.0.0.1",
            "port": port,
            "timeout": 2000,
            "stopOnEntry": true
        })),
    );

    // The refusal must happen before TCP connect. If the old implementation
    // accepted the request, close that connection before returning the failure
    // so the intentional pre-fix red remains bounded and cleanup-safe.
    let connected = match listener.accept() {
        Ok((socket, _)) => {
            drop(socket);
            true
        }
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => false,
        Err(error) => {
            let _ = adapter.handle_request(3, "disconnect", Some(json!({})));
            return Err(error.into());
        }
    };
    if connected {
        let _ = adapter.handle_request(3, "disconnect", Some(json!({})));
    }

    let message = response_failure_message(attach_response, "attach")?;
    if !message.contains("stopOnEntry") || !message.contains("TCP") {
        return Err(format!(
            "TCP stopOnEntry refusal must explain the unsupported adapter-controlled pause: {message}"
        )
        .into());
    }
    if connected {
        return Err("TCP refusal must occur before opening the peer socket".into());
    }

    while let Ok(message) = rx.try_recv() {
        if let DapMessage::Event { event, .. } = message {
            return Err(
                format!("refused attach must not publish postinitialize event `{event}`").into()
            );
        }
    }

    let threads_body = response_success(adapter.handle_request(4, "threads", None), "threads")?
        .ok_or("threads response missing body")?;
    let threads = threads_body
        .get("threads")
        .and_then(Value::as_array)
        .ok_or("threads response missing thread list")?;
    if !threads.is_empty() {
        return Err("refused attach must leave no active session".into());
    }
    Ok(())
}

#[test]
fn dap_attach_e2e_tcp_attach_timeout_returns_actionable_message() -> TestResult {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    drop(listener);

    let mut adapter = DebugAdapter::new();
    response_success(adapter.handle_request(1, "initialize", None), "initialize")?;

    let message = response_failure_message(
        adapter.handle_request(
            2,
            "attach",
            Some(json!({
                "host": "127.0.0.1",
                "port": port,
                "timeout": 250
            })),
        ),
        "attach",
    )?;

    assert!(message.contains("Cannot attach to Perl debugger at 127.0.0.1"));
    assert!(message.contains("(250ms timeout)"));
    assert!(message.contains("RemotePort=127.0.0.1"));

    Ok(())
}

#[test]
fn dap_attach_validation_errors_reach_the_request_response() -> TestResult {
    let cases = [
        (json!({"host": "", "port": 13603}), "Set the 'host' field"),
        (json!({"host": "localhost", "port": 0}), "Set the 'port' field"),
        (
            json!({"host": "127.0.0.1", "port": 13603, "timeout": 0}),
            "Timeout must be greater than 0 milliseconds",
        ),
        (
            json!({"host": "127.0.0.1", "port": 13603, "timeout": 300_001}),
            "Timeout cannot exceed 300000 milliseconds",
        ),
        (
            json!({
                "host": "does-not-resolve.invalid",
                "port": 13603,
                "timeout": 0
            }),
            "Timeout must be greater than 0 milliseconds",
        ),
    ];

    for (arguments, expected_guidance) in cases {
        let mut adapter = DebugAdapter::new();
        response_success(adapter.handle_request(1, "initialize", None), "initialize")?;
        let message = response_failure_message(
            adapter.handle_request(2, "attach", Some(arguments)),
            "attach",
        )?;
        assert!(
            message.contains(expected_guidance),
            "expected attach response to contain {expected_guidance:?}, got {message:?}"
        );
    }

    Ok(())
}
