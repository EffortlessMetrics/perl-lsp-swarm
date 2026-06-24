// Regression tests for issue #1367: parser P0 hang risks — unguarded recursion
// in parse_word_not_expr (precedence.rs) and parse_unary (unary.rs).
//
// Before the fix:
//   - 5000 nested `not` operators → SIGSEGV in parse_word_not_expr
//   - 200  nested `!` operators   → stack overflow in parse_unary
//
// After the fix both sites are wrapped in with_recursion_guard() so deeply
// nested input returns NestingTooDeep instead of crashing.

mod cpan_test_helpers;
use cpan_test_helpers::*;

use perl_parser_core::{ParseError, Parser};

fn fails_gracefully(code: &str) -> bool {
    let mut parser = Parser::new(code);
    let result = parser.parse();
    result.as_ref().err().is_some_and(|e| matches!(e, ParseError::NestingTooDeep { .. }))
        || parser.errors().iter().any(|e| matches!(e, ParseError::NestingTooDeep { .. }))
}

// --- parse_word_not_expr (precedence.rs) ---

#[test]
fn word_not_5000_deep_does_not_sigsegv() {
    // ('not ' x 5000).'1' in Perl — 5000 nested `not` operators.
    // Before fix: SIGSEGV at ~5000 due to unguarded self-recursion.
    let code = "not ".repeat(5000) + "1";
    assert!(
        fails_gracefully(&code),
        "5000-deep `not` chain should fail with NestingTooDeep, not crash"
    );
}

#[test]
fn word_not_depth_130_hits_limit() {
    // 130 levels is just above MAX_RECURSION_DEPTH (128).
    let code = "not ".repeat(130) + "1";
    assert!(fails_gracefully(&code), "130-deep `not` chain should hit the recursion guard");
}

#[test]
fn word_not_depth_128_hits_limit() {
    // Exactly at the limit — the 129th call should trip the guard.
    let code = "not ".repeat(129) + "1";
    assert!(fails_gracefully(&code), "129-deep `not` chain should hit the recursion guard");
}

// --- parse_unary (unary.rs) ---

#[test]
fn bang_200_deep_does_not_sigsegv() {
    // ('!' x 200).'1' — 200 nested `!` operators.
    // Before fix: stack overflow in parse_unary at ~200 levels.
    let code = "!".repeat(200) + "1";
    assert!(
        fails_gracefully(&code),
        "200-deep `!` chain should fail with NestingTooDeep, not crash"
    );
}

#[test]
fn bang_depth_130_hits_limit() {
    let code = "!".repeat(130) + "1";
    assert!(fails_gracefully(&code), "130-deep `!` chain should hit the recursion guard");
}

#[test]
fn unary_minus_depth_hits_limit() {
    // parse_unary recurses for `-` as well — verify the same guard fires.
    // Use 300 dashes: even if the lexer collapses pairs into Decrement tokens
    // (giving 150 recursion levels), that still exceeds MAX_RECURSION_DEPTH=128.
    let code = "-".repeat(300) + "1";
    assert!(fails_gracefully(&code), "300-deep unary-minus chain should hit the recursion guard");
}

#[test]
fn increment_depth_130_hits_limit() {
    // Pre-increment also recurses through parse_unary.
    let code = "++".repeat(130) + "$x";
    assert!(fails_gracefully(&code), "130-deep `++` chain should hit the recursion guard");
}

// --- regression: shallow nesting still parses cleanly ---

#[test]
fn word_not_single_still_parses() {
    assert_clean_parse("not $x");
}

#[test]
fn word_not_three_deep_still_parses() {
    assert_clean_parse("not not not $x");
}

#[test]
fn bang_single_still_parses() {
    assert_clean_parse("!$x");
}

#[test]
fn bang_three_deep_still_parses() {
    assert_clean_parse("!!!$x");
}

#[test]
fn unary_minus_still_parses() {
    assert_clean_parse("-$x");
}

#[test]
fn bang_in_condition_still_parses() {
    assert_clean_parse("if (!$ok) { die; }");
}

#[test]
fn word_not_in_condition_still_parses() {
    assert_clean_parse("die unless not $ok;");
}
