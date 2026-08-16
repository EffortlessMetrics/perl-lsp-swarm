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
use std::collections::BTreeSet;

use crate::{
    AnchorId, Confidence, FileId, InvalidationDependency, Provenance, SemanticConfidence,
    SemanticFactEnvelope, SemanticFactStatus, SemanticFreshness, SemanticProducer,
    SemanticProvenance, SourceGeneration,
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
        Self { version: version.into(), generation }
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
        Self { module_name: module_name.into(), file_id, generation, observed_version: None }
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
        Self { is_cancelled: false }
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
        Self { max_emitted_facts, max_payload_bytes }
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
        mut invalidation_inputs: Vec<InvalidationDependency>,
        budget: Option<AdapterBudget>,
        cancellation: AdapterCancellation,
    ) -> Self {
        invalidation_inputs.sort();
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
        Self { description: description.into(), is_blocking, confidence_impact }
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
        self.limitation.as_ref().is_some_and(|limitation| limitation.is_blocking)
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
        Self { sink_id, adapter_id, facts: Vec::new(), total_payload_bytes: 0 }
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
    /// A result must be checked against the input admitted for the invocation.
    InputRequired,
    /// Result identity does not match the admitted input.
    InputMismatch,
    /// The result exceeded the admitted fact-count or payload budget.
    BudgetExceeded,
    /// The result emitted a class that was not requested by the input.
    UnsupportedFactClass,
    /// An emitted fact did not preserve the input invalidation dependencies.
    InvalidationMismatch,
    /// The sink payload total was not the canonical serialized fact payload size.
    PayloadMismatch,
    /// Two emitted facts reused one canonical fact identity.
    DuplicateFactId,
    /// A fact package did not match the invocation source scope.
    PackageIdentityMismatch,
    /// A fact confidence exceeded a declared limitation ceiling.
    ConfidenceLimitExceeded,
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

    /// Fail closed because publication authority requires the admitted invocation input.
    pub fn validate_authority(&self) -> Result<(), AdapterAuthorityError> {
        Err(AdapterAuthorityError::InputRequired)
    }

    fn validate_structure(&self) -> Result<(), AdapterAuthorityError> {
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

    /// Validate this result against the exact input admitted for the invocation.
    pub fn validate_authority_against(
        &self,
        input: &AdapterInput,
    ) -> Result<(), AdapterAuthorityError> {
        self.validate_structure()?;
        if self.descriptor != input.descriptor
            || self.source_scope != input.source_scope
            || self.invocation_generation != input.source_scope.primary_generation
        {
            return Err(AdapterAuthorityError::InputMismatch);
        }

        let AdapterOutcome::Applied { sink, limitations } = &self.outcome else {
            return Err(AdapterAuthorityError::IncompleteOutcome);
        };
        if let Some(budget) = input.budget
            && (sink.facts.len() > budget.max_emitted_facts as usize
                || sink.total_payload_bytes > budget.max_payload_bytes)
        {
            return Err(AdapterAuthorityError::BudgetExceeded);
        }
        if sink.serialized_payload_bytes() != Some(sink.total_payload_bytes) {
            return Err(AdapterAuthorityError::PayloadMismatch);
        }

        let mut fact_ids = BTreeSet::new();
        for fact in &sink.facts {
            if !fact_ids.insert(fact.envelope.fact_id) {
                return Err(AdapterAuthorityError::DuplicateFactId);
            }
            if !input.required_fact_classes.contains(&fact.fact_class) {
                return Err(AdapterAuthorityError::UnsupportedFactClass);
            }
            if fact.envelope.package != input.source_scope.package_name {
                return Err(AdapterAuthorityError::PackageIdentityMismatch);
            }
            if !dependencies_match(
                fact.envelope.invalidation_dependencies(),
                &input.invalidation_inputs,
            ) {
                return Err(AdapterAuthorityError::InvalidationMismatch);
            }
            if fact.limitation.as_ref().is_some_and(|limitation| {
                confidence_exceeds(fact.confidence, limitation.confidence_impact)
            }) || limitations
                .iter()
                .any(|limitation| confidence_exceeds(fact.confidence, limitation.confidence_impact))
            {
                return Err(AdapterAuthorityError::ConfidenceLimitExceeded);
            }
        }
        Ok(())
    }

    /// Whether this result passed the complete authority contract.
    #[must_use]
    pub fn is_authoritative(&self) -> bool {
        false
    }

    /// Whether this result passed the complete authority contract for `input`.
    #[must_use]
    pub fn is_authoritative_against(&self, input: &AdapterInput) -> bool {
        self.validate_authority_against(input).is_ok()
    }
}

impl FactSink {
    /// Return the canonical serialized size of the emitted fact payload.
    #[must_use]
    pub fn serialized_payload_bytes(&self) -> Option<u64> {
        if self.facts.is_empty() {
            return Some(0);
        }
        serde_json::to_vec(&self.facts).ok().and_then(|payload| u64::try_from(payload.len()).ok())
    }
}

fn dependencies_match(
    actual: &[InvalidationDependency],
    expected: &[InvalidationDependency],
) -> bool {
    let mut actual = actual.to_vec();
    let mut expected = expected.to_vec();
    actual.sort();
    expected.sort();
    dependencies_are_coherent(&actual) && dependencies_are_coherent(&expected) && actual == expected
}

fn confidence_exceeds(actual: Confidence, ceiling: Confidence) -> bool {
    matches!(
        (actual, ceiling),
        (Confidence::High, Confidence::Medium | Confidence::Low)
            | (Confidence::Medium, Confidence::Low)
    )
}

fn dependencies_are_coherent(dependencies: &[InvalidationDependency]) -> bool {
    if dependencies.iter().any(|dependency| {
        dependency.dependency_key.trim().is_empty() || !dependency.generation.is_known()
    }) {
        return false;
    }
    dependencies.windows(2).all(|pair| {
        pair[0].dependency_key != pair[1].dependency_key || pair[0].generation == pair[1].generation
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    // `LifecyclePhase` is referenced only from these tests, so importing it at
    // module scope makes it an unused import in a non-test build.
    use crate::{
        EntityId, FactId, LifecyclePhase, SemanticFactKind, SemanticReasonCode, SourceAnchor,
    };
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    fn descriptor(disposition: AdapterDisposition) -> AdapterDescriptor {
        AdapterDescriptor::new(AdapterId(1), "moo", "Moo", None, 1, disposition)
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
        if let Some(bytes) = sink.serialized_payload_bytes() {
            sink.total_payload_bytes = bytes;
        }
        AdapterResult::new(
            descriptor(disposition),
            scope(),
            SourceGeneration::known("generation-1"),
            AdapterOutcome::Applied { sink, limitations: Vec::new() },
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
    fn shadow_and_experimental_results_are_not_authoritative() {
        for disposition in [AdapterDisposition::Shadow, AdapterDisposition::Experimental] {
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
            assert_eq!(
                applied(disposition, fact).validate_authority_against(&input()),
                Err(AdapterAuthorityError::NonProduction)
            );
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
            result.validate_authority_against(&input()),
            Err(AdapterAuthorityError::GenerationMismatch)
        );
    }

    fn input() -> AdapterInput {
        AdapterInput::new(
            descriptor(AdapterDisposition::Production),
            scope(),
            vec![FactClass::GeneratedMembers],
            Vec::new(),
            Some(AdapterBudget::new(1, 4096)),
            AdapterCancellation::active(),
        )
    }

    #[test]
    fn authority_requires_the_admitted_input() {
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
        let result = applied(AdapterDisposition::Production, fact);
        assert_eq!(result.validate_authority(), Err(AdapterAuthorityError::InputRequired));
        assert!(!result.is_authoritative());
        assert!(result.is_authoritative_against(&input()));
    }

    #[test]
    fn authority_rejects_budget_class_dependency_and_payload_forgery() {
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

        let mut budget = input();
        budget.budget = Some(AdapterBudget::new(0, 4096));
        assert_eq!(
            result.validate_authority_against(&budget),
            Err(AdapterAuthorityError::BudgetExceeded)
        );

        let mut unsupported = input();
        unsupported.required_fact_classes = vec![FactClass::Diagnostics];
        assert_eq!(
            result.validate_authority_against(&unsupported),
            Err(AdapterAuthorityError::UnsupportedFactClass)
        );

        let mut dependency = input();
        dependency.invalidation_inputs = vec![InvalidationDependency::new(
            "module:Moo",
            SourceGeneration::known("generation-1"),
        )];
        assert_eq!(
            result.validate_authority_against(&dependency),
            Err(AdapterAuthorityError::InvalidationMismatch)
        );

        if let AdapterOutcome::Applied { sink, .. } = &mut result.outcome {
            sink.total_payload_bytes = 0;
        }
        assert_eq!(
            result.validate_authority_against(&input()),
            Err(AdapterAuthorityError::PayloadMismatch)
        );
    }

    #[test]
    fn authority_rejects_package_confidence_and_duplicate_identity_forgery() {
        let mut fact = EmittedFact::new(
            FactSinkId(7),
            AdapterId(1),
            "Moo",
            Provenance::FrameworkSynthesis,
            Confidence::High,
            envelope(Provenance::FrameworkSynthesis),
            FactClass::GeneratedMembers,
            Some(FactLimitation::new("partial", false, Confidence::Low)),
            false,
        );
        let mut package_result = applied(AdapterDisposition::Production, fact.clone());
        if let AdapterOutcome::Applied { sink, .. } = &mut package_result.outcome {
            sink.facts[0].envelope.package = Some("Other".to_string());
            if let Some(bytes) = sink.serialized_payload_bytes() {
                sink.total_payload_bytes = bytes;
            }
        }
        assert_eq!(
            package_result.validate_authority_against(&input()),
            Err(AdapterAuthorityError::PackageIdentityMismatch)
        );

        let confidence_result = applied(AdapterDisposition::Production, fact.clone());
        assert_eq!(
            confidence_result.validate_authority_against(&input()),
            Err(AdapterAuthorityError::ConfidenceLimitExceeded)
        );

        fact.limitation = None;
        let mut duplicate_result = applied(AdapterDisposition::Production, fact.clone());
        if let AdapterOutcome::Applied { sink, .. } = &mut duplicate_result.outcome {
            sink.facts.push(fact);
            if let Some(bytes) = sink.serialized_payload_bytes() {
                sink.total_payload_bytes = bytes;
            }
        }
        // The shared `input()` budget caps emitted facts at 1, so a second fact
        // trips `BudgetExceeded` before the duplicate scan is ever reached. Widen
        // the budget here so this assertion actually exercises duplicate-fact-id
        // detection; `authority_rejects_budget_class_dependency_and_payload_forgery`
        // already covers the budget path on its own.
        let mut duplicate_input = input();
        duplicate_input.budget = Some(AdapterBudget::new(2, 4096));
        assert_eq!(
            duplicate_result.validate_authority_against(&duplicate_input),
            Err(AdapterAuthorityError::DuplicateFactId)
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
        assert_eq!(
            applied(AdapterDisposition::Production, fact).validate_authority_against(&input()),
            Err(AdapterAuthorityError::InvalidFact)
        );
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
        assert!(applied(AdapterDisposition::Production, fact).is_authoritative_against(&input()));
    }

    #[test]
    fn future_enum_variant_requires_a_schema_bump() {
        let future = r#"{"FutureVariant":{"payload":1}}"#;
        assert!(serde_json::from_str::<DetectionOutcome>(future).is_err());
    }
}
