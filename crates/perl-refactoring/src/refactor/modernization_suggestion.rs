//! Shared data type for Perl modernization suggestions.
//!
//! `ModernizationSuggestion` is defined once here and re-exported from both
//! [`super::modernize`] and [`super::modernize_refactored`] so the two
//! modernization engines share a single canonical definition instead of
//! maintaining identical copies that can silently drift (see #3924).

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
