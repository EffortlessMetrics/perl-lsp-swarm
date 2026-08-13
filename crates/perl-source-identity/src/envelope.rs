//! The canonical `source_identity.v1` transport envelope.
//!
//! [`SourceIdentityEnvelope`] is the top-level answer to the fundamental
//! questions any consumer needs about a source unit:
//!
//! ```text
//! which project/root owns the source?
//! which logical source is this?
//! which exact content revision is represented?
//! which generation/currentness value is attached?
//! what source-origin/physical-role class applies?
//! what is known, unavailable, virtual, generated, staged, upstream, or runtime-derived?
//! ```
//!
//! Physical path details and origin mappings are *referenced* in the envelope
//! but their full implementation is deferred to the mapping/redaction child
//! (issue #7659).
//!
//! # Schema version
//!
//! The schema is versioned so that forward-compatible additions can be made
//! without changing the schema version, while breaking changes increment it.
//! Consumers should reject envelopes whose `schema_version` they do not
//! recognize.

use serde::{Deserialize, Serialize};

use crate::{
    ContentRevision, LogicalSourceId, PhysicalSourceRole, ProjectId, SourceGeneration,
    SourceOrigin, WorkspaceRootId,
};

/// Current schema version for [`SourceIdentityEnvelope`].
pub const SCHEMA_VERSION_V1: u32 = 1;

/// A versioned schema marker for the `source_identity.v1` envelope format.
///
/// Consumers should verify the schema version before interpreting the envelope.
/// An unknown version must be treated as an error, not silently promoted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SourceIdentitySchemaVersion(pub u32);

impl SourceIdentitySchemaVersion {
    /// The current `source_identity.v1` schema version.
    pub const V1: Self = Self(SCHEMA_VERSION_V1);

    /// Returns `true` if this version is one the current runtime recognizes.
    #[must_use]
    pub fn is_supported(&self) -> bool {
        self.0 == SCHEMA_VERSION_V1
    }

    /// Unwrap the raw integer version.
    #[must_use]
    pub fn as_u32(self) -> u32 {
        self.0
    }
}

impl std::fmt::Display for SourceIdentitySchemaVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "source_identity.v{}", self.0)
    }
}

/// The `source_identity.v1` transport envelope.
///
/// An envelope independently answers:
///
/// - **Which project and workspace root own the source?** → `project_id`,
///   `workspace_root_id`
/// - **Which logical source unit is this?** → `logical_source_id`
/// - **Which exact content revision is represented?** → `content_revision`
///   (`None` = unavailable or virtual with no current byte content)
/// - **How current is the revision?** → `generation`
/// - **Where did this source come from?** → `source_origin`
/// - **What physical/functional role does it play?** → `physical_source_role`
///
/// # Freshness contract
///
/// An envelope with `generation: SourceGeneration::Unknown` must **not** be
/// treated by consumers as evidence of a current, source-backed answer. The
/// `Unknown` generation is a first-class value, not an absent field.
///
/// # Physical path details
///
/// Physical path information and origin mappings are out of scope for v1 of
/// this envelope. They will be added in the mapping/redaction child (issue
/// #7659). The `physical_source_role` and `source_origin` fields record the
/// *class* of the physical relationship without exposing sensitive paths.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SourceIdentityEnvelope {
    /// The schema version that produced this envelope.
    ///
    /// Consumers must reject envelopes with unrecognized versions rather than
    /// silently promoting them.
    pub schema_version: SourceIdentitySchemaVersion,

    /// Durable identity for the project that owns this source.
    pub project_id: ProjectId,

    /// Identity of the specific workspace root within the project.
    pub workspace_root_id: WorkspaceRootId,

    /// Stable, revision-independent identity for this logical source unit.
    ///
    /// This ID does not change when the content changes.
    pub logical_source_id: LogicalSourceId,

    /// The exact content revision, if available.
    ///
    /// `None` indicates that no content revision is currently available (e.g.
    /// for a virtual source that has no byte content, or when the revision has
    /// not yet been computed).
    pub content_revision: Option<ContentRevision>,

    /// Freshness cursor for the content revision.
    ///
    /// `SourceGeneration::Unknown` is an explicit state — never an absent field.
    /// Consumers must not infer freshness from an unknown generation.
    pub generation: SourceGeneration,

    /// Origin classification for the logical source.
    pub source_origin: SourceOrigin,

    /// Physical/functional role of this source in the workspace.
    pub physical_source_role: PhysicalSourceRole,
}

impl SourceIdentityEnvelope {
    /// Create a minimal v1 envelope for an ordinary workspace file.
    ///
    /// Sets `source_origin` to `Workspace` and `physical_source_role` to
    /// `Primary`. Use the builder pattern for other variants.
    #[must_use]
    pub fn for_workspace_file(
        project_id: ProjectId,
        workspace_root_id: WorkspaceRootId,
        logical_source_id: LogicalSourceId,
        content_revision: Option<ContentRevision>,
        generation: SourceGeneration,
    ) -> Self {
        Self {
            schema_version: SourceIdentitySchemaVersion::V1,
            project_id,
            workspace_root_id,
            logical_source_id,
            content_revision,
            generation,
            source_origin: SourceOrigin::Workspace,
            physical_source_role: PhysicalSourceRole::Primary,
        }
    }

    /// Create a v1 envelope for a virtual source (no physical path).
    ///
    /// Sets `source_origin` to `Virtual` and `physical_source_role` to
    /// `Unavailable` since there is no physical location to reference.
    #[must_use]
    pub fn for_virtual_source(
        project_id: ProjectId,
        workspace_root_id: WorkspaceRootId,
        logical_source_id: LogicalSourceId,
        content_revision: Option<ContentRevision>,
        generation: SourceGeneration,
    ) -> Self {
        Self {
            schema_version: SourceIdentitySchemaVersion::V1,
            project_id,
            workspace_root_id,
            logical_source_id,
            content_revision,
            generation,
            source_origin: SourceOrigin::Virtual,
            physical_source_role: PhysicalSourceRole::Unavailable,
        }
    }

    /// Returns `true` if the envelope carries a known, non-empty generation
    /// label (i.e. the content revision is claimed to be current).
    #[must_use]
    pub fn has_known_generation(&self) -> bool {
        self.generation.is_known()
    }

    /// Returns `true` if the schema version is one this runtime understands.
    ///
    /// Consumers should call this before interpreting the envelope fields.
    #[must_use]
    pub fn is_schema_supported(&self) -> bool {
        self.schema_version.is_supported()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use crate::{ContentDigest, ContentRevision, LogicalSourceId};

    fn make_envelope() -> (ProjectId, WorkspaceRootId, LogicalSourceId) {
        let project = ProjectId::from_canonical_name("acme/widget");
        let root = WorkspaceRootId::from_project_and_root_key(&project, "main-branch");
        let src = LogicalSourceId::from_root_and_path(&root, "lib/Widget.pm");
        (project, root, src)
    }

    // ── Schema version tests ──────────────────────────────────────────────────

    #[test]
    fn schema_version_v1_is_supported() {
        assert!(SourceIdentitySchemaVersion::V1.is_supported());
    }

    #[test]
    fn unknown_schema_version_is_unsupported() {
        assert!(!SourceIdentitySchemaVersion(0).is_supported());
        assert!(!SourceIdentitySchemaVersion(99).is_supported());
    }

    #[test]
    fn schema_version_display() {
        assert_eq!(format!("{}", SourceIdentitySchemaVersion::V1), "source_identity.v1");
        assert_eq!(format!("{}", SourceIdentitySchemaVersion(2)), "source_identity.v2");
    }

    // ── Envelope construction tests ───────────────────────────────────────────

    #[test]
    fn workspace_file_envelope_has_correct_defaults() {
        let (project, root, src) = make_envelope();
        let digest = ContentDigest::of_bytes(b"package Widget;\n1;\n");
        let revision = Some(ContentRevision::new(src.clone(), digest));
        let env = SourceIdentityEnvelope::for_workspace_file(
            project,
            root,
            src,
            revision,
            SourceGeneration::known("42"),
        );
        assert_eq!(env.schema_version, SourceIdentitySchemaVersion::V1);
        assert!(env.is_schema_supported());
        assert!(env.has_known_generation());
        assert_eq!(env.source_origin, SourceOrigin::Workspace);
        assert_eq!(env.physical_source_role, PhysicalSourceRole::Primary);
        assert!(env.content_revision.is_some());
    }

    #[test]
    fn virtual_source_envelope_has_correct_defaults() {
        let (project, root, src) = make_envelope();
        let env = SourceIdentityEnvelope::for_virtual_source(
            project,
            root,
            src,
            None,
            SourceGeneration::Unknown,
        );
        assert_eq!(env.source_origin, SourceOrigin::Virtual);
        assert_eq!(env.physical_source_role, PhysicalSourceRole::Unavailable);
        assert!(!env.has_known_generation());
        assert!(env.content_revision.is_none());
    }

    #[test]
    fn envelope_without_content_revision_is_valid() {
        let (project, root, src) = make_envelope();
        let env = SourceIdentityEnvelope::for_workspace_file(
            project,
            root,
            src,
            None,
            SourceGeneration::Unknown,
        );
        assert!(env.content_revision.is_none(), "content revision is optional");
    }

    // ── Serde round-trip tests ────────────────────────────────────────────────

    #[test]
    fn envelope_serde_round_trip_with_known_revision() {
        let (project, root, src) = make_envelope();
        let digest = ContentDigest::of_bytes(b"package Widget;\n1;\n");
        let revision = Some(ContentRevision::new(src.clone(), digest));
        let env = SourceIdentityEnvelope::for_workspace_file(
            project,
            root,
            src,
            revision,
            SourceGeneration::known("7"),
        );
        let json = serde_json::to_string(&env).expect("serialization must not fail");
        let back: SourceIdentityEnvelope =
            serde_json::from_str(&json).expect("deserialization must not fail");
        assert_eq!(env, back, "serde round-trip must be lossless");
    }

    #[test]
    fn envelope_serde_round_trip_without_revision() {
        let (project, root, src) = make_envelope();
        let env = SourceIdentityEnvelope::for_virtual_source(
            project,
            root,
            src,
            None,
            SourceGeneration::Unknown,
        );
        let json = serde_json::to_string(&env).unwrap();
        let back: SourceIdentityEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(env, back);
    }

    #[test]
    fn envelope_schema_version_is_explicit_in_json() {
        let (project, root, src) = make_envelope();
        let env = SourceIdentityEnvelope::for_workspace_file(
            project,
            root,
            src,
            None,
            SourceGeneration::Unknown,
        );
        let json = serde_json::to_string(&env).unwrap();
        assert!(
            json.contains("\"schema_version\""),
            "schema_version must appear explicitly in JSON; got: {json}"
        );
    }

    // ── Logical source stability tests ────────────────────────────────────────

    #[test]
    fn logical_source_id_stable_across_envelope_revisions() {
        let (project, root, src) = make_envelope();
        let src2 = src.clone();
        let rev1 = Some(ContentRevision::new(src.clone(), ContentDigest::of_bytes(b"version 1")));
        let rev2 = Some(ContentRevision::new(src2.clone(), ContentDigest::of_bytes(b"version 2")));

        let env1 = SourceIdentityEnvelope::for_workspace_file(
            project.clone(),
            root.clone(),
            src.clone(),
            rev1,
            SourceGeneration::known("1"),
        );
        let env2 = SourceIdentityEnvelope::for_workspace_file(
            project,
            root,
            src2,
            rev2,
            SourceGeneration::known("2"),
        );

        assert_eq!(
            env1.logical_source_id, env2.logical_source_id,
            "logical source id must be stable across content revisions"
        );
        assert_ne!(env1.generation, env2.generation, "generation changes between revisions");
        assert_ne!(env1.content_revision, env2.content_revision, "content revision changes");
    }

    // ── Source-identical later generation test ────────────────────────────────

    #[test]
    fn same_content_with_later_generation() {
        let (project, root, src) = make_envelope();
        let bytes = b"# unchanged source";
        let digest = ContentDigest::of_bytes(bytes);

        let rev1 = Some(ContentRevision::new(src.clone(), digest.clone()));
        let rev2 = Some(ContentRevision::new(src.clone(), digest));

        let env1 = SourceIdentityEnvelope::for_workspace_file(
            project.clone(),
            root.clone(),
            src.clone(),
            rev1,
            SourceGeneration::known("10"),
        );
        let env2 = SourceIdentityEnvelope::for_workspace_file(
            project,
            root,
            src,
            rev2,
            SourceGeneration::known("11"),
        );

        assert_eq!(
            env1.content_revision, env2.content_revision,
            "same bytes → same content revision"
        );
        assert_ne!(env1.generation, env2.generation, "generation advances");
    }
}
