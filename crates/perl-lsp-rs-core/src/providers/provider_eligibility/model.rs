use super::super::semantic_port::{
    ProviderCancellationState, ProviderIdentity, ProviderQueryCapability, ProviderQueryDeadline,
    ProviderReadinessRequirement, ProviderReadinessState,
};
use super::super::semantic_port_adapters::ProviderSnapshotCompleteness;
use perl_semantic_facts::{SemanticProducer, SourceGeneration};
use serde::Serialize;

/// Request profile selecting completeness and readiness requirements for one evaluation.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderEligibilityRequestProfile {
    /// Stable request-class identifier such as `textDocument/references`.
    pub request_class: String,
    /// Fact family whose completeness law applies.
    pub capability: ProviderQueryCapability,
    /// Readiness scope required by the request family.
    pub readiness_requirement: ProviderReadinessRequirement,
    /// Whether the request family can produce edits when eligible.
    pub edit_producing: bool,
}

impl ProviderEligibilityRequestProfile {
    /// Construct a profile for one request family.
    #[must_use]
    pub fn new(
        request_class: impl Into<String>,
        capability: ProviderQueryCapability,
        readiness_requirement: ProviderReadinessRequirement,
        edit_producing: bool,
    ) -> Self {
        Self {
            request_class: request_class.into(),
            capability,
            readiness_requirement,
            edit_producing,
        }
    }
}

/// Lifecycle observations bound to the request subject.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum ProviderSubjectLifecycleState {
    /// Source and root bindings are current for this request.
    Current,
    /// The logical source was deleted after facts were produced.
    SourceDeleted,
    /// The workspace root binding was removed.
    RootRemoved,
    /// The document instance changed while URI or bytes may look unchanged.
    InstanceReplaced {
        /// Prior document-instance identity that produced the evidence.
        prior_instance_identity: ProviderIdentity,
    },
}

/// Immutable current request subject supplied to the pure evaluator.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderRequestSubjectV1 {
    pub profile: ProviderEligibilityRequestProfile,
    pub project_identity: ProviderIdentity,
    pub root_identity: ProviderIdentity,
    pub workspace_generation: SourceGeneration,
    /// Document-instance / ABA identity for the accepted source view.
    pub document_instance_identity: ProviderIdentity,
    pub document_generation: SourceGeneration,
    pub parser_generation: SourceGeneration,
    pub semantic_contribution_generation: Option<SourceGeneration>,
    pub compiler_contribution_generation: Option<SourceGeneration>,
    pub configuration_identity: ProviderIdentity,
    pub environment_identity: ProviderIdentity,
    pub lifecycle: ProviderSubjectLifecycleState,
    pub readiness_state: ProviderReadinessState,
    pub deadline: ProviderQueryDeadline,
    pub cancellation: ProviderCancellationState,
}

/// Request-specific completeness requirement profile.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderCompletenessRequirementV1 {
    pub capability: ProviderQueryCapability,
    pub require_current_binding: bool,
    pub require_complete_denominator: bool,
    pub same_file_scope: bool,
    pub include_declaration: bool,
    /// When set, edit-producing requests must observe a complete edit denominator.
    pub edit_denominator_complete: Option<bool>,
}

impl ProviderCompletenessRequirementV1 {
    /// Lexical references cohort requirement from #12154.
    #[must_use]
    pub fn lexical_references_same_file(include_declaration: bool) -> Self {
        Self {
            capability: ProviderQueryCapability::References,
            require_current_binding: true,
            require_complete_denominator: true,
            same_file_scope: true,
            include_declaration,
            edit_denominator_complete: None,
        }
    }

    /// Edit-producing request families require an explicit complete edit denominator.
    #[must_use]
    pub fn edit_producing(capability: ProviderQueryCapability) -> Self {
        Self {
            capability,
            require_current_binding: true,
            require_complete_denominator: true,
            same_file_scope: false,
            include_declaration: false,
            edit_denominator_complete: Some(false),
        }
    }
}

/// Instrument state carried with result evidence.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum ProviderEligibilityInstrumentState {
    Ok,
    Failed { reason_code: String },
}

impl ProviderEligibilityInstrumentState {
    #[must_use]
    pub const fn ok() -> Self {
        Self::Ok
    }
}

/// Immutable semantic/compiler result evidence supplied to the pure evaluator.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderResultEvidenceV1 {
    pub project_identity: ProviderIdentity,
    pub root_identity: ProviderIdentity,
    pub workspace_generation: SourceGeneration,
    pub document_instance_identity: ProviderIdentity,
    pub document_generation: SourceGeneration,
    pub parser_generation: SourceGeneration,
    pub semantic_contribution_generation: Option<SourceGeneration>,
    pub compiler_contribution_generation: Option<SourceGeneration>,
    pub configuration_identity: ProviderIdentity,
    pub environment_identity: ProviderIdentity,
    pub capability: ProviderQueryCapability,
    pub completeness: ProviderSnapshotCompleteness,
    pub producer_lineage: Vec<SemanticProducer>,
    pub limitations: Vec<String>,
    pub instrument_state: ProviderEligibilityInstrumentState,
    /// Whether a current lexical binding exists for the request subject.
    pub has_current_binding: bool,
    /// Facts remaining after request-specific filtering; never used alone for exactness.
    pub filtered_fact_count: u32,
    /// Whether the producer asserts a complete request-specific denominator.
    pub denominator_complete: bool,
    pub dynamic_boundary_present: bool,
    pub recovered_facts_present: bool,
    pub mixed_producer_lineage: bool,
}

/// Stable blocker taxonomy retained on non-exact decisions.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderEligibilityBlockerClass {
    SourceDeleted,
    RootRemoved,
    DocumentInstanceMismatch,
    StaleDocumentGeneration,
    StaleWorkspaceGeneration,
    WrongRootOrProject,
    ConfigurationOrEnvironmentMismatch,
    ReadinessBoundary,
    IncompleteDenominator,
    NoMatchingEntity,
    FactFamilyMismatch,
    PartialOrRecoveredFacts,
    DynamicBoundary,
    AmbiguousFacts,
    UnsupportedRequestProfile,
    IncompleteEditDenominator,
    ProducerUpgradeAttempt,
    MissingRequiredField,
    AdapterLimitation,
}

/// Owner dimension implicated by a blocker decision.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderEligibilityOwnerDimension {
    Project,
    Root,
    Workspace,
    Document,
    Parser,
    Semantic,
    Compiler,
    Configuration,
    Environment,
    Readiness,
    Completeness,
    Instrument,
}

/// Closed eligibility outcome for one request/evidence pair.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderEligibilityOutcome {
    ExactEligible { exact_empty: bool },
    QualifiedEligible,
    FallbackRequired { reason: ProviderEligibilityBlockerClass },
    Refuse { reason: ProviderEligibilityBlockerClass },
    Stale { reason: ProviderEligibilityBlockerClass },
    Dynamic { reason: ProviderEligibilityBlockerClass },
    Ambiguous { reason: ProviderEligibilityBlockerClass },
    Unavailable { reason: ProviderEligibilityBlockerClass },
    Cancelled,
    DeadlineExceeded,
    InstrumentFailure,
    NotProven,
}

/// Typed eligibility decision plus stable blocker evidence.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderEligibilityDecisionV1 {
    pub outcome: ProviderEligibilityOutcome,
    pub evidence: ProviderEligibilityBlockerEvidence,
}

/// Stable evidence attached to every decision.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderEligibilityBlockerEvidence {
    pub blocker_class: Option<ProviderEligibilityBlockerClass>,
    pub owner_dimension: Option<ProviderEligibilityOwnerDimension>,
    pub expected_identity: Option<String>,
    pub observed_identity: Option<String>,
    pub required_capability: Option<ProviderQueryCapability>,
    pub limitations: Vec<String>,
    pub fallback_eligible: bool,
    pub instrument_state: ProviderEligibilityInstrumentState,
}

impl ProviderEligibilityBlockerEvidence {
    pub(crate) fn for_outcome(
        outcome: &ProviderEligibilityOutcome,
        limitations: Vec<String>,
        instrument_state: ProviderEligibilityInstrumentState,
    ) -> Self {
        let (blocker_class, owner_dimension, fallback_eligible) = match outcome {
            ProviderEligibilityOutcome::ExactEligible { .. }
            | ProviderEligibilityOutcome::QualifiedEligible => (None, None, false),
            ProviderEligibilityOutcome::FallbackRequired { reason }
            | ProviderEligibilityOutcome::Refuse { reason }
            | ProviderEligibilityOutcome::Stale { reason }
            | ProviderEligibilityOutcome::Dynamic { reason }
            | ProviderEligibilityOutcome::Ambiguous { reason }
            | ProviderEligibilityOutcome::Unavailable { reason } => {
                (Some(*reason), owner_for_blocker(*reason), fallback_for_blocker(*reason))
            }
            ProviderEligibilityOutcome::Cancelled
            | ProviderEligibilityOutcome::DeadlineExceeded => {
                (None, Some(ProviderEligibilityOwnerDimension::Instrument), false)
            }
            ProviderEligibilityOutcome::InstrumentFailure => {
                (None, Some(ProviderEligibilityOwnerDimension::Instrument), false)
            }
            ProviderEligibilityOutcome::NotProven => (
                Some(ProviderEligibilityBlockerClass::MissingRequiredField),
                Some(ProviderEligibilityOwnerDimension::Completeness),
                false,
            ),
        };
        Self {
            blocker_class,
            owner_dimension,
            expected_identity: None,
            observed_identity: None,
            required_capability: None,
            limitations,
            fallback_eligible,
            instrument_state,
        }
    }
}

fn owner_for_blocker(
    blocker: ProviderEligibilityBlockerClass,
) -> Option<ProviderEligibilityOwnerDimension> {
    Some(match blocker {
        ProviderEligibilityBlockerClass::SourceDeleted
        | ProviderEligibilityBlockerClass::DocumentInstanceMismatch => {
            ProviderEligibilityOwnerDimension::Document
        }
        ProviderEligibilityBlockerClass::RootRemoved
        | ProviderEligibilityBlockerClass::WrongRootOrProject => {
            ProviderEligibilityOwnerDimension::Root
        }
        ProviderEligibilityBlockerClass::StaleDocumentGeneration => {
            ProviderEligibilityOwnerDimension::Document
        }
        ProviderEligibilityBlockerClass::StaleWorkspaceGeneration => {
            ProviderEligibilityOwnerDimension::Workspace
        }
        ProviderEligibilityBlockerClass::ConfigurationOrEnvironmentMismatch => {
            ProviderEligibilityOwnerDimension::Configuration
        }
        ProviderEligibilityBlockerClass::ReadinessBoundary => {
            ProviderEligibilityOwnerDimension::Readiness
        }
        ProviderEligibilityBlockerClass::IncompleteDenominator
        | ProviderEligibilityBlockerClass::NoMatchingEntity
        | ProviderEligibilityBlockerClass::FactFamilyMismatch
        | ProviderEligibilityBlockerClass::IncompleteEditDenominator
        | ProviderEligibilityBlockerClass::MissingRequiredField
        | ProviderEligibilityBlockerClass::UnsupportedRequestProfile => {
            ProviderEligibilityOwnerDimension::Completeness
        }
        ProviderEligibilityBlockerClass::PartialOrRecoveredFacts
        | ProviderEligibilityBlockerClass::AdapterLimitation => {
            ProviderEligibilityOwnerDimension::Semantic
        }
        ProviderEligibilityBlockerClass::DynamicBoundary => {
            ProviderEligibilityOwnerDimension::Semantic
        }
        ProviderEligibilityBlockerClass::AmbiguousFacts => {
            ProviderEligibilityOwnerDimension::Semantic
        }
        ProviderEligibilityBlockerClass::ProducerUpgradeAttempt => {
            ProviderEligibilityOwnerDimension::Semantic
        }
    })
}

fn fallback_for_blocker(blocker: ProviderEligibilityBlockerClass) -> bool {
    matches!(
        blocker,
        ProviderEligibilityBlockerClass::PartialOrRecoveredFacts
            | ProviderEligibilityBlockerClass::ReadinessBoundary
            | ProviderEligibilityBlockerClass::AdapterLimitation
    )
}

pub(crate) fn identity_observed(identity: &ProviderIdentity) -> Option<String> {
    match identity {
        ProviderIdentity::Known(value) if !value.trim().is_empty() => Some(value.clone()),
        _ => None,
    }
}

pub(crate) fn generation_observed(generation: &SourceGeneration) -> Option<String> {
    match generation {
        SourceGeneration::Known(value) if !value.is_empty() => Some(value.clone()),
        _ => None,
    }
}

pub(crate) fn identity_matches(expected: &ProviderIdentity, observed: &ProviderIdentity) -> bool {
    matches!(
        (expected, observed),
        (ProviderIdentity::Known(left), ProviderIdentity::Known(right)) if left == right && !left.trim().is_empty()
    )
}

pub(crate) fn generation_matches(expected: &SourceGeneration, observed: &SourceGeneration) -> bool {
    expected.is_known() && observed.is_known() && expected == observed
}

pub(crate) fn optional_generation_matches(
    expected: Option<&SourceGeneration>,
    observed: Option<&SourceGeneration>,
) -> bool {
    match (expected, observed) {
        (None, None) => true,
        (Some(expected), Some(observed)) => generation_matches(expected, observed),
        (Some(expected), None) => !expected.is_known(),
        (None, Some(observed)) => !observed.is_known(),
    }
}

pub(crate) fn subject_has_required_fields(subject: &ProviderRequestSubjectV1) -> bool {
    subject.project_identity.is_known()
        && subject.root_identity.is_known()
        && subject.document_instance_identity.is_known()
        && subject.document_generation.is_known()
        && subject.workspace_generation.is_known()
        && subject.parser_generation.is_known()
        && subject.configuration_identity.is_known()
        && subject.environment_identity.is_known()
        && !subject.profile.request_class.trim().is_empty()
}

pub(crate) fn evidence_has_required_fields(evidence: &ProviderResultEvidenceV1) -> bool {
    evidence.project_identity.is_known()
        && evidence.root_identity.is_known()
        && evidence.document_instance_identity.is_known()
        && evidence.document_generation.is_known()
        && evidence.workspace_generation.is_known()
        && evidence.parser_generation.is_known()
        && evidence.configuration_identity.is_known()
        && evidence.environment_identity.is_known()
}
