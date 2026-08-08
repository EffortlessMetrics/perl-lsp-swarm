//! Built-in function completion for Perl
//!
//! Provides completion for Perl built-in functions with signatures.

mod catalog;
mod metadata;

use super::{context::CompletionContext, items::CompletionItem, items::InsertTextFormat};
use std::borrow::Cow;
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
#[cfg(test)]
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
                label: Cow::Borrowed(builtin),
                kind: super::items::CompletionItemKind::Function,
                detail: Some(Cow::Borrowed(detail)),
                documentation: documentation.map(Cow::Borrowed),
                insert_text: Some(Cow::Borrowed(insert_text)),
                sort_text: Some(Cow::Owned(format!("3_{}", builtin))),
                filter_text: Some(Cow::Borrowed(builtin)),
                additional_edits: vec![],
                text_edit_range: Some((context.prefix_start, context.position)),
                commit_characters: None,
                insert_text_format: InsertTextFormat::for_authored_body(insert_text),
                label_details: None,
            });
        }
    }
}

/// Remove pragma-gated names from `set` based on the active `PragmaState`.
///
/// - `say` is removed unless `use feature 'say'` (or an equivalent version
///   bundle such as `use 5.010` / `use v5.10`) is in scope.
/// - Each `use builtin` short name is removed unless `has_builtin_import`
///   returns `true` for that name.  Do **NOT** use `has_feature("builtin")` —
///   that alias resolves to `"module_true"` in the pragma crate and is
///   incorrect for this check.
pub fn filter_pragma_gated(set: &mut HashSet<&'static str>, state: &perl_pragma::PragmaState) {
    if !state.has_feature("say") {
        set.remove("say");
    }

    for name in catalog::builtin_import_names() {
        if !state.has_builtin_import(name) {
            set.remove(name);
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

    // AC1: no feature pragma → say absent
    #[test]
    fn ac1_say_absent_without_feature_pragma() {
        let state = perl_pragma::PragmaState::default();
        let mut set = create_builtins();
        filter_pragma_gated(&mut set, &state);
        assert!(!set.contains("say"), "say must be absent when no feature pragma is in scope");
    }

    // AC2: use feature 'say' → say present
    #[test]
    fn ac2_say_present_with_use_feature_say() {
        let mut state = perl_pragma::PragmaState::default();
        state.features.push("say");
        let mut set = create_builtins();
        filter_pragma_gated(&mut set, &state);
        assert!(set.contains("say"), "say must be present when 'use feature say' is in scope");
    }

    // AC3: use 5.010 implies say via version bundle
    #[test]
    fn ac3_say_present_with_version_implied_bundle() {
        // version 5.10 enables the say feature via its bundle
        let features =
            perl_pragma::features_enabled_by_version(perl_pragma::PerlVersion::new(5, 10));
        let mut state = perl_pragma::PragmaState::default();
        state.features = features.into_iter().collect();
        let mut set = create_builtins();
        filter_pragma_gated(&mut set, &state);
        assert!(set.contains("say"), "say must be present when use 5.010 implies the say feature");
    }

    // AC4: no use builtin → true and trim absent
    #[test]
    fn ac4_builtin_names_absent_without_use_builtin() {
        let state = perl_pragma::PragmaState::default();
        let mut set = create_builtins();
        filter_pragma_gated(&mut set, &state);
        assert!(!set.contains("true"), "true must be absent when use builtin is not in scope");
        assert!(!set.contains("trim"), "trim must be absent when use builtin is not in scope");
    }

    // AC5: use builtin 'true', 'false' → true present
    #[test]
    fn ac5_true_present_with_use_builtin_true_false() {
        let mut state = perl_pragma::PragmaState::default();
        state.builtin_imports.push("true".to_string());
        state.builtin_imports.push("false".to_string());
        let mut set = create_builtins();
        filter_pragma_gated(&mut set, &state);
        assert!(set.contains("true"), "true must be present when 'use builtin true' is in scope");
        assert!(
            set.contains("false"),
            "false must be present when 'use builtin false' is in scope"
        );
        assert!(!set.contains("trim"), "trim must be absent when not imported via use builtin");
    }

    // AC6: say appears exactly once when feature is active
    #[test]
    fn ac6_say_appears_exactly_once_with_feature() {
        let mut state = perl_pragma::PragmaState::default();
        state.features.push("say");
        let mut set = create_builtins();
        filter_pragma_gated(&mut set, &state);
        let mut completions = Vec::new();
        add_builtin_completions(&mut completions, &context_for("sa"), &set);
        let say_count = completions.iter().filter(|item| item.label == "say").count();
        assert_eq!(say_count, 1, "say must appear exactly once in completions");
    }
}
