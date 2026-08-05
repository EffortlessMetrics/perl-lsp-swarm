//! Built-in function completion for Perl
//!
//! Provides completion for Perl built-in functions with signatures.

mod catalog;
mod metadata;

use super::{context::CompletionContext, items::CompletionItem, items::InsertTextFormat};
use std::collections::HashSet;
use std::sync::LazyLock;

/// Static builtins HashSet — avoids rebuilding ~200 entries on every completion
/// request (#5053 item 2).
static BUILTIN_SET: LazyLock<HashSet<&'static str>> =
    LazyLock::new(|| catalog::all_names().collect());

/// Get a reference to the static builtins HashSet.
pub fn builtin_set() -> &'static HashSet<&'static str> {
    &BUILTIN_SET
}

/// Create the builtins HashSet (legacy API — prefer builtin_set() for hot paths).
pub fn create_builtins() -> HashSet<&'static str> {
    catalog::all_names().collect()
}

/// Add built-in function completions
pub fn add_builtin_completions(
    completions: &mut Vec<CompletionItem>,
    context: &CompletionContext,
    builtins: &HashSet<&'static str>,
) {
    for builtin in builtins {
        if builtin.starts_with(&context.prefix) {
            let (insert_text, detail, documentation) = metadata::builtin_info(builtin);

            completions.push(CompletionItem {
                label: builtin.to_string(),
                kind: super::items::CompletionItemKind::Function,
                detail: Some(detail.to_string()),
                documentation: documentation.map(str::to_string),
                insert_text: Some(insert_text.to_string()),
                sort_text: Some(format!("3_{}", builtin)),
                filter_text: Some(builtin.to_string()),
                additional_edits: vec![],
                text_edit_range: Some((context.prefix_start, context.position)),
                commit_characters: None,
                insert_text_format: InsertTextFormat::for_authored_body(insert_text),
                label_details: None,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn context_for(prefix: &str) -> CompletionContext {
        CompletionContext {
            position: prefix.len(),
            trigger_character: None,
            in_string: false,
            in_regex: false,
            in_comment: false,
            in_use_statement: false,
            current_package: "main".to_string(),
            prefix: prefix.to_string(),
            prefix_start: 0,
            cursor_scope_id: 0,
        }
    }

    #[test]
    fn builtin_completion_uses_cataloged_metadata() -> TestResult {
        let builtins = create_builtins();
        let mut completions = Vec::new();
        add_builtin_completions(&mut completions, &context_for("op"), &builtins);

        let open = completions
            .iter()
            .find(|item| item.label == "open")
            .ok_or("open completion missing")?;

        assert_eq!(open.detail.as_deref(), Some("open FILEHANDLE, MODE, FILENAME"));
        assert_eq!(
            open.insert_text.as_deref(),
            Some(
                "open(my \\$fh, '${1:<}', ${2:\\$file}) or die \"Cannot open ${2:\\$file}: \\$!\";"
            )
        );
        assert!(open.documentation.as_deref().is_some_and(|doc| doc.contains("Three-arg")));

        // #4956: this body is a snippet even though the item is a Function.
        // Both facts have to hold, and the fallback has to be valid Perl.
        assert_eq!(open.kind, super::super::items::CompletionItemKind::Function);
        assert_eq!(
            open.insert_text_format.plain_fallback(),
            Some("open(my $fh, '<', $file) or die \"Cannot open $file: $!\";")
        );

        Ok(())
    }

    /// A builtin whose insert text merely mentions a Perl variable is not a
    /// snippet: `$dh` must reach the buffer verbatim, not be swallowed as an
    /// unknown snippet variable.
    #[test]
    fn builtin_with_a_literal_perl_variable_stays_plaintext() -> TestResult {
        let builtins = create_builtins();
        let mut completions = Vec::new();
        add_builtin_completions(&mut completions, &context_for("opendir"), &builtins);

        let opendir = completions
            .iter()
            .find(|item| item.label == "opendir")
            .ok_or("opendir completion missing")?;

        assert_eq!(opendir.insert_text.as_deref(), Some("opendir(my $dh, )"));
        assert_eq!(opendir.insert_text_format, InsertTextFormat::PlainText);

        Ok(())
    }

    /// Ordinary builtins are unaffected — no snippet framing added.
    #[test]
    fn ordinary_builtin_is_plaintext() -> TestResult {
        let builtins = create_builtins();
        let mut completions = Vec::new();
        add_builtin_completions(&mut completions, &context_for("print"), &builtins);

        let print = completions
            .iter()
            .find(|item| item.label == "print")
            .ok_or("print completion missing")?;

        assert_eq!(print.insert_text.as_deref(), Some("print "));
        assert_eq!(print.insert_text_format, InsertTextFormat::PlainText);

        Ok(())
    }

    #[test]
    fn builtin_completion_keeps_fallback_for_catalog_entries_without_metadata() -> TestResult {
        let builtins = create_builtins();
        let mut completions = Vec::new();
        add_builtin_completions(&mut completions, &context_for("-r"), &builtins);

        let file_test =
            completions.iter().find(|item| item.label == "-r").ok_or("-r completion missing")?;

        assert_eq!(file_test.detail.as_deref(), Some("built-in function"));
        assert_eq!(file_test.insert_text.as_deref(), Some("-r"));
        assert_eq!(file_test.documentation.as_deref(), Some("Perl built-in function."));

        Ok(())
    }
}
