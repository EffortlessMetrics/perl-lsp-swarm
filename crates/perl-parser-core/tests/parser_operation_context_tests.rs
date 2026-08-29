//! Production operation-context contract for #8757 (train #8700 B01).
//!
//! Discriminators:
//! - a post-hoc `BudgetTracker::new()` that copies `errors_emitted` from
//!   diagnostics cannot report `max_depth_reached > 0`;
//! - a live tracker that governs recursion must unwind `current_depth` to 0
//!   while retaining maximum depth;
//! - a second operation must not inherit the first operation's counters;
//! - this row does not charge tokens/nodes/diagnostics (B02 / #8786).
//!
//! `ParserContext` is exercised only as a non-competing parallel helper.

use perl_parser_core::{ParseError, ParseOutput, Parser, ParserConfigIdentity, ParserOperationId};
use perl_tdd_support::must;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

fn parse_recovery(source: &str) -> ParseOutput {
    let mut parser = Parser::new(source);
    parser.parse_with_recovery()
}

/// Nested `not` operators that exercise production recursion depth (`with_depth`).
/// Bare `{...}` blocks use syntactic `block_depth`, not the live tracker.
fn nested_not(depth: usize) -> String {
    format!("{}1", "not ".repeat(depth))
}

fn is_reconstructed_from_diagnostics(output: &ParseOutput) -> bool {
    output.budget_usage.max_depth_reached == 0
        && output.budget_usage.current_depth == 0
        && output.budget_usage.errors_emitted == output.diagnostics.len()
        && output.budget_usage.tokens_skipped == 0
        && output.budget_usage.recoveries_attempted == 0
}

/// Recovered syntax must keep the live tracker and must not be relabeled as a
/// terminal stop. A reconstruction that maps any non-empty diagnostics list
/// onto `Some(stop_cause)` would pass depth assertions without this control.
fn assert_recovered_completion(output: &ParseOutput) {
    assert!(!output.diagnostics.is_empty(), "fixture must produce recovered diagnostics");
    assert_eq!(
        output.stop_cause(),
        None,
        "recovered syntax must not set a terminal stop cause, got {:?}",
        output.stop_cause()
    );
    assert!(!output.terminated_early(), "recovered syntax must not terminate early");
}

#[test]
fn clean_strict_and_recovery_paths_record_live_depth() {
    let source = "my $x = 1;";
    let mut strict = Parser::new(source);
    let ast = must(strict.parse());
    assert!(matches!(ast.kind, perl_parser_core::NodeKind::Program { .. }));

    let recovery = parse_recovery(source);
    assert!(matches!(recovery.ast.kind, perl_parser_core::NodeKind::Program { .. }));
    assert!(recovery.diagnostics.is_empty(), "valid source must stay diagnostic-clean");
    assert_eq!(recovery.stop_cause(), None);

    // Post-hoc reconstruction never records depth. A live tracker must.
    assert!(
        recovery.budget_usage.max_depth_reached > 0,
        "live tracker must record production recursion depth, got {:?}",
        recovery.budget_usage
    );
    assert_eq!(recovery.budget_usage.current_depth, 0, "depth must unwind to zero on success");
}

#[test]
fn recovery_aware_output_is_not_a_post_hoc_tracker() {
    let output = parse_recovery("my $x = ;");
    assert_recovered_completion(&output);
    assert!(
        !is_reconstructed_from_diagnostics(&output),
        "budget_usage must be the live tracker, not BudgetTracker::new() + errors_emitted=diagnostics.len(); got {:?}",
        output.budget_usage
    );
    assert_eq!(output.budget_usage.current_depth, 0);
    assert!(output.budget_usage.max_depth_reached > 0);
}

#[test]
fn nested_success_retains_max_depth_and_unwinds() {
    let output = parse_recovery(&nested_not(8));
    assert_eq!(output.stop_cause(), None, "shallow nesting must complete");
    assert_eq!(output.budget_usage.current_depth, 0);
    assert!(
        output.budget_usage.max_depth_reached >= 8,
        "nested not-chain must raise tracked max depth, got {}",
        output.budget_usage.max_depth_reached
    );
}

#[test]
fn syntax_recovery_unwinds_depth() {
    let output = parse_recovery("sub foo { my $x = ; }");
    assert_recovered_completion(&output);
    assert_eq!(
        output.budget_usage.current_depth, 0,
        "recovery must unwind live depth, got {}",
        output.budget_usage.current_depth
    );
    assert!(output.budget_usage.max_depth_reached > 0);
}

#[test]
fn catastrophic_recursion_unwinds_and_keeps_max_depth() {
    let source = format!("my $x = {}1;", "not ".repeat(200));
    let output = parse_recovery(&source);
    assert!(output.terminated_early(), "deep not-chain must terminate early");
    let max_depth = ParserConfigIdentity::production_default().max_recursion_depth();
    assert!(
        matches!(
            output.stop_cause(),
            Some(perl_parser_core::ParseStopCause::RecursionBudgetExhausted {
                limit: Some(limit),
                usage: Some(usage),
            }) if limit == max_depth && usage == max_depth.saturating_add(1)
        ),
        "expression recursion must keep RecursionBudgetExhausted with the production limit, got {:?}",
        output.stop_cause()
    );
    assert_eq!(output.budget_usage.current_depth, 0, "exhaustion must unwind current depth");
    assert!(
        output.budget_usage.max_depth_reached > 0,
        "exhaustion must retain maximum depth reached"
    );
}

#[test]
fn pre_set_cancellation_returns_live_uncharged_tracker() {
    let flag = Arc::new(AtomicBool::new(true));
    let mut parser = Parser::new_with_cancellation("my $x = 1; { { { 1 } } }", flag);
    let output = parser.parse_with_recovery();
    assert_eq!(output.stop_cause(), Some(perl_parser_core::ParseStopCause::Cancelled));
    assert!(
        output.diagnostics.iter().any(|e| matches!(e, ParseError::Cancelled)),
        "cancelled parse must surface Cancelled in diagnostics"
    );
    // Entry cancellation happens before recursion work. A reconstructed tracker
    // would still set errors_emitted = diagnostics.len(). This row does not
    // charge diagnostics (B02).
    assert_eq!(output.budget_usage.current_depth, 0);
    assert_eq!(output.budget_usage.max_depth_reached, 0);
    assert_eq!(
        output.budget_usage.errors_emitted, 0,
        "diagnostic charging is B02; live tracker must not be reconstructed from diagnostics"
    );
    assert_ne!(output.budget_usage.errors_emitted, output.diagnostics.len());
}

#[test]
fn second_parse_starts_from_fresh_tracker_counters() {
    let source = nested_not(6);
    let mut parser = Parser::new(&source);
    let first = parser.parse_with_recovery();
    assert!(first.budget_usage.max_depth_reached >= 6);

    let second = parser.parse_with_recovery();
    // After the first operation the instance is at EOF. `take_tracker()` already
    // zeroes the field, so this row rejects leftover counters when neither take
    // nor `begin()` reset; it does not by itself prove `begin()` reset. That
    // discriminator is `begin_resets_tracker_and_allocates_a_new_operation_id`.
    // Rewinding the token stream to re-parse a non-empty source is out of this
    // claim (incremental behavior is a non-goal).
    assert_eq!(
        second.budget_usage.max_depth_reached, 0,
        "second operation must start from a fresh tracker, got max_depth_reached={}",
        second.budget_usage.max_depth_reached
    );
    assert_eq!(second.budget_usage.current_depth, 0);
}

#[test]
fn independent_parser_instances_do_not_share_counters() {
    let deep = parse_recovery(&nested_not(20));
    let shallow = parse_recovery("1;");
    assert!(deep.budget_usage.max_depth_reached > shallow.budget_usage.max_depth_reached);
    assert_eq!(shallow.budget_usage.current_depth, 0);
    assert_eq!(deep.budget_usage.current_depth, 0);
}

#[test]
fn default_and_recovery_constructors_preserve_ast_for_valid_source() {
    let source = "my $x = 1; sub foo { $x }";
    let mut via_new = Parser::new(source);
    let mut via_recovery_ctor = Parser::new_with_recovery_config(source, ());
    let strict_ast = must(via_new.parse());
    let recovery = via_recovery_ctor.parse_with_recovery();
    assert_eq!(strict_ast.to_sexp(), recovery.ast.to_sexp());
    assert!(recovery.diagnostics.is_empty());
}

#[test]
fn parser_context_is_not_the_production_tracker() {
    // ParserContext remains a parallel AST-v2 helper (retire/migrate in
    // #8700 B04 / #7105). Production output must not be satisfiable by
    // constructing a ParserContext and reading its tracker.
    let ctx = perl_parser_core::parser_context::ParserContext::new("my $x = 1;".into());
    assert_eq!(ctx.budget_tracker().max_depth_reached, 0);
    assert_eq!(ctx.budget_tracker().current_depth, 0);

    let production = parse_recovery("my $x = 1;");
    assert!(
        production.budget_usage.max_depth_reached > ctx.budget_tracker().max_depth_reached,
        "ParserContext tracker is not the production live tracker"
    );
}

#[test]
fn uncharged_dimensions_stay_zero_on_recovered_parse() {
    let output = parse_recovery("my $x = ;");
    assert_recovered_completion(&output);
    assert_eq!(
        output.budget_usage.errors_emitted, 0,
        "token/node/diagnostic charging is B02 / #8786"
    );
    assert_eq!(output.budget_usage.tokens_skipped, 0);
    assert_eq!(output.budget_usage.recoveries_attempted, 0);
}

#[test]
fn cancellation_flag_is_not_leaked_as_terminal_state_across_fresh_parsers() {
    let flag = Arc::new(AtomicBool::new(false));
    let mut cancelled_elsewhere = Parser::new_with_cancellation("1;", Arc::clone(&flag));
    flag.store(true, Ordering::Relaxed);
    let cancelled = cancelled_elsewhere.parse_with_recovery();
    assert!(cancelled.terminated_early());

    let clean = parse_recovery("1;");
    assert_eq!(clean.stop_cause(), None);
    assert!(!clean.terminated_early());
}

#[test]
fn default_and_explicit_equivalent_config_share_identity() {
    let source = "my $x = 1;";
    let via_new = Parser::new(source);
    let via_recovery = Parser::new_with_recovery_config(source, ());
    let via_explicit =
        Parser::with_production_config(source, ParserConfigIdentity::production_default());
    assert_eq!(via_new.config_identity(), ParserConfigIdentity::production_default());
    assert_eq!(via_new.config_identity(), via_recovery.config_identity());
    assert_eq!(via_new.config_identity(), via_explicit.config_identity());
}

#[test]
fn repeated_operations_allocate_distinct_operation_ids() {
    let mut parser = Parser::new("1;");
    let first: ParserOperationId = parser.operation_id();
    let _ = parser.parse_with_recovery();
    let after_first = parser.operation_id();
    let _ = parser.parse_with_recovery();
    let after_second = parser.operation_id();
    assert_ne!(first, after_first);
    assert_ne!(after_first, after_second);
}

#[test]
fn from_tokens_uses_the_same_production_config_identity() {
    let source = "1;";
    let mut stream = perl_parser_core::TokenStream::new(source);
    let mut tokens = Vec::new();
    loop {
        match stream.next() {
            Ok(t) if t.kind() == perl_parser_core::TokenKind::Eof => break,
            Ok(t) => tokens.push(t),
            Err(_) => break,
        }
    }
    let parser = Parser::from_tokens(tokens, source);
    assert_eq!(parser.config_identity(), ParserConfigIdentity::production_default());
}
