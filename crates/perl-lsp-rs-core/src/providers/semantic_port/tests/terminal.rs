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
        Some(ProviderQueryContractError::InvalidOutcomeEvidence(
            ProviderQueryOutcome::Cancelled
        ))
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
fn observed_cancellation_and_deadline_can_cross_the_checked_boundary() -> Result<(), Box<dyn Error>> {
    let request = request(ProviderQueryKind::Readiness, ProviderQuerySubject::Workspace);
    let cancelled = execute(
        &request,
        ProviderQueryResultDraft::new(
            ProviderQueryOutcome::Cancelled,
            Vec::new(),
            None,
            terminal(ProviderQueryTerminalState::Cancelled),
        ),
        &TestControl {
            cancelled: true,
            deadline_expired: false,
        },
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
        &TestControl {
            cancelled: false,
            deadline_expired: true,
        },
    )?;
    assert!(
        deadline
            .evidence()
            .control_observation()
            .deadline_expired()
    );
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
        &TestControl {
            cancelled: true,
            deadline_expired: true,
        },
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
    let result = execute(
        &request,
        ProviderQueryResultDraft::new(
            ProviderQueryOutcome::Exact,
            vec![value],
            None,
            primary(SemanticReasonCode::ExactSource),
        ),
        &TestControl {
            cancelled: true,
            deadline_expired: false,
        },
    );
    assert_eq!(
        result.err(),
        Some(ProviderQueryContractError::InvalidOutcomeEvidence(
            ProviderQueryOutcome::Exact
        ))
    );
    Ok(())
}
