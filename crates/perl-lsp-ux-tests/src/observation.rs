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
        let mut state = self.lock();
        let position = state.responses.iter().position(|(candidate, _)| *candidate == id)?;
        state.responses.remove(position).map(|(_, value)| value)
    }

    /// Drain every buffered event, leaving the responses untouched.
    pub fn drain_events(&self) -> Vec<Value> {
        let mut state = self.lock();
        state.events.drain(..).map(|(_, value)| value).collect()
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
        mut select: impl FnMut(&InboxSnapshot) -> Option<T>,
    ) -> Result<T, WaitEnd> {
        let deadline = Instant::now().checked_add(timeout).unwrap_or_else(Instant::now);
        loop {
            let snapshot = self.snapshot();
            if let Some(matched) = select(&snapshot) {
                return Ok(matched);
            }
            self.wait_past(snapshot.seq, deadline, timeout)?;
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
                .find(|(_, value)| value["id"] == wanted)
                .map(|(observation, _)| *observation)
        }
    }

    #[test]
    fn response_already_buffered_returns_without_touching_the_deadline() {
        let inbox = Inbox::new();
        inbox.push_response(response(json!(7)));

        let started = Instant::now();
        let matched = inbox.wait_for(GENEROUS, select_response_id(7));

        assert!(matched.is_ok(), "buffered response must match immediately");
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "a buffered response must not depend on the deadline, took {:?}",
            started.elapsed()
        );
    }

    /// The lost-wakeup falsifier: the observation is published *while the
    /// predicate is running outside the lock*, i.e. in the exact window a
    /// register-then-check design would miss.
    #[test]
    fn observation_published_during_predicate_evaluation_is_not_missed() {
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

        assert!(matched.is_ok(), "observation arriving inside the predicate window must be seen");
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "a lost wakeup would have burned the whole deadline, took {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn unrelated_traffic_cannot_satisfy_or_consume_a_wait() {
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
            unreachable!("the awaited response was published");
        };
        let taken = inbox.take_response(observation);
        assert_eq!(taken.map(|value| value["id"].clone()), Some(json!(42)));

        // The unrelated responses were woken past, not consumed.
        let remaining = inbox.snapshot().responses().len();
        assert_eq!(remaining, 25, "unrelated responses must survive another waiter's match");
    }

    #[test]
    fn numeric_and_string_ids_are_distinct_subjects() {
        let inbox = Inbox::new();
        inbox.push_response(response(json!("1")));

        let matched = inbox.wait_for(Duration::from_millis(150), select_response_id(1));

        assert!(
            matches!(matched, Err(WaitEnd::Deadline { .. })),
            "string id \"1\" must not complete a wait for numeric id 1, got {matched:?}"
        );
    }

    #[test]
    fn transport_failure_wakes_a_held_waiter_before_its_deadline() {
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
            unreachable!("expected a typed transport failure, got {matched:?}");
        };
        assert!(detail.contains("Content-Length"), "failure must carry exact evidence: {detail}");
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "transport failure must not be reported only after the timeout, took {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn orderly_shutdown_is_not_reported_as_transport_corruption() {
        let inbox = Inbox::new();
        let closer = inbox.clone();
        thread::spawn(move || closer.close(StreamEnd::ServerClosed));

        let matched = inbox.wait_for(GENEROUS, select_response_id(1));

        assert!(
            matches!(matched, Err(WaitEnd::Ended(StreamEnd::ServerClosed))),
            "expected an orderly stream end, got {matched:?}"
        );
    }

    /// A response published immediately before the close must still be
    /// observed: shutdown must not race ahead of the last message.
    #[test]
    fn a_response_published_before_the_close_still_matches() {
        let inbox = Inbox::new();
        inbox.push_response(response(json!(5)));
        inbox.close(StreamEnd::ServerClosed);

        let matched = inbox.wait_for(GENEROUS, select_response_id(5));

        assert!(matched.is_ok(), "the last response before shutdown must not be lost");
    }

    #[test]
    fn deadline_is_reported_only_while_the_stream_is_live() {
        let inbox = Inbox::new();
        let started = Instant::now();

        let matched = inbox.wait_for(Duration::from_millis(120), select_response_id(1));

        assert!(
            matches!(matched, Err(WaitEnd::Deadline { .. })),
            "a live but silent stream must report the deadline, got {matched:?}"
        );
        assert!(
            started.elapsed() >= Duration::from_millis(100),
            "the deadline is an outer bound and must actually be honoured"
        );
    }

    #[test]
    fn the_first_recorded_stream_end_wins() {
        let inbox = Inbox::new();
        inbox.close(StreamEnd::TransportFailure { detail: "truncated body".to_string() });
        inbox.close(StreamEnd::ServerClosed);

        assert!(
            matches!(inbox.stream_end(), Some(StreamEnd::TransportFailure { .. })),
            "a later orderly close must not relabel an already classified failure"
        );
    }

    #[test]
    fn a_consumed_response_is_not_handed_to_a_second_waiter() {
        let inbox = Inbox::new();
        let observation = inbox.push_response(response(json!(3)));

        assert!(inbox.take_response(observation).is_some(), "first take must win");
        assert!(inbox.take_response(observation).is_none(), "second take must not resurrect it");
    }

    #[test]
    fn events_are_never_consumed_by_a_response_wait() {
        let inbox = Inbox::new();
        inbox.push_event(json!({"method": "window/showMessage"}));
        inbox.push_response(response(json!(8)));

        let Ok(observation) = inbox.wait_for(GENEROUS, select_response_id(8)) else {
            unreachable!("the response was published");
        };
        let _ = inbox.take_response(observation);

        assert_eq!(inbox.snapshot().events().len(), 1, "events must survive a response wait");
    }

    /// The predicate must not run while the inbox lock is held, or a predicate
    /// that touches the inbox would deadlock instead of returning.
    #[test]
    fn predicates_run_without_holding_the_inbox_lock() {
        let inbox = Inbox::new();
        inbox.push_response(response(json!(2)));
        let probe = inbox.clone();

        let matched = inbox.wait_for(GENEROUS, move |snapshot| {
            // A lock held across the predicate would wedge this call forever.
            let _ = probe.snapshot();
            select_response_id(2)(snapshot)
        });

        assert!(matched.is_ok(), "a predicate that reads the inbox must not deadlock");
    }
}
