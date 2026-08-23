//! Close-proof schema train (CP01, issue #10380).
//!
//! Defines and validates the versioned machine-readable representation of the
//! proposition an issue owns:
//!
//! - `issue_contract.v1` ([`contract::IssueContract`]) — issue kind, required
//!   proof level, allowed close modes, stable denominator rows, negative
//!   controls, mandatory children, transfer policy, and current identity.
//! - `issue_close_proof.v1` ([`packet::ClosePacket`] and
//!   [`packet::validate_packet_against_contract`]) — requested close mode,
//!   contract binding, claims, row/control/child dispositions, and independent
//!   PR-scope versus issue-close verdicts.
//!
//! This layer is representation-only: it validates documents and their
//! referential integrity. It does not decide whether a requested close mode is
//! semantically satisfied — CP03 (#10382) owns that evaluation — and it does
//! not inspect live GitHub state, PR bodies, or closing keywords.
//!
//! The immutable regression corpus under `.ci/close-proof-contract/`
//! ([`corpus`]) carries bounded offline fixtures with expected dispositions.
//! Immutability is enforced by a content-addressed manifest plus deterministic
//! canonical re-serialization; repository history remains the final arbiter
//! for reviewed mutation.
//!
//! Serde strictness note: top-level documents reject unknown fields; payload
//! variants inside internally tagged enums (`disposition`, `state`) cannot use
//! `deny_unknown_fields`, so unknown keys inside variant payloads are ignored
//! by serde, and serde_json resolves duplicate JSON object keys last-wins.
//! Downstream evaluators must not treat that silence as authority.

pub mod contract;
pub mod corpus;
pub mod model;
pub mod packet;

#[cfg(test)]
mod tests;

pub use contract::{IssueContract, compute_denominator_digest};
pub use corpus::{
    CORPUS_MANIFEST_SCHEMA_V1, CorpusManifest, FIXTURE_SCHEMA_V1, FixtureCase, FixtureDocument,
    FixtureProvenance, ManifestEntry, load_corpus_manifest, verify_corpus,
};
use std::fmt::{Display, Formatter};
use std::path::PathBuf;

pub use model::{
    CLOSE_PACKET_SCHEMA_V1, ChildDispositionRecord, ChildState, ClaimStatement, CloseMode,
    ClosePacket, CloseVerdict, ContractIdentity, ControlOutcome, DenominatorRow, DuplicateRef,
    EvidenceRef, ISSUE_CONTRACT_SCHEMA_V1, IssueCloseOutcome, IssueKind, IssueRef,
    NegativeControlRow, PacketBinding, PrScopeOutcome, ProofLevel, RowDispositionValue,
    RulingIdentity, TransferPolicy,
};
pub use packet::validate_packet_against_contract;

/// Validation failures are classified so callers can distinguish mis-typed
/// documents from semantic integrity violations and stale bindings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CloseProofError {
    /// Document shape violations: schema version, tokens, empty fields, and
    /// JSON deserialization errors (including unknown fields).
    Schema { field: String, message: String },
    /// Malformed or non-matching digest material.
    Digest { message: String },
    /// Missing or extra denominator rows, controls, or mandatory children.
    Coverage { message: String },
    /// Issue body, denominator, or ruling movement invalidating a packet.
    Identity { message: String },
    /// Regression-corpus manifest or fixture integrity failures.
    Corpus { message: String },
}

impl Display for CloseProofError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Schema { field, message } => {
                write!(formatter, "close-proof schema violation at `{field}`: {message}")
            }
            Self::Digest { message } => write!(formatter, "close-proof digest failure: {message}"),
            Self::Coverage { message } => {
                write!(formatter, "close-proof coverage failure: {message}")
            }
            Self::Identity { message } => {
                write!(formatter, "close-proof identity failure: {message}")
            }
            Self::Corpus { message } => write!(formatter, "close-proof corpus failure: {message}"),
        }
    }
}

impl std::error::Error for CloseProofError {}

/// SHA-256 hex digest used across contracts, packets, manifests, and evidence
/// references, so downstream producers bind identical content identically.
pub fn content_digest_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    contract::hex_digest(&hasher.finalize())
}

/// Deterministic JSON serialization used by round-trip proofs and digests.
/// `serde_json` maps are `BTreeMap`s in this workspace, so key order is stable.
pub fn canonical_json<T: serde::Serialize>(value: &T) -> Result<String, CloseProofError> {
    serde_json::to_string_pretty(value).map_err(|error| CloseProofError::Schema {
        field: "canonical_json".to_string(),
        message: error.to_string(),
    })
}

/// Stable identifiers are lowercase tokens of `[a-z0-9._-]`, at most 128
/// characters, starting alphanumeric. Row IDs own the denominator, never
/// checkbox position or wording (#10380).
pub(crate) fn is_stable_token(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 128 || !bytes[0].is_ascii_alphanumeric() {
        return false;
    }
    bytes.iter().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
    })
}

/// Digests are 64 lowercase hex characters (SHA-256).
pub(crate) fn is_digest_hex(value: &str) -> bool {
    value.len() == 64
        && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

/// Repository identifiers are lowercase `owner/name`.
pub(crate) fn is_repository_id(value: &str) -> bool {
    let mut parts = value.split('/');
    let owner = parts.next().unwrap_or_default();
    let name = parts.next().unwrap_or_default();
    if parts.next().is_some()
        || owner.is_empty()
        || name.is_empty()
        || value != value.to_lowercase()
    {
        return false;
    }
    !owner.contains(|c: char| c.is_whitespace()) && !name.contains(|c: char| c.is_whitespace())
}

/// Root directory of the immutable regression corpus.
pub fn corpus_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|path| path.join(".ci").join("close-proof-contract"))
        .unwrap_or_else(|| PathBuf::from(".ci/close-proof-contract"))
}
