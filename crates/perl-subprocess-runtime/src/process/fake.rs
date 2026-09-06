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

/// Why a [`ScriptedRun`] was refused at script time.
///
/// These are fixture errors, not domain outcomes. A real backend cannot reach
/// a stream that announces a control-plane cause its `ControlState` then
/// denies; the fake refuses that input when the test author writes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptedRunContradiction {
    /// `events` contain [`TerminationPhase::DeadlineReached`] but
    /// [`ControlState::deadline_reached`] is `false`.
    DeadlineReachedWithoutControl,
    /// `events` contain [`TerminationPhase::CancellationRequested`] with a
    /// reason that is not [`ControlState::cancellation_requested`].
    CancellationRequestedMismatch {
        /// The reason the event announced.
        event: CancellationReason,
        /// The reason recorded in control state, if any.
        control: Option<CancellationReason>,
    },
    /// `events` contain [`ProcessEventKind::LimitReached`] but
    /// [`ControlState::output_limit_exceeded`] is `false`.
    LimitReachedWithoutControl,
}

impl std::fmt::Display for ScriptedRunContradiction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DeadlineReachedWithoutControl => {
                f.write_str("scripted DeadlineReached without control.deadline_reached")
            }
            Self::CancellationRequestedMismatch { event, control } => match control {
                None => write!(
                    f,
                    "scripted CancellationRequested({event:?}) without control.cancellation_requested"
                ),
                Some(reason) => write!(
                    f,
                    "scripted CancellationRequested({event:?}) but control.cancellation_requested is {reason:?}"
                ),
            },
            Self::LimitReachedWithoutControl => {
                f.write_str("scripted LimitReached without control.output_limit_exceeded")
            }
        }
    }
}

impl std::error::Error for ScriptedRunContradiction {}

/// A scripted run the fake will replay.
#[derive(Debug, Clone)]
pub struct ScriptedRun {
    /// Non-terminal events replayed before settlement, in order.
    ///
    /// A terminal event here is malformed: the handle emits the terminal event
    /// itself, elected from `control` and `settlement`, so a scripted one could
    /// announce an outcome that disagrees with the result `wait` returns.
    ///
    /// A `DeadlineReached`, `CancellationRequested`, or `LimitReached` event
    /// that disagrees with `control` is refused by [`FakeSupervisor::script`],
    /// not replayed. A control flag without a matching event is legal: a
    /// backend need not emit a phase for every control fact. Live
    /// [`ProcessHandle::cancel`] after an already-admitted
    /// `CancellationRequested` of a different reason is refused the same way:
    /// it does not rewrite the emitted phase.
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

    /// The first scripted event that contradicts [`Self::control`], if any.
    ///
    /// [`TerminalDisposition::elect`] is driven by `control`, not by this
    /// list. An event that announces a deadline, cancellation, or output
    /// limit the control state then denies is a malformed fixture: replaying
    /// it would hand the consumer a stream the result contradicts.
    ///
    /// The converse is not a contradiction. A backend may record a control
    /// fact without emitting the corresponding phase.
    pub fn control_contradiction(&self) -> Option<ScriptedRunContradiction> {
        self.events.iter().find_map(|kind| event_control_contradiction(kind, &self.control))
    }
}

/// Pair one scripted event against the control state that will elect the result.
///
/// Exhaustive on [`ProcessEventKind`] so a new lifecycle event has to be
/// classified rather than defaulting to admissible.
fn event_control_contradiction(
    kind: &ProcessEventKind,
    control: &ControlState,
) -> Option<ScriptedRunContradiction> {
    match kind {
        ProcessEventKind::TerminationPhase(TerminationPhase::DeadlineReached) => {
            if control.deadline_reached {
                None
            } else {
                Some(ScriptedRunContradiction::DeadlineReachedWithoutControl)
            }
        }
        ProcessEventKind::TerminationPhase(TerminationPhase::CancellationRequested(reason)) => {
            if control.cancellation_requested == Some(*reason) {
                None
            } else {
                Some(ScriptedRunContradiction::CancellationRequestedMismatch {
                    event: *reason,
                    control: control.cancellation_requested,
                })
            }
        }
        ProcessEventKind::LimitReached(_) => {
            if control.output_limit_exceeded {
                None
            } else {
                Some(ScriptedRunContradiction::LimitReachedWithoutControl)
            }
        }
        ProcessEventKind::Started
        | ProcessEventKind::StdoutBytes(_)
        | ProcessEventKind::StderrBytes(_)
        | ProcessEventKind::TerminationPhase(
            TerminationPhase::GracefulSignalSent
            | TerminationPhase::ForcedSignalSent
            | TerminationPhase::GroupReaped,
        )
        | ProcessEventKind::Terminal(_) => None,
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
    ///
    /// A [`ScriptedOutcome::Run`] whose events contradict its control is
    /// refused here, so a malformed fixture fails at the point it was written
    /// rather than emitting a stream the elected result then denies.
    pub fn script(&self, outcome: ScriptedOutcome) -> Result<(), ScriptedRunContradiction> {
        if let ScriptedOutcome::Run(run) = &outcome
            && let Some(contradiction) = run.control_contradiction()
        {
            return Err(contradiction);
        }
        lock(&self.outcomes).push_back(outcome);
        Ok(())
    }

    /// Queue a run for the next start attempt.
    pub fn script_run(&self, run: ScriptedRun) -> Result<(), ScriptedRunContradiction> {
        self.script(ScriptedOutcome::Run(Box::new(run)))
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

    /// Hand out the next run identity.
    ///
    /// Sequential and per-supervisor, so a test can name a run without the
    /// nondeterminism a real identity source would introduce.
    fn allocate_run_id(&self) -> RunId {
        let mut next = lock(&self.next_run);
        let id = *next;
        *next = next.saturating_add(1);
        RunId::new(format!("fake-run-{id}"))
    }
}

/// The backend identity every result from this supervisor carries.
///
/// `EvidenceClass::Fake` is not decoration: it is what stops a scripted result
/// from ever reading as evidence about a real process.
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

/// Assemble the result for a start this supervisor refused.
///
/// Falls back to `ProcessResult::supervisor_failure` if the refusal itself
/// cannot be expressed coherently, so a refusal never becomes a panic.
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
    announced_cancellation: Option<CancellationReason>,
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
            announced_cancellation: None,
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

    /// The terminal cause the run's control state and settlement elect.
    ///
    /// Read through `TerminalDisposition::elect` rather than stored, so the
    /// fake cannot script a cause the precedence rule would not produce.
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

    /// Record facts the live `cancel` path must not contradict later.
    ///
    /// `child_started` is whether a `Started` event was admitted, not how
    /// many polls happened. `announced_cancellation` is the reason already
    /// shown to the consumer: replacing it with a different reason would
    /// make `wait` elect a cause the stream has already denied.
    fn note_admitted_kind(&mut self, kind: &ProcessEventKind) {
        if matches!(kind, ProcessEventKind::Started) {
            self.child_started = true;
        }
        if let ProcessEventKind::TerminationPhase(TerminationPhase::CancellationRequested(reason)) =
            kind
        {
            self.announced_cancellation = Some(*reason);
        }
    }

    /// Whether `reason` would replace an already-admitted cancellation phase.
    fn contradicts_emitted_cancellation(&self, reason: CancellationReason) -> bool {
        self.announced_cancellation.is_some_and(|emitted| emitted != reason)
    }

    /// Apply a cancellation the consumer has not been shown a conflicting
    /// reason for.
    ///
    /// Same-reason `cancel` after a matching admitted event still runs this
    /// path: pre-start tests rely on it to clear a scripted `Started` that
    /// has not been admitted yet and to reconcile settlement.
    fn apply_accepted_cancel(&mut self, reason: CancellationReason) {
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
                self.note_admitted_kind(event.kind());
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
        if self.contradicts_emitted_cancellation(reason) {
            return CancellationAcknowledgement::ContradictsEmittedReason;
        }
        self.apply_accepted_cancel(reason);
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

#[cfg(test)]
mod script_control_coherence {
    use super::*;
    use crate::process::event::{LimitEvidence, StreamChunkEvidence};
    use crate::process::plan::CaptureBound;

    fn observation_limit(channel: StreamChannel, run_continues: bool) -> LimitEvidence {
        LimitEvidence { channel, bound: CaptureBound::Observation, limit_bytes: 1, run_continues }
    }

    fn with_event(kind: ProcessEventKind) -> ScriptedRun {
        let mut run = ScriptedRun::exiting(0);
        run.events.push(kind);
        run
    }

    fn queued_len(supervisor: &FakeSupervisor) -> usize {
        lock(&supervisor.outcomes).len()
    }

    #[test]
    fn deadline_event_without_control_is_refused_at_script_time() {
        // The wrong implementation this kills: accepting the fixture and
        // emitting DeadlineReached, then settling as CompletedExit.
        let supervisor = FakeSupervisor::new();
        let run = with_event(ProcessEventKind::TerminationPhase(TerminationPhase::DeadlineReached));
        assert_eq!(
            supervisor.script_run(run),
            Err(ScriptedRunContradiction::DeadlineReachedWithoutControl)
        );
        assert_eq!(queued_len(&supervisor), 0);
    }

    #[test]
    fn deadline_event_with_control_is_scriptable() {
        let run = with_event(ProcessEventKind::TerminationPhase(TerminationPhase::DeadlineReached))
            .with_control(ControlState { deadline_reached: true, ..ControlState::default() });
        assert_eq!(
            FakeSupervisor::new().script_run(run),
            Ok(()),
            "matching deadline control was refused"
        );
    }

    #[test]
    fn deadline_control_without_event_is_scriptable() {
        // The converse is legal: a backend need not emit a phase for every
        // control fact.
        let run = ScriptedRun::exiting(0)
            .with_control(ControlState { deadline_reached: true, ..ControlState::default() });
        assert_eq!(
            FakeSupervisor::new().script_run(run),
            Ok(()),
            "deadline control without event was refused"
        );
    }

    #[test]
    fn cancellation_event_without_control_is_refused_at_script_time() {
        let supervisor = FakeSupervisor::new();
        let run = with_event(ProcessEventKind::TerminationPhase(
            TerminationPhase::CancellationRequested(CancellationReason::Shutdown),
        ));
        assert_eq!(
            supervisor.script_run(run),
            Err(ScriptedRunContradiction::CancellationRequestedMismatch {
                event: CancellationReason::Shutdown,
                control: None,
            })
        );
        assert_eq!(queued_len(&supervisor), 0);
    }

    #[test]
    fn cancellation_event_with_matching_reason_is_scriptable() {
        let run = with_event(ProcessEventKind::TerminationPhase(
            TerminationPhase::CancellationRequested(CancellationReason::UserRequested),
        ))
        .with_control(ControlState {
            cancellation_requested: Some(CancellationReason::UserRequested),
            ..ControlState::default()
        });
        assert_eq!(
            FakeSupervisor::new().script_run(run),
            Ok(()),
            "matching cancellation control was refused"
        );
    }

    #[test]
    fn cancellation_event_with_mismatched_reason_is_refused() {
        // The wrong implementation this kills: treating any Some as enough,
        // so UserRequested in the stream elects as Shutdown in the result.
        let run = with_event(ProcessEventKind::TerminationPhase(
            TerminationPhase::CancellationRequested(CancellationReason::UserRequested),
        ))
        .with_control(ControlState {
            cancellation_requested: Some(CancellationReason::Shutdown),
            ..ControlState::default()
        });
        assert_eq!(
            FakeSupervisor::new().script_run(run),
            Err(ScriptedRunContradiction::CancellationRequestedMismatch {
                event: CancellationReason::UserRequested,
                control: Some(CancellationReason::Shutdown),
            })
        );
    }

    #[test]
    fn cancellation_control_without_event_is_scriptable() {
        let run = ScriptedRun::exiting(0).with_control(ControlState {
            cancellation_requested: Some(CancellationReason::OperationSuperseded),
            ..ControlState::default()
        });
        assert_eq!(
            FakeSupervisor::new().script_run(run),
            Ok(()),
            "cancellation control without event was refused"
        );
    }

    #[test]
    fn limit_event_without_control_is_refused_at_script_time() {
        let supervisor = FakeSupervisor::new();
        let run = with_event(ProcessEventKind::LimitReached(observation_limit(
            StreamChannel::Stdout,
            false,
        )));
        assert_eq!(
            supervisor.script_run(run),
            Err(ScriptedRunContradiction::LimitReachedWithoutControl)
        );
        assert_eq!(queued_len(&supervisor), 0);
    }

    #[test]
    fn limit_event_with_control_is_scriptable() {
        let run = with_event(ProcessEventKind::LimitReached(observation_limit(
            StreamChannel::Stdout,
            false,
        )))
        .with_control(ControlState { output_limit_exceeded: true, ..ControlState::default() });
        assert_eq!(
            FakeSupervisor::new().script_run(run),
            Ok(()),
            "matching limit control was refused"
        );
    }

    #[test]
    fn limit_control_without_event_is_scriptable() {
        let run = ScriptedRun::exiting(0)
            .with_control(ControlState { output_limit_exceeded: true, ..ControlState::default() });
        assert_eq!(
            FakeSupervisor::new().script_run(run),
            Ok(()),
            "limit control without event was refused"
        );
    }

    #[test]
    fn continuing_limit_event_still_requires_the_flag() {
        // The wrong implementation this kills: treating run_continues as
        // permission to omit output_limit_exceeded, so the stream announces a
        // bound the elected result never names.
        let run = with_event(ProcessEventKind::LimitReached(observation_limit(
            StreamChannel::Stderr,
            true,
        )));
        assert_eq!(
            FakeSupervisor::new().script_run(run),
            Err(ScriptedRunContradiction::LimitReachedWithoutControl)
        );
    }

    #[test]
    fn a_middle_event_is_still_checked() {
        // The wrong implementation this kills: inspecting only the first or
        // last event, so a contradiction buried in the list is replayed.
        let mut run = ScriptedRun::exiting(0);
        run.events = vec![
            ProcessEventKind::Started,
            ProcessEventKind::TerminationPhase(TerminationPhase::DeadlineReached),
            ProcessEventKind::StdoutBytes(StreamChunkEvidence {
                byte_count: 1,
                offset: 0,
                retained: true,
            }),
        ];
        assert_eq!(
            FakeSupervisor::new().script_run(run),
            Err(ScriptedRunContradiction::DeadlineReachedWithoutControl)
        );
    }

    #[test]
    fn the_first_contradiction_in_script_order_wins() {
        let mut run = ScriptedRun::exiting(0);
        run.events = vec![
            ProcessEventKind::Started,
            ProcessEventKind::TerminationPhase(TerminationPhase::DeadlineReached),
            ProcessEventKind::LimitReached(observation_limit(StreamChannel::Stdout, false)),
        ];
        assert_eq!(
            FakeSupervisor::new().script_run(run),
            Err(ScriptedRunContradiction::DeadlineReachedWithoutControl)
        );
    }

    #[test]
    fn other_control_activity_does_not_excuse_a_deadline_event() {
        // The wrong implementation this kills: treating any control-plane
        // activity as enough, so a deadline event settles as a cancellation.
        let run = with_event(ProcessEventKind::TerminationPhase(TerminationPhase::DeadlineReached))
            .with_control(ControlState {
                cancellation_requested: Some(CancellationReason::Shutdown),
                ..ControlState::default()
            });
        assert_eq!(
            FakeSupervisor::new().script_run(run),
            Err(ScriptedRunContradiction::DeadlineReachedWithoutControl)
        );
    }

    #[test]
    fn other_control_activity_does_not_excuse_a_cancellation_event() {
        let run = with_event(ProcessEventKind::TerminationPhase(
            TerminationPhase::CancellationRequested(CancellationReason::Shutdown),
        ))
        .with_control(ControlState { deadline_reached: true, ..ControlState::default() });
        assert_eq!(
            FakeSupervisor::new().script_run(run),
            Err(ScriptedRunContradiction::CancellationRequestedMismatch {
                event: CancellationReason::Shutdown,
                control: None,
            })
        );
    }

    #[test]
    fn other_control_activity_does_not_excuse_a_limit_event() {
        let run = with_event(ProcessEventKind::LimitReached(observation_limit(
            StreamChannel::Stdout,
            false,
        )))
        .with_control(ControlState { deadline_reached: true, ..ControlState::default() });
        assert_eq!(
            FakeSupervisor::new().script_run(run),
            Err(ScriptedRunContradiction::LimitReachedWithoutControl)
        );
    }

    #[test]
    fn operation_superseded_cancellation_event_requires_the_same_reason() {
        let matching = with_event(ProcessEventKind::TerminationPhase(
            TerminationPhase::CancellationRequested(CancellationReason::OperationSuperseded),
        ))
        .with_control(ControlState {
            cancellation_requested: Some(CancellationReason::OperationSuperseded),
            ..ControlState::default()
        });
        assert_eq!(FakeSupervisor::new().script_run(matching), Ok(()));

        let mismatched = with_event(ProcessEventKind::TerminationPhase(
            TerminationPhase::CancellationRequested(CancellationReason::OperationSuperseded),
        ))
        .with_control(ControlState {
            cancellation_requested: Some(CancellationReason::UserRequested),
            ..ControlState::default()
        });
        assert_eq!(
            FakeSupervisor::new().script_run(mismatched),
            Err(ScriptedRunContradiction::CancellationRequestedMismatch {
                event: CancellationReason::OperationSuperseded,
                control: Some(CancellationReason::UserRequested),
            })
        );
    }

    #[test]
    fn a_refused_start_is_not_subject_to_run_pairing() {
        assert_eq!(
            FakeSupervisor::new()
                .script(ScriptedOutcome::RefuseStart(TerminalDisposition::NotProven)),
            Ok(()),
            "a start refusal was subjected to run pairing"
        );
    }

    #[test]
    fn a_terminal_in_the_event_list_is_still_a_replay_malformation() {
        // Script-time pairing is not a second copy of the terminal-in-events
        // rule. That stays a replay refusal so this check cannot swallow it.
        let mut run = ScriptedRun::exiting(0);
        run.events = vec![
            ProcessEventKind::Started,
            ProcessEventKind::Terminal(TerminalDisposition::CompletedExit { code: 0 }),
        ];
        assert_eq!(
            FakeSupervisor::new().script_run(run),
            Ok(()),
            "a terminal-in-events fixture was refused at script time"
        );
    }

    #[test]
    fn a_rejected_script_does_not_queue_behind_a_later_valid_one() {
        let supervisor = FakeSupervisor::new();
        let bad = with_event(ProcessEventKind::TerminationPhase(TerminationPhase::DeadlineReached));
        assert!(supervisor.script_run(bad).is_err());
        assert_eq!(
            supervisor.script_run(ScriptedRun::exiting(0)),
            Ok(()),
            "a valid run after a rejected fixture was refused"
        );
        assert_eq!(queued_len(&supervisor), 1);
    }

    #[test]
    fn two_cancellation_reasons_in_one_script_must_both_match() {
        let mut run = ScriptedRun::exiting(0).with_control(ControlState {
            cancellation_requested: Some(CancellationReason::Shutdown),
            ..ControlState::default()
        });
        run.events = vec![
            ProcessEventKind::Started,
            ProcessEventKind::TerminationPhase(TerminationPhase::CancellationRequested(
                CancellationReason::Shutdown,
            )),
            ProcessEventKind::TerminationPhase(TerminationPhase::CancellationRequested(
                CancellationReason::UserRequested,
            )),
        ];
        assert_eq!(
            FakeSupervisor::new().script_run(run),
            Err(ScriptedRunContradiction::CancellationRequestedMismatch {
                event: CancellationReason::UserRequested,
                control: Some(CancellationReason::Shutdown),
            })
        );
    }
}

#[cfg(test)]
mod cancel_after_emitted_reason {
    use super::*;
    use crate::process::encoding::Fingerprint;

    type TestResult = Result<(), &'static str>;

    fn dummy_fingerprint() -> PlanFingerprint {
        PlanFingerprint::new(Fingerprint::of(b"fake-handle-cancel-test"))
    }

    fn cancellable_handle(run: ScriptedRun) -> FakeHandle {
        FakeHandle::new(
            PlanId::new("plan"),
            dummy_fingerprint(),
            RunId::new("run"),
            run,
            true,
            false,
            Arc::new(Mutex::new(Vec::new())),
            Arc::new(Mutex::new(Vec::new())),
        )
    }

    fn started_then_cancelled(reason: CancellationReason) -> ScriptedRun {
        let mut run = ScriptedRun::exiting(0).with_control(ControlState {
            cancellation_requested: Some(reason),
            started_before_cancellation: true,
            ..ControlState::default()
        });
        run.events = vec![
            ProcessEventKind::Started,
            ProcessEventKind::TerminationPhase(TerminationPhase::CancellationRequested(reason)),
        ];
        run
    }

    fn drain(handle: &mut FakeHandle) -> Vec<ProcessEventKind> {
        let mut kinds = Vec::new();
        while let Some(event) = handle.next_event() {
            kinds.push(event.kind().clone());
        }
        kinds
    }

    fn take_event(handle: &mut FakeHandle) -> Result<ProcessEvent, &'static str> {
        handle.next_event().ok_or("the fake emitted no further event")
    }

    fn poll_started_and_cancellation(
        handle: &mut FakeHandle,
        reason: CancellationReason,
    ) -> TestResult {
        let started = take_event(handle)?;
        if !matches!(started.kind(), ProcessEventKind::Started) {
            return Err("first event was not Started");
        }
        let phase = take_event(handle)?;
        if !is_cancellation(phase.kind(), reason) {
            return Err("second event was not the expected CancellationRequested");
        }
        Ok(())
    }

    fn is_cancellation(kind: &ProcessEventKind, reason: CancellationReason) -> bool {
        matches!(
            kind,
            ProcessEventKind::TerminationPhase(TerminationPhase::CancellationRequested(emitted))
                if *emitted == reason
        )
    }

    #[test]
    fn different_reason_after_scripted_phase_is_refused() -> TestResult {
        // The wrong implementation this kills: accepting UserRequested after
        // Shutdown was already admitted, so wait elects UserRequested while
        // the consumer already holds Shutdown.
        let mut handle = cancellable_handle(started_then_cancelled(CancellationReason::Shutdown));
        poll_started_and_cancellation(&mut handle, CancellationReason::Shutdown)?;

        assert_eq!(
            handle.cancel(CancellationReason::UserRequested),
            CancellationAcknowledgement::ContradictsEmittedReason
        );

        let kinds = drain(&mut handle);
        assert!(
            kinds.iter().all(|kind| !is_cancellation(kind, CancellationReason::UserRequested)),
            "refused cancel still emitted UserRequested: {kinds:?}"
        );
        assert!(
            matches!(
                kinds.last(),
                Some(ProcessEventKind::Terminal(TerminalDisposition::CancelledRunning(
                    CancellationReason::Shutdown
                )))
            ),
            "wait-equivalent terminal was {:?}",
            kinds.last()
        );

        let result = Box::new(handle).wait();
        assert_eq!(
            result.disposition(),
            &TerminalDisposition::CancelledRunning(CancellationReason::Shutdown)
        );
        assert_ne!(
            result.disposition(),
            &TerminalDisposition::SupervisorFailed,
            "a coherent run was supervisor-failed instead of keeping the emitted reason"
        );
        Ok(())
    }

    #[test]
    fn same_reason_after_scripted_phase_stays_accepted() -> TestResult {
        let mut handle = cancellable_handle(started_then_cancelled(CancellationReason::Shutdown));
        poll_started_and_cancellation(&mut handle, CancellationReason::Shutdown)?;

        assert_eq!(
            handle.cancel(CancellationReason::Shutdown),
            CancellationAcknowledgement::Accepted
        );

        let result = Box::new(handle).wait();
        assert_eq!(
            result.disposition(),
            &TerminalDisposition::CancelledRunning(CancellationReason::Shutdown)
        );
        Ok(())
    }

    #[test]
    fn pending_not_yet_admitted_phase_may_be_replaced() -> TestResult {
        // The wrong implementation this kills: treating a scripted pending
        // CancellationRequested as already emitted, so a live cancel with
        // a different reason is refused before the consumer has seen one.
        let mut handle = cancellable_handle(started_then_cancelled(CancellationReason::Shutdown));
        let started = take_event(&mut handle)?;
        if !matches!(started.kind(), ProcessEventKind::Started) {
            return Err("first event was not Started");
        }

        assert_eq!(
            handle.cancel(CancellationReason::UserRequested),
            CancellationAcknowledgement::Accepted
        );

        let kinds = drain(&mut handle);
        assert!(
            kinds.iter().all(|kind| !is_cancellation(kind, CancellationReason::Shutdown)),
            "unannounced Shutdown survived a legal live cancel: {kinds:?}"
        );
        assert!(
            kinds.iter().any(|kind| is_cancellation(kind, CancellationReason::UserRequested)),
            "legal live cancel did not emit UserRequested: {kinds:?}"
        );

        let result = Box::new(handle).wait();
        assert_eq!(
            result.disposition(),
            &TerminalDisposition::CancelledRunning(CancellationReason::UserRequested)
        );
        Ok(())
    }

    #[test]
    fn live_cancel_then_different_reason_after_the_phase_is_admitted_is_refused() -> TestResult {
        let mut handle = cancellable_handle(ScriptedRun::exiting(0));
        let started = take_event(&mut handle)?;
        if !matches!(started.kind(), ProcessEventKind::Started) {
            return Err("first event was not Started");
        }
        assert_eq!(
            handle.cancel(CancellationReason::UserRequested),
            CancellationAcknowledgement::Accepted
        );
        let phase = take_event(&mut handle)?;
        if !is_cancellation(phase.kind(), CancellationReason::UserRequested) {
            return Err("live cancel did not emit UserRequested");
        }

        assert_eq!(
            handle.cancel(CancellationReason::Shutdown),
            CancellationAcknowledgement::ContradictsEmittedReason
        );

        let result = Box::new(handle).wait();
        assert_eq!(
            result.disposition(),
            &TerminalDisposition::CancelledRunning(CancellationReason::UserRequested)
        );
        Ok(())
    }

    #[test]
    fn scripted_control_without_event_may_be_replaced_by_live_cancel() -> TestResult {
        // The #14556 converse: a control flag with no phase is scriptable.
        // Nothing has been announced, so live cancel may name a different
        // reason.
        let run = ScriptedRun::exiting(0).with_control(ControlState {
            cancellation_requested: Some(CancellationReason::Shutdown),
            ..ControlState::default()
        });
        let mut handle = cancellable_handle(run);
        let started = take_event(&mut handle)?;
        if !matches!(started.kind(), ProcessEventKind::Started) {
            return Err("first event was not Started");
        }

        assert_eq!(
            handle.cancel(CancellationReason::UserRequested),
            CancellationAcknowledgement::Accepted
        );

        let result = Box::new(handle).wait();
        assert_eq!(
            result.disposition(),
            &TerminalDisposition::CancelledRunning(CancellationReason::UserRequested)
        );
        Ok(())
    }

    #[test]
    fn refused_second_reason_does_not_drop_unpolled_scripted_events() -> TestResult {
        // The wrong implementation this kills: refusing by clearing pending,
        // so a later Started the consumer had not yet seen disappears.
        let mut run = ScriptedRun::exiting(0).with_control(ControlState {
            cancellation_requested: Some(CancellationReason::Shutdown),
            started_before_cancellation: false,
            ..ControlState::default()
        });
        run.events = vec![
            ProcessEventKind::TerminationPhase(TerminationPhase::CancellationRequested(
                CancellationReason::Shutdown,
            )),
            ProcessEventKind::Started,
        ];
        let mut handle = cancellable_handle(run);
        let phase = take_event(&mut handle)?;
        if !is_cancellation(phase.kind(), CancellationReason::Shutdown) {
            return Err("first event was not CancellationRequested(Shutdown)");
        }

        assert_eq!(
            handle.cancel(CancellationReason::OperationSuperseded),
            CancellationAcknowledgement::ContradictsEmittedReason
        );

        let kinds = drain(&mut handle);
        assert!(
            kinds.iter().any(|kind| matches!(kind, ProcessEventKind::Started)),
            "refused cancel dropped the unpolled Started: {kinds:?}"
        );
        assert!(
            kinds
                .iter()
                .all(|kind| !is_cancellation(kind, CancellationReason::OperationSuperseded)),
            "refused cancel still emitted OperationSuperseded: {kinds:?}"
        );

        let result = Box::new(handle).wait();
        assert_eq!(
            result.disposition(),
            &TerminalDisposition::CancelledBeforeStart(CancellationReason::Shutdown)
        );
        Ok(())
    }

    #[test]
    fn same_reason_after_a_pre_start_scripted_phase_still_reconciles() -> TestResult {
        // Existing contract: cancelling before Started is admitted must
        // still rewrite settlement even when the matching phase was already
        // emitted.
        let mut run = ScriptedRun::exiting(0).with_control(ControlState {
            cancellation_requested: Some(CancellationReason::Shutdown),
            ..ControlState::default()
        });
        run.events = vec![
            ProcessEventKind::TerminationPhase(TerminationPhase::CancellationRequested(
                CancellationReason::Shutdown,
            )),
            ProcessEventKind::Started,
        ];
        let mut handle = cancellable_handle(run);
        let phase = take_event(&mut handle)?;
        if !is_cancellation(phase.kind(), CancellationReason::Shutdown) {
            return Err("first event was not CancellationRequested(Shutdown)");
        }

        assert_eq!(
            handle.cancel(CancellationReason::Shutdown),
            CancellationAcknowledgement::Accepted
        );

        let kinds = drain(&mut handle);
        assert!(
            kinds.iter().all(|kind| !matches!(kind, ProcessEventKind::Started)),
            "same-reason pre-start cancel left Started in the stream: {kinds:?}"
        );

        let result = Box::new(handle).wait();
        assert_eq!(
            result.disposition(),
            &TerminalDisposition::CancelledBeforeStart(CancellationReason::Shutdown)
        );
        Ok(())
    }

    #[test]
    fn a_later_same_reason_cancel_is_still_legal_after_a_refused_one() -> TestResult {
        let mut handle = cancellable_handle(started_then_cancelled(CancellationReason::Shutdown));
        poll_started_and_cancellation(&mut handle, CancellationReason::Shutdown)?;
        assert_eq!(
            handle.cancel(CancellationReason::UserRequested),
            CancellationAcknowledgement::ContradictsEmittedReason
        );
        assert_eq!(
            handle.cancel(CancellationReason::Shutdown),
            CancellationAcknowledgement::Accepted
        );
        let result = Box::new(handle).wait();
        assert_eq!(
            result.disposition(),
            &TerminalDisposition::CancelledRunning(CancellationReason::Shutdown)
        );
        Ok(())
    }

    #[test]
    fn not_cancellable_still_wins_before_reason_comparison() -> TestResult {
        let mut handle = FakeHandle::new(
            PlanId::new("plan"),
            dummy_fingerprint(),
            RunId::new("run"),
            started_then_cancelled(CancellationReason::Shutdown),
            false,
            false,
            Arc::new(Mutex::new(Vec::new())),
            Arc::new(Mutex::new(Vec::new())),
        );
        poll_started_and_cancellation(&mut handle, CancellationReason::Shutdown)?;
        assert_eq!(
            handle.cancel(CancellationReason::UserRequested),
            CancellationAcknowledgement::NotCancellable
        );
        let result = Box::new(handle).wait();
        assert_eq!(
            result.disposition(),
            &TerminalDisposition::CancelledRunning(CancellationReason::Shutdown)
        );
        Ok(())
    }
}
