use std::collections::BTreeMap;
use std::error::Error;

use perl_parser_comparison::{
    BoundedText, ComparisonModelError, ConformanceOutcome, DiagnosticSummary, DivergencePath,
    EvidenceValueError, HarnessFailure, HarnessOutcome, InstrumentState, MismatchClass,
    MismatchDetail, NonDecisiveOutcome, ObservationDisposition, ObservationPlane, ObserverId,
    ReviewedExpectationId, ScoredComparison, SemanticFingerprint, StableId, SubjectDisposition,
    SubjectExecution, SubjectRole, Verdict, execute_v1, execute_v3, parse_v2,
};

fn observer_id() -> Result<ObserverId, EvidenceValueError> {
    ObserverId::new("assignment-shape.v1")
}

fn expectation_id() -> Result<ReviewedExpectationId, EvidenceValueError> {
    ReviewedExpectationId::new("assignment-shape.expected.v1")
}

fn fingerprint(value: &str) -> Result<SemanticFingerprint, EvidenceValueError> {
    SemanticFingerprint::new(value)
}

fn wrong_child_order() -> Result<MismatchDetail, EvidenceValueError> {
    DivergencePath::new("children[0]")
        .map(|path| MismatchDetail::new(MismatchClass::WrongOrderOrOwnership, path))
}

#[test]
fn clean_execution_can_still_score_a_structural_mismatch() -> Result<(), Box<dyn Error>> {
    let execution = execute_v3("my $x = 42;")?;
    assert_eq!(execution.subject(), SubjectRole::NativeRecursiveDescent);
    assert_eq!(execution.harness(), HarnessOutcome::Completed);
    assert_eq!(
        execution.subject_disposition(),
        Some(&SubjectDisposition::AcceptedClean)
    );

    let comparison = ScoredComparison::mismatch(
        &execution,
        observer_id()?,
        expectation_id()?,
        ObservationPlane::Structure,
        fingerprint("assignment(expected-child-order)")?,
        fingerprint("assignment(reversed-child-order)")?,
        wrong_child_order()?,
    )?;

    assert_eq!(comparison.outcome(), ConformanceOutcome::Mismatch);
    assert_eq!(
        comparison
            .mismatch_detail()
            .map(MismatchDetail::first_divergence)
            .map(DivergencePath::as_str),
        Some("children[0]")
    );
    Ok(())
}

#[test]
fn two_clean_subjects_do_not_create_correctness_agreement() -> Result<(), Box<dyn Error>> {
    let historical = execute_v1("my $x = 42;")?;
    let native = execute_v3("my $x = 42;")?;

    assert_eq!(
        historical.subject_disposition(),
        Some(&SubjectDisposition::AcceptedClean)
    );
    assert_eq!(
        native.subject_disposition(),
        Some(&SubjectDisposition::AcceptedClean)
    );

    for execution in [&historical, &native] {
        let comparison = ScoredComparison::mismatch(
            execution,
            observer_id()?,
            expectation_id()?,
            ObservationPlane::Structure,
            fingerprint("assignment(variable,integer)")?,
            fingerprint("assignment(integer,variable)")?,
            wrong_child_order()?,
        )?;
        assert_eq!(comparison.outcome(), ConformanceOutcome::Mismatch);
    }
    Ok(())
}

#[test]
fn native_diagnostic_output_is_recovered_execution_not_rejection() -> Result<(), Box<dyn Error>> {
    let execution = execute_v3("my $x = ;")?;

    assert_eq!(execution.harness(), HarnessOutcome::Completed);
    assert_eq!(
        execution.subject_disposition(),
        Some(&SubjectDisposition::AcceptedRecovered)
    );
    assert!(execution.diagnostics().recovery_observed());
    assert!(execution.diagnostics().diagnostic_count() > 0);
    Ok(())
}

#[test]
fn historical_error_nodes_are_recovery_not_instrument_failure() -> Result<(), Box<dyn Error>> {
    let execution = execute_v1(
        "my $prefix = 1;\n@@@ this is garbage not perl @@@\nmy $suffix = 2;\n",
    )?;

    assert_eq!(execution.harness(), HarnessOutcome::Completed);
    assert_eq!(
        execution.subject_disposition(),
        Some(&SubjectDisposition::AcceptedRecovered)
    );
    assert!(execution.diagnostics().error_node_observed());
    assert_eq!(
        execution.observation(&ObservationPlane::Recovery),
        Some(ObservationDisposition::Observed)
    );
    assert_eq!(execution.instrument_state(), InstrumentState::Complete);
    Ok(())
}

#[test]
fn failed_harness_cannot_carry_subject_disposition_or_decisive_score(
) -> Result<(), Box<dyn Error>> {
    let execution = SubjectExecution::failed(
        SubjectRole::NativeRecursiveDescent,
        HarnessFailure::TimedOut,
        DiagnosticSummary::default(),
        BTreeMap::from([(
            ObservationPlane::Structure,
            ObservationDisposition::NotProven,
        )]),
        None,
        InstrumentState::Failed,
        Some(BoundedText::new("deadline exceeded", 64)?),
    )?;

    assert_eq!(
        execution.harness(),
        HarnessOutcome::Failed(HarnessFailure::TimedOut)
    );
    assert_eq!(execution.subject_disposition(), None);

    let comparison = ScoredComparison::matches_expected(
        &execution,
        observer_id()?,
        expectation_id()?,
        ObservationPlane::Structure,
        fingerprint("assignment(variable,integer)")?,
        fingerprint("assignment(variable,integer)")?,
    );
    assert_eq!(
        comparison,
        Err(ComparisonModelError::ScoringRequiresCompletedHarness)
    );
    Ok(())
}

#[test]
fn incomplete_instrument_cannot_carry_decisive_score() -> Result<(), Box<dyn Error>> {
    let execution = SubjectExecution::completed(
        SubjectRole::NativeRecursiveDescent,
        SubjectDisposition::AcceptedClean,
        DiagnosticSummary::default(),
        BTreeMap::from([(
            ObservationPlane::Structure,
            ObservationDisposition::ObservedWithLimitations,
        )]),
        None,
        InstrumentState::Partial,
    )?;

    let comparison = ScoredComparison::matches_expected(
        &execution,
        observer_id()?,
        expectation_id()?,
        ObservationPlane::Structure,
        fingerprint("assignment(variable,integer)")?,
        fingerprint("assignment(variable,integer)")?,
    );
    assert_eq!(
        comparison,
        Err(ComparisonModelError::ScoringRequiresCompleteInstrument)
    );
    Ok(())
}

#[test]
fn complete_observation_requires_complete_instrument() {
    let execution = SubjectExecution::completed(
        SubjectRole::NativeRecursiveDescent,
        SubjectDisposition::AcceptedClean,
        DiagnosticSummary::default(),
        BTreeMap::from([(
            ObservationPlane::Structure,
            ObservationDisposition::Observed,
        )]),
        None,
        InstrumentState::Partial,
    );

    assert_eq!(
        execution,
        Err(ComparisonModelError::ObservedFromIncompleteInstrument)
    );
}

#[test]
fn match_and_mismatch_fingerprints_are_consistent() -> Result<(), Box<dyn Error>> {
    let execution = execute_v3("my $x = 42;")?;

    let false_match = ScoredComparison::matches_expected(
        &execution,
        observer_id()?,
        expectation_id()?,
        ObservationPlane::Structure,
        fingerprint("expected")?,
        fingerprint("actual")?,
    );
    assert_eq!(
        false_match,
        Err(ComparisonModelError::MatchFingerprintMismatch)
    );

    let false_mismatch = ScoredComparison::mismatch(
        &execution,
        observer_id()?,
        expectation_id()?,
        ObservationPlane::Structure,
        fingerprint("same")?,
        fingerprint("same")?,
        wrong_child_order()?,
    );
    assert_eq!(
        false_mismatch,
        Err(ComparisonModelError::MismatchFingerprintMatch)
    );
    Ok(())
}

#[test]
fn missing_authority_is_explicitly_non_decisive() {
    let comparison = ScoredComparison::non_decisive(
        ObservationPlane::SourceGeometry,
        NonDecisiveOutcome::NotProven,
    );

    assert_eq!(comparison.observer_id(), None);
    assert_eq!(comparison.expectation_id(), None);
    assert_eq!(comparison.expected_fingerprint(), None);
    assert_eq!(comparison.outcome(), ConformanceOutcome::NotProven);
}

#[test]
fn stable_ids_reject_free_form_extension_strings() {
    assert!(StableId::new("Uppercase").is_err());
    assert!(StableId::new("contains space").is_err());
    assert!(StableId::new("valid.reason-v1").is_ok());
}

#[test]
fn bounded_text_exposes_omitted_bytes_without_splitting_utf8() -> Result<(), Box<dyn Error>> {
    let bounded = BoundedText::new("ééé", 3)?;

    assert_eq!(bounded.as_str(), "é");
    assert_eq!(bounded.original_bytes(), 6);
    assert_eq!(bounded.omitted_bytes(), 4);
    assert!(bounded.is_truncated());
    Ok(())
}

#[test]
fn pest_legacy_projection_is_unchanged_in_this_slice() {
    let result = parse_v2("my $x = 42;");
    assert_eq!(result.verdict, Verdict::Correct);
}

#[test]
fn observation_dispositions_have_deterministic_key_order() -> Result<(), Box<dyn Error>> {
    let execution = execute_v3("my $x = 42;")?;
    let planes = execution.observations().keys().cloned().collect::<Vec<_>>();
    let mut sorted = planes.clone();
    sorted.sort();

    assert_eq!(planes, sorted);
    Ok(())
}
