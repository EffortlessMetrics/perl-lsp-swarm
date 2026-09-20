//! Work tracker, canonical control port, and terminal state.

use super::receipt::{
    ReachabilityExhaustionAttempt, ReachabilityStageLimitation, ReachabilityWorkHonestyError,
    ReachabilityWorkPath, ReachabilityWorkPathTarget, ReachabilityWorkReceipt,
};
use super::{
    ReachabilityClaimLimitation, ReachabilityContractError, ReachabilityOperationSubject,
    ReachabilityStageId, ReachabilitySubjectIdentity, ReachabilityWorkBudget,
    ReachabilityWorkDimension,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A terminal condition observed through the canonical control.
///
/// Cancellation, deadline expiry, and supersession remain distinct: a newer
/// accepted workspace/root/configuration/profile subject yields
/// [`ReachabilityTerminalObservation::Superseded`], never generic
/// cancellation.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReachabilityTerminalObservation {
    /// Live cancellation was observed, with the exact control identity.
    Cancelled {
        /// The external control that cancelled the operation.
        control_identity: ReachabilitySubjectIdentity,
    },
    /// The operation deadline expired, with the exact deadline/profile
    /// identity.
    DeadlineExceeded {
        /// The deadline authority that expired.
        deadline_profile: ReachabilitySubjectIdentity,
    },
    /// A newer accepted subject superseded this operation.
    Superseded {
        /// The subject this operation expected to be current.
        expected: ReachabilitySubjectIdentity,
        /// The subject the authority actually accepted.
        observed: ReachabilitySubjectIdentity,
    },
}

impl ReachabilityTerminalObservation {
    /// Whether this observation is a cancellation (and nothing else).
    #[must_use]
    pub const fn is_cancellation(&self) -> bool {
        matches!(self, Self::Cancelled { .. })
    }

    /// Whether this observation is a deadline expiry (and nothing else).
    #[must_use]
    pub const fn is_deadline(&self) -> bool {
        matches!(self, Self::DeadlineExceeded { .. })
    }

    /// Whether this observation is a supersession (and nothing else).
    #[must_use]
    pub const fn is_supersession(&self) -> bool {
        matches!(self, Self::Superseded { .. })
    }
}

/// The latched terminal state of one operation.
///
/// Budget exhaustion and checked-arithmetic overflow are internal terminal
/// states, deliberately distinct from every external control observation:
/// exhaustion is never cancellation, deadline expiry, or supersession, and
/// overflow is typed instrument evidence, never wraparound.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReachabilityTerminalState {
    /// A terminal condition observed through the canonical control.
    External(ReachabilityTerminalObservation),
    /// A work-dimension limit was reached.
    ResourceExhausted {
        /// The dimension whose limit was hit.
        dimension: ReachabilityWorkDimension,
        /// The limit in force.
        limit: u64,
        /// Units already charged when the attempt was refused.
        charged: u64,
    },
    /// Checked arithmetic rejected a charge; the counter is unchanged and
    /// the failure is typed instrument evidence.
    CounterOverflow {
        /// The dimension whose counter would overflow.
        dimension: ReachabilityWorkDimension,
    },
}

impl ReachabilityTerminalState {
    /// Whether this terminal state is an external cancellation.
    #[must_use]
    pub const fn is_cancellation(&self) -> bool {
        matches!(self, Self::External(observation) if observation.is_cancellation())
    }

    /// Whether this terminal state is resource exhaustion (and nothing
    /// else).
    #[must_use]
    pub const fn is_resource_exhausted(&self) -> bool {
        matches!(self, Self::ResourceExhausted { .. })
    }
}

/// The canonical control port stages poll at declared checkpoints.
///
/// The owning runtime binds this port to its canonical external
/// cancellation/deadline authority (#7098/#10492/#10493). This contract
/// creates no request registry, timer, or clock; test seams inject
/// deterministic observations.
pub trait ReachabilityOperationControl {
    /// Observe the control for one subject at one checkpoint. Returning
    /// `None` means the operation may continue.
    fn poll(
        &self,
        subject: &ReachabilityOperationSubject,
    ) -> Option<ReachabilityTerminalObservation>;
}

/// Fail-closed result of charging one work dimension.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReachabilityChargeError {
    /// The charge would exceed the dimension limit; nothing beyond the limit
    /// is charged.
    Exhausted {
        /// The dimension whose limit was hit.
        dimension: ReachabilityWorkDimension,
        /// The limit in force.
        limit: u64,
        /// Units already charged when the attempt was refused.
        charged: u64,
    },
    /// Checked arithmetic rejected the charge; the counter is unchanged.
    CounterOverflow {
        /// The dimension whose counter would overflow.
        dimension: ReachabilityWorkDimension,
    },
}

impl std::fmt::Display for ReachabilityChargeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Exhausted { dimension, limit, charged } => write!(
                f,
                "work dimension `{}` exhausted its limit {limit} after {charged} units",
                dimension.as_str()
            ),
            Self::CounterOverflow { dimension } => {
                write!(f, "checked arithmetic rejected a charge to `{}`", dimension.as_str())
            }
        }
    }
}

impl std::error::Error for ReachabilityChargeError {}

/// One reachability operation's work tracker.
///
/// The tracker enforces the deterministic work budget with checked
/// arithmetic, records checkpoints, stage completions, limitations, and
/// work-honesty paths, and latches terminal state. Terminal state is sticky:
/// after a terminal observation, non-interruptible work may still be charged
/// (recorded as work after eligibility was lost) but the operation can never
/// publish.
#[derive(Debug, Clone)]
pub struct ReachabilityWorkTracker {
    subject: ReachabilityOperationSubject,
    budget: ReachabilityWorkBudget,
    charged: BTreeMap<ReachabilityWorkDimension, u64>,
    exhausted_attempts: Vec<ReachabilityExhaustionAttempt>,
    work_paths: Vec<ReachabilityWorkPath>,
    checkpoints_observed: Vec<ReachabilityStageId>,
    completed_stages: Vec<ReachabilityStageId>,
    stage_limitations: Vec<ReachabilityStageLimitation>,
    terminal: Option<ReachabilityTerminalState>,
    work_after_eligibility_lost: u64,
    work_after_eligibility_lost_overflow: bool,
    instrument_identity: Option<ReachabilitySubjectIdentity>,
    instrument_evidence_complete: bool,
}

impl ReachabilityWorkTracker {
    /// Start one tracked operation under its validated budget.
    ///
    /// # Errors
    ///
    /// Returns [`ReachabilityContractError::BudgetProfileMismatch`] when the
    /// tracker's budget profile does not match the subject's declared
    /// work-budget profile identity.
    pub fn new(
        subject: ReachabilityOperationSubject,
        budget: ReachabilityWorkBudget,
    ) -> Result<Self, ReachabilityContractError> {
        if subject.budget_profile_id() != budget.profile_id() {
            return Err(ReachabilityContractError::BudgetProfileMismatch);
        }
        Ok(Self {
            subject,
            budget,
            charged: BTreeMap::new(),
            exhausted_attempts: Vec::new(),
            work_paths: Vec::new(),
            checkpoints_observed: Vec::new(),
            completed_stages: Vec::new(),
            stage_limitations: Vec::new(),
            terminal: None,
            work_after_eligibility_lost: 0,
            work_after_eligibility_lost_overflow: false,
            instrument_identity: None,
            instrument_evidence_complete: false,
        })
    }

    /// The operation subject this tracker carries.
    #[must_use]
    pub fn subject(&self) -> &ReachabilityOperationSubject {
        &self.subject
    }

    /// The budget in force.
    #[must_use]
    pub fn budget(&self) -> &ReachabilityWorkBudget {
        &self.budget
    }

    /// The latched terminal state, if any. Terminal state is sticky for the
    /// remainder of the operation.
    #[must_use]
    pub fn terminal(&self) -> Option<&ReachabilityTerminalState> {
        self.terminal.as_ref()
    }

    /// Charge `units` of work against one dimension with checked
    /// arithmetic.
    ///
    /// Charging is monotonic: counters never decrease within an operation,
    /// and a refused charge never partially applies.
    ///
    /// # Errors
    ///
    /// Returns [`ReachabilityChargeError::Exhausted`] when the charge would
    /// exceed the dimension limit — the attempt is recorded and, absent an
    /// earlier terminal, latches [`ReachabilityTerminalState::ResourceExhausted`]
    /// — or [`ReachabilityChargeError::CounterOverflow`] on checked-arithmetic
    /// overflow, which latches
    /// [`ReachabilityTerminalState::CounterOverflow`].
    pub fn charge(
        &mut self,
        dimension: ReachabilityWorkDimension,
        units: u64,
    ) -> Result<(), ReachabilityChargeError> {
        if units == 0 {
            return Ok(());
        }
        let current = self.charged.get(&dimension).copied().unwrap_or(0);
        let Some(total) = current.checked_add(units) else {
            self.latch_terminal(ReachabilityTerminalState::CounterOverflow { dimension });
            return Err(ReachabilityChargeError::CounterOverflow { dimension });
        };
        let limit = match self.budget.limit_for(dimension) {
            Some(super::ReachabilityDimensionLimit::Bounded(limit)) => Some(limit),
            // Unlimited dimensions still carry the reviewed safety bound.
            Some(super::ReachabilityDimensionLimit::Unlimited { safety_bound }) => {
                Some(safety_bound)
            }
            None => None,
        };
        if let Some(limit) = limit
            && total > limit
        {
            self.exhausted_attempts.push(ReachabilityExhaustionAttempt {
                dimension,
                limit,
                charged: current,
            });
            self.latch_terminal(ReachabilityTerminalState::ResourceExhausted {
                dimension,
                limit,
                charged: current,
            });
            return Err(ReachabilityChargeError::Exhausted { dimension, limit, charged: current });
        }
        if self.terminal.is_some() {
            match self.work_after_eligibility_lost.checked_add(units) {
                Some(total_after) => self.work_after_eligibility_lost = total_after,
                None => {
                    // The terminal is already latched, so the receipt is
                    // already non-publishable; the receipt still records that
                    // the post-eligibility accounting itself overflowed
                    // instead of silently under-reporting charged work.
                    self.work_after_eligibility_lost_overflow = true;
                }
            }
        }
        self.charged.insert(dimension, total);
        Ok(())
    }

    /// Latch a terminal state unless one is already latched.
    fn latch_terminal(&mut self, state: ReachabilityTerminalState) {
        if self.terminal.is_none() {
            self.terminal = Some(state);
        }
    }

    /// Observe the canonical control at one declared checkpoint.
    ///
    /// Returns the latched terminal state when the control reports one (or
    /// when a terminal was already latched).
    pub fn poll_checkpoint(
        &mut self,
        stage: &ReachabilityStageId,
        control: &dyn ReachabilityOperationControl,
    ) -> Option<ReachabilityTerminalState> {
        self.checkpoints_observed.push(stage.clone());
        if self.terminal.is_some() {
            return self.terminal.clone();
        }
        let observed = control.poll(&self.subject).map(ReachabilityTerminalState::External);
        if observed.is_some() {
            self.terminal = observed;
        }
        self.terminal.clone()
    }

    /// Complete one stage, appending its exact output identity and
    /// limitations.
    ///
    /// Limitations accumulate: no later call can remove them, and the
    /// subject's stage outputs are append-only.
    pub fn complete_stage(
        &mut self,
        stage: ReachabilityStageId,
        output: Option<ReachabilitySubjectIdentity>,
        limitations: Vec<ReachabilityClaimLimitation>,
    ) {
        if let Some(output) = output {
            self.subject.append_stage_output(stage.clone(), output);
        }
        if !self.completed_stages.contains(&stage) {
            self.completed_stages.push(stage.clone());
        }
        for limitation in limitations {
            self.stage_limitations
                .push(ReachabilityStageLimitation { stage: stage.clone(), limitation });
        }
    }

    /// Record a full construction for one target.
    ///
    /// # Errors
    ///
    /// Returns a work-honesty error when a validated reuse was already
    /// recorded for the same target.
    pub fn record_full_construction(
        &mut self,
        stage: ReachabilityStageId,
        target: ReachabilityWorkPathTarget,
    ) -> Result<(), ReachabilityContractError> {
        if self.work_paths.iter().any(|path| *path.target() == target && path.is_validated_reuse())
        {
            return Err(ReachabilityContractError::WorkHonesty(
                ReachabilityWorkHonestyError::FullConstructionAfterValidatedReuse { stage },
            ));
        }
        self.work_paths.push(ReachabilityWorkReceipt::work_path_record(stage, target, None));
        Ok(())
    }

    /// Record a validated reuse, requiring the reused identity to be a
    /// declared identity of the operation subject.
    ///
    /// # Errors
    ///
    /// Returns a work-honesty error when the reused identity is not declared
    /// by the subject — a matching digest without the declared current
    /// identity is not work avoided.
    pub fn record_validated_reuse(
        &mut self,
        stage: ReachabilityStageId,
        target: ReachabilityWorkPathTarget,
        reused_identity: ReachabilitySubjectIdentity,
    ) -> Result<(), ReachabilityContractError> {
        let declared = self.subject.identities().contains(&reused_identity)
            || self
                .subject
                .stage_outputs()
                .iter()
                .any(|output| *output.output() == reused_identity);
        if !declared {
            return Err(ReachabilityContractError::WorkHonesty(
                ReachabilityWorkHonestyError::ReuseWithoutDeclaredIdentity { stage },
            ));
        }
        let record =
            ReachabilityWorkReceipt::work_path_record(stage, target, Some(reused_identity));
        self.work_paths.push(record);
        Ok(())
    }

    /// Record the instrument identity and complete instrument evidence.
    ///
    /// A missing instrument is never zero work; without this note the
    /// finished receipt cannot support an exact claim.
    pub fn note_instrument_evidence(&mut self, instrument: ReachabilitySubjectIdentity) {
        self.instrument_evidence_complete = true;
        self.instrument_identity = Some(instrument);
    }

    /// Finish the operation and produce its canonical receipt.
    #[must_use]
    pub fn finish(self) -> ReachabilityWorkReceipt {
        ReachabilityWorkReceipt::from_parts(
            self.charged,
            self.exhausted_attempts,
            self.work_paths,
            self.checkpoints_observed,
            self.completed_stages,
            self.stage_limitations,
            self.terminal,
            self.work_after_eligibility_lost,
            self.work_after_eligibility_lost_overflow,
            self.instrument_identity,
            self.instrument_evidence_complete,
        )
    }
}
