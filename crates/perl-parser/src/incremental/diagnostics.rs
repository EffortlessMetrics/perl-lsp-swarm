use lsp_types::Diagnostic;
use perl_parser_core::error::ParseOutput;
use std::ops::Range;

/// Result of incremental reparse.
#[derive(Debug)]
#[non_exhaustive]
pub struct ReparseResult {
    /// Byte ranges reparsed or replaced by the selected strategy.
    pub changed_ranges: Vec<Range<usize>>,
    /// Authoritative native parser output for the current source generation.
    ///
    /// This carries the AST, ordered parser diagnostics, recovery count,
    /// budget usage, and early-termination state produced by the same
    /// `Parser::parse_with_recovery` contract used by a fresh parse.
    pub parse_output: ParseOutput,
    /// Legacy LSP-shaped diagnostics retained for compatibility.
    ///
    /// Parser consumers should use [`Self::parse_output`]. LSP projection is a
    /// transport concern and remains intentionally separate from the native
    /// parser output contract.
    pub diagnostics: Vec<Diagnostic>,
    /// Number of source bytes covered by reparsing work.
    pub reparsed_bytes: usize,
    /// Number of lexer tokens retained from the previous state.
    pub reused_tokens: usize,
    /// Total token count in the resulting incremental state.
    pub token_count: usize,
}
