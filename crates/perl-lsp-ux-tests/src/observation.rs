//! Event-driven observation substrate for the real-process UX harness.
//!
//! Every harness wait — responses, server-initiated events, and terminal
//! transport/process state — goes through one [`Inbox`]. The reader thread
//! publishes into it and wakes waiters; waiters block on a condition variable
//! rather than sleeping on a wall-clock timer.
//!
//! # Why not poll
//!
//! A timeout is a valid *outer bound*. A repeated wall-clock sleep is a weak
//! correctness oracle: it lengthens fast runs, makes results runner-speed
//! sensitive, and makes a lost wakeup indistinguishable from slow product
//! behavior. Here the deadline never decides correctness by itself — it is only
//! the bound after which a wait reports [`WaitEnd::Deadline`].
//!
//! # Lost-wakeup safety
//!
//! Every mutation of the inbox bumps a monotonic sequence counter. A waiter
//!
//! 1. snapshots the buffer *and* the sequence under the lock,
//! 2. evaluates its predicate with **no lock held**,
//! 3. re-acquires the lock and blocks only while the sequence is still the one
//!    it snapshotted.
//!
//! An observation that arrives between (1) and (3) bumps the sequence, so step
//! (3) does not block at all and the predicate is re-evaluated immediately. The
//! caller can therefore never be forced to wait until its deadline for an
//! observation that already arrived.
//!
//! # Outcome precedence
//!
//! A wait resolves in this order, deliberately:
//!
//! 1. **a match** — the predicate always runs on the freshest view first;
//! 2. **a stream end** — an ended stream outranks the bound, so a closed or
//!    broken transport is never reported as "nothing happened in time";
//! 3. **the deadline**.
//!
//! Rule 1 means an observation that lands around the deadline can still satisfy
//! the wait. That is intentional. The alternative — checking the clock first —
//! discards an observation that genuinely arrived in time but whose waiter had
//! not been scheduled yet, which is a false negative whose likelihood grows
//! with machine load. That is precisely the runner-speed sensitivity this
//! module exists to remove, so the tie is broken in favour of the observation.
//!
//! The consequence is that the deadline bounds **how long a wait blocks**, not
//! the arrival time of what it returns. It is not a latency oracle; measuring
//! server latency is a separate concern with its own authority. The loop is
//! still bounded: at most one further evaluation happens once the bound passes.
//!
//! # Where the typed reason survives — and where it does not
//!
//! [`WaitEnd`] is produced by every wait here, but not every *caller* keeps it.
//! Be precise about which layer you are relying on:
//!
//! - **Kept.** The substrate itself, and `UxClient::wait_for_response`, which
//!   folds the reason (plus the child's real exit status) into its error.
//! - **Discarded today.** The convenience wrappers above it collapse the
//!   outcome to a plain value: `DiagnosticsTracker`'s waits end in `.ok()`,
//!   `UxHarness::wait_for_active_document_ready` and
//!   `wait_for_index_ready_event_after` in `.is_ok()`, and
//!   `wait_for_diagnostics` / `wait_for_latest_diagnostics` in
//!   `.unwrap_or_default()`. A scenario calling those still cannot tell a
//!   closed stream from an expired bound.
//!
//! So "the harness can tell you why a wait ended" is true of this module and
//! the request path, and not yet true end to end. Propagating the reason
//! through those wrappers changes their signatures and their call sites, and is
//! tracked as remaining work on the still-open #13319 rather than claimed here.

use serde_json::Value;
use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Monotonic identity of one buffered observation.
///
/// Identities are unique for the life of an [`Inbox`] and are never reused, so
/// consuming a response by identity can never take a different message that
/// happens to carry the same JSON-RPC id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObservationId(u64);

impl ObservationId {
    /// The raw monotonic value, for diagnostics and ordering assertions.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Why the server's output stream ended.
///
/// This is the discriminator between an orderly shutdown and a corrupted
/// transport. It is deliberately *not* collapsed into the deadline outcome:
/// a caller must be able to tell "the server exited as expected" from "the
/// framing broke" from "nothing happened in time".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEnd {
    /// Clean end of stream at a message boundary — the orderly shutdown path.
    ServerClosed,
    /// The transport failed: malformed framing, a truncated body, unparsable
    /// JSON, or an I/O error mid-message.
    TransportFailure {
        /// Human-readable detail describing the exact framing/IO failure.
        detail: String,
    },
}

/// Outcome of a wait that did not match its predicate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaitEnd {
    /// The absolute deadline expired while the stream was still live.
    ///
    /// This is the *only* outcome that means "nothing decided" — a live stream
    /// that simply did not produce the awaited observation in time.
    Deadline {
        /// The bound the caller supplied.
        timeout: Duration,
    },
    /// The stream ended before the predicate matched.
    Ended(StreamEnd),
}

impl WaitEnd {
    /// Describe the outcome for assertion messages.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Deadline { timeout } => {
                format!(
                    "deadline expired after {}ms with the stream still live",
                    timeout.as_millis()
                )
            }
            Self::Ended(StreamEnd::ServerClosed) => {
                "server closed its output stream (orderly end of stream)".to_string()
            }
            Self::Ended(StreamEnd::TransportFailure { detail }) => {
                format!("transport failure: {detail}")
            }
        }
    }

    /// True when the stream is still live and only the bound expired.
    #[must_use]
    pub const fn is_deadline(&self) -> bool {
        matches!(self, Self::Deadline { .. })
    }
}

/// A consistent, lock-free view of the inbox at one sequence point.
#[derive(Debug, Clone)]
pub struct InboxSnapshot {
    /// Sequence counter at the moment this snapshot was taken.
    seq: u64,
    /// Server-initiated messages, oldest first. Never consumed by a wait.
    events: Vec<Value>,
    /// Buffered responses, oldest first, each with its consumable identity.
    responses: Vec<(ObservationId, Value)>,
}

impl InboxSnapshot {
    /// Server-initiated messages buffered at this sequence point, oldest first.
    #[must_use]
    pub fn events(&self) -> &[Value] {
        &self.events
    }

    /// Buffered responses at this sequence point, oldest first.
    #[must_use]
    pub fn responses(&self) -> &[(ObservationId, Value)] {
        &self.responses
    }

    /// The sequence point this snapshot was taken at.
    #[must_use]
    pub const fn seq(&self) -> u64 {
        self.seq
    }
}

#[derive(Debug)]
struct InboxState {
    events: VecDeque<(ObservationId, Value)>,
    responses: VecDeque<(ObservationId, Value)>,
    seq: u64,
    next_id: u64,
    terminal: Option<StreamEnd>,
}

/// The shared observation buffer every harness wait consumes.
///
/// Cheap to clone: clones share one underlying buffer.
#[derive(Debug, Clone)]
pub struct Inbox {
    inner: Arc<InboxInner>,
}

#[derive(Debug)]
struct InboxInner {
    state: Mutex<InboxState>,
    signal: Condvar,
}

impl Default for Inbox {
    fn default() -> Self {
        Self::new()
    }
}

impl Inbox {
    /// Create an empty, live inbox.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(InboxInner {
                state: Mutex::new(InboxState {
                    events: VecDeque::new(),
                    responses: VecDeque::new(),
                    seq: 0,
                    next_id: 0,
                    terminal: None,
                }),
                signal: Condvar::new(),
            }),
        }
    }

    /// Lock the state, tolerating a poisoned mutex the way the rest of this
    /// harness does: a panicking reader thread must not wedge every waiter.
    fn lock(&self) -> MutexGuard<'_, InboxState> {
        self.inner.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Publish a server-initiated message and wake every waiter.
    pub fn push_event(&self, message: Value) -> ObservationId {
        self.push(message, false)
    }

    /// Publish a response and wake every waiter.
    pub fn push_response(&self, message: Value) -> ObservationId {
        self.push(message, true)
    }

    fn push(&self, message: Value, is_response: bool) -> ObservationId {
        let id = {
            let mut state = self.lock();
            let id = ObservationId(state.next_id);
            state.next_id += 1;
            state.seq += 1;
            if is_response {
                state.responses.push_back((id, message));
            } else {
                state.events.push_back((id, message));
            }
            id
        };
        // Notify outside the lock so a woken waiter does not immediately block
        // on a mutex this thread still holds.
        self.inner.signal.notify_all();
        id
    }

    /// Record why the stream ended and wake every waiter.
    ///
    /// This deliberately does **not** bump the sequence counter: any
    /// observation published before the close is therefore always evaluated by
    /// a waiter before the terminal state is reported, so a final response can
    /// never be lost to the shutdown that immediately follows it.
    ///
    /// The first end recorded wins; a later close cannot relabel an already
    /// classified failure.
    pub fn close(&self, end: StreamEnd) {
        {
            let mut state = self.lock();
            if state.terminal.is_none() {
                state.terminal = Some(end);
            }
        }
        self.inner.signal.notify_all();
    }

    /// The recorded stream end, if the stream has ended.
    #[must_use]
    pub fn stream_end(&self) -> Option<StreamEnd> {
        self.lock().terminal.clone()
    }

    /// Take a consistent view of the buffer. The lock is held only for the
    /// clone — never while a caller's predicate runs.
    #[must_use]
    pub fn snapshot(&self) -> InboxSnapshot {
        let state = self.lock();
        InboxSnapshot {
            seq: state.seq,
            events: state.events.iter().map(|(_, value)| value.clone()).collect(),
            responses: state.responses.iter().cloned().collect(),
        }
    }

    /// Remove one response by its observation identity.
    ///
    /// Returns `None` when another waiter already consumed it, which is the
    /// signal for the caller to re-evaluate rather than assume a match.
    pub fn take_response(&self, id: ObservationId) -> Option<Value> {
        let taken = {
            let mut state = self.lock();
            let position = state.responses.iter().position(|(candidate, _)| *candidate == id)?;
            // Consuming is a mutation of a waiter's input, so it advances the
            // sequence for the same reason a publication or a drain does. No
            // predicate here reasons about a response *disappearing* today, but
            // the lost-wakeup guard is only sound while the sequence tracks
            // every change — a silent removal would be a trap for the first one
            // that does.
            let taken = state.responses.remove(position).map(|(_, value)| value);
            state.seq += 1;
            taken
        };
        self.inner.signal.notify_all();
        taken
    }

    /// Drain every buffered event, leaving the responses untouched.
    pub fn drain_events(&self) -> Vec<Value> {
        let drained = {
            let mut state = self.lock();
            // Draining is a mutation like any other, so it advances the
            // sequence: the invariant that `seq` moves whenever a waiter's
            // input changes must hold for removals too, not just publications.
            state.seq += 1;
            state.events.drain(..).map(|(_, value)| value).collect::<Vec<_>>()
        };
        self.inner.signal.notify_all();
        drained
    }

    /// Block until `select` matches, the stream ends, or `timeout` expires.
    ///
    /// `select` is evaluated with **no inbox lock held**, so a caller predicate
    /// may do arbitrary work without blocking the reader thread. See the module
    /// documentation for why this stays lost-wakeup safe.
    ///
    /// # Errors
    ///
    /// Returns [`WaitEnd::Ended`] when the stream ended before a match, and
    /// [`WaitEnd::Deadline`] when the bound expired with the stream still live.
    /// The stream end is always reported in preference to the deadline.
    pub fn wait_for<T>(
        &self,
        timeout: Duration,
        select: impl FnMut(&InboxSnapshot) -> Option<T>,
    ) -> Result<T, WaitEnd> {
        self.wait_projected(timeout, Self::snapshot_of, select)
    }

    /// Wait over the buffered **responses** only.
    ///
    /// Prefer this to [`Self::wait_for`] when the predicate only inspects
    /// responses. Every observation wakes every waiter, and each wake copies
    /// the buffers the waiter asked for — so a response wait that also copied
    /// the event buffer would do work proportional to all unrelated
    /// notifications received so far, on every one of them. That is quadratic
    /// in notification count for a single wait, and it contends with the reader
    /// thread that is trying to publish. Responses are consumed as they match,
    /// so this view stays small.
    ///
    /// # Errors
    ///
    /// Returns the typed [`WaitEnd`].
    pub fn wait_for_responses<T>(
        &self,
        timeout: Duration,
        select: impl FnMut(&[(ObservationId, Value)]) -> Option<T>,
    ) -> Result<T, WaitEnd> {
        let mut select = select;
        self.wait_projected(
            timeout,
            |state| state.responses.iter().cloned().collect::<Vec<_>>(),
            move |responses: &Vec<(ObservationId, Value)>| select(responses),
        )
    }

    /// Wait over the buffered server-initiated **events** only.
    ///
    /// The counterpart to [`Self::wait_for_responses`]; see its note on why a
    /// wait copies only the buffer it actually inspects.
    ///
    /// # Errors
    ///
    /// Returns the typed [`WaitEnd`].
    pub fn wait_for_raw_events<T>(
        &self,
        timeout: Duration,
        select: impl FnMut(&[Value]) -> Option<T>,
    ) -> Result<T, WaitEnd> {
        let mut select = select;
        self.wait_projected(
            timeout,
            |state| state.events.iter().map(|(_, value)| value.clone()).collect::<Vec<_>>(),
            move |events: &Vec<Value>| select(events),
        )
    }

    fn snapshot_of(state: &InboxState) -> InboxSnapshot {
        InboxSnapshot {
            seq: state.seq,
            events: state.events.iter().map(|(_, value)| value.clone()).collect(),
            responses: state.responses.iter().cloned().collect(),
        }
    }

    /// The shared wait: copy only the requested view under the lock, evaluate
    /// the predicate with no lock held, then block until something changes.
    fn wait_projected<V, T>(
        &self,
        timeout: Duration,
        mut project: impl FnMut(&InboxState) -> V,
        mut select: impl FnMut(&V) -> Option<T>,
    ) -> Result<T, WaitEnd> {
        let deadline = Instant::now().checked_add(timeout).unwrap_or_else(Instant::now);
        loop {
            let (seq, view) = {
                let state = self.lock();
                (state.seq, project(&state))
            };
            if let Some(matched) = select(&view) {
                return Ok(matched);
            }
            // Bound this loop, not just the blocking step inside `wait_past`.
            // Every new observation advances the sequence, and `wait_past`
            // returns immediately when it has — so a steady stream of
            // unrelated traffic would otherwise spin here forever and a
            // "bounded" wait would never honour its bound.
            //
            // The predicate above always runs first on the freshest snapshot,
            // so a match that genuinely arrived in time is still returned; and
            // a terminal stream state still outranks the deadline.
            if Instant::now() >= deadline {
                // The view just evaluated may already be stale: an observation
                // can land while the predicate runs outside the lock, and the
                // predicate itself may publish or close. Returning a terminal
                // outcome now would report `Ended`/`Deadline` with a match
                // sitting buffered, contradicting the documented
                // match-before-stream-end precedence at exactly the boundary
                // where it matters most.
                //
                // So give the freshest view one final evaluation before
                // reporting. This is bounded: it happens at most once, only
                // when the sequence actually moved, and it returns either way.
                let (fresh_seq, fresh_view) = {
                    let state = self.lock();
                    (state.seq, project(&state))
                };
                if fresh_seq != seq
                    && let Some(matched) = select(&fresh_view)
                {
                    return Ok(matched);
                }
                return Err(self
                    .stream_end()
                    .map_or(WaitEnd::Deadline { timeout }, WaitEnd::Ended));
            }
            self.wait_past(seq, deadline, timeout)?;
        }
    }

    /// Block until the sequence advances past `observed_seq`, the stream ends,
    /// or the deadline expires.
    fn wait_past(
        &self,
        observed_seq: u64,
        deadline: Instant,
        timeout: Duration,
    ) -> Result<(), WaitEnd> {
        let mut state = self.lock();
        loop {
            // The sequence moved while the predicate ran outside the lock: a
            // new observation is already buffered, so re-evaluate immediately
            // instead of blocking. This is the lost-wakeup guard.
            if state.seq != observed_seq {
                return Ok(());
            }
            // A terminal stream state outranks the deadline: a caller must
            // learn that the server closed or the framing broke, never that
            // "nothing happened in time".
            if let Some(end) = state.terminal.clone() {
                return Err(WaitEnd::Ended(end));
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(WaitEnd::Deadline { timeout });
            }
            let (guard, _) = self
                .inner
                .signal
                .wait_timeout(state, deadline - now)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state = guard;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;

    /// A generous bound. Every passing test here must finish far inside it;
    /// a test that only passes by consuming it has proved nothing.
    const GENEROUS: Duration = Duration::from_secs(30);

    fn response(id: Value) -> Value {
        json!({"jsonrpc": "2.0", "id": id, "result": {"ok": true}})
    }

    /// Mirrors the production matcher in `UxClient::wait_for_response`: exact
    /// JSON equality against the numeric id, hoisted out of the scan.
    fn select_response_id(id: u64) -> impl FnMut(&InboxSnapshot) -> Option<ObservationId> {
        let wanted = Value::from(id);
        move |snapshot| {
            snapshot
                .responses()
                .iter()
                .find(|(_, value)| value.get("id") == Some(&wanted))
                .map(|(observation, _)| *observation)
        }
    }

    #[test]
    fn response_already_buffered_returns_without_touching_the_deadline() -> anyhow::Result<()> {
        let inbox = Inbox::new();
        inbox.push_response(response(json!(7)));

        let started = Instant::now();
        let matched = inbox.wait_for(GENEROUS, select_response_id(7));

        anyhow::ensure!(matched.is_ok(), "buffered response must match immediately");
        anyhow::ensure!(
            started.elapsed() < Duration::from_secs(1),
            "a buffered response must not depend on the deadline, took {:?}",
            started.elapsed()
        );
        Ok(())
    }

    /// The lost-wakeup falsifier: the observation is published *while the
    /// predicate is running outside the lock*, i.e. in the exact window a
    /// register-then-check design would miss.
    #[test]
    fn observation_published_during_predicate_evaluation_is_not_missed() -> anyhow::Result<()> {
        let inbox = Inbox::new();
        let publisher = inbox.clone();
        let evaluations = AtomicUsize::new(0);

        let started = Instant::now();
        let matched = inbox.wait_for(GENEROUS, |snapshot| {
            // On the first evaluation the buffer is empty. Publish from another
            // thread and join it *before* returning None, so the push is
            // guaranteed to land inside the predicate window.
            if evaluations.fetch_add(1, Ordering::SeqCst) == 0 {
                let publisher = publisher.clone();
                let handle = thread::spawn(move || publisher.push_response(response(json!(1))));
                let _ = handle.join();
            }
            select_response_id(1)(snapshot)
        });

        anyhow::ensure!(
            matched.is_ok(),
            "observation arriving inside the predicate window must be seen"
        );
        anyhow::ensure!(
            started.elapsed() < Duration::from_secs(1),
            "a lost wakeup would have burned the whole deadline, took {:?}",
            started.elapsed()
        );
        Ok(())
    }

    #[test]
    fn unrelated_traffic_cannot_satisfy_or_consume_a_wait() -> anyhow::Result<()> {
        let inbox = Inbox::new();
        let publisher = inbox.clone();
        let noise = thread::spawn(move || {
            for _ in 0..25 {
                publisher.push_event(json!({"method": "window/logMessage"}));
                publisher.push_response(response(json!(999)));
            }
            publisher.push_response(response(json!(42)));
        });

        let matched = inbox.wait_for(GENEROUS, select_response_id(42));
        let _ = noise.join();

        let Ok(observation) = matched else {
            anyhow::bail!("the awaited response was published");
        };
        let taken = inbox.take_response(observation);
        anyhow::ensure!(
            taken.as_ref().and_then(|value| value.get("id")) == Some(&json!(42)),
            "awaited response must retain numeric id 42, got {taken:?}"
        );

        // The unrelated responses were woken past, not consumed.
        let remaining = inbox.snapshot().responses().len();
        anyhow::ensure!(remaining == 25, "unrelated responses must survive another waiter's match");
        Ok(())
    }

    #[test]
    fn numeric_and_string_ids_are_distinct_subjects() -> anyhow::Result<()> {
        let inbox = Inbox::new();
        inbox.push_response(response(json!("1")));

        let matched = inbox.wait_for(Duration::from_millis(150), select_response_id(1));

        anyhow::ensure!(
            matches!(matched, Err(WaitEnd::Deadline { .. })),
            "string id \"1\" must not complete a wait for numeric id 1, got {matched:?}"
        );
        Ok(())
    }

    #[test]
    fn transport_failure_wakes_a_held_waiter_before_its_deadline() -> anyhow::Result<()> {
        let inbox = Inbox::new();
        let closer = inbox.clone();
        thread::spawn(move || {
            closer.close(StreamEnd::TransportFailure {
                detail: "invalid Content-Length header".to_string(),
            });
        });

        let started = Instant::now();
        let matched = inbox.wait_for(GENEROUS, select_response_id(1));

        let Err(WaitEnd::Ended(StreamEnd::TransportFailure { detail })) = matched else {
            anyhow::bail!("expected a typed transport failure, got {matched:?}");
        };
        anyhow::ensure!(
            detail.contains("Content-Length"),
            "failure must carry exact evidence: {detail}"
        );
        anyhow::ensure!(
            started.elapsed() < Duration::from_secs(1),
            "transport failure must not be reported only after the timeout, took {:?}",
            started.elapsed()
        );
        Ok(())
    }

    #[test]
    fn orderly_shutdown_is_not_reported_as_transport_corruption() -> anyhow::Result<()> {
        let inbox = Inbox::new();
        let closer = inbox.clone();
        thread::spawn(move || closer.close(StreamEnd::ServerClosed));

        let matched = inbox.wait_for(GENEROUS, select_response_id(1));

        anyhow::ensure!(
            matches!(matched, Err(WaitEnd::Ended(StreamEnd::ServerClosed))),
            "expected an orderly stream end, got {matched:?}"
        );
        Ok(())
    }

    /// A response published immediately before the close must still be
    /// observed: shutdown must not race ahead of the last message.
    #[test]
    fn a_response_published_before_the_close_still_matches() -> anyhow::Result<()> {
        let inbox = Inbox::new();
        inbox.push_response(response(json!(5)));
        inbox.close(StreamEnd::ServerClosed);

        let matched = inbox.wait_for(GENEROUS, select_response_id(5));

        anyhow::ensure!(matched.is_ok(), "the last response before shutdown must not be lost");
        Ok(())
    }

    #[test]
    fn deadline_is_reported_only_while_the_stream_is_live() -> anyhow::Result<()> {
        let inbox = Inbox::new();
        let started = Instant::now();

        let matched = inbox.wait_for(Duration::from_millis(120), select_response_id(1));

        anyhow::ensure!(
            matches!(matched, Err(WaitEnd::Deadline { .. })),
            "a live but silent stream must report the deadline, got {matched:?}"
        );
        anyhow::ensure!(
            started.elapsed() >= Duration::from_millis(100),
            "the deadline is an outer bound and must actually be honoured"
        );
        Ok(())
    }

    /// Traffic that never stops must not outrun the bound.
    ///
    /// Every observation advances the sequence, and the lost-wakeup guard
    /// returns immediately when it has — so a waiter fed a fresh unrelated
    /// observation on each evaluation never blocks. Without a deadline check
    /// on that path this spins forever and the "bounded" wait is unbounded.
    #[test]
    fn continuous_unrelated_traffic_cannot_outrun_the_deadline() -> anyhow::Result<()> {
        let inbox = Inbox::new();
        let publisher = inbox.clone();

        let started = Instant::now();
        let matched = inbox.wait_for(Duration::from_millis(150), |_snapshot| {
            // Publish from inside the predicate, so the sequence has always
            // moved by the time the waiter would otherwise block.
            publisher.push_event(json!({"method": "window/logMessage"}));
            None::<()>
        });

        anyhow::ensure!(
            matches!(matched, Err(WaitEnd::Deadline { .. })),
            "a never-matching wait under constant traffic must report its bound, got {matched:?}"
        );
        anyhow::ensure!(
            started.elapsed() < Duration::from_secs(5),
            "the wait must honour its 150ms bound; took {:?}",
            started.elapsed()
        );
        Ok(())
    }

    /// The deadline boundary must not discard a match the predicate itself
    /// created.
    ///
    /// With an already-expired bound, the first evaluation publishes the
    /// awaited response *and* closes the stream, then reports no match. A
    /// terminal return at that point would say `Ended` while the match sits
    /// buffered — contradicting the documented match-before-stream-end
    /// precedence. Zero timeout makes this deterministic: no sleeping, no
    /// racing, the boundary is hit on the very first iteration.
    #[test]
    fn a_match_published_inside_the_predicate_window_wins_at_the_deadline() -> anyhow::Result<()> {
        let inbox = Inbox::new();
        let publisher = inbox.clone();
        let mut evaluations = 0_u32;

        let matched = inbox.wait_for(Duration::ZERO, |snapshot| {
            evaluations += 1;
            if evaluations == 1 {
                // Land the match and the stream end inside this predicate's
                // own window, after the snapshot it is looking at was taken.
                publisher.push_response(response(json!(11)));
                publisher.close(StreamEnd::ServerClosed);
            }
            select_response_id(11)(snapshot)
        });

        anyhow::ensure!(
            matched.is_ok(),
            "a match buffered during the predicate window must win over the \
             terminal outcome at the deadline, got {matched:?}"
        );
        anyhow::ensure!(
            evaluations == 2,
            "exactly one extra evaluation is expected at the boundary"
        );
        Ok(())
    }

    /// The boundary recheck must not become an escape hatch: with nothing new
    /// buffered, an expired bound still reports terminally.
    #[test]
    fn an_expired_bound_with_no_new_observation_still_reports_terminally() -> anyhow::Result<()> {
        let inbox = Inbox::new();

        let matched = inbox.wait_for(Duration::ZERO, select_response_id(12));

        anyhow::ensure!(
            matches!(matched, Err(WaitEnd::Deadline { .. })),
            "an expired bound over an unchanged inbox must report the deadline, got {matched:?}"
        );
        Ok(())
    }

    /// A stream end still outranks the deadline on the bounded path above.
    #[test]
    fn a_stream_end_outranks_the_deadline_even_under_traffic() -> anyhow::Result<()> {
        let inbox = Inbox::new();
        let publisher = inbox.clone();
        publisher.close(StreamEnd::ServerClosed);

        let matched = inbox.wait_for(Duration::from_millis(120), move |_snapshot| {
            publisher.push_event(json!({"method": "window/logMessage"}));
            None::<()>
        });

        anyhow::ensure!(
            matches!(matched, Err(WaitEnd::Ended(StreamEnd::ServerClosed))),
            "the stream end must still win over the deadline, got {matched:?}"
        );
        Ok(())
    }

    /// Consuming a response is a mutation too, and must advance the sequence
    /// for the same reason.
    #[test]
    fn taking_a_response_advances_the_sequence() -> anyhow::Result<()> {
        let inbox = Inbox::new();
        let observation = inbox.push_response(response(json!(4)));
        let before = inbox.snapshot().seq();

        anyhow::ensure!(
            inbox.take_response(observation).is_some(),
            "published response must be available for its first take"
        );

        anyhow::ensure!(
            inbox.snapshot().seq() != before,
            "a consumed response must advance the sequence"
        );
        Ok(())
    }

    /// A failed take changes nothing, so it must not advance the sequence
    /// either — a spurious bump would wake every waiter for no reason.
    #[test]
    fn a_take_that_removes_nothing_leaves_the_sequence_alone() -> anyhow::Result<()> {
        let inbox = Inbox::new();
        let observation = inbox.push_response(response(json!(4)));
        anyhow::ensure!(
            inbox.take_response(observation).is_some(),
            "published response must be available for its first take"
        );
        let before = inbox.snapshot().seq();

        anyhow::ensure!(
            inbox.take_response(observation).is_none(),
            "a consumed response must remain absent"
        );

        anyhow::ensure!(
            inbox.snapshot().seq() == before,
            "a no-op take must not advance the sequence"
        );
        Ok(())
    }

    /// Draining is a mutation, so it must advance the sequence and wake
    /// waiters like any other change to a waiter's input.
    #[test]
    fn draining_events_advances_the_sequence() -> anyhow::Result<()> {
        let inbox = Inbox::new();
        inbox.push_event(json!({"method": "window/logMessage"}));
        let before = inbox.snapshot().seq();

        let drained = inbox.drain_events();

        anyhow::ensure!(drained.len() == 1, "expected one drained event, got {drained:?}");
        anyhow::ensure!(inbox.snapshot().seq() != before, "a drain must advance the sequence");
        Ok(())
    }

    #[test]
    fn the_first_recorded_stream_end_wins() -> anyhow::Result<()> {
        let inbox = Inbox::new();
        inbox.close(StreamEnd::TransportFailure { detail: "truncated body".to_string() });
        inbox.close(StreamEnd::ServerClosed);

        anyhow::ensure!(
            matches!(inbox.stream_end(), Some(StreamEnd::TransportFailure { .. })),
            "a later orderly close must not relabel an already classified failure"
        );
        Ok(())
    }

    #[test]
    fn a_consumed_response_is_not_handed_to_a_second_waiter() -> anyhow::Result<()> {
        let inbox = Inbox::new();
        let observation = inbox.push_response(response(json!(3)));

        anyhow::ensure!(inbox.take_response(observation).is_some(), "first take must win");
        anyhow::ensure!(
            inbox.take_response(observation).is_none(),
            "second take must not resurrect it"
        );
        Ok(())
    }

    #[test]
    fn events_are_never_consumed_by_a_response_wait() -> anyhow::Result<()> {
        let inbox = Inbox::new();
        inbox.push_event(json!({"method": "window/showMessage"}));
        inbox.push_response(response(json!(8)));

        let Ok(observation) = inbox.wait_for(GENEROUS, select_response_id(8)) else {
            anyhow::bail!("the response was published");
        };
        let _ = inbox.take_response(observation);

        anyhow::ensure!(
            inbox.snapshot().events().len() == 1,
            "events must survive a response wait"
        );
        Ok(())
    }

    /// The predicate must not run while the inbox lock is held, or a predicate
    /// that touches the inbox would deadlock instead of returning.
    #[test]
    fn predicates_run_without_holding_the_inbox_lock() -> anyhow::Result<()> {
        let inbox = Inbox::new();
        inbox.push_response(response(json!(2)));
        let probe = inbox.clone();

        let matched = inbox.wait_for(GENEROUS, move |snapshot| {
            // A lock held across the predicate would wedge this call forever.
            let _ = probe.snapshot();
            select_response_id(2)(snapshot)
        });

        anyhow::ensure!(matched.is_ok(), "a predicate that reads the inbox must not deadlock");
        Ok(())
    }
}
