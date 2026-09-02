//! A deterministic, in-memory supervisor for consumer tests.
//!
//! The fake runs no process, spawns no thread, and consults no clock, so a
//! consumer test built on it cannot race. Every result it produces is marked
//! [`EvidenceClass::Fake`] and carries [`Limitation::FakeEvidenceOnly`]:
//! fake evidence can prove that a consumer handles a disposition, and can
//! never stand in for evidence that a real process behaves that way.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};

use super::encoding::PlanFingerprint;
use super::event::{EventLedger, ProcessEvent, ProcessEventKind, TerminationPhase};
use super::identity::{PlanId, RunId};
use super::plan::CancellationPolicy;
use super::plan::StdinPolicy;
use super::port::{
    CancellationAcknowledgement, HandleDropDisposition, ProcessHandle, ProcessSupervisor,
    StdinWriteOutcome,
};
use super::result::{
    BackendIdentity, CancellationReason, CleanupDisposition, ControlState, EvidenceClass,
    Limitation, ObservedSettlement, ProcessResult, StreamChannel, StreamEvidence,
    TerminalDisposition, TreeDisposition, WorkMetadata,
};
use super::validation::ValidatedProcessPlan;

/// The backend name every fake result carries.
pub const FAKE_BACKEND_NAME: &str = "fake-in-memory";

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// A scripted run the fake will replay.
#[derive(Debug, Clone)]
pub struct ScriptedRun {
    /// Non-terminal events replayed before settlement, in order.
    ///
    /// A terminal event here is malformed: the handle emits the terminal event
    /// itself, elected from `control` and `settlement`, so a scripted one could
    /// announce an outcome that disagrees with the result `wait` returns.
    pub events: Vec<ProcessEventKind>,
    /// What the child is observed to do.
    pub settlement: ObservedSettlement,
    /// The control-plane state at settlement.
    pub control: ControlState,
    /// Evidence for stdout.
    pub stdout: StreamEvidence,
    /// Evidence for stderr.
    pub stderr: StreamEvidence,
    /// Whether cleanup was proven.
    pub cleanup: CleanupDisposition,
    /// What was done about descendants.
    pub tree: TreeDisposition,
}

impl ScriptedRun {
    /// A run whose child starts and exits with the given code.
    pub fn exiting(code: i32) -> Self {
        Self {
            events: vec![ProcessEventKind::Started],
            settlement: ObservedSettlement::Exited { code },
            control: ControlState::default(),
            stdout: StreamEvidence::empty(StreamChannel::Stdout),
            stderr: StreamEvidence::empty(StreamChannel::Stderr),
            cleanup: CleanupDisposition::Completed,
            tree: TreeDisposition::NotRequired,
        }
    }

    /// Attach stdout bytes to the run.
    #[must_use]
    pub fn with_stdout(mut self, bytes: impl Into<Vec<u8>>) -> Self {
        self.stdout = StreamEvidence::complete(StreamChannel::Stdout, bytes.into());
        self
    }

    /// Attach stderr bytes to the run.
    #[must_use]
    pub fn with_stderr(mut self, bytes: impl Into<Vec<u8>>) -> Self {
        self.stderr = StreamEvidence::complete(StreamChannel::Stderr, bytes.into());
        self
    }

    /// Replace the stdout evidence wholesale.
    #[must_use]
    pub fn with_stdout_evidence(mut self, evidence: StreamEvidence) -> Self {
        self.stdout = evidence;
        self
    }

    /// Replace the control state, for example to script a deadline.
    #[must_use]
    pub fn with_control(mut self, control: ControlState) -> Self {
        self.control = control;
        self
    }

    /// Replace the cleanup and tree dispositions.
    #[must_use]
    pub fn with_cleanup(mut self, cleanup: CleanupDisposition, tree: TreeDisposition) -> Self {
        self.cleanup = cleanup;
        self.tree = tree;
        self
    }
}

/// Whether a disposition can truthfully describe a refused start.
///
/// A start that never produced a running child cannot have completed an exit
/// or been signalled; scripting one of those as a refusal would let a failed
/// start read as an ordinary success.
fn describes_a_refused_start(disposition: &TerminalDisposition) -> bool {
    matches!(
        disposition,
        TerminalDisposition::SpawnRejected(_)
            | TerminalDisposition::SpawnFailed { .. }
            | TerminalDisposition::CancelledBeforeStart(_)
            | TerminalDisposition::UnsupportedBackend
            | TerminalDisposition::StaleOrUnauthorized(_)
            | TerminalDisposition::SupervisorFailed
            | TerminalDisposition::NotProven
    )
}

/// What the fake does with the next start attempt.
#[derive(Debug, Clone)]
pub enum ScriptedOutcome {
    /// Hand back a handle that replays this run.
    Run(Box<ScriptedRun>),
    /// Refuse the start attempt with this terminal disposition.
    RefuseStart(TerminalDisposition),
}

/// A deterministic supervisor that records plans and replays scripted runs.
#[derive(Debug, Default)]
pub struct FakeSupervisor {
    outcomes: Mutex<VecDeque<ScriptedOutcome>>,
    recorded_plans: Mutex<Vec<ValidatedProcessPlan>>,
    drop_dispositions: Arc<Mutex<Vec<HandleDropDisposition>>>,
    stdin_written: Arc<Mutex<Vec<(RunId, Vec<u8>)>>>,
    next_run: Mutex<u64>,
}

impl FakeSupervisor {
    /// Create a fake with no scripted outcomes.
    ///
    /// An unscripted start attempt settles as
    /// [`TerminalDisposition::NotProven`] rather than as a success: an
    /// unconfigured fake must never look green.
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue an outcome for the next start attempt.
    pub fn script(&self, outcome: ScriptedOutcome) {
        lock(&self.outcomes).push_back(outcome);
    }

    /// Queue a run for the next start attempt.
    pub fn script_run(&self, run: ScriptedRun) {
        self.script(ScriptedOutcome::Run(Box::new(run)));
    }

    /// The plans this supervisor was asked to start, in order.
    pub fn recorded_plans(&self) -> Vec<ValidatedProcessPlan> {
        lock(&self.recorded_plans).clone()
    }

    /// The drop dispositions of every handle this supervisor produced.
    pub fn drop_dispositions(&self) -> Vec<HandleDropDisposition> {
        lock(&self.drop_dispositions).clone()
    }

    /// Every byte accepted through one run's stdin channel, in order.
    ///
    /// Attributed per run: a test driving two runs must be able to tell which
    /// of them received what, and a single merged buffer cannot say.
    pub fn stdin_written_for(&self, run_id: &RunId) -> Vec<u8> {
        lock(&self.stdin_written)
            .iter()
            .filter(|(run, _)| run == run_id)
            .flat_map(|(_, bytes)| bytes.iter().copied())
            .collect()
    }

    /// Every byte accepted across every run, in order, paired with its run.
    pub fn stdin_writes(&self) -> Vec<(RunId, Vec<u8>)> {
        lock(&self.stdin_written).clone()
    }

    fn allocate_run_id(&self) -> RunId {
        let mut next = lock(&self.next_run);
        let id = *next;
        *next = next.saturating_add(1);
        RunId::new(format!("fake-run-{id}"))
    }
}

fn fake_backend() -> BackendIdentity {
    BackendIdentity::new(FAKE_BACKEND_NAME, EvidenceClass::Fake)
}

impl ProcessSupervisor for FakeSupervisor {
    fn start(
        &self,
        plan: ValidatedProcessPlan,
    ) -> Result<Box<dyn ProcessHandle>, Box<ProcessResult>> {
        let run_id = self.allocate_run_id();
        let plan_id = plan.plan().plan_id().clone();
        let fingerprint = plan.fingerprint();
        let cancellable = plan.plan().cancellation() != CancellationPolicy::NotCancellable;
        let streamed_stdin = matches!(plan.plan().stdin(), StdinPolicy::Streamed);
        lock(&self.recorded_plans).push(plan);

        let outcome = lock(&self.outcomes).pop_front();
        match outcome {
            Some(ScriptedOutcome::RefuseStart(disposition)) => {
                let disposition = if describes_a_refused_start(&disposition) {
                    disposition
                } else {
                    // A script asking for a refusal that reads as a completed
                    // run is malformed; it settles as a supervisor failure
                    // rather than becoming the success it asked for.
                    TerminalDisposition::SupervisorFailed
                };
                Err(Box::new(refusal_result(plan_id, fingerprint, run_id, disposition)))
            }
            Some(ScriptedOutcome::Run(run)) => Ok(Box::new(FakeHandle::new(
                plan_id,
                fingerprint,
                run_id,
                *run,
                cancellable,
                streamed_stdin,
                Arc::clone(&self.drop_dispositions),
                Arc::clone(&self.stdin_written),
            ))),
            None => Err(Box::new(refusal_result(
                plan_id,
                fingerprint,
                run_id,
                TerminalDisposition::NotProven,
            ))),
        }
    }
}

fn refusal_result(
    plan_id: PlanId,
    fingerprint: PlanFingerprint,
    run_id: RunId,
    disposition: TerminalDisposition,
) -> ProcessResult {
    match ProcessResult::new(
        plan_id.clone(),
        fingerprint,
        run_id.clone(),
        disposition,
        StreamEvidence::empty(StreamChannel::Stdout),
        StreamEvidence::empty(StreamChannel::Stderr),
        CleanupDisposition::NotRequired,
        TreeDisposition::NotRequired,
        fake_backend(),
        WorkMetadata::default(),
        Vec::new(),
    ) {
        Ok(result) => result,
        // A refused start produced no handle, so no events were emitted and
        // no bytes were read: empty streams are the truthful shape here, not
        // a default.
        Err(_) => ProcessResult::supervisor_failure(
            plan_id,
            fingerprint,
            run_id,
            fake_backend(),
            WorkMetadata::default(),
            StreamEvidence::empty(StreamChannel::Stdout),
            StreamEvidence::empty(StreamChannel::Stderr),
        ),
    }
}

/// A handle over a scripted run.
#[derive(Debug)]
struct FakeHandle {
    plan_id: PlanId,
    fingerprint: PlanFingerprint,
    run_id: RunId,
    ledger: EventLedger,
    pending: VecDeque<ProcessEventKind>,
    run: ScriptedRun,
    cancellable: bool,
    script_rejected: bool,
    child_started: bool,
    streamed_stdin: bool,
    stdin_closed: bool,
    stdin_written: Arc<Mutex<Vec<(RunId, Vec<u8>)>>>,
    settled: bool,
    drop_dispositions: Arc<Mutex<Vec<HandleDropDisposition>>>,
}

impl FakeHandle {
    fn new(
        plan_id: PlanId,
        fingerprint: PlanFingerprint,
        run_id: RunId,
        run: ScriptedRun,
        cancellable: bool,
        streamed_stdin: bool,
        drop_dispositions: Arc<Mutex<Vec<HandleDropDisposition>>>,
        stdin_written: Arc<Mutex<Vec<(RunId, Vec<u8>)>>>,
    ) -> Self {
        let pending = run.events.iter().cloned().collect();
        Self {
            plan_id,
            fingerprint,
            run_id: run_id.clone(),
            ledger: EventLedger::new(run_id),
            pending,
            run,
            cancellable,
            script_rejected: false,
            child_started: false,
            streamed_stdin,
            stdin_closed: false,
            stdin_written,
            settled: false,
            drop_dispositions,
        }
    }

    /// The work this run actually did, so every result path reports the same
    /// event count the consumer received.
    fn work_metadata(&self) -> WorkMetadata {
        WorkMetadata {
            wall_time: std::time::Duration::ZERO,
            events_emitted: self.ledger.admitted_count(),
        }
    }

    fn elected(&self) -> TerminalDisposition {
        TerminalDisposition::elect(self.run.control, self.run.settlement)
    }

    /// Attempt the scripted result from the run's own components.
    ///
    /// Fails exactly when the script describes a run that cannot have
    /// happened — swapped channels, evidence claiming a completeness it lacks,
    /// a completed exit alongside a failed cleanup, output attributed to a
    /// start that never occurred.
    fn assemble(&self) -> Option<ProcessResult> {
        ProcessResult::new(
            self.plan_id.clone(),
            self.fingerprint,
            self.run_id.clone(),
            self.elected(),
            self.run.stdout.clone(),
            self.run.stderr.clone(),
            self.run.cleanup,
            self.run.tree,
            fake_backend(),
            self.work_metadata(),
            Vec::new(),
        )
        .ok()
    }

    /// The supervisor-failure result, carrying the events actually emitted.
    ///
    /// The rejection settles the stream with a terminal event, so the run did
    /// emit events. Defaulting the count to zero would contradict what the
    /// consumer just received.
    fn failure_result(&self) -> ProcessResult {
        // The ledger's per-channel totals are facts the consumer already
        // holds. Reporting zero here would contradict chunk events already
        // delivered; reporting a fingerprint would claim an identity this
        // fake never computed, since a scripted chunk carries a count and no
        // bytes. So the count is published and the identity is withheld.
        ProcessResult::supervisor_failure(
            self.plan_id.clone(),
            self.fingerprint,
            self.run_id.clone(),
            fake_backend(),
            self.work_metadata(),
            StreamEvidence::observed_but_unidentified(
                StreamChannel::Stdout,
                self.ledger.observed_bytes(StreamChannel::Stdout),
            ),
            StreamEvidence::observed_but_unidentified(
                StreamChannel::Stderr,
                self.ledger.observed_bytes(StreamChannel::Stderr),
            ),
        )
    }

    /// Assemble the scripted result.
    ///
    /// A script that describes an incoherent run settles as a supervisor
    /// failure rather than becoming a result that asserts something untrue.
    fn build_result(&self) -> ProcessResult {
        if self.script_rejected {
            return self.failure_result();
        }
        match self.assemble() {
            Some(result) => result,
            None => self.failure_result(),
        }
    }
}

impl ProcessHandle for FakeHandle {
    fn run_id(&self) -> &RunId {
        &self.run_id
    }

    fn next_event(&mut self) -> Option<ProcessEvent> {
        if self.ledger.is_settled() {
            return None;
        }
        let kind = match self.pending.pop_front() {
            Some(kind) if kind.is_terminal() => {
                // The handle elects and emits the terminal event itself. A
                // scripted one — anywhere in the list, including last — could
                // announce an outcome that disagrees with what `wait` returns,
                // so the script is refused rather than replayed.
                //
                // The run settles here, with the same supervisor failure
                // `wait` will report. Returning `None` without settling would
                // leave the stream open, and the next call would emit the
                // *elected* terminal event — reintroducing exactly the
                // divergence this branch exists to prevent.
                self.script_rejected = true;
                self.pending.clear();
                ProcessEventKind::Terminal(TerminalDisposition::SupervisorFailed)
            }
            Some(kind) => kind,
            None => {
                // The terminal event must name the outcome `wait` will report,
                // so the decision is made here, before anything is announced.
                // Assembling the result is what discovers that a script
                // describes an impossible run; discovering it afterwards would
                // leave the consumer holding a terminal event that the result
                // then contradicts.
                if self.assemble().is_none() {
                    self.script_rejected = true;
                    ProcessEventKind::Terminal(TerminalDisposition::SupervisorFailed)
                } else {
                    ProcessEventKind::Terminal(self.elected())
                }
            }
        };
        // The ledger refuses a malformed script — a terminal event placed
        // mid-stream, or a chunk whose offset does not continue its channel.
        // That must not read as an ordinary end of stream, or an invalid test
        // setup silently hides the events it swallowed.
        //
        // The rejection has to *settle* the run, not merely stop it. Returning
        // `None` while the ledger stayed open let the next poll emit the
        // elected terminal event, announcing a success that `wait` then
        // contradicted — the same divergence the mid-stream guard exists to
        // prevent, reached one call later.
        match self.ledger.admit(kind) {
            Ok(event) => {
                if matches!(event.kind(), ProcessEventKind::Started) {
                    self.child_started = true;
                }
                Some(event)
            }
            Err(_) => {
                self.script_rejected = true;
                self.pending.clear();
                self.ledger
                    .admit(ProcessEventKind::Terminal(TerminalDisposition::SupervisorFailed))
                    .ok()
            }
        }
    }

    fn write_stdin(&mut self, bytes: &[u8]) -> StdinWriteOutcome {
        if self.ledger.is_settled() {
            return StdinWriteOutcome::RunSettled;
        }
        if !self.streamed_stdin {
            return StdinWriteOutcome::NotStreamed;
        }
        if self.stdin_closed {
            return StdinWriteOutcome::AlreadyClosed;
        }
        lock(&self.stdin_written).push((self.run_id.clone(), bytes.to_vec()));
        StdinWriteOutcome::Accepted { bytes: bytes.len() }
    }

    fn close_stdin(&mut self) -> StdinWriteOutcome {
        if self.ledger.is_settled() {
            return StdinWriteOutcome::RunSettled;
        }
        if !self.streamed_stdin {
            return StdinWriteOutcome::NotStreamed;
        }
        if self.stdin_closed {
            return StdinWriteOutcome::AlreadyClosed;
        }
        self.stdin_closed = true;
        StdinWriteOutcome::Accepted { bytes: 0 }
    }

    fn cancel(&mut self, reason: CancellationReason) -> CancellationAcknowledgement {
        if self.ledger.is_settled() {
            return CancellationAcknowledgement::AlreadySettled;
        }
        if !self.cancellable {
            return CancellationAcknowledgement::NotCancellable;
        }
        self.run.control.cancellation_requested = Some(reason);
        // Whether a `Started` event was actually admitted, not how many polls
        // happened. A poll count is not proof that the child started: a script
        // whose first event is not `Started` would claim one, and cancelling
        // before the first poll would deny one the script does describe.
        self.run.control.started_before_cancellation = self.child_started;
        if !self.child_started {
            // A run cancelled before it started did not go on to exit, and
            // nothing exists to clean up or terminate. Every piece of the
            // scripted child evidence has to be reconciled together: leaving
            // the settlement alone elects `NotProven`, and leaving the cleanup
            // alone leaves a completed cleanup beside a child that never ran,
            // which result assembly refuses.
            self.run.settlement = ObservedSettlement::NotStarted;
            self.run.cleanup = CleanupDisposition::NotRequired;
            self.run.tree = TreeDisposition::NotRequired;
            self.run.stdout = StreamEvidence::empty(StreamChannel::Stdout);
            self.run.stderr = StreamEvidence::empty(StreamChannel::Stderr);
        }
        self.pending.clear();
        self.pending.push_back(ProcessEventKind::TerminationPhase(
            TerminationPhase::CancellationRequested(reason),
        ));
        CancellationAcknowledgement::Accepted
    }

    fn wait(mut self: Box<Self>) -> ProcessResult {
        while self.next_event().is_some() {}
        self.settled = true;
        self.build_result()
    }
}

impl Drop for FakeHandle {
    fn drop(&mut self) {
        let disposition = if self.settled {
            HandleDropDisposition::SettledBeforeDrop
        } else {
            HandleDropDisposition::AbandonedWithoutSettlement
        };
        lock(&self.drop_dispositions).push(disposition);
    }
}

/// The limitations every fake result carries.
///
/// Exposed so a consumer test can assert that it is not mistaking fake
/// evidence for executed evidence.
pub const FAKE_RESULT_LIMITATIONS: &[Limitation] =
    &[Limitation::FakeEvidenceOnly, Limitation::NoIsolationClaimed];
