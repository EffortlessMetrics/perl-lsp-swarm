use perl_semantic_facts::framework::{
    AdapterCancellation, AdapterDescriptor, AdapterDetectionInput, AdapterDetectionResult,
    AdapterDisposition, AdapterId, DetectionAbsenceReason, DetectionAuthorityError,
    DetectionConfigurationEvidence, DetectionConfigurationObservation, DetectionConfigurationValue,
    DetectionEvidenceClass, DetectionOutcome, ModuleActivationIdentity, ModuleObservationReceipt,
    ModuleSelectorEvaluation, ModuleVersionEvidence,
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

fn configuration_descriptor() -> AdapterDescriptor {
    descriptor(None).with_configuration_exclusion(
        "frameworks.moo.disabled",
        DetectionConfigurationValue::Boolean(true),
        "exclude-moo-when-disabled.v1",
    )
}

fn module(name: &str, generation: &str, version: Option<&str>) -> ModuleActivationIdentity {
    let row =
        ModuleActivationIdentity::new(name, Some(FileId(7)), SourceGeneration::known(generation));
    match version {
        Some(version) => row.with_observed_version(ModuleVersionEvidence::new(
            version,
            SourceGeneration::known(generation),
        )),
        None => row,
    }
}

fn observation(evaluations: Vec<ModuleSelectorEvaluation>) -> ModuleObservationReceipt {
    ModuleObservationReceipt::new(
        "module-resolver.v1",
        "root:fixture",
        "project-environment.v1",
        SourceGeneration::known("project-1"),
        "sha256:input",
        evaluations,
    )
}

fn input(evaluations: Vec<ModuleSelectorEvaluation>) -> AdapterDetectionInput {
    AdapterDetectionInput::new(
        descriptor(None),
        observation(evaluations),
        None,
        AdapterCancellation::active(),
    )
}

const LEGACY_ADAPTER_DETECTION_INPUT_JSON: &str = r#"
{
  "descriptor": {
    "adapter_id": 1,
    "name": "moo",
    "framework_name": "Moo",
    "framework_version_constraint": null,
    "schema_version": 1,
    "disposition": "Production"
  },
  "available_modules": [
    {
      "module_name": "Moo",
      "file_id": 7,
      "generation": {"Known": "project-1"},
      "observed_version": null
    }
  ],
  "project_generation": {"Known": "project-1"},
  "content_digest": null,
  "budget": null,
  "cancellation": {"is_cancelled": false}
}
"#;

const CURRENT_ADAPTER_DETECTION_INPUT_JSON: &str = r#"
{
  "descriptor": {
    "adapter_id": 1,
    "name": "moo",
    "framework_name": "Moo",
    "required_module_selectors": ["Moo"],
    "framework_version_constraint": null,
    "configuration_exclusion_key": null,
    "configuration_exclusion_value": null,
    "configuration_exclusion_rule": null,
    "schema_version": 1,
    "disposition": "Production"
  },
  "module_observation": {
    "schema_version": 1,
    "resolver_identity": "module-resolver.v1",
    "scope_identity": "root:fixture",
    "environment_identity": "project-environment.v1",
    "generation": {"Known": "project-1"},
    "content_digest": "sha256:input",
    "evaluations": [
      {
        "selector": "Moo",
        "outcome": {
          "Matched": {
            "activation": {
              "module_name": "Moo",
              "file_id": 7,
              "generation": {"Known": "project-1"},
              "observed_version": null
            },
            "evidence_class": "ResolvedModule"
          }
        }
      }
    ]
  },
  "configuration_observations": [],
  "detector_policy_identity": "framework_adapter_sdk.v1",
  "budget": null,
  "cancellation": {"is_cancelled": false}
}
"#;

#[test]
fn current_sdk_json_fixture_loads_as_authority_input() -> Result<(), serde_json::Error> {
    let input: AdapterDetectionInput = serde_json::from_str(CURRENT_ADAPTER_DETECTION_INPUT_JSON)?;

    assert_eq!(input.module_observation.schema_version, 1);
    assert_eq!(input.detector_policy_identity, "framework_adapter_sdk.v1");
    assert_eq!(input.descriptor.required_module_selectors, vec!["Moo".to_string()]);
    assert_eq!(input.module_observation.evaluations.len(), 1);
    Ok(())
}

#[test]
fn legacy_sdk_json_fixture_loads_but_cannot_claim_current_authority()
-> Result<(), serde_json::Error> {
    let input: AdapterDetectionInput = serde_json::from_str(LEGACY_ADAPTER_DETECTION_INPUT_JSON)?;

    assert_eq!(input.project_generation(), &SourceGeneration::known("project-1"));
    assert_eq!(input.descriptor.required_module_selectors, vec!["Moo".to_string()]);
    assert_eq!(input.module_observation.evaluations.len(), 1);

    let result = AdapterDetectionResult::for_input(
        &input,
        DetectionOutcome::Detected { confidence: Confidence::High, framework_version: None },
    );
    assert_eq!(
        result.validate_authority_against(&input),
        Err(DetectionAuthorityError::UnsupportedSchema)
    );
    Ok(())
}

fn configuration_input(evaluations: Vec<ModuleSelectorEvaluation>) -> AdapterDetectionInput {
    AdapterDetectionInput::new(
        configuration_descriptor(),
        observation(evaluations),
        None,
        AdapterCancellation::active(),
    )
}

fn matched_moo(
    version: Option<&str>,
    evidence_class: DetectionEvidenceClass,
) -> (ModuleSelectorEvaluation, ModuleActivationIdentity) {
    let activation = module("Moo", "project-1", version);
    (ModuleSelectorEvaluation::matched("Moo", activation.clone(), evidence_class), activation)
}

fn configuration_observation(value: bool) -> DetectionConfigurationObservation {
    DetectionConfigurationObservation::new(
        "workspace-config:perl-lsp.toml",
        "sha256:configuration",
        "frameworks.moo.disabled",
        DetectionConfigurationValue::Boolean(value),
        "root:fixture",
        SourceGeneration::known("project-1"),
        "project-environment-config.v1",
        "framework-config.v1",
    )
}

#[test]
fn raw_deserialized_shape_cannot_self_authorize_detection() {
    let result = AdapterDetectionResult::new(
        descriptor(None),
        SourceGeneration::known("project-1"),
        DetectionOutcome::Detected { confidence: Confidence::High, framework_version: None },
    );
    let (evaluation, _) = matched_moo(None, DetectionEvidenceClass::ResolvedModule);
    let observed = input(vec![evaluation]);
    assert_eq!(
        result.validate_authority_against(&observed),
        Err(DetectionAuthorityError::MissingInputIdentity)
    );
}

#[test]
fn required_module_present_invalidates_missing_module_verdict() {
    let (evaluation, _) = matched_moo(None, DetectionEvidenceClass::ResolvedModule);
    let observed = input(vec![evaluation]);
    let result = AdapterDetectionResult::for_input(
        &observed,
        DetectionOutcome::Absent { reason: DetectionAbsenceReason::RequiredModulesMissing },
    );
    assert_eq!(
        result.validate_authority_against(&observed),
        Err(DetectionAuthorityError::RequiredModulePresent)
    );
}

#[test]
fn unresolved_selector_cannot_prove_absence() {
    let observed =
        input(vec![ModuleSelectorEvaluation::unresolved("Moo", "module resolver unavailable")]);
    let result = AdapterDetectionResult::for_input(
        &observed,
        DetectionOutcome::Absent { reason: DetectionAbsenceReason::RequiredModulesMissing },
    );
    assert_eq!(
        result.validate_authority_against(&observed),
        Err(DetectionAuthorityError::IncompleteModuleUniverse)
    );
}

#[test]
fn copied_result_cannot_authorize_another_input() {
    let (first_evaluation, first_activation) =
        matched_moo(None, DetectionEvidenceClass::ResolvedModule);
    let first = input(vec![first_evaluation]);
    let second = input(vec![ModuleSelectorEvaluation::absent("Moo")]);
    let result = AdapterDetectionResult::for_input(
        &first,
        DetectionOutcome::Detected { confidence: Confidence::High, framework_version: None },
    )
    .with_contributing_modules(vec![first_activation]);
    assert_eq!(
        result.validate_authority_against(&second),
        Err(DetectionAuthorityError::InputIdentityMismatch)
    );
}

#[test]
fn version_string_without_observed_module_evidence_is_not_authority() {
    let (evaluation, activation) = matched_moo(None, DetectionEvidenceClass::ResolvedModule);
    let mut observed = input(vec![evaluation]);
    observed.descriptor = descriptor(Some(">=2"));
    let result = AdapterDetectionResult::for_input(
        &observed,
        DetectionOutcome::Detected {
            confidence: Confidence::High,
            framework_version: Some("2.1".into()),
        },
    )
    .with_contributing_modules(vec![activation])
    .with_version_evidence(ModuleVersionEvidence::new("2.1", SourceGeneration::known("project-1")));
    assert_eq!(
        result.validate_authority_against(&observed),
        Err(DetectionAuthorityError::InvalidVersionEvidence)
    );
}

#[test]
fn configuration_exclusion_requires_matching_typed_fact() {
    let observed = configuration_input(vec![ModuleSelectorEvaluation::absent("Moo")]);
    let result = AdapterDetectionResult::for_input(
        &observed,
        DetectionOutcome::Absent { reason: DetectionAbsenceReason::ExcludedByConfiguration },
    );
    assert_eq!(
        result.validate_authority_against(&observed),
        Err(DetectionAuthorityError::MissingConfigurationEvidence)
    );
}

#[test]
fn exact_current_detected_result_is_authoritative() {
    let (evaluation, activation) = matched_moo(Some("2.1"), DetectionEvidenceClass::ResolvedModule);
    let mut observed = input(vec![evaluation]);
    observed.descriptor = descriptor(Some(">=2"));
    let result = AdapterDetectionResult::for_input(
        &observed,
        DetectionOutcome::Detected {
            confidence: Confidence::High,
            framework_version: Some("2.1".into()),
        },
    )
    .with_contributing_modules(vec![activation])
    .with_version_evidence(ModuleVersionEvidence::new("2.1", SourceGeneration::known("project-1")));
    assert!(result.is_authoritative_against(&observed));
}

#[test]
fn exact_complete_missing_module_result_is_authoritative() {
    let observed = input(vec![ModuleSelectorEvaluation::absent("Moo")]);
    let result = AdapterDetectionResult::for_input(
        &observed,
        DetectionOutcome::Absent { reason: DetectionAbsenceReason::RequiredModulesMissing },
    );
    assert!(result.is_authoritative_against(&observed));
}

#[test]
fn exact_configuration_exclusion_is_authoritative() {
    let observation = configuration_observation(true);
    let observed = configuration_input(vec![ModuleSelectorEvaluation::absent("Moo")])
        .with_configuration_observations(vec![observation.clone()]);
    let evidence = DetectionConfigurationEvidence::new(
        observation,
        DetectionConfigurationValue::Boolean(true),
        "exclude-moo-when-disabled.v1",
    );
    let result = AdapterDetectionResult::for_input(
        &observed,
        DetectionOutcome::Absent { reason: DetectionAbsenceReason::ExcludedByConfiguration },
    )
    .with_configuration_evidence(evidence);
    assert!(result.is_authoritative_against(&observed));
}

#[test]
fn duplicate_or_cross_generation_selector_rows_fail_closed() {
    let (evaluation, _) = matched_moo(None, DetectionEvidenceClass::ResolvedModule);
    let observed = input(vec![evaluation.clone(), evaluation]);
    let result = AdapterDetectionResult::for_input(
        &observed,
        DetectionOutcome::Detected { confidence: Confidence::High, framework_version: None },
    );
    assert_eq!(
        result.validate_authority_against(&observed),
        Err(DetectionAuthorityError::InvalidSelectorEvidence)
    );

    let stale_activation = module("Moo", "project-0", None);
    let stale = input(vec![ModuleSelectorEvaluation::matched(
        "Moo",
        stale_activation,
        DetectionEvidenceClass::ResolvedModule,
    )]);
    let result = AdapterDetectionResult::for_input(
        &stale,
        DetectionOutcome::Detected { confidence: Confidence::High, framework_version: None },
    );
    assert_eq!(
        result.validate_authority_against(&stale),
        Err(DetectionAuthorityError::InvalidModuleEvidence)
    );
}

#[test]
fn empty_content_digest_has_its_own_failure_class() {
    let (evaluation, _) = matched_moo(None, DetectionEvidenceClass::ResolvedModule);
    let mut observed = input(vec![evaluation]);
    observed.module_observation.content_digest.clear();
    let result = AdapterDetectionResult::for_input(
        &observed,
        DetectionOutcome::Detected { confidence: Confidence::High, framework_version: None },
    );
    assert_eq!(
        result.validate_authority_against(&observed),
        Err(DetectionAuthorityError::InvalidContentDigest)
    );
}

#[test]
fn descriptor_owned_required_selectors_control_authority() {
    let foo = module("Foo", "project-1", None);
    let evaluation = ModuleSelectorEvaluation::matched(
        "Foo",
        foo.clone(),
        DetectionEvidenceClass::ResolvedModule,
    );

    let mut empty = input(vec![evaluation.clone()]);
    empty.descriptor.required_module_selectors.clear();
    let result = AdapterDetectionResult::for_input(
        &empty,
        DetectionOutcome::Detected { confidence: Confidence::High, framework_version: None },
    );
    assert_eq!(
        result.validate_authority_against(&empty),
        Err(DetectionAuthorityError::InvalidSelectorEvidence)
    );

    let mut observed = input(vec![evaluation]);
    observed.descriptor.required_module_selectors = vec!["Foo".into()];
    let result = AdapterDetectionResult::for_input(
        &observed,
        DetectionOutcome::Detected { confidence: Confidence::High, framework_version: None },
    )
    .with_contributing_modules(vec![foo]);
    assert!(result.is_authoritative_against(&observed));
}

#[test]
fn asserted_high_confidence_cannot_upgrade_probable_evidence() {
    let (evaluation, activation) = matched_moo(None, DetectionEvidenceClass::ProbableImport);
    let observed = input(vec![evaluation]);
    let result = AdapterDetectionResult::for_input(
        &observed,
        DetectionOutcome::Detected { confidence: Confidence::High, framework_version: None },
    )
    .with_contributing_modules(vec![activation]);
    assert_eq!(
        result.validate_authority_against(&observed),
        Err(DetectionAuthorityError::InsufficientConfidence)
    );
}

#[test]
fn matching_key_with_nonexcluding_value_cannot_prove_exclusion() {
    let observation = configuration_observation(false);
    let observed = configuration_input(vec![ModuleSelectorEvaluation::absent("Moo")])
        .with_configuration_observations(vec![observation.clone()]);
    let evidence = DetectionConfigurationEvidence::new(
        observation,
        DetectionConfigurationValue::Boolean(true),
        "exclude-moo-when-disabled.v1",
    );
    let result = AdapterDetectionResult::for_input(
        &observed,
        DetectionOutcome::Absent { reason: DetectionAbsenceReason::ExcludedByConfiguration },
    )
    .with_configuration_evidence(evidence);
    assert_eq!(
        result.validate_authority_against(&observed),
        Err(DetectionAuthorityError::ConfigurationRuleNotSatisfied)
    );
}

#[test]
fn descriptor_owned_excluding_value_rejects_result_override() {
    let observation = configuration_observation(false);
    let observed = configuration_input(vec![ModuleSelectorEvaluation::absent("Moo")])
        .with_configuration_observations(vec![observation.clone()]);
    let evidence = DetectionConfigurationEvidence::new(
        observation,
        DetectionConfigurationValue::Boolean(false),
        "exclude-moo-when-disabled.v1",
    );
    let result = AdapterDetectionResult::for_input(
        &observed,
        DetectionOutcome::Absent { reason: DetectionAbsenceReason::ExcludedByConfiguration },
    )
    .with_configuration_evidence(evidence);
    assert_eq!(
        result.validate_authority_against(&observed),
        Err(DetectionAuthorityError::InvalidConfigurationEvidence)
    );
}

#[test]
fn wrong_configuration_key_or_rule_cannot_prove_exclusion() {
    let expected_observation = configuration_observation(true);
    let expected_input = configuration_input(vec![ModuleSelectorEvaluation::absent("Moo")])
        .with_configuration_observations(vec![expected_observation.clone()]);

    let wrong_key_observation = DetectionConfigurationObservation::new(
        "workspace-config:perl-lsp.toml",
        "sha256:configuration",
        "frameworks.other.disabled",
        DetectionConfigurationValue::Boolean(true),
        "root:fixture",
        SourceGeneration::known("project-1"),
        "project-environment-config.v1",
        "framework-config.v1",
    );
    let wrong_key_input = configuration_input(vec![ModuleSelectorEvaluation::absent("Moo")])
        .with_configuration_observations(vec![wrong_key_observation.clone()]);
    let wrong_key = AdapterDetectionResult::for_input(
        &wrong_key_input,
        DetectionOutcome::Absent { reason: DetectionAbsenceReason::ExcludedByConfiguration },
    )
    .with_configuration_evidence(DetectionConfigurationEvidence::new(
        wrong_key_observation,
        DetectionConfigurationValue::Boolean(true),
        "exclude-moo-when-disabled.v1",
    ));
    assert_eq!(
        wrong_key.validate_authority_against(&wrong_key_input),
        Err(DetectionAuthorityError::InvalidConfigurationEvidence)
    );

    let wrong_rule = AdapterDetectionResult::for_input(
        &expected_input,
        DetectionOutcome::Absent { reason: DetectionAbsenceReason::ExcludedByConfiguration },
    )
    .with_configuration_evidence(DetectionConfigurationEvidence::new(
        expected_observation,
        DetectionConfigurationValue::Boolean(true),
        "exclude-other-framework.v1",
    ));
    assert_eq!(
        wrong_rule.validate_authority_against(&expected_input),
        Err(DetectionAuthorityError::InvalidConfigurationEvidence)
    );
}

#[test]
fn authority_receipt_parity_tracks_success_and_failure() {
    let (evaluation, activation) = matched_moo(Some("2.1"), DetectionEvidenceClass::ResolvedModule);
    let mut observed = input(vec![evaluation]);
    observed.descriptor = descriptor(Some(">=2"));
    let successful = AdapterDetectionResult::for_input(
        &observed,
        DetectionOutcome::Detected {
            confidence: Confidence::High,
            framework_version: Some("2.1".into()),
        },
    )
    .with_contributing_modules(vec![activation])
    .with_version_evidence(ModuleVersionEvidence::new("2.1", SourceGeneration::known("project-1")));

    let success_error = successful.validate_authority_against(&observed).err();
    let success_receipt = successful.authority_receipt_against(&observed);
    assert_eq!(success_error, None);
    assert!(success_receipt.authoritative);
    assert_eq!(success_receipt.error, success_error);
    assert_eq!(success_receipt.input_identity, observed.identity());
    assert_eq!(success_receipt.descriptor, successful.descriptor);
    assert_eq!(success_receipt.outcome, successful.outcome);

    // A result copied across inputs must fail closed, and the receipt must carry
    // the same reason rather than merely reporting a generic non-authority.
    let mut mismatched_input = input(vec![ModuleSelectorEvaluation::absent("Moo")]);
    mismatched_input.descriptor = observed.descriptor.clone();
    let failure_error = successful.validate_authority_against(&mismatched_input).err();
    let failure_receipt = successful.authority_receipt_against(&mismatched_input);
    assert_eq!(failure_error, Some(DetectionAuthorityError::InputIdentityMismatch));
    assert!(!failure_receipt.authoritative);
    assert_eq!(failure_receipt.error, failure_error);
    assert_eq!(failure_receipt.input_identity, mismatched_input.identity());
}
