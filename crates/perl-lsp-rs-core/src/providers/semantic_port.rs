//! Transport-neutral semantic query contract for LSP provider implementations.
//!
//! The contract in this module is request-bound and fail-closed:
//!
//! - one canonical fact set supplies both values and evidence;
//! - every fact identity is unique;
//! - value facts must match the requested family and subject;
//! - exact-empty is represented only by an exact result over a complete,
//!   generation-bound authority;
//! - no-value outcomes carry outcome-specific evidence;
//! - retained results are serializable but cannot bypass validation through
//!   direct deserialization;
//! - live cancellation and deadline controls remain available while a query
//!   implementation is running.
//!
//! The module contains no LSP request/response, URI, editor, AST, HIR, or PIR
//! types. Fact producers adapt their local data into [`ProviderQueryFact`]
//! values backed by [`SemanticFactEnvelope`].

use perl_semantic_facts::{
    BoundaryDisposition, BoundaryKind, BoundaryLink, EntityId, FactId, FileId, ProviderFactTrace,
    ProviderSurface, SemanticConfidence, SemanticFactEnvelope, SemanticFactKind,
    SemanticFactStatus, SemanticFreshness, SemanticProducer, SemanticProvenance,
    SemanticReasonCode, SourceAnchor, SourceGeneration,
};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;

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
    /// Construct a known provider identity.
    #[must_use]
    pub fn known(value: impl Into<String>) -> Self {
        Self::Known(value.into())
    }

    /// Whether this value carries a non-empty stable identity.
    #[must_use]
    pub fn is_known(&self) -> bool {
        matches!(self, Self::Known(value) if !value.trim().is_empty())
    }

    fn is_malformed(&self) -> bool {
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
    /// A future generation-aware edit guard is required.
    ///
    /// The base semantic port does not authorize edits. #6819 owns the
    /// additional eligibility proof required for this scope.
    EditAuthorizing,
}

/// Readiness state observed by the caller when the query is admitted.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ProviderReadinessState {
    /// Required facts are current and available.
    Ready,
    /// A useful bounded subset is current, but the requested wider scope is incomplete.
    ReadyLimited,
    /// Required facts are still being built.
    Building,
    /// A prior snapshot exists but is stale for the request.
    Stale,
    /// Required facts are unavailable.
    Unavailable,
    /// Fact production failed for the requested scope.
    Failed,
}

/// Serializable deadline snapshot captured at query admission.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ProviderQueryDeadline {
    /// The caller supplied no deadline.
    None,
    /// Milliseconds remaining when the query context was captured.
    RemainingMillis(u64),
    /// The deadline had already expired at admission.
    Expired,
}

/// Serializable cancellation snapshot captured at query admission.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ProviderCancellationState {
    /// The request was active at admission.
    Active,
    /// Cancellation had already been requested at admission.
    Cancelled,
}

/// Live control surface available while a provider query is executing.
///
/// Admission snapshots remain useful for receipts, but implementations must poll
/// this control to observe cancellation or deadline expiry after dispatch.
pub trait ProviderQueryControl: Send + Sync {
    /// Whether cancellation has been requested now.
    fn is_cancelled(&self) -> bool;

    /// Whether the live deadline has expired now.
    fn deadline_expired(&self) -> bool;
}

/// Live control that never cancels and has no deadline.
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
    /// Readiness scope the provider requires.
    pub readiness_requirement: ProviderReadinessRequirement,
    /// Readiness state observed at query admission.
    pub readiness_state: ProviderReadinessState,
    /// Deadline snapshot captured at query admission.
    pub deadline: ProviderQueryDeadline,
    /// Cancellation snapshot captured at query admission.
    pub cancellation: ProviderCancellationState,
}

impl ProviderQueryContext {
    /// Construct a provider query context.
    #[allow(clippy::too_many_arguments)] // mirrors the explicit context contract
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

    fn is_well_formed(&self) -> bool {
        !self.project_identity.is_malformed()
            && !self.root_identity.is_malformed()
            && generation_is_well_formed(&self.document_generation)
            && generation_is_well_formed(&self.workspace_generation)
    }

    fn is_exact_ready(&self) -> bool {
        self.project_identity.is_known()
            && self.root_identity.is_known()
            && generation_is_known(&self.document_generation)
            && generation_is_known(&self.workspace_generation)
            && self.readiness_state == ProviderReadinessState::Ready
            && self.readiness_requirement != ProviderReadinessRequirement::EditAuthorizing
            && self.cancellation == ProviderCancellationState::Active
            && self.deadline != ProviderQueryDeadline::Expired
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
        /// Whether the declaration should be included with references.
        include_declaration: bool,
    },
    /// Resolve package, module, import, export, or visible-symbol facts.
    Visibility,
    /// Resolve scope, binding, or lexical-storage facts.
    ScopeBindings,
    /// Resolve generated, dynamic, compatibility, or source-locked boundaries.
    Boundaries,
    /// Resolve readiness/freshness state without requesting semantic values.
    Readiness,
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
    fn is_well_formed(&self) -> bool {
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
    ///
    /// Unknown identities remain representable. Exact eligibility is stricter
    /// and is enforced when constructing an exact result.
    #[must_use]
    pub fn is_well_formed(&self) -> bool {
        !self.request_class.trim().is_empty()
            && self.subject.is_well_formed()
            && self.context.is_well_formed()
    }
}

/// Match identity used to bind a semantic fact to the requested subject.
///
/// File, entity, and package keys are added automatically from the envelope.
/// Adapters add symbol keys only when the producer actually supplies that
/// spelling; a bare envelope cannot satisfy a symbol query by inference.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum ProviderQueryMatchKey {
    /// Canonical entity identity.
    Entity(EntityId),
    /// Canonical file identity.
    File(FileId),
    /// Package or module identity.
    Package(String),
    /// Source-level symbol spelling.
    Symbol(String),
}

/// Role one fact plays in a provider result.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum ProviderQueryFactRole {
    /// Fact is part of the externally returned semantic value set.
    Value,
    /// Fact supports a degraded, refusal, stale, dynamic, or other outcome.
    Supporting,
}

/// One canonical semantic fact bound to query match identities.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderQueryFact {
    role: ProviderQueryFactRole,
    envelope: SemanticFactEnvelope,
    match_keys: Vec<ProviderQueryMatchKey>,
}

impl ProviderQueryFact {
    /// Construct and canonicalize a query fact.
    pub fn try_new(
        role: ProviderQueryFactRole,
        envelope: SemanticFactEnvelope,
        match_keys: impl IntoIterator<Item = ProviderQueryMatchKey>,
    ) -> Result<Self, ProviderQueryContractError> {
        let mut keys: Vec<_> = match_keys.into_iter().collect();
        if let Some(entity_id) = envelope.entity_id {
            keys.push(ProviderQueryMatchKey::Entity(entity_id));
        }
        keys.push(ProviderQueryMatchKey::File(envelope.anchor.file_id));
        if let Some(package) = &envelope.package {
            keys.push(ProviderQueryMatchKey::Package(package.clone()));
        }
        for key in &keys {
            if matches!(
                key,
                ProviderQueryMatchKey::Package(value) | ProviderQueryMatchKey::Symbol(value)
                    if value.trim().is_empty()
            ) {
                return Err(ProviderQueryContractError::MalformedMatchKey);
            }
        }
        keys.sort();
        keys.dedup();
        Ok(Self {
            role,
            envelope,
            match_keys: keys,
        })
    }

    /// Construct a query fact using only identities already present in the envelope.
    pub fn from_envelope(
        role: ProviderQueryFactRole,
        envelope: SemanticFactEnvelope,
    ) -> Result<Self, ProviderQueryContractError> {
        Self::try_new(role, envelope, Vec::new())
    }

    /// Fact role.
    #[must_use]
    pub const fn role(&self) -> ProviderQueryFactRole {
        self.role
    }

    /// Canonical semantic envelope.
    #[must_use]
    pub const fn envelope(&self) -> &SemanticFactEnvelope {
        &self.envelope
    }

    /// Canonical match keys.
    #[must_use]
    pub fn match_keys(&self) -> &[ProviderQueryMatchKey] {
        &self.match_keys
    }

    fn matches_subject(&self, subject: &ProviderQuerySubject) -> bool {
        match subject {
            ProviderQuerySubject::Entity(entity_id) => self
                .match_keys
                .contains(&ProviderQueryMatchKey::Entity(*entity_id)),
            ProviderQuerySubject::File(file_id) => self
                .match_keys
                .contains(&ProviderQueryMatchKey::File(*file_id)),
            ProviderQuerySubject::Position {
                file_id,
                byte_offset,
            } => {
                self.envelope.anchor.file_id == *file_id
                    && range_contains(&self.envelope.anchor, *byte_offset)
            }
            ProviderQuerySubject::Package(package) => self
                .match_keys
                .contains(&ProviderQueryMatchKey::Package(package.clone())),
            ProviderQuerySubject::Symbol(symbol) => self
                .match_keys
                .contains(&ProviderQueryMatchKey::Symbol(symbol.clone())),
            ProviderQuerySubject::Workspace => true,
        }
    }
}

/// Completeness of the producer's denominator for the requested fact family.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum ProviderEvidenceCompleteness {
    /// The producer explicitly establishes the supported denominator.
    Complete,
    /// A useful subset is present, but exact empty is not authorized.
    Partial,
    /// Completeness was not measured.
    Unknown,
}

/// Terminal control state observed when the query result was constructed.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum ProviderQueryTerminalState {
    /// Query completed without live cancellation, deadline expiry, or instrument failure.
    Completed,
    /// Live cancellation was observed.
    Cancelled,
    /// Live deadline expiry was observed.
    DeadlineExceeded,
    /// Product or instrument execution failed.
    Failed,
}

/// Input metadata used to construct checked query evidence.
///
/// This type is serializable for diagnostics but intentionally not directly
/// deserializable as a retained result. [`ProviderQueryResult::try_new`]
/// validates and canonicalizes the complete request-bound contract.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderQueryEvidenceInput {
    completeness: ProviderEvidenceCompleteness,
    authority_producers: Vec<SemanticProducer>,
    provenance: SemanticProvenance,
    confidence: SemanticConfidence,
    freshness: SemanticFreshness,
    document_generation: SourceGeneration,
    workspace_generation: SourceGeneration,
    primary_anchor: Option<SourceAnchor>,
    boundary: Option<BoundaryLink>,
    semantic_reason: SemanticReasonCode,
    traces: Vec<ProviderFactTrace>,
    limitations: Vec<String>,
    terminal_state: ProviderQueryTerminalState,
}

impl ProviderQueryEvidenceInput {
    /// Construct and canonicalize evidence input.
    #[allow(clippy::too_many_arguments)] // mirrors the retained evidence contract
    #[must_use]
    pub fn new(
        completeness: ProviderEvidenceCompleteness,
        mut authority_producers: Vec<SemanticProducer>,
        provenance: SemanticProvenance,
        confidence: SemanticConfidence,
        freshness: SemanticFreshness,
        document_generation: SourceGeneration,
        workspace_generation: SourceGeneration,
        primary_anchor: Option<SourceAnchor>,
        boundary: Option<BoundaryLink>,
        semantic_reason: SemanticReasonCode,
        mut traces: Vec<ProviderFactTrace>,
        mut limitations: Vec<String>,
        terminal_state: ProviderQueryTerminalState,
    ) -> Self {
        authority_producers.retain(|producer| *producer != SemanticProducer::Unknown);
        authority_producers.sort();
        authority_producers.dedup();
        traces.sort_by(compare_traces);
        traces.dedup();
        limitations.retain(|limitation| !limitation.trim().is_empty());
        limitations.sort();
        limitations.dedup();
        Self {
            completeness,
            authority_producers,
            provenance,
            confidence,
            freshness,
            document_generation,
            workspace_generation,
            primary_anchor,
            boundary,
            semantic_reason,
            traces,
            limitations,
            terminal_state,
        }
    }
}

/// Proof and safety class derived from a checked provider result.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum ProviderProofClass {
    /// Current request-bound source evidence supports a read-only exact answer.
    ExactRead,
    /// Evidence supports a qualified or degraded read-only answer.
    QualifiedRead,
    /// The result is available only through an explicit fallback.
    FallbackOnly,
    /// Evidence supports refusal or another no-value outcome.
    RefusalOnly,
    /// Product/instrument state prevents a semantic proof class.
    Unknown,
}

/// Query-level outcome visible to provider policy.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum ProviderQueryOutcome {
    /// Current evidence supports an exact result. No value facts means legitimate exact empty.
    Exact,
    /// Current evidence supports a useful qualified result.
    Degraded,
    /// A weaker explicit fallback supplied the result.
    Fallback,
    /// Policy safely refused to return a value.
    Refused,
    /// Relevant facts belong to an older generation.
    Stale,
    /// Runtime-dynamic behavior prevents a static value.
    Dynamic,
    /// Multiple candidates prevent one authoritative value.
    Ambiguous,
    /// Required facts are unavailable.
    Unavailable,
    /// Live cancellation was observed.
    Cancelled,
    /// Live deadline expiry was observed.
    DeadlineExceeded,
    /// Product or instrument execution failed.
    Error,
}

/// Canonical checked evidence attached to one provider result.
///
/// Facts are retained only once by [`ProviderQueryResult`]. This evidence
/// summarizes that same canonical fact set and cannot authorize different
/// payload bytes.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderQueryEvidence {
    proof_class: ProviderProofClass,
    completeness: ProviderEvidenceCompleteness,
    producers: Vec<SemanticProducer>,
    provenance: SemanticProvenance,
    confidence: SemanticConfidence,
    freshness: SemanticFreshness,
    document_generation: SourceGeneration,
    workspace_generation: SourceGeneration,
    primary_anchor: Option<SourceAnchor>,
    boundary: Option<BoundaryLink>,
    semantic_reason: SemanticReasonCode,
    traces: Vec<ProviderFactTrace>,
    limitations: Vec<String>,
    terminal_state: ProviderQueryTerminalState,
}

impl ProviderQueryEvidence {
    /// Derived proof/safety class.
    #[must_use]
    pub const fn proof_class(&self) -> ProviderProofClass {
        self.proof_class
    }

    /// Producer-denominator completeness.
    #[must_use]
    pub const fn completeness(&self) -> ProviderEvidenceCompleteness {
        self.completeness
    }

    /// Deterministically ordered producer set.
    #[must_use]
    pub fn producers(&self) -> &[SemanticProducer] {
        &self.producers
    }

    /// Query-level provenance summary.
    #[must_use]
    pub const fn provenance(&self) -> SemanticProvenance {
        self.provenance
    }

    /// Query-level confidence summary.
    #[must_use]
    pub const fn confidence(&self) -> SemanticConfidence {
        self.confidence
    }

    /// Query-level freshness summary.
    #[must_use]
    pub const fn freshness(&self) -> SemanticFreshness {
        self.freshness
    }

    /// Document generation used by the result.
    #[must_use]
    pub fn document_generation(&self) -> &SourceGeneration {
        &self.document_generation
    }

    /// Workspace/model generation used by the result.
    #[must_use]
    pub fn workspace_generation(&self) -> &SourceGeneration {
        &self.workspace_generation
    }

    /// Primary source anchor for the result, when one exists.
    #[must_use]
    pub const fn primary_anchor(&self) -> Option<SourceAnchor> {
        self.primary_anchor
    }

    /// Dynamic or compatibility boundary limiting the result.
    #[must_use]
    pub fn boundary(&self) -> Option<&BoundaryLink> {
        self.boundary.as_ref()
    }

    /// Stable semantic reason code.
    #[must_use]
    pub const fn semantic_reason(&self) -> SemanticReasonCode {
        self.semantic_reason
    }

    /// Provider-local source traces retained for parity and explanation.
    #[must_use]
    pub fn traces(&self) -> &[ProviderFactTrace] {
        &self.traces
    }

    /// Deterministically ordered bounded limitations.
    #[must_use]
    pub fn limitations(&self) -> &[String] {
        &self.limitations
    }

    /// Terminal control state.
    #[must_use]
    pub const fn terminal_state(&self) -> ProviderQueryTerminalState {
        self.terminal_state
    }
}

/// Request-bound checked provider query result.
///
/// The result intentionally does not implement `Deserialize`; callers cannot
/// create impossible outcome/value/evidence cross-products by loading raw JSON.
/// Receipt consumers use their own versioned validated schema.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderQueryResult {
    request: ProviderQueryRequest,
    outcome: ProviderQueryOutcome,
    facts: Vec<ProviderQueryFact>,
    evidence: ProviderQueryEvidence,
}

impl ProviderQueryResult {
    /// Construct, canonicalize, and validate one request-bound result.
    pub fn try_new(
        request: &ProviderQueryRequest,
        outcome: ProviderQueryOutcome,
        mut facts: Vec<ProviderQueryFact>,
        input: ProviderQueryEvidenceInput,
    ) -> Result<Self, ProviderQueryContractError> {
        if !request.is_well_formed() {
            return Err(ProviderQueryContractError::MalformedRequest);
        }
        facts.sort_by_key(|fact| fact.envelope.fact_id);
        reject_duplicate_fact_ids(&facts)?;
        for fact in &facts {
            if !fact.matches_subject(&request.subject) {
                return Err(ProviderQueryContractError::FactDoesNotMatchSubject(
                    fact.envelope.fact_id,
                ));
            }
            if fact.role == ProviderQueryFactRole::Value
                && !value_kind_matches(&request.kind, fact.envelope.kind)
            {
                return Err(ProviderQueryContractError::FactKindDoesNotMatchRequest(
                    fact.envelope.fact_id,
                ));
            }
        }

        let evidence = build_evidence(outcome, &facts, input);
        let result = Self {
            request: request.clone(),
            outcome,
            facts,
            evidence,
        };
        result.validate_internal()?;
        Ok(result)
    }

    /// Original request identity bound to this result.
    #[must_use]
    pub const fn request(&self) -> &ProviderQueryRequest {
        &self.request
    }

    /// Query-level outcome.
    #[must_use]
    pub const fn outcome(&self) -> ProviderQueryOutcome {
        self.outcome
    }

    /// Canonical fact set supplying both values and evidence.
    #[must_use]
    pub fn facts(&self) -> &[ProviderQueryFact] {
        &self.facts
    }

    /// Value facts returned to the provider.
    pub fn value_facts(&self) -> impl Iterator<Item = &SemanticFactEnvelope> {
        self.facts
            .iter()
            .filter(|fact| fact.role == ProviderQueryFactRole::Value)
            .map(|fact| &fact.envelope)
    }

    /// Supporting facts used to explain a non-exact or no-value outcome.
    pub fn supporting_facts(&self) -> impl Iterator<Item = &SemanticFactEnvelope> {
        self.facts
            .iter()
            .filter(|fact| fact.role == ProviderQueryFactRole::Supporting)
            .map(|fact| &fact.envelope)
    }

    /// Checked evidence derived from the same canonical fact set.
    #[must_use]
    pub const fn evidence(&self) -> &ProviderQueryEvidence {
        &self.evidence
    }

    /// Whether this is an authoritative exact empty result.
    #[must_use]
    pub fn is_exact_empty(&self) -> bool {
        self.outcome == ProviderQueryOutcome::Exact && self.value_facts().next().is_none()
    }

    /// Revalidate this result against the request that a consumer intends to use.
    pub fn validate_against(
        &self,
        request: &ProviderQueryRequest,
    ) -> Result<(), ProviderQueryContractError> {
        if &self.request != request {
            return Err(ProviderQueryContractError::RequestBindingMismatch);
        }
        self.validate_internal()
    }

    fn validate_internal(&self) -> Result<(), ProviderQueryContractError> {
        let value_count = self
            .facts
            .iter()
            .filter(|fact| fact.role == ProviderQueryFactRole::Value)
            .count();
        let supporting_count = self.facts.len().saturating_sub(value_count);
        let all_facts_exact = self
            .facts
            .iter()
            .all(|fact| fact.envelope.status() == SemanticFactStatus::Exact);
        let any_stale = self
            .facts
            .iter()
            .any(|fact| fact.envelope.status() == SemanticFactStatus::Stale);
        let any_refused = self
            .facts
            .iter()
            .any(|fact| fact.envelope.status() == SemanticFactStatus::Refused);
        let has_dynamic_boundary = self
            .evidence
            .boundary
            .as_ref()
            .is_some_and(|boundary| is_dynamic_boundary(boundary.kind))
            || self.facts.iter().any(|fact| {
                fact.envelope
                    .boundary
                    .as_ref()
                    .is_some_and(|boundary| is_dynamic_boundary(boundary.kind))
            });
        let has_refuse_boundary = self
            .evidence
            .boundary
            .as_ref()
            .is_some_and(|boundary| boundary.disposition == BoundaryDisposition::Refuse)
            || self.facts.iter().any(|fact| {
                fact.envelope
                    .boundary
                    .as_ref()
                    .is_some_and(|boundary| {
                        boundary.disposition == BoundaryDisposition::Refuse
                    })
            });

        match self.outcome {
            ProviderQueryOutcome::Exact => {
                require_completed(self.evidence.terminal_state, self.outcome)?;
                if self.evidence.proof_class != ProviderProofClass::ExactRead
                    || self.evidence.completeness != ProviderEvidenceCompleteness::Complete
                    || self.evidence.producers.is_empty()
                    || !self.request.context.is_exact_ready()
                    || self.evidence.document_generation
                        != self.request.context.document_generation
                    || self.evidence.workspace_generation
                        != self.request.context.workspace_generation
                    || self.evidence.freshness != SemanticFreshness::Fresh
                    || self.evidence.boundary.is_some()
                    || !self.evidence.limitations.is_empty()
                    || supporting_count != 0
                    || !all_facts_exact
                    || self.facts.iter().any(|fact| {
                        fact.envelope.freshness != SemanticFreshness::Fresh
                            || fact.envelope.source_generation
                                != self.request.context.document_generation
                            || fact.envelope.boundary.is_some()
                    })
                {
                    return Err(ProviderQueryContractError::InvalidOutcomeEvidence(
                        ProviderQueryOutcome::Exact,
                    ));
                }
            }
            ProviderQueryOutcome::Degraded => {
                require_completed(self.evidence.terminal_state, self.outcome)?;
                if self.evidence.proof_class != ProviderProofClass::QualifiedRead
                    || value_count == 0
                    || any_stale
                    || any_refused
                {
                    return Err(ProviderQueryContractError::InvalidOutcomeEvidence(
                        ProviderQueryOutcome::Degraded,
                    ));
                }
            }
            ProviderQueryOutcome::Fallback => {
                require_completed(self.evidence.terminal_state, self.outcome)?;
                if self.evidence.proof_class != ProviderProofClass::FallbackOnly
                    || value_count == 0
                    || any_stale
                    || any_refused
                    || (self.evidence.semantic_reason == SemanticReasonCode::ExactSource
                        && self.evidence.limitations.is_empty()
                        && self.evidence.traces.iter().all(|trace| {
                            trace.fallback_state
                                != perl_semantic_facts::ProviderFallbackState::Fallback
                        }))
                {
                    return Err(ProviderQueryContractError::InvalidOutcomeEvidence(
                        ProviderQueryOutcome::Fallback,
                    ));
                }
            }
            ProviderQueryOutcome::Refused => {
                require_no_values(value_count, self.outcome)?;
                require_completed(self.evidence.terminal_state, self.outcome)?;
                if self.evidence.proof_class != ProviderProofClass::RefusalOnly
                    || !(any_refused
                        || has_refuse_boundary
                        || (self.evidence.semantic_reason
                            == SemanticReasonCode::UnsupportedEffect
                            && !self.evidence.limitations.is_empty()))
                {
                    return Err(ProviderQueryContractError::InvalidOutcomeEvidence(
                        ProviderQueryOutcome::Refused,
                    ));
                }
            }
            ProviderQueryOutcome::Stale => {
                require_no_values(value_count, self.outcome)?;
                require_completed(self.evidence.terminal_state, self.outcome)?;
                if self.evidence.proof_class != ProviderProofClass::RefusalOnly
                    || self.evidence.semantic_reason != SemanticReasonCode::StaleDependency
                    || !(any_stale
                        || self.evidence.freshness == SemanticFreshness::Stale
                        || self.request.context.readiness_state == ProviderReadinessState::Stale)
                {
                    return Err(ProviderQueryContractError::InvalidOutcomeEvidence(
                        ProviderQueryOutcome::Stale,
                    ));
                }
            }
            ProviderQueryOutcome::Dynamic => {
                require_no_values(value_count, self.outcome)?;
                require_completed(self.evidence.terminal_state, self.outcome)?;
                if self.evidence.proof_class != ProviderProofClass::RefusalOnly
                    || self.evidence.semantic_reason != SemanticReasonCode::DynamicValue
                    || !has_dynamic_boundary
                {
                    return Err(ProviderQueryContractError::InvalidOutcomeEvidence(
                        ProviderQueryOutcome::Dynamic,
                    ));
                }
            }
            ProviderQueryOutcome::Ambiguous => {
                require_no_values(value_count, self.outcome)?;
                require_completed(self.evidence.terminal_state, self.outcome)?;
                if self.evidence.proof_class != ProviderProofClass::RefusalOnly
                    || (supporting_count < 2 && self.evidence.limitations.is_empty())
                {
                    return Err(ProviderQueryContractError::InvalidOutcomeEvidence(
                        ProviderQueryOutcome::Ambiguous,
                    ));
                }
            }
            ProviderQueryOutcome::Unavailable => {
                require_no_values(value_count, self.outcome)?;
                require_completed(self.evidence.terminal_state, self.outcome)?;
                if self.evidence.proof_class != ProviderProofClass::RefusalOnly
                    || (self.evidence.completeness == ProviderEvidenceCompleteness::Complete
                        && !self.evidence.producers.is_empty()
                        && self.request.context.readiness_state == ProviderReadinessState::Ready
                        && self.evidence.limitations.is_empty())
                {
                    return Err(ProviderQueryContractError::InvalidOutcomeEvidence(
                        ProviderQueryOutcome::Unavailable,
                    ));
                }
            }
            ProviderQueryOutcome::Cancelled => {
                require_no_values(value_count, self.outcome)?;
                if self.evidence.proof_class != ProviderProofClass::RefusalOnly
                    || self.evidence.terminal_state != ProviderQueryTerminalState::Cancelled
                {
                    return Err(ProviderQueryContractError::InvalidOutcomeEvidence(
                        ProviderQueryOutcome::Cancelled,
                    ));
                }
            }
            ProviderQueryOutcome::DeadlineExceeded => {
                require_no_values(value_count, self.outcome)?;
                if self.evidence.proof_class != ProviderProofClass::RefusalOnly
                    || self.evidence.terminal_state
                        != ProviderQueryTerminalState::DeadlineExceeded
                {
                    return Err(ProviderQueryContractError::InvalidOutcomeEvidence(
                        ProviderQueryOutcome::DeadlineExceeded,
                    ));
                }
            }
            ProviderQueryOutcome::Error => {
                require_no_values(value_count, self.outcome)?;
                if self.evidence.proof_class != ProviderProofClass::Unknown
                    || self.evidence.terminal_state != ProviderQueryTerminalState::Failed
                {
                    return Err(ProviderQueryContractError::InvalidOutcomeEvidence(
                        ProviderQueryOutcome::Error,
                    ));
                }
            }
        }
        Ok(())
    }
}

/// Provider-facing semantic fact port.
///
/// Implementations adapt current AST, workspace, ProjectModel, or compiler facts
/// into checked query facts. The live control must be polled during any
/// potentially long-running operation.
pub trait ProviderSemanticPort {
    /// Query canonical semantic facts for one provider request.
    fn query(
        &self,
        request: &ProviderQueryRequest,
        control: &dyn ProviderQueryControl,
    ) -> Result<ProviderQueryResult, ProviderQueryContractError>;
}

/// Failure to construct or validate the provider query contract.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderQueryContractError {
    /// Request contains a malformed explicit identity.
    MalformedRequest,
    /// Match key contains an empty package or symbol identity.
    MalformedMatchKey,
    /// More than one fact uses the same canonical fact identity.
    DuplicateFactId(FactId),
    /// Fact does not match the query subject.
    FactDoesNotMatchSubject(FactId),
    /// Value fact kind does not match the requested fact family.
    FactKindDoesNotMatchRequest(FactId),
    /// Result is being consumed against a different request.
    RequestBindingMismatch,
    /// Outcome, values, terminal state, or evidence are contradictory.
    InvalidOutcomeEvidence(ProviderQueryOutcome),
}

impl fmt::Display for ProviderQueryContractError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MalformedRequest => formatter.write_str("provider query request is malformed"),
            Self::MalformedMatchKey => formatter.write_str("provider query match key is malformed"),
            Self::DuplicateFactId(fact_id) => {
                write!(formatter, "duplicate provider fact identity {}", fact_id.0)
            }
            Self::FactDoesNotMatchSubject(fact_id) => {
                write!(formatter, "provider fact {} does not match query subject", fact_id.0)
            }
            Self::FactKindDoesNotMatchRequest(fact_id) => {
                write!(formatter, "provider fact {} does not match query family", fact_id.0)
            }
            Self::RequestBindingMismatch => {
                formatter.write_str("provider result is bound to a different request")
            }
            Self::InvalidOutcomeEvidence(outcome) => {
                write!(formatter, "provider outcome {outcome:?} has contradictory evidence")
            }
        }
    }
}

impl Error for ProviderQueryContractError {}

fn build_evidence(
    outcome: ProviderQueryOutcome,
    facts: &[ProviderQueryFact],
    mut input: ProviderQueryEvidenceInput,
) -> ProviderQueryEvidence {
    let mut producers = input.authority_producers;
    producers.extend(facts.iter().map(|fact| fact.envelope.producer));
    producers.retain(|producer| *producer != SemanticProducer::Unknown);
    producers.sort();
    producers.dedup();

    let fact_envelopes: Vec<_> = facts.iter().map(|fact| &fact.envelope).collect();
    let provenance = summarize_provenance(&fact_envelopes, input.provenance);
    let confidence = summarize_confidence(&fact_envelopes, input.confidence);
    let freshness = summarize_freshness(&fact_envelopes, input.freshness);
    let document_generation =
        summarize_generation(&fact_envelopes, input.document_generation.clone());
    let primary_anchor = facts
        .first()
        .map(|fact| fact.envelope.anchor)
        .or(input.primary_anchor);
    let boundary = facts
        .iter()
        .find_map(|fact| fact.envelope.boundary.clone())
        .or(input.boundary);
    let semantic_reason = summarize_reason(outcome, &fact_envelopes, input.semantic_reason);

    input.traces.sort_by(compare_traces);
    input.traces.dedup();
    input.limitations.sort();
    input.limitations.dedup();

    ProviderQueryEvidence {
        proof_class: proof_for_outcome(outcome),
        completeness: input.completeness,
        producers,
        provenance,
        confidence,
        freshness,
        document_generation,
        workspace_generation: input.workspace_generation,
        primary_anchor,
        boundary,
        semantic_reason,
        traces: input.traces,
        limitations: input.limitations,
        terminal_state: input.terminal_state,
    }
}

fn proof_for_outcome(outcome: ProviderQueryOutcome) -> ProviderProofClass {
    match outcome {
        ProviderQueryOutcome::Exact => ProviderProofClass::ExactRead,
        ProviderQueryOutcome::Degraded => ProviderProofClass::QualifiedRead,
        ProviderQueryOutcome::Fallback => ProviderProofClass::FallbackOnly,
        ProviderQueryOutcome::Refused
        | ProviderQueryOutcome::Stale
        | ProviderQueryOutcome::Dynamic
        | ProviderQueryOutcome::Ambiguous
        | ProviderQueryOutcome::Unavailable
        | ProviderQueryOutcome::Cancelled
        | ProviderQueryOutcome::DeadlineExceeded => ProviderProofClass::RefusalOnly,
        ProviderQueryOutcome::Error => ProviderProofClass::Unknown,
    }
}

fn reject_duplicate_fact_ids(
    facts: &[ProviderQueryFact],
) -> Result<(), ProviderQueryContractError> {
    let mut seen = BTreeSet::new();
    for fact in facts {
        if !seen.insert(fact.envelope.fact_id) {
            return Err(ProviderQueryContractError::DuplicateFactId(
                fact.envelope.fact_id,
            ));
        }
    }
    Ok(())
}

fn value_kind_matches(kind: &ProviderQueryKind, fact_kind: SemanticFactKind) -> bool {
    match kind {
        ProviderQueryKind::Declaration => matches!(
            fact_kind,
            SemanticFactKind::Declaration | SemanticFactKind::Module
        ),
        ProviderQueryKind::References {
            include_declaration,
        } => {
            fact_kind == SemanticFactKind::Occurrence
                || (*include_declaration
                    && matches!(
                        fact_kind,
                        SemanticFactKind::Declaration | SemanticFactKind::Module
                    ))
        }
        ProviderQueryKind::Visibility => {
            matches!(fact_kind, SemanticFactKind::Import | SemanticFactKind::Module)
        }
        ProviderQueryKind::ScopeBindings => matches!(
            fact_kind,
            SemanticFactKind::Declaration | SemanticFactKind::Occurrence
        ),
        ProviderQueryKind::Boundaries => fact_kind == SemanticFactKind::Boundary,
        ProviderQueryKind::Readiness => false,
    }
}

fn compare_traces(left: &ProviderFactTrace, right: &ProviderFactTrace) -> Ordering {
    left.surface
        .cmp(&right.surface)
        .then_with(|| left.source.cmp(&right.source))
        .then_with(|| left.provenance.cmp(&right.provenance))
        .then_with(|| left.confidence.cmp(&right.confidence))
        .then_with(|| left.freshness.cmp(&right.freshness))
        .then_with(|| left.fallback_state.cmp(&right.fallback_state))
        .then_with(|| left.source_hash.cmp(&right.source_hash))
        .then_with(|| left.anchor_id.cmp(&right.anchor_id))
        .then_with(|| left.model_version.cmp(&right.model_version))
}

fn summarize_provenance(
    facts: &[&SemanticFactEnvelope],
    fallback: SemanticProvenance,
) -> SemanticProvenance {
    let mut values = facts.iter().map(|fact| fact.provenance);
    let Some(first) = values.next() else {
        return fallback;
    };
    if values.all(|value| value == first) {
        first
    } else {
        SemanticProvenance::Unknown
    }
}

fn summarize_confidence(
    facts: &[&SemanticFactEnvelope],
    fallback: SemanticConfidence,
) -> SemanticConfidence {
    if facts.is_empty() {
        return fallback;
    }
    let mut values = facts.iter().map(|fact| fact.confidence);
    let Some(first) = values.next() else {
        return fallback;
    };
    if values.all(|value| value == first) {
        first
    } else {
        SemanticConfidence::Unknown
    }
}

fn summarize_freshness(
    facts: &[&SemanticFactEnvelope],
    fallback: SemanticFreshness,
) -> SemanticFreshness {
    if facts
        .iter()
        .any(|fact| fact.freshness == SemanticFreshness::Stale)
    {
        SemanticFreshness::Stale
    } else if facts
        .iter()
        .any(|fact| fact.freshness == SemanticFreshness::Unknown)
    {
        SemanticFreshness::Unknown
    } else if facts.is_empty() {
        fallback
    } else if facts
        .iter()
        .all(|fact| fact.freshness == SemanticFreshness::Fresh)
    {
        SemanticFreshness::Fresh
    } else {
        SemanticFreshness::NotApplicable
    }
}

fn summarize_generation(
    facts: &[&SemanticFactEnvelope],
    fallback: SourceGeneration,
) -> SourceGeneration {
    let mut values = facts.iter().map(|fact| &fact.source_generation);
    let Some(first) = values.next() else {
        return fallback;
    };
    if values.all(|value| value == first) {
        first.clone()
    } else {
        SourceGeneration::Unknown
    }
}

fn summarize_reason(
    outcome: ProviderQueryOutcome,
    facts: &[&SemanticFactEnvelope],
    fallback: SemanticReasonCode,
) -> SemanticReasonCode {
    match outcome {
        ProviderQueryOutcome::Exact => SemanticReasonCode::ExactSource,
        ProviderQueryOutcome::Stale => SemanticReasonCode::StaleDependency,
        ProviderQueryOutcome::Dynamic => SemanticReasonCode::DynamicValue,
        ProviderQueryOutcome::Refused => facts
            .iter()
            .find(|fact| fact.status() == SemanticFactStatus::Refused)
            .map(|fact| fact.reason_code)
            .unwrap_or(fallback),
        ProviderQueryOutcome::Degraded | ProviderQueryOutcome::Fallback => facts
            .iter()
            .find(|fact| fact.reason_code != SemanticReasonCode::ExactSource)
            .map(|fact| fact.reason_code)
            .unwrap_or(fallback),
        _ => fallback,
    }
}

fn range_contains(anchor: &SourceAnchor, byte_offset: u32) -> bool {
    if anchor.start_byte == anchor.end_byte {
        byte_offset == anchor.start_byte
    } else {
        anchor.start_byte <= byte_offset && byte_offset < anchor.end_byte
    }
}

fn generation_is_known(generation: &SourceGeneration) -> bool {
    matches!(generation, SourceGeneration::Known(value) if !value.trim().is_empty())
}

fn generation_is_well_formed(generation: &SourceGeneration) -> bool {
    !matches!(generation, SourceGeneration::Known(value) if value.trim().is_empty())
}

fn is_dynamic_boundary(kind: BoundaryKind) -> bool {
    matches!(
        kind,
        BoundaryKind::DynamicValue
            | BoundaryKind::DynamicRequire
            | BoundaryKind::DynamicIncludePath
            | BoundaryKind::CompileTimeExecution
            | BoundaryKind::SymbolicReference
    )
}

fn require_completed(
    terminal: ProviderQueryTerminalState,
    outcome: ProviderQueryOutcome,
) -> Result<(), ProviderQueryContractError> {
    if terminal == ProviderQueryTerminalState::Completed {
        Ok(())
    } else {
        Err(ProviderQueryContractError::InvalidOutcomeEvidence(outcome))
    }
}

fn require_no_values(
    value_count: usize,
    outcome: ProviderQueryOutcome,
) -> Result<(), ProviderQueryContractError> {
    if value_count == 0 {
        Ok(())
    } else {
        Err(ProviderQueryContractError::InvalidOutcomeEvidence(outcome))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_semantic_facts::{
        AnchorId, Confidence, LifecyclePhase, Provenance, ProviderFactFreshness,
        ProviderFactSourceKind, ProviderFallbackState,
    };
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

    fn context() -> ProviderQueryContext {
        ProviderQueryContext::new(
            ProviderIdentity::known("project"),
            ProviderIdentity::known("root"),
            SourceGeneration::known("document-1"),
            SourceGeneration::known("workspace-1"),
            ProviderReadinessRequirement::ActiveDocument,
            ProviderReadinessState::Ready,
            ProviderQueryDeadline::RemainingMillis(250),
            ProviderCancellationState::Active,
        )
    }

    fn request(kind: ProviderQueryKind, subject: ProviderQuerySubject) -> ProviderQueryRequest {
        ProviderQueryRequest::new(
            ProviderSurface::Definition,
            "test/request",
            kind,
            subject,
            context(),
        )
    }

    fn envelope(
        fact_id: u64,
        kind: SemanticFactKind,
        producer: SemanticProducer,
        freshness: SemanticFreshness,
        generation: &str,
        boundary: Option<BoundaryLink>,
    ) -> SemanticFactEnvelope {
        SemanticFactEnvelope::new(
            FactId(fact_id),
            Some(EntityId(fact_id + 100)),
            kind,
            SourceAnchor::new(Some(AnchorId(fact_id + 200)), FileId(1), 1, 4),
            SourceGeneration::known(generation),
            None,
            Some("Example".to_string()),
            LifecyclePhase::Runtime,
            producer,
            SemanticProvenance::Known(Provenance::ExactAst),
            SemanticConfidence::Known(Confidence::High),
            freshness,
            boundary,
            Vec::new(),
            if freshness == SemanticFreshness::Stale {
                SemanticReasonCode::StaleDependency
            } else {
                SemanticReasonCode::ExactSource
            },
        )
    }

    fn input(
        completeness: ProviderEvidenceCompleteness,
        producers: Vec<SemanticProducer>,
        freshness: SemanticFreshness,
        reason: SemanticReasonCode,
        boundary: Option<BoundaryLink>,
        terminal: ProviderQueryTerminalState,
        limitations: Vec<String>,
    ) -> ProviderQueryEvidenceInput {
        ProviderQueryEvidenceInput::new(
            completeness,
            producers,
            SemanticProvenance::Known(Provenance::ExactAst),
            SemanticConfidence::Known(Confidence::High),
            freshness,
            SourceGeneration::known("document-1"),
            SourceGeneration::known("workspace-1"),
            None,
            boundary,
            reason,
            Vec::new(),
            limitations,
            terminal,
        )
    }

    fn value_fact(
        fact_id: u64,
        producer: SemanticProducer,
    ) -> Result<ProviderQueryFact, ProviderQueryContractError> {
        ProviderQueryFact::try_new(
            ProviderQueryFactRole::Value,
            envelope(
                fact_id,
                SemanticFactKind::Declaration,
                producer,
                SemanticFreshness::Fresh,
                "document-1",
                None,
            ),
            [ProviderQueryMatchKey::Symbol("target".to_string())],
        )
    }

    #[test]
    fn exact_empty_is_distinct_from_unavailable() -> Result<(), Box<dyn Error>> {
        let request = request(
            ProviderQueryKind::Declaration,
            ProviderQuerySubject::Symbol("target".to_string()),
        );
        let exact = ProviderQueryResult::try_new(
            &request,
            ProviderQueryOutcome::Exact,
            Vec::new(),
            input(
                ProviderEvidenceCompleteness::Complete,
                vec![SemanticProducer::Parser],
                SemanticFreshness::Fresh,
                SemanticReasonCode::ExactSource,
                None,
                ProviderQueryTerminalState::Completed,
                Vec::new(),
            ),
        )?;
        assert!(exact.is_exact_empty());
        assert_eq!(exact.value_facts().count(), 0);
        exact.validate_against(&request)?;

        let unavailable = ProviderQueryResult::try_new(
            &request,
            ProviderQueryOutcome::Unavailable,
            Vec::new(),
            input(
                ProviderEvidenceCompleteness::Partial,
                vec![SemanticProducer::Parser],
                SemanticFreshness::Fresh,
                SemanticReasonCode::Unknown,
                None,
                ProviderQueryTerminalState::Completed,
                Vec::new(),
            ),
        )?;
        assert!(!unavailable.is_exact_empty());
        assert_eq!(unavailable.value_facts().count(), 0);
        unavailable.validate_against(&request)?;
        Ok(())
    }

    #[test]
    fn duplicate_fact_identity_is_rejected() -> Result<(), Box<dyn Error>> {
        let request = request(
            ProviderQueryKind::Declaration,
            ProviderQuerySubject::Symbol("target".to_string()),
        );
        let fact = value_fact(1, SemanticProducer::Parser)?;
        let result = ProviderQueryResult::try_new(
            &request,
            ProviderQueryOutcome::Exact,
            vec![fact.clone(), fact],
            input(
                ProviderEvidenceCompleteness::Complete,
                vec![SemanticProducer::Parser],
                SemanticFreshness::Fresh,
                SemanticReasonCode::ExactSource,
                None,
                ProviderQueryTerminalState::Completed,
                Vec::new(),
            ),
        );
        assert_eq!(
            result.err(),
            Some(ProviderQueryContractError::DuplicateFactId(FactId(1)))
        );
        Ok(())
    }

    #[test]
    fn result_is_bound_to_request_family_subject_and_generation() -> Result<(), Box<dyn Error>> {
        let definition_request = request(
            ProviderQueryKind::Declaration,
            ProviderQuerySubject::Symbol("target".to_string()),
        );
        let result = ProviderQueryResult::try_new(
            &definition_request,
            ProviderQueryOutcome::Exact,
            vec![value_fact(1, SemanticProducer::Parser)?],
            input(
                ProviderEvidenceCompleteness::Complete,
                vec![SemanticProducer::Parser],
                SemanticFreshness::Fresh,
                SemanticReasonCode::ExactSource,
                None,
                ProviderQueryTerminalState::Completed,
                Vec::new(),
            ),
        )?;
        result.validate_against(&definition_request)?;

        let wrong_subject = request(
            ProviderQueryKind::Declaration,
            ProviderQuerySubject::Symbol("other".to_string()),
        );
        assert_eq!(
            result.validate_against(&wrong_subject).err(),
            Some(ProviderQueryContractError::RequestBindingMismatch)
        );

        let wrong_kind = ProviderQueryResult::try_new(
            &request(
                ProviderQueryKind::Boundaries,
                ProviderQuerySubject::Symbol("target".to_string()),
            ),
            ProviderQueryOutcome::Degraded,
            vec![value_fact(2, SemanticProducer::Parser)?],
            input(
                ProviderEvidenceCompleteness::Partial,
                vec![SemanticProducer::Parser],
                SemanticFreshness::Fresh,
                SemanticReasonCode::CompatibilityBoundary,
                None,
                ProviderQueryTerminalState::Completed,
                Vec::new(),
            ),
        );
        assert_eq!(
            wrong_kind.err(),
            Some(ProviderQueryContractError::FactKindDoesNotMatchRequest(
                FactId(2)
            ))
        );

        let stale_fact = ProviderQueryFact::try_new(
            ProviderQueryFactRole::Value,
            envelope(
                3,
                SemanticFactKind::Declaration,
                SemanticProducer::Parser,
                SemanticFreshness::Fresh,
                "old-document",
                None,
            ),
            [ProviderQueryMatchKey::Symbol("target".to_string())],
        )?;
        let cross_generation = ProviderQueryResult::try_new(
            &definition_request,
            ProviderQueryOutcome::Exact,
            vec![stale_fact],
            input(
                ProviderEvidenceCompleteness::Complete,
                vec![SemanticProducer::Parser],
                SemanticFreshness::Fresh,
                SemanticReasonCode::ExactSource,
                None,
                ProviderQueryTerminalState::Completed,
                Vec::new(),
            ),
        );
        assert_eq!(
            cross_generation.err(),
            Some(ProviderQueryContractError::InvalidOutcomeEvidence(
                ProviderQueryOutcome::Exact
            ))
        );
        Ok(())
    }

    #[test]
    fn producer_name_cannot_upgrade_partial_evidence() -> Result<(), Box<dyn Error>> {
        let request = request(
            ProviderQueryKind::Declaration,
            ProviderQuerySubject::Symbol("target".to_string()),
        );
        let exact_attempt = ProviderQueryResult::try_new(
            &request,
            ProviderQueryOutcome::Exact,
            vec![value_fact(1, SemanticProducer::PirA)?],
            input(
                ProviderEvidenceCompleteness::Partial,
                vec![SemanticProducer::PirA],
                SemanticFreshness::Fresh,
                SemanticReasonCode::ExactSource,
                None,
                ProviderQueryTerminalState::Completed,
                Vec::new(),
            ),
        );
        assert_eq!(
            exact_attempt.err(),
            Some(ProviderQueryContractError::InvalidOutcomeEvidence(
                ProviderQueryOutcome::Exact
            ))
        );

        let fallback = ProviderQueryResult::try_new(
            &request,
            ProviderQueryOutcome::Fallback,
            vec![value_fact(2, SemanticProducer::PirA)?],
            input(
                ProviderEvidenceCompleteness::Partial,
                vec![SemanticProducer::PirA],
                SemanticFreshness::Fresh,
                SemanticReasonCode::CompatibilityBoundary,
                None,
                ProviderQueryTerminalState::Completed,
                vec!["fallback selected".to_string()],
            ),
        )?;
        assert_eq!(
            fallback.evidence().proof_class(),
            ProviderProofClass::FallbackOnly
        );
        assert_eq!(fallback.evidence().producers(), &[SemanticProducer::PirA]);
        Ok(())
    }

    #[test]
    fn no_value_outcomes_require_distinct_evidence() -> Result<(), Box<dyn Error>> {
        let request = request(
            ProviderQueryKind::Boundaries,
            ProviderQuerySubject::Workspace,
        );
        let dynamic_boundary = BoundaryLink::new(
            Some(FactId(9)),
            BoundaryKind::DynamicValue,
            BoundaryDisposition::Degrade,
            SemanticReasonCode::DynamicValue,
        );
        let dynamic = ProviderQueryResult::try_new(
            &request,
            ProviderQueryOutcome::Dynamic,
            Vec::new(),
            input(
                ProviderEvidenceCompleteness::Partial,
                vec![SemanticProducer::SemanticAnalyzer],
                SemanticFreshness::Fresh,
                SemanticReasonCode::DynamicValue,
                Some(dynamic_boundary),
                ProviderQueryTerminalState::Completed,
                Vec::new(),
            ),
        )?;
        dynamic.validate_against(&request)?;

        let bad_dynamic = ProviderQueryResult::try_new(
            &request,
            ProviderQueryOutcome::Dynamic,
            Vec::new(),
            input(
                ProviderEvidenceCompleteness::Partial,
                vec![SemanticProducer::SemanticAnalyzer],
                SemanticFreshness::Fresh,
                SemanticReasonCode::DynamicValue,
                None,
                ProviderQueryTerminalState::Completed,
                Vec::new(),
            ),
        );
        assert_eq!(
            bad_dynamic.err(),
            Some(ProviderQueryContractError::InvalidOutcomeEvidence(
                ProviderQueryOutcome::Dynamic
            ))
        );

        let stale = ProviderQueryResult::try_new(
            &request,
            ProviderQueryOutcome::Stale,
            Vec::new(),
            input(
                ProviderEvidenceCompleteness::Partial,
                vec![SemanticProducer::WorkspaceIndex],
                SemanticFreshness::Stale,
                SemanticReasonCode::StaleDependency,
                None,
                ProviderQueryTerminalState::Completed,
                Vec::new(),
            ),
        )?;
        stale.validate_against(&request)?;

        let cancelled = ProviderQueryResult::try_new(
            &request,
            ProviderQueryOutcome::Cancelled,
            Vec::new(),
            input(
                ProviderEvidenceCompleteness::Unknown,
                Vec::new(),
                SemanticFreshness::Unknown,
                SemanticReasonCode::Unknown,
                None,
                ProviderQueryTerminalState::Cancelled,
                Vec::new(),
            ),
        )?;
        cancelled.validate_against(&request)?;

        let bad_cancelled = ProviderQueryResult::try_new(
            &request,
            ProviderQueryOutcome::Cancelled,
            Vec::new(),
            input(
                ProviderEvidenceCompleteness::Unknown,
                Vec::new(),
                SemanticFreshness::Unknown,
                SemanticReasonCode::Unknown,
                None,
                ProviderQueryTerminalState::Completed,
                Vec::new(),
            ),
        );
        assert_eq!(
            bad_cancelled.err(),
            Some(ProviderQueryContractError::InvalidOutcomeEvidence(
                ProviderQueryOutcome::Cancelled
            ))
        );
        Ok(())
    }

    #[test]
    fn canonical_serialization_is_order_independent() -> Result<(), Box<dyn Error>> {
        let request = request(
            ProviderQueryKind::Declaration,
            ProviderQuerySubject::Workspace,
        );
        let first = ProviderQueryFact::from_envelope(
            ProviderQueryFactRole::Value,
            envelope(
                1,
                SemanticFactKind::Declaration,
                SemanticProducer::Parser,
                SemanticFreshness::Fresh,
                "document-1",
                None,
            ),
        )?;
        let second = ProviderQueryFact::from_envelope(
            ProviderQueryFactRole::Value,
            envelope(
                2,
                SemanticFactKind::Declaration,
                SemanticProducer::PirA,
                SemanticFreshness::Fresh,
                "document-1",
                None,
            ),
        )?;
        let left = ProviderQueryResult::try_new(
            &request,
            ProviderQueryOutcome::Exact,
            vec![second.clone(), first.clone()],
            input(
                ProviderEvidenceCompleteness::Complete,
                vec![SemanticProducer::PirA, SemanticProducer::Parser],
                SemanticFreshness::Fresh,
                SemanticReasonCode::ExactSource,
                None,
                ProviderQueryTerminalState::Completed,
                Vec::new(),
            ),
        )?;
        let right = ProviderQueryResult::try_new(
            &request,
            ProviderQueryOutcome::Exact,
            vec![first, second],
            input(
                ProviderEvidenceCompleteness::Complete,
                vec![SemanticProducer::Parser, SemanticProducer::PirA],
                SemanticFreshness::Fresh,
                SemanticReasonCode::ExactSource,
                None,
                ProviderQueryTerminalState::Completed,
                Vec::new(),
            ),
        )?;
        assert_eq!(serde_json::to_string(&left)?, serde_json::to_string(&right)?);
        Ok(())
    }

    struct SequencedControl {
        cancellation_checks: AtomicUsize,
        deadline_checks: AtomicUsize,
    }

    impl SequencedControl {
        fn cancellation_after_dispatch() -> Self {
            Self {
                cancellation_checks: AtomicUsize::new(0),
                deadline_checks: AtomicUsize::new(0),
            }
        }

        fn deadline_after_dispatch() -> Self {
            Self {
                cancellation_checks: AtomicUsize::new(usize::MAX / 2),
                deadline_checks: AtomicUsize::new(0),
            }
        }
    }

    impl ProviderQueryControl for SequencedControl {
        fn is_cancelled(&self) -> bool {
            self.cancellation_checks
                .fetch_add(1, AtomicOrdering::SeqCst)
                == 1
        }

        fn deadline_expired(&self) -> bool {
            self.deadline_checks.fetch_add(1, AtomicOrdering::SeqCst) == 1
        }
    }

    struct PollingPort;

    impl ProviderSemanticPort for PollingPort {
        fn query(
            &self,
            request: &ProviderQueryRequest,
            control: &dyn ProviderQueryControl,
        ) -> Result<ProviderQueryResult, ProviderQueryContractError> {
            let _started_cancelled = control.is_cancelled();
            let _started_expired = control.deadline_expired();
            if control.is_cancelled() {
                return ProviderQueryResult::try_new(
                    request,
                    ProviderQueryOutcome::Cancelled,
                    Vec::new(),
                    input(
                        ProviderEvidenceCompleteness::Unknown,
                        Vec::new(),
                        SemanticFreshness::Unknown,
                        SemanticReasonCode::Unknown,
                        None,
                        ProviderQueryTerminalState::Cancelled,
                        Vec::new(),
                    ),
                );
            }
            if control.deadline_expired() {
                return ProviderQueryResult::try_new(
                    request,
                    ProviderQueryOutcome::DeadlineExceeded,
                    Vec::new(),
                    input(
                        ProviderEvidenceCompleteness::Unknown,
                        Vec::new(),
                        SemanticFreshness::Unknown,
                        SemanticReasonCode::Unknown,
                        None,
                        ProviderQueryTerminalState::DeadlineExceeded,
                        Vec::new(),
                    ),
                );
            }
            ProviderQueryResult::try_new(
                request,
                ProviderQueryOutcome::Unavailable,
                Vec::new(),
                input(
                    ProviderEvidenceCompleteness::Unknown,
                    Vec::new(),
                    SemanticFreshness::Unknown,
                    SemanticReasonCode::Unknown,
                    None,
                    ProviderQueryTerminalState::Completed,
                    Vec::new(),
                ),
            )
        }
    }

    #[test]
    fn live_control_observes_cancellation_and_deadline_after_dispatch() -> Result<(), Box<dyn Error>>
    {
        let request = request(
            ProviderQueryKind::Readiness,
            ProviderQuerySubject::Workspace,
        );
        let cancelled =
            PollingPort.query(&request, &SequencedControl::cancellation_after_dispatch())?;
        assert_eq!(cancelled.outcome(), ProviderQueryOutcome::Cancelled);

        let deadline = PollingPort.query(&request, &SequencedControl::deadline_after_dispatch())?;
        assert_eq!(deadline.outcome(), ProviderQueryOutcome::DeadlineExceeded);
        Ok(())
    }

    #[test]
    fn trace_order_is_canonicalized() {
        let first = ProviderFactTrace::new(
            ProviderSurface::Definition,
            ProviderFactSourceKind::ParserSyntax,
            Provenance::ExactAst,
            Confidence::High,
            ProviderFactFreshness::Fresh,
            ProviderFallbackState::Primary,
            Some("a".to_string()),
            Some(AnchorId(1)),
            Some(1),
        );
        let second = ProviderFactTrace::new(
            ProviderSurface::Definition,
            ProviderFactSourceKind::CompilerFact,
            Provenance::ExactAst,
            Confidence::High,
            ProviderFactFreshness::Fresh,
            ProviderFallbackState::Shadow,
            Some("b".to_string()),
            Some(AnchorId(2)),
            Some(1),
        );
        let evidence = ProviderQueryEvidenceInput::new(
            ProviderEvidenceCompleteness::Partial,
            Vec::new(),
            SemanticProvenance::Unknown,
            SemanticConfidence::Unknown,
            SemanticFreshness::Unknown,
            SourceGeneration::Unknown,
            SourceGeneration::Unknown,
            None,
            None,
            SemanticReasonCode::Unknown,
            vec![second.clone(), first.clone(), second.clone()],
            vec!["z".to_string(), "a".to_string(), "a".to_string()],
            ProviderQueryTerminalState::Completed,
        );
        assert_eq!(evidence.traces, vec![first, second]);
        assert_eq!(evidence.limitations, vec!["a".to_string(), "z".to_string()]);
    }
}
