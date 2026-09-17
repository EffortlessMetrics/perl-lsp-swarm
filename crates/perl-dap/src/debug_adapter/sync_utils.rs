use super::DapMessage;
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

/// Counts dropped `output` events due to a full outbound queue.
static DROPPED_OUTPUT_EVENTS: AtomicU64 = AtomicU64::new(0);

/// The [`DROPPED_OUTPUT_EVENTS`] value as of the last successfully-emitted drop-notice
/// event. Used to rate-limit the synthetic notice: a new one is only attempted once more
/// drops have accumulated since the last one that actually made it onto the wire.
static LAST_NOTIFIED_DROP_COUNT: AtomicU64 = AtomicU64::new(0);

/// Warn on the first drop and every [`OUTPUT_DROP_WARN_INTERVAL`] thereafter so a chatty
/// debuggee cannot turn bounded-queue drops into unbounded log I/O (issue #5149 defect 3).
const OUTPUT_DROP_WARN_INTERVAL: u64 = 64;

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

/// Request-scoped drain barrier (issue #15725).
///
/// Bounded wait before a response write, used by the transport worker to
/// let the event-consumer thread drain the events that *this request's
/// handler* published before the handler returned. Every accepted
/// publication reserves one ticket (under the publication-ordering seq
/// lock, so ticket order equals wire order); a refused, dropped, stale,
/// or disconnected dispatch rolls its ticket back. The consumer completes
/// the exact drained count after its receive loop, which removes the
/// drained tickets in ticket order.
///
/// The response waits on its own *highest* ticket, not on the whole
/// latch: unrelated asynchronous traffic (output-reader events, forwarded
/// attach events) reserves tickets that no response ever waits on. This
/// is the request scoping required by FC-DRAIN-NOT-REQUEST-SCOPED — the
/// previous global pending counter made a busy debug session pay
/// unrelated-event latency on every response, up to the full fail-open
/// cap.
///
/// Fail-open posture is unchanged: a consumer that dies mid-batch or a
/// stalled wire can only hold a ticket until the bounded wait times out,
/// never hang a response, and [`EventDrainLatch::reset`] clears any
/// residue between transport runs.
#[derive(Clone, Default)]
pub(crate) struct EventDrainLatch {
    pending: std::sync::Arc<(Mutex<DrainState>, std::sync::Condvar)>,
}

#[derive(Default)]
struct DrainState {
    /// Monotonic ticket source. Tickets are assigned while the caller
    /// holds the publication-ordering seq lock, so ticket order matches
    /// outbound channel order.
    next_ticket: u64,
    /// Tickets of messages accepted onto the outbound channel and not yet
    /// written by the consumer (plus, transiently, reservations whose
    /// dispatch failed and is about to roll back — the rollback happens
    /// before the seq lock is released, so outside the lock the set only
    /// ever contains published tickets).
    outstanding: std::collections::BTreeSet<u64>,
}

impl EventDrainLatch {
    /// Reserve one outbound slot. The caller must hold the
    /// publication-ordering seq lock from reservation through send so
    /// ticket order cannot invert wire order.
    pub(crate) fn reserve(&self) -> u64 {
        let (mutex, _) = &*self.pending;
        let mut state = lock_or_recover(mutex, "event_drain_latch.reserve");
        state.next_ticket = state.next_ticket.wrapping_add(1);
        let ticket = state.next_ticket;
        state.outstanding.insert(ticket);
        ticket
    }

    /// Roll a reservation back: the dispatch was refused, dropped, stale,
    /// or disconnected, so no message carrying this ticket will be
    /// written. Must run before the caller releases the publication-
    /// ordering seq lock (see [`DrainState::outstanding`]).
    pub(crate) fn rollback(&self, ticket: u64) {
        let (mutex, condvar) = &*self.pending;
        let mut state = lock_or_recover(mutex, "event_drain_latch.rollback");
        if state.outstanding.remove(&ticket) {
            condvar.notify_all();
        }
    }

    /// Record that the consumer wrote `count` previously reserved
    /// messages. The consumer drains the channel FIFO, so these are the
    /// `count` smallest outstanding tickets.
    pub(crate) fn complete(&self, count: usize) {
        if count == 0 {
            return;
        }
        let (mutex, condvar) = &*self.pending;
        let mut state = lock_or_recover(mutex, "event_drain_latch.complete");
        let mut removed_any = false;
        for _ in 0..count {
            match state.outstanding.pop_first() {
                Some(_) => removed_any = true,
                None => break,
            }
        }
        if removed_any {
            condvar.notify_all();
        }
    }

    /// Clear any residue (for example from a previous transport run whose
    /// consumer terminated mid-batch).
    pub(crate) fn reset(&self) {
        let (mutex, condvar) = &*self.pending;
        lock_or_recover(mutex, "event_drain_latch.reset").outstanding.clear();
        condvar.notify_all();
    }

    /// Wait until `ticket` — and, by ticket order, every event published
    /// before it — has been written by the consumer, bounded by `cap`.
    /// Returns `true` when drained, `false` on timeout (fail-open).
    pub(crate) fn wait_for_ticket(&self, ticket: u64, cap: std::time::Duration) -> bool {
        let start = std::time::Instant::now();
        let (mutex, condvar) = &*self.pending;
        let mut state = lock_or_recover(mutex, "event_drain_latch.wait");
        while state.outstanding.contains(&ticket) {
            let elapsed = start.elapsed();
            if elapsed >= cap {
                return false;
            }
            let (guard, timed_out) = match condvar.wait_timeout(state, cap - elapsed) {
                Ok(pair) => pair,
                Err(poisoned) => poisoned.into_inner(),
            };
            state = guard;
            if timed_out.timed_out()
                && state.outstanding.contains(&ticket)
                && start.elapsed() >= cap
            {
                return false;
            }
        }
        true
    }

    /// Whether any reserved message is still outstanding. Test-facing
    /// residue probe (retained-reservation checks).
    #[cfg(test)]
    pub(crate) fn has_outstanding(&self) -> bool {
        let (mutex, _) = &*self.pending;
        !lock_or_recover(mutex, "event_drain_latch.has_outstanding").outstanding.is_empty()
    }
}

/// Request-scoped ticket collection for the drain barrier (#15725).
///
/// The transport worker activates a scope while a request handler runs;
/// every event published *on that thread* while the scope is active
/// records its latch ticket into the scope (see [`note_published_ticket`]).
/// After the handler returns, the worker waits on the scope's highest
/// ticket: the response is ordered after exactly the events its own
/// handler emitted. Threads without an active scope (output readers, the
/// TCP-attach forwarder) publish normally — their tickets join the latch
/// accounting but no response waits on them.
#[derive(Default)]
struct RequestScopeState {
    active: bool,
    max_ticket: Option<u64>,
}

thread_local! {
    static REQUEST_DRAIN_SCOPE: std::cell::RefCell<RequestScopeState> =
        std::cell::RefCell::new(RequestScopeState::default());
}

/// RAII scope active on the worker thread while one request handler runs.
pub(crate) struct RequestDrainScope {
    _priv: (),
}

impl RequestDrainScope {
    /// Activate request-scoped ticket collection on the current thread.
    /// Nested or concurrent activation on one thread is not a supported
    /// shape (request handling is FIFO on a single worker); activation
    /// resets the state so a leaked stale scope cannot poison a later one.
    pub(crate) fn activate() -> Self {
        REQUEST_DRAIN_SCOPE.with(|scope| {
            *scope.borrow_mut() = RequestScopeState { active: true, max_ticket: None };
        });
        Self { _priv: () }
    }

    /// The highest ticket published under this scope, if the handler
    /// published any event.
    pub(crate) fn wait_target(&self) -> Option<u64> {
        REQUEST_DRAIN_SCOPE.with(|scope| scope.borrow().max_ticket)
    }
}

impl Drop for RequestDrainScope {
    fn drop(&mut self) {
        REQUEST_DRAIN_SCOPE.with(|scope| *scope.borrow_mut() = RequestScopeState::default());
    }
}

/// Record a successfully published ticket into the active request scope,
/// if one is active on the publishing thread. Called by the dispatch
/// primitives right after the message was accepted onto the outbound
/// channel (while the publication-ordering seq lock is still held).
pub(super) fn note_published_ticket(ticket: u64) {
    REQUEST_DRAIN_SCOPE.with(|scope| {
        let mut state = scope.borrow_mut();
        if state.active {
            state.max_ticket = Some(state.max_ticket.map_or(ticket, |max| max.max(ticket)));
        }
    });
}

/// Shared event-sender admission gate. A producer clones the sender while the
/// gate is held, then performs its potentially blocking send after releasing
/// the gate. Closing therefore cannot wait on a producer, while an admitted
/// clone keeps the receiver connected until that send completes.
#[derive(Clone)]
pub(crate) struct EventSender(Arc<Mutex<Option<SyncSender<DapMessage>>>>);

impl EventSender {
    pub(crate) fn new(sender: SyncSender<DapMessage>) -> Self {
        Self(Arc::new(Mutex::new(Some(sender))))
    }

    pub(crate) fn close(&self) {
        *lock_or_recover(&self.0, "event_sender") = None;
    }

    pub(super) fn admitted_sender(&self) -> Option<SyncSender<DapMessage>> {
        lock_or_recover(&self.0, "event_sender").as_ref().cloned()
    }

    pub(crate) fn send_event(
        &self,
        seq: &Mutex<i64>,
        event: &str,
        body: Option<Value>,
        drain: Option<&EventDrainLatch>,
    ) -> EventDispatchResult {
        let Some(sender) = self.admitted_sender() else {
            return EventDispatchResult::Disconnected;
        };
        dispatch_event(&sender, seq, event, body, drain)
    }

    pub(crate) fn send_event_generation_guarded(
        &self,
        seq: &Mutex<i64>,
        event: &str,
        body: Option<Value>,
        stale: &dyn Fn() -> bool,
        drain: Option<&EventDrainLatch>,
    ) -> GuardedDispatchResult {
        let Some(sender) = self.admitted_sender() else {
            return GuardedDispatchResult::Disconnected;
        };
        dispatch_event_generation_guarded(&sender, seq, event, body, stale, drain)
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
/// outbound channel. The drain ticket (when `drain` is given) is reserved inside the
/// same critical section, so ticket order equals wire order and the consumer's
/// smallest-first completion removes exactly the drained tickets (#15725).
///
/// Callers must not hold any other lock that the writer/consumer thread may need to
/// acquire while draining the channel (e.g. the transport's response-writer mutex) —
/// doing so can deadlock when this call blocks on a full queue. See `transport.rs`'s
/// `run_with_io` for the scoped-guard pattern that avoids this.
pub(crate) fn dispatch_event(
    sender: &SyncSender<DapMessage>,
    seq: &Mutex<i64>,
    event: &str,
    body: Option<Value>,
    drain: Option<&EventDrainLatch>,
) -> EventDispatchResult {
    let (msg, mut seq_lock) = {
        let mut seq_lock = lock_or_recover(seq, "dispatch_event.seq");
        *seq_lock += 1;
        (DapMessage::Event { seq: *seq_lock, event: event.to_string(), body }, seq_lock)
    };
    // Reserve inside the seq-locked region so ticket order cannot invert
    // publication order; a refused or dropped dispatch rolls the ticket
    // back before the lock is released.
    let ticket = drain.map(|drain| drain.reserve());

    let result = if is_output_event(event) {
        match sender.try_send(msg) {
            Ok(()) => EventDispatchResult::Sent,
            Err(TrySendError::Full(_)) => {
                let dropped_total = DROPPED_OUTPUT_EVENTS.fetch_add(1, Ordering::Relaxed) + 1;
                if should_warn_on_drop(dropped_total) {
                    tracing::warn!(
                        dropped = dropped_total,
                        "DAP outbound queue full; dropping output events"
                    );
                }
                try_emit_drop_notice(sender, &mut seq_lock, dropped_total, drain);
                EventDispatchResult::Dropped
            }
            Err(TrySendError::Disconnected(_)) => EventDispatchResult::Disconnected,
        }
    } else {
        match sender.send(msg) {
            Ok(()) => EventDispatchResult::Sent,
            Err(_) => EventDispatchResult::Disconnected,
        }
    };

    if let Some(ticket) = ticket {
        if matches!(&result, EventDispatchResult::Sent) {
            note_published_ticket(ticket);
        } else if let Some(drain) = drain {
            drain.rollback(ticket);
        }
    }
    result
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
    sender: &SyncSender<DapMessage>,
    seq: &Mutex<i64>,
    event: &str,
    body: Option<Value>,
    stale: &dyn Fn() -> bool,
    drain: Option<&EventDrainLatch>,
) -> GuardedDispatchResult {
    let (mut msg, mut seq_lock) = {
        let mut seq_lock = lock_or_recover(seq, "dispatch_event.seq");
        *seq_lock += 1;
        (DapMessage::Event { seq: *seq_lock, event: event.to_string(), body }, seq_lock)
    };
    // Reserved inside the seq-locked region (which spans the retry parks)
    // so ticket order equals publication order; a stale or disconnected
    // outcome rolls the ticket back before the lock is released.
    let ticket = drain.map(|drain| drain.reserve());

    let result = if is_output_event(event) {
        match sender.try_send(msg) {
            Ok(()) => GuardedDispatchResult::Sent,
            Err(TrySendError::Full(_)) => {
                let dropped_total = DROPPED_OUTPUT_EVENTS.fetch_add(1, Ordering::Relaxed) + 1;
                if should_warn_on_drop(dropped_total) {
                    tracing::warn!(
                        dropped = dropped_total,
                        "DAP outbound queue full; dropping output events"
                    );
                }
                try_emit_drop_notice(sender, &mut seq_lock, dropped_total, drain);
                GuardedDispatchResult::Dropped
            }
            Err(TrySendError::Disconnected(_)) => GuardedDispatchResult::Disconnected,
        }
    } else {
        loop {
            if stale() {
                break GuardedDispatchResult::Stale;
            }
            match sender.try_send(msg) {
                Ok(()) => break GuardedDispatchResult::Sent,
                Err(TrySendError::Full(returned)) => {
                    msg = returned;
                    std::thread::sleep(GENERATION_GUARD_PARK);
                }
                Err(TrySendError::Disconnected(_)) => break GuardedDispatchResult::Disconnected,
            }
        }
    };

    if let Some(ticket) = ticket {
        if matches!(&result, GuardedDispatchResult::Sent) {
            note_published_ticket(ticket);
        } else if let Some(drain) = drain {
            drain.rollback(ticket);
        }
    }
    result
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
///   re-acquiring the mutex. When `drain` is given, the notice reserves its own ticket
///   inside that critical section so every channel message holds exactly one
///   outstanding ticket (#15725).
fn try_emit_drop_notice(
    sender: &SyncSender<DapMessage>,
    seq_lock: &mut MutexGuard<'_, i64>,
    dropped_total: u64,
    drain: Option<&EventDrainLatch>,
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
    let ticket = drain.map(|drain| drain.reserve());

    // Bounded, non-blocking retries: gives a slow-but-not-permanently-stalled client's
    // writer thread a few scheduling slices to drain a slot, without ever looping
    // unboundedly or blocking. Fixed upper bound, no recursion.
    const MAX_ATTEMPTS: u8 = 8;
    let mut published = false;
    for attempt in 0..MAX_ATTEMPTS {
        let msg =
            DapMessage::Event { seq: next_seq, event: "output".to_string(), body: body.clone() };
        match sender.try_send(msg) {
            Ok(()) => {
                **seq_lock = next_seq;
                LAST_NOTIFIED_DROP_COUNT.store(dropped_total, Ordering::Relaxed);
                published = true;
                break;
            }
            Err(TrySendError::Disconnected(_)) => break,
            Err(TrySendError::Full(_)) => {
                if attempt + 1 < MAX_ATTEMPTS {
                    std::thread::yield_now();
                }
            }
        }
    }
    match (ticket, published) {
        (Some(ticket), true) => note_published_ticket(ticket),
        (Some(ticket), false) => {
            if let Some(drain) = drain {
                drain.rollback(ticket);
            }
        }
        (None, _) => {}
    }
    // Queue stayed full for every attempt when not published: skip silently. Do not
    // retry beyond the fixed bound above, do not consume a seq number, do not recurse
    // into the drop-counting path.
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

    #[test]
    fn event_sender_closes_admission_after_flushing_existing_event() -> Result<(), String> {
        let (tx, rx) = sync_channel::<DapMessage>(2);
        let sender = EventSender::new(tx);
        let seq = Mutex::new(0i64);
        if sender.send_event(&seq, "output", Some(json!({"output": "before-close\n"})), None)
            != EventDispatchResult::Sent
        {
            return Err("pre-close event was not admitted".to_string());
        }
        sender.close();
        if sender.send_event(&seq, "output", Some(json!({"output": "after-close\n"})), None)
            != EventDispatchResult::Disconnected
        {
            return Err("late event was admitted after close".to_string());
        }
        match rx.recv().map_err(|error| error.to_string())? {
            DapMessage::Event { event, .. } if event == "output" => {}
            other => return Err(format!("unexpected flushed event: {other:?}")),
        }
        match rx.try_recv() {
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Ok(()),
            other => Err(format!("sender remained connected after close: {other:?}")),
        }
    }

    #[test]
    fn event_sender_flushes_admitted_lifecycle_event_before_close() -> Result<(), String> {
        let (tx, rx) = sync_channel::<DapMessage>(1);
        let sender = EventSender::new(tx);
        let seq = Arc::new(Mutex::new(0i64));
        if sender.send_event(&seq, "output", Some(json!({"output": "queued\n"})), None)
            != EventDispatchResult::Sent
        {
            return Err("queue-filling event was not admitted".to_string());
        }

        let producer_sender = sender.clone();
        let producer_seq = Arc::clone(&seq);
        let producer = thread::spawn(move || {
            producer_sender.send_event(
                &producer_seq,
                "stopped",
                Some(json!({"reason": "pause"})),
                None,
            )
        });
        match rx.recv().map_err(|error| error.to_string())? {
            DapMessage::Event { event, .. } if event == "output" => {}
            other => return Err(format!("unexpected queued event: {other:?}")),
        }
        if producer.join().map_err(|_| "producer panicked".to_string())?
            != EventDispatchResult::Sent
        {
            return Err("admitted lifecycle event was not delivered".to_string());
        }

        sender.close();
        if sender.send_event(&seq, "stopped", Some(json!({"reason": "late"})), None)
            != EventDispatchResult::Disconnected
        {
            return Err("late lifecycle event was admitted after close".to_string());
        }
        match rx.recv().map_err(|error| error.to_string())? {
            DapMessage::Event { event, .. } if event == "stopped" => {}
            other => return Err(format!("expected flushed lifecycle event: {other:?}")),
        }
        drop(sender);
        match rx.try_recv() {
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Ok(()),
            other => Err(format!("event channel remained open: {other:?}")),
        }
    }

    #[test]
    fn event_sender_close_does_not_wait_on_admitted_send() -> Result<(), String> {
        let (tx, rx) = sync_channel::<DapMessage>(1);
        let sender = EventSender::new(tx);
        let seq = Mutex::new(0i64);
        if sender.send_event(&seq, "output", Some(json!({"output": "queued\n"})), None)
            != EventDispatchResult::Sent
        {
            return Err("queue-filling event was not admitted".to_string());
        }
        let seq = Arc::new(seq);
        let producer_seq = Arc::clone(&seq);
        let producer_sender = sender.clone();
        let producer = thread::spawn(move || {
            producer_sender.send_event(
                &producer_seq,
                "stopped",
                Some(json!({"reason": "pause"})),
                None,
            )
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
        if !matches!(&queued, DapMessage::Event { event, .. } if event == "output") {
            let _ = producer.join();
            let _ = closer.join();
            return Err(format!("unexpected queued event: {queued:?}"));
        }
        let producer_result =
            producer.join().map_err(|_| "admitted producer panicked".to_string())?;
        let _ = closer.join();
        let late_result = sender.send_event(&seq, "stopped", Some(json!({"reason": "late"})), None);
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
            DapMessage::Event { event, .. } if event == "stopped" => {}
            other => return Err(format!("expected admitted lifecycle event: {other:?}")),
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
        let (tx, _rx) = sync_channel::<DapMessage>(cap);
        let seq = Mutex::new(0i64);

        for i in 0..cap {
            let result = dispatch_event(
                &tx,
                &seq,
                "output",
                Some(json!({"output": format!("line {i}\n")})),
                None,
            );
            if result != EventDispatchResult::Sent {
                return Err(format!("slot {i} should be accepted, got {result:?}"));
            }
        }

        let result =
            dispatch_event(&tx, &seq, "output", Some(json!({"output": "overflow\n"})), None);
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
        let (tx, rx) = sync_channel::<DapMessage>(cap);
        let seq = Arc::new(Mutex::new(0i64));

        let r = dispatch_event(&tx, &seq, "output", Some(json!({"output": "filling\n"})), None);
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
                None,
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
        let (tx, _rx) = sync_channel::<DapMessage>(cap);
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
                None,
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
        let (tx, rx) = sync_channel::<DapMessage>(4);
        let seq = Mutex::new(0i64);

        drop(rx); // disconnect the receiver

        let r = dispatch_event(&tx, &seq, "output", Some(json!({"output": "x\n"})), None);
        if r != EventDispatchResult::Disconnected {
            return Err(format!("output must be Disconnected when rx dropped, got {r:?}"));
        }

        let r2 = dispatch_event(&tx, &seq, "stopped", Some(json!({"reason": "end"})), None);
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
        let (tx, rx) = sync_channel::<DapMessage>(cap);
        let seq = Arc::new(Mutex::new(0i64));

        let r = dispatch_event(&tx, &seq, "output", Some(json!({"output": "fill\n"})), None);
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
                            None,
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
                None,
            )
        });

        let mut observed_seqs: Vec<i64> = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(DapMessage::Event { seq, .. }) => observed_seqs.push(seq),
                Ok(_) => {}
                Err(_) => {
                    if flood_handles.iter().all(|h| h.is_finished()) && life_handle.is_finished() {
                        while let Ok(msg) = rx.try_recv() {
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
        let (tx, rx) = sync_channel::<DapMessage>(cap);
        let seq = Arc::new(Mutex::new(0i64));

        let r = dispatch_event(&tx, &seq, "output", Some(json!({"output": "filling\n"})), None);
        if r != EventDispatchResult::Sent {
            return Err(format!("expected Sent when filling queue, got {r:?}"));
        }

        let tx2 = tx.clone();
        let seq2 = Arc::clone(&seq);
        let handle = thread::spawn(move || {
            dispatch_event(&tx2, &seq2, "terminated", Some(json!({"restart": false})), None)
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
        let (tx, rx) = sync_channel::<DapMessage>(cap);
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
                            None,
                        );
                    }
                })
            })
            .collect();

        let mut found_notice = false;
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(msg) if is_drop_notice(&msg) => {
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
        let (tx, rx) = sync_channel::<DapMessage>(cap);
        let seq = Mutex::new(0i64);

        let r = dispatch_event(&tx, &seq, "output", Some(json!({"output": "keep\n"})), None);
        if r != EventDispatchResult::Sent {
            return Err(format!("expected Sent when filling queue, got {r:?}"));
        }

        let total = 500usize;
        let mut dropped = 0usize;
        for i in 0..total {
            if dispatch_event(&tx, &seq, "output", Some(json!({"output": format!("l{i}\n")})), None)
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
        while let Ok(msg) = rx.try_recv() {
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

    // ── request-scoped drain barrier (#15725, FC-DRAIN-NOT-REQUEST-SCOPED) ─────

    /// Core falsifier of the request scoping: the response waits on its own
    /// ticket only. A request-scoped barrier must release the response as soon
    /// as the request's own event is written, even though an unrelated
    /// asynchronous reservation is still outstanding — the previous global
    /// pending counter kept the waiter blocked here (deterministic
    /// interleaving, no timing dependency: zero-cap waits never park).
    #[test]
    fn request_scoped_wait_ignores_unrelated_reservations() -> Result<(), String> {
        let latch = EventDrainLatch::default();

        // Interleaving: the handler's event is reserved first, then an
        // unrelated asynchronous event (e.g. an output-reader emission)
        // reserves behind it.
        let own = latch.reserve();
        let unrelated = latch.reserve();

        // The consumer writes in FIFO/ticket order: the request's own event.
        latch.complete(1);

        // The response's request-scoped wait must be satisfied by its own
        // ticket's completion alone.
        if !latch.wait_for_ticket(own, Duration::from_millis(0)) {
            return Err("the response must not wait for an unrelated event's reservation \
                 after its own event drained (request scoping violated)"
                .to_string());
        }
        // And the unrelated reservation must still be outstanding: it was
        // not consumed by the response's wait.
        if !latch.has_outstanding() {
            return Err("the unrelated reservation must still be outstanding".to_string());
        }
        if latch.wait_for_ticket(unrelated, Duration::from_millis(0)) {
            return Err("an outstanding unrelated ticket must not report drained".to_string());
        }

        // Draining the unrelated event releases a waiter that targets it.
        latch.complete(1);
        if !latch.wait_for_ticket(unrelated, Duration::from_millis(0)) {
            return Err(
                "the unrelated ticket must drain after the consumer completes it".to_string()
            );
        }
        if latch.has_outstanding() {
            return Err("the latch must be fully drained".to_string());
        }
        Ok(())
    }

    /// A rolled-back reservation must not wedge the smallest-first completion
    /// accounting: the consumer completes exactly the tickets of the messages
    /// it wrote, and waiters on later tickets are released at their own ticket.
    #[test]
    fn rollback_keeps_ticket_accounting_aligned() -> Result<(), String> {
        let latch = EventDrainLatch::default();
        let first = latch.reserve();
        let second = latch.reserve();

        // The first dispatch was refused (queue full / disconnected): its
        // message never enters the channel, so its ticket is rolled back.
        latch.rollback(first);
        if latch.wait_for_ticket(second, Duration::from_millis(0)) {
            return Err(
                "a ticket behind an outstanding message must not report drained".to_string()
            );
        }

        // The consumer writes the only published message.
        latch.complete(1);
        if !latch.wait_for_ticket(second, Duration::from_millis(0)) {
            return Err("the published message's ticket must drain after completion".to_string());
        }
        if latch.has_outstanding() {
            return Err("a rolled-back ticket must not leave residue".to_string());
        }
        Ok(())
    }

    /// Ticket collection is thread-scoped: publications recorded on a thread
    /// without an active request scope never join a scope on another thread.
    #[test]
    fn scope_collection_follows_the_publishing_thread() -> Result<(), String> {
        // No scope active here: a recorded publication is invisible.
        note_published_ticket(41);
        let scope = RequestDrainScope::activate();
        if scope.wait_target().is_some() {
            return Err("activation must not inherit tickets from outside the scope".to_string());
        }
        note_published_ticket(7);
        note_published_ticket(3);
        let target = scope.wait_target();
        if target != Some(7) {
            return Err(format!(
                "the scope must track the highest published ticket (7), got {target:?}"
            ));
        }
        drop(scope);
        let after = RequestDrainScope::activate();
        if after.wait_target().is_some() {
            return Err(
                "a fresh scope must start empty after the previous scope closed".to_string()
            );
        }
        Ok(())
    }

    /// The `count == 0` boundary: completing nothing is a no-op — no
    /// reservation may be consumed and no waiter woken (the consumer's
    /// batch-completion fast path must not be able to eat tickets).
    #[test]
    fn complete_zero_consumes_nothing() -> Result<(), String> {
        let latch = EventDrainLatch::default();
        let ticket = latch.reserve();
        latch.complete(0);
        if !latch.has_outstanding() {
            return Err("complete(0) must not consume the outstanding reservation".to_string());
        }
        if latch.wait_for_ticket(ticket, Duration::from_millis(0)) {
            return Err(
                "complete(0) must not release a waiter on an outstanding ticket".to_string()
            );
        }
        // And the latch stays usable afterwards.
        latch.complete(1);
        if !latch.wait_for_ticket(ticket, Duration::from_millis(0)) {
            return Err("the ticket must drain after a real completion".to_string());
        }
        Ok(())
    }
}
