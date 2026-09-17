use super::DapMessage;
use serde_json::Value;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

/// Outbound channel message: a DAP event paired with the [`DrainEpoch`]
/// under which it was reserved. The event-consumer thread reads the
/// epoch on consume and debits the matching per-epoch reservation so the
/// request-scoped wait drains only the events emitted by that request
/// (#15725).
pub(crate) type DapMessageWithEpoch = (DapMessage, DrainEpoch);

/// Counts dropped `output` events due to a full outbound queue.
static DROPPED_OUTPUT_EVENTS: AtomicU64 = AtomicU64::new(0);

/// The [`DROPPED_OUTPUT_EVENTS`] value as of the last successfully-emitted drop-notice
/// event. Used to rate-limit the synthetic notice: a new one is only attempted once more
/// drops have accumulated since the last one that actually made it onto the wire.
static LAST_NOTIFIED_DROP_COUNT: AtomicU64 = AtomicU64::new(0);

/// Warn on the first drop and every [`OUTPUT_DROP_WARN_INTERVAL`] thereafter so a chatty
/// debuggee cannot turn bounded-queue drops into unbounded log I/O (issue #5149 defect 3).
const OUTPUT_DROP_WARN_INTERVAL: u64 = 64;

/// Identity of a single DAP event for the purpose of the request-scoped
/// drain barrier (#15725).
///
/// Every accepted event is reserved against exactly one [`DrainEpoch`]
/// before it is pushed onto the outbound channel; the consumer debits
/// the same epoch when it writes the message. The transport response
/// wait only counts reservations against the epoch that captured this
/// request, so unrelated asynchronous `send_event` traffic emitted under
/// other epochs (background threads, the TCP-attach forwarder, synthetic
/// drop notices, or unrelated prior requests) does not push the response
/// wait up to the full `EVENT_DRAIN_MAX_WAIT` cap.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum DrainEpoch {
    /// Events emitted outside a request-handler scope (background threads,
    /// the TCP-attach forwarder, the synthetic drop notice, the reserved
    /// `terminated` event, tests that drive the latch directly). Reserved
    /// for backward compatibility and for the global-wait seam but not
    /// waited on by any single request's response path.
    Global,
    /// Events emitted by a specific in-flight request handler. The
    /// transport response wait drains only this epoch for that request's
    /// response.
    Request(u64),
}

impl DrainEpoch {
    /// Convenience constructor for [`DrainEpoch::Global`].
    pub(crate) const fn global() -> Self {
        Self::Global
    }
}

/// Thread-local request epoch bound by the transport worker thread
/// before calling into the request handler and cleared immediately after.
///
/// `dispatch_event` reads this cell to tag each accepted event with the
/// epoch under which the handler emitted it; the transport response wait
/// then drains only that epoch. Producers outside the worker thread
/// (test threads, background readers, the forwarder thread) see the
/// unset cell and fall through to [`DrainEpoch::Global`], so their
/// events still reserve against the latch for backward compatibility
/// but are not waited on by any single request's response path (#15725).
thread_local! {
    static DRAIN_EPOCH: RefCell<Option<u64>> = const { RefCell::new(None) };
}

/// Set the calling thread's request epoch (used by the transport worker
/// to mark each request handler invocation so its emitted events drain
/// against the response wait for that request). Pass `None` to restore
/// the unset state.
pub(super) fn set_drain_epoch(epoch: Option<u64>) {
    DRAIN_EPOCH.with(|cell| *cell.borrow_mut() = epoch);
}

/// Read the calling thread's currently bound request epoch, returning
/// [`DrainEpoch::Global`] if no request context is active.
pub(crate) fn current_drain_epoch() -> DrainEpoch {
    DRAIN_EPOCH.with(|cell| cell.borrow().map(DrainEpoch::Request)).unwrap_or(DrainEpoch::Global)
}

/// Bounded wait before a response write, used by the transport loop to
/// let the event-consumer thread drain events that a command handler
/// enqueued before the handler returned. Events accepted by
/// [`EventSender::send_event`] reserve the latch before publishing (a
/// refused or dropped dispatch rolls its reservation back); the consumer
/// completes the exact drained count after its receive loop. The
/// transport waits (capped) on the latch before writing a response, so a
/// client observes a command's events before the terminal response that
/// can imply their effect (review finding on #12745: queueing alone does
/// not order the wire).
///
/// Saturation semantics keep the latch fail-open: uncounted synthetic
/// messages (the drop notice) or lost decrements can only open the
/// barrier early, never hang it, and the bounded wait caps every
/// response's added latency even when the consumer is stalled on a
/// blocked wire.
///
/// # Request-scoped wait (#15725)
///
/// The pending map is keyed by [`DrainEpoch`] rather than a single
/// `usize`. The transport worker tags every event it emits inside a
/// request handler with the captured request epoch via
/// [`current_drain_epoch`]; the response wait then drains only the
/// entries for that epoch via [`EventDrainLatch::wait_for_epoch`].
/// Reservations against other epochs (background threads, the forwarder,
/// prior requests still draining, synthetic drop notices) do not
/// contribute to the per-request wait, so a busy debug session no longer
/// pays unrelated-event latency on every response.
#[derive(Clone, Default)]
pub(crate) struct EventDrainLatch {
    pending: std::sync::Arc<(Mutex<BTreeMap<DrainEpoch, usize>>, std::sync::Condvar)>,
}

impl EventDrainLatch {
    /// Record `count` messages accepted onto the outbound channel under
    /// [`DrainEpoch::Global`]. Kept for backward compatibility with code
    /// paths that emit outside a request-handler scope; the request
    /// handler response path does not wait on this epoch.
    pub(crate) fn enqueue(&self, count: usize) {
        self.enqueue_at(DrainEpoch::Global, count)
    }

    /// Record `count` messages accepted onto the outbound channel,
    /// reserved against `epoch` so the matching
    /// [`EventDrainLatch::wait_for_epoch`] call drains only this epoch.
    /// Used by [`dispatch_event`] when the producer's thread-local
    /// [`DRAIN_EPOCH`] is set (#15725).
    pub(crate) fn enqueue_at(&self, epoch: DrainEpoch, count: usize) {
        if count == 0 {
            return;
        }
        let (mutex, _) = &*self.pending;
        let mut map = lock_or_recover(mutex, "event_drain_latch.enqueue");
        let entry = map.entry(epoch).or_insert(0);
        *entry = entry.saturating_add(count);
    }

    /// Record that the consumer wrote `count` previously counted
    /// messages reserved against [`DrainEpoch::Global`].
    pub(crate) fn complete(&self, count: usize) {
        self.complete_at(DrainEpoch::Global, count)
    }

    /// Record that the consumer wrote `count` previously counted
    /// messages reserved against `epoch`. Removes the map entry when its
    /// remaining count reaches zero so the wait can short-circuit.
    pub(crate) fn complete_at(&self, epoch: DrainEpoch, count: usize) {
        let (mutex, condvar) = &*self.pending;
        let mut map = lock_or_recover(mutex, "event_drain_latch.complete");
        if let Some(entry) = map.get_mut(&epoch) {
            *entry = entry.saturating_sub(count);
            if *entry == 0 {
                map.remove(&epoch);
            }
        }
        condvar.notify_all();
    }

    /// Clear any residue for every epoch (for example from a previous
    /// transport run whose consumer terminated mid-batch).
    pub(crate) fn reset(&self) {
        let (mutex, condvar) = &*self.pending;
        *lock_or_recover(mutex, "event_drain_latch.reset") = BTreeMap::new();
        condvar.notify_all();
    }

    /// Wait until every counted message reserved against
    /// [`DrainEpoch::Global`] has been written, bounded by `cap`.
    /// Returns `true` when fully drained, `false` on timeout. Kept for
    /// the historical seam; the request handler response path uses
    /// [`EventDrainLatch::wait_for_epoch`] to drain only its own epoch.
    pub(crate) fn wait_until_drained(&self, cap: std::time::Duration) -> bool {
        self.wait_for_epoch(DrainEpoch::Global, cap)
    }

    /// Wait until every counted message reserved against `epoch` has
    /// been written, bounded by `cap`. Returns `true` when fully drained,
    /// `false` on timeout.
    ///
    /// The per-epoch wait isolates the response write from races against
    /// unrelated asynchronous `send_event` traffic on other epochs
    /// (#15725): reservations against other epochs (background threads,
    /// the forwarder, synthetic drop notices, prior requests still
    /// draining) do not contribute to this wait, so a busy debug session
    /// no longer pays unrelated-event latency on every response. The
    /// wait is still bounded and fail-open to preserve the saturation
    /// semantics of [`EventDrainLatch`].
    pub(crate) fn wait_for_epoch(&self, epoch: DrainEpoch, cap: std::time::Duration) -> bool {
        let start = std::time::Instant::now();
        let (mutex, condvar) = &*self.pending;
        let mut map = lock_or_recover(mutex, "event_drain_latch.wait");
        loop {
            let pending = map.get(&epoch).copied().unwrap_or(0);
            if pending == 0 {
                return true;
            }
            let elapsed = start.elapsed();
            if elapsed >= cap {
                return false;
            }
            let (guard, timed_out) = match condvar.wait_timeout(map, cap - elapsed) {
                Ok(pair) => pair,
                Err(poisoned) => poisoned.into_inner(),
            };
            map = guard;
            if timed_out.timed_out()
                && map.get(&epoch).copied().unwrap_or(0) > 0
                && start.elapsed() >= cap
            {
                return false;
            }
        }
    }
}

/// Shared event-sender admission gate. A producer clones the sender while the
/// gate is held, then performs its potentially blocking send after releasing
/// the gate. Closing therefore cannot wait on a producer, while an admitted
/// clone keeps the receiver connected until that send completes.
#[derive(Clone)]
pub(crate) struct EventSender(Arc<Mutex<Option<SyncSender<DapMessageWithEpoch>>>>);

impl EventSender {
    pub(crate) fn new(sender: SyncSender<DapMessageWithEpoch>) -> Self {
        Self(Arc::new(Mutex::new(Some(sender))))
    }

    pub(crate) fn close(&self) {
        *lock_or_recover(&self.0, "event_sender") = None;
    }

    pub(super) fn admitted_sender(&self) -> Option<SyncSender<DapMessageWithEpoch>> {
        lock_or_recover(&self.0, "event_sender").as_ref().cloned()
    }

    pub(crate) fn send_event(
        &self,
        seq: &Mutex<i64>,
        event: &str,
        body: Option<Value>,
    ) -> EventDispatchResult {
        let Some(sender) = self.admitted_sender() else {
            return EventDispatchResult::Disconnected;
        };
        dispatch_event(&sender, seq, event, body)
    }

    pub(crate) fn send_event_generation_guarded(
        &self,
        seq: &Mutex<i64>,
        event: &str,
        body: Option<Value>,
        stale: &dyn Fn() -> bool,
    ) -> GuardedDispatchResult {
        let Some(sender) = self.admitted_sender() else {
            return GuardedDispatchResult::Disconnected;
        };
        dispatch_event_generation_guarded(&sender, seq, event, body, stale)
    }
}

/// Result of a generation-guarded dispatch ([`dispatch_event_generation_guarded`]).
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum GuardedDispatchResult {
    /// Event was accepted into the queue.
    Sent,
    /// `output` event was dropped because the queue was full (lossy policy).
    Dropped,
    /// The channel is disconnected; the transport has gone away.
    Disconnected,
    /// The session generation was replaced while waiting for queue room: the
    /// stale event was discarded before publication (#9521).
    Stale,
}

/// Park interval between full-queue retries in the generation-guarded
/// dispatch: bounded-latency staleness detection without a busy spin (#9521).
pub(crate) const GENERATION_GUARD_PARK: Duration = Duration::from_millis(1);

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

/// Returns `true` if a drop-count of `count` should be logged: the first drop, then every
/// [`OUTPUT_DROP_WARN_INTERVAL`] thereafter, so a chatty debuggee cannot turn bounded-queue
/// drops into unbounded log I/O.
fn should_warn_on_drop(count: u64) -> bool {
    count == 1 || count.is_multiple_of(OUTPUT_DROP_WARN_INTERVAL)
}

/// Dispatch a DAP event through the bounded outbound channel.
///
/// **`output` events**: non-blocking (`try_send`).  On `Full`, the event is dropped, the
/// drop counter is incremented, a rate-limited `warn` log is emitted (at most once per
/// [`OUTPUT_DROP_WARN_INTERVAL`] drops), and a best-effort synthetic `output` notice is
/// attempted (see [`try_emit_drop_notice`]) so the drop is visible in the debug console.
///
/// **All other events** (lifecycle / control: `stopped`, `terminated`, `initialized`,
/// `thread`, `breakpoint`, `continued`, `exited`, `process`, …): blocking (`send`),
/// applying backpressure to the producer until the writer thread drains a slot. On
/// `SendError`, the channel is disconnected.
///
/// The `seq` guard is held for the *entire* dispatch, including the `try_send`/`send`
/// call, so that seq-assignment and enqueue are atomic: two threads racing to dispatch
/// events can never have the later-assigned `seq` overtake the earlier one in the
/// outbound channel.
///
/// Callers must not hold any other lock that the writer/consumer thread may need to
/// acquire while draining the channel (e.g. the transport's response-writer mutex) —
/// doing so can deadlock when this call blocks on a full queue. See `transport.rs`'s
/// `run_with_io` for the scoped-guard pattern that avoids this.
pub(crate) fn dispatch_event(
    sender: &SyncSender<DapMessageWithEpoch>,
    seq: &Mutex<i64>,
    event: &str,
    body: Option<Value>,
) -> EventDispatchResult {
    let (msg, mut seq_lock) = {
        let mut seq_lock = lock_or_recover(seq, "dispatch_event.seq");
        *seq_lock += 1;
        (DapMessage::Event { seq: *seq_lock, event: event.to_string(), body }, seq_lock)
    };

    let epoch = current_drain_epoch();

    if is_output_event(event) {
        match sender.try_send((msg, epoch)) {
            Ok(()) => EventDispatchResult::Sent,
            Err(TrySendError::Full(_)) => {
                let dropped_total = DROPPED_OUTPUT_EVENTS.fetch_add(1, Ordering::Relaxed) + 1;
                if should_warn_on_drop(dropped_total) {
                    tracing::warn!(
                        dropped = dropped_total,
                        "DAP outbound queue full; dropping output events"
                    );
                }
                try_emit_drop_notice(sender, &mut seq_lock, epoch, dropped_total);
                EventDispatchResult::Dropped
            }
            Err(TrySendError::Disconnected(_)) => EventDispatchResult::Disconnected,
        }
    } else {
        match sender.send((msg, epoch)) {
            Ok(()) => EventDispatchResult::Sent,
            Err(_) => EventDispatchResult::Disconnected,
        }
    }
}

/// [`dispatch_event`] with a staleness hook for the generation-aware TCP-attach
/// forwarder (#9521).
///
/// Identical admission policy, except a non-output event that cannot be
/// enqueued immediately waits as a bounded-rate retry (`try_send` +
/// [`GENERATION_GUARD_PARK`]) that re-checks `stale` before every commit
/// attempt. A replacement session therefore retires a blocked stale event
/// instead of an unbounded blocking send committing it into the replacement's
/// outbound stream after the pre-dispatch generation check already passed;
/// each commit is one non-blocking `try_send` immediately after its final
/// staleness check.
///
/// The seq guard is held across the whole dispatch (including retry parks),
/// preserving the seq-assignment/enqueue atomicity of [`dispatch_event`];
/// unlike an unbounded blocking send, a stale hook releases it promptly.
///
/// **`output` events** keep the lossy non-blocking policy — a full queue sheds
/// them, so they cannot park long enough for staleness to matter.
pub(crate) fn dispatch_event_generation_guarded(
    sender: &SyncSender<DapMessageWithEpoch>,
    seq: &Mutex<i64>,
    event: &str,
    body: Option<Value>,
    stale: &dyn Fn() -> bool,
) -> GuardedDispatchResult {
    let (mut msg, mut seq_lock) = {
        let mut seq_lock = lock_or_recover(seq, "dispatch_event.seq");
        *seq_lock += 1;
        (DapMessage::Event { seq: *seq_lock, event: event.to_string(), body }, seq_lock)
    };

    let epoch = current_drain_epoch();

    if is_output_event(event) {
        match sender.try_send((msg, epoch)) {
            Ok(()) => GuardedDispatchResult::Sent,
            Err(TrySendError::Full(_)) => {
                let dropped_total = DROPPED_OUTPUT_EVENTS.fetch_add(1, Ordering::Relaxed) + 1;
                if should_warn_on_drop(dropped_total) {
                    tracing::warn!(
                        dropped = dropped_total,
                        "DAP outbound queue full; dropping output events"
                    );
                }
                try_emit_drop_notice(sender, &mut seq_lock, epoch, dropped_total);
                GuardedDispatchResult::Dropped
            }
            Err(TrySendError::Disconnected(_)) => GuardedDispatchResult::Disconnected,
        }
    } else {
        loop {
            if stale() {
                return GuardedDispatchResult::Stale;
            }
            match sender.try_send((msg, epoch)) {
                Ok(()) => return GuardedDispatchResult::Sent,
                Err(TrySendError::Full(returned)) => {
                    msg = returned.0;
                    std::thread::sleep(GENERATION_GUARD_PARK);
                }
                Err(TrySendError::Disconnected(_)) => return GuardedDispatchResult::Disconnected,
            }
        }
    }
}

/// Best-effort emission of a synthetic `output` event telling the user that output lines
/// were dropped, so the drop is visible in the debug console rather than only in the
/// adapter's own log (issue #5149 defect 3).
///
/// Safety / anti-flood properties:
/// - Uses `try_send` only, in a small bounded number of attempts — never blocks
///   indefinitely.
/// - Never calls back into [`dispatch_event`] or increments [`DROPPED_OUTPUT_EVENTS`], so
///   it cannot recurse even if the notice itself would also need to be dropped.
/// - Rate-limited on two axes: it only *attempts* a send when the drop count has grown
///   since the last successfully-emitted notice, and it only *succeeds* when the queue has
///   room for one of a handful of `try_send` attempts (each separated by a cooperative
///   yield, never a sleep or blocking wait). A sustained flood where the queue stays full
///   therefore produces zero notices until the client catches up enough to free a slot —
///   never one notice per dropped line.
/// - Called while the caller already holds `seq_lock`; reuses that guard instead of
///   re-acquiring the mutex.
fn try_emit_drop_notice(
    sender: &SyncSender<DapMessageWithEpoch>,
    seq_lock: &mut MutexGuard<'_, i64>,
    epoch: DrainEpoch,
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
        match sender.try_send((msg, epoch)) {
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

/// Return the cumulative count of dropped `output` events (test instrumentation).
#[cfg(test)]
pub(crate) fn dropped_output_event_count() -> u64 {
    DROPPED_OUTPUT_EVENTS.load(Ordering::Relaxed)
}

/// Regression tests for the bounded outbound DAP event queue (issue #5149) and its
/// backpressure/ordering/notification behavior (PR #5318 defects 1-3). Kept in-crate
/// (rather than as an integration test) so `dispatch_event` et al. stay `pub(crate)` and
/// no `.ci/public-api-baselines/perl-dap.txt` update is required.
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Arc;
    use std::sync::mpsc::sync_channel;
    use std::thread;
    use std::time::{Duration, Instant};

    /// Returns `true` if `msg` is the synthetic drop-notice `output` event emitted by
    /// `dispatch_event` after output events have been dropped.
    fn is_drop_notice(msg: &DapMessage) -> bool {
        matches!(msg, DapMessage::Event { event, body, .. }
            if event == "output"
                && body
                    .as_ref()
                    .and_then(|b| b.get("output"))
                    .and_then(|o| o.as_str())
                    .is_some_and(|t| t.contains("dropped due to slow debug client")))
    }

    /// Per-epoch reservations are independent: `wait_for_epoch(R, …)` must
    /// not block on reservations made against a different epoch, and
    /// `complete_at(R, 1)` must not affect a different epoch's wait
    /// (#15725).
    #[test]
    fn event_drain_latch_isolates_per_epoch_waits() {
        let latch = EventDrainLatch::default();
        latch.enqueue_at(DrainEpoch::Global, 1);
        latch.enqueue_at(DrainEpoch::Request(7), 3);
        latch.enqueue_at(DrainEpoch::Request(9), 2);

        // Global epoch drains only when its reservation completes, even
        // though Request epochs have live reservations.
        assert!(!latch.wait_for_epoch(DrainEpoch::Global, Duration::from_millis(50)));
        latch.complete_at(DrainEpoch::Global, 1);
        assert!(latch.wait_for_epoch(DrainEpoch::Global, Duration::from_millis(50)));

        // Per-request epoch drains only when its own reservation completes;
        // unrelated-epoch traffic does not contribute.
        assert!(!latch.wait_for_epoch(DrainEpoch::Request(7), Duration::from_millis(50)));
        assert!(!latch.wait_for_epoch(DrainEpoch::Request(9), Duration::from_millis(50)));
        // Debiting the unrelated Request(9) does not advance Request(7).
        latch.complete_at(DrainEpoch::Request(9), 2);
        assert!(latch.wait_for_epoch(DrainEpoch::Request(9), Duration::from_millis(50)));
        assert!(!latch.wait_for_epoch(DrainEpoch::Request(7), Duration::from_millis(50)));
        // A second `complete_at` from the consumer thread on the same
        // request eventually drains it.
        latch.complete_at(DrainEpoch::Request(7), 3);
        assert!(latch.wait_for_epoch(DrainEpoch::Request(7), Duration::from_millis(50)));

        // Over-completing an epoch is a saturating no-op (fail-open).
        latch.complete_at(DrainEpoch::Request(7), 99);
        assert!(latch.wait_for_epoch(DrainEpoch::Request(7), Duration::from_millis(50)));
    }

    /// `wait_until_drained` retains its historical semantics over the
    /// global epoch (#15725).
    #[test]
    fn event_drain_latch_wait_until_drained_still_targets_global() {
        let latch = EventDrainLatch::default();
        latch.enqueue(1);
        assert!(!latch.wait_until_drained(Duration::from_millis(50)));
        latch.complete(1);
        assert!(latch.wait_until_drained(Duration::from_millis(50)));
    }

    /// `reset` clears every epoch's reservations so a fresh run cannot
    /// inherit a poisoned map from a prior run (#15725).
    #[test]
    fn event_drain_latch_reset_clears_every_epoch() {
        let latch = EventDrainLatch::default();
        latch.enqueue_at(DrainEpoch::Global, 1);
        latch.enqueue_at(DrainEpoch::Request(3), 1);
        latch.reset();
        assert!(latch.wait_for_epoch(DrainEpoch::Global, Duration::from_millis(50)));
        assert!(latch.wait_for_epoch(DrainEpoch::Request(3), Duration::from_millis(50)));
    }

    #[test]
    fn event_sender_closes_admission_after_flushing_existing_event() -> Result<(), String> {
        let (tx, rx) = sync_channel::<DapMessageWithEpoch>(2);
        let sender = EventSender::new(tx);
        let seq = Mutex::new(0i64);
        if sender.send_event(&seq, "output", Some(json!({"output": "before-close\n"})))
            != EventDispatchResult::Sent
        {
            return Err("pre-close event was not admitted".to_string());
        }
        sender.close();
        if sender.send_event(&seq, "output", Some(json!({"output": "after-close\n"})))
            != EventDispatchResult::Disconnected
        {
            return Err("late event was admitted after close".to_string());
        }
        match rx.recv().map_err(|error| error.to_string())? {
            (DapMessage::Event { event, .. }, _) if event == "output" => {}
            (other, _) => return Err(format!("unexpected flushed event: {other:?}")),
        }
        match rx.try_recv() {
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Ok(()),
            other => Err(format!("sender remained connected after close: {other:?}")),
        }
    }

    #[test]
    fn event_sender_flushes_admitted_lifecycle_event_before_close() -> Result<(), String> {
        let (tx, rx) = sync_channel::<DapMessageWithEpoch>(1);
        let sender = EventSender::new(tx);
        let seq = Arc::new(Mutex::new(0i64));
        if sender.send_event(&seq, "output", Some(json!({"output": "queued\n"})))
            != EventDispatchResult::Sent
        {
            return Err("queue-filling event was not admitted".to_string());
        }

        let producer_sender = sender.clone();
        let producer_seq = Arc::clone(&seq);
        let producer = thread::spawn(move || {
            producer_sender.send_event(&producer_seq, "stopped", Some(json!({"reason": "pause"})))
        });
        match rx.recv().map_err(|error| error.to_string())? {
            (DapMessage::Event { event, .. }, _) if event == "output" => {}
            (other, _) => return Err(format!("unexpected queued event: {other:?}")),
        }
        if producer.join().map_err(|_| "producer panicked".to_string())?
            != EventDispatchResult::Sent
        {
            return Err("admitted lifecycle event was not delivered".to_string());
        }

        sender.close();
        if sender.send_event(&seq, "stopped", Some(json!({"reason": "late"})))
            != EventDispatchResult::Disconnected
        {
            return Err("late lifecycle event was admitted after close".to_string());
        }
        match rx.recv().map_err(|error| error.to_string())? {
            (DapMessage::Event { event, .. }, _) if event == "stopped" => {}
            (other, _) => return Err(format!("expected flushed lifecycle event: {other:?}")),
        }
        drop(sender);
        match rx.try_recv() {
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Ok(()),
            other => Err(format!("event channel remained open: {other:?}")),
        }
    }

    #[test]
    fn event_sender_close_does_not_wait_on_admitted_send() -> Result<(), String> {
        let (tx, rx) = sync_channel::<DapMessageWithEpoch>(1);
        let sender = EventSender::new(tx);
        let seq = Mutex::new(0i64);
        if sender.send_event(&seq, "output", Some(json!({"output": "queued\n"})))
            != EventDispatchResult::Sent
        {
            return Err("queue-filling event was not admitted".to_string());
        }
        let seq = Arc::new(seq);
        let producer_seq = Arc::clone(&seq);
        let producer_sender = sender.clone();
        let producer = thread::spawn(move || {
            producer_sender.send_event(&producer_seq, "stopped", Some(json!({"reason": "pause"})))
        });

        let deadline = Instant::now() + Duration::from_secs(1);
        let producer_blocked = loop {
            match seq.try_lock() {
                Ok(_) => {
                    if Instant::now() >= deadline {
                        break false;
                    }
                    thread::yield_now();
                }
                Err(std::sync::TryLockError::WouldBlock) => break true,
                Err(std::sync::TryLockError::Poisoned(_)) => break false,
            }
        };
        let (close_done_tx, close_done_rx) = sync_channel(1);
        let close_sender = sender.clone();
        let closer = thread::spawn(move || {
            close_sender.close();
            let _ = close_done_tx.send(());
        });
        let close_completed_before_drain =
            close_done_rx.recv_timeout(Duration::from_millis(200)).is_ok();

        let queued = rx.recv().map_err(|error| error.to_string())?;
        if !matches!(&queued, (DapMessage::Event { event, .. }, _) if event == "output") {
            let _ = producer.join();
            let _ = closer.join();
            return Err(format!("unexpected queued event: {queued:?}"));
        }
        let producer_result =
            producer.join().map_err(|_| "admitted producer panicked".to_string())?;
        let _ = closer.join();
        let late_result = sender.send_event(&seq, "stopped", Some(json!({"reason": "late"})));
        if !producer_blocked || !close_completed_before_drain {
            return Err(format!(
                "close must complete before draining a blocked admitted send: blocked={producer_blocked}, close_completed={close_completed_before_drain}"
            ));
        }
        if late_result != EventDispatchResult::Disconnected {
            return Err("late lifecycle event was admitted after close".to_string());
        }
        if producer_result != EventDispatchResult::Sent {
            return Err("admitted lifecycle event was not delivered".to_string());
        }
        match rx.recv().map_err(|error| error.to_string())? {
            (DapMessage::Event { event, .. }, _) if event == "stopped" => {}
            (other, _) => return Err(format!("expected admitted lifecycle event: {other:?}")),
        }
        match rx.recv_timeout(Duration::from_millis(200)) {
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Ok(()),
            other => Err(format!("closed sender retained the channel after delivery: {other:?}")),
        }
    }

    /// `output` events that arrive on a full queue are dropped with `Dropped`,
    /// not queued or treated as a disconnect.
    #[test]
    fn output_drop_when_queue_full() -> Result<(), String> {
        let cap = 2;
        let (tx, _rx) = sync_channel::<DapMessageWithEpoch>(cap);
        let seq = Mutex::new(0i64);

        for i in 0..cap {
            let result =
                dispatch_event(&tx, &seq, "output", Some(json!({"output": format!("line {i}\n")})));
            if result != EventDispatchResult::Sent {
                return Err(format!("slot {i} should be accepted, got {result:?}"));
            }
        }

        let result = dispatch_event(&tx, &seq, "output", Some(json!({"output": "overflow\n"})));
        if result != EventDispatchResult::Dropped {
            return Err(format!(
                "output event on a full queue must be Dropped, not Sent or Disconnected; got {result:?}"
            ));
        }
        Ok(())
    }

    /// A `stopped` lifecycle event blocks until a slot is available, then is delivered.
    #[test]
    fn lifecycle_blocks_until_drain() -> Result<(), String> {
        let cap = 1;
        let (tx, rx) = sync_channel::<DapMessageWithEpoch>(cap);
        let seq = Arc::new(Mutex::new(0i64));

        let r = dispatch_event(&tx, &seq, "output", Some(json!({"output": "filling\n"})));
        if r != EventDispatchResult::Sent {
            return Err(format!("expected Sent when filling queue, got {r:?}"));
        }

        let tx2 = tx.clone();
        let seq2 = Arc::clone(&seq);
        let handle = thread::spawn(move || {
            dispatch_event(
                &tx2,
                &seq2,
                "stopped",
                Some(json!({"reason": "pause", "threadId": 1, "allThreadsStopped": true})),
            )
        });

        thread::sleep(Duration::from_millis(20));

        let drained = rx
            .recv_timeout(Duration::from_millis(200))
            .map_err(|e| format!("output event must be drainable: {e}"))?;
        if !matches!(&drained, DapMessage::Event { event, .. } if event == "output") {
            return Err(format!("expected output event, got: {drained:?}"));
        }

        let stopped = rx
            .recv_timeout(Duration::from_millis(500))
            .map_err(|e| format!("stopped event must arrive after queue drains: {e}"))?;
        if !matches!(&stopped, DapMessage::Event { event, .. } if event == "stopped") {
            return Err(format!("expected stopped event, got: {stopped:?}"));
        }

        let result = handle.join().map_err(|_| "dispatch thread panicked".to_string())?;
        if result != EventDispatchResult::Sent {
            return Err(format!("stopped event must be delivered (Sent), got {result:?}"));
        }
        Ok(())
    }

    /// Flooding the queue with many output events from a producer that outruns a slow
    /// consumer must not hang and must produce drops rather than unbounded growth.
    #[test]
    fn slow_consumer_output_flood_stays_bounded() -> Result<(), String> {
        let cap = 4;
        let (tx, _rx) = sync_channel::<DapMessageWithEpoch>(cap);
        let seq = Mutex::new(0i64);

        let initial_drops = dropped_output_event_count();
        let total = 50usize;
        let mut sent = 0usize;
        let mut dropped = 0usize;

        for i in 0..total {
            match dispatch_event(
                &tx,
                &seq,
                "output",
                Some(json!({"output": format!("line {i}\n")})),
            ) {
                EventDispatchResult::Sent => sent += 1,
                EventDispatchResult::Dropped => dropped += 1,
                EventDispatchResult::Disconnected => {
                    return Err(format!("unexpected disconnect on iteration {i}"));
                }
            }
        }

        if dropped == 0 {
            return Err(
                "at least one output event must be dropped when producer outruns consumer".into()
            );
        }
        if sent > cap {
            return Err(format!(
                "in-queue items cannot exceed channel capacity ({cap}), sent={sent}"
            ));
        }
        if sent + dropped != total {
            return Err(format!(
                "every event is either sent or dropped; sent={sent} dropped={dropped} total={total}"
            ));
        }
        if dropped_output_event_count() <= initial_drops {
            return Err("global drop counter must advance when output events are dropped".into());
        }
        Ok(())
    }

    /// Sending to a disconnected receiver returns `Disconnected` for both output
    /// and lifecycle events.
    #[test]
    fn disconnected_receiver_returns_disconnected() -> Result<(), String> {
        let (tx, rx) = sync_channel::<DapMessageWithEpoch>(4);
        let seq = Mutex::new(0i64);

        drop(rx); // disconnect the receiver

        let r = dispatch_event(&tx, &seq, "output", Some(json!({"output": "x\n"})));
        if r != EventDispatchResult::Disconnected {
            return Err(format!("output must be Disconnected when rx dropped, got {r:?}"));
        }

        let r2 = dispatch_event(&tx, &seq, "stopped", Some(json!({"reason": "end"})));
        if r2 != EventDispatchResult::Disconnected {
            return Err(format!("stopped must be Disconnected when rx dropped, got {r2:?}"));
        }
        Ok(())
    }

    /// Real-thread ordering regression test (PR #5318, defect 1 / issue #5149).
    ///
    /// Production shares one `seq: Arc<Mutex<i64>>` and one `SyncSender<DapMessage>`
    /// between the main thread and the output-reader thread. This test reproduces that
    /// shape with two genuine OS threads racing on the same `seq`/`sender` pair. Several
    /// threads flood `output` events (drop-on-full, non-blocking `try_send`) while the
    /// queue is full; another emits a single lifecycle event (blocking `send`). If
    /// seq-assignment and enqueue are not atomic, a later-assigned seq can land on the
    /// wire before an earlier one — a non-monotonic sequence as observed by the receiver.
    #[test]
    fn concurrent_output_flood_and_lifecycle_event_preserve_seq_order() -> Result<(), String> {
        let trials = 5;
        for trial in 0..trials {
            run_seq_order_race_trial(trial)?;
        }
        Ok(())
    }

    fn run_seq_order_race_trial(trial: usize) -> Result<(), String> {
        let cap = 1;
        let (tx, rx) = sync_channel::<DapMessageWithEpoch>(cap);
        let seq = Arc::new(Mutex::new(0i64));

        let r = dispatch_event(&tx, &seq, "output", Some(json!({"output": "fill\n"})));
        if r != EventDispatchResult::Sent {
            return Err(format!("trial {trial}: initial fill should be accepted, got {r:?}"));
        }

        let flood_threads = 12;
        let iterations_per_thread = 4_000;

        let flood_handles: Vec<_> = (0..flood_threads)
            .map(|t| {
                let tx = tx.clone();
                let seq = Arc::clone(&seq);
                thread::spawn(move || {
                    for i in 0..iterations_per_thread {
                        let _ = dispatch_event(
                            &tx,
                            &seq,
                            "output",
                            Some(json!({"output": format!("t{trial}-flood{t}-{i}\n")})),
                        );
                    }
                })
            })
            .collect();

        let tx_life = tx.clone();
        let seq_life = Arc::clone(&seq);
        let life_handle = thread::spawn(move || {
            dispatch_event(
                &tx_life,
                &seq_life,
                "stopped",
                Some(json!({"reason": "pause", "threadId": 1, "allThreadsStopped": true})),
            )
        });

        let mut observed_seqs: Vec<i64> = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok((DapMessage::Event { seq, .. }, _)) => observed_seqs.push(seq),
                Ok(_) => {}
                Err(_) => {
                    if flood_handles.iter().all(|h| h.is_finished()) && life_handle.is_finished() {
                        while let Ok((msg, _)) = rx.try_recv() {
                            if let DapMessage::Event { seq, .. } = msg {
                                observed_seqs.push(seq);
                            }
                        }
                        break;
                    }
                    if Instant::now() > deadline {
                        break;
                    }
                }
            }
        }

        for handle in flood_handles {
            handle.join().map_err(|_| format!("trial {trial}: flood thread panicked"))?;
        }
        let life_result =
            life_handle.join().map_err(|_| format!("trial {trial}: lifecycle thread panicked"))?;
        if life_result != EventDispatchResult::Sent {
            return Err(format!(
                "trial {trial}: stopped event must be delivered, got {life_result:?}"
            ));
        }

        for w in observed_seqs.windows(2) {
            if w[0] > w[1] {
                return Err(format!(
                    "trial {trial}: outbound events observed out of seq order: seq {} arrived \
                     before seq {} (full observed sequence: {:?})",
                    w[0], w[1], observed_seqs
                ));
            }
        }
        Ok(())
    }

    /// Explicit `terminated` case: a lifecycle event named literally `"terminated"` must
    /// never be dropped, even when the outbound queue is full (issue #5149 defect 2).
    /// `terminated` uses the blocking `send` path (only `output` uses drop-on-full
    /// `try_send`).
    #[test]
    fn terminated_event_never_dropped_when_queue_full() -> Result<(), String> {
        let cap = 1;
        let (tx, rx) = sync_channel::<DapMessageWithEpoch>(cap);
        let seq = Arc::new(Mutex::new(0i64));

        let r = dispatch_event(&tx, &seq, "output", Some(json!({"output": "filling\n"})));
        if r != EventDispatchResult::Sent {
            return Err(format!("expected Sent when filling queue, got {r:?}"));
        }

        let tx2 = tx.clone();
        let seq2 = Arc::clone(&seq);
        let handle = thread::spawn(move || {
            dispatch_event(&tx2, &seq2, "terminated", Some(json!({"restart": false})))
        });

        thread::sleep(Duration::from_millis(20));

        let drained = rx
            .recv_timeout(Duration::from_millis(200))
            .map_err(|e| format!("output event must be drainable: {e}"))?;
        if !matches!(&drained, DapMessage::Event { event, .. } if event == "output") {
            return Err(format!("expected output event, got: {drained:?}"));
        }

        let terminated = rx
            .recv_timeout(Duration::from_millis(500))
            .map_err(|e| format!("terminated event must arrive after queue drains: {e}"))?;
        if !matches!(&terminated, DapMessage::Event { event, .. } if event == "terminated") {
            return Err(format!("expected terminated event, got: {terminated:?}"));
        }

        let result = handle.join().map_err(|_| "dispatch thread panicked".to_string())?;
        if result != EventDispatchResult::Sent {
            return Err(format!("terminated event must be delivered (Sent), got {result:?}"));
        }
        Ok(())
    }

    /// After output events are dropped, a synthetic `output` event notifying the user
    /// must eventually surface once the queue has room (issue #5149 defect 3).
    #[test]
    fn drop_notice_appears_after_drops() -> Result<(), String> {
        let cap = 1;
        let (tx, rx) = sync_channel::<DapMessageWithEpoch>(cap);
        let seq = Arc::new(Mutex::new(0i64));

        let producer_threads = 4;
        let iterations_per_producer = 5_000;

        let producers: Vec<_> = (0..producer_threads)
            .map(|p| {
                let tx = tx.clone();
                let seq = Arc::clone(&seq);
                thread::spawn(move || {
                    for i in 0..iterations_per_producer {
                        let _ = dispatch_event(
                            &tx,
                            &seq,
                            "output",
                            Some(json!({"output": format!("p{p}-l{i}\n")})),
                        );
                    }
                })
            })
            .collect();

        let mut found_notice = false;
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok((msg, _)) if is_drop_notice(&msg) => {
                    found_notice = true;
                    break;
                }
                Ok(_) => {}
                Err(_) => {
                    if producers.iter().all(|h| h.is_finished()) {
                        while let Ok(msg) = rx.try_recv() {
                            if is_drop_notice(&msg) {
                                found_notice = true;
                            }
                        }
                        break;
                    }
                }
            }
            if Instant::now() > deadline {
                break;
            }
        }

        for handle in producers {
            handle.join().map_err(|_| "producer thread panicked".to_string())?;
        }

        if !found_notice {
            return Err(
                "expected a synthetic drop-notice output event to appear during a sustained \
                 multi-producer output flood against a slow consumer"
                    .into(),
            );
        }
        Ok(())
    }

    /// A sustained flood must not produce one drop-notice per dropped line. With the queue
    /// permanently full (never drained at all), the notice's own bounded `try_send` retries
    /// can never succeed either, so zero notices land no matter how many output events are
    /// dropped — a strict, deterministic proof the mechanism cannot flood the console.
    #[test]
    fn drop_notice_flood_does_not_produce_one_per_line() -> Result<(), String> {
        let cap = 1;
        let (tx, rx) = sync_channel::<DapMessageWithEpoch>(cap);
        let seq = Mutex::new(0i64);

        let r = dispatch_event(&tx, &seq, "output", Some(json!({"output": "keep\n"})));
        if r != EventDispatchResult::Sent {
            return Err(format!("expected Sent when filling queue, got {r:?}"));
        }

        let total = 500usize;
        let mut dropped = 0usize;
        for i in 0..total {
            if dispatch_event(&tx, &seq, "output", Some(json!({"output": format!("l{i}\n")})))
                == EventDispatchResult::Dropped
            {
                dropped += 1;
            }
        }
        if dropped != total {
            return Err(format!(
                "every send after the first must be dropped (queue never drains); dropped={dropped} total={total}"
            ));
        }

        let mut notices = 0usize;
        let mut total_drained = 0usize;
        while let Ok((msg, _)) = rx.try_recv() {
            total_drained += 1;
            if is_drop_notice(&msg) {
                notices += 1;
            }
        }

        if total_drained != 1 {
            return Err(format!(
                "a capacity-1 queue that never drained can hold only one message; \
                 drained={total_drained}"
            ));
        }
        if notices != 0 {
            return Err(format!(
                "a permanently-full queue must produce zero notices, not one per dropped line \
                 ({dropped} drops); notices={notices}"
            ));
        }
        Ok(())
    }
}
