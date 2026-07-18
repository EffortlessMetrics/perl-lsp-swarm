//! Shared `ModernizationSuggestion` type.
//!
//! Both modernization implementations — [`crate::refactor::modernize`] (legacy
//! pattern checks) and [`crate::refactor::modernize_refactored`] (structured
//! pattern metadata) — produce the same suggestion shape. The type lives here
//! as the single source of truth and is re-exported from both modules so the
//! existing `modernize::ModernizationSuggestion` and
//! `modernize_refactored::ModernizationSuggestion` paths continue to resolve.

/// A suggestion for modernizing legacy Perl code patterns.
#[derive(Debug, Clone, PartialEq)]
pub struct ModernizationSuggestion {
    /// The deprecated or outdated code pattern to be replaced.
    pub old_pattern: String,
    /// The modern replacement pattern.
    pub new_pattern: String,
    /// Human-readable explanation of why this change is recommended.
    pub description: String,
    /// Whether this suggestion requires human review before applying.
    pub manual_review_required: bool,
    /// Byte offset where the pattern starts in the source code.
    pub start: usize,
    /// Byte offset where the pattern ends in the source code.
    pub end: usize,
}
