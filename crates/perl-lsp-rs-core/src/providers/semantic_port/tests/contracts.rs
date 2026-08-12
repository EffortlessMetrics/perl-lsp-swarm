#[test]
fn visibility_accepts_visible_declaration_facts() -> Result<(), Box<dyn Error>> {
    let request = request(
        ProviderQueryKind::Visibility,
        ProviderQuerySubject::Symbol("foo".to_string()),
    );
    let visible = fact(
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
        Some(ProviderQueryContractError::InvalidOutcomeEvidence(
            ProviderQueryOutcome::Exact
        ))
    );
    Ok(())
}

#[test]
fn exact_results_remain_ineligible_for_edit_authorizing_requests() -> Result<(), Box<dyn Error>> {
    let mut request = request(
        ProviderQueryKind::Declaration,
        ProviderQuerySubject::Symbol("foo".to_string()),
    );
    request.context.readiness_requirement = ProviderReadinessRequirement::EditAuthorizing;
    let value = fact(
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
        Some(ProviderQueryContractError::InvalidOutcomeEvidence(
            ProviderQueryOutcome::Exact
        ))
    );
    Ok(())
}

#[test]
fn deterministic_serialization_ignores_input_order() -> Result<(), Box<dyn Error>> {
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
            2,
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
    let original_request = request(
        ProviderQueryKind::Declaration,
        ProviderQuerySubject::Symbol("foo".to_string()),
    );
    let value = fact(
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
    let other = request(
        ProviderQueryKind::Declaration,
        ProviderQuerySubject::Symbol("bar".to_string()),
    );
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
        Some(ProviderQueryContractError::InvalidOutcomeEvidence(
            ProviderQueryOutcome::Dynamic
        ))
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
