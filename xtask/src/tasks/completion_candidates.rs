//! Inventory every live `textDocument/completion` candidate producer and hold
//! the finalizer route closed (#10949, controller #8969, identity controller
//! #8963).
//!
//! This is a control-plane task. It changes no candidate inclusion, ordering,
//! insertion, provider behavior, protocol response, or finalizer
//! implementation. It answers one question mechanically and fails closed
//! rather than guessing: *which* code can put a candidate in front of a user,
//! what identity and evidence does it carry, which entry paths reach it, and
//! who owns moving it off compatibility.
//!
//! Without that denominator, #10229 (one live application engine) can look
//! complete while a legacy source still appends outside the shared route, and
//! a focused producer migration can fabricate identity from a label with the
//! suite green.
//!
//! # Why two discovery planes
//!
//! One plane cannot establish the denominator on its own, so discovery
//! reconciles two independent populations over the same scanned source:
//!
//! * **Append channel** — every function that takes `&mut Vec<CompletionItem>`.
//!   In this codebase that reference *is* the candidate-append channel: a
//!   producer contributes by pushing into a caller-owned vector. This plane is
//!   authoritative for "what can append?".
//! * **Construction** — every file that builds a `CompletionItem` value. A file
//!   that constructs candidates but exposes no append-channel function is
//!   invisible to the first plane, so it must be dispositioned explicitly
//!   rather than silently omitted.
//!
//! A file may only stay out of the producer population when a
//! `[[construction_only]]` row says why. That is the seam through which a new
//! hidden producer would otherwise enter.
//!
//! # Discovery ceiling
//!
//! Stated so a reader does not over-trust the check: discovery is syntactic. A
//! producer that returned candidates by value, or appended through a wrapper
//! type rather than `&mut Vec<CompletionItem>`, would not appear in the first
//! plane; the construction plane is what bounds that gap at file granularity.
//! Reachability for core-provider rows is declared, not proven — only rows
//! called directly from a runtime entry point are mechanically reconciled
//! against the call site. Every row records which of those two it is, so no
//! reader mistakes a declared reach for a proven one.

use crate::utils::project_root;
use color_eyre::eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::process::Command;
use syn::visit::Visit;

/// Hand-authored disposition ledger. Humans own this file.
pub const LEDGER_PATH: &str = "policy/completion-candidate-producers.toml";
/// Generated human-readable projection of the reconciled inventory.
pub const PROJECTION_PATH: &str = "docs/architecture/completion-candidate-producers.md";

const SCHEMA: &str = "completion_candidate_producers.v1";

/// Source under the standard-completion surface. Everything reachable from
/// `textDocument/completion` that can produce or finalize a candidate lives
/// here; the inventory's denominator is derived from exactly these files.
const SCAN_ROOTS: &[&str] = &[
    "crates/perl-lsp-rs-core/src/providers/completion/",
    "crates/perl-lsp-rs-core/src/providers/completion_item/",
    "crates/perl-lsp-rs-core/src/providers/dancer2/completion.rs",
    // Reached through the inventoried `completion/completion/file_path.rs`
    // facade, which delegates candidate production to them outright. A facade
    // in the denominator does not put its delegates there.
    "crates/perl-lsp-rs-core/src/providers/file_completion/",
    "crates/perl-lsp-rs-core/src/providers/htmx/",
    "crates/perl-lsp-rs/src/runtime/language/completion.rs",
];

/// Files inside [`SCAN_ROOTS`] that are test surfaces rather than product
/// source. They are excluded by name because their whole contents are proof,
/// not a candidate route.
const TEST_SURFACE_FILES: &[&str] = &[
    "crates/perl-lsp-rs-core/src/providers/completion/completion/tests.rs",
    "crates/perl-lsp-rs-core/src/providers/completion/completion/keyword_role_tests.rs",
];

/// The runtime file holding the LSP entry points. Reachability is reconciled
/// against direct calls made inside these functions.
const ENTRY_FILE: &str = "crates/perl-lsp-rs/src/runtime/language/completion.rs";

/// Every shipped `textDocument/completion` entry point. A producer reached by
/// fewer than all of them has a route divergence and must name its owner.
const ENTRY_POINTS: &[&str] = &["handle_completion", "handle_completion_cancellable"];

/// The runtime seam that merges, ranks, and caps. Nothing may append after it.
const FINALIZER_CALL: &str = "sort_and_cap_completions";

/// The binding the entry points append candidates into. Post-finalizer append
/// detection is scoped to this name so an unrelated `Vec::push` in the
/// serialization tail is not mistaken for a candidate append.
const CANDIDATE_BINDING: &str = "completions";

/// Controller issues. A controller coordinates a programme; it cannot be the
/// implementation owner of a row, because "owned by the controller" is how a
/// migration silently acquires no owner at all.
const CONTROLLER_ISSUES: &[&str] = &["#8969", "#8963", "#9621"];

/// Candidate classes that are genuinely server-authored catalogues, and so may
/// carry a reviewed stable identity instead of a semantic entity.
const CATALOGUE_CLASSES: &[CandidateClass] = &[
    CandidateClass::Builtin,
    CandidateClass::Keyword,
    CandidateClass::Snippet,
    CandidateClass::RegexContext,
    CandidateClass::TestFramework,
    CandidateClass::XsApi,
];

// ---------------------------------------------------------------------------
// Ledger model
// ---------------------------------------------------------------------------

/// The authored side of the inventory.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Ledger {
    pub schema_version: String,
    /// Issue that owns this inventory.
    pub controlling_issue: String,
    /// Issue that owns the one live application/finalizer route.
    pub route_owner: String,
    /// Issue that owns pure merge/rank/cap semantics.
    pub finalizer_owner: String,
    /// Issue that owns source completeness and terminal protocol outcome.
    pub outcome_owner: String,
    /// Digest of the scanned source at the time the dispositions were audited.
    /// A source change invalidates the audit.
    pub source_digest: String,
    pub producers: Vec<ProducerRow>,
    #[serde(default)]
    pub construction_only: Vec<ConstructionOnlyRow>,
    #[serde(default)]
    pub delegations: Vec<DelegationRow>,
}

/// One function that can put a candidate into the shared pool.
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct ProducerRow {
    /// Stable `module::function` (or `module::Type::function`) identity.
    pub id: String,
    /// Workspace package declaring the function.
    pub package: String,
    pub candidate_class: CandidateClass,
    pub tier: Tier,
    pub identity: IdentityDisposition,
    pub insertion_plan: InsertionDisposition,
    pub evidence: EvidenceDisposition,
    pub rank: RankDisposition,
    pub source_completeness: CompletenessDisposition,
    pub finalizer_route: FinalizerRoute,
    /// How `reached_by` was established. `direct_call` is reconciled against
    /// the entry-point call sites; `provider_seam` is declared.
    pub reach_evidence: ReachEvidence,
    /// Entry points that reach this producer.
    pub reached_by: Vec<String>,
    /// Issue owning the migration off compatibility, or the retirement.
    pub migration_owner: String,
    /// Required when `rank` is `compatibility_permanent_reviewed`.
    #[serde(default)]
    pub reviewed_reason: Option<String>,
    /// Required when `reached_by` is not every entry point.
    #[serde(default)]
    pub route_divergence_owner: Option<String>,
    /// Required alongside `route_divergence_owner`.
    #[serde(default)]
    pub route_divergence_note: Option<String>,
    /// Required when `reach_evidence` is `unreachable`, and forbidden
    /// otherwise. Says why a producer with no shipped caller still exists.
    #[serde(default)]
    pub unreachable_reason: Option<String>,
    /// What a reader must not infer from this row.
    pub limitations: String,
    pub note: String,
}

/// A `providers::` module the scanned surface reaches into but does not scan.
///
/// Candidate production delegated out of the scanned tree is how a live
/// producer stays invisible while its facade sits in the denominator: the
/// inventoried function returns candidates it did not build. Every such module
/// is either brought into the scan roots or dispositioned here.
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct DelegationRow {
    /// Module name under `providers::`.
    pub module: String,
    /// Why reaching into it does not put a producer outside the denominator.
    pub reason: String,
}

/// A file that constructs candidates but exposes no append-channel function.
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct ConstructionOnlyRow {
    pub path: String,
    /// Why constructing here is not a producer seam of its own.
    pub reason: String,
    /// Producer row that owns the candidates this file builds.
    pub consumed_by: String,
}

/// Candidate classes the standard completion surface offers.
#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum CandidateClass {
    /// Local lexical and package symbols.
    LocalSymbol,
    /// Compiler-visible import, default, and tag projections.
    ImportProjection,
    /// Workspace symbol index results and fallback.
    WorkspaceSymbol,
    /// Receiver and method candidates.
    Method,
    /// Module and include-root candidates.
    Module,
    /// Declared dependency enrichment.
    DeclaredDependency,
    /// Server-authored builtin catalogue.
    Builtin,
    /// Server-authored keyword catalogue.
    Keyword,
    /// Server-authored snippet catalogue.
    Snippet,
    /// Hash keys, constants, and properties.
    KeyOrConstant,
    /// Filesystem path candidates.
    FilePath,
    /// Regex-context candidates.
    RegexContext,
    /// Test-framework candidates.
    TestFramework,
    /// XS binding candidates.
    XsApi,
    /// Framework or generated members, such as the Dancer2 DSL.
    FrameworkGenerated,
    /// Degraded route used when no AST is available.
    LexicalFallback,
    /// Routing seam that contributes no candidates of its own.
    Router,
    /// Shared merge, rank, and cap.
    Finalizer,
}

impl CandidateClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LocalSymbol => "local_symbol",
            Self::ImportProjection => "import_projection",
            Self::WorkspaceSymbol => "workspace_symbol",
            Self::Method => "method",
            Self::Module => "module",
            Self::DeclaredDependency => "declared_dependency",
            Self::Builtin => "builtin",
            Self::Keyword => "keyword",
            Self::Snippet => "snippet",
            Self::KeyOrConstant => "key_or_constant",
            Self::FilePath => "file_path",
            Self::RegexContext => "regex_context",
            Self::TestFramework => "test_framework",
            Self::XsApi => "xs_api",
            Self::FrameworkGenerated => "framework_generated",
            Self::LexicalFallback => "lexical_fallback",
            Self::Router => "router",
            Self::Finalizer => "finalizer",
        }
    }
}

/// Where a row sits between the LSP entry point and the shared pool.
#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    /// Runtime enrichment called directly by an entry point.
    RuntimeEnrichment,
    /// Provider-side producer reached through the provider call.
    CoreProvider,
    /// Shared finalization or presentation seam.
    SharedFinalizer,
}

impl Tier {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RuntimeEnrichment => "runtime_enrichment",
            Self::CoreProvider => "core_provider",
            Self::SharedFinalizer => "shared_finalizer",
        }
    }
}

/// Identity a candidate from this producer carries into merge.
#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum IdentityDisposition {
    /// Canonical semantic entity plus projection.
    SemanticEntityProjection,
    /// Source-backed anchor where the canonical entity is not available yet.
    SourceAnchoredProjection,
    /// Reviewed stable identity for server-authored catalogue items.
    StableMetadataCandidate,
    /// Label-only compatibility, pending migration.
    LegacyLabelCompatibility,
    /// Contributes no candidate identity of its own.
    NotApplicable,
    /// A candidate reaching merge with no identity at all. Never admissible.
    UnidentifiedForbidden,
}

impl IdentityDisposition {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SemanticEntityProjection => "semantic_entity_projection",
            Self::SourceAnchoredProjection => "source_anchored_projection",
            Self::StableMetadataCandidate => "stable_metadata_candidate",
            Self::LegacyLabelCompatibility => "legacy_label_compatibility",
            Self::NotApplicable => "not_applicable",
            Self::UnidentifiedForbidden => "unidentified_forbidden",
        }
    }

    fn forbidden(self) -> bool {
        matches!(self, Self::UnidentifiedForbidden)
    }
}

/// Who owns the edit a candidate applies.
#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum InsertionDisposition {
    /// Routing or finalization seam that plans no insertion.
    NotApplicable,
    /// Plain label insertion with no additional edit.
    None,
    /// The canonical insertion planner owns the edit.
    CanonicalPlanner,
    /// The source owns a reviewed local plan.
    SourceOwnedSafePlan,
    /// Legacy compatibility plan, pending migration.
    LegacyCompatibility,
    /// An edit with no owner. Never admissible.
    ConflictingOrUnowned,
}

impl InsertionDisposition {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotApplicable => "not_applicable",
            Self::None => "none",
            Self::CanonicalPlanner => "canonical_planner",
            Self::SourceOwnedSafePlan => "source_owned_safe_plan",
            Self::LegacyCompatibility => "legacy_compatibility",
            Self::ConflictingOrUnowned => "conflicting_or_unowned",
        }
    }

    fn forbidden(self) -> bool {
        matches!(self, Self::ConflictingOrUnowned)
    }
}

/// Evidence and currentness behind the candidate.
#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceDisposition {
    /// Routing or finalization seam that carries its callees' evidence and
    /// establishes none of its own.
    NotApplicable,
    /// Accepted semantic snapshot for the request.
    AcceptedSemanticSnapshot,
    /// Bound to the current document generation.
    CurrentDocumentGeneration,
    /// Generated from a source anchor.
    SourceAnchoredGenerated,
    /// Static server-authored metadata with no document binding.
    StaticServerMetadata,
    /// Bounded fallback evidence with known limits.
    BoundedFallback,
    /// Evidence not established. Never admissible.
    UnknownForbidden,
}

impl EvidenceDisposition {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotApplicable => "not_applicable",
            Self::AcceptedSemanticSnapshot => "accepted_semantic_snapshot",
            Self::CurrentDocumentGeneration => "current_document_generation",
            Self::SourceAnchoredGenerated => "source_anchored_generated",
            Self::StaticServerMetadata => "static_server_metadata",
            Self::BoundedFallback => "bounded_fallback",
            Self::UnknownForbidden => "unknown_forbidden",
        }
    }

    fn forbidden(self) -> bool {
        matches!(self, Self::UnknownForbidden)
    }
}

/// Whether the producer supplies typed rank dimensions.
#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum RankDisposition {
    /// Supplies typed dimensions the finalizer consumes.
    Typed,
    /// Relies on the compatibility projection and has a named exit.
    CompatibilityWithExit,
    /// Reviewed permanent compatibility with a recorded reason.
    CompatibilityPermanentReviewed,
    /// Contributes no rank input.
    NotApplicable,
    /// Rank behavior not established. Never admissible.
    UnclassifiedForbidden,
}

impl RankDisposition {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Typed => "typed",
            Self::CompatibilityWithExit => "compatibility_with_exit",
            Self::CompatibilityPermanentReviewed => "compatibility_permanent_reviewed",
            Self::NotApplicable => "not_applicable",
            Self::UnclassifiedForbidden => "unclassified_forbidden",
        }
    }

    fn forbidden(self) -> bool {
        matches!(self, Self::UnclassifiedForbidden)
    }
}

/// What the producer can say about its own completeness.
///
/// `legacy_unreported` is the honest current state for every unmigrated row:
/// the source returns candidates and says nothing about whether it finished.
/// #10230 may not report `Complete` while any reached row is
/// `legacy_unreported`, which is precisely why the state is named rather than
/// folded into `complete_or_empty`.
#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum CompletenessDisposition {
    /// Reports an exact complete result, empty included.
    CompleteOrEmpty,
    /// Reports a partial result with stated limitations.
    QualifiedPartial,
    /// Can report that its inputs were not ready.
    NotReady,
    /// Can report cancellation or deadline exhaustion.
    CancelledOrDeadline,
    /// Refuses a dynamic or unsupported shape rather than guessing.
    DynamicOrUnsupported,
    /// Reports nothing about completeness. Blocks a `Complete` outcome.
    LegacyUnreported,
}

impl CompletenessDisposition {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CompleteOrEmpty => "complete_or_empty",
            Self::QualifiedPartial => "qualified_partial",
            Self::NotReady => "not_ready",
            Self::CancelledOrDeadline => "cancelled_or_deadline",
            Self::DynamicOrUnsupported => "dynamic_or_unsupported",
            Self::LegacyUnreported => "legacy_unreported",
        }
    }
}

/// How the producer's candidates reach finalization.
#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum FinalizerRoute {
    /// Contributes typed candidates to the shared pool.
    SharedCandidatePool,
    /// Enters the shared finalizer through the legacy adapter.
    CompatibilityAdapterBeforeSharedFinalizer,
    /// Is the shared finalizer.
    IsSharedFinalizer,
    /// Reaches the client without passing the shared finalizer. Never
    /// admissible.
    OutsideFinalizerForbidden,
}

impl FinalizerRoute {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SharedCandidatePool => "shared_candidate_pool",
            Self::CompatibilityAdapterBeforeSharedFinalizer => {
                "compatibility_adapter_before_shared_finalizer"
            }
            Self::IsSharedFinalizer => "is_shared_finalizer",
            Self::OutsideFinalizerForbidden => "outside_finalizer_forbidden",
        }
    }

    fn forbidden(self) -> bool {
        matches!(self, Self::OutsideFinalizerForbidden)
    }
}

/// Whether `reached_by` is proven or declared.
#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ReachEvidence {
    /// The entry point names this function directly; reconciled against source.
    DirectCall,
    /// Reached through the provider call; declared, not proven here.
    ProviderSeam,
    /// No shipped entry point reaches it. Recorded rather than dropped: a
    /// producer that exists but serves no request is the obvious wrong place
    /// for a future fix to land, and #10229 needs to know it is there.
    Unreachable,
}

impl ReachEvidence {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DirectCall => "direct_call",
            Self::ProviderSeam => "provider_seam",
            Self::Unreachable => "unreachable",
        }
    }
}

// ---------------------------------------------------------------------------
// Discovery model
// ---------------------------------------------------------------------------

/// One function discovered to carry the candidate channel.
#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct DiscoveredProducer {
    pub id: String,
    pub package: String,
    pub file: String,
    pub function: String,
    /// How the function carries candidates.
    pub channel: Channel,
    /// Declarations sharing this id. Greater than one means mutually exclusive
    /// `cfg` arms of one logical producer, which is a single row.
    pub declarations: usize,
}

/// How a producer moves candidates.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    /// Appends into a caller-owned vector.
    Append,
    /// Returns candidates by value.
    Returned,
    /// Both, on different parameters.
    AppendAndReturned,
}

impl Channel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Append => "append",
            Self::Returned => "returned",
            Self::AppendAndReturned => "append_and_returned",
        }
    }

    fn merge(self, other: Self) -> Self {
        if self == other { self } else { Self::AppendAndReturned }
    }
}

/// The mechanical population derived from the working tree.
#[derive(Debug, Default, Serialize)]
pub struct Discovered {
    pub producers: Vec<DiscoveredProducer>,
    /// Files that construct `CompletionItem` values.
    pub construction_files: BTreeSet<String>,
    /// Files carrying at least one discovered producer.
    pub producer_files: BTreeSet<String>,
    /// `providers::` modules the scanned files reference, to the files doing so.
    pub provider_references: BTreeMap<String, BTreeSet<String>>,
    /// Entry point to the functions it calls directly.
    pub entry_direct_calls: BTreeMap<String, BTreeSet<String>>,
    /// Entry points that append to the candidate binding after the finalizer.
    pub post_finalizer_appends: Vec<String>,
    pub source_files: Vec<String>,
    pub source_digest: String,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// `cargo xtask completion-candidates <verb>`.
#[derive(clap::Subcommand, Debug)]
pub enum CompletionCandidatesSubcommand {
    /// Reconcile the ledger against current source and require the projection
    /// to be current. Never writes.
    Check,
    /// List producers grouped by candidate class, with identity and owner.
    List,
    /// Emit the cheap-agent packet for one producer id.
    Explain {
        /// Producer id, as printed by `list`.
        producer_id: String,
    },
    /// Regenerate the checked-in projection from the ledger and source.
    Graph {
        /// Print to stdout instead of writing the projection.
        #[arg(long)]
        stdout: bool,
    },
}

/// Run one verb.
pub fn run(command: CompletionCandidatesSubcommand) -> Result<()> {
    let root = project_root()?;
    let ledger = load(&root)?;
    let discovered = discover(&root)?;

    match command {
        CompletionCandidatesSubcommand::Check => {
            validate(&ledger, &discovered)?;
            let markdown = render_markdown(&ledger, &discovered);
            let path = root.join(PROJECTION_PATH);
            let existing = fs::read_to_string(&path)
                .wrap_err_with(|| format!("failed to read {PROJECTION_PATH}"))?;
            if normalize_newlines(&existing) != markdown {
                bail!("{PROJECTION_PATH} is stale; run `cargo xtask completion-candidates graph`");
            }
            println!(
                "completion candidate inventory is valid and current: {} producers across {} \
                 classes, {} construction-only files, {} post-finalizer appends",
                ledger.producers.len(),
                class_counts(&ledger).len(),
                ledger.construction_only.len(),
                discovered.post_finalizer_appends.len()
            );
            Ok(())
        }
        CompletionCandidatesSubcommand::List => {
            validate(&ledger, &discovered)?;
            print!("{}", render_list(&ledger));
            Ok(())
        }
        CompletionCandidatesSubcommand::Explain { producer_id } => {
            validate(&ledger, &discovered)?;
            let row =
                ledger.producers.iter().find(|row| row.id == producer_id).ok_or_else(|| {
                    color_eyre::eyre::eyre!(
                        "no producer `{producer_id}` in {LEDGER_PATH}; run `cargo xtask \
                         completion-candidates list`"
                    )
                })?;
            print!("{}", render_explain(&ledger, &discovered, row));
            Ok(())
        }
        CompletionCandidatesSubcommand::Graph { stdout } => {
            validate(&ledger, &discovered)?;
            let markdown = render_markdown(&ledger, &discovered);
            if stdout {
                print!("{markdown}");
                return Ok(());
            }
            let path = root.join(PROJECTION_PATH);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .wrap_err_with(|| format!("failed to create {}", parent.display()))?;
            }
            fs::write(&path, &markdown)
                .wrap_err_with(|| format!("failed to write {PROJECTION_PATH}"))?;
            println!("wrote {PROJECTION_PATH} from {} producers", ledger.producers.len());
            Ok(())
        }
    }
}

/// Read the authored ledger from the repository root.
fn load(root: &Path) -> Result<Ledger> {
    let path = root.join(LEDGER_PATH);
    let text =
        fs::read_to_string(&path).wrap_err_with(|| format!("failed to read {LEDGER_PATH}"))?;
    parse(&text)
}

/// Parse the authored ledger. Unknown fields are rejected so a typo cannot
/// silently drop a disposition.
pub fn parse(text: &str) -> Result<Ledger> {
    toml::from_str(text).wrap_err_with(|| format!("failed to parse {LEDGER_PATH}"))
}

/// Compare generated against checked-in content by logical line, so a CRLF
/// checkout does not report drift the author cannot repair.
fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n")
}

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

/// Build the mechanical population from the working tree.
pub fn discover(root: &Path) -> Result<Discovered> {
    let files = scanned_files(root)?;

    let mut producers = Vec::new();
    let mut construction_files = BTreeSet::new();
    let mut producer_files = BTreeSet::new();

    for file in &files {
        let source = fs::read_to_string(root.join(file))
            .wrap_err_with(|| format!("failed to read {file}"))?;
        let parsed = syn::parse_file(&source)
            .wrap_err_with(|| format!("failed to parse {file} with syn"))?;

        let mut visitor = SeamVisitor::new(file);
        visitor.visit_file(&parsed);

        if visitor.constructions > 0 {
            construction_files.insert(file.clone());
        }
        if !visitor.producers.is_empty() {
            producer_files.insert(file.clone());
        }
        producers.extend(visitor.producers);
    }
    let producers = merge_declarations(producers)?;

    let (entry_direct_calls, post_finalizer_appends) = discover_entry_routes(root)?;
    let source_digest = digest_files(root, &files)?;
    let provider_references = discover_provider_references(root, &files)?;

    Ok(Discovered {
        producers,
        construction_files,
        producer_files,
        provider_references,
        entry_direct_calls,
        post_finalizer_appends,
        source_files: files,
        source_digest,
    })
}

/// Collapse the mutually exclusive `cfg` arms of one logical producer into a
/// single row.
///
/// A function declared twice under `#[cfg(target_arch = "wasm32")]` and its
/// negation is one producer with two bodies, not two producers: it has one
/// disposition and one owner. Two declarations of the same id in *different*
/// files are not that, and are refused, because the id would no longer say
/// which source a row was audited against.
fn merge_declarations(producers: Vec<DiscoveredProducer>) -> Result<Vec<DiscoveredProducer>> {
    let mut merged: BTreeMap<String, DiscoveredProducer> = BTreeMap::new();
    for producer in producers {
        match merged.get_mut(&producer.id) {
            None => {
                merged.insert(producer.id.clone(), producer);
            }
            Some(existing) => {
                if existing.file != producer.file {
                    bail!(
                        "`{}` is declared in both {} and {}; a producer id must name one source, \
                         otherwise a row cannot say which declaration it was audited against",
                        producer.id,
                        existing.file,
                        producer.file
                    );
                }
                existing.declarations += 1;
                existing.channel = existing.channel.merge(producer.channel);
            }
        }
    }
    Ok(merged.into_values().collect())
}

/// `providers::<module>` names the scanned files reach into, mapped to the
/// files that reach.
///
/// Textual rather than resolved: the question is only which sibling provider
/// modules this surface depends on at all, and a `use` or a fully qualified
/// call are equally good evidence of that.
fn discover_provider_references(
    root: &Path,
    files: &[String],
) -> Result<BTreeMap<String, BTreeSet<String>>> {
    let pattern = regex::Regex::new(r"(?:crate|perl_lsp_rs_core)::providers::([a-z_0-9]+)")
        .wrap_err("failed to compile the provider-reference pattern")?;
    let mut references: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for file in files {
        let source = fs::read_to_string(root.join(file))
            .wrap_err_with(|| format!("failed to read {file}"))?;
        for capture in pattern.captures_iter(&source) {
            let Some(module) = capture.get(1) else {
                continue;
            };
            references.entry(module.as_str().to_string()).or_default().insert(file.clone());
        }
    }
    Ok(references)
}

/// Is every file of this `providers::` module inside the scan roots?
///
/// A root naming a single file does not cover its module: the rest of that
/// module is unscanned, and a producer there would be invisible.
fn module_is_fully_scanned(module: &str) -> bool {
    let directory = format!("crates/perl-lsp-rs-core/src/providers/{module}/");
    SCAN_ROOTS.iter().any(|root| root.ends_with('/') && directory.starts_with(root))
}

/// Tracked Rust files under the scan roots, minus the declared test surfaces.
///
/// Tracked rather than walked: an untracked scratch file next to a provider
/// must not silently join or leave the denominator between two runs.
fn scanned_files(root: &Path) -> Result<Vec<String>> {
    let tracked = tracked_files(root)?;
    let mut files: Vec<String> = tracked
        .into_iter()
        .filter(|path| path.ends_with(".rs"))
        .filter(|path| SCAN_ROOTS.iter().any(|scope| path.starts_with(scope) || path == scope))
        .filter(|path| !TEST_SURFACE_FILES.contains(&path.as_str()))
        .collect();
    files.sort();
    files.dedup();
    if files.is_empty() {
        bail!(
            "no tracked Rust source found under the completion scan roots; the inventory would \
             report an empty denominator rather than fail, so this stops the run"
        );
    }

    // An untracked `.rs` file under a scan root is the one way a real producer
    // can sit in the working tree and contribute nothing to the digest or the
    // population: `git ls-files` cannot see it, so `check` would report a
    // green tree it has not actually inspected. Refusing is the honest
    // outcome — the run cannot speak for a tree it is blind to.
    let untracked = untracked_scan_root_files(root)?;
    if !untracked.is_empty() {
        bail!(
            "untracked Rust file(s) under the completion scan roots: {}.\n\
             Discovery reads tracked files, so an untracked producer would not appear in the \
             denominator and this check would report a green tree it had not inspected. \
             `git add` them (or remove them) and run again.",
            untracked.join(", ")
        );
    }

    Ok(files)
}

/// Untracked, non-ignored `.rs` paths under the scan roots.
fn untracked_scan_root_files(root: &Path) -> Result<Vec<String>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z", "--others", "--exclude-standard"])
        .output()
        .wrap_err("failed to run `git ls-files --others`")?;
    if !output.status.success() {
        bail!(
            "`git ls-files --others` failed with status {}{}",
            output.status,
            first_stderr_line(&output.stderr)
        );
    }
    let mut paths: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .split('\0')
        .filter(|entry| !entry.is_empty())
        .filter(|entry| entry.ends_with(".rs"))
        .filter(|entry| SCAN_ROOTS.iter().any(|scope| entry.starts_with(scope) || entry == scope))
        .map(str::to_string)
        .collect();
    paths.sort();
    Ok(paths)
}

/// Repository-tracked paths, so discovery matches what review can see.
fn tracked_files(root: &Path) -> Result<Vec<String>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z"])
        .output()
        .wrap_err("failed to run `git ls-files`")?;
    if !output.status.success() {
        // An exit status alone is not diagnosable: the usual causes — not a
        // repository, an unreadable index, a permissions failure — are
        // distinguishable only from what git said.
        bail!(
            "`git ls-files` failed with status {}{}",
            output.status,
            first_stderr_line(&output.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .split('\0')
        .filter(|entry| !entry.is_empty())
        .map(str::to_string)
        .collect())
}

/// Longest child-stderr excerpt carried into an error message.
const MAX_STDERR_DIAGNOSTIC: usize = 200;

/// The first non-empty stderr line, bounded, ready to append to an error.
///
/// Bounded rather than whole: a failing child can write an arbitrary amount to
/// stderr, and an error message that dumps it is unreadable in a CI log. The
/// first line carries the cause.
fn first_stderr_line(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let Some(line) = text.lines().map(str::trim).find(|line| !line.is_empty()) else {
        return String::new();
    };
    let mut excerpt: String = line.chars().take(MAX_STDERR_DIAGNOSTIC).collect();
    if line.chars().count() > MAX_STDERR_DIAGNOSTIC {
        excerpt.push('…');
    }
    format!(": {excerpt}")
}

/// Package owning a scanned path.
fn package_for(file: &str) -> &'static str {
    if file.starts_with("crates/perl-lsp-rs-core/") { "perl-lsp-rs-core" } else { "perl-lsp-rs" }
}

/// `crate_module::path::segments` for a source file, used to build stable ids.
fn module_path_for(file: &str) -> String {
    let package = package_for(file);
    let crate_module = package.replace('-', "_");
    let prefix = format!("crates/{package}/src/");
    let relative = file.strip_prefix(&prefix).unwrap_or(file);
    let stem = relative.strip_suffix(".rs").unwrap_or(relative);
    let stem = stem.strip_suffix("/mod").unwrap_or(stem);
    if stem.is_empty() || stem == "lib" {
        return crate_module;
    }
    format!("{crate_module}::{}", stem.replace('/', "::"))
}

/// SHA-256 over an ordered file list, binding each path to its bytes.
fn digest_files(root: &Path, files: &[String]) -> Result<String> {
    let mut hasher = Sha256::new();
    for path in files {
        let bytes = fs::read(root.join(path)).wrap_err_with(|| format!("failed to read {path}"))?;
        // Length-prefix each field so a rename cannot collide with a content
        // change that happens to shift bytes across the boundary.
        hasher.update((path.len() as u64).to_le_bytes());
        hasher.update(path.as_bytes());
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
    }
    let hex: String = hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(format!("sha256:{hex}"))
}

/// Collects append-channel functions and candidate constructions in one file.
struct SeamVisitor<'a> {
    file: &'a str,
    module: String,
    package: &'static str,
    impl_type: Option<String>,
    producers: Vec<DiscoveredProducer>,
    constructions: usize,
}

impl<'a> SeamVisitor<'a> {
    fn new(file: &'a str) -> Self {
        Self {
            file,
            module: module_path_for(file),
            package: package_for(file),
            impl_type: None,
            producers: Vec::new(),
            constructions: 0,
        }
    }

    fn record(&mut self, name: &str, sig: &syn::Signature) {
        let appends = sig.inputs.iter().any(takes_append_channel);
        let returns = match &sig.output {
            syn::ReturnType::Type(_, ty) => mentions_candidate_vec(ty),
            syn::ReturnType::Default => false,
        };
        let channel = match (appends, returns) {
            (true, true) => Channel::AppendAndReturned,
            (true, false) => Channel::Append,
            (false, true) => Channel::Returned,
            (false, false) => return,
        };
        let id = match &self.impl_type {
            Some(ty) => format!("{}::{ty}::{name}", self.module),
            None => format!("{}::{name}", self.module),
        };
        self.producers.push(DiscoveredProducer {
            id,
            package: self.package.to_string(),
            file: self.file.to_string(),
            function: name.to_string(),
            channel,
            declarations: 1,
        });
    }
}

impl<'ast> Visit<'ast> for SeamVisitor<'_> {
    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        if has_cfg_test(&node.attrs) {
            return;
        }
        syn::visit::visit_item_mod(self, node);
    }

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        if has_test_attribute(&node.attrs) || has_cfg_test(&node.attrs) {
            return;
        }
        self.record(&node.sig.ident.to_string(), &node.sig);
        syn::visit::visit_item_fn(self, node);
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        if has_cfg_test(&node.attrs) {
            return;
        }
        let previous = self.impl_type.take();
        self.impl_type = impl_type_name(node);
        syn::visit::visit_item_impl(self, node);
        self.impl_type = previous;
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        if has_test_attribute(&node.attrs) || has_cfg_test(&node.attrs) {
            return;
        }
        self.record(&node.sig.ident.to_string(), &node.sig);
        syn::visit::visit_impl_item_fn(self, node);
    }

    fn visit_expr_struct(&mut self, node: &'ast syn::ExprStruct) {
        if last_segment_is(&node.path, "CompletionItem") {
            self.constructions += 1;
        }
        syn::visit::visit_expr_struct(self, node);
    }
}

/// Candidate element types. `CompletionItem` is today's compatibility shape and
/// `CompletionCandidate` is the migration target, so an inventory that tracks
/// migration has to watch both.
const CANDIDATE_TYPES: &[&str] = &["CompletionItem", "CompletionCandidate"];

/// Is this parameter a `&mut Vec<Candidate>` append channel?
fn takes_append_channel(arg: &syn::FnArg) -> bool {
    let syn::FnArg::Typed(typed) = arg else {
        return false;
    };
    let syn::Type::Reference(reference) = typed.ty.as_ref() else {
        return false;
    };
    reference.mutability.is_some() && is_candidate_vec(&reference.elem)
}

/// Does this type carry a candidate vector anywhere in its shape?
///
/// Recursive because a producer may return `Vec<CompletionItem>` directly, or
/// wrapped — `(Vec<CompletionItem>, bool)` is how the runtime finalizer returns
/// its page and its incompleteness flag together.
fn mentions_candidate_vec(ty: &syn::Type) -> bool {
    match ty {
        syn::Type::Path(path) => {
            if is_candidate_vec(ty) {
                return true;
            }
            path.path.segments.iter().any(|segment| match &segment.arguments {
                syn::PathArguments::AngleBracketed(args) => args.args.iter().any(|arg| match arg {
                    syn::GenericArgument::Type(inner) => mentions_candidate_vec(inner),
                    _ => false,
                }),
                _ => false,
            })
        }
        syn::Type::Tuple(tuple) => tuple.elems.iter().any(mentions_candidate_vec),
        syn::Type::Reference(reference) => mentions_candidate_vec(&reference.elem),
        syn::Type::Paren(paren) => mentions_candidate_vec(&paren.elem),
        syn::Type::Group(group) => mentions_candidate_vec(&group.elem),
        _ => false,
    }
}

/// Is this type exactly `Vec<Candidate>`?
fn is_candidate_vec(ty: &syn::Type) -> bool {
    let syn::Type::Path(path) = ty else {
        return false;
    };
    let Some(segment) = path.path.segments.last() else {
        return false;
    };
    if segment.ident != "Vec" {
        return false;
    }
    let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
        return false;
    };
    args.args.iter().any(|arg| match arg {
        syn::GenericArgument::Type(syn::Type::Path(inner)) => {
            CANDIDATE_TYPES.iter().any(|name| last_segment_is(&inner.path, name))
        }
        _ => false,
    })
}

fn last_segment_is(path: &syn::Path, name: &str) -> bool {
    path.segments.last().is_some_and(|segment| segment.ident == name)
}

/// Name segment for a producer id declared inside an `impl` block.
///
/// A trait impl is qualified `<Type as Trait>`, because the type alone does not
/// identify the method: Rust permits two traits with the same method name on
/// one type, and dropping the trait would give both the same producer id. Two
/// same-id declarations in one file are indistinguishable from the mutually
/// exclusive `cfg` arms `merge_declarations` legitimately collapses, so an
/// unqualified trait method could join an existing row's declaration count and
/// acquire its disposition without ever needing one of its own.
fn impl_type_name(node: &syn::ItemImpl) -> Option<String> {
    let self_ty = type_name(node.self_ty.as_ref())?;
    match &node.trait_ {
        Some((path, _)) => {
            let trait_name = path
                .segments
                .last()
                .map(|segment| segment.ident.to_string())
                .unwrap_or_else(|| "?".to_string());
            Some(format!("<{self_ty} as {trait_name}>"))
        }
        None => Some(self_ty),
    }
}

fn type_name(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(path) => path.path.segments.last().map(|segment| segment.ident.to_string()),
        _ => None,
    }
}

/// Does this item exist only under `cfg(test)`?
///
/// Only `all(..)` is descended into. The distinction is not pedantic:
/// `all(test, feature = "x")` compiles solely under test and is proof, while
/// `any(test, feature = "x")` and `not(test)` both ship, so their items are
/// product source and need a disposition like any other producer.
fn has_cfg_test(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if !attr.path().is_ident("cfg") {
            return false;
        }
        let syn::Meta::List(list) = &attr.meta else {
            return false;
        };
        cfg_list_is_test_only(list)
    })
}

fn cfg_list_is_test_only(list: &syn::MetaList) -> bool {
    let Ok(predicates) = list.parse_args_with(
        syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
    ) else {
        // An unparseable predicate is not evidence of test-only code, and
        // treating it as product source keeps the denominator inclusive.
        return false;
    };
    predicates.iter().any(cfg_predicate_is_test_only)
}

fn cfg_predicate_is_test_only(meta: &syn::Meta) -> bool {
    match meta {
        syn::Meta::Path(path) => path.is_ident("test"),
        syn::Meta::List(list) if list.path.is_ident("all") => cfg_list_is_test_only(list),
        _ => false,
    }
}

fn has_test_attribute(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| attr.path().segments.last().is_some_and(|s| s.ident == "test"))
}

// ---------------------------------------------------------------------------
// Entry-point route discovery
// ---------------------------------------------------------------------------

/// For each entry point, the functions it calls directly, plus whether it
/// appends to the candidate binding after the finalizer.
fn discover_entry_routes(root: &Path) -> Result<(BTreeMap<String, BTreeSet<String>>, Vec<String>)> {
    let source = fs::read_to_string(root.join(ENTRY_FILE))
        .wrap_err_with(|| format!("failed to read {ENTRY_FILE}"))?;
    let parsed =
        syn::parse_file(&source).wrap_err_with(|| format!("failed to parse {ENTRY_FILE}"))?;

    let mut collector = EntryCollector::default();
    collector.visit_file(&parsed);

    for entry in ENTRY_POINTS {
        if !collector.calls.contains_key(*entry) {
            bail!(
                "{ENTRY_FILE} no longer declares the entry point `{entry}`; the inventory's \
                 reachability model names it, so the ledger and this task must be updated \
                 together rather than silently losing a route"
            );
        }
    }

    Ok((collector.calls, collector.post_finalizer_appends))
}

#[derive(Default)]
struct EntryCollector {
    calls: BTreeMap<String, BTreeSet<String>>,
    post_finalizer_appends: Vec<String>,
}

impl<'ast> Visit<'ast> for EntryCollector {
    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        if has_cfg_test(&node.attrs) {
            return;
        }
        syn::visit::visit_item_mod(self, node);
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        let name = node.sig.ident.to_string();
        if !ENTRY_POINTS.contains(&name.as_str()) {
            return;
        }
        let mut body = EntryBodyVisitor::default();
        body.visit_block(&node.block);
        if body.appended_after_finalizer {
            self.post_finalizer_appends.push(name.clone());
        }
        self.calls.insert(name, body.called);
    }
}

/// Walks one entry-point body in syntactic order.
///
/// Order matters for exactly one question: does a candidate append occur after
/// the finalizer call? `syn`'s visitor descends statements and expression
/// fields in source order, so a monotonic "have I passed the finalizer yet"
/// flag answers it without needing span locations.
struct EntryBodyVisitor {
    called: BTreeSet<String>,
    seen_finalizer: bool,
    appended_after_finalizer: bool,
    /// Every local name currently holding the candidate collection.
    ///
    /// Matching one fixed identifier is not enough: `let mut smuggled =
    /// completions;` renames the page, and an append through the new name
    /// reaches the client exactly as the original would have. Following `let`
    /// bindings whose initializer mentions a known candidate name keeps the
    /// control attached to the value rather than to the spelling.
    bindings: BTreeSet<String>,
}

impl Default for EntryBodyVisitor {
    fn default() -> Self {
        Self {
            called: BTreeSet::new(),
            seen_finalizer: false,
            appended_after_finalizer: false,
            bindings: BTreeSet::from([CANDIDATE_BINDING.to_string()]),
        }
    }
}

impl EntryBodyVisitor {
    /// A candidate append is `<candidate>.push(..)`/`.extend(..)`, or handing
    /// `&mut <candidate>` to something else.
    fn note_append(&mut self) {
        if self.seen_finalizer {
            self.appended_after_finalizer = true;
        }
    }

    fn holds_candidates(&self, expr: &syn::Expr) -> bool {
        match expr {
            syn::Expr::Path(path) => path
                .path
                .get_ident()
                .is_some_and(|ident| self.bindings.contains(&ident.to_string())),
            syn::Expr::Reference(reference) => self.holds_candidates(&reference.expr),
            syn::Expr::Paren(paren) => self.holds_candidates(&paren.expr),
            syn::Expr::Group(group) => self.holds_candidates(&group.expr),
            _ => false,
        }
    }

    /// Does this initializer carry the candidate collection anywhere inside it?
    ///
    /// Deliberately broad: `std::mem::take(&mut completions)` and a bare
    /// `completions` both hand the page to the new name.
    fn initializer_carries_candidates(&self, expr: &syn::Expr) -> bool {
        if self.holds_candidates(expr) {
            return true;
        }
        let mut probe = CandidateMentionProbe { bindings: &self.bindings, found: false };
        probe.visit_expr(expr);
        probe.found
    }
}

/// Finds a mention of any known candidate binding anywhere in an expression.
struct CandidateMentionProbe<'a> {
    bindings: &'a BTreeSet<String>,
    found: bool,
}

impl<'ast> Visit<'ast> for CandidateMentionProbe<'_> {
    fn visit_expr_path(&mut self, node: &'ast syn::ExprPath) {
        if node.path.get_ident().is_some_and(|ident| self.bindings.contains(&ident.to_string())) {
            self.found = true;
        }
        syn::visit::visit_expr_path(self, node);
    }
}

/// Names bound by a `let` pattern, so a rebinding can be tracked.
fn pattern_idents(pat: &syn::Pat, out: &mut Vec<String>) {
    match pat {
        syn::Pat::Ident(ident) => out.push(ident.ident.to_string()),
        syn::Pat::Tuple(tuple) => {
            for elem in &tuple.elems {
                pattern_idents(elem, out);
            }
        }
        syn::Pat::Type(typed) => pattern_idents(&typed.pat, out),
        syn::Pat::Reference(reference) => pattern_idents(&reference.pat, out),
        syn::Pat::Paren(paren) => pattern_idents(&paren.pat, out),
        _ => {}
    }
}

impl<'ast> Visit<'ast> for EntryBodyVisitor {
    fn visit_local(&mut self, node: &'ast syn::Local) {
        if let Some(init) = &node.init {
            // Visit the initializer first: it is evaluated before the new name
            // exists, and it may itself be the finalizer call.
            syn::visit::visit_local_init(self, init);
            if self.initializer_carries_candidates(&init.expr) {
                let mut names = Vec::new();
                pattern_idents(&node.pat, &mut names);
                self.bindings.extend(names);
            }
        }
        syn::visit::visit_pat(self, &node.pat);
    }

    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = node.func.as_ref()
            && let Some(segment) = path.path.segments.last()
        {
            let name = segment.ident.to_string();
            self.called.insert(name.clone());
            if name == FINALIZER_CALL {
                self.seen_finalizer = true;
            }
        }
        syn::visit::visit_expr_call(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        let method = node.method.to_string();
        self.called.insert(method.clone());
        if APPEND_METHODS.contains(&method.as_str()) && self.holds_candidates(&node.receiver) {
            self.note_append();
        }
        syn::visit::visit_expr_method_call(self, node);
    }

    fn visit_expr_reference(&mut self, node: &'ast syn::ExprReference) {
        if node.mutability.is_some() && self.holds_candidates(&node.expr) {
            self.note_append();
        }
        syn::visit::visit_expr_reference(self, node);
    }
}

/// Vec methods that can put a candidate into the page.
///
/// `push`/`extend` are the shapes the tree uses; the rest are here so a
/// rewritten append is still an append.
const APPEND_METHODS: &[&str] = &[
    "push",
    "extend",
    "append",
    "insert",
    "splice",
    "extend_from_slice",
    "push_within_capacity",
    // `resize`/`resize_with` grow the page with cloned or constructed entries,
    // which is an append however it is spelled.
    "resize",
    "resize_with",
];

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Reconcile authored dispositions against discovered reality.
///
/// Every failure mode here is a way the inventory could otherwise lie to
/// #10229, #10230, or a focused producer migration.
pub fn validate(ledger: &Ledger, discovered: &Discovered) -> Result<()> {
    if ledger.schema_version != SCHEMA {
        bail!(
            "{LEDGER_PATH} declares schema_version `{}`; expected `{SCHEMA}`",
            ledger.schema_version
        );
    }
    for (field, value) in [
        ("controlling_issue", &ledger.controlling_issue),
        ("route_owner", &ledger.route_owner),
        ("finalizer_owner", &ledger.finalizer_owner),
        ("outcome_owner", &ledger.outcome_owner),
    ] {
        if !is_issue_ref(value) {
            bail!(
                "{LEDGER_PATH} field `{field}` must be an issue reference like `#10949`, got \
                 `{value}`"
            );
        }
    }

    if ledger.source_digest != discovered.source_digest {
        bail!(
            "the completion surface changed since the inventory was audited.\n  \
             ledger source_digest:  {}\n  \
             current source_digest: {}\n\
             Re-audit the producer rows against the new source, then update `source_digest` in \
             {LEDGER_PATH} and run `cargo xtask completion-candidates graph`. The {} scanned \
             files are listed in {PROJECTION_PATH}.",
            ledger.source_digest,
            discovered.source_digest,
            discovered.source_files.len()
        );
    }

    validate_producer_population(ledger, discovered)?;
    validate_construction_plane(ledger, discovered)?;
    validate_delegations(ledger, discovered)?;
    validate_dispositions(ledger)?;
    validate_owners(ledger)?;
    validate_reachability(ledger, discovered)?;
    validate_no_post_finalizer_append(discovered)?;
    Ok(())
}

/// Every discovered append-channel function has exactly one row, and every row
/// still names a function the source declares.
fn validate_producer_population(ledger: &Ledger, discovered: &Discovered) -> Result<()> {
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for row in &ledger.producers {
        if !seen.insert(row.id.as_str()) {
            bail!("{LEDGER_PATH} declares duplicate producer id `{}`", row.id);
        }
    }

    let discovered_by_id: BTreeMap<&str, &DiscoveredProducer> =
        discovered.producers.iter().map(|p| (p.id.as_str(), p)).collect();

    for producer in &discovered.producers {
        if !seen.contains(producer.id.as_str()) {
            bail!(
                "`{}` in {} takes the candidate append channel but {LEDGER_PATH} has no row for \
                 it; every producer needs exactly one disposition before it can put a candidate \
                 in front of a user",
                producer.id,
                producer.file
            );
        }
    }

    for row in &ledger.producers {
        match discovered_by_id.get(row.id.as_str()) {
            None => bail!(
                "{LEDGER_PATH} has a row for `{}` but no scanned source declares it; remove the \
                 stale row or restore the producer",
                row.id
            ),
            Some(producer) if producer.package != row.package => bail!(
                "{LEDGER_PATH} records `{}` in package `{}` but the source declares it in `{}`",
                row.id,
                row.package,
                producer.package
            ),
            Some(_) => {}
        }
    }
    Ok(())
}

/// A file that builds candidates but exposes no producer must say why.
fn validate_construction_plane(ledger: &Ledger, discovered: &Discovered) -> Result<()> {
    let declared: BTreeMap<&str, &ConstructionOnlyRow> =
        ledger.construction_only.iter().map(|row| (row.path.as_str(), row)).collect();
    if declared.len() != ledger.construction_only.len() {
        bail!("{LEDGER_PATH} declares a duplicate `construction_only` path");
    }

    let producer_ids: BTreeSet<&str> = ledger.producers.iter().map(|r| r.id.as_str()).collect();

    for file in &discovered.construction_files {
        if discovered.producer_files.contains(file) {
            continue;
        }
        let Some(row) = declared.get(file.as_str()) else {
            bail!(
                "{file} constructs `CompletionItem` values but exposes no append-channel \
                 function and has no `[[construction_only]]` row in {LEDGER_PATH}; a candidate \
                 source must not be invisible to the inventory"
            );
        };
        if !producer_ids.contains(row.consumed_by.as_str()) {
            bail!(
                "{LEDGER_PATH} construction_only row `{}` names `consumed_by = \"{}\"`, which is \
                 not a producer id in this ledger",
                row.path,
                row.consumed_by
            );
        }
    }

    for row in &ledger.construction_only {
        if !discovered.construction_files.contains(&row.path) {
            bail!(
                "{LEDGER_PATH} has a `construction_only` row for `{}` but that file no longer \
                 constructs `CompletionItem` values; remove the stale row",
                row.path
            );
        }
        if discovered.producer_files.contains(&row.path) {
            bail!(
                "{LEDGER_PATH} has a `construction_only` row for `{}` but that file now exposes \
                 an append-channel producer; it needs a producer row instead",
                row.path
            );
        }
    }
    Ok(())
}

/// Every provider module the scanned surface reaches into is either scanned in
/// full or dispositioned.
///
/// This is the third discovery plane. The append-channel and construction
/// planes both look inside the scanned tree; neither can see a producer the
/// tree delegates to. A facade that returns candidates built elsewhere would
/// otherwise satisfy the inventory while the real producer carried no
/// disposition and no owner.
fn validate_delegations(ledger: &Ledger, discovered: &Discovered) -> Result<()> {
    let declared: BTreeMap<&str, &DelegationRow> =
        ledger.delegations.iter().map(|row| (row.module.as_str(), row)).collect();
    if declared.len() != ledger.delegations.len() {
        bail!("{LEDGER_PATH} declares a duplicate `delegations` module");
    }

    for (module, files) in &discovered.provider_references {
        if module_is_fully_scanned(module) {
            continue;
        }
        let Some(row) = declared.get(module.as_str()) else {
            bail!(
                "scanned completion source reaches into `providers::{module}` ({}), which is not \
                 fully inside the scan roots and has no `[[delegations]]` row in {LEDGER_PATH}.\n\
                 Candidate production delegated out of the scanned tree is invisible to this \
                 inventory: the facade would carry a disposition while the producer carried \
                 none. Add the module to the scan roots, or record why reaching into it does not \
                 move a producer out of the denominator.",
                files.iter().cloned().collect::<Vec<_>>().join(", ")
            );
        };
        if row.reason.trim().is_empty() {
            bail!("{LEDGER_PATH} delegation row `{module}` records an empty reason");
        }
    }

    for row in &ledger.delegations {
        if module_is_fully_scanned(&row.module) {
            bail!(
                "{LEDGER_PATH} has a `delegations` row for `providers::{}`, but that module is \
                 now fully scanned; its producers carry rows, so remove the stale delegation",
                row.module
            );
        }
        if !discovered.provider_references.contains_key(&row.module) {
            bail!(
                "{LEDGER_PATH} has a `delegations` row for `providers::{}`, but no scanned file \
                 reaches into it any more; remove the stale row",
                row.module
            );
        }
    }
    Ok(())
}

/// Reject the forbidden dispositions outright, and require the evidence a
/// compatibility disposition depends on.
fn validate_dispositions(ledger: &Ledger) -> Result<()> {
    for row in &ledger.producers {
        if row.identity.forbidden() {
            bail!(
                "{LEDGER_PATH} row `{}` carries identity `{}`; an unidentified candidate cannot \
                 reach merge",
                row.id,
                row.identity.as_str()
            );
        }
        if row.insertion_plan.forbidden() {
            bail!(
                "{LEDGER_PATH} row `{}` carries insertion_plan `{}`; an edit with no owner \
                 cannot ship",
                row.id,
                row.insertion_plan.as_str()
            );
        }
        if row.evidence.forbidden() {
            bail!(
                "{LEDGER_PATH} row `{}` carries evidence `{}`; a candidate with unestablished \
                 evidence cannot be offered",
                row.id,
                row.evidence.as_str()
            );
        }
        if row.rank.forbidden() {
            bail!(
                "{LEDGER_PATH} row `{}` carries rank `{}`; every producer's rank behavior must \
                 be classified",
                row.id,
                row.rank.as_str()
            );
        }
        if row.finalizer_route.forbidden() {
            bail!(
                "{LEDGER_PATH} row `{}` carries finalizer_route `{}`; a producer outside the \
                 shared finalizer is exactly what this inventory exists to refuse",
                row.id,
                row.finalizer_route.as_str()
            );
        }

        if row.rank == RankDisposition::CompatibilityPermanentReviewed
            && row.reviewed_reason.as_deref().unwrap_or("").trim().is_empty()
        {
            bail!(
                "{LEDGER_PATH} row `{}` claims permanent rank compatibility but records no \
                 `reviewed_reason`; permanent compatibility without a reason is an unowned \
                 exception",
                row.id
            );
        }
        if row.rank != RankDisposition::CompatibilityPermanentReviewed
            && row.reviewed_reason.is_some()
        {
            bail!(
                "{LEDGER_PATH} row `{}` records a `reviewed_reason` but its rank is `{}`; the \
                 reason would not be read",
                row.id,
                row.rank.as_str()
            );
        }

        if row.limitations.trim().is_empty() {
            bail!(
                "{LEDGER_PATH} row `{}` records no `limitations`; a row that states nothing a \
                 reader must not infer is not a disposition",
                row.id
            );
        }
        if row.note.trim().is_empty() {
            bail!("{LEDGER_PATH} row `{}` records an empty `note`", row.id);
        }

        // `stable_metadata_candidate` is for server-authored catalogues that
        // have no semantic entity to point at. Letting a workspace, method, or
        // import candidate claim it is how a label hash becomes "identity":
        // those candidates *do* have an entity, and saying otherwise ends the
        // migration by redefining it.
        if row.identity == IdentityDisposition::StableMetadataCandidate
            && !CATALOGUE_CLASSES.contains(&row.candidate_class)
        {
            bail!(
                "{LEDGER_PATH} row `{}` is a `{}` candidate claiming identity `{}`, which is \
                 reserved for server-authored catalogues. A candidate that resolves to a real \
                 entity must carry that entity, not a reviewed label",
                row.id,
                row.candidate_class.as_str(),
                row.identity.as_str()
            );
        }

        // The finalizer is the one row allowed to *be* the finalizer, and no
        // other row may claim it.
        let is_finalizer_class = row.candidate_class == CandidateClass::Finalizer;
        let claims_finalizer = row.finalizer_route == FinalizerRoute::IsSharedFinalizer;
        if is_finalizer_class != claims_finalizer {
            bail!(
                "{LEDGER_PATH} row `{}` disagrees with itself: candidate_class `{}` and \
                 finalizer_route `{}` must both name the shared finalizer or neither",
                row.id,
                row.candidate_class.as_str(),
                row.finalizer_route.as_str()
            );
        }
    }
    Ok(())
}

/// Every row names one implementation owner, and a controller is never it.
fn validate_owners(ledger: &Ledger) -> Result<()> {
    for row in &ledger.producers {
        if !is_issue_ref(&row.migration_owner) {
            bail!(
                "{LEDGER_PATH} row `{}` must name a `migration_owner` issue like `#11021`, got \
                 `{}`",
                row.id,
                row.migration_owner
            );
        }
        if CONTROLLER_ISSUES.contains(&row.migration_owner.as_str()) {
            bail!(
                "{LEDGER_PATH} row `{}` names controller `{}` as its migration owner; a \
                 controller coordinates a programme and cannot implement a row, so this row \
                 would have no owner at all",
                row.id,
                row.migration_owner
            );
        }
    }
    Ok(())
}

/// Reconcile declared reachability against the entry-point call sites, and
/// require an owner for every route divergence.
fn validate_reachability(ledger: &Ledger, discovered: &Discovered) -> Result<()> {
    let entry_set: BTreeSet<String> = ENTRY_POINTS.iter().map(|e| (*e).to_string()).collect();

    // Reach is matched by the trailing function-name segment, which two rows in
    // different modules could share. Rather than let one row's evidence ride on
    // an unrelated same-named call site, refuse the ambiguity outright: the
    // entry-point call sites cannot say which row they meant.
    let mut by_function: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for row in &ledger.producers {
        by_function.entry(row.function_name()).or_default().push(row.id.as_str());
    }
    for (function, ids) in &by_function {
        if ids.len() < 2 {
            continue;
        }
        let called_by_an_entry =
            discovered.entry_direct_calls.values().any(|calls| calls.contains(function));
        if called_by_an_entry {
            bail!(
                "{LEDGER_PATH} has {} rows whose trailing function name is `{function}` ({}), and \
                 a completion entry point calls that name directly. Reach is reconciled by \
                 trailing name, so neither row's `reached_by` can be trusted; give the entry \
                 point an unambiguous target or teach this check to resolve the call.",
                ids.len(),
                ids.join(", ")
            );
        }
    }

    for row in &ledger.producers {
        for entry in &row.reached_by {
            if !entry_set.contains(entry) {
                bail!(
                    "{LEDGER_PATH} row `{}` claims to be reached by `{entry}`, which is not a \
                     completion entry point",
                    row.id
                );
            }
        }
        if row.reached_by.is_empty() && row.reach_evidence != ReachEvidence::Unreachable {
            bail!(
                "{LEDGER_PATH} row `{}` lists no entry point but does not declare \
                 `reach_evidence = \"unreachable\"`; a producer reaches every request, some \
                 request, or none, and which one it is has to be said",
                row.id
            );
        }

        let function = &row.function_name();
        let observed: BTreeSet<String> = discovered
            .entry_direct_calls
            .iter()
            .filter(|(_, calls)| calls.contains(function))
            .map(|(entry, _)| entry.clone())
            .collect();

        match row.reach_evidence {
            ReachEvidence::DirectCall => {
                if observed.is_empty() {
                    bail!(
                        "{LEDGER_PATH} row `{}` claims `reach_evidence = \"direct_call\"` but no \
                         completion entry point calls `{function}` directly; either the call \
                         moved behind the provider seam or the row is stale",
                        row.id
                    );
                }
                let declared: BTreeSet<String> = row.reached_by.iter().cloned().collect();
                if declared != observed {
                    bail!(
                        "{LEDGER_PATH} row `{}` declares `reached_by = [{}]` but the entry points \
                         that call `{function}` directly are [{}].\n\
                         A producer reached by one route and not the other is a live behavior \
                         difference between two shipped completion paths. Update the row and its \
                         route-divergence owner rather than the expectation.",
                        row.id,
                        quoted_list(&row.reached_by),
                        quoted_list(&observed.iter().cloned().collect::<Vec<_>>())
                    );
                }
            }
            ReachEvidence::ProviderSeam => {
                if !observed.is_empty() {
                    bail!(
                        "{LEDGER_PATH} row `{}` claims `reach_evidence = \"provider_seam\"` but \
                         `{function}` is now called directly from [{}]; a row that became \
                         directly reachable must be reconciled against the call site",
                        row.id,
                        quoted_list(&observed.iter().cloned().collect::<Vec<_>>())
                    );
                }
                let declared: BTreeSet<String> = row.reached_by.iter().cloned().collect();
                if declared != entry_set {
                    bail!(
                        "{LEDGER_PATH} row `{}` reaches the client through the provider seam, so \
                         it is reached by every entry point; `reached_by` must list all of [{}]",
                        row.id,
                        quoted_list(&entry_set.iter().cloned().collect::<Vec<_>>())
                    );
                }
            }
            ReachEvidence::Unreachable => {
                if !observed.is_empty() {
                    bail!(
                        "{LEDGER_PATH} row `{}` is recorded unreachable but `{function}` is \
                         called directly from [{}]; a producer that came back into service must \
                         be re-audited, not left recorded as dead",
                        row.id,
                        quoted_list(&observed.iter().cloned().collect::<Vec<_>>())
                    );
                }
                if !row.reached_by.is_empty() {
                    bail!(
                        "{LEDGER_PATH} row `{}` is recorded unreachable but lists `reached_by = \
                         [{}]`",
                        row.id,
                        quoted_list(&row.reached_by)
                    );
                }
            }
        }

        let unreachable = row.reach_evidence == ReachEvidence::Unreachable;
        match (&row.unreachable_reason, unreachable) {
            (None, true) => bail!(
                "{LEDGER_PATH} row `{}` is recorded unreachable but gives no \
                 `unreachable_reason`; a producer no request reaches is either dead code to \
                 delete or a route to restore, and the row has to say which",
                row.id
            ),
            (Some(_), false) => bail!(
                "{LEDGER_PATH} row `{}` records an `unreachable_reason` but is reached by [{}]; \
                 remove the stale field",
                row.id,
                quoted_list(&row.reached_by)
            ),
            (Some(reason), true) if reason.trim().is_empty() => {
                bail!("{LEDGER_PATH} row `{}` has an empty `unreachable_reason`", row.id)
            }
            _ => {}
        }

        // An unreachable producer is not a route divergence: it serves no
        // route at all, and its owner is already named by the reason.
        let diverges = !unreachable && row.reached_by.len() != ENTRY_POINTS.len();
        let owner = row.route_divergence_owner.as_deref().unwrap_or("");
        let note = row.route_divergence_note.as_deref().unwrap_or("");
        if diverges {
            if !is_issue_ref(owner) {
                bail!(
                    "{LEDGER_PATH} row `{}` is reached by [{}] but not every entry point, and \
                     names no `route_divergence_owner`; an unowned difference between two \
                     shipped completion paths is exactly the state this inventory refuses",
                    row.id,
                    quoted_list(&row.reached_by)
                );
            }
            if note.trim().is_empty() {
                bail!(
                    "{LEDGER_PATH} row `{}` has a route divergence owner but no \
                     `route_divergence_note` saying what a user actually sees",
                    row.id
                );
            }
        } else if !owner.is_empty() || !note.trim().is_empty() {
            bail!(
                "{LEDGER_PATH} row `{}` records a route divergence but is reached by every entry \
                 point; remove the stale divergence fields",
                row.id
            );
        }
    }
    Ok(())
}

impl ProducerRow {
    /// Trailing `function` segment of the row id.
    fn function_name(&self) -> String {
        self.id.rsplit("::").next().unwrap_or(&self.id).to_string()
    }

    /// Leading module segments of the row id.
    fn module_path(&self) -> String {
        match self.id.rfind("::") {
            Some(index) => self.id[..index].to_string(),
            None => String::new(),
        }
    }
}

/// Nothing may append a candidate after merge, rank, and cap have run.
fn validate_no_post_finalizer_append(discovered: &Discovered) -> Result<()> {
    if discovered.post_finalizer_appends.is_empty() {
        return Ok(());
    }
    bail!(
        "candidate append after `{FINALIZER_CALL}` in {}.\n\
         A candidate added after merge, rank, and cap has not been ranked, can duplicate an \
         existing identity, and can push a ranked candidate off the capped page. Contribute it \
         before finalization instead.",
        discovered.post_finalizer_appends.join(", ")
    );
}

fn is_issue_ref(value: &str) -> bool {
    let Some(rest) = value.strip_prefix('#') else {
        return false;
    };
    !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
}

fn quoted_list(values: &[String]) -> String {
    values.iter().map(|value| format!("\"{value}\"")).collect::<Vec<_>>().join(", ")
}

// ---------------------------------------------------------------------------
// Projections
// ---------------------------------------------------------------------------

fn class_counts(ledger: &Ledger) -> BTreeMap<&'static str, usize> {
    let mut counts = BTreeMap::new();
    for row in &ledger.producers {
        *counts.entry(row.candidate_class.as_str()).or_insert(0) += 1;
    }
    counts
}

fn sorted_rows(ledger: &Ledger) -> Vec<&ProducerRow> {
    let mut rows: Vec<&ProducerRow> = ledger.producers.iter().collect();
    rows.sort_by(|left, right| {
        left.candidate_class.cmp(&right.candidate_class).then_with(|| left.id.cmp(&right.id))
    });
    rows
}

/// `list` output: producers grouped by candidate class.
pub fn render_list(ledger: &Ledger) -> String {
    let mut out = String::new();
    let mut current: Option<CandidateClass> = None;
    for row in sorted_rows(ledger) {
        if current != Some(row.candidate_class) {
            let _ = writeln!(out, "\n{}", row.candidate_class.as_str());
            current = Some(row.candidate_class);
        }
        let _ = writeln!(
            out,
            "  {}\n    identity={} rank={} completeness={} route={} owner={}",
            row.id,
            row.identity.as_str(),
            row.rank.as_str(),
            row.source_completeness.as_str(),
            row.finalizer_route.as_str(),
            row.migration_owner
        );
    }
    let _ = writeln!(
        out,
        "\n{} producers, {} classes",
        ledger.producers.len(),
        class_counts(ledger).len()
    );
    out
}

/// `explain` output: the packet a cheap agent needs before touching one row.
pub fn render_explain(ledger: &Ledger, discovered: &Discovered, row: &ProducerRow) -> String {
    let file = discovered
        .producers
        .iter()
        .find(|p| p.id == row.id)
        .map(|p| p.file.as_str())
        .unwrap_or("<unknown>");

    let mut out = String::new();
    let _ = writeln!(out, "producer      {}", row.id);
    let _ = writeln!(out, "source        {file}");
    let _ = writeln!(out, "package       {}", row.package);
    let _ = writeln!(out, "module        {}", row.module_path());
    let _ = writeln!(out, "class         {}", row.candidate_class.as_str());
    let _ = writeln!(out, "tier          {}", row.tier.as_str());
    let _ = writeln!(out);
    let _ = writeln!(out, "identity      {}", row.identity.as_str());
    let _ = writeln!(out, "insertion     {}", row.insertion_plan.as_str());
    let _ = writeln!(out, "evidence      {}", row.evidence.as_str());
    let _ = writeln!(out, "rank          {}", row.rank.as_str());
    let _ = writeln!(out, "completeness  {}", row.source_completeness.as_str());
    let _ = writeln!(out, "finalizer     {}", row.finalizer_route.as_str());
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "reached by    {} ({})",
        row.reached_by.join(", "),
        row.reach_evidence.as_str()
    );
    if let Some(owner) = &row.route_divergence_owner {
        let _ = writeln!(out, "divergence    {owner}");
        if let Some(note) = &row.route_divergence_note {
            let _ = writeln!(out, "              {note}");
        }
    }
    let _ = writeln!(out, "owner         {}", row.migration_owner);
    if let Some(reason) = &row.reviewed_reason {
        let _ = writeln!(out, "reviewed      {reason}");
    }
    let _ = writeln!(out);
    let _ = writeln!(out, "do not infer  {}", row.limitations);
    let _ = writeln!(out, "note          {}", row.note);
    let _ = writeln!(out);
    let _ = writeln!(out, "route owner   {}", ledger.route_owner);
    let _ = writeln!(out, "finalizer own {}", ledger.finalizer_owner);
    let _ = writeln!(out, "outcome owner {}", ledger.outcome_owner);
    out
}

fn cell(value: &str) -> String {
    value.replace('|', "\\|")
}

/// The checked-in projection: a reader-facing view of the reconciled ledger.
pub fn render_markdown(ledger: &Ledger, discovered: &Discovered) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "<!-- auto-generated by `cargo xtask completion-candidates graph`; do not edit -->"
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "# Completion candidate producer inventory");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "Generated from `{LEDGER_PATH}` reconciled against the completion surface's real source. \
         That file is the authority; this page is a projection of it. Edit the ledger and run \
         `cargo xtask completion-candidates graph`."
    );
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "Controlling issue {}. The one live application route is {}, pure merge/rank/cap is {}, \
         and source completeness is {}. This inventory changes no completion behavior.",
        ledger.controlling_issue, ledger.route_owner, ledger.finalizer_owner, ledger.outcome_owner
    );
    let _ = writeln!(out);

    let _ = writeln!(out, "## Source binding");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "The dispositions below were audited against this exact source. A change to any file \
         listed here invalidates the audit and `check` fails until the rows are re-checked."
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "- Digest: `{}`", ledger.source_digest);
    let _ = writeln!(out, "- Files ({}):", discovered.source_files.len());
    for file in &discovered.source_files {
        let _ = writeln!(out, "  - `{file}`");
    }
    let _ = writeln!(out);

    let _ = writeln!(out, "## Denominator");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "A producer is a function taking the shared `&mut Vec<CompletionItem>` append channel. \
         A file that constructs candidates without exposing one must carry a `construction_only` \
         row saying which producer owns its output."
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "| Population | Count |");
    let _ = writeln!(out, "| --- | --- |");
    let _ = writeln!(out, "| producers | {} |", ledger.producers.len());
    let _ = writeln!(out, "| candidate classes | {} |", class_counts(ledger).len());
    let _ = writeln!(out, "| construction-only files | {} |", ledger.construction_only.len());
    let _ = writeln!(out, "| delegated modules | {} |", ledger.delegations.len());
    let _ = writeln!(out, "| entry points | {} |", ENTRY_POINTS.len());
    let _ =
        writeln!(out, "| post-finalizer appends | {} |", discovered.post_finalizer_appends.len());
    let _ = writeln!(out);

    let _ = writeln!(out, "## Producers");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "| Producer | Class | Seam | Tier | Identity | Insertion | Evidence | Rank | \
         Completeness | Finalizer route | Reached by | Owner |"
    );
    let _ =
        writeln!(out, "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |");
    let seams: BTreeMap<&str, &DiscoveredProducer> =
        discovered.producers.iter().map(|producer| (producer.id.as_str(), producer)).collect();
    for row in sorted_rows(ledger) {
        // `×2` marks one logical producer with two mutually exclusive `cfg`
        // bodies, so a reader does not read it as a duplicated row.
        let seam = match seams.get(row.id.as_str()) {
            Some(producer) if producer.declarations > 1 => {
                format!("{} ×{}", producer.channel.as_str(), producer.declarations)
            }
            Some(producer) => producer.channel.as_str().to_string(),
            None => "unknown".to_string(),
        };
        let _ = writeln!(
            out,
            "| `{}` | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            cell(&row.id),
            row.candidate_class.as_str(),
            seam,
            row.tier.as_str(),
            row.identity.as_str(),
            row.insertion_plan.as_str(),
            row.evidence.as_str(),
            row.rank.as_str(),
            row.source_completeness.as_str(),
            row.finalizer_route.as_str(),
            cell(&row.reached_by.join(", ")),
            cell(&row.migration_owner)
        );
    }
    let _ = writeln!(out);

    let divergences: Vec<&ProducerRow> = sorted_rows(ledger)
        .into_iter()
        .filter(|row| row.route_divergence_owner.is_some())
        .collect();
    let _ = writeln!(out, "## Route divergences");
    let _ = writeln!(out);
    if divergences.is_empty() {
        let _ = writeln!(
            out,
            "None. Every producer is reached by every shipped completion entry point."
        );
    } else {
        let _ = writeln!(
            out,
            "A producer reached by one shipped entry point and not another is a live difference \
             in what a user is offered, decided by which request path the editor took. Each row \
             below names the issue that converges the routes."
        );
        let _ = writeln!(out);
        let _ = writeln!(out, "| Producer | Reached by | Owner | What a user sees |");
        let _ = writeln!(out, "| --- | --- | --- | --- |");
        for row in divergences {
            let _ = writeln!(
                out,
                "| `{}` | {} | {} | {} |",
                cell(&row.id),
                cell(&row.reached_by.join(", ")),
                cell(row.route_divergence_owner.as_deref().unwrap_or("")),
                cell(row.route_divergence_note.as_deref().unwrap_or(""))
            );
        }
    }
    let _ = writeln!(out);

    let _ = writeln!(out, "## Construction-only files");
    let _ = writeln!(out);
    if ledger.construction_only.is_empty() {
        let _ = writeln!(out, "None.");
    } else {
        let _ = writeln!(out, "| File | Consumed by | Reason |");
        let _ = writeln!(out, "| --- | --- | --- |");
        let mut rows: Vec<&ConstructionOnlyRow> = ledger.construction_only.iter().collect();
        rows.sort_by(|left, right| left.path.cmp(&right.path));
        for row in rows {
            let _ = writeln!(
                out,
                "| `{}` | `{}` | {} |",
                cell(&row.path),
                cell(&row.consumed_by),
                cell(&row.reason)
            );
        }
    }
    let _ = writeln!(out);

    let _ = writeln!(out, "## Delegated modules");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "`providers::` modules the scanned surface reaches into but does not scan in full. A \
         facade that returned candidates built in an unscanned module would carry a disposition \
         its producer did not, so each one is dispositioned here."
    );
    let _ = writeln!(out);
    if ledger.delegations.is_empty() {
        let _ = writeln!(out, "None. Every referenced provider module is scanned in full.");
    } else {
        let _ = writeln!(out, "| Module | Why it carries no unscanned producer |");
        let _ = writeln!(out, "| --- | --- |");
        let mut rows: Vec<&DelegationRow> = ledger.delegations.iter().collect();
        rows.sort_by(|left, right| left.module.cmp(&right.module));
        for row in rows {
            let _ = writeln!(out, "| `providers::{}` | {} |", cell(&row.module), cell(&row.reason));
        }
    }
    let _ = writeln!(out);

    let _ = writeln!(out, "## Route");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "Producers are grouped by candidate class and by which entry points reach them, so a \
         class that one shipped path offers and the other does not is visible as a missing edge \
         rather than buried in the table above. Counts are producer rows in that group."
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "```mermaid");
    let _ = writeln!(out, "flowchart LR");
    for entry in ENTRY_POINTS {
        let _ = writeln!(out, "  {}[\"{entry}\"]", node_id(entry));
    }
    let _ = writeln!(out, "  pool([\"shared candidate pool\"])");
    let _ = writeln!(out, "  final[\"{FINALIZER_CALL}\"]");
    let _ = writeln!(out, "  wire([\"LSP CompletionList\"])");

    // One node per (class, reach pattern). Aggregating by class alone would
    // merge a class some entry points reach with one they all reach, which is
    // the single distinction this graph exists to show.
    let mut groups: BTreeMap<(CandidateClass, Vec<String>), usize> = BTreeMap::new();
    let mut unreached: BTreeMap<CandidateClass, usize> = BTreeMap::new();
    for row in sorted_rows(ledger) {
        if row.candidate_class == CandidateClass::Finalizer {
            continue;
        }
        if row.reach_evidence == ReachEvidence::Unreachable {
            *unreached.entry(row.candidate_class).or_insert(0) += 1;
            continue;
        }
        let mut reached = row.reached_by.clone();
        reached.sort();
        *groups.entry((row.candidate_class, reached)).or_insert(0) += 1;
    }

    for ((class, reached), count) in &groups {
        let node = node_id(&format!("{}_{}", class.as_str(), reached.join("_")));
        let _ = writeln!(out, "  {node}[\"{} ×{count}\"]", class.as_str());
        for entry in reached {
            let _ = writeln!(out, "  {} --> {node}", node_id(entry));
        }
        let _ = writeln!(out, "  {node} --> pool");
    }
    let _ = writeln!(out, "  pool --> final");
    let _ = writeln!(out, "  final --> wire");

    if !unreached.is_empty() {
        let _ = writeln!(out, "  subgraph unreached [\"reached by no entry point\"]");
        for (class, count) in &unreached {
            let node = node_id(&format!("unreached_{}", class.as_str()));
            let _ = writeln!(out, "    {node}[\"{} ×{count}\"]", class.as_str());
        }
        let _ = writeln!(out, "  end");
    }
    let _ = writeln!(out, "```");
    let _ = writeln!(out);

    let _ = writeln!(out, "## Reading this inventory");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "- `reached_by` with `direct_call` evidence is reconciled against the entry-point call \
         site. `provider_seam` evidence is declared: the producer is reached through the \
         provider call, and this task does not prove that edge."
    );
    let _ = writeln!(
        out,
        "- `legacy_unreported` completeness means the producer says nothing about whether it \
         finished. While any reached row is `legacy_unreported`, a `Complete` completion outcome \
         is not earned."
    );
    let _ = writeln!(
        out,
        "- Discovery is syntactic. A producer that returned candidates by value rather than \
         taking the append channel would not appear as a producer row; the construction-only \
         plane bounds that gap at file granularity."
    );
    let _ = writeln!(
        out,
        "- Inline completion and `completionItem/resolve` are outside this denominator. Neither \
         contributes to a `textDocument/completion` candidate pool."
    );

    out
}

/// Mermaid-safe node id.
fn node_id(value: &str) -> String {
    let mut out = String::from("n");
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Proof
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// The checked-in ledger reconciled against the real tree is the valid
    /// fixture. Every negative test below corrupts that fixture in memory along
    /// one intended falsifier axis and proves the validator refuses it for that
    /// reason — so a passing `check` means the axis was actually exercised, not
    /// that a synthetic fixture happened to be well formed.
    fn fixture() -> (Ledger, Discovered) {
        let root = project_root().expect("project root");
        let ledger = load(&root).expect("ledger parses");
        let discovered = discover(&root).expect("discovery runs");
        (ledger, discovered)
    }

    fn refuses(ledger: &Ledger, discovered: &Discovered, expected: &str) {
        match validate(ledger, discovered) {
            Ok(()) => panic!("validator accepted a ledger it must refuse: {expected}"),
            Err(error) => {
                let rendered = format!("{error:?}");
                assert!(
                    rendered.contains(expected),
                    "validator refused for the wrong reason.\n  wanted substring: \
                     {expected}\n  got: {rendered}"
                );
            }
        }
    }

    #[test]
    fn checked_in_ledger_is_current() {
        let (ledger, discovered) = fixture();
        validate(&ledger, &discovered)
            .expect("checked-in ledger reconciles against current source");
    }

    #[test]
    fn checked_in_projection_is_current() {
        let root = project_root().expect("project root");
        let (ledger, discovered) = fixture();
        let generated = render_markdown(&ledger, &discovered);
        let existing = fs::read_to_string(root.join(PROJECTION_PATH)).expect("projection exists");
        assert_eq!(
            normalize_newlines(&existing),
            generated,
            "{PROJECTION_PATH} is stale; run `cargo xtask completion-candidates graph`"
        );
    }

    /// Second generation produces identical bytes. Without this a reviewer
    /// cannot tell a real disposition change from map iteration order.
    #[test]
    fn generation_is_deterministic() {
        let (ledger, discovered) = fixture();
        assert_eq!(render_markdown(&ledger, &discovered), render_markdown(&ledger, &discovered));
        assert_eq!(render_list(&ledger), render_list(&ledger));
    }

    /// Discovery is not vacuous. A denominator that quietly became empty would
    /// make every other assertion here pass while proving nothing.
    #[test]
    fn discovery_finds_the_live_surface() {
        let (_, discovered) = fixture();
        assert!(
            discovered.producers.len() >= 40,
            "discovery found only {} producers; the scan roots or the append-channel \
             predicate have stopped matching the completion surface",
            discovered.producers.len()
        );
        for entry in ENTRY_POINTS {
            let calls = discovered
                .entry_direct_calls
                .get(*entry)
                .unwrap_or_else(|| panic!("no direct calls collected for {entry}"));
            assert!(
                calls.contains(FINALIZER_CALL),
                "{entry} no longer calls {FINALIZER_CALL}; the post-finalizer control has \
                 nothing to anchor to"
            );
        }
    }

    /// The finding this inventory exists to make legible: one shipped entry
    /// point offers Dancer2 keywords and the other does not.
    #[test]
    fn dancer2_route_divergence_is_observed_in_source() {
        let (_, discovered) = fixture();
        let ordinary = &discovered.entry_direct_calls["handle_completion"];
        let cancellable = &discovered.entry_direct_calls["handle_completion_cancellable"];
        assert!(
            !ordinary.contains("add_dancer2_keyword_completions"),
            "handle_completion now calls add_dancer2_keyword_completions; the routes may have \
             converged, which is #10229's outcome — update the ledger row rather than this test"
        );
        assert!(
            cancellable.contains("add_dancer2_keyword_completions"),
            "handle_completion_cancellable no longer calls add_dancer2_keyword_completions"
        );
    }

    #[test]
    fn refuses_a_producer_with_no_row() {
        let (mut ledger, discovered) = fixture();
        let removed = ledger.producers.remove(0);
        refuses(&ledger, &discovered, &format!("`{}`", removed.id));
        refuses(&ledger, &discovered, "has no row for it");
    }

    #[test]
    fn refuses_a_row_naming_a_producer_the_source_dropped() {
        let (mut ledger, discovered) = fixture();
        let mut ghost = ledger.producers[0].clone();
        ghost.id = format!("{}::ghost_producer", CORE_TEST_MODULE);
        ledger.producers.push(ghost);
        refuses(&ledger, &discovered, "no scanned source declares it");
    }

    #[test]
    fn refuses_a_duplicate_producer_row() {
        let (mut ledger, discovered) = fixture();
        let duplicate = ledger.producers[0].clone();
        ledger.producers.push(duplicate);
        refuses(&ledger, &discovered, "duplicate producer id");
    }

    #[test]
    fn refuses_a_source_change_without_a_re_audit() {
        let (mut ledger, discovered) = fixture();
        ledger.source_digest =
            "sha256:0000000000000000000000000000000000000000000000000000000000000000".to_string();
        refuses(&ledger, &discovered, "changed since the inventory was audited");
    }

    #[test]
    fn refuses_a_controller_as_implementation_owner() {
        let (mut ledger, discovered) = fixture();
        ledger.producers[0].migration_owner = "#8969".to_string();
        refuses(&ledger, &discovered, "as its migration owner");
    }

    #[test]
    fn refuses_a_row_with_no_owner() {
        let (mut ledger, discovered) = fixture();
        ledger.producers[0].migration_owner = "the completion team".to_string();
        refuses(&ledger, &discovered, "must name a `migration_owner` issue");
    }

    #[test]
    fn refuses_permanent_compatibility_without_a_reviewed_reason() {
        let (mut ledger, discovered) = fixture();
        let row = row_mut(&mut ledger, |row| row.rank == RankDisposition::CompatibilityWithExit);
        row.rank = RankDisposition::CompatibilityPermanentReviewed;
        refuses(&ledger, &discovered, "records no `reviewed_reason`");
    }

    #[test]
    fn refuses_a_producer_outside_the_shared_finalizer() {
        let (mut ledger, discovered) = fixture();
        let row = row_mut(&mut ledger, |row| {
            row.finalizer_route == FinalizerRoute::CompatibilityAdapterBeforeSharedFinalizer
        });
        row.finalizer_route = FinalizerRoute::OutsideFinalizerForbidden;
        refuses(&ledger, &discovered, "outside the shared finalizer");
    }

    #[test]
    fn refuses_an_unidentified_candidate() {
        let (mut ledger, discovered) = fixture();
        ledger.producers[0].identity = IdentityDisposition::UnidentifiedForbidden;
        refuses(&ledger, &discovered, "cannot reach merge");
    }

    #[test]
    fn refuses_an_edit_with_no_owner() {
        let (mut ledger, discovered) = fixture();
        ledger.producers[0].insertion_plan = InsertionDisposition::ConflictingOrUnowned;
        refuses(&ledger, &discovered, "an edit with no owner");
    }

    #[test]
    fn refuses_unestablished_evidence() {
        let (mut ledger, discovered) = fixture();
        ledger.producers[0].evidence = EvidenceDisposition::UnknownForbidden;
        refuses(&ledger, &discovered, "unestablished evidence");
    }

    #[test]
    fn refuses_unclassified_rank() {
        let (mut ledger, discovered) = fixture();
        ledger.producers[0].rank = RankDisposition::UnclassifiedForbidden;
        refuses(&ledger, &discovered, "rank behavior must be classified");
    }

    /// A workspace or method candidate has a real entity behind it, so calling
    /// it server-authored metadata is how a label becomes "identity".
    #[test]
    fn refuses_stable_metadata_identity_for_a_resolved_candidate() {
        let (mut ledger, discovered) = fixture();
        let row =
            row_mut(&mut ledger, |row| row.candidate_class == CandidateClass::WorkspaceSymbol);
        row.identity = IdentityDisposition::StableMetadataCandidate;
        refuses(&ledger, &discovered, "reserved for server-authored catalogues");
    }

    /// The route control. Claiming both paths reach a producer that only one
    /// path calls is exactly the drift this inventory refuses to normalize.
    #[test]
    fn refuses_a_declared_reach_the_call_site_contradicts() {
        let (mut ledger, discovered) = fixture();
        let row = row_mut(&mut ledger, |row| row.id.ends_with("add_dancer2_keyword_completions"));
        row.reached_by = ENTRY_POINTS.iter().map(|e| (*e).to_string()).collect();
        row.route_divergence_owner = None;
        row.route_divergence_note = None;
        refuses(&ledger, &discovered, "but the entry points that call");
    }

    #[test]
    fn refuses_an_unowned_route_divergence() {
        let (mut ledger, discovered) = fixture();
        let row = row_mut(&mut ledger, |row| row.id.ends_with("add_dancer2_keyword_completions"));
        row.route_divergence_owner = None;
        row.route_divergence_note = None;
        refuses(&ledger, &discovered, "names no `route_divergence_owner`");
    }

    #[test]
    fn refuses_a_provider_seam_row_that_drops_an_entry_point() {
        let (mut ledger, discovered) = fixture();
        let row = row_mut(&mut ledger, |row| row.reach_evidence == ReachEvidence::ProviderSeam);
        row.reached_by = vec!["handle_completion".to_string()];
        refuses(&ledger, &discovered, "must list all of");
    }

    #[test]
    fn refuses_an_unreachable_row_with_no_reason() {
        let (mut ledger, discovered) = fixture();
        let row = row_mut(&mut ledger, |row| row.reach_evidence == ReachEvidence::Unreachable);
        row.unreachable_reason = None;
        refuses(&ledger, &discovered, "gives no `unreachable_reason`");
    }

    #[test]
    fn refuses_a_dead_row_that_came_back_into_service() {
        let (mut ledger, discovered) = fixture();
        let row = row_mut(&mut ledger, |row| {
            row.id.ends_with("CompletionProvider::get_completions_with_path")
                && !row.id.ends_with("cancellable")
        });
        row.reach_evidence = ReachEvidence::Unreachable;
        row.reached_by.clear();
        row.route_divergence_owner = None;
        row.route_divergence_note = None;
        row.unreachable_reason = Some("stale".to_string());
        refuses(&ledger, &discovered, "recorded unreachable but");
    }

    #[test]
    fn refuses_a_construction_only_row_for_a_file_that_produces() {
        let (mut ledger, discovered) = fixture();
        let producing = discovered
            .producer_files
            .iter()
            .find(|file| discovered.construction_files.contains(*file))
            .expect("a producer file that also constructs candidates")
            .clone();
        ledger.construction_only.push(ConstructionOnlyRow {
            path: producing,
            reason: "spurious".to_string(),
            consumed_by: ledger.producers[0].id.clone(),
        });
        refuses(&ledger, &discovered, "needs a producer row instead");
    }

    /// The delegation plane. A producer the scanned tree hands off to is
    /// invisible to the other two planes, which is how the htmx and
    /// file-completion producers stayed out of the first ledger.
    #[test]
    fn refuses_an_undeclared_delegated_module() {
        let (mut ledger, discovered) = fixture();
        ledger.delegations.retain(|row| row.module != "dancer2");
        refuses(&ledger, &discovered, "has no `[[delegations]]` row");
    }

    #[test]
    fn refuses_a_delegation_row_for_a_scanned_module() {
        let (mut ledger, discovered) = fixture();
        ledger
            .delegations
            .push(DelegationRow { module: "htmx".to_string(), reason: "stale".to_string() });
        refuses(&ledger, &discovered, "now fully scanned");
    }

    #[test]
    fn refuses_a_delegation_row_nothing_reaches() {
        let (mut ledger, discovered) = fixture();
        ledger.delegations.push(DelegationRow {
            module: "nonexistent_module".to_string(),
            reason: "stale".to_string(),
        });
        refuses(&ledger, &discovered, "reaches into it any more");
    }

    /// A scan root naming one file does not cover the module around it.
    #[test]
    fn a_single_file_scan_root_does_not_cover_its_module() {
        assert!(module_is_fully_scanned("htmx"), "a directory root covers its module");
        assert!(module_is_fully_scanned("file_completion"));
        assert!(
            !module_is_fully_scanned("dancer2"),
            "only dancer2/completion.rs is scanned, so the module is not covered"
        );
        assert!(!module_is_fully_scanned("testing"));
    }

    /// The producers Devin's review found outside the original scan roots.
    /// They reach the client through an inventoried facade, so a regression
    /// here would restore exactly the hole that review closed.
    #[test]
    fn delegated_producers_are_in_the_denominator() {
        let (ledger, discovered) = fixture();
        for id in [
            "perl_lsp_rs_core::providers::file_completion::complete_file_paths",
            "perl_lsp_rs_core::providers::htmx::complete_header_names",
        ] {
            assert!(
                discovered.producers.iter().any(|producer| producer.id == id),
                "discovery lost the delegated producer `{id}`"
            );
            assert!(
                ledger.producers.iter().any(|row| row.id == id),
                "the ledger lost the delegated producer `{id}`"
            );
        }
    }

    /// Growing the finalized page is an append however it is spelled.
    #[test]
    fn resize_after_finalization_is_an_append() {
        for method in ["resize", "resize_with", "append", "insert", "splice"] {
            let source = format!(
                "fn entry() {{
                    let (completions, is_incomplete) = sort_and_cap_completions(completions, cap);
                    completions.{method}(extra);
                }}"
            );
            let parsed: syn::ItemFn = syn::parse_str(&source).expect("fixture parses");
            let mut visitor = EntryBodyVisitor::default();
            visitor.visit_block(&parsed.block);
            assert!(
                visitor.appended_after_finalizer,
                "`{method}` after the finalizer went undetected"
            );
        }
    }

    #[test]
    fn refuses_an_empty_limitations_field() {
        let (mut ledger, discovered) = fixture();
        ledger.producers[0].limitations = "   ".to_string();
        refuses(&ledger, &discovered, "records no `limitations`");
    }

    /// The post-finalizer control fires on a mutated discovery rather than a
    /// mutated ledger: the hazard is in source, not in the authored rows.
    #[test]
    fn refuses_an_append_after_finalization() {
        let (_, mut discovered) = fixture();
        discovered.post_finalizer_appends.push("handle_completion".to_string());
        assert!(validate_no_post_finalizer_append(&discovered).is_err());
        let error = format!("{:?}", validate_no_post_finalizer_append(&discovered).unwrap_err());
        assert!(error.contains("has not been ranked"), "unexpected message: {error}");
    }

    /// The append-channel predicate is what makes the denominator mechanical,
    /// so it is proven directly rather than only through the ledger.
    #[test]
    fn stderr_diagnostic_is_bounded_and_meaningful() {
        assert_eq!(first_stderr_line(b""), "");
        assert_eq!(first_stderr_line(b"\n\n   \n"), "");
        assert_eq!(
            first_stderr_line(b"\nfatal: not a git repository\nsecond line\n"),
            ": fatal: not a git repository",
            "the first meaningful line is the cause; later lines are noise"
        );
        let long = vec![b'x'; MAX_STDERR_DIAGNOSTIC * 4];
        let bounded = first_stderr_line(&long);
        assert!(
            bounded.chars().count() <= MAX_STDERR_DIAGNOSTIC + 3,
            "a child that floods stderr must not flood the error message: {} chars",
            bounded.chars().count()
        );
        assert!(bounded.ends_with('\u{2026}'), "truncation must be visible: {bounded}");
    }

    /// Two trait impls can share a method name on one type, and an
    /// unqualified id would let the second silently join the first row's
    /// declaration count instead of needing a disposition of its own.
    #[test]
    fn trait_impl_producers_do_not_share_one_id() {
        let file: syn::File = syn::parse_quote! {
            impl ProducerA for Shared {
                fn add(&self, completions: &mut Vec<CompletionItem>) {}
            }
            impl ProducerB for Shared {
                fn add(&self, completions: &mut Vec<CompletionItem>) {}
            }
        };
        let mut visitor = SeamVisitor::new(PROBE_FILE);
        visitor.visit_file(&file);
        assert_eq!(visitor.producers.len(), 2, "both trait methods are producers");

        let merged = merge_declarations(visitor.producers).expect("distinct ids do not collide");
        assert_eq!(
            merged.len(),
            2,
            "two trait impls collapsed into one row: {:?}",
            merged.iter().map(|p| &p.id).collect::<Vec<_>>()
        );
        for producer in &merged {
            assert_eq!(producer.declarations, 1, "neither row is a cfg-arm pair");
            assert!(
                producer.id.contains(" as "),
                "trait impl id must name the trait: {}",
                producer.id
            );
        }
    }

    /// The genuine `cfg` arm pair must still collapse: it is one producer with
    /// one disposition, and splitting it would demand two rows for one seam.
    #[test]
    fn cfg_arms_of_one_producer_stay_one_row() {
        let file: syn::File = syn::parse_quote! {
            impl Provider {
                #[cfg(not(target_arch = "wasm32"))]
                fn add(&self, completions: &mut Vec<CompletionItem>) {}
                #[cfg(target_arch = "wasm32")]
                fn add(&self, completions: &mut Vec<CompletionItem>) {}
            }
        };
        let mut visitor = SeamVisitor::new(PROBE_FILE);
        visitor.visit_file(&file);
        let merged = merge_declarations(visitor.producers).expect("same-file cfg arms merge");
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].declarations, 2);
    }

    /// `all(test, ..)` compiles only under test; `any(test, ..)` and
    /// `not(test)` both ship, so only the first is proof rather than product.
    #[test]
    fn cfg_test_detection_follows_predicate_meaning() {
        let only_under_test: syn::ItemFn = syn::parse_quote! {
            #[cfg(all(test, feature = "wasm"))]
            fn f(completions: &mut Vec<CompletionItem>) {}
        };
        assert!(has_cfg_test(&only_under_test.attrs), "all(test, ..) is test-only");

        let nested: syn::ItemFn = syn::parse_quote! {
            #[cfg(all(feature = "wasm", all(test)))]
            fn f(completions: &mut Vec<CompletionItem>) {}
        };
        assert!(has_cfg_test(&nested.attrs), "nested all(..) is still test-only");

        let also_ships: syn::ItemFn = syn::parse_quote! {
            #[cfg(any(test, feature = "x"))]
            fn f(completions: &mut Vec<CompletionItem>) {}
        };
        assert!(
            !has_cfg_test(&also_ships.attrs),
            "any(test, ..) can ship, so it is product source"
        );

        let never_test: syn::ItemFn = syn::parse_quote! {
            #[cfg(not(test))]
            fn f(completions: &mut Vec<CompletionItem>) {}
        };
        assert!(!has_cfg_test(&never_test.attrs), "not(test) ships");

        let plain: syn::ItemFn = syn::parse_quote! {
            #[cfg(test)]
            fn f(completions: &mut Vec<CompletionItem>) {}
        };
        assert!(has_cfg_test(&plain.attrs));
    }

    /// The post-finalizer control must follow the page, not the spelling: a
    /// candidate pushed through a renamed binding reaches the client exactly
    /// as one pushed through the original would.
    #[test]
    fn post_finalizer_append_survives_a_renamed_binding() {
        let renamed: syn::ItemFn = syn::parse_quote! {
            fn entry() {
                let (completions, is_incomplete) = sort_and_cap_completions(completions, cap);
                let mut smuggled = completions;
                smuggled.push(sneaky());
                let completions = smuggled;
            }
        };
        let mut visitor = EntryBodyVisitor::default();
        visitor.visit_block(&renamed.block);
        assert!(
            visitor.appended_after_finalizer,
            "an append through a renamed binding after the finalizer went undetected"
        );

        let taken: syn::ItemFn = syn::parse_quote! {
            fn entry() {
                let (completions, is_incomplete) = sort_and_cap_completions(completions, cap);
                let mut page = std::mem::take(&mut completions);
                page.append(&mut extra);
            }
        };
        let mut visitor = EntryBodyVisitor::default();
        visitor.visit_block(&taken.block);
        assert!(visitor.appended_after_finalizer, "`append` through a moved page went undetected");
    }

    /// The same control must not fire on the ordinary shape, or every entry
    /// point would be permanently red and the signal would be worthless.
    #[test]
    fn ordinary_finalization_is_not_a_post_finalizer_append() {
        let ordinary: syn::ItemFn = syn::parse_quote! {
            fn entry() {
                let mut completions = provider.get_completions_with_path();
                self.add_runtime_workspace_completions(&mut completions);
                let (completions, is_incomplete) = sort_and_cap_completions(completions, cap);
                let items: Vec<Value> = completions.into_iter().map(|c| render(c)).collect();
                let mut payload = Vec::new();
                payload.push(items);
            }
        };
        let mut visitor = EntryBodyVisitor::default();
        visitor.visit_block(&ordinary.block);
        assert!(
            !visitor.appended_after_finalizer,
            "the serialization tail was mistaken for a candidate append"
        );
    }

    const PROBE_FILE: &str = "crates/perl-lsp-rs-core/src/providers/completion/completion/probe.rs";

    #[test]
    fn append_channel_predicate_matches_the_real_shapes() {
        let accepted: syn::ItemFn = syn::parse_quote! {
            fn producer(completions: &mut Vec<CompletionItem>) {}
        };
        assert!(accepted.sig.inputs.iter().any(takes_append_channel));

        let qualified: syn::ItemFn = syn::parse_quote! {
            fn producer(completions: &mut Vec<crate::completion::CompletionItem>) {}
        };
        assert!(qualified.sig.inputs.iter().any(takes_append_channel));

        // A shared slice reorders in place; it cannot add a candidate.
        let slice: syn::ItemFn = syn::parse_quote! {
            fn reorder(completions: &mut [CompletionItem]) {}
        };
        assert!(!slice.sig.inputs.iter().any(takes_append_channel));

        // Read-only access is not an append channel.
        let shared: syn::ItemFn = syn::parse_quote! {
            fn inspect(completions: &Vec<CompletionItem>) {}
        };
        assert!(!shared.sig.inputs.iter().any(takes_append_channel));

        // An unrelated vector must not widen the denominator.
        let unrelated: syn::ItemFn = syn::parse_quote! {
            fn unrelated(values: &mut Vec<String>) {}
        };
        assert!(!unrelated.sig.inputs.iter().any(takes_append_channel));
    }

    #[test]
    fn return_channel_predicate_matches_the_real_shapes() {
        let returned: syn::ItemFn = syn::parse_quote! {
            fn producer() -> Vec<CompletionItem> { Vec::new() }
        };
        assert!(
            matches!(&returned.sig.output, syn::ReturnType::Type(_, ty) if mentions_candidate_vec(ty))
        );

        // The runtime finalizer returns its page beside an incompleteness flag.
        let paired: syn::ItemFn = syn::parse_quote! {
            fn finalize() -> (Vec<CompletionItem>, bool) { unimplemented!() }
        };
        assert!(
            matches!(&paired.sig.output, syn::ReturnType::Type(_, ty) if mentions_candidate_vec(ty))
        );

        // The migration target shape counts too.
        let typed: syn::ItemFn = syn::parse_quote! {
            fn merge() -> Vec<CompletionCandidate> { Vec::new() }
        };
        assert!(
            matches!(&typed.sig.output, syn::ReturnType::Type(_, ty) if mentions_candidate_vec(ty))
        );

        let unrelated: syn::ItemFn = syn::parse_quote! {
            fn unrelated() -> Vec<String> { Vec::new() }
        };
        assert!(
            matches!(&unrelated.sig.output, syn::ReturnType::Type(_, ty) if !mentions_candidate_vec(ty))
        );
    }

    const CORE_TEST_MODULE: &str = "perl_lsp_rs_core::providers::completion::completion";

    fn row_mut(ledger: &mut Ledger, predicate: impl Fn(&ProducerRow) -> bool) -> &mut ProducerRow {
        ledger
            .producers
            .iter_mut()
            .find(|row| predicate(row))
            .expect("the ledger no longer contains a row matching this falsifier's precondition")
    }
}
