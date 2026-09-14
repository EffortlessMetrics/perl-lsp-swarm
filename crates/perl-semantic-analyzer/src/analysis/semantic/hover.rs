//! Hover information types and documentation extraction.

use perl_semantic_facts::{Confidence, Provenance};

/// Detail line appended to every `AUTOLOAD`-resolved method hover.
///
/// `AUTOLOAD` dispatch is a `DynamicBoundary` under PLSP-SPEC-0017: the handler
/// is source-backed, but the method name that reaches it is computed at runtime.
/// Stating that explicitly is the "may explain fallback" behaviour the spec
/// allows, as opposed to presenting the handler as an exact definition.
///
/// Exposed so LSP-side tests can assert the rendered card without duplicating
/// the wording.
pub const AUTOLOAD_DYNAMIC_DISPATCH_DETAIL: &str =
    "Dispatch is dynamic: the method name is resolved at runtime, not from source.";

/// Hover information for symbols displayed in LSP hover requests.
///
/// Provides comprehensive symbol information including signature,
/// documentation, and contextual details for enhanced developer experience.
///
/// Used during Navigate/Analyze stages to answer hover queries.
///
/// # Performance Characteristics
/// - Computation: <100μs for typical symbol lookup
/// - Memory: Cached per symbol for repeated access
/// - LSP response: <50ms end-to-end including network
///
/// # Perl Context Integration
/// - Subroutine signatures with parameter information
/// - Package qualification and scope context
/// - POD documentation extraction and formatting
/// - Variable type inference and usage patterns
///
/// Workflow: Navigate/Analyze hover details for LSP.
#[derive(Debug, Clone)]
pub struct HoverInfo {
    /// Symbol signature or declaration
    pub signature: String,
    /// Documentation extracted from POD or comments
    pub documentation: Option<String>,
    /// Additional contextual details
    pub details: Vec<String>,
    /// How strongly this hover is backed by source evidence.
    ///
    /// [`Confidence::High`] means the signature names the subroutine the call
    /// actually reaches. [`Confidence::Low`] means it does not.
    ///
    /// This field is deliberately required rather than defaulted: per
    /// PLSP-SPEC-0017 a dynamic boundary "must not become exact definition,
    /// reference, symbol, token, or receiver proof", and a default of
    /// [`Confidence::High`] would silently grant that authority to any future
    /// dynamic hover path.
    ///
    /// Confidence alone does **not** identify a dynamic boundary — see
    /// [`provenance`](Self::provenance).
    pub confidence: Confidence,
    /// Which evidence class produced this hover, or `None` when it has not been
    /// classified yet.
    ///
    /// PLSP-SPEC-0002 lists "Low confidence" and "Dynamic boundary" as *separate*
    /// states with different required behaviour, so confidence cannot stand in
    /// for provenance: a future heuristic hover could legitimately be
    /// [`Confidence::Low`] without being a dynamic boundary. Consumers that need
    /// to know whether a fact is a boundary must test this field for
    /// [`Provenance::DynamicBoundary`], not test `confidence` for
    /// [`Confidence::Low`].
    ///
    /// `None` is the spec's "explicitly remains unknown". Only `AUTOLOAD`
    /// dispatch is classified today; the remaining hover sites are left
    /// unclassified deliberately rather than being asserted as exact, because
    /// some of them (framework-generated accessors reaching the symbol table,
    /// for instance) are `SourceBackedGenerated` rather than exact source, and
    /// minting a class for them here would be a new unproven claim.
    ///
    /// Consumers must branch on this field rather than pattern-matching the
    /// prose in [`details`](Self::details).
    pub provenance: Option<Provenance>,
}
