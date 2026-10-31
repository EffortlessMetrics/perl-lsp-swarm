//! Invalidation graph records for convergence transactions.
//!
//! Each record ties one typed cause to the descendants it made stale and why,
//! keeping `stale`, `superseded`, `rejected`, and `not_proven` distinct
//! (issue #11282).

use crate::ids::GenerationId;
use crate::lease::TimestampMs;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Typed causes of invalidation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvalidationCause {
    /// Source master moved away from the generation's exact parent.
    SourceMasterMovement,
    /// Swarm main moved away from the generation's exact parent.
    SwarmMainMovement,
    /// Product proof inputs moved.
    ProductProofMovement,
    /// Projection or divergence-ledger inputs moved.
    ProjectionLedgerMovement,
    /// Candidate head moved (for example a non-forced transport violation).
    CandidateHeadMovement,
    /// Publication source admission state moved.
    PublicationSourceAdmissionMovement,
    /// Checked/live protection drifted from recorded expectations.
    ProtectionDrift,
    /// Release-specific projection overlaps ordinary continuous policy.
    ReleaseSpecificOverlap,
    /// Receipt or artifact retention failed.
    RetentionFailure,
}

impl InvalidationCause {
    /// Canonical wire spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SourceMasterMovement => "source_master_movement",
            Self::SwarmMainMovement => "swarm_main_movement",
            Self::ProductProofMovement => "product_proof_movement",
            Self::ProjectionLedgerMovement => "projection_ledger_movement",
            Self::CandidateHeadMovement => "candidate_head_movement",
            Self::PublicationSourceAdmissionMovement => "publication_source_admission_movement",
            Self::ProtectionDrift => "checked_live_protection_drift",
            Self::ReleaseSpecificOverlap => "release_specific_projection_overlap",
            Self::RetentionFailure => "receipt_artifact_retention_failure",
        }
    }
}

impl fmt::Display for InvalidationCause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How the invalidated descendant is dispositioned.
///
/// These statuses stay distinct: staleness is not rejection, and neither is
/// supersession or missing proof.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StaleDisposition {
    /// Descendant must be regenerated against new exact inputs.
    Stale,
    /// Descendant was replaced by an explicit successor.
    Superseded,
    /// Descendant was rejected with immutable evidence.
    Rejected,
    /// Proof for the descendant is absent or instrument-failed.
    NotProven,
}

impl StaleDisposition {
    /// Canonical wire spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stale => "stale",
            Self::Superseded => "superseded",
            Self::Rejected => "rejected",
            Self::NotProven => "not_proven",
        }
    }
}

impl fmt::Display for StaleDisposition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One descendant marked stale by a cause, with its reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StaleDescendant {
    /// Generation made stale.
    pub generation: GenerationId,
    /// Distinct disposition assigned by this invalidation.
    pub disposition: StaleDisposition,
    /// Human-readable why; never credentials or private paths.
    pub reason: String,
}

/// An immutable invalidation record in the durable journal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvalidationRecord {
    /// Generation whose input movement triggered the invalidation.
    pub invalidated_generation: GenerationId,
    /// Typed cause.
    pub cause: InvalidationCause,
    /// When the invalidation was observed.
    pub observed_at: TimestampMs,
    /// Digest binding the observed movement evidence to this record.
    ///
    /// Retained as a digest so expiring artifact URLs are never the sole
    /// authority (negative control 8).
    pub movement_evidence_digest: String,
    /// Descendants marked stale and why.
    pub stale_descendants: Vec<StaleDescendant>,
}

impl InvalidationRecord {
    /// Construct a validated record: non-empty evidence digest required.
    pub fn new(
        invalidated_generation: GenerationId,
        cause: InvalidationCause,
        observed_at: TimestampMs,
        movement_evidence_digest: impl Into<String>,
        stale_descendants: Vec<StaleDescendant>,
    ) -> Result<Self, InvalidationError> {
        let movement_evidence_digest = movement_evidence_digest.into();
        if movement_evidence_digest.is_empty() {
            return Err(InvalidationError::MissingEvidenceDigest);
        }
        Ok(Self {
            invalidated_generation,
            cause,
            observed_at,
            movement_evidence_digest,
            stale_descendants,
        })
    }

    /// All generations marked stale by this record.
    #[must_use]
    pub fn stale_generation_ids(&self) -> Vec<&GenerationId> {
        self.stale_descendants.iter().map(|d| &d.generation).collect()
    }
}

/// Invalidation construction errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidationError {
    /// Movement evidence digest was empty.
    MissingEvidenceDigest,
}

impl fmt::Display for InvalidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingEvidenceDigest => {
                f.write_str("invalidation requires a movement evidence digest")
            }
        }
    }
}

impl std::error::Error for InvalidationError {}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn generation(byte: u8) -> GenerationId {
        let hex: String =
            (0..64).map(|_| char::from_digit(u32::from(byte % 16), 16).unwrap()).collect();
        GenerationId::parse(format!("gen:sha256:{hex}")).unwrap()
    }

    #[test]
    fn all_causes_round_trip_with_distinct_spellings() {
        let causes = [
            InvalidationCause::SourceMasterMovement,
            InvalidationCause::SwarmMainMovement,
            InvalidationCause::ProductProofMovement,
            InvalidationCause::ProjectionLedgerMovement,
            InvalidationCause::CandidateHeadMovement,
            InvalidationCause::PublicationSourceAdmissionMovement,
            InvalidationCause::ProtectionDrift,
            InvalidationCause::ReleaseSpecificOverlap,
            InvalidationCause::RetentionFailure,
        ];
        let spellings: Vec<_> = causes.iter().map(|c| c.as_str()).collect();
        assert_eq!(spellings.len(), 9);
        let mut sorted = spellings.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), spellings.len());
        for cause in causes {
            let json = serde_json::to_string(&cause).unwrap();
            assert_eq!(serde_json::from_str::<InvalidationCause>(&json).unwrap(), cause);
        }
    }

    #[test]
    fn dispositions_stay_distinct() {
        assert_ne!(StaleDisposition::Stale, StaleDisposition::Superseded);
        assert_ne!(StaleDisposition::Superseded, StaleDisposition::Rejected);
        assert_ne!(StaleDisposition::Rejected, StaleDisposition::NotProven);
    }

    #[test]
    fn record_requires_movement_evidence() {
        let err = InvalidationRecord::new(
            generation(1),
            InvalidationCause::SwarmMainMovement,
            TimestampMs::from_millis(1),
            "",
            vec![],
        )
        .unwrap_err();
        assert_eq!(err, InvalidationError::MissingEvidenceDigest);
    }

    #[test]
    fn descendants_are_listed_with_reasons() {
        let record = InvalidationRecord::new(
            generation(2),
            InvalidationCause::SourceMasterMovement,
            TimestampMs::from_millis(5),
            "sha256:movement",
            vec![StaleDescendant {
                generation: generation(3),
                disposition: StaleDisposition::Stale,
                reason: "source master advanced past recorded parent".into(),
            }],
        )
        .unwrap();
        assert_eq!(record.stale_generation_ids().len(), 1);
        assert_eq!(
            record.stale_descendants[0].reason,
            "source master advanced past recorded parent"
        );
    }
}
