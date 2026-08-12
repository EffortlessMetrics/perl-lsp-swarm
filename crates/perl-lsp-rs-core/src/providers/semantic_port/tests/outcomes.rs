#[test]
fn fresh_label_cannot_hide_a_cross_generation_degraded_value() -> Result<(), Box<dyn Error>> {
    let request = request(
        ProviderQueryKind::Declaration,
        ProviderQuerySubject::Symbol("foo".to_string()),
    );
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
        Some(ProviderQueryContractError::InvalidOutcomeEvidence(
            ProviderQueryOutcome::Degraded
        ))
    );
    Ok(())
}

#[test]
fn fallback_requires_current_values_and_a_fallback_trace() -> Result<(), Box<dyn Error>> {
    let request = request(
        ProviderQueryKind::Declaration,
        ProviderQuerySubject::Symbol("foo".to_string()),
    );
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
    let request = request(
        ProviderQueryKind::Declaration,
        ProviderQuerySubject::Symbol("foo".to_string()),
    );
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
        Some(ProviderQueryContractError::InvalidOutcomeEvidence(
            ProviderQueryOutcome::Unavailable
        ))
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
    assert_eq!(
        readiness_result.outcome(),
        ProviderQueryOutcome::Unavailable
    );

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
    let request = request(
        ProviderQueryKind::Declaration,
        ProviderQuerySubject::Symbol("foo".to_string()),
    );
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
        Some(ProviderQueryContractError::InvalidOutcomeEvidence(
            ProviderQueryOutcome::Refused
        ))
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
    let request = request(
        ProviderQueryKind::Declaration,
        ProviderQuerySubject::Symbol("foo".to_string()),
    );
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
    assert_eq!(
        result.err(),
        Some(ProviderQueryContractError::MalformedFact(FactId(1)))
    );
}

#[test]
fn duplicate_fact_ids_are_rejected() -> Result<(), Box<dyn Error>> {
    let request = request(
        ProviderQueryKind::Declaration,
        ProviderQuerySubject::Symbol("foo".to_string()),
    );
    let first = fact(
        ProviderQueryFactRole::SelectorValue,
        exact_envelope(
            1,
            42,
            SemanticFactKind::Declaration,
            1,
            10,
            20,
            SemanticProducer::Parser,
        ),
        &["foo"],
    )?;
    let second = fact(
        ProviderQueryFactRole::Supporting,
        exact_envelope(
            1,
            42,
            SemanticFactKind::Occurrence,
            1,
            30,
            34,
            SemanticProducer::Parser,
        ),
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
    assert_eq!(
        result.err(),
        Some(ProviderQueryContractError::DuplicateFactId(FactId(1)))
    );
    Ok(())
}
