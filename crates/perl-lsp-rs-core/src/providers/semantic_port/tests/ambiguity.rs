#[test]
fn ambiguity_counts_distinct_entity_candidates() -> Result<(), Box<dyn Error>> {
    let request = request(
        ProviderQueryKind::Declaration,
        ProviderQuerySubject::Symbol("foo".to_string()),
    );
    let first = fact(
        ProviderQueryFactRole::Selector,
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
    let second_same_entity = fact(
        ProviderQueryFactRole::Supporting,
        exact_envelope(
            2,
            42,
            SemanticFactKind::Occurrence,
            1,
            30,
            34,
            SemanticProducer::Parser,
        ),
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
        Some(ProviderQueryContractError::InvalidOutcomeEvidence(
            ProviderQueryOutcome::Ambiguous
        ))
    );

    let second_entity = fact(
        ProviderQueryFactRole::Selector,
        exact_envelope(
            3,
            99,
            SemanticFactKind::Declaration,
            1,
            40,
            50,
            SemanticProducer::Parser,
        ),
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
