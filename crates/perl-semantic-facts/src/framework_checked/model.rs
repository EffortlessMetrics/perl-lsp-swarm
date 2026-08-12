use super::{
    AdapterBudget, AdapterCancellation, AdapterDescriptor, DetectionOutcome,
    ModuleActivationIdentity, ModuleVersionEvidence, FRAMEWORK_ADAPTER_SDK_VERSION,
};
use crate::SourceGeneration;
use serde::{Deserialize, Serialize};

/// Current schema for checked detection input identities and receipts.
pub const DETECTION_AUTHORITY_SCHEMA_VERSION: u32 = 1;

/// Completeness of the module population observed for one project/root scope.
#[non_exhaustive]
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
pub enum ModuleObservationCompleteness {
    /// Every relevant module selector was evaluated against a complete universe.
    Complete,
    /// Positive rows may be usable, but missing rows cannot prove absence.
    #[default]
    Partial,
    /// Module observation was unavailable.
    Unavailable,
}

/// Typed configuration fact capable of proving an explicit framework exclusion.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct DetectionConfigurationEvidence {
    /// Stable exclusion key or rule identity.
    pub exclusion_key: String,
    /// Root, package, or source scope to which the exclusion applies.
    pub scope_identity: String,
    /// Generation that produced the configuration fact.
    pub generation: SourceGeneration,
    /// Versioned configuration-policy identity.
    pub policy_identity: String,
}

impl DetectionConfigurationEvidence {
    /// Construct current typed exclusion evidence.
    #[must_use]
    pub fn new(
        exclusion_key: impl Into<String>,
        scope_identity: impl Into<String>,
        generation: SourceGeneration,
        policy_identity: impl Into<String>,
    ) -> Self {
        Self {
            exclusion_key: exclusion_key.into(),
            scope_identity: scope_identity.into(),
            generation,
            policy_identity: policy_identity.into(),
        }
    }
}

/// Deterministic versioned identity of all load-bearing detection input.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectionInputIdentity {
    /// Detection identity schema.
    pub schema_version: u32,
    /// Exact descriptor identity.
    pub descriptor: AdapterDescriptor,
    /// Exact module selectors whose presence or absence was evaluated.
    pub required_modules: Vec<String>,
    /// Sorted module observations, including duplicate/conflicting rows.
    pub available_modules: Vec<ModuleActivationIdentity>,
    /// Project generation represented by the observation.
    pub project_generation: SourceGeneration,
    /// Optional digest over source/module inputs.
    pub content_digest: Option<String>,
    /// Completeness of the observed module universe.
    pub module_observation: ModuleObservationCompleteness,
    /// Sorted typed configuration evidence.
    pub configuration_evidence: Vec<DetectionConfigurationEvidence>,
    /// Detector/registry/policy implementation identity.
    pub detector_policy_identity: String,
}

/// Input to a checked framework-detection pass.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterDetectionInput {
    /// Adapter descriptor.
    pub descriptor: AdapterDescriptor,
    /// Exact required module names/selectors for this bounded SDK version.
    pub required_modules: Vec<String>,
    /// Modules visible in the current project model.
    pub available_modules: Vec<ModuleActivationIdentity>,
    /// Project-model generation.
    pub project_generation: SourceGeneration,
    /// Optional digest over the activation/input population.
    pub content_digest: Option<String>,
    /// Completeness of the module observation.
    #[serde(default)]
    pub module_observation: ModuleObservationCompleteness,
    /// Typed configuration facts relevant to exclusion.
    #[serde(default)]
    pub configuration_evidence: Vec<DetectionConfigurationEvidence>,
    /// Versioned detector/registry/policy identity.
    pub detector_policy_identity: String,
    /// Optional resource budget.
    pub budget: Option<AdapterBudget>,
    /// Admission-time cancellation snapshot.
    pub cancellation: AdapterCancellation,
}

impl AdapterDetectionInput {
    /// Construct a fail-closed partial input using the framework name as the
    /// first exact required-module selector.
    #[must_use]
    pub fn new(
        descriptor: AdapterDescriptor,
        available_modules: Vec<ModuleActivationIdentity>,
        project_generation: SourceGeneration,
        content_digest: Option<String>,
        budget: Option<AdapterBudget>,
        cancellation: AdapterCancellation,
    ) -> Self {
        let required_modules = vec![descriptor.framework_name.clone()];
        Self {
            descriptor,
            required_modules,
            available_modules,
            project_generation,
            content_digest,
            module_observation: ModuleObservationCompleteness::Partial,
            configuration_evidence: Vec::new(),
            detector_policy_identity: FRAMEWORK_ADAPTER_SDK_VERSION.to_string(),
            budget,
            cancellation,
        }
    }

    /// Replace the exact required-module selectors.
    #[must_use]
    pub fn with_required_modules(mut self, required_modules: Vec<String>) -> Self {
        self.required_modules = required_modules;
        self
    }

    /// Mark the module population complete enough to prove absence.
    #[must_use]
    pub fn with_complete_module_observation(mut self) -> Self {
        self.module_observation = ModuleObservationCompleteness::Complete;
        self
    }

    /// Attach typed exclusion evidence.
    #[must_use]
    pub fn with_configuration_evidence(
        mut self,
        evidence: Vec<DetectionConfigurationEvidence>,
    ) -> Self {
        self.configuration_evidence = evidence;
        self
    }

    /// Set the detector/registry/policy implementation identity.
    #[must_use]
    pub fn with_detector_policy_identity(mut self, identity: impl Into<String>) -> Self {
        self.detector_policy_identity = identity.into();
        self
    }

    /// Build the deterministic identity recorded by a result.
    #[must_use]
    pub fn identity(&self) -> DetectionInputIdentity {
        let mut required_modules = self.required_modules.clone();
        required_modules.sort();
        let mut available_modules = self.available_modules.clone();
        available_modules.sort();
        let mut configuration_evidence = self.configuration_evidence.clone();
        configuration_evidence.sort();
        DetectionInputIdentity {
            schema_version: DETECTION_AUTHORITY_SCHEMA_VERSION,
            descriptor: self.descriptor.clone(),
            required_modules,
            available_modules,
            project_generation: self.project_generation.clone(),
            content_digest: self.content_digest.clone(),
            module_observation: self.module_observation,
            configuration_evidence,
            detector_policy_identity: self.detector_policy_identity.clone(),
        }
    }

    /// Whether coherent version evidence exists for `module_name`.
    #[must_use]
    pub fn has_version_evidence(&self, module_name: &str) -> bool {
        self.available_modules.iter().any(|module| {
            module.module_name == module_name
                && module.generation == self.project_generation
                && module.observed_version.as_ref().is_some_and(|version| {
                    !version.version.trim().is_empty()
                        && version.generation == module.generation
                })
        })
    }
}

/// Checked detection-authority failure.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DetectionAuthorityError {
    /// Descriptor or identity schema is unsupported.
    UnsupportedSchema,
    /// Descriptor disposition is not production.
    NonProduction,
    /// Descriptor/result/input identities disagree.
    DescriptorMismatch,
    /// Project or evidence generations are missing or inconsistent.
    GenerationMismatch,
    /// Admission was already cancelled.
    CancelledInput,
    /// Detector policy identity is missing.
    MissingPolicyIdentity,
    /// Required module selector is missing or duplicated.
    InvalidRequiredModules,
    /// Module evidence is malformed, stale, duplicated, or contradictory.
    InvalidModuleEvidence,
    /// Configuration evidence is malformed, stale, duplicated, or unrelated.
    InvalidConfigurationEvidence,
    /// Result did not record the exact input identity.
    MissingInputIdentity,
    /// Result input identity differs from the supplied observation.
    InputIdentityMismatch,
    /// A positive result lacks exact contributing module evidence.
    MissingContributingEvidence,
    /// Contributing evidence is not present in the exact input population.
    UnrelatedContributingEvidence,
    /// Detection confidence is not high.
    InsufficientConfidence,
    /// Absence was claimed from an incomplete module universe.
    IncompleteModuleUniverse,
    /// A required module is present despite a missing-module verdict.
    RequiredModulePresent,
    /// Version evidence is absent or disagrees with the observed module.
    InvalidVersionEvidence,
    /// Version-constraint syntax is unsupported by this SDK version.
    UnsupportedVersionConstraint,
    /// Observed version does not satisfy a positive detection constraint.
    VersionConstraintNotSatisfied,
    /// Observed version satisfies a constraint claimed as unsatisfied.
    VersionConstraintSatisfied,
    /// Exclusion was asserted without matching typed configuration evidence.
    MissingConfigurationEvidence,
    /// Outcome is inspectable but cannot be positive or negative authority.
    NonAuthoritativeOutcome,
}

/// Serializable authority receipt for one result/input comparison.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectionAuthorityReceipt {
    /// Exact input identity used for validation.
    pub input_identity: DetectionInputIdentity,
    /// Adapter descriptor represented by the result.
    pub descriptor: AdapterDescriptor,
    /// Detection outcome.
    pub outcome: DetectionOutcome,
    /// Whether the result passed the checked contract.
    pub authoritative: bool,
    /// Stable non-authority reason.
    pub error: Option<DetectionAuthorityError>,
}

/// Public detection result. Raw constructors remain non-authoritative until the
/// exact input identity and reason-specific evidence are attached and checked.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterDetectionResult {
    /// Adapter descriptor.
    pub descriptor: AdapterDescriptor,
    /// Project generation represented by the result.
    pub project_generation: SourceGeneration,
    /// Detection outcome.
    pub outcome: DetectionOutcome,
    /// Version evidence used for a version-qualified verdict.
    #[serde(default)]
    pub version_evidence: Option<ModuleVersionEvidence>,
    /// Exact observed-input identity.
    #[serde(default)]
    pub input_identity: Option<DetectionInputIdentity>,
    /// Exact activation rows that contributed to the verdict.
    #[serde(default)]
    pub contributing_modules: Vec<ModuleActivationIdentity>,
    /// Typed configuration evidence that caused an exclusion verdict.
    #[serde(default)]
    pub configuration_evidence: Option<DetectionConfigurationEvidence>,
}

impl AdapterDetectionResult {
    /// Construct an intentionally unbound result. It is inspectable but cannot
    /// become authority until validated evidence is attached.
    #[must_use]
    pub fn new(
        descriptor: AdapterDescriptor,
        project_generation: SourceGeneration,
        outcome: DetectionOutcome,
    ) -> Self {
        Self {
            descriptor,
            project_generation,
            outcome,
            version_evidence: None,
            input_identity: None,
            contributing_modules: Vec::new(),
            configuration_evidence: None,
        }
    }

    /// Construct a result bound to one exact observed input.
    #[must_use]
    pub fn for_input(input: &AdapterDetectionInput, outcome: DetectionOutcome) -> Self {
        Self {
            descriptor: input.descriptor.clone(),
            project_generation: input.project_generation.clone(),
            outcome,
            version_evidence: None,
            input_identity: Some(input.identity()),
            contributing_modules: Vec::new(),
            configuration_evidence: None,
        }
    }

    /// Attach contributing activation rows.
    #[must_use]
    pub fn with_contributing_modules(
        mut self,
        contributing_modules: Vec<ModuleActivationIdentity>,
    ) -> Self {
        self.contributing_modules = contributing_modules;
        self
    }

    /// Attach observed version evidence.
    #[must_use]
    pub fn with_version_evidence(mut self, evidence: ModuleVersionEvidence) -> Self {
        self.version_evidence = Some(evidence);
        self
    }

    /// Attach typed configuration evidence for an exclusion verdict.
    #[must_use]
    pub fn with_configuration_evidence(
        mut self,
        evidence: DetectionConfigurationEvidence,
    ) -> Self {
        self.configuration_evidence = Some(evidence);
        self
    }

    /// Whether this result reports framework presence.
    #[must_use]
    pub fn is_detected(&self) -> bool {
        matches!(self.outcome, DetectionOutcome::Detected { .. })
    }
}
