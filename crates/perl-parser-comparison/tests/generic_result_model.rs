use std::collections::BTreeMap;
use std::error::Error;

use perl_parser_comparison::{
    BoundedText, ComparisonModelError, ConformanceOutcome, DiagnosticSummary, DivergencePath,
    EvidenceValueError, HarnessFailure, HarnessOutcome, InstrumentState, MismatchClass,
    MismatchDetail, NonDecisiveOutcome, ObservationDisposition, ObservationPlane, ObserverId,
    ParserLabel, ReviewedExpectationId, ScoredComparison, SemanticFingerprint, StableId,
    SubjectDisposition, SubjectExecution, SubjectRole, Verdict, execute_v1, execute_v3, parse_v1,
    parse_v2, parse_v3,
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
    assert_eq!(execution.subject_disposition(), Some(&SubjectDisposition::AcceptedClean));

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

    assert_eq!(historical.subject_disposition(), Some(&SubjectDisposition::AcceptedClean));
    assert_eq!(native.subject_disposition(), Some(&SubjectDisposition::AcceptedClean));

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
    assert_eq!(execution.subject_disposition(), Some(&SubjectDisposition::AcceptedRecovered));
    assert!(execution.diagnostics().recovery_observed());
    assert!(execution.diagnostics().diagnostic_count() > 0);
    Ok(())
}

#[test]
fn historical_error_nodes_are_recovery_not_instrument_failure() -> Result<(), Box<dyn Error>> {
    let execution =
        execute_v1("my $prefix = 1;\n@@@ this is garbage not perl @@@\nmy $suffix = 2;\n")?;

    assert_eq!(execution.harness(), HarnessOutcome::Completed);
    assert_eq!(execution.subject_disposition(), Some(&SubjectDisposition::AcceptedRecovered));
    assert!(execution.diagnostics().error_node_observed());
    assert_eq!(
        execution.observation(&ObservationPlane::Recovery),
        Some(ObservationDisposition::Observed)
    );
    assert_eq!(execution.instrument_state(), InstrumentState::Complete);
    Ok(())
}

#[test]
fn failed_harness_cannot_carry_subject_disposition_or_decisive_score() -> Result<(), Box<dyn Error>>
{
    let execution = SubjectExecution::failed(
        SubjectRole::NativeRecursiveDescent,
        HarnessFailure::TimedOut,
        DiagnosticSummary::default(),
        BTreeMap::from([(ObservationPlane::Structure, ObservationDisposition::NotProven)]),
        None,
        InstrumentState::Failed,
        Some(BoundedText::new("deadline exceeded", 64)?),
    )?;

    assert_eq!(execution.harness(), HarnessOutcome::Failed(HarnessFailure::TimedOut));
    assert_eq!(execution.subject_disposition(), None);

    let comparison = ScoredComparison::matches_expected(
        &execution,
        observer_id()?,
        expectation_id()?,
        ObservationPlane::Structure,
        fingerprint("assignment(variable,integer)")?,
        fingerprint("assignment(variable,integer)")?,
    );
    assert_eq!(comparison, Err(ComparisonModelError::ScoringRequiresCompletedHarness));
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
    assert_eq!(comparison, Err(ComparisonModelError::ScoringRequiresCompleteInstrument));
    Ok(())
}

#[test]
fn limited_observation_cannot_carry_decisive_score() -> Result<(), Box<dyn Error>> {
    let execution = SubjectExecution::completed(
        SubjectRole::NativeRecursiveDescent,
        SubjectDisposition::AcceptedClean,
        DiagnosticSummary::default(),
        BTreeMap::from([(
            ObservationPlane::Structure,
            ObservationDisposition::ObservedWithLimitations,
        )]),
        None,
        InstrumentState::Complete,
    )?;

    let comparison = ScoredComparison::matches_expected(
        &execution,
        observer_id()?,
        expectation_id()?,
        ObservationPlane::Structure,
        fingerprint("assignment(variable,integer)")?,
        fingerprint("assignment(variable,integer)")?,
    );
    assert_eq!(comparison, Err(ComparisonModelError::ScoringRequiresObservedPlane));
    Ok(())
}

#[test]
fn unobserved_planes_cannot_carry_decisive_score() -> Result<(), Box<dyn Error>> {
    let execution = execute_v3("my $x = 42;")?;
    assert_eq!(
        execution.observation(&ObservationPlane::SourceGeometry),
        Some(ObservationDisposition::NotProven)
    );
    assert_eq!(
        execution.observation(&ObservationPlane::QueryOrHighlight),
        Some(ObservationDisposition::Unsupported)
    );

    for plane in [
        ObservationPlane::SourceGeometry,
        ObservationPlane::QueryOrHighlight,
        ObservationPlane::BodyOwnership,
    ] {
        let comparison = ScoredComparison::mismatch(
            &execution,
            observer_id()?,
            expectation_id()?,
            plane,
            fingerprint("expected")?,
            fingerprint("actual")?,
            wrong_child_order()?,
        );
        assert_eq!(comparison, Err(ComparisonModelError::ScoringRequiresObservedPlane));
    }
    Ok(())
}

#[test]
fn mismatch_scoring_also_requires_completed_harness() -> Result<(), Box<dyn Error>> {
    let execution = SubjectExecution::failed(
        SubjectRole::HistoricalTreeSitterC,
        HarnessFailure::SetupFailed,
        DiagnosticSummary::default(),
        BTreeMap::from([(ObservationPlane::Structure, ObservationDisposition::NotProven)]),
        None,
        InstrumentState::Unavailable,
        None,
    )?;

    let comparison = ScoredComparison::mismatch(
        &execution,
        observer_id()?,
        expectation_id()?,
        ObservationPlane::Structure,
        fingerprint("expected")?,
        fingerprint("actual")?,
        wrong_child_order()?,
    );
    assert_eq!(comparison, Err(ComparisonModelError::ScoringRequiresCompletedHarness));
    Ok(())
}

#[test]
fn failed_harness_rejects_observed_plane_and_complete_instrument() {
    let observed_plane = SubjectExecution::failed(
        SubjectRole::NativeRecursiveDescent,
        HarnessFailure::CrashedOrSignalled,
        DiagnosticSummary::default(),
        BTreeMap::from([(ObservationPlane::Structure, ObservationDisposition::Observed)]),
        None,
        InstrumentState::Failed,
        None,
    );
    assert_eq!(observed_plane, Err(ComparisonModelError::ObservationFromFailedHarness));

    let complete_instrument = SubjectExecution::failed(
        SubjectRole::NativeRecursiveDescent,
        HarnessFailure::TimedOut,
        DiagnosticSummary::default(),
        BTreeMap::from([(ObservationPlane::Structure, ObservationDisposition::NotProven)]),
        None,
        InstrumentState::Complete,
        None,
    );
    assert_eq!(complete_instrument, Err(ComparisonModelError::CompleteInstrumentFromFailedHarness));
}

#[test]
fn stable_ids_reject_empty_leading_separator_non_ascii_and_overlong_values() {
    assert!(StableId::new("").is_err());
    assert!(StableId::new(".leading-dot").is_err());
    assert!(StableId::new("-leading-dash").is_err());
    assert!(StableId::new("_leading_underscore").is_err());
    assert!(StableId::new("café").is_err());
    assert!(StableId::new("a".repeat(129)).is_err());
    assert!(StableId::new("9lives").is_ok());
}

#[test]
fn bounded_text_rejects_zero_limit_and_keeps_short_input_intact() -> Result<(), Box<dyn Error>> {
    assert!(BoundedText::new("anything", 0).is_err());

    let bounded = BoundedText::new("short", 1_024)?;
    assert_eq!(bounded.as_str(), "short");
    assert!(!bounded.is_truncated());
    assert_eq!(bounded.omitted_bytes(), 0);
    Ok(())
}

#[test]
fn complete_observation_requires_complete_instrument() {
    let execution = SubjectExecution::completed(
        SubjectRole::NativeRecursiveDescent,
        SubjectDisposition::AcceptedClean,
        DiagnosticSummary::default(),
        BTreeMap::from([(ObservationPlane::Structure, ObservationDisposition::Observed)]),
        None,
        InstrumentState::Partial,
    );

    assert_eq!(execution, Err(ComparisonModelError::ObservedFromIncompleteInstrument));
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
    assert_eq!(false_match, Err(ComparisonModelError::MatchFingerprintMismatch));

    let false_mismatch = ScoredComparison::mismatch(
        &execution,
        observer_id()?,
        expectation_id()?,
        ObservationPlane::Structure,
        fingerprint("same")?,
        fingerprint("same")?,
        wrong_child_order()?,
    );
    assert_eq!(false_mismatch, Err(ComparisonModelError::MismatchFingerprintMatch));
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

#[test]
fn checked_id_and_fingerprint_display_surface_is_stable() -> Result<(), Box<dyn Error>> {
    let id = StableId::new("stable.id-v1")?;
    assert_eq!(id.as_str(), "stable.id-v1");
    assert_eq!(id.to_string(), "stable.id-v1");

    let observer = observer_id()?;
    assert_eq!(observer.as_str(), "assignment-shape.v1");
    assert_eq!(observer.to_string(), "assignment-shape.v1");
    assert_eq!(observer.stable_id().as_str(), "assignment-shape.v1");

    let expectation = expectation_id()?;
    assert_eq!(expectation.as_str(), "assignment-shape.expected.v1");
    assert_eq!(expectation.to_string(), "assignment-shape.expected.v1");
    assert_eq!(expectation.stable_id().as_str(), "assignment-shape.expected.v1");

    let fingerprint = fingerprint("assignment(variable,integer)")?;
    assert_eq!(fingerprint.as_str(), "assignment(variable,integer)");
    assert_eq!(fingerprint.to_string(), "assignment(variable,integer)");

    let path = DivergencePath::new("children[0]")?;
    assert_eq!(path.as_str(), "children[0]");
    assert_eq!(path.to_string(), "children[0]");
    Ok(())
}

#[test]
fn legacy_label_and_verdict_display_is_unchanged() {
    assert_eq!(ParserLabel::V1TreeSitterC.to_string(), "v1(tree-sitter-c)");
    assert_eq!(ParserLabel::V2Pest.to_string(), "v2(pest)");
    assert_eq!(ParserLabel::V3RecursiveDescent.to_string(), "v3(recursive-descent)");

    assert_eq!(Verdict::Correct.to_string(), "Correct");
    assert_eq!(Verdict::WrongButPlausible.to_string(), "WrongButPlausible");
    assert_eq!(Verdict::SilentlyEmpty.to_string(), "SilentlyEmpty");
    assert_eq!(Verdict::Errors.to_string(), "Errors");
    assert_eq!(Verdict::Crashes.to_string(), "Crashes");
}

#[test]
fn model_error_display_and_source_are_stable() {
    let cases = [
        (
            ComparisonModelError::CompleteInstrumentFromFailedHarness,
            "complete subject instrumentation",
        ),
        (ComparisonModelError::ObservationFromFailedHarness, "observed comparison plane"),
        (ComparisonModelError::ObservedFromIncompleteInstrument, "complete instrumentation"),
        (ComparisonModelError::LimitedObservationFromUnusableInstrument, "limited observation"),
        (ComparisonModelError::ScoringRequiresCompletedHarness, "completed harness execution"),
        (ComparisonModelError::ScoringRequiresCompleteInstrument, "complete instrumentation"),
        (ComparisonModelError::ScoringRequiresObservedPlane, "exactly observed plane"),
        (ComparisonModelError::MatchFingerprintMismatch, "identical expected and actual"),
        (ComparisonModelError::MismatchFingerprintMatch, "different expected and actual"),
    ];
    for (error, needle) in cases {
        assert!(error.to_string().contains(needle), "{error:?}");
        assert!(error.source().is_none(), "{error:?}");
    }

    let wrapped =
        ComparisonModelError::from(EvidenceValueError::ZeroLimit { kind: "bounded_text" });
    assert!(wrapped.to_string().contains("non-zero byte limit"));
    assert!(wrapped.source().is_some());

    let value_errors: [EvidenceValueError; 4] = [
        EvidenceValueError::Empty { kind: "stable_id" },
        EvidenceValueError::TooLong { kind: "stable_id", actual: 200, maximum: 128 },
        EvidenceValueError::InvalidCharacter { kind: "stable_id", index: 3, character: 'X' },
        EvidenceValueError::ZeroLimit { kind: "bounded_text" },
    ];
    let needles = ["must not be empty", "200 bytes", "invalid character", "non-zero byte limit"];
    for (error, needle) in value_errors.iter().zip(needles) {
        assert!(error.to_string().contains(needle), "{error:?}");
    }
}

#[test]
fn registered_extension_variants_participate_in_scoring() -> Result<(), Box<dyn Error>> {
    let plane = ObservationPlane::Registered(StableId::new("custom-plane.v1")?);
    let execution = SubjectExecution::completed(
        SubjectRole::ExperimentalPest,
        SubjectDisposition::Registered(StableId::new("custom-disposition.v1")?),
        DiagnosticSummary::default(),
        BTreeMap::from([(plane.clone(), ObservationDisposition::Observed)]),
        None,
        InstrumentState::Complete,
    )?;

    let comparison = ScoredComparison::matches_expected(
        &execution,
        observer_id()?,
        expectation_id()?,
        plane,
        fingerprint("same")?,
        fingerprint("same")?,
    )?;
    assert_eq!(comparison.outcome(), ConformanceOutcome::MatchesExpected);

    let mismatch = MismatchDetail::new(
        MismatchClass::Registered(StableId::new("custom-mismatch.v1")?),
        DivergencePath::new("root")?,
    );
    assert_eq!(mismatch.first_divergence().as_str(), "root");
    assert!(matches!(mismatch.class(), MismatchClass::Registered(_)));
    Ok(())
}

#[test]
fn execution_and_comparison_accessors_expose_every_axis() -> Result<(), Box<dyn Error>> {
    let execution = execute_v3("my $x = 42;")?;
    assert_eq!(execution.subject(), SubjectRole::NativeRecursiveDescent);
    assert_eq!(execution.harness(), HarnessOutcome::Completed);
    assert_eq!(execution.subject_disposition(), Some(&SubjectDisposition::AcceptedClean));
    assert_eq!(execution.instrument_state(), InstrumentState::Complete);
    assert_eq!(
        execution.observation(&ObservationPlane::Structure),
        Some(ObservationDisposition::Observed)
    );
    assert!(!execution.observations().is_empty());
    assert_eq!(execution.diagnostics().diagnostic_count(), 0);
    assert!(!execution.diagnostics().recovery_observed());
    assert!(!execution.diagnostics().error_node_observed());
    let projection = execution.debug_projection().expect("clean parse keeps a debug projection");
    assert!(projection.original_bytes() > 0);
    assert_eq!(projection.as_str().len() + projection.omitted_bytes(), projection.original_bytes());
    assert!(execution.error().is_none());

    let comparison = ScoredComparison::matches_expected(
        &execution,
        observer_id()?,
        expectation_id()?,
        ObservationPlane::Structure,
        fingerprint("assignment(variable,integer)")?,
        fingerprint("assignment(variable,integer)")?,
    )?;
    assert_eq!(comparison.observer_id().map(ObserverId::as_str), Some("assignment-shape.v1"));
    assert_eq!(
        comparison.expectation_id().map(ReviewedExpectationId::as_str),
        Some("assignment-shape.expected.v1")
    );
    assert_eq!(comparison.plane(), &ObservationPlane::Structure);
    assert_eq!(
        comparison.expected_fingerprint().map(SemanticFingerprint::as_str),
        Some("assignment(variable,integer)")
    );
    assert_eq!(
        comparison.actual_fingerprint().map(SemanticFingerprint::as_str),
        Some("assignment(variable,integer)")
    );
    assert!(comparison.mismatch_detail().is_none());
    Ok(())
}

#[test]
fn failed_execution_accessors_expose_bounded_error_text() -> Result<(), Box<dyn Error>> {
    let execution = SubjectExecution::failed(
        SubjectRole::HistoricalTreeSitterC,
        HarnessFailure::SetupFailed,
        DiagnosticSummary::default(),
        BTreeMap::from([(ObservationPlane::Structure, ObservationDisposition::NotProven)]),
        None,
        InstrumentState::Unavailable,
        Some(BoundedText::new("language load failed", 64)?),
    )?;

    assert_eq!(execution.subject(), SubjectRole::HistoricalTreeSitterC);
    assert_eq!(execution.harness(), HarnessOutcome::Failed(HarnessFailure::SetupFailed));
    assert_eq!(execution.error().map(BoundedText::as_str), Some("language load failed"));
    assert!(execution.debug_projection().is_none());
    assert_eq!(execution.instrument_state(), InstrumentState::Unavailable);
    Ok(())
}

#[test]
fn non_decisive_outcomes_project_into_conformance_outcomes() {
    assert_eq!(
        ConformanceOutcome::from(NonDecisiveOutcome::Unscored),
        ConformanceOutcome::Unscored
    );
    assert_eq!(ConformanceOutcome::from(NonDecisiveOutcome::Unknown), ConformanceOutcome::Unknown);
    assert_eq!(
        ConformanceOutcome::from(NonDecisiveOutcome::NotProven),
        ConformanceOutcome::NotProven
    );

    for outcome in [NonDecisiveOutcome::Unscored, NonDecisiveOutcome::Unknown] {
        let comparison = ScoredComparison::non_decisive(ObservationPlane::Recovery, outcome);
        assert_eq!(comparison.outcome(), ConformanceOutcome::from(outcome));
        assert!(comparison.actual_fingerprint().is_none());
    }
}

#[test]
fn limited_observation_rejects_unusable_instrument_states() {
    for instrument in
        [InstrumentState::Unavailable, InstrumentState::Failed, InstrumentState::SchemaMismatch]
    {
        let execution = SubjectExecution::completed(
            SubjectRole::NativeRecursiveDescent,
            SubjectDisposition::AcceptedClean,
            DiagnosticSummary::default(),
            BTreeMap::from([(
                ObservationPlane::Structure,
                ObservationDisposition::ObservedWithLimitations,
            )]),
            None,
            instrument,
        );
        assert_eq!(
            execution,
            Err(ComparisonModelError::LimitedObservationFromUnusableInstrument),
            "{instrument:?}"
        );
    }
}

#[test]
fn legacy_verdict_bridge_projects_each_reachable_arm() {
    let clean_v1 = parse_v1("my $x = 42;");
    assert_eq!(clean_v1.verdict, Verdict::Correct);
    assert!(clean_v1.sexp_contains("scalar"));

    let clean_v3 = parse_v3("my $x = 42;");
    assert_eq!(clean_v3.verdict, Verdict::Correct);

    let recovered_v1 = parse_v1("@@@ this is garbage not perl @@@");
    assert_eq!(recovered_v1.verdict, Verdict::Errors);

    let recovered_v3 = parse_v3("my $x = ;");
    assert_eq!(recovered_v3.verdict, Verdict::Errors);

    let rejected_v2 = parse_v2("@@@ this is garbage not perl @@@");
    assert_eq!(rejected_v2.verdict, Verdict::Errors);
}
