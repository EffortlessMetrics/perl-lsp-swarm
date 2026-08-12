//! Checked framework-adapter SDK vocabulary.
//!
//! Serialized request structures retain only admission-time snapshots. Live
//! cancellation is supplied separately through [`AdapterCancellationControl`]
//! and cannot be represented truthfully in JSON.
//!
//! Wire compatibility is versioned. Additive struct fields are tolerated by
//! older consumers because Serde ignores unknown fields by default, but adding
//! an enum discriminant requires a new schema version: `#[non_exhaustive]` is a
//! Rust source-compatibility tool, not an unknown-variant wire fallback.

use serde::{Deserialize, Serialize};

use crate::{
    AnchorId, Confidence, FileId, InvalidationDependency, LifecyclePhase, Provenance,
    SemanticConfidence, SemanticFactEnvelope, SemanticFactStatus, SemanticFreshness,
    SemanticProducer, SemanticProvenance, SourceGeneration,
};

/// Current framework-adapter SDK wire version.
pub const FRAMEWORK_ADAPTER_SDK_VERSION: &str = "framework_adapter_sdk.v1";

/// Current numeric schema version for descriptor/result records.
pub const FRAMEWORK_ADAPTER_SCHEMA_VERSION: u32 = 1;

/// Stable opaque identity for a registered adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct AdapterId(pub u64);

/// Opaque identity for one invocation-local fact sink.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct FactSinkId(pub u64);

/// Deployment disposition for an adapter.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum AdapterDisposition {
    /// Output may be published after validation.
    Production,
    /// Output is comparison-only.
    Shadow,
    /// Output is experimental and not publication authority.
    Experimental,
}

/// Versioned self-description of one adapter.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterDescriptor {
    /// Stable adapter identity.
    pub adapter_id: AdapterId,
    /// Human-readable adapter name.
    pub name: String,
    /// Framework or module family handled by the adapter.
    pub framework_name: String,
    /// Optional registry-level framework-version constraint.
    pub framework_version_constraint: Option<String>,
    /// Descriptor schema version.
    pub schema_version: u32,
    /// Deployment disposition.
    pub disposition: AdapterDisposition,
}

impl AdapterDescriptor {
    /// Construct a descriptor.
    #[must_use]
    pub fn new(
        adapter_id: AdapterId,
        name: impl Into<String>,
        framework_name: impl Into<String>,
        framework_version_constraint: Option<String>,
        schema_version: u32,
        disposition: AdapterDisposition,
    ) -> Self {
        Self {
            adapter_id,
            name: name.into(),
            framework_name: framework_name.into(),
            framework_version_constraint,
            schema_version,
            disposition,
        }
    }

    fn is_valid_authority(&self) -> bool {
        self.schema_version == FRAMEWORK_ADAPTER_SCHEMA_VERSION
            && self.disposition == AdapterDisposition::Production
            && !self.name.trim().is_empty()
            && !self.framework_name.trim().is_empty()
    }
}

/// Observed version evidence for one activation module.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ModuleVersionEvidence {
    /// Version spelling observed by the project model.
    pub version: String,
    /// Generation that produced the version observation.
    pub generation: SourceGeneration,
}

impl ModuleVersionEvidence {
    /// Construct known module-version evidence.
    #[must_use]
    pub fn new(version: impl Into<String>, generation: SourceGeneration) -> Self {
        Self {
            version: version.into(),
            generation,
        }
    }

    fn is_known(&self) -> bool {
        !self.version.trim().is_empty() && self.generation.is_known()
    }
}

/// Identity of one module available to the detection pass.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ModuleActivationIdentity {
    /// Fully qualified module name.
    pub module_name: String,
    /// Optional source file identity.
    pub file_id: Option<FileId>,
    /// Source generation represented by this activation row.
    pub generation: SourceGeneration,
    /// Optional observed framework/module version.
    #[serde(default)]
    pub observed_version: Option<ModuleVersionEvidence>,
}

impl ModuleActivationIdentity {
    /// Construct an activation identity without version evidence.
    #[must_use]
    pub fn new(
        module_name: impl Into<String>,
        file_id: Option<FileId>,
        generation: SourceGeneration,
    ) -> Self {
        Self {
            module_name: module_name.into(),
            file_id,
            generation,
            observed_version: None,
        }
    }

    /// Attach version evidence produced from the same module generation.
    #[must_use]
    pub fn with_observed_version(mut self, evidence: ModuleVersionEvidence) -> Self {
        self.observed_version = Some(evidence);
        self
    }
}

/// Serializable cancellation state captured when work is admitted.
///
/// This is deliberately a snapshot. Runtime implementations receive a live
/// [`AdapterCancellationControl`] separately and poll that control during work.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterCancellation {
    /// Whether cancellation had already been requested at admission.
    pub is_cancelled: bool,
}

impl AdapterCancellation {
    /// Construct an active admission snapshot.
    #[must_use]
    pub const fn active() -> Self {
        Self {
            is_cancelled: false,
        }
    }

    /// Construct a pre-cancelled admission snapshot.
    #[must_use]
    pub const fn cancelled() -> Self {
        Self { is_cancelled: true }
    }
}

/// Live cancellation port supplied alongside serialized adapter input.
pub trait AdapterCancellationControl: Send + Sync {
    /// Whether cancellation is currently requested.
    fn is_cancelled(&self) -> bool;
}

/// Live control that never cancels.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopAdapterCancellationControl;

impl AdapterCancellationControl for NoopAdapterCancellationControl {
    fn is_cancelled(&self) -> bool {
        false
    }
}

/// Resource budget for one detection or invocation operation.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterBudget {
    /// Maximum emitted fact count.
    pub max_emitted_facts: u32,
    /// Maximum serialized payload bytes.
    pub max_payload_bytes: u64,
}

impl AdapterBudget {
    /// Construct a budget.
    #[must_use]
    pub const fn new(max_emitted_facts: u32, max_payload_bytes: u64) -> Self {
        Self {
            max_emitted_facts,
            max_payload_bytes,
        }
    }
}

/// Input to a framework-detection pass.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterDetectionInput {
    /// Adapter descriptor.
    pub descriptor: AdapterDescriptor,
    /// Modules visible in the current project model.
    pub available_modules: Vec<ModuleActivationIdentity>,
    /// Project-model generation.
    pub project_generation: SourceGeneration,
    /// Optional digest over the activation list.
    pub content_digest: Option<String>,
    /// Optional resource budget.
    pub budget: Option<AdapterBudget>,
    /// Admission-time cancellation snapshot.
    pub cancellation: AdapterCancellation,
}

impl AdapterDetectionInput {
    /// Construct a detection input.
    #[must_use]
    pub fn new(
        descriptor: AdapterDescriptor,
        available_modules: Vec<ModuleActivationIdentity>,
        project_generation: SourceGeneration,
        content_digest: Option<String>,
        budget: Option<AdapterBudget>,
        cancellation: AdapterCancellation,
    ) -> Self {
        Self {
            descriptor,
            available_modules,
            project_generation,
            content_digest,
            budget,
            cancellation,
        }
    }

    /// Whether the input contains coherent version evidence for `module_name`.
    #[must_use]
    pub fn has_version_evidence(&self, module_name: &str) -> bool {
        self.available_modules.iter().any(|module| {
            module.module_name == module_name
                && module.generation.is_known()
                && module
                    .observed_version
                    .as_ref()
                    .is_some_and(|version| version.is_known() && version.generation == module.generation)
        })
    }
}

/// Structured reason a framework is absent.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DetectionAbsenceReason {
    /// Required activation modules were missing.
    RequiredModulesMissing,
    /// Observed version evidence did not satisfy the descriptor constraint.
    VersionConstraintNotSatisfied,
    /// Project configuration explicitly excluded the framework.
    ExcludedByConfiguration,
}

/// Reason detection or invocation could not proceed.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnavailableReason {
    /// Required generation identity was unavailable.
    MissingGeneration,
    /// No activation modules were available.
    NoModulesAvailable,
    /// An internal invariant failed.
    InternalError,
}

/// Outcome of one framework-detection pass.
///
/// Enum additions are wire-schema changes and require a new SDK version.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DetectionOutcome {
    /// Framework presence was established.
    Detected {
        /// Detection confidence.
        confidence: Confidence,
        /// Observed framework version, when known.
        framework_version: Option<String>,
    },
    /// Framework absence was established.
    Absent {
        /// Absence reason.
        reason: DetectionAbsenceReason,
    },
    /// Conflicting signals prevented one answer.
    Conflicting {
        /// Conflict descriptions.
        conflict_descriptions: Vec<String>,
    },
    /// Detection could not execute.
    Unavailable {
        /// Unavailability reason.
        reason: UnavailableReason,
    },
    /// Detection was cancelled.
    Cancelled,
    /// Detection exhausted its resource budget.
    BudgetExhausted,
    /// The adapter does not support this configuration.
    Unsupported {
        /// Bounded explanation.
        reason: String,
    },
}

/// Result of one detection pass.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterDetectionResult {
    /// Adapter descriptor.
    pub descriptor: AdapterDescriptor,
    /// Project generation represented by the result.
    pub project_generation: SourceGeneration,
    /// Detection outcome.
    pub outcome: DetectionOutcome,
    /// Version evidence used for a version-qualified absence.
    #[serde(default)]
    pub version_evidence: Option<ModuleVersionEvidence>,
}

impl AdapterDetectionResult {
    /// Construct a detection result without version evidence.
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
        }
    }

    /// Attach the observed version used for a version-qualified verdict.
    #[must_use]
    pub fn with_version_evidence(mut self, evidence: ModuleVersionEvidence) -> Self {
        self.version_evidence = Some(evidence);
        self
    }

    /// Whether this result reports framework presence.
    #[must_use]
    pub fn is_detected(&self) -> bool {
        matches!(self.outcome, DetectionOutcome::Detected { .. })
    }

    /// Whether this result is eligible to act as detection authority.
    #[must_use]
    pub fn is_authoritative(&self) -> bool {
        if !self.descriptor.is_valid_authority() || !self.project_generation.is_known() {
            return false;
        }
        match &self.outcome {
            DetectionOutcome::Detected {
                confidence,
                framework_version,
            } => {
                *confidence == Confidence::High
                    && self
                        .descriptor
                        .framework_version_constraint
                        .as_ref()
                        .is_none_or(|_| framework_version.as_ref().is_some_and(|value| !value.trim().is_empty()))
            }
            DetectionOutcome::Absent {
                reason: DetectionAbsenceReason::VersionConstraintNotSatisfied,
            } => {
                self.descriptor.framework_version_constraint.is_some()
                    && self.version_evidence.as_ref().is_some_and(|evidence| {
                        evidence.is_known() && evidence.generation == self.project_generation
                    })
            }
            DetectionOutcome::Absent { .. } => true,
            _ => false,
        }
    }
}

/// Class of semantic facts an adapter may emit.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum FactClass {
    /// Framework-generated members.
    GeneratedMembers,
    /// Package inheritance or role-composition facts.
    PackageGraph,
    /// Framework import/export facts.
    FrameworkImports,
    /// Framework diagnostics.
    Diagnostics,
    /// Versioned extension class.
    Extension,
}

/// Source scope presented to one adapter invocation.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterSourceScope {
    /// Primary source file.
    pub primary_file_id: FileId,
    /// Primary source generation.
    pub primary_generation: SourceGeneration,
    /// Optional content digest.
    pub primary_content_digest: Option<String>,
    /// Activation statement anchor.
    pub activation_anchor_id: Option<AnchorId>,
    /// Package or class name.
    pub package_name: Option<String>,
}

impl AdapterSourceScope {
    /// Construct a source scope.
    #[must_use]
    pub fn new(
        primary_file_id: FileId,
        primary_generation: SourceGeneration,
        primary_content_digest: Option<String>,
        activation_anchor_id: Option<AnchorId>,
        package_name: Option<String>,
    ) -> Self {
        Self {
            primary_file_id,
            primary_generation,
            primary_content_digest,
            activation_anchor_id,
            package_name,
        }
    }
}

/// Input to one adapter invocation.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterInput {
    /// Adapter descriptor.
    pub descriptor: AdapterDescriptor,
    /// Source scope.
    pub source_scope: AdapterSourceScope,
    /// Fact classes requested by the caller.
    pub required_fact_classes: Vec<FactClass>,
    /// Invalidation dependencies.
    pub invalidation_inputs: Vec<InvalidationDependency>,
    /// Optional budget.
    pub budget: Option<AdapterBudget>,
    /// Admission-time cancellation snapshot.
    pub cancellation: AdapterCancellation,
}

impl AdapterInput {
    /// Construct an invocation input.
    #[must_use]
    pub fn new(
        descriptor: AdapterDescriptor,
        source_scope: AdapterSourceScope,
        required_fact_classes: Vec<FactClass>,
        invalidation_inputs: Vec<InvalidationDependency>,
        budget: Option<AdapterBudget>,
        cancellation: AdapterCancellation,
    ) -> Self {
        Self {
            descriptor,
            source_scope,
            required_fact_classes,
            invalidation_inputs,
            budget,
            cancellation,
        }
    }
}

/// Reason a fact is bounded or unusable.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactLimitation {
    /// Human-readable description.
    pub description: String,
    /// Whether this limitation blocks publication.
    pub is_blocking: bool,
    /// Maximum confidence allowed by the limitation.
    pub confidence_impact: Confidence,
}

impl FactLimitation {
    /// Construct a limitation.
    #[must_use]
    pub fn new(
        description: impl Into<String>,
        is_blocking: bool,
        confidence_impact: Confidence,
    ) -> Self {
        Self {
            description: description.into(),
            is_blocking,
            confidence_impact,
        }
    }
}

/// One semantic fact emitted by an adapter.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmittedFact {
    /// Sink identity.
    pub sink_id: FactSinkId,
    /// Adapter identity.
    pub adapter_id: AdapterId,
    /// Framework name.
    pub framework_name: String,
    /// Adapter-level provenance.
    pub provenance: Provenance,
    /// Adapter-level confidence.
    pub confidence: Confidence,
    /// Canonical semantic envelope.
    pub envelope: SemanticFactEnvelope,
    /// Fact class.
    pub fact_class: FactClass,
    /// Optional limitation.
    pub limitation: Option<FactLimitation>,
    /// Untrusted compatibility hint from the producing adapter.
    ///
    /// Downstream code must use [`EmittedFact::can_override_generated`] instead
    /// of trusting this serialized field directly.
    pub is_stronger_than_generated: bool,
}

impl EmittedFact {
    /// Construct an emitted fact.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        sink_id: FactSinkId,
        adapter_id: AdapterId,
        framework_name: impl Into<String>,
        provenance: Provenance,
        confidence: Confidence,
        envelope: SemanticFactEnvelope,
        fact_class: FactClass,
        limitation: Option<FactLimitation>,
        is_stronger_than_generated: bool,
    ) -> Self {
        Self {
            sink_id,
            adapter_id,
            framework_name: framework_name.into(),
            provenance,
            confidence,
            envelope,
            fact_class,
            limitation,
            is_stronger_than_generated,
        }
    }

    /// Whether this fact has a blocking limitation.
    #[must_use]
    pub fn is_blocked(&self) -> bool {
        self.limitation
            .as_ref()
            .is_some_and(|limitation| limitation.is_blocking)
    }

    /// Whether source-backed evidence validates the precedence hint.
    #[must_use]
    pub fn can_override_generated(&self) -> bool {
        if !self.is_stronger_than_generated {
            return false;
        }
        matches!(
            self.provenance,
            Provenance::ExactAst
                | Provenance::DesugaredAst
                | Provenance::SemanticAnalyzer
                | Provenance::LiteralRequireImport
        ) && matches!(
            self.envelope.provenance,
            SemanticProvenance::Known(value) if value == self.provenance
        ) && self.envelope.reason_code == crate::SemanticReasonCode::ExactSource
    }

    fn is_structurally_coherent(
        &self,
        descriptor: &AdapterDescriptor,
        scope: &AdapterSourceScope,
        sink_id: FactSinkId,
        generation: &SourceGeneration,
    ) -> bool {
        self.sink_id == sink_id
            && self.adapter_id == descriptor.adapter_id
            && self.framework_name == descriptor.framework_name
            && !self.framework_name.trim().is_empty()
            && self.envelope.anchor.file_id == scope.primary_file_id
            && self.envelope.anchor.start_byte <= self.envelope.anchor.end_byte
            && &self.envelope.source_generation == generation
            && self.envelope.producer == SemanticProducer::FrameworkAdapter
            && self.envelope.freshness == SemanticFreshness::Fresh
            && !matches!(
                self.envelope.status(),
                SemanticFactStatus::Stale | SemanticFactStatus::Refused
            )
            && matches!(
                self.envelope.provenance,
                SemanticProvenance::Known(value) if value == self.provenance
            )
            && matches!(
                self.envelope.confidence,
                SemanticConfidence::Known(value) if value == self.confidence
            )
            && dependencies_are_coherent(self.envelope.invalidation_dependencies())
            && (!self.is_stronger_than_generated || self.can_override_generated())
            && !self.is_blocked()
    }
}

/// Bounded collection produced by one invocation.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactSink {
    /// Sink identity.
    pub sink_id: FactSinkId,
    /// Producing adapter identity.
    pub adapter_id: AdapterId,
    /// Ordered emitted facts.
    pub facts: Vec<EmittedFact>,
    /// Serialized payload bytes.
    pub total_payload_bytes: u64,
}

impl FactSink {
    /// Construct an empty sink.
    #[must_use]
    pub const fn new(sink_id: FactSinkId, adapter_id: AdapterId) -> Self {
        Self {
            sink_id,
            adapter_id,
            facts: Vec::new(),
            total_payload_bytes: 0,
        }
    }

    /// Whether the sink is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.facts.is_empty()
    }

    /// Number of emitted facts.
    #[must_use]
    pub fn len(&self) -> usize {
        self.facts.len()
    }

    /// Iterate blocking facts.
    pub fn blocking_limited_facts(&self) -> impl Iterator<Item = &EmittedFact> {
        self.facts.iter().filter(|fact| fact.is_blocked())
    }

    /// Iterate facts without blocking limitations.
    pub fn usable_facts(&self) -> impl Iterator<Item = &EmittedFact> {
        self.facts.iter().filter(|fact| !fact.is_blocked())
    }

    /// Iterate facts whose source-backed precedence is validated.
    pub fn source_precedence_facts(&self) -> impl Iterator<Item = &EmittedFact> {
        self.facts.iter().filter(|fact| fact.can_override_generated())
    }
}

/// Outcome of one adapter invocation.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdapterOutcome {
    /// Adapter completed.
    Applied {
        /// Produced sink.
        sink: FactSink,
        /// Result-level limitations.
        limitations: Vec<FactLimitation>,
    },
    /// A dynamic boundary prevented completion.
    Dynamic {
        /// Boundary explanation.
        reason: String,
        /// Partial facts.
        partial_sink: Option<FactSink>,
    },
    /// Configuration is unsupported.
    Unsupported {
        /// Explanation.
        reason: String,
    },
    /// Conflicting signals prevented a result.
    Conflict {
        /// Conflict descriptions.
        conflict_descriptions: Vec<String>,
    },
    /// Budget was exhausted.
    BudgetExhausted {
        /// Partial facts.
        partial_sink: Option<FactSink>,
    },
    /// Invocation was cancelled.
    Cancelled,
}

/// Checked authority failure for an adapter result.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdapterAuthorityError {
    /// Result or descriptor schema is unsupported.
    UnsupportedSchema,
    /// Adapter disposition is not production.
    NonProduction,
    /// Generations are unknown or disagree.
    GenerationMismatch,
    /// Outcome is not complete `Applied`.
    IncompleteOutcome,
    /// A result-level blocking limitation exists.
    BlockingLimitation,
    /// Sink identity disagrees with the descriptor.
    SinkIdentityMismatch,
    /// One emitted fact is structurally incoherent.
    InvalidFact,
}

/// Full result of one adapter invocation.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterResult {
    /// Result schema version.
    pub schema_version: u32,
    /// Adapter descriptor.
    pub descriptor: AdapterDescriptor,
    /// Source scope.
    pub source_scope: AdapterSourceScope,
    /// Invocation generation.
    pub invocation_generation: SourceGeneration,
    /// Invocation outcome.
    pub outcome: AdapterOutcome,
}

impl AdapterResult {
    /// Construct a result.
    #[must_use]
    pub fn new(
        descriptor: AdapterDescriptor,
        source_scope: AdapterSourceScope,
        invocation_generation: SourceGeneration,
        outcome: AdapterOutcome,
    ) -> Self {
        Self {
            schema_version: FRAMEWORK_ADAPTER_SCHEMA_VERSION,
            descriptor,
            source_scope,
            invocation_generation,
            outcome,
        }
    }

    /// Whether any complete or partial sink contains facts.
    #[must_use]
    pub fn has_facts(&self) -> bool {
        match &self.outcome {
            AdapterOutcome::Applied { sink, .. } => !sink.is_empty(),
            AdapterOutcome::Dynamic { partial_sink, .. }
            | AdapterOutcome::BudgetExhausted { partial_sink } => {
                partial_sink.as_ref().is_some_and(|sink| !sink.is_empty())
            }
            _ => false,
        }
    }

    /// Validate whether this result may act as publication authority.
    pub fn validate_authority(&self) -> Result<(), AdapterAuthorityError> {
        if self.schema_version != FRAMEWORK_ADAPTER_SCHEMA_VERSION
            || self.descriptor.schema_version != FRAMEWORK_ADAPTER_SCHEMA_VERSION
        {
            return Err(AdapterAuthorityError::UnsupportedSchema);
        }
        if self.descriptor.disposition != AdapterDisposition::Production {
            return Err(AdapterAuthorityError::NonProduction);
        }
        if !self.source_scope.primary_generation.is_known()
            || !self.invocation_generation.is_known()
            || self.source_scope.primary_generation != self.invocation_generation
        {
            return Err(AdapterAuthorityError::GenerationMismatch);
        }

        let AdapterOutcome::Applied { sink, limitations } = &self.outcome else {
            return Err(AdapterAuthorityError::IncompleteOutcome);
        };
        if limitations.iter().any(|limitation| limitation.is_blocking) {
            return Err(AdapterAuthorityError::BlockingLimitation);
        }
        if sink.adapter_id != self.descriptor.adapter_id {
            return Err(AdapterAuthorityError::SinkIdentityMismatch);
        }
        if sink.facts.iter().any(|fact| {
            !fact.is_structurally_coherent(
                &self.descriptor,
                &self.source_scope,
                sink.sink_id,
                &self.invocation_generation,
            )
        }) {
            return Err(AdapterAuthorityError::InvalidFact);
        }
        Ok(())
    }

    /// Whether this result passed the complete authority contract.
    #[must_use]
    pub fn is_authoritative(&self) -> bool {
        self.validate_authority().is_ok()
    }
}

fn dependencies_are_coherent(dependencies: &[InvalidationDependency]) -> bool {
    if dependencies
        .iter()
        .any(|dependency| dependency.dependency_key.trim().is_empty() || !dependency.generation.is_known())
    {
        return false;
    }
    dependencies.windows(2).all(|pair| {
        pair[0].dependency_key != pair[1].dependency_key
            || pair[0].generation == pair[1].generation
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        EntityId, FactId, SemanticFactKind, SemanticReasonCode, SourceAnchor,
    };
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    fn descriptor(disposition: AdapterDisposition) -> AdapterDescriptor {
        AdapterDescriptor::new(
            AdapterId(1),
            "moo",
            "Moo",
            None,
            1,
            disposition,
        )
    }

    fn scope() -> AdapterSourceScope {
        AdapterSourceScope::new(
            FileId(10),
            SourceGeneration::known("generation-1"),
            None,
            Some(AnchorId(2)),
            Some("Example".to_string()),
        )
    }

    fn envelope(provenance: Provenance) -> SemanticFactEnvelope {
        SemanticFactEnvelope::new(
            FactId(1),
            Some(EntityId(2)),
            SemanticFactKind::Declaration,
            SourceAnchor::new(Some(AnchorId(3)), FileId(10), 4, 12),
            SourceGeneration::known("generation-1"),
            None,
            Some("Example".to_string()),
            LifecyclePhase::Runtime,
            SemanticProducer::FrameworkAdapter,
            SemanticProvenance::Known(provenance),
            SemanticConfidence::Known(Confidence::High),
            SemanticFreshness::Fresh,
            None,
            Vec::new(),
            if matches!(
                provenance,
                Provenance::ExactAst
                    | Provenance::DesugaredAst
                    | Provenance::SemanticAnalyzer
                    | Provenance::LiteralRequireImport
            ) {
                SemanticReasonCode::ExactSource
            } else {
                SemanticReasonCode::GeneratedFromSource
            },
        )
    }

    fn applied(disposition: AdapterDisposition, fact: EmittedFact) -> AdapterResult {
        let mut sink = FactSink::new(FactSinkId(7), AdapterId(1));
        sink.facts.push(fact);
        AdapterResult::new(
            descriptor(disposition),
            scope(),
            SourceGeneration::known("generation-1"),
            AdapterOutcome::Applied {
                sink,
                limitations: Vec::new(),
            },
        )
    }

    struct AtomicControl(Arc<AtomicBool>);

    impl AdapterCancellationControl for AtomicControl {
        fn is_cancelled(&self) -> bool {
            self.0.load(Ordering::SeqCst)
        }
    }

    #[test]
    fn live_cancellation_can_change_after_dispatch() {
        let flag = Arc::new(AtomicBool::new(false));
        let control = AtomicControl(Arc::clone(&flag));
        assert!(!control.is_cancelled());
        flag.store(true, Ordering::SeqCst);
        assert!(control.is_cancelled());
    }

    #[test]
    fn version_qualified_absence_requires_version_evidence() {
        let result = AdapterDetectionResult::new(
            AdapterDescriptor::new(
                AdapterId(1),
                "moo",
                "Moo",
                Some(">=2".to_string()),
                1,
                AdapterDisposition::Production,
            ),
            SourceGeneration::known("project-1"),
            DetectionOutcome::Absent {
                reason: DetectionAbsenceReason::VersionConstraintNotSatisfied,
            },
        );
        assert!(!result.is_authoritative());

        let result = result.with_version_evidence(ModuleVersionEvidence::new(
            "1.9",
            SourceGeneration::known("project-1"),
        ));
        assert!(result.is_authoritative());
    }

    #[test]
    fn shadow_and_experimental_results_are_not_authoritative() {
        for disposition in [
            AdapterDisposition::Shadow,
            AdapterDisposition::Experimental,
        ] {
            let fact = EmittedFact::new(
                FactSinkId(7),
                AdapterId(1),
                "Moo",
                Provenance::FrameworkSynthesis,
                Confidence::High,
                envelope(Provenance::FrameworkSynthesis),
                FactClass::GeneratedMembers,
                None,
                false,
            );
            assert!(!applied(disposition, fact).is_authoritative());
        }
    }

    #[test]
    fn generation_and_sink_identity_must_be_coherent() {
        let fact = EmittedFact::new(
            FactSinkId(7),
            AdapterId(1),
            "Moo",
            Provenance::FrameworkSynthesis,
            Confidence::High,
            envelope(Provenance::FrameworkSynthesis),
            FactClass::GeneratedMembers,
            None,
            false,
        );
        let mut result = applied(AdapterDisposition::Production, fact);
        result.invocation_generation = SourceGeneration::known("generation-2");
        assert_eq!(
            result.validate_authority(),
            Err(AdapterAuthorityError::GenerationMismatch)
        );
    }

    #[test]
    fn generated_precedence_hint_is_not_validated() {
        let fact = EmittedFact::new(
            FactSinkId(7),
            AdapterId(1),
            "Moo",
            Provenance::FrameworkSynthesis,
            Confidence::High,
            envelope(Provenance::FrameworkSynthesis),
            FactClass::GeneratedMembers,
            None,
            true,
        );
        assert!(fact.is_stronger_than_generated);
        assert!(!fact.can_override_generated());
        assert!(!applied(AdapterDisposition::Production, fact).is_authoritative());
    }

    #[test]
    fn exact_source_precedence_can_be_validated() {
        let fact = EmittedFact::new(
            FactSinkId(7),
            AdapterId(1),
            "Moo",
            Provenance::ExactAst,
            Confidence::High,
            envelope(Provenance::ExactAst),
            FactClass::GeneratedMembers,
            None,
            true,
        );
        assert!(fact.can_override_generated());
        assert!(applied(AdapterDisposition::Production, fact).is_authoritative());
    }

    #[test]
    fn future_enum_variant_requires_a_schema_bump() {
        let future = r#"{"FutureVariant":{"payload":1}}"#;
        assert!(serde_json::from_str::<DetectionOutcome>(future).is_err());
    }
}
