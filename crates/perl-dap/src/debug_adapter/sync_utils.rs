use super::DapMessage;
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError};
use std::sync::{Mutex, MutexGuard};

/// Counts dropped `output` events due to a full outbound queue.
static DROPPED_OUTPUT_EVENTS: AtomicU64 = AtomicU64::new(0);

/// Result of dispatching a DAP event to the bounded outbound channel.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum EventDispatchResult {
    /// Event was accepted into the queue.
    Sent,
    /// Event was dropped because the queue was full (`output` events only).
    Dropped,
    /// The channel is disconnected; the transport has gone away.
    Disconnected,
}

/// Poison-safe mutex lock that recovers from poisoned state.
pub(crate) fn lock_or_recover<'a, T>(mutex: &'a Mutex<T>, ctx: &'static str) -> MutexGuard<'a, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            tracing::warn!(ctx, "Poisoned mutex recovered");
            poisoned.into_inner()
        }
    }
}

/// `output` events are the only high-frequency events eligible for drop-on-full.
fn is_output_event(event: &str) -> bool {
    event == "output"
}

/// Dispatch a DAP event through the bounded outbound channel.
///
/// **`output` events**: non-blocking (`try_send`).  On `Full`, the event is
/// dropped and a `warn` log is emitted; the drop counter is incremented.
///
/// **All other events** (lifecycle / control: `stopped`, `terminated`,
/// `initialized`, `thread`, `breakpoint`, `continued`, `exited`, `process`, …):
/// blocking (`send`), applying backpressure to the producer until the writer
/// thread drains a slot.  On `SendError`, the channel is disconnected.
///
/// The sequence counter is incremented before the send regardless of whether the
/// event is eventually dropped, so sequence numbers remain monotonic.
pub(crate) fn dispatch_event(
    sender: &SyncSender<DapMessage>,
    seq: &Mutex<i64>,
    event: &str,
    body: Option<Value>,
) -> EventDispatchResult {
    let msg = {
        let mut seq_lock = lock_or_recover(seq, "dispatch_event.seq");
        *seq_lock += 1;
        DapMessage::Event { seq: *seq_lock, event: event.to_string(), body }
    };

    if is_output_event(event) {
        match sender.try_send(msg) {
            Ok(()) => EventDispatchResult::Sent,
            Err(TrySendError::Full(_)) => {
                DROPPED_OUTPUT_EVENTS.fetch_add(1, Ordering::Relaxed);
                tracing::warn!(event, "DAP outbound queue full; dropping output event");
                EventDispatchResult::Dropped
            }
            Err(TrySendError::Disconnected(_)) => EventDispatchResult::Disconnected,
        }
    } else {
        match sender.send(msg) {
            Ok(()) => EventDispatchResult::Sent,
            Err(_) => EventDispatchResult::Disconnected,
        }
    }
}

/// Send a DAP event through the bounded event channel with poison-safe sequence numbering.
///
/// Returns `true` when the event was either queued or shed due to a full queue
/// (both are normal outcomes); `false` only when the channel is disconnected
/// (transport gone).
pub(crate) fn emit_event_safe(
    sender: &SyncSender<DapMessage>,
    seq: &Mutex<i64>,
    event: &str,
    body: Option<Value>,
) -> bool {
    dispatch_event(sender, seq, event, body) != EventDispatchResult::Disconnected
}

/// Return the cumulative count of dropped `output` events (test instrumentation).
#[cfg(test)]
pub(crate) fn dropped_output_event_count() -> u64 {
    DROPPED_OUTPUT_EVENTS.load(Ordering::Relaxed)
}
