/// Opaque project or workspace-root identity used by provider queries.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ProviderIdentity {
    /// Stable non-empty identity supplied by the current project model.
    Known(String),
    /// Identity is unavailable and must not be inferred from another field.
    Unknown,
}

impl ProviderIdentity {
    /// Construct a known identity.
    #[must_use]
    pub fn known(value: impl Into<String>) -> Self {
        Self::Known(value.into())
    }

    /// Whether this carries a non-empty identity.
    #[must_use]
    pub fn is_known(&self) -> bool {
        matches!(self, Self::Known(value) if !value.trim().is_empty())
    }

    pub(crate) fn is_malformed(&self) -> bool {
        matches!(self, Self::Known(value) if value.trim().is_empty())
    }
}

/// Scope of readiness a provider query requires.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ProviderReadinessRequirement {
    /// Only the current accepted document snapshot is required.
    ActiveDocument,
    /// The current document and its dependency neighborhood are required.
    DependencyNeighborhood,
    /// A current whole-workspace view is required.
    WholeWorkspace,
    /// A future guarded edit plan is required.
    EditAuthorizing,
}

/// Readiness state observed when the query is admitted.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ProviderReadinessState {
    /// Required facts are current and available.
    Ready,
    /// A useful bounded subset is current.
    ReadyLimited,
    /// Required facts are still being built.
    Building,
    /// A prior snapshot exists but is stale.
    Stale,
    /// Required facts are unavailable.
    Unavailable,
    /// Fact production failed.
    Failed,
}

/// Serializable deadline snapshot captured at query admission.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ProviderQueryDeadline {
    /// The caller supplied no deadline.
    None,
    /// Milliseconds remaining when the context was captured.
    RemainingMillis(u64),
    /// The deadline had already expired.
    Expired,
}

/// Serializable cancellation snapshot captured at query admission.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ProviderCancellationState {
    /// The request was active at admission.
    Active,
    /// Cancellation was already requested.
    Cancelled,
}

/// Live control available while a provider query is executing.
pub trait ProviderQueryControl: Send + Sync {
    /// Whether cancellation is currently requested.
    fn is_cancelled(&self) -> bool;

    /// Whether the live deadline has expired.
    fn deadline_expired(&self) -> bool;
}

/// Live control that never cancels and never expires.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopProviderQueryControl;

impl ProviderQueryControl for NoopProviderQueryControl {
    fn is_cancelled(&self) -> bool {
        false
    }

    fn deadline_expired(&self) -> bool {
        false
    }
}

/// Context shared by all semantic provider queries.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderQueryContext {
    /// Project identity for the requested semantic view.
    pub project_identity: ProviderIdentity,
    /// Workspace-root identity selected for the request.
    pub root_identity: ProviderIdentity,
    /// Current source/document generation.
    pub document_generation: SourceGeneration,
    /// Current workspace/model generation.
    pub workspace_generation: SourceGeneration,
    /// Readiness scope required by the provider.
    pub readiness_requirement: ProviderReadinessRequirement,
    /// Readiness state observed at admission.
    pub readiness_state: ProviderReadinessState,
    /// Deadline snapshot captured at admission.
    pub deadline: ProviderQueryDeadline,
    /// Cancellation snapshot captured at admission.
    pub cancellation: ProviderCancellationState,
}

impl ProviderQueryContext {
    /// Construct a query context.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        project_identity: ProviderIdentity,
        root_identity: ProviderIdentity,
        document_generation: SourceGeneration,
        workspace_generation: SourceGeneration,
        readiness_requirement: ProviderReadinessRequirement,
        readiness_state: ProviderReadinessState,
        deadline: ProviderQueryDeadline,
        cancellation: ProviderCancellationState,
    ) -> Self {
        Self {
            project_identity,
            root_identity,
            document_generation,
            workspace_generation,
            readiness_requirement,
            readiness_state,
            deadline,
            cancellation,
        }
    }

    pub(crate) fn is_well_formed(&self) -> bool {
        !self.project_identity.is_malformed()
            && !self.root_identity.is_malformed()
            && generation_is_well_formed(&self.document_generation)
            && generation_is_well_formed(&self.workspace_generation)
    }

    pub(crate) fn controls_are_active(&self) -> bool {
        self.cancellation == ProviderCancellationState::Active
            && self.deadline != ProviderQueryDeadline::Expired
    }

    pub(crate) fn has_bound_generations(&self) -> bool {
        generation_is_known(&self.document_generation)
            && generation_is_known(&self.workspace_generation)
    }

    pub(crate) fn is_exact_ready(&self) -> bool {
        self.project_identity.is_known()
            && self.root_identity.is_known()
            && self.has_bound_generations()
            && self.readiness_state == ProviderReadinessState::Ready
            && self.readiness_requirement != ProviderReadinessRequirement::EditAuthorizing
            && self.controls_are_active()
    }

    pub(crate) fn is_degraded_ready(&self) -> bool {
        self.project_identity.is_known()
            && self.root_identity.is_known()
            && self.has_bound_generations()
            && matches!(
                self.readiness_state,
                ProviderReadinessState::Ready | ProviderReadinessState::ReadyLimited
            )
            && self.controls_are_active()
    }

    pub(crate) fn is_fallback_ready(&self) -> bool {
        self.project_identity.is_known()
            && self.root_identity.is_known()
            && self.has_bound_generations()
            && matches!(
                self.readiness_state,
                ProviderReadinessState::Ready
                    | ProviderReadinessState::ReadyLimited
                    | ProviderReadinessState::Building
            )
            && self.controls_are_active()
    }
}

/// Semantic query family requested by a provider.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ProviderQueryKind {
    /// Resolve declarations or entities.
    Declaration,
    /// Resolve reference occurrences and their roles.
    References {
        /// Whether declarations should be included with references.
        include_declaration: bool,
    },
    /// Resolve package, module, import, export, or visible-symbol facts.
    Visibility,
    /// Resolve scope, binding, or lexical-storage facts.
    ScopeBindings,
    /// Resolve generated, dynamic, compatibility, or source-locked boundaries.
    Boundaries,
    /// Resolve readiness/freshness state without semantic values.
    Readiness,
}

/// Query family whose supported denominator may be declared complete.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum ProviderQueryCapability {
    /// Declaration and entity facts.
    Declarations,
    /// Reference occurrence facts.
    References,
    /// Module/import/export visibility facts.
    Visibility,
    /// Scope and binding facts.
    ScopeBindings,
    /// Dynamic or compatibility boundaries.
    Boundaries,
    /// Readiness-only state.
    Readiness,
}

impl ProviderQueryCapability {
    /// Map a request kind to its completeness family.
    #[must_use]
    pub fn from_query(kind: &ProviderQueryKind) -> Self {
        match kind {
            ProviderQueryKind::Declaration => Self::Declarations,
            ProviderQueryKind::References { .. } => Self::References,
            ProviderQueryKind::Visibility => Self::Visibility,
            ProviderQueryKind::ScopeBindings => Self::ScopeBindings,
            ProviderQueryKind::Boundaries => Self::Boundaries,
            ProviderQueryKind::Readiness => Self::Readiness,
        }
    }
}

/// Subject selected for a provider query.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ProviderQuerySubject {
    /// Query by canonical entity identity.
    Entity(EntityId),
    /// Query all relevant facts for one file.
    File(FileId),
    /// Query at one byte position in a file.
    Position {
        /// File containing the position.
        file_id: FileId,
        /// UTF-8 byte offset in the accepted source generation.
        byte_offset: u32,
    },
    /// Query by package or module name.
    Package(String),
    /// Query by source-level symbol spelling.
    Symbol(String),
    /// Query workspace-wide facts.
    Workspace,
}

impl ProviderQuerySubject {
    pub(crate) fn is_well_formed(&self) -> bool {
        match self {
            Self::Package(value) | Self::Symbol(value) => !value.trim().is_empty(),
            _ => true,
        }
    }
}

/// One transport-neutral provider query.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderQueryRequest {
    /// Provider surface making the query.
    pub surface: ProviderSurface,
    /// Stable request-class identifier such as `textDocument/definition`.
    pub request_class: String,
    /// Semantic query family.
    pub kind: ProviderQueryKind,
    /// Subject of the query.
    pub subject: ProviderQuerySubject,
    /// Project, generation, readiness, deadline, and cancellation context.
    pub context: ProviderQueryContext,
}

impl ProviderQueryRequest {
    /// Construct a provider query request.
    #[must_use]
    pub fn new(
        surface: ProviderSurface,
        request_class: impl Into<String>,
        kind: ProviderQueryKind,
        subject: ProviderQuerySubject,
        context: ProviderQueryContext,
    ) -> Self {
        Self {
            surface,
            request_class: request_class.into(),
            kind,
            subject,
            context,
        }
    }

    /// Whether the request contains no malformed explicit identities.
    #[must_use]
    pub fn is_well_formed(&self) -> bool {
        !self.request_class.trim().is_empty()
            && self.subject.is_well_formed()
            && self.context.is_well_formed()
    }
}
