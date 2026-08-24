//! Generation and transaction identity for `convergence_transaction.v1`.
//!
//! Generation identity is content-addressed over the exact immutable inputs of
//! a generation. A moved exact input therefore produces a different
//! [`GenerationId`], which forces a successor generation instead of an edit to
//! an existing receipt (negative control 1 of issue #11282).

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;

/// Length in hex digits of the lowercase SHA-256 digest suffix.
const HEX_DIGEST_LEN: usize = 64;

/// Compute a domain-separated lowercase hex SHA-256 over length-prefixed parts.
fn domain_hex(domain: &str, parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(length_prefixed(domain));
    for part in parts {
        hasher.update(length_prefixed(part));
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(HEX_DIGEST_LEN);
    for byte in digest {
        hex.push(char::from_digit(u32::from(byte >> 4), 16).unwrap_or('0'));
        hex.push(char::from_digit(u32::from(byte & 0x0f), 16).unwrap_or('0'));
    }
    debug_assert_eq!(hex.len(), HEX_DIGEST_LEN);
    hex
}

/// Length-prefix one field so field boundaries cannot shift.
fn length_prefixed(field: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(field.len() + 8);
    bytes.extend_from_slice(&(field.len() as u64).to_be_bytes());
    bytes.extend_from_slice(field.as_bytes());
    bytes
}

fn validate_wire(kind: &str, value: &str) -> Result<(), IdentityError> {
    let prefix = format!("{kind}:sha256:");
    let Some(suffix) = value.strip_prefix(&prefix) else {
        return Err(IdentityError::BadPrefix { kind: kind.to_string(), value: value.to_string() });
    };
    let lowercase_hex = suffix.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    if suffix.len() != HEX_DIGEST_LEN || !lowercase_hex {
        return Err(IdentityError::BadDigest { kind: kind.to_string(), value: value.to_string() });
    }
    Ok(())
}

/// Error produced when a durable identity wire form is ill-formed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentityError {
    /// Wire prefix missing or from a different identity namespace.
    BadPrefix {
        /// Expected namespace kind.
        kind: String,
        /// Offending wire value.
        value: String,
    },
    /// Digest suffix is not exactly 64 lowercase hex digits.
    BadDigest {
        /// Namespace kind.
        kind: String,
        /// Offending wire value.
        value: String,
    },
    /// Transaction ID was empty or contained control characters.
    InvalidTransactionId(String),
}

impl fmt::Display for IdentityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadPrefix { kind, value } => {
                write!(f, "identity {kind} lacks the `{kind}:sha256:` prefix: {value}")
            }
            Self::BadDigest { kind, value } => {
                write!(f, "identity {kind} digest must be 64 lowercase hex digits: {value}")
            }
            Self::InvalidTransactionId(value) => {
                write!(f, "transaction id must be non-empty without control characters: {value:?}")
            }
        }
    }
}

impl std::error::Error for IdentityError {}

/// Immutable transaction identifier grouping successor generations.
///
/// Opaque, caller-assigned, validated at construction and at the serde
/// boundary.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TransactionId(String);

impl TransactionId {
    /// Validate and construct a transaction ID.
    ///
    /// IDs double as directory names, so path separators, drive colons, and
    /// leading dots are rejected alongside emptiness and control characters.
    pub fn new(value: impl Into<String>) -> Result<Self, IdentityError> {
        let value = value.into();
        let unsafe_chars = value.chars().any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'));
        if value.is_empty() || unsafe_chars || value.starts_with('.') {
            return Err(IdentityError::InvalidTransactionId(value));
        }
        Ok(Self(value))
    }

    /// Wire spelling.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TransactionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Immutable content-addressed generation identifier.
///
/// Derived via [`GenerationId::from_inputs`] from the exact inputs; equality
/// therefore means "same exact inputs", and any moved input yields a distinct
/// identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct GenerationId(String);

impl GenerationId {
    const DOMAIN: &'static str = "perl_lsp.convergence.generation.v1";

    /// Derive the generation ID from its exact immutable inputs.
    ///
    /// Inputs are the direction tag, release-context mode, source repository
    /// and exact parent SHA/tree, swarm repository and exact parent SHA/tree,
    /// and the prior accepted generation (empty when none).
    #[must_use]
    pub fn from_inputs(inputs: &GenerationInputs<'_>) -> Self {
        let hex = domain_hex(
            Self::DOMAIN,
            &[
                inputs.direction.as_str(),
                inputs.release_mode.as_str(),
                &inputs.source_repository,
                &inputs.source_parent_sha,
                &inputs.source_parent_tree,
                &inputs.swarm_repository,
                &inputs.swarm_parent_sha,
                &inputs.swarm_parent_tree,
                inputs.prior_accepted_generation,
            ],
        );
        Self(format!("gen:sha256:{hex}"))
    }

    /// Parse and validate an existing wire form.
    pub fn parse(value: impl Into<String>) -> Result<Self, IdentityError> {
        let value = value.into();
        validate_wire("gen", &value)?;
        Ok(Self(value))
    }

    /// Wire spelling.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for GenerationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Exact immutable inputs that define one convergence generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationInputs<'a> {
    /// Convergence direction.
    pub direction: crate::model::Direction,
    /// Ordinary continuous or release-specific context.
    pub release_mode: crate::model::ReleaseContextMode,
    /// Source repository canonical name (for example `EffortlessMetrics/perl-lsp`).
    pub source_repository: String,
    /// Exact source parent commit SHA.
    pub source_parent_sha: String,
    /// Exact source parent tree SHA.
    pub source_parent_tree: String,
    /// Swarm repository canonical name.
    pub swarm_repository: String,
    /// Exact swarm parent commit SHA.
    pub swarm_parent_sha: String,
    /// Exact swarm parent tree SHA.
    pub swarm_parent_tree: String,
    /// Prior accepted generation, or empty when none exists.
    pub prior_accepted_generation: &'a str,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use crate::model::{Direction, ReleaseContextMode};

    fn inputs(prior: &'static str) -> GenerationInputs<'static> {
        GenerationInputs {
            direction: Direction::SwarmToSource,
            release_mode: ReleaseContextMode::OrdinaryContinuous,
            source_repository: "EffortlessMetrics/perl-lsp".into(),
            source_parent_sha: "a".repeat(40),
            source_parent_tree: "b".repeat(40),
            swarm_repository: "EffortlessMetrics/perl-lsp-swarm".into(),
            swarm_parent_sha: "c".repeat(40),
            swarm_parent_tree: "d".repeat(40),
            prior_accepted_generation: prior,
        }
    }

    #[test]
    fn same_inputs_produce_identical_ids() {
        assert_eq!(GenerationId::from_inputs(&inputs("")), GenerationId::from_inputs(&inputs("")));
    }

    #[test]
    fn moved_input_changes_identity() {
        let base = GenerationId::from_inputs(&inputs(""));
        let moved = GenerationInputs { source_parent_sha: "f".repeat(40), ..inputs("") };
        assert_ne!(base, GenerationId::from_inputs(&moved));
    }

    #[test]
    fn prior_generation_participates_in_identity() {
        let base = GenerationId::from_inputs(&inputs(""));
        let chained = GenerationId::from_inputs(&inputs("gen:sha256:aa"));
        assert_ne!(base, chained);
    }

    #[test]
    fn wire_round_trip_and_rejection() {
        let id = GenerationId::from_inputs(&inputs(""));
        assert_eq!(GenerationId::parse(id.as_str()).as_ref(), Ok(&id));
        assert!(GenerationId::parse(format!("gen:sha256:{}", "A".repeat(64))).is_err());
        assert!(GenerationId::parse(format!("tx:sha256:{}", "0".repeat(64))).is_err());
        assert!(GenerationId::parse("gen:sha256:short").is_err());
    }

    #[test]
    fn transaction_id_validation() {
        assert!(TransactionId::new("bridge-2026-08").is_ok());
        assert!(TransactionId::new("").is_err());
        assert!(TransactionId::new("bad\nid").is_err());
        assert!(TransactionId::new("a/b").is_err());
        assert!(TransactionId::new("a\\b").is_err());
        assert!(TransactionId::new("C:temp").is_err());
        assert!(TransactionId::new(".hidden").is_err());
    }
}
