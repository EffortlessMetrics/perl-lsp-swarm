use crate::incremental::work::IncrementalWorkReceipt;
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
    /// Restore a complete checkpoint reproduced by replaying the old prefix.
    #[deprecated(note = "Use StoredCheckpointToEof; replay is no longer the canonical path.")]
    LiveCheckpointToEof,
    /// Restore a complete generation-bound checkpoint without replaying old bytes.
    StoredCheckpointToEof,
}

/// Truthful lexer restart and token-retention receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct LexRestartReport {
    /// Strategy that produced the current token stream.
    pub strategy: LexRestartStrategy,
    /// Byte boundary where fresh lexing began.
    pub restart_byte: usize,
    /// Number of old-source prefix bytes replayed only to reconstruct state.
    pub old_prefix_bytes_replayed: usize,
    /// Number of current-source bytes lexed from restart to EOF.
    pub relexed_bytes: usize,
    /// Tokens before the restart boundary retained without re-lexing.
    pub reused_prefix_tokens: usize,
    /// Tokens after a synchronization boundary retained from the old suffix.
    pub reused_suffix_tokens: usize,
    /// Complete generation-bound checkpoints retained in the resulting state.
    pub stored_checkpoint_count: usize,
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
    pub parse_output: ParseOutput,
    /// Legacy LSP-shaped diagnostics retained for compatibility.
    pub diagnostics: Vec<Diagnostic>,
    /// Lexer restart, fresh-work, and token-retention receipt.
    pub lex_restart: LexRestartReport,
    /// Validated production strategy and performed-work receipt.
    pub work: IncrementalWorkReceipt,
    /// Number of source bytes covered by parser reparsing work.
    pub reparsed_bytes: usize,
    /// Compatibility total of old lexer tokens retained from prefix and suffix.
    pub reused_tokens: usize,
    /// Total token count in the resulting incremental state.
    pub token_count: usize,
}