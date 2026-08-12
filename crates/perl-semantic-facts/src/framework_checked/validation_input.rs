use super::model::{
    AdapterDetectionInput, AdapterDetectionResult, DetectionAuthorityError,
};
use super::{AdapterDisposition, FRAMEWORK_ADAPTER_SCHEMA_VERSION};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn validate_input(input: &AdapterDetectionInput) -> Result<(), DetectionAuthorityError> {
    if input.descriptor.schema_version != FRAMEWORK_ADAPTER_SCHEMA_VERSION {
        return Err(DetectionAuthorityError::UnsupportedSchema);
    }
    if input.descriptor.disposition != AdapterDisposition::Production {
        return Err(DetectionAuthorityError::NonProduction);
    }
    if !input.project_generation.is_known() {
        return Err(DetectionAuthorityError::GenerationMismatch);
    }
    if input.cancellation.is_cancelled {
        return Err(DetectionAuthorityError::CancelledInput);
    }
    if input.detector_policy_identity.trim().is_empty() {
        return Err(DetectionAuthorityError::MissingPolicyIdentity);
    }
    if input
        .content_digest
        .as_ref()
        .is_some_and(|digest| digest.trim().is_empty())
    {
        return Err(DetectionAuthorityError::InputIdentityMismatch);
    }

    let mut required = BTreeSet::new();
    if input.required_modules.is_empty()
        || input.required_modules.iter().any(|module| {
            module.trim().is_empty() || !required.insert(module.as_str())
        })
    {
        return Err(DetectionAuthorityError::InvalidRequiredModules);
    }

    let mut modules = BTreeMap::new();
    for module in &input.available_modules {
        if module.module_name.trim().is_empty()
            || !module.generation.is_known()
            || module.generation != input.project_generation
        {
            return Err(DetectionAuthorityError::InvalidModuleEvidence);
        }
        if let Some(version) = &module.observed_version
            && (version.version.trim().is_empty() || version.generation != module.generation)
        {
            return Err(DetectionAuthorityError::InvalidModuleEvidence);
        }
        let key = (&module.module_name, module.file_id, &module.generation);
        if modules.insert(key, &module.observed_version).is_some() {
            return Err(DetectionAuthorityError::InvalidModuleEvidence);
        }
    }

    let mut configurations = BTreeSet::new();
    for evidence in &input.configuration_evidence {
        if evidence.exclusion_key.trim().is_empty()
            || evidence.scope_identity.trim().is_empty()
            || evidence.policy_identity.trim().is_empty()
            || !evidence.generation.is_known()
            || evidence.generation != input.project_generation
            || !configurations.insert(evidence)
        {
            return Err(DetectionAuthorityError::InvalidConfigurationEvidence);
        }
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
        if !input.available_modules.contains(module) {
            return Err(DetectionAuthorityError::UnrelatedContributingEvidence);
        }
    }
    Ok(())
}
