//! Source generation (currentness) and content revision.
//!
//! These types separate two distinct concepts that must not be accidentally
//! conflated:
//!
//! | Concept | Type | Stable across content changes? |
//! |---|---|---|
//! | Logical file identity | [`LogicalSourceId`] | Yes |
//! | Exact byte content | [`ContentDigest`] | N/A — it *is* the content |
//! | One content snapshot | [`ContentRevision`] | No |
//! | Mutable freshness cursor | [`SourceGeneration`] | No |
//!
//! A **content revision** pairs a logical source with its exact bytes at one
//! point in time. Two revisions of the same logical source differ when the
//! content changes.
//!
//! A **source generation** is a monotonic or session-scoped freshness cursor
//! that tells consumers *how current* the paired revision is. An unknown
//! generation is always explicit — it must never be treated as "current".

use serde::{Deserialize, Serialize};

use crate::{ContentDigest, LogicalSourceId};

// ── SourceGeneration ──────────────────────────────────────────────────────────

/// A freshness cursor attached to one content revision of a logical source.
///
/// `SourceGeneration` describes *how current* a paired [`ContentRevision`] is
/// relative to the producer's session or document version. An unknown generation
/// must never be treated as evidence that the revision is up-to-date.
///
/// # Variants
///
/// * `Known(value)` — the producer has a stable, monotonically increasing or
///   session-unique freshness label (e.g. a document version counter, an LSP
///   document version integer, or an editor document generation). An empty
///   `Known` value is valid but carries low confidence; prefer `Unknown` for
///   truly absent information.
///
/// * `Unknown` — the producer cannot determine the generation at the time of
///   fact emission. Consumers must not infer freshness from this value.
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SourceGeneration {
    /// A known freshness label.
    Known(String),
    /// The generation is not available from the producer.
    #[default]
    Unknown,
}

impl SourceGeneration {
    /// Construct a `Known` generation from any string-convertible value.
    #[must_use]
    pub fn known(value: impl Into<String>) -> Self {
        Self::Known(value.into())
    }

    /// Returns `true` if and only if the generation label is present and
    /// non-empty.
    ///
    /// An `Unknown` variant always returns `false`. A `Known("")` variant also
    /// returns `false` because an empty label conveys no freshness information.
    #[must_use]
    pub fn is_known(&self) -> bool {
        matches!(self, Self::Known(v) if !v.is_empty())
    }

    /// Returns `true` if the generation is `Unknown`.
    #[must_use]
    pub fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown)
    }

    /// Borrow the generation label, if known and non-empty.
    #[must_use]
    pub fn as_label(&self) -> Option<&str> {
        match self {
            Self::Known(v) if !v.is_empty() => Some(v.as_str()),
            _ => None,
        }
    }
}

impl std::fmt::Display for SourceGeneration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Known(v) => write!(f, "generation:{v}"),
            Self::Unknown => f.write_str("generation:unknown"),
        }
    }
}

// ── ContentRevision ───────────────────────────────────────────────────────────

/// A specific content revision of a logical source: the exact bytes present at
/// one point in time.
///
/// `ContentRevision` ties a [`LogicalSourceId`] to a [`ContentDigest`].
/// Together they uniquely identify *which* file and *which* exact content — but
/// carry no freshness claim. Freshness is a property of [`SourceGeneration`],
/// not of `ContentRevision`.
///
/// Two `ContentRevision` values are equal when both the logical source and the
/// content digest agree.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ContentRevision {
    /// The stable logical file identity (revision-independent).
    pub logical_source_id: LogicalSourceId,
    /// The collision-resistant digest of the exact byte content.
    pub content_digest: ContentDigest,
}

impl ContentRevision {
    /// Create a content revision from a logical source ID and content digest.
    #[must_use]
    pub fn new(logical_source_id: LogicalSourceId, content_digest: ContentDigest) -> Self {
        Self { logical_source_id, content_digest }
    }

    /// Returns `true` when the content of this revision matches that of
    /// `other` — i.e., when both content digests are equal.
    ///
    /// Note: two revisions can have matching content but belong to different
    /// logical sources. This method compares content only, not ownership.
    #[must_use]
    pub fn same_content_as(&self, other: &Self) -> bool {
        self.content_digest == other.content_digest
    }

    /// Returns `true` when both revisions belong to the same logical source.
    #[must_use]
    pub fn same_logical_source_as(&self, other: &Self) -> bool {
        self.logical_source_id == other.logical_source_id
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use crate::{ProjectId, WorkspaceRootId};

    fn make_root() -> crate::WorkspaceRootId {
        let p = ProjectId::from_canonical_name("acme/test");
        WorkspaceRootId::from_project_and_root_key(&p, "main")
    }

    fn make_src(root: &WorkspaceRootId, path: &str) -> LogicalSourceId {
        LogicalSourceId::from_root_and_path(root, path)
    }

    // ── SourceGeneration tests ────────────────────────────────────────────────

    #[test]
    fn source_generation_known_is_known() {
        let g = SourceGeneration::known("42");
        assert!(g.is_known());
        assert!(!g.is_unknown());
        assert_eq!(g.as_label(), Some("42"));
    }

    #[test]
    fn source_generation_unknown_is_unknown() {
        let g = SourceGeneration::Unknown;
        assert!(!g.is_known());
        assert!(g.is_unknown());
        assert_eq!(g.as_label(), None);
    }

    #[test]
    fn source_generation_empty_known_is_not_known() {
        let g = SourceGeneration::Known(String::new());
        assert!(!g.is_known(), "empty Known is not a useful freshness label");
        assert_eq!(g.as_label(), None);
    }

    #[test]
    fn source_generation_default_is_unknown() {
        assert_eq!(SourceGeneration::default(), SourceGeneration::Unknown);
    }

    #[test]
    fn source_generation_display() {
        assert_eq!(format!("{}", SourceGeneration::known("7")), "generation:7");
        assert_eq!(format!("{}", SourceGeneration::Unknown), "generation:unknown");
    }

    // ── ContentRevision tests ─────────────────────────────────────────────────

    #[test]
    fn same_logical_source_different_content_gives_different_revisions() {
        let root = make_root();
        let src = make_src(&root, "lib/App.pm");
        let v1 = ContentRevision::new(src.clone(), ContentDigest::of_bytes(b"version 1"));
        let v2 = ContentRevision::new(src, ContentDigest::of_bytes(b"version 2"));
        assert_ne!(v1, v2, "different content → different revision");
        assert!(v1.same_logical_source_as(&v2), "same logical source despite different content");
        assert!(!v1.same_content_as(&v2));
    }

    #[test]
    fn same_bytes_different_logical_sources_gives_different_revisions() {
        let root = make_root();
        let src_a = make_src(&root, "lib/A.pm");
        let src_b = make_src(&root, "lib/B.pm");
        let same_content = ContentDigest::of_bytes(b"# identical bytes");
        let rev_a = ContentRevision::new(src_a, same_content.clone());
        let rev_b = ContentRevision::new(src_b, same_content);
        assert_ne!(rev_a, rev_b, "different owners → different revisions");
        assert!(!rev_a.same_logical_source_as(&rev_b));
        assert!(rev_a.same_content_as(&rev_b), "content digest is the same");
    }

    #[test]
    fn same_logical_source_same_content_gives_equal_revision() {
        let root = make_root();
        let src = make_src(&root, "lib/App.pm");
        let digest = ContentDigest::of_bytes(b"package App;\n1;\n");
        let r1 = ContentRevision::new(src.clone(), digest.clone());
        let r2 = ContentRevision::new(src, digest);
        assert_eq!(r1, r2);
    }

    #[test]
    fn logical_source_id_stable_across_revisions() {
        // The point of LogicalSourceId: it never changes, even as content does.
        let root = make_root();
        let src = make_src(&root, "lib/App.pm");
        let rev_old = ContentRevision::new(src.clone(), ContentDigest::of_bytes(b"old"));
        let rev_new = ContentRevision::new(src, ContentDigest::of_bytes(b"new"));
        assert_eq!(
            rev_old.logical_source_id, rev_new.logical_source_id,
            "logical source id must not change when content changes"
        );
    }

    #[test]
    fn identical_bytes_in_two_roots_have_distinct_revisions() {
        let p = ProjectId::from_canonical_name("acme/widget");
        let root_a = WorkspaceRootId::from_project_and_root_key(&p, "branch-a");
        let root_b = WorkspaceRootId::from_project_and_root_key(&p, "branch-b");
        let src_a = LogicalSourceId::from_root_and_path(&root_a, "lib/Shared.pm");
        let src_b = LogicalSourceId::from_root_and_path(&root_b, "lib/Shared.pm");
        let content = ContentDigest::of_bytes(b"# shared bytes");
        let rev_a = ContentRevision::new(src_a, content.clone());
        let rev_b = ContentRevision::new(src_b, content);
        assert_ne!(rev_a, rev_b, "same path, same bytes, different roots → distinct revisions");
        assert_ne!(rev_a.logical_source_id, rev_b.logical_source_id);
        assert!(rev_a.same_content_as(&rev_b));
    }
}
