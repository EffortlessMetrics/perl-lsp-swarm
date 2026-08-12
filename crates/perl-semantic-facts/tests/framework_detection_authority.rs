use perl_semantic_facts::framework::{
    AdapterCancellation, AdapterDescriptor, AdapterDetectionInput, AdapterDetectionResult,
    AdapterDisposition, AdapterId, DetectionAbsenceReason, DetectionAuthorityError,
    DetectionConfigurationEvidence, DetectionOutcome, ModuleActivationIdentity,
    ModuleVersionEvidence,
};
use perl_semantic_facts::{Confidence, FileId, SourceGeneration};

fn descriptor(constraint: Option<&str>) -> AdapterDescriptor {
    AdapterDescriptor::new(
        AdapterId(1),
        "moo",
        "Moo",
        constraint.map(ToOwned::to_owned),
        1,
        AdapterDisposition::Production,
    )
}

fn module(name: &str, generation: &str, version: Option<&str>) -> ModuleActivationIdentity {
    let row = ModuleActivationIdentity::new(
        name,
        Some(FileId(7)),
        SourceGeneration::known(generation),
    );
    match version {
        Some(version) => row.with_observed_version(ModuleVersionEvidence::new(
            version,
            SourceGeneration::known(generation),
        )),
        None => row,
    }
}

fn input(modules: Vec<ModuleActivationIdentity>) -> AdapterDetectionInput {
    AdapterDetectionInput::new(
        descriptor(None),
        modules,
        SourceGeneration::known("project-1"),
        Some("sha256:input".into()),
        None,
        AdapterCancellation::active(),
    )
}

#[test]
fn raw_deserialized_shape_cannot_self_authorize_detection() {
    let result = AdapterDetectionResult::new(
        descriptor(None),
        SourceGeneration::known("project-1"),
        DetectionOutcome::Detected {
            confidence: Confidence::High,
            framework_version: None,
        },
    );
    let observed = input(vec![module("Moo", "project-1", None)]);
    assert_eq!(
        result.validate_authority_against(&observed),
        Err(DetectionAuthorityError::MissingInputIdentity)
    );
}

#[test]
fn required_module_present_invalidates_missing_module_verdict() {
    let observed = input(vec![module("Moo", "project-1", None)])
        .with_complete_module_observation();
    let result = AdapterDetectionResult::for_input(
        &observed,
        DetectionOutcome::Absent {
            reason: DetectionAbsenceReason::RequiredModulesMissing,
        },
    );
    assert_eq!(
        result.validate_authority_against(&observed),
        Err(DetectionAuthorityError::RequiredModulePresent)
    );
}

#[test]
fn partial_module_universe_cannot_prove_absence() {
    let observed = input(Vec::new());
    let result = AdapterDetectionResult::for_input(
        &observed,
        DetectionOutcome::Absent {
            reason: DetectionAbsenceReason::RequiredModulesMissing,
        },
    );
    assert_eq!(
        result.validate_authority_against(&observed),
        Err(DetectionAuthorityError::IncompleteModuleUniverse)
    );
}

#[test]
fn copied_result_cannot_authorize_another_input() {
    let first = input(vec![module("Moo", "project-1", None)]);
    let second = input(vec![module("Moo::Role", "project-1", None)]);
    let result = AdapterDetectionResult::for_input(
        &first,
        DetectionOutcome::Detected {
            confidence: Confidence::High,
            framework_version: None,
        },
    )
    .with_contributing_modules(first.available_modules.clone());
    assert_eq!(
        result.validate_authority_against(&second),
        Err(DetectionAuthorityError::InputIdentityMismatch)
    );
}

#[test]
fn version_string_without_observed_module_evidence_is_not_authority() {
    let mut observed = input(vec![module("Moo", "project-1", None)]);
    observed.descriptor = descriptor(Some(">=2"));
    let result = AdapterDetectionResult::for_input(
        &observed,
        DetectionOutcome::Detected {
            confidence: Confidence::High,
            framework_version: Some("2.1".into()),
        },
    )
    .with_contributing_modules(observed.available_modules.clone())
    .with_version_evidence(ModuleVersionEvidence::new(
        "2.1",
        SourceGeneration::known("project-1"),
    ));
    assert_eq!(
        result.validate_authority_against(&observed),
        Err(DetectionAuthorityError::InvalidVersionEvidence)
    );
}

#[test]
fn configuration_exclusion_requires_matching_typed_fact() {
    let observed = input(Vec::new()).with_complete_module_observation();
    let result = AdapterDetectionResult::for_input(
        &observed,
        DetectionOutcome::Absent {
            reason: DetectionAbsenceReason::ExcludedByConfiguration,
        },
    );
    assert_eq!(
        result.validate_authority_against(&observed),
        Err(DetectionAuthorityError::MissingConfigurationEvidence)
    );
}

#[test]
fn exact_current_detected_result_is_authoritative() {
    let activation = module("Moo", "project-1", Some("2.1"));
    let mut observed = input(vec![activation.clone()]);
    observed.descriptor = descriptor(Some(">=2"));
    let result = AdapterDetectionResult::for_input(
        &observed,
        DetectionOutcome::Detected {
            confidence: Confidence::High,
            framework_version: Some("2.1".into()),
        },
    )
    .with_contributing_modules(vec![activation])
    .with_version_evidence(ModuleVersionEvidence::new(
        "2.1",
        SourceGeneration::known("project-1"),
    ));
    assert!(result.is_authoritative_against(&observed));
}

#[test]
fn complete_current_missing_module_result_is_authoritative() {
    let observed = input(Vec::new()).with_complete_module_observation();
    let result = AdapterDetectionResult::for_input(
        &observed,
        DetectionOutcome::Absent {
            reason: DetectionAbsenceReason::RequiredModulesMissing,
        },
    );
    assert!(result.is_authoritative_against(&observed));
}

#[test]
fn exact_configuration_exclusion_is_authoritative() {
    let evidence = DetectionConfigurationEvidence::new(
        "frameworks.moo.disabled",
        "root:fixture",
        SourceGeneration::known("project-1"),
        "framework-config.v1",
    );
    let observed = input(Vec::new())
        .with_complete_module_observation()
        .with_configuration_evidence(vec![evidence.clone()]);
    let result = AdapterDetectionResult::for_input(
        &observed,
        DetectionOutcome::Absent {
            reason: DetectionAbsenceReason::ExcludedByConfiguration,
        },
    )
    .with_configuration_evidence(evidence);
    assert!(result.is_authoritative_against(&observed));
}

#[test]
fn duplicate_or_cross_generation_module_rows_fail_closed() {
    let duplicate = module("Moo", "project-1", None);
    let observed = input(vec![duplicate.clone(), duplicate]);
    let result = AdapterDetectionResult::for_input(
        &observed,
        DetectionOutcome::Detected {
            confidence: Confidence::High,
            framework_version: None,
        },
    );
    assert_eq!(
        result.validate_authority_against(&observed),
        Err(DetectionAuthorityError::InvalidModuleEvidence)
    );

    let stale = input(vec![module("Moo", "project-0", None)]);
    let result = AdapterDetectionResult::for_input(
        &stale,
        DetectionOutcome::Detected {
            confidence: Confidence::High,
            framework_version: None,
        },
    );
    assert_eq!(
        result.validate_authority_against(&stale),
        Err(DetectionAuthorityError::InvalidModuleEvidence)
    );
}
