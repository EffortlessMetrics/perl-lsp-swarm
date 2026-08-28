//! Regression proof for Test2 `-target` import options.
//!
//! The resolver owns import semantics; completion is the live bridge consumer.

use perl_lsp_rs_core::providers::{
    completion::{CompletionItem, CompletionProvider},
    testing::test2::resolve_import,
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

#[test]
fn scalar_targets_preserve_defaults_and_generate_class() -> TestResult {
    for (module, expected_default) in [("Test2::V0", "ok"), ("Test2::V1", "T2")] {
        let resolved = resolve_import(module, "-target => 'Foo'")
            .ok_or_else(|| io::Error::other(format!("{module} must be recognized")))?;

        assert!(
            resolved.symbols.contains(expected_default),
            "{module} must retain its reviewed default exports"
        );
        assert!(
            resolved.symbols.contains("CLASS"),
            "{module} must expose Test2::Tools::Target's scalar helper"
        );
        assert!(
            !resolved.symbols.contains("Foo"),
            "the target package is not itself an imported function"
        );
    }
    Ok(())
}

#[test]
fn false_scalar_targets_do_not_generate_class() -> TestResult {
    for target in ["0", "undef", "''", "\"\""] {
        let resolved = resolve_import("Test2::V0", &format!("-target => {target}"))
            .ok_or_else(|| io::Error::other("Test2::V0 must be recognized"))?;

        assert!(resolved.symbols.contains("ok"));
        assert!(
            !resolved.symbols.contains("CLASS"),
            "false target {target:?} must not install CLASS"
        );
    }
    Ok(())
}

#[test]
fn empty_scalar_targets_preserve_following_exports() -> TestResult {
    for target in ["''", "\"\""] {
        let resolved = resolve_import("Test2::V0", &format!("-target => {target}, ok"))
            .ok_or_else(|| io::Error::other("Test2::V0 must be recognized"))?;

        assert!(resolved.symbols.contains("ok"));
        assert!(!resolved.symbols.contains("CLASS"));
        assert!(!resolved.symbols.contains("is"));
    }
    Ok(())
}

#[test]
fn dynamic_and_uncertain_numeric_targets_do_not_generate_class() -> TestResult {
    for target in ["$target", "0.0", "0e0", "+0", "-0"] {
        let resolved = resolve_import("Test2::V0", &format!("-target => {target}"))
            .ok_or_else(|| io::Error::other("Test2::V0 must be recognized"))?;

        assert!(!resolved.symbols.contains("CLASS"), "target {target:?} is not proven truthy");
    }
    Ok(())
}

#[test]
fn named_hash_targets_preserve_defaults_and_generate_helpers() -> TestResult {
    let args = "-target => { pkg => 'Widget', other => 'Gadget' }";
    for (module, expected_default) in [("Test2::V0", "is"), ("Test2::V1", "T2")] {
        let resolved = resolve_import(module, args)
            .ok_or_else(|| io::Error::other(format!("{module} must be recognized")))?;

        assert!(resolved.symbols.contains(expected_default));
        assert!(resolved.symbols.contains("pkg"));
        assert!(resolved.symbols.contains("other"));
        assert!(!resolved.symbols.contains("Widget"));
        assert!(!resolved.symbols.contains("Gadget"));
    }
    Ok(())
}

#[test]
fn wrapped_hash_targets_ignore_structural_tokens() -> TestResult {
    for args in [
        "-target => +{ pkg => 'Widget', other => 'Gadget' }",
        "-target => ({ pkg => 'Widget', other => 'Gadget' })",
    ] {
        let resolved = resolve_import("Test2::V0", args)
            .ok_or_else(|| io::Error::other("Test2::V0 must be recognized"))?;

        assert!(resolved.symbols.contains("pkg"));
        assert!(resolved.symbols.contains("other"));
        assert!(!resolved.symbols.contains("Widget"));
        assert!(!resolved.symbols.contains("Gadget"));
    }
    Ok(())
}

#[test]
fn whitespace_separated_hash_wrappers_preserve_helpers() -> TestResult {
    for args in ["-target => ( { pkg => 'Widget' } )", "-target => + { pkg => 'Widget' }"] {
        let resolved = resolve_import("Test2::V0", args)
            .ok_or_else(|| io::Error::other("Test2::V0 must be recognized"))?;

        assert!(resolved.symbols.contains("pkg"));
        assert!(!resolved.symbols.contains("CLASS"));
        assert!(!resolved.symbols.contains("Widget"));
    }
    Ok(())
}

#[test]
fn parenthesized_scalar_targets_consume_only_the_target_value() -> TestResult {
    let truthy = resolve_import("Test2::V0", "-target => ( 'Foo' ), ok")
        .ok_or_else(|| io::Error::other("Test2::V0 must be recognized"))?;
    assert!(truthy.symbols.contains("CLASS"));
    assert!(truthy.symbols.contains("ok"));
    assert!(!truthy.symbols.contains("Foo"));
    assert!(!truthy.symbols.contains("is"));

    for target in ["0", "undef", "''"] {
        let falsey = resolve_import("Test2::V0", &format!("-target => ( {target} ), ok"))
            .ok_or_else(|| io::Error::other("Test2::V0 must be recognized"))?;
        assert!(!falsey.symbols.contains("CLASS"));
        assert!(falsey.symbols.contains("ok"));
        assert!(!falsey.symbols.contains("is"));
    }
    Ok(())
}

#[test]
fn target_helpers_are_not_invented_for_tool_modules() -> TestResult {
    let resolved = resolve_import("Test2::Tools::Compare", "-target => 'Foo'")
        .ok_or_else(|| io::Error::other("Test2::Tools::Compare must be recognized"))?;

    assert!(!resolved.symbols.contains("CLASS"));
    Ok(())
}

#[test]
fn target_consumption_stops_before_an_explicit_import() -> TestResult {
    let resolved = resolve_import("Test2::V0", "-target => 'Foo', ok")
        .ok_or_else(|| io::Error::other("Test2::V0 must be recognized"))?;

    assert!(resolved.symbols.contains("ok"));
    assert!(resolved.symbols.contains("CLASS"));
    assert!(
        !resolved.symbols.contains("is"),
        "the following explicit import must still suppress unselected defaults"
    );
    assert!(!resolved.symbols.contains("Foo"));
    Ok(())
}

#[test]
fn export_exclusions_do_not_remove_target_helpers() -> TestResult {
    let resolved = resolve_import("Test2::V1", "-target => 'Foo', '!CLASS'")
        .ok_or_else(|| io::Error::other("Test2::V1 must be recognized"))?;

    assert!(resolved.symbols.contains("T2"));
    assert!(resolved.symbols.contains("CLASS"));
    Ok(())
}

#[test]
fn target_helpers_reach_live_bundle_completion() {
    let v0 = complete("use Test2::V0 -target => 'Foo';\no|");
    assert!(has_test2_completion(&v0, "ok"));

    let v0_class = complete("use Test2::V0 -target => 'Foo';\nC|");
    assert!(has_test2_completion(&v0_class, "CLASS"));

    let v0_target = complete("use Test2::V0 -target => 'Foo';\nF|");
    assert!(
        !has_test2_completion(&v0_target, "Foo"),
        "completion must not project the target package as a Test2 function"
    );

    let v0_hash = complete("use Test2::V0 -target => { pkg => 'Widget', other => 'Gadget' };\np|");
    assert!(has_test2_completion(&v0_hash, "plan"));
    assert!(has_test2_completion(&v0_hash, "pkg"));

    let v0_hash_value =
        complete("use Test2::V0 -target => { pkg => 'Widget', other => 'Gadget' };\nW|");
    assert!(
        !has_test2_completion(&v0_hash_value, "Widget"),
        "completion must not project a target package value as a Test2 function"
    );

    let v1 = complete("use Test2::V1 -target => 'Foo';\nT|");
    assert!(has_test2_completion(&v1, "T2"));

    let v1_class = complete("use Test2::V1 -target => 'Foo';\nC|");
    assert!(has_test2_completion(&v1_class, "CLASS"));
}
