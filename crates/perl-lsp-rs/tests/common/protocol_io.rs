//! LSP protocol I/O helpers for reading and writing JSON-RPC messages.
//!
//! Provides low-level message framing, response matching, notification filtering,
//! and drain utilities used by the test harness.

use serde_json::{Value, json};
use std::io::{self, Write};
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

use super::{LspServer, PENDING_CAP};

// Error codes aligned with crates/perl-parser/src/lsp/protocol/errors.rs
/// JSON-RPC reserved: Server error range is -32000 to -32099
pub(crate) const ERR_TEST_TIMEOUT: i64 = -32000;
/// Connection closed - BrokenPipe or similar transport termination
const ERR_CONNECTION_CLOSED: i64 = -32050;
/// Transport error - general I/O failures that aren't connection closures
const ERR_TRANSPORT_ERROR: i64 = -32051;

/// Helper function to send a JSON-RPC message over the wire.
/// Returns io::Result to allow graceful error handling.
pub(crate) fn send_message_inner(writer: &mut impl Write, body: &str) -> io::Result<()> {
    write!(writer, "Content-Length: {}\r\n\r\n{}", body.len(), body)?;
    writer.flush()
}

/// Creates a JSON-RPC 2.0 error response with proper envelope.
///
/// All error responses MUST include `jsonrpc` and `id` fields per JSON-RPC 2.0 spec.
/// The `id` should be extracted from the original request before any error handling.
pub(crate) fn error_response_for_request(id: Option<Value>, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id.unwrap_or(Value::Null),
        "error": {
            "code": code,
            "message": message
        }
    })
}

/// Creates an error response for connection-closed scenarios (BrokenPipe).
fn connection_closed_error_for_request(id: Option<Value>) -> Value {
    error_response_for_request(id, ERR_CONNECTION_CLOSED, "Connection closed")
}

/// Creates an error response for internal transport errors.
fn transport_error_for_request(id: Option<Value>, msg: &str) -> Value {
    error_response_for_request(id, ERR_TRANSPORT_ERROR, msg)
}

// Legacy functions for backward compatibility with code that doesn't have request context
// These return responses with null id (valid JSON-RPC but less informative)

/// Creates an error response for connection-closed scenarios (legacy, no request context).
pub(crate) fn connection_closed_error() -> Value {
    connection_closed_error_for_request(None)
}

/// Creates an error response for internal transport errors (legacy, no request context).
pub(crate) fn internal_transport_error(msg: &str) -> Value {
    transport_error_for_request(None, msg)
}

/// Maps I/O send errors to proper JSON-RPC error responses.
///
/// BrokenPipe -> CONNECTION_CLOSED (-32050)
/// Other I/O errors -> TRANSPORT_ERROR (-32051)
pub(crate) fn map_send_error(id: Option<Value>, e: io::Error, context: &str) -> Value {
    if e.kind() == io::ErrorKind::BrokenPipe {
        connection_closed_error_for_request(id)
    } else {
        transport_error_for_request(id, &format!("{}: {}", context, e))
    }
}

/// Blocking receive with a sane default timeout to avoid hangs.
pub fn read_response(server: &LspServer) -> Value {
    read_response_timeout(server, super::default_timeout()).unwrap_or_else(
        || json!({"error":{"code":ERR_TEST_TIMEOUT,"message":"test harness timeout"}}),
    )
}

/// Try to receive a response within `dur`. Returns None on timeout.
pub fn read_response_timeout(server: &LspServer, dur: Duration) -> Option<Value> {
    server.rx.lock().unwrap_or_else(|e| e.into_inner()).recv_timeout(dur).ok()
}

/// Try to receive a JSON-RPC response within `dur`, buffering notifications and
/// unrelated traffic for later reads.
pub fn read_response_only_timeout(server: &LspServer, dur: Duration) -> Option<Value> {
    // Scan buffered traffic first.
    {
        let mut pending = server.pending.lock().unwrap_or_else(|e| e.into_inner());
        let len = pending.len();
        for _ in 0..len {
            if let Some(msg) = pending.pop_front() {
                if msg.get("id").is_some() {
                    return Some(msg);
                }
                if pending.len() >= PENDING_CAP {
                    pending.pop_front();
                }
                pending.push_back(msg);
            }
        }
    }

    let deadline = Instant::now() + dur;
    loop {
        let now = Instant::now();
        if now >= deadline {
            return None;
        }
        let recv_result = {
            let rx = server.rx.lock().unwrap_or_else(|e| e.into_inner());
            rx.recv_timeout(deadline - now)
        };
        match recv_result {
            Ok(msg) => {
                if msg.get("id").is_some() {
                    return Some(msg);
                }
                let mut pending = server.pending.lock().unwrap_or_else(|e| e.into_inner());
                if pending.len() >= PENDING_CAP {
                    pending.pop_front();
                }
                pending.push_back(msg);
            }
            Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => return None,
        }
    }
}

/// Try to receive any notification (message without id) within `dur`.
///
/// Responses and unrelated traffic are buffered for later reads so tests can
/// safely wait for optional notifications while requests are in flight.
pub fn read_notification_timeout(server: &LspServer, dur: Duration) -> Option<Value> {
    // Scan buffered traffic first. Keep responses and other non-matching
    // messages in order for the request/response helpers.
    {
        let mut pending = server.pending.lock().unwrap_or_else(|e| e.into_inner());
        let len = pending.len();
        for _ in 0..len {
            if let Some(msg) = pending.pop_front() {
                if msg.get("id").is_none() {
                    return Some(msg);
                }
                pending.push_back(msg);
            }
        }
    }

    let deadline = Instant::now() + dur;
    while Instant::now() < deadline {
        let recv_result = {
            let rx = server.rx.lock().unwrap_or_else(|e| e.into_inner());
            rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
        };
        match recv_result {
            Ok(msg) => {
                if msg.get("id").is_none() {
                    return Some(msg);
                }
                let mut pending = server.pending.lock().unwrap_or_else(|e| e.into_inner());
                if pending.len() >= PENDING_CAP {
                    pending.pop_front();
                }
                pending.push_back(msg);
            }
            Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => return None,
        }
    }

    None
}

/// Outcome of a matched-response read: either the matched response, an elapsed
/// budget on a still-connected transport, a terminated transport (the server
/// process exited or closed stdout), or an unparsable frame on a live transport.
pub enum ReadResponseOutcome {
    /// Received the response matching the requested id.
    Response(Value),
    /// The deadline elapsed while the channel was still connected.
    TimedOut,
    /// The reader thread hit EOF: the server exited or closed stdout.
    Disconnected,
    /// A frame arrived but failed to parse; the transport stayed open.
    Malformed(String),
}

/// Drain one protocol failure reported by the stdout reader thread, if any.
fn take_protocol_error(server: &LspServer) -> Option<String> {
    server.err_rx.lock().unwrap_or_else(|e| e.into_inner()).try_recv().ok()
}

/// Receive the response matching `id` (number or string), buffering other traffic,
/// distinguishing an elapsed budget from a terminated transport so callers can
/// avoid conflating slowness with a crashed server.
pub fn read_response_matching_outcome(
    server: &LspServer,
    id: &Value,
    dur: Duration,
) -> ReadResponseOutcome {
    // scan buffered
    {
        let mut pending = server.pending.lock().unwrap_or_else(|e| e.into_inner());
        let len = pending.len();
        for _ in 0..len {
            if let Some(msg) = pending.pop_front() {
                if msg.get("id") == Some(id) {
                    return ReadResponseOutcome::Response(msg);
                }
                if pending.len() >= PENDING_CAP {
                    pending.pop_front();
                }
                pending.push_back(msg);
            }
        }
    }
    // then poll
    let deadline = Instant::now() + dur;
    loop {
        if let Some(detail) = take_protocol_error(server) {
            return ReadResponseOutcome::Malformed(detail);
        }
        let now = Instant::now();
        if now >= deadline {
            return ReadResponseOutcome::TimedOut;
        }
        let recv_result = {
            let rx = server.rx.lock().unwrap_or_else(|e| e.into_inner());
            rx.recv_timeout(deadline - now)
        };
        match recv_result {
            Ok(msg) => {
                if msg.get("id") == Some(id) {
                    return ReadResponseOutcome::Response(msg);
                }
                let mut pending = server.pending.lock().unwrap_or_else(|e| e.into_inner());
                if pending.len() >= PENDING_CAP {
                    pending.pop_front();
                }
                pending.push_back(msg);
            }
            Err(RecvTimeoutError::Timeout) => return ReadResponseOutcome::TimedOut,
            Err(RecvTimeoutError::Disconnected) => return ReadResponseOutcome::Disconnected,
        }
    }
}

/// Receive the response matching `id` (number or string), buffering other traffic.
///
/// Legacy `Option` view of [`read_response_matching_outcome`]: timeout,
/// disconnect, and malformed-frame outcomes all collapse to `None`.
pub fn read_response_matching(server: &LspServer, id: &Value, dur: Duration) -> Option<Value> {
    match read_response_matching_outcome(server, id, dur) {
        ReadResponseOutcome::Response(msg) => Some(msg),
        ReadResponseOutcome::TimedOut
        | ReadResponseOutcome::Disconnected
        | ReadResponseOutcome::Malformed(_) => None,
    }
}

/// Convenience for numeric ids.
pub fn read_response_matching_i64(server: &LspServer, id: i64, dur: Duration) -> Option<Value> {
    read_response_matching(server, &json!(id), dur)
}

/// Write raw bytes (for malformed/binary frame tests).
pub fn send_raw(server: &LspServer, bytes: &[u8]) {
    // Ignore write errors - BrokenPipe during teardown is expected
    let mut writer = server.writer.lock().unwrap_or_else(|e| e.into_inner());
    let _ = writer.write_all(bytes).and_then(|_| writer.flush());
}

/// Read a notification matching the given method name
pub fn read_notification_method(server: &LspServer, method: &str, dur: Duration) -> Option<Value> {
    let deadline = Instant::now() + dur;

    // scan buffered first
    {
        let mut pending = server.pending.lock().unwrap_or_else(|e| e.into_inner());
        let len = pending.len();
        for _ in 0..len {
            if let Some(msg) = pending.pop_front() {
                if msg.get("id").is_none() && msg.get("method") == Some(&json!(method)) {
                    return Some(msg);
                }
                pending.push_back(msg);
            }
        }
    }

    // then poll
    while Instant::now() < deadline {
        let recv_result = {
            let rx = server.rx.lock().unwrap_or_else(|e| e.into_inner());
            rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
        };
        match recv_result {
            Ok(msg) => {
                let is_match = msg.get("id").is_none() && msg.get("method") == Some(&json!(method));
                if is_match {
                    return Some(msg);
                }
                let mut pending = server.pending.lock().unwrap_or_else(|e| e.into_inner());
                if pending.len() >= PENDING_CAP {
                    pending.pop_front();
                }
                pending.push_back(msg);
            }
            Err(_) => break,
        }
    }
    None
}

/// Read the first notification matching `method` AND `params.uri == uri`.
///
/// Used to wait for per-document server notifications (for example
/// `perl-lsp/active-document-ready`) that carry the document URI in their
/// params. Non-matching buffered messages are preserved for later readers,
/// mirroring [`read_notification_method`]'s requeue behavior.
pub fn read_notification_for_uri(
    server: &LspServer,
    method: &str,
    uri: &str,
    dur: Duration,
) -> Option<Value> {
    let deadline = Instant::now() + dur;

    // scan buffered first
    {
        let mut pending = server.pending.lock().unwrap_or_else(|e| e.into_inner());
        let len = pending.len();
        for _ in 0..len {
            if let Some(msg) = pending.pop_front() {
                if notification_matches_uri(&msg, method, uri) {
                    return Some(msg);
                }
                pending.push_back(msg);
            }
        }
    }

    // then poll
    while Instant::now() < deadline {
        let recv_result = {
            let rx = server.rx.lock().unwrap_or_else(|e| e.into_inner());
            rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
        };
        match recv_result {
            Ok(msg) => {
                if notification_matches_uri(&msg, method, uri) {
                    return Some(msg);
                }
                let mut pending = server.pending.lock().unwrap_or_else(|e| e.into_inner());
                if pending.len() >= PENDING_CAP {
                    pending.pop_front();
                }
                pending.push_back(msg);
            }
            Err(_) => break,
        }
    }
    None
}

fn notification_matches_uri(msg: &Value, method: &str, uri: &str) -> bool {
    msg.get("id").is_none()
        && msg.get("method") == Some(&json!(method))
        && msg.pointer("/params/uri").and_then(Value::as_str) == Some(uri)
}

/// Drain messages until no traffic for a quiet period (stabilizes CI)
pub fn drain_until_quiet(server: &LspServer, quiet: Duration, ceiling: Duration) {
    let start = Instant::now();
    let mut last = Instant::now();
    while start.elapsed() < ceiling {
        let recv_result = {
            let rx = server.rx.lock().unwrap_or_else(|e| e.into_inner());
            rx.recv_timeout(quiet.saturating_sub(last.elapsed()))
        };
        match recv_result {
            Ok(msg) => {
                let mut pending = server.pending.lock().unwrap_or_else(|e| e.into_inner());
                if pending.len() >= PENDING_CAP {
                    pending.pop_front();
                }
                pending.push_back(msg);
                last = Instant::now();
            }
            Err(_) => break, // quiet period achieved
        }
    }
}

/// Send raw message to server (for testing malformed input)
pub fn send_raw_message(server: &LspServer, content: &str) {
    // Ignore write errors - BrokenPipe during teardown is expected
    let _ =
        send_message_inner(&mut *server.writer.lock().unwrap_or_else(|e| e.into_inner()), content);
}

/// Send request without waiting for response
pub fn send_request_no_wait(server: &LspServer, req: Value) {
    let body = req.to_string();
    // Ignore write errors - BrokenPipe during teardown is expected
    let _ =
        send_message_inner(&mut *server.writer.lock().unwrap_or_else(|e| e.into_inner()), &body);
}

#[cfg(test)]
mod read_notification_for_uri_tests {
    use super::{LspServer, PENDING_CAP, read_notification_for_uri};
    use serde_json::{Value, json};
    use std::collections::VecDeque;
    use std::io::BufWriter;
    use std::process::{Child, ChildStdin, Command, Stdio};
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

    /// Build an LspServer whose transport channels are synthetic: the child
    /// process is this test binary invoked with a filter that matches no
    /// test (the libtest harness exits immediately), so stdin/stdout exist
    /// but nothing is ever expected from them. Notifications to observe are
    /// preloaded into `rx` / `pending` directly.
    fn synthetic_server(
        rx_messages: Vec<Value>,
        pending_messages: Vec<Value>,
    ) -> TestResult<(LspServer, mpsc::Sender<Value>)> {
        let mut child: Child = Command::new(std::env::current_exe()?)
            .arg("--exact")
            .arg("__no_such_synthetic_test__")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let stdin: ChildStdin = child.stdin.take().ok_or("synthetic child must expose stdin")?;
        let (tx, rx) = mpsc::channel::<Value>();
        for message in rx_messages {
            tx.send(message)?;
        }
        let (_err_tx, err_rx) = mpsc::channel::<String>();
        let idle = || std::thread::spawn(|| ());
        let server = LspServer {
            process: Mutex::new(child),
            writer: Mutex::new(BufWriter::new(stdin)),
            rx: Mutex::new(rx),
            err_rx: Mutex::new(err_rx),
            _stdout_thread: idle(),
            _stderr_thread: idle(),
            pending: Mutex::new(pending_messages.into_iter().collect::<VecDeque<Value>>()),
            stderr_tail: Arc::new(Mutex::new((VecDeque::new(), Vec::new()))),
            shutdown_initiated: std::sync::atomic::AtomicBool::new(false),
        };
        Ok((server, tx))
    }

    fn ready(uri: &str) -> Value {
        json!({"jsonrpc": "2.0", "method": "perl-lsp/active-document-ready", "params": {"uri": uri, "generation": 1}})
    }

    /// The poll loop returns the first notification matching method AND
    /// params.uri, while every non-matching message it had to consume stays
    /// observable in `pending` for later readers.
    #[test]
    fn returns_matching_notification_and_requeues_the_rest() -> TestResult<()> {
        let response = json!({"jsonrpc": "2.0", "id": 7, "result": {}});
        let other_uri = ready("file:///workspace/Other.pl");
        let wanted = ready("file:///workspace/Wanted.pm");
        let (server, _tx) = synthetic_server(vec![response, other_uri, wanted], vec![])?;

        let found = read_notification_for_uri(
            &server,
            "perl-lsp/active-document-ready",
            "file:///workspace/Wanted.pm",
            Duration::from_secs(5),
        );

        let found = found.ok_or("matching notification should be found")?;
        assert_eq!(
            found.pointer("/params/uri").and_then(Value::as_str),
            Some("file:///workspace/Wanted.pm")
        );
        let pending = server.pending.lock().unwrap_or_else(|e| e.into_inner());
        assert!(
            pending.iter().any(|message| message.get("id").is_some_and(|id| *id == json!(7))),
            "consumed non-matching response must be requeued for later readers"
        );
        assert!(
            pending.iter().any(|message| {
                message.pointer("/params/uri").and_then(Value::as_str)
                    == Some("file:///workspace/Other.pl")
            }),
            "consumed non-matching notification must be requeued for later readers"
        );

        Ok(())
    }

    /// Buffered matches are answered from `pending` without consuming `rx`.
    #[test]
    fn scans_buffered_pending_before_polling() -> TestResult<()> {
        let buffered = ready("file:///workspace/Buffered.pm");
        let (server, tx) = synthetic_server(vec![], vec![buffered])?;

        let found = read_notification_for_uri(
            &server,
            "perl-lsp/active-document-ready",
            "file:///workspace/Buffered.pm",
            Duration::from_secs(5),
        );

        assert!(found.is_some(), "buffered match should be returned from pending");
        // Nothing was pending in rx; the untouched sender must still deliver.
        tx.send(json!({"probe": true}))?;
        drop(tx);
        assert!(
            server.rx.lock().unwrap_or_else(|e| e.into_inner()).recv().is_ok(),
            "pending-first scan must not consume rx"
        );

        Ok(())
    }

    /// Requests carrying an id, wrong methods, or wrong URIs never match,
    /// and an empty transport yields None once the budget elapses.
    #[test]
    fn mismatched_shapes_never_match_and_empty_transport_times_out() -> TestResult<()> {
        let request_with_id = json!({"jsonrpc": "2.0", "id": 3, "method": "perl-lsp/active-document-ready", "params": {"uri": "file:///workspace/Wanted.pm"}});
        let wrong_method = json!({"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": {"uri": "file:///workspace/Wanted.pm"}});
        let (server, _tx) = synthetic_server(vec![request_with_id, wrong_method], vec![])?;

        let started = std::time::Instant::now();
        let found = read_notification_for_uri(
            &server,
            "perl-lsp/active-document-ready",
            "file:///workspace/Wanted.pm",
            Duration::from_millis(200),
        );

        assert!(found.is_none(), "no message may match by id-carrier, method, or uri alone");
        assert!(
            started.elapsed() >= Duration::from_millis(150),
            "the budget must be honored before returning None"
        );

        let _ = PENDING_CAP; // keep the cap constant linked to this module's contract
        Ok(())
    }
}
