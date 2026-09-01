//! Prompt construction from inline completion context.

use crate::providers::inline_completion::PreparedInlineCompletionContext;

/// Sentinel injected between the prefix and the suffix in the user message.
const CURSOR_MARKER: &str = "<CURSOR>";
/// Zero-collision substitute for marker occurrences inside captured user
/// text. Escaping is not invertible, but the prompt only needs the real
/// marker to appear exactly once.
const CURSOR_MARKER_ESCAPE: &str = "<CURSOR_ESCAPED>";

/// Escape cursor-marker occurrences in user-supplied text before injection
/// so captured code containing the literal sentinel cannot create a second,
/// ambiguous completion point.
fn escape_cursor_marker(text: &str) -> String {
    text.replace(CURSOR_MARKER, CURSOR_MARKER_ESCAPE)
}

/// Build an OpenAI-compatible prompt from the prepared context.
///
/// Returns a `(system, user)` message pair suitable for the chat completions API.
pub fn build_fim_prompt(context: &PreparedInlineCompletionContext) -> (String, String) {
    let mut system = String::from(
        "You are a Perl code completion assistant. Complete the code at the cursor position. \
         Return ONLY the completion text, no explanation, no markdown.",
    );
    // The text after the cursor already exists in the document. Naming it
    // keeps the model from re-emitting the suffix or the epilogue (#5049):
    // without it, mid-block completions routinely duplicate the closing
    // brace the prompt just showed.
    system.push_str(
        "\nText after <CURSOR> already exists in the document: complete only the code that \
         belongs at the cursor position and do not repeat the existing text.",
    );

    // Add context about the current scope
    if let Some(ref pkg) = context.current_package {
        system.push_str(&format!("\nCurrent package: {pkg}"));
    }
    if let Some(ref func) = context.current_function {
        system.push_str(&format!("\nInside subroutine: {func}"));
    }
    if !context.imports.is_empty() {
        system.push_str(&format!("\nImported modules: {}", context.imports.join(", ")));
    }

    // Build the user message with the code context. The budgeted preceding
    // lines give the model the left-of-cursor scope it was captured for;
    // the suffix and the bounded following lines (#10273 capture) close the
    // FIM gap: the model can see the closing brace or function epilogue
    // after the cursor. Every captured piece is marker-escaped: user text
    // containing the literal sentinel must not create a second completion
    // point.
    let mut user = String::new();
    for line in &context.preceding_lines {
        user.push_str(&escape_cursor_marker(line));
        user.push('\n');
    }
    if let Some(ref prev) = context.previous_non_empty_line {
        // `preceding_lines` covers the document rows before the cursor, so
        // the closest previous non-empty line is already among them when the
        // budget retained it (possibly followed by blank rows). Re-emitting
        // it would duplicate context and misrepresent source order.
        let duplicated = context.preceding_lines.iter().any(|line| line == prev);
        if !duplicated {
            user.push_str(&escape_cursor_marker(prev));
            user.push('\n');
        }
    }
    user.push_str(&escape_cursor_marker(&context.prefix));
    user.push_str(CURSOR_MARKER);
    user.push_str(&escape_cursor_marker(&context.suffix));
    for line in &context.following_lines {
        user.push('\n');
        user.push_str(&escape_cursor_marker(line));
    }

    (system, user)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_includes_preceding_lines() {
        let ctx = PreparedInlineCompletionContext {
            prefix: "my $x = ".to_string(),
            current_line: "my $x = ".to_string(),
            previous_non_empty_line: Some("my $y = 2;".to_string()),
            preceding_lines: vec![
                "use strict;".to_string(),
                "use warnings;".to_string(),
                "my $y = 2;".to_string(),
            ],
            ..PreparedInlineCompletionContext::default()
        };
        let (_, user) = build_fim_prompt(&ctx);
        // Budgeted preceding context must reach the model, not just the
        // closest line.
        assert!(user.contains("use warnings;"), "preceding lines must be included, got: {user}");
        // And it must precede the cursor marker, not be silently dropped.
        let cursor = user.find("<CURSOR>").expect("cursor marker");
        let warn = user.find("use warnings;").expect("warnings line");
        assert!(warn < cursor, "preceding line must precede cursor, got: {user}");
    }

    #[test]
    fn prompt_does_not_duplicate_the_closest_preceding_line() {
        // `preceding_lines` already ends with the closest previous non-empty
        // line when the budget retained it; emitting it twice wastes tokens.
        let ctx = PreparedInlineCompletionContext {
            prefix: "my $x = ".to_string(),
            current_line: "my $x = ".to_string(),
            previous_non_empty_line: Some("my $y = 2;".to_string()),
            preceding_lines: vec![
                "use strict;".to_string(),
                "use warnings;".to_string(),
                "my $y = 2;".to_string(),
            ],
            ..PreparedInlineCompletionContext::default()
        };
        let (_, user) = build_fim_prompt(&ctx);
        assert_eq!(
            user.matches("my $y = 2;").count(),
            1,
            "closest preceding line must appear once, got: {user}"
        );
    }

    #[test]
    fn prompt_does_not_duplicate_when_blank_rows_follow_the_previous_line() {
        // Blank lines after the closest previous non-empty line must not
        // hide the duplicate: the code row is already in preceding_lines.
        let ctx = PreparedInlineCompletionContext {
            prefix: "if ($ready) {".to_string(),
            current_line: "if ($ready) {".to_string(),
            previous_non_empty_line: Some("my $y = 2;".to_string()),
            preceding_lines: vec![
                "use strict;".to_string(),
                "my $y = 2;".to_string(),
                String::new(),
                String::new(),
            ],
            ..PreparedInlineCompletionContext::default()
        };
        let (_, user) = build_fim_prompt(&ctx);
        assert_eq!(
            user.matches("my $y = 2;").count(),
            1,
            "earlier context row must appear once, got: {user}"
        );
    }

    #[test]
    fn basic_prompt_includes_prefix() {
        let ctx = PreparedInlineCompletionContext {
            prefix: "my $x = ".to_string(),
            current_line: "my $x = ".to_string(),
            previous_non_empty_line: Some("use strict;".to_string()),
            current_function: Some("new".to_string()),
            current_package: Some("MyClass".to_string()),
            variables: vec!["$self".to_string()],
            imports: vec!["strict".to_string()],
            ..PreparedInlineCompletionContext::default()
        };
        let (system, user) = build_fim_prompt(&ctx);
        assert!(system.contains("Perl"));
        assert!(system.contains("MyClass"));
        assert!(system.contains("new"));
        assert!(user.contains("my $x = "));
        assert!(user.contains("<CURSOR>"));
        assert!(user.contains("use strict;"));
    }

    #[test]
    fn prompt_includes_suffix_and_following_lines() {
        let ctx = PreparedInlineCompletionContext {
            prefix: "if ($ready) {".to_string(),
            current_line: "if ($ready) {".to_string(),
            suffix: "}".to_string(),
            following_lines: vec!["}".to_string(), "return 1;".to_string()],
            ..PreparedInlineCompletionContext::default()
        };
        let (system, user) = build_fim_prompt(&ctx);
        // The existing text after the cursor must be visible to the model:
        // the suffix directly follows the cursor marker, and the bounded
        // following lines (closing brace, epilogue) appear after it.
        assert!(user.contains("<CURSOR>}"), "suffix must follow the cursor marker, got: {user}");
        assert!(user.contains("return 1;"), "following lines must be included, got: {user}");
        // The system message must tell the model the suffix already exists so
        // it does not repeat it.
        assert!(system.contains("after <CURSOR>"), "system must explain the suffix, got: {system}");
    }

    #[test]
    fn prompt_without_suffix_ends_at_cursor_marker() {
        let ctx = PreparedInlineCompletionContext {
            prefix: "my $x = ".to_string(),
            current_line: "my $x = ".to_string(),
            ..PreparedInlineCompletionContext::default()
        };
        let (_, user) = build_fim_prompt(&ctx);
        assert!(user.ends_with("<CURSOR>"), "no suffix must end at the marker, got: {user}");
    }

    #[test]
    fn user_text_containing_cursor_marker_is_escaped() {
        let ctx = PreparedInlineCompletionContext {
            prefix: "my $x = ".to_string(),
            current_line: "my $x = ".to_string(),
            suffix: "s{<CURSOR>}{replacement}g;".to_string(),
            following_lines: vec!["print q[<CURSOR>];".to_string()],
            ..PreparedInlineCompletionContext::default()
        };
        let (_, user) = build_fim_prompt(&ctx);
        // Exactly one literal marker may remain: the injected completion
        // point. A second marker from captured text makes the prompt
        // ambiguous about where the completion belongs.
        assert_eq!(
            user.matches("<CURSOR>").count(),
            1,
            "cursor marker must stay unambiguous, got: {user}"
        );
        assert!(
            user.contains("<CURSOR_ESCAPED>"),
            "user-supplied marker occurrences must be escaped, got: {user}"
        );
    }
}
