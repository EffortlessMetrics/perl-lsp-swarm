use super::super::{CompletionContext, CompletionItem, CompletionProvider};
use crate::providers::completion_item::{
    CompletionItemKind, InsertTextFormat,
};
use crate::providers::testing::test2::Test2Facts;
use std::borrow::Cow;

const TEST_MORE_DETAIL: &str = "Test::More";
const TEST2_DETAIL: &str = "Test2 imported symbol";

/// Reconcile the generic test table with framework imports in the cursor's
/// active package.
///
/// The dispatch layer historically adds the complete Test::More table for a
/// generic test context. This bridge runs only when that table is actually
/// present, so structural/use/string/method completion flows cannot acquire
/// Test2 functions after the fact. It consumes the existing reviewed Test2
/// import resolver and uses the semantic package ranges already held by the
/// completion provider; it does not create another export table.
pub(super) fn reconcile(
    completions: &mut Vec<CompletionItem>,
    provider: &CompletionProvider,
    context: &CompletionContext,
    source: &str,
) {
    let generic_test_table_was_added =
        completions.iter().any(|item| item.detail.as_deref() == Some(TEST_MORE_DETAIL));
    if !generic_test_table_was_added {
        return;
    }

    let package_statements = use_statements_for_package(
        source,
        provider,
        context.current_package.as_str(),
    );
    let uses_test_more = package_statements.iter().any(|statement| {
        use_module_and_args(statement)
            .is_some_and(|(module, args)| module == "Test::More" && args.trim() != "()")
    });
    if !uses_test_more {
        completions.retain(|item| item.detail.as_deref() != Some(TEST_MORE_DETAIL));
    }

    let scoped_source = package_statements
        .iter()
        .map(|statement| format!("{statement};\n"))
        .collect::<String>();
    let facts = Test2Facts::from_source(scoped_source.as_str());
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
            // Rich snippets/signatures move to canonical adapter facts;
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

fn use_statements_for_package(
    source: &str,
    provider: &CompletionProvider,
    current_package: &str,
) -> Vec<String> {
    use_statements_with_offsets(source)
        .into_iter()
        .filter_map(|(offset, statement)| {
            let package = CompletionContext::detect_current_package(
                &provider.symbol_table,
                offset,
            );
            (package == current_package).then_some(statement)
        })
        .collect()
}

fn use_module_and_args(statement: &str) -> Option<(&str, &str)> {
    let rest = statement.strip_prefix("use")?.trim_start();
    let module_end = rest
        .find(|character: char| {
            !(character.is_ascii_alphanumeric() || character == '_' || character == ':')
        })
        .unwrap_or(rest.len());
    (module_end > 0).then(|| (&rest[..module_end], rest[module_end..].trim()))
}

/// Extract semicolon-terminated `use` statements with their real source offset.
///
/// This is statement location plumbing only. Module import semantics remain in
/// `providers::testing::test2`; package ownership comes from the provider's
/// semantic symbol table.
fn use_statements_with_offsets(source: &str) -> Vec<(usize, String)> {
    let mut output = Vec::new();
    let mut current = String::new();
    let mut statement_offset = None;
    let mut in_single = false;
    let mut in_double = false;
    let mut in_comment = false;
    let mut escaped = false;

    for (offset, character) in source.char_indices() {
        if in_comment {
            if character == '\n' {
                in_comment = false;
            }
            continue;
        }

        if escaped {
            current.push(character);
            escaped = false;
            continue;
        }

        match character {
            '\\' if in_single || in_double => {
                if statement_offset.is_none() {
                    statement_offset = Some(offset);
                }
                escaped = true;
                current.push(character);
            }
            '#' if !in_single && !in_double => in_comment = true,
            '\'' if !in_double => {
                if statement_offset.is_none() {
                    statement_offset = Some(offset);
                }
                in_single = !in_single;
                current.push(character);
            }
            '"' if !in_single => {
                if statement_offset.is_none() {
                    statement_offset = Some(offset);
                }
                in_double = !in_double;
                current.push(character);
            }
            ';' if !in_single && !in_double => {
                let statement = current.trim();
                if statement
                    .strip_prefix("use")
                    .is_some_and(|rest| rest.starts_with(char::is_whitespace))
                {
                    output.push((
                        statement_offset.unwrap_or(offset),
                        statement.to_string(),
                    ));
                }
                current.clear();
                statement_offset = None;
            }
            _ => {
                if statement_offset.is_none() && !character.is_whitespace() {
                    statement_offset = Some(offset);
                }
                current.push(character);
            }
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::completion::completion::test_more::add_test_more_completions;
    use perl_parser_core::Parser;
    use perl_tdd_support::must;

    fn context(prefix: &str, package: &str) -> CompletionContext {
        CompletionContext {
            position: prefix.len(),
            trigger_character: None,
            in_string: false,
            in_regex: false,
            in_comment: false,
            in_use_statement: false,
            current_package: package.to_string(),
            prefix: prefix.to_string(),
            prefix_start: 0,
            cursor_scope_id: 0,
        }
    }

    fn provider(source: &str) -> CompletionProvider {
        let mut parser = Parser::new(source);
        let ast = must(parser.parse());
        CompletionProvider::new_with_index_and_source(&ast, source, None)
    }

    fn labels(items: &[CompletionItem]) -> Vec<&str> {
        items.iter().map(|item| item.label.as_ref()).collect()
    }

    fn seeded_test_more(prefix: &str, package: &str) -> (CompletionContext, Vec<CompletionItem>) {
        let context = context(prefix, package);
        let mut items = Vec::new();
        add_test_more_completions(&mut items, &context);
        (context, items)
    }

    fn non_test_item(label: &'static str) -> CompletionItem {
        CompletionItem {
            label: Cow::Borrowed(label),
            kind: CompletionItemKind::Module,
            detail: Some(Cow::Borrowed("module")),
            documentation: None,
            insert_text: Some(Cow::Borrowed(label)),
            sort_text: None,
            filter_text: None,
            additional_edits: vec![],
            text_edit_range: None,
            commit_characters: None,
            insert_text_format: InsertTextFormat::PlainText,
            label_details: None,
        }
    }

    #[test]
    fn generic_test_context_does_not_assume_test_more() {
        let source = "ok(1);\n";
        let provider = provider(source);
        let (context, mut items) = seeded_test_more("", "main");

        reconcile(&mut items, &provider, &context, source);

        assert!(
            items.is_empty(),
            "a .t context without an import must stay framework-neutral"
        );
    }

    #[test]
    fn explicit_test_more_import_preserves_test_more_items() {
        let source = "use Test::More;\n";
        let provider = provider(source);
        let (context, mut items) = seeded_test_more("eq_", "main");

        reconcile(&mut items, &provider, &context, source);

        let labels = labels(&items);
        assert!(labels.contains(&"eq_array"));
        assert!(labels.contains(&"eq_hash"));
        assert!(labels.contains(&"eq_set"));
    }

    #[test]
    fn test2_exclusion_removes_false_ok_completion() {
        let source = "use Test2::V0 '!ok';\nis(1, 1);\n";
        let provider = provider(source);
        let (context, mut items) = seeded_test_more("", "main");

        reconcile(&mut items, &provider, &context, source);

        let labels = labels(&items);
        assert!(!labels.contains(&"ok"));
        assert!(labels.contains(&"is"));
        assert!(
            items
                .iter()
                .all(|item| item.detail.as_deref() != Some(TEST_MORE_DETAIL))
        );
    }

    #[test]
    fn test2_alias_completes_the_local_name_only() {
        let source = "use Test2::V0 ok => {-as => 'my_ok'};\nmy_ok(1);\n";
        let provider = provider(source);
        let (context, mut items) = seeded_test_more("", "main");

        reconcile(&mut items, &provider, &context, source);

        let labels = labels(&items);
        assert!(labels.contains(&"my_ok"));
        assert!(!labels.contains(&"ok"));
    }

    #[test]
    fn plain_test2_v1_completes_only_the_t2_handle() {
        let source = "use Test2::V1;\nT2->ok(1);\n";
        let provider = provider(source);
        let (context, mut items) = seeded_test_more("", "main");

        reconcile(&mut items, &provider, &context, source);

        assert_eq!(labels(&items), vec!["T2"]);
    }

    #[test]
    fn standalone_compare_uses_its_own_default_imports() {
        let source = "use Test2::Tools::Compare;\nis(1, 1);\nlike('x', qr/x/);\n";
        let provider = provider(source);
        let (context, mut items) = seeded_test_more("", "main");

        reconcile(&mut items, &provider, &context, source);

        assert_eq!(labels(&items), vec!["is", "like"]);
    }

    #[test]
    fn imports_do_not_leak_across_package_boundaries() {
        let source = "package One;\nuse Test2::V0;\npackage Two;\nmy $value = 1;\n";
        let provider = provider(source);
        let (two_context, mut two_items) = seeded_test_more("i", "Two");

        reconcile(&mut two_items, &provider, &two_context, source);

        assert!(
            !labels(&two_items).contains(&"is"),
            "package Two must not receive package One's Test2 imports"
        );

        let (one_context, mut one_items) = seeded_test_more("i", "One");
        reconcile(&mut one_items, &provider, &one_context, source);
        assert!(labels(&one_items).contains(&"is"));
    }

    #[test]
    fn use_statement_flow_cannot_gain_test2_items() {
        let source = "use Test2::V0;\nuse I";
        let provider = provider(source);
        let mut context = context("I", "main");
        context.in_use_statement = true;
        let mut items = vec![non_test_item("Importer")];

        reconcile(&mut items, &provider, &context, source);

        assert_eq!(labels(&items), vec!["Importer"]);
    }

    #[test]
    fn string_path_flow_cannot_gain_test2_items() {
        let source = "use Test2::V0;\nmy $path = 'is';\n";
        let provider = provider(source);
        let mut context = context("is", "main");
        context.in_string = true;
        let mut items = vec![non_test_item("island.txt")];

        reconcile(&mut items, &provider, &context, source);

        assert_eq!(labels(&items), vec!["island.txt"]);
    }
}
