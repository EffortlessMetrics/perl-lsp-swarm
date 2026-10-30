//! Production reach for digit-led package segments after `::` (#16640).
//!
//! Lexer-owned: after `::`, `2022_KR` is a name segment. This file pins the
//! parser consumer (`package` / `use`) so a lexer-only token split cannot
//! silently regress the shipped `Encode::KR::2022_KR` form.
//!
//! Oracle: perl 5.38+ `-ce` on this host accepts the clean cases and rejects
//! a digit-led first segment and `package Foo::1.2`.

mod cpan_test_helpers;
use cpan_test_helpers::*;

fn package_name(source: &str) -> Option<String> {
    let ast = parse(source);
    match &ast.kind {
        perl_parser_core::NodeKind::Program { statements } => {
            for stmt in statements {
                let node = match &stmt.kind {
                    perl_parser_core::NodeKind::ExpressionStatement { expression } => expression,
                    _ => stmt,
                };
                if let perl_parser_core::NodeKind::Package { name, .. } = &node.kind {
                    return Some(name.clone());
                }
            }
            None
        }
        _ => None,
    }
}

#[test]
fn package_encode_kr_2022_kr_parses_as_that_name() {
    let source = "package Encode::KR::2022_KR;";
    assert_clean_parse(source);
    assert_eq!(package_name(source), Some("Encode::KR::2022_KR".to_string()));
}

#[test]
fn package_leading_colon_colon_digit_segment_parses() {
    let source = "package ::2022_KR;";
    assert_clean_parse(source);
    assert_eq!(package_name(source), Some("::2022_KR".to_string()));
}

#[test]
fn package_pure_digit_segment_and_following_letter_segment() {
    assert_clean_parse("package Foo::1;");
    assert_eq!(package_name("package Foo::1;"), Some("Foo::1".to_string()));
    assert_clean_parse("package Foo::2022_KR::Bar;");
    assert_eq!(package_name("package Foo::2022_KR::Bar;"), Some("Foo::2022_KR::Bar".to_string()));
}

#[test]
fn use_digit_led_module_name_parses() {
    assert_clean_parse("use Encode::KR::2022_KR;");
}

#[test]
fn package_version_after_letter_name_is_not_swallowed() {
    let source = "package Foo::Bar 1.23;";
    assert_clean_parse(source);
    assert_eq!(package_name(source), Some("Foo::Bar".to_string()));
    assert_eq!(top_level_kinds(&parse(source)), vec!["Package"]);
}

#[test]
fn digit_led_first_segment_is_not_a_package_name() {
    // perl -ce 'package 2022_KR;' → Invalid version format / syntax error.
    // A wrong implementation that admits any digit-led identifier would
    // accept this as a name.
    let source = "package 2022_KR;";
    let name = package_name(source);
    assert!(
        name.as_deref().is_none_or(|name| !name.contains("2022")),
        "first package segment must not become a digit-led identifier, got {name:?}"
    );
}

#[test]
fn combining_mark_after_colon_colon_is_not_a_package_name() {
    // perl -Mutf8 -ce 'package Foo::́Bar;' → Unrecognized character.
    let source = "package Foo::\u{0301}Bar;";
    let name = package_name(source);
    assert!(
        name.as_deref().is_none_or(|name| !name.contains('\u{0301}')),
        "combining mark after :: must not become a package name, got {name:?}"
    );
}

#[test]
fn dotted_versionish_tail_is_not_folded_into_the_package_name() {
    // perl -ce 'package Foo::1.2;' → Invalid version format (0 before decimal
    // required). The name stops at Foo::1; `.2` must not join the name.
    let source = "package Foo::1.2;";
    assert_eq!(
        package_name(source).as_deref(),
        Some("Foo::1"),
        "dot-tail after a digit segment must remain outside the package name"
    );
}
