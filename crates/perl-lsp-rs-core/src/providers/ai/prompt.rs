//! Prompt construction from inline completion context.

use crate::providers::inline_completion::PreparedInlineCompletionContext;

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

    // Build the user message with the code context. The suffix and the
    // bounded following lines (#10273 capture) close the FIM gap: the model
    // can see the closing brace or function epilogue after the cursor.
    let mut user = String::new();
    if let Some(ref prev) = context.previous_non_empty_line {
        user.push_str(prev);
        user.push('\n');
    }
    user.push_str(&context.prefix);
    user.push_str("<CURSOR>");
    user.push_str(&context.suffix);
    for line in &context.following_lines {
        user.push('\n');
        user.push_str(line);
    }

    (system, user)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
