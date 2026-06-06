//! Native formatter contract types.
//!
//! This module defines the Rust-native formatter API and default formatter
//! implementation. It intentionally lives beside the subprocess-backed
//! `PerlTidyFormatter` adapter so consumers can keep an explicit legacy
//! compatibility path while the LSP runtime uses native formatting by default.

mod block;
mod config;
mod doc;
mod line;
mod preserve;
mod result;
mod statement;

pub use config::{
    BracePlacement, ElsePlacement, FinalNewline, FormatConfig, FormatterMode, KeywordSpacing,
    TrailingComma,
};
pub use doc::FormatDoc;
pub use result::{
    FormatDiagnostic, FormatDiagnosticSeverity, FormatResult, TextEdit, TextPosition, TextRange,
};

use line::{format_simple_line, range_includes_line, split_line_ending};
use preserve::literal_preserve_region;
use result::utf16_len;

const PARSE_ERROR_CODE: &str = "native.format.parse_error";
const PARSE_PRESERVATION_CODE: &str = "native.format.parse_preservation";
const LITERAL_PRESERVE_CODE: &str = "native.format.literal_preserve_region";

/// Native Perl formatter interface.
pub trait PerlFormatter {
    /// Format a complete source document.
    fn format_document(&self, source: &str, config: &FormatConfig) -> FormatResult;

    /// Format a source range.
    fn format_range(&self, source: &str, range: TextRange, config: &FormatConfig) -> FormatResult;
}

/// Parse-gated Rust-native Perl formatter.
///
/// This initial engine performs only deliberately small syntax layout rewrites
/// and is the safety boundary that future native formatter passes should compose
/// with: source and formatted output must both parse cleanly before any native
/// formatting edit is returned.
#[derive(Debug, Default, Clone, Copy)]
pub struct NativeFormatter;

impl NativeFormatter {
    /// Create a parse-gated native formatter.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    fn validate_clean_parse(source: &str) -> Result<(), FormatDiagnostic> {
        if let Some(kind) = literal_preserve_region(source) {
            return Err(FormatDiagnostic::new(
                LITERAL_PRESERVE_CODE,
                FormatDiagnosticSeverity::Warning,
                None,
                format!("native formatting skipped because {kind} preservation is not enabled yet"),
            ));
        }

        let mut parser = perl_parser_core::Parser::new(source);
        let output = parser.parse_with_recovery();

        if output.terminated_early {
            return Err(FormatDiagnostic::new(
                PARSE_ERROR_CODE,
                FormatDiagnosticSeverity::Warning,
                None,
                "native formatting skipped because parsing terminated early",
            ));
        }

        if let Some(error) = output.diagnostics.first() {
            return Err(FormatDiagnostic::new(
                PARSE_ERROR_CODE,
                FormatDiagnosticSeverity::Warning,
                error.location().map(|offset| TextRange::at_byte_offset(source, offset)),
                format!(
                    "native formatting skipped because the source does not parse cleanly: {error}"
                ),
            ));
        }

        Ok(())
    }

    fn format_safe_subset(source: &str, config: &FormatConfig) -> String {
        let mut formatted = String::with_capacity(source.len());

        for line in source.split_inclusive('\n') {
            let (body, line_ending) = split_line_ending(line);
            formatted
                .push_str(&format_simple_line(body, config).unwrap_or_else(|| body.to_string()));
            formatted.push_str(line_ending);
        }

        formatted
    }

    fn format_safe_subset_range(
        source: &str,
        range: TextRange,
        config: &FormatConfig,
    ) -> (String, Vec<TextEdit>) {
        let mut formatted = String::with_capacity(source.len());
        let mut edits = Vec::new();

        for (line_index, line) in source.split_inclusive('\n').enumerate() {
            let line_index = line_index as u32;
            let (body, line_ending) = split_line_ending(line);
            let formatted_body = if range_includes_line(range, line_index) {
                format_simple_line(body, config)
            } else {
                None
            };

            if let Some(formatted_line) = formatted_body {
                if formatted_line != body {
                    edits.push(TextEdit::new(
                        TextRange::new(
                            TextPosition::new(line_index, 0),
                            TextPosition::new(line_index, utf16_len(body) as u32),
                        ),
                        formatted_line.clone(),
                    ));
                    formatted.push_str(&formatted_line);
                } else {
                    formatted.push_str(body);
                }
            } else {
                formatted.push_str(body);
            }
            formatted.push_str(line_ending);
        }

        (formatted, edits)
    }

    fn apply_final_newline(source: &str, config: &FormatConfig) -> String {
        match config.final_newline {
            FinalNewline::Preserve => source.to_string(),
            FinalNewline::Insert => {
                let trimmed = source.trim_end_matches(['\n', '\r']);
                format!("{trimmed}\n")
            }
            FinalNewline::Trim => source.trim_end_matches(['\n', '\r']).to_string(),
        }
    }
}

impl PerlFormatter for NativeFormatter {
    fn format_document(&self, source: &str, config: &FormatConfig) -> FormatResult {
        if matches!(config.mode, FormatterMode::Off) {
            return FormatResult::unchanged(source);
        }

        if let Err(diagnostic) = Self::validate_clean_parse(source) {
            let mut result = FormatResult::unchanged(source);
            result.diagnostics.push(diagnostic);
            return result;
        }

        let formatted =
            Self::apply_final_newline(&Self::format_safe_subset(source, config), config);
        if let Err(diagnostic) = Self::validate_clean_parse(&formatted) {
            let mut result = FormatResult::unchanged(source);
            result.diagnostics.push(FormatDiagnostic::new(
                PARSE_PRESERVATION_CODE,
                FormatDiagnosticSeverity::Warning,
                diagnostic.range,
                "native formatting skipped because formatted output did not parse cleanly",
            ));
            return result;
        }

        FormatResult::replace_document(source, formatted)
    }

    fn format_range(&self, source: &str, range: TextRange, config: &FormatConfig) -> FormatResult {
        if matches!(config.mode, FormatterMode::Off) {
            return FormatResult::unchanged(source);
        }

        if let Err(diagnostic) = Self::validate_clean_parse(source) {
            let mut result = FormatResult::unchanged(source);
            result.diagnostics.push(diagnostic);
            return result;
        }

        let (formatted, edits) = Self::format_safe_subset_range(source, range, config);
        if let Err(diagnostic) = Self::validate_clean_parse(&formatted) {
            let mut result = FormatResult::unchanged(source);
            result.diagnostics.push(FormatDiagnostic::new(
                PARSE_PRESERVATION_CODE,
                FormatDiagnosticSeverity::Warning,
                diagnostic.range,
                "native range formatting skipped because formatted output did not parse cleanly",
            ));
            return result;
        }

        FormatResult { formatted, changed: !edits.is_empty(), edits, diagnostics: Vec::new() }
    }
}

#[cfg(test)]
mod tests {
    use super::line::split_trailing_comment;
    use super::{
        TextPosition, TextRange, literal_preserve_region, range_includes_line, split_line_ending,
    };

    #[test]
    fn split_trailing_comment_ignores_hash_inside_backticks()
    -> Result<(), Box<dyn std::error::Error>> {
        let (code, comment) = split_trailing_comment("my$out=`printf '#value'`; # trailing");
        assert_eq!(code, "my$out=`printf '#value'`;");
        assert_eq!(comment, Some("# trailing"));

        let (code, comment) = split_trailing_comment("my$out=`printf '#value'`;");
        assert_eq!(code, "my$out=`printf '#value'`;");
        assert_eq!(comment, None);

        Ok(())
    }

    #[test]
    fn split_line_ending_preserves_crlf_lf_and_unterminated_lines()
    -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(split_line_ending("my $x = 1;\r\n"), ("my $x = 1;", "\r\n"));
        assert_eq!(split_line_ending("my $x = 1;\n"), ("my $x = 1;", "\n"));
        assert_eq!(split_line_ending("my $x = 1;"), ("my $x = 1;", ""));

        Ok(())
    }

    #[test]
    fn range_includes_line_treats_zero_width_end_line_as_exclusive()
    -> Result<(), Box<dyn std::error::Error>> {
        let range = TextRange::new(TextPosition::new(1, 0), TextPosition::new(3, 0));
        assert!(!range_includes_line(range, 0));
        assert!(range_includes_line(range, 1));
        assert!(range_includes_line(range, 2));
        assert!(!range_includes_line(range, 3));

        let range = TextRange::new(TextPosition::new(1, 0), TextPosition::new(3, 4));
        assert!(range_includes_line(range, 3));

        Ok(())
    }

    #[test]
    fn literal_preserve_region_detects_perl_constructs_that_must_not_be_reflowed()
    -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(literal_preserve_region("=head1 NAME\nDemo\n=cut\n"), Some("POD"));
        assert_eq!(
            literal_preserve_region("my $x = 1;\n__DATA__\nraw\n"),
            Some("DATA/END section")
        );
        assert_eq!(literal_preserve_region("my $text = <<~'EOF';\nbody\nEOF\n"), Some("heredoc"));
        assert_eq!(literal_preserve_region("format STDOUT =\n@<<<<\n$x\n.\n"), Some("format body"));
        assert_eq!(literal_preserve_region("my $x = 1;\n"), None);

        Ok(())
    }
}
