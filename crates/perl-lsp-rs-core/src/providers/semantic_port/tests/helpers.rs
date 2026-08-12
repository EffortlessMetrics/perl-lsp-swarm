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
