//! External Perl::Tidy compatibility adapter.
//!
//! Native document and range formatting live in the typed policy wrapper. This
//! module intentionally retains only the subprocess-backed whole-document path.

use super::{FormatRange, FormatTextEdit, FormattedDocument, FormattingOptions};
use perl_subprocess_runtime::SubprocessRuntime;
use perl_tidy_perltidy::{ExternalPerlTidyError, LegacyPerlTidyExecutor};

/// Legacy spelling for the external adapter configuration.
pub use perl_tidy_perltidy::ExternalPerlTidyConfig as PerlTidyConfig;

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

impl From<ExternalPerlTidyError> for FormattingError {
    fn from(error: ExternalPerlTidyError) -> Self {
        match error {
            ExternalPerlTidyError::Runtime { message } => Self::PerltidyNotFound(message),
            ExternalPerlTidyError::CommandNotFound { stderr, .. } => Self::PerltidyNotFound(stderr),
            ExternalPerlTidyError::Rejected { stderr, .. } => Self::PerltidyError(stderr),
            ExternalPerlTidyError::InvalidUtf8 { .. } => Self::InvalidOutputEncoding,
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
        LegacyPerlTidyExecutor::new(&self.runtime)
            .format(program, &args, content)
            .map_err(FormattingError::from)
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
        assert_eq!(args.first().map(String::as_str), Some("-st"));
        assert_eq!(args.get(1).map(String::as_str), Some("-se"));
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

    /// A runtime that completes the subprocess with a non-success status and
    /// canned stderr, standing in for either a failed perltidy run or a
    /// cmd.exe wrapper that could not start its target.
    struct FailingRuntime {
        stderr: &'static str,
        status_code: i32,
    }

    impl SubprocessRuntime for FailingRuntime {
        fn run_command(
            &self,
            _program: &str,
            _args: &[&str],
            _stdin: Option<&[u8]>,
        ) -> std::result::Result<SubprocessOutput, SubprocessError> {
            Ok(SubprocessOutput {
                stdout: Vec::new(),
                stderr: self.stderr.as_bytes().to_vec(),
                status_code: self.status_code,
            })
        }
    }

    /// Regression control for #17301: a resolved-but-unstartable perltidy (the
    /// OS wrapper reports it "is not recognized") is an install/repair
    /// problem, so it must carry `perltidy_not_found` and the Infra class, not
    /// send the user auditing their Perl syntax.
    #[test]
    fn unstartable_perltidy_classifies_as_not_found() -> Result<()> {
        for (stderr, status_code) in [
            (
                "'\"C:\\Strawberry\\perl\\bin\\perltidy.BAT\"' is not recognized as an internal \
                 or external command,\r\noperable program or batch file.\r\n",
                1,
            ),
            ("'perltidy' is not recognized as an internal or external command.\r\n", 9009),
            ("", 9009),
        ] {
            let provider = FormattingProvider::new(FailingRuntime { stderr, status_code });
            let error = provider
                .format_document("my $x = 1;\n", &options())
                .err()
                .ok_or_else(|| anyhow::anyhow!("launch failure must fail closed"))?;
            assert_eq!(error.error_kind(), "perltidy_not_found", "stderr was: {stderr:?}");
            assert!(
                error.to_string().starts_with("perltidy not found:"),
                "launch failure must keep the install remediation text: {error}"
            );
            assert_eq!(
                perl_parser_core::ErrorClass::error_class(&error),
                perl_parser_core::ErrorCategory::Infra
            );
        }
        Ok(())
    }

    /// A child that demonstrably executed can emit OS errors while reading its
    /// profile. Preserve its stderr and typed tool error instead of install advice.
    #[cfg(unix)]
    #[test]
    fn executed_perltidy_os_errors_remain_tool_errors() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        for stderr in [
            "Access is denied.",
            "perltidy: cannot read profile /tmp/Access is denied.rc: Access is denied",
            "The filename, directory name, or volume label syntax is incorrect.",
        ] {
            let directory = tempfile::tempdir()?;
            let program = directory.path().join("fake-perltidy");
            std::fs::write(
                &program,
                format!(
                    "#!/bin/sh\nprintf ran > \"$0.ran\"\ncat >/dev/null\nprintf 'untrusted output'\nprintf '%s\\n' '{stderr}' >&2\nexit 1\n"
                ),
            )?;
            std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700))?;
            let provider = FormattingProvider::new(
                perl_subprocess_runtime::OsSubprocessRuntime::with_timeout(5),
            )
            .with_perltidy_path(
                program
                    .to_str()
                    .ok_or_else(|| anyhow::anyhow!("fixture path must be UTF-8"))?
                    .to_owned(),
            );
            let error =
                provider.format_document("my $x = 1;\n", &options()).err().ok_or_else(|| {
                    anyhow::anyhow!("non-success child must withhold formatted output")
                })?;

            assert_eq!(std::fs::read(program.with_extension("ran"))?, b"ran");
            assert!(
                matches!(&error, FormattingError::PerltidyError(message) if message == &format!("{stderr}\n"))
            );
            assert_eq!(error.error_kind(), "perltidy_error");
            assert_eq!(
                perl_parser_core::ErrorClass::error_class(&error),
                perl_parser_core::ErrorCategory::UserError
            );
            assert_eq!(
                error.to_string(),
                format!("perltidy error (check Perl syntax): {stderr}\n")
            );
            assert!(!error.to_string().contains("To install perltidy:"));
        }
        Ok(())
    }

    /// Actual spawn failure keeps the actionable missing-tool error independently
    /// of stderr heuristics for a completed wrapper process.
    #[test]
    fn missing_perltidy_executable_keeps_install_guidance() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let program = directory.path().join("missing-perltidy.exe");
        let provider =
            FormattingProvider::new(perl_subprocess_runtime::OsSubprocessRuntime::with_timeout(5))
                .with_perltidy_path(
                    program
                        .to_str()
                        .ok_or_else(|| anyhow::anyhow!("fixture path must be UTF-8"))?
                        .to_owned(),
                );
        let error = provider
            .format_document("my $x = 1;\n", &options())
            .err()
            .ok_or_else(|| anyhow::anyhow!("missing executable must fail closed"))?;
        assert!(
            matches!(&error, FormattingError::PerltidyNotFound(message) if !message.is_empty())
        );
        assert_eq!(error.error_kind(), "perltidy_not_found");
        assert_eq!(
            perl_parser_core::ErrorClass::error_class(&error),
            perl_parser_core::ErrorCategory::Infra
        );
        assert!(error.to_string().contains("To install perltidy:"));
        Ok(())
    }

    /// Perltidy that starts and rejects the input keeps the syntax-advice
    /// classification, so the not-found reclassification cannot swallow it.
    #[test]
    fn perltidy_rejection_still_classifies_as_perltidy_error() -> Result<()> {
        let provider = FormattingProvider::new(FailingRuntime {
            stderr: "syntax error at \"(stdin)\" line 2\n",
            status_code: 1,
        });
        let error = provider
            .format_document("my $x = ;\n", &options())
            .err()
            .ok_or_else(|| anyhow::anyhow!("perltidy rejection must fail closed"))?;
        assert_eq!(error.error_kind(), "perltidy_error");
        assert!(
            error.to_string().starts_with("perltidy error (check Perl syntax):"),
            "genuine perltidy failure must keep the syntax remediation text: {error}"
        );
        assert_eq!(
            perl_parser_core::ErrorClass::error_class(&error),
            perl_parser_core::ErrorCategory::UserError
        );
        Ok(())
    }

    #[test]
    fn profile_does_not_receive_editor_fallbacks_or_scalar_defaults() -> Result<()> {
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let provider = FormattingProvider::new(RecordingRuntime {
            args: recorded.clone(),
            output: b"my $x = 1;\n".to_vec(),
        })
        .with_perltidy_config(PerlTidyConfig {
            profile: Some("unreadable profile.rc".to_string()),
            extra_args: vec!["--keep-this-argument".to_string()],
            ..PerlTidyConfig::default()
        });
        provider.format_document("my$x=1;\n", &options())?;
        let args = recorded.lock().map_err(|_| anyhow::anyhow!("args mutex poisoned"))?;
        assert_eq!(
            *args,
            ["-st", "-se", "--profile=unreadable profile.rc", "--keep-this-argument"]
        );
        Ok(())
    }

    #[test]
    fn configured_indentation_and_tabs_suppress_only_their_editor_fallbacks() -> Result<()> {
        for (indent, tabs, expected_indent_fallback, expected_tabs_fallback) in [
            (Some(2), Some(false), false, false),
            (Some(2), None, false, true),
            (None, Some(true), true, false),
        ] {
            let recorded = Arc::new(Mutex::new(Vec::new()));
            let provider = FormattingProvider::new(RecordingRuntime {
                args: recorded.clone(),
                output: b"my $x = 1;\n".to_vec(),
            })
            .with_perltidy_config(PerlTidyConfig {
                indent_columns: indent,
                tabs,
                ..PerlTidyConfig::default()
            });
            provider.format_document("my$x=1;\n", &options())?;
            let args = recorded.lock().map_err(|_| anyhow::anyhow!("args mutex poisoned"))?;
            assert_eq!(args.first().map(String::as_str), Some("-st"));
            assert_eq!(args.get(1).map(String::as_str), Some("-se"));
            assert_eq!(args.contains(&"-i=4".to_string()), expected_indent_fallback);
            assert_eq!(args.contains(&"-et=4".to_string()), expected_tabs_fallback);
            assert_eq!(args.contains(&"--indent-columns=2".to_string()), indent.is_some());
            assert_eq!(args.contains(&"--tabs".to_string()), tabs == Some(true));
            assert_eq!(args.contains(&"--notabs".to_string()), tabs == Some(false));
        }
        Ok(())
    }
}
