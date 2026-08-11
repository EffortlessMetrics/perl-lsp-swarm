//! Framework adapter SDK types.
//!
//! Defines the low-level vocabulary for the `FrameworkAdapter` interface
//! without implementing any production adapter, registry dispatch, or
//! `ProjectModel` publication.
//!
//! # Dependency boundary
//!
//! These types depend only on `serde` and the existing vocabulary in this
//! crate — [`Confidence`], [`Provenance`], [`SourceGeneration`],
//! [`InvalidationDependency`], [`SemanticFactEnvelope`], and the ID
//! newtypes. LSP, VS Code, DAP, and provider presentation types must never
//! be imported here.
//!
//! # Versioning
//!
//! Every wire/persisted structure carries an explicit schema version.
//! The current top-level version constant is
//! [`FRAMEWORK_ADAPTER_SDK_VERSION`]. Consumers must tolerate additive
//! changes (unknown `serde(default)` fields) from newer schema versions.
//!
//! # What this module does **not** do
//!
//! - No registry or dispatch.
//! - No `ProjectModel` publication.
//! - No production framework adapter (Exporter, Moo, Moose, …).
//! - No provider shadowing or cutover.
//! - No arbitrary framework execution.
//!
//! [`Confidence`]: crate::Confidence
//! [`Provenance`]: crate::Provenance
//! [`SourceGeneration`]: crate::SourceGeneration
//! [`InvalidationDependency`]: crate::InvalidationDependency
//! [`SemanticFactEnvelope`]: crate::SemanticFactEnvelope

use serde::{Deserialize, Serialize};

use crate::{
    AnchorId, Confidence, FileId, InvalidationDependency, Provenance, SemanticFactEnvelope,
    SourceGeneration,
};

/// Current additive schema version for all framework adapter SDK types.
///
/// Consumers should record this string alongside serialized adapter results
/// so that forward-incompatible schema changes can be detected. Unknown fields
/// added in future minor versions must be tolerated via `#[serde(default)]`.
pub const FRAMEWORK_ADAPTER_SDK_VERSION: &str = "framework_adapter_sdk.v1";

// ── ID newtypes ────────────────────────────────────────────────────────────

/// Stable opaque identity for a registered adapter.
///
/// `AdapterId` is not a semantic entity ID — it must not be used as a key
/// into the workspace entity graph. Adapters may derive their own ID
/// deterministically from their name and schema version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct AdapterId(pub u64);

/// Opaque identity for a [`FactSink`] produced in one adapter invocation.
///
/// Scoped to the current server instance; all prior identities are invalid
/// after a server restart. Must not be exposed to untrusted client input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct FactSinkId(pub u64);

// ── Adapter descriptor ─────────────────────────────────────────────────────

/// Deployment disposition for an adapter.
///
/// Controls whether the adapter's output is authoritative, shadowed for
/// comparison only, or experimental (subject to removal without notice).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum AdapterDisposition {
    /// Output is authoritative and visible to downstream providers.
    Production,
    /// Output is produced and compared against existing results but not
    /// delivered — used for incremental rollout and A/B evaluation.
    Shadow,
    /// Output may be incorrect or incomplete; only enabled in explicit
    /// experimental configurations.
    Experimental,
}

/// Versioned self-description of one registered adapter.
///
/// An `AdapterDescriptor` is stable across invocations and safe to
/// serialize across process boundaries. It carries no live mutable state.
///
/// The `schema_version` field tracks the version of the *descriptor format*
/// itself, independent of the adapter SDK version constant.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterDescriptor {
    /// Stable opaque identity for this adapter.
    pub adapter_id: AdapterId,
    /// Human-readable adapter name (e.g. `"moo"`, `"moose"`, `"exporter"`).
    pub name: String,
    /// Framework or module family this adapter handles (e.g. `"Moo"`).
    pub framework_name: String,
    /// Optional semver-style version constraint for the target framework
    /// (e.g. `">=2.0,<3.0"`). `None` means any version is accepted.
    pub framework_version_constraint: Option<String>,
    /// Schema version of this descriptor record. Currently `1`.
    pub schema_version: u32,
    /// Deployment disposition for this adapter instance.
    pub disposition: AdapterDisposition,
}

impl AdapterDescriptor {
    /// Construct a new `AdapterDescriptor`.
    ///
    /// Required because the struct is `#[non_exhaustive]`.
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

// ── Detection ──────────────────────────────────────────────────────────────

/// Identity of one module available in the current project for detection.
///
/// Adapters use this to decide if their target framework is present without
/// reading the file system directly or invoking dynamic module resolution.
/// No path or source-text content is carried — only stable IDs and generations.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ModuleActivationIdentity {
    /// Fully qualified package or module name (e.g. `"Moo"`, `"Moose::Role"`).
    pub module_name: String,
    /// Optional source file identity for this module within the current workspace.
    pub file_id: Option<FileId>,
    /// Source generation snapshot for this module's source bytes.
    pub generation: SourceGeneration,
}

impl ModuleActivationIdentity {
    /// Construct a module activation identity.
    pub fn new(
        module_name: impl Into<String>,
        file_id: Option<FileId>,
        generation: SourceGeneration,
    ) -> Self {
        Self { module_name: module_name.into(), file_id, generation }
    }
}

/// Cancellation token for adapter detection and invocation operations.
///
/// Adapters must poll `is_cancelled` at natural checkpoints and return a
/// `Cancelled` outcome immediately — without completing partial work — when
/// the token is set.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterCancellation {
    /// `true` when the caller has requested cancellation.
    pub is_cancelled: bool,
}

impl AdapterCancellation {
    /// Construct a token representing an active (non-cancelled) operation.
    #[must_use]
    pub const fn active() -> Self {
        Self { is_cancelled: false }
    }

    /// Construct a pre-cancelled token.
    #[must_use]
    pub const fn cancelled() -> Self {
        Self { is_cancelled: true }
    }
}

/// Resource budget for one adapter detection or invocation operation.
///
/// Limits are enforced by the adapter itself. Results that exceed the budget
/// must return a `BudgetExhausted` outcome with any partial facts collected.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterBudget {
    /// Maximum number of [`EmittedFact`]s the adapter may produce.
    pub max_emitted_facts: u32,
    /// Maximum total payload bytes across all serialized emitted facts.
    pub max_payload_bytes: u64,
}

impl AdapterBudget {
    /// Construct a budget constraint.
    #[must_use]
    pub const fn new(max_emitted_facts: u32, max_payload_bytes: u64) -> Self {
        Self { max_emitted_facts, max_payload_bytes }
    }
}

/// Input to the framework detection pass.
///
/// Contains only the information needed to decide whether the target framework
/// is present in the current project. No private paths, no source text, and
/// no workspace write access.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterDetectionInput {
    /// Descriptor of the adapter being queried.
    pub descriptor: AdapterDescriptor,
    /// Modules visible in the current project index.
    pub available_modules: Vec<ModuleActivationIdentity>,
    /// Source generation for the whole project snapshot.
    pub project_generation: SourceGeneration,
    /// Optional content digest over the module activation list
    /// (for cache invalidation — not source text).
    pub content_digest: Option<String>,
    /// Optional resource budget for this detection call.
    pub budget: Option<AdapterBudget>,
    /// Cancellation token.
    pub cancellation: AdapterCancellation,
}

impl AdapterDetectionInput {
    /// Construct a detection input.
    ///
    /// Required because the struct is `#[non_exhaustive]`.
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
}

/// Structured reason a required framework was found to be absent.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DetectionAbsenceReason {
    /// None of the activation modules required by this adapter were present.
    RequiredModulesMissing,
    /// An activation module was present but its version did not satisfy the
    /// adapter's [`AdapterDescriptor::framework_version_constraint`].
    VersionConstraintNotSatisfied,
    /// The framework is explicitly excluded by project configuration.
    ExcludedByConfiguration,
}

/// Reason an adapter could not proceed with detection or invocation.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnavailableReason {
    /// The project source generation was not available (unknown or empty).
    MissingGeneration,
    /// The module activation list provided to the adapter was empty.
    NoModulesAvailable,
    /// An adapter-internal invariant was violated.
    InternalError,
}

/// Concrete outcome of one framework detection pass.
///
/// All outcome variants are `#[non_exhaustive]` because new discriminants
/// may be added without a major version bump. Deserializing an unknown
/// variant must be handled gracefully by callers.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DetectionOutcome {
    /// The framework was definitively detected.
    Detected {
        /// Confidence in the detection result.
        confidence: Confidence,
        /// Detected framework version string, when identifiable.
        framework_version: Option<String>,
    },
    /// The framework is definitively absent in the current project.
    Absent {
        /// Structured reason explaining why the framework was not found.
        reason: DetectionAbsenceReason,
    },
    /// Framework presence is ambiguous due to conflicting activation signals.
    Conflicting {
        /// Human-readable description of each detected conflict.
        conflict_descriptions: Vec<String>,
    },
    /// Detection could not complete due to a structural problem.
    Unavailable {
        /// Reason the detection operation could not proceed.
        reason: UnavailableReason,
    },
    /// Detection was cancelled before completing.
    Cancelled,
    /// The resource budget was exhausted before detection could complete.
    BudgetExhausted,
    /// This adapter does not support detecting the current framework version
    /// or configuration.
    Unsupported {
        /// Human-readable explanation of why this adapter cannot help.
        reason: String,
    },
}

/// Full result of one framework detection pass.
///
/// Carries the originating descriptor and project generation so consumers
/// can verify the result is still authoritative against the current workspace.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterDetectionResult {
    /// Descriptor of the adapter that performed detection.
    pub descriptor: AdapterDescriptor,
    /// Project generation in effect during detection.
    pub project_generation: SourceGeneration,
    /// Concrete outcome of the detection pass.
    pub outcome: DetectionOutcome,
}

impl AdapterDetectionResult {
    /// Construct a detection result.
    ///
    /// Required because the struct is `#[non_exhaustive]`.
    pub fn new(
        descriptor: AdapterDescriptor,
        project_generation: SourceGeneration,
        outcome: DetectionOutcome,
    ) -> Self {
        Self { descriptor, project_generation, outcome }
    }

    /// Whether this result indicates definite framework presence.
    ///
    /// Returns `false` for all outcomes except [`DetectionOutcome::Detected`].
    #[must_use]
    pub fn is_detected(&self) -> bool {
        matches!(self.outcome, DetectionOutcome::Detected { .. })
    }

    /// Whether the detection result is authoritative for the current
    /// project generation (i.e. not stale, partial, or unknown).
    #[must_use]
    pub fn is_authoritative(&self) -> bool {
        self.project_generation.is_known()
            && matches!(
                self.outcome,
                DetectionOutcome::Detected { .. } | DetectionOutcome::Absent { .. }
            )
    }
}

// ── Adapter input (invocation) ─────────────────────────────────────────────

/// Class of semantic facts an adapter may emit.
///
/// Adapters declare which classes they produce; the runtime may filter or
/// skip adapters whose output classes are irrelevant for the current query.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum FactClass {
    /// Framework-synthesized entity members (e.g. Moo/Moose accessors from `has`).
    GeneratedMembers,
    /// Package inheritance and role-composition relationships.
    PackageGraph,
    /// Framework-specific import and export facts.
    FrameworkImports,
    /// Diagnostic facts describing framework usage errors.
    Diagnostics,
    /// Catch-all for fact classes introduced by future adapter families.
    Extension,
}

/// Source scope presented to an adapter during invocation.
///
/// The adapter may only inspect facts about entities within this scope.
/// Private paths, external filesystem access, and unbounded workspace reads
/// are not permitted — the scope carries only stable IDs and generation tokens.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterSourceScope {
    /// Primary file being analysed.
    pub primary_file_id: FileId,
    /// Source generation snapshot for the primary file.
    pub primary_generation: SourceGeneration,
    /// Content digest for the primary file (for invalidation — not source text).
    pub primary_content_digest: Option<String>,
    /// Anchor of the framework activation statement that scopes this run
    /// (e.g. the `use Moo;` statement in the target file).
    pub activation_anchor_id: Option<AnchorId>,
    /// Package or class name being analysed.
    pub package_name: Option<String>,
}

impl AdapterSourceScope {
    /// Construct an adapter source scope.
    ///
    /// Required because the struct is `#[non_exhaustive]`.
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

/// Full input to one adapter invocation.
///
/// The adapter must complete within the given budget and respect the
/// cancellation token. Private paths and source text must not be read
/// beyond the scope identified by `source_scope`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterInput {
    /// Descriptor of the adapter to invoke.
    pub descriptor: AdapterDescriptor,
    /// Source scope the adapter may inspect.
    pub source_scope: AdapterSourceScope,
    /// Fact classes the caller requires from this invocation.
    pub required_fact_classes: Vec<FactClass>,
    /// Source identities that, if changed, would invalidate this result.
    pub invalidation_inputs: Vec<InvalidationDependency>,
    /// Optional resource budget.
    pub budget: Option<AdapterBudget>,
    /// Cancellation token.
    pub cancellation: AdapterCancellation,
}

impl AdapterInput {
    /// Construct an adapter input.
    ///
    /// Required because the struct is `#[non_exhaustive]`.
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

// ── FactSink and emitted facts ─────────────────────────────────────────────

/// Reason a generated fact is limited, bounded, or incomplete.
///
/// Carried alongside an [`EmittedFact`] to explain why the fact may need
/// to be treated with reduced confidence or refused for certain operations.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactLimitation {
    /// Human-readable description of the limitation.
    pub description: String,
    /// Whether this limitation prevents the fact from being used at all.
    ///
    /// When `true`, downstream consumers should treat the fact as refused.
    pub is_blocking: bool,
    /// Confidence impact — the confidence of the fact should not exceed this.
    pub confidence_impact: Confidence,
}

impl FactLimitation {
    /// Construct a fact limitation.
    pub fn new(
        description: impl Into<String>,
        is_blocking: bool,
        confidence_impact: Confidence,
    ) -> Self {
        Self { description: description.into(), is_blocking, confidence_impact }
    }
}

/// One semantic fact emitted by an adapter, carrying full provenance metadata.
///
/// Wraps a [`SemanticFactEnvelope`] with adapter identity and limitation
/// context. The `is_stronger_than_generated` flag allows explicit source
/// declarations (e.g. a literal `has` declaration with a `reader` override)
/// to take precedence over synthesised equivalents.
///
/// Generated facts must use canonical project/semantic identities from the
/// existing vocabulary in [`crate`]. Provider-specific output structures are
/// structurally impossible through this interface.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmittedFact {
    /// Sink this fact was emitted into.
    pub sink_id: FactSinkId,
    /// Adapter that produced this fact.
    pub adapter_id: AdapterId,
    /// Framework name the producing adapter handles.
    pub framework_name: String,
    /// Provenance of this specific fact.
    ///
    /// Must be [`Provenance::FrameworkSynthesis`] for generated members.
    pub provenance: Provenance,
    /// Confidence in this fact before any limitation adjustment.
    pub confidence: Confidence,
    /// Canonical semantic fact transport record.
    pub envelope: SemanticFactEnvelope,
    /// Class of this emitted fact.
    pub fact_class: FactClass,
    /// Optional limitation describing why this fact may be incomplete.
    pub limitation: Option<FactLimitation>,
    /// Whether this fact represents an explicit source declaration that is
    /// stronger than a synthesised/generated equivalent.
    ///
    /// When `true`, a conflicting synthesised fact should be discarded in
    /// favour of this one during conflict resolution.
    pub is_stronger_than_generated: bool,
}

impl EmittedFact {
    /// Construct an emitted fact.
    ///
    /// Required because the struct is `#[non_exhaustive]`.
    #[allow(clippy::too_many_arguments)] // mirrors the struct fields 1-to-1
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
        self.limitation.as_ref().is_some_and(|l| l.is_blocking)
    }
}

/// Bounded collection of facts produced by one adapter invocation.
///
/// Facts are ordered by emission sequence, preserving adapter-defined priority
/// for conflict resolution. The sink is not persisted beyond the current
/// server-instance invocation; all facts are discarded on server restart.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactSink {
    /// Stable identity of this sink within the current server instance.
    pub sink_id: FactSinkId,
    /// Adapter that produced this sink.
    pub adapter_id: AdapterId,
    /// Ordered sequence of emitted facts.
    pub facts: Vec<EmittedFact>,
    /// Total serialized payload bytes emitted, for budget accounting.
    pub total_payload_bytes: u64,
}

impl FactSink {
    /// Construct an empty sink.
    #[must_use]
    pub const fn new(sink_id: FactSinkId, adapter_id: AdapterId) -> Self {
        Self { sink_id, adapter_id, facts: Vec::new(), total_payload_bytes: 0 }
    }

    /// Whether the sink carries any facts.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.facts.is_empty()
    }

    /// Count of emitted facts.
    #[must_use]
    pub fn len(&self) -> usize {
        self.facts.len()
    }

    /// Iterate over facts that have a blocking limitation.
    pub fn blocking_limited_facts(&self) -> impl Iterator<Item = &EmittedFact> {
        self.facts.iter().filter(|f| f.is_blocked())
    }

    /// Iterate over usable facts (no blocking limitation).
    pub fn usable_facts(&self) -> impl Iterator<Item = &EmittedFact> {
        self.facts.iter().filter(|f| !f.is_blocked())
    }
}

// ── Adapter result ─────────────────────────────────────────────────────────

/// Concrete outcome of one adapter invocation.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdapterOutcome {
    /// The adapter completed successfully and produced facts.
    Applied {
        /// All facts produced by this invocation.
        sink: FactSink,
        /// Non-blocking limitations on the produced result.
        limitations: Vec<FactLimitation>,
    },
    /// The result is bounded by a dynamic Perl feature the adapter cannot model.
    ///
    /// Equivalent to a `DynamicBoundary` — the adapter ran but its output is
    /// incomplete because a runtime value would be required.
    Dynamic {
        /// Human-readable reason for the dynamic boundary.
        reason: String,
        /// Partial facts collected before the dynamic boundary was reached.
        partial_sink: Option<FactSink>,
    },
    /// This adapter does not support the current framework version or
    /// configuration.
    Unsupported {
        /// Human-readable explanation.
        reason: String,
    },
    /// Conflicting framework signals prevent the adapter from producing a
    /// coherent result.
    Conflict {
        /// Human-readable description of each detected conflict.
        conflict_descriptions: Vec<String>,
    },
    /// The resource budget was exhausted before the adapter completed.
    BudgetExhausted {
        /// Partial facts collected before the budget was reached.
        partial_sink: Option<FactSink>,
    },
    /// The operation was cancelled before completing.
    Cancelled,
}

/// Full result of one adapter invocation.
///
/// Carries the originating descriptor and source generation so consumers
/// can verify the result is still authoritative against the current workspace.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterResult {
    /// Schema version of this result record. Currently `1`.
    pub schema_version: u32,
    /// Descriptor of the adapter that ran.
    pub descriptor: AdapterDescriptor,
    /// Source scope used during this invocation.
    pub source_scope: AdapterSourceScope,
    /// Generation in effect at invocation time.
    pub invocation_generation: SourceGeneration,
    /// Concrete outcome of the invocation.
    pub outcome: AdapterOutcome,
}

impl AdapterResult {
    /// Construct an adapter result with the current schema version (`1`).
    ///
    /// Required because the struct is `#[non_exhaustive]`.
    pub fn new(
        descriptor: AdapterDescriptor,
        source_scope: AdapterSourceScope,
        invocation_generation: SourceGeneration,
        outcome: AdapterOutcome,
    ) -> Self {
        Self { schema_version: 1, descriptor, source_scope, invocation_generation, outcome }
    }

    /// Whether the adapter produced at least one fact.
    #[must_use]
    pub fn has_facts(&self) -> bool {
        match &self.outcome {
            AdapterOutcome::Applied { sink, .. } => !sink.is_empty(),
            AdapterOutcome::Dynamic { partial_sink, .. } => {
                partial_sink.as_ref().is_some_and(|s| !s.is_empty())
            }
            AdapterOutcome::BudgetExhausted { partial_sink } => {
                partial_sink.as_ref().is_some_and(|s| !s.is_empty())
            }
            _ => false,
        }
    }

    /// Whether the result is fully authoritative (not a fallback or partial).
    ///
    /// Returns `true` only for [`AdapterOutcome::Applied`] with a known
    /// invocation generation.
    #[must_use]
    pub fn is_authoritative(&self) -> bool {
        self.invocation_generation.is_known()
            && matches!(self.outcome, AdapterOutcome::Applied { .. })
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::{
        AnchorId, EntityId, FactId, LifecyclePhase, SemanticConfidence, SemanticFactEnvelope,
        SemanticFactKind, SemanticFreshness, SemanticProducer, SemanticProvenance,
        SemanticReasonCode, SourceAnchor,
    };

    fn sample_descriptor() -> AdapterDescriptor {
        AdapterDescriptor::new(
            AdapterId(1),
            "moo-test",
            "Moo",
            None,
            1,
            AdapterDisposition::Production,
        )
    }

    fn sample_scope() -> AdapterSourceScope {
        AdapterSourceScope::new(
            FileId(10),
            SourceGeneration::known("sha256:aabbcc"),
            Some("digest:aabbcc".to_string()),
            Some(AnchorId(5)),
            Some("My::Package".to_string()),
        )
    }

    fn sample_envelope(file_id: FileId) -> SemanticFactEnvelope {
        SemanticFactEnvelope::new(
            FactId(1),
            Some(EntityId(2)),
            SemanticFactKind::Declaration,
            SourceAnchor::new(Some(AnchorId(3)), file_id, 10, 20),
            SourceGeneration::known("sha256:aabbcc"),
            None,
            Some("My::Package".to_string()),
            LifecyclePhase::Runtime,
            SemanticProducer::FrameworkAdapter,
            SemanticProvenance::Known(Provenance::FrameworkSynthesis),
            SemanticConfidence::Known(Confidence::Medium),
            SemanticFreshness::Fresh,
            None,
            vec![],
            SemanticReasonCode::GeneratedFromSource,
        )
    }

    #[test]
    fn adapter_descriptor_roundtrips_through_json() {
        let descriptor = sample_descriptor();
        let json = serde_json::to_string(&descriptor).expect("serialize descriptor");
        let decoded: AdapterDescriptor =
            serde_json::from_str(&json).expect("deserialize descriptor");
        assert_eq!(decoded, descriptor);
    }

    #[test]
    fn detection_result_detected_roundtrips() {
        let result = AdapterDetectionResult::new(
            sample_descriptor(),
            SourceGeneration::known("sha256:aabbcc"),
            DetectionOutcome::Detected {
                confidence: Confidence::High,
                framework_version: Some("2.5.0".to_string()),
            },
        );
        let json = serde_json::to_string(&result).expect("serialize");
        let decoded: AdapterDetectionResult = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, result);
        assert!(result.is_detected());
        assert!(result.is_authoritative());
    }

    #[test]
    fn detection_result_absent_is_not_detected() {
        let result = AdapterDetectionResult::new(
            sample_descriptor(),
            SourceGeneration::known("sha256:aabbcc"),
            DetectionOutcome::Absent { reason: DetectionAbsenceReason::RequiredModulesMissing },
        );
        assert!(!result.is_detected());
        assert!(result.is_authoritative());
    }

    #[test]
    fn detection_result_with_unknown_generation_is_not_authoritative() {
        let result = AdapterDetectionResult::new(
            sample_descriptor(),
            SourceGeneration::Unknown,
            DetectionOutcome::Detected { confidence: Confidence::High, framework_version: None },
        );
        assert!(result.is_detected());
        assert!(!result.is_authoritative(), "unknown generation must not be authoritative");
    }

    #[test]
    fn detection_conflicting_roundtrips() {
        let result = AdapterDetectionResult::new(
            sample_descriptor(),
            SourceGeneration::known("sha256:cc"),
            DetectionOutcome::Conflicting {
                conflict_descriptions: vec![
                    "both Moo and Moose activated".to_string(),
                    "version constraint violated".to_string(),
                ],
            },
        );
        let json = serde_json::to_string(&result).expect("serialize");
        let decoded: AdapterDetectionResult = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, result);
        assert!(!result.is_detected());
    }

    #[test]
    fn detection_budget_exhausted_roundtrips() {
        let result = AdapterDetectionResult::new(
            sample_descriptor(),
            SourceGeneration::known("sha256:cc"),
            DetectionOutcome::BudgetExhausted,
        );
        let json = serde_json::to_string(&result).expect("serialize");
        let decoded: AdapterDetectionResult = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, result);
        assert!(!result.is_detected());
        assert!(!result.is_authoritative());
    }

    #[test]
    fn detection_unsupported_roundtrips() {
        let result = AdapterDetectionResult::new(
            sample_descriptor(),
            SourceGeneration::known("sha256:cc"),
            DetectionOutcome::Unsupported { reason: "Moo v1 not supported".to_string() },
        );
        let json = serde_json::to_string(&result).expect("serialize");
        let decoded: AdapterDetectionResult = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, result);
    }

    #[test]
    fn cancellation_token_semantics() {
        let active = AdapterCancellation::active();
        assert!(!active.is_cancelled);
        let cancelled = AdapterCancellation::cancelled();
        assert!(cancelled.is_cancelled);
    }

    #[test]
    fn adapter_budget_roundtrips() {
        let budget = AdapterBudget::new(100, 1_024 * 1_024);
        let json = serde_json::to_string(&budget).expect("serialize");
        let decoded: AdapterBudget = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, budget);
    }

    #[test]
    fn adapter_input_roundtrips() {
        let input = AdapterInput::new(
            sample_descriptor(),
            sample_scope(),
            vec![FactClass::GeneratedMembers, FactClass::PackageGraph],
            vec![InvalidationDependency::new(
                "file:My/Package.pm",
                SourceGeneration::known("sha256:aabbcc"),
            )],
            Some(AdapterBudget::new(50, 512_000)),
            AdapterCancellation::active(),
        );
        let json = serde_json::to_string(&input).expect("serialize");
        let decoded: AdapterInput = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, input);
    }

    #[test]
    fn fact_sink_is_empty_on_construction() {
        let sink = FactSink::new(FactSinkId(1), AdapterId(1));
        assert!(sink.is_empty());
        assert_eq!(sink.len(), 0);
        assert_eq!(sink.blocking_limited_facts().count(), 0);
        assert_eq!(sink.usable_facts().count(), 0);
    }

    #[test]
    fn emitted_fact_blocking_limitation_is_detectable() {
        let envelope = sample_envelope(FileId(10));
        let fact = EmittedFact::new(
            FactSinkId(1),
            AdapterId(1),
            "Moo",
            Provenance::FrameworkSynthesis,
            Confidence::Medium,
            envelope,
            FactClass::GeneratedMembers,
            Some(FactLimitation::new("dynamic has body", true, Confidence::Low)),
            false,
        );
        assert!(fact.is_blocked());
    }

    #[test]
    fn emitted_fact_without_limitation_is_usable() {
        let envelope = sample_envelope(FileId(10));
        let fact = EmittedFact::new(
            FactSinkId(1),
            AdapterId(1),
            "Moo",
            Provenance::FrameworkSynthesis,
            Confidence::Medium,
            envelope,
            FactClass::GeneratedMembers,
            None,
            false,
        );
        assert!(!fact.is_blocked());
    }

    #[test]
    fn stronger_than_generated_flag_preserved() {
        let envelope = sample_envelope(FileId(10));
        let fact = EmittedFact::new(
            FactSinkId(1),
            AdapterId(1),
            "Moo",
            Provenance::FrameworkSynthesis,
            Confidence::High,
            envelope,
            FactClass::GeneratedMembers,
            None,
            true, // explicit source declaration — stronger than synthesised
        );
        assert!(fact.is_stronger_than_generated);
        let json = serde_json::to_string(&fact).expect("serialize");
        let decoded: EmittedFact = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, fact);
    }

    #[test]
    fn adapter_result_applied_is_authoritative() {
        let mut sink = FactSink::new(FactSinkId(1), AdapterId(1));
        let envelope = sample_envelope(FileId(10));
        sink.facts.push(EmittedFact::new(
            FactSinkId(1),
            AdapterId(1),
            "Moo",
            Provenance::FrameworkSynthesis,
            Confidence::Medium,
            envelope,
            FactClass::GeneratedMembers,
            None,
            false,
        ));
        sink.total_payload_bytes = 512;
        let result = AdapterResult::new(
            sample_descriptor(),
            sample_scope(),
            SourceGeneration::known("sha256:aabbcc"),
            AdapterOutcome::Applied { sink, limitations: Vec::new() },
        );
        assert!(result.is_authoritative());
        assert!(result.has_facts());
        assert_eq!(result.schema_version, 1);
    }

    #[test]
    fn adapter_result_dynamic_boundary_is_not_authoritative() {
        let result = AdapterResult::new(
            sample_descriptor(),
            sample_scope(),
            SourceGeneration::known("sha256:aabbcc"),
            AdapterOutcome::Dynamic {
                reason: "has body uses a runtime variable".to_string(),
                partial_sink: None,
            },
        );
        assert!(!result.is_authoritative());
        assert!(!result.has_facts());
    }

    #[test]
    fn adapter_result_budget_exhausted_with_partial_sink() {
        let mut partial = FactSink::new(FactSinkId(2), AdapterId(1));
        let envelope = sample_envelope(FileId(10));
        partial.facts.push(EmittedFact::new(
            FactSinkId(2),
            AdapterId(1),
            "Moo",
            Provenance::FrameworkSynthesis,
            Confidence::Low,
            envelope,
            FactClass::GeneratedMembers,
            None,
            false,
        ));
        let result = AdapterResult::new(
            sample_descriptor(),
            sample_scope(),
            SourceGeneration::known("sha256:aabbcc"),
            AdapterOutcome::BudgetExhausted { partial_sink: Some(partial) },
        );
        assert!(!result.is_authoritative());
        assert!(result.has_facts(), "partial sink facts should be reported");
    }

    #[test]
    fn adapter_result_cancelled_has_no_facts() {
        let result = AdapterResult::new(
            sample_descriptor(),
            sample_scope(),
            SourceGeneration::known("sha256:aabbcc"),
            AdapterOutcome::Cancelled,
        );
        assert!(!result.is_authoritative());
        assert!(!result.has_facts());
    }

    #[test]
    fn adapter_result_conflict_roundtrips() {
        let result = AdapterResult::new(
            sample_descriptor(),
            sample_scope(),
            SourceGeneration::known("sha256:aabbcc"),
            AdapterOutcome::Conflict {
                conflict_descriptions: vec!["Moo and Moose co-activated".to_string()],
            },
        );
        let json = serde_json::to_string(&result).expect("serialize");
        let decoded: AdapterResult = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, result);
        assert!(!result.is_authoritative());
    }

    #[test]
    fn adapter_result_unknown_generation_not_authoritative() {
        let sink = FactSink::new(FactSinkId(1), AdapterId(1));
        let result = AdapterResult::new(
            sample_descriptor(),
            sample_scope(),
            SourceGeneration::Unknown,
            AdapterOutcome::Applied { sink, limitations: Vec::new() },
        );
        assert!(
            !result.is_authoritative(),
            "unknown generation must not be authoritative even for Applied"
        );
    }

    #[test]
    fn module_activation_identity_roundtrips() {
        let identity = ModuleActivationIdentity::new(
            "Moo",
            Some(FileId(42)),
            SourceGeneration::known("sha256:deadbeef"),
        );
        let json = serde_json::to_string(&identity).expect("serialize");
        let decoded: ModuleActivationIdentity = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, identity);
    }

    #[test]
    fn detection_input_roundtrips() {
        let input = AdapterDetectionInput::new(
            sample_descriptor(),
            vec![
                ModuleActivationIdentity::new(
                    "Moo",
                    Some(FileId(1)),
                    SourceGeneration::known("sha256:aa"),
                ),
                ModuleActivationIdentity::new(
                    "Moo::Role",
                    None,
                    SourceGeneration::known("sha256:bb"),
                ),
            ],
            SourceGeneration::known("sha256:project"),
            Some("digest:project".to_string()),
            Some(AdapterBudget::new(10, 65536)),
            AdapterCancellation::active(),
        );
        let json = serde_json::to_string(&input).expect("serialize");
        let decoded: AdapterDetectionInput = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, input);
    }

    /// Verify that a minimal test-only adapter can compile against the interface
    /// without going through a registry. This is the compile-contract test
    /// from acceptance criteria §8 of issue #6820.
    #[test]
    fn minimal_test_adapter_compiles_against_interface() {
        fn run_detection(input: &AdapterDetectionInput) -> AdapterDetectionResult {
            // A test adapter that unconditionally reports the framework absent.
            AdapterDetectionResult::new(
                input.descriptor.clone(),
                input.project_generation.clone(),
                DetectionOutcome::Absent { reason: DetectionAbsenceReason::RequiredModulesMissing },
            )
        }

        fn run_adapter(input: &AdapterInput) -> AdapterResult {
            // A test adapter that emits an empty Applied result.
            let sink = FactSink::new(FactSinkId(99), input.descriptor.adapter_id);
            AdapterResult::new(
                input.descriptor.clone(),
                input.source_scope.clone(),
                input.source_scope.primary_generation.clone(),
                AdapterOutcome::Applied { sink, limitations: Vec::new() },
            )
        }

        let detection_input = AdapterDetectionInput::new(
            sample_descriptor(),
            vec![],
            SourceGeneration::known("sha256:test"),
            None,
            None,
            AdapterCancellation::active(),
        );
        let detection_result = run_detection(&detection_input);
        assert!(!detection_result.is_detected());

        let adapter_input = AdapterInput::new(
            sample_descriptor(),
            sample_scope(),
            vec![FactClass::GeneratedMembers],
            vec![],
            None,
            AdapterCancellation::active(),
        );
        let adapter_result = run_adapter(&adapter_input);
        // Empty Applied sink: has no facts but is authoritative when generation is known.
        assert!(adapter_result.is_authoritative());
        assert!(!adapter_result.has_facts());
    }

    #[test]
    fn sdk_version_constant_is_stable() {
        assert_eq!(FRAMEWORK_ADAPTER_SDK_VERSION, "framework_adapter_sdk.v1");
    }

    #[test]
    fn adapter_disposition_variants_roundtrip() {
        for disposition in [
            AdapterDisposition::Production,
            AdapterDisposition::Shadow,
            AdapterDisposition::Experimental,
        ] {
            let json = serde_json::to_string(&disposition).expect("serialize");
            let decoded: AdapterDisposition = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(decoded, disposition);
        }
    }

    #[test]
    fn fact_class_variants_roundtrip() {
        for class in [
            FactClass::GeneratedMembers,
            FactClass::PackageGraph,
            FactClass::FrameworkImports,
            FactClass::Diagnostics,
            FactClass::Extension,
        ] {
            let json = serde_json::to_string(&class).expect("serialize");
            let decoded: FactClass = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(decoded, class);
        }
    }
}
