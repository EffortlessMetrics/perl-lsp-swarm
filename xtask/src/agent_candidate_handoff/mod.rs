//! `agent_candidate_handoff.v1` — content-addressed candidate transport (D1,
//! issue #13379).
//!
//! An executor can legitimately finish substantial work in a workspace with no
//! authenticated GitHub client and no usable remote. Reporting a short SHA in a
//! comment records intent but preserves nothing: the commit, its ordered
//! parents, tree, file modes, renames, deletions, and binary blobs all live in
//! a disposable local object database.
//!
//! This module turns that local candidate into one immutable, independently
//! validatable envelope:
//!
//! ```text
//! <envelope>/
//!   manifest.json     agent_candidate_handoff.v1 — the semantic claim
//!   candidate.pack    a self-contained Git object set
//!   receipt.json      the producer's own validation result
//!   proof/<id>        optional content-addressed proof artifacts
//! ```
//!
//! # Authority boundary
//!
//! Creating and checking a handoff is read-only and credential-free. This
//! layer publishes no branch, opens or updates no pull request, claims no
//! hosted check, and grants no integration authority — D2 (#13386) owns
//! compare-and-swap publication. A referenced local proof is local proof, and
//! every manifest says so through [`model::LimitationCode::LocalProofOnly`].

pub mod check;
pub mod create;
pub mod git;
pub mod hygiene;
pub mod model;
pub mod render;

#[cfg(test)]
mod tests;

pub use check::{CheckDimension, CheckReport, DimensionVerdict, check_handoff};
pub use create::{CreateRequest, create_handoff};
pub use model::{
    HANDOFF_MANIFEST_SCHEMA_V1, HANDOFF_RECEIPT_SCHEMA_V1, MANIFEST_FILE_NAME, Manifest,
    PACK_FILE_NAME, PROOF_DIR_NAME, ProducerReceipt, RECEIPT_FILE_NAME,
};
pub use render::{ExplainDocument, describe, explain, render_check_human, render_explain_human};

use serde::{Deserialize, Serialize};

/// Terminal classification of one handoff evaluation.
///
/// The vocabulary keeps failure classes apart that a single boolean would
/// collapse: a transport whose bytes were altered, a manifest whose claims no
/// longer match the objects, an envelope carrying a secret, and an instrument
/// that could not run are four different facts with four different repairs.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HandoffOutcome {
    /// Every dimension is valid; the candidate is independently reconstructable.
    ValidHandoff,
    /// The manifest is absent, unparseable, or structurally invalid.
    InvalidManifest,
    /// A declared object is missing from the transport.
    MissingObject,
    /// Declared transport bytes or sizes do not match the envelope.
    DigestMismatch,
    /// The imported candidate tree differs from the declared tree.
    TreeMismatch,
    /// Declared parents differ from the imported commit's ordered parents.
    ParentMismatch,
    /// The recomputed changed-path inventory differs from the declared one.
    InventoryMismatch,
    /// Credential or secret material was found in retained content.
    UnsafeContent,
    /// The envelope claims an object class this format cannot carry.
    UnsupportedObjectClass,
    /// The candidate is transportable, but no repository identity was proven.
    RepositoryIdentityNotProven,
    /// A declared proof artifact is not bound to this exact candidate.
    ProofSubjectMismatch,
    /// Git, the filesystem, or the temporary object database failed.
    InstrumentFailure,
}

impl HandoffOutcome {
    /// Stable machine spelling.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::ValidHandoff => "VALID_HANDOFF",
            Self::InvalidManifest => "INVALID_MANIFEST",
            Self::MissingObject => "MISSING_OBJECT",
            Self::DigestMismatch => "DIGEST_MISMATCH",
            Self::TreeMismatch => "TREE_MISMATCH",
            Self::ParentMismatch => "PARENT_MISMATCH",
            Self::InventoryMismatch => "INVENTORY_MISMATCH",
            Self::UnsafeContent => "UNSAFE_CONTENT",
            Self::UnsupportedObjectClass => "UNSUPPORTED_OBJECT_CLASS",
            Self::RepositoryIdentityNotProven => "REPOSITORY_IDENTITY_NOT_PROVEN",
            Self::ProofSubjectMismatch => "PROOF_SUBJECT_MISMATCH",
            Self::InstrumentFailure => "INSTRUMENT_FAILURE",
        }
    }

    /// Stable process exit code for shell and workflow consumers.
    ///
    /// `0` valid, `2` an invalid candidate claim, `3` transportable but not
    /// proven, `4` the instrument itself failed. A caller that only branches
    /// on zero still behaves correctly; a caller that needs the distinction
    /// between "this handoff is wrong" and "this handoff could not be
    /// evaluated" has it.
    #[must_use]
    pub const fn exit_code(&self) -> u8 {
        match self {
            Self::ValidHandoff => 0,
            Self::InvalidManifest
            | Self::MissingObject
            | Self::DigestMismatch
            | Self::TreeMismatch
            | Self::ParentMismatch
            | Self::InventoryMismatch
            | Self::UnsafeContent
            | Self::UnsupportedObjectClass
            | Self::ProofSubjectMismatch => 2,
            Self::RepositoryIdentityNotProven => 3,
            Self::InstrumentFailure => 4,
        }
    }
}

/// SHA-256 of `bytes` as lowercase hex.
#[must_use]
pub fn content_digest_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        // Writing to a String cannot fail; the result is discarded knowingly.
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// Deterministic JSON used for digests and for every document written to disk.
///
/// Struct field order is declaration order and maps are ordered, so the same
/// value always produces the same bytes.
pub fn canonical_json<T: Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_string_pretty(value).map_err(|error| format!("serializing JSON: {error}"))
}

/// Whether `value` is a lowercase 64-character SHA-256 hex digest.
#[must_use]
pub fn is_digest_hex(value: &str) -> bool {
    value.len() == 64
        && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
