//! Ownership boundary for application background-worker execution lifetime,
//! cancellation, join, and settlement (#10024, "R01").
//!
//! Before this component, the diagnostic debouncer, off-lock parse worker,
//! and file-watcher debouncer lived behind three independent
//! `Mutex<Option<..>>` fields on `LspServer`. Nothing retained a task
//! identity, a registration record, an instrument-failure outcome, or a
//! settlement result: `DiagnosticDebouncer::with_interval` dropped its
//! `JoinHandle` and only logged a spawn failure;
//! `LspServer::install_default_parse_worker` detected a non-operational
//! worker and silently declined to install it; only `FileWatcherDebouncer`
//! modeled partial spawn failure at all (via `assemble`). Shutdown could not
//! distinguish "every application worker settled cleanly" from "a worker
//! never spawned" or "a worker failed" because no component owned that
//! state.
//!
//! `RuntimeServices` now owns the three worker slots AND, per
//! [`ApplicationTaskClass`], the retained terminal outcome of that worker's
//! install/spawn attempt. A fault terminal (failed, timed out, or never
//! instrumented) is retained permanently once recorded: a later clean or
//! cancelled terminal can never paper over an earlier failure, and
//! [`RuntimeServices::begin_application_shutdown`] can therefore refuse to
//! report [`ApplicationShutdown::Complete`] unless every registered class
//! settled cleanly.
//!
//! ## Hard boundary (maintainer ruling, 2026-08-18)
//!
//! This component owns execution lifetime, cancellation, join, and
//! settlement ONLY. It does not own or authorize semantic
//! readiness/currentness/publication. `indexing_in_progress`,
//! `indexing_rescan_pending`, `indexing_transition_lock`,
//! `pending_index_task_count`, and `parse_cancel_flags` remain on
//! `LspServer` and are out of scope here.
//!
//! ## Scope
//!
//! This PR is ownership-only: it moves the three worker slots and retains
//! their install-time outcomes. It does not redesign any worker, does not
//! move startup triggers (`scheduler.rs`/`workspace.rs` call the same
//! `install_*` methods at the same time), and does not add a generic job
//! scheduler. Framework tasks (the `tokio::spawn` mutation worker and read
//! dispatcher in `scheduler.rs`) are owned by the async runtime, not this
//! component, and have no [`ApplicationTaskClass`] variant.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use super::diagnostic_debounce::DiagnosticDebouncer;
use super::file_watcher_debounce::{FileWatcherDebouncer, WatcherAdmission};
use super::parse_worker::ParseWorker;

/// One class of application background work whose execution lifetime
/// `RuntimeServices` owns.
///
/// Deliberately closed and small: framework tasks (the `tokio::spawn`
/// mutation worker and read dispatcher spawned by `Scheduler::new`) are
/// owned by the async runtime and have no variant here (#10024).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum ApplicationTaskClass {
    ParseWorker,
    DiagnosticDebounce,
    FileWatcherDebounce,
}

impl ApplicationTaskClass {
    /// Every application task class this component governs. Used by the
    /// #10024 framework/application-separation negative control.
    #[allow(dead_code)] // Read by this module's own falsifiers.
    pub(crate) const ALL: [ApplicationTaskClass; 3] = [
        ApplicationTaskClass::ParseWorker,
        ApplicationTaskClass::DiagnosticDebounce,
        ApplicationTaskClass::FileWatcherDebounce,
    ];

    #[allow(dead_code)] // Read by this module's own falsifiers.
    pub(crate) fn label(self) -> &'static str {
        match self {
            ApplicationTaskClass::ParseWorker => "parse-worker",
            ApplicationTaskClass::DiagnosticDebounce => "diagnostic-debounce",
            ApplicationTaskClass::FileWatcherDebounce => "file-watcher-debounce",
        }
    }
}

/// Why an application shutdown was requested.
///
/// Not yet constructed from production code: the shutdown-request wiring
/// that will select a reason is #9508's job, out of scope for this
/// ownership-only PR. Exercised by this module's own falsifiers.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShutdownReason {
    /// The client sent an LSP `shutdown` request.
    ClientShutdownRequest,
    /// The owning `LspServer` is being dropped.
    ServerDrop,
}

/// The retained terminal outcome of one application task class.
///
/// [`TaskTerminal::Completed`] and [`TaskTerminal::Cancelled`] are accepted
/// outcomes of a requested shutdown; the rest are faults and are sticky. See
/// [`TaskTerminal::is_accepted`] and [`TaskTerminal::is_failure`].
/// [`TaskTerminal::InstrumentFailed`] is the only variant reachable from a
/// running server today, via the three `install_*` methods that
/// `Scheduler::new` calls. The rest are constructed only by
/// [`RuntimeServices::begin_application_shutdown`] and by this module's own
/// falsifiers; nothing in real server operation invokes that path until the
/// #9508 shutdown-request wiring lands.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TaskTerminal {
    /// The task settled with no error.
    Completed,
    /// The task ran and ended with an error.
    Failed { reason: String },
    /// The task was cancelled before it settled.
    Cancelled { reason: ShutdownReason },
    /// Settlement was awaited past its deadline with no terminal recorded.
    TimedOut,
    /// The task's underlying worker never became operational (e.g. every
    /// spawn attempt failed), so it can never settle at all.
    InstrumentFailed { reason: String },
}

impl TaskTerminal {
    /// True for the terminals a requested application shutdown expects to
    /// see: the task either finished its own work
    /// ([`TaskTerminal::Completed`]) or stopped because this component asked
    /// it to ([`TaskTerminal::Cancelled`]). Neither blocks
    /// [`ApplicationShutdown::Complete`] -- a cooperative stop is the normal
    /// outcome of `shutdown`, not a fault.
    fn is_accepted(&self) -> bool {
        matches!(self, TaskTerminal::Completed | TaskTerminal::Cancelled { .. })
    }

    /// True for the terminals that record a fault. A fault is sticky: once
    /// recorded it survives every later terminal, so a worker known to have
    /// failed, timed out, or never spawned can never be papered over by a
    /// subsequent clean or cancelled outcome.
    fn is_failure(&self) -> bool {
        matches!(
            self,
            TaskTerminal::Failed { .. }
                | TaskTerminal::TimedOut
                | TaskTerminal::InstrumentFailed { .. }
        )
    }
}

/// Aggregate outcome of one [`RuntimeServices::begin_application_shutdown`]
/// call.
///
/// Not yet read from production code: no caller invokes
/// `begin_application_shutdown` until the #9508 shutdown-request wiring
/// lands. Exercised by this module's own falsifiers.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ApplicationShutdown {
    /// Every registered class retained an accepted terminal
    /// ([`TaskTerminal::Completed`] or [`TaskTerminal::Cancelled`]).
    Complete,
    /// At least one registered class retained [`TaskTerminal::Failed`] and
    /// none is [`TaskTerminal::InstrumentFailed`] or
    /// [`TaskTerminal::TimedOut`].
    Failed,
    /// The deadline elapsed with at least one registered class still
    /// missing a retained terminal, or a class explicitly retained
    /// [`TaskTerminal::TimedOut`].
    TimedOut,
    /// At least one registered class retained
    /// [`TaskTerminal::InstrumentFailed`].
    InstrumentFailed,
}

/// A second [`RuntimeServices::register_task`] call for an already
/// registered class. The FIRST registration's identity is retained
/// untouched.
///
/// Constructed only by the strict raw-API registration path, which the
/// `install_*` methods deliberately do not use (a replacement worker is a
/// new lifetime, not a duplicate). Awaits the #9508 wiring; exercised by
/// this module's own falsifiers.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DuplicateRegistration;

/// A [`RuntimeServices::record_terminal`] call for a class that was never
/// registered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct UnregisteredTask;

struct TaskRecord {
    terminal: Option<TaskTerminal>,
}

/// Point-in-time view over every registered application task's retained
/// terminal.
///
/// Not yet constructed from production code: nothing calls
/// [`RuntimeServices::settlement_snapshot`] until the #9508 shutdown-request
/// wiring lands. Exercised by this module's own falsifiers.
#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub(crate) struct SettlementSnapshot {
    /// Registered classes that retain a terminal, with that terminal.
    pub(crate) settled: Vec<(ApplicationTaskClass, TaskTerminal)>,
    /// Registered classes with no retained terminal yet.
    pub(crate) pending: Vec<ApplicationTaskClass>,
}

impl SettlementSnapshot {
    /// True only when every registered class retained an accepted terminal
    /// ([`TaskTerminal::Completed`] or [`TaskTerminal::Cancelled`]) and none
    /// is still pending.
    #[allow(dead_code)] // Read by this module's own falsifiers.
    pub(crate) fn is_fully_clean(&self) -> bool {
        self.pending.is_empty() && self.settled.iter().all(|(_, terminal)| terminal.is_accepted())
    }
}

/// Owns the execution lifetime, cancellation, join, and settlement of every
/// application background worker.
///
/// See the module documentation for the #10024 hard boundary: this
/// component never owns semantic readiness/currentness/publication state.
pub(crate) struct RuntimeServices {
    tasks: Mutex<HashMap<ApplicationTaskClass, TaskRecord>>,
    diagnostic_debouncer: Mutex<Option<DiagnosticDebouncer>>,
    parse_worker_handle: Mutex<Option<Arc<ParseWorker>>>,
    file_watcher_debouncer: Mutex<Option<Arc<FileWatcherDebouncer>>>,
}

impl Default for RuntimeServices {
    fn default() -> Self {
        Self {
            tasks: Mutex::new(HashMap::new()),
            diagnostic_debouncer: Mutex::new(None),
            parse_worker_handle: Mutex::new(None),
            file_watcher_debouncer: Mutex::new(None),
        }
    }
}

impl RuntimeServices {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Register one task class before any terminal is recorded against it.
    ///
    /// Rejects a second registration of the same class with
    /// [`DuplicateRegistration`]; the first registration's identity is left
    /// untouched (its retained terminal, if any, is not reset).
    #[allow(dead_code)] // Strict raw-API registration; called once #9508 lands.
    pub(crate) fn register_task(
        &self,
        class: ApplicationTaskClass,
    ) -> Result<(), DuplicateRegistration> {
        let mut tasks = self.tasks.lock();
        if tasks.contains_key(&class) {
            return Err(DuplicateRegistration);
        }
        tasks.insert(class, TaskRecord { terminal: None });
        Ok(())
    }

    /// Start a fresh task lifetime for `class`, discarding any settlement
    /// retained from a previous worker in that slot.
    ///
    /// Installing a new worker IS a new execution lifetime. Without this, a
    /// replacement worker would inherit the retired one's terminal and a
    /// live worker could present as already settled -- or, worse, a
    /// replacement for a worker that failed to spawn would be permanently
    /// stuck reporting `InstrumentFailed`. This is distinct from
    /// [`Self::register_task`], which is the strict raw-API registration
    /// and still rejects a duplicate.
    fn begin_task_lifetime(&self, class: ApplicationTaskClass) {
        self.tasks.lock().insert(class, TaskRecord { terminal: None });
    }

    /// Ask one task class's installed worker to stop.
    ///
    /// This ONLY signals. It deliberately records no terminal, because a
    /// stop request is not a stop: `ParseWorker::request_shutdown` lets the
    /// current job finish, and `DiagnosticDebouncer::shutdown_now` only
    /// posts a message to the worker loop. Recording
    /// [`TaskTerminal::Cancelled`] here would let
    /// [`Self::begin_application_shutdown`] report
    /// [`ApplicationShutdown::Complete`] while a worker was still running --
    /// exactly the false-clean settlement this component exists to prevent.
    /// Exit is observed separately by [`Self::worker_has_exited`].
    ///
    /// Per-document parse cancellation is a different thing and stays on
    /// `LspServer` (`parse_cancel_flags`) under the #10024 hard boundary.
    pub(crate) fn request_cancel(&self, class: ApplicationTaskClass) {
        // Each worker is cloned/taken out of its slot guard BEFORE the stop
        // call, so no `RuntimeServices` lock is ever held across a blocking
        // join. `FileWatcherDebouncer::shutdown_now` joins its own threads;
        // holding a slot lock across that would block every concurrent
        // reader of the same slot.
        match class {
            ApplicationTaskClass::ParseWorker => {
                let worker = self.parse_worker_handle.lock().clone();
                if let Some(worker) = worker {
                    worker.request_shutdown();
                }
            }
            ApplicationTaskClass::DiagnosticDebounce => {
                let guard = self.diagnostic_debouncer.lock();
                let Some(debouncer) = guard.as_ref() else {
                    return;
                };
                // Sends one message; does not block on the worker.
                debouncer.shutdown_now();
            }
            ApplicationTaskClass::FileWatcherDebounce => {
                // `signal_shutdown`, NOT `shutdown_now`: the latter joins both
                // watcher threads, and a dispatcher stuck in a blocked
                // callback never returns. Calling it here would strand
                // `begin_application_shutdown` before its first deadline
                // check, so the bounded settlement outcome this component
                // promises could never be recorded. Signalling returns
                // immediately; exit is observed by `worker_has_exited`, and
                // the join stays in teardown where it belongs.
                //
                // The slot is an `Arc` so the handle is cloned out and the
                // guard released before the call, keeping a concurrent
                // `schedule_file_watcher_uri` on its fast shutting-down
                // refusal path (#8064).
                let debouncer = self.file_watcher_debouncer.lock().clone();
                if let Some(debouncer) = debouncer {
                    debouncer.signal_shutdown();
                }
            }
        }
    }

    /// Whether the installed worker for `class` has actually exited.
    ///
    /// An empty slot reports `false`: nothing was ever installed, so nothing
    /// can be observed to have settled, and a shutdown waiting on that class
    /// must time out rather than read absence as success.
    /// The terminal to retain for `class` once its worker is observed to have
    /// stopped, or `None` while it is still running or was never installed.
    ///
    /// An exit is not automatically an orderly one. A debouncer whose callback
    /// panicked has also "exited", and recording that as
    /// [`TaskTerminal::Cancelled`] would let a dead worker report a clean
    /// shutdown -- the same false-clean settlement this component exists to
    /// prevent. Each worker's own failure signal decides which terminal
    /// applies:
    ///
    /// - the file watcher folds a panicked sink into `is_operational`;
    /// - the diagnostic debouncer reports whether its loop returned normally
    ///   via `exited_cleanly`;
    /// - the parse worker recovers job panics inside `catch_unwind` and keeps
    ///   its threads alive, so its pool exiting really is the cooperative stop
    ///   it was asked for.
    fn observed_exit(
        &self,
        class: ApplicationTaskClass,
        reason: ShutdownReason,
    ) -> Option<TaskTerminal> {
        let cancelled = || TaskTerminal::Cancelled { reason };
        match class {
            ApplicationTaskClass::ParseWorker => {
                let worker = self.parse_worker_handle.lock().clone()?;
                (!worker.is_operational()).then(cancelled)
            }
            ApplicationTaskClass::DiagnosticDebounce => {
                let guard = self.diagnostic_debouncer.lock();
                let debouncer = guard.as_ref()?;
                if !debouncer.has_exited() {
                    return None;
                }
                Some(if debouncer.exited_cleanly() {
                    cancelled()
                } else {
                    TaskTerminal::Failed {
                        reason: "diagnostic debounce worker exited abnormally".to_string(),
                    }
                })
            }
            ApplicationTaskClass::FileWatcherDebounce => {
                let debouncer = self.file_watcher_debouncer.lock().clone()?;
                if !debouncer.has_exited() {
                    return None;
                }
                Some(if debouncer.is_operational() {
                    cancelled()
                } else {
                    TaskTerminal::Failed {
                        reason: "file watcher debounce worker exited abnormally".to_string(),
                    }
                })
            }
        }
    }

    /// Record the terminal outcome of one registered task class.
    ///
    /// Rejects an unregistered class with [`UnregisteredTask`].
    ///
    /// A task lifetime retains exactly one terminal outcome, fixed by the
    /// first call that settles it. Two rules keep that true:
    ///
    /// - a fault already on record (see [`TaskTerminal::is_failure`]) is
    ///   never overwritten by any later terminal: once a class is known to
    ///   have failed, timed out, or never spawned, that finding survives;
    /// - an accepted terminal already on record (see
    ///   [`TaskTerminal::is_accepted`]) is never overwritten by another
    ///   accepted terminal, so a settled class keeps its first causal
    ///   outcome and its typed [`ShutdownReason`] instead of taking whichever
    ///   caller happened to run last.
    ///
    /// A fault still dominates an accepted terminal, so a worker that is
    /// later discovered to have failed is not papered over by an earlier
    /// clean or cancelled record.
    ///
    /// Beginning a new lifetime for the class (see
    /// [`RuntimeServices::begin_task_lifetime`]) clears the retained
    /// terminal; that is the only way a settled class becomes settleable
    /// again.
    pub(crate) fn record_terminal(
        &self,
        class: ApplicationTaskClass,
        terminal: TaskTerminal,
    ) -> Result<(), UnregisteredTask> {
        let mut tasks = self.tasks.lock();
        let record = tasks.get_mut(&class).ok_or(UnregisteredTask)?;
        match &record.terminal {
            // A recorded fault survives every later terminal.
            Some(existing) if existing.is_failure() => {}
            // The existing terminal is accepted (the arm above took every
            // fault). A second accepted terminal must not rewrite the first
            // causal outcome or its reason; a fault still wins.
            Some(_) if terminal.is_accepted() => {}
            _ => record.terminal = Some(terminal),
        }
        Ok(())
    }

    /// Point-in-time settlement view over every registered class.
    #[allow(dead_code)] // Called once the #9508 shutdown-request wiring lands.
    pub(crate) fn settlement_snapshot(&self) -> SettlementSnapshot {
        let tasks = self.tasks.lock();
        let mut snapshot = SettlementSnapshot::default();
        for (class, record) in tasks.iter() {
            match &record.terminal {
                Some(terminal) => snapshot.settled.push((*class, terminal.clone())),
                None => snapshot.pending.push(*class),
            }
        }
        // `tasks` is a `HashMap`, so iteration order is not stable. Sort by
        // class so callers (and assertions) see a deterministic snapshot.
        snapshot.settled.sort_by_key(|(class, _)| *class);
        snapshot.pending.sort_unstable();
        snapshot
    }

    /// Ask every registered class to stop for `reason`, then decide the
    /// aggregate application-shutdown outcome.
    ///
    /// After requesting cancellation, polls retained settlement state until
    /// every registered class has a terminal or `deadline` elapses. A
    /// registered class that never retains a terminal -- a dropped or
    /// detached handle, or a class whose worker slot was never filled --
    /// resolves the aggregate to [`ApplicationShutdown::TimedOut`] rather
    /// than [`ApplicationShutdown::Complete`]: a missing terminal can never
    /// be read as success.
    #[allow(dead_code)] // Called once the #9508 shutdown-request wiring lands.
    pub(crate) fn begin_application_shutdown(
        &self,
        reason: ShutdownReason,
        deadline: Instant,
    ) -> ApplicationShutdown {
        let registered: Vec<ApplicationTaskClass> = self.tasks.lock().keys().copied().collect();
        for class in &registered {
            self.request_cancel(*class);
        }
        loop {
            // Promote each pending class whose worker is OBSERVED to have
            // exited. A stop request alone never settles a class.
            let snapshot = self.settlement_snapshot();
            for class in &snapshot.pending {
                if let Some(terminal) = self.observed_exit(*class, reason) {
                    let _ = self.record_terminal(*class, terminal);
                }
            }

            let snapshot = self.settlement_snapshot();
            if snapshot.pending.is_empty() {
                return aggregate_settled_outcome(&snapshot);
            }
            if Instant::now() >= deadline {
                // Persist the timeout against every class still pending, so
                // the sticky-fault rule holds: a later completion cannot
                // erase an observed timeout and let a retry report
                // `Complete`.
                for class in &snapshot.pending {
                    let _ = self.record_terminal(*class, TaskTerminal::TimedOut);
                }
                return ApplicationShutdown::TimedOut;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    // -- Worker slot ownership: install/accessor methods forwarded from
    //    `LspServer` so external behavior stays byte-for-byte identical.

    /// Install the diagnostic debouncer, registering
    /// [`ApplicationTaskClass::DiagnosticDebounce`] and retaining
    /// [`TaskTerminal::InstrumentFailed`] immediately if its worker thread
    /// never spawned.
    pub(crate) fn install_diagnostic_debouncer(&self, debouncer: DiagnosticDebouncer) {
        self.begin_task_lifetime(ApplicationTaskClass::DiagnosticDebounce);
        if !debouncer.is_operational() {
            let _ = self.record_terminal(
                ApplicationTaskClass::DiagnosticDebounce,
                TaskTerminal::InstrumentFailed {
                    reason: "diagnostic debounce worker thread failed to spawn".to_string(),
                },
            );
        }
        *self.diagnostic_debouncer.lock() = Some(debouncer);
    }

    /// Schedule `uri` on the installed diagnostic debouncer. Returns `false`
    /// when the caller must fall back to immediate publication: either no
    /// debouncer is installed (unit-test path), or the installed one refused
    /// the admission because its channel has no receiver.
    ///
    /// A refused admission also **evicts** the slot. That worker can never
    /// publish again, so leaving it installed would route every later
    /// `publish_diagnostics_debounced` into a dead channel and silently drop
    /// the diagnostics (#14252). Evicting puts the immediate path in charge,
    /// which is the fail-open behavior #14322 established.
    pub(crate) fn schedule_diagnostic_debounce(&self, uri: &str) -> bool {
        let mut guard = self.diagnostic_debouncer.lock();
        let Some(debouncer) = guard.as_ref() else {
            return false;
        };
        if debouncer.schedule(uri) {
            return true;
        }
        *guard = None;
        false
    }

    /// Whether a diagnostic debouncer is currently installed. Test-only
    /// observation of the slot this component owns, replacing the direct
    /// `LspServer.diagnostic_debouncer` field access that #10024 removed.
    #[cfg(test)]
    pub(crate) fn diagnostic_debouncer_is_installed(&self) -> bool {
        self.diagnostic_debouncer.lock().is_some()
    }

    #[allow(dead_code)] // Read by test/debug runtime pressure snapshots.
    pub(crate) fn diagnostic_debounce_pending_uris(&self) -> usize {
        self.diagnostic_debouncer.lock().as_ref().map_or(0, DiagnosticDebouncer::pending_uris)
    }

    /// Install the off-lock parse worker, registering
    /// [`ApplicationTaskClass::ParseWorker`]. Returns whether the worker was
    /// operational: a non-operational worker is NOT installed (the
    /// synchronous fallback path stays active) and its outcome is retained
    /// as [`TaskTerminal::InstrumentFailed`] instead of only being logged.
    pub(crate) fn install_parse_worker(&self, worker: ParseWorker) -> bool {
        let operational = worker.is_operational();
        self.begin_task_lifetime(ApplicationTaskClass::ParseWorker);
        if operational {
            *self.parse_worker_handle.lock() = Some(Arc::new(worker));
        } else {
            // Retire whatever occupied the slot. Leaving a previous worker
            // installed while this lifetime retains `InstrumentFailed` would
            // be a settlement lie: `parse_worker()` would keep handing out a
            // live worker for a class recorded as never instrumented. Clearing
            // it puts the synchronous fallback back in charge, which is what
            // the retained terminal claims.
            self.parse_worker_handle.lock().take();
            let _ = self.record_terminal(
                ApplicationTaskClass::ParseWorker,
                TaskTerminal::InstrumentFailed {
                    reason: "parse worker pool failed to spawn any threads".to_string(),
                },
            );
        }
        operational
    }

    /// The installed off-lock parse worker, if any. `None` means the
    /// synchronous fallback path is active.
    pub(crate) fn parse_worker(&self) -> Option<Arc<ParseWorker>> {
        self.parse_worker_handle.lock().clone()
    }

    /// Install the file watcher debouncer, registering
    /// [`ApplicationTaskClass::FileWatcherDebounce`] and retaining
    /// [`TaskTerminal::InstrumentFailed`] immediately if neither worker
    /// thread spawned.
    pub(crate) fn install_file_watcher_debouncer(&self, debouncer: FileWatcherDebouncer) {
        self.begin_task_lifetime(ApplicationTaskClass::FileWatcherDebounce);
        if !debouncer.is_operational() {
            let _ = self.record_terminal(
                ApplicationTaskClass::FileWatcherDebounce,
                TaskTerminal::InstrumentFailed {
                    reason: "file watcher debounce worker spawn failed".to_string(),
                },
            );
        }
        *self.file_watcher_debouncer.lock() = Some(Arc::new(debouncer));
    }

    /// Schedule `uri` on the installed file watcher debouncer. Returns
    /// `true` only for a genuinely queued admission (accepted or
    /// coalesced); `false` for no debouncer installed or any degraded
    /// admission, unchanged from before this PR (#8064).
    pub(crate) fn schedule_file_watcher_uri(&self, uri: &str) -> bool {
        let guard = self.file_watcher_debouncer.lock();
        match guard.as_ref() {
            None => false,
            Some(debouncer) => matches!(
                debouncer.try_schedule(uri),
                WatcherAdmission::Accepted | WatcherAdmission::Coalesced
            ),
        }
    }

    #[cfg(any(test, feature = "expose_lsp_test_api"))]
    pub(crate) fn file_watcher_pressure(
        &self,
    ) -> Option<super::file_watcher_debounce::WatcherPressureSnapshot> {
        self.file_watcher_debouncer.lock().as_ref().map(|debouncer| debouncer.pressure())
    }

    /// Test observation of the watcher's own health signal, which folds in a
    /// panicked sink. `None` when no debouncer is installed.
    #[cfg(test)]
    pub(crate) fn file_watcher_is_operational(&self) -> Option<bool> {
        self.file_watcher_debouncer.lock().as_ref().map(|d| d.is_operational())
    }

    #[cfg(test)]
    pub(crate) fn file_watcher_debouncer_installed(&self) -> bool {
        self.file_watcher_debouncer.lock().is_some()
    }

    /// Test-only direct teardown of the installed file watcher debouncer,
    /// standing in for the raw field access the pre-#10024 tests used.
    #[cfg(test)]
    pub(crate) fn shutdown_file_watcher_debouncer_for_test(&self) {
        let debouncer = self.file_watcher_debouncer.lock().clone();
        if let Some(debouncer) = debouncer {
            debouncer.shutdown_now();
        }
    }
}

#[allow(dead_code)] // Called once the #9508 shutdown-request wiring lands.
fn aggregate_settled_outcome(snapshot: &SettlementSnapshot) -> ApplicationShutdown {
    if snapshot
        .settled
        .iter()
        .any(|(_, terminal)| matches!(terminal, TaskTerminal::InstrumentFailed { .. }))
    {
        return ApplicationShutdown::InstrumentFailed;
    }
    if snapshot.settled.iter().any(|(_, terminal)| matches!(terminal, TaskTerminal::TimedOut)) {
        return ApplicationShutdown::TimedOut;
    }
    if snapshot.settled.iter().any(|(_, terminal)| matches!(terminal, TaskTerminal::Failed { .. }))
    {
        return ApplicationShutdown::Failed;
    }
    // Only accepted terminals remain: `Completed`, and `Cancelled` from the
    // cooperative stop this shutdown itself requested.
    ApplicationShutdown::Complete
}
