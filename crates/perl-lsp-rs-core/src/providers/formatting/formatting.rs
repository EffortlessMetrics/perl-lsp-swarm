//! Code formatting support for Perl parsing workflow pipeline.

pub use crate::providers::formatting_types::{
    FormatPosition, FormatRange, FormatTextEdit, FormattedDocument, FormattingOptions,
};
use crate::tooling::perltidy::{
    BracePlacement, ElsePlacement, FinalNewline, FormatConfig, FormatterMode, KeywordSpacing,
    NativeFormatter, PerlFormatter, TextPosition, TextRange, TrailingComma,
};

/// Re-export PerlTidyConfig from perl-lsp-perltidy for convenience.
pub use perl_lsp_perltidy::PerlTidyConfig;

/// Count the number of UTF-16 code units in `s`.
///
/// LSP positions use UTF-16 code units (see Language Server Protocol spec Â§3.1).
/// Characters in the Basic Multilingual Plane (U+0000â€“U+FFFF) count as 1 unit;
/// supplementary-plane characters (U+10000 and above) count as 2 units.
fn utf16_len(s: &str) -> usize {
    s.chars().map(|c| if c as u32 >= 0x10000 { 2 } else { 1 }).sum()
}

/// Formatting error.
#[derive(Debug, thiserror::Error)]
pub enum FormattingError {
    #[error(
        "perltidy not found: {0}\n\nTo install perltidy:\n  - Recommended: cpanm Perl::Tidy\n  - CPAN: cpan Perl::Tidy\n  - Debian/Ubuntu: apt-get install perltidy\n  - RedHat/Fedora: yum install perltidy\n  - macOS: brew install perltidy\n  - Windows: cpanm Perl::Tidy"
    )]
    /// perltidy executable not found on system PATH.
    PerltidyNotFound(String),

    /// Error occurred during perltidy execution.
    ///
    /// This usually means perltidy ran but reported a problem â€” check that the
    /// Perl code is syntactically valid, or inspect the perltidy output below.
    #[error("perltidy error (check Perl syntax): {0}")]
    PerltidyError(String),

    /// I/O error during file operations.
    #[error("IO error: {0}")]
    IoError(String),
}

impl FormattingError {
    /// Return a stable machine-readable error kind string for structured LSP error data.
    ///
    /// Used by LSP handlers to populate the JSON-RPC error `data` field so that
    /// clients (e.g. the VSCode extension) can present targeted remediation actions.
    #[must_use]
    pub fn error_kind(&self) -> &'static str {
        match self {
            Self::PerltidyNotFound(_) => "perltidy_not_found",
            Self::PerltidyError(_) => "perltidy_error",
            Self::IoError(_) => "io_error",
        }
    }
}

/// Code formatter using native formatting with an external perltidy adapter.
pub struct FormattingProvider<R> {
    /// Subprocess runtime for executing perltidy.
    runtime: R,
    /// Optional custom perltidy path.
    perltidy_path: Option<String>,
    /// Optional perltidy configuration.
    perltidy_config: Option<PerlTidyConfig>,
    /// Formatting engine selected for this provider.
    mode: FormatterMode,
}

impl<R> FormattingProvider<R> {
    /// Create a new formatting provider with the given runtime.
    pub fn new(runtime: R) -> Self {
        Self { runtime, perltidy_path: None, perltidy_config: None, mode: FormatterMode::Native }
    }

    /// Set a custom perltidy path.
    pub fn with_perltidy_path(mut self, path: String) -> Self {
        self.perltidy_path = Some(path);
        self
    }

    /// Set perltidy configuration.
    pub fn with_perltidy_config(mut self, config: PerlTidyConfig) -> Self {
        self.perltidy_config = Some(config);
        self
    }

    /// Select the formatter engine.
    pub fn with_formatter_mode(mut self, mode: FormatterMode) -> Self {
        self.mode = mode;
        self
    }
}

impl<R: perl_subprocess_runtime::SubprocessRuntime> FormattingProvider<R> {
    /// Format the entire Perl script document.
    pub fn format_document(
        &self,
        content: &str,
        options: &FormattingOptions,
    ) -> Result<FormattedDocument, FormattingError> {
        match self.mode {
            // Compat mode is currently a no-op alias for Native (#5054 item 6).
            FormatterMode::Native | FormatterMode::Compat => {
                Ok(native_format_document(content, options, self.perltidy_config.as_ref()))
            }
            FormatterMode::ExternalLegacy => self.format_document_with_perltidy(content, options),
            FormatterMode::Off => {
                Ok(FormattedDocument { text: content.to_string(), edits: vec![] })
            }
        }
    }

    /// Format a specific range in the document.
    pub fn format_range(
        &self,
        content: &str,
        range: &FormatRange,
        options: &FormattingOptions,
    ) -> Result<FormattedDocument, FormattingError> {
        let lines: Vec<&str> = content.lines().collect();
        let start_line = range.start.line as usize;
        let end_line = (range.end.line as usize).min(lines.len().saturating_sub(1));

        if start_line >= lines.len() {
            return Ok(FormattedDocument { text: content.to_string(), edits: vec![] });
        }

        if end_line < start_line {
            return Ok(FormattedDocument { text: content.to_string(), edits: vec![] });
        }

        match self.mode {
            // Compat mode is currently a no-op alias for Native (#5054 item 6).
            FormatterMode::Native | FormatterMode::Compat => {
                Ok(native_format_range(content, range, options, self.perltidy_config.as_ref()))
            }
            FormatterMode::ExternalLegacy => {
                self.format_range_with_perltidy(content, options, &lines, start_line, end_line)
            }
            FormatterMode::Off => {
                Ok(FormattedDocument { text: content.to_string(), edits: vec![] })
            }
        }
    }

    fn format_document_with_perltidy(
        &self,
        content: &str,
        options: &FormattingOptions,
    ) -> Result<FormattedDocument, FormattingError> {
        let formatted =
            apply_lsp_whitespace_options(&self.run_perltidy(content, options)?, options);

        if formatted == content {
            return Ok(FormattedDocument { text: formatted, edits: vec![] });
        }

        Ok(FormattedDocument {
            text: formatted.clone(),
            edits: vec![FormatTextEdit {
                range: FormatRange::whole_document(content),
                new_text: formatted,
            }],
        })
    }

    fn format_range_with_perltidy(
        &self,
        content: &str,
        options: &FormattingOptions,
        lines: &[&str],
        start_line: usize,
        end_line: usize,
    ) -> Result<FormattedDocument, FormattingError> {
        // Detect line ending to preserve CRLF (#5075)
        let line_ending = if content.contains("\r\n") { "\r\n" } else { "\n" };
        let text_to_format = lines[start_line..=end_line].join(line_ending);
        let formatted = self.run_perltidy(&text_to_format, options)?;

        if formatted == text_to_format {
            return Ok(FormattedDocument { text: content.to_string(), edits: vec![] });
        }

        Ok(FormattedDocument {
            text: content.to_string(),
            edits: vec![FormatTextEdit {
                range: FormatRange::new(
                    FormatPosition::new(start_line as u32, 0),
                    FormatPosition::new(end_line as u32, utf16_len(lines[end_line]) as u32),
                ),
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

        // If we have a perltidy config, use it to generate args
        if let Some(ref config) = self.perltidy_config {
            // A profile owns everything the workspace did not configure, but it
            // must not silently discard indentation the workspace DID
            // configure: `to_args` returns after emitting only `--profile` and
            // `extra_args`. perltidy lets command-line flags override profile
            // settings, so explicit indent/tabs are appended after the profile
            // and before `extra_args`, which stays last as the escape hatch.
            //
            // The editor `tabSize` / `insertSpaces` fallback is deliberately
            // NOT applied here: with a profile present, an unset field is the
            // profile's to decide, and injecting the editor's width would
            // override the profile's own indentation for workspaces that
            // configured nothing.
            if let Some(profile) = config.profile.as_ref() {
                args.push(format!("--profile={profile}"));
                if let Some(indent) = config.indent_columns {
                    args.push(format!("--indent-columns={indent}"));
                }
                if let Some(tabs) = config.tabs {
                    args.push(if tabs { "--tabs".to_string() } else { "--notabs".to_string() });
                }
                args.extend(config.extra_args.clone());
            } else {
                // Merge LSP options with config options.
                //
                // Explicitly configured indentation wins; the editor's
                // `tabSize` / `insertSpaces` are only a fallback for the parts
                // the configuration leaves unset. `to_args` emits
                // `--indent-columns` / `--tabs` / `--notabs` only for fields
                // that are actually set, so the fallbacks below are appended
                // last and apply exactly when nothing configured them.
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
        } else {
            // Fallback to LSP options only
            if options.insert_spaces {
                args.push(format!("-et={}", options.tab_size));
                args.push(format!("-i={}", options.tab_size));
            } else {
                args.push("-dt".to_string());
                args.push(format!("-i={}", options.tab_size));
            }
        }

        let perltidy_cmd = self.perltidy_path.as_deref().unwrap_or("perltidy");

        let output = self
            .runtime
            .run_command(
                perltidy_cmd,
                &args.iter().map(String::as_str).collect::<Vec<_>>(),
                Some(content.as_bytes()),
            )
            .map_err(|error| FormattingError::PerltidyNotFound(error.message))?;

        if !output.success() {
            return Err(FormattingError::PerltidyError(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

fn native_format_document(
    content: &str,
    options: &FormattingOptions,
    perltidy_config: Option<&PerlTidyConfig>,
) -> FormattedDocument {
    let config = native_format_config(options, perltidy_config, true);
    let result = NativeFormatter::new().format_document(content, &config);
    if result.diagnostics.is_empty() {
        let edits: Vec<_> = result.edits.into_iter().map(native_edit_to_format_edit).collect();
        // Log when the native formatter produces no edits on a non-trivial document,
        // so users understand why their code wasn't reformatted (#5054 item 7).
        // The native formatter supports only a subset of Perl (lexical/assignment/
        // call/if-else/for-sub); complex constructs pass through unchanged.
        if edits.is_empty() && content.trim().len() > 20 {
            tracing::debug!(
                len = content.len(),
                "native formatter produced no edits — document may contain unsupported constructs \
                 (print, grep/map blocks, ternaries, heredocs, regex); consider using external perltidy"
            );
        }
        let formatted = apply_lsp_whitespace_options(&result.formatted, options);
        if formatted != result.formatted {
            return FormattedDocument {
                text: formatted.clone(),
                edits: vec![FormatTextEdit {
                    range: FormatRange::whole_document(content),
                    new_text: formatted,
                }],
            };
        }

        return FormattedDocument { text: result.formatted, edits };
    }

    FormattedDocument { text: content.to_string(), edits: vec![] }
}

fn native_format_range(
    content: &str,
    range: &FormatRange,
    options: &FormattingOptions,
    perltidy_config: Option<&PerlTidyConfig>,
) -> FormattedDocument {
    let native_range = TextRange::new(
        TextPosition::new(range.start.line, range.start.character),
        TextPosition::new(range.end.line, range.end.character),
    );
    let result = NativeFormatter::new().format_range(
        content,
        native_range,
        &native_format_config(options, perltidy_config, false),
    );
    if result.diagnostics.is_empty() {
        if result.edits.is_empty() {
            return whitespace_range_fallback(content, range, options);
        }

        return FormattedDocument {
            text: result.formatted,
            edits: result.edits.into_iter().map(native_edit_to_format_edit).collect(),
        };
    }

    FormattedDocument { text: content.to_string(), edits: vec![] }
}

fn native_format_config(
    options: &FormattingOptions,
    perltidy_config: Option<&PerlTidyConfig>,
    allow_final_newline: bool,
) -> FormatConfig {
    let mut config = FormatConfig {
        indent_width: options.tab_size,
        use_tabs: !options.insert_spaces,
        final_newline: if allow_final_newline {
            if options.trim_final_newlines.unwrap_or(false) {
                FinalNewline::Trim
            } else if options.insert_final_newline.unwrap_or(false) {
                FinalNewline::Insert
            } else {
                FinalNewline::Preserve
            }
        } else {
            FinalNewline::Preserve
        },
        ..FormatConfig::default()
    };

    if let Some(perltidy_config) = perltidy_config {
        if let Some(width) = perltidy_config.maximum_line_length {
            config.line_width = width;
        }
        // Explicitly configured indentation wins over the editor's `tabSize` /
        // `insertSpaces`; the editor options above remain the fallback for an
        // unconfigured workspace.
        if let Some(indent_columns) = perltidy_config.indent_columns {
            config.indent_width = indent_columns;
        }
        if let Some(tabs) = perltidy_config.tabs {
            config.use_tabs = tabs;
        }
        if let Some(opening_brace_on_new_line) = perltidy_config.opening_brace_on_new_line {
            config.brace_placement = if opening_brace_on_new_line {
                BracePlacement::NextLine
            } else {
                BracePlacement::SameLine
            };
        }
        if let Some(cuddled_else) = perltidy_config.cuddled_else {
            config.else_placement =
                if cuddled_else { ElsePlacement::Cuddled } else { ElsePlacement::SeparateLine };
        }
        if let Some(space_after_keyword) = perltidy_config.space_after_keyword {
            config.keyword_spacing =
                if space_after_keyword { KeywordSpacing::Space } else { KeywordSpacing::Compact };
        }
        if let Some(add_trailing_commas) = perltidy_config.add_trailing_commas {
            config.trailing_comma = if add_trailing_commas {
                TrailingComma::AddWhenWrapped
            } else {
                TrailingComma::Preserve
            };
        }
    }

    config
}

fn native_edit_to_format_edit(edit: crate::tooling::perltidy::TextEdit) -> FormatTextEdit {
    FormatTextEdit {
        range: FormatRange::new(
            FormatPosition::new(edit.range.start.line, edit.range.start.character),
            FormatPosition::new(edit.range.end.line, edit.range.end.character),
        ),
        new_text: edit.new_text,
    }
}

fn whitespace_range_fallback(
    content: &str,
    range: &FormatRange,
    options: &FormattingOptions,
) -> FormattedDocument {
    let lines: Vec<&str> = content.lines().collect();
    let start_line = range.start.line as usize;
    let end_line = (range.end.line as usize).min(lines.len().saturating_sub(1));

    if start_line >= lines.len() || end_line < start_line {
        return FormattedDocument { text: content.to_string(), edits: vec![] };
    }

    // Detect line ending to preserve CRLF (#5075)
    let line_ending = if content.contains("\r\n") { "\r\n" } else { "\n" };
    let text_to_format = lines[start_line..=end_line].join(line_ending);
    let raw = apply_lsp_whitespace_options(&text_to_format, options);
    let formatted = raw.trim_end_matches(['\r', '\n']).to_string();
    if formatted == text_to_format {
        return FormattedDocument { text: content.to_string(), edits: vec![] };
    }

    FormattedDocument {
        text: content.to_string(),
        edits: vec![FormatTextEdit {
            range: FormatRange::new(
                FormatPosition::new(start_line as u32, 0),
                FormatPosition::new(end_line as u32, utf16_len(lines[end_line]) as u32),
            ),
            new_text: formatted,
        }],
    }
}

fn apply_lsp_whitespace_options(content: &str, options: &FormattingOptions) -> String {
    let mut output = content.to_string();

    if options.trim_trailing_whitespace.unwrap_or(false) {
        output = trim_trailing_whitespace(&output);
    }

    if options.trim_final_newlines.unwrap_or(false) {
        while output.ends_with('\n') {
            output.pop();
        }
    }

    if options.insert_final_newline.unwrap_or(false) && !output.ends_with('\n') {
        output.push('\n');
    }

    output
}

fn trim_trailing_whitespace(content: &str) -> String {
    let mut result = String::with_capacity(content.len());
    for line in content.split_inclusive('\n') {
        if let Some(without_nl) = line.strip_suffix('\n') {
            let trimmed = without_nl.trim_end_matches([' ', '\t']);
            result.push_str(trimmed);
            result.push('\n');
        } else {
            result.push_str(line.trim_end_matches([' ', '\t']));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use perl_subprocess_runtime::{SubprocessError, SubprocessOutput, SubprocessRuntime};

    struct MissingPerltidyRuntime;

    impl SubprocessRuntime for MissingPerltidyRuntime {
        fn run_command(
            &self,
            _program: &str,
            _args: &[&str],
            _stdin: Option<&[u8]>,
        ) -> std::result::Result<SubprocessOutput, SubprocessError> {
            Err(SubprocessError::new("perltidy missing"))
        }
    }

    struct FakePerltidyRuntime;

    impl SubprocessRuntime for FakePerltidyRuntime {
        fn run_command(
            &self,
            _program: &str,
            _args: &[&str],
            _stdin: Option<&[u8]>,
        ) -> std::result::Result<SubprocessOutput, SubprocessError> {
            Ok(SubprocessOutput {
                stdout: b"my $external = 1;\n".to_vec(),
                stderr: Vec::new(),
                status_code: 0,
            })
        }
    }

    /// A `perltidy` that is present and working (as if resolvable on PATH), and
    /// records whether it was ever invoked. Used to prove the default native path
    /// never shells out just because `perltidy` is available.
    struct RecordingPerltidyRuntime {
        invoked: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    impl SubprocessRuntime for RecordingPerltidyRuntime {
        fn run_command(
            &self,
            _program: &str,
            _args: &[&str],
            _stdin: Option<&[u8]>,
        ) -> std::result::Result<SubprocessOutput, SubprocessError> {
            self.invoked.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(SubprocessOutput {
                stdout: b"my $external = 1;\n".to_vec(),
                stderr: Vec::new(),
                status_code: 0,
            })
        }
    }

    #[test]
    fn format_document_uses_native_formatter_when_perltidy_missing() -> Result<()> {
        let provider = FormattingProvider::new(MissingPerltidyRuntime);
        let options = FormattingOptions {
            tab_size: 4,
            insert_spaces: true,
            trim_trailing_whitespace: None,
            insert_final_newline: None,
            trim_final_newlines: None,
        };

        let formatted = provider.format_document("my$x=1;\n", &options)?;
        assert_eq!(formatted.edits.len(), 1);
        assert_eq!(formatted.edits[0].new_text, "my $x = 1;\n");
        Ok(())
    }

    #[test]
    fn perltidy_available_does_not_change_default_native_formatting() -> Result<()> {
        // Behavioral counterpart to the config-level PATH guard
        // (`perltidy_discoverable_on_path_still_yields_native_default`): even when
        // `perltidy` is present and working, the DEFAULT (native) engine must
        // format natively AND must never shell out to perltidy. Merely having the
        // external tool available cannot flip formatting behavior.
        let invoked = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        // No `.with_formatter_mode(...)` — exercise the default engine.
        let provider =
            FormattingProvider::new(RecordingPerltidyRuntime { invoked: invoked.clone() });
        let options = FormattingOptions {
            tab_size: 4,
            insert_spaces: true,
            trim_trailing_whitespace: None,
            insert_final_newline: None,
            trim_final_newlines: None,
        };

        let formatted = provider.format_document("my$x=1;\n", &options)?;
        assert_eq!(formatted.edits.len(), 1);
        assert_eq!(
            formatted.edits[0].new_text, "my $x = 1;\n",
            "default engine must format natively even when perltidy is available"
        );
        assert!(
            !invoked.load(std::sync::atomic::Ordering::SeqCst),
            "the native default must not invoke perltidy"
        );
        Ok(())
    }

    #[test]
    fn format_document_native_applies_configured_formatting_policies() -> Result<()> {
        let config = PerlTidyConfig {
            maximum_line_length: Some(20),
            opening_brace_on_new_line: Some(true),
            cuddled_else: Some(false),
            space_after_keyword: Some(false),
            add_trailing_commas: Some(true),
            ..PerlTidyConfig::default()
        };
        let provider = FormattingProvider::new(MissingPerltidyRuntime)
            .with_perltidy_config(config)
            .with_formatter_mode(FormatterMode::Native);
        let options = FormattingOptions {
            tab_size: 4,
            insert_spaces: true,
            trim_trailing_whitespace: None,
            insert_final_newline: None,
            trim_final_newlines: None,
        };

        let formatted = provider.format_document(
            "if($ok){return foo($alpha,$beta,$gamma);}else{return bar();}\n",
            &options,
        )?;

        assert_eq!(formatted.edits.len(), 1);
        assert_eq!(
            formatted.edits[0].new_text,
            concat!(
                "if($ok)\n",
                "{\n",
                "    return foo(\n",
                "    $alpha,\n",
                "    $beta,\n",
                "    $gamma,\n",
                ");\n",
                "}\n",
                "else\n",
                "{\n",
                "    return bar();\n",
                "}\n",
            )
        );
        Ok(())
    }

    /// Records the argv handed to `perltidy` so the external path's indentation
    /// precedence can be asserted without an installed `perltidy`.
    struct ArgRecordingPerltidyRuntime {
        args: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    }

    impl SubprocessRuntime for ArgRecordingPerltidyRuntime {
        fn run_command(
            &self,
            _program: &str,
            args: &[&str],
            _stdin: Option<&[u8]>,
        ) -> std::result::Result<SubprocessOutput, SubprocessError> {
            if let Ok(mut recorded) = self.args.lock() {
                *recorded = args.iter().map(|arg| (*arg).to_string()).collect();
            }
            Ok(SubprocessOutput {
                stdout: b"my $external = 1;\n".to_vec(),
                stderr: Vec::new(),
                status_code: 0,
            })
        }
    }

    fn options_with_tab_size(tab_size: u32, insert_spaces: bool) -> FormattingOptions {
        FormattingOptions {
            tab_size,
            insert_spaces,
            trim_trailing_whitespace: None,
            insert_final_newline: None,
            trim_final_newlines: None,
        }
    }

    #[test]
    fn native_configured_indent_columns_win_over_editor_tab_size() {
        // A workspace that sets `perltidy_indent_columns = 2` must get 2 even
        // when the editor advertises tabSize 4.
        let config = PerlTidyConfig { indent_columns: Some(2), ..PerlTidyConfig::default() };
        let format_config =
            native_format_config(&options_with_tab_size(4, true), Some(&config), false);

        assert_eq!(format_config.indent_width, 2);
        assert!(!format_config.use_tabs);
    }

    #[test]
    fn native_configured_tabs_win_over_editor_insert_spaces() {
        let config = PerlTidyConfig { tabs: Some(true), ..PerlTidyConfig::default() };
        let format_config =
            native_format_config(&options_with_tab_size(4, true), Some(&config), false);

        assert!(format_config.use_tabs);
    }

    #[test]
    fn native_unconfigured_indentation_falls_back_to_editor_options() {
        // Nothing configured indentation, so the editor stays authoritative --
        // the fix must not pin unconfigured projects to a built-in width.
        let config = PerlTidyConfig::default();
        let format_config =
            native_format_config(&options_with_tab_size(2, false), Some(&config), false);

        assert_eq!(format_config.indent_width, 2);
        assert!(format_config.use_tabs);
    }

    #[test]
    fn native_configured_indent_columns_change_rendered_output() -> Result<()> {
        // End-to-end counterpart to the config-level assertions above: the
        // configured width must reach the emitted text, not just FormatConfig.
        let config = PerlTidyConfig { indent_columns: Some(2), ..PerlTidyConfig::default() };
        let provider = FormattingProvider::new(MissingPerltidyRuntime)
            .with_perltidy_config(config)
            .with_formatter_mode(FormatterMode::Native);

        let formatted =
            provider.format_document("if($ok){return 1;}\n", &options_with_tab_size(4, true))?;

        assert_eq!(formatted.edits.len(), 1);
        assert_eq!(
            formatted.edits[0].new_text, "if ($ok) {\n  return 1;\n}\n",
            "the configured 2-column indent must reach the emitted text, not the editor's 4"
        );
        Ok(())
    }

    #[test]
    fn external_configured_indent_columns_win_over_editor_tab_size() -> Result<()> {
        let recorded = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let config = PerlTidyConfig {
            indent_columns: Some(2),
            tabs: Some(false),
            ..PerlTidyConfig::default()
        };
        let provider =
            FormattingProvider::new(ArgRecordingPerltidyRuntime { args: recorded.clone() })
                .with_perltidy_config(config)
                .with_formatter_mode(FormatterMode::ExternalLegacy);

        provider.format_document("my $x = 1;\n", &options_with_tab_size(4, true))?;

        let args = recorded.lock().map_err(|_| anyhow::anyhow!("args mutex poisoned"))?.clone();
        assert!(args.contains(&"--indent-columns=2".to_string()), "{args:?}");
        assert!(args.contains(&"--notabs".to_string()), "{args:?}");
        assert!(
            !args.iter().any(|arg| arg.starts_with("-i=")),
            "the editor's tabSize must not override a configured indent width: {args:?}"
        );
        Ok(())
    }

    #[test]
    fn external_unconfigured_indentation_falls_back_to_editor_options() -> Result<()> {
        let recorded = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let provider =
            FormattingProvider::new(ArgRecordingPerltidyRuntime { args: recorded.clone() })
                .with_perltidy_config(PerlTidyConfig::default())
                .with_formatter_mode(FormatterMode::ExternalLegacy);

        provider.format_document("my $x = 1;\n", &options_with_tab_size(2, true))?;

        let args = recorded.lock().map_err(|_| anyhow::anyhow!("args mutex poisoned"))?.clone();
        assert!(args.contains(&"-i=2".to_string()), "{args:?}");
        assert!(args.contains(&"-et=2".to_string()), "{args:?}");
        assert!(!args.iter().any(|arg| arg.starts_with("--indent-columns=")), "{args:?}");
        Ok(())
    }

    #[test]
    fn external_profile_still_applies_configured_indent_columns() -> Result<()> {
        // A discovered `.perltidyrc` (or explicit perltidy_profile) sets
        // `profile`, which used to make `to_args` emit only `--profile` and
        // drop configured indentation entirely. perltidy lets command-line
        // flags override profile settings, so an explicitly configured width
        // must still reach the argv.
        let recorded = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let config = PerlTidyConfig {
            profile: Some("/workspace/.perltidyrc".to_string()),
            indent_columns: Some(2),
            tabs: Some(true),
            extra_args: vec!["--maximum-line-length=120".to_string()],
            ..PerlTidyConfig::default()
        };
        let provider =
            FormattingProvider::new(ArgRecordingPerltidyRuntime { args: recorded.clone() })
                .with_perltidy_config(config)
                .with_formatter_mode(FormatterMode::ExternalLegacy);

        provider.format_document("my $x = 1;\n", &options_with_tab_size(4, true))?;

        let args = recorded.lock().map_err(|_| anyhow::anyhow!("args mutex poisoned"))?.clone();
        assert!(args.contains(&"--profile=/workspace/.perltidyrc".to_string()), "{args:?}");
        assert!(args.contains(&"--indent-columns=2".to_string()), "{args:?}");
        assert!(args.contains(&"--tabs".to_string()), "{args:?}");

        // extra_args stays last so it remains the escape hatch that can
        // override the typed fields.
        let indent_at = args.iter().position(|a| a == "--indent-columns=2");
        let extra_at = args.iter().position(|a| a == "--maximum-line-length=120");
        assert!(indent_at < extra_at, "extra_args must stay last: {args:?}");
        Ok(())
    }

    #[test]
    fn external_profile_without_configured_indent_defers_to_the_profile() -> Result<()> {
        // Opposite-direction control for the decision above: when a profile is
        // present and the workspace configured no indentation, the editor's
        // tabSize must NOT be injected — that would override the profile's own
        // indentation for workspaces that configured nothing.
        let recorded = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let config = PerlTidyConfig {
            profile: Some("/workspace/.perltidyrc".to_string()),
            ..PerlTidyConfig::default()
        };
        let provider =
            FormattingProvider::new(ArgRecordingPerltidyRuntime { args: recorded.clone() })
                .with_perltidy_config(config)
                .with_formatter_mode(FormatterMode::ExternalLegacy);

        provider.format_document("my $x = 1;\n", &options_with_tab_size(2, true))?;

        let args = recorded.lock().map_err(|_| anyhow::anyhow!("args mutex poisoned"))?.clone();
        assert!(args.contains(&"--profile=/workspace/.perltidyrc".to_string()), "{args:?}");
        assert!(
            !args.iter().any(|a| a.starts_with("-i=") || a.starts_with("--indent-columns=")),
            "the editor's tabSize must not override the profile: {args:?}"
        );
        assert!(
            !args.iter().any(|a| a.starts_with("-et=") || a == "-dt"),
            "the editor's insertSpaces must not override the profile: {args:?}"
        );
        Ok(())
    }

    #[test]
    fn format_range_native_applies_configured_formatting_policies() -> Result<()> {
        let config = PerlTidyConfig {
            opening_brace_on_new_line: Some(true),
            cuddled_else: Some(false),
            space_after_keyword: Some(false),
            ..PerlTidyConfig::default()
        };
        let provider = FormattingProvider::new(MissingPerltidyRuntime)
            .with_perltidy_config(config)
            .with_formatter_mode(FormatterMode::Native);
        let options = FormattingOptions {
            tab_size: 4,
            insert_spaces: true,
            trim_trailing_whitespace: None,
            insert_final_newline: None,
            trim_final_newlines: None,
        };
        let source = "my $prefix = 1;\nif($ok){return 1;}else{return 0;}\nmy $suffix = 1;\n";
        let range = FormatRange::new(FormatPosition::new(1, 0), FormatPosition::new(1, 34));

        let formatted = provider.format_range(source, &range, &options)?;

        assert_eq!(formatted.edits.len(), 1);
        assert_eq!(
            formatted.edits[0].new_text,
            concat!(
                "if($ok)\n",
                "{\n",
                "    return 1;\n",
                "}\n",
                "else\n",
                "{\n",
                "    return 0;\n",
                "}"
            )
        );
        Ok(())
    }

    #[test]
    fn format_document_external_legacy_uses_perltidy_adapter() -> Result<()> {
        let provider = FormattingProvider::new(FakePerltidyRuntime)
            .with_formatter_mode(FormatterMode::ExternalLegacy);
        let options = FormattingOptions {
            tab_size: 4,
            insert_spaces: true,
            trim_trailing_whitespace: None,
            insert_final_newline: None,
            trim_final_newlines: None,
        };

        let formatted = provider.format_document("my$x=1;\n", &options)?;
        assert_eq!(formatted.edits.len(), 1);
        assert_eq!(formatted.edits[0].new_text, "my $external = 1;\n");
        Ok(())
    }

    #[test]
    fn format_document_external_legacy_reports_missing_perltidy() -> Result<()> {
        let provider = FormattingProvider::new(MissingPerltidyRuntime)
            .with_formatter_mode(FormatterMode::ExternalLegacy);
        let options = FormattingOptions {
            tab_size: 4,
            insert_spaces: true,
            trim_trailing_whitespace: None,
            insert_final_newline: None,
            trim_final_newlines: None,
        };

        let error = provider.format_document("my$x=1;\n", &options).err().ok_or_else(|| {
            anyhow::anyhow!("explicit external legacy mode must report missing perltidy")
        })?;
        assert_eq!(error.error_kind(), "perltidy_not_found");
        Ok(())
    }

    #[test]
    fn format_document_returns_empty_edits_when_native_formatter_has_no_changes() -> Result<()> {
        let provider = FormattingProvider::new(MissingPerltidyRuntime);
        let options = FormattingOptions {
            tab_size: 4,
            insert_spaces: true,
            trim_trailing_whitespace: None,
            insert_final_newline: None,
            trim_final_newlines: None,
        };

        let result = provider.format_document("my $x = 1;\n", &options)?;
        assert!(result.edits.is_empty());
        assert_eq!(result.text, "my $x = 1;\n");
        Ok(())
    }

    #[test]
    fn format_document_returns_empty_edits_when_native_reports_literal_preserve() -> Result<()> {
        let provider = FormattingProvider::new(MissingPerltidyRuntime);
        let options = FormattingOptions {
            tab_size: 4,
            insert_spaces: true,
            trim_trailing_whitespace: Some(true),
            insert_final_newline: Some(false),
            trim_final_newlines: None,
        };

        let formatted = provider.format_document("=pod\n\n=cut\n\nmy $x = 1;   \n", &options)?;
        assert!(formatted.edits.is_empty());
        assert_eq!(formatted.text, "=pod\n\n=cut\n\nmy $x = 1;   \n");
        Ok(())
    }

    #[test]
    fn format_range_returns_empty_edits_when_native_reports_literal_preserve() -> Result<()> {
        let provider = FormattingProvider::new(MissingPerltidyRuntime);
        let options = FormattingOptions {
            tab_size: 4,
            insert_spaces: true,
            trim_trailing_whitespace: Some(true),
            insert_final_newline: Some(false),
            trim_final_newlines: None,
        };
        let range = FormatRange::new(FormatPosition::new(0, 0), FormatPosition::new(0, 31));

        let formatted =
            provider.format_range("my $matched = $text =~ /needle/i;   \n", &range, &options)?;

        assert!(formatted.edits.is_empty());
        Ok(())
    }

    #[test]
    fn format_range_uses_native_formatter_when_perltidy_missing() -> Result<()> {
        let provider = FormattingProvider::new(MissingPerltidyRuntime);
        let options = FormattingOptions {
            tab_size: 4,
            insert_spaces: true,
            trim_trailing_whitespace: Some(true),
            insert_final_newline: Some(false),
            trim_final_newlines: Some(false),
        };
        let range = FormatRange::new(FormatPosition::new(1, 0), FormatPosition::new(1, 10));

        let formatted = provider.format_range(
            "line1
my$x=1;
line3
",
            &range,
            &options,
        )?;

        assert_eq!(formatted.edits.len(), 1);
        assert_eq!(formatted.edits[0].new_text, "my $x = 1;");
        Ok(())
    }

    #[test]
    fn format_range_fallback_does_not_inject_newline_when_insert_final_newline_set() -> Result<()> {
        // Regression: apply_lsp_whitespace_options appends '\n' when insert_final_newline
        // is true. For a range edit, new_text must not have a trailing newline because the
        // replacement range already sits between existing document newlines.
        let provider = FormattingProvider::new(MissingPerltidyRuntime);
        let options = FormattingOptions {
            tab_size: 4,
            insert_spaces: true,
            trim_trailing_whitespace: Some(true),
            insert_final_newline: Some(true), // would normally append \n to fragment
            trim_final_newlines: None,
        };
        let range = FormatRange::new(FormatPosition::new(1, 0), FormatPosition::new(1, 13));

        let formatted = provider.format_range("line1\nmy $x = 1;   \nline3\n", &range, &options)?;

        assert_eq!(formatted.edits.len(), 1);
        // new_text must NOT end with '\n' — that would insert a spurious blank line
        let new_text = &formatted.edits[0].new_text;
        assert_eq!(new_text, "my $x = 1;");
        assert!(!new_text.ends_with('\n'), "range edit new_text must not end with '\\n'");
        Ok(())
    }

    #[test]
    fn format_range_returns_empty_edits_when_native_formatter_has_no_changes() -> Result<()> {
        let provider = FormattingProvider::new(MissingPerltidyRuntime);
        let options = FormattingOptions {
            tab_size: 4,
            insert_spaces: true,
            trim_trailing_whitespace: None,
            insert_final_newline: None,
            trim_final_newlines: None,
        };
        let range = FormatRange::new(FormatPosition::new(0, 0), FormatPosition::new(0, 10));

        let result = provider.format_range(
            "my $x = 1;
",
            &range,
            &options,
        )?;
        assert!(result.edits.is_empty());
        Ok(())
    }

    #[test]
    fn apply_lsp_whitespace_options_trim_final_newlines_removes_all_trailing_newlines() {
        // Regression: previous implementation used `ends_with("\n\n")` which left
        // one trailing newline. LSP trimFinalNewlines must remove ALL trailing newlines.
        let options = FormattingOptions {
            tab_size: 4,
            insert_spaces: true,
            trim_trailing_whitespace: None,
            insert_final_newline: None,
            trim_final_newlines: Some(true),
        };
        let r = apply_lsp_whitespace_options("content\n", &options);
        assert_eq!(r, "content");
        let r = apply_lsp_whitespace_options("content\n\n", &options);
        assert_eq!(r, "content");
        let r = apply_lsp_whitespace_options("content\n\n\n", &options);
        assert_eq!(r, "content");
    }
}
