use perl_lsp_rs_core::providers::completion::{CompletionItem, CompletionProvider};
use perl_lsp_rs_core::providers::testing::test2::{ResolvedImport, Test2Facts, resolve_import};
use perl_parser_core::Parser;
use perl_tdd_support::must;
use std::io;

fn resolve_v0(args: &str) -> Result<ResolvedImport, Box<dyn std::error::Error>> {
    resolve_import("Test2::V0", args)
        .ok_or_else(|| io::Error::other("Test2::V0 must be recognized").into())
}

fn complete(source_with_cursor: &str) -> Vec<CompletionItem> {
    let position = source_with_cursor
        .find('|')
        .expect("completion fixture must mark the cursor with |");
    let source = source_with_cursor.replacen('|', "", 1);
    let mut parser = Parser::new(source.as_str());
    let ast = must(parser.parse());
    let provider = CompletionProvider::new_with_index_and_source(&ast, source.as_str(), None);
    provider.get_completions_with_path(source.as_str(), position, Some("lib/Example.pm"))
}

fn test2_labels(items: &[CompletionItem]) -> Vec<&str> {
    items
        .iter()
        .filter(|item| item.sort_text.as_deref().is_some_and(|sort| sort.starts_with("2_test2_")))
        .map(|item| item.label.as_ref())
        .collect()
}

#[test]
fn parenthesized_explicit_selection_replaces_v0_defaults(
) -> Result<(), Box<dyn std::error::Error>> {
    let resolved = resolve_v0("('ok')")?;

    assert!(resolved.symbols.contains("ok"));
    assert!(!resolved.symbols.contains("is"), "nonempty parentheses must not restore defaults");
    assert!(!resolved.symbols.contains("like"), "nonempty parentheses must not restore the compare defaults");
    Ok(())
}

#[test]
fn parenthesized_exclusion_keeps_other_v0_defaults() -> Result<(), Box<dyn std::error::Error>> {
    let resolved = resolve_v0("('!ok')")?;

    assert!(!resolved.symbols.contains("ok"));
    assert!(resolved.symbols.contains("is"), "an exclusion alone retains other defaults");
    Ok(())
}

#[test]
fn parenthesized_qw_list_remains_explicit() -> Result<(), Box<dyn std::error::Error>> {
    let resolved = resolve_v0("(qw(ok is))")?;

    assert!(resolved.symbols.contains("ok"));
    assert!(resolved.symbols.contains("is"));
    assert!(!resolved.symbols.contains("like"), "the outer list must not restore defaults");
    Ok(())
}

#[test]
fn explicit_empty_parenthesized_import_remains_empty() -> Result<(), Box<dyn std::error::Error>> {
    let resolved = resolve_v0("()")?;

    assert!(resolved.symbols.is_empty());
    assert!(resolved.pragmas.is_none(), "use Module () does not call import");
    Ok(())
}

#[test]
fn source_facts_preserve_parenthesized_selection_and_pragmas() {
    let facts = Test2Facts::from_source("use Test2::V0 ('ok');\n");

    assert!(facts.is_imported("ok"));
    assert!(!facts.is_imported("is"));
    assert_eq!((facts.strict, facts.warnings), (true, true));
}

#[test]
fn completion_projects_only_the_parenthesized_selection() {
    let items = complete("use Test2::V0 ('ok');\n|");
    let labels = test2_labels(&items);

    assert!(!labels.is_empty(), "expected Test2-owned completion rows");
    assert!(labels.contains(&"ok"));
    assert!(!labels.contains(&"is"), "completion must not restore unselected V0 defaults");
}

#[test]
fn completion_honors_a_parenthesized_exclusion() {
    let items = complete("use Test2::V0 ('!ok');\n|");
    let labels = test2_labels(&items);

    assert!(!labels.is_empty(), "expected Test2-owned completion rows");
    assert!(!labels.contains(&"ok"));
    assert!(labels.contains(&"is"), "other reviewed V0 defaults remain available");
}
