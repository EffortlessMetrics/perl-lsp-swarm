//! Direct `use Test2::Tools::Target ...;` generated helper facts.

use perl_lsp_rs_core::providers::{
    completion::{CompletionItem, CompletionProvider},
    testing::test2::{Test2Facts, resolve_import},
};
use perl_parser::Parser;
use std::io;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn complete(source_with_cursor: &str) -> Vec<CompletionItem> {
    let position = source_with_cursor.find('|').unwrap_or(source_with_cursor.len());
    let source = source_with_cursor.replacen('|', "", 1);
    let ast = Parser::new(source.as_str()).parse_with_recovery().ast;
    let provider = CompletionProvider::new_with_index_and_source(&ast, source.as_str(), None);
    provider.get_completions(source.as_str(), position)
}

fn has_test2_completion(items: &[CompletionItem], label: &str) -> bool {
    items
        .iter()
        .any(|item| item.label == label && item.detail.as_deref() == Some("Test2 imported symbol"))
}

const TARGET: &str = "Test2::Tools::Target";

#[test]
fn scalar_target_installs_class_not_package() -> TestResult {
    let resolved = resolve_import(TARGET, "'Some::Package'")
        .ok_or_else(|| io::Error::other("Target must be recognized"))?;
    assert!(resolved.symbols.contains("CLASS"));
    assert!(!resolved.symbols.contains("Some"));
    assert!(!resolved.symbols.contains("Package"));
    Ok(())
}

#[test]
fn fatcomma_target_installs_named_helper_only() -> TestResult {
    let resolved = resolve_import(TARGET, "service => 'Some::Service'")
        .ok_or_else(|| io::Error::other("Target must be recognized"))?;
    assert!(resolved.symbols.contains("service"));
    assert!(!resolved.symbols.contains("Some"));
    assert!(!resolved.symbols.contains("Service"));
    Ok(())
}

#[test]
fn hashref_target_installs_each_key_once() -> TestResult {
    let resolved = resolve_import(TARGET, "{ reader => 'Some::Reader', writer => 'Some::Writer' }")
        .ok_or_else(|| io::Error::other("Target must be recognized"))?;
    for key in ["reader", "writer"] {
        assert!(resolved.symbols.contains(key), "{key} missing");
    }
    for leaked in ["Some", "Reader", "Writer"] {
        assert!(!resolved.symbols.contains(leaked), "{leaked} leaked");
    }
    Ok(())
}

#[test]
fn bare_target_import_emits_no_helpers() -> TestResult {
    let resolved =
        resolve_import(TARGET, "").ok_or_else(|| io::Error::other("Target must be recognized"))?;
    assert!(resolved.symbols.is_empty());
    Ok(())
}

#[test]
fn dynamic_target_value_emits_no_guessed_helper() -> TestResult {
    let resolved = resolve_import(TARGET, "\"$ENV{TARGET}\"")
        .ok_or_else(|| io::Error::other("Target must be recognized"))?;
    assert!(!resolved.symbols.contains("CLASS"));
    assert!(!resolved.symbols.contains("ENV"));
    Ok(())
}

#[test]
fn similar_tool_module_does_not_inherit_target_generation() -> TestResult {
    let resolved = resolve_import("Test2::Tools::Unknown", "'Some::Package'")
        .ok_or_else(|| io::Error::other("unknown tool must still be a Test2 module"))?;
    assert!(!resolved.symbols.contains("CLASS"));
    Ok(())
}

#[test]
fn direct_target_helpers_reach_live_completion() {
    let class = complete("use Test2::Tools::Target 'Some::Package';\nC|");
    assert!(has_test2_completion(&class, "CLASS"));

    let pkg = complete("use Test2::Tools::Target 'Some::Package';\nS|");
    assert!(
        !has_test2_completion(&pkg, "Some"),
        "package atoms must not become completion candidates"
    );

    let named = complete("use Test2::Tools::Target service => 'Some::Service';\ns|");
    assert!(has_test2_completion(&named, "service"));
}

#[test]
fn from_source_aggregates_direct_target_helpers() -> TestResult {
    let source = r#"
use Test2::Tools::Target 'Some::Package';
use Test2::Tools::Target service => 'Some::Service';
"#;
    let facts = Test2Facts::from_source(source);
    assert!(facts.is_imported("CLASS"));
    assert!(facts.is_imported("service"));
    assert!(!facts.is_imported("Some"));
    Ok(())
}
