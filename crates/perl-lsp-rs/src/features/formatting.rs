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
use perl_subprocess_runtime::{SubprocessError, SubprocessOutput, SubprocessRuntime};
use std::sync::Arc;

const MAX_FORMATTER_TIMEOUT_SECS: u64 = 300;

/// Code formatter using the OS subprocess runtime.
pub struct CodeFormatter {
    inner: FormattingProvider<SharedSubprocessRuntime>,
}

struct SharedSubprocessRuntime(Arc<dyn SubprocessRuntime>);

impl SubprocessRuntime for SharedSubprocessRuntime {
    fn run_command(
        &self,
        program: &str,
        args: &[&str],
        stdin: Option<&[u8]>,
    ) -> Result<SubprocessOutput, SubprocessError> {
        self.0.run_command(program, args, stdin)
    }
}

impl CodeFormatter {
    /// Create a native-default formatter with the standard interactive timeout.
    pub fn new() -> Self {
        Self {
            inner: FormattingProvider::new(SharedSubprocessRuntime(Arc::new(
                OsSubprocessRuntime::with_bounded_timeout(10, MAX_FORMATTER_TIMEOUT_SECS),
            ))),
        }
    }

    /// Create a native formatter with perltidy-compatible style configuration.
    pub fn with_config(config: PerlTidyConfig) -> Self {
        Self::with_config_and_mode(config, FormatterMode::Native)
    }

    /// Create a formatter with explicit configuration and requested engine mode.
    pub fn with_config_and_mode(config: PerlTidyConfig, mode: FormatterMode) -> Self {
        let timeout = config.timeout_secs;
        Self {
            inner: FormattingProvider::new(SharedSubprocessRuntime(Arc::new(
                OsSubprocessRuntime::with_bounded_timeout(timeout, MAX_FORMATTER_TIMEOUT_SECS),
            )))
            .with_perltidy_config(config)
            .with_formatter_mode(mode),
        }
    }

    /// Create a formatter with a caller-supplied runtime for bounded tests.
    #[cfg(any(test, feature = "expose_lsp_test_api"))]
    pub(crate) fn with_runtime_and_config_and_mode(
        runtime: Arc<dyn SubprocessRuntime>,
        config: PerlTidyConfig,
        mode: FormatterMode,
    ) -> Self {
        Self {
            inner: FormattingProvider::new(SharedSubprocessRuntime(runtime))
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
