//! LSP and local-host constructors for [`WorkspaceSession`].

use perl_source_identity::{
    ContentDigest, LogicalSourceId, ProjectId, RootRelativeLogicalPath, SourceGeneration,
    WorkspaceRootId,
};

use crate::features::policy::FeatureProfile;
use crate::providers::semantic_port::{
    ProviderCancellationState, ProviderQueryDeadline, ProviderReadinessRequirement,
    ProviderReadinessState,
};
use crate::semantic_snapshot::DocumentInstanceId;

use super::model::{
    ConfigurationIdentity, DocumentSource, WorkspaceRootBinding, WorkspaceSession,
    WorkspaceSessionError, require_known_generation,
};

/// Explicit document-source selection. Absence of a selection is inference.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceSelection {
    /// Use the retained/open snapshot and refuse disk/caller fallback.
    OpenRetained,
    /// Use caller-supplied text and refuse disk/open fallback.
    CallerSupplied,
    /// Use the disk observation and refuse impersonating an open buffer.
    DiskObservation,
}

/// Candidate open/retained document facts from #6726/#7075.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenDocumentFacts {
    /// Document instance identity for this open lifetime.
    pub instance: DocumentInstanceId,
    /// Internal document generation.
    pub generation: SourceGeneration,
    /// Exact open-buffer bytes.
    pub text: String,
}

/// Caller-supplied snapshot that must not fall back to disk or cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuppliedDocumentFacts {
    /// Explicit generation identity for the supplied snapshot.
    pub generation: SourceGeneration,
    /// Exact caller-supplied bytes.
    pub text: String,
}

/// Why a disk observation cannot be used.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskObservationError {
    /// No on-disk file exists for the logical source.
    Missing,
    /// The file exists but could not be read.
    Unreadable,
}

/// Offered document sources plus the explicit selection, if any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentSourceOffer {
    /// When `None`, a unique present candidate may be used; two or more is ambiguous.
    pub selection: Option<SourceSelection>,
    /// Whether a document source is required for this session.
    pub document_required: bool,
    /// Open/retained candidate.
    pub open: Option<OpenDocumentFacts>,
    /// Caller-supplied candidate.
    pub supplied: Option<SuppliedDocumentFacts>,
    /// Disk candidate or a typed read failure.
    pub disk: Result<DiskObservationFacts, DiskObservationError>,
    /// Expected open instance; mismatch is a replaced-instance error.
    pub expected_instance: Option<DocumentInstanceId>,
    /// Expected open generation; mismatch is a stale-generation error.
    pub expected_generation: Option<SourceGeneration>,
}

/// Successful disk observation: digest identity plus metadata, not an open buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskObservationFacts {
    /// Observation generation (watcher/read generation, not an open instance).
    pub generation: SourceGeneration,
    /// Exact disk bytes.
    pub text: String,
    /// Metadata identity such as length plus mtime token; must be non-empty.
    pub metadata_identity: String,
}

/// Already-projected LSP lifecycle facts. Construction does not touch `LspServer`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspSessionFacts {
    /// Canonical project identity already resolved by the LSP host.
    pub project: ProjectId,
    /// Reviewed root binding, or unsupported multi-root.
    pub roots: Result<WorkspaceRootBinding, UnsupportedRoots>,
    /// Current root generation.
    pub root_generation: SourceGeneration,
    /// Optional expected root generation from the caller bind.
    pub expected_root_generation: Option<SourceGeneration>,
    /// Current configuration generation.
    pub config_generation: SourceGeneration,
    /// Optional expected configuration generation from the caller bind.
    pub expected_config_generation: Option<SourceGeneration>,
    /// Feature profile resolved with that configuration.
    pub feature_profile: FeatureProfile,
    /// Workspace generation.
    pub workspace_generation: SourceGeneration,
    /// Index generation.
    pub index_generation: SourceGeneration,
    /// Readiness requirement for the intended query.
    pub readiness_requirement: ProviderReadinessRequirement,
    /// Observed readiness.
    pub readiness_state: ProviderReadinessState,
    /// Deadline snapshot.
    pub deadline: ProviderQueryDeadline,
    /// Cancellation snapshot.
    pub cancellation: ProviderCancellationState,
    /// Root-relative logical path for a document-scoped session.
    pub logical_path: Option<String>,
    /// Offered document sources.
    pub document: DocumentSourceOffer,
}

/// Local fixed-root host facts, for MCP-style constructors that are not an LSP client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalHostFacts {
    /// Authority-defined project name.
    pub project_name: String,
    /// Authority-defined root key.
    pub root_key: String,
    /// Current root generation.
    pub root_generation: SourceGeneration,
    /// Optional expected root generation from the caller bind.
    pub expected_root_generation: Option<SourceGeneration>,
    /// Current configuration generation.
    pub config_generation: SourceGeneration,
    /// Optional expected configuration generation from the caller bind.
    pub expected_config_generation: Option<SourceGeneration>,
    /// Feature profile resolved with that configuration.
    pub feature_profile: FeatureProfile,
    /// Workspace generation.
    pub workspace_generation: SourceGeneration,
    /// Index generation.
    pub index_generation: SourceGeneration,
    /// Readiness requirement for the intended query.
    pub readiness_requirement: ProviderReadinessRequirement,
    /// Observed readiness.
    pub readiness_state: ProviderReadinessState,
    /// Deadline snapshot.
    pub deadline: ProviderQueryDeadline,
    /// Cancellation snapshot.
    pub cancellation: ProviderCancellationState,
    /// Root-relative logical path for a document-scoped session.
    pub logical_path: Option<String>,
    /// Offered document sources.
    pub document: DocumentSourceOffer,
}

/// Explicit unsupported multi-root marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnsupportedRoots;

impl WorkspaceSession {
    /// Project current LSP-owned facts into a session without mutating them.
    ///
    /// # Errors
    ///
    /// Returns a typed construction error for stale, ambiguous, missing,
    /// unreadable, escaped, or unsupported-multi-root inputs.
    pub fn from_lsp_facts(facts: &LspSessionFacts) -> Result<Self, WorkspaceSessionError> {
        from_shared(SharedFacts::from_lsp(facts)?)
    }

    /// Construct the same semantic session from a fixed-root local host.
    ///
    /// This does not fabricate an LSP initialize handshake or import MCP types.
    ///
    /// # Errors
    ///
    /// Returns a typed construction error for stale, ambiguous, missing,
    /// unreadable, escaped, or malformed identity inputs.
    pub fn from_local_host(facts: &LocalHostFacts) -> Result<Self, WorkspaceSessionError> {
        from_shared(SharedFacts::from_local(facts)?)
    }
}

struct SharedFacts {
    project: ProjectId,
    roots: WorkspaceRootBinding,
    root_generation: SourceGeneration,
    expected_root_generation: Option<SourceGeneration>,
    config_generation: SourceGeneration,
    expected_config_generation: Option<SourceGeneration>,
    feature_profile: FeatureProfile,
    workspace_generation: SourceGeneration,
    index_generation: SourceGeneration,
    readiness_requirement: ProviderReadinessRequirement,
    readiness_state: ProviderReadinessState,
    deadline: ProviderQueryDeadline,
    cancellation: ProviderCancellationState,
    logical_path: Option<String>,
    document: DocumentSourceOffer,
}

impl SharedFacts {
    fn from_lsp(facts: &LspSessionFacts) -> Result<Self, WorkspaceSessionError> {
        let roots = facts.roots.clone().map_err(|_| WorkspaceSessionError::UnsupportedMultiRoot)?;
        Ok(Self {
            project: facts.project.clone(),
            roots,
            root_generation: facts.root_generation.clone(),
            expected_root_generation: facts.expected_root_generation.clone(),
            config_generation: facts.config_generation.clone(),
            expected_config_generation: facts.expected_config_generation.clone(),
            feature_profile: facts.feature_profile,
            workspace_generation: facts.workspace_generation.clone(),
            index_generation: facts.index_generation.clone(),
            readiness_requirement: facts.readiness_requirement,
            readiness_state: facts.readiness_state,
            deadline: facts.deadline,
            cancellation: facts.cancellation,
            logical_path: facts.logical_path.clone(),
            document: facts.document.clone(),
        })
    }

    fn from_local(facts: &LocalHostFacts) -> Result<Self, WorkspaceSessionError> {
        if facts.project_name.trim().is_empty() || facts.root_key.trim().is_empty() {
            return Err(WorkspaceSessionError::MalformedIdentity);
        }
        let project = ProjectId::try_from_canonical_name(&facts.project_name)
            .map_err(|_| WorkspaceSessionError::MalformedIdentity)?;
        let root = WorkspaceRootId::try_from_project_and_root_key(&project, &facts.root_key)
            .map_err(|_| WorkspaceSessionError::MalformedIdentity)?;
        Ok(Self {
            project,
            roots: WorkspaceRootBinding::Single { root },
            root_generation: facts.root_generation.clone(),
            expected_root_generation: facts.expected_root_generation.clone(),
            config_generation: facts.config_generation.clone(),
            expected_config_generation: facts.expected_config_generation.clone(),
            feature_profile: facts.feature_profile,
            workspace_generation: facts.workspace_generation.clone(),
            index_generation: facts.index_generation.clone(),
            readiness_requirement: facts.readiness_requirement,
            readiness_state: facts.readiness_state,
            deadline: facts.deadline,
            cancellation: facts.cancellation,
            logical_path: facts.logical_path.clone(),
            document: facts.document.clone(),
        })
    }
}

fn from_shared(facts: SharedFacts) -> Result<WorkspaceSession, WorkspaceSessionError> {
    require_known_generation(&facts.root_generation)?;
    require_known_generation(&facts.config_generation)?;
    if let Some(expected) = facts.expected_root_generation.as_ref()
        && expected != &facts.root_generation
    {
        return Err(WorkspaceSessionError::StaleRootGeneration {
            expected: expected.clone(),
            observed: facts.root_generation.clone(),
        });
    }
    if let Some(expected) = facts.expected_config_generation.as_ref()
        && expected != &facts.config_generation
    {
        return Err(WorkspaceSessionError::StaleConfigGeneration {
            expected: expected.clone(),
            observed: facts.config_generation.clone(),
        });
    }

    let logical_source = match facts.logical_path.as_deref() {
        Some(path) => Some(parse_contained_source(facts.roots.canonical_root(), path)?),
        None if facts.document.document_required => {
            return Err(WorkspaceSessionError::MissingDocument);
        }
        None => None,
    };
    let document = resolve_document_source(logical_source.as_ref(), &facts.document)?;
    if facts.document.document_required && document.is_none() {
        return Err(WorkspaceSessionError::MissingDocument);
    }
    let config = ConfigurationIdentity::new(facts.config_generation, facts.feature_profile)?;
    WorkspaceSession::from_canonical_parts(
        facts.project,
        facts.roots,
        facts.root_generation,
        config,
        document,
        facts.workspace_generation,
        facts.index_generation,
        facts.readiness_requirement,
        facts.readiness_state,
        facts.deadline,
        facts.cancellation,
    )
}

fn parse_contained_source(
    root: &WorkspaceRootId,
    path: &str,
) -> Result<LogicalSourceId, WorkspaceSessionError> {
    let logical_path = RootRelativeLogicalPath::parse(path)
        .map_err(|reason| WorkspaceSessionError::EscapedRoot { reason })?;
    LogicalSourceId::try_from_root_and_logical_path(root, &logical_path)
        .map_err(|_| WorkspaceSessionError::MalformedIdentity)
}

fn resolve_document_source(
    logical_source: Option<&LogicalSourceId>,
    offer: &DocumentSourceOffer,
) -> Result<Option<DocumentSource>, WorkspaceSessionError> {
    let present = present_kinds(offer);
    let selected = match offer.selection {
        Some(selection) => selection,
        None if present.len() > 1 => return Err(WorkspaceSessionError::AmbiguousDocumentSource),
        None if present.len() == 1 => present[0],
        None => return Ok(None),
    };
    let Some(logical_source) = logical_source else {
        return if offer.document_required {
            Err(WorkspaceSessionError::MissingDocument)
        } else {
            Ok(None)
        };
    };
    match selected {
        SourceSelection::OpenRetained => {
            let open = offer.open.as_ref().ok_or(WorkspaceSessionError::MissingDocument)?;
            bind_open_source(logical_source.clone(), open, offer)
        }
        SourceSelection::CallerSupplied => {
            let Some(supplied) = offer.supplied.as_ref() else {
                return Err(WorkspaceSessionError::CallerSuppliedUnavailable);
            };
            DocumentSource::caller_supplied(
                logical_source.clone(),
                content_digest(supplied.text.as_bytes())?,
                supplied.generation.clone(),
            )
            .map(Some)
        }
        SourceSelection::DiskObservation => match &offer.disk {
            Ok(disk) => DocumentSource::disk_observation(
                logical_source.clone(),
                content_digest(disk.text.as_bytes())?,
                disk.generation.clone(),
                disk.metadata_identity.clone(),
            )
            .map(Some),
            Err(DiskObservationError::Missing) => Err(WorkspaceSessionError::MissingDocument),
            Err(DiskObservationError::Unreadable) => Err(WorkspaceSessionError::UnreadableDocument),
        },
    }
}

fn bind_open_source(
    logical_source: LogicalSourceId,
    open: &OpenDocumentFacts,
    offer: &DocumentSourceOffer,
) -> Result<Option<DocumentSource>, WorkspaceSessionError> {
    if let Some(expected) = offer.expected_instance.as_ref()
        && expected != &open.instance
    {
        return Err(WorkspaceSessionError::ReplacedDocumentInstance {
            expected: expected.clone(),
            observed: open.instance.clone(),
        });
    }
    if let Some(expected) = offer.expected_generation.as_ref()
        && expected != &open.generation
    {
        return Err(WorkspaceSessionError::StaleDocumentGeneration {
            expected: expected.clone(),
            observed: open.generation.clone(),
        });
    }
    DocumentSource::open_retained(
        logical_source,
        content_digest(open.text.as_bytes())?,
        open.generation.clone(),
        open.instance.clone(),
    )
    .map(Some)
}

fn content_digest(bytes: &[u8]) -> Result<ContentDigest, WorkspaceSessionError> {
    ContentDigest::try_of_bytes(bytes).map_err(|_| WorkspaceSessionError::MalformedIdentity)
}

fn present_kinds(offer: &DocumentSourceOffer) -> Vec<SourceSelection> {
    let mut kinds = Vec::new();
    if offer.open.is_some() {
        kinds.push(SourceSelection::OpenRetained);
    }
    if offer.supplied.is_some() {
        kinds.push(SourceSelection::CallerSupplied);
    }
    if offer.disk.is_ok() {
        kinds.push(SourceSelection::DiskObservation);
    }
    kinds
}

impl WorkspaceRootBinding {
    /// Bind one canonical root.
    #[must_use]
    pub fn single(root: WorkspaceRootId) -> Self {
        Self::Single { root }
    }

    /// Bind reviewed multi-root state. Additional roots must not include the canonical root.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceSessionError::MalformedIdentity`] when `additional` is empty
    /// or repeats the canonical root.
    pub fn reviewed_multi_root(
        canonical: WorkspaceRootId,
        additional: Vec<WorkspaceRootId>,
    ) -> Result<Self, WorkspaceSessionError> {
        if additional.is_empty() || additional.iter().any(|root| root == &canonical) {
            return Err(WorkspaceSessionError::MalformedIdentity);
        }
        Ok(Self::ReviewedMultiRoot { canonical, additional })
    }
}
