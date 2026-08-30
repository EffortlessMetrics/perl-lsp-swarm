//! Hover information types and documentation extraction.

use perl_semantic_facts::Confidence;

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
    /// actually reaches. [`Confidence::Low`] marks a `DynamicBoundary`
    /// resolution in the sense of PLSP-SPEC-0017 — today, `AUTOLOAD` dispatch —
    /// where the requested method name is only known at runtime.
    ///
    /// This field is deliberately required rather than defaulted: per
    /// PLSP-SPEC-0017 a dynamic boundary "must not become exact definition,
    /// reference, symbol, token, or receiver proof", and a default of
    /// [`Confidence::High`] would silently grant that authority to any future
    /// dynamic hover path.
    ///
    /// Consumers must branch on this field rather than pattern-matching the
    /// prose in [`details`](Self::details).
    pub confidence: Confidence,
}
