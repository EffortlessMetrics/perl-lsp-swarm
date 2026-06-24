/// Regression tests for parser recursion-guard bypasses.
///
/// These tests verify that `parse_word_not_expr` and `parse_unary` enforce
/// MAX_RECURSION_DEPTH instead of overflowing the call stack (SIGSEGV) on
/// deeply-nested operator inputs.  Before the fix, both functions self-recurse
/// without calling `check_recursion()`, so 5 000 nested `not` or 200 nested `!`
/// bypassed the global depth budget and crashed the process.
use perl_parser_core::Parser;

fn parse_recovers(source: &str) {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    // The tree must be present — we got some result back (no panic).
    // terminated_early or non-empty diagnostics are fine; a crash is not.
    let _ = output;
}

fn has_nesting_err(source: &str) -> bool {
    let mut p = Parser::new(source);
    let out = p.parse_with_recovery();
    out.diagnostics.iter().any(|e| format!("{e:?}").contains("NestingTooDeep"))
}

// ---------------------------------------------------------------------------
// P1: parse_word_not_expr — 5 000 nested `not` previously caused SIGSEGV
// ---------------------------------------------------------------------------

#[test]
fn test_parse_word_not_guard_bypass() {
    // Reproducer from robustness audit: `('not ' x 5000).'1'`
    let src = format!("{}1", "not ".repeat(5_000));
    parse_recovers(&src);
}

#[test]
fn test_parse_word_not_boundary_at_depth() {
    // Small depths must not produce a NestingTooDeep error.
    let src_shallow = format!("{}1", "not ".repeat(5));
    assert!(
        !has_nesting_err(&src_shallow),
        "shallow `not` nesting should not trip the depth guard"
    );

    // Deep nesting must be handled gracefully (no crash, possibly an error node).
    let src_deep = format!("{}1", "not ".repeat(300));
    parse_recovers(&src_deep);
}

// ---------------------------------------------------------------------------
// P2: parse_unary — 200 nested `!` previously bypassed MAX_RECURSION_DEPTH
// ---------------------------------------------------------------------------

#[test]
fn test_parse_unary_not_guard_bypass() {
    // Reproducer from robustness audit: `('!' x 200).'1'`
    let src = format!("{}1", "!".repeat(200));
    parse_recovers(&src);
}

#[test]
fn test_parse_unary_not_depth_boundary() {
    // Depths well below the limit must parse without error.
    let src_shallow = format!("{}1", "!".repeat(10));
    assert!(!has_nesting_err(&src_shallow), "10 nested `!` should not trip the depth guard");

    // 5 000 deeply nested `!` — must not crash.
    let src_very_deep = format!("{}1", "!".repeat(5_000));
    parse_recovers(&src_very_deep);
}

#[test]
fn test_parse_unary_minus_guard_bypass() {
    // Unary minus is also handled by parse_unary — verify it is guarded.
    let src = format!("{}1", "-".repeat(300));
    parse_recovers(&src);
}

#[test]
fn test_mixed_operator_nesting() {
    // Mix of word `not` and symbolic `!` — verify combined depth is guarded.
    let inner = format!("{}1", "not ".repeat(100));
    let src = format!("{}({inner})", "!".repeat(100));
    parse_recovers(&src);
}

// ---------------------------------------------------------------------------
// Regression: normal expressions must still parse correctly after the fix
// ---------------------------------------------------------------------------

#[test]
fn test_normal_not_expression_still_works() {
    let cases = ["not 1", "not $x", "not (1 == 1)", "not not 1", "!$x", "!!$x", "-$n"];
    for src in cases {
        assert!(!has_nesting_err(src), "normal expression `{src}` must not produce NestingTooDeep");
    }
}
