//! Formatting compatibility shim for LSP.
//!
//! [`CodeFormatter`] owns the OS subprocess runtime adapter while delegating all
//! formatter admission and outcome classification to `perl-lsp-rs-core`.

use crate::convert::WireRange;
pub use perl_lsp_rs_core::providers::formatting::{
    FormatPosition, FormatRange, FormatTextEdit, FormattedDocument, FormattingDecision,
    FormattingError, FormattingOptions, FormattingProvider, PerlTidyConfig,
};
use perl_lsp_rs_core::tooling::OsSubprocessRuntime;
use perl_lsp_rs_core::tooling::perltidy::FormatterMode;
pub use perl_lsp_rs_core::tooling::perltidy::native::FormatContext;

/// Code formatter using the OS subprocess runtime.
pub struct CodeFormatter {
    inner: FormattingProvider<OsSubprocessRuntime>,
}

impl CodeFormatter {
    /// Create a native-default formatter with the standard interactive timeout.
    pub fn new() -> Self {
        Self { inner: FormattingProvider::new(OsSubprocessRuntime::with_timeout(10)) }
    }

    /// Create a native formatter with perltidy-compatible style configuration.
    pub fn with_config(config: PerlTidyConfig) -> Self {
        Self::with_config_and_mode(config, FormatterMode::Native)
    }

    /// Create a formatter with explicit configuration and requested engine mode.
    pub fn with_config_and_mode(config: PerlTidyConfig, mode: FormatterMode) -> Self {
        let timeout = config.timeout_secs;
        Self {
            inner: FormattingProvider::new(OsSubprocessRuntime::with_timeout(timeout))
                .with_perltidy_config(config)
                .with_formatter_mode(mode),
        }
    }

    /// Format a document and return only edits for compatibility callers.
    pub fn format_document(
        &self,
        content: &str,
        options: &FormattingOptions,
    ) -> Result<Vec<FormatTextEdit>, FormattingError> {
        self.format_document_decision(content, options, &FormatContext::default())
            .map(|decision| decision.document.edits)
    }

    /// Format a document and retain the typed terminal decision.
    pub fn format_document_decision(
        &self,
        content: &str,
        options: &FormattingOptions,
        context: &FormatContext,
    ) -> Result<FormattingDecision, FormattingError> {
        self.inner.format_document_decision(content, options, context)
    }

    /// Format a range and return only edits for compatibility callers.
    pub fn format_range(
        &self,
        content: &str,
        range: &WireRange,
        options: &FormattingOptions,
    ) -> Result<Vec<FormatTextEdit>, FormattingError> {
        self.format_range_decision(content, range, options, &FormatContext::default())
            .map(|decision| decision.document.edits)
    }

    /// Format a range and retain the typed terminal decision.
    pub fn format_range_decision(
        &self,
        content: &str,
        range: &WireRange,
        options: &FormattingOptions,
        context: &FormatContext,
    ) -> Result<FormattingDecision, FormattingError> {
        self.inner.format_range_decision(content, &to_format_range(range), options, context)
    }
}

impl Default for CodeFormatter {
    fn default() -> Self {
        Self::new()
    }
}

fn to_format_range(range: &WireRange) -> FormatRange {
    FormatRange {
        start: FormatPosition { line: range.start.line, character: range.start.character },
        end: FormatPosition { line: range.end.line, character: range.end.character },
    }
}
