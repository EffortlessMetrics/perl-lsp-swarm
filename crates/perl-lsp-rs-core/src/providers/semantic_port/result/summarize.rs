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
    if values.all(|value| value == first) {
        first
    } else {
        SemanticProvenance::Unknown
    }
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
    if values.all(|value| value == first) {
        first
    } else {
        SemanticConfidence::Unknown
    }
}

fn summarize_freshness(
    facts: &[ProviderQueryFact],
    completeness: Option<&ProviderCompletenessGrant>,
) -> SemanticFreshness {
    if facts
        .iter()
        .any(|fact| fact.envelope().freshness == SemanticFreshness::Stale)
    {
        return SemanticFreshness::Stale;
    }
    if facts
        .iter()
        .any(|fact| fact.envelope().freshness == SemanticFreshness::Unknown)
    {
        return SemanticFreshness::Unknown;
    }
    if facts.is_empty() {
        return completeness
            .map(ProviderCompletenessGrant::freshness)
            .unwrap_or(SemanticFreshness::Unknown);
    }
    if facts
        .iter()
        .all(|fact| fact.envelope().freshness == SemanticFreshness::Fresh)
    {
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
    if value_count == 0 {
        Ok(())
    } else {
        invalid(outcome)
    }
}

fn invalid<T>(outcome: ProviderQueryOutcome) -> Result<T, ProviderQueryContractError> {
    Err(ProviderQueryContractError::InvalidOutcomeEvidence(
        outcome,
    ))
}
