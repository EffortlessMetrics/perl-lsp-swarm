use perl_semantic_facts::{
    Confidence, EntityId, FileId, Provenance, ProviderSurface, SemanticConfidence,
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
        Self { surface, request_class: request_class.into(), kind, subject, context }
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
        Ok(Self { role, generation_scope, envelope, symbols })
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
            ProviderQuerySubject::Position { file_id, byte_offset } => {
                self.envelope.anchor.file_id == *file_id
                    && range_contains(&self.envelope, *byte_offset)
            }
            ProviderQuerySubject::Package(package) => {
                self.envelope.package.as_deref() == Some(package.as_str())
            }
            ProviderQuerySubject::Symbol(symbol) => self.symbols.binary_search(symbol).is_ok(),
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
/// Concrete denominator receipt retained for an exact-empty result.
///
/// The receipt is created only from a verified snapshot inside the semantic-port
/// control plane. Public provider implementations can inspect it but cannot
/// manufacture one from labels or exact-looking enums.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderCompletenessAuthorityReceipt {
    capability: ProviderQueryCapability,
    query_kind: ProviderQueryKind,
    subject: ProviderQuerySubject,
    project_identity: ProviderIdentity,
    root_identity: ProviderIdentity,
    document_generation: SourceGeneration,
    workspace_generation: SourceGeneration,
    producer: SemanticProducer,
    denominator_id: String,
    snapshot_id: String,
    covered_unit_count: u64,
}

impl ProviderCompletenessAuthorityReceipt {
    /// Query family whose supported denominator is complete.
    #[must_use]
    pub const fn capability(&self) -> ProviderQueryCapability {
        self.capability
    }

    /// Project identity bound to the denominator snapshot.
    #[must_use]
    pub const fn project_identity(&self) -> &ProviderIdentity {
        &self.project_identity
    }

    /// Workspace-root identity bound to the denominator snapshot.
    #[must_use]
    pub const fn root_identity(&self) -> &ProviderIdentity {
        &self.root_identity
    }

    /// Document generation bound to the denominator snapshot.
    #[must_use]
    pub const fn document_generation(&self) -> &SourceGeneration {
        &self.document_generation
    }

    /// Workspace/model generation bound to the denominator snapshot.
    #[must_use]
    pub const fn workspace_generation(&self) -> &SourceGeneration {
        &self.workspace_generation
    }

    /// Producer that owns the denominator snapshot.
    #[must_use]
    pub const fn producer(&self) -> SemanticProducer {
        self.producer
    }

    /// Stable denominator identity for the query family.
    #[must_use]
    pub fn denominator_id(&self) -> &str {
        &self.denominator_id
    }

    /// Stable snapshot identity for the authority input.
    #[must_use]
    pub fn snapshot_id(&self) -> &str {
        &self.snapshot_id
    }

    /// Number of concrete units covered by the denominator snapshot.
    #[must_use]
    pub const fn covered_unit_count(&self) -> u64 {
        self.covered_unit_count
    }
}

/// Verified completeness snapshot bound to one request.
///
/// Production issuance flows through the crate-owned provider adapters in
/// [`super::super::semantic_port_adapters`] (#6817), which supply a concrete
/// producer denominator. Public provider implementations still cannot
/// manufacture a grant from labels or exact-looking enums.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VerifiedProviderCompletenessSnapshot {
    authority: ProviderCompletenessAuthorityReceipt,
    provenance: SemanticProvenance,
    confidence: SemanticConfidence,
    freshness: SemanticFreshness,
}

impl VerifiedProviderCompletenessSnapshot {
    /// Validate a concrete denominator snapshot before it can issue a grant.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn try_new(
        request: &ProviderQueryRequest,
        capability: ProviderQueryCapability,
        producer: SemanticProducer,
        denominator_id: impl Into<String>,
        snapshot_id: impl Into<String>,
        covered_unit_count: u64,
        provenance: SemanticProvenance,
        confidence: SemanticConfidence,
        freshness: SemanticFreshness,
    ) -> Result<Self, ProviderQueryContractError> {
        let denominator_id = denominator_id.into();
        let snapshot_id = snapshot_id.into();
        if !request.is_well_formed()
            || !request.context.is_exact_ready()
            || capability != ProviderQueryCapability::from_query(&request.kind)
            || producer == SemanticProducer::Unknown
            || denominator_id.trim().is_empty()
            || snapshot_id.trim().is_empty()
            || covered_unit_count == 0
            || !semantic_provenance_is_exact(provenance)
            || confidence != SemanticConfidence::Known(Confidence::High)
            || freshness != SemanticFreshness::Fresh
        {
            return Err(ProviderQueryContractError::InvalidCompletenessGrant);
        }
        Ok(Self {
            authority: ProviderCompletenessAuthorityReceipt {
                capability,
                query_kind: request.kind.clone(),
                subject: request.subject.clone(),
                project_identity: request.context.project_identity.clone(),
                root_identity: request.context.root_identity.clone(),
                document_generation: request.context.document_generation.clone(),
                workspace_generation: request.context.workspace_generation.clone(),
                producer,
                denominator_id,
                snapshot_id,
                covered_unit_count,
            },
            provenance,
            confidence,
            freshness,
        })
    }
}

/// Exact supported-denominator authority for one request family.
///
/// The type is public so checked results can expose its evidence, but it has no
/// public constructor. Crate-owned adapters (#6817) issue grants through
/// [`ProviderCompletenessGrant::issue_for_request`] from a concrete producer
/// denominator snapshot; external code cannot manufacture one.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderCompletenessGrant {
    authority: ProviderCompletenessAuthorityReceipt,
    provenance: SemanticProvenance,
    confidence: SemanticConfidence,
    freshness: SemanticFreshness,
}

impl ProviderCompletenessGrant {
    /// Issue a request-bound grant from a concrete producer denominator snapshot.
    ///
    /// Crate-internal: only owning adapters can supply the denominator evidence.
    /// The snapshot is validated against the request before a grant exists.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn issue_for_request(
        request: &ProviderQueryRequest,
        producer: SemanticProducer,
        denominator_id: impl Into<String>,
        snapshot_id: impl Into<String>,
        covered_unit_count: u64,
        provenance: SemanticProvenance,
        confidence: SemanticConfidence,
        freshness: SemanticFreshness,
    ) -> Result<Self, ProviderQueryContractError> {
        let snapshot = VerifiedProviderCompletenessSnapshot::try_new(
            request,
            ProviderQueryCapability::from_query(&request.kind),
            producer,
            denominator_id,
            snapshot_id,
            covered_unit_count,
            provenance,
            confidence,
            freshness,
        )?;
        Ok(Self::from_verified_snapshot(snapshot))
    }

    pub(crate) fn from_verified_snapshot(snapshot: VerifiedProviderCompletenessSnapshot) -> Self {
        Self {
            authority: snapshot.authority,
            provenance: snapshot.provenance,
            confidence: snapshot.confidence,
            freshness: snapshot.freshness,
        }
    }

    pub(crate) fn matches(&self, request: &ProviderQueryRequest) -> bool {
        self.authority.capability == ProviderQueryCapability::from_query(&request.kind)
            && self.authority.query_kind == request.kind
            && self.authority.subject == request.subject
            && self.authority.project_identity == request.context.project_identity
            && self.authority.root_identity == request.context.root_identity
            && self.authority.document_generation == request.context.document_generation
            && self.authority.workspace_generation == request.context.workspace_generation
            && self.authority.producer != SemanticProducer::Unknown
            && !self.authority.denominator_id.trim().is_empty()
            && !self.authority.snapshot_id.trim().is_empty()
            && self.authority.covered_unit_count > 0
            && semantic_provenance_is_exact(self.provenance)
            && self.confidence == SemanticConfidence::Known(Confidence::High)
            && self.freshness == SemanticFreshness::Fresh
    }

    pub(crate) const fn authority(&self) -> &ProviderCompletenessAuthorityReceipt {
        &self.authority
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

pub(crate) fn validate_envelope_structure(
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

/// Whether two facts carry a canonical or explicit relation to the same target.
///
/// Package and scope equality intentionally do not establish identity. They may
/// bound a search, but two different entities in one package or lexical scope are
/// still different targets.
pub(crate) fn facts_are_related(left: &ProviderQueryFact, right: &ProviderQueryFact) -> bool {
    left.envelope.entity_id.is_some() && left.envelope.entity_id == right.envelope.entity_id
        || left.envelope.boundary.as_ref().and_then(|boundary| boundary.boundary_id)
            == Some(right.envelope.fact_id)
        || right.envelope.boundary.as_ref().and_then(|boundary| boundary.boundary_id)
            == Some(left.envelope.fact_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exact_ready_request() -> ProviderQueryRequest {
        ProviderQueryRequest::new(
            ProviderSurface::Hover,
            "textDocument/hover",
            ProviderQueryKind::Declaration,
            ProviderQuerySubject::Entity(EntityId(1)),
            ProviderQueryContext::new(
                ProviderIdentity::known("test-project"),
                ProviderIdentity::known("test-root"),
                SourceGeneration::Known("gen-1".into()),
                SourceGeneration::Known("wgen-1".into()),
                ProviderReadinessRequirement::ActiveDocument,
                ProviderReadinessState::Ready,
                ProviderQueryDeadline::None,
                ProviderCancellationState::Active,
            ),
        )
    }

    fn valid_snapshot_args() -> (
        SemanticProducer,
        String,
        String,
        u64,
        SemanticProvenance,
        SemanticConfidence,
        SemanticFreshness,
    ) {
        (
            SemanticProducer::Parser,
            "denominator-1".into(),
            "snapshot-1".into(),
            42,
            SemanticProvenance::Known(Provenance::ExactAst),
            SemanticConfidence::Known(Confidence::High),
            SemanticFreshness::Fresh,
        )
    }

    // RIPR: exercises the freshness field construction at the snapshot
    // validation boundary (model.rs:636).
    #[test]
    fn snapshot_try_new_accepts_fresh_and_rejects_stale() {
        let request = exact_ready_request();
        let (producer, denom, snap, count, prov, conf, fresh) = valid_snapshot_args();

        let ok = VerifiedProviderCompletenessSnapshot::try_new(
            &request,
            ProviderQueryCapability::Declarations,
            producer,
            &denom,
            &snap,
            count,
            prov,
            conf,
            fresh,
        );
        assert!(ok.is_ok());

        let stale = VerifiedProviderCompletenessSnapshot::try_new(
            &request,
            ProviderQueryCapability::Declarations,
            producer,
            &denom,
            &snap,
            count,
            prov,
            conf,
            SemanticFreshness::Stale,
        );
        assert!(stale.is_err());
    }

    // RIPR: exercises denominator_id and snapshot_id call sites in
    // issue_for_request (model.rs:642-643).
    #[test]
    fn issue_for_request_validates_denominator_and_snapshot_ids() {
        let request = exact_ready_request();
        let (producer, _denom, _snap, count, prov, conf, fresh) = valid_snapshot_args();

        let ok = ProviderCompletenessGrant::issue_for_request(
            &request,
            producer,
            "denominator-a",
            "snapshot-b",
            count,
            prov,
            conf,
            fresh,
        );
        assert!(ok.is_ok());

        let empty_denom = ProviderCompletenessGrant::issue_for_request(
            &request,
            producer,
            "",
            "snapshot-b",
            count,
            prov,
            conf,
            fresh,
        );
        assert!(empty_denom.is_err());

        let empty_snap = ProviderCompletenessGrant::issue_for_request(
            &request,
            producer,
            "denominator-a",
            "",
            count,
            prov,
            conf,
            fresh,
        );
        assert!(empty_snap.is_err());
    }

    // RIPR: proves the grant matches only its originating request.
    #[test]
    fn issued_grant_matches_originating_request() {
        let request = exact_ready_request();
        let (producer, denom, snap, count, prov, conf, fresh) = valid_snapshot_args();

        let grant = ProviderCompletenessGrant::issue_for_request(
            &request, producer, denom, snap, count, prov, conf, fresh,
        )
        .expect("valid args must succeed");

        assert!(grant.matches(&request));

        // A non-originating request must not match; an always-true matches()
        // implementation would otherwise satisfy the positive case above.
        let mut other = exact_ready_request();
        other.subject = ProviderQuerySubject::Entity(EntityId(2));
        assert!(!grant.matches(&other));
    }
}
