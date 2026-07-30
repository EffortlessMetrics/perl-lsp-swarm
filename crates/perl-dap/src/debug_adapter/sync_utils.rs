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

/// The [`OUTPUT_DROP_COUNTER`] value as of the last successfully-emitted drop-notice
/// event. Used to rate-limit the synthetic notice: we only attempt to emit a new one
/// once more drops have accumulated since the last one that actually made it onto the
/// wire.
static LAST_NOTIFIED_DROP_COUNT: AtomicU64 = AtomicU64::new(0);

/// Warn on the first drop and every [`OUTPUT_DROP_WARN_INTERVAL`] thereafter so a
/// chatty debuggee cannot turn bounded-queue drops into unbounded log I/O.
const OUTPUT_DROP_WARN_INTERVAL: u64 = 64;

/// Returns the current count of `output` events dropped due to a full outbound queue.
pub fn output_drop_count() -> u64 {
    OUTPUT_DROP_COUNTER.load(Ordering::Relaxed)
}

fn should_warn_on_drop(count: u64) -> bool {
    count == 1 || count.is_multiple_of(OUTPUT_DROP_WARN_INTERVAL)
}

/// Dispatch a DAP event through the bounded outbound channel.
///
/// Policy:
/// - `output` events use `try_send`. On `Full` the event is dropped, the global
///   [`OUTPUT_DROP_COUNTER`] is incremented, and a rate-limited warning is emitted.
///   A best-effort synthetic `output` notice is also attempted (see
///   [`try_emit_drop_notice`]) so the drop is user-visible in the debug console.
/// - All other events (lifecycle: `stopped`, `terminated`, `continued`, `initialized`, …) use
///   blocking `send`. These are rare and must be delivered; the call applies backpressure to
///   the producer until the writer thread drains a slot.
///
/// The `seq` guard is held for the *entire* dispatch, including the `try_send`/`send` call,
/// so that seq-assignment and enqueue are atomic: two threads racing to dispatch events can
/// never have the later-assigned `seq` overtake the earlier one in the outbound channel.
///
/// For paths that must not block on a stalled client (e.g. optional cleanup helpers), prefer
/// [`dispatch_event_nonblocking`] so cleanup cannot hang behind a full queue.
pub fn dispatch_event(
    sender: &SyncSender<DapMessage>,
    seq: &Mutex<i64>,
    event: &str,
    body: Option<Value>,
) -> EventDispatchResult {
    let mut seq_lock = lock_or_recover(seq, "dispatch_event.seq");
    *seq_lock += 1;
    let msg = DapMessage::Event { seq: *seq_lock, event: event.to_string(), body };

    if event == "output" {
        match sender.try_send(msg) {
            Ok(()) => EventDispatchResult::Sent,
            Err(TrySendError::Full(_)) => {
                let dropped_total = OUTPUT_DROP_COUNTER.fetch_add(1, Ordering::Relaxed) + 1;
                if should_warn_on_drop(dropped_total) {
                    tracing::warn!(
                        dropped = dropped_total,
                        "DAP outbound queue full; dropping output events"
                    );
                }
                // Deliberately scoped to *this* channel's own full-queue moment: only a
                // call that has already independently proven this exact channel full for a
                // real event may attempt the notice. That keeps the (process-wide) drop
                // counters from ever letting a notice intended for one channel land on an
                // unrelated, otherwise-idle channel (relevant when multiple adapters run in
                // one process, e.g. in tests).
                try_emit_drop_notice(sender, &mut seq_lock, dropped_total);
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

/// Best-effort emission of a synthetic `output` event telling the user that output lines
/// were dropped, so the drop is visible in the debug console rather than only in the
/// adapter's own log.
///
/// Safety / anti-flood properties:
/// - Uses `try_send` only, in a small bounded number of attempts — never blocks
///   indefinitely, and can never deadlock against the writer thread (which never acquires
///   `seq`; see `transport.rs`'s event-handler loop).
/// - Never calls back into [`dispatch_event`] or increments [`OUTPUT_DROP_COUNTER`], so it
///   cannot recurse even if the notice itself would also need to be dropped.
/// - Rate-limited on two axes: it only *attempts* a send when the drop count has grown since
///   the last successfully-emitted notice (`dropped_total > LAST_NOTIFIED_DROP_COUNT`), and it
///   only *succeeds* when the queue has room for one of a handful of `try_send` attempts
///   (each separated by a cooperative [`std::thread::yield_now`], never a sleep or blocking
///   wait). A sustained flood where the queue stays full therefore produces zero notices
///   until the client catches up enough to free a slot — never one notice per dropped line.
/// - Called while the caller already holds `seq_lock`; reuses that guard instead of
///   re-acquiring the mutex.
fn try_emit_drop_notice(
    sender: &SyncSender<DapMessage>,
    seq_lock: &mut MutexGuard<'_, i64>,
    dropped_total: u64,
) {
    let last_notified = LAST_NOTIFIED_DROP_COUNT.load(Ordering::Relaxed);
    if dropped_total <= last_notified {
        // Nothing new to report since the last notice that actually made it out.
        return;
    }
    let newly_dropped = dropped_total - last_notified;
    let body = Some(serde_json::json!({
        "category": "console",
        "output": format!(
            "[perl-lsp] {newly_dropped} output line(s) dropped due to slow debug client\n"
        ),
    }));
    let next_seq = **seq_lock + 1;

    // Bounded, non-blocking retries: gives a slow-but-not-permanently-stalled client's
    // writer thread a few scheduling slices to drain a slot, without ever looping
    // unboundedly or blocking. Fixed upper bound, no recursion.
    const MAX_ATTEMPTS: u8 = 8;
    for attempt in 0..MAX_ATTEMPTS {
        let msg =
            DapMessage::Event { seq: next_seq, event: "output".to_string(), body: body.clone() };
        match sender.try_send(msg) {
            Ok(()) => {
                **seq_lock = next_seq;
                LAST_NOTIFIED_DROP_COUNT.store(dropped_total, Ordering::Relaxed);
                return;
            }
            Err(TrySendError::Disconnected(_)) => return,
            Err(TrySendError::Full(_)) => {
                if attempt + 1 < MAX_ATTEMPTS {
                    std::thread::yield_now();
                }
            }
        }
    }
    // Queue stayed full for every attempt: skip silently. Do not retry beyond the fixed
    // bound above, do not consume a seq number, do not recurse into the drop-counting path.
}

/// Non-blocking dispatch for cleanup-critical events.
///
/// Uses `try_send` for every event kind. On `Full`, returns
/// [`EventDispatchResult::Dropped`] so the caller can finish process kill without
/// waiting for the DAP client to drain the queue. Holds `seq` for the entire
/// try_send so seq/enqueue stay atomic (same invariant as [`dispatch_event`]).
///
/// Only `output` drops increment [`OUTPUT_DROP_COUNTER`] and attempt
/// [`try_emit_drop_notice`] — non-output drops must not inflate the output-drop
/// accounting used for the user-visible console notice.
///
/// Hidden from the published facade: currently exercised by integration tests; not
/// yet wired into production cleanup callers (which still use [`dispatch_event`]).
#[doc(hidden)]
pub fn dispatch_event_nonblocking(
    sender: &SyncSender<DapMessage>,
    seq: &Mutex<i64>,
    event: &str,
    body: Option<Value>,
) -> EventDispatchResult {
    let mut seq_lock = lock_or_recover(seq, "dispatch_event_nonblocking.seq");
    *seq_lock += 1;
    let msg = DapMessage::Event { seq: *seq_lock, event: event.to_string(), body };
    match sender.try_send(msg) {
        Ok(()) => EventDispatchResult::Sent,
        Err(TrySendError::Full(_)) if event == "output" => {
            let dropped_total = OUTPUT_DROP_COUNTER.fetch_add(1, Ordering::Relaxed) + 1;
            if should_warn_on_drop(dropped_total) {
                tracing::warn!(
                    dropped = dropped_total,
                    "DAP outbound queue full; dropping output events (nonblocking path)"
                );
            }
            try_emit_drop_notice(sender, &mut seq_lock, dropped_total);
            EventDispatchResult::Dropped
        }
        Err(TrySendError::Full(_)) => {
            tracing::debug!(
                event,
                "DAP outbound queue full; dropping non-output event (nonblocking path)"
            );
            EventDispatchResult::Dropped
        }
        Err(TrySendError::Disconnected(_)) => EventDispatchResult::Disconnected,
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
