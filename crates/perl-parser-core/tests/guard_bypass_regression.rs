// Regression tests for issue #1367: parser P0 hang risks — unguarded recursion
// in parse_word_not_expr (precedence.rs) and parse_unary (unary.rs).
//
// Before the fix:
//   - 5000 nested `not` operators → SIGSEGV in parse_word_not_expr
//   - 200  nested `!` operators   → stack overflow in parse_unary
//
// After the fix both sites are wrapped in with_recursion_guard() so deeply
// nested input returns a recursion-depth error instead of crashing. Structural
// block nesting remains a separate NestingTooDeep proof below.

mod cpan_test_helpers;
use cpan_test_helpers::*;

use perl_parser_core::{ParseError, ParseOutput, ParseStopCause, Parser};

fn is_recursion_guard_error(error: &ParseError) -> bool {
    matches!(error, ParseError::RecursionDepthExhausted { depth: 129, max_depth: 128 })
}

fn is_recursion_guard_family(error: &ParseError) -> bool {
    matches!(error, ParseError::RecursionDepthExhausted { .. })
}

fn is_structural_nesting_error(error: &ParseError) -> bool {
    matches!(error, ParseError::NestingTooDeep { depth: 513, max_depth: 512 })
}

fn is_structural_nesting_family(error: &ParseError) -> bool {
    matches!(error, ParseError::NestingTooDeep { .. })
}

fn is_recovered_error(error: &ParseError) -> bool {
    matches!(error, ParseError::Recovered { .. })
}

fn never_matches(_: &ParseError) -> bool {
    false
}

fn require(condition: bool, message: impl Into<String>) -> Result<(), String> {
    condition.then_some(()).ok_or_else(|| message.into())
}

fn parse_fails_with(
    code: &str,
    expected: fn(&ParseError) -> bool,
    forbidden: fn(&ParseError) -> bool,
) -> bool {
    let mut parser = Parser::new(code);
    let result = parser.parse();
    result.is_err()
        && has_exclusive_error_family(result.as_ref().err(), parser.errors(), expected, forbidden)
}

fn has_exclusive_error_family(
    direct: Option<&ParseError>,
    recorded: &[ParseError],
    expected: fn(&ParseError) -> bool,
    forbidden: fn(&ParseError) -> bool,
) -> bool {
    let has_expected = direct.is_some_and(expected) || recorded.iter().any(expected);
    let has_forbidden = direct.is_some_and(forbidden) || recorded.iter().any(forbidden);
    has_expected && !has_forbidden
}

fn has_exclusive_diagnostic_family(
    diagnostics: &[ParseError],
    expected: fn(&ParseError) -> bool,
    forbidden: fn(&ParseError) -> bool,
) -> bool {
    diagnostics.iter().any(expected) && !diagnostics.iter().any(forbidden)
}

fn has_recursion_guard_diagnostic(output: &ParseOutput) -> bool {
    has_exclusive_diagnostic_family(
        &output.diagnostics,
        is_recursion_guard_error,
        is_structural_nesting_family,
    )
}

fn has_recursion_stop_cause(output: &ParseOutput) -> bool {
    matches!(
        output.stop_cause(),
        Some(ParseStopCause::RecursionBudgetExhausted { limit: Some(128), usage: Some(129) })
    )
}

fn has_structural_nesting_stop_cause(output: &ParseOutput) -> bool {
    matches!(
        output.stop_cause(),
        Some(ParseStopCause::NestingOrDepthBudgetExhausted { limit: 512, usage: 513 })
    )
}

fn has_structural_nesting_error(output: &ParseOutput) -> bool {
    output.diagnostics.iter().any(is_structural_nesting_error)
        && !output.diagnostics.iter().any(is_recursion_guard_family)
}

#[test]
fn parse_error_oracle_rejects_swapped_field_direct_recorded_contradictions() -> Result<(), String> {
    let recursion = ParseError::RecursionDepthExhausted { depth: 129, max_depth: 128 };
    let structural_with_recursion_fields =
        ParseError::NestingTooDeep { depth: 129, max_depth: 128 };
    require(
        !has_exclusive_error_family(
            Some(&recursion),
            &[structural_with_recursion_fields],
            is_recursion_guard_error,
            is_structural_nesting_family,
        ),
        "recursion oracle accepted a structural contradiction with recursion fields",
    )?;

    let structural = ParseError::NestingTooDeep { depth: 513, max_depth: 512 };
    let recursion_with_structural_fields =
        ParseError::RecursionDepthExhausted { depth: 513, max_depth: 512 };
    require(
        !has_exclusive_error_family(
            Some(&structural),
            &[recursion_with_structural_fields],
            is_structural_nesting_error,
            is_recursion_guard_family,
        ),
        "structural oracle accepted a recursion contradiction with structural fields",
    )
}

#[test]
fn recovery_oracle_rejects_swapped_structural_fields() -> Result<(), String> {
    let diagnostics = [
        ParseError::RecursionDepthExhausted { depth: 129, max_depth: 128 },
        ParseError::NestingTooDeep { depth: 129, max_depth: 128 },
    ];
    require(
        !has_exclusive_diagnostic_family(
            &diagnostics,
            is_recursion_guard_error,
            is_structural_nesting_family,
        ),
        "recovery oracle accepted a structural contradiction with recursion fields",
    )
}

#[test]
fn recovery_oracle_rejects_swapped_recursion_fields() -> Result<(), String> {
    let diagnostics = [
        ParseError::NestingTooDeep { depth: 513, max_depth: 512 },
        ParseError::RecursionDepthExhausted { depth: 513, max_depth: 512 },
    ];
    require(
        !has_exclusive_diagnostic_family(
            &diagnostics,
            is_structural_nesting_error,
            is_recursion_guard_family,
        ),
        "recovery oracle accepted a recursion contradiction with structural fields",
    )
}

#[test]
fn parse_error_oracle_rejects_contradictory_guard_families() -> Result<(), String> {
    let errors = [
        ParseError::RecursionDepthExhausted { depth: 129, max_depth: 128 },
        ParseError::NestingTooDeep { depth: 513, max_depth: 512 },
    ];
    let [recursion, structural] = &errors;
    require(
        !has_exclusive_error_family(
            None,
            &errors,
            is_recursion_guard_error,
            is_structural_nesting_family,
        ),
        "recorded contradictory guard families were accepted",
    )?;
    require(
        !has_exclusive_error_family(
            Some(recursion),
            std::slice::from_ref(structural),
            is_recursion_guard_error,
            is_structural_nesting_family,
        ),
        "direct recursion plus recorded structural contradiction was accepted",
    )?;
    require(
        !has_exclusive_error_family(
            Some(structural),
            std::slice::from_ref(recursion),
            is_recursion_guard_error,
            is_structural_nesting_family,
        ),
        "direct structural plus recorded recursion contradiction was accepted",
    )
}

#[test]
fn recursion_oracle_rejects_structural_guard_substitution() -> Result<(), String> {
    let code = nested_eval_blocks(600);
    require(
        !parse_fails_with(&code, is_recursion_guard_error, is_structural_nesting_family),
        "recursion oracle accepted structural guard substitution",
    )
}

#[test]
fn recursion_oracle_rejects_lifted_limit_signature() -> Result<(), String> {
    let lifted_limit = ParseError::RecursionDepthExhausted { depth: 129, max_depth: 129 };
    require(
        !is_recursion_guard_error(&lifted_limit),
        "recursion oracle accepted a lifted limit signature",
    )?;

    let lifted_structural_limit = ParseError::NestingTooDeep { depth: 513, max_depth: 513 };
    require(
        !is_structural_nesting_error(&lifted_structural_limit),
        "structural oracle accepted a lifted limit signature",
    )
}

#[test]
fn parse_fails_with_rejects_successful_recovery_with_recorded_errors() -> Result<(), String> {
    let code = "my $x = ; print 1;";
    let mut parser = Parser::new(code);
    let result = parser.parse();
    require(result.is_ok(), "the recovery fixture should return a partial AST")?;
    require(
        parser.errors().iter().any(is_recovered_error),
        "the recovery fixture should record a recovered diagnostic",
    )?;

    require(
        !parse_fails_with(code, is_recovered_error, never_matches),
        "successful recovery was accepted as a failed parse",
    )
}

// --- parse_word_not_expr (precedence.rs) ---

#[test]
fn word_not_5000_deep_does_not_sigsegv() {
    // ('not ' x 5000).'1' in Perl — 5000 nested `not` operators.
    // Before fix: SIGSEGV at ~5000 due to unguarded self-recursion.
    let code = "not ".repeat(5000) + "1";
    assert!(
        parse_fails_with(&code, is_recursion_guard_error, is_structural_nesting_family),
        "5000-deep `not` chain should fail with the recursion guard, not crash"
    );
}

#[test]
fn word_not_depth_130_hits_limit() {
    // 130 levels is just above MAX_RECURSION_DEPTH (128).
    let code = "not ".repeat(130) + "1";
    assert!(
        parse_fails_with(&code, is_recursion_guard_error, is_structural_nesting_family),
        "130-deep `not` chain should hit the recursion guard"
    );
}

#[test]
fn word_not_129_calls_hit_limit() {
    // The 129th call is just above MAX_RECURSION_DEPTH (128) and trips the guard.
    let code = "not ".repeat(129) + "1";
    assert!(
        parse_fails_with(&code, is_recursion_guard_error, is_structural_nesting_family),
        "129-deep `not` chain should hit the recursion guard"
    );
}

// --- parse_unary (unary.rs) ---

#[test]
fn bang_200_deep_does_not_sigsegv() {
    // ('!' x 200).'1' — 200 nested `!` operators.
    // Before fix: stack overflow in parse_unary at ~200 levels.
    let code = "!".repeat(200) + "1";
    assert!(
        parse_fails_with(&code, is_recursion_guard_error, is_structural_nesting_family),
        "200-deep `!` chain should fail with RecursionDepthExhausted, not crash"
    );
}

#[test]
fn bang_depth_130_hits_limit() {
    let code = "!".repeat(130) + "1";
    assert!(
        parse_fails_with(&code, is_recursion_guard_error, is_structural_nesting_family),
        "130-deep `!` chain should hit the recursion guard"
    );
}

#[test]
fn unary_minus_depth_hits_limit() {
    // parse_unary recurses for `-` as well — verify the same guard fires.
    // Use 300 dashes: even if the lexer collapses pairs into Decrement tokens
    // (giving 150 recursion levels), that still exceeds MAX_RECURSION_DEPTH=128.
    let code = "-".repeat(300) + "1";
    assert!(
        parse_fails_with(&code, is_recursion_guard_error, is_structural_nesting_family),
        "300-deep unary-minus chain should hit the recursion guard"
    );
}

#[test]
fn increment_depth_130_hits_limit() {
    // Pre-increment also recurses through parse_unary.
    let code = "++".repeat(130) + "$x";
    assert!(
        parse_fails_with(&code, is_recursion_guard_error, is_structural_nesting_family),
        "130-deep `++` chain should hit the recursion guard"
    );
}

#[test]
fn power_chain_depth_hits_limit() {
    let code = "1 ** ".repeat(130) + "1";
    assert!(
        parse_fails_with(&code, is_recursion_guard_error, is_structural_nesting_family),
        "130-deep power chain should fail with the recursion guard, not overflow the stack"
    );
}

#[test]
fn deep_power_chain_recovery_surfaces_recursion_diagnostic() -> Result<(), String> {
    let code = "1 ** ".repeat(2_000) + "1";
    let mut parser = Parser::new(&code);
    let output = parser.parse_with_recovery();
    require(
        has_recursion_guard_diagnostic(&output),
        "parse_with_recovery should surface RecursionDepthExhausted for a deep power chain",
    )?;
    require(
        has_recursion_stop_cause(&output),
        format!(
            "parse_with_recovery should preserve the recursion stop cause, got {:?}",
            output.stop_cause()
        ),
    )
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

// --- Test 1: mixed operator nesting (carried from superseded #2669) ---

#[test]
fn mixed_not_and_bang_nesting_hits_limit() {
    // Carried from superseded #2669 (test_mixed_operator_nesting): symbolic `!`
    // and word `not` may share the global recursion budget if both call the same
    // recursive parsing path. 100 `!` wrapping (100 `not` 1) = ~200 combined levels.
    // If the guard is per-function (not shared), 100+100=200 exceeds MAX_RECURSION_DEPTH (128).
    // If the guard is global and shared, even fewer nesting levels trip it.
    let inner = "not ".repeat(100) + "1";
    let code = "!".repeat(100) + "(" + &inner + ")";
    assert!(
        parse_fails_with(&code, is_recursion_guard_error, is_structural_nesting_family),
        "mixed !/not nesting should hit the recursion guard"
    );
}

// --- Test 2: LSP-facing path (parse_with_recovery) ---

#[test]
fn deep_nesting_recovers_with_recursion_diagnostic_on_lsp_path() -> Result<(), String> {
    // LSP uses parse_with_recovery(): deep nesting must yield a (partial) tree
    // AND a depth-guard diagnostic — not a crash, not a silent success.
    let code = "not ".repeat(300) + "1";
    let mut parser = Parser::new(&code);
    let output = parser.parse_with_recovery();
    require(
        has_recursion_guard_diagnostic(&output),
        "parse_with_recovery should surface RecursionDepthExhausted for deep nesting",
    )?;
    require(
        has_recursion_stop_cause(&output),
        format!(
            "parse_with_recovery should preserve the recursion stop cause, got {:?}",
            output.stop_cause()
        ),
    )
}

#[test]
fn comp_parser_pl_lex_brackstack_block_depth_parses() {
    let code = nested_eval_blocks(150);
    assert_clean_parse(&code);
}

#[test]
fn pathological_block_depth_still_hits_limit() -> Result<(), String> {
    let code = nested_eval_blocks(600);
    let mut parser = Parser::new(&code);
    let output = parser.parse_with_recovery();
    require(
        has_structural_nesting_error(&output),
        "600-deep bare blocks should still hit the structural nesting guard",
    )?;
    require(
        has_structural_nesting_stop_cause(&output),
        format!(
            "structural block nesting should preserve its stop cause, got {:?}",
            output.stop_cause()
        ),
    )
}

fn nested_eval_blocks(depth: usize) -> String {
    let mut code = "eval ".to_string();
    code.push_str(&"{".repeat(depth + 1));
    code.push_str(&"}".repeat(depth + 1));
    code.push(';');
    code
}
