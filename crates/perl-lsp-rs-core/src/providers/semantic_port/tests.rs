use super::model::VerifiedProviderCompletenessSnapshot;
use super::*;
use perl_semantic_facts::{
    AnchorId, BoundaryDisposition, BoundaryKind, BoundaryLink, Confidence, EntityId, FactId,
    FileId, LifecyclePhase, Provenance, ProviderFactFreshness, ProviderFactSourceKind,
    ProviderFallbackState, ProviderSurface, ScopeId, SemanticConfidence, SemanticFactEnvelope,
    SemanticFactKind, SemanticFreshness, SemanticProducer, SemanticProvenance, SemanticReasonCode,
    SourceAnchor, SourceGeneration,
};
use std::error::Error;

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
    ProviderQueryRequest::new(ProviderSurface::Definition, "test/request", kind, subject, context())
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
    file_id: u64,
    start: u32,
    end: u32,
    producer: SemanticProducer,
) -> SemanticFactEnvelope {
    envelope(
        fact_id,
        Some(entity_id),
        kind,
        file_id,
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

fn terminal(state: ProviderQueryTerminalState) -> ProviderQueryEvidenceInput {
    ProviderQueryEvidenceInput::new(
        ProviderResultPath::Primary,
        None,
        SemanticReasonCode::Unknown,
        Vec::new(),
        Vec::new(),
        state,
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

fn unavailable_trace() -> perl_semantic_facts::ProviderFactTrace {
    perl_semantic_facts::ProviderFactTrace::new(
        ProviderSurface::Definition,
        ProviderFactSourceKind::LegacyWorkspace,
        Provenance::NameHeuristic,
        Confidence::Medium,
        ProviderFactFreshness::Fresh,
        ProviderFallbackState::Unavailable,
        Some("unavailable".to_string()),
        None,
        Some(1),
    )
}

#[derive(Clone)]
struct StaticPort {
    draft: ProviderQueryResultDraft,
}

impl ProviderSemanticPort for StaticPort {
    fn query(
        &self,
        _request: &ProviderQueryRequest,
        _control: &dyn ProviderQueryControl,
    ) -> Result<ProviderQueryResultDraft, ProviderQueryContractError> {
        Ok(self.draft.clone())
    }
}

#[derive(Debug, Clone, Copy)]
struct TestControl {
    cancelled: bool,
    deadline_expired: bool,
}

impl ProviderQueryControl for TestControl {
    fn is_cancelled(&self) -> bool {
        self.cancelled
    }

    fn deadline_expired(&self) -> bool {
        self.deadline_expired
    }
}

fn execute(
    request: &ProviderQueryRequest,
    draft: ProviderQueryResultDraft,
    control: &dyn ProviderQueryControl,
) -> Result<ProviderQueryResult, ProviderQueryContractError> {
    execute_provider_query(&StaticPort { draft }, request, control)
}

fn verified_grant(
    request: &ProviderQueryRequest,
) -> Result<ProviderCompletenessGrant, ProviderQueryContractError> {
    let snapshot = VerifiedProviderCompletenessSnapshot::try_new(
        request,
        ProviderQueryCapability::from_query(&request.kind),
        SemanticProducer::Parser,
        "parser:declarations:file-1",
        "document-1:ast-root-7",
        1,
        SemanticProvenance::Known(Provenance::ExactAst),
        SemanticConfidence::Known(Confidence::High),
        SemanticFreshness::Fresh,
    )?;
    Ok(ProviderCompletenessGrant::from_verified_snapshot(snapshot))
}
#[test]
fn position_selector_can_return_a_declaration_elsewhere() -> Result<(), Box<dyn Error>> {
    let request = request(
        ProviderQueryKind::Declaration,
        ProviderQuerySubject::Position { file_id: FileId(1), byte_offset: 11 },
    );
    let selector = fact(
        ProviderQueryFactRole::Selector,
        exact_envelope(1, 42, SemanticFactKind::Occurrence, 1, 10, 14, SemanticProducer::Parser),
        &[],
    )?;
    let value = fact(
        ProviderQueryFactRole::Value,
        exact_envelope(
            2,
            42,
            SemanticFactKind::Declaration,
            2,
            100,
            120,
            SemanticProducer::WorkspaceIndex,
        ),
        &[],
    )?;
    let result = execute(
        &request,
        ProviderQueryResultDraft::new(
            ProviderQueryOutcome::Exact,
            vec![value, selector],
            None,
            primary(SemanticReasonCode::ExactSource),
        ),
        &NoopProviderQueryControl,
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
fn references_can_return_occurrences_in_another_file() -> Result<(), Box<dyn Error>> {
    let request = request(
        ProviderQueryKind::References { include_declaration: false },
        ProviderQuerySubject::Position { file_id: FileId(1), byte_offset: 11 },
    );
    let selector = fact(
        ProviderQueryFactRole::Selector,
        exact_envelope(1, 42, SemanticFactKind::Occurrence, 1, 10, 14, SemanticProducer::Parser),
        &[],
    )?;
    let remote = fact(
        ProviderQueryFactRole::Value,
        exact_envelope(
            2,
            42,
            SemanticFactKind::Occurrence,
            2,
            80,
            84,
            SemanticProducer::WorkspaceIndex,
        ),
        &[],
    )?;
    let result = execute(
        &request,
        ProviderQueryResultDraft::new(
            ProviderQueryOutcome::Exact,
            vec![remote, selector],
            None,
            primary(SemanticReasonCode::ExactSource),
        ),
        &NoopProviderQueryControl,
    )?;
    assert_eq!(result.value_facts().count(), 1);
    Ok(())
}

#[test]
fn package_and_scope_do_not_relate_different_entities() -> Result<(), Box<dyn Error>> {
    let request = request(
        ProviderQueryKind::Declaration,
        ProviderQuerySubject::Position { file_id: FileId(1), byte_offset: 11 },
    );
    let selector = fact(
        ProviderQueryFactRole::Selector,
        exact_envelope(1, 42, SemanticFactKind::Occurrence, 1, 10, 14, SemanticProducer::Parser),
        &[],
    )?;
    let unrelated = fact(
        ProviderQueryFactRole::Value,
        exact_envelope(
            2,
            99,
            SemanticFactKind::Declaration,
            2,
            100,
            120,
            SemanticProducer::WorkspaceIndex,
        ),
        &[],
    )?;
    let error = execute(
        &request,
        ProviderQueryResultDraft::new(
            ProviderQueryOutcome::Exact,
            vec![selector, unrelated],
            None,
            primary(SemanticReasonCode::ExactSource),
        ),
        &NoopProviderQueryControl,
    );
    assert_eq!(error.err(), Some(ProviderQueryContractError::UnrelatedPositionValue(FactId(2))));
    Ok(())
}
#[test]
fn exact_empty_requires_verified_denominator_authority() -> Result<(), Box<dyn Error>> {
    let request = request(
        ProviderQueryKind::Declaration,
        ProviderQuerySubject::Symbol("missing".to_string()),
    );
    let missing = execute(
        &request,
        ProviderQueryResultDraft::new(
            ProviderQueryOutcome::Exact,
            Vec::new(),
            None,
            primary(SemanticReasonCode::ExactSource),
        ),
        &NoopProviderQueryControl,
    );
    assert_eq!(missing.err(), Some(ProviderQueryContractError::MissingCompletenessGrant));

    let grant = verified_grant(&request)?;
    let exact = execute(
        &request,
        ProviderQueryResultDraft::new(
            ProviderQueryOutcome::Exact,
            Vec::new(),
            Some(grant),
            primary(SemanticReasonCode::ExactSource),
        ),
        &NoopProviderQueryControl,
    )?;
    assert!(exact.is_exact_empty());
    assert!(exact.evidence().producers().is_empty());
    let authority = exact.evidence().completeness_authority();
    assert!(authority.is_some(), "exact empty retains its denominator receipt");
    if let Some(authority) = authority {
        assert_eq!(authority.producer(), SemanticProducer::Parser);
        assert_eq!(authority.denominator_id(), "parser:declarations:file-1");
        assert_eq!(authority.snapshot_id(), "document-1:ast-root-7");
        assert_eq!(authority.covered_unit_count(), 1);
    }
    Ok(())
}

#[test]
fn unsupported_or_opaque_denominators_cannot_issue_completeness() {
    let request = request(
        ProviderQueryKind::Declaration,
        ProviderQuerySubject::Symbol("missing".to_string()),
    );
    let wrong_family = VerifiedProviderCompletenessSnapshot::try_new(
        &request,
        ProviderQueryCapability::References,
        SemanticProducer::Parser,
        "parser:references:file-1",
        "document-1:ast-root-7",
        1,
        SemanticProvenance::Known(Provenance::ExactAst),
        SemanticConfidence::Known(Confidence::High),
        SemanticFreshness::Fresh,
    );
    assert_eq!(wrong_family.err(), Some(ProviderQueryContractError::InvalidCompletenessGrant));

    let empty_denominator = VerifiedProviderCompletenessSnapshot::try_new(
        &request,
        ProviderQueryCapability::Declarations,
        SemanticProducer::Parser,
        "",
        "document-1:ast-root-7",
        1,
        SemanticProvenance::Known(Provenance::ExactAst),
        SemanticConfidence::Known(Confidence::High),
        SemanticFreshness::Fresh,
    );
    assert_eq!(empty_denominator.err(), Some(ProviderQueryContractError::InvalidCompletenessGrant));

    let zero_coverage = VerifiedProviderCompletenessSnapshot::try_new(
        &request,
        ProviderQueryCapability::Declarations,
        SemanticProducer::Parser,
        "parser:declarations:file-1",
        "document-1:ast-root-7",
        0,
        SemanticProvenance::Known(Provenance::ExactAst),
        SemanticConfidence::Known(Confidence::High),
        SemanticFreshness::Fresh,
    );
    assert_eq!(zero_coverage.err(), Some(ProviderQueryContractError::InvalidCompletenessGrant));
}

#[test]
fn completeness_cannot_add_producer_attribution_to_nonempty_values() -> Result<(), Box<dyn Error>> {
    let request =
        request(ProviderQueryKind::Declaration, ProviderQuerySubject::Symbol("foo".to_string()));
    let value = fact(
        ProviderQueryFactRole::SelectorValue,
        exact_envelope(1, 42, SemanticFactKind::Declaration, 1, 10, 20, SemanticProducer::Parser),
        &["foo"],
    )?;
    let error = execute(
        &request,
        ProviderQueryResultDraft::new(
            ProviderQueryOutcome::Exact,
            vec![value],
            Some(verified_grant(&request)?),
            primary(SemanticReasonCode::ExactSource),
        ),
        &NoopProviderQueryControl,
    );
    assert_eq!(error.err(), Some(ProviderQueryContractError::UnexpectedCompletenessGrant));
    Ok(())
}
#[test]
fn ambiguity_counts_distinct_entity_candidates() -> Result<(), Box<dyn Error>> {
    let request =
        request(ProviderQueryKind::Declaration, ProviderQuerySubject::Symbol("foo".to_string()));
    let first = fact(
        ProviderQueryFactRole::Selector,
        exact_envelope(1, 42, SemanticFactKind::Declaration, 1, 10, 20, SemanticProducer::Parser),
        &["foo"],
    )?;
    let second_same_entity = fact(
        ProviderQueryFactRole::Supporting,
        exact_envelope(2, 42, SemanticFactKind::Occurrence, 1, 30, 34, SemanticProducer::Parser),
        &["foo"],
    )?;
    let same_entity = execute(
        &request,
        ProviderQueryResultDraft::new(
            ProviderQueryOutcome::Ambiguous,
            vec![first.clone(), second_same_entity],
            None,
            primary(SemanticReasonCode::Unknown),
        ),
        &NoopProviderQueryControl,
    );
    assert_eq!(
        same_entity.err(),
        Some(ProviderQueryContractError::InvalidOutcomeEvidence(ProviderQueryOutcome::Ambiguous))
    );

    let second_entity = fact(
        ProviderQueryFactRole::Selector,
        exact_envelope(3, 99, SemanticFactKind::Declaration, 1, 40, 50, SemanticProducer::Parser),
        &["foo"],
    )?;
    let ambiguous = execute(
        &request,
        ProviderQueryResultDraft::new(
            ProviderQueryOutcome::Ambiguous,
            vec![first, second_entity],
            None,
            primary(SemanticReasonCode::Unknown),
        ),
        &NoopProviderQueryControl,
    )?;
    assert_eq!(ambiguous.outcome(), ProviderQueryOutcome::Ambiguous);
    Ok(())
}
#[test]
fn false_terminal_claims_are_rejected_by_caller_control() {
    let request = request(ProviderQueryKind::Readiness, ProviderQuerySubject::Workspace);
    let false_cancel = execute(
        &request,
        ProviderQueryResultDraft::new(
            ProviderQueryOutcome::Cancelled,
            Vec::new(),
            None,
            terminal(ProviderQueryTerminalState::Cancelled),
        ),
        &NoopProviderQueryControl,
    );
    assert_eq!(
        false_cancel.err(),
        Some(ProviderQueryContractError::InvalidOutcomeEvidence(ProviderQueryOutcome::Cancelled))
    );

    let false_deadline = execute(
        &request,
        ProviderQueryResultDraft::new(
            ProviderQueryOutcome::DeadlineExceeded,
            Vec::new(),
            None,
            terminal(ProviderQueryTerminalState::DeadlineExceeded),
        ),
        &NoopProviderQueryControl,
    );
    assert_eq!(
        false_deadline.err(),
        Some(ProviderQueryContractError::InvalidOutcomeEvidence(
            ProviderQueryOutcome::DeadlineExceeded
        ))
    );
}

#[test]
fn observed_cancellation_and_deadline_can_cross_the_checked_boundary() -> Result<(), Box<dyn Error>>
{
    let request = request(ProviderQueryKind::Readiness, ProviderQuerySubject::Workspace);
    let cancelled = execute(
        &request,
        ProviderQueryResultDraft::new(
            ProviderQueryOutcome::Cancelled,
            Vec::new(),
            None,
            terminal(ProviderQueryTerminalState::Cancelled),
        ),
        &TestControl { cancelled: true, deadline_expired: false },
    )?;
    assert!(cancelled.evidence().control_observation().cancelled());

    let deadline = execute(
        &request,
        ProviderQueryResultDraft::new(
            ProviderQueryOutcome::DeadlineExceeded,
            Vec::new(),
            None,
            terminal(ProviderQueryTerminalState::DeadlineExceeded),
        ),
        &TestControl { cancelled: false, deadline_expired: true },
    )?;
    assert!(deadline.evidence().control_observation().deadline_expired());
    Ok(())
}

#[test]
fn cancellation_has_precedence_when_both_controls_are_terminal() {
    let request = request(ProviderQueryKind::Readiness, ProviderQuerySubject::Workspace);
    let deadline = execute(
        &request,
        ProviderQueryResultDraft::new(
            ProviderQueryOutcome::DeadlineExceeded,
            Vec::new(),
            None,
            terminal(ProviderQueryTerminalState::DeadlineExceeded),
        ),
        &TestControl { cancelled: true, deadline_expired: true },
    );
    assert_eq!(
        deadline.err(),
        Some(ProviderQueryContractError::InvalidOutcomeEvidence(
            ProviderQueryOutcome::DeadlineExceeded
        ))
    );
}

#[test]
fn completed_draft_is_rejected_after_live_cancellation() -> Result<(), Box<dyn Error>> {
    let request =
        request(ProviderQueryKind::Declaration, ProviderQuerySubject::Symbol("foo".to_string()));
    let value = fact(
        ProviderQueryFactRole::SelectorValue,
        exact_envelope(1, 42, SemanticFactKind::Declaration, 1, 10, 20, SemanticProducer::Parser),
        &["foo"],
    )?;
    let result = execute(
        &request,
        ProviderQueryResultDraft::new(
            ProviderQueryOutcome::Exact,
            vec![value],
            None,
            primary(SemanticReasonCode::ExactSource),
        ),
        &TestControl { cancelled: true, deadline_expired: false },
    );
    assert_eq!(
        result.err(),
        Some(ProviderQueryContractError::InvalidOutcomeEvidence(ProviderQueryOutcome::Exact))
    );
    Ok(())
}
#[test]
fn fresh_label_cannot_hide_a_cross_generation_degraded_value() -> Result<(), Box<dyn Error>> {
    let request =
        request(ProviderQueryKind::Declaration, ProviderQuerySubject::Symbol("foo".to_string()));
    let stale_generation = fact(
        ProviderQueryFactRole::SelectorValue,
        envelope(
            1,
            Some(42),
            SemanticFactKind::Declaration,
            1,
            10,
            20,
            "document-0",
            SemanticProducer::Parser,
            Provenance::ExactAst,
            Confidence::Medium,
            SemanticFreshness::Fresh,
            Some("Example"),
            Some(9),
            None,
            SemanticReasonCode::GeneratedFromSource,
        ),
        &["foo"],
    )?;
    let result = execute(
        &request,
        ProviderQueryResultDraft::new(
            ProviderQueryOutcome::Degraded,
            vec![stale_generation],
            None,
            ProviderQueryEvidenceInput::new(
                ProviderResultPath::Primary,
                None,
                SemanticReasonCode::GeneratedFromSource,
                Vec::new(),
                vec!["qualified".to_string()],
                ProviderQueryTerminalState::Completed,
            ),
        ),
        &NoopProviderQueryControl,
    );
    assert_eq!(
        result.err(),
        Some(ProviderQueryContractError::InvalidOutcomeEvidence(ProviderQueryOutcome::Degraded))
    );
    Ok(())
}

#[test]
fn fallback_requires_current_values_and_a_fallback_trace() -> Result<(), Box<dyn Error>> {
    let request =
        request(ProviderQueryKind::Declaration, ProviderQuerySubject::Symbol("foo".to_string()));
    let value = fact(
        ProviderQueryFactRole::SelectorValue,
        envelope(
            1,
            Some(42),
            SemanticFactKind::Declaration,
            1,
            10,
            20,
            "document-1",
            SemanticProducer::Parser,
            Provenance::NameHeuristic,
            Confidence::Medium,
            SemanticFreshness::Fresh,
            Some("Example"),
            Some(9),
            None,
            SemanticReasonCode::CompatibilityBoundary,
        ),
        &["foo"],
    )?;
    let result = execute(
        &request,
        ProviderQueryResultDraft::new(
            ProviderQueryOutcome::Fallback,
            vec![value],
            None,
            ProviderQueryEvidenceInput::new(
                ProviderResultPath::Fallback,
                None,
                SemanticReasonCode::CompatibilityBoundary,
                vec![fallback_trace()],
                vec!["legacy workspace fallback".to_string()],
                ProviderQueryTerminalState::Completed,
            ),
        ),
        &NoopProviderQueryControl,
    )?;
    assert_eq!(result.evidence().proof_class(), ProviderProofClass::FallbackOnly);
    Ok(())
}

#[test]
fn unavailable_requires_readiness_or_typed_evidence() -> Result<(), Box<dyn Error>> {
    let request =
        request(ProviderQueryKind::Declaration, ProviderQuerySubject::Symbol("foo".to_string()));
    assert_eq!(
        execute(
            &request,
            ProviderQueryResultDraft::new(
                ProviderQueryOutcome::Unavailable,
                Vec::new(),
                None,
                primary(SemanticReasonCode::Unknown),
            ),
            &NoopProviderQueryControl,
        )
        .err(),
        Some(ProviderQueryContractError::InvalidOutcomeEvidence(ProviderQueryOutcome::Unavailable))
    );

    let mut unavailable_request = request.clone();
    unavailable_request.context.readiness_state = ProviderReadinessState::Unavailable;
    let readiness_result = execute(
        &unavailable_request,
        ProviderQueryResultDraft::new(
            ProviderQueryOutcome::Unavailable,
            Vec::new(),
            None,
            primary(SemanticReasonCode::Unknown),
        ),
        &NoopProviderQueryControl,
    )?;
    assert_eq!(readiness_result.outcome(), ProviderQueryOutcome::Unavailable);

    let typed_result = execute(
        &request,
        ProviderQueryResultDraft::new(
            ProviderQueryOutcome::Unavailable,
            Vec::new(),
            None,
            ProviderQueryEvidenceInput::new(
                ProviderResultPath::Primary,
                None,
                SemanticReasonCode::Unknown,
                vec![unavailable_trace()],
                Vec::new(),
                ProviderQueryTerminalState::Completed,
            ),
        ),
        &NoopProviderQueryControl,
    )?;
    assert_eq!(typed_result.outcome(), ProviderQueryOutcome::Unavailable);
    Ok(())
}

#[test]
fn refused_requires_refusal_evidence() -> Result<(), Box<dyn Error>> {
    let request =
        request(ProviderQueryKind::Declaration, ProviderQuerySubject::Symbol("foo".to_string()));
    assert_eq!(
        execute(
            &request,
            ProviderQueryResultDraft::new(
                ProviderQueryOutcome::Refused,
                Vec::new(),
                None,
                primary(SemanticReasonCode::Unknown),
            ),
            &NoopProviderQueryControl,
        )
        .err(),
        Some(ProviderQueryContractError::InvalidOutcomeEvidence(ProviderQueryOutcome::Refused))
    );
    let result = execute(
        &request,
        ProviderQueryResultDraft::new(
            ProviderQueryOutcome::Refused,
            Vec::new(),
            None,
            primary(SemanticReasonCode::UnsupportedEffect),
        ),
        &NoopProviderQueryControl,
    )?;
    assert_eq!(result.outcome(), ProviderQueryOutcome::Refused);
    Ok(())
}

#[test]
fn error_requires_a_failed_terminal_state() -> Result<(), Box<dyn Error>> {
    let request =
        request(ProviderQueryKind::Declaration, ProviderQuerySubject::Symbol("foo".to_string()));
    let result = execute(
        &request,
        ProviderQueryResultDraft::new(
            ProviderQueryOutcome::Error,
            Vec::new(),
            None,
            terminal(ProviderQueryTerminalState::Failed),
        ),
        &NoopProviderQueryControl,
    )?;
    assert_eq!(result.outcome(), ProviderQueryOutcome::Error);
    Ok(())
}

#[test]
fn malformed_ranges_fail_before_degraded_policy() {
    let malformed = envelope(
        1,
        Some(42),
        SemanticFactKind::Declaration,
        1,
        20,
        10,
        "document-1",
        SemanticProducer::Parser,
        Provenance::ExactAst,
        Confidence::Medium,
        SemanticFreshness::Fresh,
        Some("Example"),
        Some(9),
        None,
        SemanticReasonCode::GeneratedFromSource,
    );
    let result = ProviderQueryFact::from_envelope(
        ProviderQueryFactRole::Value,
        ProviderFactGenerationScope::Document,
        malformed,
    );
    assert_eq!(result.err(), Some(ProviderQueryContractError::MalformedFact(FactId(1))));
}

#[test]
fn duplicate_fact_ids_are_rejected() -> Result<(), Box<dyn Error>> {
    let request =
        request(ProviderQueryKind::Declaration, ProviderQuerySubject::Symbol("foo".to_string()));
    let first = fact(
        ProviderQueryFactRole::SelectorValue,
        exact_envelope(1, 42, SemanticFactKind::Declaration, 1, 10, 20, SemanticProducer::Parser),
        &["foo"],
    )?;
    let second = fact(
        ProviderQueryFactRole::Supporting,
        exact_envelope(1, 42, SemanticFactKind::Occurrence, 1, 30, 34, SemanticProducer::Parser),
        &["foo"],
    )?;
    let result = execute(
        &request,
        ProviderQueryResultDraft::new(
            ProviderQueryOutcome::Exact,
            vec![first, second],
            None,
            primary(SemanticReasonCode::ExactSource),
        ),
        &NoopProviderQueryControl,
    );
    assert_eq!(result.err(), Some(ProviderQueryContractError::DuplicateFactId(FactId(1))));
    Ok(())
}
#[test]
fn visibility_accepts_visible_declaration_facts() -> Result<(), Box<dyn Error>> {
    let request =
        request(ProviderQueryKind::Visibility, ProviderQuerySubject::Symbol("foo".to_string()));
    let visible = fact(
        ProviderQueryFactRole::SelectorValue,
        exact_envelope(1, 42, SemanticFactKind::Declaration, 1, 10, 20, SemanticProducer::Parser),
        &["foo"],
    )?;
    let result = execute(
        &request,
        ProviderQueryResultDraft::new(
            ProviderQueryOutcome::Exact,
            vec![visible],
            None,
            primary(SemanticReasonCode::ExactSource),
        ),
        &NoopProviderQueryControl,
    )?;
    assert_eq!(result.value_facts().count(), 1);
    Ok(())
}

#[test]
fn mixed_exact_provenance_does_not_summarize_to_exact() -> Result<(), Box<dyn Error>> {
    let request = request(
        ProviderQueryKind::Declaration,
        ProviderQuerySubject::Position { file_id: FileId(1), byte_offset: 11 },
    );
    let selector = fact(
        ProviderQueryFactRole::Selector,
        exact_envelope(1, 42, SemanticFactKind::Occurrence, 1, 10, 14, SemanticProducer::Parser),
        &[],
    )?;
    let value = fact(
        ProviderQueryFactRole::Value,
        envelope(
            2,
            Some(42),
            SemanticFactKind::Declaration,
            2,
            100,
            120,
            "document-1",
            SemanticProducer::SemanticAnalyzer,
            Provenance::SemanticAnalyzer,
            Confidence::High,
            SemanticFreshness::Fresh,
            Some("Example"),
            Some(9),
            None,
            SemanticReasonCode::ExactSource,
        ),
        &[],
    )?;
    let result = execute(
        &request,
        ProviderQueryResultDraft::new(
            ProviderQueryOutcome::Exact,
            vec![selector, value],
            None,
            primary(SemanticReasonCode::ExactSource),
        ),
        &NoopProviderQueryControl,
    );
    assert_eq!(
        result.err(),
        Some(ProviderQueryContractError::InvalidOutcomeEvidence(ProviderQueryOutcome::Exact))
    );
    Ok(())
}

#[test]
fn exact_results_remain_ineligible_for_edit_authorizing_requests() -> Result<(), Box<dyn Error>> {
    let mut request =
        request(ProviderQueryKind::Declaration, ProviderQuerySubject::Symbol("foo".to_string()));
    request.context.readiness_requirement = ProviderReadinessRequirement::EditAuthorizing;
    let value = fact(
        ProviderQueryFactRole::SelectorValue,
        exact_envelope(1, 42, SemanticFactKind::Declaration, 1, 10, 20, SemanticProducer::Parser),
        &["foo"],
    )?;
    let result = execute(
        &request,
        ProviderQueryResultDraft::new(
            ProviderQueryOutcome::Exact,
            vec![value],
            None,
            primary(SemanticReasonCode::ExactSource),
        ),
        &NoopProviderQueryControl,
    );
    assert_eq!(
        result.err(),
        Some(ProviderQueryContractError::InvalidOutcomeEvidence(ProviderQueryOutcome::Exact))
    );
    Ok(())
}

#[test]
fn deterministic_serialization_ignores_input_order() -> Result<(), Box<dyn Error>> {
    let request = request(
        ProviderQueryKind::Declaration,
        ProviderQuerySubject::Position { file_id: FileId(1), byte_offset: 11 },
    );
    let selector = fact(
        ProviderQueryFactRole::Selector,
        exact_envelope(2, 42, SemanticFactKind::Occurrence, 1, 10, 14, SemanticProducer::Parser),
        &[],
    )?;
    let value = fact(
        ProviderQueryFactRole::Value,
        exact_envelope(
            1,
            42,
            SemanticFactKind::Declaration,
            2,
            100,
            120,
            SemanticProducer::WorkspaceIndex,
        ),
        &[],
    )?;
    let left = execute(
        &request,
        ProviderQueryResultDraft::new(
            ProviderQueryOutcome::Exact,
            vec![selector.clone(), value.clone()],
            None,
            primary(SemanticReasonCode::ExactSource),
        ),
        &NoopProviderQueryControl,
    )?;
    let right = execute(
        &request,
        ProviderQueryResultDraft::new(
            ProviderQueryOutcome::Exact,
            vec![value, selector],
            None,
            primary(SemanticReasonCode::ExactSource),
        ),
        &NoopProviderQueryControl,
    )?;
    assert_eq!(serde_json::to_string(&left)?, serde_json::to_string(&right)?);
    Ok(())
}

#[test]
fn retained_result_rejects_a_different_request() -> Result<(), Box<dyn Error>> {
    let original_request =
        request(ProviderQueryKind::Declaration, ProviderQuerySubject::Symbol("foo".to_string()));
    let value = fact(
        ProviderQueryFactRole::SelectorValue,
        exact_envelope(1, 42, SemanticFactKind::Declaration, 1, 10, 20, SemanticProducer::Parser),
        &["foo"],
    )?;
    let result = execute(
        &original_request,
        ProviderQueryResultDraft::new(
            ProviderQueryOutcome::Exact,
            vec![value],
            None,
            primary(SemanticReasonCode::ExactSource),
        ),
        &NoopProviderQueryControl,
    )?;
    let other =
        request(ProviderQueryKind::Declaration, ProviderQuerySubject::Symbol("bar".to_string()));
    assert_eq!(
        result.validate_against(&other).err(),
        Some(ProviderQueryContractError::RequestBindingMismatch)
    );
    Ok(())
}

#[test]
fn dynamic_outcome_requires_a_dynamic_boundary() -> Result<(), Box<dyn Error>> {
    let request = request(ProviderQueryKind::Boundaries, ProviderQuerySubject::Workspace);
    let missing = execute(
        &request,
        ProviderQueryResultDraft::new(
            ProviderQueryOutcome::Dynamic,
            Vec::new(),
            None,
            primary(SemanticReasonCode::DynamicValue),
        ),
        &NoopProviderQueryControl,
    );
    assert_eq!(
        missing.err(),
        Some(ProviderQueryContractError::InvalidOutcomeEvidence(ProviderQueryOutcome::Dynamic))
    );

    let boundary = BoundaryLink::new(
        None,
        BoundaryKind::DynamicValue,
        BoundaryDisposition::Refuse,
        SemanticReasonCode::DynamicValue,
    );
    let dynamic = execute(
        &request,
        ProviderQueryResultDraft::new(
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
        ),
        &NoopProviderQueryControl,
    )?;
    assert_eq!(dynamic.outcome(), ProviderQueryOutcome::Dynamic);
    Ok(())
}

#[test]
fn exact_empty_authority_rejects_subject_and_query_shape_replay() -> Result<(), Box<dyn Error>> {
    let source = request(ProviderQueryKind::Declaration, ProviderQuerySubject::File(FileId(1)));
    let grant = verified_grant(&source)?;
    let other_file = request(ProviderQueryKind::Declaration, ProviderQuerySubject::File(FileId(2)));
    let replay = execute(
        &other_file,
        ProviderQueryResultDraft::new(
            ProviderQueryOutcome::Exact,
            Vec::new(),
            Some(grant),
            primary(SemanticReasonCode::ExactSource),
        ),
        &NoopProviderQueryControl,
    );
    assert_eq!(replay.err(), Some(ProviderQueryContractError::InvalidCompletenessGrant));

    let references_without_declaration = request(
        ProviderQueryKind::References { include_declaration: false },
        ProviderQuerySubject::File(FileId(1)),
    );
    let reference_grant = verified_grant(&references_without_declaration)?;
    let references_with_declaration = request(
        ProviderQueryKind::References { include_declaration: true },
        ProviderQuerySubject::File(FileId(1)),
    );
    let shape_replay = execute(
        &references_with_declaration,
        ProviderQueryResultDraft::new(
            ProviderQueryOutcome::Exact,
            Vec::new(),
            Some(reference_grant),
            primary(SemanticReasonCode::ExactSource),
        ),
        &NoopProviderQueryControl,
    );
    assert_eq!(shape_replay.err(), Some(ProviderQueryContractError::InvalidCompletenessGrant));
    Ok(())
}
