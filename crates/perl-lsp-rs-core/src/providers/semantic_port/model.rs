use perl_semantic_facts::{
    Confidence, EntityId, FactId, FileId, Provenance, ProviderSurface, SemanticConfidence,
    SemanticFactEnvelope, SemanticFreshness, SemanticProducer, SemanticProvenance,
    SourceGeneration,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use super::ProviderQueryContractError;

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

/// Role one canonical fact plays in a provider query result.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum ProviderQueryFactRole {
    /// Fact selects the target at the request subject but is not returned.
    Selector,
    /// Fact is returned as a semantic value.
    Value,
    /// Fact both selects the target and is returned.
    SelectorValue,
    /// Fact supports a degraded or no-value outcome.
    Supporting,
}

impl ProviderQueryFactRole {
    pub(crate) fn is_selector(self) -> bool {
        matches!(self, Self::Selector | Self::SelectorValue)
    }

    pub(crate) fn is_value(self) -> bool {
        matches!(self, Self::Value | Self::SelectorValue)
    }

    pub(crate) fn is_supporting(self) -> bool {
        self == Self::Supporting
    }
}

/// Request generation to which a fact is bound.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum ProviderFactGenerationScope {
    /// Fact is bound to the request's document generation.
    Document,
    /// Fact is bound to the request's workspace/model generation.
    Workspace,
}

/// One canonical semantic fact with only source-level symbol aliases supplied by an adapter.
///
/// Entity, file, package, scope, and source geometry are always derived from the
/// envelope and cannot be overridden through parallel match keys.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderQueryFact {
    role: ProviderQueryFactRole,
    generation_scope: ProviderFactGenerationScope,
    envelope: SemanticFactEnvelope,
    symbols: Vec<String>,
}

impl ProviderQueryFact {
    /// Construct and validate a query fact.
    pub fn try_new(
        role: ProviderQueryFactRole,
        generation_scope: ProviderFactGenerationScope,
        envelope: SemanticFactEnvelope,
        symbols: impl IntoIterator<Item = String>,
    ) -> Result<Self, ProviderQueryContractError> {
        validate_envelope_structure(&envelope)?;
        let mut symbols: Vec<_> = symbols.into_iter().collect();
        if symbols.iter().any(|symbol| symbol.trim().is_empty()) {
            return Err(ProviderQueryContractError::MalformedSymbolKey);
        }
        symbols.sort();
        symbols.dedup();
        Ok(Self {
            role,
            generation_scope,
            envelope,
            symbols,
        })
    }

    /// Construct a query fact without source-level symbol aliases.
    pub fn from_envelope(
        role: ProviderQueryFactRole,
        generation_scope: ProviderFactGenerationScope,
        envelope: SemanticFactEnvelope,
    ) -> Result<Self, ProviderQueryContractError> {
        Self::try_new(role, generation_scope, envelope, Vec::new())
    }

    /// Fact role.
    #[must_use]
    pub const fn role(&self) -> ProviderQueryFactRole {
        self.role
    }

    /// Generation binding.
    #[must_use]
    pub const fn generation_scope(&self) -> ProviderFactGenerationScope {
        self.generation_scope
    }

    /// Canonical semantic envelope.
    #[must_use]
    pub const fn envelope(&self) -> &SemanticFactEnvelope {
        &self.envelope
    }

    /// Canonical source-level symbol aliases.
    #[must_use]
    pub fn symbols(&self) -> &[String] {
        &self.symbols
    }

    pub(crate) fn matches_subject_directly(&self, subject: &ProviderQuerySubject) -> bool {
        match subject {
            ProviderQuerySubject::Entity(entity_id) => self.envelope.entity_id == Some(*entity_id),
            ProviderQuerySubject::File(file_id) => self.envelope.anchor.file_id == *file_id,
            ProviderQuerySubject::Position {
                file_id,
                byte_offset,
            } => {
                self.envelope.anchor.file_id == *file_id
                    && range_contains(&self.envelope, *byte_offset)
            }
            ProviderQuerySubject::Package(package) => {
                self.envelope.package.as_deref() == Some(package.as_str())
            }
            ProviderQuerySubject::Symbol(symbol) => {
                self.symbols.binary_search(symbol).is_ok()
            }
            ProviderQuerySubject::Workspace => true,
        }
    }

    pub(crate) fn is_generation_current(&self, request: &ProviderQueryRequest) -> bool {
        let expected = match self.generation_scope {
            ProviderFactGenerationScope::Document => &request.context.document_generation,
            ProviderFactGenerationScope::Workspace => &request.context.workspace_generation,
        };
        generation_is_known(expected)
            && generation_is_known(&self.envelope.source_generation)
            && &self.envelope.source_generation == expected
    }
}

/// Exact supported-denominator authority for one request family.
///
/// This is a separate request-bound object rather than a field on generic
/// evidence input. It may authorize only an exact empty result; non-empty exact
/// values are established by their own canonical facts.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderCompletenessGrant {
    capability: ProviderQueryCapability,
    project_identity: ProviderIdentity,
    root_identity: ProviderIdentity,
    document_generation: SourceGeneration,
    workspace_generation: SourceGeneration,
    producers: Vec<SemanticProducer>,
    provenance: SemanticProvenance,
    confidence: SemanticConfidence,
    freshness: SemanticFreshness,
}

impl ProviderCompletenessGrant {
    /// Issue exact-grade completeness authority bound to one request.
    pub fn try_new(
        request: &ProviderQueryRequest,
        producers: impl IntoIterator<Item = SemanticProducer>,
        provenance: SemanticProvenance,
        confidence: SemanticConfidence,
        freshness: SemanticFreshness,
    ) -> Result<Self, ProviderQueryContractError> {
        if !request.is_well_formed() || !request.context.is_exact_ready() {
            return Err(ProviderQueryContractError::InvalidCompletenessGrant);
        }
        let mut producers: Vec<_> = producers.into_iter().collect();
        producers.sort();
        producers.dedup();
        if producers.is_empty()
            || producers.contains(&SemanticProducer::Unknown)
            || !semantic_provenance_is_exact(provenance)
            || confidence != SemanticConfidence::Known(Confidence::High)
            || freshness != SemanticFreshness::Fresh
        {
            return Err(ProviderQueryContractError::InvalidCompletenessGrant);
        }
        Ok(Self {
            capability: ProviderQueryCapability::from_query(&request.kind),
            project_identity: request.context.project_identity.clone(),
            root_identity: request.context.root_identity.clone(),
            document_generation: request.context.document_generation.clone(),
            workspace_generation: request.context.workspace_generation.clone(),
            producers,
            provenance,
            confidence,
            freshness,
        })
    }

    pub(crate) fn matches(&self, request: &ProviderQueryRequest) -> bool {
        self.capability == ProviderQueryCapability::from_query(&request.kind)
            && self.project_identity == request.context.project_identity
            && self.root_identity == request.context.root_identity
            && self.document_generation == request.context.document_generation
            && self.workspace_generation == request.context.workspace_generation
            && !self.producers.is_empty()
            && semantic_provenance_is_exact(self.provenance)
            && self.confidence == SemanticConfidence::Known(Confidence::High)
            && self.freshness == SemanticFreshness::Fresh
    }

    pub(crate) fn producers(&self) -> &[SemanticProducer] {
        &self.producers
    }

    pub(crate) const fn provenance(&self) -> SemanticProvenance {
        self.provenance
    }

    pub(crate) const fn confidence(&self) -> SemanticConfidence {
        self.confidence
    }

    pub(crate) const fn freshness(&self) -> SemanticFreshness {
        self.freshness
    }
}

pub(crate) fn generation_is_known(generation: &SourceGeneration) -> bool {
    matches!(generation, SourceGeneration::Known(value) if !value.trim().is_empty())
}

fn generation_is_well_formed(generation: &SourceGeneration) -> bool {
    !matches!(generation, SourceGeneration::Known(value) if value.trim().is_empty())
}

pub(crate) fn semantic_provenance_is_exact(provenance: SemanticProvenance) -> bool {
    matches!(
        provenance,
        SemanticProvenance::Known(
            Provenance::ExactAst
                | Provenance::DesugaredAst
                | Provenance::SemanticAnalyzer
                | Provenance::LiteralRequireImport
        )
    )
}

fn range_contains(envelope: &SemanticFactEnvelope, byte_offset: u32) -> bool {
    let anchor = &envelope.anchor;
    if anchor.start_byte == anchor.end_byte {
        byte_offset == anchor.start_byte
    } else {
        anchor.start_byte <= byte_offset && byte_offset < anchor.end_byte
    }
}

fn validate_envelope_structure(
    envelope: &SemanticFactEnvelope,
) -> Result<(), ProviderQueryContractError> {
    if envelope.anchor.start_byte > envelope.anchor.end_byte
        || matches!(&envelope.source_generation, SourceGeneration::Known(value) if value.trim().is_empty())
        || envelope.package.as_ref().is_some_and(|package| package.trim().is_empty())
        || envelope.producer == SemanticProducer::Unknown
    {
        return Err(ProviderQueryContractError::MalformedFact(envelope.fact_id));
    }

    let mut dependency_keys = BTreeSet::new();
    for dependency in envelope.invalidation_dependencies() {
        if dependency.dependency_key.trim().is_empty()
            || matches!(&dependency.generation, SourceGeneration::Known(value) if value.trim().is_empty())
            || !dependency_keys.insert(dependency.dependency_key.as_str())
        {
            return Err(ProviderQueryContractError::MalformedFact(envelope.fact_id));
        }
    }
    Ok(())
}

pub(crate) fn facts_are_related(left: &ProviderQueryFact, right: &ProviderQueryFact) -> bool {
    left.envelope.entity_id.is_some()
        && left.envelope.entity_id == right.envelope.entity_id
        || left.envelope.package.is_some()
            && left.envelope.package == right.envelope.package
        || left.envelope.scope_id.is_some()
            && left.envelope.scope_id == right.envelope.scope_id
        || left
            .envelope
            .boundary
            .as_ref()
            .and_then(|boundary| boundary.boundary_id)
            == Some(right.envelope.fact_id)
        || right
            .envelope
            .boundary
            .as_ref()
            .and_then(|boundary| boundary.boundary_id)
            == Some(left.envelope.fact_id)
}
