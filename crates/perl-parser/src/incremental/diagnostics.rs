use lsp_types::Diagnostic;
use perl_parser_core::error::ParseOutput;
use std::ops::Range;

/// Lexer work strategy selected for one incremental parse result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum LexRestartStrategy {
    /// Reuse the current token stream without performing lexer work.
    Unchanged,
    /// Lex the complete current source from byte zero.
    FullRelex,
    /// Restore one complete live lexer checkpoint and re-lex from there to EOF.
    LiveCheckpointToEof,
}

/// Truthful lexer restart and token-retention receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct LexRestartReport {
    /// Strategy that produced the current token stream.
    pub strategy: LexRestartStrategy,
    /// Byte boundary where fresh lexing began.
    ///
    /// For [`LexRestartStrategy::Unchanged`], this is the current source length:
    /// the complete old token stream is retained and no byte is freshly lexed.
    pub restart_byte: usize,
    /// Number of source bytes lexed from the restart boundary to EOF.
    pub relexed_bytes: usize,
    /// Tokens before the restart boundary retained without re-lexing.
    pub reused_prefix_tokens: usize,
    /// Tokens after a synchronization boundary retained from the old suffix.
    pub reused_suffix_tokens: usize,
}

impl LexRestartReport {
    /// Total old tokens retained by the selected strategy.
    #[must_use]
    pub fn reused_tokens(self) -> usize {
        self.reused_prefix_tokens.saturating_add(self.reused_suffix_tokens)
    }
}

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
    /// Lexer restart, fresh-work, and token-retention receipt.
    pub lex_restart: LexRestartReport,
    /// Number of source bytes covered by parser reparsing work.
    pub reparsed_bytes: usize,
    /// Compatibility total of old lexer tokens retained from prefix and suffix.
    ///
    /// New consumers should use [`Self::lex_restart`] to distinguish prefix
    /// retention from state-proven suffix reuse.
    pub reused_tokens: usize,
    /// Total token count in the resulting incremental state.
    pub token_count: usize,
}
