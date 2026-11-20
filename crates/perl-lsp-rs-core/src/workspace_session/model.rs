//! Session, document-source, and identity types for #7218.

use std::fmt::{self, Write as _};

use perl_source_identity::{
    ContentDigest, LogicalSourceId, ProjectId, SourceGeneration, WorkspaceRootId,
};
use sha2::{Digest, Sha256};

use crate::features::policy::FeatureProfile;
use crate::providers::semantic_port::{
    ProviderCancellationState, ProviderIdentity, ProviderQueryContext, ProviderQueryDeadline,
    ProviderReadinessRequirement, ProviderReadinessState,
};
use crate::semantic_snapshot::DocumentInstanceId;

const SESSION_IDENTITY_DOMAIN: &[u8] = b"perl-lsp:workspace-session-id:v1\0";
const OPERATION_IDENTITY_DOMAIN: &[u8] = b"perl-lsp:workspace-session-operation-id:v1\0";

/// Deterministic session identity minted from canonical session facts.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SessionIdentity(String);

impl SessionIdentity {
    pub(super) fn from_parts(parts: &[&[u8]]) -> Self {
        Self(format!("session:sha256:{}", domain_digest(SESSION_IDENTITY_DOMAIN, parts)))
    }

    /// Wire form `session:sha256:<64 lowercase hex digits>`.
    #[must_use]
    pub fn as_wire(&self) -> &str {
        &self.0
    }
}

/// Deterministic operation identity: session identity plus live controls.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OperationIdentity(String);

impl OperationIdentity {
    pub(super) fn from_session_and_controls(
        session: &SessionIdentity,
        deadline: ProviderQueryDeadline,
        cancellation: ProviderCancellationState,
    ) -> Self {
        Self(format!(
            "operation:sha256:{}",
            domain_digest(
                OPERATION_IDENTITY_DOMAIN,
                &[
                    session.as_wire().as_bytes(),
                    deadline_tag(deadline).as_bytes(),
                    cancellation_tag(cancellation).as_bytes(),
                ],
            )
        ))
    }

    /// Wire form `operation:sha256:<64 lowercase hex digits>`.
    #[must_use]
    pub fn as_wire(&self) -> &str {
        &self.0
    }
}

/// Kind of document source authority bound into a session.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DocumentSourceKind {
    /// Open or retained editor buffer for one document instance and generation.
    OpenRetained,
    /// Caller-supplied snapshot that cannot fall back to disk or cache.
    CallerSupplied,
    /// Disk observation identified by digest and metadata, not an open buffer.
    DiskObservation,
}

impl DocumentSourceKind {
    fn tag(self) -> &'static str {
        match self {
            Self::OpenRetained => "open-retained",
            Self::CallerSupplied => "caller-supplied",
            Self::DiskObservation => "disk-observation",
        }
    }
}

/// Exact document source bound into a valid session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentSource {
    kind: DocumentSourceKind,
    logical_source: LogicalSourceId,
    digest: ContentDigest,
    generation: SourceGeneration,
    document_instance: Option<DocumentInstanceId>,
    disk_metadata_identity: Option<String>,
}

impl DocumentSource {
    pub(super) fn open_retained(
        logical_source: LogicalSourceId,
        digest: ContentDigest,
        generation: SourceGeneration,
        document_instance: DocumentInstanceId,
    ) -> Result<Self, WorkspaceSessionError> {
        require_known_generation(&generation)?;
        Ok(Self {
            kind: DocumentSourceKind::OpenRetained,
            logical_source,
            digest,
            generation,
            document_instance: Some(document_instance),
            disk_metadata_identity: None,
        })
    }

    pub(super) fn caller_supplied(
        logical_source: LogicalSourceId,
        digest: ContentDigest,
        generation: SourceGeneration,
    ) -> Result<Self, WorkspaceSessionError> {
        require_known_generation(&generation)?;
        Ok(Self {
            kind: DocumentSourceKind::CallerSupplied,
            logical_source,
            digest,
            generation,
            document_instance: None,
            disk_metadata_identity: None,
        })
    }

    pub(super) fn disk_observation(
        logical_source: LogicalSourceId,
        digest: ContentDigest,
        generation: SourceGeneration,
        metadata_identity: String,
    ) -> Result<Self, WorkspaceSessionError> {
        require_known_generation(&generation)?;
        if metadata_identity.trim().is_empty() {
            return Err(WorkspaceSessionError::MalformedIdentity);
        }
        Ok(Self {
            kind: DocumentSourceKind::DiskObservation,
            logical_source,
            digest,
            generation,
            document_instance: None,
            disk_metadata_identity: Some(metadata_identity),
        })
    }

    /// Source kind; never inferred from digest equality.
    #[must_use]
    pub const fn kind(&self) -> DocumentSourceKind {
        self.kind
    }

    /// Logical source bound to this document.
    #[must_use]
    pub const fn logical_source(&self) -> &LogicalSourceId {
        &self.logical_source
    }

    /// Exact content digest of the selected source.
    #[must_use]
    pub const fn digest(&self) -> &ContentDigest {
        &self.digest
    }

    /// Document or observation generation of the selected source.
    #[must_use]
    pub const fn generation(&self) -> &SourceGeneration {
        &self.generation
    }

    /// Open-document instance, present only for retained/open snapshots.
    #[must_use]
    pub const fn document_instance(&self) -> Option<&DocumentInstanceId> {
        self.document_instance.as_ref()
    }

    /// Disk metadata identity, present only for disk observations.
    #[must_use]
    pub fn disk_metadata_identity(&self) -> Option<&str> {
        self.disk_metadata_identity.as_deref()
    }
}

/// Fixed canonical root, or an explicitly reviewed multi-root binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceRootBinding {
    /// One canonical workspace root.
    Single {
        /// The only admitted root.
        root: WorkspaceRootId,
    },
    /// Reviewed multi-root state with one canonical root used for queries.
    ReviewedMultiRoot {
        /// Canonical root consumed by #6816 query context.
        canonical: WorkspaceRootId,
        /// Additional reviewed roots; they do not impersonate the canonical root.
        additional: Vec<WorkspaceRootId>,
    },
}

impl WorkspaceRootBinding {
    pub(super) fn canonical_root(&self) -> &WorkspaceRootId {
        match self {
            Self::Single { root } => root,
            Self::ReviewedMultiRoot { canonical, .. } => canonical,
        }
    }

    fn identity_parts(&self) -> Vec<u8> {
        match self {
            Self::Single { root } => {
                let mut out = Vec::from(&b"single\0"[..]);
                out.extend_from_slice(root.as_wire().as_bytes());
                out
            }
            Self::ReviewedMultiRoot { canonical, additional } => {
                let mut out = Vec::from(&b"reviewed-multi\0"[..]);
                out.extend_from_slice(canonical.as_wire().as_bytes());
                for root in additional {
                    out.push(0);
                    out.extend_from_slice(root.as_wire().as_bytes());
                }
                out
            }
        }
    }
}

/// Resolved configuration generation plus the feature profile it selected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigurationIdentity {
    generation: SourceGeneration,
    profile: FeatureProfile,
}

impl ConfigurationIdentity {
    pub(super) fn new(
        generation: SourceGeneration,
        profile: FeatureProfile,
    ) -> Result<Self, WorkspaceSessionError> {
        require_known_generation(&generation)?;
        Ok(Self { generation, profile })
    }

    /// Configuration generation identity.
    #[must_use]
    pub const fn generation(&self) -> &SourceGeneration {
        &self.generation
    }

    /// Feature profile resolved with this configuration generation.
    #[must_use]
    pub const fn profile(&self) -> FeatureProfile {
        self.profile
    }
}

/// Transport-neutral workspace session projected from canonical facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceSession {
    session_id: SessionIdentity,
    operation_id: OperationIdentity,
    project: ProjectId,
    roots: WorkspaceRootBinding,
    root_generation: SourceGeneration,
    config: ConfigurationIdentity,
    document: Option<DocumentSource>,
    workspace_generation: SourceGeneration,
    index_generation: SourceGeneration,
    readiness_requirement: ProviderReadinessRequirement,
    readiness_state: ProviderReadinessState,
    deadline: ProviderQueryDeadline,
    cancellation: ProviderCancellationState,
}

impl WorkspaceSession {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn from_canonical_parts(
        project: ProjectId,
        roots: WorkspaceRootBinding,
        root_generation: SourceGeneration,
        config: ConfigurationIdentity,
        document: Option<DocumentSource>,
        workspace_generation: SourceGeneration,
        index_generation: SourceGeneration,
        readiness_requirement: ProviderReadinessRequirement,
        readiness_state: ProviderReadinessState,
        deadline: ProviderQueryDeadline,
        cancellation: ProviderCancellationState,
    ) -> Result<Self, WorkspaceSessionError> {
        require_known_generation(&root_generation)?;
        require_known_generation(&workspace_generation)?;
        require_known_generation(&index_generation)?;
        let session_id = mint_session_identity(
            &project,
            &roots,
            &root_generation,
            &config,
            document.as_ref(),
            &workspace_generation,
            &index_generation,
            readiness_requirement,
            readiness_state,
        );
        let operation_id =
            OperationIdentity::from_session_and_controls(&session_id, deadline, cancellation);
        Ok(Self {
            session_id,
            operation_id,
            project,
            roots,
            root_generation,
            config,
            document,
            workspace_generation,
            index_generation,
            readiness_requirement,
            readiness_state,
            deadline,
            cancellation,
        })
    }

    /// Session identity for these canonical facts.
    #[must_use]
    pub const fn session_id(&self) -> &SessionIdentity {
        &self.session_id
    }

    /// Operation identity, including cancellation and deadline.
    #[must_use]
    pub const fn operation_id(&self) -> &OperationIdentity {
        &self.operation_id
    }

    /// Project identity.
    #[must_use]
    pub const fn project(&self) -> &ProjectId {
        &self.project
    }

    /// Canonical root and reviewed multi-root state.
    #[must_use]
    pub const fn roots(&self) -> &WorkspaceRootBinding {
        &self.roots
    }

    /// Root generation bound into this session.
    #[must_use]
    pub const fn root_generation(&self) -> &SourceGeneration {
        &self.root_generation
    }

    /// Resolved configuration identity and feature profile.
    #[must_use]
    pub const fn configuration(&self) -> &ConfigurationIdentity {
        &self.config
    }

    /// Bound document source, when the session is document-scoped.
    #[must_use]
    pub const fn document_source(&self) -> Option<&DocumentSource> {
        self.document.as_ref()
    }

    /// Workspace generation.
    #[must_use]
    pub const fn workspace_generation(&self) -> &SourceGeneration {
        &self.workspace_generation
    }

    /// Index generation.
    #[must_use]
    pub const fn index_generation(&self) -> &SourceGeneration {
        &self.index_generation
    }

    /// Readiness scope required by consumers of this session.
    #[must_use]
    pub const fn readiness_requirement(&self) -> ProviderReadinessRequirement {
        self.readiness_requirement
    }

    /// Readiness observed when the session was projected.
    #[must_use]
    pub const fn readiness_state(&self) -> ProviderReadinessState {
        self.readiness_state
    }

    /// Deadline snapshot captured at construction.
    #[must_use]
    pub const fn deadline(&self) -> ProviderQueryDeadline {
        self.deadline
    }

    /// Cancellation snapshot captured at construction.
    #[must_use]
    pub const fn cancellation(&self) -> ProviderCancellationState {
        self.cancellation
    }

    /// Project the #6816 provider query context from this session.
    ///
    /// Does not define a parallel request model. Document generation uses the
    /// bound document source when present, otherwise the workspace generation.
    #[must_use]
    pub fn provider_query_context(&self) -> ProviderQueryContext {
        let document_generation = self.document.as_ref().map_or_else(
            || fact_generation(&self.workspace_generation),
            |source| fact_generation(source.generation()),
        );
        ProviderQueryContext::new(
            ProviderIdentity::known(self.project.as_wire()),
            ProviderIdentity::known(self.roots.canonical_root().as_wire()),
            document_generation,
            fact_generation(&self.workspace_generation),
            self.readiness_requirement,
            self.readiness_state,
            self.deadline,
            self.cancellation,
        )
    }
}

/// Typed reasons a session cannot be constructed.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceSessionError {
    /// No document source was available where one was required.
    MissingDocument,
    /// The selected disk observation could not be read.
    UnreadableDocument,
    /// More than one document source was offered without an explicit selection.
    AmbiguousDocumentSource,
    /// The logical path is not a contained root-relative path.
    EscapedRoot {
        /// Path-validation reason; does not echo the rejected path.
        reason: perl_source_identity::LogicalPathError,
    },
    /// More than one root was offered without an explicit reviewed binding.
    UnsupportedMultiRoot,
    /// No workspace root was supplied.
    MissingRoot,
    /// Retained open facts do not match the expected document generation.
    StaleDocumentGeneration {
        /// Generation the caller bound.
        expected: SourceGeneration,
        /// Generation retained by the lifecycle authority.
        observed: SourceGeneration,
    },
    /// Retained open facts belong to a different document instance.
    ReplacedDocumentInstance {
        /// Instance the caller bound.
        expected: DocumentInstanceId,
        /// Instance retained by the lifecycle authority.
        observed: DocumentInstanceId,
    },
    /// Root generation moved relative to the expected bind.
    StaleRootGeneration {
        /// Generation the caller bound.
        expected: SourceGeneration,
        /// Current root generation.
        observed: SourceGeneration,
    },
    /// Configuration generation moved relative to the expected bind.
    StaleConfigGeneration {
        /// Generation the caller bound.
        expected: SourceGeneration,
        /// Current configuration generation.
        observed: SourceGeneration,
    },
    /// A required generation was unknown, so the session would be unidentifiable.
    UnknownGeneration,
    /// An identity field was empty or otherwise unusable.
    MalformedIdentity,
    /// Caller-supplied text was selected but not provided; disk/open were not used.
    CallerSuppliedUnavailable,
}

impl fmt::Display for WorkspaceSessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingDocument => {
                formatter.write_str("workspace session is missing a document source")
            }
            Self::UnreadableDocument => {
                formatter.write_str("workspace session disk observation is unreadable")
            }
            Self::AmbiguousDocumentSource => {
                formatter.write_str("workspace session document source is ambiguous")
            }
            Self::EscapedRoot { reason } => {
                write!(formatter, "workspace session path is not contained by the root: {reason}")
            }
            Self::UnsupportedMultiRoot => {
                formatter.write_str("workspace session has unsupported multi-root state")
            }
            Self::MissingRoot => {
                formatter.write_str("workspace session is missing a workspace root")
            }
            Self::StaleDocumentGeneration { .. } => {
                formatter.write_str("workspace session document generation is stale")
            }
            Self::ReplacedDocumentInstance { .. } => {
                formatter.write_str("workspace session document instance was replaced")
            }
            Self::StaleRootGeneration { .. } => {
                formatter.write_str("workspace session root generation is stale")
            }
            Self::StaleConfigGeneration { .. } => {
                formatter.write_str("workspace session configuration generation is stale")
            }
            Self::UnknownGeneration => {
                formatter.write_str("workspace session generation is unknown")
            }
            Self::MalformedIdentity => {
                formatter.write_str("workspace session identity is malformed")
            }
            Self::CallerSuppliedUnavailable => formatter
                .write_str("caller-supplied document source is unavailable and must not fall back"),
        }
    }
}

impl std::error::Error for WorkspaceSessionError {}

pub(super) fn require_known_generation(
    generation: &SourceGeneration,
) -> Result<(), WorkspaceSessionError> {
    if generation.is_known() { Ok(()) } else { Err(WorkspaceSessionError::UnknownGeneration) }
}

pub(super) fn fact_generation(
    generation: &SourceGeneration,
) -> perl_semantic_facts::SourceGeneration {
    generation.as_label().map_or(
        perl_semantic_facts::SourceGeneration::Unknown,
        perl_semantic_facts::SourceGeneration::known,
    )
}

#[allow(clippy::too_many_arguments)]
fn mint_session_identity(
    project: &ProjectId,
    roots: &WorkspaceRootBinding,
    root_generation: &SourceGeneration,
    config: &ConfigurationIdentity,
    document: Option<&DocumentSource>,
    workspace_generation: &SourceGeneration,
    index_generation: &SourceGeneration,
    readiness_requirement: ProviderReadinessRequirement,
    readiness_state: ProviderReadinessState,
) -> SessionIdentity {
    let root_parts = roots.identity_parts();
    let root_gen = generation_tag(root_generation);
    let config_gen = generation_tag(config.generation());
    let workspace_gen = generation_tag(workspace_generation);
    let index_gen = generation_tag(index_generation);
    let doc_parts = document_identity_bytes(document);
    SessionIdentity::from_parts(&[
        project.as_wire().as_bytes(),
        &root_parts,
        root_gen.as_bytes(),
        config_gen.as_bytes(),
        config.profile.as_str().as_bytes(),
        &doc_parts,
        workspace_gen.as_bytes(),
        index_gen.as_bytes(),
        readiness_requirement_tag(readiness_requirement).as_bytes(),
        readiness_state_tag(readiness_state).as_bytes(),
    ])
}

fn document_identity_bytes(document: Option<&DocumentSource>) -> Vec<u8> {
    let Some(document) = document else {
        return Vec::from(&b"none"[..]);
    };
    let mut out = Vec::from(document.kind.tag().as_bytes());
    out.push(0);
    out.extend_from_slice(document.logical_source.as_wire().as_bytes());
    out.push(0);
    out.extend_from_slice(document.digest.as_wire().as_bytes());
    out.push(0);
    out.extend_from_slice(generation_tag(document.generation()).as_bytes());
    out.push(0);
    if let Some(instance) = document.document_instance() {
        out.extend_from_slice(instance.as_wire().as_bytes());
    } else {
        out.extend_from_slice(b"no-instance");
    }
    out.push(0);
    if let Some(metadata) = document.disk_metadata_identity() {
        out.extend_from_slice(metadata.as_bytes());
    } else {
        out.extend_from_slice(b"no-disk-metadata");
    }
    out
}

fn generation_tag(generation: &SourceGeneration) -> String {
    generation.as_label().map_or_else(|| "unknown".to_string(), |label| format!("known:{label}"))
}

fn deadline_tag(deadline: ProviderQueryDeadline) -> String {
    match deadline {
        ProviderQueryDeadline::None => "none".to_string(),
        ProviderQueryDeadline::RemainingMillis(millis) => format!("remaining:{millis}"),
        ProviderQueryDeadline::Expired => "expired".to_string(),
    }
}

fn cancellation_tag(cancellation: ProviderCancellationState) -> &'static str {
    match cancellation {
        ProviderCancellationState::Active => "active",
        ProviderCancellationState::Cancelled => "cancelled",
    }
}

fn readiness_requirement_tag(requirement: ProviderReadinessRequirement) -> &'static str {
    match requirement {
        ProviderReadinessRequirement::ActiveDocument => "active-document",
        ProviderReadinessRequirement::DependencyNeighborhood => "dependency-neighborhood",
        ProviderReadinessRequirement::WholeWorkspace => "whole-workspace",
        ProviderReadinessRequirement::EditAuthorizing => "edit-authorizing",
    }
}

fn readiness_state_tag(state: ProviderReadinessState) -> &'static str {
    match state {
        ProviderReadinessState::Ready => "ready",
        ProviderReadinessState::ReadyLimited => "ready-limited",
        ProviderReadinessState::Building => "building",
        ProviderReadinessState::Stale => "stale",
        ProviderReadinessState::Unavailable => "unavailable",
        ProviderReadinessState::Failed => "failed",
    }
}

fn domain_digest(domain: &[u8], parts: &[&[u8]]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    for part in parts {
        hasher.update(u64::try_from(part.len()).unwrap_or(u64::MAX).to_be_bytes());
        hasher.update(part);
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for byte in digest {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}
