//! Falsifiers for direct-call resolution assessment (#11138, slice 1).

use std::error::Error;

use perl_semantic_analyzer::analysis::direct_callable_resolution::derive_direct_callable_assessment;
use perl_semantic_facts::direct_callable::{
    DirectCallableCallShapeDisposition, DirectCallableCallSyntaxClass,
    DirectCallableCoverageDisposition, DirectCallableDisposition, DirectCallableDynamicBoundary,
    DirectCallableEvidenceAxes, DirectCallableLegacyInputKind, DirectCallableSubject,
    DirectCallableTargetResolution,
};
use perl_semantic_facts::{AnchorId, FileId, ScopeId, SourceGeneration};

type TestResult = Result<(), Box<dyn Error>>;

fn bare_subject() -> DirectCallableSubject {
    DirectCallableSubject {
        subject_id: "subj-bare".to_string(),
        document: FileId(7),
        call_anchor: AnchorId(99),
        syntax_class: DirectCallableCallSyntaxClass::BareParenthesizedFunctionCall,
        callee_spelling: "missing_sub".to_string(),
        owning_package: Some("Main".to_string()),
        lexical_scope: Some(ScopeId(3)),
        source_generation: SourceGeneration::known("gen-direct-call-1"),
        root_identity: "root-1".to_string(),
        query_snapshot_id: "snap-1".to_string(),
    }
}

fn complete_denominator_axes(legacy: DirectCallableLegacyInputKind) -> DirectCallableEvidenceAxes {
    DirectCallableEvidenceAxes {
        call_shape: DirectCallableCallShapeDisposition::ExactBareParenthesized,
        target: DirectCallableTargetResolution::NoneObserved,
        dynamic: DirectCallableDynamicBoundary::ProvenAbsentForSubject,
        coverage: DirectCallableCoverageDisposition::CompleteForCallClass,
        legacy_input: legacy,
        resolved_target: None,
        reason_ids: vec!["denominator.complete".to_string()],
    }
}

#[test]
fn direct_callable_resolution_empty_vector_never_exact_unresolved() -> TestResult {
    let assessment = derive_direct_callable_assessment(
        bare_subject(),
        complete_denominator_axes(DirectCallableLegacyInputKind::EmptyVectorAbsence),
    )?;
    assert_ne!(
        assessment.disposition,
        DirectCallableDisposition::ExactUnresolved,
        "empty Vec absence must not become exact_unresolved"
    );
    assert_eq!(assessment.disposition, DirectCallableDisposition::NotReadyOrIncomplete);
    Ok(())
}

#[test]
fn direct_callable_resolution_dynamic_none_never_exact_unresolved() -> TestResult {
    let assessment = derive_direct_callable_assessment(
        bare_subject(),
        complete_denominator_axes(DirectCallableLegacyInputKind::DynamicNoneAbsence),
    )?;
    assert_ne!(assessment.disposition, DirectCallableDisposition::ExactUnresolved);
    Ok(())
}

#[test]
fn direct_callable_resolution_method_call_never_exact_unresolved() -> TestResult {
    let mut subject = bare_subject();
    subject.syntax_class = DirectCallableCallSyntaxClass::MethodCall;
    subject.callee_spelling = "missing_method".to_string();
    let axes = DirectCallableEvidenceAxes {
        call_shape: DirectCallableCallShapeDisposition::Method,
        target: DirectCallableTargetResolution::NoneObserved,
        dynamic: DirectCallableDynamicBoundary::ProvenAbsentForSubject,
        coverage: DirectCallableCoverageDisposition::CompleteForCallClass,
        legacy_input: DirectCallableLegacyInputKind::None,
        resolved_target: None,
        reason_ids: vec![],
    };
    let assessment = derive_direct_callable_assessment(subject, axes)?;
    assert_ne!(assessment.disposition, DirectCallableDisposition::ExactUnresolved);
    assert_eq!(assessment.disposition, DirectCallableDisposition::UnsupportedCallShape);
    Ok(())
}

#[test]
fn direct_callable_resolution_external_unindexed_never_exact_unresolved() -> TestResult {
    let axes = DirectCallableEvidenceAxes {
        call_shape: DirectCallableCallShapeDisposition::ExactQualified,
        target: DirectCallableTargetResolution::NoneObserved,
        dynamic: DirectCallableDynamicBoundary::ProvenAbsentForSubject,
        coverage: DirectCallableCoverageDisposition::ExternalOrDependencyUnindexed,
        legacy_input: DirectCallableLegacyInputKind::None,
        resolved_target: None,
        reason_ids: vec!["external.unindexed".to_string()],
    };
    let assessment = derive_direct_callable_assessment(bare_subject(), axes)?;
    assert_eq!(assessment.disposition, DirectCallableDisposition::ExternalOrDependencyUnindexed);
    Ok(())
}

#[test]
fn direct_callable_resolution_complete_denominator_allows_exact_unresolved() -> TestResult {
    let assessment = derive_direct_callable_assessment(
        bare_subject(),
        complete_denominator_axes(DirectCallableLegacyInputKind::None),
    )?;
    assert_eq!(assessment.disposition, DirectCallableDisposition::ExactUnresolved);
    assessment.validate().map_err(|err| format!("assessment must validate: {err}"))?;
    Ok(())
}
