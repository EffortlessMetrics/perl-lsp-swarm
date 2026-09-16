//! Transport layer: run (stdin/stdout) and run_with_io.

use super::sync_utils::EventSender;
#[cfg(test)]
use super::sync_utils::dispatch_event;
use super::{
    Arc, AtomicBool, ContentLengthFramer, DapMessage, DebugAdapter, EVENT_QUEUE_CAPACITY, Mutex,
    Read, Write, io, lock_or_recover, sync_channel, thread,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{TryRecvError, TrySendError};
use std::time::Duration;

const INTAKE_POLL_INTERVAL: Duration = Duration::from_millis(100);

pub(super) trait BoundedRead {
    fn read_with_timeout(
        &mut self,
        target: &mut [u8],
        timeout: Duration,
    ) -> io::Result<Option<usize>>;
}

impl<T> BoundedRead for std::io::Cursor<T>
where
    std::io::Cursor<T>: Read,
{
    fn read_with_timeout(
        &mut self,
        target: &mut [u8],
        _timeout: Duration,
    ) -> io::Result<Option<usize>> {
        self.read(target).map(Some)
    }
}

struct StdioReader<R> {
    input: R,
}

impl<R> StdioReader<R> {
    fn new(input: R) -> Self {
        Self { input }
    }
}

#[cfg(unix)]
impl<R> BoundedRead for StdioReader<R>
where
    R: Read + std::os::fd::AsFd,
{
    fn read_with_timeout(
        &mut self,
        target: &mut [u8],
        timeout: Duration,
    ) -> io::Result<Option<usize>> {
        use nix::poll::{PollFd, PollFlags, PollTimeout, poll};

        let poll_timeout = PollTimeout::try_from(timeout)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        let mut descriptor = [PollFd::new(self.input.as_fd(), PollFlags::POLLIN)];
        let ready = poll(&mut descriptor, poll_timeout).map_err(io::Error::other)?;
        if ready == 0 {
            return Ok(None);
        }
        self.input.read(target).map(Some)
    }
}

#[cfg(windows)]
impl<R> BoundedRead for StdioReader<R>
where
    R: Read + std::os::windows::io::AsRawHandle,
{
    fn read_with_timeout(
        &mut self,
        target: &mut [u8],
        timeout: Duration,
    ) -> io::Result<Option<usize>> {
        use std::ptr::null_mut;
        use std::thread;
        use winapi::shared::minwindef::DWORD;
        use winapi::um::fileapi::GetFileType;
        use winapi::um::handleapi::INVALID_HANDLE_VALUE;
        use winapi::um::namedpipeapi::PeekNamedPipe;
        use winapi::um::winbase::{FILE_TYPE_CHAR, FILE_TYPE_DISK, FILE_TYPE_PIPE};

        let handle = self.input.as_raw_handle() as winapi::shared::ntdef::HANDLE;
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "stdin handle is invalid"));
        }
        // SAFETY: `handle` is borrowed from `self.input` and remains valid for
        // this call; no ownership is transferred to the Win32 API.
        let file_type = unsafe { GetFileType(handle) };
        if file_type == FILE_TYPE_DISK {
            return self.input.read(target).map(Some);
        }
        if file_type == FILE_TYPE_CHAR {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "interactive console input is unsupported for DAP stdio",
            ));
        }
        if file_type != FILE_TYPE_PIPE {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "DAP stdio input is not a pipe or regular file",
            ));
        }
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let mut available: DWORD = 0;
            // SAFETY: `handle` remains borrowed from `self.input`; all output
            // pointers are either null or point to this stack-local counter.
            let ok = unsafe {
                PeekNamedPipe(handle, null_mut(), 0, null_mut(), &mut available, null_mut())
            };
            if ok != 0 {
                if available != 0 {
                    return self.input.read(target).map(Some);
                }
            } else {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(109) {
                    return Ok(Some(0));
                }
                return Err(error);
            }
            if std::time::Instant::now() >= deadline {
                return Ok(None);
            }
            thread::sleep(Duration::from_millis(2));
        }
    }
}

const EVENT_WRITE_BATCH_MAX: usize = 64;

/// Upper bound on how long a response waits for handler-emitted events to
/// drain before it is written anyway. Generous enough for the consumer to
/// drain a full queue to a healthy wire (including slow CI schedulers);
/// small enough that a genuinely stalled consumer cannot stall the
/// session. Commands that emit no events never wait at all.
const EVENT_DRAIN_MAX_WAIT: std::time::Duration = std::time::Duration::from_secs(1);

fn next_transport_seq(seq: &Mutex<i64>) -> i64 {
    let mut value = lock_or_recover(seq, "transport.seq");
    *value += 1;
    *value
}

fn assign_wire_seq_locked(message: &mut DapMessage, next: &mut i64) {
    *next += 1;
    match message {
        DapMessage::Request { seq: value, .. }
        | DapMessage::Response { seq: value, .. }
        | DapMessage::Event { seq: value, .. } => *value = *next,
    }
}

fn write_message_with_wire_seq<W: Write>(
    shared_writer: &Mutex<W>,
    mut message: DapMessage,
    wire_seq: &Mutex<i64>,
) -> io::Result<()> {
    let mut writer = lock_or_recover(shared_writer, "response_writer");
    let mut next = lock_or_recover(wire_seq, "transport.wire_seq");
    assign_wire_seq_locked(&mut message, &mut next);
    let payload = serde_json::to_vec(&message).map_err(io::Error::other)?;
    write_framed_payload(&mut *writer, &payload)?;
    writer.flush()
}

fn write_framed_payload<W: Write>(writer: &mut W, payload: &[u8]) -> io::Result<()> {
    tracing::debug!(payload_len = payload.len(), "Writing DAP frame");
    writer.write_all(b"Content-Length: ")?;
    writer.write_all(payload.len().to_string().as_bytes())?;
    writer.write_all(b"\r\n\r\n")?;
    writer.write_all(payload)
}

fn write_event_payloads<W: Write>(
    writer: &mut W,
    payloads: &[Vec<u8>],
    transport_broken: &AtomicBool,
    flushed: &mut bool,
) -> bool {
    *flushed = false;
    let mut write_failed = false;
    let mut transport_marked_broken = false;
    for payload in payloads {
        if let Err(e) = write_framed_payload(writer, payload) {
            tracing::error!(error = %e, "Failed to write DAP frame in event handler");
            write_failed = true;
            transport_broken.store(true, Ordering::Release);
            transport_marked_broken = true;
            break;
        }
    }
    if !write_failed {
        if let Err(e) = writer.flush() {
            tracing::error!(error = %e, "Failed to flush DAP frame in event handler");
            transport_broken.store(true, Ordering::Release);
            transport_marked_broken = true;
        } else {
            *flushed = true;
        }
    }
    transport_marked_broken
}

impl DebugAdapter {
    /// Run the debug adapter server
    pub(crate) fn run(&mut self) -> io::Result<()> {
        self.run_with_io(StdioReader::new(io::stdin()), io::stdout())
    }

    /// Shared DAP transport loop used by stdio and generic reader/writer paths.
    pub(super) fn run_with_io<R, W>(&mut self, input: R, output: W) -> io::Result<()>
    where
        R: BoundedRead,
        W: Write + Send + 'static,
    {
        self.native_stdio_transport = true;
        let result = self.run_with_io_inner(input, output);
        self.native_stdio_transport = false;
        result
    }

    fn run_with_io_inner<R, W>(&mut self, input: R, output: W) -> io::Result<()>
    where
        R: BoundedRead,
        W: Write + Send + 'static,
    {
        // The shipped native server reaches this transport only through stdio.
        // Set the profile before initialize is dispatched so capability
        // advertisement and request-floor admission use the same authority.
        // Create a shared writer to prevent interleaving between the main loop
        // and the event handler thread.
        let shared_writer: Arc<Mutex<W>> = Arc::new(Mutex::new(output));
        let event_writer = Arc::clone(&shared_writer);

        // Create bounded channel for asynchronous events.
        let (tx, rx) = sync_channel::<DapMessage>(EVENT_QUEUE_CAPACITY);
        let event_sender = EventSender::new(tx);
        self.event_sender = Some(event_sender.clone());
        let (writer_done_tx, writer_done_rx) = sync_channel::<bool>(1);

        // A new transport run starts with a clean drain latch: any residue
        // from a previous run whose consumer died mid-batch must not make
        // this run's responses wait for a drain that can never complete.
        self.event_drain.reset();
        let event_drain = self.event_drain.clone();

        // Clone transport_broken flag to pass to the event handler thread.
        let transport_broken = Arc::clone(&self.transport_broken);
        let event_transport_broken = Arc::clone(&transport_broken);
        let wire_seq = Arc::new(Mutex::new(0i64));
        let event_wire_seq = Arc::clone(&wire_seq);
        // This handshake is local to this transport run.  It cannot be
        // satisfied by a terminal event from a previous session.
        thread::spawn(move || {
            let mut event_delivery_failed = false;

            while let Ok(first_msg) = rx.recv() {
                // Check if transport is already marked broken
                if event_transport_broken.load(Ordering::Acquire) {
                    break;
                }

                let mut batch = Vec::with_capacity(EVENT_WRITE_BATCH_MAX);
                batch.push(first_msg);

                let mut disconnected = false;
                while batch.len() < EVENT_WRITE_BATCH_MAX {
                    match rx.try_recv() {
                        Ok(msg) => batch.push(msg),
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => {
                            disconnected = true;
                            break;
                        }
                    }
                }
                // Count the batch after the receive loop: every message
                // removed from the channel releases one latch reservation
                // (phantom-batch fix — a count taken after only `first_msg`
                // under-completed multi-event batches and pushed every later
                // response through the full drain timeout).
                let drained = batch.len();

                let mut writer = lock_or_recover(&event_writer, "event_writer");
                let mut wire_seq = lock_or_recover(&event_wire_seq, "transport.wire_seq");
                let mut payloads = Vec::with_capacity(batch.len());
                for mut msg in batch {
                    assign_wire_seq_locked(&mut msg, &mut wire_seq);
                    match serde_json::to_vec(&msg) {
                        Ok(payload) => payloads.push(payload),
                        Err(e) => {
                            event_delivery_failed = true;
                            tracing::error!(
                                error = %e,
                                message = ?msg,
                                "Failed to serialize DAP message"
                            );
                        }
                    }
                }

                if payloads.is_empty() {
                    // Nothing observable was written, but the drained
                    // messages still hold latch reservations: release them so
                    // a batch of unserializable events cannot stall later
                    // responses on a drain that can never complete.
                    event_drain.complete(drained);
                    if disconnected {
                        break;
                    }
                    continue;
                }

                // Release the worker's drain barrier for this batch: every
                // message removed from the channel above releases one latch
                // reservation — written, unserializable (never observable, so
                // nothing to wait for), or failed-open on a broken transport.
                let mut event_flushed = false;
                if write_event_payloads(
                    &mut *writer,
                    &payloads,
                    &event_transport_broken,
                    &mut event_flushed,
                ) {
                    event_delivery_failed = true;
                    event_drain.complete(drained);
                    tracing::error!(
                        "Event handler detected a write failure; marking transport broken"
                    );
                    break;
                }
                event_delivery_failed |= !event_flushed;
                event_drain.complete(drained);
                drop(wire_seq);

                if disconnected {
                    break;
                }
            }
            tracing::debug!("Event handler thread terminating");
            let _ = writer_done_tx
                .send(!event_delivery_failed && !event_transport_broken.load(Ordering::Acquire));
        });

        struct EventSenderCloseGuard(EventSender);
        impl Drop for EventSenderCloseGuard {
            fn drop(&mut self) {
                self.0.close();
            }
        }
        let _close_guard = EventSenderCloseGuard(event_sender.clone());

        // Intake owns parsing and admission; one scoped worker owns ordinary
        // request execution. This keeps the transport responsive to the
        // public cancel floor while preserving FIFO execution without placing
        // a mutex around the whole adapter.
        struct QueuedRequest {
            request_seq: i64,
            command: String,
            arguments: Option<serde_json::Value>,
        }
        const REQUEST_QUEUE_CAPACITY: usize = 8;
        let queued_count = Arc::new(AtomicUsize::new(0));
        let worker_queued_count = Arc::clone(&queued_count);
        let (request_tx, request_rx) =
            std::sync::mpsc::sync_channel::<QueuedRequest>(REQUEST_QUEUE_CAPACITY + 1);
        let worker_writer = Arc::clone(&shared_writer);
        let worker_wire_seq = Arc::clone(&wire_seq);
        let worker_event_sender = event_sender.clone();
        let transport_seq = Arc::clone(&self.seq);
        let worker_seq = Arc::clone(&self.seq);
        let native_stdio_transport = self.native_stdio_transport;
        let worker_transport_broken = Arc::clone(&transport_broken);
        let worker_request_rx = request_rx;
        let operation_broker = Arc::clone(&self.operation_broker);
        let shutdown_requested = Arc::new(AtomicBool::new(false));
        let worker_shutdown_requested = Arc::clone(&shutdown_requested);
        let shutdown_reason = Arc::new(Mutex::new(None::<&'static str>));
        let worker_shutdown_reason = Arc::clone(&shutdown_reason);
        let (disconnect_done_tx, disconnect_done_rx) = sync_channel::<bool>(1);
        let mut reader = input;
        let mut framer = ContentLengthFramer::new();
        let mut read_buf = [0u8; 8 * 1024];

        thread::scope(|scope| {
            let worker_done = scope.spawn(move || -> io::Result<()> {
                while let Ok(request) = worker_request_rx.recv() {
                    if request.command != "disconnect" {
                        worker_queued_count.fetch_sub(1, Ordering::AcqRel);
                    }
                    let is_disconnect = request.command == "disconnect";
                    // This load is the worker's claim point. Disconnect refuses
                    // requests that have not started; a request already claimed
                    // may finish or settle through the broker. Clean EOF does
                    // not set this flag and preserves accepted FIFO execution.
                    let response = if worker_shutdown_requested.load(Ordering::Acquire)
                        && !is_disconnect
                    {
                        let reason =
                            *lock_or_recover(&worker_shutdown_reason, "transport.shutdown_reason")
                                .as_ref()
                                .unwrap_or(&"shutdown");
                        DapMessage::Response {
                            seq: self.next_seq(),
                            request_seq: request.request_seq,
                            success: false,
                            command: request.command.clone(),
                            body: None,
                            message: Some(format!(
                                "Request was accepted before {reason} and then cancelled"
                            )),
                        }
                    } else if request.command == crate::reload_family::LOADED_MODULE_RELOAD_REQUEST
                        && self.loaded_module_reload_route_enabled()
                    {
                        // R03 reload-family route (#10102), mirrored from
                        // `handle_request`: the worker's table-owned
                        // `dispatch_request` bypasses that seam (like the
                        // #9581 floor at intake), so the profiled family
                        // route is repeated here; otherwise the family stays
                        // unavailable on the wire without advertisement.
                        self.handle_loaded_module_reload(
                            self.next_seq(),
                            request.request_seq,
                            request.arguments,
                        )
                    } else {
                        self.dispatch_request(
                            request.request_seq,
                            &request.command,
                            request.arguments,
                        )
                    };
                    let notify_initialized = request.command == "initialize"
                        && DebugAdapter::response_succeeded_for_command(&response, "initialize");
                    let disconnect_succeeded = is_disconnect
                        && matches!(&response, DapMessage::Response { success: true, .. });
                    // Handler-emitted events must reach the client before the
                    // terminal response that can imply their effect: queueing
                    // alone does not order the wire because the event consumer
                    // is asynchronous. Wait (bounded, fail-open) for the drain;
                    // on timeout the response proceeds without the ordering
                    // guarantee rather than stalling the session.
                    if !self.event_drain.wait_until_drained(EVENT_DRAIN_MAX_WAIT) {
                        tracing::warn!(
                            wait_ms = EVENT_DRAIN_MAX_WAIT.as_millis() as u64,
                            "event drain barrier timed out; writing response without event ordering"
                        );
                    }
                    if let Err(error) = write_message_then_notify_initialized(
                        &worker_writer,
                        response,
                        notify_initialized,
                        Some(&worker_event_sender),
                        &worker_seq,
                        &worker_wire_seq,
                    ) {
                        if is_disconnect {
                            let _ = disconnect_done_tx.send(false);
                        }
                        worker_transport_broken.store(true, Ordering::Release);
                        return Err(error);
                    }
                    if disconnect_succeeded {
                        let _ = disconnect_done_tx.send(true);
                        worker_event_sender.close();
                        break;
                    }
                    if is_disconnect {
                        let _ = disconnect_done_tx.send(false);
                    }
                }
                Ok(())
            });

            let transport_result = 'transport: loop {
                // Check if transport has been marked broken by the event handler
                if transport_broken.load(Ordering::Acquire) {
                    tracing::error!(
                        "Transport is broken; a transport writer detected persistent write failure"
                    );
                    break 'transport Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "Transport writer detected persistent write failure; transport is broken",
                    ));
                }

                let bytes_read = match reader.read_with_timeout(&mut read_buf, INTAKE_POLL_INTERVAL)
                {
                    Ok(Some(bytes_read)) => bytes_read,
                    Ok(None) => continue,
                    Err(error) => {
                        *lock_or_recover(&shutdown_reason, "transport.shutdown_reason") =
                            Some("transport intake failure");
                        shutdown_requested.store(true, Ordering::Release);
                        operation_broker.settle_all("transport intake failed");
                        break 'transport Err(error);
                    }
                };
                if bytes_read == 0 {
                    break 'transport Ok(());
                }

                framer.push(&read_buf[..bytes_read]);

                loop {
                    let body = match framer.try_next() {
                        Ok(Some(body)) => body,
                        Ok(None) => break,
                        Err(error) => {
                            tracing::warn!(%error, "Failed to parse DAP transport frame");
                            continue;
                        }
                    };

                    let msg = match serde_json::from_slice::<DapMessage>(&body) {
                        Ok(msg) => msg,
                        Err(_) => {
                            tracing::warn!(body = %String::from_utf8_lossy(&body), "Failed to parse DAP message");
                            continue;
                        }
                    };

                    let (seq, command, arguments) = match msg {
                        DapMessage::Request { seq, command, arguments } => {
                            (seq, command, arguments)
                        }
                        DapMessage::Response {
                            seq,
                            request_seq,
                            command,
                            success,
                            message,
                            ..
                        } => {
                            // Log reception of response messages from client (for potential future
                            // server-initiated requests that expect responses). Currently the adapter
                            // does not initiate requests, so these are unexpected but valid per DAP spec.
                            tracing::debug!(
                                seq,
                                request_seq,
                                command,
                                success,
                                message = ?message,
                                "Received Response message from client (not yet handled)"
                            );
                            continue;
                        }
                        DapMessage::Event { seq, event, body } => {
                            // Log reception of event messages from client. The DAP protocol permits
                            // bidirectional event flow for advanced features. Currently these are
                            // unexpected, but we handle them gracefully by logging.
                            tracing::debug!(
                                seq,
                                event,
                                body = ?body,
                                "Received Event message from client (not yet handled)"
                            );
                            continue;
                        }
                    };

                    // Cancellation is the one control request that must be
                    // dispatched by intake while the FIFO worker may be
                    // blocked in an ordinary debugger operation.  Routing it
                    // through that worker would leave the broker token
                    // unreachable until the operation returned.
                    if native_stdio_transport && command == "cancel" {
                        if let Some(request) = arguments
                            .as_ref()
                            .and_then(|value| value.get("requestId"))
                            .and_then(serde_json::Value::as_i64)
                        {
                            operation_broker.cancel_request(request);
                        }
                        let response = DapMessage::Response {
                            seq: next_transport_seq(&transport_seq),
                            request_seq: seq,
                            success: true,
                            command,
                            body: None,
                            message: None,
                        };
                        if let Err(error) = write_message_then_notify_initialized(
                            &shared_writer,
                            response,
                            false,
                            Some(&event_sender),
                            &transport_seq,
                            &wire_seq,
                        ) {
                            break 'transport Err(error);
                        }
                        continue;
                    }

                    // #9581 secondary-capability floor, ahead of the table-owned
                    // dispatch: a floored wire request is refused before any
                    // handler can run. The `initialized` notification below still
                    // keys off the (never-floored) initialize response.
                    let response =
                        crate::backend::capabilities::capability_floor_message_for_native_stdio(
                            &command,
                            arguments.as_ref(),
                            native_stdio_transport,
                        )
                        .map(|message| DapMessage::Response {
                            seq: next_transport_seq(&transport_seq),
                            request_seq: seq,
                            success: false,
                            command: command.clone(),
                            body: None,
                            message: Some(message),
                        });
                    if let Some(response) = response {
                        if let Err(error) = write_message_then_notify_initialized(
                            &shared_writer,
                            response,
                            false,
                            Some(&event_sender),
                            &transport_seq,
                            &wire_seq,
                        ) {
                            break 'transport Err(error);
                        }
                    } else {
                        if command == "disconnect" {
                            // Cancellation must wake an active worker before the
                            // disconnect barrier is queued. The extra queue slot
                            // is reserved for this control request.
                            *lock_or_recover(&shutdown_reason, "transport.shutdown_reason") =
                                Some("disconnect");
                            shutdown_requested.store(true, Ordering::Release);
                            operation_broker.settle_all("disconnect");
                        } else if queued_count.load(Ordering::Acquire) >= REQUEST_QUEUE_CAPACITY {
                            let response = DapMessage::Response {
                                seq: next_transport_seq(&transport_seq),
                                request_seq: seq,
                                success: false,
                                command,
                                body: None,
                                message: Some(
                                    "Request queue is full; retry the request".to_string(),
                                ),
                            };
                            if let Err(error) = write_message_then_notify_initialized(
                                &shared_writer,
                                response,
                                false,
                                Some(&event_sender),
                                &transport_seq,
                                &wire_seq,
                            ) {
                                break 'transport Err(error);
                            }
                            continue;
                        }
                        let request =
                            QueuedRequest { request_seq: seq, command: command.clone(), arguments };
                        let is_disconnect = command == "disconnect";
                        if !is_disconnect {
                            queued_count.fetch_add(1, Ordering::AcqRel);
                        }
                        match request_tx.try_send(request) {
                            Ok(()) => {}
                            Err(TrySendError::Full(request)) => {
                                if !is_disconnect {
                                    queued_count.fetch_sub(1, Ordering::AcqRel);
                                }
                                let response = DapMessage::Response {
                                    seq: next_transport_seq(&transport_seq),
                                    request_seq: request.request_seq,
                                    success: false,
                                    command: request.command,
                                    body: None,
                                    message: Some(
                                        "Request queue is full; retry the request".to_string(),
                                    ),
                                };
                                if let Err(error) = write_message_then_notify_initialized(
                                    &shared_writer,
                                    response,
                                    false,
                                    Some(&event_sender),
                                    &transport_seq,
                                    &wire_seq,
                                ) {
                                    break 'transport Err(error);
                                }
                            }
                            Err(TrySendError::Disconnected(_)) => {
                                break 'transport Err(io::Error::new(
                                    io::ErrorKind::BrokenPipe,
                                    "DAP request worker stopped",
                                ));
                            }
                        }
                        if is_disconnect {
                            loop {
                                match disconnect_done_rx.recv_timeout(INTAKE_POLL_INTERVAL) {
                                    Ok(true) => break 'transport Ok(()),
                                    Ok(false) => {
                                        // Cleanup failed, so keep intake alive for a
                                        // client retry while the retained session remains
                                        // owned by the adapter.
                                        break;
                                    }
                                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                                        if transport_broken.load(Ordering::Acquire) {
                                            break 'transport Err(io::Error::new(
                                                io::ErrorKind::BrokenPipe,
                                                "DAP transport failed while disconnect was pending",
                                            ));
                                        }
                                    }
                                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                                        break 'transport Err(io::Error::new(
                                            io::ErrorKind::BrokenPipe,
                                            "DAP disconnect worker stopped",
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }
            };
            drop(request_tx);
            if transport_result.is_err() {
                // Every intake failure must take the same shutdown path as an
                // explicit disconnect. This wakes an in-flight request and
                // prevents the scoped worker from outliving the failed input.
                lock_or_recover(&shutdown_reason, "transport.shutdown_reason")
                    .get_or_insert("transport intake failure");
                shutdown_requested.store(true, Ordering::Release);
                operation_broker.settle_all("transport intake failed");
            }
            let worker_result = worker_done
                .join()
                .map_err(|_| io::Error::other("DAP request worker panicked"))
                .and_then(|result| result);
            event_sender.close();
            let events_drained =
                writer_done_rx.recv_timeout(std::time::Duration::from_secs(5)).map_err(
                    |error| io::Error::other(format!("event writer did not terminate: {error}")),
                )?;
            if !events_drained {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "event delivery failed"));
            }
            transport_result.and(worker_result)
        })
    }
}

/// Write a framed response payload, then — if `notify_initialized` is set — dispatch the
/// `initialized` event. DAP requires `initialized` only after a successful `initialize`
/// response is sent.
///
/// **Lock-ordering contract (issue #5149 / PR #5318 defect 1):** the response-writer
/// guard MUST be dropped before dispatching `initialized`. `initialized` is not an
/// `output` event, so `dispatch_event` takes the *blocking* `send` path, which can block
/// whenever the outbound queue is full. The event-consumer thread needs this very same
/// writer mutex to drain a batch and free a slot — holding both at once (writer guard +
/// blocking send) is a lock-ordering deadlock: the producer blocked on the channel send
/// while holding the writer mutex, and the consumer blocked on the writer mutex while
/// trying to drain the channel that would unblock the producer. The write helper
/// releases its guards before returning, ahead of `send_event` below.
fn write_message_then_notify_initialized<W: Write>(
    shared_writer: &Mutex<W>,
    message: DapMessage,
    notify_initialized: bool,
    event_sender: Option<&EventSender>,
    seq: &Mutex<i64>,
    wire_seq: &Mutex<i64>,
) -> io::Result<()> {
    write_message_with_wire_seq(shared_writer, message, wire_seq)?;
    if notify_initialized && let Some(sender) = event_sender {
        let _ = sender.send_event(seq, "initialized", None);
    }
    Ok(())
}

/// Transport supervision tests — placed inside this module to access the
/// `pub(super)` `run_with_io` without widening its visibility.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::debug_adapter::DebugState;
    use crate::debug_adapter::sync_utils;
    use std::io::Cursor;
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicUsize, Ordering as AOrdering};
    use std::sync::mpsc::Receiver;
    use std::time::{Duration, Instant};

    // ── Minimal Write impl that always fails ──────────────────────────────────

    struct FailingWriter {
        fail_after_writes: usize,
        write_count: Arc<AtomicUsize>,
    }

    impl FailingWriter {
        fn always_failing() -> Self {
            Self { fail_after_writes: 0, write_count: Arc::new(AtomicUsize::new(0)) }
        }

        fn fail_after(n: usize) -> Self {
            Self { fail_after_writes: n, write_count: Arc::new(AtomicUsize::new(0)) }
        }
    }

    impl Write for FailingWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let n = self.write_count.fetch_add(1, AOrdering::AcqRel);
            if n >= self.fail_after_writes {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "mock write failure"));
            }
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            let n = self.write_count.load(AOrdering::Acquire);
            if n >= self.fail_after_writes {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "mock flush failure"));
            }
            Ok(())
        }
    }

    #[derive(Default)]
    struct FlushFailingWriter {
        bytes: Vec<u8>,
    }

    struct ChunkedWriter {
        bytes: Vec<u8>,
        max_chunk: usize,
    }

    impl Write for ChunkedWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let chunk_len = buf.len().min(self.max_chunk);
            self.bytes.extend_from_slice(&buf[..chunk_len]);
            Ok(chunk_len)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Write for FlushFailingWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.bytes.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "mock flush failure"))
        }
    }

    #[derive(Clone, Default)]
    struct SharedWriter {
        bytes: Arc<Mutex<Vec<u8>>>,
        fail_writes: Arc<AtomicBool>,
    }

    impl Write for SharedWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if self.fail_writes.load(Ordering::Acquire) {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "mock write failure"));
            }
            let mut bytes =
                self.bytes.lock().map_err(|_| io::Error::other("writer buffer mutex poisoned"))?;
            bytes.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            if self.fail_writes.load(Ordering::Acquire) {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "mock flush failure"));
            }
            Ok(())
        }
    }

    // ── Frame builder ─────────────────────────────────────────────────────────

    fn framed_request(seq: i64, command: &str) -> Vec<u8> {
        let body = serde_json::to_vec(&serde_json::json!({
            "type": "request",
            "seq": seq,
            "command": command,
        }))
        .unwrap_or_default();
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        let mut frame = header.into_bytes();
        frame.extend_from_slice(&body);
        frame
    }

    fn framed_request_with_arguments(
        seq: i64,
        command: &str,
        arguments: serde_json::Value,
    ) -> Result<Vec<u8>, serde_json::Error> {
        let body = serde_json::to_vec(&serde_json::json!({
            "type": "request",
            "seq": seq,
            "command": command,
            "arguments": arguments,
        }))?;
        let mut frame = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
        frame.extend_from_slice(&body);
        Ok(frame)
    }

    struct ChannelReader {
        receiver: Receiver<Vec<u8>>,
        pending: Cursor<Vec<u8>>,
        failure_trigger: Option<Arc<AtomicBool>>,
        failed: bool,
    }

    impl Read for ChannelReader {
        fn read(&mut self, target: &mut [u8]) -> io::Result<usize> {
            loop {
                let amount = self.pending.read(target)?;
                if amount != 0 || target.is_empty() {
                    return Ok(amount);
                }
                match self.receiver.recv() {
                    Ok(input) => self.pending = Cursor::new(input),
                    Err(_) => return Ok(0),
                }
            }
        }
    }

    impl BoundedRead for ChannelReader {
        fn read_with_timeout(
            &mut self,
            target: &mut [u8],
            timeout: Duration,
        ) -> io::Result<Option<usize>> {
            loop {
                let amount = self.pending.read(target)?;
                if amount != 0 || target.is_empty() {
                    return Ok(Some(amount));
                }
                match self.receiver.recv_timeout(timeout) {
                    Ok(input) => self.pending = Cursor::new(input),
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        if !self.failed
                            && self
                                .failure_trigger
                                .as_ref()
                                .is_some_and(|trigger| trigger.load(Ordering::Acquire))
                        {
                            self.failed = true;
                            return Err(io::Error::new(
                                io::ErrorKind::BrokenPipe,
                                "injected transport intake failure",
                            ));
                        }
                        return Ok(None);
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Ok(Some(0)),
                }
            }
        }
    }

    #[test]
    fn channel_reader_timeout_is_distinct_from_eof() -> Result<(), Box<dyn std::error::Error>> {
        let (_sender, receiver) = std::sync::mpsc::channel();
        let mut reader = ChannelReader {
            receiver,
            pending: Cursor::new(Vec::new()),
            failure_trigger: None,
            failed: false,
        };
        let mut buffer = [0u8; 1];
        let result = reader.read_with_timeout(&mut buffer, Duration::from_millis(20))?;
        if result.is_some() {
            return Err(format!("idle channel must return timeout None, got {result:?}").into());
        }
        Ok(())
    }

    #[test]
    fn stdio_reader_observes_child_stdout_eof_while_stdin_remains_open()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut child = Command::new("perl")
            .arg("-e")
            .arg("close STDOUT; sleep 60")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let _child_stdin = child.stdin.take().ok_or("child stdin was not piped")?;
        let child_stdout = child.stdout.take().ok_or("child stdout was not piped")?;
        let mut reader = StdioReader::new(child_stdout);
        let mut buffer = [0u8; 1];
        let observed = reader.read_with_timeout(&mut buffer, Duration::from_secs(2));
        let _ = child.kill();
        child.wait()?;
        let observed = observed?;
        if observed != Some(0) {
            return Err(format!(
                "expected child stdout EOF while stdin remained open, got {observed:?}"
            )
            .into());
        }
        Ok(())
    }

    fn transport_messages(
        output: &SharedWriter,
    ) -> Result<Vec<DapMessage>, Box<dyn std::error::Error>> {
        let bytes = output.bytes.lock().map_err(|_| "writer poisoned")?.clone();
        let mut framer = ContentLengthFramer::new();
        framer.push(&bytes);
        let mut messages = Vec::new();
        while let Some(body) =
            framer.try_next().map_err(|error| format!("bad output frame: {error}"))?
        {
            messages.push(serde_json::from_slice(&body)?);
        }
        Ok(messages)
    }

    /// Regression for #14527: a supported, deliberately blocked inspection must
    /// not prevent the transport from observing the following public cancel
    /// refusal. The fake debugger records receipt of the framed evaluate query
    /// before waiting on a release file, so the test proves the handler is in
    /// its wait rather than merely relying on a sleeping child.
    #[test]
    fn blocked_evaluate_does_not_hide_following_cancel_frame()
    -> Result<(), Box<dyn std::error::Error>> {
        exercise_blocked_evaluate_transport(TransportScenario::Cancel)
    }

    #[test]
    fn blocked_evaluate_input_write_failure_still_settles_and_joins_worker()
    -> Result<(), Box<dyn std::error::Error>> {
        exercise_blocked_evaluate_transport(TransportScenario::CancelWriteFailure)
    }

    #[test]
    fn released_evaluate_transport_returns_value_and_accepts_next_request()
    -> Result<(), Box<dyn std::error::Error>> {
        exercise_blocked_evaluate_transport(TransportScenario::Released)
    }

    #[test]
    fn full_request_queue_does_not_hide_cancel_and_accepted_requests_recover()
    -> Result<(), Box<dyn std::error::Error>> {
        exercise_blocked_evaluate_transport(TransportScenario::FullQueueCancel)
    }

    #[test]
    fn full_request_queue_disconnect_settles_active_and_refuses_queued_work_without_another_read()
    -> Result<(), Box<dyn std::error::Error>> {
        exercise_blocked_evaluate_transport(TransportScenario::FullQueueDisconnect)
    }

    #[test]
    fn intake_failure_reports_truthful_reason_to_queued_request()
    -> Result<(), Box<dyn std::error::Error>> {
        exercise_blocked_evaluate_transport(TransportScenario::IntakeFailure)
    }

    #[derive(Clone, Copy)]
    enum TransportScenario {
        Released,
        Cancel,
        CancelWriteFailure,
        FullQueueCancel,
        FullQueueDisconnect,
        IntakeFailure,
    }

    fn exercise_blocked_evaluate_transport(
        scenario: TransportScenario,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let disconnect = matches!(scenario, TransportScenario::FullQueueDisconnect);
        let saturated = matches!(
            scenario,
            TransportScenario::FullQueueCancel | TransportScenario::FullQueueDisconnect
        );
        let require_early_cancel = !matches!(scenario, TransportScenario::Released);
        let cancel_write_failure = matches!(scenario, TransportScenario::CancelWriteFailure);
        let intake_failure = matches!(scenario, TransportScenario::IntakeFailure);
        let directory = tempfile::tempdir()?;
        let log = directory.path().join("peer.log");
        let seen = directory.path().join("evaluate-seen");
        let release = directory.path().join("release-evaluate");
        let (input_tx, input_rx) = std::sync::mpsc::channel();
        let intake_failure_trigger = Arc::new(AtomicBool::new(false));
        let reader_failure_trigger = Arc::clone(&intake_failure_trigger);
        let output = SharedWriter::default();
        let fail_writes = Arc::clone(&output.fail_writes);
        let output_view = output.clone();
        let mut adapter = DebugAdapter::new();
        adapter.seed_session_for_test()?;
        {
            let mut session = lock_or_recover(&adapter.session, "transport.test.session");
            let script = r#"
use strict;
use warnings;
use IO::Handle;
$| = 1;
my ($log, $seen, $release) = @ARGV;
open my $log_fh, '>>', $log or die $!;
$log_fh->autoflush(1);
print "READY\n";
while (my $line = <STDIN>) {
    print {$log_fh} $line;
    if ($line =~ /DAP_BEGIN_(\d+)/) {
        print "DAP_BEGIN_$1\n";
    } elsif ($line =~ /^x /) {
        open my $seen_fh, '>', $seen or die $!;
        print {$seen_fh} "evaluate\n";
        close $seen_fh;
        while (!-e $release) { select undef, undef, undef, 0.01; }
        print "42\n";
    } elsif ($line =~ /DAP_END_(\d+)/) {
        print "DAP_END_$1\n";
    }
}
"#;
            let replacement = Command::new("perl")
                .arg("-e")
                .arg(script)
                .arg(&log)
                .arg(&seen)
                .arg(&release)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()?;
            if let Some(session) = session.as_mut() {
                let mut old_process = std::mem::replace(&mut session.process, replacement);
                let old = old_process.id();
                let _ = old_process.kill();
                old_process.wait()?;
                tracing::debug!(old, "reaped short-lived test debugger");
            } else {
                return Err(
                    io::Error::other("test session disappeared while installing peer").into()
                );
            }
        }
        adapter.begin_session_generation();
        adapter.operation_broker.open_session();
        adapter.start_output_reader(std::path::PathBuf::from("."));
        let deadline = Instant::now() + Duration::from_secs(2);
        while !lock_or_recover(&adapter.recent_output, "test.output")
            .lines
            .iter()
            .any(|line| line.normalized == "READY")
        {
            if Instant::now() >= deadline {
                return Err("fake debugger reader did not acknowledge READY".into());
            }
            thread::sleep(Duration::from_millis(5));
        }
        let server = thread::spawn(move || {
            let result = adapter.run_with_io(
                ChannelReader {
                    receiver: input_rx,
                    pending: Cursor::new(Vec::new()),
                    failure_trigger: Some(reader_failure_trigger),
                    failed: false,
                },
                output,
            );
            drop(adapter);
            result
        });
        // Collect the oracle result separately so all failures still release the
        // debugger, close input, and reap the server before returning.
        let exercise = (|| -> Result<bool, Box<dyn std::error::Error>> {
            input_tx
                .send(framed_request_with_arguments(
                    1,
                    "evaluate",
                    serde_json::json!({"expression": "$x", "context": "repl"}),
                )?)
                .map_err(|error| format!("evaluate send failed: {error:?}"))?;
            let deadline = Instant::now() + Duration::from_secs(2);
            while !seen.exists() {
                if Instant::now() >= deadline {
                    return Err("fake debugger never observed evaluate".into());
                }
                thread::sleep(Duration::from_millis(5));
            }
            if intake_failure {
                input_tx.send(framed_request_with_arguments(
                    3,
                    "threads",
                    serde_json::json!({}),
                )?)?;
                // The evaluate request is still blocked in the peer. The next
                // bounded reader poll now reports the injected intake error.
                intake_failure_trigger.store(true, Ordering::Release);
                return Ok(false);
            }
            if saturated {
                for request in 10..=18 {
                    input_tx.send(framed_request_with_arguments(
                        request,
                        if disconnect { "initialize" } else { "threads" },
                        serde_json::json!({"adapterID": "perl"}),
                    )?)?;
                }
                let deadline = Instant::now() + Duration::from_secs(1);
                loop {
                    let messages = transport_messages(&output_view)?;
                    if messages.iter().any(|message| matches!(message,
                        DapMessage::Response { request_seq: 18, success: false, message: Some(message), .. }
                        if message == "Request queue is full; retry the request")) {
                        break;
                    }
                    if Instant::now() >= deadline {
                        return Err(format!(
                            "ordinary queue did not reach its bound: {messages:?}"
                        )
                        .into());
                    }
                    thread::sleep(Duration::from_millis(5));
                }
            }
            let mut control = if matches!(scenario, TransportScenario::Released) {
                Vec::new()
            } else {
                framed_request_with_arguments(
                    2,
                    if disconnect { "disconnect" } else { "cancel" },
                    serde_json::json!({"requestId": 1}),
                )?
            };
            if disconnect {
                control.extend(framed_request_with_arguments(
                    999,
                    "initialize",
                    serde_json::json!({"adapterID": "perl"}),
                )?);
            }
            if cancel_write_failure {
                fail_writes.store(true, Ordering::Release);
            }
            input_tx.send(control)?;
            let deadline = Instant::now() + Duration::from_millis(300);
            let mut early = false;
            if disconnect {
                // Keep the input sender alive and the debugger blocked: an
                // implementation that reads again or awaits the ordinary query
                // timeout cannot finish this control.
                let deadline = Instant::now() + Duration::from_secs(2);
                while !server.is_finished() && Instant::now() < deadline {
                    thread::sleep(Duration::from_millis(5));
                }
                early = server.is_finished();
            } else if require_early_cancel {
                while Instant::now() < deadline {
                    let messages = transport_messages(&output_view)?;
                    if messages.iter().any(|message| {
                        matches!(
                            message,
                            DapMessage::Response { request_seq: 1, success: true, .. }
                        )
                    }) {
                        return Err(format!(
                            "evaluate succeeded before peer release: {messages:?}"
                        )
                        .into());
                    }
                    if messages.iter().any(|message| {
                        matches!(message, DapMessage::Response { request_seq: 2, .. })
                    }) {
                        early = true;
                        break;
                    }
                    thread::sleep(Duration::from_millis(5));
                }
            }
            if !cancel_write_failure && !intake_failure {
                std::fs::write(&release, "release\n")?;
            }
            if !disconnect && !cancel_write_failure && !intake_failure {
                if saturated {
                    let deadline = Instant::now() + Duration::from_secs(2);
                    loop {
                        let messages = transport_messages(&output_view)?;
                        if (10..18).all(|request| {
                            messages.iter().any(|message| {
                                matches!(message,
                            DapMessage::Response { request_seq, .. } if *request_seq == request)
                            })
                        }) {
                            break;
                        }
                        if Instant::now() >= deadline {
                            return Err(format!(
                                "accepted queue did not drain after peer release: {messages:?}"
                            )
                            .into());
                        }
                        thread::sleep(Duration::from_millis(5));
                    }
                }
                input_tx.send(framed_request_with_arguments(
                    3,
                    "threads",
                    serde_json::json!({}),
                )?)?;
            }
            Ok(early)
        })();
        let exercise_result = exercise;
        // The write-failure case must finish while the peer remains blocked;
        // releasing it here would let an implementation without the common
        // shutdown path pass by eventually completing the evaluate request.
        let finished_before_release = if cancel_write_failure || intake_failure {
            let deadline = Instant::now() + Duration::from_secs(1);
            while !server.is_finished() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(5));
            }
            server.is_finished()
        } else {
            false
        };
        let release_result = if (cancel_write_failure || intake_failure) && finished_before_release
        {
            Ok(())
        } else {
            std::fs::write(&release, "release\n")
        };
        drop(input_tx);
        let deadline = Instant::now()
            + if (cancel_write_failure || intake_failure) && finished_before_release {
                Duration::from_secs(1)
            } else {
                Duration::from_secs(7)
            };
        while !server.is_finished() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        if !server.is_finished() {
            return Err("transport did not finish after releasing peer and closing input".into());
        }
        let server_result = server.join().map_err(|_| "transport thread panicked")?;
        let early = exercise_result?;
        if intake_failure {
            if server_result.is_ok() {
                return Err("transport intake failure unexpectedly returned Ok".into());
            }
            let messages = transport_messages(&output_view)?;
            let queued = messages
                .iter()
                .find(|message| matches!(message, DapMessage::Response { request_seq: 3, .. }));
            if !matches!(queued,
                Some(DapMessage::Response {
                    success: false,
                    message: Some(message),
                    ..
                }) if message.contains("transport intake failure"))
            {
                return Err(format!(
                    "queued request did not report the intake failure: {queued:?}; all={messages:?}"
                )
                .into());
            }
            return Ok(());
        }
        if cancel_write_failure {
            if !finished_before_release {
                return Err(
                    "transport write failure did not settle the blocked worker before peer release"
                        .into(),
                );
            }
            if server_result.is_ok() {
                return Err("transport accepted a cancel response write failure".into());
            }
            return Ok(());
        }
        server_result?;
        release_result?;
        let messages = transport_messages(&output_view)?;
        if saturated {
            for request in 10..=18 {
                let matching: Vec<_> = messages
                    .iter()
                    .filter(|message| {
                        matches!(message,
                    DapMessage::Response { request_seq, .. } if *request_seq == request)
                    })
                    .collect();
                let expected_success = !disconnect && request != 18;
                if matching.len() != 1
                    || !matches!(matching.first(),
                    Some(DapMessage::Response { success, .. }) if *success == expected_success)
                {
                    return Err(format!(
                        "queued request {request} disposition is wrong: {matching:?}"
                    )
                    .into());
                }
            }
        }
        if disconnect {
            if !early {
                return Err(
                    "disconnect did not finish before peer release and input closure".into()
                );
            }
            let responses: Vec<_> = messages
                .iter()
                .filter(|message| matches!(message, DapMessage::Response { request_seq: 2, .. }))
                .collect();
            if responses.len() != 1
                || !matches!(responses.first(),
                Some(DapMessage::Response { success: true, command, .. }) if command == "disconnect")
            {
                return Err(format!("expected one successful disconnect: {responses:?}").into());
            }
            let evaluation: Vec<_> = messages
                .iter()
                .filter(|message| matches!(message, DapMessage::Response { request_seq: 1, .. }))
                .collect();
            if evaluation.len() != 1
                || !matches!(evaluation.first(),
                Some(DapMessage::Response { command, success: false, .. }) if command == "evaluate")
            {
                return Err(
                    format!("active evaluate did not settle as a failure: {evaluation:?}").into()
                );
            }
            if messages
                .iter()
                .filter(|message| {
                    matches!(message,
                DapMessage::Event { event, .. } if event == "terminated")
                })
                .count()
                != 1
            {
                return Err(format!("expected one terminal event: {messages:?}").into());
            }
            if messages.iter().any(|message| {
                matches!(message, DapMessage::Response { request_seq: 999, .. })
                    || matches!(message,
                DapMessage::Event { event, .. } if event == "initialized")
            }) {
                return Err(
                    format!("disconnect allowed later stateful dispatch: {messages:?}").into()
                );
            }
        }
        for (request, command) in
            [(1, "evaluate"), (3, "threads")].into_iter().filter(|_| !disconnect)
        {
            let matching: Vec<_> = messages
                .iter()
                .filter(|message| {
                    matches!(message,
                DapMessage::Response { request_seq, .. } if *request_seq == request)
                })
                .collect();
            let evaluate_cancelled = request == 1
                && matches!(
                    scenario,
                    TransportScenario::Cancel | TransportScenario::FullQueueCancel
                );
            let valid_response = if evaluate_cancelled {
                matches!(matching.first(), Some(DapMessage::Response {
                    command: actual_command,
                    success: false,
                    message: Some(message),
                    ..
                }) if actual_command == command && message.contains("cancelled"))
            } else {
                matches!(matching.first(), Some(DapMessage::Response {
                    command: actual,
                    success: true,
                    body: Some(_),
                    ..
                }) if actual == command)
            };
            if matching.len() != 1 || !valid_response {
                return Err(format!("expected one successful {command}: {matching:?}").into());
            }
        }
        if !disconnect
            && !matches!(scenario, TransportScenario::Cancel | TransportScenario::FullQueueCancel)
            && !messages.iter().any(|message| {
                matches!(message,
            DapMessage::Response { request_seq: 1, body: Some(body), .. }
            if body.get("result").and_then(serde_json::Value::as_str) == Some("42"))
            })
        {
            return Err(format!("evaluate did not return peer value: {messages:?}").into());
        }
        let cancel: Vec<_> = messages
            .iter()
            .filter(|message| matches!(message, DapMessage::Response { request_seq: 2, .. }))
            .collect();
        if !disconnect
            && require_early_cancel
            && !matches!(
                scenario,
                TransportScenario::CancelWriteFailure | TransportScenario::IntakeFailure
            )
            && (cancel.len() != 1
                || !matches!(cancel.first(),
            Some(DapMessage::Response { command, success, body: None, .. })
            if command == "cancel" && *success))
        {
            return Err(format!("public cancel acknowledgement changed: {cancel:?}").into());
        }
        if require_early_cancel && !early {
            return Err("cancel acknowledgement arrived only after releasing blocked evaluate; recovery controls passed".into());
        }
        if require_early_cancel {
            for (expected, message) in (1..).zip(messages.iter()) {
                let actual = match message {
                    DapMessage::Request { seq, .. }
                    | DapMessage::Response { seq, .. }
                    | DapMessage::Event { seq, .. } => *seq,
                };
                if actual != expected {
                    return Err(format!(
                        "wire sequence must be {expected}, got {actual}: {messages:?}"
                    )
                    .into());
                }
            }
        }
        Ok(())
    }

    fn framed_message(message: &DapMessage) -> Result<Vec<u8>, serde_json::Error> {
        let body = serde_json::to_vec(message)?;
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        let mut frame = header.into_bytes();
        frame.extend_from_slice(&body);
        Ok(frame)
    }

    // ── Tests ─────────────────────────────────────────────────────────────────

    /// When the output writer fails on the very first write (e.g. the client closed
    /// the socket immediately), `run_with_io` must return an I/O error rather than
    /// hanging or panicking.
    #[test]
    fn test_run_with_io_returns_error_on_immediate_write_failure() {
        let mut adapter = DebugAdapter::new();
        let input = Cursor::new(framed_request(1, "initialize"));
        let writer = FailingWriter::always_failing();
        let result = adapter.run_with_io(input, writer);
        assert!(result.is_err(), "run_with_io must return Err when writer is broken immediately");
    }

    #[test]
    fn response_write_failure_returns_without_waiting_for_another_input_frame()
    -> Result<(), Box<dyn std::error::Error>> {
        let (input_tx, input_rx) = std::sync::mpsc::channel();
        let writer = FailingWriter::always_failing();
        let writes = Arc::clone(&writer.write_count);
        let server = thread::spawn(move || {
            let mut adapter = DebugAdapter::new();
            adapter.run_with_io(
                ChannelReader {
                    receiver: input_rx,
                    pending: Cursor::new(Vec::new()),
                    failure_trigger: None,
                    failed: false,
                },
                writer,
            )
        });
        let exercise = (|| -> Result<bool, Box<dyn std::error::Error>> {
            input_tx.send(framed_request_with_arguments(
                1,
                "initialize",
                serde_json::json!({"adapterID": "perl"}),
            )?)?;
            let deadline = Instant::now() + Duration::from_secs(2);
            while writes.load(Ordering::Acquire) == 0 {
                if Instant::now() >= deadline {
                    return Err("response never reached the failing writer".into());
                }
                thread::sleep(Duration::from_millis(5));
            }
            let deadline = Instant::now() + Duration::from_millis(300);
            while !server.is_finished() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(5));
            }
            Ok(server.is_finished())
        })();
        // Close the input only after observing the prompt-return oracle. This
        // also releases the broken implementation for deterministic cleanup.
        drop(input_tx);
        let deadline = Instant::now() + Duration::from_secs(2);
        while !server.is_finished() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        if !server.is_finished() {
            return Err("transport remained blocked after test input closed".into());
        }
        let result = server.join().map_err(|_| "transport thread panicked")?;
        if result.is_ok() {
            return Err("transport lost the response write failure".into());
        }
        if !exercise? {
            return Err(
                "response write failed, but transport returned only after input was closed".into(),
            );
        }
        Ok(())
    }

    #[test]
    fn one_event_write_failure_wakes_idle_intake() -> Result<(), Box<dyn std::error::Error>> {
        let (input_tx, input_rx) = std::sync::mpsc::channel();
        // Four writes and one flush complete the initialize response; the
        // following initialized event must be the first failed write.
        let writer = FailingWriter::fail_after(5);
        let writes = Arc::clone(&writer.write_count);
        let server = thread::spawn(move || {
            let mut adapter = DebugAdapter::new();
            adapter.run_with_io(
                ChannelReader {
                    receiver: input_rx,
                    pending: Cursor::new(Vec::new()),
                    failure_trigger: None,
                    failed: false,
                },
                writer,
            )
        });
        input_tx.send(framed_request_with_arguments(
            1,
            "initialize",
            serde_json::json!({"adapterID": "perl"}),
        )?)?;
        let deadline = Instant::now() + Duration::from_secs(2);
        while !server.is_finished() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        let finished = server.is_finished();
        drop(input_tx);
        let result = server.join().map_err(|_| "transport thread panicked")?;
        if !finished {
            return Err("one event write failure left idle intake blocked".into());
        }
        if writes.load(Ordering::Acquire) < 5 {
            return Err("failure occurred before the initialized event write".into());
        }
        if result.is_ok() {
            return Err("event write failure must terminate the transport".into());
        }
        Ok(())
    }

    #[test]
    fn failed_disconnect_retains_cleanup_owner_for_a_real_retry()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut adapter = DebugAdapter::new();
        adapter.seed_session_for_test()?;
        let expected_pid = lock_or_recover(&adapter.session, "transport.test.session")
            .as_ref()
            .ok_or("test session was not installed")?
            .process
            .id();
        adapter.fail_next_cleanup_for_test();
        let session_state = Arc::clone(&adapter.session);
        let (input_tx, input_rx) = std::sync::mpsc::channel();
        let first_request = framed_request_with_arguments(1, "disconnect", serde_json::json!({}))
            .map_err(|error| format!("first disconnect frame failed: {error}"))?;
        let retry_request = framed_request_with_arguments(2, "disconnect", serde_json::json!({}))
            .map_err(|error| format!("retry disconnect frame failed: {error}"))?;
        let output = SharedWriter::default();
        let output_view = output.clone();
        let server = thread::spawn(move || {
            adapter.run_with_io(
                ChannelReader {
                    receiver: input_rx,
                    pending: Cursor::new(Vec::new()),
                    failure_trigger: None,
                    failed: false,
                },
                output,
            )
        });
        if let Err(error) = input_tx.send(first_request) {
            drop(input_tx);
            let _ = server.join();
            return Err(format!("first disconnect send failed: {error}").into());
        }
        let first_deadline = Instant::now() + Duration::from_secs(2);
        let mut first_failed = false;
        let mut observation_error = None;
        while Instant::now() < first_deadline {
            let messages = match transport_messages(&output_view) {
                Ok(messages) => messages,
                Err(error) => {
                    observation_error =
                        Some(format!("first disconnect observation failed: {error}"));
                    break;
                }
            };
            if messages.iter().any(|message| {
                matches!(message,
                    DapMessage::Response { request_seq: 1, command, success: false, .. }
                    if command == "disconnect")
            }) {
                first_failed = true;
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        if let Some(error) = observation_error {
            drop(input_tx);
            let _ = server.join();
            return Err(error.into());
        }
        let retained = lock_or_recover(&session_state, "transport.test.session");
        let retained_owner = retained.as_ref().map(|session| session.process.id());
        let retained_state = retained.as_ref().map(|session| session.state.clone());
        drop(retained);
        if let Err(error) = input_tx.send(retry_request) {
            drop(input_tx);
            let _ = server.join();
            return Err(format!("retry disconnect send failed: {error}").into());
        }
        drop(input_tx);
        let second_deadline = Instant::now() + Duration::from_secs(5);
        while !server.is_finished() && Instant::now() < second_deadline {
            thread::sleep(Duration::from_millis(5));
        }
        if !server.is_finished() {
            let cleanup_succeeded = {
                let mut session = lock_or_recover(&session_state, "transport.test.session");
                session
                    .as_mut()
                    .map(|session| DebugAdapter::terminate_child_process(&mut session.process))
                    .unwrap_or(true)
            };
            let cleanup_deadline = Instant::now() + Duration::from_secs(2);
            while !server.is_finished() && Instant::now() < cleanup_deadline {
                thread::sleep(Duration::from_millis(5));
            }
            if server.is_finished() {
                let _ = server.join();
            }
            return Err(format!(
                "disconnect retry transport did not finish after cleanup (succeeded={cleanup_succeeded})"
            )
            .into());
        }
        let result = server.join().map_err(|_| "transport thread panicked")?;
        result?;
        let responses: Vec<_> = transport_messages(&output_view)?
            .into_iter()
            .filter_map(|message| match message {
                DapMessage::Response { request_seq, success, command, .. }
                    if command == "disconnect" =>
                {
                    Some((request_seq, success))
                }
                _ => None,
            })
            .collect();
        if !first_failed
            || retained_owner != Some(expected_pid)
            || retained_state != Some(DebugState::Terminated)
        {
            return Err(format!(
                "failed disconnect did not retain terminated owner: first_failed={first_failed}, owner={retained_owner:?}, state={retained_state:?}, responses={responses:?}"
            )
            .into());
        }
        if responses != [(1, false), (2, true)] {
            return Err(format!(
                "expected failed disconnect followed by retry success: {responses:?}"
            )
            .into());
        }
        if lock_or_recover(&session_state, "transport.test.session").is_some() {
            return Err(format!("cleanup retry retained child owner (pid {expected_pid})").into());
        }
        Ok(())
    }

    /// Regression for issue #5149 / PR #5318 defect 1, exercised against the actual
    /// production function `write_message_then_notify_initialized` that `run_with_io`
    /// calls for every request.
    ///
    /// Before the fix, the response-writer guard was held (unscoped) across the
    /// `initialized` event dispatch that follows a successful `initialize` response.
    /// `initialized` is not an `output` event, so `dispatch_event` takes the *blocking*
    /// `send` path — which blocks whenever the outbound queue is full. The event-consumer
    /// thread needs that very same writer mutex to drain a batch and free a slot, so
    /// holding both at once is a lock-ordering deadlock: the producer blocked on the
    /// channel send while holding the writer mutex, and the consumer blocked on the
    /// writer mutex while trying to drain the channel that would unblock the producer.
    ///
    /// The consumer takes one event and pauses before writing it. We refill the
    /// queue and hold the wire counter until the producer owns the writer. After
    /// releasing both gates, the consumer must finish that write before receiving
    /// again. Holding the writer across the initialized send therefore deadlocks
    /// both threads; freeing the queue before this handshake would miss the bug.
    ///
    /// It never blocks the test suite: the call runs on its own thread and is joined
    /// with a bounded timeout, so a regression fails the assertion instead of hanging.
    #[test]
    fn write_message_then_notify_initialized_does_not_deadlock_on_full_queue() -> Result<(), String>
    {
        let shared_writer: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let (tx, rx) = sync_channel::<DapMessage>(1);
        let seq = Arc::new(Mutex::new(0i64));
        let wire_seq = Arc::new(Mutex::new(0i64));
        let (consumer_ready_tx, consumer_ready_rx) = sync_channel(1);
        let (consumer_go_tx, consumer_go_rx) = sync_channel(1);

        // Fill the single slot so the `initialized` dispatch below must wait for a drain.
        let fill = dispatch_event(&tx, &seq, "output", Some(serde_json::json!({"output": "x\n"})));
        if fill != sync_utils::EventDispatchResult::Sent {
            return Err(format!("expected the fill send to succeed, got {fill:?}"));
        }

        // Consumer thread: the same shape as `run_with_io`'s event-handler thread —
        // receive from the channel (no lock needed), then acquire the writer mutex to
        // "write" the drained message, freeing the slot `initialized` is waiting for.
        let consumer_writer = Arc::clone(&shared_writer);
        let consumer = thread::spawn(move || -> Result<(), String> {
            rx.recv().map_err(|error| error.to_string())?;
            consumer_ready_tx.send(()).map_err(|error| error.to_string())?;
            consumer_go_rx.recv().map_err(|error| error.to_string())?;
            // The consumer must complete its current write batch before it
            // can receive the next queued event and free the full slot.
            drop(lock_or_recover(&consumer_writer, "test.event_writer"));
            rx.recv().map_err(|error| error.to_string())?;
            match rx.recv().map_err(|error| error.to_string())? {
                DapMessage::Event { event, .. } if event == "initialized" => Ok(()),
                other => Err(format!("expected initialized event, got {other:?}")),
            }
        });
        consumer_ready_rx
            .recv_timeout(Duration::from_secs(2))
            .map_err(|error| error.to_string())?;
        if dispatch_event(&tx, &seq, "output", None) != sync_utils::EventDispatchResult::Sent {
            return Err("could not refill the event queue".to_string());
        }

        // Drive the real production function on its own thread so the test can bound
        // how long it waits for it, rather than being at the mercy of a real deadlock.
        let producer_writer = Arc::clone(&shared_writer);
        let producer_seq = Arc::clone(&seq);
        let producer_wire_seq = Arc::clone(&wire_seq);
        // The production write path acquires the writer before this counter.
        // Holding the counter lets us establish that it owns the writer while
        // the consumer is paused and its input queue is full.
        let sequence_guard = lock_or_recover(&wire_seq, "test.wire_seq");
        let producer = thread::spawn(move || {
            write_message_then_notify_initialized(
                &producer_writer,
                DapMessage::Response {
                    seq: 0,
                    request_seq: 1,
                    command: "initialize".to_string(),
                    success: true,
                    body: None,
                    message: None,
                },
                true,
                Some(&EventSender::new(tx.clone())),
                &producer_seq,
                &producer_wire_seq,
            )
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        let writer_owned = loop {
            match shared_writer.try_lock() {
                Err(std::sync::TryLockError::WouldBlock) => break true,
                Err(std::sync::TryLockError::Poisoned(_)) => break false,
                Ok(guard) => drop(guard),
            }
            if Instant::now() >= deadline {
                break false;
            }
            thread::sleep(Duration::from_millis(1));
        };
        consumer_go_tx.send(()).map_err(|error| error.to_string())?;
        drop(sequence_guard);

        let deadline = Duration::from_secs(5);
        let start = Instant::now();
        while start.elapsed() < deadline {
            if producer.is_finished() && consumer.is_finished() {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }

        if !(producer.is_finished() && consumer.is_finished()) {
            return Err(format!(
                "write_message_then_notify_initialized failed to complete within \
                 {deadline:?} against a full outbound queue with a real consumer thread \
                 waiting on the same writer mutex — this is the #5149/PR #5318 defect 1 \
                 deadlock: the writer guard is being held across the blocking \
                 `initialized` dispatch instead of being dropped first. \
                 producer_finished={}, consumer_finished={}",
                producer.is_finished(),
                consumer.is_finished()
            ));
        }

        producer
            .join()
            .map_err(|_| "producer thread panicked".to_string())?
            .map_err(|e| format!("write_message_then_notify_initialized returned Err: {e}"))?;
        consumer.join().map_err(|_| "consumer thread panicked".to_string())??;
        if !writer_owned {
            return Err("production writer did not own serialization before sequence allocation"
                .to_string());
        }
        Ok(())
    }

    #[test]
    fn test_failing_writer_fails_at_configured_boundary() {
        let mut writer = FailingWriter::fail_after(1);

        let first = writer.write(b"a");
        assert!(matches!(first, Ok(1)), "first write should succeed before the boundary");
        assert_eq!(
            writer.write_count.load(AOrdering::Acquire),
            writer.fail_after_writes,
            "write count should sit exactly on the configured failure boundary"
        );

        let second = writer.write(b"b");
        assert!(
            matches!(second, Err(ref error) if error.kind() == io::ErrorKind::BrokenPipe),
            "write at the configured boundary must return BrokenPipe"
        );
        assert_eq!(
            writer.write_count.load(AOrdering::Acquire),
            writer.fail_after_writes + 1,
            "failed boundary write must still be counted"
        );
    }

    #[test]
    fn test_run_with_io_handles_non_request_messages_without_output()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut input_bytes = framed_message(&DapMessage::Response {
            seq: 1,
            request_seq: 99,
            success: true,
            command: "serverInitiatedRequest".to_string(),
            body: None,
            message: Some("client response".to_string()),
        })?;
        input_bytes.extend_from_slice(&framed_message(&DapMessage::Event {
            seq: 2,
            event: "clientEvent".to_string(),
            body: Some(serde_json::json!({"source": "client"})),
        })?);

        let mut adapter = DebugAdapter::new();
        let writer = SharedWriter::default();
        let written = writer.bytes.clone();

        let result = adapter.run_with_io(Cursor::new(input_bytes), writer);

        assert!(result.is_ok(), "non-request messages must not fail the transport loop");
        let bytes = written.lock().map_err(|_| "writer buffer mutex poisoned")?;
        assert!(bytes.is_empty(), "non-request messages must not emit adapter output");
        Ok(())
    }

    /// A writer that succeeds for a few writes then fails permanently triggers the
    /// supervision path: the event-handler sets `transport_broken`, and the main
    /// loop detects it on the next iteration and returns `BrokenPipe`.
    ///
    /// Regression test for #1609: before this fix the event handler would log errors
    /// forever and the main loop would never notice the broken transport.
    #[test]
    fn test_transport_broken_flag_triggers_main_loop_exit() {
        // Allow enough writes for the initialize-response framing to complete, then
        // fail everything.  Each Content-Length response involves ~3 write calls
        // (header prefix, length, \r\n\r\n, body) — 6 successes is sufficient for
        // one response while ensuring event writes fail.
        let mut adapter = DebugAdapter::new();

        // Two requests queued: initialize (triggers initialized event write which
        // will fail) + a second request so the main loop iterates again and can
        // detect the broken flag.
        let mut input_bytes = framed_request(1, "initialize");
        input_bytes.extend_from_slice(&framed_request(2, "stackTrace"));
        let input = Cursor::new(input_bytes);
        let writer = FailingWriter::fail_after(6);

        let result = adapter.run_with_io(input, writer);
        // Either the event-writer flag fires or the main-loop write fails — either
        // way the function must not return Ok while the transport is broken.
        assert!(result.is_err(), "run_with_io must return Err when output is persistently broken");
    }

    /// The event-handler thread must exit in bounded time when writes fail
    /// permanently.  This guards against infinite retry loops.
    #[test]
    fn test_event_handler_exits_in_bounded_time_after_write_failure() {
        let (done_tx, done_rx) = std::sync::mpsc::channel::<io::Result<()>>();
        thread::spawn(move || {
            let mut adapter = DebugAdapter::new();
            let input = Cursor::new(framed_request(1, "initialize"));
            let writer = FailingWriter::always_failing();
            let _ = done_tx.send(adapter.run_with_io(input, writer));
        });
        let result = done_rx.recv_timeout(Duration::from_secs(5));
        assert!(
            result.is_ok(),
            "run_with_io must complete within 5 s after persistent write failure"
        );
    }

    /// The `transport_broken` flag starts as `false` on a fresh adapter and is
    /// not set by a successful run (clean EOF on the input side).
    #[test]
    fn test_transport_broken_flag_clear_on_clean_run() -> Result<(), Box<dyn std::error::Error>> {
        let mut adapter = DebugAdapter::new();
        // Empty input → immediate EOF → clean Ok(()) return.
        let input = Cursor::new(vec![]);
        // Writer that always succeeds (Vec<u8>).
        let result = adapter.run_with_io(input, Vec::<u8>::new());
        assert!(result.is_ok(), "clean EOF must return Ok");
        // Flag must remain false.
        assert!(
            !adapter.transport_broken.load(AOrdering::Acquire),
            "transport_broken must remain false after a clean run"
        );
        assert!(!adapter.native_stdio_transport, "native stdio mode must not stick after run");
        let body = match adapter.handle_request(1, "initialize", None) {
            DapMessage::Response { body: Some(body), .. } => body,
            response => return Err(format!("direct initialize returned {response:?}").into()),
        };
        assert_eq!(
            body.get("supportsCancelRequest").and_then(serde_json::Value::as_bool),
            Some(false),
            "direct adapter reuse must not inherit native stdio cancellation"
        );
        Ok(())
    }

    #[test]
    fn test_write_event_payloads_successful_flush_keeps_transport_healthy() {
        let mut writer = Vec::<u8>::new();
        let transport_broken = AtomicBool::new(false);
        let payloads = vec![b"{}".to_vec()];
        let mut flushed = false;

        let threshold_hit =
            write_event_payloads(&mut writer, &payloads, &transport_broken, &mut flushed);

        assert!(!threshold_hit, "successful event write must not mark the transport broken");
        assert!(flushed, "successful flush must report delivery");
        assert!(
            !transport_broken.load(AOrdering::Acquire),
            "transport_broken must remain false after successful flush"
        );
        assert!(
            String::from_utf8_lossy(&writer).starts_with("Content-Length:"),
            "event payload must be written as a DAP frame"
        );
    }

    #[test]
    fn test_write_framed_payload_uses_exact_byte_length_for_truncated_payload() -> io::Result<()> {
        let payload = br#"{"type":"response","body":"unterminated"#;
        let mut writer = Vec::new();

        write_framed_payload(&mut writer, payload)?;

        let separator = writer
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "frame separator missing"))?;
        let header = &writer[..separator];
        let length =
            std::str::from_utf8(header.strip_prefix(b"Content-Length: ").ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "length header missing")
            })?)
            .map_err(io::Error::other)?
            .parse::<usize>()
            .map_err(io::Error::other)?;

        assert_eq!(length, payload.len(), "header must count payload bytes exactly");
        assert_eq!(&writer[separator + 4..], payload, "frame body must be unmodified");
        Ok(())
    }

    #[test]
    fn test_write_framed_payload_retries_short_writes_without_truncation() -> io::Result<()> {
        let payload = "{\"message\":\"café\"}".as_bytes();
        let mut writer = ChunkedWriter { bytes: Vec::new(), max_chunk: 2 };

        write_framed_payload(&mut writer, payload)?;

        let expected_header = format!("Content-Length: {}\r\n\r\n", payload.len());
        assert!(writer.bytes.starts_with(expected_header.as_bytes()));
        assert_eq!(&writer.bytes[expected_header.len()..], payload);
        Ok(())
    }

    #[test]
    fn test_write_event_payloads_flush_failure_marks_transport_broken_immediately() {
        let mut writer = FlushFailingWriter::default();
        let transport_broken = AtomicBool::new(false);
        let payloads = vec![b"{}".to_vec()];
        let mut flushed = false;

        let threshold_hit =
            write_event_payloads(&mut writer, &payloads, &transport_broken, &mut flushed);

        assert!(threshold_hit, "flush failure must mark the transport broken");
        assert!(!flushed, "failed flush must not report delivery");
        assert!(
            transport_broken.load(AOrdering::Acquire),
            "transport_broken must be set after threshold flush failure"
        );
        assert!(
            String::from_utf8_lossy(&writer.bytes).starts_with("Content-Length:"),
            "flush failure must occur after writing the DAP frame"
        );
    }

    #[test]
    fn test_run_with_io_returns_broken_pipe_when_flag_is_already_set() {
        let mut adapter = DebugAdapter::new();
        adapter.transport_broken.store(true, AOrdering::Release);

        let result = adapter.run_with_io(Cursor::new(Vec::<u8>::new()), Vec::<u8>::new());

        assert!(
            matches!(result, Err(ref error) if error.kind() == io::ErrorKind::BrokenPipe),
            "pre-marked broken transport must return BrokenPipe"
        );
        assert!(!adapter.native_stdio_transport, "native stdio mode must reset after an error");
    }
}

#[cfg(test)]
mod framing_tests {
    //! Stdio transport framing edge-case coverage for `run_with_io`.
    //!
    //! Each test drives the transport loop with an in-memory `std::io::Cursor`
    //! reader and a shared `Arc<Mutex<Vec<u8>>>` writer, asserting NO PANIC and
    //! graceful recovery or clean shutdown on EOF. These live in-crate (rather
    //! than in `tests/`) because `run_with_io` is `pub(super)` — keeping the
    //! transport loop crate-private while still covering its framing seam, with
    //! no production test-only shim.
    //!
    //! Tested at the loop level: behaviour (no panic, skip or continue), not
    //! internal framing error types.

    use super::*;
    use serde_json::json;
    use std::io::Cursor;

    // ── shared output buffer ──────────────────────────────────────────────────

    /// A clonable, thread-safe write buffer satisfying `Write + Send + 'static`.
    ///
    /// Because `run_with_io` requires `W: Write + Send + 'static` (the writer is
    /// moved into the event-handler thread), we cannot pass a `&mut Vec<u8>`.
    /// Instead we wrap the buffer in `Arc<Mutex<_>>` and implement `Write` on the
    /// wrapper so we can inspect the bytes after the loop completes.
    #[derive(Clone, Default)]
    struct SharedBuf(Arc<Mutex<Vec<u8>>>);

    struct DisconnectThenReadError {
        input: Vec<u8>,
        consumed: bool,
    }

    impl io::Read for DisconnectThenReadError {
        fn read(&mut self, target: &mut [u8]) -> io::Result<usize> {
            if self.consumed {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "read after orderly disconnect",
                ));
            }
            self.consumed = true;
            let amount = self.input.len().min(target.len());
            target[..amount].copy_from_slice(&self.input[..amount]);
            Ok(amount)
        }
    }

    impl BoundedRead for DisconnectThenReadError {
        fn read_with_timeout(
            &mut self,
            target: &mut [u8],
            _timeout: Duration,
        ) -> io::Result<Option<usize>> {
            self.read(target).map(Some)
        }
    }

    impl SharedBuf {
        fn new() -> Self {
            Self(Arc::new(Mutex::new(Vec::new())))
        }

        fn bytes_snapshot(&self) -> Vec<u8> {
            self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
        }
    }

    impl io::Write for SharedBuf {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let mut guard = self.0.lock().unwrap_or_else(|e| e.into_inner());
            guard.write(buf)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// The response is writable, but flushing a terminated event fails.  This
    /// exercises the real queue-drain acknowledgment path rather than only the
    /// write-failure counter helper.
    #[derive(Default)]
    struct TerminatedEventFlushFailingWriter {
        bytes: Vec<u8>,
    }

    impl io::Write for TerminatedEventFlushFailingWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.bytes.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            let pending = std::mem::take(&mut self.bytes);
            if pending
                .windows(b"\"event\":\"terminated\"".len())
                .any(|window| window == b"\"event\":\"terminated\"")
            {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "mock terminated-event flush failure",
                ));
            }
            Ok(())
        }
    }

    // ── helpers ────────────────────────────────────────────────────────────────

    /// Build a well-formed Content-Length framed DAP request.
    fn framed_request(seq: i64, command: &str, arguments: Option<serde_json::Value>) -> Vec<u8> {
        let args_part = match arguments {
            Some(v) => v.to_string(),
            None => "null".to_string(),
        };
        let body = format!(
            r#"{{"type":"request","seq":{seq},"command":"{command}","arguments":{args_part}}}"#
        );
        let mut frame = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
        frame.extend_from_slice(body.as_bytes());
        frame
    }

    /// Build a Content-Length framed payload from arbitrary bytes (may not be valid JSON).
    fn framed_raw(body: &[u8]) -> Vec<u8> {
        let mut frame = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
        frame.extend_from_slice(body);
        frame
    }

    /// Extract the body bytes of the first framed message from `buf`.
    ///
    /// The event-handler thread may append an "initialized" event to the shared
    /// writer *after* `run_with_io` returns (the thread is not joined before the
    /// function exits). Parsing `written[separator..end]` as JSON therefore fails
    /// non-deterministically because the slice contains the initialize response
    /// JSON followed by a second Content-Length frame.
    ///
    /// This helper parses the `Content-Length` header of the first frame and
    /// returns only those bytes, avoiding the race.
    fn first_frame_body(buf: &[u8]) -> io::Result<&[u8]> {
        // Locate the header/body separator.
        let sep_pos = buf.windows(4).position(|w| w == b"\r\n\r\n").ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "no CRLF separator in output")
        })?;
        let header = &buf[..sep_pos];
        let body_start = sep_pos + 4;

        // Parse Content-Length from the header.
        let cl_prefix = b"Content-Length: ";
        let cl_start =
            header.windows(cl_prefix.len()).position(|w| w == cl_prefix).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "no Content-Length in header")
            })? + cl_prefix.len();
        let cl_end = header[cl_start..]
            .iter()
            .position(|&b| b == b'\r' || b == b'\n')
            .map(|p| cl_start + p)
            .unwrap_or(header.len());
        let cl_str = std::str::from_utf8(&header[cl_start..cl_end])
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let cl: usize = cl_str.trim().parse().map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidData, format!("bad Content-Length: {e}"))
        })?;

        let body_end = body_start + cl;
        buf.get(body_start..body_end).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "buffer truncated before body end")
        })
    }

    // ── 1. Missing Content-Length header ──────────────────────────────────────

    #[test]
    fn test_transport_missing_content_length_header_no_panic() -> io::Result<()> {
        let input = b"X-Custom: foo\r\n\r\n".to_vec();
        let mut adapter = DebugAdapter::new();
        let result = adapter.run_with_io(Cursor::new(input), SharedBuf::new());
        match result {
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {}
            Err(e) => return Err(e),
        }
        Ok(())
    }

    // ── 2. Non-numeric Content-Length ──────────────────────────────────────────

    #[test]
    fn test_transport_non_numeric_content_length_no_panic() -> io::Result<()> {
        let input = b"Content-Length: notanumber\r\n\r\n".to_vec();
        let mut adapter = DebugAdapter::new();
        let result = adapter.run_with_io(Cursor::new(input), SharedBuf::new());
        let _ = result;
        Ok(())
    }

    // ── 3. Negative Content-Length ─────────────────────────────────────────────

    #[test]
    fn test_transport_negative_content_length_no_panic() -> io::Result<()> {
        let input = b"Content-Length: -1\r\n\r\n".to_vec();
        let mut adapter = DebugAdapter::new();
        let result = adapter.run_with_io(Cursor::new(input), SharedBuf::new());
        let _ = result;
        Ok(())
    }

    // ── 4. Valid header + malformed JSON body ──────────────────────────────────

    #[test]
    fn test_transport_valid_header_malformed_json_no_panic() -> io::Result<()> {
        let bad_body = b"this is not json at all!";
        let input = framed_raw(bad_body);
        let mut adapter = DebugAdapter::new();
        let result = adapter.run_with_io(Cursor::new(input), SharedBuf::new());
        let _ = result;
        Ok(())
    }

    // ── 5. Multiple messages in one buffer (both processed) ───────────────────

    #[test]
    fn test_transport_two_messages_in_one_buffer_no_panic() -> io::Result<()> {
        let mut input =
            framed_request(1, "initialize", Some(json!({"clientID": "test", "adapterID": "perl"})));
        input.extend(framed_request(2, "disconnect", None));

        let output = SharedBuf::new();
        let mut adapter = DebugAdapter::new();
        adapter.run_with_io(Cursor::new(input), output.clone())?;

        let written = output.bytes_snapshot();
        assert!(
            written.starts_with(b"Content-Length:"),
            "expected at least one framed response in output, got {} bytes",
            written.len()
        );
        Ok(())
    }

    #[test]
    fn test_transport_disconnect_stops_before_post_disconnect_read_error() -> io::Result<()> {
        let mut disconnect = framed_request(1, "disconnect", None);
        disconnect.extend(framed_request(2, "initialize", Some(json!({"adapterID": "perl"}))));
        let input = DisconnectThenReadError { input: disconnect, consumed: false };
        let output = SharedBuf::new();
        let mut adapter = DebugAdapter::new();
        adapter.seed_attached_pid_for_test(4242);
        adapter.run_with_io(input, output.clone())?;
        let written_bytes = output.bytes_snapshot();
        let mut framer = ContentLengthFramer::new();
        framer.push(&written_bytes);
        let mut messages = Vec::new();
        loop {
            match framer.try_next() {
                Ok(Some(body)) => messages.push(
                    serde_json::from_slice::<serde_json::Value>(&body).map_err(|error| {
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!("invalid output frame: {error}"),
                        )
                    })?,
                ),
                Ok(None) => break,
                Err(error) => {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, error.to_string()));
                }
            }
        }
        let disconnect = messages
            .iter()
            .find(|message| {
                message.get("type").and_then(serde_json::Value::as_str) == Some("response")
                    && message.get("command").and_then(serde_json::Value::as_str)
                        == Some("disconnect")
            })
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "disconnect response frame missing")
            })?;
        if disconnect.get("request_seq").and_then(serde_json::Value::as_i64) != Some(1)
            || disconnect.get("success").and_then(serde_json::Value::as_bool) != Some(true)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "disconnect response fields incorrect",
            ));
        }
        if messages.iter().any(|message| {
            message.get("type").and_then(serde_json::Value::as_str) == Some("response")
                && message.get("command").and_then(serde_json::Value::as_str) == Some("initialize")
        }) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "request after disconnect was processed",
            ));
        }
        let terminated_count = messages
            .iter()
            .filter(|message| {
                message.get("type").and_then(serde_json::Value::as_str) == Some("event")
                    && message.get("event").and_then(serde_json::Value::as_str)
                        == Some("terminated")
            })
            .count();
        if terminated_count != 1 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("expected one terminated event, got {terminated_count}"),
            ));
        }
        Ok(())
    }

    #[test]
    fn test_disconnect_does_not_ack_failed_terminated_event_flush() -> io::Result<()> {
        let input = Cursor::new(framed_request(1, "disconnect", None));
        let mut adapter = DebugAdapter::new();
        adapter.seed_attached_pid_for_test(4242);
        let result = adapter.run_with_io(input, TerminatedEventFlushFailingWriter::default());
        match result {
            Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(()),
            Err(error) => Err(io::Error::new(
                error.kind(),
                format!("failed terminated-event flush returned wrong error: {error}"),
            )),
            Ok(()) => Err(io::Error::other(
                "disconnect returned Ok despite failed terminated-event flush",
            )),
        }
    }

    // ── 6. EOF mid-header ──────────────────────────────────────────────────────

    #[test]
    fn test_transport_eof_mid_header_no_panic() -> io::Result<()> {
        let input = b"Content-Leng".to_vec();
        let mut adapter = DebugAdapter::new();
        let result = adapter.run_with_io(Cursor::new(input), SharedBuf::new());
        match result {
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {}
            Err(e) => return Err(e),
        }
        Ok(())
    }

    // ── 7. EOF mid-body ────────────────────────────────────────────────────────

    #[test]
    fn test_transport_eof_mid_body_no_panic() -> io::Result<()> {
        let input = b"Content-Length: 1000\r\n\r\nshort".to_vec();
        let mut adapter = DebugAdapter::new();
        let result = adapter.run_with_io(Cursor::new(input), SharedBuf::new());
        match result {
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {}
            Err(e) => return Err(e),
        }
        Ok(())
    }

    // ── 8. Extra / duplicate headers ───────────────────────────────────────────

    #[test]
    fn test_transport_extra_headers_no_panic() -> io::Result<()> {
        let body_str = r#"{"type":"request","seq":1,"command":"initialize","arguments":{"clientID":"test","adapterID":"perl"}}"#;
        let mut input = format!(
            "Content-Type: application/vscode-jsonrpc; charset=utf-8\r\nContent-Length: {}\r\n\r\n",
            body_str.len()
        )
        .into_bytes();
        input.extend_from_slice(body_str.as_bytes());

        let output = SharedBuf::new();
        let mut adapter = DebugAdapter::new();
        adapter.run_with_io(Cursor::new(input), output.clone())?;

        let written = output.bytes_snapshot();
        assert!(!written.is_empty(), "expected a response to the initialize request");
        Ok(())
    }

    // ── 9. LF-only separator instead of CRLF ──────────────────────────────────

    #[test]
    fn test_transport_lf_only_separator_no_panic() -> io::Result<()> {
        let body_str = r#"{"type":"request","seq":1,"command":"initialize","arguments":null}"#;
        let mut input = format!("Content-Length: {}\n\n", body_str.len()).into_bytes();
        input.extend_from_slice(body_str.as_bytes());

        let mut adapter = DebugAdapter::new();
        adapter.run_with_io(Cursor::new(input), SharedBuf::new())?;
        Ok(())
    }

    // ── 10. Malformed frame followed by well-formed one (recovery) ────────────

    #[test]
    fn test_transport_recovers_after_malformed_frame() -> io::Result<()> {
        let bad_body = b"not-json!!!";
        let mut input = framed_raw(bad_body);
        input.extend(framed_request(
            1,
            "initialize",
            Some(json!({"clientID": "test", "adapterID": "perl"})),
        ));

        let output = SharedBuf::new();
        let mut adapter = DebugAdapter::new();
        adapter.run_with_io(Cursor::new(input), output.clone())?;

        let written = output.bytes_snapshot();

        assert!(
            written.starts_with(b"Content-Length:"),
            "expected a framed response after recovery from malformed frame, got {} bytes",
            written.len()
        );

        let body = first_frame_body(&written)?;
        let parsed: serde_json::Value = serde_json::from_slice(body)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        assert_eq!(parsed["command"], "initialize", "response must be for initialize");
        assert_eq!(parsed["type"], "response", "must be a response message");
        Ok(())
    }

    // ── 11. Empty input (immediate EOF) ───────────────────────────────────────

    #[test]
    fn test_transport_empty_input_clean_shutdown() -> io::Result<()> {
        let mut adapter = DebugAdapter::new();
        adapter.run_with_io(Cursor::new(Vec::<u8>::new()), SharedBuf::new())?;
        Ok(())
    }

    // ── event-before-response ordering (drain barrier) ─────────────────────────

    /// Writer that forwards to [`SharedBuf`] but delays event payload writes,
    /// simulating a slow wire. With the drain barrier the response waits for
    /// the delayed event write; without it the response would win the race
    /// deterministically, making this test a real falsifier of the ordering
    /// claim.
    struct SlowEventWriter {
        inner: SharedBuf,
    }

    impl SlowEventWriter {
        fn new(inner: SharedBuf) -> Self {
            Self { inner }
        }
    }

    impl io::Write for SlowEventWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if buf
                .windows(b"\"event\":\"continued\"".len())
                .any(|w| w == b"\"event\":\"continued\"")
            {
                std::thread::sleep(std::time::Duration::from_millis(60));
            }
            self.inner.write(buf)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.inner.flush()
        }
    }

    /// A child whose only job is to own a piped stdin so the `continue`
    /// handler can observe a debuggee process. It exits immediately; the
    /// handler ignores stdin write failures.
    fn exited_child() -> io::Result<std::process::Child> {
        #[cfg(windows)]
        let program = "cmd";
        #[cfg(not(windows))]
        let program = "true";
        #[cfg(windows)]
        let args: &[&str] = &["/c", "exit", "0"];
        #[cfg(not(windows))]
        let args: &[&str] = &[];
        std::process::Command::new(program)
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
    }

    /// Writer that delays stopped-event payload writes, so the two
    /// stopped events a PID attach with `stopOnEntry` emits accumulate
    /// in a single consumer batch.
    struct SlowStoppedWriter {
        inner: SharedBuf,
    }

    impl SlowStoppedWriter {
        fn new(inner: SharedBuf) -> Self {
            Self { inner }
        }
    }

    impl io::Write for SlowStoppedWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if buf.windows(b"\"event\":\"stopped\"".len()).any(|w| w == b"\"event\":\"stopped\"") {
                std::thread::sleep(std::time::Duration::from_millis(60));
            }
            self.inner.write(buf)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.inner.flush()
        }
    }

    #[test]
    fn multi_event_batch_releases_the_full_drain_count() -> io::Result<()> {
        // FC-DRAIN-PHANTOM-BATCH: a PID attach with `stopOnEntry` emits
        // two stopped events. If the consumer completed only one latch
        // count per batch, phantom residue would push every later
        // response through the full drain timeout. The follow-up request
        // must therefore answer well under that bound, with both events
        // ahead of the attach response on the wire.
        let mut adapter = DebugAdapter::new();
        let own_pid = std::process::id();
        let mut input =
            framed_request(1, "attach", Some(json!({"processId": own_pid, "stopOnEntry": true})));
        input.extend(framed_request(2, "threads", None));
        let output = SharedBuf::new();
        let started = std::time::Instant::now();
        adapter.run_with_io(Cursor::new(input), SlowStoppedWriter::new(output.clone()))?;
        let elapsed = started.elapsed();
        assert!(
            elapsed < std::time::Duration::from_millis(950),
            "follow-up response must not wait out the drain timeout: took {elapsed:?}"
        );

        let snapshot = output.bytes_snapshot();
        let Some(response_offset) = windows_find(&snapshot, b"\"command\":\"attach\"") else {
            return Err(io::Error::other("attach response must be written"));
        };
        let stopped_before = snapshot[..response_offset]
            .windows(b"\"event\":\"stopped\"".len())
            .filter(|w| *w == b"\"event\":\"stopped\"")
            .count();
        assert_eq!(
            stopped_before, 2,
            "both attach stopped events must precede the attach response"
        );
        Ok(())
    }

    #[test]
    fn run_with_io_writes_handler_events_before_the_response() -> io::Result<()> {
        use crate::debug_adapter::session::{DebugSession, DebugState, ResumeMode};
        use crate::debug_adapter::variable_cache::VariableCache;
        use crate::reload::RuntimeModuleGenerationClock;
        use crate::types::StackFrame;
        use std::collections::HashMap;

        let mut adapter = DebugAdapter::new();
        {
            let mut guard = lock_or_recover(&adapter.session, "transport.test.drain.session");
            *guard = Some(DebugSession {
                process: exited_child()?,
                state: DebugState::Stopped,
                stack_frames: Vec::<StackFrame>::new(),
                stack_frame_arguments: HashMap::new(),
                variable_cache: VariableCache::default(),
                thread_id: 1,
                debuggee_cwd: std::path::PathBuf::from("."),
                last_resume_mode: ResumeMode::Unknown,
                initial_stop_pending: false,
                stopped_generation: 1,
                module_generation: RuntimeModuleGenerationClock::new(),
            });
        }
        let input = framed_request(1, "continue", Some(json!({"threadId": 1})));
        let output = SharedBuf::new();
        adapter.run_with_io(Cursor::new(input), SlowEventWriter::new(output.clone()))?;

        // The consumer thread is not joined by run_with_io; poll until the
        // response frame exists (the barrier guarantees the event precedes
        // it, so once the response is on the wire the event must be too).
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let snapshot = loop {
            let snap = output.bytes_snapshot();
            if windows_find(&snap, b"\"command\":\"continue\"").is_some() {
                break snap;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "continue response was never written to the transport"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        };

        let event_offset = windows_find(&snapshot, b"\"event\":\"continued\"")
            .expect("continue handler must emit the continued event");
        let response_offset = windows_find(&snapshot, b"\"command\":\"continue\"")
            .expect("continue response must be written");
        assert!(
            event_offset < response_offset,
            "handler-emitted events must precede the terminal response on the wire              (event at {event_offset}, response at {response_offset})"
        );
        Ok(())
    }

    /// Byte-subsequence search returning the match offset.
    fn windows_find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack.windows(needle.len()).position(|w| w == needle)
    }
}
