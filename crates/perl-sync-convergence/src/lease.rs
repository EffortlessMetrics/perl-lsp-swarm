//! Writer leases and takeover for convergence transactions.
//!
//! A lease coordinates exactly one active writer. An expired lease may be
//! reclaimed only through a recorded takeover that carries reconciliation
//! observations of exact source/swarm/GitHub state; a live lease grants no
//! merge or ref-mutation authority (issue #11282).

use crate::ids::GenerationId;
use crate::state::{PermittedAction, TransitionState};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Epoch milliseconds since the Unix epoch.
///
/// Deterministic integer time keeps persisted JSON canonical and comparable
/// without pulling a time-library dependency into the crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TimestampMs(u64);

impl TimestampMs {
    /// Construct from raw epoch milliseconds.
    #[must_use]
    pub const fn from_millis(millis: u64) -> Self {
        Self(millis)
    }

    /// Raw epoch milliseconds.
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

impl fmt::Display for TimestampMs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Writer lease over one active convergence transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lease {
    /// Identity of the claiming writer (agent/process), never credentials.
    pub claimed_by: String,
    /// When the claim was made.
    pub claimed_at: TimestampMs,
    /// Last heartbeat time.
    pub heartbeat_at: TimestampMs,
    /// Expiry after which the lease is stale and reclaimable.
    pub lease_expires_at: TimestampMs,
    /// Generation whose inputs the claimant is working against.
    pub input_generation: GenerationId,
    /// Last transition the claimant completed.
    pub last_completed_transition: Option<TransitionState>,
    /// Actions the claimant is permitted to perform next.
    pub next_permitted_actions: Vec<PermittedAction>,
}

impl Lease {
    /// Construct a validated lease; expiry must not precede the claim.
    pub fn new(
        claimed_by: impl Into<String>,
        claimed_at: TimestampMs,
        lease_duration_ms: u64,
        input_generation: GenerationId,
        next_permitted_actions: Vec<PermittedAction>,
    ) -> Result<Self, LeaseError> {
        let claimed_by = claimed_by.into();
        if claimed_by.is_empty() {
            return Err(LeaseError::EmptyClaimant);
        }
        if lease_duration_ms == 0 {
            return Err(LeaseError::NonPositiveDuration);
        }
        Ok(Self {
            claimed_by,
            claimed_at,
            heartbeat_at: claimed_at,
            lease_expires_at: TimestampMs::from_millis(claimed_at.as_u64() + lease_duration_ms),
            input_generation,
            last_completed_transition: None,
            next_permitted_actions,
        })
    }

    /// Whether the lease has expired at `now`.
    #[must_use]
    pub fn is_expired(&self, now: TimestampMs) -> bool {
        now >= self.lease_expires_at
    }

    /// Record a heartbeat, extending the expiry by `lease_duration_ms`.
    pub fn heartbeat(
        &mut self,
        now: TimestampMs,
        lease_duration_ms: u64,
    ) -> Result<(), LeaseError> {
        if self.is_expired(now) {
            return Err(LeaseError::AlreadyExpired { expires_at: self.lease_expires_at, now });
        }
        if lease_duration_ms == 0 {
            return Err(LeaseError::NonPositiveDuration);
        }
        self.heartbeat_at = now;
        self.lease_expires_at = TimestampMs::from_millis(now.as_u64() + lease_duration_ms);
        Ok(())
    }

    /// Record one completed transition and refresh the permitted set.
    pub fn complete_transition(
        &mut self,
        completed: TransitionState,
        next_permitted_actions: Vec<PermittedAction>,
    ) {
        self.last_completed_transition = Some(completed);
        self.next_permitted_actions = next_permitted_actions;
    }

    // There is deliberately no `grants_merge_authority` predicate: landing
    // and ref-mutation actions are structurally absent from `PermittedAction`,
    // so no lease can grant or be queried for merge authority at any
    // construction, update, or deserialization boundary. Controllers consult
    // transaction state (`admitted`) instead.
}

/// Recorded takeover of an expired lease after exact-state reconciliation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Takeover {
    /// Previous claimant being displaced.
    pub displaced_claimant: String,
    /// New claimant identity.
    pub reclaimed_by: String,
    /// When the takeover was recorded.
    pub reclaimed_at: TimestampMs,
    /// Generation whose inputs were re-reconciled before takeover.
    pub input_generation: GenerationId,
    /// Digests of independent observations proving exact source/swarm/GitHub
    /// state at takeover time. At least one is required.
    pub reconciled_observations: Vec<String>,
}

impl Takeover {
    /// Validate takeover completeness: non-empty identities and at least one
    /// reconciliation observation.
    pub fn validate(&self) -> Result<(), LeaseError> {
        if self.displaced_claimant.is_empty() || self.reclaimed_by.is_empty() {
            return Err(LeaseError::EmptyClaimant);
        }
        if self.reconciled_observations.is_empty()
            || self.reconciled_observations.iter().any(String::is_empty)
        {
            return Err(LeaseError::MissingReconciliationEvidence);
        }
        Ok(())
    }
}

/// Lease validation errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaseError {
    /// Claimant identity was empty.
    EmptyClaimant,
    /// Lease duration was zero.
    NonPositiveDuration,
    /// Heartbeat attempted after expiry.
    AlreadyExpired {
        /// Configured expiry instant.
        expires_at: TimestampMs,
        /// Heartbeat attempt instant.
        now: TimestampMs,
    },
    /// Takeover lacked required reconciliation observations.
    MissingReconciliationEvidence,
}

impl fmt::Display for LeaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyClaimant => f.write_str("lease claimant must be a non-empty identity"),
            Self::NonPositiveDuration => f.write_str("lease duration must be positive"),
            Self::AlreadyExpired { expires_at, now } => {
                write!(f, "lease already expired at {expires_at}; heartbeat attempted at {now}")
            }
            Self::MissingReconciliationEvidence => {
                f.write_str("takeover requires at least one reconciled observation")
            }
        }
    }
}

impl std::error::Error for LeaseError {}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn sample_generation() -> GenerationId {
        GenerationId::parse(format!("gen:sha256:{}", "ab".repeat(32))).unwrap()
    }

    #[test]
    fn expiry_and_heartbeat_lifecycle() {
        let mut lease = Lease::new(
            "writer-a",
            TimestampMs::from_millis(1_000),
            5_000,
            sample_generation(),
            vec![PermittedAction::PlanCandidate],
        )
        .unwrap();
        assert!(!lease.is_expired(TimestampMs::from_millis(5_999)));
        assert!(lease.is_expired(TimestampMs::from_millis(6_000)));

        lease.heartbeat(TimestampMs::from_millis(4_000), 5_000).unwrap();
        assert_eq!(lease.lease_expires_at, TimestampMs::from_millis(9_000));
        assert!(lease.heartbeat(TimestampMs::from_millis(10_000), 5_000).is_err());
    }

    #[test]
    fn zero_duration_rejected() {
        assert!(
            Lease::new("w", TimestampMs::from_millis(1), 0, sample_generation(), vec![]).is_err()
        );
    }

    #[test]
    fn empty_claimant_rejected() {
        assert!(
            Lease::new("", TimestampMs::from_millis(1), 1_000, sample_generation(), vec![])
                .is_err()
        );
    }

    #[test]
    fn takeover_requires_reconciliation_evidence() {
        let takeover = Takeover {
            displaced_claimant: "writer-a".into(),
            reclaimed_by: "writer-b".into(),
            reclaimed_at: TimestampMs::from_millis(9_000),
            input_generation: sample_generation(),
            reconciled_observations: vec![],
        };
        assert_eq!(takeover.validate(), Err(LeaseError::MissingReconciliationEvidence));

        let takeover = Takeover { reconciled_observations: vec!["sha256:obs".into()], ..takeover };
        assert!(takeover.validate().is_ok());
    }
}
