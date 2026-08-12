fn validate_fact_subjects(
    request: &ProviderQueryRequest,
    facts: &[ProviderQueryFact],
) -> Result<(), ProviderQueryContractError> {
    if facts.is_empty() {
        return Ok(());
    }
    let direct: Vec<_> = facts
        .iter()
        .filter(|fact| fact.matches_subject_directly(&request.subject))
        .collect();
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
        return Err(ProviderQueryContractError::FactDoesNotMatchSubject(
            fact.envelope().fact_id,
        ));
    }

    if matches!(request.subject, ProviderQuerySubject::Position { .. }) {
        let selectors: Vec<_> = direct
            .iter()
            .copied()
            .filter(|fact| fact.role().is_selector())
            .collect();
        for fact in facts.iter().filter(|fact| fact.role().is_value()) {
            if fact.matches_subject_directly(&request.subject)
                || selectors
                    .iter()
                    .any(|selector| facts_are_related(fact, selector))
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
            return Err(ProviderQueryContractError::DuplicateFactId(
                fact.envelope().fact_id,
            ));
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
    if valid {
        Ok(())
    } else {
        invalid(outcome)
    }
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
        ProviderQueryKind::Visibility => matches!(
            fact_kind,
            SemanticFactKind::Import
                | SemanticFactKind::Module
                | SemanticFactKind::Declaration
                | SemanticFactKind::Occurrence
        ),
        ProviderQueryKind::ScopeBindings => matches!(
            fact_kind,
            SemanticFactKind::Declaration | SemanticFactKind::Occurrence
        ),
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
