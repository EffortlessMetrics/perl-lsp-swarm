use super::event::{DapEvent, dap_event_from_value};
use perl_lsp_rs_core::transport::framing::ContentLengthFramer;
use serde_json::Value;
use std::io::{BufReader, Read};
use std::net::TcpStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;

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
}

/// Counts dropped `output` events due to a full TCP-attach fan-in queue (#9521).
static DROPPED_TCP_OUTPUT_EVENTS: AtomicU64 = AtomicU64::new(0);

/// [`DROPPED_TCP_OUTPUT_EVENTS`] value at the last successfully queued drop
/// notice; a new notice is only attempted once more drops have accumulated.
static LAST_NOTIFIED_TCP_DROP_COUNT: AtomicU64 = AtomicU64::new(0);

/// Warn on the first drop and every [`TCP_OUTPUT_DROP_WARN_INTERVAL`] drops
/// after, so an output flood cannot become unbounded log I/O.
const TCP_OUTPUT_DROP_WARN_INTERVAL: u64 = 64;

/// Bounded, non-blocking attempts for the single drop notice (mirrors the
/// #5149 outbound-queue notice policy; never loops unboundedly or sleeps).
const TCP_DROP_NOTICE_ATTEMPTS: u8 = 8;

/// Cumulative count of TCP-attach fan-in `output` drops (test instrumentation).
#[cfg(test)]
pub(crate) fn dropped_tcp_output_event_count() -> u64 {
    DROPPED_TCP_OUTPUT_EVENTS.load(Ordering::Relaxed)
}

/// Admit one event under the reviewed TCP-attach fan-in policy (#9521).
///
/// **`output` events** are the only high-frequency, loss-eligible events: they
/// use non-blocking `try_send`; on a full queue the event is dropped, counted,
/// warned at a bounded rate, and one bounded user-visible notice is attempted.
///
/// **State and lifecycle events** (`stopped`, `continued`, `terminated`,
/// `error`) are non-lossy: they use the same blocking-send backpressure policy
/// as the #5149 outbound queue, so they are always admitted while the
/// forwarding side lives and can never be silently discarded behind output
/// pressure. Blocking happens without any lock held, and receiver loss wakes
/// the producer immediately as `Disconnected`.
pub(crate) fn admit_event(sender: &SyncSender<DapEvent>, event: DapEvent) -> EventAdmission {
    if matches!(event, DapEvent::Output { .. }) {
        match sender.try_send(event) {
            Ok(()) => EventAdmission::Accepted,
            Err(TrySendError::Full(_)) => {
                let dropped_total = DROPPED_TCP_OUTPUT_EVENTS.fetch_add(1, Ordering::Relaxed) + 1;
                if dropped_total == 1 || dropped_total.is_multiple_of(TCP_OUTPUT_DROP_WARN_INTERVAL)
                {
                    tracing::warn!(
                        dropped = dropped_total,
                        "TCP-attach fan-in queue full; dropping debugger output events (#9521)"
                    );
                }
                try_emit_drop_notice(sender, dropped_total);
                EventAdmission::DroppedOutput
            }
            Err(TrySendError::Disconnected(_)) => EventAdmission::Disconnected,
        }
    } else {
        match sender.send(event) {
            Ok(()) => EventAdmission::Accepted,
            Err(_) => EventAdmission::Disconnected,
        }
    }
}

/// Best-effort single bounded notice that debugger output was dropped, so the
/// loss is visible instead of silently rewriting the session transcript.
///
/// Anti-flood properties (mirroring the #5149 outbound notice): `try_send`
/// only, a fixed attempt bound with cooperative yields, no recursion into the
/// drop-counting path, and rate-limited by [`LAST_NOTIFIED_TCP_DROP_COUNT`] so
/// a sustained flood with no queue room produces no notice per dropped line.
fn try_emit_drop_notice(sender: &SyncSender<DapEvent>, dropped_total: u64) {
    let last_notified = LAST_NOTIFIED_TCP_DROP_COUNT.load(Ordering::Relaxed);
    if dropped_total <= last_notified {
        return;
    }
    let newly_dropped = dropped_total - last_notified;
    let event = DapEvent::Output {
        category: "console".to_string(),
        output: format!(
            "[perl-lsp] {newly_dropped} debugger output event(s) dropped under TCP-attach \
             backpressure\n"
        ),
    };
    for attempt in 0..TCP_DROP_NOTICE_ATTEMPTS {
        match sender.try_send(event.clone()) {
            Ok(()) => {
                LAST_NOTIFIED_TCP_DROP_COUNT.store(dropped_total, Ordering::Relaxed);
                return;
            }
            Err(TrySendError::Disconnected(_)) => return,
            Err(TrySendError::Full(_)) => {
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
) {
    thread::spawn(move || run_reader(stream, connected, event_sender));
}

fn run_reader(
    stream: TcpStream,
    connected: Arc<Mutex<bool>>,
    event_sender: Option<SyncSender<DapEvent>>,
) {
    let mut reader = BufReader::new(stream);
    let mut framer = ContentLengthFramer::new();
    let mut read_buf = [0u8; 8 * 1024];

    loop {
        let bytes_read = match reader.read(&mut read_buf) {
            Ok(0) => {
                mark_disconnected(&connected);
                send_event(
                    &event_sender,
                    DapEvent::Terminated { reason: "connection_closed".to_string() },
                );
                tracing::debug!("TCP connection closed by debugger");
                return;
            }
            Ok(n) => n,
            Err(error) => {
                mark_disconnected(&connected);
                send_event(
                    &event_sender,
                    DapEvent::Error { message: format!("TCP read error: {}", error) },
                );
                tracing::error!(%error, "Error reading from TCP");
                return;
            }
        };

        framer.push(&read_buf[..bytes_read]);
        if !drain_frames(&mut framer, &event_sender) {
            // Forwarding side gone: stop reading. Session teardown observes the
            // disconnect through `connected`/channel loss (#9521).
            mark_disconnected(&connected);
            return;
        }
    }
}

/// Drain all complete frames into the bounded fan-in queue.
///
/// Returns `false` when the forwarding side is gone and the reader must stop.
fn drain_frames(
    framer: &mut ContentLengthFramer,
    event_sender: &Option<SyncSender<DapEvent>>,
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
        if !emit_frame_event(&buffer, event_sender) {
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
/// Returns `false` when the forwarding side is gone.
fn emit_frame_event(buffer: &[u8], event_sender: &Option<SyncSender<DapEvent>>) -> bool {
    let Some(sender) = event_sender else {
        return true;
    };
    let Ok(value) = serde_json::from_slice::<Value>(buffer) else {
        return true;
    };
    let Some(event) = dap_event_from_value(&value) else {
        return true;
    };

    admit_event(sender, event) != EventAdmission::Disconnected
}

/// Admit a reader-generated lifecycle event (`terminated` on EOF, `error` on a
/// read failure). Non-lossy: blocking send per the #9521 policy.
fn send_event(event_sender: &Option<SyncSender<DapEvent>>, event: DapEvent) {
    if let Some(sender) = event_sender {
        let _ = sender.send(event);
    }
}

fn mark_disconnected(connected: &Arc<Mutex<bool>>) {
    *connected.lock().unwrap_or_else(|error| error.into_inner()) = false;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::sync_channel;
    use std::thread;
    use std::time::Duration;

    /// Filling the bounded fan-in with output events against a stalled consumer
    /// drops (counts) instead of blocking or growing without bound (#9521).
    #[test]
    fn tcp_output_flood_drops_and_counts_without_blocking() -> Result<(), String> {
        let (tx, _rx) = sync_channel::<DapEvent>(2);
        let output = |i: usize| DapEvent::Output {
            category: "stdout".to_string(),
            output: format!("line {i}\n"),
        };

        let before = dropped_tcp_output_event_count();
        assert_eq!(admit_event(&tx, output(0)), EventAdmission::Accepted);
        assert_eq!(admit_event(&tx, output(1)), EventAdmission::Accepted);
        // Queue is now full: further output must be shed immediately.
        for i in 2..40 {
            assert_eq!(
                admit_event(&tx, output(i)),
                EventAdmission::DroppedOutput,
                "output on a full queue must be shed, not block or disconnect"
            );
        }
        assert!(
            dropped_tcp_output_event_count() > before,
            "dropped-output counter must advance when the fan-in queue saturates"
        );
        Ok(())
    }

    /// State events are non-lossy: a `stopped` event blocks (backpressure) and
    /// is delivered once the consumer drains, never silently discarded (#9521).
    #[test]
    fn tcp_state_event_blocks_until_drain_then_is_delivered() -> Result<(), String> {
        let (tx, rx) = sync_channel::<DapEvent>(1);
        let output = DapEvent::Output { category: "stdout".to_string(), output: "fill\n".into() };
        assert_eq!(admit_event(&tx, output), EventAdmission::Accepted);

        let tx2 = tx.clone();
        let handle = thread::spawn(move || {
            admit_event(&tx2, DapEvent::Stopped { reason: "breakpoint".into(), thread_id: 3 })
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
        let output = |i: usize| DapEvent::Output {
            category: "stdout".to_string(),
            output: format!("flood {i}\n"),
        };

        assert_eq!(admit_event(&tx, output(0)), EventAdmission::Accepted);
        let total = 200;
        let mut dropped = 0usize;
        for i in 1..total {
            if admit_event(&tx, output(i)) == EventAdmission::DroppedOutput {
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
        let result = admit_event(&tx, DapEvent::Terminated { reason: "gone".into() });
        assert_eq!(result, EventAdmission::Disconnected);
    }
}
