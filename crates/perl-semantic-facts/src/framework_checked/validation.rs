use super::model::{
    AdapterDetectionInput, AdapterDetectionResult, DetectionAuthorityError,
    DetectionAuthorityReceipt,
};
use super::validation_input::{validate_contributing_modules, validate_input};
use super::validation_outcome::{
    validate_configuration_absence, validate_detected, validate_required_modules_missing,
    validate_version_absence,
};
use super::{DetectionAbsenceReason, DetectionOutcome};

impl AdapterDetectionResult {
    /// Validate this result against the exact observed input.
    pub fn validate_authority_against(
        &self,
        input: &AdapterDetectionInput,
    ) -> Result<(), DetectionAuthorityError> {
        validate_input(input)?;
        if self.descriptor != input.descriptor {
            return Err(DetectionAuthorityError::DescriptorMismatch);
        }
        if self.project_generation != input.project_generation {
            return Err(DetectionAuthorityError::GenerationMismatch);
        }
        let Some(identity) = &self.input_identity else {
            return Err(DetectionAuthorityError::MissingInputIdentity);
        };
        if identity != &input.identity() {
            return Err(DetectionAuthorityError::InputIdentityMismatch);
        }
        validate_contributing_modules(self, input)?;

        match &self.outcome {
            DetectionOutcome::Detected {
                confidence,
                framework_version,
            } => validate_detected(self, input, *confidence, framework_version.as_deref()),
            DetectionOutcome::Absent {
                reason: DetectionAbsenceReason::RequiredModulesMissing,
            } => validate_required_modules_missing(self, input),
            DetectionOutcome::Absent {
                reason: DetectionAbsenceReason::VersionConstraintNotSatisfied,
            } => validate_version_absence(self, input),
            DetectionOutcome::Absent {
                reason: DetectionAbsenceReason::ExcludedByConfiguration,
            } => validate_configuration_absence(self, input),
            _ => Err(DetectionAuthorityError::NonAuthoritativeOutcome),
        }
    }

    /// Whether this exact result/input pair passes the checked contract.
    #[must_use]
    pub fn is_authoritative_against(&self, input: &AdapterDetectionInput) -> bool {
        self.validate_authority_against(input).is_ok()
    }

    /// Build a bounded serializable validation receipt.
    #[must_use]
    pub fn authority_receipt_against(
        &self,
        input: &AdapterDetectionInput,
    ) -> DetectionAuthorityReceipt {
        let error = self.validate_authority_against(input).err();
        DetectionAuthorityReceipt {
            input_identity: input.identity(),
            descriptor: self.descriptor.clone(),
            outcome: self.outcome.clone(),
            authoritative: error.is_none(),
            error,
        }
    }
}
