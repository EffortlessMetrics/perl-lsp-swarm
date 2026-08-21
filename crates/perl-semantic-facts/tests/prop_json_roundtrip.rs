//! Property-based tests for JSON round-trip of all fact types.
//!
//! **Property: Fact Record JSON Round-Trip** — For any valid fact record,
//! serializing to JSON then deserializing produces an equal object.
//!
//! Each fact type gets its own `proptest!` block. Strategies are shared
//! where types compose (e.g. `arb_anchor_id()` is reused by every struct
//! that contains an `AnchorId`).

use perl_semantic_facts::{
    AnchorFact, AnchorId, BoundaryDisposition, BoundaryKind, BoundaryLink, Confidence,
    DefinitionCandidate, DefinitionRank, DefinitionRankReason, DiagnosticFact, DiagnosticId,
    EdgeFact, EdgeId, EdgeKind, EntityFact, EntityId, EntityKind, ExportSet, ExportTag, FactId,
    FileId, GeneratedMember, GeneratedMemberKind, ImportKind, ImportSpec, ImportSymbols,
    InvalidationDependency, LifecyclePhase, OccurrenceFact, OccurrenceId, OccurrenceKind,
    PackageEdge, PackageEdgeKind, PackageKind, PackageNode, PlanBlocker, PlanBlockerReason,
    PlanWarning, PlannedEdit, PlannedEditCategory, Provenance, ProviderFactFreshness,
    ProviderFactSourceKind, ProviderFactTrace, ProviderFallbackState, ProviderSurface, RenamePlan,
    SafeDeletePlan, ScopeId, SemanticConfidence, SemanticFactEnvelope, SemanticFactKind,
    SemanticFreshness, SemanticProducer, SemanticProvenance, SemanticReasonCode, SourceAnchor,
    SourceGeneration, ValueShape, VisibleSymbol, VisibleSymbolContext, VisibleSymbolSource,
};
use proptest::prelude::*;

// ── Shared primitive strategies ────────────────────────────────────────────

fn arb_file_id() -> impl Strategy<Value = FileId> {
    any::<u64>().prop_map(FileId)
}

fn arb_entity_id() -> impl Strategy<Value = EntityId> {
    any::<u64>().prop_map(EntityId)
}

fn arb_anchor_id() -> impl Strategy<Value = AnchorId> {
    any::<u64>().prop_map(AnchorId)
}

fn arb_occurrence_id() -> impl Strategy<Value = OccurrenceId> {
    any::<u64>().prop_map(OccurrenceId)
}

fn arb_scope_id() -> impl Strategy<Value = ScopeId> {
    any::<u64>().prop_map(ScopeId)
}

fn arb_edge_id() -> impl Strategy<Value = EdgeId> {
    any::<u64>().prop_map(EdgeId)
}

fn arb_diagnostic_id() -> impl Strategy<Value = DiagnosticId> {
    any::<u64>().prop_map(DiagnosticId)
}

fn arb_provenance() -> impl Strategy<Value = Provenance> {
    prop_oneof![
        Just(Provenance::ExactAst),
        Just(Provenance::DesugaredAst),
        Just(Provenance::SemanticAnalyzer),
        Just(Provenance::FrameworkSynthesis),
        Just(Provenance::ImportExportInference),
        Just(Provenance::PragmaInference),
        Just(Provenance::NameHeuristic),
        Just(Provenance::SearchFallback),
        Just(Provenance::DynamicBoundary),
        Just(Provenance::LiteralRequireImport),
    ]
}

fn arb_confidence() -> impl Strategy<Value = Confidence> {
    prop_oneof![Just(Confidence::High), Just(Confidence::Medium), Just(Confidence::Low),]
}

fn arb_occurrence_kind() -> impl Strategy<Value = OccurrenceKind> {
    prop_oneof![
        Just(OccurrenceKind::Definition),
        Just(OccurrenceKind::Reference),
        Just(OccurrenceKind::Read),
        Just(OccurrenceKind::Write),
        Just(OccurrenceKind::Call),
        Just(OccurrenceKind::MethodCall),
        Just(OccurrenceKind::StaticMethodCall),
        Just(OccurrenceKind::CoderefReference),
        Just(OccurrenceKind::TypeglobReference),
        Just(OccurrenceKind::Import),
        Just(OccurrenceKind::Export),
        Just(OccurrenceKind::Inheritance),
        Just(OccurrenceKind::RoleComposition),
        Just(OccurrenceKind::GeneratedUse),
        Just(OccurrenceKind::DynamicBoundary),
    ]
}

fn arb_entity_kind() -> impl Strategy<Value = EntityKind> {
    prop_oneof![
        Just(EntityKind::Package),
        Just(EntityKind::Class),
        Just(EntityKind::Role),
        Just(EntityKind::Subroutine),
        Just(EntityKind::Method),
        Just(EntityKind::Variable),
        Just(EntityKind::Constant),
        Just(EntityKind::Field),
        Just(EntityKind::Label),
        Just(EntityKind::Format),
        Just(EntityKind::Module),
        Just(EntityKind::GeneratedMember),
        Just(EntityKind::ExternalSymbol),
        Just(EntityKind::Unknown),
    ]
}

fn arb_edge_kind() -> impl Strategy<Value = EdgeKind> {
    prop_oneof![
        Just(EdgeKind::Defines),
        Just(EdgeKind::References),
        Just(EdgeKind::Reads),
        Just(EdgeKind::Writes),
        Just(EdgeKind::Calls),
        Just(EdgeKind::ImportsModule),
        Just(EdgeKind::ImportsSymbol),
        Just(EdgeKind::ExportsSymbol),
        Just(EdgeKind::ExportsGroup),
        Just(EdgeKind::Inherits),
        Just(EdgeKind::ComposesRole),
        Just(EdgeKind::MemberOf),
        Just(EdgeKind::GeneratedFrom),
        Just(EdgeKind::AliasOf),
        Just(EdgeKind::DependsOn),
        Just(EdgeKind::DynamicBoundary),
    ]
}

fn arb_identifier() -> impl Strategy<Value = String> {
    "[A-Za-z_][A-Za-z0-9_]{0,12}"
}

fn arb_qualified_name() -> impl Strategy<Value = String> {
    prop::collection::vec(arb_identifier(), 1..=3).prop_map(|segments| segments.join("::"))
}

fn arb_symbol_key() -> impl Strategy<Value = String> {
    prop_oneof![
        arb_identifier(),
        arb_qualified_name(),
        arb_identifier().prop_map(|name| format!("${name}")),
        arb_identifier().prop_map(|name| format!("@{name}")),
        arb_identifier().prop_map(|name| format!("%{name}")),
    ]
}

fn arb_opt_string() -> impl Strategy<Value = Option<String>> {
    prop::option::of("[A-Za-z][A-Za-z0-9_:]{0,20}".prop_map(String::from))
}

fn arb_opt_anchor_id() -> impl Strategy<Value = Option<AnchorId>> {
    prop::option::of(arb_anchor_id())
}

fn arb_opt_scope_id() -> impl Strategy<Value = Option<ScopeId>> {
    prop::option::of(arb_scope_id())
}

fn arb_opt_file_id() -> impl Strategy<Value = Option<FileId>> {
    prop::option::of(arb_file_id())
}

fn arb_opt_entity_id() -> impl Strategy<Value = Option<EntityId>> {
    prop::option::of(arb_entity_id())
}

fn arb_fact_id() -> impl Strategy<Value = FactId> {
    any::<u64>().prop_map(FactId)
}

fn arb_source_generation() -> impl Strategy<Value = SourceGeneration> {
    prop_oneof![
        "[A-Za-z0-9_-]{1,20}".prop_map(SourceGeneration::known),
        Just(SourceGeneration::Unknown),
    ]
}

fn arb_semantic_fact_kind() -> impl Strategy<Value = SemanticFactKind> {
    prop_oneof![
        Just(SemanticFactKind::Declaration),
        Just(SemanticFactKind::Occurrence),
        Just(SemanticFactKind::Import),
        Just(SemanticFactKind::Module),
        Just(SemanticFactKind::Boundary),
    ]
}

fn arb_lifecycle_phase() -> impl Strategy<Value = LifecyclePhase> {
    prop_oneof![
        Just(LifecyclePhase::Begin),
        Just(LifecyclePhase::UnitCheck),
        Just(LifecyclePhase::Check),
        Just(LifecyclePhase::Init),
        Just(LifecyclePhase::End),
        Just(LifecyclePhase::Runtime),
        Just(LifecyclePhase::Unknown),
    ]
}

fn arb_semantic_producer() -> impl Strategy<Value = SemanticProducer> {
    prop_oneof![
        Just(SemanticProducer::Parser),
        Just(SemanticProducer::Hir),
        Just(SemanticProducer::PirA),
        Just(SemanticProducer::SemanticAnalyzer),
        Just(SemanticProducer::WorkspaceIndex),
        Just(SemanticProducer::FrameworkAdapter),
        Just(SemanticProducer::Unknown),
    ]
}

fn arb_semantic_freshness() -> impl Strategy<Value = SemanticFreshness> {
    prop_oneof![
        Just(SemanticFreshness::Fresh),
        Just(SemanticFreshness::Stale),
        Just(SemanticFreshness::Unknown),
        Just(SemanticFreshness::NotApplicable),
    ]
}

fn arb_semantic_provenance() -> impl Strategy<Value = SemanticProvenance> {
    prop_oneof![
        arb_provenance().prop_map(SemanticProvenance::Known),
        Just(SemanticProvenance::Unknown)
    ]
}

fn arb_semantic_confidence() -> impl Strategy<Value = SemanticConfidence> {
    prop_oneof![
        arb_confidence().prop_map(SemanticConfidence::Known),
        Just(SemanticConfidence::Unknown)
    ]
}

fn arb_boundary_link() -> impl Strategy<Value = Option<BoundaryLink>> {
    prop::option::of(
        (
            prop::option::of(arb_fact_id()),
            prop_oneof![
                Just(BoundaryKind::DynamicValue),
                Just(BoundaryKind::DynamicRequire),
                Just(BoundaryKind::DynamicIncludePath),
                Just(BoundaryKind::CompileTimeExecution),
                Just(BoundaryKind::SymbolicReference),
                Just(BoundaryKind::Compatibility),
                Just(BoundaryKind::ExternalEnvironment),
                Just(BoundaryKind::Unsupported),
            ],
            prop_oneof![Just(BoundaryDisposition::Degrade), Just(BoundaryDisposition::Refuse)],
            prop_oneof![
                Just(SemanticReasonCode::ExactSource),
                Just(SemanticReasonCode::GeneratedFromSource),
                Just(SemanticReasonCode::DynamicValue),
                Just(SemanticReasonCode::CompatibilityBoundary),
                Just(SemanticReasonCode::UnsupportedEffect),
                Just(SemanticReasonCode::MissingGeneration),
                Just(SemanticReasonCode::UnknownProvenance),
                Just(SemanticReasonCode::UnknownConfidence),
                Just(SemanticReasonCode::UnknownLifecycle),
                Just(SemanticReasonCode::StaleDependency),
                Just(SemanticReasonCode::Unknown),
            ],
        )
            .prop_map(|(boundary_id, kind, disposition, reason_code)| {
                BoundaryLink::new(boundary_id, kind, disposition, reason_code)
            }),
    )
}

fn arb_source_anchor() -> impl Strategy<Value = SourceAnchor> {
    (prop::option::of(arb_anchor_id()), arb_file_id(), 0u32..=10_000, 0u32..=100).prop_map(
        |(anchor_id, file_id, start_byte, length)| {
            SourceAnchor::new(anchor_id, file_id, start_byte, start_byte + length)
        },
    )
}

fn arb_semantic_fact_envelope() -> impl Strategy<Value = SemanticFactEnvelope> {
    (
        (
            arb_fact_id(),
            arb_opt_entity_id(),
            arb_semantic_fact_kind(),
            arb_source_anchor(),
            arb_source_generation(),
            arb_opt_scope_id(),
            arb_opt_string(),
        ),
        (
            arb_lifecycle_phase(),
            arb_semantic_producer(),
            arb_semantic_provenance(),
            arb_semantic_confidence(),
            arb_semantic_freshness(),
            arb_boundary_link(),
        ),
        (
            prop::collection::vec(
                (arb_identifier(), arb_source_generation())
                    .prop_map(|(key, generation)| InvalidationDependency::new(key, generation)),
                0..=3,
            ),
            prop_oneof![
                Just(SemanticReasonCode::ExactSource),
                Just(SemanticReasonCode::GeneratedFromSource),
                Just(SemanticReasonCode::DynamicValue),
                Just(SemanticReasonCode::CompatibilityBoundary),
                Just(SemanticReasonCode::UnsupportedEffect),
                Just(SemanticReasonCode::MissingGeneration),
                Just(SemanticReasonCode::UnknownProvenance),
                Just(SemanticReasonCode::UnknownConfidence),
                Just(SemanticReasonCode::UnknownLifecycle),
                Just(SemanticReasonCode::StaleDependency),
                Just(SemanticReasonCode::Unknown),
            ],
        ),
    )
        .prop_map(
            |(
                (fact_id, entity_id, kind, anchor, source_generation, scope_id, package),
                (lifecycle, producer, provenance, confidence, freshness, boundary),
                (dependencies, reason_code),
            )| {
                SemanticFactEnvelope::new(
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
                    dependencies,
                    reason_code,
                )
            },
        )
}

// ── Strategies: primitive fact types ──────────────────────────────────────

fn arb_reference_edge() -> impl Strategy<Value = perl_semantic_facts::ReferenceEdge> {
    (
        arb_occurrence_id(),
        arb_anchor_id(),
        arb_file_id(),
        arb_symbol_key(),
        prop::collection::vec(arb_entity_id(), 0..5),
        arb_occurrence_kind(),
        arb_provenance(),
        arb_confidence(),
    )
        .prop_map(
            |(
                occurrence_id,
                anchor_id,
                file_id,
                symbol_key,
                target_candidates,
                kind,
                provenance,
                confidence,
            )| {
                perl_semantic_facts::ReferenceEdge::new(
                    occurrence_id,
                    anchor_id,
                    file_id,
                    symbol_key,
                    target_candidates,
                    kind,
                    provenance,
                    confidence,
                )
            },
        )
}

fn arb_definition_rank() -> impl Strategy<Value = DefinitionRank> {
    prop_oneof![
        Just(DefinitionRank::ExactQualified),
        Just(DefinitionRank::SamePackage),
        Just(DefinitionRank::ExplicitImport),
        Just(DefinitionRank::DefaultExport),
        Just(DefinitionRank::WorkspaceCandidate),
        Just(DefinitionRank::Heuristic),
    ]
}

fn arb_definition_rank_reason() -> impl Strategy<Value = DefinitionRankReason> {
    prop_oneof![
        Just(DefinitionRankReason::ExactQualifiedName),
        Just(DefinitionRankReason::SamePackage),
        arb_qualified_name().prop_map(|module| DefinitionRankReason::ExplicitImport { module }),
        arb_qualified_name().prop_map(|module| DefinitionRankReason::DefaultExport { module }),
        Just(DefinitionRankReason::WorkspaceSymbol),
        Just(DefinitionRankReason::HeuristicNameMatch),
    ]
}

fn arb_definition_candidate() -> impl Strategy<Value = DefinitionCandidate> {
    (
        arb_entity_id(),
        arb_anchor_id(),
        arb_qualified_name(),
        arb_identifier(),
        prop::option::of(arb_qualified_name()),
        arb_entity_kind(),
        arb_provenance(),
        arb_confidence(),
        arb_definition_rank(),
        arb_definition_rank_reason(),
    )
        .prop_map(
            |(
                entity_id,
                anchor_id,
                canonical_name,
                display_name,
                package,
                kind,
                provenance,
                confidence,
                rank,
                rank_reason,
            )| {
                DefinitionCandidate::new(
                    entity_id,
                    anchor_id,
                    canonical_name,
                    display_name,
                    package,
                    kind,
                    provenance,
                    confidence,
                    rank,
                    rank_reason,
                )
            },
        )
}

// ── Strategies: anchor, entity, occurrence, edge, diagnostic facts ─────────

fn arb_anchor_fact() -> impl Strategy<Value = AnchorFact> {
    (
        arb_anchor_id(),
        arb_file_id(),
        0u32..100_000u32,
        arb_opt_scope_id(),
        arb_provenance(),
        arb_confidence(),
    )
        .prop_map(|(id, file_id, start, scope_id, provenance, confidence)| AnchorFact {
            id,
            file_id,
            span_start_byte: start,
            span_end_byte: start + 20,
            scope_id,
            provenance,
            confidence,
        })
}

fn arb_entity_fact() -> impl Strategy<Value = EntityFact> {
    (
        arb_entity_id(),
        arb_entity_kind(),
        arb_qualified_name(),
        arb_opt_anchor_id(),
        arb_opt_scope_id(),
        arb_provenance(),
        arb_confidence(),
    )
        .prop_map(|(id, kind, canonical_name, anchor_id, scope_id, provenance, confidence)| {
            EntityFact { id, kind, canonical_name, anchor_id, scope_id, provenance, confidence }
        })
}

fn arb_occurrence_fact() -> impl Strategy<Value = OccurrenceFact> {
    (
        arb_occurrence_id(),
        arb_occurrence_kind(),
        arb_opt_entity_id(),
        arb_anchor_id(),
        arb_opt_scope_id(),
        arb_provenance(),
        arb_confidence(),
    )
        .prop_map(|(id, kind, entity_id, anchor_id, scope_id, provenance, confidence)| {
            OccurrenceFact { id, kind, entity_id, anchor_id, scope_id, provenance, confidence }
        })
}

fn arb_edge_fact() -> impl Strategy<Value = EdgeFact> {
    (
        arb_edge_id(),
        arb_edge_kind(),
        arb_entity_id(),
        arb_entity_id(),
        prop::option::of(arb_occurrence_id()),
        arb_provenance(),
        arb_confidence(),
    )
        .prop_map(
            |(
                id,
                kind,
                from_entity_id,
                to_entity_id,
                via_occurrence_id,
                provenance,
                confidence,
            )| EdgeFact {
                id,
                kind,
                from_entity_id,
                to_entity_id,
                via_occurrence_id,
                provenance,
                confidence,
            },
        )
}

fn arb_diagnostic_fact() -> impl Strategy<Value = DiagnosticFact> {
    (
        arb_diagnostic_id(),
        arb_opt_string(),
        "[A-Za-z ]{1,40}".prop_map(String::from),
        arb_anchor_id(),
        prop::collection::vec(arb_anchor_id(), 0..3),
        arb_opt_scope_id(),
        arb_provenance(),
        arb_confidence(),
    )
        .prop_map(
            |(
                id,
                code,
                message,
                primary_anchor_id,
                related_anchor_ids,
                scope_id,
                provenance,
                confidence,
            )| DiagnosticFact {
                id,
                code,
                message,
                primary_anchor_id,
                related_anchor_ids,
                scope_id,
                provenance,
                confidence,
            },
        )
}

// ── Strategies: export and import types ───────────────────────────────────

fn arb_export_tag() -> impl Strategy<Value = ExportTag> {
    (arb_identifier(), prop::collection::vec(arb_identifier(), 0..5))
        .prop_map(|(name, members)| ExportTag { name, members })
}

fn arb_export_set() -> impl Strategy<Value = ExportSet> {
    (
        prop::collection::vec(arb_identifier(), 0..4),
        prop::collection::vec(arb_identifier(), 0..4),
        prop::collection::vec(arb_export_tag(), 0..3),
        arb_provenance(),
        arb_confidence(),
        prop::option::of(arb_qualified_name()),
        arb_opt_anchor_id(),
    )
        .prop_map(
            |(
                default_exports,
                optional_exports,
                tags,
                provenance,
                confidence,
                module_name,
                anchor_id,
            )| ExportSet {
                default_exports,
                optional_exports,
                tags,
                provenance,
                confidence,
                module_name,
                anchor_id,
            },
        )
}

fn arb_import_kind() -> impl Strategy<Value = ImportKind> {
    prop_oneof![
        Just(ImportKind::Use),
        Just(ImportKind::UseEmpty),
        Just(ImportKind::UseExplicitList),
        Just(ImportKind::UseTag),
        Just(ImportKind::Require),
        Just(ImportKind::RequireThenImport),
        Just(ImportKind::UseConstant),
        Just(ImportKind::DynamicRequire),
        Just(ImportKind::ManualImport),
    ]
}

fn arb_import_symbols() -> impl Strategy<Value = ImportSymbols> {
    prop_oneof![
        Just(ImportSymbols::Default),
        Just(ImportSymbols::None),
        prop::collection::vec(arb_identifier(), 0..5).prop_map(ImportSymbols::Explicit),
        prop::collection::vec(arb_identifier(), 0..4).prop_map(ImportSymbols::Tags),
        (
            prop::collection::vec(arb_identifier(), 0..3),
            prop::collection::vec(arb_identifier(), 0..3),
        )
            .prop_map(|(tags, names)| ImportSymbols::Mixed { tags, names }),
        Just(ImportSymbols::Dynamic),
    ]
}

fn arb_import_spec() -> impl Strategy<Value = ImportSpec> {
    (
        arb_qualified_name(),
        arb_import_kind(),
        arb_import_symbols(),
        arb_provenance(),
        arb_confidence(),
        arb_opt_file_id(),
        arb_opt_anchor_id(),
        arb_opt_scope_id(),
        prop::option::of(any::<u32>()),
    )
        .prop_map(
            |(
                module,
                kind,
                symbols,
                provenance,
                confidence,
                file_id,
                anchor_id,
                scope_id,
                span_start_byte,
            )| ImportSpec {
                module,
                kind,
                symbols,
                provenance,
                confidence,
                file_id,
                anchor_id,
                scope_id,
                span_start_byte,
            },
        )
}

// ── Strategies: visible symbol ─────────────────────────────────────────────

fn arb_visible_symbol_source() -> impl Strategy<Value = VisibleSymbolSource> {
    prop_oneof![
        Just(VisibleSymbolSource::LocalLexical),
        Just(VisibleSymbolSource::LocalPackage),
        Just(VisibleSymbolSource::ExplicitImport),
        Just(VisibleSymbolSource::DefaultExport),
        Just(VisibleSymbolSource::ExportTag),
        Just(VisibleSymbolSource::Constant),
        Just(VisibleSymbolSource::Generated),
        Just(VisibleSymbolSource::External),
        Just(VisibleSymbolSource::DynamicUnknown),
    ]
}

fn arb_visible_symbol_context() -> impl Strategy<Value = VisibleSymbolContext> {
    (prop::option::of(arb_qualified_name()), arb_opt_anchor_id(), arb_opt_anchor_id()).prop_map(
        |(source_module, source_import_anchor_id, source_export_anchor_id)| {
            VisibleSymbolContext::new(
                source_module,
                source_import_anchor_id,
                source_export_anchor_id,
            )
        },
    )
}

fn arb_visible_symbol() -> impl Strategy<Value = VisibleSymbol> {
    (
        arb_symbol_key(),
        arb_opt_entity_id(),
        arb_visible_symbol_source(),
        arb_confidence(),
        prop::option::of(arb_visible_symbol_context()),
    )
        .prop_map(|(name, entity_id, source, confidence, context)| VisibleSymbol {
            name,
            entity_id,
            source,
            confidence,
            context,
        })
}

// ── Strategies: provider fact tracing ─────────────────────────────────────

fn arb_provider_surface() -> impl Strategy<Value = ProviderSurface> {
    prop_oneof![
        Just(ProviderSurface::Diagnostics),
        Just(ProviderSurface::Completion),
        Just(ProviderSurface::Hover),
        Just(ProviderSurface::Definition),
        Just(ProviderSurface::References),
        Just(ProviderSurface::Rename),
        Just(ProviderSurface::SafeDelete),
        Just(ProviderSurface::WorkspaceSymbols),
        Just(ProviderSurface::DocumentSymbols),
        Just(ProviderSurface::SemanticTokens),
    ]
}

fn arb_provider_fact_source_kind() -> impl Strategy<Value = ProviderFactSourceKind> {
    prop_oneof![
        Just(ProviderFactSourceKind::ParserSyntax),
        Just(ProviderFactSourceKind::LegacyWorkspace),
        Just(ProviderFactSourceKind::SemanticFact),
        Just(ProviderFactSourceKind::CompilerFact),
        Just(ProviderFactSourceKind::FrameworkAdapter),
        Just(ProviderFactSourceKind::DynamicBoundary),
        Just(ProviderFactSourceKind::Fallback),
        Just(ProviderFactSourceKind::Unknown),
    ]
}

fn arb_provider_fact_freshness() -> impl Strategy<Value = ProviderFactFreshness> {
    prop_oneof![
        Just(ProviderFactFreshness::Fresh),
        Just(ProviderFactFreshness::Stale),
        Just(ProviderFactFreshness::Unknown),
        Just(ProviderFactFreshness::NotApplicable),
    ]
}

fn arb_provider_fallback_state() -> impl Strategy<Value = ProviderFallbackState> {
    prop_oneof![
        Just(ProviderFallbackState::Primary),
        Just(ProviderFallbackState::Shadow),
        Just(ProviderFallbackState::Fallback),
        Just(ProviderFallbackState::Unavailable),
        Just(ProviderFallbackState::Blocked),
    ]
}

fn arb_provider_fact_trace() -> impl Strategy<Value = ProviderFactTrace> {
    (
        arb_provider_surface(),
        arb_provider_fact_source_kind(),
        arb_provenance(),
        arb_confidence(),
        arb_provider_fact_freshness(),
        arb_provider_fallback_state(),
        prop::option::of("[A-Fa-f0-9]{8,40}".prop_map(String::from)),
        arb_opt_anchor_id(),
        prop::option::of(any::<u32>()),
    )
        .prop_map(
            |(
                surface,
                source,
                provenance,
                confidence,
                freshness,
                fallback_state,
                source_hash,
                anchor_id,
                model_version,
            )| {
                ProviderFactTrace::new(
                    surface,
                    source,
                    provenance,
                    confidence,
                    freshness,
                    fallback_state,
                    source_hash,
                    anchor_id,
                    model_version,
                )
            },
        )
}

// ── Strategies: rename and safe-delete plans ──────────────────────────────

fn arb_plan_blocker_reason() -> impl Strategy<Value = PlanBlockerReason> {
    prop_oneof![
        Just(PlanBlockerReason::DynamicBoundary),
        Just(PlanBlockerReason::AmbiguousReference),
        Just(PlanBlockerReason::CrossModuleExport),
        Just(PlanBlockerReason::ImportedSymbol),
        Just(PlanBlockerReason::ExportedSymbol),
        Just(PlanBlockerReason::ReferencesExist),
        Just(PlanBlockerReason::GeneratedMember),
        Just(PlanBlockerReason::StaleFact),
        Just(PlanBlockerReason::UnclassifiedOccurrence),
    ]
}

fn arb_plan_blocker() -> impl Strategy<Value = PlanBlocker> {
    (arb_plan_blocker_reason(), arb_opt_anchor_id(), "[A-Za-z ]{1,30}".prop_map(String::from))
        .prop_map(|(reason, anchor_id, description)| {
            PlanBlocker::new(reason, anchor_id, description)
        })
}

fn arb_plan_warning() -> impl Strategy<Value = PlanWarning> {
    ("[A-Za-z ]{1,30}".prop_map(String::from), arb_opt_anchor_id())
        .prop_map(|(message, anchor_id)| PlanWarning::new(message, anchor_id))
}

fn arb_planned_edit_category() -> impl Strategy<Value = PlannedEditCategory> {
    prop_oneof![
        Just(PlannedEditCategory::Definition),
        Just(PlannedEditCategory::Reference),
        Just(PlannedEditCategory::ImportList),
        Just(PlannedEditCategory::ExportList),
    ]
}

fn arb_planned_edit() -> impl Strategy<Value = PlannedEdit> {
    (
        arb_anchor_id(),
        arb_file_id(),
        arb_planned_edit_category(),
        arb_identifier(),
        arb_identifier(),
    )
        .prop_map(|(anchor_id, file_id, category, old_text, new_text)| {
            PlannedEdit::new(anchor_id, file_id, category, old_text, new_text)
        })
}

fn arb_rename_plan() -> impl Strategy<Value = RenamePlan> {
    (
        arb_entity_id(),
        arb_identifier(),
        arb_identifier(),
        prop::collection::vec(arb_planned_edit(), 0..4),
        prop::collection::vec(arb_plan_blocker(), 0..3),
        prop::collection::vec(arb_plan_warning(), 0..3),
    )
        .prop_map(|(entity_id, old_name, new_name, edits, blockers, warnings)| {
            RenamePlan::new(entity_id, old_name, new_name, edits, blockers, warnings)
        })
}

fn arb_safe_delete_plan() -> impl Strategy<Value = SafeDeletePlan> {
    (
        arb_entity_id(),
        arb_identifier(),
        prop::collection::vec(arb_plan_blocker(), 0..3),
        prop::collection::vec(arb_plan_warning(), 0..3),
    )
        .prop_map(|(entity_id, name, blockers, warnings)| {
            SafeDeletePlan::new(entity_id, name, blockers, warnings)
        })
}

// ── Strategies: package graph types ───────────────────────────────────────

fn arb_package_kind() -> impl Strategy<Value = PackageKind> {
    prop_oneof![
        Just(PackageKind::Package),
        Just(PackageKind::Class),
        Just(PackageKind::Role),
        Just(PackageKind::External),
    ]
}

fn arb_package_edge_kind() -> impl Strategy<Value = PackageEdgeKind> {
    prop_oneof![
        Just(PackageEdgeKind::Inherits),
        Just(PackageEdgeKind::ComposesRole),
        Just(PackageEdgeKind::DependsOn),
    ]
}

fn arb_package_node() -> impl Strategy<Value = PackageNode> {
    (
        arb_entity_id(),
        arb_qualified_name(),
        arb_package_kind(),
        arb_opt_anchor_id(),
        arb_opt_file_id(),
    )
        .prop_map(|(entity_id, name, kind, anchor_id, file_id)| {
            PackageNode::new(entity_id, name, kind, anchor_id, file_id)
        })
}

fn arb_package_edge() -> impl Strategy<Value = PackageEdge> {
    (
        arb_qualified_name(),
        arb_qualified_name(),
        arb_package_edge_kind(),
        arb_opt_anchor_id(),
        arb_provenance(),
        arb_confidence(),
    )
        .prop_map(|(from_package, to_package, kind, anchor_id, provenance, confidence)| {
            PackageEdge::new(from_package, to_package, kind, anchor_id, provenance, confidence)
        })
}

// ── Strategies: generated member ──────────────────────────────────────────

fn arb_generated_member_kind() -> impl Strategy<Value = GeneratedMemberKind> {
    prop_oneof![
        Just(GeneratedMemberKind::Getter),
        Just(GeneratedMemberKind::Setter),
        Just(GeneratedMemberKind::Accessor),
        Just(GeneratedMemberKind::Predicate),
        Just(GeneratedMemberKind::Clearer),
        Just(GeneratedMemberKind::Builder),
        Just(GeneratedMemberKind::Constant),
    ]
}

fn arb_generated_member() -> impl Strategy<Value = GeneratedMember> {
    (
        arb_entity_id(),
        arb_identifier(),
        arb_generated_member_kind(),
        arb_anchor_id(),
        arb_qualified_name(),
        arb_provenance(),
        arb_confidence(),
    )
        .prop_map(
            |(entity_id, name, kind, source_anchor_id, package, provenance, confidence)| {
                GeneratedMember::new(
                    entity_id,
                    name,
                    kind,
                    source_anchor_id,
                    package,
                    provenance,
                    confidence,
                )
            },
        )
}

// ── Strategies: value shape ────────────────────────────────────────────────

fn arb_value_shape() -> impl Strategy<Value = ValueShape> {
    prop_oneof![
        Just(ValueShape::Unknown),
        Just(ValueShape::Scalar),
        Just(ValueShape::ArrayRef),
        Just(ValueShape::HashRef),
        Just(ValueShape::CodeRef),
        arb_qualified_name().prop_map(|package| ValueShape::PackageName { package }),
        (arb_qualified_name(), arb_confidence())
            .prop_map(|(package, confidence)| ValueShape::Object { package, confidence }),
    ]
}

// ── Framework adapter SDK strategies ──────────────────────────────────────

use perl_semantic_facts::framework::{
    AdapterBudget, AdapterCancellation, AdapterDescriptor, AdapterDetectionInput,
    AdapterDetectionResult, AdapterDisposition, AdapterId, AdapterInput, AdapterOutcome,
    AdapterResult, AdapterSourceScope, DetectionAbsenceReason, DetectionAuthorityError,
    DetectionAuthorityReceipt, DetectionConfigurationEvidence, DetectionConfigurationObservation,
    DetectionConfigurationValue, DetectionEvidenceClass, DetectionInputIdentity, DetectionOutcome,
    EmittedFact, FactClass, FactLimitation, FactSink, FactSinkId, ModuleActivationIdentity,
    ModuleObservationReceipt, ModuleSelectorEvaluation, ModuleSelectorOutcome,
    ModuleVersionEvidence, UnavailableReason,
};

fn arb_adapter_id() -> impl Strategy<Value = AdapterId> {
    any::<u64>().prop_map(AdapterId)
}

fn arb_fact_sink_id() -> impl Strategy<Value = FactSinkId> {
    any::<u64>().prop_map(FactSinkId)
}

fn arb_adapter_disposition() -> impl Strategy<Value = AdapterDisposition> {
    prop_oneof![
        Just(AdapterDisposition::Production),
        Just(AdapterDisposition::Shadow),
        Just(AdapterDisposition::Experimental),
    ]
}

fn arb_detection_configuration_value() -> impl Strategy<Value = DetectionConfigurationValue> {
    prop_oneof![
        any::<bool>().prop_map(DetectionConfigurationValue::Boolean),
        any::<i64>().prop_map(DetectionConfigurationValue::Integer),
        "[A-Za-z][A-Za-z0-9_:-]{0,20}".prop_map(DetectionConfigurationValue::String),
    ]
}

fn arb_detection_configuration_observation()
-> impl Strategy<Value = DetectionConfigurationObservation> {
    (
        "[a-z][a-z0-9_.:-]{2,20}",
        "sha256:[a-f0-9]{8}",
        "[a-z][a-z0-9_.:-]{2,30}",
        arb_detection_configuration_value(),
        "[a-z][a-z0-9_.:-]{2,20}",
        arb_source_generation(),
        "[a-z][a-z0-9_.:-]{2,20}",
        "[a-z][a-z0-9_.:-]{2,20}",
    )
        .prop_map(
            |(
                source_identity,
                source_digest,
                key,
                value,
                scope_identity,
                generation,
                provenance,
                policy_identity,
            )| {
                DetectionConfigurationObservation::new(
                    source_identity,
                    source_digest,
                    key,
                    value,
                    scope_identity,
                    generation,
                    provenance,
                    policy_identity,
                )
            },
        )
}

fn arb_detection_configuration_evidence() -> impl Strategy<Value = DetectionConfigurationEvidence> {
    (
        arb_detection_configuration_observation(),
        arb_detection_configuration_value(),
        "[a-z][a-z0-9_.:-]{2,20}",
    )
        .prop_map(|(observation, excluding_value, rule_identity)| {
            DetectionConfigurationEvidence::new(observation, excluding_value, rule_identity)
        })
}

fn arb_adapter_descriptor() -> impl Strategy<Value = AdapterDescriptor> {
    (
        (
            arb_adapter_id(),
            "[a-z]{3,10}",
            "[A-Z][a-z]{2,8}",
            proptest::option::of("[0-9]+\\.[0-9]+"),
            1u32..=5u32,
            arb_adapter_disposition(),
        ),
        proptest::option::of((
            "[a-z][a-z0-9_.:-]{2,30}",
            arb_detection_configuration_value(),
            "[a-z][a-z0-9_.:-]{2,30}",
        )),
    )
        .prop_map(|((id, name, fw, constraint, schema_version, disposition), exclusion)| {
            let descriptor =
                AdapterDescriptor::new(id, name, fw, constraint, schema_version, disposition);
            match exclusion {
                Some((key, value, rule)) => {
                    descriptor.with_configuration_exclusion(key, value, rule)
                }
                None => descriptor,
            }
        })
}

fn arb_module_version_evidence() -> impl Strategy<Value = ModuleVersionEvidence> {
    ("[0-9]+\\.[0-9]+(\\.[0-9]+)?", arb_source_generation())
        .prop_map(|(version, generation)| ModuleVersionEvidence::new(version, generation))
}

fn arb_module_activation_identity() -> impl Strategy<Value = ModuleActivationIdentity> {
    (
        "[A-Z][a-z]{2,8}((::[A-Z][a-z]{2,6})?)",
        proptest::option::of(arb_file_id()),
        arb_source_generation(),
        proptest::option::of(arb_module_version_evidence()),
    )
        .prop_map(|(module_name, file_id, generation, observed)| {
            let identity = ModuleActivationIdentity::new(module_name, file_id, generation);
            match observed {
                Some(evidence) => identity.with_observed_version(evidence),
                None => identity,
            }
        })
}

fn arb_adapter_cancellation() -> impl Strategy<Value = AdapterCancellation> {
    any::<bool>().prop_map(|c| {
        if c { AdapterCancellation::cancelled() } else { AdapterCancellation::active() }
    })
}

fn arb_adapter_budget() -> impl Strategy<Value = AdapterBudget> {
    (1u32..=1000u32, 1u64..=1_048_576u64)
        .prop_map(|(facts, bytes)| AdapterBudget::new(facts, bytes))
}

fn arb_detection_evidence_class() -> impl Strategy<Value = DetectionEvidenceClass> {
    prop_oneof![
        Just(DetectionEvidenceClass::ResolvedModule),
        Just(DetectionEvidenceClass::ResolvedImport),
        Just(DetectionEvidenceClass::ProbableImport),
        Just(DetectionEvidenceClass::NameOnly),
    ]
}

fn arb_module_selector() -> impl Strategy<Value = String> {
    "[A-Z][a-z]{2,8}((::[A-Z][a-z]{2,6})?)".prop_map(String::from)
}

fn arb_module_selector_outcome() -> impl Strategy<Value = ModuleSelectorOutcome> {
    prop_oneof![
        (arb_module_activation_identity(), arb_detection_evidence_class()).prop_map(
            |(activation, evidence_class)| ModuleSelectorOutcome::Matched {
                activation,
                evidence_class,
            }
        ),
        Just(ModuleSelectorOutcome::Absent),
        "[a-z ]{4,20}".prop_map(|reason| ModuleSelectorOutcome::Unresolved { reason }),
        "[a-z ]{4,20}".prop_map(|reason| ModuleSelectorOutcome::Ambiguous { reason }),
        "[a-z ]{4,20}".prop_map(|reason| ModuleSelectorOutcome::Unavailable { reason }),
    ]
}

fn arb_module_selector_evaluation() -> impl Strategy<Value = ModuleSelectorEvaluation> {
    (arb_module_selector(), arb_module_selector_outcome())
        .prop_map(|(selector, outcome)| ModuleSelectorEvaluation::new(selector, outcome))
}

fn arb_module_observation_receipt() -> impl Strategy<Value = ModuleObservationReceipt> {
    (
        arb_source_generation(),
        "[a-f0-9]{8}".prop_map(|s| format!("digest:{s}")),
        proptest::collection::vec(arb_module_selector_evaluation(), 0..5),
    )
        .prop_map(|(generation, content_digest, evaluations)| {
            ModuleObservationReceipt::new(
                "module-resolver.v1",
                "root:prop-fixture",
                "project-environment.v1",
                generation,
                content_digest,
                evaluations,
            )
        })
}

fn arb_adapter_detection_input() -> impl Strategy<Value = AdapterDetectionInput> {
    (
        arb_adapter_descriptor(),
        arb_module_observation_receipt(),
        proptest::collection::vec(arb_detection_configuration_observation(), 0..5),
        "[a-z][a-z0-9_.:-]{2,20}",
        proptest::option::of(arb_adapter_budget()),
        arb_adapter_cancellation(),
    )
        .prop_map(|(descriptor, observation, configurations, policy, budget, cancel)| {
            AdapterDetectionInput::new(descriptor, observation, budget, cancel)
                .with_configuration_observations(configurations)
                .with_detector_policy_identity(policy)
        })
}

fn arb_detection_input_identity() -> impl Strategy<Value = DetectionInputIdentity> {
    (
        arb_adapter_descriptor(),
        arb_module_observation_receipt(),
        proptest::collection::vec(arb_detection_configuration_observation(), 0..5),
        "[a-z][a-z0-9_.:-]{2,20}",
    )
        .prop_map(|(descriptor, module_observation, configuration_observations, policy)| {
            AdapterDetectionInput::new(
                descriptor,
                module_observation,
                None,
                AdapterCancellation::active(),
            )
            .with_configuration_observations(configuration_observations)
            .with_detector_policy_identity(policy)
            .identity()
        })
}

fn arb_detection_absence_reason() -> impl Strategy<Value = DetectionAbsenceReason> {
    prop_oneof![
        Just(DetectionAbsenceReason::RequiredModulesMissing),
        Just(DetectionAbsenceReason::VersionConstraintNotSatisfied),
        Just(DetectionAbsenceReason::ExcludedByConfiguration),
    ]
}

fn arb_unavailable_reason() -> impl Strategy<Value = UnavailableReason> {
    prop_oneof![
        Just(UnavailableReason::MissingGeneration),
        Just(UnavailableReason::NoModulesAvailable),
        Just(UnavailableReason::InternalError),
    ]
}

fn arb_detection_outcome() -> impl Strategy<Value = DetectionOutcome> {
    prop_oneof![
        (arb_confidence(), proptest::option::of("[0-9]+\\.[0-9]+")).prop_map(
            |(confidence, version)| DetectionOutcome::Detected {
                confidence,
                framework_version: version,
            }
        ),
        arb_detection_absence_reason().prop_map(|reason| DetectionOutcome::Absent { reason }),
        proptest::collection::vec("[a-z ]{4,20}", 1..3)
            .prop_map(|descs| { DetectionOutcome::Conflicting { conflict_descriptions: descs } }),
        arb_unavailable_reason().prop_map(|reason| DetectionOutcome::Unavailable { reason }),
        Just(DetectionOutcome::Cancelled),
        Just(DetectionOutcome::BudgetExhausted),
        "[a-z ]{4,20}".prop_map(|reason| DetectionOutcome::Unsupported { reason }),
    ]
}

fn arb_adapter_detection_result() -> impl Strategy<Value = AdapterDetectionResult> {
    (
        arb_adapter_descriptor(),
        arb_source_generation(),
        arb_detection_outcome(),
        proptest::option::of(arb_module_version_evidence()),
        proptest::option::of(arb_detection_input_identity()),
        proptest::collection::vec(arb_module_activation_identity(), 0..5),
        proptest::option::of(arb_detection_configuration_evidence()),
    )
        .prop_map(
            |(
                desc,
                project_gen,
                outcome,
                version_evidence,
                input_identity,
                contributing_modules,
                configuration_evidence,
            )| {
                let mut result = AdapterDetectionResult::new(desc, project_gen, outcome);
                if let Some(evidence) = version_evidence {
                    result = result.with_version_evidence(evidence);
                }
                result.input_identity = input_identity;
                result.contributing_modules = contributing_modules;
                result.configuration_evidence = configuration_evidence;
                result
            },
        )
}

fn arb_detection_authority_error() -> impl Strategy<Value = DetectionAuthorityError> {
    prop_oneof![
        Just(DetectionAuthorityError::UnsupportedSchema),
        Just(DetectionAuthorityError::NonProduction),
        Just(DetectionAuthorityError::DescriptorMismatch),
        Just(DetectionAuthorityError::GenerationMismatch),
        Just(DetectionAuthorityError::CancelledInput),
        Just(DetectionAuthorityError::MissingPolicyIdentity),
        Just(DetectionAuthorityError::InvalidContentDigest),
        Just(DetectionAuthorityError::InvalidSelectorEvidence),
        Just(DetectionAuthorityError::InvalidModuleEvidence),
        Just(DetectionAuthorityError::InvalidConfigurationEvidence),
        Just(DetectionAuthorityError::MissingInputIdentity),
        Just(DetectionAuthorityError::InputIdentityMismatch),
        Just(DetectionAuthorityError::MissingContributingEvidence),
        Just(DetectionAuthorityError::UnrelatedContributingEvidence),
        Just(DetectionAuthorityError::InsufficientConfidence),
        Just(DetectionAuthorityError::IncompleteModuleUniverse),
        Just(DetectionAuthorityError::RequiredModulePresent),
        Just(DetectionAuthorityError::InvalidVersionEvidence),
        Just(DetectionAuthorityError::UnsupportedVersionConstraint),
        Just(DetectionAuthorityError::VersionConstraintNotSatisfied),
        Just(DetectionAuthorityError::VersionConstraintSatisfied),
        Just(DetectionAuthorityError::MissingConfigurationEvidence),
        Just(DetectionAuthorityError::ConfigurationRuleNotSatisfied),
        Just(DetectionAuthorityError::NonAuthoritativeOutcome),
    ]
}

fn arb_detection_authority_receipt() -> impl Strategy<Value = DetectionAuthorityReceipt> {
    (
        arb_detection_input_identity(),
        arb_adapter_descriptor(),
        arb_detection_outcome(),
        any::<bool>(),
        proptest::option::of(arb_detection_authority_error()),
    )
        .prop_map(|(input_identity, descriptor, outcome, authoritative, error)| {
            DetectionAuthorityReceipt::new(
                input_identity,
                descriptor,
                outcome,
                authoritative,
                error,
            )
        })
}

fn arb_fact_class() -> impl Strategy<Value = FactClass> {
    prop_oneof![
        Just(FactClass::GeneratedMembers),
        Just(FactClass::PackageGraph),
        Just(FactClass::FrameworkImports),
        Just(FactClass::Diagnostics),
        Just(FactClass::Extension),
    ]
}

fn arb_adapter_source_scope() -> impl Strategy<Value = AdapterSourceScope> {
    (
        arb_file_id(),
        arb_source_generation(),
        proptest::option::of("[a-f0-9]{8}".prop_map(|s| format!("digest:{s}"))),
        proptest::option::of(arb_anchor_id()),
        proptest::option::of("[A-Z][a-z]{2,8}(::[A-Z][a-z]{2,6})?"),
    )
        .prop_map(|(file, source_gen, digest, anchor, pkg)| {
            AdapterSourceScope::new(file, source_gen, digest, anchor, pkg)
        })
}

fn arb_invalidation_dependency() -> impl Strategy<Value = InvalidationDependency> {
    ("[a-z]{3,8}:[A-Z][a-z]{2,8}", arb_source_generation())
        .prop_map(|(key, generation)| InvalidationDependency::new(key, generation))
}

fn arb_adapter_input() -> impl Strategy<Value = AdapterInput> {
    (
        arb_adapter_descriptor(),
        arb_adapter_source_scope(),
        proptest::collection::vec(arb_fact_class(), 1..4),
        proptest::collection::vec(arb_invalidation_dependency(), 0..3),
        proptest::option::of(arb_adapter_budget()),
        arb_adapter_cancellation(),
    )
        .prop_map(|(desc, scope, classes, deps, budget, cancel)| {
            AdapterInput::new(desc, scope, classes, deps, budget, cancel)
        })
}

fn arb_fact_limitation() -> impl Strategy<Value = FactLimitation> {
    (any::<bool>(), arb_confidence(), "[a-z ]{4,20}").prop_map(
        |(blocking, confidence, description)| {
            FactLimitation::new(description, blocking, confidence)
        },
    )
}

fn arb_fact_sink_empty() -> impl Strategy<Value = FactSink> {
    (arb_fact_sink_id(), arb_adapter_id()).prop_map(|(sid, aid)| FactSink::new(sid, aid))
}

fn arb_fact_sink() -> impl Strategy<Value = FactSink> {
    (arb_fact_sink_id(), arb_adapter_id(), proptest::collection::vec(arb_emitted_fact(), 1..3))
        .prop_map(|(sid, aid, facts)| {
            let mut sink = FactSink::new(sid, aid);
            sink.facts = facts;
            if let Some(bytes) = sink.serialized_payload_bytes() {
                sink.total_payload_bytes = bytes;
            }
            sink
        })
}

fn arb_emitted_fact() -> impl Strategy<Value = EmittedFact> {
    (
        arb_fact_sink_id(),
        arb_adapter_id(),
        arb_provenance(),
        arb_confidence(),
        arb_semantic_fact_envelope(),
        arb_fact_class(),
        proptest::option::of(arb_fact_limitation()),
        any::<bool>(),
        "[A-Z][a-z]{2,8}",
    )
        .prop_map(|(sid, aid, prov, conf, env, class, lim, stronger, fw)| {
            EmittedFact::new(sid, aid, fw, prov, conf, env, class, lim, stronger)
        })
}

fn arb_adapter_outcome() -> impl Strategy<Value = AdapterOutcome> {
    prop_oneof![
        arb_fact_sink_empty()
            .prop_map(|sink| AdapterOutcome::Applied { sink, limitations: vec![] }),
        (arb_fact_sink(), proptest::collection::vec(arb_fact_limitation(), 0..3))
            .prop_map(|(sink, limitations)| AdapterOutcome::Applied { sink, limitations }),
        ("[a-z ]{4,20}", proptest::option::of(arb_fact_sink()))
            .prop_map(|(reason, partial_sink)| AdapterOutcome::Dynamic { reason, partial_sink },),
        "[a-z ]{4,20}".prop_map(|reason| AdapterOutcome::Unsupported { reason }),
        proptest::collection::vec("[a-z ]{4,20}", 1..3)
            .prop_map(|descs| AdapterOutcome::Conflict { conflict_descriptions: descs }),
        proptest::option::of(arb_fact_sink())
            .prop_map(|partial_sink| AdapterOutcome::BudgetExhausted { partial_sink }),
        Just(AdapterOutcome::Cancelled),
    ]
}

fn arb_adapter_result() -> impl Strategy<Value = AdapterResult> {
    (
        arb_adapter_descriptor(),
        arb_adapter_source_scope(),
        arb_source_generation(),
        arb_adapter_outcome(),
    )
        .prop_map(|(desc, scope, invocation_gen, outcome)| {
            AdapterResult::new(desc, scope, invocation_gen, outcome)
        })
}

// ── Property tests ─────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    #[test]
    fn semantic_fact_envelope_json_roundtrip(envelope in arb_semantic_fact_envelope()) {
        let json = serde_json::to_string(&envelope)
            .map_err(|error| TestCaseError::fail(error.to_string()))?;
        let decoded: SemanticFactEnvelope = serde_json::from_str(&json)
            .map_err(|error| TestCaseError::fail(error.to_string()))?;
        prop_assert_eq!(&decoded, &envelope);
    }

    // ── Original 4 types ──────────────────────────────────────────────────

    #[test]
    fn reference_edge_json_roundtrip(edge in arb_reference_edge()) {
        let json = serde_json::to_string(&edge).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: perl_semantic_facts::ReferenceEdge = serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&decoded, &edge);
    }

    #[test]
    fn definition_rank_json_roundtrip(rank in arb_definition_rank()) {
        let json = serde_json::to_string(&rank).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: DefinitionRank = serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(decoded, rank);
    }

    #[test]
    fn definition_rank_reason_json_roundtrip(reason in arb_definition_rank_reason()) {
        let json = serde_json::to_string(&reason).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: DefinitionRankReason = serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&decoded, &reason);
    }

    #[test]
    fn definition_candidate_json_roundtrip(candidate in arb_definition_candidate()) {
        let json = serde_json::to_string(&candidate).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: DefinitionCandidate = serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&decoded, &candidate);
    }

    // ── Core fact types ────────────────────────────────────────────────────

    #[test]
    fn anchor_fact_json_roundtrip(fact in arb_anchor_fact()) {
        let json = serde_json::to_string(&fact).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: AnchorFact = serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&decoded, &fact);
    }

    #[test]
    fn entity_fact_json_roundtrip(fact in arb_entity_fact()) {
        let json = serde_json::to_string(&fact).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: EntityFact = serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&decoded, &fact);
    }

    #[test]
    fn occurrence_fact_json_roundtrip(fact in arb_occurrence_fact()) {
        let json = serde_json::to_string(&fact).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: OccurrenceFact = serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&decoded, &fact);
    }

    #[test]
    fn edge_fact_json_roundtrip(fact in arb_edge_fact()) {
        let json = serde_json::to_string(&fact).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: EdgeFact = serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&decoded, &fact);
    }

    #[test]
    fn diagnostic_fact_json_roundtrip(fact in arb_diagnostic_fact()) {
        let json = serde_json::to_string(&fact).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: DiagnosticFact = serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&decoded, &fact);
    }

    // ── Export and import types ─────────────────────────────────────────────

    #[test]
    fn export_tag_json_roundtrip(tag in arb_export_tag()) {
        let json = serde_json::to_string(&tag).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: ExportTag = serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&decoded, &tag);
    }

    #[test]
    fn export_set_json_roundtrip(export_set in arb_export_set()) {
        let json = serde_json::to_string(&export_set).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: ExportSet = serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&decoded, &export_set);
    }

    #[test]
    fn import_kind_json_roundtrip(kind in arb_import_kind()) {
        let json = serde_json::to_string(&kind).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: ImportKind = serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(decoded, kind);
    }

    #[test]
    fn import_symbols_json_roundtrip(symbols in arb_import_symbols()) {
        let json = serde_json::to_string(&symbols).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: ImportSymbols = serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&decoded, &symbols);
    }

    #[test]
    fn import_spec_json_roundtrip(spec in arb_import_spec()) {
        let json = serde_json::to_string(&spec).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: ImportSpec = serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&decoded, &spec);
    }

    // ── Visible symbol ──────────────────────────────────────────────────────

    #[test]
    fn visible_symbol_source_json_roundtrip(source in arb_visible_symbol_source()) {
        let json = serde_json::to_string(&source).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: VisibleSymbolSource = serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(decoded, source);
    }

    #[test]
    fn visible_symbol_context_json_roundtrip(ctx in arb_visible_symbol_context()) {
        let json = serde_json::to_string(&ctx).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: VisibleSymbolContext = serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&decoded, &ctx);
    }

    #[test]
    fn visible_symbol_json_roundtrip(symbol in arb_visible_symbol()) {
        let json = serde_json::to_string(&symbol).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: VisibleSymbol = serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&decoded, &symbol);
    }

    // ── Provider fact tracing ───────────────────────────────────────────────

    #[test]
    fn provider_surface_json_roundtrip(surface in arb_provider_surface()) {
        let json = serde_json::to_string(&surface).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: ProviderSurface = serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(decoded, surface);
    }

    #[test]
    fn provider_fact_source_kind_json_roundtrip(source in arb_provider_fact_source_kind()) {
        let json = serde_json::to_string(&source).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: ProviderFactSourceKind = serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(decoded, source);
    }

    #[test]
    fn provider_fact_freshness_json_roundtrip(freshness in arb_provider_fact_freshness()) {
        let json = serde_json::to_string(&freshness).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: ProviderFactFreshness = serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(decoded, freshness);
    }

    #[test]
    fn provider_fallback_state_json_roundtrip(fallback_state in arb_provider_fallback_state()) {
        let json = serde_json::to_string(&fallback_state).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: ProviderFallbackState = serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(decoded, fallback_state);
    }

    #[test]
    fn provider_fact_trace_json_roundtrip(trace in arb_provider_fact_trace()) {
        let json = serde_json::to_string(&trace).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: ProviderFactTrace = serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&decoded, &trace);
    }

    // ── Rename and safe-delete plans ────────────────────────────────────────

    #[test]
    fn plan_blocker_reason_json_roundtrip(reason in arb_plan_blocker_reason()) {
        let json = serde_json::to_string(&reason).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: PlanBlockerReason = serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(decoded, reason);
    }

    #[test]
    fn plan_blocker_json_roundtrip(blocker in arb_plan_blocker()) {
        let json = serde_json::to_string(&blocker).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: PlanBlocker = serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&decoded, &blocker);
    }

    #[test]
    fn plan_warning_json_roundtrip(warning in arb_plan_warning()) {
        let json = serde_json::to_string(&warning).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: PlanWarning = serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&decoded, &warning);
    }

    #[test]
    fn planned_edit_json_roundtrip(edit in arb_planned_edit()) {
        let json = serde_json::to_string(&edit).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: PlannedEdit = serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&decoded, &edit);
    }

    #[test]
    fn rename_plan_json_roundtrip(plan in arb_rename_plan()) {
        let json = serde_json::to_string(&plan).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: RenamePlan = serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&decoded, &plan);
    }

    #[test]
    fn safe_delete_plan_json_roundtrip(plan in arb_safe_delete_plan()) {
        let json = serde_json::to_string(&plan).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: SafeDeletePlan = serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&decoded, &plan);
    }

    // ── Package graph types ─────────────────────────────────────────────────

    #[test]
    fn package_kind_json_roundtrip(kind in arb_package_kind()) {
        let json = serde_json::to_string(&kind).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: PackageKind = serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(decoded, kind);
    }

    #[test]
    fn package_edge_kind_json_roundtrip(kind in arb_package_edge_kind()) {
        let json = serde_json::to_string(&kind).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: PackageEdgeKind = serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(decoded, kind);
    }

    #[test]
    fn package_node_json_roundtrip(node in arb_package_node()) {
        let json = serde_json::to_string(&node).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: PackageNode = serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&decoded, &node);
    }

    #[test]
    fn package_edge_json_roundtrip(edge in arb_package_edge()) {
        let json = serde_json::to_string(&edge).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: PackageEdge = serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&decoded, &edge);
    }

    // ── Generated member ────────────────────────────────────────────────────

    #[test]
    fn generated_member_kind_json_roundtrip(kind in arb_generated_member_kind()) {
        let json = serde_json::to_string(&kind).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: GeneratedMemberKind = serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(decoded, kind);
    }

    #[test]
    fn generated_member_json_roundtrip(member in arb_generated_member()) {
        let json = serde_json::to_string(&member).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: GeneratedMember = serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&decoded, &member);
    }

    // ── Value shape ─────────────────────────────────────────────────────────

    #[test]
    fn value_shape_json_roundtrip(shape in arb_value_shape()) {
        let json = serde_json::to_string(&shape).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: ValueShape = serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&decoded, &shape);
    }

    // ── Framework adapter SDK types ─────────────────────────────────────────

    #[test]
    fn fact_limitation_json_roundtrip(lim in arb_fact_limitation()) {
        let json = serde_json::to_string(&lim).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: FactLimitation = serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&decoded, &lim);
    }

    #[test]
    fn emitted_fact_json_roundtrip(fact in arb_emitted_fact()) {
        let json = serde_json::to_string(&fact).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: EmittedFact = serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&decoded, &fact);
    }

    #[test]
    fn adapter_descriptor_json_roundtrip(desc in arb_adapter_descriptor()) {
        let json = serde_json::to_string(&desc).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: perl_semantic_facts::framework::AdapterDescriptor =
            serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&decoded, &desc);
    }

    #[test]
    fn detection_configuration_value_json_roundtrip(value in arb_detection_configuration_value()) {
        let json = serde_json::to_string(&value).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: DetectionConfigurationValue =
            serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&decoded, &value);
    }

    #[test]
    fn detection_configuration_observation_json_roundtrip(
        observation in arb_detection_configuration_observation()
    ) {
        let json = serde_json::to_string(&observation)
            .map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: DetectionConfigurationObservation =
            serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&decoded, &observation);
    }

    #[test]
    fn detection_configuration_evidence_json_roundtrip(
        evidence in arb_detection_configuration_evidence()
    ) {
        let json = serde_json::to_string(&evidence)
            .map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: DetectionConfigurationEvidence =
            serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&decoded, &evidence);
    }

    #[test]
    fn module_selector_outcome_json_roundtrip(outcome in arb_module_selector_outcome()) {
        let json = serde_json::to_string(&outcome).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: ModuleSelectorOutcome =
            serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&decoded, &outcome);
    }

    #[test]
    fn module_selector_evaluation_json_roundtrip(evaluation in arb_module_selector_evaluation()) {
        let json = serde_json::to_string(&evaluation)
            .map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: ModuleSelectorEvaluation =
            serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&decoded, &evaluation);
    }

    #[test]
    fn detection_outcome_json_roundtrip(outcome in arb_detection_outcome()) {
        let json = serde_json::to_string(&outcome).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: DetectionOutcome =
            serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&decoded, &outcome);
    }

    #[test]
    fn detection_input_identity_json_roundtrip(identity in arb_detection_input_identity()) {
        let json = serde_json::to_string(&identity)
            .map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: DetectionInputIdentity =
            serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&decoded, &identity);
    }

    #[test]
    fn detection_authority_error_json_roundtrip(error in arb_detection_authority_error()) {
        let json = serde_json::to_string(&error).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: DetectionAuthorityError =
            serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&decoded, &error);
    }

    #[test]
    fn detection_authority_receipt_json_roundtrip(receipt in arb_detection_authority_receipt()) {
        let json = serde_json::to_string(&receipt)
            .map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: DetectionAuthorityReceipt =
            serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&decoded, &receipt);
    }

    #[test]
    fn adapter_detection_input_json_roundtrip(input in arb_adapter_detection_input()) {
        let json = serde_json::to_string(&input).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: perl_semantic_facts::framework::AdapterDetectionInput =
            serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&decoded, &input);
    }

    #[test]
    fn adapter_detection_result_json_roundtrip(result in arb_adapter_detection_result()) {
        let json = serde_json::to_string(&result).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: perl_semantic_facts::framework::AdapterDetectionResult =
            serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&decoded, &result);
    }

    #[test]
    fn adapter_input_json_roundtrip(input in arb_adapter_input()) {
        let json = serde_json::to_string(&input).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: perl_semantic_facts::framework::AdapterInput =
            serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&decoded, &input);
    }

    #[test]
    fn adapter_result_json_roundtrip(result in arb_adapter_result()) {
        let json = serde_json::to_string(&result).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let decoded: perl_semantic_facts::framework::AdapterResult =
            serde_json::from_str(&json).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&decoded, &result);
    }
}
