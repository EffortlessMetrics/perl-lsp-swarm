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
    for target in ["0", "undef", "''", "\"\"", "q{}", "q{0}", "qq{}"] {
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
fn quote_like_targets_fail_closed_without_leaking_delimiters() -> TestResult {
    for target in ["q{}", "q{0}", "qq{}"] {
        let resolved = resolve_import("Test2::V0", &format!("-target => {target}, ok"))
            .ok_or_else(|| io::Error::other("Test2::V0 must be recognized"))?;

        assert!(resolved.symbols.contains("ok"));
        assert!(!resolved.symbols.contains("CLASS"));
        assert!(!resolved.symbols.contains("q"));
        assert!(!resolved.symbols.contains("qq"));
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
    for target in
        ["$target", "\"$ENV{TARGET}\"", "0.0", "0e0", "+0", "-0", "00", "-0.0", "0e+0", "0x0"]
    {
        let resolved = resolve_import("Test2::V0", &format!("-target => {target}"))
            .ok_or_else(|| io::Error::other("Test2::V0 must be recognized"))?;

        assert!(!resolved.symbols.contains("CLASS"), "target {target:?} is not proven truthy");
    }
    Ok(())
}

#[test]
fn dynamic_dereference_targets_are_consumed_as_one_expression() -> TestResult {
    for target in ["$ENV{TARGET}", "$ENV{TARGET} + Foo", "+ $ENV{TARGET}"] {
        let resolved = resolve_import("Test2::V0", &format!("-target => {target}, ok"))
            .ok_or_else(|| io::Error::other("Test2::V0 must be recognized"))?;
        assert!(resolved.symbols.contains("ok"), "ok missing for {target:?}");
        for leaked in ["CLASS", "ENV", "TARGET", "Foo"] {
            assert!(!resolved.symbols.contains(leaked), "{leaked} leaked for {target:?}");
        }
    }
    Ok(())
}

#[test]
fn dynamic_dereference_hash_values_do_not_leak_suffix_atoms() -> TestResult {
    let resolved = resolve_import(
        "Test2::V0",
        "-target => { pkg => $ENV{TARGET} + Foo, other => 'Gadget' }, ok",
    )
    .ok_or_else(|| io::Error::other("Test2::V0 must be recognized"))?;
    for expected in ["pkg", "other", "ok"] {
        assert!(resolved.symbols.contains(expected), "{expected} missing");
    }
    for leaked in ["CLASS", "ENV", "TARGET", "Foo", "Gadget"] {
        assert!(!resolved.symbols.contains(leaked), "{leaked} leaked");
    }
    Ok(())
}

#[test]
fn chained_dynamic_dereferences_do_not_close_target_hash() -> TestResult {
    let resolved = resolve_import(
        "Test2::V0",
        "-target => { pkg => $ENV{TARGET}->{other}, next => 'Gadget' }, ok",
    )
    .ok_or_else(|| io::Error::other("Test2::V0 must be recognized"))?;

    for expected in ["pkg", "next", "ok"] {
        assert!(resolved.symbols.contains(expected), "{expected} missing");
    }
    for leaked in ["CLASS", "ENV", "TARGET", "other", "Gadget"] {
        assert!(!resolved.symbols.contains(leaked), "{leaked} leaked");
    }
    Ok(())
}

#[test]
fn attached_parenthesized_scalar_targets_generate_class() -> TestResult {
    let resolved = resolve_import("Test2::V0", "-target => ('Foo'), ok")
        .ok_or_else(|| io::Error::other("Test2::V0 must be recognized"))?;

    assert!(resolved.symbols.contains("CLASS"));
    assert!(resolved.symbols.contains("ok"));
    assert!(!resolved.symbols.contains("Foo"));
    Ok(())
}

#[test]
fn quoted_truthy_strings_are_distinct_from_unquoted_false_literals() -> TestResult {
    for target in ["'undef'", "\"0.0\"", "'package_name'"] {
        let resolved = resolve_import("Test2::V0", &format!("-target => {target}"))
            .ok_or_else(|| io::Error::other("Test2::V0 must be recognized"))?;

        assert!(
            resolved.symbols.contains("CLASS"),
            "quoted non-empty target {target:?} must install CLASS"
        );
    }

    for target in ["undef", "0.0"] {
        let resolved = resolve_import("Test2::V0", &format!("-target => {target}"))
            .ok_or_else(|| io::Error::other("Test2::V0 must be recognized"))?;

        assert!(
            !resolved.symbols.contains("CLASS"),
            "unquoted false target {target:?} must not install CLASS"
        );
    }
    Ok(())
}

#[test]
fn qualified_quoted_scalar_targets_require_literal_proof() -> TestResult {
    let qualified = resolve_import("Test2::V0", "-target => 'Foo::Bar'")
        .ok_or_else(|| io::Error::other("Test2::V0 must be recognized"))?;
    assert!(qualified.symbols.contains("CLASS"));
    assert!(!qualified.symbols.contains("Foo"));
    assert!(!qualified.symbols.contains("Bar"));

    let interpolated = resolve_import("Test2::V0", "-target => \"$ENV{TARGET}\"")
        .ok_or_else(|| io::Error::other("Test2::V0 must be recognized"))?;
    assert!(!interpolated.symbols.contains("CLASS"));
    assert!(!interpolated.symbols.contains("ENV"));
    assert!(!interpolated.symbols.contains("TARGET"));
    Ok(())
}

#[test]
fn separated_unary_plus_targets_consume_their_operand() -> TestResult {
    for target in ["+ 'Foo'", "+ + 'Foo'"] {
        let resolved = resolve_import("Test2::V0", &format!("-target => {target}, ok"))
            .ok_or_else(|| io::Error::other("Test2::V0 must be recognized"))?;
        assert!(resolved.symbols.contains("ok"), "ok missing for {target:?}");
        assert!(!resolved.symbols.contains("CLASS"), "CLASS leaked for {target:?}");
        assert!(!resolved.symbols.contains("Foo"), "Foo leaked for {target:?}");
    }
    Ok(())
}

#[test]
fn whitespace_separated_hash_values_do_not_shift_pairing() -> TestResult {
    let resolved =
        resolve_import("Test2::V0", "-target => { pkg => uc 'Widget', other => 'Gadget' }, ok")
            .ok_or_else(|| io::Error::other("Test2::V0 must be recognized"))?;
    for expected in ["pkg", "other", "ok"] {
        assert!(resolved.symbols.contains(expected), "{expected} missing");
    }
    for leaked in ["CLASS", "uc", "Widget", "Gadget"] {
        assert!(!resolved.symbols.contains(leaked), "{leaked} leaked");
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
fn quote_like_hash_values_remain_opaque_and_do_not_leak_imports() -> TestResult {
    for value in [
        "q{Widget}",
        "qq{Widget}",
        "qx{Widget}",
        "m{Widget}",
        "s{Widget}{Other}",
        "tr{Widget}{Other}",
        "y{Widget}{Other}",
        "qw{Widget}",
    ] {
        let args = format!("-target => {{ pkg => {value}, other => 'Gadget' }}, ok");
        let resolved = resolve_import("Test2::V0", &args)
            .ok_or_else(|| io::Error::other("Test2::V0 must be recognized"))?;

        assert!(resolved.symbols.contains("pkg"), "value {value:?}");
        assert!(resolved.symbols.contains("other"), "value {value:?}");
        assert!(resolved.symbols.contains("ok"), "value {value:?}");
        for leaked in ["Widget", "Other", "Gadget"] {
            assert!(!resolved.symbols.contains(leaked), "{leaked} leaked from {value:?}");
        }
    }
    Ok(())
}

#[test]
fn non_brace_quote_like_hash_values_remain_opaque() -> TestResult {
    for value in [
        "q#Foo,Bar#",
        "qq/Foo,Bar/",
        "m/Foo,Bar/",
        "s/Foo,Bar/Baz/",
        "tr/Foo,Bar/Baz/",
        "y/Foo,Bar/Baz/",
        "qw(Foo,Bar)",
        // Non-bracketing payloads whose content would break hash-pair parity if
        // the quote-like value were split into atoms: embedded whitespace and
        // an embedded fat comma.
        "q/Widget Other/",
        "q/Foo=>Bar/",
    ] {
        let args = format!("-target => {{ pkg => {value}, other => 'Gadget' }}, ok");
        let resolved = resolve_import("Test2::V0", &args)
            .ok_or_else(|| io::Error::other("Test2::V0 must be recognized"))?;
        assert!(resolved.symbols.contains("pkg"), "value {value:?}");
        assert!(resolved.symbols.contains("other"), "value {value:?}");
        assert!(resolved.symbols.contains("ok"), "value {value:?}");
        for leaked in ["Foo", "Bar", "Baz", "Gadget", "Widget", "Other"] {
            assert!(!resolved.symbols.contains(leaked), "{leaked} leaked from {value:?}");
        }
    }
    Ok(())
}

#[test]
fn non_brace_quote_like_payload_braces_remain_opaque() -> TestResult {
    for value in [
        "q/Foo {Widget}/",
        "qq/Foo {Widget}/",
        "qx/Foo {Widget}/",
        "m/Foo {Widget}/",
        "s/Foo {Widget}/Bar {Other}/",
        "tr/Foo {Widget}/Bar {Other}/",
        "y/Foo {Widget}/Bar {Other}/",
        "qw/Foo {Widget}/",
    ] {
        let args = format!("-target => {{ pkg => {value}, other => 'Gadget' }}, ok");
        let resolved = resolve_import("Test2::V0", &args)
            .ok_or_else(|| io::Error::other("Test2::V0 must be recognized"))?;
        assert!(resolved.symbols.contains("pkg"), "value {value:?}");
        assert!(resolved.symbols.contains("other"), "value {value:?}");
        assert!(resolved.symbols.contains("ok"), "value {value:?}");
        for leaked in ["Foo", "Widget", "Bar", "Other", "Gadget"] {
            assert!(!resolved.symbols.contains(leaked), "{leaked} leaked from {value:?}");
        }
    }
    Ok(())
}

#[test]
fn nested_hash_target_restores_outer_key_value_parity() -> TestResult {
    let resolved = resolve_import(
        "Test2::V0",
        "-target => { pkg => { nested => 'Widget' }, other => 'Gadget' }, ok",
    )
    .ok_or_else(|| io::Error::other("Test2::V0 must be recognized"))?;
    assert!(resolved.symbols.contains("pkg"));
    assert!(resolved.symbols.contains("other"));
    assert!(resolved.symbols.contains("ok"));
    for leaked in ["nested", "Widget", "Gadget"] {
        assert!(!resolved.symbols.contains(leaked), "{leaked} leaked");
    }
    Ok(())
}

#[test]
fn expression_hash_values_preserve_key_value_parity() -> TestResult {
    // A unary multi-atom expression value (`uc 'Widget'`) must not shift the
    // alternating key/value scan: the real `other` key stays a helper and the
    // expression's operand does not leak as one.
    for value in ["uc 'Widget'", "scalar 'Widget'"] {
        let args = format!("-target => {{ pkg => {value}, other => 'Gadget' }}, ok");
        let resolved = resolve_import("Test2::V0", &args)
            .ok_or_else(|| io::Error::other("Test2::V0 must be recognized"))?;
        assert!(resolved.symbols.contains("pkg"), "value {value:?}");
        assert!(resolved.symbols.contains("other"), "value {value:?}");
        assert!(resolved.symbols.contains("ok"), "value {value:?}");
        for leaked in ["Widget", "Gadget", "uc", "scalar"] {
            assert!(!resolved.symbols.contains(leaked), "{leaked} leaked from {value:?}");
        }
    }
    Ok(())
}

#[test]
fn list_operator_value_hash_targets_fail_closed() -> TestResult {
    // A list operator's argument list carries its own top-level comma, which
    // is indistinguishable from a hash-pair separator at the atom level
    // (`join '-', 'Widget'` would leak Widget and drop other if pairing were
    // guessed). The whole target hash fails closed: no helpers are invented,
    // while the explicit export after the target stays visible. See #13305.
    for value in ["join '-', 'Widget'", "split ',', 'Widget'", "map { uc } 'Widget'"] {
        let args = format!("-target => {{ pkg => {value}, other => 'Gadget' }}, ok");
        let resolved = resolve_import("Test2::V0", &args)
            .ok_or_else(|| io::Error::other("Test2::V0 must be recognized"))?;
        assert!(resolved.symbols.contains("ok"), "value {value:?}");
        for not_invented in ["pkg", "other", "Widget", "Gadget", "CLASS"] {
            assert!(
                !resolved.symbols.contains(not_invented),
                "{not_invented} must not be invented from list-operator value {value:?}"
            );
        }
    }
    Ok(())
}

#[test]
fn quoted_hash_delimiters_do_not_close_or_leak_target_values() -> TestResult {
    let resolved =
        resolve_import("Test2::V0", "-target => { pkg => 'Widget}', other => 'Gadget' }, ok")
            .ok_or_else(|| io::Error::other("Test2::V0 must be recognized"))?;

    assert!(resolved.symbols.contains("pkg"));
    assert!(resolved.symbols.contains("other"));
    assert!(resolved.symbols.contains("ok"));
    assert!(!resolved.symbols.contains("Widget"));
    assert!(!resolved.symbols.contains("Gadget"));
    assert!(!resolved.symbols.contains("leaked"));
    Ok(())
}

#[test]
fn quoted_commas_and_escaped_delimiters_remain_one_target_atom() -> TestResult {
    for target in ["'Foo,Bar'", "'Foo\\'Bar'"] {
        let resolved = resolve_import("Test2::V0", &format!("-target => {target}, ok"))
            .ok_or_else(|| io::Error::other("Test2::V0 must be recognized"))?;
        assert!(resolved.symbols.contains("CLASS"), "target {target:?}");
        assert!(resolved.symbols.contains("ok"), "target {target:?}");
        assert!(!resolved.symbols.contains("Foo"));
        assert!(!resolved.symbols.contains("Bar"));
        assert!(!resolved.symbols.contains("is"));
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
fn unsupported_parenthesized_target_expressions_do_not_generate_class() -> TestResult {
    for target in [
        "('Foo', 'Bar')",
        "($target)",
        "(foo())",
        "(( 'Foo' ))",
        "({ pkg => 'Widget' })",
        "('Foo' + $suffix)",
        "('Foo'",
        "(foo())",
    ] {
        let resolved = resolve_import("Test2::V0", &format!("-target => {target}, ok"))
            .ok_or_else(|| io::Error::other("Test2::V0 must be recognized"))?;

        assert!(
            !resolved.symbols.contains("CLASS"),
            "unsupported parenthesized target {target:?} must remain unproven"
        );
        assert!(resolved.symbols.contains("ok"));
    }
    Ok(())
}

#[test]
fn comma_inside_dynamic_call_does_not_make_target_truthy() -> TestResult {
    let resolved = resolve_import("Test2::V0", "-target => foo(1, 2), ok")
        .ok_or_else(|| io::Error::other("Test2::V0 must be recognized"))?;

    assert!(!resolved.symbols.contains("CLASS"));
    assert!(resolved.symbols.contains("ok"));
    assert!(!resolved.symbols.contains("foo"));
    assert!(!resolved.symbols.contains("1"));
    assert!(!resolved.symbols.contains("2"));
    assert!(!resolved.symbols.contains("is"));
    Ok(())
}

#[test]
fn whitespace_separated_call_like_targets_fail_closed() -> TestResult {
    let resolved = resolve_import("Test2::V0", "-target => foo (), ok")
        .ok_or_else(|| io::Error::other("Test2::V0 must be recognized"))?;

    assert!(resolved.symbols.contains("ok"));
    assert!(!resolved.symbols.contains("CLASS"));
    assert!(!resolved.symbols.contains("foo"));
    assert!(!resolved.symbols.contains("is"));
    Ok(())
}

#[test]
fn compact_call_like_targets_fail_closed() -> TestResult {
    for target in ["foo()", "(foo())"] {
        let resolved = resolve_import("Test2::V0", &format!("-target => {target}, ok"))
            .ok_or_else(|| io::Error::other("Test2::V0 must be recognized"))?;

        assert!(resolved.symbols.contains("ok"));
        for leaked in ["CLASS", "foo", "1", "2"] {
            assert!(!resolved.symbols.contains(leaked), "{leaked} leaked from {target}");
        }
        assert!(!resolved.symbols.contains("is"));
    }
    Ok(())
}

#[test]
fn attached_call_hashref_arguments_do_not_leak_exports() -> TestResult {
    let resolved = resolve_import("Test2::V0", "-target => foo({ leaked => 'Widget' }), ok")
        .ok_or_else(|| io::Error::other("Test2::V0 must be recognized"))?;

    assert!(resolved.symbols.contains("ok"));
    for leaked in ["CLASS", "foo", "leaked", "Widget", "is"] {
        assert!(!resolved.symbols.contains(leaked), "{leaked} leaked");
    }
    Ok(())
}

#[test]
fn parenthesized_hash_targets_preserve_key_value_parity() -> TestResult {
    let resolved =
        resolve_import("Test2::V0", "-target => ( { first => 'One', second => 'Two' } ), ok")
            .ok_or_else(|| io::Error::other("Test2::V0 must be recognized"))?;

    assert!(resolved.symbols.contains("first"));
    assert!(resolved.symbols.contains("second"));
    assert!(resolved.symbols.contains("ok"));
    for leaked in ["One", "Two", "is", "CLASS"] {
        assert!(!resolved.symbols.contains(leaked), "{leaked} leaked from parenthesized hash");
    }
    Ok(())
}

#[test]
fn compact_parenthesized_hash_targets_preserve_key_value_parity() -> TestResult {
    let resolved = resolve_import("Test2::V0", "-target => ({first=>'One',second=>'Two'}), ok")
        .ok_or_else(|| io::Error::other("Test2::V0 must be recognized"))?;

    for expected in ["first", "second", "ok"] {
        assert!(resolved.symbols.contains(expected), "{expected} missing");
    }
    for leaked in ["One", "Two", "is", "CLASS"] {
        assert!(!resolved.symbols.contains(leaked), "{leaked} leaked");
    }
    Ok(())
}

#[test]
fn quote_like_modifiers_and_escaped_delimiters_preserve_hash_parity() -> TestResult {
    for value in [
        "m{Widget}i",
        "s{Widget}{Other}g",
        "m{Widget\\}Other}i",
        "s{Widget\\}Other}{Gadget\\}Value}g",
    ] {
        let args = format!("-target => {{ first => {value}, second => 'Two' }}, ok");
        let resolved = resolve_import("Test2::V0", &args)
            .ok_or_else(|| io::Error::other("Test2::V0 must be recognized"))?;

        assert!(resolved.symbols.contains("first"), "value {value:?}");
        assert!(resolved.symbols.contains("second"), "value {value:?}");
        assert!(resolved.symbols.contains("ok"), "value {value:?}");
        for leaked in ["Widget", "Other", "Gadget", "Two", "i", "g"] {
            assert!(!resolved.symbols.contains(leaked), "{leaked} leaked from {value:?}");
        }
    }
    Ok(())
}

#[test]
fn malformed_target_structures_fail_closed_without_leaking_atoms() -> TestResult {
    let malformed_hash =
        resolve_import("Test2::V0", "-target => { leaked_helper => 'Widget', trailing_name")
            .ok_or_else(|| io::Error::other("Test2::V0 must be recognized"))?;
    assert!(!malformed_hash.symbols.contains("leaked_helper"));
    assert!(!malformed_hash.symbols.contains("Widget"));
    assert!(!malformed_hash.symbols.contains("trailing_name"));
    assert!(malformed_hash.symbols.contains("ok"));
    assert!(malformed_hash.symbols.contains("is"));

    let malformed_parenthesized =
        resolve_import("Test2::V0", "-target => ('LeakedFunction', trailing_name")
            .ok_or_else(|| io::Error::other("Test2::V0 must be recognized"))?;
    assert!(!malformed_parenthesized.symbols.contains("CLASS"));
    assert!(!malformed_parenthesized.symbols.contains("LeakedFunction"));
    assert!(!malformed_parenthesized.symbols.contains("trailing_name"));
    assert!(malformed_parenthesized.symbols.contains("ok"));
    assert!(malformed_parenthesized.symbols.contains("is"));
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

    let qualified = complete("use Test2::V0 -target => 'Foo::Bar';\nC|");
    assert!(has_test2_completion(&qualified, "CLASS"));
    let dynamic = complete("use Test2::V0 -target => \"$ENV{TARGET}\";\nC|");
    assert!(!has_test2_completion(&dynamic, "CLASS"));

    let dynamic_dereference = complete("use Test2::V0 -target => $ENV{TARGET}, ok;\no|");
    assert!(has_test2_completion(&dynamic_dereference, "ok"));
    for leaked in ["CLASS", "ENV", "TARGET"] {
        assert!(!has_test2_completion(&dynamic_dereference, leaked), "{leaked} leaked");
    }

    for target in ["$ENV{TARGET} + Foo", "+ $ENV{TARGET}"] {
        let source = format!("use Test2::V0 -target => {target}, ok;\no|");
        let completions = complete(&source);
        assert!(has_test2_completion(&completions, "ok"), "ok missing: {source:?}");
        for leaked in ["CLASS", "ENV", "TARGET", "Foo"] {
            assert!(!has_test2_completion(&completions, leaked), "{leaked} leaked: {source:?}");
        }
    }

    let separated_plus = complete("use Test2::V0 -target => + 'Foo', ok;\no|");
    assert!(has_test2_completion(&separated_plus, "ok"));
    assert!(!has_test2_completion(&separated_plus, "Foo"));

    let nested_plus = complete("use Test2::V0 -target => + + 'Foo', ok;\no|");
    assert!(has_test2_completion(&nested_plus, "ok"));
    assert!(!has_test2_completion(&nested_plus, "Foo"));

    let v0_hash = complete("use Test2::V0 -target => { pkg => 'Widget', other => 'Gadget' };\np|");
    assert!(has_test2_completion(&v0_hash, "plan"));
    assert!(has_test2_completion(&v0_hash, "pkg"));

    let expression_hash =
        complete("use Test2::V0 -target => { pkg => uc 'Widget', other => 'Gadget' };\n|");
    assert!(has_test2_completion(&expression_hash, "pkg"));
    assert!(has_test2_completion(&expression_hash, "other"));
    for leaked in ["CLASS", "uc", "Widget", "Gadget"] {
        assert!(!has_test2_completion(&expression_hash, leaked), "{leaked} leaked");
    }

    let dynamic_expression_hash =
        complete("use Test2::V0 -target => { pkg => $ENV{TARGET} + Foo, other => 'Gadget' };\n|");
    assert!(has_test2_completion(&dynamic_expression_hash, "pkg"));
    assert!(has_test2_completion(&dynamic_expression_hash, "other"));
    for leaked in ["CLASS", "ENV", "TARGET", "Foo", "Gadget"] {
        assert!(!has_test2_completion(&dynamic_expression_hash, leaked), "{leaked} leaked");
    }

    let chained_dynamic_hash = complete(
        "use Test2::V0 -target => { pkg => $ENV{TARGET}->{other}, next => 'Gadget' }, ok;\n|",
    );
    for expected in ["pkg", "next", "ok"] {
        assert!(has_test2_completion(&chained_dynamic_hash, expected), "{expected} missing");
    }
    for leaked in ["CLASS", "ENV", "TARGET", "other", "Gadget"] {
        assert!(!has_test2_completion(&chained_dynamic_hash, leaked), "{leaked} leaked");
    }

    let v0_hash_value =
        complete("use Test2::V0 -target => { pkg => 'Widget', other => 'Gadget' };\nW|");
    assert!(
        !has_test2_completion(&v0_hash_value, "Widget"),
        "completion must not project a target package value as a Test2 function"
    );

    for value in [
        "q{Widget}",
        "qq{Widget}",
        "qx{Widget}",
        "m{Widget}",
        "s{Widget}{Other}",
        "tr{Widget}{Other}",
        "y{Widget}{Other}",
        "qw{Widget}",
    ] {
        let source =
            format!("use Test2::V0 -target => {{ pkg => {value}, other => 'Gadget' }};\n|");
        let completions = complete(&source);
        assert!(has_test2_completion(&completions, "pkg"), "value {value:?}");
        assert!(has_test2_completion(&completions, "other"), "value {value:?}");
        for leaked in ["Widget", "Other", "Gadget"] {
            assert!(!has_test2_completion(&completions, leaked), "{leaked} leaked from {value:?}");
        }
    }

    for value in [
        "q/Foo {Widget}/",
        "qq/Foo {Widget}/",
        "qx/Foo {Widget}/",
        "m/Foo {Widget}/",
        "s/Foo {Widget}/Bar {Other}/",
        "tr/Foo {Widget}/Bar {Other}/",
        "y/Foo {Widget}/Bar {Other}/",
        "qw/Foo {Widget}/",
    ] {
        let source =
            format!("use Test2::V0 -target => {{ pkg => {value}, other => 'Gadget' }};\n|");
        let completions = complete(&source);
        assert!(has_test2_completion(&completions, "pkg"), "value {value:?}");
        assert!(has_test2_completion(&completions, "other"), "value {value:?}");
        for leaked in ["Foo", "Widget", "Bar", "Other", "Gadget"] {
            assert!(!has_test2_completion(&completions, leaked), "{leaked} leaked from {value:?}");
        }
    }

    let nested = complete(
        "use Test2::V0 -target => { pkg => { nested => 'Widget' }, other => 'Gadget' };\n|",
    );
    assert!(has_test2_completion(&nested, "pkg"));
    assert!(has_test2_completion(&nested, "other"));
    for leaked in ["nested", "Widget", "Gadget"] {
        assert!(!has_test2_completion(&nested, leaked), "{leaked} leaked from nested hash");
    }

    let v1 = complete("use Test2::V1 -target => 'Foo';\nT|");
    assert!(has_test2_completion(&v1, "T2"));

    let v1_class = complete("use Test2::V1 -target => 'Foo';\nC|");
    assert!(has_test2_completion(&v1_class, "CLASS"));

    for source in [
        "use Test2::V0 -target => q{};\nC|",
        "use Test2::V0 -target => q{0};\nC|",
        "use Test2::V0 -target => qq{};\nC|",
        "use Test2::V0 -target => m{Foo};\nC|",
        "use Test2::V0 -target => s{a}{b};\nC|",
        "use Test2::V0 -target => tr{a}{b};\nC|",
        "use Test2::V0 -target => qx{command};\nC|",
        "use Test2::V0 -target => qw{} , ok;\nC|",
    ] {
        let completions = complete(source);
        assert!(
            !has_test2_completion(&completions, "CLASS"),
            "quote-like target must not synthesize CLASS: {source:?}"
        );
    }

    let truthy = complete("use Test2::V0 -target => 'Foo';\nC|");
    assert!(has_test2_completion(&truthy, "CLASS"));

    let call_like = complete("use Test2::V0 -target => foo ();\nC|");
    assert!(!has_test2_completion(&call_like, "CLASS"));
    let call_like_name = complete("use Test2::V0 -target => foo ();\nf|");
    assert!(!has_test2_completion(&call_like_name, "foo"));

    for source in
        ["use Test2::V0 -target => foo(), ok;\n|", "use Test2::V0 -target => (foo()), ok;\n|"]
    {
        let completions = complete(source);
        assert!(has_test2_completion(&completions, "ok"), "ok missing: {source:?}");
        for leaked in ["CLASS", "foo", "1", "2", "is"] {
            assert!(!has_test2_completion(&completions, leaked), "{leaked} leaked: {source:?}");
        }
    }

    let call_hashref = complete("use Test2::V0 -target => foo({ leaked => 'Widget' }), ok;\n|");
    assert!(has_test2_completion(&call_hashref, "ok"));
    for leaked in ["CLASS", "foo", "leaked", "Widget", "is"] {
        assert!(!has_test2_completion(&call_hashref, leaked), "{leaked} leaked");
    }
}
