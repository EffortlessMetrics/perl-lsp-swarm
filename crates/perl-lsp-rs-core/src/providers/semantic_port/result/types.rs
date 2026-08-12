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
        Self {
            result_path,
            boundary,
            semantic_reason,
            traces,
            limitations,
            terminal_state,
        }
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
        Self {
            outcome,
            facts,
            completeness,
            evidence,
        }
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
