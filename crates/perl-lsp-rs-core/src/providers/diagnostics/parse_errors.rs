//! Parse error to diagnostic conversion
//!
//! This module provides functionality for converting parser errors into diagnostic messages.
//!
//! # Diagnostic codes
//!
//! | Code | Severity | Description |
//! |------|----------|-------------|
//! | `syntax-error` | Error | Generic syntax error from the parser |

use perl_diagnostics::codes::{DiagnosticCode, DiagnosticSeverity};
use perl_parser_core::{ParseError, ResolvedParseDiagnosticAnchor};

/// Downstream publication disposition for a parser-owned diagnostic anchor.
///
/// LSP consumers publish only [`Self::Publish`]. Every other parser disposition
/// is retained as a typed suppression reason instead of being converted to byte
/// zero or clamped into a plausible source range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ParseDiagnosticLocationDisposition {
    /// Publish the diagnostic at this validated UTF-8 byte offset.
    Publish(usize),
    /// Suppress publication because the parser could not establish a valid
    /// anchor for the same concrete source subject.
    Suppress(ResolvedParseDiagnosticAnchor),
}

/// Resolve one parser diagnostic for the exact source snapshot being published.
///
/// `parsed_source` is the immutable source snapshot that produced `error`;
/// `current_source` is the document text the consumer is about to expose to the
/// client. The parser-owned contract validates bounds, UTF-8 boundaries, and
/// source identity. Consumers must not invent a fallback offset for a
/// [`ParseDiagnosticLocationDisposition::Suppress`] result.
#[must_use]
pub fn parse_diagnostic_location(
    error: &ParseError,
    parsed_source: &str,
    current_source: &str,
) -> ParseDiagnosticLocationDisposition {
    let resolved = error.resolved_diagnostic_anchor_for_current(parsed_source, current_source);
    match resolved {
        ResolvedParseDiagnosticAnchor::Exact(offset)
        | ResolvedParseDiagnosticAnchor::EndOfInput(offset) => {
            ParseDiagnosticLocationDisposition::Publish(offset)
        }
        _ => ParseDiagnosticLocationDisposition::Suppress(resolved),
    }
}

/// Derive the canonical diagnostic code for a parser error.
pub fn parse_error_code(error: &ParseError) -> DiagnosticCode {
    match error {
        ParseError::UnexpectedEof => DiagnosticCode::UnexpectedEof,
        ParseError::SyntaxError { message, .. } => {
            DiagnosticCode::from_message(message).unwrap_or(DiagnosticCode::SyntaxError)
        }
        ParseError::Advisory { message, .. } => {
            DiagnosticCode::from_message(message).unwrap_or(DiagnosticCode::ParseError)
        }
        _ => DiagnosticCode::ParseError,
    }
}

// A dead `parse_error_to_diagnostic` used to live here: a never-called second
// `ParseError` → `Diagnostic` mapping with its own location match, its own
// suggestion table, and the same `_ => 0` catch-all that pinned `Recovered`
// errors to line 1 column 1. It was removed rather than wired up — it carried
// the defect it looked like a fix for. The live runtime now consumes
// `parse_diagnostic_location`, while hints remain owned by
// `build_parse_error_hint`.

/// Derive the user-facing severity for a parser error.
pub fn parse_error_severity(error: &ParseError) -> DiagnosticSeverity {
    if !error.blocks_clean_parse() {
        return DiagnosticSeverity::Warning;
    }

    if matches!(error, ParseError::SyntaxError { .. })
        && (matches!(parse_error_code(error), DiagnosticCode::InvalidPrototype)
            || matches!(
                error,
                ParseError::SyntaxError { message, .. }
                    if is_unknown_subroutine_attribute_warning(message)
            ))
    {
        return DiagnosticSeverity::Warning;
    }

    DiagnosticSeverity::Error
}

fn is_unknown_subroutine_attribute_warning(message: &str) -> bool {
    message.starts_with("unknown subroutine attribute ':")
}

#[cfg(test)]
mod tests {
    use perl_diagnostics::codes::{DiagnosticCode, DiagnosticSeverity};
    use perl_parser_core::{ParseError, ResolvedParseDiagnosticAnchor};

    use super::{
        ParseDiagnosticLocationDisposition, parse_diagnostic_location, parse_error_code,
        parse_error_severity,
    };

    #[test]
    fn advisory_uses_a_non_syntax_error_code() {
        let advisory = ParseError::nested_quantifier_advisory(12);

        assert_eq!(parse_error_code(&advisory), DiagnosticCode::ParseError);
    }

    #[test]
    fn nested_quantifier_advisory_remains_an_lsp_warning() {
        let advisory = ParseError::nested_quantifier_advisory(12);

        assert_eq!(
            parse_error_severity(&advisory),
            DiagnosticSeverity::Warning,
            "valid nested quantifiers must remain visible without becoming errors"
        );
    }

    #[test]
    fn blocking_syntax_error_remains_an_lsp_error() {
        let syntax_error = ParseError::syntax("expected expression", 12);

        assert_eq!(
            parse_error_severity(&syntax_error),
            DiagnosticSeverity::Error,
            "malformed syntax must remain parse-blocking"
        );
    }

    #[test]
    fn valid_exact_and_eof_anchors_publish() {
        assert_eq!(
            parse_diagnostic_location(&ParseError::syntax("bad", 1), "abc", "abc"),
            ParseDiagnosticLocationDisposition::Publish(1)
        );
        assert_eq!(
            parse_diagnostic_location(&ParseError::UnexpectedEof, "abc", "abc"),
            ParseDiagnosticLocationDisposition::Publish(3)
        );
    }

    #[test]
    fn no_source_and_invalid_utf8_are_typed_suppressions() {
        assert_eq!(
            parse_diagnostic_location(
                &ParseError::LexerError { message: "bad byte".into() },
                "aéz",
                "aéz",
            ),
            ParseDiagnosticLocationDisposition::Suppress(
                ResolvedParseDiagnosticAnchor::NoSource
            )
        );
        assert_eq!(
            parse_diagnostic_location(&ParseError::syntax("inside code point", 2), "aéz", "aéz"),
            ParseDiagnosticLocationDisposition::Suppress(
                ResolvedParseDiagnosticAnchor::InvalidUtf8Boundary {
                    reported: 2,
                    source_len: 4,
                }
            )
        );
    }

    #[test]
    fn changed_same_length_source_is_not_publishable() {
        assert_eq!(
            parse_diagnostic_location(&ParseError::syntax("bad", 1), "abc", "axc"),
            ParseDiagnosticLocationDisposition::Suppress(
                ResolvedParseDiagnosticAnchor::StaleSource {
                    parsed_len: 3,
                    current_len: 3,
                }
            )
        );
    }
}
