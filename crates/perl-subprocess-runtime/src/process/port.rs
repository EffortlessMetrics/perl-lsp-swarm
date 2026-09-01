//! The supervisor and handle ports.
//!
//! Domain consumers depend on these traits and the surrounding value types.
//! They never depend on `std::process`, an async runtime, or a platform
//! backend — which is what lets a consumer be tested deterministically before
//! any operating-system lane exists.

use super::event::ProcessEvent;
use super::result::{CancellationReason, ProcessResult};
use super::validation::ValidatedProcessPlan;

/// Starts supervised processes.
///
/// The port accepts only a [`ValidatedProcessPlan`], so there is no way to
/// reach a production backend with an unvalidated plan.
pub trait ProcessSupervisor: Send + Sync {
    /// Attempt to start a run.
    ///
    /// Every start attempt settles exactly once: either a handle is returned
    /// and settles through [`ProcessHandle::wait`], or the attempt settles
    /// immediately as a terminal [`ProcessResult`] describing the refusal.
    /// There is no third outcome and no silent nothing.
    fn start(
        &self,
        plan: ValidatedProcessPlan,
    ) -> Result<Box<dyn ProcessHandle>, Box<ProcessResult>>;
}

/// Whether an acknowledged cancellation can be acted on.
///
/// Acknowledging a request is not performing it, and it is never cleanup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancellationAcknowledgement {
    /// The request was accepted; termination will proceed.
    Accepted,
    /// The run had already settled, so the request had no effect.
    AlreadySettled,
    /// The plan declared the run non-cancellable.
    NotCancellable,
}

/// What is known about a handle that was dropped rather than awaited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandleDropDisposition {
    /// The handle settled before being dropped.
    SettledBeforeDrop,
    /// The handle was dropped with admitted work outstanding.
    ///
    /// Never evidence of cleanup: a backend that cannot prove it reaped the
    /// child must report [`super::CleanupDisposition::NotObserved`].
    AbandonedWithoutSettlement,
}

/// A live run.
///
/// # Drop contract
///
/// Dropping a handle without calling [`ProcessHandle::wait`] abandons the run.
/// It does **not** mean the child exited, that cleanup succeeded, or that a
/// result exists. A backend must record
/// [`HandleDropDisposition::AbandonedWithoutSettlement`] and must not
/// synthesize a successful result for the abandoned run.
///
/// [`ProcessHandle::wait`] consumes the handle, so a run cannot be settled
/// twice through this port.
pub trait ProcessHandle: Send {
    /// The run's identity.
    fn run_id(&self) -> &super::identity::RunId;

    /// Take the next event, or `None` once the stream is exhausted.
    fn next_event(&mut self) -> Option<ProcessEvent>;

    /// Request cancellation.
    fn cancel(&mut self, reason: CancellationReason) -> CancellationAcknowledgement;

    /// Settle the run and produce its terminal result.
    fn wait(self: Box<Self>) -> ProcessResult;
}
