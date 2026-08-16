use perl_parser_comparison::{
    ComparisonModelError, ExecutionDisposition, ObservationAvailability, ObservationPlane,
    ScoredComparison, ScoredOutcome, SubjectRole, Verdict, execute_v1, execute_v3, parse_v2,
};

#[test]
fn clean_execution_can_still_score_a_structural_mismatch() -> Result<(), ComparisonModelError> {
    let execution = execute_v3("my $x = 42;");
    assert_eq!(execution.subject(), SubjectRole::NativeRecursiveDescent);
    assert_eq!(
        execution.disposition(),
        ExecutionDisposition::AcceptedClean
    );

    let comparison = ScoredComparison::scored(
        "assignment-shape.v1",
        ObservationPlane::Structure,
        "assignment(expected-child-order)",
        "assignment(reversed-child-order)",
        ScoredOutcome::Mismatch,
        Some("children[0]".to_string()),
    )?;

    assert_eq!(comparison.outcome(), ScoredOutcome::Mismatch);
    assert_eq!(comparison.first_mismatch(), Some("children[0]"));
    Ok(())
}

#[test]
fn two_clean_subjects_do_not_create_correctness_agreement() -> Result<(), ComparisonModelError> {
    let historical = execute_v1("my $x = 42;");
    let native = execute_v3("my $x = 42;");

    assert_eq!(historical.disposition(), ExecutionDisposition::AcceptedClean);
    assert_eq!(native.disposition(), ExecutionDisposition::AcceptedClean);

    let historical_score = ScoredComparison::scored(
        "assignment-source-role.v1",
        ObservationPlane::Structure,
        "assignment(variable,integer)",
        "assignment(integer,variable)",
        ScoredOutcome::Mismatch,
        Some("child-order".to_string()),
    )?;
    let native_score = ScoredComparison::scored(
        "assignment-source-role.v1",
        ObservationPlane::Structure,
        "assignment(variable,integer)",
        "assignment(integer,variable)",
        ScoredOutcome::Mismatch,
        Some("child-order".to_string()),
    )?;

    assert_eq!(historical_score.outcome(), ScoredOutcome::Mismatch);
    assert_eq!(native_score.outcome(), ScoredOutcome::Mismatch);
    Ok(())
}

#[test]
fn native_diagnostic_output_is_recovered_execution_not_rejection() {
    let execution = execute_v3("my $x = ;");

    assert_eq!(
        execution.disposition(),
        ExecutionDisposition::AcceptedRecovered
    );
    assert!(execution.diagnostics().recovery_observed());
    assert!(execution.diagnostics().diagnostic_count() > 0);
}

#[test]
fn historical_error_nodes_are_recovery_not_instrument_failure() {
    let execution = execute_v1(
        "my $prefix = 1;\n@@@ this is garbage not perl @@@\nmy $suffix = 2;\n",
    );

    assert_eq!(
        execution.disposition(),
        ExecutionDisposition::AcceptedRecovered
    );
    assert!(execution.diagnostics().error_node_observed());
    assert_eq!(
        execution.observation(&ObservationPlane::Recovery),
        Some(ObservationAvailability::Observable)
    );
}

#[test]
fn scored_result_requires_an_observer_and_expected_fingerprint() {
    let missing_observer = ScoredComparison::scored(
        "",
        ObservationPlane::Structure,
        "expected",
        "actual",
        ScoredOutcome::Mismatch,
        None,
    );
    assert_eq!(missing_observer, Err(ComparisonModelError::MissingObserverId));

    let missing_expectation = ScoredComparison::scored(
        "structure.v1",
        ObservationPlane::Structure,
        "",
        "actual",
        ScoredOutcome::Mismatch,
        None,
    );
    assert_eq!(
        missing_expectation,
        Err(ComparisonModelError::MissingExpectedFingerprint)
    );
}

#[test]
fn a_match_cannot_carry_different_fingerprints() {
    let result = ScoredComparison::scored(
        "structure.v1",
        ObservationPlane::Structure,
        "expected",
        "actual",
        ScoredOutcome::MatchesExpected,
        None,
    );

    assert_eq!(
        result,
        Err(ComparisonModelError::MatchFingerprintMismatch)
    );
}

#[test]
fn missing_observer_is_explicitly_unscored_or_not_proven() -> Result<(), ComparisonModelError> {
    let comparison = ScoredComparison::unscored(
        ObservationPlane::SourceGeometry,
        ScoredOutcome::NotProven,
    )?;

    assert_eq!(comparison.observer_id(), None);
    assert_eq!(comparison.expected_fingerprint(), None);
    assert_eq!(comparison.outcome(), ScoredOutcome::NotProven);
    Ok(())
}

#[test]
fn pest_legacy_projection_is_unchanged_in_this_slice() {
    let result = parse_v2("my $x = 42;");
    assert_eq!(result.verdict, Verdict::Correct);
}

#[test]
fn observation_capabilities_have_deterministic_key_order() {
    let execution = execute_v3("my $x = 42;");
    let planes = execution.observations().keys().cloned().collect::<Vec<_>>();
    let mut sorted = planes.clone();
    sorted.sort();

    assert_eq!(planes, sorted);
}
