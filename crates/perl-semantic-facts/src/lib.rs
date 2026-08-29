#![warn(missing_docs)]
#![cfg_attr(clippy, allow(missing_docs))]

//! Neutral semantic fact vocabulary for Perl analysis layers.
//!
//! This crate defines strongly-typed IDs and serializable fact records that can be shared
//! between parser-derived semantics, semantic analyzer synthesis, and workspace indexing.
//!
//! It intentionally does **not** parse Perl, implement LSP providers, or own workspace
//! storage backends.
#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.

use serde::{Deserialize, Serialize};

mod envelope;
pub mod framework;
/// Concrete registry-backed framework adapters built on the SDK.
pub mod framework_adapters;
/// Canonical framework handler relation shared by the route and hook fact
/// families (#8924).
pub mod handler;
/// Canonical framework hook fact family (#8924).
pub mod hook;
/// Dependency-neutral versioned contracts for interprocedural composition
/// (#12672).
pub mod interprocedural;
/// Transport-neutral reachability operation, work-budget, and
/// terminal-outcome contract (#11553).
pub mod reachability_operation;
/// Canonical framework route fact family (#8918).
pub mod route;
/// Transport-neutral stable semantic identity and ownership contract (#12121).
pub mod semantic_identity;
/// Pure, generation-bound planning for conventional source-module moves.
pub mod module_move;

pub use envelope::*;
pub use handler::*;
pub use hook::*;
pub use route::*;

macro_rules! id_newtype {
    ($name:ident) => {
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        pub struct $name(pub u64);
    };
}

id_newtype!(FileId);
id_newtype!(ScopeId);
id_newtype!(EntityId);
id_newtype!(AnchorId);
id_newtype!(OccurrenceId);
id_newtype!(EdgeId);
id_newtype!(DiagnosticId);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum EntityKind {
    Package,
    Class,
    Role,
    Subroutine,
    Method,
    Variable,
    Constant,
    Field,
    Label,
    Format,
    Module,
    GeneratedMember,
    ExternalSymbol,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum OccurrenceKind {
    Definition,
    Reference,
    Read,
    Write,
    Call,
    MethodCall,
    StaticMethodCall,
    CoderefReference,
    TypeglobReference,
    Import,
    Export,
    Inheritance,
    RoleComposition,
    GeneratedUse,
    DynamicBoundary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum EdgeKind {
    Defines,
    References,
    Reads,
    Writes,
    Calls,
    ImportsModule,
    ImportsSymbol,
    ExportsSymbol,
    ExportsGroup,
    Inherits,
    ComposesRole,
    MemberOf,
    GeneratedFrom,
    AliasOf,
    DependsOn,
    DynamicBoundary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Provenance {
    ExactAst,
    DesugaredAst,
    SemanticAnalyzer,
    FrameworkSynthesis,
    ImportExportInference,
    PragmaInference,
    NameHeuristic,
    SearchFallback,
    DynamicBoundary,
    /// Exact `require Module; Module->import(literal list)` pattern where all
    /// import arguments are literal strings or `qw(...)` words — no variables,
    /// no computed expressions.  More precise than `ExactAst` because it
    /// guarantees the symbol list is fully statically known.
    LiteralRequireImport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Confidence {
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnchorFact {
    pub id: AnchorId,
    pub file_id: FileId,
    pub span_start_byte: u32,
    pub span_end_byte: u32,
    pub scope_id: Option<ScopeId>,
    pub provenance: Provenance,
    pub confidence: Confidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityFact {
    pub id: EntityId,
    pub kind: EntityKind,
    pub canonical_name: String,
    pub anchor_id: Option<AnchorId>,
    pub scope_id: Option<ScopeId>,
    pub provenance: Provenance,
    pub confidence: Confidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OccurrenceFact {
    pub id: OccurrenceId,
    pub kind: OccurrenceKind,
    pub entity_id: Option<EntityId>,
    pub anchor_id: AnchorId,
    pub scope_id: Option<ScopeId>,
    pub provenance: Provenance,
    pub confidence: Confidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EdgeFact {
    pub id: EdgeId,
    pub kind: EdgeKind,
    pub from_entity_id: EntityId,
    pub to_entity_id: EntityId,
    pub via_occurrence_id: Option<OccurrenceId>,
    pub provenance: Provenance,
    pub confidence: Confidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticFact {
    pub id: DiagnosticId,
    pub code: Option<String>,
    pub message: String,
    pub primary_anchor_id: AnchorId,
    pub related_anchor_ids: Vec<AnchorId>,
    pub scope_id: Option<ScopeId>,
    pub provenance: Provenance,
    pub confidence: Confidence,
}

/// Canonical export facts inferred for a Perl package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportSet {
    /// Package symbols exported by default (`@EXPORT`).
    pub default_exports: Vec<String>,
    /// Package symbols exported on request (`@EXPORT_OK`).
    pub optional_exports: Vec<String>,
    /// Named export groups (`%EXPORT_TAGS`).
    pub tags: Vec<ExportTag>,
    /// How this export set was inferred.
    pub provenance: Provenance,
    /// Confidence for the inferred export set.
    pub confidence: Confidence,
    /// Exporting module or package name, when known.
    pub module_name: Option<String>,
    /// Source anchor of the export declaration, when available.
    pub anchor_id: Option<AnchorId>,
}

/// Named `%EXPORT_TAGS` entry and its members.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportTag {
    /// Tag name (for example `all` from `:all`).
    pub name: String,
    /// Symbols in this tag.
    pub members: Vec<String>,
}

/// Canonical import specification inferred for a single import site.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportSpec {
    /// Imported module or package name.
    pub module: String,
    /// Syntactic import shape used at the site.
    pub kind: ImportKind,
    /// Symbol selection policy represented at the site.
    pub symbols: ImportSymbols,
    /// Import site provenance.
    pub provenance: Provenance,
    /// Confidence for inferred import semantics.
    pub confidence: Confidence,
    /// File containing this import site, when known.
    pub file_id: Option<FileId>,
    /// Source anchor for this import site, when available.
    pub anchor_id: Option<AnchorId>,
    /// Scope enclosing this import site, when known.
    pub scope_id: Option<ScopeId>,
    /// Byte offset of the start of this import statement in the source file.
    ///
    /// Used for order-aware suppression: a dynamic import only suppresses
    /// barewords that appear **after** the import statement in the file.
    /// `None` means the position is unknown; callers should be conservative
    /// (no suppression) when this is absent.
    pub span_start_byte: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImportKind {
    Use,
    UseEmpty,
    UseExplicitList,
    UseTag,
    Require,
    RequireThenImport,
    UseConstant,
    DynamicRequire,
    /// A `Class->import(...)` method call — not a `use` statement.
    ///
    /// Used when a class's `import` method is called directly, typically with
    /// a dynamic argument list (`Foo->import(@names)`).  This is distinct from
    /// `Use` (which is a `use Foo` statement) and `Require` (which is a bare
    /// `require Foo` statement).
    ManualImport,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImportSymbols {
    Default,
    None,
    Explicit(Vec<String>),
    Tags(Vec<String>),
    Mixed { tags: Vec<String>, names: Vec<String> },
    Dynamic,
}

/// A single `use lib`/`no lib` include-path entry extracted from a Perl file.
///
/// Distinct from [`ImportSpec`]: `ImportSpec` is import-site-scoped (what module
/// is being imported), while `UseLibFact` is path-entry-scoped (what directory
/// is being added to or removed from `@INC`).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UseLibFact {
    /// The literal path string as it appeared in the source (after unquoting).
    ///
    /// For `use lib '../lib'`, this is `"../lib"`.
    /// Dynamic args (`use lib $var`, `use lib @dirs`) are never emitted —
    /// no fact is created for those cases.
    pub path: String,
    /// `true` = added by `use lib`; `false` = removed by `no lib`.
    ///
    /// Facts are per-statement, not net state — both facts are emitted when
    /// `use lib 'x'` is followed by `no lib 'x'`. Callers compute net state
    /// from the sequence.
    pub is_active: bool,
    /// File containing this statement.
    pub file_id: FileId,
    /// Anchor of the `use lib`/`no lib` statement.
    pub anchor_id: Option<AnchorId>,
    /// `ExactAst` for static literal entries; dynamic args are not emitted.
    pub provenance: Provenance,
    /// `High` for static string literals. Dynamic args are never emitted.
    pub confidence: Confidence,
}

impl UseLibFact {
    /// Create a new `UseLibFact`.
    pub fn new(
        path: String,
        is_active: bool,
        file_id: FileId,
        anchor_id: Option<AnchorId>,
        provenance: Provenance,
        confidence: Confidence,
    ) -> Self {
        Self { path, is_active, file_id, anchor_id, provenance, confidence }
    }
}

/// One symbol visible at a query point with source attribution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VisibleSymbol {
    /// Symbol display name visible at the query point.
    pub name: String,
    /// Optional backing entity when known.
    pub entity_id: Option<EntityId>,
    /// Visibility source classification.
    pub source: VisibleSymbolSource,
    /// Visibility confidence.
    pub confidence: Confidence,
    /// Optional origin metadata for hover explanations and rename safety.
    pub context: Option<VisibleSymbolContext>,
}

/// Origin metadata for a [`VisibleSymbol`], enabling hover explanations
/// and rename safety analysis.
///
/// Tracks the source module and the import/export anchor IDs that made
/// the symbol visible at the query point.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VisibleSymbolContext {
    /// Module that originally defined or exported the symbol.
    pub source_module: Option<String>,
    /// Anchor of the `use`/`require` statement that imported the symbol.
    pub source_import_anchor_id: Option<AnchorId>,
    /// Anchor of the `@EXPORT`/`@EXPORT_OK` declaration that exported the symbol.
    pub source_export_anchor_id: Option<AnchorId>,
}

impl VisibleSymbolContext {
    /// Create a new `VisibleSymbolContext`.
    ///
    /// Required because `#[non_exhaustive]` prevents struct-literal
    /// construction outside this crate.
    pub fn new(
        source_module: Option<String>,
        source_import_anchor_id: Option<AnchorId>,
        source_export_anchor_id: Option<AnchorId>,
    ) -> Self {
        Self { source_module, source_import_anchor_id, source_export_anchor_id }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum VisibleSymbolSource {
    LocalLexical,
    LocalPackage,
    ExplicitImport,
    DefaultExport,
    ExportTag,
    Constant,
    Generated,
    External,
    DynamicUnknown,
}

// ── Definition Ranking ──

/// Coarse ranking tier for a definition candidate.
///
/// Variants are ordered from most specific (best) to least specific (worst).
/// The `Ord` derive reflects this ordering so that sorting a candidate list
/// places `ExactQualified` first and `Heuristic` last.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DefinitionRank {
    ExactQualified,
    SamePackage,
    ExplicitImport,
    DefaultExport,
    WorkspaceCandidate,
    Heuristic,
}

/// Structured reason explaining why a [`DefinitionCandidate`] received its rank.
///
/// Variants that carry a `module` field identify the specific module that
/// contributed the import or export relationship.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DefinitionRankReason {
    ExactQualifiedName,
    SamePackage,
    ExplicitImport { module: String },
    DefaultExport { module: String },
    WorkspaceSymbol,
    HeuristicNameMatch,
}

/// A ranked entry in a definition candidate list.
///
/// Produced by the workspace definition index and consumed by
/// `SemanticQueries::definitions` to present ordered results to providers.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DefinitionCandidate {
    /// The entity this candidate refers to.
    pub entity_id: EntityId,
    /// Source anchor for the definition site.
    pub anchor_id: AnchorId,
    /// Fully qualified canonical name of the entity.
    pub canonical_name: String,
    /// Human-readable display name.
    pub display_name: String,
    /// Owning package, if known.
    pub package: Option<String>,
    /// Entity kind (Subroutine, Method, Variable, etc.).
    pub kind: EntityKind,
    /// How this candidate was discovered.
    pub provenance: Provenance,
    /// Confidence in the candidate.
    pub confidence: Confidence,
    /// Coarse ranking tier.
    pub rank: DefinitionRank,
    /// Structured reason for the assigned rank.
    pub rank_reason: DefinitionRankReason,
}

impl DefinitionCandidate {
    /// Construct a new `DefinitionCandidate`.
    ///
    /// Required because the struct is `#[non_exhaustive]` and cannot be
    /// constructed with struct-literal syntax outside this crate.
    #[allow(clippy::too_many_arguments)] // mirrors the struct fields 1-to-1
    pub fn new(
        entity_id: EntityId,
        anchor_id: AnchorId,
        canonical_name: String,
        display_name: String,
        package: Option<String>,
        kind: EntityKind,
        provenance: Provenance,
        confidence: Confidence,
        rank: DefinitionRank,
        rank_reason: DefinitionRankReason,
    ) -> Self {
        Self {
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
        }
    }
}

/// Occurrence-based reference edge linking a reference site to zero, one, or many
/// target entity candidates.
///
/// Unlike [`EdgeFact`] which connects two known entities, a `ReferenceEdge` is
/// anchored on an [`OccurrenceFact`] and carries a candidate list that may be empty
/// (unresolved), singular (exact), or plural (ambiguous).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReferenceEdge {
    /// The occurrence that produced this reference.
    pub occurrence_id: OccurrenceId,
    /// Source anchor for the reference site.
    pub anchor_id: AnchorId,
    /// File containing the reference.
    pub file_id: FileId,
    /// Bare or qualified symbol key used at the reference site.
    ///
    /// This is display/lookup spelling, not target identity: it carries the
    /// canonical name when the producer could derive one, and is empty when it
    /// could not. Target identity lives in
    /// [`target_candidates`](Self::target_candidates), which may name a
    /// resolved entity even when no spelling was derived. Producers must not
    /// synthesize a placeholder name for an unresolved occurrence
    /// (perl-lsp-swarm#8083).
    pub symbol_key: String,
    /// Zero, one, or many candidate target entities.
    pub target_candidates: Vec<EntityId>,
    /// Occurrence classification (Read, Call, MethodCall, etc.).
    pub kind: OccurrenceKind,
    /// How this reference was inferred.
    pub provenance: Provenance,
    /// Confidence in the resolution.
    pub confidence: Confidence,
}

// ── Package Graph ──

/// A node in the package/class/role graph.
///
/// Represents a known package, class, or role in the workspace.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageNode {
    /// Entity backing this package node.
    pub entity_id: EntityId,
    /// Fully qualified package name.
    pub name: String,
    /// Classification of this package node.
    pub kind: PackageKind,
    /// Source anchor for the package declaration, when available.
    pub anchor_id: Option<AnchorId>,
    /// File containing this package declaration, when known.
    pub file_id: Option<FileId>,
}

impl PackageNode {
    /// Construct a new `PackageNode`.
    ///
    /// Required because the struct is `#[non_exhaustive]` and cannot be
    /// constructed with struct-literal syntax outside this crate.
    pub fn new(
        entity_id: EntityId,
        name: String,
        kind: PackageKind,
        anchor_id: Option<AnchorId>,
        file_id: Option<FileId>,
    ) -> Self {
        Self { entity_id, name, kind, anchor_id, file_id }
    }
}

/// Classification of a package graph node.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PackageKind {
    /// A plain Perl package.
    Package,
    /// A class (Moo/Moose/native).
    Class,
    /// A role (Moo::Role/Moose::Role/Role::Tiny).
    Role,
    /// An external package not found in the workspace.
    External,
}

/// A directed edge in the package graph.
///
/// Connects a source package to a target package with a relationship kind
/// (inheritance, role composition, or dependency).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageEdge {
    /// Source package name (the inheriting/consuming package).
    pub from_package: String,
    /// Target package name (the inherited/composed package).
    pub to_package: String,
    /// Relationship kind.
    pub kind: PackageEdgeKind,
    /// Source anchor for the statement that established this edge.
    pub anchor_id: Option<AnchorId>,
    /// How this edge was inferred.
    pub provenance: Provenance,
    /// Confidence in this edge.
    pub confidence: Confidence,
}

impl PackageEdge {
    /// Construct a new `PackageEdge`.
    ///
    /// Required because the struct is `#[non_exhaustive]` and cannot be
    /// constructed with struct-literal syntax outside this crate.
    pub fn new(
        from_package: String,
        to_package: String,
        kind: PackageEdgeKind,
        anchor_id: Option<AnchorId>,
        provenance: Provenance,
        confidence: Confidence,
    ) -> Self {
        Self { from_package, to_package, kind, anchor_id, provenance, confidence }
    }
}

/// Kind of relationship in the package graph.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PackageEdgeKind {
    /// Inheritance: `use parent`, `use base`, `@ISA`, `extends`.
    Inherits,
    /// Role composition: `with 'Role'`.
    ComposesRole,
    /// General dependency (e.g. `use Module`).
    DependsOn,
}

// ── Generated Members ──────────────────────────────────────────────────

/// A framework-synthesized member (e.g. Moo/Moose accessor from `has`).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneratedMember {
    /// Deterministic entity ID for this generated member.
    pub entity_id: EntityId,
    /// Name of the generated method (e.g. `x` for `has 'x'`).
    pub name: String,
    /// What kind of generated member this is.
    pub kind: GeneratedMemberKind,
    /// Anchor of the `has` declaration that generated this member.
    pub source_anchor_id: AnchorId,
    /// Package that owns this generated member.
    pub package: String,
    /// Always `FrameworkSynthesis` for generated members.
    pub provenance: Provenance,
    /// Always `Medium` for generated members.
    pub confidence: Confidence,
}

impl GeneratedMember {
    /// Construct a new `GeneratedMember`.
    ///
    /// Required because the struct is `#[non_exhaustive]` and cannot be
    /// constructed with struct-literal syntax outside this crate.
    #[allow(clippy::too_many_arguments)] // mirrors the struct fields 1-to-1
    pub fn new(
        entity_id: EntityId,
        name: String,
        kind: GeneratedMemberKind,
        source_anchor_id: AnchorId,
        package: String,
        provenance: Provenance,
        confidence: Confidence,
    ) -> Self {
        Self { entity_id, name, kind, source_anchor_id, package, provenance, confidence }
    }
}

/// Classification of a framework-generated member.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GeneratedMemberKind {
    /// Read-only accessor (getter).
    Getter,
    /// Read-write accessor (setter).
    Setter,
    /// Combined read-write accessor (single method for both get and set).
    Accessor,
    /// Predicate method (`has_<attr>`).
    Predicate,
    /// Clearer method (`clear_<attr>`).
    Clearer,
    /// Builder method (`_build_<attr>`).
    Builder,
    /// Constant value.
    Constant,
}

// ── Value Shape ─────────────────────────────────────────────────────

/// Lightweight type approximation for a variable or expression.
///
/// Used for method candidate filtering — not a full type system.
/// `bless` is not a separate top-level shape; it produces
/// [`ValueShape::Object`] with low confidence.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValueShape {
    /// Shape could not be determined.
    Unknown,
    /// Plain scalar value.
    Scalar,
    /// Array reference (`[...]` or `\@arr`).
    ArrayRef,
    /// Hash reference (`{...}` or `\%hash`).
    HashRef,
    /// Code reference (`sub { ... }` or `\&sub`).
    CodeRef,
    /// A package name used as a class (e.g. `Foo` in `Foo->method`).
    PackageName {
        /// Fully qualified package name.
        package: String,
    },
    /// An object instance (blessed reference).
    Object {
        /// Package the object was blessed into.
        package: String,
        /// Confidence in the inferred package.
        confidence: Confidence,
    },
}

// ── Provider Fact-Source Tracing ─────────────────────────────────────

/// LSP provider surface that consumed or considered a semantic fact.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ProviderSurface {
    Diagnostics,
    Completion,
    Hover,
    Definition,
    References,
    Rename,
    SafeDelete,
    WorkspaceSymbols,
    DocumentSymbols,
    SemanticTokens,
}

/// Coarse source class for a provider answer.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ProviderFactSourceKind {
    /// Parser tokens or AST shape.
    ParserSyntax,
    /// Legacy workspace index or provider-local data.
    LegacyWorkspace,
    /// Canonical semantic fact graph.
    SemanticFact,
    /// Rust compiler-substrate fact.
    CompilerFact,
    /// Framework adapter projection.
    FrameworkAdapter,
    /// Dynamic-boundary fact used to avoid false precision.
    DynamicBoundary,
    /// Fallback behavior because no stronger fact was available.
    Fallback,
    /// Source is intentionally unknown or unavailable.
    Unknown,
}

/// Freshness state for a provider fact source.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ProviderFactFreshness {
    Fresh,
    Stale,
    Unknown,
    NotApplicable,
}

/// How a provider used a traced fact source.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ProviderFallbackState {
    /// Primary answer path used this source.
    Primary,
    /// Source was measured but not used for live behavior.
    Shadow,
    /// Provider fell back from a stronger unavailable source.
    Fallback,
    /// Source existed but could not answer this request.
    Unavailable,
    /// Source blocked an unsafe provider action.
    Blocked,
}

/// Source/provenance trace for a provider answer.
///
/// This is an additive contract for provider cutover proof. It lets providers
/// report where an answer came from before any broad provider behavior change.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderFactTrace {
    /// Provider surface that produced the answer.
    pub surface: ProviderSurface,
    /// Coarse source class used by the provider.
    pub source: ProviderFactSourceKind,
    /// Semantic provenance for the underlying fact, when known.
    pub provenance: Provenance,
    /// Confidence in the underlying fact or fallback.
    pub confidence: Confidence,
    /// Freshness of the underlying fact relative to the request.
    pub freshness: ProviderFactFreshness,
    /// Whether this source drove live behavior, shadow proof, fallback, or a blocker.
    pub fallback_state: ProviderFallbackState,
    /// Optional stable source hash used by the producer.
    pub source_hash: Option<String>,
    /// Optional semantic anchor used by the producer.
    pub anchor_id: Option<AnchorId>,
    /// Optional fact/model version used by the producer.
    pub model_version: Option<u32>,
}

impl ProviderFactTrace {
    /// Construct a new provider fact trace.
    #[allow(clippy::too_many_arguments)] // mirrors the public trace fields 1-to-1
    pub fn new(
        surface: ProviderSurface,
        source: ProviderFactSourceKind,
        provenance: Provenance,
        confidence: Confidence,
        freshness: ProviderFactFreshness,
        fallback_state: ProviderFallbackState,
        source_hash: Option<String>,
        anchor_id: Option<AnchorId>,
        model_version: Option<u32>,
    ) -> Self {
        Self {
            surface,
            source,
            provenance,
            confidence,
            freshness,
            fallback_state,
            source_hash,
            anchor_id,
            model_version,
        }
    }
}

// ── Rename and Safe Delete Plans ────────────────────────────────────

/// A conservative rename plan enumerating affected occurrences and blockers.
///
/// Produced by `SemanticQueries::rename_plan` and consumed by the rename
/// provider to decide whether the rename is safe to apply.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenamePlan {
    /// The entity being renamed.
    pub entity_id: EntityId,
    /// Current name of the entity.
    pub old_name: String,
    /// Proposed new name.
    pub new_name: String,
    /// Planned text edits for the rename.
    pub edits: Vec<PlannedEdit>,
    /// Conditions that block the rename.
    pub blockers: Vec<PlanBlocker>,
    /// Non-blocking warnings for the rename.
    pub warnings: Vec<PlanWarning>,
}

impl RenamePlan {
    /// Construct a new `RenamePlan`.
    ///
    /// Required because the struct is `#[non_exhaustive]` and cannot be
    /// constructed with struct-literal syntax outside this crate.
    pub fn new(
        entity_id: EntityId,
        old_name: String,
        new_name: String,
        edits: Vec<PlannedEdit>,
        blockers: Vec<PlanBlocker>,
        warnings: Vec<PlanWarning>,
    ) -> Self {
        Self { entity_id, old_name, new_name, edits, blockers, warnings }
    }
}

/// A conservative safe-delete plan enumerating blockers that prevent deletion.
///
/// Produced by `SemanticQueries::safe_delete_plan` and consumed by the
/// safe-delete provider to decide whether deletion is safe.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SafeDeletePlan {
    /// The entity being considered for deletion.
    pub entity_id: EntityId,
    /// Name of the entity being considered for deletion.
    pub name: String,
    /// Conditions that block the deletion.
    pub blockers: Vec<PlanBlocker>,
    /// Non-blocking warnings for the deletion.
    pub warnings: Vec<PlanWarning>,
}

impl SafeDeletePlan {
    /// Construct a new `SafeDeletePlan`.
    ///
    /// Required because the struct is `#[non_exhaustive]` and cannot be
    /// constructed with struct-literal syntax outside this crate.
    pub fn new(
        entity_id: EntityId,
        name: String,
        blockers: Vec<PlanBlocker>,
        warnings: Vec<PlanWarning>,
    ) -> Self {
        Self { entity_id, name, blockers, warnings }
    }
}

/// A condition that blocks a rename or safe-delete operation.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanBlocker {
    /// Why the operation is blocked.
    pub reason: PlanBlockerReason,
    /// Source anchor for the blocking reference, when available.
    pub anchor_id: Option<AnchorId>,
    /// Human-readable description of the blocker.
    pub description: String,
}

impl PlanBlocker {
    /// Construct a new `PlanBlocker`.
    ///
    /// Required because the struct is `#[non_exhaustive]` and cannot be
    /// constructed with struct-literal syntax outside this crate.
    pub fn new(
        reason: PlanBlockerReason,
        anchor_id: Option<AnchorId>,
        description: String,
    ) -> Self {
        Self { reason, anchor_id, description }
    }
}

/// Reason a rename or safe-delete operation is blocked.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlanBlockerReason {
    /// Reference crosses a dynamic boundary (string eval, symbolic deref, AUTOLOAD).
    DynamicBoundary,
    /// Reference is ambiguous (multiple candidate targets).
    AmbiguousReference,
    /// Symbol is exported and referenced from other modules.
    CrossModuleExport,
    /// Symbol is imported by another file.
    ImportedSymbol,
    /// Symbol is listed in an ExportSet.
    ExportedSymbol,
    /// Symbol has remaining references in the workspace.
    ReferencesExist,
    /// Symbol is a generated member without a generator-specific edit plan.
    GeneratedMember,
    /// Fact freshness is stale, so the plan cannot authorize edits or deletion.
    StaleFact,
    /// Occurrence could not be classified into a known category.
    UnclassifiedOccurrence,
}

/// A non-blocking warning attached to a rename or safe-delete plan.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanWarning {
    /// Human-readable warning message.
    pub message: String,
    /// Source anchor for the warning site, when available.
    pub anchor_id: Option<AnchorId>,
}

impl PlanWarning {
    /// Construct a new `PlanWarning`.
    ///
    /// Required because the struct is `#[non_exhaustive]` and cannot be
    /// constructed with struct-literal syntax outside this crate.
    pub fn new(message: String, anchor_id: Option<AnchorId>) -> Self {
        Self { message, anchor_id }
    }
}

/// A planned text edit within a rename operation.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedEdit {
    /// Source anchor for the edit site.
    pub anchor_id: AnchorId,
    /// File containing the edit site.
    pub file_id: FileId,
    /// Classification of this edit (definition, reference, import, export).
    pub category: PlannedEditCategory,
    /// Text being replaced.
    pub old_text: String,
    /// Replacement text.
    pub new_text: String,
}

impl PlannedEdit {
    /// Construct a new `PlannedEdit`.
    ///
    /// Required because the struct is `#[non_exhaustive]` and cannot be
    /// constructed with struct-literal syntax outside this crate.
    pub fn new(
        anchor_id: AnchorId,
        file_id: FileId,
        category: PlannedEditCategory,
        old_text: String,
        new_text: String,
    ) -> Self {
        Self { anchor_id, file_id, category, old_text, new_text }
