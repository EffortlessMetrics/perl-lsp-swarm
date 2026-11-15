//! Shared dancer2 routes contracts and reference data.
//!
//! Concrete interpretation is owned by `perl_semantic_analyzer::framework_adapters::dancer2_routes`.
//! This module preserves the fact vocabulary, stable identities, and reviewed tables.

use crate::framework_adapters::dancer2::Dancer2KeywordState;
use crate::route::{
    RouteDeclaration, RouteFact, RouteHandlerContextFact, RouteParameterFact,
    RouteParameterSegment, RoutePrefixDeclaration, RoutePrefixFact,
};
use crate::{FileId, SourceGeneration};

/// Reviewed default method vocabulary for a bare `any` route.
///
/// Dancer2 1.x registers a bare `any` route without a method restriction, so
/// the normalized method set is the full reviewed HTTP vocabulary the dispatcher
/// admits.
pub const DANCER2_ANY_DEFAULT_METHODS: &[&str] =
    &["DELETE", "GET", "HEAD", "OPTIONS", "PATCH", "POST", "PUT"];

/// Route keywords of the reviewed Dancer2 DSL contract.
pub const DANCER2_ROUTE_KEYWORDS: &[&str] =
    &["get", "post", "put", "del", "options", "patch", "any"];

/// Whether the reviewed upstream registration would accept this route
/// declaration (#8921).
///
/// A route carrying a deprecated `:splat`/`:captures` placeholder (local or
/// contributed by the composed prefix) never registers: the upstream route
/// constructor croaks before the handler can run, so no handler-context fact
/// may claim route-handler-only DSL availability for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteRegistration {
    /// The declaration registers under the reviewed upstream constructor.
    Registers,
    /// The declaration is rejected by the reviewed upstream constructor.
    NeverRegisters,
}

/// One source-extracted Dancer2 route declaration awaiting minting.
///
/// Produced by the AST extractor in
/// `perl_semantic_analyzer::analysis::dancer2_routes`; this carrier adds the
/// package/file/declaration identity around the canonical payload and the
/// route-local parameter segments extracted from the pattern operand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dancer2RouteDeclaration {
    /// Package the declaration appears in (activation scope).
    pub package: Option<String>,
    /// File the declaration appears in.
    pub file_id: FileId,
    /// Full declaration range (keyword start to last operand end).
    pub declaration_start_byte: u32,
    /// End of the full declaration range.
    pub declaration_end_byte: u32,
    /// Canonical route payload (name/methods/pattern/effective/options/handler).
    pub route: RouteDeclaration,
    /// Route-local parameter/capture segments of the pattern operand, in
    /// source order (#8921).
    pub parameters: Vec<RouteParameterSegment>,
    /// Whether the reviewed upstream registration accepts this declaration
    /// (#8921): never-registering routes keep their (degraded) route and
    /// parameter facts but mint no handler-context fact.
    pub registration: RouteRegistration,
}

/// One source-extracted Dancer2 prefix declaration awaiting minting (#8921).
///
/// Produced by the AST extractor in
/// `perl_semantic_analyzer::analysis::dancer2_routes`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dancer2PrefixDeclaration {
    /// Package the declaration appears in (activation scope).
    pub package: Option<String>,
    /// File the declaration appears in.
    pub file_id: FileId,
    /// Full declaration range (keyword start to last operand end).
    pub declaration_start_byte: u32,
    /// End of the full declaration range.
    pub declaration_end_byte: u32,
    /// Canonical prefix payload (selection/scope).
    pub prefix: RoutePrefixDeclaration,
}

/// Minted canonical route-family facts for one activating package (#8921).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Dancer2RouteFacts {
    /// Route declaration facts.
    pub routes: Vec<RouteFact>,
    /// Prefix declaration facts.
    pub prefixes: Vec<RoutePrefixFact>,
    /// Route-local parameter/capture facts.
    pub parameters: Vec<RouteParameterFact>,
    /// Inline handler-context facts.
    pub handler_contexts: Vec<RouteHandlerContextFact>,
    /// Which Dancer2 contract minted these facts: bundles from different
    /// contracts never merge, and the marker keeps the provenance durable
    /// instead of letting consumers guess from the version alone (#14989).
    pub contract: RouteFactsContract,
}

/// Which Dancer2 contract a route-family bundle belongs to (#14989).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RouteFactsContract {
    /// The registry-backed 1.x contract (production adapter).
    #[default]
    OneX,
    /// The pinned 2.x contract (shadow adapter; comparison-only).
    TwoX,
}

/// Keyword-state view the shared route-family minting core needs. Both the
/// 1.x and the 2.x activation facts implement it, so the core minting
/// logic exists exactly once (#14989).
pub trait RouteFamilyKeywordView {
    /// Whether the activation is exact (anything else mints nothing).
    fn is_exact(&self) -> bool;
    /// The adapter identity every minted leaf carries — 2.x facts must
    /// attribute to the 2.x adapter, never the 1.x one (#15006 review).
    fn adapter_id(&self) -> crate::framework::AdapterId;
    /// Application name the activation established, when exact.
    fn application_name(&self) -> Option<&str>;
    /// Observed framework version, when known.
    fn framework_version(&self) -> Option<&str>;
    /// Source generation of the activation, when known.
    fn source_generation(&self) -> Option<&SourceGeneration>;
    /// State of one DSL keyword under this activation.
    fn keyword_state(&self, keyword: &str) -> Option<Dancer2KeywordState>;
    /// Whether `prefix` was actually imported (composed projections are
    /// only proven then).
    fn prefix_keyword_imported(&self) -> bool {
        self.keyword_state("prefix") == Some(Dancer2KeywordState::Imported)
    }
}
