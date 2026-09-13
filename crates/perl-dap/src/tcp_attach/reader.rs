use super::event::{DapEvent, dap_event_from_value};
use perl_lsp_rs_core::transport::framing::ContentLengthFramer;
use serde_json::Value;
use std::io::{BufReader, Read};
use std::net::TcpStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

/// Result of admitting one event into the bounded TCP-attach fan-in queue (#9521).
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum EventAdmission {
    /// Event was accepted into the queue.
    Accepted,
    /// An `output` event was shed because the queue was full (counted; a single
    /// bounded notice is attempted).
    DroppedOutput,
    /// The forwarding side is gone: the session is dead.
    Disconnected,
    /// The session retired this producer (disconnect/replacement): the event
    /// was not admitted, and the producer must stop without delivering the
    /// stale event or touching shared connection state (#9521).
    Retired,
}

/// Park interval between full-queue retries in cancellation-aware state-event
/// admission: bounded-latency retirement without a busy spin (#9521).
pub(crate) const ADMISSION_RETIRE_CHECK: Duration = Duration::from_millis(1);

/// Per-session accounting for `output` events dropped under TCP-attach
/// backpressure (#9521).
///
/// Session-owned instead of process-global: a replacement or concurrent
/// session must never inherit, mask, or inflate another session's unreported
/// losses, so each [`crate::tcp_attach::TcpAttachSession`] owns one instance.
#[derive(Default)]
pub(crate) struct TcpOutputDropAccounting {
    dropped: AtomicU64,
    last_notified: AtomicU64,
}

impl TcpOutputDropAccounting {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Cumulative count of this session's dropped `output` events
    /// (test instrumentation).
    #[cfg(test)]
    pub(crate) fn dropped_output_events(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

/// Warn on the first drop and every [`TCP_OUTPUT_DROP_WARN_INTERVAL`] drops
/// after, so an output flood cannot become unbounded log I/O.
const TCP_OUTPUT_DROP_WARN_INTERVAL: u64 = 64;

/// Bounded, non-blocking attempts for the single drop notice (mirrors the
/// #5149 outbound-queue notice policy; never loops unboundedly or sleeps).
const TCP_DROP_NOTICE_ATTEMPTS: u8 = 8;

/// Admit one event under the reviewed TCP-attach fan-in policy (#9521).
///
/// **`output` events** are the only high-frequency, loss-eligible events: they
/// use non-blocking `try_send`; on a full queue the event is dropped, counted,
/// warned at a bounded rate, and one bounded user-visible notice is attempted.
///
/// **State and lifecycle events** (`stopped`, `continued`, `terminated`,
/// `error`) are non-lossy: they apply the same backpressure policy as the
/// #5149 outbound queue, so they are always admitted while the forwarding side
/// lives and can never be silently discarded behind output pressure. Waiting
/// happens without any lock held on the queue, as a bounded-rate retry that
/// re-checks retirement each attempt, so a disconnect or replacement retires
/// the producer instead of parking it on a stale session; receiver loss is
/// observed within [`ADMISSION_RETIRE_CHECK`] as `Disconnected`.
///
/// Retirement check and enqueue are serialized under the session's
/// [`ReaderRetirement`] gate, so an event admitted before `disconnect`
/// returned is pre-disconnect, and any attempt after it is refused — the
/// check-to-send window is closed exactly, not probabilistically.
pub(crate) struct ReaderRetirement {
    epoch: Arc<AtomicU64>,
    gate: Mutex<()>,
}

impl ReaderRetirement {
    pub(crate) fn new() -> Self {
        Self { epoch: Arc::new(AtomicU64::new(0)), gate: Mutex::new(()) }
    }

    /// The reader id a newly spawned reader captures.
    pub(crate) fn current_reader_id(&self) -> u64 {
        self.epoch.load(Ordering::SeqCst)
    }

    /// Retire every reader at or below the captured ids: the epoch bump is
    /// serialized against admission attempts under the gate, so when this
    /// returns, no in-flight admission for a retired reader can still be
    /// inside its check-to-send window.
    pub(crate) fn retire(&self) {
        let _gate = self.gate.lock().unwrap_or_else(|error| error.into_inner());
        self.epoch.fetch_add(1, Ordering::SeqCst);
    }

    /// Epoch handle for the synchronized connected-state update
    /// ([`mark_disconnected_if_current`]).
    pub(crate) fn epoch(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.epoch)
    }

    fn lock_gate(&self) -> MutexGuard<'_, ()> {
        self.gate.lock().unwrap_or_else(|error| error.into_inner())
    }

    fn is_retired(&self, reader_id: u64) -> bool {
        self.epoch.load(Ordering::SeqCst) != reader_id
    }
}

/// Admit one event for reader `reader_id` under the session's retirement
/// contract: retirement governs both admission families before any enqueue —
/// a retired producer must not put buffered frames into a queue a replacement
/// connection reuses, not even loss-eligible ones; the drop-on-full policy
/// governs pressure, not staleness (#9521 review).
/// [`admit_event_until`] for a live (never-retired) reader: the plain
/// admission entry point (test instrumentation).
#[cfg(test)]
pub(crate) fn admit_event(
    sender: &SyncSender<DapEvent>,
    event: DapEvent,
    accounting: &TcpOutputDropAccounting,
) -> EventAdmission {
    let retirement = ReaderRetirement::new();
    let reader_id = retirement.current_reader_id();
    admit_event_until(sender, event, accounting, &retirement, reader_id)
}

/// Admit one event for reader `reader_id` under the session's retirement
/// contract: retirement governs both admission families before any enqueue —
/// a retired producer must not put buffered frames into a queue a replacement
/// connection reuses, not even loss-eligible ones; the drop-on-full policy
/// governs pressure, not staleness (#9521 review).
pub(crate) fn admit_event_until(
    sender: &SyncSender<DapEvent>,
    mut event: DapEvent,
    accounting: &TcpOutputDropAccounting,
    retirement: &ReaderRetirement,
    reader_id: u64,
) -> EventAdmission {
    if matches!(event, DapEvent::Output { .. }) {
        let gate = retirement.lock_gate();
        if retirement.is_retired(reader_id) {
            return EventAdmission::Retired;
        }
        match sender.try_send(event) {
            Ok(()) => EventAdmission::Accepted,
            Err(TrySendError::Full(_)) => {
                drop(gate);
                let dropped_total = accounting.dropped.fetch_add(1, Ordering::Relaxed) + 1;
                if dropped_total == 1 || dropped_total.is_multiple_of(TCP_OUTPUT_DROP_WARN_INTERVAL)
                {
                    tracing::warn!(
                        dropped = dropped_total,
                        "TCP-attach fan-in queue full; dropping debugger output events (#9521)"
                    );
                }
                try_emit_drop_notice(sender, accounting, dropped_total);
                EventAdmission::DroppedOutput
            }
            Err(TrySendError::Disconnected(_)) => EventAdmission::Disconnected,
        }
    } else {
        loop {
            {
                // Check-to-send is serialized under the gate: a disconnect's
                // epoch bump can never land between this validation and the
                // non-blocking enqueue.
                let _gate = retirement.lock_gate();
                if retirement.is_retired(reader_id) {
                    return EventAdmission::Retired;
                }
                match sender.try_send(event) {
                    Ok(()) => return EventAdmission::Accepted,
                    Err(TrySendError::Full(returned)) => event = returned,
                    Err(TrySendError::Disconnected(_)) => return EventAdmission::Disconnected,
                }
            }
            // Park OUTSIDE the gate: a disconnect waiting to bump the epoch is
            // never delayed by more than one non-blocking try_send.
            thread::sleep(ADMISSION_RETIRE_CHECK);
        }
    }
}

/// Best-effort single bounded notice that debugger output was dropped, so the
/// loss is visible instead of silently rewriting the session transcript.
///
/// Anti-flood properties (mirroring the #5149 outbound notice): `try_send`
/// only, a fixed attempt bound with cooperative yields, no recursion into the
/// drop-counting path, and rate-limited by the session's
/// [`TcpOutputDropAccounting::last_notified`] so a sustained flood with no
/// queue room produces no notice per dropped line.
fn try_emit_drop_notice(
    sender: &SyncSender<DapEvent>,
    accounting: &TcpOutputDropAccounting,
    dropped_total: u64,
) {
    let last_notified = accounting.last_notified.load(Ordering::Relaxed);
    if dropped_total <= last_notified {
        return;
    }
    let newly_dropped = dropped_total - last_notified;
    let mut event = DapEvent::Output {
        category: "console".to_string(),
        output: format!(
            "[perl-lsp] {newly_dropped} debugger output event(s) dropped under TCP-attach \
             backpressure\n"
        ),
    };
    for attempt in 0..TCP_DROP_NOTICE_ATTEMPTS {
        // Move the event into `try_send` and reclaim it from `Full`, so a
        // saturated queue costs no per-attempt clone.
        match sender.try_send(event) {
            Ok(()) => {
                accounting.last_notified.store(dropped_total, Ordering::Relaxed);
                return;
            }
            Err(TrySendError::Disconnected(_)) => return,
            Err(TrySendError::Full(returned)) => {
                event = returned;
                if attempt + 1 < TCP_DROP_NOTICE_ATTEMPTS {
                    std::thread::yield_now();
                }
            }
        }
    }
    // Queue stayed full for every attempt: skip silently. No retries beyond the
    // fixed bound, no counter consumption, no recursion.
}

pub(crate) fn spawn_reader(
    stream: TcpStream,
    connected: Arc<Mutex<bool>>,
    event_sender: Option<SyncSender<DapEvent>>,
    accounting: Arc<TcpOutputDropAccounting>,
    retirement: Arc<ReaderRetirement>,
) {
    let reader_id = retirement.current_reader_id();
    thread::spawn(move || {
        run_reader(stream, connected, event_sender, accounting, retirement, reader_id)
    });
}

fn run_reader(
    stream: TcpStream,
    connected: Arc<Mutex<bool>>,
    event_sender: Option<SyncSender<DapEvent>>,
    accounting: Arc<TcpOutputDropAccounting>,
    retirement: Arc<ReaderRetirement>,
    reader_id: u64,
) {
    // Retirement: the session retires under the admission gate on every
    // disconnect, so a reader parked in cancellation-aware admission for a
    // stale connection retires instead of delivering stale events or
    // clobbering the shared connection state of a replacement connection
    // (#9521).
    let mut reader = BufReader::new(stream);
    let mut framer = ContentLengthFramer::new();
    let mut read_buf = [0u8; 8 * 1024];

    loop {
        let bytes_read = match reader.read(&mut read_buf) {
            Ok(0) => {
                mark_disconnected_if_current(&connected, &retirement.epoch(), reader_id);
                send_event(
                    &event_sender,
                    &accounting,
                    retirement.as_ref(),
                    reader_id,
                    DapEvent::Terminated { reason: "connection_closed".to_string() },
                );
                tracing::debug!("TCP connection closed by debugger");
                return;
            }
            Ok(n) => n,
            Err(error) => {
                mark_disconnected_if_current(&connected, &retirement.epoch(), reader_id);
                send_event(
                    &event_sender,
                    &accounting,
                    retirement.as_ref(),
                    reader_id,
                    DapEvent::Error { message: format!("TCP read error: {}", error) },
                );
                tracing::error!(%error, "Error reading from TCP");
                return;
            }
        };

        framer.push(&read_buf[..bytes_read]);
        if !drain_frames(&mut framer, &event_sender, &accounting, retirement.as_ref(), reader_id) {
            // Forwarding side gone, or the session retired this reader: stop
            // reading. Session teardown observes the disconnect through
            // `connected`/channel loss (#9521). The synchronized mark keeps a
            // retired reader from clearing a replacement connection's state.
            mark_disconnected_if_current(&connected, &retirement.epoch(), reader_id);
            return;
        }
    }
}

/// Drain all complete frames into the bounded fan-in queue.
///
/// Returns `false` when the forwarding side is gone or the session retired
/// this reader, and the reader must stop.
fn drain_frames(
    framer: &mut ContentLengthFramer,
    event_sender: &Option<SyncSender<DapEvent>>,
    accounting: &TcpOutputDropAccounting,
    retirement: &ReaderRetirement,
    reader_id: u64,
) -> bool {
    loop {
        let buffer = match framer.try_next() {
            Ok(Some(buffer)) => buffer,
            Ok(None) => return true,
            Err(error) => {
                tracing::warn!(%error, "Failed to parse TCP DAP frame");
                continue;
            }
        };

        trace_frame(&buffer);
        if !emit_frame_event(&buffer, event_sender, accounting, retirement, reader_id) {
            return false;
        }
    }
}

fn trace_frame(buffer: &[u8]) {
    if let Ok(text) = std::str::from_utf8(buffer) {
        tracing::trace!(output = %text, "Received from debugger");
    } else {
        tracing::warn!(bytes = buffer.len(), "Received non-UTF8 message from debugger");
    }
}

/// Parse one framed debugger message into a [`DapEvent`] and admit it.
///
/// Returns `false` when the forwarding side is gone or the reader was retired.
fn emit_frame_event(
    buffer: &[u8],
    event_sender: &Option<SyncSender<DapEvent>>,
    accounting: &TcpOutputDropAccounting,
    retirement: &ReaderRetirement,
    reader_id: u64,
) -> bool {
    let Some(sender) = event_sender else {
        return true;
    };
    let Ok(value) = serde_json::from_slice::<Value>(buffer) else {
        return true;
    };
    let Some(event) = dap_event_from_value(&value) else {
        return true;
    };

    match admit_event_until(sender, event, accounting, retirement, reader_id) {
        EventAdmission::Retired => false,
        other => other != EventAdmission::Disconnected,
    }
}

/// Admit a reader-generated lifecycle event (`terminated` on EOF, `error` on a
/// read failure). Non-lossy with cancellation-aware backpressure per the
/// #9521 policy; a retired reader delivers nothing.
fn send_event(
    event_sender: &Option<SyncSender<DapEvent>>,
    accounting: &TcpOutputDropAccounting,
    retirement: &ReaderRetirement,
    reader_id: u64,
    event: DapEvent,
) {
    if let Some(sender) = event_sender {
        // Lifecycle events are never `output`, so the accounting is untouched
        // on this path.
        let _ = admit_event_until(sender, event, accounting, retirement, reader_id);
    }
}

/// Epoch validation and the connected-state update as one synchronized
/// operation: the check happens under the connection lock immediately before
/// the write, so a disconnect+reconnect that completes after a retired
/// reader's epoch check can no longer be clobbered by that reader's late
/// `connected = false` (#9521 review).
fn mark_disconnected_if_current(
    connected: &Arc<Mutex<bool>>,
    reader_epoch: &Arc<AtomicU64>,
    reader_id: u64,
) {
    let mut guard = connected.lock().unwrap_or_else(|error| error.into_inner());
    if reader_epoch.load(Ordering::SeqCst) == reader_id {
        *guard = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_test_must::must_with;
    use std::sync::mpsc::sync_channel;
    use std::thread;
    use std::time::Duration;

    /// Filling the bounded fan-in with output events against a stalled consumer
    /// drops (counts) instead of blocking or growing without bound (#9521).
    #[test]
    fn tcp_output_flood_drops_and_counts_without_blocking() -> Result<(), String> {
        let (tx, _rx) = sync_channel::<DapEvent>(2);
        let accounting = TcpOutputDropAccounting::new();
        let output = |i: usize| DapEvent::Output {
            category: "stdout".to_string(),
            output: format!("line {i}\n"),
        };

        assert_eq!(admit_event(&tx, output(0), &accounting), EventAdmission::Accepted);
        assert_eq!(admit_event(&tx, output(1), &accounting), EventAdmission::Accepted);
        // Queue is now full: further output must be shed immediately.
        for i in 2..40 {
            assert_eq!(
                admit_event(&tx, output(i), &accounting),
                EventAdmission::DroppedOutput,
                "output on a full queue must be shed, not block or disconnect"
            );
        }
        assert!(
            accounting.dropped_output_events() >= 38,
            "dropped-output counter must advance when the fan-in queue saturates"
        );
        Ok(())
    }

    /// Drop accounting is per session: one session's unreported losses never
    /// inflate or alias another session's accounting (#9521 review).
    #[test]
    fn tcp_drop_accounting_is_isolated_per_session() -> Result<(), String> {
        let output = |i: usize| DapEvent::Output {
            category: "stdout".to_string(),
            output: format!("line {i}\n"),
        };

        // Session A accumulates 70 drops on a never-drained queue.
        let (tx_a, _rx_a) = sync_channel::<DapEvent>(1);
        let accounting_a = TcpOutputDropAccounting::new();
        assert_eq!(admit_event(&tx_a, output(0), &accounting_a), EventAdmission::Accepted);
        for i in 1..=70 {
            assert_eq!(admit_event(&tx_a, output(i), &accounting_a), EventAdmission::DroppedOutput);
        }
        assert_eq!(accounting_a.dropped_output_events(), 70);

        let (tx_b, _rx_b) = sync_channel::<DapEvent>(1);
        // Session B starts from zero: its first drop is its own first drop,
        // not session A's accumulated loss.
        let accounting_b = TcpOutputDropAccounting::new();
        assert_eq!(admit_event(&tx_b, output(0), &accounting_b), EventAdmission::Accepted);
        assert_eq!(admit_event(&tx_b, output(1), &accounting_b), EventAdmission::DroppedOutput);
        assert_eq!(
            accounting_b.dropped_output_events(),
            1,
            "a fresh session must count only its own drops"
        );
        Ok(())
    }

    /// State events are non-lossy: a `stopped` event blocks (backpressure) and
    /// is delivered once the consumer drains, never silently discarded (#9521).
    #[test]
    fn tcp_state_event_blocks_until_drain_then_is_delivered() -> Result<(), String> {
        let (tx, rx) = sync_channel::<DapEvent>(1);
        let accounting = TcpOutputDropAccounting::new();
        let output = DapEvent::Output { category: "stdout".to_string(), output: "fill\n".into() };
        assert_eq!(admit_event(&tx, output, &accounting), EventAdmission::Accepted);

        let tx2 = tx.clone();
        let accounting2 = TcpOutputDropAccounting::new();
        let handle = thread::spawn(move || {
            admit_event(
                &tx2,
                DapEvent::Stopped { reason: "breakpoint".into(), thread_id: 3 },
                &accounting2,
            )
        });

        thread::sleep(Duration::from_millis(20));
        let first = rx
            .recv_timeout(Duration::from_millis(500))
            .map_err(|e| format!("queued output must be drainable: {e}"))?;
        match first {
            DapEvent::Output { output, .. } => assert_eq!(output, "fill\n"),
            other => return Err(format!("expected the queued output event, got {other:?}")),
        }

        let stopped = rx
            .recv_timeout(Duration::from_secs(1))
            .map_err(|e| format!("blocked state event must arrive after drain: {e}"))?;
        match stopped {
            DapEvent::Stopped { reason, thread_id } => {
                assert_eq!((reason.as_str(), thread_id), ("breakpoint", 3));
            }
            other => return Err(format!("expected the stopped event, got {other:?}")),
        }
        assert_eq!(
            handle.join().map_err(|_| "state-event thread panicked".to_string())?,
            EventAdmission::Accepted,
            "the state event must be admitted once the queue drains"
        );
        Ok(())
    }

    /// A permanently-full queue must not produce one drop notice per dropped
    /// line: the notice attempt is bounded and rate-limited (#9521 falsifier).
    #[test]
    fn tcp_drop_notice_flood_does_not_produce_one_per_line() -> Result<(), String> {
        let (tx, rx) = sync_channel::<DapEvent>(1);
        let accounting = TcpOutputDropAccounting::new();
        let output = |i: usize| DapEvent::Output {
            category: "stdout".to_string(),
            output: format!("flood {i}\n"),
        };

        assert_eq!(admit_event(&tx, output(0), &accounting), EventAdmission::Accepted);
        let total = 200;
        let mut dropped = 0usize;
        for i in 1..total {
            if admit_event(&tx, output(i), &accounting) == EventAdmission::DroppedOutput {
                dropped += 1;
            }
        }
        if dropped != total - 1 {
            return Err(format!("every event after the first must be dropped; dropped={dropped}"));
        }

        // The never-drained queue can hold only the first event: zero notices.
        let mut drained = 0usize;
        let mut notices = 0usize;
        while let Ok(event) = rx.try_recv() {
            drained += 1;
            if let DapEvent::Output { output, .. } = event
                && output.contains("dropped under TCP-attach backpressure")
            {
                notices += 1;
            }
        }
        if drained != 1 {
            return Err(format!("a capacity-1 never-drained queue holds one event; got {drained}"));
        }
        if notices != 0 {
            return Err(format!(
                "a permanently-full queue must produce zero notices, not one per dropped line \
                 ({dropped} drops); notices={notices}"
            ));
        }
        Ok(())
    }

    /// Receiver loss wakes the producer immediately: a state event send fails
    /// as `Disconnected` instead of parking the reader forever (#9521).
    #[test]
    fn tcp_receiver_loss_wakes_state_event_producer() {
        let (tx, rx) = sync_channel::<DapEvent>(1);
        drop(rx);
        let accounting = TcpOutputDropAccounting::new();
        let result = admit_event(&tx, DapEvent::Terminated { reason: "gone".into() }, &accounting);
        assert_eq!(result, EventAdmission::Disconnected);
    }

    /// A retired producer stops without delivering its stale state event:
    /// cancellation-aware admission returns `Retired` while the queue stays
    /// full, instead of waiting for a drain that belongs to a replacement
    /// session (#9521 review).
    #[test]
    fn tcp_retired_producer_stops_without_delivering() -> Result<(), String> {
        let (tx, rx) = sync_channel::<DapEvent>(1);
        let accounting = TcpOutputDropAccounting::new();
        let output = DapEvent::Output { category: "stdout".to_string(), output: "fill\n".into() };
        assert_eq!(admit_event(&tx, output, &accounting), EventAdmission::Accepted);

        let tx2 = tx.clone();
        let handle = thread::spawn(move || {
            let accounting2 = TcpOutputDropAccounting::new();
            let retirement = ReaderRetirement::new();
            let reader_id = retirement.current_reader_id();
            // Retire the producer before the admission attempt: the gate
            // serializes the epoch bump against the check-to-send window, so
            // the stale state event is refused, not enqueued.
            retirement.retire();
            admit_event_until(
                &tx2,
                DapEvent::Stopped { reason: "stale".into(), thread_id: 7 },
                &accounting2,
                &retirement,
                reader_id,
            )
        });

        let admission = handle.join().map_err(|_| "producer thread panicked".to_string())?;
        assert_eq!(admission, EventAdmission::Retired);

        // The stale state event must not be in the queue: the only event is
        // the output that filled it.
        let drained = rx
            .recv_timeout(Duration::from_millis(200))
            .map_err(|e| format!("queued output must remain drainable: {e}"))?;
        match drained {
            DapEvent::Output { output, .. } => assert_eq!(output, "fill\n"),
            other => return Err(format!("expected only the queued output, got {other:?}")),
        }
        assert!(rx.try_recv().is_err(), "a retired producer must not deliver its stale event");
        Ok(())
    }

    /// Epoch validation and the connected-state write are one synchronized
    /// operation: a stale reader id cannot clear the flag; a current one can
    /// (#9521 review).
    #[test]
    fn mark_disconnected_is_epoch_conditional() {
        let connected = Arc::new(Mutex::new(true));
        let epoch = Arc::new(AtomicU64::new(1));

        mark_disconnected_if_current(&connected, &epoch, 0);
        assert!(
            *connected.lock().unwrap_or_else(|error| error.into_inner()),
            "a stale reader id must not clear the connection state"
        );

        mark_disconnected_if_current(&connected, &epoch, 1);
        assert!(
            !*connected.lock().unwrap_or_else(|error| error.into_inner()),
            "the current reader id marks the connection disconnected"
        );
    }

    /// The retirement gate serializes the epoch bump against admission: a
    /// `retire()` call waits for an in-flight admission window instead of
    /// landing inside its check-to-send gap, which is what makes the
    /// retirement/admission exclusion exact (#9521 review).
    #[test]
    fn retire_waits_for_an_in_flight_admission_window() {
        let retirement = Arc::new(ReaderRetirement::new());
        let gate_guard = retirement.lock_gate();

        let handle = {
            let retirement = Arc::clone(&retirement);
            thread::spawn(move || {
                retirement.retire();
            })
        };

        thread::sleep(Duration::from_millis(80));
        assert!(
            !handle.is_finished(),
            "retire() must wait for the in-flight admission window to close"
        );

        drop(gate_guard);
        must_with(
            handle.join().map_err(|_| "retire thread panicked"),
            "retire thread must finish once the gate opens",
        );
    }
}
