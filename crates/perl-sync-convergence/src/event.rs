//! Append-only convergence event journal and deterministic replay.
//!
//! A fresh process reconstructs current state and next legal actions by
//! folding the journal alone; no conversation, shell, or worktree-local
//! memory is consulted (issue #11282 acceptance).
//!
//! Replay is fail-closed: every rule violation aborts reconstruction with a
//! positioned [`ReplayError`] instead of guessing.

use crate::ids::{GenerationId, TransactionId};
use crate::invalidation::InvalidationRecord;
use crate::lease::{Lease, Takeover, TimestampMs};
use crate::model::{Direction, ReleaseContextMode};
use crate::state::{PermittedAction, TransitionState};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

/// Schema version of the journal event format.
pub const JOURNAL_SCHEMA_VERSION: u32 = 1;

/// One durable convergence event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum ConvergenceEvent {
    /// Opens a transaction with fixed direction and release mode.
    TransactionOpened {
        /// Owning transaction.
        transaction_id: TransactionId,
        /// Fixed direction.
        direction: Direction,
        /// Fixed release-context mode.
        release_mode: ReleaseContextMode,
        /// Prior accepted generation this chain continues, when any.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prior_accepted_generation: Option<GenerationId>,
        /// Open instant.
        opened_at: TimestampMs,
    },
    /// Starts one generation inside an open transaction.
    GenerationStarted {
        /// Owning transaction.
        transaction_id: TransactionId,
        /// Started generation.
        generation_id: GenerationId,
        /// Exact source parent SHA at start.
        source_parent_sha: String,
        /// Exact swarm parent SHA at start.
        swarm_parent_sha: String,
        /// Start instant.
        started_at: TimestampMs,
    },
    /// Records one legal lifecycle transition with evidence.
    ///
    /// Rejection, supersession, and no-op have dedicated events; this variant
    /// carries forward progress and explicit `not_proven` /
    /// `instrument_failure` outcomes so unresolved evidence can never be
    /// rewritten into a passing state.
    TransitionRecorded {
        /// Owning transaction.
        transaction_id: TransactionId,
        /// Transitioned generation.
        generation_id: GenerationId,
        /// Resulting state.
        to: TransitionState,
        /// Digest binding evidence to this transition.
        evidence_digest: String,
        /// Recording instant.
        recorded_at: TimestampMs,
    },
    /// Records immutable rejection evidence for a generation.
    RejectionRecorded {
        /// Owning transaction.
        transaction_id: TransactionId,
        /// Rejected generation.
        generation_id: GenerationId,
        /// Digest of the immutable rejection evidence.
        evidence_digest: String,
        /// Recording instant.
        recorded_at: TimestampMs,
    },
    /// Records a no-op receipt without opening a PR.
    NoOpRecorded {
        /// Owning transaction.
        transaction_id: TransactionId,
        /// Generation that turned out to be a no-op.
        generation_id: GenerationId,
        /// Digest of the equivalence evidence.
        evidence_digest: String,
        /// Recording instant.
        recorded_at: TimestampMs,
    },
    /// Supersedes one active generation with an explicit successor.
    GenerationSuperseded {
        /// Owning transaction.
        transaction_id: TransactionId,
        /// Displaced generation.
        old_generation: GenerationId,
        /// Successor generation.
        successor_generation: GenerationId,
        /// Digest of the supersession rationale.
        reason_digest: String,
        /// Supersession instant.
        superseded_at: TimestampMs,
    },
    /// Claims the writer lease.
    LeaseClaimed {
        /// Owning transaction.
        transaction_id: TransactionId,
        /// The claimed lease.
        lease: Lease,
    },
    /// Extends the live lease after a heartbeat.
    LeaseHeartbeat {
        /// Owning transaction.
        transaction_id: TransactionId,
        /// Claimant performing the heartbeat.
        claimed_by: String,
        /// Heartbeat instant.
        heartbeat_at: TimestampMs,
        /// New expiry instant.
        new_expires_at: TimestampMs,
    },
    /// Takes over an expired lease after exact-state reconciliation.
    TakeoverRecorded {
        /// Owning transaction.
        transaction_id: TransactionId,
        /// Validated takeover record.
        takeover: Takeover,
        /// Fresh lease held by the new claimant.
        new_lease: Lease,
    },
    /// Records invalidation causes and stale descendants.
    InvalidationRecorded {
        /// Owning transaction.
        transaction_id: TransactionId,
        /// The invalidation record.
        record: InvalidationRecord,
    },
}

impl ConvergenceEvent {
    /// Transaction this event belongs to.
    #[must_use]
    pub fn transaction_id(&self) -> &TransactionId {
        match self {
            Self::TransactionOpened { transaction_id, .. }
            | Self::GenerationStarted { transaction_id, .. }
            | Self::TransitionRecorded { transaction_id, .. }
            | Self::RejectionRecorded { transaction_id, .. }
            | Self::NoOpRecorded { transaction_id, .. }
            | Self::GenerationSuperseded { transaction_id, .. }
            | Self::LeaseClaimed { transaction_id, .. }
            | Self::LeaseHeartbeat { transaction_id, .. }
            | Self::TakeoverRecorded { transaction_id, .. }
            | Self::InvalidationRecorded { transaction_id, .. } => transaction_id,
        }
    }
}

/// Replay failure with the offending journal position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayError {
    /// Zero-based journal position of the rejected event.
    pub line: usize,
    /// Why the fold refused the event.
    pub kind: ReplayErrorKind,
}

impl fmt::Display for ReplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "journal entry {}: {}", self.line, self.kind)
    }
}

impl std::error::Error for ReplayError {}

/// Specific replay refusals; every variant is fail-closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayErrorKind {
    /// Journal was empty or began with something other than `TransactionOpened`.
    MissingOpen,
    /// Transaction was opened twice.
    DuplicateOpen,
    /// Event references a different transaction than the journal's owner.
    ForeignTransaction,
    /// Event references an unknown generation.
    UnknownGeneration,
    /// Generation was started twice.
    DuplicateGeneration,
    /// Another non-terminal generation already exists; a transaction carries
    /// at most one current generation at a time, regardless of source parent
    /// (negative control 2).
    ConcurrentActiveGeneration {
        /// Existing active generation identity.
        existing: String,
    },
    /// Transition is outside the closed legal-transition graph.
    IllegalTransition {
        /// Current state spelling.
        from: &'static str,
        /// Attempted next state spelling.
        to: &'static str,
    },
    /// Generation already reached a terminal state.
    TerminalGeneration,
    /// Rejection evidence conflicts with prior records.
    RejectionConflict,
    /// A live (unexpired) lease already exists.
    LiveLeaseExists,
    /// Heartbeat from a non-claimant or on a missing/expired lease.
    HeartbeatMismatch,
    /// A replayed or claimed lease violates its structural laws: empty
    /// claimant, backdated heartbeat, or expiry at/before the heartbeat.
    InvalidLease(String),
    /// Takeover attempted against a live or mismatched displaced lease, or
    /// the reconciliation record is incomplete.
    InvalidTakeover(String),
    /// Supersession target was unknown, terminal, or already superseded.
    InvalidSupersession,
}

impl fmt::Display for ReplayErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingOpen => f.write_str("journal must begin with transaction_opened"),
            Self::DuplicateOpen => f.write_str("transaction opened more than once"),
            Self::ForeignTransaction => f.write_str("event belongs to another transaction"),
            Self::UnknownGeneration => f.write_str("event references an unknown generation"),
            Self::DuplicateGeneration => f.write_str("generation started twice"),
            Self::ConcurrentActiveGeneration { existing } => {
                write!(
                    f,
                    "generation {existing} is still active; successors require a terminal predecessor"
                )
            }
            Self::IllegalTransition { from, to } => write!(f, "illegal transition {from} -> {to}"),
            Self::TerminalGeneration => f.write_str("generation is terminal"),
            Self::RejectionConflict => {
                f.write_str("rejection evidence conflicts with prior records")
            }
            Self::LiveLeaseExists => f.write_str("a live lease already exists"),
            Self::HeartbeatMismatch => f.write_str("heartbeat does not match the live claimant"),
            Self::InvalidLease(why) => write!(f, "invalid lease: {why}"),
            Self::InvalidTakeover(why) => write!(f, "invalid takeover: {why}"),
            Self::InvalidSupersession => {
                f.write_str("supersession target must be an active generation")
            }
        }
    }
}

fn illegal(from: TransitionState, to: TransitionState) -> ReplayErrorKind {
    ReplayErrorKind::IllegalTransition { from: from.as_str(), to: to.as_str() }
}

/// Whether `from -> to` is legal in the closed transition graph.
///
/// Terminal states have no outgoing edges. Rejection, supersession, and no-op
/// are reachable only through their dedicated events, so later green checks
/// cannot rewrite prior rejected evidence (negative control 5).
///
/// `not_proven` and `instrument_failure` are honest terminal outcomes that
/// may be recorded mid-flight; they are never bypassed toward success.
#[must_use]
pub fn is_legal_transition(from: TransitionState, to: TransitionState) -> bool {
    use TransitionState as S;
    if from.is_terminal() {
        return false;
    }
    matches!(
        (from, to),
        (S::Observed, S::Planned)
            | (S::Planned, S::Materialized)
            | (S::Materialized, S::Published)
            | (S::Published, S::AdmissionPending)
            | (S::AdmissionPending, S::Admitted)
            | (S::Admitted, S::MergePending)
            | (S::MergePending, S::Merged)
            | (S::Merged, S::PostMergeVerified)
            // Honest unresolved outcomes are recordable mid-flight.
            | (_, S::NotProven | S::InstrumentFailure)
    )
}

/// Writer actions permitted while a generation sits in `state`.
///
/// Merge and ref-mutation authority is deliberately absent from the lease
/// action vocabulary: landing actions derive from admission state observed by
/// controllers, never from a live lease grant.
#[must_use]
pub fn permitted_writer_actions(state: TransitionState) -> Vec<PermittedAction> {
    use PermittedAction as A;
    use TransitionState as S;
    match state {
        S::Observed => vec![A::PlanCandidate, A::StartSuccessorGeneration],
        S::Planned => vec![A::MaterializeCandidate],
        S::Materialized => vec![A::PublishCandidate],
        S::Published | S::AdmissionPending => vec![A::AwaitAdmission],
        S::Admitted => vec![],
        S::MergePending | S::Merged => vec![A::VerifyLanding],
        S::Rejected | S::NotProven | S::InstrumentFailure => vec![A::StartSuccessorGeneration],
        S::Superseded | S::Noop | S::PostMergeVerified => vec![],
    }
}

/// Runtime view of one generation reconstructed from the journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationRuntime {
    /// Current lifecycle state.
    pub state: TransitionState,
    /// Exact source parent SHA at generation start.
    pub source_parent_sha: String,
    /// Exact swarm parent SHA at generation start.
    pub swarm_parent_sha: String,
    /// Explicit successor when superseded.
    pub successor: Option<GenerationId>,
    /// Digest of the last recorded evidence.
    pub last_evidence_digest: Option<String>,
    /// Immutable rejection evidence digest when rejected.
    pub rejection_evidence_digest: Option<String>,
}

impl GenerationRuntime {
    /// Next writer actions implied by the reconstructed state.
    #[must_use]
    pub fn next_actions(&self) -> Vec<PermittedAction> {
        permitted_writer_actions(self.state)
    }
}

/// Reconstructed transaction view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConvergenceView {
    /// Owning transaction.
    pub transaction_id: TransactionId,
    /// Fixed direction.
    pub direction: Direction,
    /// Fixed release-context mode.
    pub release_mode: ReleaseContextMode,
    /// Prior accepted generation, when chained.
    pub prior_accepted_generation: Option<GenerationId>,
    /// All generations known to the journal, keyed by identity.
    pub generations: BTreeMap<GenerationId, GenerationRuntime>,
    /// Current lease, live or expired-but-unreclaimed, when present.
    pub lease: Option<Lease>,
    /// Every invalidation record in order.
    pub invalidations: Vec<InvalidationRecord>,
}

impl ConvergenceView {
    /// The single active generation, if exactly one is active.
    ///
    /// Returns `None` when every generation reached a terminal state; an
    /// ambiguous multi-active journal cannot happen because replay refuses
    /// starting a generation while any non-terminal predecessor exists,
    /// regardless of its source parent.
    #[must_use]
    pub fn active_generation(&self) -> Option<&GenerationRuntime> {
        let mut active =
            self.generations.values().filter(|g| !g.state.is_terminal() && g.successor.is_none());
        let first = active.next()?;
        if active.next().is_some() {
            return None;
        }
        Some(first)
    }

    /// Whether the current lease exists and is expired at `now`.
    #[must_use]
    pub fn lease_expired_at(&self, now: TimestampMs) -> bool {
        self.lease.as_ref().is_some_and(|l| l.is_expired(now))
    }
}

/// Fold one ordered journal into a reconstructed view.
pub fn replay(events: &[ConvergenceEvent]) -> Result<ConvergenceView, ReplayError> {
    let mut view: Option<ConvergenceView> = None;

    for (line, event) in events.iter().enumerate() {
        if let Some(open_view) = view.as_ref()
            && event.transaction_id() != &open_view.transaction_id
        {
            return Err(ReplayError { line, kind: ReplayErrorKind::ForeignTransaction });
        }
        apply_event(&mut view, event, line)?;
    }

    view.ok_or(ReplayError { line: 0, kind: ReplayErrorKind::MissingOpen })
}

fn apply_event(
    view: &mut Option<ConvergenceView>,
    event: &ConvergenceEvent,
    line: usize,
) -> Result<(), ReplayError> {
    match event {
        ConvergenceEvent::TransactionOpened {
            transaction_id,
            direction,
            release_mode,
            prior_accepted_generation,
            ..
        } => {
            if view.is_some() {
                return Err(ReplayError { line, kind: ReplayErrorKind::DuplicateOpen });
            }
            *view = Some(ConvergenceView {
                transaction_id: transaction_id.clone(),
                direction: *direction,
                release_mode: *release_mode,
                prior_accepted_generation: prior_accepted_generation.clone(),
                generations: BTreeMap::new(),
                lease: None,
                invalidations: Vec::new(),
            });
            Ok(())
        }
        ConvergenceEvent::GenerationStarted {
            generation_id,
            source_parent_sha,
            swarm_parent_sha,
            ..
        } => {
            let v = view_mut(view, line)?;
            if v.generations.contains_key(generation_id) {
                return Err(ReplayError { line, kind: ReplayErrorKind::DuplicateGeneration });
            }
            // A transaction carries at most one current generation at a time:
            // every prior generation must be terminal (merged, rejected,
            // superseded, ...) before another can start. Restricting the old
            // check to the same source parent allowed a different-parent
            // generation to start without supersession, after which
            // `active_generation` could not reconstruct a single current
            // generation in a fresh process.
            if let Some((existing_id, _)) =
                v.generations.iter().find(|(_, g)| !g.state.is_terminal())
            {
                return Err(ReplayError {
                    line,
                    kind: ReplayErrorKind::ConcurrentActiveGeneration {
                        existing: existing_id.as_str().to_string(),
                    },
                });
            }
            v.generations.insert(
                generation_id.clone(),
                GenerationRuntime {
                    state: TransitionState::Observed,
                    source_parent_sha: source_parent_sha.clone(),
                    swarm_parent_sha: swarm_parent_sha.clone(),
                    successor: None,
                    last_evidence_digest: None,
                    rejection_evidence_digest: None,
                },
            );
            Ok(())
        }
        ConvergenceEvent::TransitionRecorded { generation_id, to, evidence_digest, .. } => {
            let v = view_mut(view, line)?;
            let g = generation_mut(v, generation_id, line)?;
            if !is_legal_transition(g.state, *to) {
                return Err(ReplayError { line, kind: illegal(g.state, *to) });
            }
            g.state = *to;
            g.last_evidence_digest = Some(evidence_digest.clone());
            Ok(())
        }
        ConvergenceEvent::RejectionRecorded { generation_id, evidence_digest, .. } => {
            let v = view_mut(view, line)?;
            let g = generation_mut(v, generation_id, line)?;
            if !matches!(
                g.state,
                TransitionState::Observed
                    | TransitionState::Planned
                    | TransitionState::Materialized
                    | TransitionState::Published
                    | TransitionState::AdmissionPending
            ) {
                return Err(ReplayError {
                    line,
                    kind: illegal(g.state, TransitionState::Rejected),
                });
            }
            if g.rejection_evidence_digest.is_some() {
                return Err(ReplayError { line, kind: ReplayErrorKind::RejectionConflict });
            }
            g.state = TransitionState::Rejected;
            g.rejection_evidence_digest = Some(evidence_digest.clone());
            g.last_evidence_digest = Some(evidence_digest.clone());
            Ok(())
        }
        ConvergenceEvent::NoOpRecorded { generation_id, evidence_digest, .. } => {
            let v = view_mut(view, line)?;
            let g = generation_mut(v, generation_id, line)?;
            if !matches!(
                g.state,
                TransitionState::Observed
                    | TransitionState::Planned
                    | TransitionState::Materialized
            ) {
                return Err(ReplayError { line, kind: illegal(g.state, TransitionState::Noop) });
            }
            g.state = TransitionState::Noop;
            g.last_evidence_digest = Some(evidence_digest.clone());
            Ok(())
        }
        ConvergenceEvent::GenerationSuperseded { old_generation, successor_generation, .. } => {
            let v = view_mut(view, line)?;
            let g = generation_mut(v, old_generation, line)?;
            if g.successor.is_some() {
                return Err(ReplayError { line, kind: ReplayErrorKind::InvalidSupersession });
            }
            g.state = TransitionState::Superseded;
            g.successor = Some(successor_generation.clone());
            Ok(())
        }
        ConvergenceEvent::LeaseClaimed { lease, .. } => {
            let v = view_mut(view, line)?;
            // Any prior lease record — live or expired-but-unreclaimed —
            // blocks a plain claim. An expired lease is reclaimable only
            // through a recorded `TakeoverRecorded` reconciliation; this is
            // the direct-post-expiry negative control.
            if v.lease.is_some() {
                return Err(ReplayError { line, kind: ReplayErrorKind::LiveLeaseExists });
            }
            validate_replayed_lease(lease)
                .map_err(|why| ReplayError { line, kind: ReplayErrorKind::InvalidLease(why) })?;
            if !v.generations.contains_key(&lease.input_generation) {
                return Err(ReplayError { line, kind: ReplayErrorKind::UnknownGeneration });
            }
            v.lease = Some(lease.clone());
            Ok(())
        }
        ConvergenceEvent::LeaseHeartbeat { claimed_by, heartbeat_at, new_expires_at, .. } => {
            let v = view_mut(view, line)?;
            let lease = match v.lease.as_mut() {
                Some(l) => l,
                None => return Err(ReplayError { line, kind: ReplayErrorKind::HeartbeatMismatch }),
            };
            if lease.claimed_by != *claimed_by || lease.is_expired(*heartbeat_at) {
                return Err(ReplayError { line, kind: ReplayErrorKind::HeartbeatMismatch });
            }
            // Replay enforces the same monotonic extension law as
            // `Lease::heartbeat`: time never moves backward and an expiry at
            // or before the heartbeat can never install an already-expired
            // lease.
            if *heartbeat_at <= lease.heartbeat_at {
                return Err(ReplayError {
                    line,
                    kind: ReplayErrorKind::InvalidLease(
                        "heartbeat time must strictly increase".to_string(),
                    ),
                });
            }
            if *new_expires_at <= *heartbeat_at {
                return Err(ReplayError {
                    line,
                    kind: ReplayErrorKind::InvalidLease(
                        "lease expiry must be after the heartbeat instant".to_string(),
                    ),
                });
            }
            lease.heartbeat_at = *heartbeat_at;
            lease.lease_expires_at = *new_expires_at;
            Ok(())
        }
        ConvergenceEvent::TakeoverRecorded { takeover, new_lease, .. } => {
            let v = view_mut(view, line)?;
            let record_ok = takeover.validate().is_ok();
            let displaced_ok = v.lease.as_ref().is_some_and(|l| {
                l.claimed_by == takeover.displaced_claimant
                    && l.is_expired(takeover.reclaimed_at)
                    && l.input_generation == takeover.input_generation
            });
            let generation_known = v.generations.contains_key(&new_lease.input_generation);
            // The installed lease must be bound to this takeover epoch:
            // claimant, generation, claim instant, and liveness all cohere.
            let mut binding_failures: Vec<String> = Vec::new();
            if new_lease.claimed_by != takeover.reclaimed_by {
                binding_failures.push("claimant differs from the reclaiming writer".to_string());
            }
            if new_lease.claimed_at != takeover.reclaimed_at {
                binding_failures
                    .push("claim instant differs from the takeover instant".to_string());
            }
            if new_lease.input_generation != takeover.input_generation {
                binding_failures
                    .push("generation differs from the reconciled generation".to_string());
            }
            if new_lease.is_expired(takeover.reclaimed_at) {
                binding_failures.push("installed lease is already expired".to_string());
            }
            if let Err(why) = validate_replayed_lease(new_lease) {
                binding_failures.push(why);
            }
            if !(record_ok && displaced_ok && generation_known) {
                return Err(ReplayError {
                    line,
                    kind: ReplayErrorKind::InvalidTakeover(
                        if record_ok {
                            "displaced lease missing, still live, or generation mismatch"
                        } else {
                            "incomplete reconciliation record"
                        }
                        .to_string(),
                    ),
                });
            }
            if !binding_failures.is_empty() {
                return Err(ReplayError {
                    line,
                    kind: ReplayErrorKind::InvalidTakeover(binding_failures.join("; ")),
                });
            }
            v.lease = Some(new_lease.clone());
            Ok(())
        }
        ConvergenceEvent::InvalidationRecorded { record, .. } => {
            let v = view_mut(view, line)?;
            v.invalidations.push(record.clone());
            Ok(())
        }
    }
}

fn view_mut(
    view: &mut Option<ConvergenceView>,
    line: usize,
) -> Result<&mut ConvergenceView, ReplayError> {
    match view.as_mut() {
        Some(v) => Ok(v),
        None => Err(ReplayError { line, kind: ReplayErrorKind::MissingOpen }),
    }
}

fn generation_mut<'a>(
    view: &'a mut ConvergenceView,
    generation_id: &GenerationId,
    line: usize,
) -> Result<&'a mut GenerationRuntime, ReplayError> {
    match view.generations.get_mut(generation_id) {
        Some(g) => Ok(g),
        None => Err(ReplayError { line, kind: ReplayErrorKind::UnknownGeneration }),
    }
}

/// Structural laws every replayed lease must satisfy regardless of how its
/// bytes were produced. [`Lease::new`] guarantees these at construction, but
/// journal deserialization bypasses constructors, so replay re-checks them
/// before the lease participates in reconstruction.
fn validate_replayed_lease(lease: &Lease) -> Result<(), String> {
    if lease.claimed_by.trim().is_empty() {
        return Err("lease claimant must be a non-empty identity".to_string());
    }
    if lease.heartbeat_at < lease.claimed_at {
        return Err("heartbeat precedes the claim instant".to_string());
    }
    if lease.lease_expires_at <= lease.heartbeat_at {
        return Err("lease expiry must be after the heartbeat instant".to_string());
    }
    Ok(())
}
