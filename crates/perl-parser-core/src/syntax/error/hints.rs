//! Actionable parse-error hints shared by `--check`, LSP, and other consumers (#16610).

use super::ParseError;

/// Build an actionable hint for a parse error without source context.
#[must_use]
pub fn build_parse_error_hint(error: &ParseError, base_message: &str) -> Option<String> {
    build_parse_error_hint_with_source(error, base_message, None)
}

/// Build an actionable hint, optionally using source text for delimiter-aware messages.
#[must_use]
pub fn build_parse_error_hint_with_source(
    error: &ParseError,
    base_message: &str,
    source: Option<&str>,
) -> Option<String> {
    match error {
        ParseError::UnexpectedToken { expected, found, location, .. } => {
            if expected.contains(';') || expected.contains("semicolon") {
                return Some("Missing semicolon after statement. Add `;` here.".to_string());
            }
            if found == ";" {
                return Some(format!(
                    "A {expected} is required here -- the statement appears incomplete"
                ));
            }
            if found == "}" || found == ")" || found == "]" {
                return Some(format!("Check for a missing {expected} before '{found}'"));
            }
            if expected.contains('{') || expected.contains("block") {
                return Some(format!(
                    "Add an opening '{{' to start the block (found {found})"
                ));
            }
            if expected.contains(')') {
                return Some(
                    "Add a closing ')' -- there may be an unmatched opening '('".to_string(),
                );
            }
            if expected.contains(']') {
                return Some(
                    "Add a closing ']' -- there may be an unmatched opening '['".to_string(),
                );
            }
            if expected.to_lowercase().contains("variable") {
                return Some(
                    "Expected a variable like `$foo`, `@bar`, or `%hash` after the declaration keyword"
                        .to_string(),
                );
            }
            if expected.contains(',') || expected.to_lowercase().contains("comma") {
                return Some(
                    "Expected `,` between list elements -- check for a missing comma".to_string(),
                );
            }
            if expected.contains("expression") && found.contains("=>") {
                return Some(
                    "'=>' (fat arrow) is not valid here; \
                     did you forget a value before it?"
                        .to_string(),
                );
            }
            if expected.contains("expression") && found.contains("->") {
                return Some(
                    "'->' (arrow) is not valid here; \
                     did you forget the object or reference before it?"
                        .to_string(),
                );
            }
            if found.contains("unknown token") {
                if let Some(source) = source
                    && let Some(regex_hint) = unterminated_regex_delimiter_hint(source, *location)
                {
                    return Some(regex_hint);
                }
                return Some(
                    "Check for an unclosed string, regex, or heredoc near this position"
                        .to_string(),
                );
            }
            None
        }
        ParseError::UnexpectedEof => Some(
            "The file ended unexpectedly -- check for unclosed delimiters or missing semicolons"
                .to_string(),
        ),
        ParseError::UnclosedDelimiter { delimiter } => {
            Some(format!("Add a matching closing '{delimiter}' to complete the literal"))
        }
        ParseError::SyntaxError { message, .. } | ParseError::Advisory { message, .. } => {
            let msg_lower = message.to_lowercase();
            let base_lower = base_message.to_lowercase();
            if msg_lower.contains("semicolon") || msg_lower.contains("missing ;") {
                Some("Add a ';' at the end of the statement".to_string())
            } else if msg_lower.contains("heredoc") || base_lower.contains("heredoc") {
                Some(
                    "Check that the heredoc terminator appears on its own line with no extra whitespace"
                        .to_string(),
                )
            } else if msg_lower.contains("unclosed")
                || (msg_lower.contains("block") && msg_lower.contains("expected"))
                || msg_lower.contains("missing '}'")
            {
                Some("Unclosed `{` -- check for a missing `}` to close the block".to_string())
            } else {
                None
            }
        }
        ParseError::LexerError { message } => {
            let msg_lower = message.to_lowercase();
            if msg_lower.contains("unterminated") || msg_lower.contains("unclosed") {
                Some(
                    "Check for an unclosed string, regex, or heredoc near this position"
                        .to_string(),
                )
            } else if msg_lower.contains("invalid") && msg_lower.contains("character") {
                Some(
                    "Remove or replace the invalid character -- Perl source should be valid UTF-8 or the encoding declared with 'use utf8;'"
                        .to_string(),
                )
            } else {
                None
            }
        }
        ParseError::RecursionLimit => Some(
            "The code is too deeply nested -- consider refactoring into smaller subroutines"
                .to_string(),
        ),
        ParseError::InvalidNumber { literal } => Some(format!(
            "'{literal}' is not a valid number -- check for misplaced underscores or invalid digits"
        )),
        ParseError::InvalidString => Some(
            "Check for a missing closing quote or an invalid escape sequence".to_string(),
        ),
        ParseError::InvalidRegex { .. } => Some(
            "Check the regex pattern for unmatched delimiters, invalid quantifiers, or unescaped metacharacters"
                .to_string(),
        ),
        ParseError::NestingTooDeep { .. } => Some(
            "Reduce nesting depth by extracting inner logic into named subroutines".to_string(),
        ),
        ParseError::Cancelled | ParseError::Recovered { .. } => None,
        _ => None,
    }
}

fn statement_segment_end(source: &str, pattern_start: usize) -> usize {
    let tail = &source[pattern_start..];
    let line_end = tail.find('\n').map(|idx| pattern_start + idx).unwrap_or(source.len());
    source[pattern_start..line_end].find(';').map(|idx| pattern_start + idx).unwrap_or(line_end)
}

fn unterminated_regex_delimiter_hint(source: &str, location: usize) -> Option<String> {
    let safe_loc = location.min(source.len());
    let line_start = source[..safe_loc].rfind('\n').map(|idx| idx + 1).unwrap_or(0);
    let line_end = source[safe_loc..].find('\n').map(|idx| safe_loc + idx).unwrap_or(source.len());
    let line = &source[line_start..line_end];

    for opener in ["m/", "s/", "qr/", "tr/", "y/"] {
        if let Some(rel_open) = line.rfind(opener) {
            let open_idx = line_start + rel_open;
            if safe_loc < open_idx {
                continue;
            }
            let pattern_start = open_idx + opener.len();
            let scan_end = statement_segment_end(source, pattern_start);
            let segment = &source[pattern_start..scan_end];
            if !segment.contains('/') {
                let delimiter = opener.chars().last().unwrap_or('/');
                return Some(format!(
                    "Search pattern not terminated — add a closing '{delimiter}' to complete the {opener} pattern"
                ));
            }
        }
    }

    if let Some(rel_bind) = line.rfind("=~") {
        let bind_idx = line_start + rel_bind;
        if safe_loc >= bind_idx {
            let after_bind = bind_idx + "=~".len();
            let rest = source.get(after_bind..line_end)?.trim_start();
            if rest.starts_with('/') {
                let slash_start = after_bind + (source[after_bind..line_end].len() - rest.len());
                let pattern_start = slash_start + 1;
                let scan_end = statement_segment_end(source, pattern_start);
                let segment = &source[pattern_start..scan_end];
                if !segment.contains('/') {
                    return Some(
                        "Search pattern not terminated — add a closing '/' to complete the regex"
                            .to_string(),
                    );
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unterminated_m_slash_pattern_names_delimiter() {
        let source = "my $m = $s =~ m/abc;\n";
        let error = ParseError::unexpected("expression", "unknown token", source.len() - 2);
        let hint = build_parse_error_hint_with_source(&error, "", Some(source)).unwrap();
        assert!(hint.contains("Search pattern not terminated"));
        assert!(hint.contains('/'));
    }

    #[test]
    fn unterminated_regex_helper_finds_m_slash_on_issue_fixture() {
        let source = "use strict;\nuse warnings;\nmy $s = \"abc\";\nmy $m = $s =~ m/abc;\n";
        let hint = unterminated_regex_delimiter_hint(source, 55);
        assert!(
            hint.as_deref().is_some_and(|h| h.contains("Search pattern not terminated")),
            "hint={hint:?}"
        );
    }
}
