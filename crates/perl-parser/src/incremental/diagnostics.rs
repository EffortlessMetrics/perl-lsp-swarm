use crate::incremental::snapshot::ParseSnapshot;
use lsp_types::Diagnostic;
use perl_parser_core::error::ParseOutput;
use std::ops::Range;

/// Result of incremental reparse.
#[derive(Debug)]
#[non_exhaustive]
pub struct ReparseResult {
    /// Byte ranges reparsed or replaced by the selected strategy.
    pub changed_ranges: Vec<Range<usize>>,
    /// Generation-bound parser snapshot for the committed source.
    ///
    /// This is the sole owned parser-output authority in the result; read the
    /// native output through [`Self::parse_output`], which projects from the
    /// snapshot so the two can never diverge.
    pub snapshot: ParseSnapshot,
    /// Legacy LSP-shaped diagnostics retained for compatibility.
    ///
    /// Parser consumers should use `snapshot.parse_output().diagnostics`. LSP
    /// projection is a transport concern and remains intentionally separate
    /// from the native parser output contract.
    pub diagnostics: Vec<Diagnostic>,
    /// Number of source bytes covered by reparsing work.
    pub reparsed_bytes: usize,
    /// Number of lexer tokens retained from the previous state.
    pub reused_tokens: usize,
    /// Total token count in the resulting incremental state.
    pub token_count: usize,
}

impl ReparseResult {
    /// Native recovery-aware parser output for the current source generation,
    /// projected from the generation-bound snapshot.
    #[must_use]
    pub fn parse_output(&self) -> &ParseOutput {
        self.snapshot.parse_output()
    }
}
