//! Bounded deterministic coalescer for file-watcher change notifications.
//!
//! Coalesces rapid `workspace/didChangeWatchedFiles` subjects into ordered,
//! size-bounded batches and hands each batch to a caller-supplied sink after a
//! quiet period. Continuous rescheduling cannot defer a subject past its
//! maximum-latency bound.
//!
//! Contract owned here (#8064, prepared-queue stage):
//!
//! - bounded intake: distinct pending subjects are capped; admission beyond the
//!   cap resolves to [`WatcherAdmission::Overflowed`] instead of growing memory
//!   or dropping silently;
//! - typed admission: every attempt resolves to [`WatcherAdmission`] — accepted,
//!   coalesced, overflowed, worker-unavailable, or shut-down. Spawn failure and
//!   saturation are therefore visible to callers instead of masquerading as
//!   successful queueing behind a log line;
//! - deterministic order: due subjects are emitted sorted by URI regardless of
//!   arrival interleaving or hash iteration order;
//! - quiet plus maximum latency: repeated schedules extend only the quiet
//!   deadline; a subject fires at most [`MAX_LATENCY_INTERVALS`] windows after
//!   first admission, so churn cannot starve publication;
//! - truthful pressure: pending, outboxed, active (batch currently inside the
//!   sink), and high-water counts stay observable — moving work from pending to
//!   active never reports zero total work;
//! - joinable shutdown: both workers stop, one final sorted size-bounded flush
//!   is delivered while the sink is alive, and both threads are joined before
//!   teardown completes;
//! - no semantic authority: this queue never reads files, parses, indexes, or
//!   mutates workspace state. Fired batches re-enter exactly the server entry
//!   point the runtime used before, keeping the #7893/#7088 cutover a sink
//!   swap rather than a redesign.
//!
//! Terminal create/remove/rename evidence, canonical duplicate-transport
//! equivalence, and root/config/trust/watch generation binding remain owned by
//! #7893/#7088/#10770 and are intentionally not modeled here.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
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
    fn peek_live_deadline(&mut self, stats: &CoalescerStats) -> Option<u64> {
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

    /// Remove up to `limit` due subjects in deterministic URI order. Subjects
    /// beyond the limit remain pending for the next pass.
    fn take_due_up_to(&mut self, now: u64, limit: usize, stats: &CoalescerStats) -> Vec<String> {
        let mut due = Vec::new();
        while due.len() < limit {
            let Some(deadline) = self.peek_live_deadline(stats) else {
                break;
            };
            if deadline > now {
                break;
            }
            if let Some(Reverse((_, _, uri))) = self.heap.pop() {
                stats.heap_operations.fetch_add(1, Ordering::Relaxed);
                self.subjects.remove(&uri);
                due.push(uri);
            }
        }
        due.sort_unstable();
        due
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
}

struct Shared {
    state: Mutex<IntakeState>,
    /// Wakes the intake worker for admissions, shutdown, and clock progress.
    intake_cv: Condvar,
    /// Coordinates intake (batch produced / outbox space freed) with the
    /// dispatcher. Both condvars wait on the same state mutex.
    handoff_cv: Condvar,
    clock: Arc<dyn DebounceClock>,
    interval_ms: u64,
    max_latency_ms: u64,
    max_pending_subjects: usize,
    max_batch_subjects: usize,
    stats: CoalescerStats,
}

struct WorkerHandles {
    intake: JoinHandle<()>,
    dispatcher: JoinHandle<()>,
}

#[cfg(any(test, feature = "expose_lsp_test_api"))]
pub(crate) struct WatcherPressureSnapshot {
    pub(crate) pending_subjects: usize,
    pub(crate) active_subjects: usize,
    pub(crate) outboxed_batches: usize,
    pub(crate) high_water_subjects: usize,
    pub(crate) admitted_total: u64,
    pub(crate) coalesced_total: u64,
    pub(crate) overflowed_total: u64,
    pub(crate) unavailable_total: u64,
    pub(crate) rejected_after_shutdown_total: u64,
    pub(crate) batches_dispatched: u64,
    pub(crate) heap_operations: u64,
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
        F: Fn(Vec<String>) + Send + Sync + 'static,
    {
        Self::with_interval(Duration::from_millis(DEFAULT_DEBOUNCE_MS), publish_fn)
    }

    /// Create a new debouncer with a custom debounce window.
    ///
    /// The maximum-latency horizon is [`MAX_LATENCY_INTERVALS`] windows, so a
    /// continuously rescheduled subject still fires within it.
    pub fn with_interval<F>(interval: Duration, publish_fn: F) -> Self
    where
        F: Fn(Vec<String>) + Send + Sync + 'static,
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
        F: Fn(Vec<String>) + Send + Sync + 'static,
    {
        let interval_ms = u64::try_from(interval.as_millis()).unwrap_or(DEFAULT_DEBOUNCE_MS);
        let shared = make_shared(clock, interval_ms, max_pending_subjects, max_batch_subjects);

        // One worker owns scheduling (never touches the sink); one owns
        // dispatch (the only place the callback runs), so a long batch can
        // never block observation of newer events.
        let intake_shared = Arc::clone(&shared);
        let intake_handle = thread::Builder::new()
            .name("file-watcher-intake".into())
            .spawn(move || intake_loop(intake_shared));

        let dispatch_shared = Arc::clone(&shared);
        let dispatch_sink = Arc::new(publish_fn);
        let dispatch_handle = thread::Builder::new()
            .name("file-watcher-dispatch".into())
            .spawn(move || dispatch_loop(dispatch_shared, dispatch_sink));

        let workers = match (intake_handle, dispatch_handle) {
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
        let shared =
            make_shared(clock, DEFAULT_DEBOUNCE_MS, MAX_PENDING_SUBJECTS, MAX_BATCH_SUBJECTS);
        Self { shared, workers: Mutex::new(None), operational: false }
    }

    /// Whether both worker threads started successfully. A debouncer that
    /// failed to start reports [`WatcherAdmission::Unavailable`] for every
    /// attempt instead of silently absorbing events.
    pub fn is_operational(&self) -> bool {
        self.operational
    }

    /// Schedule a URI and observe the typed admission outcome.
    ///
    /// Repeated schedules of a pending subject reset only its quiet deadline;
    /// the maximum-latency deadline fixed at first admission is preserved so
    /// continuous rescheduling cannot starve publication.
    pub fn try_schedule(&self, uri: &str) -> WatcherAdmission {
        if !self.operational {
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
    /// Fire-and-forget convenience over [`Self::try_schedule`]; non-admitted
    /// outcomes degrade to debug logs here. Callers that must distinguish
    /// queued work from degraded modes should use [`Self::try_schedule`].
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
        let outboxed_batches = self.shared.state.lock().outbox.len();
        WatcherPressureSnapshot {
            pending_subjects: stats.pending_subjects.load(Ordering::SeqCst),
            active_subjects: stats.active_subjects.load(Ordering::SeqCst),
            outboxed_batches,
            high_water_subjects: stats.high_water_subjects.load(Ordering::SeqCst),
            admitted_total: stats.admitted_total.load(Ordering::Relaxed),
            coalesced_total: stats.coalesced_total.load(Ordering::Relaxed),
            overflowed_total: stats.overflowed_total.load(Ordering::Relaxed),
            unavailable_total: stats.unavailable_total.load(Ordering::Relaxed),
            rejected_after_shutdown_total: stats
                .rejected_after_shutdown_total
                .load(Ordering::Relaxed),
            batches_dispatched: stats.batches_dispatched.load(Ordering::Relaxed),
            heap_operations: stats.heap_operations.load(Ordering::Relaxed),
        }
    }

    /// Stop intake, deliver one final sorted bounded flush while the sink is
    /// alive, and join both workers. Idempotent; invoked from `Drop`.
    ///
    /// The final flush bypasses the outbox cap because total retained work is
    /// already bounded by the pending-subject cap, so memory stays bounded.
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
        let _ = handles.intake.join();
        let _ = handles.dispatcher.join();
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
        clock,
        interval_ms,
        max_latency_ms,
        max_pending_subjects,
        max_batch_subjects,
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
        },
    })
}

fn push_heap_entry(guard: &mut IntakeState, uri: String, effective_deadline: u64, seq: u64) {
    guard.heap.push(Reverse((effective_deadline, seq, uri)));
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
        let _ = handle.join();
    }
    if let Some(handle) = dispatcher {
        let _ = handle.join();
    }
}

fn intake_loop(shared: Arc<Shared>) {
    let clock = Arc::clone(&shared.clock);
    let mut guard = shared.state.lock();
    loop {
        if guard.shutting_down {
            return;
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

fn dispatch_loop<F>(shared: Arc<Shared>, sink: Arc<F>)
where
    F: Fn(Vec<String>) + Send + Sync + 'static,
{
    let mut guard = shared.state.lock();
    loop {
        match guard.outbox.pop_front() {
            Some(batch) => {
                drop(guard);
                let batch_len = batch.len();
                shared.stats.active_subjects.fetch_add(batch_len, Ordering::SeqCst);
                sink(batch);
                shared.stats.active_subjects.fetch_sub(batch_len, Ordering::SeqCst);
                shared.stats.batches_dispatched.fetch_add(1, Ordering::Relaxed);
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
            F: Fn(Vec<String>) + Send + Sync + 'static,
        {
            Self::with_caps(sink, MAX_PENDING_SUBJECTS, MAX_BATCH_SUBJECTS)
        }

        fn with_caps<F>(sink: F, max_pending_subjects: usize, max_batch_subjects: usize) -> Self
        where
            F: Fn(Vec<String>) + Send + Sync + 'static,
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

        // Quadratic intake inspects ~M^2/2 ≈ 4.5M entries across the burst;
        // lazy-deletion heap intake stays linearithmic even counting wakeup
        // re-peeks. Generous slack still falsifies per-arrival rescans.
        let ops = harness.debouncer.pressure().heap_operations;
        assert!(ops < 3000 * 60, "heap operations {ops} suggest quadratic intake");
        assert_eq!(harness.debouncer.pressure().overflowed_total, 0);
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
        assert!(harness.workers_joined());
        assert_eq!(
            harness.debouncer.try_schedule("file:///late.pl"),
            WatcherAdmission::ShuttingDown
        );
        assert_eq!(harness.debouncer.pressure().rejected_after_shutdown_total, 1);
        assert_eq!(delivered.lock().len(), 1, "no publication after shutdown");
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

        // The final flush dispatched against a dead sink: the run is counted,
        // but the dead weak upgrade means nothing observable was published.
        let pressure = harness.debouncer.pressure();
        assert_eq!(pressure.batches_dispatched, 1);
        assert_eq!(pressure.pending_subjects, 0);
    }

    #[test]
    fn file_watcher_debouncer_active_pressure_stays_visible_while_dispatch_holds_work() {
        let gate: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
        let gate_open = Arc::clone(&gate);
        let harness = Harness::with_sink(move |_uris: Vec<String>| {
            while !*gate_open.lock() {
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
}
