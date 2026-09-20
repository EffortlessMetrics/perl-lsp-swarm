//! Transport-neutral stable semantic identity and ownership contract (#12121).
//!
//! This module defines the durable scope, contribution, owner, and dependency
//! identity vocabulary consumed by fresh-full semantic construction (#7306,
//! #12135/#12136/#12138), typed AST effect migration (#8448), generation-owned
//! semantic snapshots (#8557/#12150), and — later — the incremental edit-impact
//! successor (#12122/#7308). It changes no semantic output and performs no AST
//! traversal.
//!
//! # Identity law
//!
//! - A scope or contribution identity binds the exact logical source subject
//!   (document instance, root, accepted source generation, accepted parser
//!   snapshot/configuration, semantic profile). Source-identical later
//!   generations, close/reopen instances, and the same relative path/content in
//!   two roots remain distinct current subjects.
//! - Logical identity composes the scope kind, owning-declaration key, parent
//!   logical fingerprint, source anchor digest (header/subtree text, not a raw
//!   offset, line, or display name alone), package/source-order context, and
//!   recovery/synthetic/ambiguity disposition.
//! - Identity never reduces to traversal-order [`crate::ScopeId`] values,
//!   pointer identity, bare paths, line numbers, source offsets, display names,
//!   or map insertion order. Inserting an unrelated earlier scope must not
//!   change an unaffected logical scope identity.
//! - Every semantic contribution is owned by exactly one typed owner; package
//!   transitions, imports, prototypes, class facts, and following-source
//!   context are not forced into lexical-scope buckets.
//! - Producer identity never upgrades completeness. Empty collections do not
//!   determine completeness.
//!
//! # Transport trust boundary
//!
//! `Serialize`/`Deserialize` on these types is a wire shape, not an
//! invariant guard: deserializing untrusted JSON can produce records that
//! `new()` would have rejected (contradictory statuses, duplicate relations,
//! blank payloads). Any consumer accepting identities from a transport must
//! call the type's `validate()` (and treat a fingerprint match as a
//! candidate confirmed by structural equality) before reuse. The JSON
//! round-trip fixture proves `validate()` rejects deserialized garbage.
//!
//! # Ownership fence
//!
//! This module owns no LSP protocol type, parser type, provider policy,
//! workspace storage, edit-impact classification, range-rebase law, or
//! work-avoidance strategy. The `architecture_fence` test enforces the import
//! boundary mechanically. The incremental successor (#12122) consumes these
//! identities; it does not redefine them.
//!
//! # Non-goals
//!
//! No edit-impact classifier, no range rebasing, no incremental algorithm, no
//! full contribution builder, no semantic snapshot publication, and no
//! retained/rebased/recomputed/fallback receipt semantics — the #12122
//! successor owns those fields.

mod contribution;
mod fingerprint;
mod scope;
mod work;

#[cfg(test)]
mod tests;

pub use contribution::{
    SemanticContributionId, SemanticContributionOwner, SemanticDeclarationKey,
    SemanticDependencyIdentity, SemanticDependencyKind, SemanticFactFamily,
    SemanticOwnershipDisposition, SemanticSubjectStatus,
};
pub use fingerprint::SemanticIdentityFingerprint;
pub use scope::{
    SemanticAnchorRole, SemanticScopeIdentity, SemanticScopeKind, SemanticScopeRecovery,
    SemanticSemanticProfileIdentity, SemanticSourceAnchor, SemanticSourceOrderIdentity,
    SemanticSubjectGeneration,
};
pub use work::{
    SemanticInstrumentBudgetState, SemanticProducerStrategyIdentity, SemanticWorkSubjectIdentity,
};

use serde::{Deserialize, Serialize};

/// Error returned by contract validators in this module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemanticIdentityContractError {
    /// An identity field that must identify a subject is empty or whitespace.
    EmptyIdentityField(&'static str),
    /// A structurally required companion field is missing.
    MissingCompanion(&'static str),
    /// A typed status/recovery combination cannot be claimed together.
    ContradictoryStatus(&'static str),
}

impl std::fmt::Display for SemanticIdentityContractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyIdentityField(field) => {
                write!(f, "semantic identity field `{field}` must be a non-empty identity")
            }
            Self::MissingCompanion(field) => {
                write!(f, "semantic identity requires companion field `{field}`")
            }
            Self::ContradictoryStatus(reason) => {
                write!(f, "contradictory semantic identity status: {reason}")
            }
        }
    }
}

impl std::error::Error for SemanticIdentityContractError {}

/// Schema/producer version tag carried by every identity in this contract.
///
/// Producers and consumers must agree on this tag before comparing
/// fingerprints; a mismatch is an incompatible identity, not an equality
/// failure to be repaired by string comparison.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SemanticIdentitySchema {
    /// First version of the stable semantic identity contract (#12121).
    V1,
}

impl SemanticIdentitySchema {
    /// Stable textual tag used inside deterministic fingerprints.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::V1 => "semantic-identity-v1",
        }
    }
}
