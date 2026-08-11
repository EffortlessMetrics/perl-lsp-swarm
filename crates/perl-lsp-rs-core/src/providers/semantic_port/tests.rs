use super::*;
use perl_semantic_facts::{
    AnchorId, BoundaryDisposition, BoundaryKind, BoundaryLink, Confidence, EntityId, FactId,
    FileId, LifecyclePhase, Provenance, ProviderFactFreshness, ProviderFactSourceKind,
    ProviderFallbackState, ProviderSurface, ScopeId, SemanticConfidence, SemanticFactEnvelope,
    SemanticFactKind, SemanticFreshness, SemanticProducer, SemanticProvenance,
    SemanticReasonCode, SourceAnchor, SourceGeneration,
};
use std::error::Error;
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

#[allow(clippy::too_many_arguments)]
fn envelope(
    fact_id: u64,
    entity_id: Option<u64>,
    kind: SemanticFactKind,
    file_id: u64,
    start: u32,
    end: u32,
    generation: &str,
    producer: SemanticProducer,
    provenance: Provenance,
    confidence: Confidence,
    freshness: SemanticFreshness,
    package: Option<&str>,
    scope_id: Option<u64>,
    boundary: Option<BoundaryLink>,
    reason: SemanticReasonCode,
) -> SemanticFactEnvelope {
    SemanticFactEnvelope::new(
        FactId(fact_id),
        entity_id.map(EntityId),
        kind,
        SourceAnchor::new(Some(AnchorId(fact_id + 1000)), FileId(file_id), start, end),
        SourceGeneration::known(generation),
        scope_id.map(ScopeId),
        package.map(str::to_string),
        LifecyclePhase::Runtime,
        producer,
        SemanticProvenance::Known(provenance),
        SemanticConfidence::Known(confidence),
        freshness,
        boundary,
        Vec::new(),
        reason,
    )
}

fn exact_envelope(
    fact_id: u64,
    entity_id: u64,
    kind: SemanticFactKind,
    start: u32,
    end: u32,
    producer: SemanticProducer,
) -> SemanticFactEnvelope {
    envelope(
        fact_id,
        Some(entity_id),
        kind,
        1,
        start,
        end,
        "document-1",
        producer,
        Provenance::ExactAst,
        Confidence::High,
        SemanticFreshness::Fresh,
        Some("Example"),
        Some(9),
        None,
        SemanticReasonCode::ExactSource,
    )
}

fn fact(
    role: ProviderQueryFactRole,
    envelope: SemanticFactEnvelope,
    symbols: &[&str],
) -> Result<ProviderQueryFact, ProviderQueryContractError> {
    ProviderQueryFact::try_new(
        role,
        ProviderFactGenerationScope::Document,
        envelope,
        symbols.iter().map(|symbol| (*symbol).to_string()),
    )
}

fn primary(reason: SemanticReasonCode) -> ProviderQueryEvidenceInput {
    ProviderQueryEvidenceInput::new(
        ProviderResultPath::Primary,
        None,
        reason,
        Vec::new(),
        Vec::new(),
        ProviderQueryTerminalState::Completed,
    )
}

fn fallback_trace() -> perl_semantic_facts::ProviderFactTrace {
    perl_semantic_facts::ProviderFactTrace::new(
        ProviderSurface::Definition,
        ProviderFactSourceKind::LegacyWorkspace,
        Provenance::NameHeuristic,
        Confidence::Medium,
        ProviderFactFreshness::Fresh,
        ProviderFallbackState::Fallback,
        Some("fallback".to_string()),
        None,
        Some(1),
    )
}

#[test]
fn position_selector_can_return_a_declaration_elsewhere() -> Result<(), Box<dyn Error>> {
    let request = request(
        ProviderQueryKind::Declaration,
        ProviderQuerySubject::Position {
            file_id: FileId(1),
            byte_offset: 11,
        },
    );
    let selector = fact(
        ProviderQueryFactRole::Selector,
        exact_envelope(
            1,
            42,
            SemanticFactKind::Occurrence,
            10,
            14,
            SemanticProducer::Parser,
        ),
        &[],
    )?;
    let value = fact(
        ProviderQueryFactRole::Value,
        exact_envelope(
            2,
            42,
            SemanticFactKind::Declaration,
            100,
            120,
            SemanticProducer::WorkspaceIndex,
        ),
        &[],
    )?;

    let result = ProviderQueryResult::try_new(
        &request,
        ProviderQueryOutcome::Exact,
        vec![value, selector],
        None,
        primary(SemanticReasonCode::ExactSource),
    )?;
    assert_eq!(result.selector_facts().count(), 1);
    assert_eq!(result.value_facts().count(), 1);
    assert_eq!(
        result.evidence().producers(),
        &[SemanticProducer::Parser, SemanticProducer::WorkspaceIndex]
    );
    result.validate_against(&request)?;
    Ok(())
}

#[test]
fn unrelated_position_value_is_rejected() -> Result<(), Box<dyn Error>> {
    let request = request(
        ProviderQueryKind::Declaration,
        ProviderQuerySubject::Position {
            file_id: FileId(1),
            byte_offset: 11,
        },
    );
    let selector = fact(
        ProviderQueryFactRole::Selector,
        exact_envelope(
            1,
            42,
            SemanticFactKind::Occurrence,
            10,
            14,
            SemanticProducer::Parser,
        ),
        &[],
    )?;
    let value = fact(
        ProviderQueryFactRole::Value,
        exact_envelope(
            2,
            99,
            SemanticFactKind::Declaration,
            100,
            120,
            SemanticProducer::WorkspaceIndex,
        ),
        &[],
    )?;
    let result = ProviderQueryResult::try_new(
        &request,
        ProviderQueryOutcome::Exact,
        vec![selector, value],
        None,
        primary(SemanticReasonCode::ExactSource),
    );
    assert_eq!(
        result.err(),
        Some(ProviderQueryContractError::FactDoesNotMatchSubject(FactId(2)))
    );
    Ok(())
}

#[test]
fn exact_empty_requires_separate_exact_grade_completeness() -> Result<(), Box<dyn Error>> {
    let request = request(
        ProviderQueryKind::Declaration,
        ProviderQuerySubject::Symbol("missing".to_string()),
    );
    let missing = ProviderQueryResult::try_new(
        &request,
        ProviderQueryOutcome::Exact,
        Vec::new(),
        None,
        primary(SemanticReasonCode::ExactSource),
    );
    assert_eq!(
        missing.err(),
        Some(ProviderQueryContractError::MissingCompletenessGrant)
    );

    let invalid = ProviderCompletenessGrant::try_new(
        &request,
        [SemanticProducer::Parser],
        SemanticProvenance::Unknown,
        SemanticConfidence::Known(Confidence::High),
        SemanticFreshness::Fresh,
    );
    assert_eq!(
        invalid.err(),
        Some(ProviderQueryContractError::InvalidCompletenessGrant)
    );

    let grant = ProviderCompletenessGrant::try_new(
        &request,
        [SemanticProducer::Parser],
        SemanticProvenance::Known(Provenance::ExactAst),
        SemanticConfidence::Known(Confidence::High),
        SemanticFreshness::Fresh,
    )?;
    let exact = ProviderQueryResult::try_new(
        &request,
        ProviderQueryOutcome::Exact,
        Vec::new(),
        Some(grant),
        primary(SemanticReasonCode::ExactSource),
    )?;
    assert!(exact.is_exact_empty());
    assert_eq!(
        exact.evidence().completeness(),
        ProviderEvidenceCompleteness::Complete
    );
    assert_eq!(exact.evidence().producers(), &[SemanticProducer::Parser]);
    Ok(())
}

#[test]
fn completeness_is_bound_to_capability_and_generation() -> Result<(), Box<dyn Error>> {
    let declaration = request(
        ProviderQueryKind::Declaration,
        ProviderQuerySubject::Symbol("missing".to_string()),
    );
    let grant = ProviderCompletenessGrant::try_new(
        &declaration,
        [SemanticProducer::Parser],
        SemanticProvenance::Known(Provenance::ExactAst),
        SemanticConfidence::Known(Confidence::High),
        SemanticFreshness::Fresh,
    )?;
    let references = request(
        ProviderQueryKind::References {
            include_declaration: false,
        },
        ProviderQuerySubject::Symbol("missing".to_string()),
    );
    let result = ProviderQueryResult::try_new(
        &references,
        ProviderQueryOutcome::Exact,
        Vec::new(),
        Some(grant),
        primary(SemanticReasonCode::ExactSource),
    );
    assert_eq!(
        result.err(),
        Some(ProviderQueryContractError::InvalidCompletenessGrant)
    );
    Ok(())
}

#[test]
fn malformed_envelopes_are_rejected_before_outcome_selection() {
    let malformed = envelope(
        7,
        Some(42),
        SemanticFactKind::Declaration,
        1,
        20,
        10,
        "document-1",
        SemanticProducer::Parser,
        Provenance::ExactAst,
        Confidence::High,
        SemanticFreshness::Fresh,
        Some("Example"),
        None,
        None,
        SemanticReasonCode::ExactSource,
    );
    let result = ProviderQueryFact::from_envelope(
        ProviderQueryFactRole::Value,
        ProviderFactGenerationScope::Document,
        malformed,
    );
    assert_eq!(
        result.err(),
        Some(ProviderQueryContractError::MalformedFact(FactId(7)))
    );
}

#[test]
fn ambiguity_requires_two_concrete_candidates() -> Result<(), Box<dyn Error>> {
    let request = request(
        ProviderQueryKind::Declaration,
        ProviderQuerySubject::Position {
            file_id: FileId(1),
            byte_offset: 11,
        },
    );
    let first = fact(
        ProviderQueryFactRole::Selector,
        exact_envelope(
            1,
            42,
            SemanticFactKind::Occurrence,
            10,
            14,
            SemanticProducer::Parser,
        ),
        &[],
    )?;
    let one = ProviderQueryResult::try_new(
        &request,
        ProviderQueryOutcome::Ambiguous,
        vec![first.clone()],
        None,
        primary(SemanticReasonCode::Unknown),
    );
    assert_eq!(
        one.err(),
        Some(ProviderQueryContractError::InvalidOutcomeEvidence(
            ProviderQueryOutcome::Ambiguous
        ))
    );

    let second = fact(
        ProviderQueryFactRole::Selector,
        exact_envelope(
            2,
            99,
            SemanticFactKind::Occurrence,
            10,
            14,
            SemanticProducer::SemanticAnalyzer,
        ),
        &[],
    )?;
    let ambiguous = ProviderQueryResult::try_new(
        &request,
        ProviderQueryOutcome::Ambiguous,
        vec![second, first],
        None,
        primary(SemanticReasonCode::Unknown),
    )?;
    assert_eq!(ambiguous.selector_facts().count(), 2);
    Ok(())
}

#[test]
fn visibility_accepts_declarations_and_occurrences() -> Result<(), Box<dyn Error>> {
    let request = request(
        ProviderQueryKind::Visibility,
        ProviderQuerySubject::Symbol("visible".to_string()),
    );
    let declaration = fact(
        ProviderQueryFactRole::Value,
        exact_envelope(
            1,
            42,
            SemanticFactKind::Declaration,
            10,
            20,
            SemanticProducer::Parser,
        ),
        &["visible"],
    )?;
    let occurrence = fact(
        ProviderQueryFactRole::Value,
        exact_envelope(
            2,
            42,
            SemanticFactKind::Occurrence,
            30,
            40,
            SemanticProducer::SemanticAnalyzer,
        ),
        &["visible"],
    )?;
    let result = ProviderQueryResult::try_new(
        &request,
        ProviderQueryOutcome::Exact,
        vec![occurrence, declaration],
        None,
        primary(SemanticReasonCode::ExactSource),
    )?;
    assert_eq!(result.value_facts().count(), 2);
    Ok(())
}

#[test]
fn value_bearing_qualified_results_reject_cross_generation_facts() -> Result<(), Box<dyn Error>> {
    let request = request(
        ProviderQueryKind::Declaration,
        ProviderQuerySubject::Symbol("target".to_string()),
    );
    let generated = fact(
        ProviderQueryFactRole::Value,
        envelope(
            1,
            Some(42),
            SemanticFactKind::Declaration,
            1,
            10,
            20,
            "old-document",
            SemanticProducer::SemanticAnalyzer,
            Provenance::FrameworkSynthesis,
            Confidence::Medium,
            SemanticFreshness::Fresh,
            Some("Example"),
            None,
            None,
            SemanticReasonCode::GeneratedFromSource,
        ),
        &["target"],
    )?;
    let degraded = ProviderQueryResult::try_new(
        &request,
        ProviderQueryOutcome::Degraded,
        vec![generated.clone()],
        None,
        primary(SemanticReasonCode::GeneratedFromSource),
    );
    assert_eq!(
        degraded.err(),
        Some(ProviderQueryContractError::InvalidOutcomeEvidence(
            ProviderQueryOutcome::Degraded
        ))
    );

    let fallback = ProviderQueryResult::try_new(
        &request,
        ProviderQueryOutcome::Fallback,
        vec![generated],
        None,
        ProviderQueryEvidenceInput::new(
            ProviderResultPath::Fallback,
            None,
            SemanticReasonCode::CompatibilityBoundary,
            vec![fallback_trace()],
            Vec::new(),
            ProviderQueryTerminalState::Completed,
        ),
    );
    assert_eq!(
        fallback.err(),
        Some(ProviderQueryContractError::InvalidOutcomeEvidence(
            ProviderQueryOutcome::Fallback
        ))
    );
    Ok(())
}

#[test]
fn nonempty_exact_cannot_inject_completeness_producers() -> Result<(), Box<dyn Error>> {
    let request = request(
        ProviderQueryKind::Declaration,
        ProviderQuerySubject::Symbol("target".to_string()),
    );
    let value = fact(
        ProviderQueryFactRole::Value,
        exact_envelope(
            1,
            42,
            SemanticFactKind::Declaration,
            10,
            20,
            SemanticProducer::Parser,
        ),
        &["target"],
    )?;
    let grant = ProviderCompletenessGrant::try_new(
        &request,
        [SemanticProducer::WorkspaceIndex],
        SemanticProvenance::Known(Provenance::ExactAst),
        SemanticConfidence::Known(Confidence::High),
        SemanticFreshness::Fresh,
    )?;
    let result = ProviderQueryResult::try_new(
        &request,
        ProviderQueryOutcome::Exact,
        vec![value],
        Some(grant),
        primary(SemanticReasonCode::ExactSource),
    );
    assert_eq!(
        result.err(),
        Some(ProviderQueryContractError::UnexpectedCompletenessGrant)
    );
    Ok(())
}

#[test]
fn dynamic_outcome_requires_a_typed_dynamic_boundary() -> Result<(), Box<dyn Error>> {
    let request = request(
        ProviderQueryKind::Declaration,
        ProviderQuerySubject::Symbol("target".to_string()),
    );
    let missing = ProviderQueryResult::try_new(
        &request,
        ProviderQueryOutcome::Dynamic,
        Vec::new(),
        None,
        primary(SemanticReasonCode::DynamicValue),
    );
    assert_eq!(
        missing.err(),
        Some(ProviderQueryContractError::InvalidOutcomeEvidence(
            ProviderQueryOutcome::Dynamic
        ))
    );

    let boundary = BoundaryLink::new(
        Some(FactId(9)),
        BoundaryKind::DynamicValue,
        BoundaryDisposition::Degrade,
        SemanticReasonCode::DynamicValue,
    );
    let dynamic = ProviderQueryResult::try_new(
        &request,
        ProviderQueryOutcome::Dynamic,
        Vec::new(),
        None,
        ProviderQueryEvidenceInput::new(
            ProviderResultPath::Primary,
            Some(boundary),
            SemanticReasonCode::DynamicValue,
            Vec::new(),
            Vec::new(),
            ProviderQueryTerminalState::Completed,
        ),
    )?;
    assert_eq!(dynamic.outcome(), ProviderQueryOutcome::Dynamic);
    Ok(())
}

#[test]
fn canonical_serialization_is_order_independent() -> Result<(), Box<dyn Error>> {
    let request = request(
        ProviderQueryKind::Visibility,
        ProviderQuerySubject::Workspace,
    );
    let first = fact(
        ProviderQueryFactRole::Value,
        exact_envelope(
            1,
            42,
            SemanticFactKind::Declaration,
            10,
            20,
            SemanticProducer::Parser,
        ),
        &[],
    )?;
    let second = fact(
        ProviderQueryFactRole::Value,
        exact_envelope(
            2,
            42,
            SemanticFactKind::Occurrence,
            30,
            40,
            SemanticProducer::SemanticAnalyzer,
        ),
        &[],
    )?;
    let left = ProviderQueryResult::try_new(
        &request,
        ProviderQueryOutcome::Exact,
        vec![second.clone(), first.clone()],
        None,
        primary(SemanticReasonCode::ExactSource),
    )?;
    let right = ProviderQueryResult::try_new(
        &request,
        ProviderQueryOutcome::Exact,
        vec![first, second],
        None,
        primary(SemanticReasonCode::ExactSource),
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
                None,
                ProviderQueryEvidenceInput::new(
                    ProviderResultPath::Primary,
                    None,
                    SemanticReasonCode::Unknown,
                    Vec::new(),
                    Vec::new(),
                    ProviderQueryTerminalState::Cancelled,
                ),
            );
        }
        if control.deadline_expired() {
            return ProviderQueryResult::try_new(
                request,
                ProviderQueryOutcome::DeadlineExceeded,
                Vec::new(),
                None,
                ProviderQueryEvidenceInput::new(
                    ProviderResultPath::Primary,
                    None,
                    SemanticReasonCode::Unknown,
                    Vec::new(),
                    Vec::new(),
                    ProviderQueryTerminalState::DeadlineExceeded,
                ),
            );
        }
        ProviderQueryResult::try_new(
            request,
            ProviderQueryOutcome::Unavailable,
            Vec::new(),
            None,
            primary(SemanticReasonCode::Unknown),
        )
    }
}

#[test]
fn live_control_observes_changes_after_dispatch() -> Result<(), Box<dyn Error>> {
    let request = request(ProviderQueryKind::Readiness, ProviderQuerySubject::Workspace);
    let cancelled =
        PollingPort.query(&request, &SequencedControl::cancellation_after_dispatch())?;
    assert_eq!(cancelled.outcome(), ProviderQueryOutcome::Cancelled);

    let deadline = PollingPort.query(&request, &SequencedControl::deadline_after_dispatch())?;
    assert_eq!(deadline.outcome(), ProviderQueryOutcome::DeadlineExceeded);
    Ok(())
}
