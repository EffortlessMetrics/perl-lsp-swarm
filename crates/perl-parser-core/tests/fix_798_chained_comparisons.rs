//! Issue #798: Perl 5.32+ chained comparison operators must parse correctly.
//!
//! Perl 5.32 introduced chained comparisons: `1 < $x < 10` is syntactic sugar
//! for `(1 < $x) && ($x < 10)`. The parser must recognise consecutive relational
//! operators and produce a `ChainedComparison` node rather than left-associating
//! them into nested `Binary` nodes.
//!
//! Acceptance criteria (from issue #798):
//! - `1 < $x < 10` parses without error
//! - `0 <= $n <= 100` parses without error
//! - `1 < $x <= 10` (mixed operators) parses without error
//! - Single comparisons `$x < 10` remain as `Binary` nodes (no regression)

mod cpan_test_helpers;
use cpan_test_helpers::*;

fn collect_kinds(node: &perl_parser_core::Node, out: &mut Vec<&'static str>) {
    out.push(node.kind.kind_name());
    for child in node.children() {
        collect_kinds(child, out);
    }
}

fn kinds(source: &str) -> Vec<&'static str> {
    let ast = parse(source);
    let mut out = Vec::new();
    collect_kinds(&ast, &mut out);
    out
}

// ── Acceptance: chains parse cleanly ────────────────────────────────────────

#[test]
fn test_two_less_than_chain_parses_clean() {
    assert_clean_parse("if (1 < $x < 10) { 1; }");
}

#[test]
fn test_two_less_equal_chain_parses_clean() {
    assert_clean_parse("if (0 <= $n <= 100) { 1; }");
}

#[test]
fn test_mixed_less_less_equal_chain_parses_clean() {
    assert_clean_parse("if (1 < $x <= 10) { 1; }");
}

#[test]
fn test_three_operator_chain_parses_clean() {
    assert_clean_parse("my $ok = 0 < $a < $b < 100;");
}

#[test]
fn test_greater_than_chain_parses_clean() {
    assert_clean_parse("if ($a > $b > 0) { 1; }");
}

#[test]
fn test_chain_in_expression_parses_clean() {
    assert_clean_parse("my $result = 1 < $x < 10;");
}

#[test]
fn test_chain_with_variable_bounds_parses_clean() {
    assert_clean_parse("if ($lo < $x < $hi) { 1; }");
}

// ── ChainedComparison node kind produced ────────────────────────────────────

#[test]
fn test_two_less_than_produces_chained_comparison_node() {
    let ks = kinds("my $r = 1 < $x < 10;");
    assert!(
        ks.contains(&"ChainedComparison"),
        "expected ChainedComparison NodeKind in `1 < $x < 10`, got: {ks:?}"
    );
}

#[test]
fn test_two_less_equal_produces_chained_comparison_node() {
    let ks = kinds("my $r = 0 <= $n <= 100;");
    assert!(
        ks.contains(&"ChainedComparison"),
        "expected ChainedComparison NodeKind in `0 <= $n <= 100`, got: {ks:?}"
    );
}

// ── Regression: single comparisons stay as Binary ───────────────────────────

#[test]
fn test_single_less_stays_binary() {
    let ks = kinds("my $r = $x < 10;");
    assert!(ks.contains(&"Binary"), "expected Binary NodeKind for `$x < 10`, got: {ks:?}");
    assert!(
        !ks.contains(&"ChainedComparison"),
        "unexpected ChainedComparison for single comparison `$x < 10`"
    );
}

#[test]
fn test_single_less_equal_stays_binary() {
    let ks = kinds("my $r = $x <= 10;");
    assert!(ks.contains(&"Binary"), "expected Binary for `$x <= 10`, got: {ks:?}");
    assert!(!ks.contains(&"ChainedComparison"), "unexpected ChainedComparison for `$x <= 10`");
}

#[test]
fn test_single_greater_stays_binary() {
    let ks = kinds("my $r = $x > 0;");
    assert!(ks.contains(&"Binary"), "expected Binary for `$x > 0`, got: {ks:?}");
    assert!(!ks.contains(&"ChainedComparison"), "unexpected ChainedComparison for `$x > 0`");
}

#[test]
fn test_single_equality_stays_binary() {
    let ks = kinds("my $r = $x == 42;");
    assert!(ks.contains(&"Binary"), "expected Binary for `$x == 42`, got: {ks:?}");
    assert!(!ks.contains(&"ChainedComparison"), "unexpected ChainedComparison for `$x == 42`");
}

// ── Regression: isa is never chained ─────────────────────────────────────────

#[test]
fn test_isa_stays_binary() {
    let ks = kinds(r#"my $r = $x isa Foo;"#);
    assert!(ks.contains(&"Binary"), "expected Binary for `$x isa Foo`, got: {ks:?}");
    assert!(!ks.contains(&"ChainedComparison"), "unexpected ChainedComparison for isa");
}

// ── Regression: other expression forms unaffected ───────────────────────────

#[test]
fn test_addition_unaffected() {
    assert_clean_parse("my $r = $a + $b + $c;");
}

#[test]
fn test_equality_operators_unaffected() {
    assert_clean_parse("my $ok = $x == 1;");
    assert_clean_parse("my $ok = $x != 0;");
}

#[test]
fn test_complex_condition_with_chain() {
    assert_clean_parse("if (defined $x && 0 < $x < 100) { print $x; }");
}

// ── Regression: chaining with parens still works ─────────────────────────────

#[test]
fn test_parenthesised_comparison_not_chained() {
    let ks = kinds("my $r = (1 < $x) < 10;");
    // The outer < has a grouped sub-expression on the left — this is NOT a chain.
    assert_clean_parse("my $r = (1 < $x) < 10;");
    // The outer comparison is still Binary since the left is a parenthesised group.
    assert!(
        !ks.iter().any(|&k| k == "ChainedComparison"),
        "parenthesised `(1 < $x) < 10` should not become ChainedComparison, got: {ks:?}"
    );
}

#[test]
fn test_word_relational_chain_produces_chained_comparison_node() {
    let ks = kinds("my $r = $a lt $b le $c;");
    assert!(
        ks.contains(&"ChainedComparison"),
        "expected ChainedComparison for `$a lt $b le $c`, got: {ks:?}"
    );
}

#[test]
fn test_isa_followed_by_relational_parses_clean() {
    assert_clean_parse("my $r = $x isa Foo < 10;");
}

#[test]
fn test_spaceship_and_less_do_not_chain() {
    let ks = kinds("my $r = 1 <=> $x < 10;");
    assert!(
        !ks.contains(&"ChainedComparison"),
        "cross-precedence `<=>` and `<` must not chain, got: {ks:?}"
    );
}

#[test]
fn test_standalone_spaceship_parses_clean() {
    assert_clean_parse("my $r = $a <=> $b;");
}

#[test]
fn test_standalone_cmp_parses_clean() {
    assert_clean_parse("my $r = $a cmp $b;");
}
