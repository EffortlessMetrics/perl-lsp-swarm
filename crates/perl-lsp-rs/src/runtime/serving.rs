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
            // Read LSP message using transport module. Typed decode failures are
            // logged with payload-private metadata and skipped; exact respond/
            // close/exit disposition remains #6720.
            match message_reader.read_next_outcome(reader)? {
                Some(Ok(request)) => {
                    tracing::trace!(method = %request.method, "Received request");

                    // Handle the request
                    if let Some(response) = self.handle_request(request) {
                        // Log and send response via outbound channel
                        log_response(&response);
                        self.outbound_sink().send_response(response)?;
                    }
                }
                Some(Err(error)) => {
                    tracing::warn!(
                        stage = error.stage().as_str(),
                        payload_bytes = error.payload_bytes(),
                        "incoming message rejected"
                    );
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
        let mut response_delivery_failed = false;

        loop {
            let request = tokio::select! {
                _ = self.outbound.response_failure_notified() => {
                    tracing::warn!("outbound response delivery failed; closing LSP ingress");
                    response_delivery_failed = true;
                    break;
                }
                request = rx.recv() => request,
            };
            let Some(request) = request else { break };
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
                    let send_result = tokio::select! {
                        _ = self.outbound.response_failure_notified() => None,
                        result = sched.send_mutation(request) => Some(result),
                    };
                    if send_result.is_none_or(|result| result.is_err()) {
                        response_delivery_failed = true;
                        break;
                    }
                }
                RequestClass::ReadOnly => {
                    let send_result = tokio::select! {
                        _ = self.outbound.response_failure_notified() => None,
                        result = sched.send_read(request) => Some(result),
                    };
                    if send_result.is_none_or(|result| result.is_err()) {
                        response_delivery_failed = true;
                        break;
                    }
                }
            }
        }

        // Cooperative shutdown: drop senders, drain remaining work.
        // spawn_blocking tasks run to completion and cannot be aborted.
        if response_delivery_failed {
            // Close response admission before workers are asked to drain. This
            // prevents a worker from enqueueing another required response while
            // the writer is already known to be unable to deliver it.
            self.outbound.close_admission();
        }
        sched.shutdown().await;
    }

    /// Wait until a required response can no longer be delivered. Socket
    /// frontends use this to close the peer while scheduler cleanup proceeds.
    pub(crate) async fn response_delivery_failure_notified(&self) {
        self.outbound.response_failure_notified().await;
    }

    pub(crate) fn response_delivery_failed(&self) -> bool {
        self.outbound.response_delivery_failed()
    }

    /// Handle a message from any reader (for testing)
    pub fn handle_message<R: Read>(&self, reader: &mut R) -> io::Result<()> {
        let mut buf_reader = BufReader::new(reader);
        let mut message_reader = ContentLengthMessageReader::new();
        if let Some(request) = message_reader.read_next(&mut buf_reader)?
            && let Some(response) = self.handle_request(request)
        {
            // Send response via outbound channel
            self.outbound.send_response(response)?;
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
    /// Cancellation requests for unknown or already-settled IDs are ignored by
    /// the scheduler-aware path. The legacy helper remains for internal
    /// supersession and test paths; its cap prevents unbounded growth.
    pub(crate) fn cancel_mark(&self, id: &JsonRpcId) {
        let pending = self.pending_request_ids.lock();
        let mut c = self.cancelled.lock();
        if c.len() >= CANCELLED_SET_CAP {
            c.retain(|candidate| pending.contains(candidate));
        }
        c.insert(id.clone());
    }

    /// Keep a scheduler-owned request ID protected from stale-marker trimming.
    pub(crate) fn mark_request_pending(&self, id: &JsonRpcId) {
        self.pending_request_ids.lock().insert(id.clone());
    }

    /// Mark cancellation only while the scheduler still owns this request.
    /// Holding both locks in this order closes the settlement race.
    pub(crate) fn mark_cancelled_if_pending(&self, id: &JsonRpcId) {
        let pending = self.pending_request_ids.lock();
        if !pending.contains(id) {
            return;
        }
        let mut cancelled = self.cancelled.lock();
        if cancelled.len() >= CANCELLED_SET_CAP {
            cancelled.retain(|candidate| pending.contains(candidate));
        }
        cancelled.insert(id.clone());
    }

    /// Release a scheduler-owned request ID after it is fully settled.
    pub(crate) fn clear_request_pending(&self, id: &JsonRpcId) {
        let mut pending = self.pending_request_ids.lock();
        let mut cancelled = self.cancelled.lock();
        pending.remove(id);
        cancelled.remove(id);
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

#[cfg(test)]
mod tests {
    use super::LspServer;
    use crate::protocol::{JsonRpcId, JsonRpcRequest};
    use parking_lot::Mutex;
    use std::io::{self, Write};
    use std::sync::Arc;

    fn request(id: i64, method: &str) -> JsonRpcRequest {
        JsonRpcRequest {
            _jsonrpc: "2.0".to_string(),
            id: Some(JsonRpcId::Integer(id)),
            method: method.to_string(),
            params: None,
        }
    }

    #[test]
    fn pending_cancel_is_observed_but_unknown_cancel_is_ignored()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        let pending = JsonRpcId::Integer(71_001);
        let unknown = JsonRpcId::Integer(71_002);

        server.mark_request_pending(&pending);
        server.mark_cancelled_if_pending(&pending);
        if !server.is_cancelled(&pending) {
            return Err("pending request cancellation was not recorded".into());
        }

        server.mark_cancelled_if_pending(&unknown);
        if server.is_cancelled(&unknown) {
            return Err("unknown request cancellation was recorded".into());
        }

        server.clear_request_pending(&pending);
        if server.is_cancelled(&pending) {
            return Err("settled request cancellation was not cleared".into());
        }
        Ok(())
    }

    struct FailingOutput {
        writes: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl Write for FailingOutput {
        fn write(&mut self, _bytes: &[u8]) -> io::Result<usize> {
            self.writes.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "controlled response failure"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "controlled response flush failure"))
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn serve_async_stops_live_ingress_after_required_response_failure()
    -> Result<(), Box<dyn std::error::Error>> {
        let writes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let output = Arc::new(Mutex::new(
            Box::new(FailingOutput { writes: Arc::clone(&writes) }) as Box<dyn Write + Send>
        ));
        let server = Arc::new(LspServer::with_output(output));
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        tx.send(request(14168, "unknown/method")).await?;

        // Keep the ingress sender alive: completion must be driven by the
        // required response's transport failure, rather than input EOF.
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            Arc::clone(&server).serve_async(rx),
        )
        .await
        .map_err(|_| "serve_async remained blocked with live input")?;
        if !server.response_delivery_failed() {
            return Err("serve_async returned without recording response failure".into());
        }
        if writes.load(std::sync::atomic::Ordering::SeqCst) == 0 {
            return Err("required response never reached the failing writer".into());
        }
        if server.pending_request_ids.lock().contains(&JsonRpcId::Integer(14168)) {
            return Err("failed live request remained pending after serve_async returned".into());
        }
        drop(tx);
        drop(server);
        Ok(())
    }
}
