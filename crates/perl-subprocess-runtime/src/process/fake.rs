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
    stdin_written: Arc<Mutex<Vec<u8>>>,
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

    /// Every byte accepted through a caller-driven stdin channel, in order.
    ///
    /// A consumer test asserts against this rather than against a promise
    /// that the bytes went somewhere.
    pub fn stdin_written(&self) -> Vec<u8> {
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
    ProcessResult::new(
        plan_id,
        fingerprint,
        run_id,
        disposition,
        StreamEvidence::empty(StreamChannel::Stdout),
        StreamEvidence::empty(StreamChannel::Stderr),
        CleanupDisposition::NotRequired,
        TreeDisposition::NotRequired,
        fake_backend(),
        WorkMetadata::default(),
        Vec::new(),
    )
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
    streamed_stdin: bool,
    stdin_closed: bool,
    stdin_written: Arc<Mutex<Vec<u8>>>,
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
        stdin_written: Arc<Mutex<Vec<u8>>>,
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
            streamed_stdin,
            stdin_closed: false,
            stdin_written,
            settled: false,
            drop_dispositions,
        }
    }

    fn elected(&self) -> TerminalDisposition {
        TerminalDisposition::elect(self.run.control, self.run.settlement)
    }

    fn build_result(&self) -> ProcessResult {
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
            WorkMetadata {
                wall_time: std::time::Duration::ZERO,
                events_emitted: self.ledger.admitted_count(),
            },
            Vec::new(),
        )
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
            Some(kind) => kind,
            None => ProcessEventKind::Terminal(self.elected()),
        };
        // The ledger refuses post-terminal events; a rejection here means the
        // script was malformed, and the stream simply ends.
        self.ledger.admit(kind).ok()
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
        lock(&self.stdin_written).extend_from_slice(bytes);
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
        self.run.control.started_before_cancellation = self.ledger.admitted_count() > 0;
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
