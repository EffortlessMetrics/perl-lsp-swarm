use super::model::{
    AdapterDetectionInput, AdapterDetectionResult, DetectionAuthorityError, ModuleSelectorOutcome,
};
use super::validation_input::{
    derived_confidence, descriptor_selectors, expected_contributing_modules, matched_evidence,
    selector_evaluation,
};
use super::version::constraint_matches;
use crate::Confidence;

pub(super) fn validate_detected(
    result: &AdapterDetectionResult,
    input: &AdapterDetectionInput,
    asserted_confidence: Confidence,
    framework_version: Option<&str>,
) -> Result<(), DetectionAuthorityError> {
    let derived =
        derived_confidence(input).ok_or(DetectionAuthorityError::MissingContributingEvidence)?;
    if asserted_confidence != derived || derived != Confidence::High {
        return Err(DetectionAuthorityError::InsufficientConfidence);
    }
    validate_exact_contributors(result, input)?;

    let Some(constraint) = input.descriptor.framework_version_constraint.as_deref() else {
        if let Some(version) = framework_version {
            validate_result_version(result, input, version)?;
        } else if result.version_evidence.is_some() {
            return Err(DetectionAuthorityError::InvalidVersionEvidence);
        }
        return Ok(());
    };
    let version = framework_version.ok_or(DetectionAuthorityError::InvalidVersionEvidence)?;
    validate_result_version(result, input, version)?;
    match constraint_matches(constraint, version) {
        Some(true) => Ok(()),
        Some(false) => Err(DetectionAuthorityError::VersionConstraintNotSatisfied),
        None => Err(DetectionAuthorityError::UnsupportedVersionConstraint),
    }
}

pub(super) fn validate_required_modules_missing(
    result: &AdapterDetectionResult,
    input: &AdapterDetectionInput,
) -> Result<(), DetectionAuthorityError> {
    for selector in descriptor_selectors(input) {
        let evaluation = selector_evaluation(input, selector)
            .ok_or(DetectionAuthorityError::InvalidSelectorEvidence)?;
        match &evaluation.outcome {
            ModuleSelectorOutcome::Absent => {}
            ModuleSelectorOutcome::Matched { .. } => {
                return Err(DetectionAuthorityError::RequiredModulePresent);
            }
            ModuleSelectorOutcome::Unresolved { .. }
            | ModuleSelectorOutcome::Ambiguous { .. }
            | ModuleSelectorOutcome::Unavailable { .. } => {
                return Err(DetectionAuthorityError::IncompleteModuleUniverse);
            }
        }
    }
    if !result.contributing_modules.is_empty() {
        return Err(DetectionAuthorityError::UnrelatedContributingEvidence);
    }
    Ok(())
}

pub(super) fn validate_version_absence(
    result: &AdapterDetectionResult,
    input: &AdapterDetectionInput,
) -> Result<(), DetectionAuthorityError> {
    let constraint = input
        .descriptor
        .framework_version_constraint
        .as_deref()
        .ok_or(DetectionAuthorityError::InvalidVersionEvidence)?;
    if derived_confidence(input) != Some(Confidence::High) {
        return Err(DetectionAuthorityError::InsufficientConfidence);
    }
    validate_exact_contributors(result, input)?;
    let evidence =
        result.version_evidence.as_ref().ok_or(DetectionAuthorityError::InvalidVersionEvidence)?;
    let activation_has_evidence = descriptor_selectors(input).into_iter().all(|selector| {
        selector_evaluation(input, selector).and_then(matched_evidence).is_some_and(
            |(activation, _)| {
                activation.observed_version.as_ref() == Some(evidence)
                    && evidence.generation == input.module_observation.generation
            },
        )
    });
    if !activation_has_evidence {
        return Err(DetectionAuthorityError::InvalidVersionEvidence);
    }
    match constraint_matches(constraint, &evidence.version) {
        Some(false) => Ok(()),
        Some(true) => Err(DetectionAuthorityError::VersionConstraintSatisfied),
        None => Err(DetectionAuthorityError::UnsupportedVersionConstraint),
    }
}

pub(super) fn validate_configuration_absence(
    result: &AdapterDetectionResult,
    input: &AdapterDetectionInput,
) -> Result<(), DetectionAuthorityError> {
    let evidence = result
        .configuration_evidence
        .as_ref()
        .ok_or(DetectionAuthorityError::MissingConfigurationEvidence)?;
    let expected_key = input
        .descriptor
        .configuration_exclusion_key
        .as_deref()
        .ok_or(DetectionAuthorityError::InvalidConfigurationEvidence)?;
    let expected_rule = input
        .descriptor
        .configuration_exclusion_rule
        .as_deref()
        .ok_or(DetectionAuthorityError::InvalidConfigurationEvidence)?;
    if evidence.rule_identity.trim().is_empty()
        || !input.configuration_observations.contains(&evidence.observation)
        || evidence.observation.generation != input.module_observation.generation
        || evidence.observation.key != expected_key
        || evidence.rule_identity != expected_rule
    {
        return Err(DetectionAuthorityError::InvalidConfigurationEvidence);
    }
    if evidence.observation.value != evidence.excluding_value {
        return Err(DetectionAuthorityError::ConfigurationRuleNotSatisfied);
    }
    Ok(())
}

fn validate_exact_contributors(
    result: &AdapterDetectionResult,
    input: &AdapterDetectionInput,
) -> Result<(), DetectionAuthorityError> {
    let expected = expected_contributing_modules(input)
        .ok_or(DetectionAuthorityError::MissingContributingEvidence)?;
    let mut actual = result.contributing_modules.clone();
    actual.sort();
    if actual != expected {
        return Err(DetectionAuthorityError::MissingContributingEvidence);
    }
    Ok(())
}

fn validate_result_version(
    result: &AdapterDetectionResult,
    input: &AdapterDetectionInput,
    framework_version: &str,
) -> Result<(), DetectionAuthorityError> {
    let evidence =
        result.version_evidence.as_ref().ok_or(DetectionAuthorityError::InvalidVersionEvidence)?;
    if evidence.version != framework_version
        || evidence.generation != input.module_observation.generation
    {
        return Err(DetectionAuthorityError::InvalidVersionEvidence);
    }
    let exact = descriptor_selectors(input).into_iter().all(|selector| {
        selector_evaluation(input, selector)
            .and_then(matched_evidence)
            .is_some_and(|(activation, _)| activation.observed_version.as_ref() == Some(evidence))
    });
    if !exact {
        return Err(DetectionAuthorityError::InvalidVersionEvidence);
    }
    Ok(())
}
