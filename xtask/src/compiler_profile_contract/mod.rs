//! Maintained compiler-profile identity and closure types (COMP-PROFILE-C01,
//! issue #12186).
//!
//! This module owns the dependency-neutral in-memory domain model for
//! maintained compiler operating profiles: exact profile identity, row
//! dispositions, imports, subject dimensions, evidence requirements,
//! limitations, ownership, invalidation inputs, and claim ceilings, together
//! with the closure laws that hold them honest:
//!
//! - every required applicable row is conjunctive; conditional, optional,
//!   unsupported, and not-applicable are closed typed states, never omitted
//!   rows;
//! - a profile imports an exact lower profile identity/version/digest and
//!   preserves every imported row and limitation verbatim;
//! - proof classes, source tiers, work classes, and claim families are
//!   independent axes: no axis satisfies another, though one row may require
//!   several;
//! - profile identity is a deterministic semantic fingerprint that changes
//!   when any semantic field changes and is independent of row or map
//!   insertion order;
//! - no weighted or global readiness score exists anywhere in the model, and
//!   a profile result never authorizes support, release, or publication.
//!
//! Scope boundaries (issue non-goals): there is deliberately no checked file
//! syntax, manifest loader, receipt adapter, evaluator, candidate status,
//! command, or support/release behavior here. The successor initial-row
//! inventory instantiates this vocabulary; #12187 serializes that inventory
//! as the canonical checked manifest; #12177 owns evidence/evaluation.
//!
//! Serde strictness note: struct-shaped documents reject unknown fields
//! (`deny_unknown_fields`), but payload variants inside internally tagged
//! enums (`disposition`, `subject`, `rule`, `policy`) cannot, so unknown keys
//! inside those variant payloads are ignored by serde, and serde_json
//! resolves duplicate JSON object keys last-wins. Downstream evaluators must
//! not treat that silence as authority; validation remains the arbiter.

pub mod contract;
pub mod model;

#[cfg(test)]
mod tests;

pub use model::CompilerProfileDefinition;
pub use model::{
    AllowedLimitation, ClaimCeiling, ClaimFamily, CompilerProfileId, CompilerProfileImport,
    CompilerProfileRow, CompilerProfileRowId, CompilerProfileVersion, CompletenessRequirement,
    CompletenessRule, EvidenceObservation, EvidenceRequirement, InvalidationInput,
    LegacyExitRequirement, LimitationPolicy, OwnerAndWakeEvent, OwnerToken, ProfileDigest,
    ProofClass, RowDisposition, SourceTier, SubjectSelector, WakeEvent, WorkClass, WorkObservation,
    WorkRequirement,
};

use std::fmt::{Display, Formatter};

/// Validation failures are classified so callers can distinguish mis-typed
/// shapes from broken identity, closure, and authorization laws.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompilerProfileContractError {
    /// Shape violations: tokens, empty fields, malformed payloads, and JSON
    /// deserialization errors (including unknown struct fields).
    Schema { field: String, message: String },
    /// Malformed, stale, unresolvable, or cyclic import identity.
    Identity { message: String },
    /// Broken preservation of imported rows or limitations, or a row that
    /// disappeared by omission.
    Closure { message: String },
    /// A profile row attempted to carry support, release, or publication
    /// authority.
    Authorization { message: String },
}

impl Display for CompilerProfileContractError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Schema { field, message } => {
                write!(formatter, "compiler-profile schema violation at `{field}`: {message}")
            }
            Self::Identity { message } => {
                write!(formatter, "compiler-profile identity failure: {message}")
            }
            Self::Closure { message } => {
                write!(formatter, "compiler-profile closure failure: {message}")
            }
            Self::Authorization { message } => {
                write!(formatter, "compiler-profile authorization failure: {message}")
            }
        }
    }
}

impl std::error::Error for CompilerProfileContractError {}

/// Deterministic JSON serialization used by round-trip proofs and
/// fingerprints. All maps in the model are `BTreeMap`s, so key order is
/// stable and insertion order never leaks into the bytes.
pub fn canonical_json<T: serde::Serialize>(
    value: &T,
) -> Result<String, CompilerProfileContractError> {
    serde_json::to_string_pretty(value).map_err(|error| CompilerProfileContractError::Schema {
        field: "canonical_json".to_string(),
        message: error.to_string(),
    })
}

/// Digests are 64 lowercase hex characters (SHA-256).
pub(crate) fn is_digest_hex(value: &str) -> bool {
    value.len() == 64
        && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

/// Stable identifiers are lowercase tokens of `[a-z0-9._-]`, at most 128
/// characters, starting alphanumeric. Row, profile, owner, and limitation ids
/// own identity, never ordering or wording.
pub(crate) fn is_stable_token(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 128 || !bytes[0].is_ascii_alphanumeric() {
        return false;
    }
    bytes.iter().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
    })
}

pub(crate) fn hex_digest(digest: &[u8]) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}
