//! Fail-closed errors for the frozen catalog and fixture contract.

use thiserror::Error;

/// Fail-closed catalog decode/validation error.
#[derive(Debug, Error)]
pub enum CatalogError {
    /// Checked-in TOML could not be decoded.
    #[error("invalid distribution Kwalitee catalog TOML: {0}")]
    InvalidToml(#[source] toml::de::Error),
    /// Envelope metadata does not match the frozen contract.
    #[error("distribution Kwalitee catalog metadata mismatch: {0}")]
    Metadata(String),
    /// A metric row violates a catalog invariant.
    #[error("invalid catalog metric `{id}`: {reason}")]
    InvalidMetric {
        /// Stable metric ID.
        id: String,
        /// Validation reason.
        reason: String,
    },
    /// Duplicate stable IDs or aliases.
    #[error("duplicate catalog identity `{0}`")]
    DuplicateIdentity(String),
    /// A dependency or cascade row names an unknown metric.
    #[error("catalog metric `{id}` references unknown metric `{referenced}`")]
    UnknownReference {
        /// Metric that named the reference.
        id: String,
        /// Missing metric ID.
        referenced: String,
    },
    /// Class, relationship, and score participation disagree.
    #[error("score/class contradiction for `{id}`: {reason}")]
    ScoreClassContradiction {
        /// Metric ID.
        id: String,
        /// Validation reason.
        reason: String,
    },
    /// Observation set cannot be scored.
    #[error("incompatible metric observations: {0}")]
    Observation(String),
}

/// Fail-closed fixture-contract decode/validation error.
#[derive(Debug, Error)]
pub enum FixtureError {
    /// Checked-in TOML could not be decoded.
    #[error("invalid distribution Kwalitee fixture contract TOML: {0}")]
    InvalidToml(#[source] toml::de::Error),
    /// Envelope metadata does not match the frozen contract.
    #[error("distribution Kwalitee fixture metadata mismatch: {0}")]
    Metadata(String),
    /// A fixture row violates the contract.
    #[error("invalid fixture `{id}`: {reason}")]
    InvalidFixture {
        /// Fixture ID.
        id: String,
        /// Validation reason.
        reason: String,
    },
    /// Duplicate fixture IDs.
    #[error("duplicate fixture identity `{0}`")]
    DuplicateIdentity(String),
    /// Catalog and fixture contract disagree.
    #[error("catalog/fixture binding error: {0}")]
    Binding(String),
}
