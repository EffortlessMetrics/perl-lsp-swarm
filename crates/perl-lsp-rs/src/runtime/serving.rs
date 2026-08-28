//! Message loop, cancellation state, and progress tracking.
//!
//! `run`, `serve`, `serve_async`, and `handle_message` drive the LSP
//! ingress loop. Cancellation helpers (`cancel_mark`, `cancel_clear`,
//! `is_cancelled`) track request lifecycle for `$/cancelRequest`.
//! `register_progress_request` maps progress tokens to request IDs.

use super::{
    Arc, BufRead, BufReader, ContentLengthMessageReader, JsonRpcRequest, LspServer, Ordering, Read,
    io, log_response, scheduler,
};
use crate::protocol::JsonRpcId;
use crate::transport::IncomingMessageError;

const CANCELLED_SET_CAP: usize = 256;

#[allow(dead_code)]
impl LspServer {
    /// Run the LSP server using stdio
    pub fn run(&self) -> io::Result<()> {
        tracing::info!("LSP server started (stdio)");
        let reader_arc = Arc::clone(&self.reader);
        let mut reader = reader_arc.lock();
        self.serve(&mut **reader)
    }

    /// Serve LSP requests from the given reader
    pub fn serve(&self, reader: &mut dyn BufRead) -> io::Result<()> {
        let mut message_reader = ContentLengthMessageReader::new();

        loop {
            // Read LSP message using transport module
            match message_reader.read_next_outcome(reader)? {
                Some(Ok(request)) => {
                    tracing::trace!(method = %request.method, "Received request");

                    if let Some(response) = self.handle_request(request) {
                        log_response(&response);
                        self.outbound_sink().send_response(response)?;
                    }
                }
                Some(Err(error)) => {
                    let terminal = error.is_terminal_at_eof();
                    log_incoming_error(&error, "runtime serving ingress");
                    if terminal {
                        tracing::info!("LSP server: truncated input, shutting down");
                        break;
                    }
                }
                None => {
                    // EOF reached, exit cleanly
                    tracing::info!("LSP server: EOF, shutting down");
                    break;
                }
            }
        }

        Ok(())
    }

    /// Serve LSP requests with worker-queue dispatch.
    ///
    /// The ingress loop reads messages from `rx`, classifies them via
    /// `scheduler::classify`, and routes them to dedicated worker queues.
    /// No heavy work runs inline — only classification and channel sends.
    ///
    /// Architecture:
    /// - **Control** (`$/cancelRequest`): processed inline (only touches atomics)
    /// - **Mutation/Lifecycle**: routed to single exclusive worker (sequential)
    /// - **ReadOnly**: routed to bounded read pool (N concurrent workers)
    /// - **Egress**: existing `OutboundSender` (already decoupled)
    ///
    /// ## Shutdown policy
    ///
    /// When the ingress channel closes (EOF), the scheduler's sender halves are
    /// dropped. Workers drain remaining items and exit. `spawn_blocking` tasks
    /// cannot be aborted — they run to completion.
    pub async fn serve_async(self: Arc<Self>, mut rx: tokio::sync::mpsc::Receiver<JsonRpcRequest>) {
        use scheduler::{RequestClass, classify};

        let sched = scheduler::Scheduler::new(Arc::clone(&self));

        while let Some(request) = rx.recv().await {
            let method = request.method.clone();
            tracing::trace!(method = %method, "Received request");

            match classify(&method) {
                RequestClass::Control => {
                    // Process inline — no queue, no spawn.
                    // Control methods ($/cancelRequest) only touch atomics
                    // and must complete before the next message is read.
                    let _ = self.handle_request(request);
                }
                RequestClass::Lifecycle | RequestClass::Mutation => {
                    if sched.send_mutation(request).await.is_err() {
                        break;
                    }
                }
                RequestClass::ReadOnly => {
                    if sched.send_read(request).await.is_err() {
                        break;
                    }
                }
            }
        }

        // Cooperative shutdown: drop senders, drain remaining work.
        // spawn_blocking tasks run to completion and cannot be aborted.
        sched.shutdown().await;
    }

    /// Handle a message from any reader (for testing).
    ///
    /// The local [`BufReader`] is retained for the whole recovery loop, so a
    /// malformed frame cannot discard a following valid frame that was read
    /// into the buffer. Responses are queued on the server's outbound writer;
    /// dropping the server provides the flush/join boundary for callers that
    /// need to observe the emitted bytes synchronously.
    pub fn handle_message<R: Read>(&self, reader: &mut R) -> io::Result<()> {
        let mut buf_reader = BufReader::new(reader);
        let mut message_reader = ContentLengthMessageReader::new();
        loop {
            match message_reader.read_next_outcome(&mut buf_reader)? {
                Some(Ok(request)) => {
                    if let Some(response) = self.handle_request(request) {
                        self.outbound.send_response(response)?;
                    }
                    break;
                }
                Some(Err(error)) => {
                    log_incoming_error(&error, "runtime message helper");
                    if error.is_terminal_at_eof() {
                        break;
                    }
                }
                None => break,
            }
        }
        Ok(())
    }

    /// Check if the server is initialized
    pub fn is_initialized(&self) -> bool {
        self.initialized.load(Ordering::Acquire)
    }

    /// Mark a request as cancelled.
    ///
    /// The cancelled set is advisory — entries are checked by [`is_cancelled`]
    /// and removed by [`cancel_clear`] when the routing path processes them.
    /// However, cancels for already-completed or never-dispatched requests
    /// insert entries that are never removed. To prevent unbounded growth,
    /// stale markers are removed when the set reaches
    /// [`CANCELLED_SET_CAP`] (#5032 item 2). Markers for requests that are
    /// still queued or executing are retained by the scheduler-aware pending
    /// set, so trimming cannot erase a live queued cancellation.
    pub(crate) fn cancel_mark(&self, id: &JsonRpcId) {
        let mut c = self.cancelled.lock();
        if c.len() >= CANCELLED_SET_CAP {
            let pending = self.pending_request_ids.lock();
            c.retain(|candidate| pending.contains(candidate));
        }
        c.insert(id.clone());
    }

    /// Keep a scheduler-owned request ID protected from stale-marker trimming.
    pub(crate) fn mark_request_pending(&self, id: &JsonRpcId) {
        self.pending_request_ids.lock().insert(id.clone());
    }

    /// Release a scheduler-owned request ID after it is fully settled.
    pub(crate) fn clear_request_pending(&self, id: &JsonRpcId) {
        self.pending_request_ids.lock().remove(id);
    }

    /// Clear a cancelled request
    pub(crate) fn cancel_clear(&self, id: &JsonRpcId) {
        let mut c = self.cancelled.lock();
        c.remove(id);
    }

    /// Check if a request has been cancelled
    pub(crate) fn is_cancelled(&self, id: &JsonRpcId) -> bool {
        let set = self.cancelled.lock();
        set.contains(id)
    }

    /// Register a mapping from a progress token to its originating request ID
    ///
    /// When the client sends `window/workDoneProgress/cancel` for this token,
    /// the server will look up the request ID and signal cancellation via the
    /// global cancellation registry.
    pub(crate) fn register_progress_request(&self, token: &str, request_id: JsonRpcId) {
        self.progress_token_to_request.lock().insert(token.to_string(), request_id);
    }
}

fn log_incoming_error(error: &IncomingMessageError, ingress: &'static str) {
    tracing::warn!(
        ingress,
        error_kind = error.kind(),
        %error,
        "incoming JSON-RPC message rejected"
    );
}

#[cfg(test)]
mod strict_ingress_tests {
    use super::*;
    use parking_lot::Mutex;
    use std::error::Error;
    use std::io::{self, Cursor, Write};

    struct Capture(Arc<Mutex<Vec<u8>>>);

    impl Write for Capture {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.lock().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn frame(body: &[u8]) -> Vec<u8> {
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        let mut framed = header.into_bytes();
        framed.extend_from_slice(body);
        framed
    }

    #[test]
    fn serving_ingress_rejects_malformed_frames_and_reaches_valid_request()
    -> Result<(), Box<dyn Error>> {
        let mut invalid_utf8 =
            br#"{"jsonrpc":"2.0","id":1,"method":"shutdown","params":{"ignored":"safe"}}"#.to_vec();
        let string_end = invalid_utf8.iter().rposition(|byte| *byte == b'"').ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "missing string terminator")
        })?;
        invalid_utf8.insert(string_end, 0xff);
        let malformed_json = br#"{"jsonrpc":"2.0","method":"invalid""#;
        let invalid_version = br#"{"jsonrpc":"1.0","id":1,"method":"invalid","params":{}}"#;
        let batch = br#"[{"jsonrpc":"2.0","id":1,"method":"invalid","params":{}}]"#;
        let valid_initialize = br#"{"jsonrpc":"2.0","id":2,"method":"initialize","params":{"processId":null,"capabilities":{},"rootUri":null}}"#;

        let mut stream = frame(&invalid_utf8);
        stream.extend(frame(malformed_json));
        stream.extend(frame(invalid_version));
        stream.extend(frame(batch));
        stream.extend(frame(valid_initialize));

        let captured = Arc::new(Mutex::new(Vec::new()));
        let writer: Arc<Mutex<Box<dyn Write + Send>>> =
            Arc::new(Mutex::new(Box::new(Capture(Arc::clone(&captured)))));
        let server = LspServer::with_output(writer);
        let mut input = io::BufReader::new(Cursor::new(stream));
        server.serve(&mut input)?;

        if server.shutdown_received.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid UTF-8 shutdown request was dispatched",
            )
            .into());
        }
        if !server.initialize_requested.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "valid initialize request was not reached after malformed frames",
            )
            .into());
        }
        drop(server);
        if captured.lock().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "serving ingress produced no initialize response",
            )
            .into());
        }
        Ok(())
    }

    #[test]
    fn message_helper_recovers_after_malformed_frame_in_buffered_input()
    -> Result<(), Box<dyn Error>> {
        let mut invalid_utf8 =
            br#"{"jsonrpc":"2.0","id":1,"method":"invalid","params":{"text":"safe"}}"#.to_vec();
        let string_end = invalid_utf8.iter().rposition(|byte| *byte == b'"').ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "missing string terminator")
        })?;
        invalid_utf8.insert(string_end, 0xff);
        let valid_initialize =
            br#"{"jsonrpc":"2.0","id":2,"method":"initialize","params":{"processId":null,"capabilities":{},"rootUri":null}}"#;

        let mut stream = frame(&invalid_utf8);
        stream.extend(frame(valid_initialize));
        let captured = Arc::new(Mutex::new(Vec::new()));
        let writer: Arc<Mutex<Box<dyn Write + Send>>> =
            Arc::new(Mutex::new(Box::new(Capture(Arc::clone(&captured)))));
        let server = LspServer::with_output(writer);
        let mut input = Cursor::new(stream);
        server.handle_message(&mut input)?;

        if !server.initialize_requested.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "message helper lost valid input after malformed frame",
            )
            .into());
        }
        drop(server);
        if captured.lock().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "message helper produced no response for recovered initialize",
            )
            .into());
        }
        Ok(())
    }
}
