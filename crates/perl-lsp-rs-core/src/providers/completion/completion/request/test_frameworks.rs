use super::super::{CompletionContext, CompletionItem};
use crate::providers::completion_item::{CompletionItemKind, InsertTextFormat};
use crate::providers::testing::test2::Test2Facts;
use std::borrow::Cow;
use std::collections::HashSet;

const TEST_MORE_DETAIL: &str = "Test::More";
const TEST2_DETAIL: &str = "Test2 imported symbol";

/// Reconcile generic test-context completion with the frameworks actually
/// imported by the current document.
///
/// The dispatch layer historically adds the complete Test::More table for any
/// `.t` file and for Test2 source. Until canonical FrameworkAdapter facts are
/// live, this request-boundary bridge consumes the existing reviewed Test2
/// import reader and the parser-owned module set to prevent confident false
/// suggestions without creating another framework export table.
pub(super) fn reconcile(
    completions: &mut Vec<CompletionItem>,
    context: &CompletionContext,
    source: &str,
    used_modules: &HashSet<String>,
) {
    let uses_test_more = used_modules.contains("Test::More");
    if !uses_test_more {
        completions.retain(|item| item.detail.as_deref() != Some(TEST_MORE_DETAIL));
    }

    let facts = Test2Facts::from_source(source);
    if !facts.uses_test2() {
        return;
    }

    for name in &facts.imported_symbols {
        if !context.prefix.is_empty() && !name.starts_with(&context.prefix) {
            continue;
        }

        completions.push(CompletionItem {
            label: Cow::Owned(name.clone()),
            kind: CompletionItemKind::Function,
            detail: Some(Cow::Borrowed(TEST2_DETAIL)),
            documentation: None,
            // Import facts prove the local name, not a universal call shape.
            // Rich Test2 snippets/signatures move to canonical adapter facts;
            // inserting only the authorized name avoids fabricating semantics.
            insert_text: Some(Cow::Owned(name.clone())),
            sort_text: Some(Cow::Owned(format!("2_test2_{name}"))),
            filter_text: Some(Cow::Owned(name.clone())),
            additional_edits: vec![],
            text_edit_range: Some((context.prefix_start, context.position)),
            commit_characters: None,
            insert_text_format: InsertTextFormat::PlainText,
            label_details: None,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::completion::completion::test_more::add_test_more_completions;

    fn context(prefix: &str) -> CompletionContext {
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

    fn labels(items: &[CompletionItem]) -> Vec<&str> {
        items.iter().map(|item| item.label.as_ref()).collect()
    }

    fn seeded_test_more(prefix: &str) -> (CompletionContext, Vec<CompletionItem>) {
        let context = context(prefix);
        let mut items = Vec::new();
        add_test_more_completions(&mut items, &context);
        (context, items)
    }

    #[test]
    fn generic_test_context_does_not_assume_test_more() {
        let (context, mut items) = seeded_test_more("");

        reconcile(&mut items, &context, "ok(1);\n", &HashSet::new());

        assert!(items.is_empty(), "a .t context without an import must stay framework-neutral");
    }

    #[test]
    fn explicit_test_more_import_preserves_test_more_items() {
        let (context, mut items) = seeded_test_more("eq_");
        let modules = HashSet::from(["Test::More".to_string()]);

        reconcile(&mut items, &context, "use Test::More;\n", &modules);

        let labels = labels(&items);
        assert!(labels.contains(&"eq_array"));
        assert!(labels.contains(&"eq_hash"));
        assert!(labels.contains(&"eq_set"));
    }

    #[test]
    fn test2_exclusion_removes_false_ok_completion() {
        let (context, mut items) = seeded_test_more("");

        reconcile(
            &mut items,
            &context,
            "use Test2::V0 '!ok';\nis(1, 1);\n",
            &HashSet::new(),
        );

        let labels = labels(&items);
        assert!(!labels.contains(&"ok"));
        assert!(labels.contains(&"is"));
        assert!(items.iter().all(|item| item.detail.as_deref() != Some(TEST_MORE_DETAIL)));
    }

    #[test]
    fn test2_alias_completes_the_local_name_only() {
        let (context, mut items) = seeded_test_more("");

        reconcile(
            &mut items,
            &context,
            "use Test2::V0 ok => {-as => 'my_ok'};\nmy_ok(1);\n",
            &HashSet::new(),
        );

        let labels = labels(&items);
        assert!(labels.contains(&"my_ok"));
        assert!(!labels.contains(&"ok"));
    }

    #[test]
    fn plain_test2_v1_completes_only_the_t2_handle() {
        let (context, mut items) = seeded_test_more("");

        reconcile(&mut items, &context, "use Test2::V1;\nT2->ok(1);\n", &HashSet::new());

        assert_eq!(labels(&items), vec!["T2"]);
    }

    #[test]
    fn standalone_compare_uses_its_own_default_imports() {
        let (context, mut items) = seeded_test_more("");

        reconcile(
            &mut items,
            &context,
            "use Test2::Tools::Compare;\nis(1, 1);\nlike('x', qr/x/);\n",
            &HashSet::new(),
        );

        let labels = labels(&items);
        assert_eq!(labels, vec!["is", "like"]);
    }
}
