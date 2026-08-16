use perl_semantic_facts::{
    BoundaryDisposition, BoundaryKind, BoundaryLink, Confidence, ProviderFactTrace,
    ProviderFallbackState, SemanticConfidence, SemanticFactEnvelope, SemanticFactKind,
    SemanticFactStatus, SemanticFreshness, SemanticProducer, SemanticProvenance,
    SemanticReasonCode, SourceAnchor, SourceGeneration,
};
use serde::Serialize;
use std::cmp::Ordering;
use std::collections::BTreeSet;

use super::{
    ProviderCancellationState, ProviderCompletenessAuthorityReceipt, ProviderCompletenessGrant,
    ProviderQueryContractError, ProviderQueryControl, ProviderQueryDeadline, ProviderQueryFact,
    ProviderQueryKind, ProviderQueryRequest, ProviderQuerySubject, facts_are_related,
    semantic_provenance_is_exact,
};

/// Whether the result used the primary semantic path or an explicit fallback.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum ProviderResultPath {
    /// Primary producer path.
    Primary,
    /// Explicit compatibility or legacy fallback path.
    Fallback,
}

/// Terminal state claimed by an unchecked provider draft.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum ProviderQueryTerminalState {
    /// Query completed without cancellation, deadline expiry, or instrument failure.
    Completed,
    /// Live cancellation was observed.
    Cancelled,
    /// Live deadline expiry was observed.
    DeadlineExceeded,
    /// Product or instrument execution failed.
    Failed,
}

/// Caller-owned control observation captured after the provider returns a draft.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct ProviderQueryControlObservation {
    cancelled: bool,
    deadline_expired: bool,
}

impl ProviderQueryControlObservation {
    fn capture(request: &ProviderQueryRequest, control: &dyn ProviderQueryControl) -> Self {
        Self {
            cancelled: request.context.cancellation == ProviderCancellationState::Cancelled
                || control.is_cancelled(),
            deadline_expired: request.context.deadline == ProviderQueryDeadline::Expired
                || control.deadline_expired(),
        }
    }

    /// Whether cancellation was observed at admission or result validation.
    #[must_use]
    pub const fn cancelled(&self) -> bool {
        self.cancelled
    }

    /// Whether deadline expiry was observed at admission or result validation.
    #[must_use]
    pub const fn deadline_expired(&self) -> bool {
        self.deadline_expired
    }
}

/// Whether a checked result carries exact denominator authority.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum ProviderEvidenceCompleteness {
    /// A separate request-bound completeness grant is present.
    Complete,
    /// No completeness claim is retained.
    NotClaimed,
}

/// Proof and safety class derived from a checked provider result.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum ProviderProofClass {
    /// Current request-bound facts support a read-only exact answer.
    ExactRead,
    /// Current facts support a qualified read-only answer.
    QualifiedRead,
    /// Values came through an explicit fallback.
    FallbackOnly,
    /// Evidence supports refusal or another no-value outcome.
    RefusalOnly,
    /// Product or instrument failure prevents semantic proof.
    Unknown,
}

/// Query-level outcome visible to provider policy.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum ProviderQueryOutcome {
    /// Current evidence supports an exact result. No value facts means exact empty.
    Exact,
    /// Current evidence supports a useful qualified result.
    Degraded,
    /// An explicit fallback supplied the result.
    Fallback,
    /// Policy safely refused to return a value.
    Refused,
    /// Relevant facts belong to an older generation.
    Stale,
    /// Runtime-dynamic behavior prevents a static value.
    Dynamic,
    /// Multiple concrete candidates prevent one authoritative value.
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

/// Non-authorizing metadata supplied by an unchecked provider draft.
///
/// Producers, generations, provenance, confidence, freshness, completeness,
/// and live-control truth are derived or observed by the checked boundary.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderQueryEvidenceInput {
    result_path: ProviderResultPath,
    boundary: Option<BoundaryLink>,
    semantic_reason: SemanticReasonCode,
    traces: Vec<ProviderFactTrace>,
    limitations: Vec<String>,
    terminal_state: ProviderQueryTerminalState,
}

impl ProviderQueryEvidenceInput {
    /// Construct and canonicalize non-authorizing evidence metadata.
    #[must_use]
    pub fn new(
        result_path: ProviderResultPath,
        boundary: Option<BoundaryLink>,
        semantic_reason: SemanticReasonCode,
        mut traces: Vec<ProviderFactTrace>,
        mut limitations: Vec<String>,
        terminal_state: ProviderQueryTerminalState,
    ) -> Self {
        traces.sort_by(compare_traces);
        traces.dedup();
        limitations.retain(|limitation| !limitation.trim().is_empty());
        limitations.sort();
        limitations.dedup();
        Self { result_path, boundary, semantic_reason, traces, limitations, terminal_state }
    }

    /// Primary completed evidence with no boundary or limitation.
    #[must_use]
    pub fn primary_completed() -> Self {
        Self::new(
            ProviderResultPath::Primary,
            None,
            SemanticReasonCode::ExactSource,
            Vec::new(),
            Vec::new(),
            ProviderQueryTerminalState::Completed,
        )
    }
}

/// Unchecked provider output. It is not safe to consume until the caller-owned
/// execution boundary validates it against the original request and live control.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderQueryResultDraft {
    outcome: ProviderQueryOutcome,
    facts: Vec<ProviderQueryFact>,
    completeness: Option<ProviderCompletenessGrant>,
    evidence: ProviderQueryEvidenceInput,
}

impl ProviderQueryResultDraft {
    /// Construct an unchecked provider draft.
    #[must_use]
    pub fn new(
        outcome: ProviderQueryOutcome,
        facts: Vec<ProviderQueryFact>,
        completeness: Option<ProviderCompletenessGrant>,
        evidence: ProviderQueryEvidenceInput,
    ) -> Self {
        Self { outcome, facts, completeness, evidence }
    }
}

/// Canonical checked evidence attached to one provider result.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderQueryEvidence {
    proof_class: ProviderProofClass,
    completeness: ProviderEvidenceCompleteness,
    completeness_authority: Option<ProviderCompletenessAuthorityReceipt>,
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
    control_observation: ProviderQueryControlObservation,
    result_path: ProviderResultPath,
}

impl ProviderQueryEvidence {
    /// Derived proof class.
    #[must_use]
    pub const fn proof_class(&self) -> ProviderProofClass {
        self.proof_class
    }

    /// Exact denominator status.
    #[must_use]
    pub const fn completeness(&self) -> ProviderEvidenceCompleteness {
        self.completeness
    }

    /// Concrete denominator authority retained separately from fact producers.
    #[must_use]
    pub fn completeness_authority(&self) -> Option<&ProviderCompletenessAuthorityReceipt> {
        self.completeness_authority.as_ref()
    }

    /// Deterministically ordered producer set derived only from canonical facts.
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

    /// Document generation bound to the request.
    #[must_use]
    pub fn document_generation(&self) -> &SourceGeneration {
        &self.document_generation
    }

    /// Workspace/model generation bound to the request.
    #[must_use]
    pub fn workspace_generation(&self) -> &SourceGeneration {
        &self.workspace_generation
    }

    /// Primary source anchor, when one exists.
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

    /// Provider-local source traces.
    #[must_use]
    pub fn traces(&self) -> &[ProviderFactTrace] {
        &self.traces
    }

    /// Deterministically ordered limitations.
    #[must_use]
    pub fn limitations(&self) -> &[String] {
        &self.limitations
    }

    /// Terminal claim retained from the provider draft after validation.
    #[must_use]
    pub const fn terminal_state(&self) -> ProviderQueryTerminalState {
        self.terminal_state
    }

    /// Caller-owned live-control observation used to validate the terminal claim.
    #[must_use]
    pub const fn control_observation(&self) -> ProviderQueryControlObservation {
        self.control_observation
    }

    /// Primary or fallback execution path.
    #[must_use]
    pub const fn result_path(&self) -> ProviderResultPath {
        self.result_path
    }
}

/// Request-bound checked provider query result.
///
/// The result intentionally does not implement `Deserialize`; raw receipt input
/// and unchecked provider drafts cannot create impossible combinations.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderQueryResult {
    request: ProviderQueryRequest,
    outcome: ProviderQueryOutcome,
    facts: Vec<ProviderQueryFact>,
    evidence: ProviderQueryEvidence,
}
impl ProviderQueryResult {
    fn try_from_draft(
        request: &ProviderQueryRequest,
        control: &dyn ProviderQueryControl,
        draft: ProviderQueryResultDraft,
    ) -> Result<Self, ProviderQueryContractError> {
        if !request.is_well_formed() {
            return Err(ProviderQueryContractError::MalformedRequest);
        }
        let ProviderQueryResultDraft { outcome, mut facts, completeness, evidence: input } = draft;
        facts.sort_by_key(|fact| fact.envelope().fact_id);
        reject_duplicate_fact_ids(&facts)?;
        validate_fact_subjects(request, &facts)?;
        validate_value_fact_kinds(request, &facts)?;
        validate_trace_surfaces(request, &input.traces)?;
        if completeness.as_ref().is_some_and(|grant| !grant.matches(request)) {
            return Err(ProviderQueryContractError::InvalidCompletenessGrant);
        }
        let control_observation = ProviderQueryControlObservation::capture(request, control);
        validate_terminal_claim(outcome, input.terminal_state, control_observation)?;

        let evidence = build_evidence(
            request,
            outcome,
            &facts,
            completeness.as_ref(),
            input,
            control_observation,
        );
        let result = Self { request: request.clone(), outcome, facts, evidence };
        result.validate_internal(completeness.as_ref())?;
        Ok(result)
    }

    /// Original request bound to this result.
    #[must_use]
    pub const fn request(&self) -> &ProviderQueryRequest {
        &self.request
    }

    /// Query-level outcome.
    #[must_use]
    pub const fn outcome(&self) -> ProviderQueryOutcome {
        self.outcome
    }

    /// Canonical fact set supplying selection, values, and evidence.
    #[must_use]
    pub fn facts(&self) -> &[ProviderQueryFact] {
        &self.facts
    }

    /// Facts that selected the target at the request subject.
    pub fn selector_facts(&self) -> impl Iterator<Item = &SemanticFactEnvelope> {
        self.facts.iter().filter(|fact| fact.role().is_selector()).map(ProviderQueryFact::envelope)
    }

    /// Facts returned to the provider.
    pub fn value_facts(&self) -> impl Iterator<Item = &SemanticFactEnvelope> {
        self.facts.iter().filter(|fact| fact.role().is_value()).map(ProviderQueryFact::envelope)
    }

    /// Facts used only to support a qualified or no-value outcome.
    pub fn supporting_facts(&self) -> impl Iterator<Item = &SemanticFactEnvelope> {
        self.facts
            .iter()
            .filter(|fact| fact.role().is_supporting())
            .map(ProviderQueryFact::envelope)
    }

    /// Checked evidence derived from the same facts, request, and caller control.
    #[must_use]
    pub const fn evidence(&self) -> &ProviderQueryEvidence {
        &self.evidence
    }

    /// Whether this is an authoritative exact empty result.
    #[must_use]
    pub fn is_exact_empty(&self) -> bool {
        self.outcome == ProviderQueryOutcome::Exact && self.value_facts().next().is_none()
    }

    /// Revalidate this retained result against the intended request.
    pub fn validate_against(
        &self,
        request: &ProviderQueryRequest,
    ) -> Result<(), ProviderQueryContractError> {
        if &self.request != request {
            return Err(ProviderQueryContractError::RequestBindingMismatch);
        }
        let completeness_present =
            self.evidence.completeness == ProviderEvidenceCompleteness::Complete;
        self.validate_internal_presence(completeness_present)
    }

    fn validate_internal(
        &self,
        completeness: Option<&ProviderCompletenessGrant>,
    ) -> Result<(), ProviderQueryContractError> {
        self.validate_internal_presence(completeness.is_some())
    }

    fn validate_internal_presence(
        &self,
        completeness_present: bool,
    ) -> Result<(), ProviderQueryContractError> {
        validate_terminal_claim(
            self.outcome,
            self.evidence.terminal_state,
            self.evidence.control_observation,
        )?;

        let value_count = self.facts.iter().filter(|fact| fact.role().is_value()).count();
        let supporting_count = self.facts.iter().filter(|fact| fact.role().is_supporting()).count();
        let candidate_count = distinct_candidate_count(&self.facts);
        let any_stale =
            self.facts.iter().any(|fact| fact.envelope().status() == SemanticFactStatus::Stale);
        let any_refused =
            self.facts.iter().any(|fact| fact.envelope().status() == SemanticFactStatus::Refused);
        let has_dynamic_boundary = self
            .evidence
            .boundary
            .as_ref()
            .is_some_and(|boundary| is_dynamic_boundary(boundary.kind))
            || self.facts.iter().any(|fact| {
                fact.envelope()
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
                fact.envelope()
                    .boundary
                    .as_ref()
                    .is_some_and(|boundary| boundary.disposition == BoundaryDisposition::Refuse)
            });
        let all_current = self.facts.iter().all(|fact| fact.is_generation_current(&self.request));
        let all_exact = self.facts.iter().all(|fact| fact_is_exact_grade(fact, &self.request));

        if self.outcome == ProviderQueryOutcome::Exact
            && value_count == 0
            && (!completeness_present
                || self.evidence.completeness != ProviderEvidenceCompleteness::Complete
                || self.evidence.completeness_authority.is_none()
                || !self.evidence.producers.is_empty())
        {
            return Err(ProviderQueryContractError::MissingCompletenessGrant);
        }

        match self.outcome {
            ProviderQueryOutcome::Exact => {
                if self.evidence.proof_class != ProviderProofClass::ExactRead
                    || self.evidence.terminal_state != ProviderQueryTerminalState::Completed
                    || self.evidence.result_path != ProviderResultPath::Primary
                    || !self.request.context.is_exact_ready()
                    || !self.evidence.limitations.is_empty()
                    || self.evidence.boundary.is_some()
                    || supporting_count != 0
                    || !all_exact
                    || !semantic_provenance_is_exact(self.evidence.provenance)
                    || self.evidence.confidence != SemanticConfidence::Known(Confidence::High)
                    || self.evidence.freshness != SemanticFreshness::Fresh
                {
                    return invalid(self.outcome);
                }
                if value_count == 0 {
                    if !completeness_present
                        || self.evidence.completeness != ProviderEvidenceCompleteness::Complete
                        || self.evidence.completeness_authority.is_none()
                        || !self.evidence.producers.is_empty()
                    {
                        return Err(ProviderQueryContractError::MissingCompletenessGrant);
                    }
                } else if completeness_present || self.evidence.completeness_authority.is_some() {
                    return Err(ProviderQueryContractError::UnexpectedCompletenessGrant);
                }
            }
            ProviderQueryOutcome::Degraded => {
                if completeness_present
                    || self.evidence.proof_class != ProviderProofClass::QualifiedRead
                    || self.evidence.terminal_state != ProviderQueryTerminalState::Completed
                    || self.evidence.result_path != ProviderResultPath::Primary
                    || value_count == 0
                    || !self.request.context.is_degraded_ready()
                    || !all_current
                    || any_stale
                    || any_refused
                    || (self
                        .facts
                        .iter()
                        .all(|fact| fact.envelope().status() == SemanticFactStatus::Exact)
                        && self.evidence.limitations.is_empty()
                        && self.evidence.boundary.is_none()
                        && self.request.context.readiness_state
                            != super::ProviderReadinessState::ReadyLimited)
                {
                    return invalid(self.outcome);
                }
            }
            ProviderQueryOutcome::Fallback => {
                if completeness_present
                    || self.evidence.proof_class != ProviderProofClass::FallbackOnly
                    || self.evidence.terminal_state != ProviderQueryTerminalState::Completed
                    || self.evidence.result_path != ProviderResultPath::Fallback
                    || value_count == 0
                    || !self.request.context.is_fallback_ready()
                    || !all_current
                    || any_stale
                    || any_refused
                    || !self
                        .evidence
                        .traces
                        .iter()
                        .any(|trace| trace.fallback_state == ProviderFallbackState::Fallback)
                {
                    return invalid(self.outcome);
                }
            }
            ProviderQueryOutcome::Refused => {
                require_no_values(value_count, self.outcome)?;
                if completeness_present
                    || self.evidence.proof_class != ProviderProofClass::RefusalOnly
                    || self.evidence.terminal_state != ProviderQueryTerminalState::Completed
                    || self.evidence.result_path != ProviderResultPath::Primary
                    || !(any_refused
                        || has_refuse_boundary
                        || self.evidence.semantic_reason == SemanticReasonCode::UnsupportedEffect)
                {
                    return invalid(self.outcome);
                }
            }
            ProviderQueryOutcome::Stale => {
                require_no_values(value_count, self.outcome)?;
                if completeness_present
                    || self.evidence.proof_class != ProviderProofClass::RefusalOnly
                    || self.evidence.terminal_state != ProviderQueryTerminalState::Completed
                    || self.evidence.result_path != ProviderResultPath::Primary
                    || self.evidence.semantic_reason != SemanticReasonCode::StaleDependency
                    || !(any_stale
                        || self.request.context.readiness_state
                            == super::ProviderReadinessState::Stale)
                {
                    return invalid(self.outcome);
                }
            }
            ProviderQueryOutcome::Dynamic => {
                require_no_values(value_count, self.outcome)?;
                if completeness_present
                    || self.evidence.proof_class != ProviderProofClass::RefusalOnly
                    || self.evidence.terminal_state != ProviderQueryTerminalState::Completed
                    || self.evidence.result_path != ProviderResultPath::Primary
                    || self.evidence.semantic_reason != SemanticReasonCode::DynamicValue
                    || !has_dynamic_boundary
                {
                    return invalid(self.outcome);
                }
            }
            ProviderQueryOutcome::Ambiguous => {
                require_no_values(value_count, self.outcome)?;
                if completeness_present
                    || self.evidence.proof_class != ProviderProofClass::RefusalOnly
                    || self.evidence.terminal_state != ProviderQueryTerminalState::Completed
                    || self.evidence.result_path != ProviderResultPath::Primary
                    || candidate_count < 2
                {
                    return invalid(self.outcome);
                }
            }
            ProviderQueryOutcome::Unavailable => {
                require_no_values(value_count, self.outcome)?;
                if completeness_present
                    || self.evidence.proof_class != ProviderProofClass::RefusalOnly
                    || self.evidence.terminal_state != ProviderQueryTerminalState::Completed
                    || self.evidence.result_path != ProviderResultPath::Primary
                    || !(matches!(
                        self.request.context.readiness_state,
                        super::ProviderReadinessState::Unavailable
                            | super::ProviderReadinessState::Failed
                    ) || self
                        .evidence
                        .traces
                        .iter()
                        .any(|trace| trace.fallback_state == ProviderFallbackState::Unavailable))
                {
                    return invalid(self.outcome);
                }
            }
            ProviderQueryOutcome::Cancelled => {
                require_no_values(value_count, self.outcome)?;
                if completeness_present
                    || self.evidence.proof_class != ProviderProofClass::RefusalOnly
                    || self.evidence.terminal_state != ProviderQueryTerminalState::Cancelled
                {
                    return invalid(self.outcome);
                }
            }
            ProviderQueryOutcome::DeadlineExceeded => {
                require_no_values(value_count, self.outcome)?;
                if completeness_present
                    || self.evidence.proof_class != ProviderProofClass::RefusalOnly
                    || self.evidence.terminal_state != ProviderQueryTerminalState::DeadlineExceeded
                {
                    return invalid(self.outcome);
                }
            }
            ProviderQueryOutcome::Error => {
                require_no_values(value_count, self.outcome)?;
                if completeness_present
                    || self.evidence.proof_class != ProviderProofClass::Unknown
                    || self.evidence.terminal_state != ProviderQueryTerminalState::Failed
                    || self.evidence.result_path != ProviderResultPath::Primary
                {
                    return invalid(self.outcome);
                }
            }
        }
        Ok(())
    }
}
/// Provider-facing semantic fact port. Implementations return an unchecked draft;
/// only [`execute_provider_query`] can produce the checked result consumed by policy.
pub trait ProviderSemanticPort {
    /// Query canonical semantic facts for one request.
    fn query(
        &self,
        request: &ProviderQueryRequest,
        control: &dyn ProviderQueryControl,
    ) -> Result<ProviderQueryResultDraft, ProviderQueryContractError>;
}

/// Execute one provider query and validate its draft against the original request
/// and the caller-owned live cancellation/deadline control.
pub fn execute_provider_query(
    port: &dyn ProviderSemanticPort,
    request: &ProviderQueryRequest,
    control: &dyn ProviderQueryControl,
) -> Result<ProviderQueryResult, ProviderQueryContractError> {
    let draft = port.query(request, control)?;
    ProviderQueryResult::try_from_draft(request, control, draft)
}
fn build_evidence(
    request: &ProviderQueryRequest,
    outcome: ProviderQueryOutcome,
    facts: &[ProviderQueryFact],
    completeness: Option<&ProviderCompletenessGrant>,
    mut input: ProviderQueryEvidenceInput,
    control_observation: ProviderQueryControlObservation,
) -> ProviderQueryEvidence {
    let mut producers: Vec<_> = facts.iter().map(|fact| fact.envelope().producer).collect();
    producers.retain(|producer| *producer != SemanticProducer::Unknown);
    producers.sort();
    producers.dedup();

    let provenance = summarize_provenance(facts, completeness);
    let confidence = summarize_confidence(facts, completeness);
    let freshness = summarize_freshness(facts, completeness);
    let primary_anchor = facts
        .iter()
        .find(|fact| fact.role().is_value())
        .or_else(|| facts.iter().find(|fact| fact.role().is_selector()))
        .map(|fact| fact.envelope().anchor);
    let boundary =
        facts.iter().find_map(|fact| fact.envelope().boundary.clone()).or(input.boundary.take());
    let semantic_reason = summarize_reason(outcome, facts, input.semantic_reason);

    ProviderQueryEvidence {
        proof_class: proof_for_outcome(outcome),
        completeness: if completeness.is_some() {
            ProviderEvidenceCompleteness::Complete
        } else {
            ProviderEvidenceCompleteness::NotClaimed
        },
        completeness_authority: completeness.map(|grant| grant.authority().clone()),
        producers,
        provenance,
        confidence,
        freshness,
        document_generation: request.context.document_generation.clone(),
        workspace_generation: request.context.workspace_generation.clone(),
        primary_anchor,
        boundary,
        semantic_reason,
        traces: input.traces,
        limitations: input.limitations,
        terminal_state: input.terminal_state,
        control_observation,
        result_path: input.result_path,
    }
}
fn validate_fact_subjects(
    request: &ProviderQueryRequest,
    facts: &[ProviderQueryFact],
) -> Result<(), ProviderQueryContractError> {
    if facts.is_empty() {
        return Ok(());
    }
    let direct: Vec<_> =
        facts.iter().filter(|fact| fact.matches_subject_directly(&request.subject)).collect();
    if direct.is_empty() {
        return Err(ProviderQueryContractError::FactDoesNotMatchSubject(
            facts[0].envelope().fact_id,
        ));
    }

    if matches!(request.subject, ProviderQuerySubject::Position { .. })
        && !direct.iter().any(|fact| fact.role().is_selector())
    {
        return Err(ProviderQueryContractError::MissingPositionSelector);
    }

    for fact in facts {
        if fact.matches_subject_directly(&request.subject)
            || direct.iter().any(|anchor| facts_are_related(fact, anchor))
        {
            continue;
        }
        // Position-selected values receive the more precise relation error below.
        // Other facts must match the subject directly or relate to one of its
        // canonical selector facts.
        if matches!(request.subject, ProviderQuerySubject::Position { .. })
            && fact.role().is_value()
        {
            continue;
        }
        return Err(ProviderQueryContractError::FactDoesNotMatchSubject(fact.envelope().fact_id));
    }

    if matches!(request.subject, ProviderQuerySubject::Position { .. }) {
        let selectors: Vec<_> =
            direct.iter().copied().filter(|fact| fact.role().is_selector()).collect();
        for fact in facts.iter().filter(|fact| fact.role().is_value()) {
            if fact.matches_subject_directly(&request.subject)
                || selectors.iter().any(|selector| facts_are_related(fact, selector))
            {
                continue;
            }
            return Err(ProviderQueryContractError::UnrelatedPositionValue(
                fact.envelope().fact_id,
            ));
        }
    }
    Ok(())
}

fn validate_value_fact_kinds(
    request: &ProviderQueryRequest,
    facts: &[ProviderQueryFact],
) -> Result<(), ProviderQueryContractError> {
    for fact in facts.iter().filter(|fact| fact.role().is_value()) {
        if value_kind_matches(&request.kind, fact.envelope().kind) {
            continue;
        }
        return Err(ProviderQueryContractError::FactKindDoesNotMatchRequest(
            fact.envelope().fact_id,
        ));
    }
    Ok(())
}

fn validate_trace_surfaces(
    request: &ProviderQueryRequest,
    traces: &[ProviderFactTrace],
) -> Result<(), ProviderQueryContractError> {
    if traces.iter().any(|trace| trace.surface != request.surface) {
        return Err(ProviderQueryContractError::TraceSurfaceMismatch);
    }
    Ok(())
}

fn reject_duplicate_fact_ids(
    facts: &[ProviderQueryFact],
) -> Result<(), ProviderQueryContractError> {
    let mut seen = BTreeSet::new();
    for fact in facts {
        if !seen.insert(fact.envelope().fact_id) {
            return Err(ProviderQueryContractError::DuplicateFactId(fact.envelope().fact_id));
        }
    }
    Ok(())
}

fn distinct_candidate_count(facts: &[ProviderQueryFact]) -> usize {
    facts
        .iter()
        .filter(|fact| {
            (fact.role().is_selector() || fact.role().is_supporting())
                && fact.envelope().kind != SemanticFactKind::Boundary
        })
        .filter_map(|fact| fact.envelope().entity_id)
        .collect::<BTreeSet<_>>()
        .len()
}

fn validate_terminal_claim(
    outcome: ProviderQueryOutcome,
    terminal_state: ProviderQueryTerminalState,
    observation: ProviderQueryControlObservation,
) -> Result<(), ProviderQueryContractError> {
    let valid = match outcome {
        ProviderQueryOutcome::Cancelled => {
            terminal_state == ProviderQueryTerminalState::Cancelled && observation.cancelled
        }
        ProviderQueryOutcome::DeadlineExceeded => {
            terminal_state == ProviderQueryTerminalState::DeadlineExceeded
                && !observation.cancelled
                && observation.deadline_expired
        }
        ProviderQueryOutcome::Error => {
            terminal_state == ProviderQueryTerminalState::Failed
                && !observation.cancelled
                && !observation.deadline_expired
        }
        _ => {
            terminal_state == ProviderQueryTerminalState::Completed
                && !observation.cancelled
                && !observation.deadline_expired
        }
    };
    if valid { Ok(()) } else { invalid(outcome) }
}

fn value_kind_matches(kind: &ProviderQueryKind, fact_kind: SemanticFactKind) -> bool {
    match kind {
        ProviderQueryKind::Declaration => {
            matches!(fact_kind, SemanticFactKind::Declaration | SemanticFactKind::Module)
        }
        ProviderQueryKind::References { include_declaration } => {
            fact_kind == SemanticFactKind::Occurrence
                || (*include_declaration
                    && matches!(
                        fact_kind,
                        SemanticFactKind::Declaration | SemanticFactKind::Module
                    ))
        }
        ProviderQueryKind::Visibility => matches!(
            fact_kind,
            SemanticFactKind::Import
                | SemanticFactKind::Module
                | SemanticFactKind::Declaration
                | SemanticFactKind::Occurrence
        ),
        ProviderQueryKind::ScopeBindings => {
            matches!(fact_kind, SemanticFactKind::Declaration | SemanticFactKind::Occurrence)
        }
        ProviderQueryKind::Boundaries => fact_kind == SemanticFactKind::Boundary,
        ProviderQueryKind::Readiness => false,
    }
}

fn fact_is_exact_grade(fact: &ProviderQueryFact, request: &ProviderQueryRequest) -> bool {
    fact.is_generation_current(request)
        && fact.envelope().status() == SemanticFactStatus::Exact
        && semantic_provenance_is_exact(fact.envelope().provenance)
        && fact.envelope().confidence == SemanticConfidence::Known(Confidence::High)
        && fact.envelope().freshness == SemanticFreshness::Fresh
        && fact.envelope().boundary.is_none()
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
fn summarize_provenance(
    facts: &[ProviderQueryFact],
    completeness: Option<&ProviderCompletenessGrant>,
) -> SemanticProvenance {
    let mut values = facts.iter().map(|fact| fact.envelope().provenance);
    let Some(first) = values.next() else {
        return completeness
            .map(ProviderCompletenessGrant::provenance)
            .unwrap_or(SemanticProvenance::Unknown);
    };
    if values.all(|value| value == first) { first } else { SemanticProvenance::Unknown }
}

fn summarize_confidence(
    facts: &[ProviderQueryFact],
    completeness: Option<&ProviderCompletenessGrant>,
) -> SemanticConfidence {
    let mut values = facts.iter().map(|fact| fact.envelope().confidence);
    let Some(first) = values.next() else {
        return completeness
            .map(ProviderCompletenessGrant::confidence)
            .unwrap_or(SemanticConfidence::Unknown);
    };
    if values.all(|value| value == first) { first } else { SemanticConfidence::Unknown }
}

fn summarize_freshness(
    facts: &[ProviderQueryFact],
    completeness: Option<&ProviderCompletenessGrant>,
) -> SemanticFreshness {
    if facts.iter().any(|fact| fact.envelope().freshness == SemanticFreshness::Stale) {
        return SemanticFreshness::Stale;
    }
    if facts.iter().any(|fact| fact.envelope().freshness == SemanticFreshness::Unknown) {
        return SemanticFreshness::Unknown;
    }
    if facts.is_empty() {
        return completeness
            .map(ProviderCompletenessGrant::freshness)
            .unwrap_or(SemanticFreshness::Unknown);
    }
    if facts.iter().all(|fact| fact.envelope().freshness == SemanticFreshness::Fresh) {
        SemanticFreshness::Fresh
    } else {
        SemanticFreshness::NotApplicable
    }
}

fn summarize_reason(
    outcome: ProviderQueryOutcome,
    facts: &[ProviderQueryFact],
    fallback: SemanticReasonCode,
) -> SemanticReasonCode {
    match outcome {
        ProviderQueryOutcome::Exact => SemanticReasonCode::ExactSource,
        ProviderQueryOutcome::Stale => SemanticReasonCode::StaleDependency,
        ProviderQueryOutcome::Dynamic => SemanticReasonCode::DynamicValue,
        ProviderQueryOutcome::Refused => facts
            .iter()
            .find(|fact| fact.envelope().status() == SemanticFactStatus::Refused)
            .map(|fact| fact.envelope().reason_code)
            .unwrap_or(fallback),
        ProviderQueryOutcome::Degraded | ProviderQueryOutcome::Fallback => facts
            .iter()
            .find(|fact| fact.envelope().reason_code != SemanticReasonCode::ExactSource)
            .map(|fact| fact.envelope().reason_code)
            .unwrap_or(fallback),
        _ => fallback,
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

fn require_no_values(
    value_count: usize,
    outcome: ProviderQueryOutcome,
) -> Result<(), ProviderQueryContractError> {
    if value_count == 0 { Ok(()) } else { invalid(outcome) }
}

fn invalid<T>(outcome: ProviderQueryOutcome) -> Result<T, ProviderQueryContractError> {
    Err(ProviderQueryContractError::InvalidOutcomeEvidence(outcome))
}
