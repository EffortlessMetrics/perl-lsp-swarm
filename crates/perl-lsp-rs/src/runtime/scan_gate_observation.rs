//! Instance-owned observations for diagnosing indexing commit-gate test failures.
//!
//! This module is compiled only for workspace unit tests. Events describe one
//! admitted scan; neither an open gate channel nor a timeout proves liveness.

use std::fmt;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
enum EventKind {
    Admitted,
    WorkerStarted,
    FirstCommitGate,
    Exited,
}

struct Event {
    kind: EventKind,
    at: Instant,
}

/// Registration consumed once by the next admitted scan on its server.
pub(super) struct ScanObservationRegistration {
    events: Sender<Event>,
}

/// Guard owned by the observed scan, including all of its early-return paths.
pub(super) struct ObservedScan {
    events: Sender<Event>,
    commit_gate_reached: bool,
}

/// Receiving end retained by the test that installed the observation.
pub(super) struct ScanGateObservation {
    events: Receiver<Event>,
    seen: Vec<Event>,
    disconnected: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct ScanSnapshot {
    admitted: bool,
    started: bool,
    commit_gate_reached: bool,
    exited: bool,
}

impl ScanSnapshot {
    pub(super) fn state(self) -> &'static str {
        match (self.admitted, self.started, self.commit_gate_reached, self.exited) {
            (_, _, true, true) => "exited_after_first_commit_gate",
            (_, true, false, true) => "exited_before_first_commit_gate",
            (true, false, false, true) => "exited_before_worker_start",
            (_, _, true, false) => "first_commit_gate_observed",
            (_, true, false, false) => "worker_started_no_exit_observed",
            (true, false, false, false) => "admitted_no_worker_start_observed",
            _ => "no_scan_admission_observed",
        }
    }
}

#[derive(Debug)]
pub(super) struct GateWaitFailure {
    received: RecvTimeoutError,
    elapsed: Duration,
    at_deadline: ScanSnapshot,
    after_cleanup: ScanSnapshot,
    observation_disconnected: bool,
}

impl fmt::Display for GateWaitFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "scan commit-gate wait failed: receive={:?}; elapsed={:?}; at_deadline={}; after_cleanup={}; late_first_commit_gate={}; observation_disconnected={}",
            self.received,
            self.elapsed,
            self.at_deadline.state(),
            self.after_cleanup.state(),
            !self.at_deadline.commit_gate_reached && self.after_cleanup.commit_gate_reached,
            self.observation_disconnected,
        )
    }
}

impl std::error::Error for GateWaitFailure {}

pub(super) fn observe_scan() -> (ScanObservationRegistration, ScanGateObservation) {
    let (events, receiver) = mpsc::channel();
    (
        ScanObservationRegistration { events },
        ScanGateObservation { events: receiver, seen: Vec::new(), disconnected: false },
    )
}

impl ScanObservationRegistration {
    pub(super) fn admitted(self) -> ObservedScan {
        let scan = ObservedScan { events: self.events, commit_gate_reached: false };
        scan.emit(EventKind::Admitted);
        scan
    }
}

impl ObservedScan {
    fn emit(&self, kind: EventKind) {
        let _ = self.events.send(Event { kind, at: Instant::now() });
    }

    pub(super) fn worker_started(&self) {
        self.emit(EventKind::WorkerStarted);
    }

    pub(super) fn first_commit_gate(&mut self) {
        if !self.commit_gate_reached {
            self.commit_gate_reached = true;
            self.emit(EventKind::FirstCommitGate);
        }
    }
}

impl Drop for ObservedScan {
    fn drop(&mut self) {
        self.emit(EventKind::Exited);
    }
}

impl ScanGateObservation {
    fn drain(&mut self) {
        loop {
            match self.events.try_recv() {
                Ok(event) => self.seen.push(event),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.disconnected = true;
                    break;
                }
            }
        }
    }

    pub(super) fn snapshot_at(&mut self, deadline: Instant) -> ScanSnapshot {
        self.drain();
        let mut snapshot = ScanSnapshot::default();
        for event in self.seen.iter().filter(|event| event.at <= deadline) {
            match event.kind {
                EventKind::Admitted => snapshot.admitted = true,
                EventKind::WorkerStarted => snapshot.started = true,
                EventKind::FirstCommitGate => snapshot.commit_gate_reached = true,
                EventKind::Exited => snapshot.exited = true,
            }
        }
        snapshot
    }

    pub(super) fn wait_for_exit(&mut self, budget: Duration) {
        let started = Instant::now();
        loop {
            self.drain();
            if self.disconnected
                || self.seen.iter().any(|event| matches!(event.kind, EventKind::Exited))
            {
                break;
            }
            match self.events.recv_timeout(budget.saturating_sub(started.elapsed())) {
                Ok(event) => self.seen.push(event),
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => {
                    self.disconnected = true;
                    break;
                }
            }
        }
    }

    /// Always returns the original failure, even if cleanup observes a late gate.
    pub(super) fn failed_gate_wait(
        &mut self,
        received: RecvTimeoutError,
        deadline: Instant,
        elapsed: Duration,
        release: &Sender<()>,
        cleanup_budget: Duration,
    ) -> GateWaitFailure {
        let at_deadline = self.snapshot_at(deadline);
        let _ = release.send(());
        self.wait_for_exit(cleanup_budget);
        let after_cleanup = self.snapshot_at(Instant::now());
        GateWaitFailure {
            received,
            elapsed,
            at_deadline,
            after_cleanup,
            observation_disconnected: self.disconnected,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn exit_before_commit_is_visible_with_the_gate_sender_retained() -> TestResult {
        let (_gate_sender, gate_receiver) = mpsc::channel::<()>();
        let (registration, mut observation) = observe_scan();
        let scan = registration.admitted();
        scan.worker_started();
        drop(scan);
        if gate_receiver.try_recv() != Err(TryRecvError::Empty) {
            return Err("control gate sender was not retained".into());
        }
        if observation.snapshot_at(Instant::now()).state() != "exited_before_first_commit_gate" {
            return Err("retained gate sender hid the observed scan exit".into());
        }
        Ok(())
    }

    #[test]
    fn no_observed_exit_remains_unknown_at_a_failed_deadline() -> TestResult {
        let (registration, mut observation) = observe_scan();
        let scan = registration.admitted();
        scan.worker_started();
        let (release, _receiver) = mpsc::channel();
        let failure = observation.failed_gate_wait(
            RecvTimeoutError::Timeout,
            Instant::now(),
            Duration::from_secs(5),
            &release,
            Duration::ZERO,
        );
        if failure.at_deadline.state() != "worker_started_no_exit_observed"
            || failure.after_cleanup.exited
            || failure.observation_disconnected
        {
            return Err(format!("timeout invented scan termination: {failure}").into());
        }
        drop(scan);
        Ok(())
    }

    #[test]
    fn late_commit_during_cleanup_never_clears_the_original_timeout() -> TestResult {
        let (registration, mut observation) = observe_scan();
        let mut scan = registration.admitted();
        scan.worker_started();
        let (_gate_sender, gate_receiver) = mpsc::channel::<()>();
        let received = match gate_receiver.recv_timeout(Duration::ZERO) {
            Err(error) => error,
            Ok(()) => return Err("control unexpectedly reached its gate".into()),
        };
        let deadline = Instant::now();
        let (release, receiver) = mpsc::channel();
        let worker = std::thread::spawn(move || -> Result<(), RecvTimeoutError> {
            receiver.recv_timeout(Duration::from_secs(2))?;
            scan.first_commit_gate();
            Ok(())
        });
        let failure = observation.failed_gate_wait(
            received,
            deadline,
            Duration::from_secs(5),
            &release,
            Duration::from_secs(2),
        );
        worker.join().map_err(|_| "cleanup control thread panicked")??;
        if failure.received != RecvTimeoutError::Timeout
            || failure.at_deadline.commit_gate_reached
            || failure.after_cleanup.state() != "exited_after_first_commit_gate"
            || !failure.to_string().contains("late_first_commit_gate=true")
        {
            return Err(format!("late gate erased the original deadline failure: {failure}").into());
        }
        Ok(())
    }

    #[test]
    fn scan_owners_do_not_share_exit_or_commit_observations() -> TestResult {
        let (first, mut first_observation) = observe_scan();
        let (second, mut second_observation) = observe_scan();
        let first = first.admitted();
        let mut second = second.admitted();
        first.worker_started();
        second.worker_started();
        drop(first);
        if first_observation.snapshot_at(Instant::now()).state()
            != "exited_before_first_commit_gate"
            || second_observation.snapshot_at(Instant::now()).state()
                != "worker_started_no_exit_observed"
        {
            return Err("one scan changed another owner's observation".into());
        }
        second.first_commit_gate();
        second.first_commit_gate();
        drop(second);
        if second_observation.snapshot_at(Instant::now()).state()
            != "exited_after_first_commit_gate"
            || second_observation
                .seen
                .iter()
                .filter(|event| matches!(event.kind, EventKind::FirstCommitGate))
                .count()
                != 1
        {
            return Err("first commit was not recorded exactly once for its owner".into());
        }
        Ok(())
    }

    #[test]
    fn dropped_registration_does_not_invent_an_admitted_scan_exit() -> TestResult {
        let (registration, mut observation) = observe_scan();
        drop(registration);
        if observation.snapshot_at(Instant::now()).state() != "no_scan_admission_observed"
            || !observation.disconnected
        {
            return Err("dropped instrumentation invented a scan lifecycle".into());
        }
        Ok(())
    }

    #[test]
    fn disconnected_gate_error_is_preserved() -> TestResult {
        let (_registration, mut observation) = observe_scan();
        let (gate_sender, gate_receiver) = mpsc::channel::<()>();
        drop(gate_sender);
        let received = match gate_receiver.recv_timeout(Duration::ZERO) {
            Err(error) => error,
            Ok(()) => return Err("disconnected control unexpectedly received a gate".into()),
        };
        let (release, _receiver) = mpsc::channel();
        let failure = observation.failed_gate_wait(
            received,
            Instant::now(),
            Duration::ZERO,
            &release,
            Duration::ZERO,
        );
        if failure.received != RecvTimeoutError::Disconnected {
            return Err(format!("gate disconnection was changed into a timeout: {failure}").into());
        }
        Ok(())
    }

    #[test]
    fn deadline_snapshot_excludes_events_emitted_after_the_deadline() -> TestResult {
        let origin = Instant::now();
        let later = origin.checked_add(Duration::from_millis(2)).ok_or("test instant overflow")?;
        let deadline =
            origin.checked_add(Duration::from_millis(1)).ok_or("test instant overflow")?;
        let (registration, mut observation) = observe_scan();
        for (kind, at) in [
            (EventKind::Admitted, origin),
            (EventKind::WorkerStarted, origin),
            (EventKind::FirstCommitGate, later),
        ] {
            registration.events.send(Event { kind, at })?;
        }
        if observation.snapshot_at(deadline).state() != "worker_started_no_exit_observed"
            || observation.snapshot_at(later).state() != "first_commit_gate_observed"
        {
            return Err("a later event contaminated the deadline snapshot".into());
        }
        Ok(())
    }
}
