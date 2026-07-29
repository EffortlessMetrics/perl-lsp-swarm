use super::DapMessage;
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError};
use std::sync::{Mutex, MutexGuard};

/// Capacity of the bounded outbound DAP event queue. Matches LSP `OUTBOUND_CAPACITY`
/// and `EVENT_WRITE_BATCH_MAX` so the writer thread can always drain a full batch.
pub const EVENT_QUEUE_CAPACITY: usize = 64;

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

/// Result of a bounded event dispatch attempt.
#[derive(Debug, PartialEq, Eq)]
pub enum EventDispatchResult {
    /// Event was accepted into the queue.
    Sent,
    /// Output event was dropped because the queue was full.
    Dropped,
    /// The receiving end has disconnected; the session is over.
    Disconnected,
}

/// Monotonic count of `output` events dropped due to a full outbound queue.
static OUTPUT_DROP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Returns the current count of `output` events dropped due to a full outbound queue.
pub fn output_drop_count() -> u64 {
    OUTPUT_DROP_COUNTER.load(Ordering::Relaxed)
}

/// Dispatch a DAP event through the bounded outbound channel.
///
/// Policy:
/// - `output` events use `try_send`. On `Full` the event is dropped with a warning and the
///   global [`OUTPUT_DROP_COUNTER`] is incremented. This prevents a chatty debuggee from
///   growing the queue without bound when the client is slow.
/// - All other events (lifecycle: `stopped`, `terminated`, `continued`, `initialized`, …) use
///   blocking `send`. These are rare and must be delivered; the call applies backpressure to
///   the producer until the writer thread drains a slot.
pub fn dispatch_event(
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

    if event == "output" {
        match sender.try_send(msg) {
            Ok(()) => EventDispatchResult::Sent,
            Err(TrySendError::Full(_)) => {
                OUTPUT_DROP_COUNTER.fetch_add(1, Ordering::Relaxed);
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
/// Returns `true` if the event was sent or dropped (queue full — not a disconnect),
/// `false` only when the receiving end has disconnected.
pub fn emit_event_safe(
    sender: &SyncSender<DapMessage>,
    seq: &Mutex<i64>,
    event: &str,
    body: Option<Value>,
) -> bool {
    !matches!(dispatch_event(sender, seq, event, body), EventDispatchResult::Disconnected)
}
