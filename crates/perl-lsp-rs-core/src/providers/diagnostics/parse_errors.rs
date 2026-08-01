//! Parse error to diagnostic conversion
//!
//! This module provides functionality for converting parser errors into diagnostic messages.
//!
//! # Diagnostic codes
//!
//! | Code | Severity | Description |
//! |------|----------|-------------|
//! | `syntax-error` | Error | Generic syntax error from the parser |

use perl_diagnostics::codes::DiagnosticCode;
use perl_parser_core::error::ParseError;

use perl_diagnostics::codes::DiagnosticSeverity;

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
// the defect it looked like a fix for, and the live mapping in `diagnostics.rs`
// (positions from `ParseError::location`, hints from `build_parse_error_hint`)
// is the single authority. Recover it from git history if a second entry point
// is ever genuinely needed.

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
    use perl_parser_core::error::ParseError;

    use super::{parse_error_code, parse_error_severity};

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
}
