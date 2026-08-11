//! Transport-neutral semantic query contract for LSP provider implementations.
//!
//! This module defines the provider-facing port between canonical semantic facts
//! and presentation-specific LSP handlers. It deliberately contains no LSP
//! request, response, URI, or editor types. Producers and adapters supply
//! [`SemanticFactEnvelope`] values; provider handlers remain responsible for
//! translating a query result into protocol output.

use perl_semantic_facts::{
    BoundaryLink, EntityId, FileId, ProviderFactTrace, ProviderSurface, SemanticConfidence,
    SemanticFactEnvelope, SemanticFactStatus, SemanticFreshness, SemanticProducer,
    SemanticProvenance, SemanticReasonCode, SourceAnchor, SourceGeneration,
};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

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
        matches!(self, Self::Known(value) if !value.is_empty())
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
    /// Current complete facts strong enough to authorize an edit are required.
    EditAuthorizing,
}

/// Readiness state observed by the caller when the query begins.
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

/// Serializable deadline state for one provider query.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ProviderQueryDeadline {
    /// The caller supplied no deadline.
    None,
    /// Milliseconds remaining when the query context was captured.
    RemainingMillis(u64),
    /// The deadline had already expired.
    Expired,
}

/// Cancellation state observed by the provider query boundary.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ProviderCancellationState {
    /// The request may continue.
    Active,
    /// Cancellation was already requested.
    Cancelled,
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
    /// Deadline state captured at query admission.
    pub deadline: ProviderQueryDeadline,
    /// Cancellation state captured at query admission.
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
            && !matches!(
                &self.document_generation,
                SourceGeneration::Known(value) if value.trim().is_empty()
            )
            && !matches!(
                &self.workspace_generation,
                SourceGeneration::Known(value) if value.trim().is_empty()
            )
    }
}

/// Semantic query family requested by a provider.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
        Self { surface, request_class: request_class.into(), kind, subject, context }
    }

    /// Whether the request contains no malformed explicit identities.
    ///
    /// Unknown project, root, or generation values remain representable. Exact
    /// eligibility for those unknown states is enforced by the later common
    /// freshness/readiness guard rather than guessed here.
    #[must_use]
    pub fn is_well_formed(&self) -> bool {
        !self.request_class.trim().is_empty()
            && self.subject.is_well_formed()
            && self.context.is_well_formed()
    }
}

/// Proof and safety class attached to a provider result.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ProviderProofClass {
    /// Current source-backed evidence supports a read-only exact answer.
    ExactRead,
    /// Current source-backed evidence supports an exact edit-authorizing answer.
    EditAuthorizing,
    /// Evidence supports a qualified or degraded read-only answer.
    QualifiedRead,
    /// The result is available only through an explicit fallback.
    FallbackOnly,
    /// Evidence supports refusal or another no-value outcome.
    RefusalOnly,
    /// Proof class is unavailable and cannot authorize exactness.
    Unknown,
}

/// Query-level outcome visible to provider policy.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ProviderQueryOutcome {
    /// Current evidence supports an exact result. An empty value list is a
    /// legitimate exact empty result.
    Exact,
    /// Current evidence supports a useful qualified result.
    Degraded,
    /// A weaker explicit fallback supplied the result.
    Fallback,
    /// Policy safely refused to return or authorize a value.
    Refused,
    /// Relevant facts belong to an older generation.
    Stale,
    /// Runtime-dynamic behavior prevents a static value.
    Dynamic,
    /// Multiple candidates prevent one authoritative value.
    Ambiguous,
    /// Required facts are unavailable.
    Unavailable,
    /// The request was cancelled.
    Cancelled,
    /// The request deadline expired.
    DeadlineExceeded,
    /// Product or instrument execution failed.
    Error,
}

/// Canonical evidence attached to one provider query result.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderQueryEvidence {
    proof_class: ProviderProofClass,
    producers: Vec<SemanticProducer>,
    provenance: SemanticProvenance,
    confidence: SemanticConfidence,
    freshness: SemanticFreshness,
    document_generation: SourceGeneration,
    workspace_generation: SourceGeneration,
    primary_anchor: Option<SourceAnchor>,
    boundary: Option<BoundaryLink>,
    reason_code: SemanticReasonCode,
    facts: Vec<SemanticFactEnvelope>,
    traces: Vec<ProviderFactTrace>,
    limitations: Vec<String>,
}

impl ProviderQueryEvidence {
    /// Construct evidence and canonicalize retained collection order.
    #[allow(clippy::too_many_arguments)] // mirrors the query evidence contract
    #[must_use]
    pub fn new(
        proof_class: ProviderProofClass,
        mut producers: Vec<SemanticProducer>,
        provenance: SemanticProvenance,
        confidence: SemanticConfidence,
        freshness: SemanticFreshness,
        document_generation: SourceGeneration,
        workspace_generation: SourceGeneration,
        primary_anchor: Option<SourceAnchor>,
        boundary: Option<BoundaryLink>,
        reason_code: SemanticReasonCode,
        mut facts: Vec<SemanticFactEnvelope>,
        mut traces: Vec<ProviderFactTrace>,
        mut limitations: Vec<String>,
    ) -> Self {
        producers.extend(facts.iter().map(|fact| fact.producer));
        producers.sort();
        producers.dedup();

        facts.sort_by_key(|fact| fact.fact_id);
        traces.sort_by(compare_traces);
        limitations.retain(|limitation| !limitation.trim().is_empty());
        limitations.sort();
        limitations.dedup();

        Self {
            proof_class,
            producers,
            provenance,
            confidence,
            freshness,
            document_generation,
            workspace_generation,
            primary_anchor,
            boundary,
            reason_code,
            facts,
            traces,
            limitations,
        }
    }

    /// Proof/safety class for the result.
    #[must_use]
    pub const fn proof_class(&self) -> ProviderProofClass {
        self.proof_class
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
    pub const fn reason_code(&self) -> SemanticReasonCode {
        self.reason_code
    }

    /// Canonical fact envelopes contributing to the result.
    #[must_use]
    pub fn facts(&self) -> &[SemanticFactEnvelope] {
        &self.facts
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

/// Query result whose `values` field preserves exact-empty versus unavailable.
///
/// `Some(Vec::new())` under [`ProviderQueryOutcome::Exact`] is a legitimate
/// exact empty result. `None` means the outcome does not carry a value.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderQueryResult<T> {
    outcome: ProviderQueryOutcome,
    values: Option<Vec<T>>,
    evidence: ProviderQueryEvidence,
}

impl<T> ProviderQueryResult<T> {
    /// Construct an exact result. An empty vector remains authoritative empty.
    #[must_use]
    pub fn exact(values: Vec<T>, evidence: ProviderQueryEvidence) -> Self {
        Self { outcome: ProviderQueryOutcome::Exact, values: Some(values), evidence }
    }

    /// Construct a qualified/degraded result.
    #[must_use]
    pub fn degraded(values: Vec<T>, evidence: ProviderQueryEvidence) -> Self {
        Self { outcome: ProviderQueryOutcome::Degraded, values: Some(values), evidence }
    }

    /// Construct an explicit fallback result.
    #[must_use]
    pub fn fallback(values: Vec<T>, evidence: ProviderQueryEvidence) -> Self {
        Self { outcome: ProviderQueryOutcome::Fallback, values: Some(values), evidence }
    }

    /// Construct a safe refusal.
    #[must_use]
    pub fn refused(evidence: ProviderQueryEvidence) -> Self {
        Self::without_values(ProviderQueryOutcome::Refused, evidence)
    }

    /// Construct a stale result.
    #[must_use]
    pub fn stale(evidence: ProviderQueryEvidence) -> Self {
        Self::without_values(ProviderQueryOutcome::Stale, evidence)
    }

    /// Construct a dynamic-boundary result.
    #[must_use]
    pub fn dynamic(evidence: ProviderQueryEvidence) -> Self {
        Self::without_values(ProviderQueryOutcome::Dynamic, evidence)
    }

    /// Construct an ambiguous result.
    #[must_use]
    pub fn ambiguous(evidence: ProviderQueryEvidence) -> Self {
        Self::without_values(ProviderQueryOutcome::Ambiguous, evidence)
    }

    /// Construct an unavailable result.
    #[must_use]
    pub fn unavailable(evidence: ProviderQueryEvidence) -> Self {
        Self::without_values(ProviderQueryOutcome::Unavailable, evidence)
    }

    /// Construct a cancelled result.
    #[must_use]
    pub fn cancelled(evidence: ProviderQueryEvidence) -> Self {
        Self::without_values(ProviderQueryOutcome::Cancelled, evidence)
    }

    /// Construct a deadline-exceeded result.
    #[must_use]
    pub fn deadline_exceeded(evidence: ProviderQueryEvidence) -> Self {
        Self::without_values(ProviderQueryOutcome::DeadlineExceeded, evidence)
    }

    /// Construct a product or instrument error result.
    #[must_use]
    pub fn error(evidence: ProviderQueryEvidence) -> Self {
        Self::without_values(ProviderQueryOutcome::Error, evidence)
    }

    fn without_values(outcome: ProviderQueryOutcome, evidence: ProviderQueryEvidence) -> Self {
        Self { outcome, values: None, evidence }
    }

    /// Query-level outcome.
    #[must_use]
    pub const fn outcome(&self) -> ProviderQueryOutcome {
        self.outcome
    }

    /// Values returned by exact, degraded, or fallback outcomes.
    ///
    /// `Some(&[])` is exact or qualified empty. `None` is explicit absence.
    #[must_use]
    pub fn values(&self) -> Option<&[T]> {
        self.values.as_deref()
    }

    /// Evidence attached to the result.
    #[must_use]
    pub const fn evidence(&self) -> &ProviderQueryEvidence {
        &self.evidence
    }

    /// Whether this result is an authoritative exact empty result.
    #[must_use]
    pub fn is_exact_empty(&self) -> bool {
        self.outcome == ProviderQueryOutcome::Exact
            && self.values.as_ref().is_some_and(Vec::is_empty)
    }

    /// Whether the outcome, value presence, proof class, and exact fact status
    /// agree with the contract.
    #[must_use]
    pub fn is_consistent(&self) -> bool {
        match self.outcome {
            ProviderQueryOutcome::Exact => {
                self.values.is_some()
                    && matches!(
                        self.evidence.proof_class,
                        ProviderProofClass::ExactRead | ProviderProofClass::EditAuthorizing
                    )
                    && self
                        .evidence
                        .facts
                        .iter()
                        .all(|fact| fact.status() == SemanticFactStatus::Exact)
            }
            ProviderQueryOutcome::Degraded => {
                self.values.is_some()
                    && self.evidence.proof_class == ProviderProofClass::QualifiedRead
            }
            ProviderQueryOutcome::Fallback => {
                self.values.is_some()
                    && self.evidence.proof_class == ProviderProofClass::FallbackOnly
            }
            ProviderQueryOutcome::Refused
            | ProviderQueryOutcome::Stale
            | ProviderQueryOutcome::Dynamic
            | ProviderQueryOutcome::Ambiguous
            | ProviderQueryOutcome::Unavailable
            | ProviderQueryOutcome::Cancelled
            | ProviderQueryOutcome::DeadlineExceeded
            | ProviderQueryOutcome::Error => {
                self.values.is_none()
                    && matches!(
                        self.evidence.proof_class,
                        ProviderProofClass::RefusalOnly | ProviderProofClass::Unknown
                    )
            }
        }
    }
}

/// Provider-facing semantic fact port.
///
/// Implementations adapt current AST, workspace, ProjectModel, or compiler
/// facts into [`SemanticFactEnvelope`] values. The trait contains no LSP
/// presentation types and does not imply that any query class is live.
pub trait ProviderSemanticPort {
    /// Query canonical semantic facts for one provider request.
    fn query(
        &self,
        request: &ProviderQueryRequest,
    ) -> ProviderQueryResult<SemanticFactEnvelope>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_semantic_facts::{
        AnchorId, Confidence, FactId, LifecyclePhase, Provenance, ProviderFactFreshness,
        ProviderFactSourceKind, ProviderFallbackState, SemanticFactKind,
    };

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

    fn exact_fact(fact_id: u64, producer: SemanticProducer) -> SemanticFactEnvelope {
        SemanticFactEnvelope::new(
            FactId(fact_id),
            Some(EntityId(fact_id + 100)),
            SemanticFactKind::Declaration,
            SourceAnchor::new(Some(AnchorId(fact_id + 200)), FileId(1), 1, 4),
            SourceGeneration::known("document-1"),
            None,
            Some("Example".to_string()),
            LifecyclePhase::Runtime,
            producer,
            SemanticProvenance::Known(Provenance::ExactAst),
            SemanticConfidence::Known(Confidence::High),
            SemanticFreshness::Fresh,
            None,
            Vec::new(),
            SemanticReasonCode::ExactSource,
        )
    }

    fn evidence(
        proof_class: ProviderProofClass,
        producers: Vec<SemanticProducer>,
        facts: Vec<SemanticFactEnvelope>,
    ) -> ProviderQueryEvidence {
        ProviderQueryEvidence::new(
            proof_class,
            producers,
            SemanticProvenance::Known(Provenance::ExactAst),
            SemanticConfidence::Known(Confidence::High),
            SemanticFreshness::Fresh,
            SourceGeneration::known("document-1"),
            SourceGeneration::known("workspace-1"),
            None,
            None,
            SemanticReasonCode::ExactSource,
            facts,
            Vec::new(),
            Vec::new(),
        )
    }

    #[test]
    fn request_validation_preserves_unknown_but_rejects_malformed_identities() {
        let request = ProviderQueryRequest::new(
            ProviderSurface::Definition,
            "textDocument/definition",
            ProviderQueryKind::Declaration,
            ProviderQuerySubject::Symbol("target".to_string()),
            context(),
        );
        assert!(request.is_well_formed());

        let mut empty_symbol = request.clone();
        empty_symbol.subject = ProviderQuerySubject::Symbol(String::new());
        assert!(!empty_symbol.is_well_formed());

        let mut empty_request_class = request.clone();
        empty_request_class.request_class.clear();
        assert!(!empty_request_class.is_well_formed());

        let mut malformed_root = request.clone();
        malformed_root.context.root_identity = ProviderIdentity::known(" ");
        assert!(!malformed_root.is_well_formed());

        let mut unknown_root = request;
        unknown_root.context.root_identity = ProviderIdentity::Unknown;
        assert!(unknown_root.is_well_formed());
    }

    #[test]
    fn exact_empty_is_distinct_from_unavailable() {
        let exact_empty = ProviderQueryResult::<SemanticFactEnvelope>::exact(
            Vec::new(),
            evidence(ProviderProofClass::ExactRead, Vec::new(), Vec::new()),
        );
        assert_eq!(exact_empty.values().map(|values| values.len()), Some(0));
        assert!(exact_empty.is_exact_empty());
        assert!(exact_empty.is_consistent());

        let unavailable = ProviderQueryResult::<SemanticFactEnvelope>::unavailable(evidence(
            ProviderProofClass::RefusalOnly,
            Vec::new(),
            Vec::new(),
        ));
        assert_eq!(unavailable.values(), None);
        assert_eq!(unavailable.outcome(), ProviderQueryOutcome::Unavailable);
        assert!(!unavailable.is_exact_empty());
        assert!(unavailable.is_consistent());
    }

    #[test]
    fn compiler_producer_does_not_upgrade_fallback_proof() {
        let fact = exact_fact(1, SemanticProducer::PirA);
        let fallback = ProviderQueryResult::fallback(
            vec![fact.clone()],
            evidence(ProviderProofClass::FallbackOnly, Vec::new(), vec![fact]),
        );

        assert_eq!(fallback.outcome(), ProviderQueryOutcome::Fallback);
        assert_eq!(fallback.evidence().producers(), &[SemanticProducer::PirA]);
        assert!(fallback.is_consistent());
    }

    #[test]
    fn no_value_outcomes_remain_distinct_and_consistent() {
        let constructors: [
            fn(ProviderQueryEvidence) -> ProviderQueryResult<SemanticFactEnvelope>;
            8
        ] = [
            ProviderQueryResult::<SemanticFactEnvelope>::refused,
            ProviderQueryResult::<SemanticFactEnvelope>::stale,
            ProviderQueryResult::<SemanticFactEnvelope>::dynamic,
            ProviderQueryResult::<SemanticFactEnvelope>::ambiguous,
            ProviderQueryResult::<SemanticFactEnvelope>::unavailable,
            ProviderQueryResult::<SemanticFactEnvelope>::cancelled,
            ProviderQueryResult::<SemanticFactEnvelope>::deadline_exceeded,
            ProviderQueryResult::<SemanticFactEnvelope>::error,
        ];

        let outcomes = [
            ProviderQueryOutcome::Refused,
            ProviderQueryOutcome::Stale,
            ProviderQueryOutcome::Dynamic,
            ProviderQueryOutcome::Ambiguous,
            ProviderQueryOutcome::Unavailable,
            ProviderQueryOutcome::Cancelled,
            ProviderQueryOutcome::DeadlineExceeded,
            ProviderQueryOutcome::Error,
        ];

        for (constructor, expected) in constructors.into_iter().zip(outcomes) {
            let result = constructor(evidence(
                ProviderProofClass::RefusalOnly,
                Vec::new(),
                Vec::new(),
            ));
            assert_eq!(result.outcome(), expected);
            assert_eq!(result.values(), None);
            assert!(result.is_consistent());
        }
    }

    #[test]
    fn evidence_serialization_is_deterministic() -> Result<(), serde_json::Error> {
        let first_fact = exact_fact(1, SemanticProducer::Parser);
        let second_fact = exact_fact(2, SemanticProducer::PirA);
        let first_trace = ProviderFactTrace::new(
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
        let second_trace = ProviderFactTrace::new(
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

        let left = ProviderQueryEvidence::new(
            ProviderProofClass::QualifiedRead,
            vec![
                SemanticProducer::PirA,
                SemanticProducer::Parser,
                SemanticProducer::PirA,
            ],
            SemanticProvenance::Known(Provenance::ExactAst),
            SemanticConfidence::Known(Confidence::High),
            SemanticFreshness::Fresh,
            SourceGeneration::known("document-1"),
            SourceGeneration::known("workspace-1"),
            None,
            None,
            SemanticReasonCode::GeneratedFromSource,
            vec![second_fact.clone(), first_fact.clone()],
            vec![second_trace.clone(), first_trace.clone()],
            vec!["z".to_string(), "a".to_string(), "a".to_string(), String::new()],
        );
        let right = ProviderQueryEvidence::new(
            ProviderProofClass::QualifiedRead,
            vec![SemanticProducer::Parser, SemanticProducer::PirA],
            SemanticProvenance::Known(Provenance::ExactAst),
            SemanticConfidence::Known(Confidence::High),
            SemanticFreshness::Fresh,
            SourceGeneration::known("document-1"),
            SourceGeneration::known("workspace-1"),
            None,
            None,
            SemanticReasonCode::GeneratedFromSource,
            vec![first_fact, second_fact],
            vec![first_trace, second_trace],
            vec!["a".to_string(), "z".to_string()],
        );

        assert_eq!(serde_json::to_string(&left)?, serde_json::to_string(&right)?);
        assert_eq!(left.limitations(), ["a".to_string(), "z".to_string()]);
        Ok(())
    }

    struct UnavailablePort;

    impl ProviderSemanticPort for UnavailablePort {
        fn query(
            &self,
            _request: &ProviderQueryRequest,
        ) -> ProviderQueryResult<SemanticFactEnvelope> {
            ProviderQueryResult::unavailable(evidence(
                ProviderProofClass::RefusalOnly,
                Vec::new(),
                Vec::new(),
            ))
        }
    }

    #[test]
    fn port_trait_returns_typed_non_lsp_result() {
        let request = ProviderQueryRequest::new(
            ProviderSurface::Hover,
            "textDocument/hover",
            ProviderQueryKind::Declaration,
            ProviderQuerySubject::Position { file_id: FileId(1), byte_offset: 2 },
            context(),
        );
        let result = UnavailablePort.query(&request);
        assert_eq!(result.outcome(), ProviderQueryOutcome::Unavailable);
        assert!(result.is_consistent());
    }
}
