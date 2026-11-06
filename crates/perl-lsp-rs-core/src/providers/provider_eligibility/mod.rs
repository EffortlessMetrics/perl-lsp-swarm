//! Pure transport-neutral provider currentness and completeness eligibility (#12154).
//!
//! Evaluates immutable request-subject and result-evidence snapshots. It does not
//! read mutable runtime state, invoke providers, or change live answers.

mod evaluate;
mod model;

#[cfg(test)]
mod tests;

pub use evaluate::evaluate_provider_eligibility;
pub use model::{
    ProviderCompletenessRequirementV1, ProviderEligibilityBlockerClass,
    ProviderEligibilityBlockerEvidence, ProviderEligibilityDecisionV1,
    ProviderEligibilityInstrumentState, ProviderEligibilityOutcome,
    ProviderEligibilityOwnerDimension, ProviderEligibilityRequestProfile, ProviderRequestSubjectV1,
    ProviderResultEvidenceV1, ProviderSubjectLifecycleState,
};
