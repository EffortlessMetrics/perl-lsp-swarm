use super::model::{
    AdapterDetectionInput, AdapterDetectionResult, DetectionAuthorityError,
    ModuleObservationCompleteness,
};
use super::version::constraint_matches;
use crate::Confidence;

pub(super) fn validate_detected(
    result: &AdapterDetectionResult,
    input: &AdapterDetectionInput,
    confidence: Confidence,
    framework_version: Option<&str>,
) -> Result<(), DetectionAuthorityError> {
    if confidence != Confidence::High {
        return Err(DetectionAuthorityError::InsufficientConfidence);
    }
    if result.contributing_modules.is_empty() {
        return Err(DetectionAuthorityError::MissingContributingEvidence);
    }
    if input.required_modules.iter().any(|required| {
        !result
            .contributing_modules
            .iter()
            .any(|module| module.module_name == *required)
    }) {
        return Err(DetectionAuthorityError::MissingContributingEvidence);
    }

    let Some(constraint) = input.descriptor.framework_version_constraint.as_deref() else {
        if let Some(version) = framework_version {
            validate_result_version(result, input, version)?;
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
    if input.module_observation != ModuleObservationCompleteness::Complete {
        return Err(DetectionAuthorityError::IncompleteModuleUniverse);
    }
    if input.required_modules.iter().any(|required| {
        input
            .available_modules
            .iter()
            .any(|module| module.module_name == *required)
    }) {
        return Err(DetectionAuthorityError::RequiredModulePresent);
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
    let evidence = result
        .version_evidence
        .as_ref()
        .ok_or(DetectionAuthorityError::InvalidVersionEvidence)?;
    if evidence.generation != input.project_generation
        || !result.contributing_modules.iter().any(|module| {
            module.generation == input.project_generation
                && module.observed_version.as_ref() == Some(evidence)
                && input.required_modules.contains(&module.module_name)
        })
    {
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
    if !input.configuration_evidence.contains(evidence)
        || evidence.generation != input.project_generation
    {
        return Err(DetectionAuthorityError::InvalidConfigurationEvidence);
    }
    Ok(())
}

fn validate_result_version(
    result: &AdapterDetectionResult,
    input: &AdapterDetectionInput,
    framework_version: &str,
) -> Result<(), DetectionAuthorityError> {
    let evidence = result
        .version_evidence
        .as_ref()
        .ok_or(DetectionAuthorityError::InvalidVersionEvidence)?;
    if evidence.version != framework_version
        || evidence.generation != input.project_generation
        || !result.contributing_modules.iter().any(|module| {
            module.generation == input.project_generation
                && module.observed_version.as_ref() == Some(evidence)
        })
    {
        return Err(DetectionAuthorityError::InvalidVersionEvidence);
    }
    Ok(())
}

