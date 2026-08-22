//! Bounded deterministic coalescer for file-watcher change notifications.
//!
//! Coalesces rapid `workspace/didChangeWatchedFiles` subjects into ordered,
//! size-bounded batches and hands each batch to a caller-supplied sink after a
//! quiet period. Continuous rescheduling cannot defer a subject past its
//! maximum-latency bound.
//!
//! Contract owned here (#8064, prepared-queue stage):
//!
//! - bounded intake: distinct pending subjects are capped, and retained heap
//!   entries are capped at twice that (a notification storm against
//!   still-pending subjects during a slow-callback stall cannot grow the heap
//!   without bound); admission beyond either cap resolves to
//!   [`WatcherAdmission::Overflowed`] instead of growing memory or dropping
//!   silently;
//! - typed admission: every attempt resolves to [`WatcherAdmission`] — accepted,
//!   coalesced, overflowed, worker-unavailable, or shut-down. Spawn failure and
//!   saturation are therefore visible to callers instead of masquerading as
//!   successful queueing behind a log line;
//! - deterministic order and membership: due subjects are emitted sorted by
//!   (deadline, URI) regardless of arrival interleaving or hash iteration
//!   order; when more subjects share a deadline than fit in one batch,
//!   truncation happens after the full sort, so batch membership is also
//!   interleaving-independent;
//! - quiet plus maximum latency: repeated schedules extend only the quiet
//!   deadline; a subject fires at most [`MAX_LATENCY_INTERVALS`] windows after
//!   first admission, so churn cannot starve publication;
//! - truthful pressure: pending and active counts stay observable through the
//!   runtime snapshot — a batch is counted active AT HANDOFF (atomically with
//!   leaving pending), so pending→active never reports zero total watcher
//!   work while batches wait for the dispatcher;
//! - joinable shutdown: both workers stop, pending subjects drain as chunked
//!   sorted batches delivered only while the callback closure still upgrades,
//!   and both threads join before teardown completes — except a worker whose
//!   own thread triggered the teardown (last-owner Drop from inside the
//!   callback), which detaches instead of self-joining and finishes
//!   naturally. In the production wiring the sole closure weakly captures the
//!   dropping `LspServer`, so teardown discards pending work instead of
//!   publishing post-shutdown;
//! - degraded dispatcher: if the callback closure panics mid-dispatch,
//!   admissions immediately route to [`WatcherAdmission::Unavailable`], all
//!   previously accepted-but-unprocessed work (in-flight batch, queued
//!   batches, parked subjects) is dropped AND COUNTED in `panic_dropped`
//!   instrumentation rather than silently retained behind phantom pressure,
//!   and pressure reports true zeros. Recovery/delivery-after-recovery
//!   requires the future coordinator (#7893) and worker-ownership train
//!   (#10024); this queue deliberately does not rebuild it;
//! - no semantic authority: this queue never reads files, parses, indexes, or
//!   mutates workspace state. Fired batches re-enter exactly the server entry
//!   point the runtime used before, keeping the #7893/#7088 cutover a sink
//!   swap rather than a redesign.
//!
//! Complexity note: rescheduled subjects leave stale heap entries behind.
//! They are purged lazily at the heap head on each earliest-due evaluation,
//! and the total retained population (live plus not-yet-purged stale) is
//! additionally hard-capped at twice the subject cap — an admission that
//! would push past the cap purges first and is refused if that still binds.
//! Amortized intake stays linearithmic.
//!
//! Terminal create/remove/rename evidence, canonical duplicate-transport
//! equivalence, and root/config/trust/watch generation binding remain owned by
//! #7893/#7088/#10770 and are intentionally not modeled here.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use parking_lot::{Condvar, Mutex, MutexGuard};

const DEFAULT_DEBOUNCE_MS: u64 = 500;
/// A subject fires at most this many quiet windows after first admission even
/// when every later schedule keeps resetting the quiet deadline.
const MAX_LATENCY_INTERVALS: u32 = 10;
const MAX_PENDING_SUBJECTS: usize = 4096;
const MAX_BATCH_SUBJECTS: usize = 512;
const MAX_OUTBOX_BATCHES: usize = 8;

/// Typed outcome of a schedule attempt against the watcher coalescer.
///
/// Every admission path resolves to exactly one of these variants so degraded
/// modes (saturation, dead worker, shutdown) can never masquerade as queued
/// work (#8064 WATCH-Q-002).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatcherAdmission {
    /// First admission of a subject; it now occupies pending capacity.
    Accepted,
    /// Subject was already pending; only its quiet deadline moved.
    Coalesced,
    /// Pending set is saturated; the event was NOT retained.
    Overflowed,
    /// Worker threads never started; nothing can be retained.
    Unavailable,
    /// Shutdown began; late events are refused instead of silently queued.
    ShuttingDown,
}

trait DebounceClock: Send + Sync + 'static {
    fn now_millis(&self) -> u64;

    /// Block on `cv` while holding the state lock until virtual time reaches
    /// `deadline_millis`, waking early on notification so shutdown and new
    /// deadlines are observed promptly.
    fn wait_until(
        &self,
        cv: &Condvar,
        guard: &mut MutexGuard<'_, IntakeState>,
        deadline_millis: u64,
    );
}

struct SystemClock {
    epoch: Instant,
}

impl SystemClock {
    fn new() -> Self {
        Self { epoch: Instant::now() }
    }
}

impl DebounceClock for SystemClock {
    fn now_millis(&self) -> u64 {
        u64::try_from(self.epoch.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    fn wait_until(
        &self,
        cv: &Condvar,
        guard: &mut MutexGuard<'_, IntakeState>,
        deadline_millis: u64,
    ) {
        loop {
            let now = self.now_millis();
            if now >= deadline_millis || guard.shutting_down {
                return;
            }
            let _timed_out = cv.wait_for(guard, Duration::from_millis(deadline_millis - now));
        }
    }
}

#[cfg(test)]
struct ManualClock(Mutex<u64>);

#[cfg(test)]
impl ManualClock {
    fn new() -> Self {
        Self(Mutex::new(0))
    }

    fn advance_millis(&self, millis: u64) {
        *self.0.lock() += millis;
    }
}

#[cfg(test)]
impl DebounceClock for ManualClock {
    fn now_millis(&self) -> u64 {
        *self.0.lock()
    }

    fn wait_until(
        &self,
        cv: &Condvar,
        guard: &mut MutexGuard<'_, IntakeState>,
        deadline_millis: u64,
    ) {
        while *self.0.lock() < deadline_millis && !guard.shutting_down {
            cv.wait(guard);
        }
    }
}

struct SubjectDeadlines {
    quiet_deadline: u64,
    max_deadline: u64,
    seq: u64,
}

struct IntakeState {
    subjects: HashMap<String, SubjectDeadlines>,
    heap: BinaryHeap<Reverse<(u64, u64, String)>>,
    outbox: VecDeque<Vec<String>>,
    next_seq: u64,
    shutting_down: bool,
}

impl IntakeState {
    /// Purge stale heap heads and report the earliest live effective deadline.
    ///
    /// This is the SOLE deadline oracle for the intake worker: any earliest-due
    /// computation must come through here, and every entry touched during the
    /// computation increments `heap_operations` (one per call also bumps
    /// `earliest_due_evaluations`). A regression reimplementing base-style
    /// full scans over the pending set therefore inflates entry touches past
    /// the quadratic-intake test budget — the oracle is mutation-sensitive by
    /// construction, not by convention.
    fn peek_live_deadline(&mut self, stats: &CoalescerStats) -> Option<u64> {
        stats.earliest_due_evaluations.fetch_add(1, Ordering::Relaxed);
        loop {
            let head = match self.heap.peek() {
                Some(Reverse((deadline, seq, uri))) => (*deadline, *seq, uri.clone()),
                None => return None,
            };
            stats.heap_operations.fetch_add(1, Ordering::Relaxed);
            let live = self.subjects.get(&head.2).is_some_and(|d| d.seq == head.1);
            if live {
                return Some(head.0);
            }
            self.heap.pop();
        }
    }

    /// Remove up to `limit` due subjects in deterministic order.
    ///
    /// Ordering is (deadline, uri): deadline-first so earlier windows drain
    /// first, URI as tiebreak so batch MEMBERSHIP is independent of arrival
    /// interleaving even when more than `limit` subjects share a deadline
    /// (#8064 — seq is deliberately not a sort key because it encodes
    /// interleaving). Truncation happens only AFTER the full sort, and
    /// non-emitted dues are restored to pending untouched.
    fn take_due_up_to(&mut self, now: u64, limit: usize, stats: &CoalescerStats) -> Vec<String> {
        let mut due: Vec<(u64, u64, String)> = Vec::new();
        while let Some(deadline) = self.peek_live_deadline(stats) {
            if deadline > now {
                break;
            }
            // Pop the heap entry but keep the subject mapped until we know it
            // is emitted — non-selected dues must survive this pass intact.
            if let Some(entry) = self.heap.pop() {
                stats.heap_operations.fetch_add(1, Ordering::Relaxed);
                due.push(entry.0);
            }
        }
        due.sort_unstable_by(|a, b| (a.0, &a.2).cmp(&(b.0, &b.2)));
        let mut out = Vec::with_capacity(limit.min(due.len()));
        for (index, (deadline, _seq, uri)) in due.into_iter().enumerate() {
            if index < limit {
                self.subjects.remove(&uri);
                out.push(uri);
            } else {
                // Restore: subject is still mapped, so re-arm its original
                // entry (same deadline and seq) for the next pass.
                if let Some(d) = self.subjects.get(&uri) {
                    self.heap.push(Reverse((deadline, d.seq, uri)));
                }
            }
        }
        out
    }

    /// Drop heap entries whose seq no longer matches their subject's current
    /// registration. Called when the retained-entry cap is reached before an
    /// admission would push past it; returns nothing — callers re-check the
    /// resulting length.
    fn purge_stale_entries(&mut self, stats: &CoalescerStats) {
        let retained: Vec<Reverse<(u64, u64, String)>> = std::mem::take(&mut self.heap)
            .into_vec()
            .into_iter()
            .filter(|Reverse((_, seq, uri))| {
                stats.heap_operations.fetch_add(1, Ordering::Relaxed);
                self.subjects.get(uri).is_some_and(|d| d.seq == *seq)
            })
            .collect();
        self.heap = retained.into();
    }
}

struct CoalescerStats {
    pending_subjects: AtomicUsize,
    active_subjects: AtomicUsize,
    high_water_subjects: AtomicUsize,
    admitted_total: AtomicU64,
    coalesced_total: AtomicU64,
    overflowed_total: AtomicU64,
    unavailable_total: AtomicU64,
    rejected_after_shutdown_total: AtomicU64,
    batches_dispatched: AtomicU64,
    heap_operations: AtomicU64,
    earliest_due_evaluations: AtomicU64,
    /// Subjects dropped (not delivered) when the callback panicked: in-flight
    /// batch plus anything queued or parked at degradation time.
    panic_dropped_total: AtomicU64,
}

struct Shared {
    state: Mutex<IntakeState>,
    /// Wakes the intake worker for admissions, shutdown, and clock progress.
    intake_cv: Condvar,
    /// Coordinates intake (batch produced / outbox space freed) with the
    /// dispatcher. Both condvars wait on the same state mutex.
    handoff_cv: Condvar,
    /// Set when the callback closure panicked mid-dispatch. Admissions then
    /// report [`WatcherAdmission::Unavailable`] instead of pretending work is
    /// still queueable behind a dead dispatcher.
    sink_panic: AtomicBool,
    clock: Arc<dyn DebounceClock>,
    interval_ms: u64,
    max_latency_ms: u64,
    max_pending_subjects: usize,
    max_batch_subjects: usize,
    /// Hard ceiling on retained heap entries; admissions that would push past
    /// it trigger a stale-entry purge first and are refused as
    /// [`WatcherAdmission::Overflowed`] if the cap still binds. Production
    /// value is twice [`MAX_PENDING_SUBJECTS`] (8192); tests may tighten it.
    max_heap_entries: AtomicUsize,
    stats: CoalescerStats,
}

struct WorkerHandles {
    intake: JoinHandle<()>,
    dispatcher: JoinHandle<()>,
}

#[cfg(any(test, feature = "expose_lsp_test_api"))]
#[derive(Default)]
pub(crate) struct WatcherPressureSnapshot {
    /// Subjects waiting for their deadline; consumed by the runtime snapshot.
    pub(crate) pending_subjects: usize,
    /// Subjects currently inside the callback; consumed by the runtime
    /// snapshot so pending→active transitions never report zero total work.
    pub(crate) active_subjects: usize,
    /// Batches queued for dispatch. Test instrumentation only.
    #[cfg(test)]
    pub(crate) outboxed_batches: usize,
    /// Peak pending-subject count. Test instrumentation only.
    #[cfg(test)]
    pub(crate) high_water_subjects: usize,
    /// Lifetime admission counters. Test instrumentation only.
    #[cfg(test)]
    pub(crate) admitted_total: u64,
    #[cfg(test)]
    pub(crate) coalesced_total: u64,
    #[cfg(test)]
    pub(crate) overflowed_total: u64,
    #[cfg(test)]
    pub(crate) unavailable_total: u64,
    #[cfg(test)]
    pub(crate) rejected_after_shutdown_total: u64,
    #[cfg(test)]
    pub(crate) batches_dispatched: u64,
    /// Heap entries inspected while computing earliest-due or draining due
    /// subjects. Test instrumentation only — this is the quadratic-intake
    /// oracle (see `peek_live_deadline`).
    #[cfg(test)]
    pub(crate) heap_operations: u64,
    /// Number of distinct earliest-due computations. Test instrumentation
    /// only; paired with `heap_operations` so a full-scan regression inside
    /// the sole deadline oracle inflates entry touches past the test budget.
    #[cfg(test)]
    pub(crate) earliest_due_evaluations: u64,
    /// Subjects dropped-and-counted when the callback panicked. Test
    /// instrumentation only.
    #[cfg(test)]
    pub(crate) panic_dropped_total: u64,
    /// Retained (live + stale) heap entries. Test instrumentation only; the
    /// bound is asserted by the notification-storm negative control.
    #[cfg(test)]
    pub(crate) retained_heap_entries: usize,
}

/// Debouncer for file watcher change notifications.
///
/// Accumulates URIs from rapid `workspace/didChangeWatchedFiles` notifications
/// and delivers them as sorted, bounded batches to the callback after a quiet
/// period. Admission outcomes are reported through
/// [`FileWatcherDebouncer::try_schedule`].
pub struct FileWatcherDebouncer {
    shared: Arc<Shared>,
    workers: Mutex<Option<WorkerHandles>>,
    operational: bool,
}

impl FileWatcherDebouncer {
    /// Create a new debouncer with the default window (500ms).
    pub fn new<F>(publish_fn: F) -> Self
    where
        F: Fn(Vec<String>) + Send + 'static,
    {
        Self::with_interval(Duration::from_millis(DEFAULT_DEBOUNCE_MS), publish_fn)
    }

    /// Create a new debouncer with a custom debounce window.
    ///
    /// The maximum-latency horizon is [`MAX_LATENCY_INTERVALS`] windows, so a
    /// continuously rescheduled subject still fires within it.
    pub fn with_interval<F>(interval: Duration, publish_fn: F) -> Self
    where
        F: Fn(Vec<String>) + Send + 'static,
    {
        Self::build(
            interval,
            publish_fn,
            Arc::new(SystemClock::new()),
            MAX_PENDING_SUBJECTS,
            MAX_BATCH_SUBJECTS,
        )
    }

    fn build<F>(
        interval: Duration,
        publish_fn: F,
        clock: Arc<dyn DebounceClock>,
        max_pending_subjects: usize,
        max_batch_subjects: usize,
    ) -> Self
    where
        F: Fn(Vec<String>) + Send + 'static,
    {
        let interval_ms = u64::try_from(interval.as_millis()).unwrap_or(DEFAULT_DEBOUNCE_MS);
        let shared = make_shared(clock, interval_ms, max_pending_subjects, max_batch_subjects);

        // One worker owns scheduling (never touches the callback); one owns
        // dispatch (the only place the callback runs, moved in by value so a
        // plain `Send` closure bound suffices), so a long batch can never
        // block observation of newer events.
        let intake_shared = Arc::clone(&shared);
        let intake_handle = thread::Builder::new()
            .name("file-watcher-intake".into())
            .spawn(move || intake_loop(intake_shared));

        let dispatch_shared = Arc::clone(&shared);
        let dispatch_handle = thread::Builder::new()
            .name("file-watcher-dispatch".into())
            .spawn(move || dispatch_loop(dispatch_shared, publish_fn));

        Self::assemble(shared, intake_handle, dispatch_handle)
    }

    /// Common assembly for both fully-spawned and degraded starts so the real
    /// partial-spawn branch (one worker up, one failed) is exercised by tests.
    fn assemble(
        shared: Arc<Shared>,
        intake: std::io::Result<JoinHandle<()>>,
        dispatcher: std::io::Result<JoinHandle<()>>,
    ) -> Self {
        let workers = match (intake, dispatcher) {
            (Ok(intake), Ok(dispatcher)) => Some(WorkerHandles { intake, dispatcher }),
            (intake, dispatcher) => {
                tracing::error!(
                    "file watcher debounce worker spawn failed; scheduling \
                     reports unavailable instead of silently absorbing events"
                );
                halt_workers(&shared, intake.ok(), dispatcher.ok());
                None
            }
        };

        let operational = workers.is_some();
        Self { shared, workers: Mutex::new(workers), operational }
    }

    #[cfg(test)]
    fn failed_start_for_test(clock: Arc<dyn DebounceClock>) -> Self {
        let injected = Err(std::io::Error::other("injected worker spawn failure"));
        Self::assemble(
            make_shared(clock, DEFAULT_DEBOUNCE_MS, MAX_PENDING_SUBJECTS, MAX_BATCH_SUBJECTS),
            injected,
            Err(std::io::Error::other("injected worker spawn failure")),
        )
    }

    /// Exercise the real partial-spawn branch: whichever worker is requested
    /// actually spawns; the other side fails, forcing `assemble` through
    /// `halt_workers` to join the survivor.
    #[cfg(test)]
    fn partially_spawned_for_test(spawn_intake: bool, spawn_dispatcher: bool) -> Self {
        const NAME: &str = "file-watcher-partial-test";
        let shared = make_shared(
            Arc::new(SystemClock::new()),
            5_000,
            MAX_PENDING_SUBJECTS,
            MAX_BATCH_SUBJECTS,
        );
        let injected = || Err::<JoinHandle<()>, _>(std::io::Error::other("injected"));
        let intake = if spawn_intake {
            thread::Builder::new().name(NAME.into()).spawn({
                let shared = Arc::clone(&shared);
                move || intake_loop(shared)
            })
        } else {
            injected()
        };
        let dispatcher = if spawn_dispatcher {
            thread::Builder::new().name(NAME.into()).spawn({
                let shared = Arc::clone(&shared);
                move || dispatch_loop(shared, |_uris: Vec<String>| {})
            })
        } else {
            injected()
        };
        Self::assemble(shared, intake, dispatcher)
    }

    /// Unavailable-state debouncer for caller-side admission tests: exercises
    /// the real spawn-failure disposition without exposing private types.
    #[cfg(test)]
    pub(crate) fn unavailable_for_test() -> Self {
        Self::failed_start_for_test(Arc::new(SystemClock::new()))
    }

    /// Real-clock debouncer with a tiny pending cap, for caller-side tests of
    /// the [`WatcherAdmission::Overflowed`] disposition.
    #[cfg(test)]
    pub(crate) fn saturated_for_test<F>(publish_fn: F) -> Self
    where
        F: Fn(Vec<String>) + Send + 'static,
    {
        Self::build(
            // Non-integral minutes keeps the duration-suboptimal-units lint
            // quiet; the window only needs to outlive the test.
            Duration::from_secs(90),
            publish_fn,
            Arc::new(SystemClock::new()),
            1,
            MAX_BATCH_SUBJECTS,
        )
    }

    /// Whether the coalescer can currently accept and deliver work. False
    /// after worker-spawn failure or after the callback closure panicked
    /// mid-dispatch; admissions report [`WatcherAdmission::Unavailable`]
    /// instead of silently absorbing events.
    pub fn is_operational(&self) -> bool {
        self.operational && !self.shared.sink_panic.load(Ordering::SeqCst)
    }

    /// Schedule a URI and observe the typed admission outcome.
    ///
    /// Repeated schedules of a pending subject reset only its quiet deadline;
    /// the maximum-latency deadline fixed at first admission is preserved so
    /// continuous rescheduling cannot starve publication.
    pub fn try_schedule(&self, uri: &str) -> WatcherAdmission {
        if !self.is_operational() {
            self.shared.stats.unavailable_total.fetch_add(1, Ordering::Relaxed);
            return WatcherAdmission::Unavailable;
        }

        let shared = &self.shared;
        let now = shared.clock.now_millis();
        let mut guard = shared.state.lock();

        if guard.shutting_down {
            shared.stats.rejected_after_shutdown_total.fetch_add(1, Ordering::Relaxed);
            return WatcherAdmission::ShuttingDown;
        }

        // Both arms below push exactly one heap entry. Enforce the retained-
        // entry cap BEFORE any deadline/seq mutation: rejecting a coalesce
        // after mutating seq would orphan the subject's older entries and
        // silently prevent it from ever firing.
        if guard.heap.len() >= shared.max_heap_entries.load(Ordering::Relaxed) {
            guard.purge_stale_entries(&shared.stats);
            if guard.heap.len() >= shared.max_heap_entries.load(Ordering::Relaxed) {
                shared.stats.overflowed_total.fetch_add(1, Ordering::Relaxed);
                return WatcherAdmission::Overflowed;
            }
        }

        guard.next_seq = guard.next_seq.wrapping_add(1);
        let seq = guard.next_seq;
        let quiet = now.saturating_add(shared.interval_ms);

        enum IntakeOutcome {
            Coalesced { max_deadline: u64 },
            Admit,
        }

        let outcome = match guard.subjects.get_mut(uri) {
            Some(deadlines) => {
                deadlines.quiet_deadline = quiet;
                deadlines.seq = seq;
                IntakeOutcome::Coalesced { max_deadline: deadlines.max_deadline }
            }
            None => {
                if guard.subjects.len() >= shared.max_pending_subjects {
                    shared.stats.overflowed_total.fetch_add(1, Ordering::Relaxed);
                    return WatcherAdmission::Overflowed;
                }
                IntakeOutcome::Admit
            }
        };

        match outcome {
            IntakeOutcome::Coalesced { max_deadline } => {
                push_heap_entry(&mut guard, uri.to_string(), quiet.min(max_deadline), seq);
                shared.stats.coalesced_total.fetch_add(1, Ordering::Relaxed);
                shared.intake_cv.notify_one();
                WatcherAdmission::Coalesced
            }
            IntakeOutcome::Admit => {
                let max = now.saturating_add(shared.max_latency_ms);
                let effective = quiet.min(max);
                push_heap_entry(&mut guard, uri.to_string(), effective, seq);
                guard.subjects.insert(
                    uri.to_string(),
                    SubjectDeadlines { quiet_deadline: quiet, max_deadline: max, seq },
                );
                let len = guard.subjects.len();
                shared.stats.pending_subjects.store(len, Ordering::SeqCst);
                shared.stats.high_water_subjects.fetch_max(len, Ordering::SeqCst);
                shared.stats.admitted_total.fetch_add(1, Ordering::Relaxed);
                shared.intake_cv.notify_one();
                WatcherAdmission::Accepted
            }
        }
    }

    /// Schedule a URI for debounced batch delivery.
    ///
    /// Fire-and-forget convenience over [`Self::try_schedule`] for in-crate
    /// tests: non-admitted outcomes degrade to debug logs here, which is why
    /// this shim is test-only — production callers must observe admission
    /// through [`Self::try_schedule`].
    #[cfg(test)]
    pub fn schedule(&self, uri: &str) {
        match self.try_schedule(uri) {
            WatcherAdmission::Accepted | WatcherAdmission::Coalesced => {}
            WatcherAdmission::Overflowed => {
                tracing::debug!(uri, "file watcher debounce: pending set saturated");
            }
            WatcherAdmission::Unavailable => {
                tracing::debug!(uri, "file watcher debounce: worker unavailable");
            }
            WatcherAdmission::ShuttingDown => {
                tracing::debug!(uri, "file watcher debounce: shutting down");
            }
        }
    }

    /// Number of unique URIs currently waiting in the debounce window.
    pub fn pending_uris(&self) -> usize {
        self.shared.stats.pending_subjects.load(Ordering::SeqCst)
    }

    #[cfg(any(test, feature = "expose_lsp_test_api"))]
    pub(crate) fn pressure(&self) -> WatcherPressureSnapshot {
        let stats = &self.shared.stats;
        let mut snapshot = WatcherPressureSnapshot {
            pending_subjects: stats.pending_subjects.load(Ordering::SeqCst),
            active_subjects: stats.active_subjects.load(Ordering::SeqCst),
            ..WatcherPressureSnapshot::default()
        };
        #[cfg(test)]
        {
            let state = self.shared.state.lock();
            snapshot.outboxed_batches = state.outbox.len();
            snapshot.retained_heap_entries = state.heap.len();
            drop(state);
            snapshot.high_water_subjects = stats.high_water_subjects.load(Ordering::SeqCst);
            snapshot.admitted_total = stats.admitted_total.load(Ordering::Relaxed);
            snapshot.coalesced_total = stats.coalesced_total.load(Ordering::Relaxed);
            snapshot.overflowed_total = stats.overflowed_total.load(Ordering::Relaxed);
            snapshot.unavailable_total = stats.unavailable_total.load(Ordering::Relaxed);
            snapshot.rejected_after_shutdown_total =
                stats.rejected_after_shutdown_total.load(Ordering::Relaxed);
            snapshot.batches_dispatched = stats.batches_dispatched.load(Ordering::Relaxed);
            snapshot.heap_operations = stats.heap_operations.load(Ordering::Relaxed);
            snapshot.earliest_due_evaluations =
                stats.earliest_due_evaluations.load(Ordering::Relaxed);
            snapshot.panic_dropped_total = stats.panic_dropped_total.load(Ordering::Relaxed);
        }
        snapshot
    }

    /// Stop intake and join both workers. Idempotent; invoked from `Drop`.
    ///
    /// Actual teardown policy: intake stops first; whatever is still pending
    /// is chunked into sorted batches of at most [`MAX_BATCH_SUBJECTS`]
    /// subjects (so a large backlog drains as multiple batch deliveries, not
    /// one) and handed to the dispatcher, which delivers only while the
    /// callback closure still upgrades — i.e. only while something independent
    /// of this queue keeps the closure's captured state alive. In the
    /// production wiring the sole closure weakly captures the dropping
    /// `LspServer`, so at server teardown the upgrade fails and pending work
    /// is discarded: nothing publishes after shutdown. The flush bypasses the
    /// outbox cap because total retained work is already bounded by the
    /// pending-subject cap, so memory stays bounded.
    pub(crate) fn shutdown_now(&self) {
        let handles = {
            let mut workers = self.workers.lock();
            let Some(handles) = workers.take() else {
                return;
            };
            let shared = &self.shared;
            {
                let mut guard = shared.state.lock();
                guard.shutting_down = true;
                let mut remaining: Vec<String> = guard.subjects.keys().cloned().collect();
                remaining.sort_unstable();
                for chunk in remaining.chunks(shared.max_batch_subjects) {
                    guard.outbox.push_back(chunk.to_vec());
                }
                guard.subjects.clear();
                guard.heap.clear();
                shared.stats.pending_subjects.store(0, Ordering::SeqCst);
                shared.intake_cv.notify_all();
                shared.handoff_cv.notify_all();
            }
            handles
        };
        join_worker(handles.intake);
        join_worker(handles.dispatcher);
    }
}

impl Drop for FileWatcherDebouncer {
    fn drop(&mut self) {
        self.shutdown_now();
    }
}

fn make_shared(
    clock: Arc<dyn DebounceClock>,
    interval_ms: u64,
    max_pending_subjects: usize,
    max_batch_subjects: usize,
) -> Arc<Shared> {
    let max_latency_ms = interval_ms.saturating_mul(u64::from(MAX_LATENCY_INTERVALS));
    Arc::new(Shared {
        state: Mutex::new(IntakeState {
            subjects: HashMap::new(),
            heap: BinaryHeap::new(),
            outbox: VecDeque::new(),
            next_seq: 0,
            shutting_down: false,
        }),
        intake_cv: Condvar::new(),
        handoff_cv: Condvar::new(),
        sink_panic: AtomicBool::new(false),
        clock,
        interval_ms,
        max_latency_ms,
        max_pending_subjects,
        max_batch_subjects,
        max_heap_entries: AtomicUsize::new(max_pending_subjects.saturating_mul(2)),
        stats: CoalescerStats {
            pending_subjects: AtomicUsize::new(0),
            active_subjects: AtomicUsize::new(0),
            high_water_subjects: AtomicUsize::new(0),
            admitted_total: AtomicU64::new(0),
            coalesced_total: AtomicU64::new(0),
            overflowed_total: AtomicU64::new(0),
            unavailable_total: AtomicU64::new(0),
            rejected_after_shutdown_total: AtomicU64::new(0),
            batches_dispatched: AtomicU64::new(0),
            heap_operations: AtomicU64::new(0),
            earliest_due_evaluations: AtomicU64::new(0),
            panic_dropped_total: AtomicU64::new(0),
        },
    })
}

fn push_heap_entry(guard: &mut IntakeState, uri: String, effective_deadline: u64, seq: u64) {
    guard.heap.push(Reverse((effective_deadline, seq, uri)));
}

fn join_worker(handle: JoinHandle<()>) {
    if handle.thread().id() == thread::current().id() {
        // Self-join would panic ("a thread cannot join itself"). This happens
        // when the callback's upgraded state is the last strong owner of the
        // queue: its Drop runs shutdown_now ON the dispatcher thread. Dropping
        // the handle detaches; this worker simply finishes naturally once its
        // own call stack returns.
        drop(handle);
        return;
    }
    let _ = handle.join();
}

fn halt_workers(
    shared: &Shared,
    intake: Option<JoinHandle<()>>,
    dispatcher: Option<JoinHandle<()>>,
) {
    {
        let mut guard = shared.state.lock();
        guard.shutting_down = true;
        shared.intake_cv.notify_all();
        shared.handoff_cv.notify_all();
    }
    if let Some(handle) = intake {
        join_worker(handle);
    }
    if let Some(handle) = dispatcher {
        join_worker(handle);
    }
}

fn intake_loop(shared: Arc<Shared>) {
    let clock = Arc::clone(&shared.clock);
    let mut guard = shared.state.lock();
    loop {
        if guard.shutting_down {
            return;
        }
        if shared.sink_panic.load(Ordering::SeqCst) {
            // The dispatcher died on a panicking callback; admissions already
            // report Unavailable. Park until shutdown reclaims this worker.
            shared.handoff_cv.wait(&mut guard);
            continue;
        }
        if let Some(deadline) = guard.peek_live_deadline(&shared.stats) {
            let now = clock.now_millis();
            if deadline <= now {
                if guard.outbox.len() >= MAX_OUTBOX_BATCHES {
                    // Backpressure: leave remaining due subjects pending until
                    // the dispatcher frees outbox capacity.
                    shared.handoff_cv.wait(&mut guard);
                    continue;
                }
                let due = guard.take_due_up_to(now, shared.max_batch_subjects, &shared.stats);
                if !due.is_empty() {
                    // Active accounting moves AT THE HANDOFF (issue req. 9):
                    // incrementing before the pending store means a queued
                    // batch is never invisible — pending→active never reports
                    // zero total work while a batch waits for the dispatcher.
                    shared.stats.active_subjects.fetch_add(due.len(), Ordering::SeqCst);
                    guard.outbox.push_back(due);
                    shared.stats.pending_subjects.store(guard.subjects.len(), Ordering::SeqCst);
                    shared.handoff_cv.notify_one();
                }
                continue;
            }
            clock.wait_until(&shared.intake_cv, &mut guard, deadline);
            continue;
        }
        shared.intake_cv.wait(&mut guard);
    }
}

fn dispatch_loop<F>(shared: Arc<Shared>, sink: F)
where
    F: Fn(Vec<String>) + Send + 'static,
{
    let mut guard = shared.state.lock();
    loop {
        match guard.outbox.pop_front() {
            Some(batch) => {
                // Active was already counted at handoff (intake side); popping
                // changes no accounting. Decrement happens after the sink
                // completes, success or failure.
                drop(guard);
                let batch_len = batch.len();
                // A panicking callback must not masquerade as delivered work:
                // mark the coalescer degraded so admissions route to the
                // unavailable disposition instead of apparent success.
                let outcome =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sink(batch)));
                shared.stats.active_subjects.fetch_sub(batch_len, Ordering::SeqCst);
                shared.stats.batches_dispatched.fetch_add(1, Ordering::Relaxed);
                if outcome.is_err() {
                    tracing::error!(
                        "file watcher debounce callback panicked; \
                         scheduling reports unavailable until restart"
                    );
                    // Flag FIRST: intake checks it at loop top and stops
                    // firing, so the accounting drain below races nothing.
                    shared.sink_panic.store(true, Ordering::SeqCst);
                    // Truthful degradation (#8064): accepted-but-unprocessed
                    // work is dropped and COUNTED, never silently retained.
                    // The in-flight batch left this loop via pop_front, so its
                    // subjects are neither pending nor delivered — count them
                    // here alongside anything queued or still parked.
                    let mut dropped = batch_len;
                    {
                        let mut state_guard = shared.state.lock();
                        for queued in state_guard.outbox.drain(..) {
                            dropped += queued.len();
                        }
                        dropped += state_guard.subjects.len();
                        state_guard.subjects.clear();
                        state_guard.heap.clear();
                        // Dropped queued batches were already counted active
                        // at handoff; their decrement would only happen
                        // post-sink, which never runs for them.
                        shared.stats.active_subjects.store(0, Ordering::SeqCst);
                        shared.stats.pending_subjects.store(0, Ordering::SeqCst);
                        shared.stats.panic_dropped_total.store(dropped as u64, Ordering::SeqCst);
                    }
                    {
                        let _relock = shared.state.lock();
                        shared.handoff_cv.notify_all();
                    }
                    return;
                }
                guard = shared.state.lock();
                shared.handoff_cv.notify_all();
            }
            None => {
                if guard.shutting_down {
                    return;
                }
                shared.handoff_cv.wait(&mut guard);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Harness {
        debouncer: FileWatcherDebouncer,
        shared: Arc<Shared>,
        clock: Arc<ManualClock>,
    }

    impl Harness {
        fn with_sink<F>(sink: F) -> Self
        where
            F: Fn(Vec<String>) + Send + 'static,
        {
            Self::with_caps(sink, MAX_PENDING_SUBJECTS, MAX_BATCH_SUBJECTS)
        }

        fn with_caps<F>(sink: F, max_pending_subjects: usize, max_batch_subjects: usize) -> Self
        where
            F: Fn(Vec<String>) + Send + 'static,
        {
            let clock = Arc::new(ManualClock::new());
            let debouncer = FileWatcherDebouncer::build(
                Duration::from_millis(100),
                sink,
                Arc::clone(&clock) as Arc<dyn DebounceClock>,
                max_pending_subjects,
                max_batch_subjects,
            );
            let shared = Arc::clone(&debouncer.shared);
            Self { debouncer, shared, clock }
        }

        fn advance(&self, millis: u64) {
            self.clock.advance_millis(millis);
            self.shared.intake_cv.notify_all();
            self.shared.handoff_cv.notify_all();
        }

        fn wait_for(&self, predicate: impl Fn() -> bool, label: &str) {
            let deadline = Instant::now() + Duration::from_secs(10);
            while !predicate() {
                assert!(Instant::now() < deadline, "timed out waiting for {label}");
                std::thread::sleep(Duration::from_millis(2));
            }
        }

        fn workers_joined(&self) -> bool {
            self.debouncer.workers.lock().is_none()
        }
    }

    impl FileWatcherDebouncer {
        fn workers_lock_is_empty(&self) -> bool {
            self.workers.lock().is_none()
        }
    }

    fn flatten(batches: &[Vec<String>]) -> Vec<String> {
        let mut all: Vec<String> = batches.iter().flat_map(|b| b.iter().cloned()).collect();
        all.sort_unstable();
        all
    }

    fn collect_delivered() -> (Arc<Mutex<Vec<Vec<String>>>>, impl Fn(Vec<String>) + Send + Sync) {
        let delivered: Arc<Mutex<Vec<Vec<String>>>> = Arc::new(Mutex::new(Vec::new()));
        let sink_delivered = Arc::clone(&delivered);
        let sink = move |uris: Vec<String>| sink_delivered.lock().push(uris);
        (delivered, sink)
    }

    #[test]
    fn file_watcher_debouncer_burst_admissions_stay_bounded_not_quadratic() {
        let harness = Harness::with_sink(|_uris| {});
        let subjects: Vec<String> = (0..3000u32).map(|i| format!("file:///burst/{i}.pl")).collect();

        for uri in &subjects {
            assert_eq!(harness.debouncer.try_schedule(uri), WatcherAdmission::Accepted);
        }

        let pressure = harness.debouncer.pressure();
        assert_eq!(pressure.pending_subjects, 3000);
        assert_eq!(pressure.high_water_subjects, 3000);
        assert_eq!(pressure.admitted_total, 3000);

        harness.advance(101);
        harness.wait_for(|| harness.debouncer.pressure().pending_subjects == 0, "full burst drain");

        // Quadratic intake inspects ~M^2/2 ≈ 4.5M entries across the burst.
        // The oracle counts entry touches inside the SOLE deadline oracle
        // (`peek_live_deadline`), so a regression reimplementing base-style
        // full scans over the pending set — which must still route its
        // earliest-due computation through that oracle — inflates the count
        // past this budget. Linearithmic heap behavior plus per-admission
        // notify wakeups stays far below even this generous slack.
        let pressure = harness.debouncer.pressure();
        assert!(
            pressure.heap_operations < 3000 * 60,
            "heap operations {} suggest quadratic intake",
            pressure.heap_operations
        );
        assert!(
            pressure.earliest_due_evaluations <= pressure.heap_operations,
            "evaluations {} exceed touched entries {}; oracle inconsistency",
            pressure.earliest_due_evaluations,
            pressure.heap_operations
        );
        assert_eq!(pressure.overflowed_total, 0);
    }

    #[test]
    fn file_watcher_debouncer_continuous_rescheduling_fires_at_maximum_latency() {
        let (delivered, sink) = collect_delivered();
        let harness = Harness::with_sink(sink);

        // Reset the quiet deadline every window except the final stretch.
        // Tick 0 is the first admission; every later tick coalesces.
        for tick in 0..8 {
            harness.advance(99);
            let expected =
                if tick == 0 { WatcherAdmission::Accepted } else { WatcherAdmission::Coalesced };
            assert_eq!(harness.debouncer.try_schedule("file:///churn.pl"), expected, "tick {tick}");
        }
        assert!(
            delivered.lock().is_empty(),
            "quiet resets must defer firing below maximum latency"
        );

        harness.advance(99);
        harness.advance(802);

        harness.wait_for(|| !delivered.lock().is_empty(), "maximum-latency publication");
        let batches = delivered.lock();
        assert_eq!(batches.len(), 1);
        assert_eq!(flatten(&batches), vec!["file:///churn.pl".to_string()]);
    }

    #[test]
    fn rescheduling_below_maximum_extends_quiet_deadline() {
        let (delivered, sink) = collect_delivered();
        let harness = Harness::with_sink(sink);

        harness.debouncer.try_schedule("file:///a.pl");
        harness.advance(90);
        harness.debouncer.try_schedule("file:///a.pl");
        harness.advance(90);
        assert!(delivered.lock().is_empty(), "reschedule must reset the quiet deadline");

        harness.advance(20);
        harness.wait_for(|| !delivered.lock().is_empty(), "quiet-window publication");
        assert_eq!(delivered.lock().len(), 1);
        assert_eq!(flatten(&delivered.lock()), vec!["file:///a.pl".to_string()]);
    }

    #[test]
    fn duplicate_schedules_coalesce_with_truthful_counts() {
        let (delivered, sink) = collect_delivered();
        let harness = Harness::with_sink(sink);

        // The first schedule admits the subject; the other 24 coalesce.
        assert_eq!(harness.debouncer.try_schedule("file:///dup.pl"), WatcherAdmission::Accepted);
        for _ in 1..25 {
            assert_eq!(
                harness.debouncer.try_schedule("file:///dup.pl"),
                WatcherAdmission::Coalesced
            );
        }

        let pressure = harness.debouncer.pressure();
        assert_eq!(pressure.admitted_total, 1);
        assert_eq!(pressure.coalesced_total, 24);
        assert_eq!(pressure.pending_subjects, 1);

        harness.advance(101);
        harness.wait_for(|| !delivered.lock().is_empty(), "coalesced delivery");
        assert_eq!(delivered.lock().len(), 1);
        assert_eq!(flatten(&delivered.lock()), vec!["file:///dup.pl".to_string()]);
    }

    #[test]
    fn file_watcher_debouncer_batch_order_is_deterministic_regardless_of_arrival_interleaving() {
        let orders: [Vec<usize>; 2] =
            [(0..12usize).collect(), vec![7usize, 2, 11, 0, 9, 4, 1, 10, 5, 3, 8, 6]];

        let mut results = Vec::new();
        for order in &orders {
            let (delivered, sink) = collect_delivered();
            let harness = Harness::with_sink(sink);

            for index in order {
                harness.debouncer.try_schedule(&format!("file:///ord{index}.pl"));
            }
            harness.advance(101);
            harness.wait_for(|| !delivered.lock().is_empty(), "ordered delivery");
            let batches = delivered.lock();
            assert_eq!(batches.len(), 1, "one window emits one batch");
            results.push(batches[0].clone());
        }

        assert_eq!(results[0], results[1]);
        let mut expected: Vec<String> =
            (0..12usize).map(|i| format!("file:///ord{i}.pl")).collect();
        expected.sort();
        assert_eq!(results[0], expected);
    }

    #[test]
    fn oversized_windows_chunk_into_bounded_batches_without_loss() {
        const CHUNK_LIMIT: usize = 128;
        let (delivered, sink) = collect_delivered();
        let harness = Harness::with_caps(sink, MAX_PENDING_SUBJECTS, CHUNK_LIMIT);

        let total = 600usize;
        for i in 0..total {
            assert_eq!(
                harness.debouncer.try_schedule(&format!("file:///chunk{i}.pl")),
                WatcherAdmission::Accepted
            );
        }

        harness.advance(101);
        harness.wait_for(
            || {
                let batches = delivered.lock();
                batches.iter().map(Vec::len).sum::<usize>() == total
            },
            "chunked delivery",
        );

        let batches = delivered.lock();
        assert!(batches.len() >= 2, "expected multiple bounded chunks");
        for batch in batches.iter() {
            assert!(batch.len() <= CHUNK_LIMIT);
        }
        let mut expected: Vec<String> =
            (0..total).map(|i| format!("file:///chunk{i}.pl")).collect();
        expected.sort();
        assert_eq!(flatten(&batches), expected);
    }

    #[test]
    fn file_watcher_debouncer_overflow_is_typed_counted_and_non_silent() {
        let harness = Harness::with_caps(|_uris| {}, 8, MAX_BATCH_SUBJECTS);

        for i in 0..8 {
            assert_eq!(
                harness.debouncer.try_schedule(&format!("file:///cap{i}.pl")),
                WatcherAdmission::Accepted
            );
        }
        assert_eq!(harness.debouncer.try_schedule("file:///over.pl"), WatcherAdmission::Overflowed);
        assert_eq!(
            harness.debouncer.try_schedule("file:///over2.pl"),
            WatcherAdmission::Overflowed
        );

        let pressure = harness.debouncer.pressure();
        assert_eq!(pressure.overflowed_total, 2);
        assert_eq!(pressure.pending_subjects, 8, "rejections must not disturb pending state");

        // An already-pending subject always remains schedulable.
        assert_eq!(harness.debouncer.try_schedule("file:///cap0.pl"), WatcherAdmission::Coalesced);
    }

    #[test]
    fn spawn_failure_reports_unavailable_instead_of_false_success() {
        let clock = Arc::new(ManualClock::new());
        let debouncer = FileWatcherDebouncer::failed_start_for_test(clock);
        assert!(!debouncer.is_operational());
        assert_eq!(debouncer.try_schedule("file:///x.pl"), WatcherAdmission::Unavailable);
        debouncer.schedule("file:///y.pl"); // logs, never panics; also counted
        let pressure = debouncer.pressure();
        assert_eq!(pressure.unavailable_total, 2);
        assert_eq!(pressure.pending_subjects, 0);
        debouncer.shutdown_now(); // idempotent on a failed-start instance
    }

    #[test]
    fn file_watcher_debouncer_shutdown_flushes_one_final_sorted_bounded_flush_then_joins() {
        let (delivered, sink) = collect_delivered();
        let harness = Harness::with_sink(sink);

        for i in 0..40usize {
            harness.debouncer.try_schedule(&format!("file:///flush{i}.pl"));
        }

        harness.debouncer.shutdown_now();

        let batches = delivered.lock();
        assert_eq!(batches.len(), 1, "final flush is one bounded batch");
        let mut expected: Vec<String> =
            (0..40usize).map(|i| format!("file:///flush{i}.pl")).collect();
        expected.sort();
        assert_eq!(batches[0], expected);
        drop(batches);

        assert_eq!(harness.debouncer.pending_uris(), 0);
        let pressure = harness.debouncer.pressure();
        assert_eq!(pressure.outboxed_batches, 0, "outbox drains to empty before join");
        assert!(pressure.batches_dispatched >= 1);
        assert!(harness.workers_joined());
        assert_eq!(
            harness.debouncer.try_schedule("file:///late.pl"),
            WatcherAdmission::ShuttingDown
        );
        assert_eq!(harness.debouncer.pressure().rejected_after_shutdown_total, 1);
        assert_eq!(delivered.lock().len(), 1, "no publication after shutdown");
    }

    #[test]
    fn file_watcher_debouncer_panicking_sink_drops_and_counts_stranded_work() {
        let panic_gate: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
        let trip = Arc::clone(&panic_gate);
        // Batch cap 2 so the panic strands in-flight AND queued subjects.
        let (delivered, _unused_sink) = collect_delivered();
        let delivered_for_sink = Arc::clone(&delivered);
        let harness = Harness::with_caps(
            move |uris: Vec<String>| {
                if *trip.lock() {
                    // Force an out-of-bounds access so the dispatcher's
                    // catch_unwind observes a genuine unwind. Explicit panic!
                    // macros and panic_any are denied even in cfg(test) code
                    // by the crate's clippy configuration.
                    let boom: [u8; 0] = [];
                    let _ = boom[uris.len()];
                }
                delivered_for_sink.lock().push(uris);
            },
            MAX_PENDING_SUBJECTS,
            2,
        );

        // Trip BEFORE any dispatch so the very first batch panics mid-flight
        // with follow-up batches already queued behind it.
        *panic_gate.lock() = true;
        for i in 0..5usize {
            harness.debouncer.try_schedule(&format!("file:///strand{i}.pl"));
        }
        harness.advance(101);
        harness.wait_for(
            || !harness.debouncer.is_operational(),
            "degraded flag after panicking dispatch",
        );

        // Truthful transition: in-flight batch (2) plus queued batches (3)
        // were dropped AND COUNTED, and pressure reports true zeros instead
        // of phantom retention.
        let pressure = harness.debouncer.pressure();
        assert_eq!(pressure.panic_dropped_total, 5, "in-flight + queued subjects counted");
        assert_eq!(pressure.pending_subjects, 0, "no phantom pending after panic");
        assert_eq!(pressure.outboxed_batches, 0, "no phantom queued after panic");
        assert_eq!(pressure.active_subjects, 0);

        assert_eq!(
            harness.debouncer.try_schedule("file:///after.pl"),
            WatcherAdmission::Unavailable,
            "degraded queue must refuse admissions instead of apparent success"
        );
        assert_eq!(
            harness.debouncer.pressure().unavailable_total,
            1,
            "refusal after sink panic is counted as unavailable"
        );

        // Shutdown still reclaims both workers promptly (the panicked
        // dispatcher already exited; intake parks until the shutdown notify).
        harness.debouncer.shutdown_now();
        assert!(harness.workers_joined());
        assert_eq!(harness.debouncer.pending_uris(), 0);
        assert_eq!(delivered.lock().len(), 0, "nothing was delivered");
    }

    #[test]
    fn file_watcher_debouncer_partial_spawn_joins_survivor_and_reports_unavailable() {
        for (spawn_intake, spawn_dispatcher) in [(true, false), (false, true), (false, false)] {
            let debouncer =
                FileWatcherDebouncer::partially_spawned_for_test(spawn_intake, spawn_dispatcher);
            assert!(
                !debouncer.is_operational(),
                "intake={spawn_intake} dispatcher={spawn_dispatcher}"
            );
            assert_eq!(
                debouncer.try_schedule("file:///x.pl"),
                WatcherAdmission::Unavailable,
                "intake={spawn_intake} dispatcher={spawn_dispatcher}"
            );
            // Joins whichever worker survived; must not hang or leak threads.
            debouncer.shutdown_now();
            assert!(debouncer.workers_lock_is_empty());
        }
    }

    #[test]
    fn no_publication_once_sink_target_dropped() {
        let target: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let weak_target = Arc::downgrade(&target);
        let harness = Harness::with_sink(move |uris: Vec<String>| {
            if let Some(target) = weak_target.upgrade() {
                target.lock().extend(uris);
            }
        });

        harness.debouncer.try_schedule("file:///ghost.pl");
        drop(target);
        harness.advance(101);
        harness.debouncer.shutdown_now();

        // The final flush dispatched against a closure whose captured state
        // is gone: the run is counted, but the failed weak upgrade means
        // nothing observable was published.
        let pressure = harness.debouncer.pressure();
        assert_eq!(pressure.batches_dispatched, 1);
        assert_eq!(pressure.pending_subjects, 0);
    }

    #[test]
    fn file_watcher_debouncer_active_pressure_stays_visible_while_dispatch_holds_work() {
        let gate: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
        let gate_open = Arc::clone(&gate);
        let harness = Harness::with_sink(move |_uris: Vec<String>| {
            // Bounded block so a failing assert can never wedge shutdown.
            let start = Instant::now();
            while !*gate_open.lock() && start.elapsed() < Duration::from_secs(30) {
                std::thread::sleep(Duration::from_millis(2));
            }
        });

        harness.debouncer.try_schedule("file:///held.pl");
        harness.advance(101);

        harness.wait_for(
            || {
                let pressure = harness.debouncer.pressure();
                pressure.active_subjects == 1 && pressure.pending_subjects == 0
            },
            "active visibility during held dispatch",
        );
        let pressure = harness.debouncer.pressure();
        assert_eq!(pressure.pending_subjects, 0);
        assert_eq!(pressure.active_subjects, 1, "active work must stay observable");

        // Issue req. 9 zero-window control: while the dispatcher is stuck in
        // the held callback, a second wave goes due and is queued. The queue
        // counts it active AT HANDOFF — pending+active never reports zero
        // while queued work waits for the dispatcher.
        harness.debouncer.try_schedule("file:///queued-a.pl");
        harness.debouncer.try_schedule("file:///queued-b.pl");
        harness.advance(101);
        harness.wait_for(
            || {
                let p = harness.debouncer.pressure();
                p.pending_subjects == 0 && p.outboxed_batches == 1 && p.active_subjects == 3
            },
            "queued batch visible as active while dispatcher blocked",
        );
        {
            let p = harness.debouncer.pressure();
            assert_eq!(p.pending_subjects, 0);
            assert_eq!(p.active_subjects, 3, "1 in callback + 2 queued at handoff");
        }

        *gate.lock() = true;
        harness.wait_for(|| harness.debouncer.pressure().active_subjects == 0, "active release");
    }

    #[test]
    fn real_clock_end_to_end_delivery_smoke() {
        let delivered: Arc<Mutex<Vec<Vec<String>>>> = Arc::new(Mutex::new(Vec::new()));
        let sink_delivered = Arc::clone(&delivered);
        let debouncer =
            FileWatcherDebouncer::with_interval(Duration::from_millis(30), move |uris| {
                let mut sorted = uris;
                sorted.sort();
                sink_delivered.lock().push(sorted);
            });
        assert!(debouncer.is_operational());

        debouncer.schedule("file:///real-b.pl");
        debouncer.schedule("file:///real-a.pl");

        let deadline = Instant::now() + Duration::from_secs(5);
        while delivered.lock().is_empty() {
            assert!(Instant::now() < deadline, "real-clock delivery timed out");
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(
            delivered.lock()[0],
            vec!["file:///real-a.pl".to_string(), "file:///real-b.pl".to_string()]
        );
        assert_eq!(debouncer.pending_uris(), 0);
        drop(debouncer);
    }

    #[test]
    fn legacy_flushes_on_drop_via_final_flush() {
        let count: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
        let sink_count = Arc::clone(&count);
        let debouncer = FileWatcherDebouncer::with_interval(Duration::from_secs(5), move |_uris| {
            sink_count.fetch_add(1, Ordering::SeqCst);
        });
        debouncer.schedule("file:///drop.pl");
        assert_eq!(count.load(Ordering::SeqCst), 0);
        drop(debouncer);
        let deadline = Instant::now() + Duration::from_secs(5);
        while count.load(Ordering::SeqCst) == 0 {
            assert!(Instant::now() < deadline, "final flush did not run");
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn file_watcher_debouncer_heap_cap_bounds_reschedule_storm_under_stall() {
        let gate: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
        let gate_open = Arc::clone(&gate);
        // pending cap 40 → heap cap 80; batch 2 + outbox 8 ⇒ 16 subjects
        // drain into a saturated outbox and the remaining 24 stay pending
        // while intake parks at the backpressure gate.
        let harness = Harness::with_caps(
            move |_uris: Vec<String>| {
                // Bounded block: even if the test fails before opening the
                // gate, shutdown's join must complete instead of hanging the
                // whole suite.
                let start = Instant::now();
                while !*gate_open.lock() && start.elapsed() < Duration::from_secs(30) {
                    std::thread::sleep(Duration::from_millis(2));
                }
            },
            40,
            2,
        );

        for i in 0..40usize {
            assert_eq!(
                harness.debouncer.try_schedule(&format!("file:///storm{i}.pl")),
                WatcherAdmission::Accepted
            );
        }
        harness.advance(101);
        // Dispatcher pops batch #1 immediately (in-callback, blocked on the
        // gate), so queued batches plateau at 7 — count the stall by the
        // pending map instead, which excludes both in-flight and queued work.
        harness.wait_for(
            || harness.debouncer.pressure().pending_subjects == 24,
            "remainder parks at backpressure",
        );
        // Dispatcher holds batch #1 in-callback; up to 8 more fill the
        // outbox; the remaining dues stay parked pending.
        let parked: Vec<String> = {
            let state = harness.shared.state.lock();
            let mut keys: Vec<String> = state.subjects.keys().cloned().collect();
            keys.sort();
            keys
        };
        assert_eq!(parked.len(), 24, "40 subjects − 16 outboxed − 2 in-flight must park");

        // Storm: repeated schedules for still-pending URIs. Each supersedes a
        // previous heap entry; the retained-entry cap plus lazy purge must
        // keep the heap bounded instead of growing without limit.
        for _ in 0..300 {
            for uri in &parked {
                let admission = harness.debouncer.try_schedule(uri);
                assert!(
                    matches!(admission, WatcherAdmission::Coalesced | WatcherAdmission::Overflowed),
                    "unexpected admission {admission:?}"
                );
            }
            assert!(
                harness.debouncer.pressure().retained_heap_entries <= 80,
                "heap exceeded cap during storm"
            );
        }

        let pressure = harness.debouncer.pressure();
        assert!(pressure.retained_heap_entries <= 80);
        assert!(pressure.retained_heap_entries >= parked.len(), "live entries survive purge");
        assert_eq!(pressure.pending_subjects, parked.len());
        *gate.lock() = true;
    }

    #[test]
    fn file_watcher_debouncer_heap_cap_flips_to_overflowed_when_purge_cannot_free() {
        let harness = Harness::with_caps(|_uris: Vec<String>| {}, 16, MAX_BATCH_SUBJECTS);
        // Tighten the retained-entry cap to the live-entry count so the purge
        // pass cannot free anything: every further coalesce must refuse.
        harness.shared.max_heap_entries.store(4, Ordering::Relaxed);

        for i in 0..4usize {
            assert_eq!(
                harness.debouncer.try_schedule(&format!("file:///cap{i}.pl")),
                WatcherAdmission::Accepted
            );
        }
        for attempt in 0..10 {
            assert_eq!(
                harness.debouncer.try_schedule("file:///cap0.pl"),
                WatcherAdmission::Overflowed,
                "attempt {attempt}: live entries at cap must flip admissions"
            );
        }
        let pressure = harness.debouncer.pressure();
        assert_eq!(pressure.overflowed_total, 10);
        assert_eq!(pressure.retained_heap_entries, 4, "no growth past the cap");
        assert_eq!(pressure.coalesced_total, 0);
    }

    #[test]
    fn file_watcher_debouncer_batch_membership_is_interleaving_independent_above_batch_limit() {
        const CHUNK_LIMIT: usize = 128;
        let orders: [Vec<usize>; 2] = [(0..600usize).collect(), {
            let mut v: Vec<usize> = (0..600usize).collect();
            // Deterministic non-trivial permutation (xorshift shuffle).
            let mut seed: u64 = 0x9E3779B97F4A7C15;
            for i in (1..v.len()).rev() {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                v.swap(i, (seed as usize) % (i + 1));
            }
            v
        }];

        let mut runs: Vec<Vec<Vec<String>>> = Vec::new();
        for order in &orders {
            let (delivered, sink) = collect_delivered();
            let harness = Harness::with_caps(sink, MAX_PENDING_SUBJECTS, CHUNK_LIMIT);

            for index in order {
                harness.debouncer.try_schedule(&format!("file:///mem{index}.pl"));
            }
            harness.advance(101);
            harness.wait_for(
                || {
                    let batches = delivered.lock();
                    batches.iter().map(Vec::len).sum::<usize>() == 600
                },
                "membership run drain",
            );
            runs.push(delivered.lock().clone());
        }

        // Identical membership sequences across interleavings.
        assert_eq!(runs[0], runs[1]);
        // Each batch bounded and internally sorted; concatenation is the full
        // sorted set with every URI exactly once.
        let mut expected: Vec<String> =
            (0..600usize).map(|i| format!("file:///mem{i}.pl")).collect();
        expected.sort();
        for batch in &runs[0] {
            assert!(batch.len() <= CHUNK_LIMIT);
            let mut sorted = batch.clone();
            sorted.sort();
            assert_eq!(&sorted, batch, "batch itself must be sorted");
        }
        let flat: Vec<String> = runs[0].iter().flat_map(|b| b.iter().cloned()).collect();
        assert_eq!(flat, expected);
    }

    #[test]
    fn file_watcher_debouncer_survives_last_owner_drop_from_dispatcher_thread() {
        let cell: Arc<Mutex<Option<Arc<FileWatcherDebouncer>>>> = Arc::new(Mutex::new(None));
        let trip: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
        let latch: Arc<Mutex<bool>> = Arc::new(Mutex::new(true)); // open
        let delivered_log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

        let cell_for_sink = Arc::clone(&cell);
        let trip_for_sink = Arc::clone(&trip);
        let latch_for_sink = Arc::clone(&latch);
        let log_for_sink = Arc::clone(&delivered_log);

        let raw = FileWatcherDebouncer::with_interval(Duration::from_millis(30), move |uris| {
            for uri in uris {
                log_for_sink.lock().push(uri);
            }
            // Hold the dispatcher inside this callback while the latch is
            // closed, so the test can queue follow-up work behind it.
            // Bounded so a failing assert can never wedge shutdown.
            let spin_start = Instant::now();
            while !*latch_for_sink.lock() && spin_start.elapsed() < Duration::from_secs(30) {
                std::thread::sleep(Duration::from_millis(2));
            }
            if *trip_for_sink.lock() {
                // Last-owner drop FROM the dispatcher thread: Drop ->
                // shutdown_now would join this very thread. Pre-fix this
                // self-joined (panic -> misreported sink panic -> stranded
                // work); post-fix the worker detaches and finishes naturally.
                if let Some(last) = cell_for_sink.lock().take() {
                    drop(last);
                }
            }
        });
        let debouncer = Arc::new(raw);
        *cell.lock() = Some(Arc::clone(&debouncer));

        // Warm-up with the latch open.
        debouncer.try_schedule("file:///selfjoin-a.pl");
        let deadline = Instant::now() + Duration::from_secs(10);
        while !delivered_log.lock().iter().any(|u| u.ends_with("selfjoin-a.pl")) {
            assert!(Instant::now() < deadline, "warm-up delivery timed out");
            std::thread::sleep(Duration::from_millis(2));
        }

        // Close the latch, wait out the current window so any in-flight
        // callback has resolved, then schedule B: its callback must block.
        *latch.lock() = false;
        std::thread::sleep(Duration::from_millis(80));
        debouncer.try_schedule("file:///selfjoin-b.pl");
        let deadline = Instant::now() + Duration::from_secs(10);
        while !delivered_log.lock().iter().any(|u| u.ends_with("selfjoin-b.pl")) {
            assert!(Instant::now() < deadline, "blocked-callback entry timed out");
            std::thread::sleep(Duration::from_millis(2));
        }
        debouncer.try_schedule("file:///selfjoin-c.pl");
        let deadline = Instant::now() + Duration::from_secs(10);
        while debouncer.pressure().outboxed_batches == 0 {
            assert!(Instant::now() < deadline, "follow-up batch never reached outbox");
            std::thread::sleep(Duration::from_millis(2));
        }

        drop(debouncer); // cell now holds the only strong reference
        *trip.lock() = true;
        *latch.lock() = true;

        // Discriminator: post-fix the detached dispatcher resumes, delivers C
        // after surviving its own teardown, and no phantom sink panic was
        // recorded. Pre-fix, the self-join panic strands C forever.
        let deadline = Instant::now() + Duration::from_secs(10);
        while !delivered_log.lock().iter().any(|u| u.ends_with("selfjoin-c.pl")) {
            assert!(
                Instant::now() < deadline,
                "post-teardown delivery timed out (self-join hazard)"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}
