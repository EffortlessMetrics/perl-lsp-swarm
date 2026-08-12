use super::{
    AdapterBudget, AdapterCancellation, AdapterDescriptor, DetectionOutcome,
    ModuleActivationIdentity, ModuleVersionEvidence, FRAMEWORK_ADAPTER_SDK_VERSION,
};
use crate::{Confidence, SourceGeneration};
use serde::{Deserialize, Serialize};

/// Current schema for checked detection input identities and receipts.
pub const DETECTION_AUTHORITY_SCHEMA_VERSION: u32 = 1;

/// Evidence class emitted by the canonical module/import observation producer.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DetectionEvidenceClass {
    /// Exact resolved module identity in the current project/root.
    ResolvedModule,
    /// Exact resolved import/activation fact in the current scope.
    ResolvedImport,
    /// Probable import inferred from incomplete evidence.
    ProbableImport,
    /// Name-only heuristic without resolved module identity.
    NameOnly,
}

impl DetectionEvidenceClass {
    /// Maximum confidence this evidence class can support.
    #[must_use]
    pub const fn confidence_ceiling(self) -> Confidence {
        match self {
            Self::ResolvedModule | Self::ResolvedImport => Confidence::High,
            Self::ProbableImport => Confidence::Medium,
            Self::NameOnly => Confidence::Low,
        }
    }
}

/// One descriptor-owned module selector evaluation.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ModuleSelectorOutcome {
    /// Selector matched one exact activation row.
    Matched {
        /// Current activation identity.
        activation: ModuleActivationIdentity,
        /// Evidence class that established the match.
        evidence_class: DetectionEvidenceClass,
    },
    /// Complete resolver evidence established absence.
    Absent,
    /// Resolver could not resolve the selector.
    Unresolved { reason: String },
    /// More than one candidate prevented a unique verdict.
    Ambiguous { reason: String },
    /// Resolver or environment evidence was unavailable.
    Unavailable { reason: String },
}

/// One exact selector and its observed terminal or incomplete outcome.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ModuleSelectorEvaluation {
    /// Exact module selector evaluated by the resolver.
    pub selector: String,
    /// Resolver result for that selector.
    pub outcome: ModuleSelectorOutcome,
}

impl ModuleSelectorEvaluation {
    /// Construct an exact matched selector row.
    #[must_use]
    pub fn matched(
        selector: impl Into<String>,
        activation: ModuleActivationIdentity,
        evidence_class: DetectionEvidenceClass,
    ) -> Self {
        Self {
            selector: selector.into(),
            outcome: ModuleSelectorOutcome::Matched {
                activation,
                evidence_class,
            },
        }
    }

    /// Construct an exact absent selector row.
    #[must_use]
    pub fn absent(selector: impl Into<String>) -> Self {
        Self {
            selector: selector.into(),
            outcome: ModuleSelectorOutcome::Absent,
        }
    }

    /// Construct an unresolved selector row.
    #[must_use]
    pub fn unresolved(selector: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            selector: selector.into(),
            outcome: ModuleSelectorOutcome::Unresolved {
                reason: reason.into(),
            },
        }
    }
}

/// Checked module/import observation packet consumed by detection validation.
///
/// Completeness is derived from one terminal evaluation per descriptor-owned
/// selector. There is no public `complete = true` switch.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleObservationReceipt {
    /// Observation schema.
    pub schema_version: u32,
    /// Versioned canonical resolver/discovery implementation identity.
    pub resolver_identity: String,
    /// Exact project/root/package scope observed.
    pub scope_identity: String,
    /// Exact project environment identity.
    pub environment_identity: String,
    /// Current project generation.
    pub generation: SourceGeneration,
    /// Digest over the resolver's source/module population.
    pub content_digest: String,
    /// Selector evaluations retained by the resolver.
    pub evaluations: Vec<ModuleSelectorEvaluation>,
}

impl ModuleObservationReceipt {
    /// Construct a module observation and canonicalize selector order.
    #[must_use]
    pub fn new(
        resolver_identity: impl Into<String>,
        scope_identity: impl Into<String>,
        environment_identity: impl Into<String>,
        generation: SourceGeneration,
        content_digest: impl Into<String>,
        mut evaluations: Vec<ModuleSelectorEvaluation>,
    ) -> Self {
        evaluations.sort();
        Self {
            schema_version: DETECTION_AUTHORITY_SCHEMA_VERSION,
            resolver_identity: resolver_identity.into(),
            scope_identity: scope_identity.into(),
            environment_identity: environment_identity.into(),
            generation,
            content_digest: content_digest.into(),
            evaluations,
        }
    }
}

/// Typed configuration value used for exclusion-rule evaluation.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DetectionConfigurationValue {
    Boolean(bool),
    Integer(i64),
    String(String),
}

/// Exact configuration observation from one source and generation.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct DetectionConfigurationObservation {
    /// Stable configuration source identity.
    pub source_identity: String,
    /// Digest of the exact configuration source/input.
    pub source_digest: String,
    /// Exact configuration key.
    pub key: String,
    /// Observed typed value.
    pub value: DetectionConfigurationValue,
    /// Root/package/source scope to which the value applies.
    pub scope_identity: String,
    /// Generation that produced the observation.
    pub generation: SourceGeneration,
    /// Provenance class of the observation.
    pub provenance: String,
    /// Versioned configuration parser/policy identity.
    pub policy_identity: String,
}

impl DetectionConfigurationObservation {
    /// Construct one exact configuration observation.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        source_identity: impl Into<String>,
        source_digest: impl Into<String>,
        key: impl Into<String>,
        value: DetectionConfigurationValue,
        scope_identity: impl Into<String>,
        generation: SourceGeneration,
        provenance: impl Into<String>,
        policy_identity: impl Into<String>,
    ) -> Self {
        Self {
            source_identity: source_identity.into(),
            source_digest: source_digest.into(),
            key: key.into(),
            value,
            scope_identity: scope_identity.into(),
            generation,
            provenance: provenance.into(),
            policy_identity: policy_identity.into(),
        }
    }
}

/// Reason-specific evidence that a configuration observation actually excludes
/// this adapter under one reviewed rule.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectionConfigurationEvidence {
    /// Exact observed configuration fact.
    pub observation: DetectionConfigurationObservation,
    /// Value that causes exclusion under this rule.
    pub excluding_value: DetectionConfigurationValue,
    /// Stable rule identity.
    pub rule_identity: String,
}

impl DetectionConfigurationEvidence {
    /// Construct exclusion evidence from an observed value and rule.
    #[must_use]
    pub fn new(
        observation: DetectionConfigurationObservation,
        excluding_value: DetectionConfigurationValue,
        rule_identity: impl Into<String>,
    ) -> Self {
        Self {
            observation,
            excluding_value,
            rule_identity: rule_identity.into(),
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
    /// Canonical resolver/discovery packet.
    pub module_observation: ModuleObservationReceipt,
    /// Sorted typed configuration observations.
    pub configuration_observations: Vec<DetectionConfigurationObservation>,
    /// Detector/registry/policy implementation identity.
    pub detector_policy_identity: String,
}

/// Input to a checked framework-detection pass.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterDetectionInput {
    /// Adapter descriptor.
    pub descriptor: AdapterDescriptor,
    /// Canonical resolver/discovery packet.
    pub module_observation: ModuleObservationReceipt,
    /// Typed configuration observations relevant to exclusion.
    #[serde(default)]
    pub configuration_observations: Vec<DetectionConfigurationObservation>,
    /// Versioned detector/registry/policy identity.
    pub detector_policy_identity: String,
    /// Optional resource budget.
    pub budget: Option<AdapterBudget>,
    /// Admission-time cancellation snapshot.
    pub cancellation: AdapterCancellation,
}

impl AdapterDetectionInput {
    /// Construct a fail-closed input from one resolver observation packet.
    #[must_use]
    pub fn new(
        descriptor: AdapterDescriptor,
        module_observation: ModuleObservationReceipt,
        budget: Option<AdapterBudget>,
        cancellation: AdapterCancellation,
    ) -> Self {
        Self {
            descriptor,
            module_observation,
            configuration_observations: Vec::new(),
            detector_policy_identity: FRAMEWORK_ADAPTER_SDK_VERSION.to_string(),
            budget,
            cancellation,
        }
    }

    /// Attach typed configuration observations.
    #[must_use]
    pub fn with_configuration_observations(
        mut self,
        mut observations: Vec<DetectionConfigurationObservation>,
    ) -> Self {
        observations.sort();
        self.configuration_observations = observations;
        self
    }

    /// Set the detector/registry/policy implementation identity.
    #[must_use]
    pub fn with_detector_policy_identity(mut self, identity: impl Into<String>) -> Self {
        self.detector_policy_identity = identity.into();
        self
    }

    /// Project generation represented by this input.
    #[must_use]
    pub fn project_generation(&self) -> &SourceGeneration {
        &self.module_observation.generation
    }

    /// Build the deterministic identity recorded by a result.
    #[must_use]
    pub fn identity(&self) -> DetectionInputIdentity {
        let mut module_observation = self.module_observation.clone();
        module_observation.evaluations.sort();
        let mut configuration_observations = self.configuration_observations.clone();
        configuration_observations.sort();
        DetectionInputIdentity {
            schema_version: DETECTION_AUTHORITY_SCHEMA_VERSION,
            descriptor: self.descriptor.clone(),
            module_observation,
            configuration_observations,
            detector_policy_identity: self.detector_policy_identity.clone(),
        }
    }
}

/// Checked detection-authority failure.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DetectionAuthorityError {
    UnsupportedSchema,
    NonProduction,
    DescriptorMismatch,
    GenerationMismatch,
    CancelledInput,
    MissingPolicyIdentity,
    InvalidContentDigest,
    InvalidSelectorEvidence,
    InvalidModuleEvidence,
    InvalidConfigurationEvidence,
    MissingInputIdentity,
    InputIdentityMismatch,
    MissingContributingEvidence,
    UnrelatedContributingEvidence,
    InsufficientConfidence,
    IncompleteModuleUniverse,
    RequiredModulePresent,
    InvalidVersionEvidence,
    UnsupportedVersionConstraint,
    VersionConstraintNotSatisfied,
    VersionConstraintSatisfied,
    MissingConfigurationEvidence,
    ConfigurationRuleNotSatisfied,
    NonAuthoritativeOutcome,
}

/// Serializable authority receipt for one result/input comparison.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectionAuthorityReceipt {
    pub input_identity: DetectionInputIdentity,
    pub descriptor: AdapterDescriptor,
    pub outcome: DetectionOutcome,
    pub authoritative: bool,
    pub error: Option<DetectionAuthorityError>,
}

/// Public detection result. Raw constructors remain non-authoritative until the
/// exact input identity and reason-specific evidence are attached and checked.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterDetectionResult {
    pub descriptor: AdapterDescriptor,
    pub project_generation: SourceGeneration,
    pub outcome: DetectionOutcome,
    #[serde(default)]
    pub version_evidence: Option<ModuleVersionEvidence>,
    #[serde(default)]
    pub input_identity: Option<DetectionInputIdentity>,
    #[serde(default)]
    pub contributing_modules: Vec<ModuleActivationIdentity>,
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
            project_generation: input.module_observation.generation.clone(),
            outcome,
            version_evidence: None,
            input_identity: Some(input.identity()),
            contributing_modules: Vec::new(),
            configuration_evidence: None,
        }
    }

    #[must_use]
    pub fn with_contributing_modules(
        mut self,
        contributing_modules: Vec<ModuleActivationIdentity>,
    ) -> Self {
        self.contributing_modules = contributing_modules;
        self
    }

    #[must_use]
    pub fn with_version_evidence(mut self, evidence: ModuleVersionEvidence) -> Self {
        self.version_evidence = Some(evidence);
        self
    }

    #[must_use]
    pub fn with_configuration_evidence(
        mut self,
        evidence: DetectionConfigurationEvidence,
    ) -> Self {
        self.configuration_evidence = Some(evidence);
        self
    }

    #[must_use]
    pub fn is_detected(&self) -> bool {
        matches!(self.outcome, DetectionOutcome::Detected { .. })
    }
}
