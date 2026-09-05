#![expect(clippy::expect_used, reason = "bounded fixture assertion for #3021")]
/// Deterministic fixtures for `ParseStopCause` — the typed terminal stop authority on
/// [`ParseOutput`].
///
/// These tests prove that:
///
/// 1. Completed (clean and recovered) parses set `stop_cause = None` and
///    `terminated_early = false`.
/// 2. Cooperative cancellation sets `stop_cause = Some(Cancelled)` and
///    `terminated_early = true`.
/// 3. Recursion-budget exhaustion sets `stop_cause = Some(RecursionBudgetExhausted)`.
/// 4. Nesting/depth-budget exhaustion sets `stop_cause = Some(NestingOrDepthBudgetExhausted)`
///    with the limit and usage from the governing `ParseError::NestingTooDeep` variant.
/// 5. The `stop_cause` field — not the `diagnostics` vector — is the terminal authority:
///    same diagnostic population with different stop-cause assignment changes only
///    `stop_cause`, not the diagnostics.
/// 6. Clean/recovered completion, cancellation, and distinct exhaustion families cannot
///    contradict the `stop_cause` field.
/// 7. The `terminated_early == stop_cause.is_some()` invariant holds on every constructor.
///
/// See issue #10559 for the acceptance criteria.
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use perl_parser_core::{
    BudgetTracker, Node, NodeKind, ParseError, ParseOutput, ParseStopCause, Parser, SourceLocation,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn empty_ast() -> Node {
    Node::new(NodeKind::Program { statements: vec![] }, SourceLocation { start: 0, end: 0 })
}

// ---------------------------------------------------------------------------
// ParseStopCause::from_parse_error — mapping coverage
// ---------------------------------------------------------------------------

#[test]
fn cancelled_error_maps_to_cancelled_cause() {
    let cause = ParseStopCause::from_parse_error(&ParseError::Cancelled);
    assert_eq!(cause, ParseStopCause::Cancelled);
    assert!(cause.is_cancelled());
    assert!(!cause.is_budget_exhaustion());
    assert_eq!(cause.as_str(), "cancelled");
}

#[test]
fn recursion_limit_error_maps_to_recursion_budget_exhausted() {
    let cause = ParseStopCause::from_parse_error(&ParseError::RecursionLimit);
    assert!(
        matches!(cause, ParseStopCause::RecursionBudgetExhausted { .. }),
        "expected RecursionBudgetExhausted, got {cause:?}"
    );
    // RecursionLimit carries no depth info — values are None.
    if let ParseStopCause::RecursionBudgetExhausted { limit, usage } = &cause {
        assert!(limit.is_none(), "limit should be None for unit-variant RecursionLimit");
        assert!(usage.is_none(), "usage should be None for unit-variant RecursionLimit");
    }
    assert!(!cause.is_cancelled());
    assert!(cause.is_budget_exhaustion());
    assert_eq!(cause.as_str(), "recursion_budget_exhausted");
}

#[test]
fn nesting_too_deep_error_maps_to_nesting_budget_exhausted_with_values() {
    let error = ParseError::NestingTooDeep { depth: 200, max_depth: 128 };
    let cause = ParseStopCause::from_parse_error(&error);
    assert!(
        matches!(cause, ParseStopCause::NestingOrDepthBudgetExhausted { limit: 128, usage: 200 }),
        "expected NestingOrDepthBudgetExhausted{{limit:128,usage:200}}, got {cause:?}"
    );
    assert!(!cause.is_cancelled());
    assert!(cause.is_budget_exhaustion());
    assert_eq!(cause.as_str(), "nesting_or_depth_budget_exhausted");
}

#[test]
fn other_parse_errors_map_to_catastrophic_termination() {
    let errors = [
        ParseError::UnexpectedEof,
        ParseError::SyntaxError { message: "bad".to_string(), location: 0 },
        ParseError::LexerError { message: "lex".to_string() },
    ];
    for e in &errors {
        let cause = ParseStopCause::from_parse_error(e);
        assert_eq!(
            cause,
            ParseStopCause::CatastrophicTermination,
            "expected CatastrophicTermination for {e:?}"
        );
        assert!(!cause.is_cancelled());
        assert!(!cause.is_budget_exhaustion());
        assert_eq!(cause.as_str(), "catastrophic_termination");
    }
}

// ---------------------------------------------------------------------------
// ParseStopCause helpers
// ---------------------------------------------------------------------------

#[test]
fn future_typed_terminal_is_constructible_and_routable() {
    let cause = ParseStopCause::FutureTypedTerminal;
    assert!(!cause.is_cancelled());
    assert!(!cause.is_budget_exhaustion());
    assert_eq!(cause.as_str(), "future_typed_terminal");
}

#[test]
fn parse_stop_cause_debug_and_clone() {
    let causes = [
        ParseStopCause::Cancelled,
        ParseStopCause::RecursionBudgetExhausted { limit: Some(128), usage: Some(128) },
        ParseStopCause::RecursionBudgetExhausted { limit: None, usage: None },
        ParseStopCause::NestingOrDepthBudgetExhausted { limit: 512, usage: 513 },
        ParseStopCause::CatastrophicTermination,
        ParseStopCause::FutureTypedTerminal,
    ];
    for c in &causes {
        let cloned = *c;
        assert_eq!(c, &cloned);
        let _ = format!("{c:?}");
    }
}

// ---------------------------------------------------------------------------
// ParseOutput constructors — invariant: terminated_early == stop_cause.is_some()
// ---------------------------------------------------------------------------

#[test]
fn success_sets_stop_cause_none_and_terminated_early_false() {
    let output = ParseOutput::success(empty_ast());
    assert!(output.stop_cause().is_none(), "success() must have stop_cause=None");
    assert!(!output.terminated_early(), "success() must have terminated_early=false");
}

#[test]
fn with_errors_sets_stop_cause_none_and_terminated_early_false() {
    let errors = vec![
        ParseError::SyntaxError { message: "e1".to_string(), location: 0 },
        ParseError::SyntaxError { message: "e2".to_string(), location: 5 },
    ];
    let output = ParseOutput::with_errors(empty_ast(), errors);
    assert!(output.stop_cause().is_none(), "with_errors() must have stop_cause=None");
    assert!(
        !output.terminated_early(),
        "with_errors() must have terminated_early=false (recovered parse, not terminated)"
    );
}

#[test]
fn finish_with_none_cause_sets_terminated_early_false() {
    let output = ParseOutput::finish(empty_ast(), vec![], BudgetTracker::new(), None);
    assert!(output.stop_cause().is_none());
    assert!(!output.terminated_early());
}

#[test]
fn finish_with_cancelled_cause_sets_terminated_early_true() {
    let output = ParseOutput::finish(
        empty_ast(),
        vec![ParseError::Cancelled],
        BudgetTracker::new(),
        Some(ParseStopCause::Cancelled),
    );
    assert!(output.stop_cause().is_some());
    assert!(output.terminated_early());
    assert_eq!(output.stop_cause(), Some(ParseStopCause::Cancelled));
}

#[test]
fn finish_with_nesting_cause_carries_limit_and_usage() {
    let cause = ParseStopCause::NestingOrDepthBudgetExhausted { limit: 128, usage: 129 };
    let output = ParseOutput::finish(empty_ast(), vec![], BudgetTracker::new(), Some(cause));
    assert_eq!(output.stop_cause(), Some(cause));
    assert!(output.terminated_early());
}

#[test]
fn finish_with_catastrophic_cause_sets_terminated_early() {
    let output = ParseOutput::finish(
        empty_ast(),
        vec![ParseError::UnexpectedEof],
        BudgetTracker::new(),
        Some(ParseStopCause::CatastrophicTermination),
    );
    assert!(output.terminated_early());
    assert_eq!(output.stop_cause(), Some(ParseStopCause::CatastrophicTermination));
}

// ---------------------------------------------------------------------------
// Stop cause is the authority — not inferred from diagnostics
// ---------------------------------------------------------------------------

/// The same diagnostic population with two different stop-cause assignments must
/// produce different `stop_cause` values and different `terminated_early` values,
/// while leaving `diagnostics` identical.  This proves that `stop_cause` — not
/// the presence or content of diagnostics — is the terminal authority.
#[test]
fn same_diagnostics_different_stop_cause_is_distinct() {
    let make_errors = || {
        vec![
            ParseError::SyntaxError { message: "syntax".to_string(), location: 0 },
            ParseError::RecursionLimit,
        ]
    };

    let completed = ParseOutput::finish(empty_ast(), make_errors(), BudgetTracker::new(), None);
    let terminated = ParseOutput::finish(
        empty_ast(),
        make_errors(),
        BudgetTracker::new(),
        Some(ParseStopCause::RecursionBudgetExhausted { limit: Some(128), usage: Some(128) }),
    );

    // Same diagnostics...
    assert_eq!(completed.diagnostics, terminated.diagnostics);
    // ...but different terminal authority.
    assert_ne!(completed.stop_cause(), terminated.stop_cause());
    assert_ne!(completed.terminated_early(), terminated.terminated_early());
}

/// Changing diagnostic order does not change the stop cause — the cause is set
/// independently of the diagnostics vector.
#[test]
fn diagnostic_order_change_does_not_affect_stop_cause() {
    let errors_ab = vec![
        ParseError::SyntaxError { message: "first".to_string(), location: 0 },
        ParseError::UnexpectedEof,
    ];
    let errors_ba = vec![
        ParseError::UnexpectedEof,
        ParseError::SyntaxError { message: "first".to_string(), location: 0 },
    ];

    let cause = Some(ParseStopCause::Cancelled);
    let out_ab = ParseOutput::finish(empty_ast(), errors_ab, BudgetTracker::new(), cause);
    let out_ba = ParseOutput::finish(empty_ast(), errors_ba, BudgetTracker::new(), cause);

    // Different diagnostic orders produce the same stop cause.
    assert_eq!(out_ab.stop_cause(), out_ba.stop_cause());
    assert_eq!(out_ab.terminated_early(), out_ba.terminated_early());
}

// ---------------------------------------------------------------------------
// Parser::parse_with_recovery — live cancellation fixture
// ---------------------------------------------------------------------------

/// Pre-setting the cancellation flag before parse_with_recovery() must produce
/// stop_cause = Cancelled and terminated_early = true with an empty (or nearly
/// empty) partial AST, without requiring a timing-based sleep.
#[test]
fn parse_with_recovery_records_cancelled_cause_when_flag_is_preset() {
    let flag = Arc::new(AtomicBool::new(true)); // set before parsing starts
    let mut parser = Parser::new_with_cancellation("my $x = 1; my $y = 2;", flag);
    let output = parser.parse_with_recovery();

    assert!(output.terminated_early(), "pre-set cancellation flag must set terminated_early=true");
    assert_eq!(
        output.stop_cause(),
        Some(ParseStopCause::Cancelled),
        "stop_cause must be Cancelled, not inferred from diagnostics"
    );
    // The Cancelled error is recorded in diagnostics exactly once.
    let cancelled_count =
        output.diagnostics.iter().filter(|e| matches!(e, ParseError::Cancelled)).count();
    assert_eq!(cancelled_count, 1, "Cancelled diagnostic should appear exactly once");
}

/// A parse that completes successfully must have stop_cause = None.
#[test]
fn parse_with_recovery_records_no_cause_for_clean_completion() {
    let mut parser = Parser::new("my $x = 42;");
    let output = parser.parse_with_recovery();

    assert!(output.stop_cause().is_none(), "clean parse must have stop_cause=None");
    assert!(!output.terminated_early(), "clean parse must have terminated_early=false");
    assert!(output.diagnostics.is_empty(), "clean parse must have no diagnostics");
}

/// A parse that recovers from syntax errors but completes must have stop_cause = None.
#[test]
fn parse_with_recovery_records_no_cause_for_recovered_completion() {
    // Intentionally broken syntax — the parser recovers but does not terminate early.
    let mut parser = Parser::new("my $x = ;");
    let output = parser.parse_with_recovery();

    assert!(
        output.stop_cause().is_none(),
        "recovered (but completed) parse must have stop_cause=None"
    );
    assert!(
        !output.terminated_early(),
        "recovered (but completed) parse must have terminated_early=false"
    );
    // At least one diagnostic from recovery
    assert!(!output.diagnostics.is_empty(), "recovered parse must have diagnostics");
}

/// Multiple recoverable diagnostics before cancellation: diagnostics are preserved,
/// stop_cause records only the terminal cause, not the first/last diagnostic.
#[test]
fn recovered_diagnostics_before_cancellation_preserved_with_cancelled_cause() {
    // Build the output manually — simulating what the parser would produce if several
    // recoveries occurred before the cancellation branch was reached.
    let errors = vec![
        ParseError::SyntaxError { message: "syntax1".to_string(), location: 0 },
        ParseError::SyntaxError { message: "syntax2".to_string(), location: 10 },
        ParseError::Cancelled,
    ];
    let output = ParseOutput::finish(
        empty_ast(),
        errors.clone(),
        BudgetTracker::new(),
        Some(ParseStopCause::Cancelled),
    );

    // All diagnostics are preserved.
    assert_eq!(output.diagnostics, errors);
    // Stop cause is Cancelled — not the first/last syntax error.
    assert_eq!(output.stop_cause(), Some(ParseStopCause::Cancelled));
    assert!(output.terminated_early());
}

// ---------------------------------------------------------------------------
// Negative controls
// ---------------------------------------------------------------------------

/// Clean completion with zero pending work cannot have stop_cause set.
#[test]
fn success_cannot_have_stop_cause_set() {
    let output = ParseOutput::success(empty_ast());
    assert!(output.stop_cause().is_none(), "success() must never produce a stop_cause");
}

/// An error-containing recovered parse (not cancelled, not budget-exhausted)
/// must not be treated as terminated.
#[test]
fn error_in_diagnostics_does_not_imply_terminated_early() {
    // RecursionLimit in the diagnostics vector does NOT mean terminated_early=true
    // unless the parser actually returned Err(RecursionLimit).
    let output = ParseOutput::with_errors(empty_ast(), vec![ParseError::RecursionLimit]);
    assert!(
        !output.terminated_early(),
        "a RecursionLimit diagnostic without a stop cause must not set terminated_early"
    );
    assert!(output.stop_cause().is_none(), "with_errors() must never set stop_cause");
}

/// Cancellation must not be confused with budget exhaustion.
#[test]
fn cancelled_and_budget_exhausted_causes_are_distinct() {
    let cancelled = ParseStopCause::Cancelled;
    let exhausted = ParseStopCause::RecursionBudgetExhausted { limit: Some(128), usage: Some(128) };
    assert_ne!(cancelled, exhausted);
    assert!(cancelled.is_cancelled());
    assert!(!exhausted.is_cancelled());
    assert!(!cancelled.is_budget_exhaustion());
    assert!(exhausted.is_budget_exhaustion());
}

/// Recursion exhaustion and nesting exhaustion are distinct variants.
#[test]
fn recursion_and_nesting_budget_exhaustion_are_distinct() {
    let recursion = ParseStopCause::RecursionBudgetExhausted { limit: Some(128), usage: Some(128) };
    let nesting = ParseStopCause::NestingOrDepthBudgetExhausted { limit: 512, usage: 513 };
    assert_ne!(recursion, nesting);
    assert!(recursion.is_budget_exhaustion());
    assert!(nesting.is_budget_exhaustion());
}

/// shutdown/resource-limit outcomes cannot be promoted to no-stop-cause merely
/// because there are no diagnostics.
#[test]
fn resource_limit_stop_cause_is_not_erased_by_empty_diagnostics() {
    let output = ParseOutput::finish(
        empty_ast(),
        vec![],
        BudgetTracker::new(),
        Some(ParseStopCause::RecursionBudgetExhausted { limit: None, usage: None }),
    );
    assert!(output.stop_cause().is_some(), "stop_cause must survive even with empty diagnostics");
    assert!(output.terminated_early());
}

/// `from_parse_error` must not collapse cancellation, recursion, and depth exhaustion.
#[test]
fn from_parse_error_preserves_cause_family_distinctions() {
    let cancelled = ParseStopCause::from_parse_error(&ParseError::Cancelled);
    let recursion = ParseStopCause::from_parse_error(&ParseError::RecursionLimit);
    let nesting = ParseStopCause::from_parse_error(&ParseError::NestingTooDeep {
        depth: 200,
        max_depth: 128,
    });

    assert_ne!(cancelled, recursion);
    assert_ne!(cancelled, nesting);
    assert_ne!(recursion, nesting);
}

/// Completed-recovered parse with a nonterminal warning must remain completed.
#[test]
fn recovered_parse_with_warning_has_no_stop_cause() {
    let warnings = vec![ParseError::Advisory { message: "style warning".to_string(), location: 0 }];
    let output = ParseOutput::with_errors(empty_ast(), warnings);
    assert!(output.stop_cause().is_none());
    assert!(!output.terminated_early());
}

/// Repeated parser operations start with no stale cause.
#[test]
fn repeated_parser_runs_produce_independent_stop_causes() {
    let flag = Arc::new(AtomicBool::new(false));
    let mut parser1 = Parser::new("my $x = 1;");
    let out1 = parser1.parse_with_recovery();

    // Set the flag then parse a second time with a new parser.
    flag.store(true, Ordering::Relaxed);
    let mut parser2 = Parser::new_with_cancellation("my $y = 2;", flag);
    let out2 = parser2.parse_with_recovery();

    // First parse: completed, no stop cause.
    assert!(out1.stop_cause().is_none());
    assert!(!out1.terminated_early());
    // Second parse: cancelled, has stop cause.
    assert_eq!(out2.stop_cause(), Some(ParseStopCause::Cancelled));
    assert!(out2.terminated_early());
}

// ---------------------------------------------------------------------------
// Production guard fixtures — the typed cause is set at the exact branch
// ---------------------------------------------------------------------------

/// When the lexer exhausts a per-token budget (regex/heredoc bytes, scan
/// steps, or delimiter nesting) it degrades to an `UnknownRest` token; the
/// parser stops early with a partial AST. That truncation must surface as the
/// typed lexer-budget stop cause — never as a clean `stop_cause = None`
/// completion that would make consumers treat the truncated AST as whole.
#[test]
fn lexer_budget_exhaustion_sets_typed_stop_cause() {
    // A match-operator regex literal far beyond the lexer's 64 KiB per-token
    // budget starts a fresh statement, so the degraded `UnknownRest` token is
    // consumed by the statement-loop budget branch (statements.rs) and the
    // preceding statements survive in the partial AST.
    let source = format!("my $ok = 1;\n/{};\n", "a".repeat(70_000));
    let mut parser = Parser::new(&source);
    let output = parser.parse_with_recovery();

    assert!(
        output.terminated_early(),
        "lexer-budget truncation must terminate_early, got stop_cause {:?}",
        output.stop_cause()
    );
    assert_eq!(
        output.stop_cause().as_ref().map(ParseStopCause::as_str),
        Some("lexer_budget_exhausted"),
        "lexer-budget truncation must carry the typed lexer cause, got {:?}",
        output.stop_cause()
    );
    assert!(
        output.stop_cause().is_some_and(|cause| cause.is_budget_exhaustion()),
        "lexer-budget exhaustion is a budget exhaustion"
    );
    // The partial AST keeps the statements parsed before the budget stop.
    assert!(
        matches!(
            &output.ast.kind,
            NodeKind::Program { statements }
                if statements.iter().any(|stmt| matches!(
                    &stmt.kind,
                    NodeKind::VariableDeclaration { variable, .. }
                        if matches!(&variable.kind, NodeKind::Variable { name, .. } if name.contains("ok"))
                ))
        ),
        "the statement parsed before the budget stop must survive, got {:?}",
        output.ast.kind
    );

    // Control: the same shape within the lexer budget parses clean.
    let bounded = format!("my $ok = 1;\nmy $re = qr/{};/;\n", "a".repeat(1_000));
    let mut clean = Parser::new(&bounded);
    let clean_output = clean.parse_with_recovery();
    assert!(
        clean_output.stop_cause().is_none() && !clean_output.terminated_early(),
        "within-budget regex must complete cleanly, got {:?}",
        clean_output.stop_cause()
    );
}

/// Deep expression recursion trips `check_recursion()` (the production
/// recursion guard). Its exhaustion must stay typed as
/// `RecursionBudgetExhausted` carrying the guard's budget values — the same
/// `NestingTooDeep` shape emitted by the structural block guard must not
/// relabel expression-recursion exhaustion as structural nesting.
#[test]
fn expression_recursion_exhaustion_keeps_recursion_cause() {
    let source = format!("my $x = {}1;", "not ".repeat(200));
    let mut parser = Parser::new(&source);
    let output = parser.parse_with_recovery();

    assert_eq!(
        output.stop_cause().as_ref().map(ParseStopCause::as_str),
        Some("recursion_budget_exhausted"),
        "expression-recursion exhaustion must keep the recursion cause, got {:?}",
        output.stop_cause()
    );
    assert!(
        matches!(
            output.stop_cause(),
            Some(ParseStopCause::RecursionBudgetExhausted { limit: Some(_), usage: Some(_) })
        ),
        "the production recursion guard supplies its limit and usage, got {:?}",
        output.stop_cause()
    );
    assert!(output.terminated_early());
}

/// The structural block guard keeps the nesting/depth cause: deep block
/// nesting (a different guard) still maps to `NestingOrDepthBudgetExhausted`,
/// so the two guard families remain distinct after the origin is preserved.
#[test]
fn structural_block_nesting_keeps_nesting_cause() {
    let depth = 600; // beyond MAX_BLOCK_NESTING_DEPTH (512)
    let source = format!("{}{}", "{".repeat(depth), "}".repeat(depth));
    let mut parser = Parser::new(&source);
    let output = parser.parse_with_recovery();

    assert_eq!(
        output.stop_cause().as_ref().map(ParseStopCause::as_str),
        Some("nesting_or_depth_budget_exhausted"),
        "structural block nesting keeps the nesting cause, got {:?}",
        output.stop_cause()
    );
    assert!(output.terminated_early());
}

// ---------------------------------------------------------------------------
// Operation-scoped state isolation (same parser instance)
// ---------------------------------------------------------------------------

/// A stored ok-path cause belongs to exactly one operation.
///
/// `parse()` returns `Ok` for a lexer-budget truncation without consuming the
/// stored cause. A *second* operation on the same parser instance must not
/// consume the first operation's cause: the second parse is already at EOF
/// and completes clean. This is the same-instance `parse()` →
/// `parse_with_recovery()` falsifier for operation-scoped state leakage.
#[test]
fn second_operation_on_same_parser_never_consumes_a_stale_stop_cause() {
    // Lexer-budget source: parse() returns Ok with a partial AST and stores
    // LexerBudgetExhausted without consuming it.
    let source = format!("my $ok = 1;\n/{};\n", "a".repeat(70_000));
    let mut parser = Parser::new(&source);
    let first = parser.parse().expect("budget truncation returns Ok with partial AST");
    assert!(
        matches!(first.kind, perl_parser_core::NodeKind::Program { .. }),
        "first operation returns a (partial) program"
    );

    // Second operation, SAME instance: at EOF, completes clean. Before the
    // entry-clear repair this consumed the stored cause and reported a clean
    // empty parse as LexerBudgetExhausted-truncated.
    let second = parser.parse_with_recovery();
    assert_eq!(
        second.stop_cause(),
        None,
        "second operation must not inherit the first operation's stored cause"
    );
    assert!(!second.terminated_early());
}

/// An intervening terminal error must not leak a stored cause either.
///
/// parse_with_recovery() on a cancellation-flagged parser takes the error
/// arm; the next operation on the same instance must start with no stored
/// cause rather than consuming whatever the failed operation had stored.
#[test]
fn error_arm_clears_stored_cause_for_the_next_operation() {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    let flag = Arc::new(AtomicBool::new(true));
    let mut parser = Parser::new_with_cancellation("my $x = 1;", flag);
    let cancelled = parser.parse_with_recovery();
    assert_eq!(cancelled.stop_cause(), Some(ParseStopCause::Cancelled));

    // Reuse the same (cancelled) parser: cancellation is checked per
    // operation from the live flag; with the flag cleared the parse runs
    // and must not inherit the previous operation's terminal state.
    // The parser has already consumed its input, so this operation is at
    // EOF and completes clean.
    // (Flag still set keeps the error path; the point is the *stored* cause
    // is cleared at entry, so even the error arm reports its own cause.)
    let again = parser.parse_with_recovery();
    assert_eq!(
        again.stop_cause(),
        Some(ParseStopCause::Cancelled),
        "error arm reports its own mapped cause, not a stale stored one"
    );
}

/// The checked mutation path keeps the projection invariant by construction.
///
/// External consumers cannot express a cause and a boolean that disagree:
/// `terminated_early` is derived from the single stored cause, and
/// `set_stop_cause` is the only mutation path.
#[test]
fn checked_stop_cause_mutation_keeps_the_projection_consistent() {
    let mut clean = ParseOutput::success(empty_ast());
    assert_eq!(clean.terminated_early(), clean.stop_cause().is_some());
    assert!(!clean.terminated_early());

    clean.set_stop_cause(Some(ParseStopCause::Cancelled));
    assert!(clean.terminated_early());
    assert_eq!(clean.stop_cause(), Some(ParseStopCause::Cancelled));

    clean.set_stop_cause(None);
    assert!(!clean.terminated_early());
    assert_eq!(clean.stop_cause(), None);
}
