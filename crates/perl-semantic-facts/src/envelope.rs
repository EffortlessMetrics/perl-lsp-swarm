//! Canonical transport envelope for compiler/workspace semantic facts.
//!
//! The envelope is deliberately independent of AST, HIR, and PIR types. Producers
//! adapt their local facts into this contract; providers can then decide whether a
//! fact is safe to use without reopening compiler internals.

use super::{AnchorId, Confidence, EntityId, FileId, Provenance, ScopeId, ValueShape};
use serde::{Deserialize, Deserializer, Serialize};

/// Stable identity for one promoted semantic fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct FactId(pub u64);

/// Kind of semantic fact carried by an envelope.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SemanticFactKind {
    /// A declaration fact (package/sub/variable definition).
    Declaration,
    /// An occurrence fact (reference/read/write/call site).
    Occurrence,
    /// An import fact.
    Import,
    /// A module-level fact.
    Module,
    /// A dynamic or compatibility boundary fact.
    Boundary,
    /// Callable return relation and exit coverage.
    CallableResult,
    /// Framework route declaration (canonical route fact family).
    Route,
    /// Framework route prefix declaration (canonical route fact family).
    RoutePrefix,
    /// Route-local parameter/capture segment of one route pattern (canonical
    /// route fact family).
    RouteParameter,
    /// Route-handler source interval where route-handler-only DSL keywords
    /// are semantically available (canonical route fact family).
    RouteHandlerContext,
    /// Framework hook declaration (canonical hook fact family).
    Hook,
}

/// Source identity for a fact's bytes or compiler input snapshot.
///
/// An unknown generation is explicit. It must never be treated as an exact
/// source-backed answer by [`SemanticFactEnvelope::status`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SourceGeneration {
    /// The producer identified the exact source snapshot.
    Known(String),
    /// The generation is unknown; never treat the fact as exact.
    Unknown,
}

impl SourceGeneration {
    /// Construct a known generation when the producer has a stable identity.
    #[must_use]
    pub fn known(value: impl Into<String>) -> Self {
        Self::Known(value.into())
    }

    /// Whether this generation can identify the source snapshot.
    #[must_use]
    pub fn is_known(&self) -> bool {
        matches!(self, Self::Known(value) if !value.is_empty())
    }
}

/// Provenance that preserves an explicit unknown state at the transport boundary.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SemanticProvenance {
    /// Provenance carried from the producing layer.
    Known(Provenance),
    /// Provenance was not preserved across the boundary.
    Unknown,
}

impl SemanticProvenance {
    /// Whether this provenance is precise enough to back an exact answer.
    ///
    /// One allowlist serves every exact-capable consumer in this crate so the
    /// envelope classifier and the semantic-query contract cannot drift apart.
    #[must_use]
    pub fn is_exact_grade(&self) -> bool {
        matches!(
            self,
            Self::Known(
                Provenance::ExactAst
                    | Provenance::DesugaredAst
                    | Provenance::SemanticAnalyzer
                    | Provenance::LiteralRequireImport
            )
        )
    }
}

/// Confidence that preserves an explicit unknown state at the transport boundary.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SemanticConfidence {
    /// Confidence carried from the producing layer.
    Known(Confidence),
    /// Confidence was not preserved across the boundary.
    Unknown,
}

/// Producer subsystem that created or adapted the fact.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SemanticProducer {
    /// The parser layer.
    Parser,
    /// The HIR layer.
    Hir,
    /// The compiler PIR (tooling intermediate representation) analysis layer.
    PirA,
    /// The semantic analyzer layer.
    SemanticAnalyzer,
    /// The workspace index.
    WorkspaceIndex,
    /// A framework adapter.
    FrameworkAdapter,
    /// The producer is unknown.
    Unknown,
}

/// Compile/runtime lifecycle context for a fact.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum LifecyclePhase {
    /// `BEGIN` compile-time phase.
    Begin,
    /// `UNITCHECK` phase.
    UnitCheck,
    /// `CHECK` phase.
    Check,
    /// `INIT` phase.
    Init,
    /// `END` phase.
    End,
    /// Normal runtime execution.
    Runtime,
    /// The phase could not be determined.
    Unknown,
}

/// Freshness of a fact and its dependency view for the consuming request.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SemanticFreshness {
    /// Verified current for this request.
    Fresh,
    /// Known older than the request's generation.
    Stale,
    /// Freshness could not be established.
    Unknown,
    /// Freshness does not apply to this fact.
    NotApplicable,
}

/// Source range and stable anchor for a fact.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SourceAnchor {
    /// Stable anchor identity, when the workspace has minted one.
    pub anchor_id: Option<AnchorId>,
    /// File containing the range.
    pub file_id: FileId,
    /// Start of the range in bytes.
    pub start_byte: u32,
    /// End of the range in bytes.
    pub end_byte: u32,
}

impl SourceAnchor {
    /// Construct a source anchor reference.
    #[must_use]
    pub const fn new(
        anchor_id: Option<AnchorId>,
        file_id: FileId,
        start_byte: u32,
        end_byte: u32,
    ) -> Self {
        Self { anchor_id, file_id, start_byte, end_byte }
    }
}

/// Explicit reason why a fact is exact, degraded, or refused.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SemanticReasonCode {
    /// Backed exactly by current source bytes.
    ExactSource,
    /// Generated from source by a reviewed synthesis rule.
    GeneratedFromSource,
    /// A dynamic value prevents exactness.
    DynamicValue,
    /// A compatibility boundary limits the answer.
    CompatibilityBoundary,
    /// An unsupported effect refuses promotion.
    UnsupportedEffect,
    /// No source generation was recorded.
    MissingGeneration,
    /// Provenance was not preserved.
    UnknownProvenance,
    /// Confidence was not preserved.
    UnknownConfidence,
    /// The lifecycle phase is unknown.
    UnknownLifecycle,
    /// An invalidation dependency is stale.
    StaleDependency,
    /// Unrecognized reason code; deserialization fails closed to this value.
    #[serde(other)]
    Unknown,
}

/// Category for a linked dynamic or compatibility boundary.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum BoundaryKind {
    /// A value computed at runtime.
    DynamicValue,
    /// A module name computed at runtime (`require $expr`).
    DynamicRequire,
    /// An include path modified at runtime (`@INC`).
    DynamicIncludePath,
    /// Compile-time code execution affects the fact.
    CompileTimeExecution,
    /// A symbolic reference (`&{$name}`).
    SymbolicReference,
    /// A compatibility shim or legacy surface.
    Compatibility,
    /// The outside environment (filesystem, `%ENV`).
    ExternalEnvironment,
    /// An effect the fact layer does not model.
    Unsupported,
}

/// Whether a boundary permits a degraded answer or refuses promotion.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum BoundaryDisposition {
    /// Permit the fact as a degraded answer.
    Degrade,
    /// Refuse promotion of the fact.
    Refuse,
}

/// Link from a fact to the exact boundary that limits its certainty.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct BoundaryLink {
    /// The linked boundary fact, when it carries its own envelope.
    pub boundary_id: Option<FactId>,
    /// Classification of the boundary.
    pub kind: BoundaryKind,
    /// Whether the boundary degrades or refuses.
    pub disposition: BoundaryDisposition,
    /// Reason code explaining the boundary effect.
    pub reason_code: SemanticReasonCode,
}

impl BoundaryLink {
    /// Construct a boundary link with a stable reason.
    #[must_use]
    pub const fn new(
        boundary_id: Option<FactId>,
        kind: BoundaryKind,
        disposition: BoundaryDisposition,
        reason_code: SemanticReasonCode,
    ) -> Self {
        Self { boundary_id, kind, disposition, reason_code }
    }
}

/// One source or module identity that must remain unchanged for a fact to be fresh.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct InvalidationDependency {
    /// Stable identity of the dependency (source file or module).
    pub dependency_key: String,
    /// Generation the dependency was observed at.
    pub generation: SourceGeneration,
}

impl InvalidationDependency {
    /// Construct a dependency identity.
    #[must_use]
    pub fn new(dependency_key: impl Into<String>, generation: SourceGeneration) -> Self {
        Self { dependency_key: dependency_key.into(), generation }
    }
}

/// Classification available to a provider without inspecting AST/HIR internals.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SemanticFactStatus {
    /// Safe to use as an exact answer.
    Exact,
    /// Usable, but limited by an explicit reason.
    Degraded,
    /// Not safe to use; promotion was refused.
    Refused,
    /// Known stale relative to the consuming request.
    Stale,
}

/// Canonical semantic fact transport contract between producers and providers.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticFactEnvelope {
    /// Stable identity for this fact.
    pub fact_id: FactId,
    /// Backing entity, when the fact is entity-scoped.
    pub entity_id: Option<EntityId>,
    /// Fact kind.
    pub kind: SemanticFactKind,
    /// Source range and anchor for the fact.
    pub anchor: SourceAnchor,
    /// Source snapshot the fact was derived from.
    pub source_generation: SourceGeneration,
    /// Enclosing scope, when known.
    pub scope_id: Option<ScopeId>,
    /// Owning package, when known.
    pub package: Option<String>,
    /// Compile/runtime lifecycle phase.
    pub lifecycle: LifecyclePhase,
    /// Subsystem that created or adapted the fact.
    pub producer: SemanticProducer,
    /// Provenance with explicit unknown state.
    pub provenance: SemanticProvenance,
    /// Confidence with explicit unknown state.
    pub confidence: SemanticConfidence,
    /// Freshness for the consuming request.
    pub freshness: SemanticFreshness,
    /// Boundary limiting the fact, when present.
    pub boundary: Option<BoundaryLink>,
    #[serde(deserialize_with = "deserialize_dependencies")]
    invalidation_dependencies: Vec<InvalidationDependency>,
    /// Why the fact is exact, degraded, or refused.
    pub reason_code: SemanticReasonCode,
}

fn deserialize_dependencies<'de, D>(
    deserializer: D,
) -> Result<Vec<InvalidationDependency>, D::Error>
where
    D: Deserializer<'de>,
{
    let mut dependencies = Vec::<InvalidationDependency>::deserialize(deserializer)?;
    dependencies.sort();
    Ok(dependencies)
}

impl SemanticFactEnvelope {
    /// Construct an envelope and canonicalize dependency order for stable receipts.
    #[allow(clippy::too_many_arguments)] // the constructor mirrors the contract fields
    #[must_use]
    pub fn new(
        fact_id: FactId,
        entity_id: Option<EntityId>,
        kind: SemanticFactKind,
        anchor: SourceAnchor,
        source_generation: SourceGeneration,
        scope_id: Option<ScopeId>,
        package: Option<String>,
        lifecycle: LifecyclePhase,
        producer: SemanticProducer,
        provenance: SemanticProvenance,
        confidence: SemanticConfidence,
        freshness: SemanticFreshness,
        boundary: Option<BoundaryLink>,
        mut invalidation_dependencies: Vec<InvalidationDependency>,
        reason_code: SemanticReasonCode,
    ) -> Self {
        invalidation_dependencies.sort();
        Self {
            fact_id,
            entity_id,
            kind,
            anchor,
            source_generation,
            scope_id,
            package,
            lifecycle,
            producer,
            provenance,
            confidence,
            freshness,
            boundary,
            invalidation_dependencies,
            reason_code,
        }
    }

    /// Return the canonical dependency view without exposing mutable storage.
    #[must_use]
    pub fn invalidation_dependencies(&self) -> &[InvalidationDependency] {
        &self.invalidation_dependencies
    }

    fn has_malformed_structure(&self) -> bool {
        if self.anchor.start_byte > self.anchor.end_byte
            || self
                .invalidation_dependencies
                .iter()
                .any(|dependency| dependency.dependency_key.trim().is_empty())
        {
            return true;
        }

        // The constructor and deserializer canonicalize dependency order, so
        // conflicting generations for one key are adjacent here.
        self.invalidation_dependencies.windows(2).any(|dependencies| {
            dependencies[0].dependency_key == dependencies[1].dependency_key
                && dependencies[0].generation != dependencies[1].generation
        })
    }

    fn has_exact_provenance(&self) -> bool {
        self.provenance.is_exact_grade()
    }

    /// Classify the envelope for a provider decision.
    #[must_use]
    pub fn status(&self) -> SemanticFactStatus {
        if matches!(self.freshness, SemanticFreshness::Stale)
            || self
                .invalidation_dependencies
                .iter()
                .any(|dependency| !dependency.generation.is_known())
        {
            return SemanticFactStatus::Stale;
        }
        if matches!(
            self.boundary,
            Some(BoundaryLink { disposition: BoundaryDisposition::Refuse, .. })
        ) {
            return SemanticFactStatus::Refused;
        }
        match self.reason_code {
            SemanticReasonCode::UnsupportedEffect => return SemanticFactStatus::Refused,
            SemanticReasonCode::StaleDependency => return SemanticFactStatus::Stale,
            _ => {}
        }
        if !matches!(self.reason_code, SemanticReasonCode::ExactSource) {
            return SemanticFactStatus::Degraded;
        }
        if self.has_malformed_structure() {
            return SemanticFactStatus::Degraded;
        }
        if !self.source_generation.is_known()
            || matches!(self.provenance, SemanticProvenance::Unknown)
            || !matches!(self.confidence, SemanticConfidence::Known(Confidence::High))
            || !self.has_exact_provenance()
            || matches!(self.lifecycle, LifecyclePhase::Unknown)
            || !matches!(self.freshness, SemanticFreshness::Fresh)
            || matches!(self.producer, SemanticProducer::Unknown)
        {
            return SemanticFactStatus::Degraded;
        }
        if self.boundary.is_some() {
            SemanticFactStatus::Degraded
        } else {
            SemanticFactStatus::Exact
        }
    }
}

/// Relationship between a callable invocation and its returned value.
///
/// The relation remains symbolic until an exact callsite supplies the receiver
/// and arguments. This prevents framework and ordinary-source producers from
/// hard-coding one concrete package for receiver-self or argument-dependent
/// methods.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CallableResultRelation {
    /// One source-backed concrete value shape.
    Concrete(ValueShape),
    /// The call returns its current receiver/invocant.
    ReceiverSelf,
    /// The call returns one exact parameter binding supplied by the callsite.
    Argument {
        /// Canonical parameter binding entity.
        parameter_entity_id: EntityId,
        /// Zero-based parameter position retained for explanation and fallback.
        position: u16,
    },
    /// Perl bare-return semantics; materialization remains context-sensitive.
    BareReturn,
    /// Optional value relation, preserving the inner identity.
    Optional(Box<CallableResultRelation>),
    /// Complete finite relation alternatives across admitted exits.
    FiniteUnion(Vec<CallableResultRelation>),
    /// No exact relation is available.
    Unknown,
}

impl CallableResultRelation {
    /// Build a deterministic finite union, flattening nested unions and removing
    /// duplicate alternatives. Empty input becomes [`Self::Unknown`].
    #[must_use]
    pub fn finite_union(relations: Vec<Self>) -> Self {
        let mut flattened = Vec::new();
        for relation in relations {
            match relation {
                Self::FiniteUnion(inner) => flattened.extend(inner),
                relation => flattened.push(relation),
            }
        }
        flattened.sort_by_key(relation_sort_key);
        flattened.dedup();
        match flattened.len() {
            0 => Self::Unknown,
            1 => flattened.pop().unwrap_or(Self::Unknown),
            _ => Self::FiniteUnion(flattened),
        }
    }

    /// Whether this relation or one nested alternative remains unknown.
    #[must_use]
    pub fn contains_unknown(&self) -> bool {
        match self {
            Self::Concrete(ValueShape::Unknown) | Self::Unknown => true,
            Self::Optional(inner) => inner.contains_unknown(),
            Self::FiniteUnion(relations) => relations.iter().any(Self::contains_unknown),
            _ => false,
        }
    }
}

fn relation_sort_key(relation: &CallableResultRelation) -> String {
    match relation {
        CallableResultRelation::Concrete(shape) => format!("0:{}", value_shape_sort_key(shape)),
        CallableResultRelation::ReceiverSelf => "1:receiver-self".to_string(),
        CallableResultRelation::Argument { parameter_entity_id, position } => {
            format!("2:{position:05}:{}", parameter_entity_id.0)
        }
        CallableResultRelation::BareReturn => "3:bare-return".to_string(),
        CallableResultRelation::Optional(inner) => format!("4:{}", relation_sort_key(inner)),
        CallableResultRelation::FiniteUnion(relations) => {
            format!("5:{}", relations.iter().map(relation_sort_key).collect::<Vec<_>>().join("|"))
        }
        CallableResultRelation::Unknown => "9:unknown".to_string(),
    }
}

fn value_shape_sort_key(shape: &ValueShape) -> String {
    match shape {
        ValueShape::Unknown => "unknown".to_string(),
        ValueShape::Scalar => "scalar".to_string(),
        ValueShape::ArrayRef => "array-ref".to_string(),
        ValueShape::HashRef => "hash-ref".to_string(),
        ValueShape::CodeRef => "code-ref".to_string(),
        ValueShape::PackageName { package } => format!("package:{package}"),
        ValueShape::Object { package, confidence } => {
            format!("object:{package}:{confidence:?}")
        }
    }
}

/// Whether every admitted callable exit contributed to the relation.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum CallableResultCompleteness {
    /// Every admitted reachable exit is represented.
    Complete,
    /// Useful alternatives exist, but the reachable-exit denominator is incomplete.
    Partial,
}

/// Context or source limitation retained by a callable result fact.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum CallableResultLimitation {
    /// Observed only in scalar context.
    ScalarContext,
    /// Observed only in list context.
    ListContext,
    /// Observed only in void context.
    VoidContext,
    /// A conditional exit bounded the relation.
    ConditionalControl,
    /// Loop control (`last`/`next`) bounded the relation.
    LoopControl,
    /// Exception control (`die`/`croak`) bounded the relation.
    ExceptionControl,
    /// A dynamic return value limited materialization.
    DynamicValue,
    /// Syntax recovery limited the extraction.
    RecoveredSyntax,
    /// Generated code without exact source bytes.
    GeneratedNoSource,
    /// The work budget was exhausted before full analysis.
    BudgetExhausted,
    /// An unsupported construct limited the relation.
    Unsupported,
}

/// Canonical callable-result payload plus its semantic envelope.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallableResultFact {
    /// Shared semantic identity, source generation, proof, and invalidation data.
    pub envelope: SemanticFactEnvelope,
    /// Symbolic or concrete relation to materialize at an exact callsite.
    pub relation: CallableResultRelation,
    /// Source anchors for every admitted exit contributor.
    exit_anchors: Vec<SourceAnchor>,
    /// Reachable-exit coverage for the relation.
    pub completeness: CallableResultCompleteness,
    /// Context and source limitations preserved for query policy.
    limitations: Vec<CallableResultLimitation>,
}

impl CallableResultFact {
    /// Construct a callable-result fact and canonicalize its contributor and
    /// limitation order. The envelope kind is forced to `CallableResult`.
    #[must_use]
    pub fn new(
        mut envelope: SemanticFactEnvelope,
        relation: CallableResultRelation,
        mut exit_anchors: Vec<SourceAnchor>,
        completeness: CallableResultCompleteness,
        mut limitations: Vec<CallableResultLimitation>,
    ) -> Self {
        envelope.kind = SemanticFactKind::CallableResult;
        exit_anchors.sort();
        exit_anchors.dedup();
        limitations.sort();
        limitations.dedup();
        Self { envelope, relation, exit_anchors, completeness, limitations }
    }

    /// Canonical exit-contributor view.
    #[must_use]
    pub fn exit_anchors(&self) -> &[SourceAnchor] {
        &self.exit_anchors
    }

    /// Canonical limitation view.
    #[must_use]
    pub fn limitations(&self) -> &[CallableResultLimitation] {
        &self.limitations
    }

    /// Classify whether this complete payload may participate in exact callsite
    /// materialization without reopening producer internals.
    #[must_use]
    pub fn status(&self) -> SemanticFactStatus {
        let envelope_status = self.envelope.status();
        if !matches!(envelope_status, SemanticFactStatus::Exact) {
            return envelope_status;
        }
        if !matches!(self.completeness, CallableResultCompleteness::Complete)
            || self.relation.contains_unknown()
            || self.exit_anchors.is_empty()
            || !self.limitations.is_empty()
        {
            SemanticFactStatus::Degraded
        } else {
            SemanticFactStatus::Exact
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn exact_envelope(dependencies: Vec<InvalidationDependency>) -> SemanticFactEnvelope {
        SemanticFactEnvelope::new(
            FactId(1),
            Some(EntityId(2)),
            SemanticFactKind::Declaration,
            SourceAnchor::new(Some(AnchorId(3)), FileId(4), 10, 20),
            SourceGeneration::known("source-sha"),
            Some(ScopeId(5)),
            Some("Example".to_string()),
            LifecyclePhase::Runtime,
            SemanticProducer::PirA,
            SemanticProvenance::Known(Provenance::ExactAst),
            SemanticConfidence::Known(Confidence::High),
            SemanticFreshness::Fresh,
            None,
            dependencies,
            SemanticReasonCode::ExactSource,
        )
    }

    #[test]
    fn envelope_roundtrip_is_deterministic_and_sorts_dependencies() -> Result<(), serde_json::Error>
    {
        let envelope = exact_envelope(vec![
            InvalidationDependency::new("module:Z", SourceGeneration::known("z")),
            InvalidationDependency::new("module:A", SourceGeneration::known("a")),
        ]);
        let serialized = serde_json::to_string(&envelope)?;
        let decoded: SemanticFactEnvelope = serde_json::from_str(&serialized)?;
        assert_eq!(decoded, envelope);
        assert_eq!(envelope.invalidation_dependencies()[0].dependency_key, "module:A");
        assert_eq!(serde_json::to_string(&decoded)?, serialized);
        Ok(())
    }

    #[test]
    fn deserialization_canonicalizes_dependency_order() -> Result<(), serde_json::Error> {
        let mut value = serde_json::to_value(exact_envelope(Vec::new()))?;
        value["invalidation_dependencies"] = serde_json::json!([
            {
                "dependency_key": "module:Z",
                "generation": { "Known": "z" }
            },
            {
                "dependency_key": "module:A",
                "generation": { "Known": "a" }
            }
        ]);
        let decoded: SemanticFactEnvelope = serde_json::from_value(value)?;
        assert_eq!(
            decoded
                .invalidation_dependencies()
                .iter()
                .map(|dependency| dependency.dependency_key.as_str())
                .collect::<Vec<_>>(),
            ["module:A", "module:Z"]
        );
        Ok(())
    }

    #[test]
    fn envelope_kind_is_discriminated_in_json() -> Result<(), serde_json::Error> {
        let value: Value = serde_json::to_value(exact_envelope(Vec::new()))?;
        assert_eq!(value["kind"], "Declaration");
        assert_eq!(value["producer"], "PirA");
        Ok(())
    }

    #[test]
    fn exact_fact_is_available_to_a_provider_without_ast_access() {
        assert_eq!(exact_envelope(Vec::new()).status(), SemanticFactStatus::Exact);
    }

    #[test]
    fn only_fresh_facts_can_be_promoted_as_exact() {
        let mut envelope = exact_envelope(Vec::new());
        envelope.freshness = SemanticFreshness::NotApplicable;
        assert_eq!(envelope.status(), SemanticFactStatus::Degraded);
    }

    #[test]
    fn unknown_metadata_cannot_be_promoted_as_exact() {
        let mut envelope = exact_envelope(Vec::new());
        envelope.source_generation = SourceGeneration::Unknown;
        assert_eq!(envelope.status(), SemanticFactStatus::Degraded);

        envelope.source_generation = SourceGeneration::known("source-sha");
        envelope.provenance = SemanticProvenance::Unknown;
        assert_eq!(envelope.status(), SemanticFactStatus::Degraded);

        envelope.provenance = SemanticProvenance::Known(Provenance::ExactAst);
        envelope.confidence = SemanticConfidence::Known(Confidence::Medium);
        assert_eq!(envelope.status(), SemanticFactStatus::Degraded);

        envelope.confidence = SemanticConfidence::Known(Confidence::Low);
        assert_eq!(envelope.status(), SemanticFactStatus::Degraded);
    }

    #[test]
    fn inferred_synthetic_and_dynamic_provenance_cannot_be_exact() {
        for provenance in [
            Provenance::FrameworkSynthesis,
            Provenance::ImportExportInference,
            Provenance::PragmaInference,
            Provenance::NameHeuristic,
            Provenance::SearchFallback,
            Provenance::DynamicBoundary,
        ] {
            let mut envelope = exact_envelope(Vec::new());
            envelope.provenance = SemanticProvenance::Known(provenance);
            assert_eq!(envelope.status(), SemanticFactStatus::Degraded);
        }
    }

    #[test]
    fn allowlisted_exact_provenance_remains_exact() {
        for provenance in [
            Provenance::ExactAst,
            Provenance::DesugaredAst,
            Provenance::SemanticAnalyzer,
            Provenance::LiteralRequireImport,
        ] {
            let mut envelope = exact_envelope(Vec::new());
            envelope.provenance = SemanticProvenance::Known(provenance);
            assert_eq!(envelope.status(), SemanticFactStatus::Exact);
        }
    }

    #[test]
    fn exact_source_reason_remains_exact() {
        let mut envelope = exact_envelope(Vec::new());
        envelope.reason_code = SemanticReasonCode::ExactSource;
        assert_eq!(envelope.status(), SemanticFactStatus::Exact);
    }

    #[test]
    fn generated_source_reason_remains_degraded() {
        let mut envelope = exact_envelope(Vec::new());
        envelope.reason_code = SemanticReasonCode::GeneratedFromSource;
        assert_eq!(envelope.status(), SemanticFactStatus::Degraded);
    }

    #[test]
    fn stale_dependency_reason_is_stale() {
        let mut envelope = exact_envelope(Vec::new());
        envelope.reason_code = SemanticReasonCode::StaleDependency;
        assert_eq!(envelope.status(), SemanticFactStatus::Stale);
    }

    #[test]
    fn future_serialized_reason_code_fails_closed_to_degraded() -> Result<(), serde_json::Error> {
        let mut value = serde_json::to_value(exact_envelope(Vec::new()))?;
        value["reason_code"] = serde_json::json!("FutureReasonCode");

        let decoded: SemanticFactEnvelope = serde_json::from_value(value)?;
        assert_eq!(decoded.reason_code, SemanticReasonCode::Unknown);
        assert_eq!(decoded.status(), SemanticFactStatus::Degraded);
        Ok(())
    }

    #[test]
    fn uncertainty_reason_codes_fail_closed_without_a_boundary_link() {
        for (reason_code, expected) in [
            (SemanticReasonCode::DynamicValue, SemanticFactStatus::Degraded),
            (SemanticReasonCode::CompatibilityBoundary, SemanticFactStatus::Degraded),
            (SemanticReasonCode::MissingGeneration, SemanticFactStatus::Degraded),
            (SemanticReasonCode::UnsupportedEffect, SemanticFactStatus::Refused),
            (SemanticReasonCode::UnknownProvenance, SemanticFactStatus::Degraded),
            (SemanticReasonCode::UnknownConfidence, SemanticFactStatus::Degraded),
            (SemanticReasonCode::UnknownLifecycle, SemanticFactStatus::Degraded),
            (SemanticReasonCode::Unknown, SemanticFactStatus::Degraded),
        ] {
            let mut envelope = exact_envelope(Vec::new());
            envelope.reason_code = reason_code;
            assert_eq!(envelope.status(), expected);
        }
    }

    #[test]
    fn unverifiable_dependency_generation_is_stale() {
        for generation in [SourceGeneration::Unknown, SourceGeneration::known("")] {
            let envelope =
                exact_envelope(vec![InvalidationDependency::new("module:unknown", generation)]);
            assert_eq!(envelope.status(), SemanticFactStatus::Stale);
        }
    }

    #[test]
    fn malformed_structure_cannot_be_promoted_as_exact() {
        let mut inverted_range = exact_envelope(Vec::new());
        inverted_range.anchor.start_byte = 20;
        inverted_range.anchor.end_byte = 10;
        assert_eq!(inverted_range.status(), SemanticFactStatus::Degraded);

        let empty_dependency_key = exact_envelope(vec![InvalidationDependency::new(
            "",
            SourceGeneration::known("generation"),
        )]);
        assert_eq!(empty_dependency_key.status(), SemanticFactStatus::Degraded);

        // #12668: whitespace-only keys are empty keys, matching the provider
        // port validator's trim semantics for the same envelope.
        let whitespace_dependency_key = exact_envelope(vec![InvalidationDependency::new(
            "  ",
            SourceGeneration::known("generation"),
        )]);
        assert_eq!(whitespace_dependency_key.status(), SemanticFactStatus::Degraded);

        let conflicting_dependencies = exact_envelope(vec![
            InvalidationDependency::new("module:Example", SourceGeneration::known("one")),
            InvalidationDependency::new("module:Example", SourceGeneration::known("two")),
        ]);
        assert_eq!(conflicting_dependencies.status(), SemanticFactStatus::Degraded);
    }

    #[test]
    fn boundary_and_staleness_are_distinguished() {
        let mut envelope = exact_envelope(Vec::new());
        envelope.boundary = Some(BoundaryLink::new(
            Some(FactId(9)),
            BoundaryKind::DynamicValue,
            BoundaryDisposition::Degrade,
            SemanticReasonCode::DynamicValue,
        ));
        assert_eq!(envelope.status(), SemanticFactStatus::Degraded);

        envelope.boundary = Some(BoundaryLink::new(
            Some(FactId(10)),
            BoundaryKind::Unsupported,
            BoundaryDisposition::Refuse,
            SemanticReasonCode::UnsupportedEffect,
        ));
        assert_eq!(envelope.status(), SemanticFactStatus::Refused);

        envelope.boundary = None;
        envelope.freshness = SemanticFreshness::Stale;
        assert_eq!(envelope.status(), SemanticFactStatus::Stale);
    }

    #[test]
    fn callable_result_union_is_deterministic_and_deduplicated() -> Result<(), String> {
        let relation = CallableResultRelation::finite_union(vec![
            CallableResultRelation::ReceiverSelf,
            CallableResultRelation::Concrete(ValueShape::Scalar),
            CallableResultRelation::ReceiverSelf,
        ]);
        let relations = match relation {
            CallableResultRelation::FiniteUnion(relations) => relations,
            other => return Err(format!("expected a finite union, got {other:?}")),
        };
        assert_eq!(relations.len(), 2);
        assert_eq!(
            CallableResultRelation::finite_union(relations.clone()),
            CallableResultRelation::FiniteUnion(relations)
        );
        Ok(())
    }

    #[test]
    fn callable_result_fact_roundtrips_and_forces_kind() -> Result<(), serde_json::Error> {
        let fact = CallableResultFact::new(
            exact_envelope(Vec::new()),
            CallableResultRelation::ReceiverSelf,
            vec![SourceAnchor::new(Some(AnchorId(12)), FileId(4), 30, 40)],
            CallableResultCompleteness::Complete,
            Vec::new(),
        );
        assert_eq!(fact.envelope.kind, SemanticFactKind::CallableResult);
        assert_eq!(fact.status(), SemanticFactStatus::Exact);

        let serialized = serde_json::to_string(&fact)?;
        let decoded: CallableResultFact = serde_json::from_str(&serialized)?;
        assert_eq!(decoded, fact);
        assert_eq!(decoded.status(), SemanticFactStatus::Exact);
        Ok(())
    }

    #[test]
    fn partial_unknown_or_limited_result_cannot_be_exact() {
        let exit = SourceAnchor::new(Some(AnchorId(12)), FileId(4), 30, 40);
        let partial = CallableResultFact::new(
            exact_envelope(Vec::new()),
            CallableResultRelation::ReceiverSelf,
            vec![exit],
            CallableResultCompleteness::Partial,
            Vec::new(),
        );
        assert_eq!(partial.status(), SemanticFactStatus::Degraded);

        let unknown = CallableResultFact::new(
            exact_envelope(Vec::new()),
            CallableResultRelation::Unknown,
            vec![exit],
            CallableResultCompleteness::Complete,
            Vec::new(),
        );
        assert_eq!(unknown.status(), SemanticFactStatus::Degraded);

        let limited = CallableResultFact::new(
            exact_envelope(Vec::new()),
            CallableResultRelation::ReceiverSelf,
            vec![exit],
            CallableResultCompleteness::Complete,
            vec![CallableResultLimitation::ConditionalControl],
        );
        assert_eq!(limited.status(), SemanticFactStatus::Degraded);
    }

    #[test]
    fn receiver_self_and_argument_remain_symbolic_relations() {
        let receiver = CallableResultRelation::ReceiverSelf;
        let argument =
            CallableResultRelation::Argument { parameter_entity_id: EntityId(88), position: 1 };
        assert_ne!(receiver, argument);
        assert!(!receiver.contains_unknown());
        assert!(!argument.contains_unknown());
    }
}
