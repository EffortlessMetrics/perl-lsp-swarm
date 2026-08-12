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
    assert_eq!(
        missing.err(),
        Some(ProviderQueryContractError::MissingCompletenessGrant)
    );

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
    assert_eq!(
        wrong_family.err(),
        Some(ProviderQueryContractError::InvalidCompletenessGrant)
    );

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
    assert_eq!(
        empty_denominator.err(),
        Some(ProviderQueryContractError::InvalidCompletenessGrant)
    );

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
    assert_eq!(
        zero_coverage.err(),
        Some(ProviderQueryContractError::InvalidCompletenessGrant)
    );
}

#[test]
fn completeness_cannot_add_producer_attribution_to_nonempty_values() -> Result<(), Box<dyn Error>> {
    let request = request(
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
    assert_eq!(
        error.err(),
        Some(ProviderQueryContractError::UnexpectedCompletenessGrant)
    );
    Ok(())
}
