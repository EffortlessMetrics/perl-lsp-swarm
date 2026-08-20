use super::model::{
    AdapterDetectionInput, AdapterDetectionResult, DETECTION_AUTHORITY_SCHEMA_VERSION,
    DetectionAuthorityError, DetectionConfigurationObservation, DetectionEvidenceClass,
    ModuleSelectorEvaluation, ModuleSelectorOutcome,
};
use super::{AdapterDisposition, FRAMEWORK_ADAPTER_SCHEMA_VERSION, ModuleActivationIdentity};
use crate::Confidence;
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn descriptor_selectors(input: &AdapterDetectionInput) -> Vec<&str> {
    input.descriptor.required_module_selectors.iter().map(String::as_str).collect()
}

pub(super) fn selector_evaluation<'a>(
    input: &'a AdapterDetectionInput,
    selector: &str,
) -> Option<&'a ModuleSelectorEvaluation> {
    input.module_observation.evaluations.iter().find(|evaluation| evaluation.selector == selector)
}

pub(super) fn matched_evidence(
    evaluation: &ModuleSelectorEvaluation,
) -> Option<(&ModuleActivationIdentity, DetectionEvidenceClass)> {
    match &evaluation.outcome {
        ModuleSelectorOutcome::Matched { activation, evidence_class } => {
            Some((activation, *evidence_class))
        }
        _ => None,
    }
}

pub(super) fn derived_confidence(input: &AdapterDetectionInput) -> Option<Confidence> {
    let mut confidence = Confidence::High;
    for selector in descriptor_selectors(input) {
        let evaluation = selector_evaluation(input, selector)?;
        let (_, evidence_class) = matched_evidence(evaluation)?;
        confidence = minimum_confidence(confidence, evidence_class.confidence_ceiling());
    }
    Some(confidence)
}

pub(super) fn expected_contributing_modules(
    input: &AdapterDetectionInput,
) -> Option<Vec<ModuleActivationIdentity>> {
    let mut modules = Vec::new();
    for selector in descriptor_selectors(input) {
        let evaluation = selector_evaluation(input, selector)?;
        let (activation, _) = matched_evidence(evaluation)?;
        modules.push(activation.clone());
    }
    modules.sort();
    Some(modules)
}

pub(super) fn validate_input(input: &AdapterDetectionInput) -> Result<(), DetectionAuthorityError> {
    if input.descriptor.schema_version != FRAMEWORK_ADAPTER_SCHEMA_VERSION
        || input.module_observation.schema_version != DETECTION_AUTHORITY_SCHEMA_VERSION
    {
        return Err(DetectionAuthorityError::UnsupportedSchema);
    }
    if input.descriptor.disposition != AdapterDisposition::Production {
        return Err(DetectionAuthorityError::NonProduction);
    }
    if input.descriptor.framework_name.trim().is_empty()
        || input.descriptor.required_module_selectors.is_empty()
    {
        return Err(DetectionAuthorityError::InvalidSelectorEvidence);
    }
    if !input.module_observation.generation.is_known() {
        return Err(DetectionAuthorityError::GenerationMismatch);
    }
    if input.cancellation.is_cancelled {
        return Err(DetectionAuthorityError::CancelledInput);
    }
    if input.detector_policy_identity.trim().is_empty()
        || input.module_observation.resolver_identity.trim().is_empty()
        || input.module_observation.scope_identity.trim().is_empty()
        || input.module_observation.environment_identity.trim().is_empty()
    {
        return Err(DetectionAuthorityError::MissingPolicyIdentity);
    }
    if input.module_observation.content_digest.trim().is_empty() {
        return Err(DetectionAuthorityError::InvalidContentDigest);
    }

    let mut evaluations = BTreeMap::new();
    for evaluation in &input.module_observation.evaluations {
        if evaluation.selector.trim().is_empty()
            || evaluations.insert(evaluation.selector.as_str(), evaluation).is_some()
        {
            return Err(DetectionAuthorityError::InvalidSelectorEvidence);
        }
        validate_selector_evaluation(input, evaluation)?;
    }
    for selector in descriptor_selectors(input) {
        if !evaluations.contains_key(selector) {
            return Err(DetectionAuthorityError::InvalidSelectorEvidence);
        }
    }

    validate_configuration_observations(input)?;
    Ok(())
}

fn validate_selector_evaluation(
    input: &AdapterDetectionInput,
    evaluation: &ModuleSelectorEvaluation,
) -> Result<(), DetectionAuthorityError> {
    match &evaluation.outcome {
        ModuleSelectorOutcome::Matched { activation, .. } => {
            if activation.module_name != evaluation.selector
                || activation.generation != input.module_observation.generation
                || !activation.generation.is_known()
            {
                return Err(DetectionAuthorityError::InvalidModuleEvidence);
            }
            if let Some(version) = &activation.observed_version
                && (version.version.trim().is_empty()
                    || version.generation != activation.generation)
            {
                return Err(DetectionAuthorityError::InvalidModuleEvidence);
            }
        }
        ModuleSelectorOutcome::Absent => {}
        ModuleSelectorOutcome::Unresolved { reason }
        | ModuleSelectorOutcome::Ambiguous { reason }
        | ModuleSelectorOutcome::Unavailable { reason } => {
            if reason.trim().is_empty() {
                return Err(DetectionAuthorityError::InvalidSelectorEvidence);
            }
        }
    }
    Ok(())
}

fn validate_configuration_observations(
    input: &AdapterDetectionInput,
) -> Result<(), DetectionAuthorityError> {
    let mut observations = BTreeSet::new();
    for observation in &input.configuration_observations {
        validate_configuration_observation(input, observation)?;
        if !observations.insert(observation) {
            return Err(DetectionAuthorityError::InvalidConfigurationEvidence);
        }
    }
    Ok(())
}

fn validate_configuration_observation(
    input: &AdapterDetectionInput,
    observation: &DetectionConfigurationObservation,
) -> Result<(), DetectionAuthorityError> {
    if observation.source_identity.trim().is_empty()
        || observation.source_digest.trim().is_empty()
        || observation.key.trim().is_empty()
        || observation.scope_identity.trim().is_empty()
        || observation.provenance.trim().is_empty()
        || observation.policy_identity.trim().is_empty()
        || !observation.generation.is_known()
        || observation.generation != input.module_observation.generation
    {
        return Err(DetectionAuthorityError::InvalidConfigurationEvidence);
    }
    Ok(())
}

pub(super) fn validate_contributing_modules(
    result: &AdapterDetectionResult,
    input: &AdapterDetectionInput,
) -> Result<(), DetectionAuthorityError> {
    let mut seen = BTreeSet::new();
    for module in &result.contributing_modules {
        if !seen.insert(module) {
            return Err(DetectionAuthorityError::InvalidModuleEvidence);
        }
        let related = input.module_observation.evaluations.iter().any(|evaluation| {
            matched_evidence(evaluation).is_some_and(|(activation, _)| activation == module)
        });
        if !related {
            return Err(DetectionAuthorityError::UnrelatedContributingEvidence);
        }
    }
    Ok(())
}

fn minimum_confidence(left: Confidence, right: Confidence) -> Confidence {
    match (left, right) {
        (Confidence::Low, _) | (_, Confidence::Low) => Confidence::Low,
        (Confidence::Medium, _) | (_, Confidence::Medium) => Confidence::Medium,
        (Confidence::High, Confidence::High) => Confidence::High,
    }
}
