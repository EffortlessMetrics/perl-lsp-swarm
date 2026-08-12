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
