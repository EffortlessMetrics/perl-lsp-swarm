//! Typed machine inputs for parse-error quick fixes (#8396).

use perl_parser_core::error::ParseError;

/// Stable parse-error fix identity carried on diagnostics and quick-fix metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseErrorFixKind {
    /// Statement missing a terminating semicolon.
    MissingSemicolon,
}

/// Legacy subtype code used before structured metadata was authoritative.
pub const LEGACY_MISSING_SEMICOLON_CODE: &str = "parse-error-missingsemicolon";

/// Resolve a fix kind from a legacy `parse-error-*` diagnostic code.
#[must_use]
pub fn parse_error_fix_kind_from_legacy_code(code: &str) -> Option<ParseErrorFixKind> {
    match code {
        LEGACY_MISSING_SEMICOLON_CODE => Some(ParseErrorFixKind::MissingSemicolon),
        _ => None,
    }
}

/// Derive fix kind from parser-owned `ParseError` variants at diagnostic publication.
#[must_use]
pub fn parse_error_fix_kind_from_parse_error(error: &ParseError) -> Option<ParseErrorFixKind> {
    match error {
        ParseError::UnexpectedToken { expected, .. } => {
            let expected_lower = expected.to_ascii_lowercase();
            if expected.contains(';') || expected_lower.contains("semicolon") {
                Some(ParseErrorFixKind::MissingSemicolon)
            } else {
                None
            }
        }
        ParseError::SyntaxError { message, .. } | ParseError::Advisory { message, .. } => {
            parse_error_fix_kind_from_message_compat(message)
        }
        _ => None,
    }
}

/// Compatibility: clients or stale push payloads may supply only prose (#8396).
///
/// Owner: `area:lsp` code-actions. Exit when push/pull routes always attach
/// `parse_error_fix_kind` from parser output for governed families.
#[must_use]
pub fn parse_error_fix_kind_from_message_compat(message: &str) -> Option<ParseErrorFixKind> {
    if message.to_ascii_lowercase().contains("missing semicolon") {
        Some(ParseErrorFixKind::MissingSemicolon)
    } else {
        None
    }
}

/// Merge stored machine context, legacy codes, and bounded compatibility prose.
#[must_use]
pub fn parse_error_fix_kind_for_diagnostic(
    code: Option<&str>,
    message: &str,
    stored: Option<ParseErrorFixKind>,
) -> Option<ParseErrorFixKind> {
    if let Some(kind) = stored {
        return Some(kind);
    }
    if let Some(code) = code
        && let Some(kind) = parse_error_fix_kind_from_legacy_code(code)
    {
        return Some(kind);
    }
    if let Some(code) = code
        && matches!(code, "PL001" | "PL002" | "PL003")
    {
        return parse_error_fix_kind_from_message_compat(message);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_parser_core::error::ParseError;

    #[test]
    fn parse_error_token_expecting_semicolon_yields_missing_semicolon_kind() {
        let error = ParseError::UnexpectedToken {
            expected: ";".to_string(),
            found: "my".to_string(),
            location: 4,
        };
        assert_eq!(
            parse_error_fix_kind_from_parse_error(&error),
            Some(ParseErrorFixKind::MissingSemicolon)
        );
    }

    #[test]
    fn stored_kind_wins_over_reworded_prose() {
        assert_eq!(
            parse_error_fix_kind_for_diagnostic(
                Some("PL001"),
                "totally unrelated presentation",
                Some(ParseErrorFixKind::MissingSemicolon),
            ),
            Some(ParseErrorFixKind::MissingSemicolon)
        );
    }

    #[test]
    fn near_miss_prose_without_stored_kind_or_parse_code_is_not_authoritative() {
        assert_eq!(
            parse_error_fix_kind_for_diagnostic(
                Some("PL109"),
                "Missing semicolon after statement",
                None,
            ),
            None
        );
    }

    #[test]
    fn compat_prose_applies_only_to_governed_parse_codes() {
        assert_eq!(
            parse_error_fix_kind_for_diagnostic(
                Some("PL001"),
                "Missing semicolon after statement",
                None,
            ),
            Some(ParseErrorFixKind::MissingSemicolon)
        );
    }
}
