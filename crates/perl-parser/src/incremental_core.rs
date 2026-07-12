//! Compatibility adapter over the lower-tier incremental parser kernel.
//!
//! The long-standing [`crate::incremental::IncrementalState`] remains
//! available for downstream compatibility and delegates safe single edits to
//! this same kernel while retaining its historical public caches. New
//! consumers can use this explicit adapter when they need the shared
//! `perl-parser-core` token-replay engine; the tree-sitter facade uses the
//! kernel directly.

use perl_ast::Node;
use perl_parser_core::{ParseError, Parser, incremental};

pub use perl_parser_core::incremental::{FallbackReason, IncrementalEdit, IncrementalMetrics};

/// A typed failure while creating or advancing a core-backed incremental state.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum CoreIncrementalError {
    /// The initial source could not produce an AST.
    InitialParse(ParseError),
    /// The requested edit could not be applied safely.
    Edit(ParseError),
}

impl std::fmt::Display for CoreIncrementalError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InitialParse(error) => write!(formatter, "initial parse failed: {error}"),
            Self::Edit(error) => write!(formatter, "incremental edit failed: {error}"),
        }
    }
}

impl std::error::Error for CoreIncrementalError {}

/// One result from the shared incremental kernel.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct CoreIncrementalResult {
    /// The AST produced by the replay or safe full-parse fallback.
    pub ast: Node,
    /// Measurements and fallback classification for this operation.
    pub metrics: IncrementalMetrics,
    /// Diagnostics collected while parsing the resulting source.
    pub diagnostics: Vec<ParseError>,
}

/// A `perl-parser` compatibility facade over `perl-parser-core::incremental`.
#[derive(Debug, Clone)]
pub struct CoreIncrementalState {
    kernel: incremental::IncrementalState,
    ast: Node,
}

impl CoreIncrementalState {
    /// Parse an initial source and create a core-backed incremental state.
    ///
    /// # Errors
    /// Returns [`CoreIncrementalError::InitialParse`] when no AST can be built.
    pub fn new(source: &str) -> Result<Self, CoreIncrementalError> {
        let mut parser = Parser::new(source);
        let ast = parser.parse().map_err(CoreIncrementalError::InitialParse)?;
        let kernel = incremental::IncrementalState::with_diagnostics(source, parser.errors());
        Ok(Self { kernel, ast })
    }

    /// Return the current source held by the shared kernel.
    #[must_use]
    pub fn source(&self) -> &str {
        self.kernel.source()
    }

    /// Return the current AST without exposing the kernel's internal cache.
    #[must_use]
    pub fn ast(&self) -> &Node {
        &self.ast
    }

    /// Apply one edit through the shared lower-tier kernel.
    ///
    /// The kernel may safely fall back to a complete parse. The returned
    /// metrics make that decision observable to callers.
    ///
    /// # Errors
    /// Returns [`CoreIncrementalError::Edit`] when the edit is invalid or the
    /// parser cannot produce a usable AST.
    pub fn apply_edit(
        &mut self,
        new_source: &str,
        edit: &IncrementalEdit,
    ) -> Result<CoreIncrementalResult, CoreIncrementalError> {
        let ast = self.kernel.reparse(new_source, edit).map_err(CoreIncrementalError::Edit)?;
        self.ast = ast.clone();
        Ok(CoreIncrementalResult {
            ast,
            metrics: self.kernel.metrics().clone(),
            diagnostics: self.kernel.diagnostics().to_vec(),
        })
    }

    /// Return measurements from the most recent operation.
    #[must_use]
    pub fn metrics(&self) -> &IncrementalMetrics {
        self.kernel.metrics()
    }

    /// Return diagnostics from the most recent operation.
    #[must_use]
    pub fn diagnostics(&self) -> &[ParseError] {
        self.kernel.diagnostics()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_matches_a_fresh_parse_and_reports_reuse() -> Result<(), Box<dyn std::error::Error>> {
        let source = "my $first = 1;\n".repeat(40);
        let start = source.rfind("$first").ok_or("fixture variable missing")? + 1;
        let mut new_source = source.clone();
        new_source.replace_range(start..start + 5, "frist");
        let edit = IncrementalEdit::new(start, start + 5, "frist");

        let mut state = CoreIncrementalState::new(&source)?;
        let result = state.apply_edit(&new_source, &edit)?;
        let mut fresh_parser = Parser::new(&new_source);
        let fresh = fresh_parser.parse()?;

        if result.ast.to_sexp() != fresh.to_sexp() {
            return Err("incremental AST differs from a fresh parse".into());
        }
        if result.metrics.full_parse {
            return Err("safe replay unexpectedly fell back to a full parse".into());
        }
        if result.metrics.tokens_reused == 0 {
            return Err("safe replay reported no reused tokens".into());
        }
        if state.source() != new_source {
            return Err("incremental state retained the old source".into());
        }
        Ok(())
    }

    #[test]
    fn oversized_edits_are_explicit_full_parse_fallbacks() -> Result<(), Box<dyn std::error::Error>>
    {
        let source = "my $value = 1;\n";
        let replacement = "x".repeat(4097);
        let new_source = format!("{replacement}{source}");
        let edit = IncrementalEdit::new(0, 0, &replacement);
        let mut state = CoreIncrementalState::new(source)?;
        let result = state.apply_edit(&new_source, &edit)?;

        if !result.metrics.full_parse {
            return Err("oversized edit did not use a full parse".into());
        }
        if result.metrics.fallback != Some(FallbackReason::EditTooLarge) {
            return Err("oversized edit did not report EditTooLarge".into());
        }
        Ok(())
    }
}
