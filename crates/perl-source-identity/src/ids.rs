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

use serde::{Deserialize, Serialize};

use crate::digest::DomainHasher;

// ── Domain tags ──────────────────────────────────────────────────────────────

const PROJECT_DOMAIN: &[u8] = b"perl-lsp:project-id:v1\0";
const WORKSPACE_ROOT_DOMAIN: &[u8] = b"perl-lsp:workspace-root-id:v1\0";
const LOGICAL_SOURCE_DOMAIN: &[u8] = b"perl-lsp:logical-source-id:v1\0";

// ── Wire-format helpers ───────────────────────────────────────────────────────

fn bytes_to_wire_hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
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
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
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

    /// The wire representation, e.g. `project:sha256:…`.
    #[must_use]
    pub fn as_wire(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ProjectId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

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
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
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

    /// The wire representation, e.g. `root:sha256:…`.
    #[must_use]
    pub fn as_wire(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for WorkspaceRootId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

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
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
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

    /// The wire representation, e.g. `src:sha256:…`.
    #[must_use]
    pub fn as_wire(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for LogicalSourceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

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
}
