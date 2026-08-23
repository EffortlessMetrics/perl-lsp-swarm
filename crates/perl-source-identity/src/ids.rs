//! Stable, durable identity types for projects, workspace roots, and logical sources.
//!
//! All IDs are **SHA-256 digests** of domain-separated construction material, so:
//!
//! - fixed inputs always produce byte-identical IDs;
//! - no host path, URI, traversal-order counter, or process-local value becomes
//!   stable identity;
//! - IDs of different kinds never collide even when construction material is identical.
//!
//! # Identity hierarchy
//!
//! ```text
//! ProjectId           — stable across roots, machines, and sessions
//!   └── WorkspaceRootId  — one checkout of the project at a specific root
//!         └── LogicalSourceId  — one logical file within that root
//! ```
//!
//! `LogicalSourceId` is stable across content changes: the same file at the
//! same logical path within the same root always has the same logical ID
//! regardless of what bytes it currently contains. Content revision is tracked
//! separately by [`crate::ContentRevision`].

use serde::{Deserialize, Deserializer, Serialize};

use crate::digest::{DomainHasher, validate_prefixed_wire, wire_error};

// ── Domain tags ──────────────────────────────────────────────────────────────

const PROJECT_DOMAIN: &[u8] = b"perl-lsp:project-id:v1\0";
const WORKSPACE_ROOT_DOMAIN: &[u8] = b"perl-lsp:workspace-root-id:v1\0";
const LOGICAL_SOURCE_DOMAIN: &[u8] = b"perl-lsp:logical-source-id:v1\0";

// ── Wire-format helpers ───────────────────────────────────────────────────────

fn bytes_to_wire_hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Give one ID newtype its validating wire contract.
///
/// Every durable ID in this crate shares the same wire shape
/// (`<type>:sha256:<64 lowercase hex digits>`) and the same fail-closed
/// deserialization rule. Generating the three implementations from one macro
/// keeps that contract identical by construction rather than by review: a
/// change to the accepted form cannot land for one ID type and be forgotten
/// for the others.
macro_rules! wire_id {
    ($ty:ident, $prefix:literal, $expected:literal) => {
        impl $ty {
            /// Parse this ID from its wire representation.
            ///
            /// Returns `None` unless the string is exactly
            #[doc = concat!("`", $prefix, "<64 lowercase hex digits>`.")]
            /// Uppercase hex is rejected rather than normalized, because
            /// equality and hashing are defined over the wire string — one ID
            /// must have exactly one spelling.
            #[must_use]
            pub fn from_wire(s: &str) -> Option<Self> {
                validate_prefixed_wire(s, $prefix).then(|| Self(s.to_owned()))
            }

            /// The wire representation, e.g.
            #[doc = concat!("`", $prefix, "…`.")]
            #[must_use]
            pub fn as_wire(&self) -> &str {
                &self.0
            }
        }

        impl<'de> Deserialize<'de> for $ty {
            /// Validating deserialization: an ID that does not match the wire
            /// contract is rejected at the serde boundary, so an ill-formed ID
            /// can never exist as a value of this type.
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let raw = String::deserialize(deserializer)?;
                Self::from_wire(&raw).ok_or_else(|| wire_error(&raw, $expected))
            }
        }

        impl std::fmt::Display for $ty {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

// ── ProjectId ─────────────────────────────────────────────────────────────────

/// Durable identity for a project (logical grouping of source trees).
///
/// A `ProjectId` is stable across machines, repository checkouts, and time: it
/// depends only on the **canonical project name** (e.g. a VCS remote URL or an
/// authority-defined slug), never on the local checkout path or host environment.
///
/// # Encoding
///
/// ```text
/// perl-lsp:project-id:v1\0 || length_prefixed(canonical_name)
/// ```
///
/// Where `length_prefixed(x)` = `u32_be(len(x)) || x`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct ProjectId(String);

impl ProjectId {
    /// Derive a project ID from its canonical authority-defined name.
    ///
    /// The `canonical_name` should be a stable, authority-defined identifier
    /// such as a VCS URL (`https://github.com/acme/widget`) or a slug
    /// (`acme/widget`). It must not encode local paths, user names, or any
    /// host-specific state.
    #[must_use]
    pub fn from_canonical_name(canonical_name: &str) -> Self {
        let mut h = DomainHasher::new(PROJECT_DOMAIN);
        h.push_field(canonical_name.as_bytes());
        let raw = h.finish();
        Self(format!("project:sha256:{}", bytes_to_wire_hex(&raw)))
    }
}

wire_id!(
    ProjectId,
    "project:sha256:",
    "a project ID of the form `project:sha256:<64 lowercase hex digits>`"
);

// ── WorkspaceRootId ───────────────────────────────────────────────────────────

/// Durable identity for a specific workspace root within a project.
///
/// Multiple workspace roots of the same project produce distinct IDs even when
/// the root-relative content is identical. A workspace root is typically one
/// checkout of the project at a specific commit or worktree, identified by an
/// authority-defined root key (e.g. a VCS commit SHA or a canonical workspace
/// label).
///
/// # Encoding
///
/// ```text
/// perl-lsp:workspace-root-id:v1\0
///   || length_prefixed(project_id.as_wire())
///   || length_prefixed(root_key)
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct WorkspaceRootId(String);

impl WorkspaceRootId {
    /// Derive a workspace root ID from its project and an authority-defined root key.
    ///
    /// The `root_key` should be a stable, authority-defined identifier for this
    /// particular root within the project (e.g. a VCS commit SHA, a worktree
    /// label, or a build-system canonical root path that is not host-specific).
    ///
    /// Physical host paths must **not** be used as the root key. Two roots at
    /// different local paths but representing the same logical checkout of the
    /// same commit would otherwise produce different IDs, breaking cross-machine
    /// identity.
    #[must_use]
    pub fn from_project_and_root_key(project_id: &ProjectId, root_key: &str) -> Self {
        let mut h = DomainHasher::new(WORKSPACE_ROOT_DOMAIN);
        h.push_field(project_id.as_wire().as_bytes());
        h.push_field(root_key.as_bytes());
        let raw = h.finish();
        Self(format!("root:sha256:{}", bytes_to_wire_hex(&raw)))
    }
}

wire_id!(
    WorkspaceRootId,
    "root:sha256:",
    "a workspace root ID of the form `root:sha256:<64 lowercase hex digits>`"
);

// ── LogicalSourceId ───────────────────────────────────────────────────────────

/// Stable identity for one logical source file within a workspace root.
///
/// `LogicalSourceId` is **revision-independent**: the same file at the same
/// logical path within the same root always produces the same ID regardless of
/// what bytes it currently contains. Content revision is a separate concept
/// tracked by [`crate::ContentRevision`].
///
/// Two files at the same `root_relative_path` but in different roots produce
/// **different** `LogicalSourceId` values — ownership by the root is part of
/// the identity.
///
/// # Path requirements
///
/// `root_relative_path` must be a forward-slash separated path relative to the
/// workspace root, without a leading slash, and must not contain `..` segments.
/// No path normalization is performed here; callers are responsible for
/// supplying a canonical form.
///
/// # Encoding
///
/// ```text
/// perl-lsp:logical-source-id:v1\0
///   || length_prefixed(workspace_root_id.as_wire())
///   || length_prefixed(root_relative_path)
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct LogicalSourceId(String);

impl LogicalSourceId {
    /// Derive a logical source ID from its workspace root and root-relative path.
    ///
    /// The `root_relative_path` must be canonical (forward-slash separated, no
    /// leading slash, no `..` segments). This crate does not enforce that
    /// invariant; callers must normalize before calling.
    #[must_use]
    pub fn from_root_and_path(
        workspace_root_id: &WorkspaceRootId,
        root_relative_path: &str,
    ) -> Self {
        let mut h = DomainHasher::new(LOGICAL_SOURCE_DOMAIN);
        h.push_field(workspace_root_id.as_wire().as_bytes());
        h.push_field(root_relative_path.as_bytes());
        let raw = h.finish();
        Self(format!("src:sha256:{}", bytes_to_wire_hex(&raw)))
    }
}

wire_id!(
    LogicalSourceId,
    "src:sha256:",
    "a logical source ID of the form `src:sha256:<64 lowercase hex digits>`"
);

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    // ── ProjectId tests ───────────────────────────────────────────────────────

    #[test]
    fn project_id_is_deterministic() {
        let a = ProjectId::from_canonical_name("https://github.com/acme/widget");
        let b = ProjectId::from_canonical_name("https://github.com/acme/widget");
        assert_eq!(a, b);
    }

    #[test]
    fn different_project_names_produce_different_ids() {
        let a = ProjectId::from_canonical_name("acme/widget");
        let b = ProjectId::from_canonical_name("acme/gadget");
        assert_ne!(a, b);
    }

    #[test]
    fn project_id_wire_has_prefix() {
        let id = ProjectId::from_canonical_name("test-project");
        assert!(id.as_wire().starts_with("project:sha256:"));
    }

    #[test]
    fn project_id_display_matches_wire() {
        let id = ProjectId::from_canonical_name("test");
        assert_eq!(format!("{id}"), id.as_wire());
    }

    // ── WorkspaceRootId tests ─────────────────────────────────────────────────

    #[test]
    fn root_id_is_deterministic() {
        let project = ProjectId::from_canonical_name("acme/widget");
        let a = WorkspaceRootId::from_project_and_root_key(&project, "abc123");
        let b = WorkspaceRootId::from_project_and_root_key(&project, "abc123");
        assert_eq!(a, b);
    }

    #[test]
    fn same_root_key_different_projects_gives_different_root_ids() {
        let p1 = ProjectId::from_canonical_name("acme/widget");
        let p2 = ProjectId::from_canonical_name("acme/gadget");
        let r1 = WorkspaceRootId::from_project_and_root_key(&p1, "same-key");
        let r2 = WorkspaceRootId::from_project_and_root_key(&p2, "same-key");
        assert_ne!(r1, r2);
    }

    #[test]
    fn different_root_keys_same_project_gives_different_root_ids() {
        let project = ProjectId::from_canonical_name("acme/widget");
        let r1 = WorkspaceRootId::from_project_and_root_key(&project, "main-branch");
        let r2 = WorkspaceRootId::from_project_and_root_key(&project, "feature-branch");
        assert_ne!(r1, r2);
    }

    #[test]
    fn root_id_wire_has_prefix() {
        let project = ProjectId::from_canonical_name("test");
        let id = WorkspaceRootId::from_project_and_root_key(&project, "key");
        assert!(id.as_wire().starts_with("root:sha256:"));
    }

    // ── LogicalSourceId tests ─────────────────────────────────────────────────

    #[test]
    fn logical_source_id_is_deterministic() {
        let project = ProjectId::from_canonical_name("acme/widget");
        let root = WorkspaceRootId::from_project_and_root_key(&project, "main");
        let a = LogicalSourceId::from_root_and_path(&root, "lib/Widget.pm");
        let b = LogicalSourceId::from_root_and_path(&root, "lib/Widget.pm");
        assert_eq!(a, b);
    }

    #[test]
    fn same_path_different_roots_gives_different_logical_ids() {
        let project = ProjectId::from_canonical_name("acme/widget");
        let root1 = WorkspaceRootId::from_project_and_root_key(&project, "main");
        let root2 = WorkspaceRootId::from_project_and_root_key(&project, "feature");
        let id1 = LogicalSourceId::from_root_and_path(&root1, "lib/Widget.pm");
        let id2 = LogicalSourceId::from_root_and_path(&root2, "lib/Widget.pm");
        assert_ne!(id1, id2, "same path in different roots → different logical IDs");
    }

    #[test]
    fn different_paths_same_root_gives_different_logical_ids() {
        let project = ProjectId::from_canonical_name("acme/widget");
        let root = WorkspaceRootId::from_project_and_root_key(&project, "main");
        let id1 = LogicalSourceId::from_root_and_path(&root, "lib/Foo.pm");
        let id2 = LogicalSourceId::from_root_and_path(&root, "lib/Bar.pm");
        assert_ne!(id1, id2);
    }

    #[test]
    fn logical_source_id_wire_has_prefix() {
        let project = ProjectId::from_canonical_name("test");
        let root = WorkspaceRootId::from_project_and_root_key(&project, "main");
        let id = LogicalSourceId::from_root_and_path(&root, "lib/App.pm");
        assert!(id.as_wire().starts_with("src:sha256:"));
    }

    #[test]
    fn logical_source_id_stable_across_content_changes() {
        // The logical ID must not depend on file content — only on root + path.
        let project = ProjectId::from_canonical_name("acme/widget");
        let root = WorkspaceRootId::from_project_and_root_key(&project, "main");
        let id_v1 = LogicalSourceId::from_root_and_path(&root, "lib/App.pm");
        let id_v2 = LogicalSourceId::from_root_and_path(&root, "lib/App.pm");
        // Content not involved — these must be equal regardless of file bytes.
        assert_eq!(id_v1, id_v2, "logical id is revision-independent");
    }

    // ── Cross-type collision resistance ───────────────────────────────────────

    #[test]
    fn project_id_wire_does_not_collide_with_root_id_wire() {
        // Even with the same input material, different ID types must not produce
        // the same wire string.
        let project = ProjectId::from_canonical_name("same-material");
        let root = WorkspaceRootId::from_project_and_root_key(
            &ProjectId::from_canonical_name("parent"),
            "same-material",
        );
        assert_ne!(
            project.as_wire(),
            root.as_wire(),
            "type prefixes must prevent cross-type collisions"
        );
    }

    // ── Wire parsing and validating deserialization ───────────────────────────

    #[test]
    fn id_wire_round_trips_through_from_wire() {
        let project = ProjectId::from_canonical_name("acme/widget");
        let root = WorkspaceRootId::from_project_and_root_key(&project, "main");
        let src = LogicalSourceId::from_root_and_path(&root, "lib/App.pm");

        assert_eq!(ProjectId::from_wire(project.as_wire()).as_ref(), Some(&project));
        assert_eq!(WorkspaceRootId::from_wire(root.as_wire()).as_ref(), Some(&root));
        assert_eq!(LogicalSourceId::from_wire(src.as_wire()).as_ref(), Some(&src));
    }

    /// The type prefix is load-bearing: a wire string minted for one ID kind
    /// must not parse as another, even though the digest body is well-formed.
    #[test]
    fn id_wire_rejects_cross_type_substitution() {
        let project = ProjectId::from_canonical_name("acme/widget");
        let root = WorkspaceRootId::from_project_and_root_key(&project, "main");
        let src = LogicalSourceId::from_root_and_path(&root, "lib/App.pm");

        assert!(ProjectId::from_wire(root.as_wire()).is_none(), "root wire is not a project ID");
        assert!(ProjectId::from_wire(src.as_wire()).is_none(), "src wire is not a project ID");
        assert!(
            WorkspaceRootId::from_wire(project.as_wire()).is_none(),
            "project wire is not a root ID"
        );
        assert!(
            LogicalSourceId::from_wire(root.as_wire()).is_none(),
            "root wire is not a logical source ID"
        );
    }

    #[test]
    fn id_wire_rejects_malformed_bodies() {
        let hex64 = "0".repeat(64);
        assert!(ProjectId::from_wire("project:sha256:short").is_none(), "short body");
        assert!(ProjectId::from_wire(&format!("project:sha256:{hex64}x")).is_none(), "long body");
        assert!(ProjectId::from_wire(&hex64).is_none(), "no prefix");
        assert!(ProjectId::from_wire("").is_none(), "empty");
        assert!(
            ProjectId::from_wire(&format!("project:sha256:{}", "A".repeat(64))).is_none(),
            "uppercase hex must be rejected, not normalized"
        );
    }

    #[test]
    fn id_deserialization_is_validating() {
        let project = ProjectId::from_canonical_name("acme/widget");
        let json = serde_json::to_string(&project).expect("serialize");
        let back: ProjectId = serde_json::from_str(&json).expect("valid wire must parse");
        assert_eq!(project, back);

        for bad in [
            "\"\"",
            "\"not-an-id\"",
            "\"project:sha256:short\"",
            "\"root:sha256:0000000000000000000000000000000000000000000000000000000000000000\"",
            "\"project:sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\"",
        ] {
            assert!(
                serde_json::from_str::<ProjectId>(bad).is_err(),
                "ProjectId deserialization must reject {bad}"
            );
        }

        // A well-formed *project* ID must not deserialize into a root ID.
        assert!(
            serde_json::from_str::<WorkspaceRootId>(&json).is_err(),
            "cross-type wire strings must not deserialize"
        );
    }
}
