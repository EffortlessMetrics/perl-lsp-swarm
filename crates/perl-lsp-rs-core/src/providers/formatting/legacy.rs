//! External Perl::Tidy compatibility adapter.
//!
//! Native document and range formatting live in the typed policy wrapper. This
//! module intentionally retains only the subprocess-backed whole-document path.

use super::{FormatRange, FormatTextEdit, FormattedDocument, FormattingOptions};
use perl_subprocess_runtime::SubprocessRuntime;

/// Re-export PerlTidyConfig from perl-lsp-perltidy for convenience.
pub use perl_lsp_perltidy::PerlTidyConfig;

/// Formatting error reported by the external compatibility adapter.
#[derive(Debug, thiserror::Error)]
pub enum FormattingError {
    #[error(
        "perltidy not found: {0}\n\nTo install perltidy:\n  - Recommended: cpanm Perl::Tidy\n  - CPAN: cpan Perl::Tidy\n  - Debian/Ubuntu: apt-get install perltidy\n  - RedHat/Fedora: yum install perltidy\n  - macOS: brew install perltidy\n  - Windows: cpanm Perl::Tidy"
    )]
    /// Perltidy executable was not found or could not be started.
    PerltidyNotFound(String),
    /// Perltidy returned a non-success status.
    #[error("perltidy error (check Perl syntax): {0}")]
    PerltidyError(String),
    /// Perltidy returned bytes that are not valid UTF-8 source text.
    #[error("perltidy returned invalid UTF-8 output")]
    InvalidOutputEncoding,
    /// I/O error during file operations.
    #[error("IO error: {0}")]
    IoError(String),
}

impl FormattingError {
    /// Return a stable machine-readable error kind.
    #[must_use]
    pub fn error_kind(&self) -> &'static str {
        match self {
            Self::PerltidyNotFound(_) => "perltidy_not_found",
            Self::PerltidyError(_) => "perltidy_error",
            Self::InvalidOutputEncoding => "invalid_output_encoding",
            Self::IoError(_) => "io_error",
        }
    }
}

impl perl_parser_core::ErrorClass for FormattingError {
    fn error_class(&self) -> perl_parser_core::ErrorCategory {
        match self {
            Self::PerltidyNotFound(_) | Self::IoError(_) => perl_parser_core::ErrorCategory::Infra,
            Self::PerltidyError(_) => perl_parser_core::ErrorCategory::UserError,
            Self::InvalidOutputEncoding => perl_parser_core::ErrorCategory::Bug,
        }
    }
}

/// Whole-document Perl::Tidy compatibility adapter.
pub struct FormattingProvider<R> {
    runtime: R,
    perltidy_path: Option<String>,
    perltidy_config: Option<PerlTidyConfig>,
}

impl<R> FormattingProvider<R> {
    /// Create an external adapter with the given subprocess runtime.
    pub fn new(runtime: R) -> Self {
        Self { runtime, perltidy_path: None, perltidy_config: None }
    }

    /// Set a custom perltidy executable path.
    pub fn with_perltidy_path(mut self, path: String) -> Self {
        self.perltidy_path = Some(path);
        self
    }

    /// Set external perltidy configuration.
    pub fn with_perltidy_config(mut self, config: PerlTidyConfig) -> Self {
        self.perltidy_config = Some(config);
        self
    }
}

impl<R: SubprocessRuntime> FormattingProvider<R> {
    /// Format a whole document through external Perl::Tidy.
    pub fn format_document(
        &self,
        content: &str,
        options: &FormattingOptions,
    ) -> Result<FormattedDocument, FormattingError> {
        let formatted =
            super::apply_lsp_whitespace_options(&self.run_perltidy(content, options)?, options);
        if formatted == content {
            return Ok(FormattedDocument { text: formatted, edits: Vec::new() });
        }
        Ok(FormattedDocument {
            text: formatted.clone(),
            edits: vec![FormatTextEdit {
                range: FormatRange::whole_document(content),
                new_text: formatted,
            }],
        })
    }

    fn run_perltidy(
        &self,
        content: &str,
        options: &FormattingOptions,
    ) -> Result<String, FormattingError> {
        let mut args = vec!["-st".to_string(), "-se".to_string()];
        if let Some(config) = &self.perltidy_config {
            if config.profile.is_some() {
                args.append(&mut config.to_args());
            } else {
                let config_sets_indent = config.indent_columns.is_some();
                let config_sets_tabs = config.tabs.is_some();
                args.append(&mut config.to_args());
                if !config_sets_indent {
                    args.push(format!("-i={}", options.tab_size));
                }
                if !config_sets_tabs {
                    if options.insert_spaces {
                        args.push(format!("-et={}", options.tab_size));
                    } else {
                        args.push("-dt".to_string());
                    }
                }
            }
        } else if options.insert_spaces {
            args.push(format!("-et={}", options.tab_size));
            args.push(format!("-i={}", options.tab_size));
        } else {
            args.push("-dt".to_string());
            args.push(format!("-i={}", options.tab_size));
        }

        let program = self.perltidy_path.as_deref().unwrap_or("perltidy");
        let output = self
            .runtime
            .run_command(
                program,
                &args.iter().map(String::as_str).collect::<Vec<_>>(),
                Some(content.as_bytes()),
            )
            .map_err(|error| FormattingError::PerltidyNotFound(error.message))?;
        if !output.success() {
            return Err(FormattingError::PerltidyError(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }
        String::from_utf8(output.stdout).map_err(|_| FormattingError::InvalidOutputEncoding)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use perl_subprocess_runtime::{SubprocessError, SubprocessOutput};
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct RecordingRuntime {
        args: Arc<Mutex<Vec<String>>>,
        output: Vec<u8>,
    }

    impl SubprocessRuntime for RecordingRuntime {
        fn run_command(
            &self,
            _program: &str,
            args: &[&str],
            _stdin: Option<&[u8]>,
        ) -> std::result::Result<SubprocessOutput, SubprocessError> {
            *self.args.lock().map_err(|_| SubprocessError::new("args mutex poisoned"))? =
                args.iter().map(|arg| (*arg).to_string()).collect();
            Ok(SubprocessOutput { stdout: self.output.clone(), stderr: Vec::new(), status_code: 0 })
        }
    }

    fn options() -> FormattingOptions {
        FormattingOptions {
            tab_size: 4,
            insert_spaces: true,
            trim_trailing_whitespace: None,
            insert_final_newline: None,
            trim_final_newlines: None,
        }
    }

    #[test]
    fn external_adapter_formats_whole_document_and_records_options() -> Result<()> {
        let args = Arc::new(Mutex::new(Vec::new()));
        let provider = FormattingProvider::new(RecordingRuntime {
            args: args.clone(),
            output: b"my $external = 1;\n".to_vec(),
        });

        let formatted = provider.format_document("my$x=1;\n", &options())?;

        assert_eq!(formatted.edits.len(), 1);
        assert_eq!(formatted.edits[0].new_text, "my $external = 1;\n");
        let args = args.lock().map_err(|_| anyhow::anyhow!("args mutex poisoned"))?;
        assert!(
            args.contains(&"-i=4".to_string()),
            "editor indentation must reach perltidy: {args:?}"
        );
        assert!(
            args.contains(&"-et=4".to_string()),
            "editor spacing must reach perltidy: {args:?}"
        );
        Ok(())
    }

    #[test]
    fn external_adapter_rejects_invalid_utf8() -> Result<()> {
        let provider = FormattingProvider::new(RecordingRuntime {
            args: Arc::new(Mutex::new(Vec::new())),
            output: vec![0xff],
        });
        let error = provider
            .format_document("my $x = 1;\n", &options())
            .err()
            .ok_or_else(|| anyhow::anyhow!("invalid output must fail closed"))?;
        assert_eq!(error.error_kind(), "invalid_output_encoding");
        Ok(())
    }
}
