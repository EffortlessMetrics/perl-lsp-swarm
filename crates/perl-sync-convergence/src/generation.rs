//! Immutable generation receipts for `convergence_transaction.v1`.
//!
//! One receipt captures every exact input the issue requires. Receipts are
//! content-addressed by [`GenerationId`] and never rewritten: a moved input
//! produces a successor generation (issue #11282).

use crate::ids::{GenerationId, GenerationInputs, TransactionId};
use crate::model::{
    ArtifactDigest, Direction, ImportedCommit, LandingMerge, ModelError, PublishedCandidate,
    ReleaseContextMode,
};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Schema version of the persisted generation receipt format.
pub const GENERATION_RECEIPT_SCHEMA_VERSION: u32 = 1;

/// Top-level wrapper adding an explicit schema version to a receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenerationReceiptFile {
    /// Persisted format version; readers reject unsupported versions.
    pub schema_version: u32,
    /// The immutable receipt.
    pub receipt: ConvergenceGeneration,
}

/// Immutable receipt describing one convergence generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConvergenceGeneration {
    /// Owning transaction.
    pub transaction_id: TransactionId,
    /// Content-addressed generation identity over the exact inputs below.
    pub generation_id: GenerationId,
    /// Convergence direction.
    pub direction: Direction,
    /// Ordinary continuous or audited release-specific context.
    ///
    /// Modes stay distinct; a release-specific generation is never silently
    /// reinterpreted as ordinary bridge policy (negative control 4).
    pub release_context_mode: ReleaseContextMode,
    /// Source repository canonical name.
    pub source_repository: String,
    /// Exact source master parent commit SHA.
    pub source_parent_sha: String,
    /// Exact source master parent tree SHA.
    pub source_parent_tree: String,
    /// Swarm repository canonical name.
    pub swarm_repository: String,
    /// Exact swarm main parent commit SHA.
    pub swarm_parent_sha: String,
    /// Exact swarm main parent tree SHA.
    pub swarm_parent_tree: String,
    /// Prior accepted convergence generation, when chaining.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prior_accepted_generation: Option<GenerationId>,
    /// Upstream squash commits/PRs imported by this generation.
    pub imported_commits: Vec<ImportedCommit>,
    /// Continuous projection/divergence-ledger artifact digests.
    pub projection_digests: Vec<ArtifactDigest>,
    /// Candidate commit SHA, when materialized.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_commit: Option<String>,
    /// Candidate tree SHA, when materialized.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_tree: Option<String>,
    /// Published branch/PR identities, when published.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_candidate: Option<PublishedCandidate>,
    /// Landing merge/tree, when completed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub landing_merge: Option<LandingMerge>,
}

impl ConvergenceGeneration {
    /// Derive the expected [`GenerationId`] for these exact inputs.
    #[must_use]
    pub fn expected_id(&self) -> GenerationId {
        GenerationId::from_inputs(&GenerationInputs {
            direction: self.direction,
            release_mode: self.release_context_mode,
            source_repository: self.source_repository.clone(),
            source_parent_sha: self.source_parent_sha.clone(),
            source_parent_tree: self.source_parent_tree.clone(),
            swarm_repository: self.swarm_repository.clone(),
            swarm_parent_sha: self.swarm_parent_sha.clone(),
            swarm_parent_tree: self.swarm_parent_tree.clone(),
            prior_accepted_generation: self
                .prior_accepted_generation
                .as_ref()
                .map_or("", |g| g.as_str()),
        })
    }

    /// Validate internal coherence: declared ID matches derived ID and
    /// required SHA fields are present.
    pub fn validate(&self) -> Result<(), ModelError> {
        if self.source_parent_sha.is_empty()
            || self.source_parent_tree.is_empty()
            || self.swarm_parent_sha.is_empty()
            || self.swarm_parent_tree.is_empty()
        {
            return Err(ModelError::EmptyRequiredField);
        }
        if self.expected_id() != self.generation_id {
            return Err(ModelError::IdentityMismatch {
                declared: self.generation_id.as_str().to_string(),
                derived: self.expected_id().as_str().to_string(),
            });
        }
        Ok(())
    }
}

impl fmt::Display for ConvergenceGeneration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}[{}] {} {}@{} <- {}@{} ({})",
            self.transaction_id,
            self.generation_id,
            self.direction,
            self.source_repository,
            &self.source_parent_sha[..self.source_parent_sha.len().min(12)],
            self.swarm_repository,
            &self.swarm_parent_sha[..self.swarm_parent_sha.len().min(12)],
            self.release_context_mode,
        )
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn sample(prior: Option<GenerationId>) -> ConvergenceGeneration {
        let generation_id = GenerationId::from_inputs(&GenerationInputs {
            direction: Direction::SwarmToSource,
            release_mode: ReleaseContextMode::OrdinaryContinuous,
            source_repository: "EffortlessMetrics/perl-lsp".into(),
            source_parent_sha: "a".repeat(40),
            source_parent_tree: "b".repeat(40),
            swarm_repository: "EffortlessMetrics/perl-lsp-swarm".into(),
            swarm_parent_sha: "c".repeat(40),
            swarm_parent_tree: "d".repeat(40),
            prior_accepted_generation: prior.as_ref().map_or("", |g| g.as_str()),
        });
        ConvergenceGeneration {
            transaction_id: TransactionId::new("bridge-2026-08").unwrap(),
            generation_id,
            direction: Direction::SwarmToSource,
            release_context_mode: ReleaseContextMode::OrdinaryContinuous,
            source_repository: "EffortlessMetrics/perl-lsp".into(),
            source_parent_sha: "a".repeat(40),
            source_parent_tree: "b".repeat(40),
            swarm_repository: "EffortlessMetrics/perl-lsp-swarm".into(),
            swarm_parent_sha: "c".repeat(40),
            swarm_parent_tree: "d".repeat(40),
            prior_accepted_generation: prior,
            imported_commits: vec![
                ImportedCommit::new("e".repeat(40), Some(42), "sha256:proof").unwrap(),
            ],
            projection_digests: vec![],
            candidate_commit: None,
            candidate_tree: None,
            published_candidate: None,
            landing_merge: None,
        }
    }

    #[test]
    fn receipt_validates_against_derived_identity() {
        assert!(sample(None).validate().is_ok());
    }

    #[test]
    fn edited_input_breaks_identity_validation() {
        let mut tampered = sample(None);
        tampered.source_parent_sha = "9".repeat(40);
        assert!(tampered.validate().is_err());
    }

    #[test]
    fn receipt_json_round_trip_is_stable() {
        let receipt =
            sample(Some(GenerationId::parse("gen:sha256:0".to_owned() + &"0".repeat(63)).unwrap()));
        let file = GenerationReceiptFile {
            schema_version: GENERATION_RECEIPT_SCHEMA_VERSION,
            receipt: receipt.clone(),
        };
        let json = serde_json::to_string(&file).unwrap();
        let back: GenerationReceiptFile = serde_json::from_str(&json).unwrap();
        assert_eq!(back.receipt, receipt);
        let json2 = serde_json::to_string(&file).unwrap();
        assert_eq!(json, json2);
    }

    #[test]
    fn unsupported_schema_version_fails_closed() {
        let receipt = sample(None);
        let json = format!(
            "{{\"schema_version\":999,\"receipt\":{}}}",
            serde_json::to_string(&receipt).unwrap()
        );
        let file: GenerationReceiptFile = serde_json::from_str(&json).unwrap();
        assert_ne!(file.schema_version, GENERATION_RECEIPT_SCHEMA_VERSION);
    }
}
