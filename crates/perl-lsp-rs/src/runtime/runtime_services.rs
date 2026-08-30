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
/// [`TaskTerminal::InstrumentFailed`] and [`TaskTerminal::Cancelled`] are
/// constructed from production code today; the remainder await the #9508
/// shutdown-request/completion-hook wiring and are exercised by this
/// module's own falsifiers.
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
    file_watcher_debouncer: Mutex<Option<FileWatcherDebouncer>>,
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

    /// Request cooperative cancellation of one task class by forwarding to
    /// the installed worker's own stop path, then retain
    /// [`TaskTerminal::Cancelled`].
    ///
    /// A class whose worker slot is empty is NOT recorded as cancelled:
    /// there is nothing to stop, so it stays pending and a shutdown that
    /// waits on it resolves to [`ApplicationShutdown::TimedOut`] rather than
    /// reading an absent worker as an orderly stop.
    ///
    /// Per-document parse cancellation is a different thing and stays on
    /// `LspServer` (`parse_cancel_flags`) under the #10024 hard boundary.
    pub(crate) fn request_cancel(&self, class: ApplicationTaskClass, reason: ShutdownReason) {
        let stopped = match class {
            ApplicationTaskClass::ParseWorker => self
                .parse_worker_handle
                .lock()
                .as_ref()
                .map(|worker| worker.request_shutdown())
                .is_some(),
            ApplicationTaskClass::DiagnosticDebounce => self
                .diagnostic_debouncer
                .lock()
                .as_ref()
                .map(DiagnosticDebouncer::shutdown_now)
                .is_some(),
            ApplicationTaskClass::FileWatcherDebounce => self
                .file_watcher_debouncer
                .lock()
                .as_ref()
                .map(FileWatcherDebouncer::shutdown_now)
                .is_some(),
        };
        if stopped {
            // `Err` here means the class was never registered, which cannot
            // happen for an installed worker: every `install_*` registers
            // before filling its slot.
            let _ = self.record_terminal(class, TaskTerminal::Cancelled { reason });
        }
    }

    /// Record the terminal outcome of one registered task class.
    ///
    /// Rejects an unregistered class with [`UnregisteredTask`]. A fault
    /// terminal already on record (see [`TaskTerminal::is_failure`]) is
    /// never overwritten by a later terminal, accepted or not: once a class
    /// is known to have failed, timed out, or never spawned, that finding
    /// survives for the life of this `RuntimeServices` instance.
    pub(crate) fn record_terminal(
        &self,
        class: ApplicationTaskClass,
        terminal: TaskTerminal,
    ) -> Result<(), UnregisteredTask> {
        let mut tasks = self.tasks.lock();
        let record = tasks.get_mut(&class).ok_or(UnregisteredTask)?;
        match &record.terminal {
            Some(existing) if existing.is_failure() => {}
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
        for class in registered {
            self.request_cancel(class, reason);
        }
        loop {
            let snapshot = self.settlement_snapshot();
            if snapshot.pending.is_empty() {
                return aggregate_settled_outcome(&snapshot);
            }
            if Instant::now() >= deadline {
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
        let _ = self.register_task(ApplicationTaskClass::DiagnosticDebounce);
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
    /// when no debouncer is installed so the caller can fall back to
    /// immediate publication (unit-test path, unchanged from before this
    /// PR).
    pub(crate) fn schedule_diagnostic_debounce(&self, uri: &str) -> bool {
        let guard = self.diagnostic_debouncer.lock();
        if let Some(ref debouncer) = *guard {
            debouncer.schedule(uri);
            true
        } else {
            false
        }
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
        let _ = self.register_task(ApplicationTaskClass::ParseWorker);
        if operational {
            *self.parse_worker_handle.lock() = Some(Arc::new(worker));
        } else {
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
        let _ = self.register_task(ApplicationTaskClass::FileWatcherDebounce);
        if !debouncer.is_operational() {
            let _ = self.record_terminal(
                ApplicationTaskClass::FileWatcherDebounce,
                TaskTerminal::InstrumentFailed {
                    reason: "file watcher debounce worker spawn failed".to_string(),
                },
            );
        }
        *self.file_watcher_debouncer.lock() = Some(debouncer);
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
        self.file_watcher_debouncer.lock().as_ref().map(FileWatcherDebouncer::pressure)
    }

    #[cfg(test)]
    pub(crate) fn file_watcher_debouncer_installed(&self) -> bool {
        self.file_watcher_debouncer.lock().is_some()
    }

    /// Test-only direct teardown of the installed file watcher debouncer,
    /// standing in for the raw field access the pre-#10024 tests used.
    #[cfg(test)]
    pub(crate) fn shutdown_file_watcher_debouncer_for_test(&self) {
        if let Some(debouncer) = self.file_watcher_debouncer.lock().as_ref() {
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
