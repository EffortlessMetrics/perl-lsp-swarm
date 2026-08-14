use super::super::{CompletionContext, CompletionItem, CompletionProvider};
use crate::providers::completion_item::{CompletionItemKind, InsertTextFormat};
use crate::providers::testing::test2::{Test2Facts, is_test2_module};
use perl_parser_core::Parser;
use perl_parser_core::ast::{Node, NodeKind};
use std::borrow::Cow;
use std::collections::BTreeSet;
use std::sync::LazyLock;

const TEST_MORE_DETAIL: &str = "Test::More";
const TEST2_DETAIL: &str = "Test2 imported symbol";

static COMMON_TEST_NAMES: LazyLock<BTreeSet<String>> =
    LazyLock::new(|| Test2Facts::from_source("use Test2::V0;\n").imported_symbols);

#[derive(Debug, Clone, PartialEq, Eq)]
struct ScopedUse {
    package: String,
    module: String,
    statement: String,
}

/// Reconcile generic test completions with parser-backed imports in the cursor's
/// active Perl package.
///
/// This remains a compatibility bridge while FrameworkAdapter facts are built.
/// Source statement boundaries and package ownership come from the parser AST;
/// Test2 import semantics continue to come from the existing reviewed Test2
/// resolver. Completion code therefore owns neither a second Perl scanner nor a
/// second Test2 export table.
pub(super) fn reconcile(
    completions: &mut Vec<CompletionItem>,
    provider: &CompletionProvider,
    context: &CompletionContext,
    source: &str,
    filepath: Option<&str>,
) {
    if !is_plain_symbol_context(provider, context, source) {
        return;
    }

    let Some(scoped_uses) = collect_scoped_uses(source) else {
        // In an incomplete buffer that cannot produce an AST, preserve the
        // ordinary completion result rather than fabricating import facts.
        return;
    };
    let package_uses: Vec<&ScopedUse> =
        scoped_uses.iter().filter(|import| import.package == context.current_package).collect();

    let uses_test_more = package_uses.iter().any(|import| {
        import.module == "Test::More"
            && use_module_and_args(import.statement.as_str())
                .is_some_and(|(_, args)| args.trim() != "()")
    });
    let scoped_test2_source = package_uses
        .iter()
        .filter(|import| is_test2_module(import.module.as_str()))
        .map(|import| format!("{};\n", import.statement))
        .collect::<String>();
    let test2_facts = Test2Facts::from_source(scoped_test2_source.as_str());
    let uses_test2 = test2_facts.uses_test2();
    let generic_test_table_present =
        completions.iter().any(|item| item.detail.as_deref() == Some(TEST_MORE_DETAIL));

    if !uses_test_more && !uses_test2 {
        if !generic_test_table_present {
            return;
        }

        if filepath.is_some_and(|path| path.ends_with(".t")) {
            // A framework-neutral .t file keeps only the vocabulary shared by
            // the existing Test::More table and Test2::V0. This preserves useful
            // test editing without claiming Test::More-only exports.
            completions.retain(|item| {
                item.detail.as_deref() != Some(TEST_MORE_DETAIL)
                    || COMMON_TEST_NAMES.contains(item.label.as_ref())
            });
        } else {
            // `source.contains("use Test2::V0")` historically admitted quoted
            // fixture text. Parser facts prove that no framework import exists.
            completions.retain(|item| item.detail.as_deref() != Some(TEST_MORE_DETAIL));
        }
        return;
    }

    if !uses_test_more {
        completions.retain(|item| item.detail.as_deref() != Some(TEST_MORE_DETAIL));
    }

    if !uses_test2 {
        return;
    }

    for name in &test2_facts.imported_symbols {
        if !context.prefix.is_empty() && !name.starts_with(&context.prefix) {
            continue;
        }

        completions.push(CompletionItem {
            label: Cow::Owned(name.clone()),
            kind: CompletionItemKind::Function,
            detail: Some(Cow::Borrowed(TEST2_DETAIL)),
            documentation: None,
            // Import facts prove the local name, not a universal call shape.
            // Rich snippets/signatures move to canonical adapter facts.
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

/// Reject completion flows that dispatch owns as structural rather than plain
/// bareword/function completion. This keeps Test2 projection out of use lists,
/// paths, strings, regexes, sigils, hash keys, constructor options, and method
/// calls even though several of those paths share `SortAndReturn`.
fn is_plain_symbol_context(
    provider: &CompletionProvider,
    context: &CompletionContext,
    source: &str,
) -> bool {
    if context.in_comment || context.in_string || context.in_regex || context.in_use_statement {
        return false;
    }
    if context
        .prefix
        .chars()
        .next()
        .is_some_and(|character| matches!(character, '$' | '@' | '%' | '&'))
        || context.prefix.contains("->")
        || context.prefix.contains("::")
    {
        return false;
    }
    if CompletionProvider::detect_use_qw_import_context(source, context.position).is_some()
        || provider.is_has_type_value_context(source, context.position)
        || provider.is_has_options_key_context(source, context.position)
        || CompletionProvider::detect_hash_key_context(source, context.position).is_some()
        || provider.object_pad_constructor_package(source, context.position).is_some()
        || looks_like_indirect_method_context(context, source)
    {
        return false;
    }
    true
}

fn looks_like_indirect_method_context(context: &CompletionContext, source: &str) -> bool {
    let Some(first) = context.prefix.chars().next() else {
        return false;
    };
    if !(first.is_ascii_lowercase() || first == '_')
        || !context
            .prefix
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        return false;
    }

    let bytes = source.as_bytes();
    let mut word_end = context.position.min(bytes.len());
    while word_end < bytes.len()
        && (bytes[word_end].is_ascii_alphanumeric() || bytes[word_end] == b'_')
    {
        word_end += 1;
    }
    let Some(tail) = source.get(word_end..) else {
        return false;
    };
    let receiver = tail.trim_start_matches([' ', '\t']);
    if receiver.len() == tail.len() {
        return false;
    }

    receiver.starts_with('$')
        || receiver.chars().next().is_some_and(|character| character.is_ascii_uppercase())
}

fn collect_scoped_uses(source: &str) -> Option<Vec<ScopedUse>> {
    let mut parser = Parser::new(source);
    let ast = parser.parse().ok()?;
    let mut imports = Vec::new();
    let mut current_package = "main".to_string();
    walk_scoped_uses(&ast, source, &mut current_package, &mut imports);
    Some(imports)
}

fn walk_scoped_uses(
    node: &Node,
    source: &str,
    current_package: &mut String,
    imports: &mut Vec<ScopedUse>,
) {
    match &node.kind {
        NodeKind::Program { statements } => {
            for statement in statements {
                walk_scoped_uses(statement, source, current_package, imports);
            }
            return;
        }
        NodeKind::Block { statements } => {
            let saved_package = current_package.clone();
            for statement in statements {
                walk_scoped_uses(statement, source, current_package, imports);
            }
            *current_package = saved_package;
            return;
        }
        NodeKind::Package { name, block: Some(block), .. } => {
            let mut block_package = name.clone();
            walk_scoped_uses(block, source, &mut block_package, imports);
            return;
        }
        NodeKind::Package { name, block: None, .. } => {
            *current_package = name.clone();
            return;
        }
        NodeKind::Use { module, args, .. } if module == "Test::More" || is_test2_module(module) => {
            imports.push(ScopedUse {
                package: current_package.clone(),
                module: module.clone(),
                statement: source_use_statement(node, source, module, args),
            });
            return;
        }
        _ => {}
    }

    for child in node.children() {
        walk_scoped_uses(child, source, current_package, imports);
    }
}

fn source_use_statement(node: &Node, source: &str, module: &str, args: &[String]) -> String {
    let raw = source.get(node.location.start..node.location.end).unwrap_or_default().trim();
    let raw = raw.strip_suffix(';').unwrap_or(raw).trim();
    if raw.strip_prefix("use").is_some_and(|rest| rest.starts_with(char::is_whitespace)) {
        return raw.to_string();
    }

    let mut reconstructed = format!("use {module}");
    if !args.is_empty() {
        reconstructed.push(' ');
        reconstructed.push_str(args.join(" ").as_str());
    }
    reconstructed
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

#[cfg(test)]
mod tests {
    use super::*;
    use perl_tdd_support::must;

    fn complete(source_with_cursor: &str, filepath: Option<&str>) -> Vec<CompletionItem> {
        let position = source_with_cursor.find('|').unwrap_or(source_with_cursor.len());
        let source = source_with_cursor.replacen('|', "", 1);
        let mut parser = Parser::new(source.as_str());
        let ast = must(parser.parse());
        let provider = CompletionProvider::new_with_index_and_source(&ast, source.as_str(), None);
        provider.get_completions_with_path(source.as_str(), position, filepath)
    }

    fn labels(items: &[CompletionItem]) -> Vec<&str> {
        items.iter().map(|item| item.label.as_ref()).collect()
    }

    #[test]
    fn framework_neutral_test_file_keeps_only_common_test_vocabulary() {
        let common = complete("i|", Some("t/example.t"));
        assert!(labels(&common).contains(&"is"));

        let test_more_only = complete("eq_|", Some("t/example.t"));
        assert!(!labels(&test_more_only).contains(&"eq_array"));
    }

    #[test]
    fn explicit_test_more_import_preserves_test_more_items() {
        let items = complete("use Test::More;\neq_|", Some("t/example.t"));
        let labels = labels(&items);
        assert!(labels.contains(&"eq_array"));
        assert!(labels.contains(&"eq_hash"));
        assert!(labels.contains(&"eq_set"));
    }

    #[test]
    fn test2_exclusion_removes_false_ok_completion() {
        let items = complete("use Test2::V0 '!ok';\no|", Some("t/example.t"));
        assert!(!labels(&items).contains(&"ok"));
    }

    #[test]
    fn test2_alias_completes_unique_local_prefix() {
        let items = complete("use Test2::V0 ok => {-as => 'my_ok'};\nmy_|", Some("t/example.t"));
        let labels = labels(&items);
        assert!(labels.contains(&"my_ok"));
        assert!(!labels.contains(&"ok"));
    }

    #[test]
    fn plain_test2_v1_completes_t2_in_a_non_test_file() {
        let items = complete("use Test2::V1;\nT|", Some("lib/Example.pm"));
        assert_eq!(labels(&items), vec!["T2"]);
    }

    #[test]
    fn standalone_compare_completes_in_a_non_test_file() {
        let items = complete("use Test2::Tools::Compare;\nli|", Some("lib/Example.pm"));
        assert!(labels(&items).contains(&"like"));
    }

    #[test]
    fn imports_do_not_leak_across_package_boundaries() {
        let items =
            complete("package One;\nuse Test2::V0;\npackage Two;\ni|", Some("lib/Example.pm"));
        assert!(!labels(&items).contains(&"is"));
    }

    #[test]
    fn block_package_first_import_is_discovered() {
        let items =
            complete("package Foo {\n    use Test2::V0;\n    i|\n}\n", Some("lib/Example.pm"));
        assert!(labels(&items).contains(&"is"));
    }

    #[test]
    fn quote_like_fixture_text_does_not_create_an_import() {
        let items = complete("my $fixture = q{x; use Test2::V0;};\neq_|", Some("lib/Example.pm"));
        assert!(!labels(&items).contains(&"eq_array"));
    }

    #[test]
    fn bare_v0_import_reaches_completion() {
        let items = complete("use Test2::V0;\ni|", Some("t/example.t"));
        assert!(labels(&items).contains(&"is"));
    }

    #[test]
    fn empty_v0_import_does_not_add_test2_names() {
        let items = complete("use Test2::V0 ();\no|", Some("t/example.t"));
        assert!(!labels(&items).contains(&"ok"));
    }

    #[test]
    fn use_statement_flow_cannot_gain_test2_items() {
        let items = complete("use Test2::V0;\nuse I|", Some("t/example.t"));
        assert!(!labels(&items).contains(&"is"));
    }

    #[test]
    fn string_path_flow_cannot_gain_test2_items() {
        let items = complete("use Test2::V0;\nmy $path = 'is|';\n", Some("t/example.t"));
        assert!(!labels(&items).contains(&"is"));
    }
}
