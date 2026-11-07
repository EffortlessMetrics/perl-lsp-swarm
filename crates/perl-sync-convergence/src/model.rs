//! Core value model for `convergence_transaction.v1` generations.
//!
//! Direction and release-context vocabularies are closed: unknown spellings
//! fail at the serde boundary rather than being silently reinterpreted
//! (negative control 4 of issue #11282).

use serde::{Deserialize, Serialize};
use std::fmt;

/// Convergence direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// Swarm development source converges into the public source repository.
    SwarmToSource,
    /// Public source repository changes converge back into the swarm.
    SourceToSwarm,
}

impl Direction {
    /// Canonical wire spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SwarmToSource => "swarm_to_source",
            Self::SourceToSwarm => "source_to_swarm",
        }
    }
}

impl fmt::Display for Direction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Release context of a convergence transaction.
///
/// The v0.18 release-specific R/S/J/M transaction remains separately governed;
/// ordinary continuous generations may reference it but never rewrite it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseContextMode {
    /// Ordinary continuous bridge outside any release transaction.
    OrdinaryContinuous,
    /// Audited release-specific projection (v0.18 J/M shape under #4348).
    ReleaseSpecific,
}

impl ReleaseContextMode {
    /// Canonical wire spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OrdinaryContinuous => "ordinary_continuous",
            Self::ReleaseSpecific => "release_specific",
        }
    }
}

impl fmt::Display for ReleaseContextMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One exact upstream squash commit/PR imported into a generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportedCommit {
    /// Upstream commit SHA.
    pub commit_sha: String,
    /// Upstream PR number, when the commit landed through one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pull_request: Option<u64>,
    /// Digest of the product proof associated with this import.
    ///
    /// A digest string is retained, never an expiring artifact URL alone.
    pub product_proof_digest: String,
}

impl ImportedCommit {
    /// Construct an import record with validated non-empty identity fields.
    pub fn new(
        commit_sha: impl Into<String>,
        pull_request: Option<u64>,
        product_proof_digest: impl Into<String>,
    ) -> Result<Self, ModelError> {
        let commit_sha = commit_sha.into();
        let product_proof_digest = product_proof_digest.into();
        if commit_sha.is_empty() || product_proof_digest.is_empty() {
            return Err(ModelError::EmptyRequiredField);
        }
        Ok(Self { commit_sha, pull_request, product_proof_digest })
    }
}

/// Digest of a continuous projection or divergence ledger artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactDigest {
    /// Stable artifact kind (for example `projection_manifest`,
    /// `divergence_ledger`).
    pub kind: String,
    /// Hex digest over the canonical artifact bytes.
    pub digest: String,
    /// Retention class of the artifact (for example `durable_receipt`).
    pub retention_class: String,
    /// Durable-copy state; expiring workflow storage is not durable.
    pub durable_copy: DurableCopyState,
}

/// Where the canonical bytes of a digested artifact live.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DurableCopyState {
    /// A durable copy is retained in-repository or in equivalent storage.
    RetainedDurable,
    /// Only an expiring artifact copy exists; state is not reconstructible
    /// from this digest alone.
    ExpiringOnly,
    /// No copy exists at all.
    Missing,
}

/// Exact candidate identities once published to the source repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishedCandidate {
    /// Published source branch name (non-forced transport only).
    pub branch: String,
    /// Published source PR number.
    pub pull_request: u64,
    /// Exact candidate head SHA at publication time.
    pub head_sha: String,
}

/// Exact landing merge once completed on the source repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LandingMerge {
    /// Merge commit SHA created by the source repository.
    pub merge_sha: String,
    /// Resulting tree SHA after landing.
    pub merge_tree: String,
}

/// Error raised by model constructors that enforce non-degenerate records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelError {
    /// A required identity field was empty.
    EmptyRequiredField,
    /// Declared content-addressed identity does not match the identity
    /// derived from the exact inputs.
    IdentityMismatch {
        /// Identity declared on the record.
        declared: String,
        /// Identity re-derived from inputs.
        derived: String,
    },
}

impl fmt::Display for ModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyRequiredField => f.write_str("required identity field was empty"),
            Self::IdentityMismatch { declared, derived } => {
                write!(
                    f,
                    "declared generation identity {declared} does not match derived identity {derived}"
                )
            }
        }
    }
}

impl std::error::Error for ModelError {}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn direction_wire_spellings() {
        assert_eq!(
            serde_json::to_value(Direction::SwarmToSource).unwrap(),
            serde_json::json!("swarm_to_source")
        );
        assert_eq!(
            serde_json::to_value(Direction::SourceToSwarm).unwrap(),
            serde_json::json!("source_to_swarm")
        );
        assert!(serde_json::from_value::<Direction>(serde_json::json!("other")).is_err());
    }

    #[test]
    fn release_mode_unknown_fails_closed() {
        assert!(
            serde_json::from_value::<ReleaseContextMode>(serde_json::json!("experimental"))
                .is_err()
        );
    }

    #[test]
    fn imported_commit_requires_identity() {
        assert!(ImportedCommit::new("", None, "sha").is_err());
        assert!(ImportedCommit::new("abc", None, "").is_err());
        assert!(ImportedCommit::new("abc", Some(7), "sha").is_ok());
    }
}
