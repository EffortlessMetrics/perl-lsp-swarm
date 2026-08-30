//! Outbound message channel
//!
//! Decouples message serialization from I/O by sending outbound messages through
//! a bounded channel to a dedicated writer thread. This eliminates the writer
//! lock as a contention point and enables concurrent handler execution.
//!
//! ## Backpressure policy
//!
//! The channel capacity is capped at [`OUTBOUND_CAPACITY`] (matching the inbound
//! scheduler queues in `runtime/scheduler.rs`). Producers call [`try_send`] — the
//! non-blocking variant — so they never block or deadlock when the channel is full.
//! When the channel is full, `send_*` methods return [`io::ErrorKind::WouldBlock`],
//! signalling that the client is not consuming messages fast enough. Callers that
//! propagate this error via `?` will tear down the request, which is the correct
//! backpressure response: a client that can't keep up should not receive more work.
//!
//! ## Deadlock analysis
//!
//! `try_send` is non-blocking — it never waits on the consumer. The writer thread
//! holds the `output` lock (for `spawn_writer_shared`) only while performing the
//! actual write, and it reads from the channel via `blocking_recv`/`try_recv` with
//! no other lock held. No producer holds a lock when calling `try_send`. Therefore
//! there is no circular lock+channel dependency and deadlock is impossible.

#[cfg(test)]
use crate::protocol::JsonRpcId;
use crate::protocol::JsonRpcResponse;
use crate::runtime::types::ServerRequestId;
use crate::transport::frame;
use serde_json::{Value, json};
use std::io::{self, Write};
use std::thread;

/// Capacity of the bounded outbound message channel.
///
/// Matches the `QUEUE_CAPACITY` constant in `runtime/scheduler.rs` (64) so that
/// the outbound queue applies the same memory budget as the inbound queues.
/// A slow or disconnected client will fill this buffer before backpressure kicks
/// in, bounding peak memory usage for outbound messages to roughly
/// 64 × (max serialized message size).
pub(crate) const OUTBOUND_CAPACITY: usize = 64;

/// An outbound LSP message (response, notification, or server→client request).
pub(crate) enum OutboundMessage {
    /// JSON-RPC response to a client request.
    Response(JsonRpcResponse),
    /// JSON-RPC notification (no id, no response expected).
    Notification { method: String, params: Value },
    /// JSON-RPC request from server to client (has id, expects response).
    Request { id: ServerRequestId, method: String, params: Value },
}

/// Trait abstracting the outbound message channel.
///
/// This trait decouples LSP handlers from the concrete `OutboundSender`,
/// enabling unit tests to use a mock sink without constructing the full
/// 60-field `LspServer`. Production code uses `OutboundSender`; tests
/// use `RecordingSink` or their own implementations (#5015 PR-1).
///
/// The trait intentionally mirrors the three `send_*` methods on
/// `OutboundSender` so the production impl is a zero-cost passthrough.
pub(crate) trait OutboundSink {
    /// Send a JSON-RPC response to the client.
    fn send_response(&self, response: JsonRpcResponse) -> io::Result<()>;

    /// Send a JSON-RPC notification to the client.
    fn send_notification(&self, method: &str, params: Value) -> io::Result<()>;

    /// Send a server→client JSON-RPC request.
    fn send_request(&self, id: ServerRequestId, method: &str, params: Value) -> io::Result<()>;
}

/// Recording sink for tests — captures all sent messages for assertions.
#[cfg(test)]
pub(crate) struct RecordingSink {
    pub messages: std::sync::Mutex<Vec<OutboundMessage>>,
}

// This RecordingSink test helper is `#[cfg(test)]`-only and uses `unwrap()`
// on its own private mutex, which is never contended across a panic
// boundary; the workspace-wide deny is a production-code rule.
#[cfg(test)]
#[allow(clippy::unwrap_used)]
impl RecordingSink {
    pub(crate) fn new() -> Self {
        Self { messages: std::sync::Mutex::new(Vec::new()) }
    }

    /// Drain and return all recorded messages.
    pub(crate) fn drain(&self) -> Vec<OutboundMessage> {
        std::mem::take(&mut *self.messages.lock().unwrap())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
impl OutboundSink for RecordingSink {
    fn send_response(&self, response: JsonRpcResponse) -> io::Result<()> {
        self.messages.lock().unwrap().push(OutboundMessage::Response(response));
        Ok(())
    }

    fn send_notification(&self, method: &str, params: Value) -> io::Result<()> {
        self.messages
            .lock()
            .unwrap()
            .push(OutboundMessage::Notification { method: method.to_string(), params });
        Ok(())
    }

    fn send_request(&self, id: ServerRequestId, method: &str, params: Value) -> io::Result<()> {
        self.messages.lock().unwrap().push(OutboundMessage::Request {
            id,
            method: method.to_string(),
            params,
        });
        Ok(())
    }
}

/// Cloneable handle for sending outbound messages.
///
/// Multiple tasks/threads can hold a clone and send concurrently;
/// all messages are serialized by the single writer thread.
#[derive(Clone)]
pub(crate) struct OutboundSender {
    tx: tokio::sync::mpsc::Sender<OutboundMessage>,
}

/// Map a [`tokio::sync::mpsc::error::TrySendError`] to an [`io::Error`].
///
/// - `Closed` → `BrokenPipe` (writer thread has exited)
/// - `Full`   → `WouldBlock` (client is not consuming fast enough; caller should
///   propagate this to tear down the in-flight request — the correct backpressure
///   response)
fn map_try_send_error<T>(e: tokio::sync::mpsc::error::TrySendError<T>) -> io::Error {
    match e {
        tokio::sync::mpsc::error::TrySendError::Full(_) => {
            io::Error::new(io::ErrorKind::WouldBlock, "outbound channel full")
        }
        tokio::sync::mpsc::error::TrySendError::Closed(_) => {
            io::Error::new(io::ErrorKind::BrokenPipe, "outbound channel closed")
        }
    }
}

impl OutboundSender {
    /// Send a JSON-RPC response.
    pub fn send_response(&self, response: JsonRpcResponse) -> io::Result<()> {
        self.tx.try_send(OutboundMessage::Response(response)).map_err(map_try_send_error)
    }

    /// Send a JSON-RPC notification.
    ///
    /// Method-direction admission (#8896 §3): a frame whose method is
    /// positively registered client→server can only be a reversed-direction
    /// bug, so it is refused here instead of reaching the client.
    pub fn send_notification(&self, method: &str, params: Value) -> io::Result<()> {
        if let Some(reason) = crate::protocol::method_direction::outbound_rejection(method) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("outbound notification `{method}` {reason}"),
            ));
        }
        self.tx
            .try_send(OutboundMessage::Notification { method: method.to_string(), params })
            .map_err(map_try_send_error)
    }

    /// Send a server→client JSON-RPC request.
    ///
    /// Method-direction admission (#8896 §3), mirroring [`Self::send_notification`].
    pub(crate) fn send_request(
        &self,
        id: ServerRequestId,
        method: &str,
        params: Value,
    ) -> io::Result<()> {
        if let Some(reason) = crate::protocol::method_direction::outbound_rejection(method) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("outbound request `{method}` {reason}"),
            ));
        }
        self.tx
            .try_send(OutboundMessage::Request { id, method: method.to_string(), params })
            .map_err(map_try_send_error)
    }
}

impl OutboundSink for OutboundSender {
    fn send_response(&self, response: JsonRpcResponse) -> io::Result<()> {
        OutboundSender::send_response(self, response)
    }

    fn send_notification(&self, method: &str, params: Value) -> io::Result<()> {
        OutboundSender::send_notification(self, method, params)
    }

    fn send_request(&self, id: ServerRequestId, method: &str, params: Value) -> io::Result<()> {
        OutboundSender::send_request(self, id, method, params)
    }
}

/// Create an `OutboundSender` backed by a writer thread.
///
/// Returns the sender handle and a join-handle for the writer thread.
/// The writer thread runs until the last sender is dropped (channel closes),
/// then resolves to its [`WriterTerminalOutcome`] so connection/session
/// settlement can retain the first causal transport failure (#8402).
pub(crate) fn spawn_writer(
    output: Box<dyn Write + Send>,
) -> (OutboundSender, thread::JoinHandle<WriterTerminalOutcome>) {
    let (tx, rx) = tokio::sync::mpsc::channel(OUTBOUND_CAPACITY);
    let handle = thread::spawn(move || writer_loop_batched(rx, output));
    (OutboundSender { tx }, handle)
}

/// Create an `OutboundSender` backed by a shared `Arc<Mutex<Box<dyn Write + Send>>>`.
///
/// Backward-compatible variant for `with_output()` constructors.
pub(crate) fn spawn_writer_shared(
    output: std::sync::Arc<parking_lot::Mutex<Box<dyn Write + Send>>>,
) -> (OutboundSender, thread::JoinHandle<WriterTerminalOutcome>) {
    let (tx, rx) = tokio::sync::mpsc::channel(OUTBOUND_CAPACITY);
    let handle = thread::spawn(move || writer_loop_batched_shared(rx, output));
    (OutboundSender { tx }, handle)
}

/// Create an already-closed sender for shutdown replacement paths.
pub(crate) fn closed_sender() -> OutboundSender {
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    drop(rx);
    OutboundSender { tx }
}

/// Terminal outcome of the outbound writer thread (#8402).
///
/// The writer loop stops at its first sink I/O failure and records that first
/// causal outcome; later channel-closed observations by producers (`BrokenPipe`
/// from [`map_try_send_error`]) can never overwrite it, because the thread has
/// already exited with the outcome fixed.
///
/// A distinct `shutdown` outcome is intentionally absent: the writer's only
/// termination signal today is channel close (whether from Drop settlement or
/// an explicit shutdown path), which maps to [`WriterTerminalOutcome::NormalClose`]
/// when no I/O failure occurred. Runtime shutdown ownership (#8388) can layer an
/// explicit distinction on top without changing this shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WriterTerminalOutcome {
    /// The channel closed and every batch was written and flushed with no I/O
    /// failure. Non-error settlement.
    NormalClose,
    /// `write_all` failed. `batch_bytes` frame bytes were not confirmed
    /// delivered; `queued` messages were accepted by the channel but never
    /// attempted.
    WriteFailed { kind: io::ErrorKind, queued: usize, batch_bytes: usize },
    /// `write_all` succeeded but `flush` failed. `batch_bytes` were handed to
    /// the sink but delivery is not confirmed; `queued` messages were accepted
    /// by the channel but never attempted.
    FlushFailed { kind: io::ErrorKind, queued: usize, batch_bytes: usize },
}

impl WriterTerminalOutcome {
    /// True only when the writer terminated on an outbound sink I/O failure.
    /// Normal close/shutdown is non-error.
    pub(crate) fn is_io_failure(&self) -> bool {
        !matches!(self, WriterTerminalOutcome::NormalClose)
    }

    /// Conservative count of accepted messages that may not have been
    /// delivered, or `None` when the writer closed normally with every batch
    /// written and flushed. The batch messages and the still-queued depth are
    /// counted; message payloads are never retained (bounded context).
    pub(crate) fn possibly_undelivered_messages(&self) -> Option<usize> {
        match self {
            WriterTerminalOutcome::NormalClose => None,
            WriterTerminalOutcome::WriteFailed { queued, batch_bytes, .. } => {
                // Every frame in the failed batch came from an accepted
                // message; approximate the message count as at least one.
                Some(queued + usize::from(*batch_bytes > 0))
            }
            WriterTerminalOutcome::FlushFailed { queued, batch_bytes, .. } => {
                Some(queued + usize::from(*batch_bytes > 0))
            }
        }
    }

    /// Structured settlement evidence at connection/session shutdown (#8402).
    /// The writer thread itself never blocks on reporting; the joining thread
    /// consumes the terminal outcome at settlement time.
    pub(crate) fn report_settlement(&self) {
        if !self.is_io_failure() {
            tracing::debug!("outbound writer settled: normal channel close, no I/O failure");
            return;
        }
        let (phase, kind, queued, batch_bytes) = match self {
            WriterTerminalOutcome::NormalClose => return,
            WriterTerminalOutcome::WriteFailed { kind, queued, batch_bytes } => {
                ("write", kind, queued, batch_bytes)
            }
            WriterTerminalOutcome::FlushFailed { kind, queued, batch_bytes } => {
                ("flush(written-bytes-unconfirmed)", kind, queued, batch_bytes)
            }
        };
        tracing::error!(
            phase,
            error_kind = %kind,
            queued_messages = queued,
            batch_bytes = batch_bytes,
            possibly_undelivered = ?self.possibly_undelivered_messages(),
            "outbound writer settled: transport I/O failure; accepted messages may not have been delivered"
        );
    }
}

/// Blocking receive loop with message batching.
///
/// Drains the channel and writes all immediately-available messages
/// in a single write+flush cycle, reducing syscalls under burst load.
///
/// Returns the first causal terminal outcome: the loop exits at the first
/// sink I/O failure, so later producer observations cannot overwrite it.
fn writer_loop_batched(
    mut rx: tokio::sync::mpsc::Receiver<OutboundMessage>,
    mut output: Box<dyn Write + Send>,
) -> WriterTerminalOutcome {
    let mut batch_buf = Vec::with_capacity(4096);
    while let Some(msg) = rx.blocking_recv() {
        // Serialize first message.
        let bytes = serialize_message(&msg);
        let framed = frame(&bytes);
        batch_buf.extend_from_slice(&framed);

        // Drain any immediately available messages (coalescing).
        while let Ok(msg) = rx.try_recv() {
            let bytes = serialize_message(&msg);
            let framed = frame(&bytes);
            batch_buf.extend_from_slice(&framed);
        }

        // Single write+flush for the whole batch.
        let queued = rx.len();
        if let Err(e) = output.write_all(&batch_buf) {
            return WriterTerminalOutcome::WriteFailed {
                kind: e.kind(),
                queued,
                batch_bytes: batch_buf.len(),
            };
        }
        if let Err(e) = output.flush() {
            return WriterTerminalOutcome::FlushFailed {
                kind: e.kind(),
                queued,
                batch_bytes: batch_buf.len(),
            };
        }
        batch_buf.clear();
    }
    WriterTerminalOutcome::NormalClose
}

/// Blocking receive loop with message batching for shared writer.
///
/// Same coalescing strategy as [`writer_loop_batched`] but acquires the
/// shared lock once per batch rather than once per message.
fn writer_loop_batched_shared(
    mut rx: tokio::sync::mpsc::Receiver<OutboundMessage>,
    output: std::sync::Arc<parking_lot::Mutex<Box<dyn Write + Send>>>,
) -> WriterTerminalOutcome {
    let mut batch_buf = Vec::with_capacity(4096);
    while let Some(msg) = rx.blocking_recv() {
        // Serialize first message.
        let bytes = serialize_message(&msg);
        let framed = frame(&bytes);
        batch_buf.extend_from_slice(&framed);

        // Drain any immediately available messages (coalescing).
        while let Ok(msg) = rx.try_recv() {
            let bytes = serialize_message(&msg);
            let framed = frame(&bytes);
            batch_buf.extend_from_slice(&framed);
        }

        // Acquire lock once for the entire batch.
        let mut out = output.lock();
        let queued = rx.len();
        if let Err(e) = out.write_all(&batch_buf) {
            return WriterTerminalOutcome::WriteFailed {
                kind: e.kind(),
                queued,
                batch_bytes: batch_buf.len(),
            };
        }
        if let Err(e) = out.flush() {
            return WriterTerminalOutcome::FlushFailed {
                kind: e.kind(),
                queued,
                batch_bytes: batch_buf.len(),
            };
        }
        drop(out);
        batch_buf.clear();
    }
    WriterTerminalOutcome::NormalClose
}

/// Serialize an `OutboundMessage` to JSON bytes.
///
/// Returns an empty `Vec` on serialization failure and logs the error via
/// `tracing::error!` so callers have diagnostic visibility rather than a
/// silently-malformed empty frame being delivered to the client.
fn serialize_message(msg: &OutboundMessage) -> Vec<u8> {
    match msg {
        OutboundMessage::Response(resp) => serde_json::to_vec(resp).unwrap_or_else(|e| {
            tracing::error!("Failed to serialize outbound response: {e}");
            Vec::new()
        }),
        OutboundMessage::Notification { method, params } => {
            let val = json!({
                "jsonrpc": "2.0",
                "method": method,
                "params": params,
            });
            serde_json::to_vec(&val).unwrap_or_else(|e| {
                tracing::error!(method = %method, "Failed to serialize outbound notification: {e}");
                Vec::new()
            })
        }
        OutboundMessage::Request { id, method, params } => {
            let val = json!({
                "jsonrpc": "2.0",
                "id": id.as_i32(),
                "method": method,
                "params": params,
            });
            serde_json::to_vec(&val).unwrap_or_else(|e| {
                tracing::error!(id = id.as_i32(), method = %method, "Failed to serialize outbound request: {e}");
                Vec::new()
            })
        }
    }
}

#[cfg(test)]
mod tests {
    // Test assertions favor `unwrap()`/`panic!` over propagating errors;
    // the workspace-wide deny is a production-code rule.
    #![allow(clippy::unwrap_used, clippy::panic)]
    use super::*;
    use serde_json::json;
    use std::error::Error;
    use std::io::Write;
    use std::sync::Arc;

    #[derive(Clone, Default)]
    struct SharedBuffer {
        inner: Arc<parking_lot::Mutex<Vec<u8>>>,
    }

    impl SharedBuffer {
        fn new() -> Self {
            Self::default()
        }

        fn bytes(&self) -> Vec<u8> {
            self.inner.lock().clone()
        }
    }

    impl Write for SharedBuffer {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.inner.lock().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn parse_framed_payloads(raw: &[u8]) -> Result<Vec<serde_json::Value>, Box<dyn Error>> {
        let mut cursor = 0usize;
        let mut payloads = Vec::new();

        while cursor < raw.len() {
            let remainder = &raw[cursor..];
            let separator = b"\r\n\r\n";
            let Some(header_end) = remainder.windows(separator.len()).position(|w| w == separator)
            else {
                return Err("missing frame header separator".into());
            };
            let header_bytes = &remainder[..header_end];
            let header = std::str::from_utf8(header_bytes)?;
            let content_length = header
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length: "))
                .ok_or("missing Content-Length header")?
                .parse::<usize>()?;

            let body_start = cursor + header_end + separator.len();
            let body_end = body_start + content_length;
            if body_end > raw.len() {
                return Err("framed body shorter than declared Content-Length".into());
            }
            let body = &raw[body_start..body_end];
            payloads.push(serde_json::from_slice(body)?);
            cursor = body_end;
        }

        Ok(payloads)
    }

    #[test]
    fn closed_sender_returns_broken_pipe_for_all_message_types() -> Result<(), Box<dyn Error>> {
        let sender = closed_sender();
        let response_result =
            sender.send_response(JsonRpcResponse::success(Some(JsonRpcId::Integer(1)), json!({})));
        let notification_result = sender.send_notification("window/logMessage", json!({"x": 1}));
        let request_id = ServerRequestId::new(7).ok_or("valid id")?;
        let request_result = sender.send_request(request_id, "workspace/configuration", json!({}));

        for result in [response_result, notification_result, request_result] {
            assert!(
                matches!(result, Err(err) if err.kind() == std::io::ErrorKind::BrokenPipe),
                "closed outbound channel should return BrokenPipe"
            );
        }
        Ok(())
    }

    #[test]
    fn spawn_writer_serializes_response_notification_and_request() -> Result<(), Box<dyn Error>> {
        let buffer = SharedBuffer::new();
        let (sender, handle) = spawn_writer(Box::new(buffer.clone()));

        sender.send_response(JsonRpcResponse::success(
            Some(JsonRpcId::Integer(11)),
            json!({"ok": true}),
        ))?;
        sender.send_notification("window/logMessage", json!({"type": 3, "message": "hello"}))?;
        let request_id = ServerRequestId::new(42).ok_or("valid id")?;
        sender.send_request(request_id, "workspace/configuration", json!({"items": []}))?;

        drop(sender);
        let outcome = handle.join().map_err(|_| "writer thread panicked")?;
        assert_eq!(outcome, WriterTerminalOutcome::NormalClose);
        assert!(
            !outcome.is_io_failure(),
            "normal channel close must not be reported as an I/O failure"
        );
        assert_eq!(
            outcome.possibly_undelivered_messages(),
            None,
            "normal close must not claim undelivered accepted work"
        );

        let payloads = parse_framed_payloads(&buffer.bytes())?;
        assert_eq!(payloads.len(), 3);
        assert_eq!(payloads[0]["id"], 11);
        assert_eq!(payloads[0]["result"]["ok"], true);
        assert_eq!(payloads[1]["method"], "window/logMessage");
        assert_eq!(payloads[2]["id"], 42);
        assert_eq!(payloads[2]["method"], "workspace/configuration");

        Ok(())
    }

    #[test]
    fn spawn_writer_shared_serializes_payloads() -> Result<(), Box<dyn Error>> {
        let buffer = SharedBuffer::new();
        let shared =
            Arc::new(parking_lot::Mutex::new(Box::new(buffer.clone()) as Box<dyn Write + Send>));

        let (sender, handle) = spawn_writer_shared(Arc::clone(&shared));
        sender.send_notification("telemetry/event", json!({"name": "batch"}))?;
        let request_id = ServerRequestId::new(9).ok_or("valid id")?;
        sender.send_request(
            request_id,
            "client/registerCapability",
            json!({"registrations": []}),
        )?;

        drop(sender);
        let outcome = handle.join().map_err(|_| "writer thread panicked")?;
        assert_eq!(outcome, WriterTerminalOutcome::NormalClose);

        let payloads = parse_framed_payloads(&buffer.bytes())?;
        assert_eq!(payloads.len(), 2);
        assert_eq!(payloads[0]["method"], "telemetry/event");
        assert_eq!(payloads[1]["id"], 9);
        assert_eq!(payloads[1]["method"], "client/registerCapability");

        Ok(())
    }

    /// Sink whose `write` always fails with a fixed error kind and records how
    /// many bytes reached the write boundary at all.
    struct WriteFailsSink {
        write_kind: io::ErrorKind,
        attempted_bytes: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl Write for WriteFailsSink {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.attempted_bytes.fetch_add(buf.len(), std::sync::atomic::Ordering::SeqCst);
            Err(io::Error::new(self.write_kind, "controlled write failure (#8402)"))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// Sink whose `write` succeeds (bytes recorded) but whose `flush` always
    /// fails with a fixed error kind.
    struct FlushFailsSink {
        flush_kind: io::ErrorKind,
        written: SharedBuffer,
    }

    impl Write for FlushFailsSink {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.written.write(buf)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Err(io::Error::new(self.flush_kind, "controlled flush failure (#8402)"))
        }
    }

    /// #8402: a forced `write_all` failure must surface as the typed
    /// `WriteFailed` first cause with bounded context, never as the channel's
    /// `BrokenPipe`, and later producer observations (which only ever see the
    /// closed channel) cannot overwrite the recorded first cause.
    #[test]
    fn writer_write_failure_preserves_first_cause_against_broken_pipe_observations()
    -> Result<(), Box<dyn Error>> {
        // Distinct from BrokenPipe so a masquerading channel error cannot pass.
        let sink_kind = io::ErrorKind::ConnectionAborted;
        let attempted = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (sender, handle) = spawn_writer(Box::new(WriteFailsSink {
            write_kind: sink_kind,
            attempted_bytes: Arc::clone(&attempted),
        }));

        sender.send_notification("window/logMessage", json!({"n": 1}))?;
        // Keep a probe alive so the channel-closed surface can be exercised
        // after the writer has settled.
        let probe = sender.clone();
        drop(sender);

        let outcome = handle.join().map_err(|_| "writer thread panicked")?;

        // After writer death the receiver is gone: later producer observations
        // are Closed→BrokenPipe, never the causal sink error, and they cannot
        // overwrite the already-recorded first cause.
        let producer_error = probe
            .send_notification("window/logMessage", json!({"n": 2}))
            .err()
            .ok_or("send after writer death must fail")?;
        assert_eq!(
            producer_error.kind(),
            io::ErrorKind::BrokenPipe,
            "producers must observe the closed channel after writer death"
        );
        assert_ne!(
            producer_error.kind(),
            sink_kind,
            "producer channel error must not masquerade as the causal sink error"
        );
        match &outcome {
            WriterTerminalOutcome::WriteFailed { kind, batch_bytes, .. } => {
                assert_eq!(*kind, sink_kind, "first causal I/O class must be preserved");
                assert!(*batch_bytes > 0, "failed batch context must be bounded and non-empty");
            }
            other => panic!("expected WriteFailed first cause, got {other:?}"),
        }
        assert!(outcome.is_io_failure(), "write failure is an I/O failure settlement");
        assert!(
            outcome.possibly_undelivered_messages().unwrap_or(0) >= 1,
            "accepted messages must be represented conservatively as possibly undelivered"
        );
        assert!(
            attempted.load(std::sync::atomic::Ordering::SeqCst) > 0,
            "the failed batch must have reached the write boundary"
        );
        Ok(())
    }

    /// #8402: a forced `flush` failure must be recorded as the distinct
    /// `FlushFailed` outcome — never misclassified as a write failure — with
    /// the written-but-unconfirmed batch represented conservatively.
    #[test]
    fn writer_flush_failure_is_distinct_from_write_failure() -> Result<(), Box<dyn Error>> {
        let sink_kind = io::ErrorKind::BrokenPipe;
        let buffer = SharedBuffer::new();
        let (sender, handle) = spawn_writer(Box::new(FlushFailsSink {
            flush_kind: sink_kind,
            written: buffer.clone(),
        }));

        sender.send_notification("telemetry/event", json!({"x": 1}))?;
        drop(sender);

        let outcome = handle.join().map_err(|_| "writer thread panicked")?;
        match &outcome {
            WriterTerminalOutcome::FlushFailed { kind, batch_bytes, .. } => {
                assert_eq!(*kind, sink_kind, "flush failure class must be preserved");
                assert!(
                    *batch_bytes > 0,
                    "unconfirmed batch context must be bounded and non-empty"
                );
            }
            WriterTerminalOutcome::WriteFailed { .. } => {
                panic!("forced flush failure must not be misclassified as write failure")
            }
            other => panic!("expected FlushFailed outcome, got {other:?}"),
        }
        assert!(
            !buffer.bytes().is_empty(),
            "the batch was written to the sink even though flush failed"
        );
        assert!(
            outcome.possibly_undelivered_messages().unwrap_or(0) >= 1,
            "flush failure leaves delivery unconfirmed for accepted work"
        );
        Ok(())
    }

    /// #8402: the shared-writer variant must produce the same typed terminal
    /// outcomes as the owned-writer variant.
    #[test]
    fn shared_writer_flush_failure_keeps_typed_outcome() -> Result<(), Box<dyn Error>> {
        let sink_kind = io::ErrorKind::ConnectionReset;
        let buffer = SharedBuffer::new();
        let shared = Arc::new(parking_lot::Mutex::new(Box::new(FlushFailsSink {
            flush_kind: sink_kind,
            written: buffer.clone(),
        }) as Box<dyn Write + Send>));

        let (sender, handle) = spawn_writer_shared(shared);
        sender.send_notification("telemetry/event", json!({"x": 2}))?;
        drop(sender);

        let outcome = handle.join().map_err(|_| "writer thread panicked")?;
        assert!(
            matches!(
                &outcome,
                WriterTerminalOutcome::FlushFailed { kind, .. } if *kind == sink_kind
            ),
            "shared writer must keep the typed flush-failure outcome, got {outcome:?}"
        );
        Ok(())
    }

    /// Verify that the outbound channel is bounded: once the capacity is exhausted
    /// by a non-draining consumer, further sends return `WouldBlock` rather than
    /// queuing indefinitely.
    ///
    /// Strategy: create a `Sender` with a small test capacity (2 slots) directly,
    /// wrap it in an `OutboundSender`, fill both slots, then assert the third send
    /// fails with `WouldBlock`. This avoids spawning threads and is deterministic.
    #[test]
    fn outbound_sender_returns_would_block_when_channel_is_full() -> Result<(), Box<dyn Error>> {
        // Use a tiny capacity so we don't have to send 64 messages.
        let (tx, _rx) = tokio::sync::mpsc::channel::<OutboundMessage>(2);
        let sender = OutboundSender { tx };

        // Fill both slots.
        sender.send_notification("slot/one", json!({}))?;
        sender.send_notification("slot/two", json!({}))?;

        // Channel is now at capacity — the next send must NOT succeed.
        let result = sender.send_notification("slot/overflow", json!({}));
        assert!(
            matches!(&result, Err(e) if e.kind() == std::io::ErrorKind::WouldBlock),
            "expected WouldBlock when outbound channel is full, got {result:?}"
        );

        // _rx is still live, so the channel is full (not closed).
        // Explicitly verify it is NOT BrokenPipe.
        assert!(
            !matches!(&result, Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe),
            "full channel must not masquerade as BrokenPipe"
        );

        Ok(())
    }

    /// Verify that `OUTBOUND_CAPACITY` is a finite, non-zero constant that matches
    /// the inbound queue capacity convention.
    #[test]
    fn outbound_capacity_constant_is_sane() {
        // Must be non-zero (channel(0) would panic in tokio). OUTBOUND_CAPACITY is a
        // const, so this is a genuine compile-time check (clippy::assertions_on_constants).
        const { assert!(OUTBOUND_CAPACITY > 0, "OUTBOUND_CAPACITY must be positive") };
        // Must be finite / reasonable (not usize::MAX, not absurdly large).
        const {
            assert!(
                OUTBOUND_CAPACITY <= 1024,
                "OUTBOUND_CAPACITY is suspiciously large — check the value"
            )
        };
        // Must match the inbound QUEUE_CAPACITY convention (64).
        assert_eq!(
            OUTBOUND_CAPACITY, 64,
            "OUTBOUND_CAPACITY should match the inbound scheduler QUEUE_CAPACITY (64)"
        );
    }

    /// Verify that a handler using &dyn OutboundSink can send a notification
    /// and a RecordingSink captures it (#5015 PR-2).
    ///
    /// This test demonstrates the migration pattern: a handler function
    /// accepts `&dyn OutboundSink` instead of `&LspServer`, enabling
    /// unit testing without constructing the full server.
    #[test]
    fn outbound_sink_trait_works_with_recording_sink() {
        fn send_diagnostics_notification(sink: &dyn OutboundSink, uri: &str) -> io::Result<()> {
            sink.send_notification(
                "textDocument/publishDiagnostics",
                json!({ "uri": uri, "diagnostics": [] }),
            )
        }

        let sink = RecordingSink::new();
        send_diagnostics_notification(&sink, "file:///test.pl").unwrap();

        let messages = sink.drain();
        assert_eq!(messages.len(), 1, "exactly one message should be recorded");
        match &messages[0] {
            OutboundMessage::Notification { method, params } => {
                assert_eq!(method, "textDocument/publishDiagnostics");
                assert_eq!(params["uri"], "file:///test.pl");
            }
            _ => panic!("expected Notification, got something else"),
        }
    }

    /// Verify that OutboundSender also satisfies the OutboundSink trait,
    /// so production code and test code share the same interface (#5015 PR-2).
    #[test]
    fn outbound_sender_satisfies_sink_trait() {
        fn accept_sink(sink: &dyn OutboundSink) -> io::Result<()> {
            sink.send_notification("test/method", json!({}))
        }

        let (sender, _handle) = spawn_writer(Box::new(std::io::sink()));
        // This compiles only if OutboundSender implements OutboundSink.
        accept_sink(&sender).unwrap();
    }

    /// #8896 §3: the outbound seam refuses to emit frames whose method is
    /// positively registered client→server — such a frame can only be a
    /// reversed-direction bug. Unknown custom names stay tolerated at this
    /// transport layer; the inventory test forces new literals into the
    /// registry instead.
    #[test]
    fn outbound_admission_refuses_client_to_server_methods() -> Result<(), Box<dyn Error>> {
        let (tx, _rx) = tokio::sync::mpsc::channel::<OutboundMessage>(4);
        let sender = OutboundSender { tx };
        let request_id = ServerRequestId::new(1).ok_or("valid id")?;

        for method in ["initialize", "textDocument/hover", "textDocument/didOpen"] {
            let request_error =
                sender.send_request(request_id, method, json!({})).err().ok_or_else(|| {
                    format!("outbound request `{method}` must be refused by direction admission")
                })?;
            assert_eq!(
                request_error.kind(),
                io::ErrorKind::InvalidInput,
                "outbound request `{method}`"
            );
            assert!(
                request_error.to_string().contains("client-to-server"),
                "outbound `{method}` rejection names the direction boundary: {request_error}"
            );

            let notification_error =
                sender.send_notification(method, json!({})).err().ok_or_else(|| {
                    format!(
                        "outbound notification `{method}` must be refused by direction admission"
                    )
                })?;
            assert_eq!(notification_error.kind(), io::ErrorKind::InvalidInput);
        }
        Ok(())
    }

    #[test]
    fn outbound_admission_allows_server_to_client_methods() -> Result<(), Box<dyn Error>> {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<OutboundMessage>(4);
        let sender = OutboundSender { tx };
        let request_id = ServerRequestId::new(7).ok_or("valid id")?;

        sender.send_request(request_id, "workspace/configuration", json!({"items": []}))?;
        sender.send_notification("window/showMessage", json!({"type": 3, "message": "x"}))?;
        sender.send_notification("slot/unregistered", json!({}))?;
        drop(sender);

        let mut frames = Vec::new();
        while let Ok(message) = rx.try_recv() {
            match message {
                OutboundMessage::Request { method, .. } => frames.push(format!("request:{method}")),
                OutboundMessage::Notification { method, .. } => {
                    frames.push(format!("notification:{method}"))
                }
                OutboundMessage::Response(_) => frames.push("response".to_string()),
            }
        }

        assert!(
            frames.contains(&"request:workspace/configuration".to_string()),
            "server→client request must pass admission: {frames:?}"
        );
        assert!(
            frames.contains(&"notification:window/showMessage".to_string()),
            "server→client notification must pass admission: {frames:?}"
        );
        assert!(
            frames.contains(&"notification:slot/unregistered".to_string()),
            "unknown custom names stay tolerated at the transport seam: {frames:?}"
        );
        Ok(())
    }
}
