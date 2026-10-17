use super::{
    AuthorityPort, Binding, BoundaryReference, ContentDigest, Deserialize, Facet, FactClass,
    LifecyclePhase, MAX_BUNDLE_BYTES, MAX_SNAPSHOT_BYTES, Origin, Outcome, PortRole, Read,
    SchemaError, SemanticScopeIdentity, SemanticSourceAnchor, SemanticSourceOrderIdentity,
    Serialize, validation, wire,
};
use perl_semantic_facts::{BoundaryKind, CompileEffectSourceKind, SemanticReasonCode};
/// Closed compile-effect vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryDisposition {
    /// exact_applied.
    ExactApplied,
    /// exact_no_effect.
    ExactNoEffect,
    /// conditional.
    Conditional,
    /// limited.
    Limited,
    /// unsupported.
    Unsupported,
    /// unavailable.
    Unavailable,
    /// stale.
    Stale,
    /// resource_or_instrument_failure.
    ResourceOrInstrumentFailure,
}
/// Closed compile-effect vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryReason {
    /// dynamic_conditional_effect.
    DynamicConditionalEffect,
    /// ambiguous_conditional_target.
    AmbiguousConditionalTarget,
    /// dynamic_import.
    DynamicImport,
    /// unknown_warning_name.
    UnknownWarningName,
    /// unknown_feature_name.
    UnknownFeatureName,
    /// dynamic_warning_arguments.
    DynamicWarningArguments,
    /// dynamic_feature_arguments.
    DynamicFeatureArguments,
    /// source_filter.
    SourceFilter,
    /// generated_source.
    GeneratedSource,
    /// eval_string.
    EvalString,
    /// dynamic_require.
    DynamicRequire,
    /// dynamic_include_root.
    DynamicIncludeRoot,
    /// malformed_directive.
    MalformedDirective,
    /// recovery_ambiguous_directive.
    RecoveryAmbiguousDirective,
    /// dynamic_stash.
    DynamicStash,
    /// dynamic_typeglob.
    DynamicTypeglob,
    /// dynamic_inheritance.
    DynamicInheritance,
    /// dynamic_generated_member.
    DynamicGeneratedMember,
    /// phase_block_execution.
    PhaseBlockExecution,
    /// ambient_input.
    AmbientInput,
    /// native_capability.
    NativeCapability,
    /// stale_profile.
    StaleProfile,
    /// stale_source.
    StaleSource,
    /// stale_dependency.
    StaleDependency,
    /// resource_exhaustion.
    ResourceExhaustion,
    /// instrument_failure.
    InstrumentFailure,
    /// dynamic_unimport.
    DynamicUnimport,
    /// custom_import.
    CustomImport,
    /// custom_unimport.
    CustomUnimport,
}
impl BoundaryReason {
    /// Conservative compatible coarse classification; the detailed reason remains canonical.
    pub fn semantic_reason(self) -> Option<SemanticReasonCode> {
        match self {
            Self::DynamicConditionalEffect => Some(SemanticReasonCode::DynamicValue),
            Self::AmbiguousConditionalTarget => None,
            Self::DynamicImport => Some(SemanticReasonCode::DynamicValue),
            Self::UnknownWarningName => None,
            Self::UnknownFeatureName => None,
            Self::DynamicWarningArguments => Some(SemanticReasonCode::DynamicValue),
            Self::DynamicFeatureArguments => Some(SemanticReasonCode::DynamicValue),
            Self::SourceFilter => None,
            Self::GeneratedSource => None,
            Self::EvalString => None,
            Self::DynamicRequire => Some(SemanticReasonCode::DynamicValue),
            Self::DynamicIncludeRoot => Some(SemanticReasonCode::DynamicValue),
            Self::MalformedDirective => None,
            Self::RecoveryAmbiguousDirective => None,
            Self::DynamicStash => Some(SemanticReasonCode::DynamicValue),
            Self::DynamicTypeglob => Some(SemanticReasonCode::DynamicValue),
            Self::DynamicInheritance => Some(SemanticReasonCode::DynamicValue),
            Self::DynamicGeneratedMember => Some(SemanticReasonCode::DynamicValue),
            Self::PhaseBlockExecution => None,
            Self::AmbientInput => None,
            Self::NativeCapability => None,
            Self::StaleProfile => Some(SemanticReasonCode::StaleDependency),
            Self::StaleSource => Some(SemanticReasonCode::StaleDependency),
            Self::StaleDependency => Some(SemanticReasonCode::StaleDependency),
            Self::ResourceExhaustion => None,
            Self::InstrumentFailure => None,
            Self::DynamicUnimport => Some(SemanticReasonCode::DynamicValue),
            Self::CustomImport => None,
            Self::CustomUnimport => None,
        }
    }
    /// Conservative compatible coarse classification; the detailed reason remains canonical.
    pub fn boundary_kind(self) -> Option<BoundaryKind> {
        match self {
            Self::DynamicConditionalEffect => Some(BoundaryKind::DynamicValue),
            Self::AmbiguousConditionalTarget => None,
            Self::DynamicImport => None,
            Self::UnknownWarningName => None,
            Self::UnknownFeatureName => None,
            Self::DynamicWarningArguments => Some(BoundaryKind::DynamicValue),
            Self::DynamicFeatureArguments => Some(BoundaryKind::DynamicValue),
            Self::SourceFilter => None,
            Self::GeneratedSource => None,
            Self::EvalString => None,
            Self::DynamicRequire => Some(BoundaryKind::DynamicRequire),
            Self::DynamicIncludeRoot => Some(BoundaryKind::DynamicIncludePath),
            Self::MalformedDirective => None,
            Self::RecoveryAmbiguousDirective => None,
            Self::DynamicStash => None,
            Self::DynamicTypeglob => None,
            Self::DynamicInheritance => None,
            Self::DynamicGeneratedMember => None,
            Self::PhaseBlockExecution => None,
            Self::AmbientInput => Some(BoundaryKind::ExternalEnvironment),
            Self::NativeCapability => None,
            Self::StaleProfile => None,
            Self::StaleSource => None,
            Self::StaleDependency => None,
            Self::ResourceExhaustion => None,
            Self::InstrumentFailure => None,
            Self::DynamicUnimport => None,
            Self::CustomImport => None,
            Self::CustomUnimport => None,
        }
    }
}
/// Independently addressable strict category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StrictCategory {
    /// Variable declarations.
    Vars,
    /// Bareword/subroutine declarations.
    Subs,
    /// Symbolic references.
    Refs,
}
/// Missing persistent authority cannot support exact application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PendingAuthority {
    /// No canonical category catalog identity is available.
    CategoryCatalog,
    /// No canonical module/SCC identity is available.
    ModuleGraph,
}
/// Machine-readable reason why effect extent is unbounded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnknownExtentReason {
    /// Target is computed dynamically.
    DynamicTarget,
    /// Source needed to bound the effect is unavailable.
    UnavailableSource,
    /// Arbitrary execution has no narrower declared extent.
    UnmodeledExecution,
}
/// Declared localized effect extent; external tagging keeps unknown fields visible.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ImpactSelector {
    /// One strict category in the record's lexical scope.
    StrictCategory(StrictCategory),
    /// One named fact in the record's bound scope.
    NamedFact {
        /// Fact family.
        class: FactClass,
        /// Source-level name, not a global numeric ID.
        name: String,
    },
    /// Pending catalog subtree; this never establishes an exact category identity.
    CategorySubtree {
        /// Fact family.
        class: FactClass,
        /// Unresolved category spelling.
        category: String,
        /// Explicit missing authority.
        pending: PendingAuthority,
    },
    /// Remainder of the bound logical source.
    SourceSuffix {
        /// Fact family.
        class: FactClass,
        /// Inclusive source byte offset.
        start_byte: u64,
    },
    /// One accepted lexical scope.
    LexicalScope {
        /// Fact family.
        class: FactClass,
        /// Scope within the same complete binding.
        scope: Box<SemanticScopeIdentity>,
    },
    /// One package declaration context within the bound source.
    Package {
        /// Fact family.
        class: FactClass,
        /// Source-order package identity.
        package: SemanticSourceOrderIdentity,
    },
    /// Pending module or SCC closure, without inventing graph identity.
    ModuleClosure {
        /// Unresolved module spelling.
        module: String,
        /// Whether the requested closure is an SCC.
        scc: bool,
        /// Explicit missing authority.
        pending: PendingAuthority,
        /// Declared possible families.
        classes: Vec<FactClass>,
    },
    /// Explicit finite set of affected families.
    DeclaredFacts(Vec<FactClass>),
    /// Unknown extent; cannot coexist with any narrower declaration.
    Unbounded {
        /// Typed reason, not explanatory prose.
        unknown_extent: UnknownExtentReason,
    },
}
/// Bounded human-facing fields excluded from semantic identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoundaryMetadata {
    /// Explanation, not semantic authority.
    pub explanation: String,
    /// Bounded explanatory limitations, never semantic identity.
    pub limitations: Vec<String>,
    /// Maintainer reference.
    pub owner: Option<String>,
    /// Replacement tracking reference.
    pub replacement: Option<String>,
    /// Presentation-only source excerpt.
    pub trigger_excerpt: Option<String>,
}
/// Untrusted full boundary record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoundaryDraft {
    /// Wire version.
    pub schema_version: u32,
    /// Complete source and compiler subject.
    pub binding: Binding,
    /// Lexical scope.
    pub scope: SemanticScopeIdentity,
    /// Package identity.
    pub package: Option<SemanticSourceOrderIdentity>,
    /// Lifecycle phase.
    pub phase: LifecyclePhase,
    /// Originating construct.
    pub source_kind: CompileEffectSourceKind,
    /// Producer effect identity.
    pub effect_id: String,
    /// Source-bound logical anchor.
    pub anchor: SemanticSourceAnchor,
    /// Start byte.
    pub start_byte: u64,
    /// Exclusive end byte.
    pub end_byte: u64,
    /// Detailed reason, distinct from instance identity.
    pub reason: BoundaryReason,
    /// Effect disposition.
    pub disposition: BoundaryDisposition,
    /// Qualified effect authority.
    pub effect: Facet<AuthorityPort>,
    /// Potential affected facts.
    pub possible_impacts: Vec<ImpactSelector>,
    /// Actually applied facts.
    pub applied_impacts: Vec<ImpactSelector>,
    /// Prior state identity, when available.
    pub state_before: Option<ContentDigest>,
    /// Possible state identities.
    pub possible_states: Vec<ContentDigest>,
    /// Retained provenance.
    pub origins: Vec<Origin>,
    /// Nonsemantic presentation/tracking fields.
    pub metadata: BoundaryMetadata,
}

/// Maximum boundaries in one source/module bundle.
pub const MAX_BOUNDARIES: usize = 4096;
/// Maximum selectors or referenced possible states per boundary.
pub const MAX_BOUNDARY_ITEMS: usize = 256;
/// Maximum explanation or trigger UTF-8 bytes.
pub const MAX_BOUNDARY_TEXT_BYTES: usize = 4096;

impl BoundaryDisposition {
    /// Coarse facet qualification; exact-no-effect remains distinct on the record.
    pub fn outcome(self) -> Outcome {
        match self {
            Self::ExactApplied | Self::ExactNoEffect => Outcome::Exact,
            Self::Conditional => Outcome::Conditional,
            Self::Limited => Outcome::Limited,
            Self::Unsupported => Outcome::Unsupported,
            Self::Unavailable => Outcome::Unavailable,
            Self::Stale => Outcome::Stale,
            Self::ResourceOrInstrumentFailure => Outcome::ResourceInstrument,
        }
    }
}

/// Validated boundary record. Metadata is retained but excluded from its semantic ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundaryRecord(BoundaryDraft);
impl BoundaryRecord {
    /// Validate a caller-owned draft without interpreting its effect.
    /// Operational `SchemaError::Limited` creates no record: callers must retain a
    /// resource/instrument refusal, never synthesize an exact or ordinary limited record.
    pub fn admit(mut draft: BoundaryDraft) -> Result<Self, SchemaError> {
        super::boundary_validation::validate(&mut draft)?;
        wire::encode(&draft, MAX_SNAPSHOT_BYTES)?;
        Ok(Self(draft))
    }
    /// Bounded closed-wire input; includes whitespace in its input budget.
    pub fn read(reader: impl Read) -> Result<Self, SchemaError> {
        Self::admit(wire::read(reader, MAX_SNAPSHOT_BYTES)?)
    }
    /// Canonical complete wire record, including nonsemantic metadata.
    pub fn to_json(&self) -> Result<Vec<u8>, SchemaError> {
        wire::encode(&self.0, MAX_SNAPSHOT_BYTES)
    }
    /// Borrow the admitted draft.
    pub fn draft(&self) -> &BoundaryDraft {
        &self.0
    }
    /// Domain-separated identity over the declared semantic projection.
    pub fn digest(&self) -> Result<ContentDigest, SchemaError> {
        // Alphabetical object keys are an explicit v1 wire contract. No metadata,
        // metadata-carried host path, wall clock, workflow identity or self-digest enters this map.
        let mut semantic = serde_json::to_value(&self.0)
            .map_err(|error| SchemaError::Instrument(error.to_string()))?;
        let object = semantic.as_object_mut().ok_or_else(|| {
            SchemaError::Instrument("boundary projection is not an object".into())
        })?;
        object.remove("metadata");
        let effect =
            object.get_mut("effect").and_then(serde_json::Value::as_object_mut).ok_or_else(
                || SchemaError::Instrument("boundary effect projection is not an object".into()),
            )?;
        effect.remove("reason");
        Ok(ContentDigest::of_bytes(&wire::canonical(
            "compile_effect_boundary.v1",
            &semantic,
            MAX_SNAPSHOT_BYTES,
        )?))
    }
    /// Qualified reference to this record; never upgrades pending or nonexact effects.
    /// A reference validates declared identity and qualification, not remote authenticity.
    pub fn reference(&self) -> Result<BoundaryReference, SchemaError> {
        let digest = self.digest()?;
        let mut affects = Vec::new();
        for selector in &self.0.possible_impacts {
            affects.extend(selector.fact_classes());
        }
        affects.sort();
        affects.dedup();
        let outcome = self.0.disposition.outcome();
        Ok(BoundaryReference {
            id: digest.clone(),
            binding: self.0.binding.clone(),
            disposition: self.0.disposition,
            pending_authority: self.0.possible_impacts.iter().any(|impact| {
                matches!(
                    impact,
                    ImpactSelector::CategorySubtree { .. } | ImpactSelector::ModuleClosure { .. }
                )
            }),
            affects,
            authority: Facet {
                outcome,
                value: Some(AuthorityPort {
                    role: PortRole::Boundary,
                    schema_version: 1,
                    contract: "compile_effect_boundary.v1".into(),
                    subject: self.0.binding.semantic.clone(),
                    compiler_generation: self.0.binding.compiler_generation.clone(),
                    payload: digest,
                }),
                reason: if outcome == Outcome::Exact {
                    None
                } else {
                    Some("qualified compile-effect boundary".into())
                },
                origins: self.0.origins.clone(),
            },
        })
    }
}
impl ImpactSelector {
    /// Declared possible classes, not inferred applied effects or a reason-to-fact rule.
    pub fn fact_classes(&self) -> Vec<FactClass> {
        match self {
            Self::StrictCategory(_) => vec![FactClass::Strict],
            Self::NamedFact { class, .. }
            | Self::CategorySubtree { class, .. }
            | Self::SourceSuffix { class, .. }
            | Self::LexicalScope { class, .. }
            | Self::Package { class, .. } => vec![*class],
            Self::ModuleClosure { classes, .. } | Self::DeclaredFacts(classes) => classes.clone(),
            Self::Unbounded { .. } => vec![FactClass::AllFacts],
        }
    }
    pub(super) fn cannot_be_exact(&self) -> bool {
        matches!(
            self,
            Self::CategorySubtree { .. } | Self::ModuleClosure { .. } | Self::Unbounded { .. }
        )
    }
}
/// Bounded ordered source/module boundary collection. No propagation is performed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundaryBundle(Vec<BoundaryRecord>);
impl BoundaryBundle {
    /// Admit every record or refuse the whole collection; never truncate exact output.
    pub fn admit(drafts: Vec<BoundaryDraft>) -> Result<Self, SchemaError> {
        validation::count(drafts.len(), MAX_BOUNDARIES, "boundary count")?;
        let mut ids = std::collections::BTreeSet::new();
        let mut records: Vec<BoundaryRecord> = Vec::with_capacity(drafts.len());
        for draft in drafts {
            let record = BoundaryRecord::admit(draft)?;
            if let Some(first) = records.first()
                && first.draft().binding != record.draft().binding
            {
                return Err(SchemaError::Invalid("boundary bundle subject mismatch".into()));
            }
            if !ids.insert(record.digest()?) {
                return Err(SchemaError::Invalid("duplicate boundary instance".into()));
            }
            records.push(record);
        }
        let bundle = Self(records);
        bundle.to_json()?;
        Ok(bundle)
    }
    /// Bounded input including whitespace and trailing-data checks.
    pub fn read(reader: impl Read) -> Result<Self, SchemaError> {
        Self::admit(wire::read(reader, MAX_BUNDLE_BYTES)?)
    }
    /// Source order is retained; this schema does not sort or apply boundaries.
    pub fn records(&self) -> &[BoundaryRecord] {
        &self.0
    }
    /// Deterministic bounded full records.
    pub fn to_json(&self) -> Result<Vec<u8>, SchemaError> {
        wire::encode(
            &self.0.iter().map(|record| record.draft()).collect::<Vec<_>>(),
            MAX_BUNDLE_BYTES,
        )
    }
}
