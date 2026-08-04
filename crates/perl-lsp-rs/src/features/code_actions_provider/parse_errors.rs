pub(super) fn parse_error_fix_code_from_message(message: &str) -> Option<&'static str> {
    let message_lower = message.to_ascii_lowercase();
    if message_lower.contains("missing semicolon") {
        return Some("parse-error-missingsemicolon");
    }
    if message_lower.contains("unclosed string") || message_lower.contains("unterminated string") {
        return Some("parse-error-unclosedstring");
    }
    if message_lower.contains("unclosed parenthesis") {
        return Some("parse-error-unclosedparen");
    }
    if message_lower.contains("unclosed brace")
        || message_lower.contains("unclosed block")
        || message_lower.contains("missing '}'")
        || message_lower.contains("unclosed `{`")
    {
        return Some("parse-error-unclosedbrace");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::parse_error_fix_code_from_message;

    #[test]
    fn matcher_resolves_real_parser_unclosed_block_message() {
        // #5547: the parser emits "Unclosed block: expected '}' but reached end
        // of input" (statements.rs), which the old matcher (unclosed brace /
        // missing '}' / unclosed `{`) never matched, leaving the "Add closing
        // brace" quick fix unreachable for the one diagnostic it repairs.
        assert_eq!(
            parse_error_fix_code_from_message(
                "Unclosed block: expected '}' but reached end of input"
            ),
            Some("parse-error-unclosedbrace")
        );
    }

    #[test]
    fn matcher_resolves_classifier_unclosed_block_message() {
        // The classifier emits "Unclosed code block - missing '}'"
        // (classifier.rs ParseErrorKind::UnclosedBlock).
        assert_eq!(
            parse_error_fix_code_from_message("Unclosed code block - missing '}'"),
            Some("parse-error-unclosedbrace")
        );
    }

    #[test]
    fn matcher_still_resolves_unclosed_brace_and_paren_variants() {
        // Guard against regressions on the existing arms.
        assert_eq!(
            parse_error_fix_code_from_message("Unclosed brace - missing '}'"),
            Some("parse-error-unclosedbrace")
        );
        assert_eq!(
            parse_error_fix_code_from_message("Unclosed parenthesis - missing ')'"),
            Some("parse-error-unclosedparen")
        );
        assert_eq!(
            parse_error_fix_code_from_message("Unclosed string literal"),
            Some("parse-error-unclosedstring")
        );
        assert_eq!(
            parse_error_fix_code_from_message("Missing semicolon"),
            Some("parse-error-missingsemicolon")
        );
        assert_eq!(parse_error_fix_code_from_message("something unrelated"), None);
    }
}
