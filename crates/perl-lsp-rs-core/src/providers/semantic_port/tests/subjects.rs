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
            1,
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
        ProviderQueryKind::References {
            include_declaration: false,
        },
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
            1,
            10,
            14,
            SemanticProducer::Parser,
        ),
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
            1,
            10,
            14,
            SemanticProducer::Parser,
        ),
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
    assert_eq!(
        error.err(),
        Some(ProviderQueryContractError::UnrelatedPositionValue(FactId(2)))
    );
    Ok(())
}
