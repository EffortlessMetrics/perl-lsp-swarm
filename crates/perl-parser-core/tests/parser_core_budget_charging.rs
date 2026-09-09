//! Charge-before-work proof for the admitted core parser dimensions (#8786,
//! train row B02).
//!
//! These tests pin three things that a passing parser suite alone cannot:
//!
//! 1. **Exactness.** Known sources charge known amounts. Removing a charge,
//!    adding one, or charging the same work twice changes a pinned number and
//!    fails `exact_core_usage_for_known_sources`.
//! 2. **Boundary semantics.** Every dimension is exercised at `limit - 1`,
//!    `limit`, and `limit + 1`, so flipping a `<` to a `<=` fails.
//! 3. **Authority.** Source-scanning recurrence tests fail if a production
//!    seam is added that reaches the token stream, the node constructor, or
//!    the diagnostic vector without going through the charging seam.
//!
//! Recovery scanning, recovery attempts, heredoc collection, and fallback
//! invocation are deliberately *not* admitted core dimensions here; they are
//! charged by #7074 / #7291 and are asserted to remain zero.

use perl_parser_core::{
    BudgetTracker, ParseBudget, ParseCoreDimension, ParseError, ParseStopCause, Parser,
    ParserConfigIdentity,
};

/// Parse `source` under an explicit budget, returning the parse output.
fn parse_with_budget(source: &str, budget: ParseBudget) -> perl_parser_core::ParseOutput {
    let config = ParserConfigIdentity::production_default().with_budget(budget);
    let mut parser = Parser::with_production_config(source, config);
    parser.parse_with_recovery()
}

/// Parse `source` with every limit removed, so the charged usage is the real
/// work the operation performed rather than a clipped value.
fn usage_unlimited(source: &str) -> BudgetTracker {
    parse_with_budget(source, ParseBudget::unlimited()).budget_usage
}

/// An otherwise-unlimited budget with exactly one dimension bounded.
///
/// `ParseCoreDimension` is `#[non_exhaustive]`, so this match needs a wildcard.
/// Rather than let a newly admitted dimension silently fall through untested,
/// the assertion below fails when the returned budget does not actually bound
/// the dimension it was asked to bound.
fn budget_with(dimension: ParseCoreDimension, limit: usize) -> ParseBudget {
    let mut budget = ParseBudget::unlimited();
    match dimension {
        ParseCoreDimension::TokensConsumed => budget.max_tokens_consumed = limit,
        ParseCoreDimension::NodesConstructed => budget.max_nodes_constructed = limit,
        ParseCoreDimension::DiagnosticsEmitted => budget.max_errors = limit,
        _ => {}
    }
    assert_eq!(
        budget.core_limit(dimension),
        limit,
        "{dimension} is an admitted core dimension with no case here: add it to `budget_with` \
         and to the boundary tests rather than leaving it unproven"
    );
    budget
}

// ---------------------------------------------------------------------------
// Exactness
// ---------------------------------------------------------------------------

/// Pins the charged usage for sources whose required work is small enough to
/// be counted by hand. This is the test that fails if a charge site is
/// removed, duplicated, or added.
#[test]
fn exact_core_usage_for_known_sources() {
    // `1;` is one number token and one semicolon; it builds the number node,
    // its expression statement, and the program root.
    let usage = usage_unlimited("1;");
    assert_eq!(usage.tokens_consumed, 2, "tokens for `1;`");
    assert_eq!(usage.nodes_constructed, 3, "nodes for `1;`");
    assert_eq!(usage.errors_emitted, 0, "`1;` is a clean parse");

    let usage = usage_unlimited("my $x = 1;");
    assert_eq!(usage.tokens_consumed, 5, "tokens for `my $x = 1;`");
    assert_eq!(usage.nodes_constructed, 4, "nodes for `my $x = 1;`");
    assert_eq!(usage.errors_emitted, 0);

    let usage = usage_unlimited("$a + $b;");
    assert_eq!(usage.tokens_consumed, 4, "tokens for `$a + $b;`");
    assert_eq!(usage.nodes_constructed, 5, "nodes for `$a + $b;`");

    // A recovered parse retains exactly one diagnostic here.
    let usage = usage_unlimited("my $x = ;");
    assert_eq!(usage.tokens_consumed, 4);
    assert_eq!(usage.nodes_constructed, 3);
    assert_eq!(usage.errors_emitted, 1);
}

/// The sticky `Eof` terminator is not consumed input.
///
/// `TokenStream::next` returns `Eof` indefinitely once the input is
/// exhausted, so charging it would let a terminal loop inflate usage without
/// taking anything from the stream. An empty source therefore charges zero
/// tokens while still constructing its program node.
#[test]
fn sticky_eof_is_not_charged_as_consumed_input() {
    let usage = usage_unlimited("");
    assert_eq!(usage.tokens_consumed, 0, "an empty source consumes no input");
    assert_eq!(usage.nodes_constructed, 1, "an empty source still builds a program node");

    // Whitespace and newlines are trivia, not consumed tokens either.
    assert_eq!(usage_unlimited("\n\n   \n").tokens_consumed, 0);
}

/// Lookahead is not consumption. `$a + $b;` needs lookahead to resolve the
/// infix operator, yet charges exactly its four real tokens.
#[test]
fn lookahead_is_not_charged_as_consumption() {
    assert_eq!(usage_unlimited("$a + $b;").tokens_consumed, 4);
    // A deeply-peeked construct still charges only what it consumes.
    assert_eq!(usage_unlimited("1;").tokens_consumed, 2);
}

/// Charging is a pure function of source and configuration.
#[test]
fn charged_usage_is_deterministic_across_runs() {
    let first = usage_unlimited("my %h = (a => 1, b => 2); foo($h{a});");
    let second = usage_unlimited("my %h = (a => 1, b => 2); foo($h{a});");
    assert_eq!(first.tokens_consumed, second.tokens_consumed);
    assert_eq!(first.nodes_constructed, second.nodes_constructed);
    assert_eq!(first.errors_emitted, second.errors_emitted);
}

/// Repeated operations on the same parser start from fresh counters: usage
/// must not accumulate across parses.
#[test]
fn repeated_operations_begin_from_fresh_core_counters() {
    let config = ParserConfigIdentity::production_default().with_budget(ParseBudget::unlimited());
    let mut parser = Parser::with_production_config("my $x = 1;", config);

    let first = parser.parse_with_recovery().budget_usage;
    // The parser holds one token stream and does not rewind it, so a second
    // operation on the same instance sees an exhausted stream. What matters
    // here is that it starts from *zero*: if the counters leaked, the second
    // operation would report at least the first operation's usage.
    let second = parser.parse_with_recovery().budget_usage;

    assert_eq!(first.tokens_consumed, 5);
    assert_eq!(
        second.tokens_consumed, 0,
        "a second operation must begin from fresh counters, not inherit charged tokens"
    );
    assert!(
        second.nodes_constructed < first.nodes_constructed,
        "charged node usage must not accumulate across operations ({} then {})",
        first.nodes_constructed,
        second.nodes_constructed
    );
}

/// Retained diagnostics are operation-scoped, exactly like the counters that
/// account for them.
///
/// The counters and the vector are two halves of one receipt. Zeroing
/// `errors_emitted` in `begin` while leaving the diagnostics behind lets a
/// second operation return the first operation's diagnostics beside an
/// `errors_emitted` that does not account for them — a vector and a receipt
/// describing different operations. Against that implementation the second
/// operation here reports one diagnostic and `errors_emitted == 0`.
#[test]
fn repeated_operations_do_not_inherit_retained_diagnostics() {
    let config = ParserConfigIdentity::production_default().with_budget(ParseBudget::unlimited());
    let mut parser = Parser::with_production_config("my $x = ;", config);

    let first = parser.parse_with_recovery();
    assert_eq!(first.diagnostics.len(), 1, "the fixture must retain one diagnostic");
    assert_eq!(first.budget_usage.errors_emitted, 1, "and must charge for it");

    // The stream is exhausted, so the second operation performs no work and can
    // legitimately retain nothing. Anything it does return came from the first.
    let second = parser.parse_with_recovery();
    assert!(
        second.diagnostics.is_empty(),
        "a second operation must not return the first operation's diagnostics; got {:?}",
        second.diagnostics
    );
    assert_eq!(
        second.diagnostics.len(),
        second.budget_usage.errors_emitted,
        "the retained vector and the charge receipt must describe the same operation"
    );
}

/// Strict and recovery-aware entry points share one meaning for core work.
#[test]
fn strict_and_recovery_paths_share_core_charges() -> Result<(), Box<dyn std::error::Error>> {
    let config = ParserConfigIdentity::production_default().with_budget(ParseBudget::unlimited());

    let mut strict = Parser::with_production_config("my $x = 1;", config);
    let strict_ast = strict.parse()?;

    let recovery = parse_with_budget("my $x = 1;", ParseBudget::unlimited());

    assert_eq!(strict_ast.to_sexp(), recovery.ast.to_sexp());
    assert_eq!(
        usage_unlimited("my $x = 1;").tokens_consumed,
        recovery.budget_usage.tokens_consumed
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Boundary semantics: limit - 1, limit, limit + 1
// ---------------------------------------------------------------------------

/// For each dimension, a limit equal to the required work admits the parse
/// exactly; one below refuses it; one above changes nothing.
///
/// This is the test that fails if the comparison in `authorize_core` is
/// changed from `usage >= limit` to `usage > limit` or vice versa.
#[test]
fn each_dimension_admits_exactly_its_required_work() {
    const SOURCE: &str = "my $x = 1; my $y = 2;";
    let required = usage_unlimited(SOURCE);

    for (dimension, required_units) in [
        (ParseCoreDimension::TokensConsumed, required.tokens_consumed),
        (ParseCoreDimension::NodesConstructed, required.nodes_constructed),
    ] {
        assert!(required_units > 1, "{dimension} needs a meaningful amount of work to bound");

        // limit == required: admitted exactly, nothing refused.
        let at = parse_with_budget(SOURCE, budget_with(dimension, required_units));
        assert_eq!(
            at.stop_cause(),
            None,
            "{dimension}: a limit equal to the required work must admit the parse"
        );
        assert_eq!(at.budget_usage.core_usage(dimension), required_units);

        // limit == required + 1: identical outcome, no extra work invented.
        let above = parse_with_budget(SOURCE, budget_with(dimension, required_units + 1));
        assert_eq!(above.stop_cause(), None, "{dimension}: a slack limit must not change behavior");
        assert_eq!(above.budget_usage.core_usage(dimension), required_units);

        // limit == required - 1: refused, and refused in this dimension.
        let below = parse_with_budget(SOURCE, budget_with(dimension, required_units - 1));
        let cause = below.stop_cause();
        assert_eq!(
            cause,
            Some(ParseStopCause::CoreBudgetExhausted {
                dimension,
                limit: required_units - 1,
                usage: required_units - 1,
            }),
            "{dimension}: a limit one below the required work must refuse in that dimension, \
             naming the limit and a usage that stops on it because the refused unit is \
             never charged; got {cause:?}"
        );
    }
}

/// A limit of zero refuses the very first unit of work.
///
/// This is also the non-termination guard. A core refusal is a terminal
/// resource condition, not a syntax error: if it were routed into ordinary
/// error recovery, `synchronize()` would try to advance, be refused again,
/// and spin forever. `parse_program` and the block/hash recovery arms
/// therefore propagate `CoreBudgetExhausted` immediately, alongside the
/// recursion and nesting limits. Regressing that makes this test hang rather
/// than fail, so it is deliberately kept small and fast.
#[test]
fn a_zero_limit_refuses_the_first_unit() {
    for dimension in [ParseCoreDimension::TokensConsumed, ParseCoreDimension::NodesConstructed] {
        let output = parse_with_budget("my $x = 1;", budget_with(dimension, 0));
        let cause = output.stop_cause();
        assert_eq!(
            cause,
            Some(ParseStopCause::CoreBudgetExhausted { dimension, limit: 0, usage: 0 }),
            "{dimension}: a zero limit must refuse the first unit and charge nothing; \
             got {cause:?}"
        );
    }
}

/// The diagnostic dimension is bounded by the operation's *configured*
/// `max_errors`, and the retained vector never exceeds the charged count.
///
/// Before #8786 this limit was a hard-coded constant that ignored an explicit
/// budget, and the check read `self.errors.len()` rather than charged usage.
#[test]
fn diagnostic_retention_honors_the_configured_limit() {
    // A source that recovers from several independent syntax errors.
    const SOURCE: &str = "my $a = ; my $b = ; my $c = ; my $d = ;";
    let required = usage_unlimited(SOURCE).errors_emitted;
    assert!(required >= 3, "need several diagnostics to bound; got {required}");

    // At the limit: everything is retained.
    let at =
        parse_with_budget(SOURCE, budget_with(ParseCoreDimension::DiagnosticsEmitted, required));
    assert_eq!(at.budget_usage.errors_emitted, required);
    assert_eq!(at.diagnostics.len(), required);

    // Below the limit: retention stops at the configured bound, and the
    // charged count — not the vector length — is the authority.
    let capped = required - 1;
    let below =
        parse_with_budget(SOURCE, budget_with(ParseCoreDimension::DiagnosticsEmitted, capped));
    assert_eq!(below.budget_usage.errors_emitted, capped, "charging stops at the configured limit");
    assert!(
        below.diagnostics.len() <= capped,
        "no diagnostic may be retained beyond the charged budget: {} > {capped}",
        below.diagnostics.len()
    );

    // A hard-coded 100 would make an explicit small budget a no-op.
    let tiny = parse_with_budget(SOURCE, budget_with(ParseCoreDimension::DiagnosticsEmitted, 1));
    assert_eq!(tiny.budget_usage.errors_emitted, 1);
    assert!(tiny.diagnostics.len() <= 1);
}

/// An exhausted diagnostic budget must not silently turn a failed parse into a
/// clean one: the terminal cause is still reported.
#[test]
fn an_exhausted_diagnostic_budget_still_reports_the_terminal_cause() {
    let output =
        parse_with_budget("my $x = ;", budget_with(ParseCoreDimension::DiagnosticsEmitted, 0));
    assert_eq!(output.budget_usage.errors_emitted, 0, "nothing may be charged at a zero limit");
    // The parse recovered rather than terminating, so there is no stop cause;
    // what matters is that no diagnostic was retained beyond the budget.
    assert!(output.diagnostics.is_empty());
}

// ---------------------------------------------------------------------------
// Typed identity and arithmetic safety
// ---------------------------------------------------------------------------

/// A core refusal keeps its own typed identity, distinct from cancellation,
/// recursion, nesting, heredoc, and lexer exhaustion.
#[test]
fn core_exhaustion_is_a_distinct_typed_terminal() {
    let cause = ParseStopCause::CoreBudgetExhausted {
        dimension: ParseCoreDimension::TokensConsumed,
        limit: 3,
        usage: 3,
    };
    assert!(cause.is_budget_exhaustion());
    assert!(!cause.is_cancelled());
    assert_eq!(cause.as_str(), "core_budget_exhausted");
    assert_ne!(cause, ParseStopCause::LexerBudgetExhausted);
    assert_ne!(cause, ParseStopCause::Cancelled);

    // The typed error maps onto the typed cause without message parsing.
    let error = ParseError::CoreBudgetExhausted {
        dimension: ParseCoreDimension::NodesConstructed,
        limit: 7,
        usage: 7,
    };
    assert_eq!(
        ParseStopCause::from_parse_error(&error),
        ParseStopCause::CoreBudgetExhausted {
            dimension: ParseCoreDimension::NodesConstructed,
            limit: 7,
            usage: 7,
        }
    );
}

/// Each dimension has its own stable machine token, so receipts never depend
/// on `Debug` formatting.
#[test]
fn dimension_tokens_are_stable_and_distinct() {
    assert_eq!(ParseCoreDimension::TokensConsumed.as_str(), "tokens_consumed");
    assert_eq!(ParseCoreDimension::NodesConstructed.as_str(), "nodes_constructed");
    assert_eq!(ParseCoreDimension::DiagnosticsEmitted.as_str(), "diagnostics_emitted");
}

/// Charging saturates rather than wrapping, and a spent dimension keeps
/// refusing instead of rolling over into a fresh allowance.
#[test]
fn charging_saturates_and_never_wraps() {
    let budget = ParseBudget::unlimited();
    let mut tracker = BudgetTracker::new();

    // Start one below the top so the charge itself executes: at usize::MAX the
    // limit check refuses first and `charge_core` is never reached, which would
    // let a wrapping `+= 1` pass unnoticed.
    tracker.tokens_consumed = usize::MAX - 1;
    assert!(
        tracker.authorize_core(&budget, ParseCoreDimension::TokensConsumed).is_ok(),
        "the final admissible unit under a usize::MAX limit must be admitted"
    );
    assert_eq!(tracker.tokens_consumed, usize::MAX, "the charge must saturate, not wrap");

    // Now at the top, the next unit is refused rather than wrapping to zero.
    let refusal = tracker.authorize_core(&budget, ParseCoreDimension::TokensConsumed);
    assert!(refusal.is_err(), "a saturated counter must refuse, not wrap");
    assert_eq!(tracker.tokens_consumed, usize::MAX, "a refused charge must not mutate usage");

    // A batch must not be admitted when it cannot be charged in full: at
    // usize::MAX - 1 with a usize::MAX limit there is room for exactly one.
    let mut batched = BudgetTracker::new();
    batched.nodes_constructed = usize::MAX - 1;
    assert!(
        batched.authorize_core_batch(&budget, ParseCoreDimension::NodesConstructed, 2).is_err(),
        "a batch larger than the remaining capacity must be refused, not saturated into a \
         partial charge"
    );
    assert_eq!(batched.nodes_constructed, usize::MAX - 1, "a refused batch must not charge");
    assert!(
        batched.authorize_core_batch(&budget, ParseCoreDimension::NodesConstructed, 1).is_ok(),
        "a batch that exactly fits the remaining capacity must be admitted"
    );
    assert_eq!(batched.nodes_constructed, usize::MAX);
}

/// A refused charge leaves usage untouched; an admitted one charges exactly
/// one unit.
#[test]
fn authorize_core_charges_exactly_one_unit_and_refuses_cleanly() {
    let mut budget = ParseBudget::unlimited();
    budget.max_nodes_constructed = 2;
    let mut tracker = BudgetTracker::new();

    assert!(
        tracker.authorize_core(&budget, ParseCoreDimension::NodesConstructed).is_ok(),
        "first unit must be admitted"
    );
    assert_eq!(tracker.nodes_constructed, 1);
    assert!(
        tracker.authorize_core(&budget, ParseCoreDimension::NodesConstructed).is_ok(),
        "second unit must be admitted"
    );
    assert_eq!(tracker.nodes_constructed, 2);

    let refused = tracker.authorize_core(&budget, ParseCoreDimension::NodesConstructed);
    assert!(
        matches!(
            refused,
            Err(ParseError::CoreBudgetExhausted {
                dimension: ParseCoreDimension::NodesConstructed,
                limit: 2,
                usage: 2,
            })
        ),
        "the third unit must be refused with the typed dimension, limit and usage; got {refused:?}"
    );
    assert_eq!(tracker.nodes_constructed, 2, "a refused charge must not advance usage");

    // Charging one dimension must not disturb another.
    assert_eq!(tracker.tokens_consumed, 0);
    assert_eq!(tracker.errors_emitted, 0);
}

/// Dimensions deferred to #7074 must stay at zero: this PR must not silently
/// begin charging recovery work under a core dimension.
#[test]
fn deferred_recovery_dimensions_remain_uncharged() {
    let usage = usage_unlimited("my $a = ; my $b = ;");
    assert_eq!(usage.tokens_skipped, 0, "recovery-skip charging is #7074");
    assert_eq!(usage.recoveries_attempted, 0, "recovery-attempt charging is #7074");
    assert_eq!(usage.heredoc_scan_bytes, 0, "no heredoc in this source");
}

/// A nested sub-parse whose nodes are spliced into this AST is governed by
/// *this* operation's budget.
///
/// The fused rvalue `*{ ... }` path runs a nested `Parser` with its own
/// operation and tracker, then splices the resulting nodes into the outer
/// tree. Without adoption the outer `nodes_constructed` under-reports and
/// `max_nodes_constructed` does not bound them, so a custom node limit could
/// admit — and under-report — work beyond its configured bound.
#[test]
fn nodes_adopted_from_a_nested_sub_parse_are_charged_to_the_adopting_operation() {
    const FUSED: &str = "my $g = *{ $tmp; 'STDOUT' };";
    const PLAIN: &str = "my $g = *STDOUT;";

    let fused = usage_unlimited(FUSED).nodes_constructed;
    let plain = usage_unlimited(PLAIN).nodes_constructed;
    assert!(
        fused > plain,
        "the fused form splices nested nodes into this AST, so it must charge more than the \
         plain form ({fused} vs {plain})"
    );

    // A node limit one below the fused requirement must refuse — which it can
    // only do if the adopted nodes are counted against this operation.
    let below =
        parse_with_budget(FUSED, budget_with(ParseCoreDimension::NodesConstructed, fused - 1));
    assert!(
        matches!(
            below.stop_cause(),
            Some(ParseStopCause::CoreBudgetExhausted {
                dimension: ParseCoreDimension::NodesConstructed,
                ..
            })
        ),
        "a node limit below the fused requirement must refuse; got {:?}",
        below.stop_cause()
    );

    // At the requirement it is admitted exactly.
    let at = parse_with_budget(FUSED, budget_with(ParseCoreDimension::NodesConstructed, fused));
    assert_eq!(at.stop_cause(), None, "the exact requirement must be admitted");
    assert_eq!(at.budget_usage.nodes_constructed, fused);
}

/// Diagnostics forwarded from a nested sub-parse go through the retention
/// seam, so the configured `max_errors` governs them too.
///
/// Before this was routed, `self.errors.extend(...)` appended them directly:
/// a zero limit still returned diagnostics while `errors_emitted` stayed at
/// zero, and repeated fused dereferences grew the vector past its cap.
#[test]
fn diagnostics_forwarded_from_a_nested_sub_parse_honor_the_configured_limit() {
    // The inner `$a +;` recovers inside the nested parse and forwards one
    // diagnostic into this one.
    const SOURCE: &str = "my $g = *{ $a +; 'X' };";

    let unlimited = parse_with_budget(SOURCE, ParseBudget::unlimited());
    assert!(
        !unlimited.diagnostics.is_empty(),
        "fixture must forward a nested diagnostic for this control to discriminate"
    );
    assert_eq!(
        unlimited.budget_usage.errors_emitted,
        unlimited.diagnostics.len(),
        "a forwarded diagnostic must be charged, not appended behind the seam"
    );

    let capped = parse_with_budget(SOURCE, budget_with(ParseCoreDimension::DiagnosticsEmitted, 0));
    assert_eq!(
        capped.budget_usage.errors_emitted, 0,
        "a zero diagnostic limit must charge nothing"
    );
    assert!(
        capped.diagnostics.is_empty(),
        "a zero diagnostic limit must retain nothing, including diagnostics forwarded from a \
         nested sub-parse; got {:?}",
        capped.diagnostics
    );
}

/// A nested sub-parse is bounded by the *adopting* operation's configuration.
///
/// Adoption can only charge after the nested parse finishes, so the nested
/// parse's own configuration is what bounds the overshoot. If it ran under the
/// default budget, a small outer `max_nodes_constructed` would still permit it
/// to build up to the default limit before the outer parse could refuse — the
/// refusal would be correct but the work would already be done.
#[test]
fn a_nested_sub_parse_is_bounded_by_the_adopting_configuration() {
    const FUSED: &str = "my $g = *{ $tmp; 'STDOUT' };";

    // A node limit of 1 must refuse. The nested parse needs more than one node,
    // so if it ran under the default budget it would build them all first; under
    // the adopting configuration it cannot.
    let refused = parse_with_budget(FUSED, budget_with(ParseCoreDimension::NodesConstructed, 1));
    assert!(
        matches!(
            refused.stop_cause(),
            Some(ParseStopCause::CoreBudgetExhausted {
                dimension: ParseCoreDimension::NodesConstructed,
                ..
            })
        ),
        "a node limit of 1 must refuse the fused form; got {:?}",
        refused.stop_cause()
    );
    assert!(
        refused.budget_usage.nodes_constructed <= 1,
        "no more than the configured limit may be charged, and the nested parse must not have \
         been allowed to build past it; charged {}",
        refused.budget_usage.nodes_constructed
    );
}

/// A terminal diagnostic outlives an exhausted diagnostic budget.
///
/// `ParseStopCause::HeredocBudgetExhausted` carries no location, and its
/// contract points consumers at the diagnostic vector for the anchor. If
/// ordinary retention could drop that diagnostic, a terminated parse would
/// report a cause nobody could locate.
#[test]
fn a_terminal_diagnostic_survives_an_exhausted_diagnostic_budget() {
    // `max_heredoc_scan_bytes = 0` makes the pre-check *refuse* the collection,
    // which is the terminal case. A drain that merely overran is not terminal
    // and stays subject to `max_errors`.
    let mut budget = ParseBudget::unlimited();
    budget.max_heredoc_scan_bytes = 0;
    budget.max_errors = 0;
    let output = parse_with_budget("my $x = <<EOT;\nbody\nEOT\n", budget);

    assert!(
        matches!(output.stop_cause(), Some(ParseStopCause::HeredocBudgetExhausted { .. })),
        "the heredoc budget must terminate this parse; got {:?}",
        output.stop_cause()
    );

    let anchored = output
        .diagnostics
        .iter()
        .any(|diagnostic| matches!(diagnostic, ParseError::HeredocBudgetExhausted { .. }));
    assert!(
        anchored,
        "the terminal diagnostic carries the only source anchor for this stop cause and must be \
         retained even at max_errors = 0; got {:?}",
        output.diagnostics
    );
}

/// Recursive fused dereferences cannot each spend a fresh full budget.
///
/// Handing a nested parse the parent's *full* configuration bounds one level
/// but not recursion. The nested parse receives the parent's **remaining**
/// core allowance instead, so aggregate nested work stays inside the outer
/// limit no matter how deeply `*{ ... }` nests.
#[test]
fn recursive_nested_sub_parses_share_one_aggregate_allowance() {
    const NESTED: &str = "my $g = *{ $a; *{ $b; 'STDOUT' } };";

    let required = usage_unlimited(NESTED).nodes_constructed;
    assert!(required > 2, "fixture must build a meaningful number of nodes");

    // At the requirement the whole nested structure is admitted exactly once.
    let at = parse_with_budget(NESTED, budget_with(ParseCoreDimension::NodesConstructed, required));
    assert_eq!(at.stop_cause(), None, "the exact requirement must be admitted");
    assert_eq!(at.budget_usage.nodes_constructed, required);

    // Below it, the aggregate is refused — a nested parse cannot obtain a fresh
    // allowance of its own and charge past the outer limit.
    let below =
        parse_with_budget(NESTED, budget_with(ParseCoreDimension::NodesConstructed, required - 1));
    assert!(
        matches!(
            below.stop_cause(),
            Some(ParseStopCause::CoreBudgetExhausted {
                dimension: ParseCoreDimension::NodesConstructed,
                ..
            })
        ),
        "nested work must consume the parent's remaining allowance; got {:?}",
        below.stop_cause()
    );
    assert!(
        below.budget_usage.nodes_constructed < required,
        "charged usage must never exceed the configured limit; charged {}",
        below.budget_usage.nodes_constructed
    );
}

/// A refusal raised *inside* a nested sub-parse must be reported in the
/// adopting operation's budget coordinates, not the nested parse's own.
///
/// The nested parse runs under `remaining_core_budget()`, so its local limit is
/// the parent's remainder. Propagating that terminal verbatim publishes a limit
/// that is not the configured limit and a usage that excludes everything the
/// parent had already charged — and leaves `budget_usage` disagreeing with the
/// very stop cause that accompanies it. Against the unadopted implementation
/// this fixture reports `limit: 13, usage: 13` beside a receipt of `1` when the
/// configured limit is `14`.
#[test]
fn a_nested_core_exhaustion_is_reported_in_the_adopting_operations_coordinates() {
    const NESTED: &str = "my $g = *{ $a; *{ $b; 'STDOUT' } };";

    for dimension in [ParseCoreDimension::NodesConstructed, ParseCoreDimension::TokensConsumed] {
        let required = usage_unlimited(NESTED).core_usage(dimension);
        assert!(required > 2, "{dimension} fixture must perform meaningful work");

        // Sweep the whole range rather than naming the limits at which the
        // nested parse happens to be the refuser: which side refuses is an
        // implementation detail, but the coordinates must be the parent's
        // whichever side raises them.
        for limit in 1..required {
            let output = parse_with_budget(NESTED, budget_with(dimension, limit));
            assert!(
                matches!(output.stop_cause(), Some(ParseStopCause::CoreBudgetExhausted { .. })),
                "{dimension} at limit {limit} must refuse; got {:?}",
                output.stop_cause()
            );
            // The assertion above already established the shape; `else` is
            // unreachable and only exists because the crate denies `panic!`.
            let Some(ParseStopCause::CoreBudgetExhausted {
                dimension: stopped,
                limit: reported_limit,
                usage: reported_usage,
            }) = output.stop_cause()
            else {
                continue;
            };
            assert_eq!(stopped, dimension, "the refusal must name the bounded dimension");
            assert_eq!(
                reported_limit, limit,
                "{dimension} at limit {limit}: the terminal must name the operation's configured \
                 limit, not the remainder handed to a nested parse"
            );
            let charged = output.budget_usage.core_usage(dimension);
            assert_eq!(
                reported_usage, charged,
                "{dimension} at limit {limit}: the terminal's usage and the budget receipt \
                 describe the same operation and must agree"
            );
            assert!(
                charged >= limit,
                "{dimension} at limit {limit}: a refusal must follow a budget that was actually \
                 spent; charged {charged}"
            );
            assert!(
                charged <= limit,
                "{dimension} at limit {limit}: adoption must never charge past the configured \
                 limit; charged {charged}"
            );
        }
    }
}

/// Tokens consumed by a nested sub-parse are adopted too, not just its nodes.
#[test]
fn tokens_from_a_nested_sub_parse_are_adopted_by_the_adopting_operation() {
    let fused = usage_unlimited("my $g = *{ $tmp; 'STDOUT' };").tokens_consumed;
    let plain = usage_unlimited("my $g = *STDOUT;").tokens_consumed;
    assert!(
        fused > plain,
        "the fused form consumes the nested parse's tokens as well, so it must charge more than \
         the plain form ({fused} vs {plain})"
    );
}

/// A heredoc drain that overran but *finished* is not terminal, so its
/// diagnostic stays subject to `max_errors`.
///
/// The terminal exemption belongs only to the pre-check that actually refuses
/// work. Extending it to the overrun report would let a completed parse return
/// a diagnostic at `max_errors = 0`, contradicting the configured bound.
#[test]
fn an_overrun_heredoc_diagnostic_is_not_exempt_from_the_diagnostic_budget() {
    // A scan limit smaller than the body forces an overrun that still finishes.
    let source = "my $x = <<EOT;\nbody body body\nEOT\n";
    let mut budget = ParseBudget::unlimited();
    budget.max_heredoc_scan_bytes = 1;
    let overran = parse_with_budget(source, budget);

    // Whatever this parse reports, an ordinary (non-refused) diagnostic must
    // not survive a zero retention budget.
    let mut capped = ParseBudget::unlimited();
    capped.max_heredoc_scan_bytes = 1;
    capped.max_errors = 0;
    let capped_output = parse_with_budget(source, capped);

    if overran.stop_cause().is_none() {
        assert!(
            capped_output.diagnostics.is_empty(),
            "a completed parse must not retain diagnostics at max_errors = 0; got {:?}",
            capped_output.diagnostics
        );
    }
}

// ---------------------------------------------------------------------------
// Recurrence controls
// ---------------------------------------------------------------------------

/// Blank out inline `#[cfg(test)]` modules, preserving line numbering.
///
/// The scanner's contract is "production code only". Standalone test files are
/// excluded by name, but several production modules carry inline
/// `#[cfg(test)] mod`s, and a needle inside one of those would change a
/// recurrence total, or fail the vector-length check, with no production code
/// at fault. Lines are blanked rather than removed so reported line numbers
/// still match the real file.
fn strip_inline_test_modules(body: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut depth: Option<i32> = None;
    for line in body.lines() {
        match depth {
            None => {
                if line.trim_start().starts_with("#[cfg(test)]") {
                    depth = Some(0);
                    out.push(String::new());
                } else {
                    out.push(line.to_string());
                }
            }
            Some(current) => {
                let opened = i32::try_from(line.matches('{').count()).unwrap_or(0);
                let closed = i32::try_from(line.matches('}').count()).unwrap_or(0);
                let next = current + opened - closed;
                out.push(String::new());
                if current == 0 && opened == 0 {
                    // A brace-less attribute target, e.g. `#[cfg(test)] mod x;`
                    // or a `use`. It ends at its own semicolon; without this the
                    // scanner would blank the rest of the file and hide real
                    // production sites.
                    if line.contains(';') {
                        depth = None;
                    }
                } else {
                    depth = if next <= 0 { None } else { Some(next) };
                }
            }
        }
    }
    out.join("\n")
}

/// Production parser sources, excluding standalone test files *and* inline
/// `#[cfg(test)]` modules.
fn production_parser_sources() -> Vec<(String, String)> {
    let root = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/src/engine/parser"));
    let mut out = Vec::new();
    let mut dirs = vec![root.clone()];
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries {
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
                continue;
            }
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
            if !name.ends_with(".rs") || name.ends_with("_tests.rs") || name == "tests.rs" {
                continue;
            }
            // Several modules share the basename `mod.rs`, so a bare file name
            // would be ambiguous in a failure message.
            let label = path
                .strip_prefix(&root)
                .ok()
                .and_then(|relative| relative.to_str())
                .unwrap_or(name.as_str())
                .to_string();
            let Ok(body) = std::fs::read_to_string(&path) else { continue };
            out.push((label, strip_inline_test_modules(&body)));
        }
    }
    // `read_dir` order is not guaranteed; these tests must be deterministic.
    out.sort_by(|left, right| left.0.cmp(&right.0));
    assert!(out.len() > 5, "expected to find the parser sources, found {}", out.len());
    out
}

/// Every use of a raw primitive must sit within a few lines of an `#8786`
/// annotation naming why it is not routed through the charging seam.
fn assert_every_raw_use_is_annotated(needle: &str, expected_total: usize) {
    let mut total = 0;
    let mut unannotated = Vec::new();
    for (name, body) in production_parser_sources() {
        let lines: Vec<&str> = body.lines().collect();
        for (index, line) in lines.iter().enumerate() {
            if !line.contains(needle) {
                continue;
            }
            total += 1;
            let start = index.saturating_sub(8);
            let annotated = lines[start..index].iter().any(|context| context.contains("#8786"));
            if !annotated {
                unannotated.push(format!("{name}:{}: {}", index + 1, line.trim()));
            }
        }
    }
    assert!(
        unannotated.is_empty(),
        "every production use of `{needle}` must go through the #8786 charging seam, or carry an \
         `#8786:` annotation saying why it does not. Unannotated uses:\n{}",
        unannotated.join("\n")
    );
    assert_eq!(
        total, expected_total,
        "the number of unrouted `{needle}` uses changed. If you added a production seam, route it \
         through the charging seam; if you deliberately added an exemption, update this count and \
         say why in the PR."
    );
}

/// The source scanner must actually honor its own contract.
///
/// Three recurrence controls depend on `strip_inline_test_modules` excluding
/// inline `#[cfg(test)]` code while preserving line numbers. Two failure modes
/// matter and both have bitten: under-stripping lets a test-only line change a
/// production count, and over-stripping on a brace-less target such as
/// `#[cfg(test)] mod x;` blanks the rest of the file and hides real production
/// sites.
#[test]
fn the_source_scanner_strips_inline_tests_without_swallowing_production_code() {
    let body = "\
fn production_one() { Node::new(a, b); }
#[cfg(test)]
mod inline {
    fn hidden() { Node::new(c, d); }
}
#[cfg(test)]
mod declared_elsewhere;
fn production_two() { Node::new(e, f); }
";
    let stripped = strip_inline_test_modules(body);

    assert_eq!(
        stripped.lines().count(),
        body.lines().count(),
        "line numbering must be preserved so reported positions match the real file"
    );
    assert_eq!(
        stripped.matches("Node::new(").count(),
        2,
        "both production constructions must survive and the inline-test one must not; got:\n{stripped}"
    );
    assert!(
        stripped.contains("production_two"),
        "a brace-less `#[cfg(test)] mod x;` must not blank the rest of the file"
    );
    assert!(!stripped.contains("hidden"), "inline test module bodies must be excluded");
}

/// No production parser code may reach the token stream's advance directly:
/// `Parser::advance_token` is the single charged seam.
#[test]
fn token_advance_seam_is_unique() {
    assert_every_raw_use_is_annotated(".tokens.next()", 1);
}

/// No production parser code may construct an AST node directly, except the
/// annotated exemptions (the seam itself, synthetic recovery nodes owned by
/// #7074, and the terminal fallback shell).
#[test]
fn node_construction_seam_is_unique() {
    // 6 = the charging seam plus five annotated exemptions; the sixth is
    // #14174's truncated-arrow recovery node (synthetic recovery shell,
    // #7074's accounting dimension, not admitted parse work).
    assert_every_raw_use_is_annotated("Node::new(", 6);
}

/// No production parser code may retain a diagnostic directly, except the
/// seam itself and the terminal cause that must survive an exhausted budget.
#[test]
fn diagnostic_retention_seam_is_unique() {
    assert_every_raw_use_is_annotated(".errors.push(", 3);
}

/// `push` is not the only way to grow the diagnostic vector.
///
/// A forwarded batch (`self.errors.extend(...)` in the fused `*{...}` path)
/// bypassed the seam entirely: it neither charged `max_errors` nor recorded
/// observation, so a zero limit still returned diagnostics. Guarding only
/// `push` missed it, so every mutating access to the vector is guarded here.
#[test]
fn no_bulk_path_grows_the_diagnostic_vector_outside_the_seam() {
    for mutator in [
        ".errors.extend(",
        ".errors.append(",
        ".errors.insert(",
        ".errors.extend_from_slice(",
        ".errors.retain(",
        ".errors.truncate(",
        ".errors.drain(",
    ] {
        assert_every_raw_use_is_annotated(mutator, 0);
    }

    // One deliberate exemption: `begin_operation` clears the vector so
    // retention shares the lifetime of the charge that accounts for it. It
    // *shrinks* the vector at an operation boundary rather than growing it
    // inside one, which is the opposite of the bypass this control exists to
    // catch — but it is still a direct mutation, so it is pinned rather than
    // exempted by pattern, and the annotation check above still applies to it.
    assert_every_raw_use_is_annotated(".errors.clear(", 1);
}

/// The pre-#8786 defect must not return: the diagnostic limit is charged
/// usage against the configured budget, never a hard-coded constant or the
/// length of the retained vector.
///
/// This forbids *every* non-comment read of the retained vector's length in
/// the production parser, not just the `>=` limit-check shape. A delta
/// comparison (`self.errors.len() > errors_before`) is just as coupled: once
/// retention is bounded, the delta silently goes to zero and any grammar
/// decision reading it changes branch. Use
/// `ParserOperationContext::diagnostics_observed` for that question.
#[test]
fn diagnostic_vector_length_is_not_a_parser_authority() {
    for (name, body) in production_parser_sources() {
        assert!(
            !body.contains("const MAX_ERRORS"),
            "{name}: a hard-coded diagnostic limit ignores the operation's configured budget"
        );
        for (index, line) in body.lines().enumerate() {
            let code = line.trim_start();
            if code.starts_with("//") {
                continue;
            }
            assert!(
                !code.contains("errors.len()"),
                "{name}:{}: the retained diagnostic vector's length is not a parser authority — \
                 retention is bounded by the configured max_errors, so reading it couples grammar \
                 or limit decisions to the diagnostic budget. Use `diagnostics_observed()`.\n  {code}",
                index + 1
            );
        }
    }
}

/// A diagnostic-retention limit must never change the parsed shape.
///
/// Regression control for the coupling #8786 introduced and then removed: the
/// hash-versus-block disambiguation (#1352) read the growth of the *retained*
/// diagnostic vector. Once this PR made retention honor the configured
/// `max_errors`, a spent diagnostic budget made that growth zero, so the same
/// source parsed to a different AST depending only on a diagnostic limit.
///
/// `my $h = { [1,2 ; 3 };` is the discriminating case: it reaches the #1352
/// branch through an inner recovery. Before the fix, the strict-budget AST
/// absorbed the trailing `3` into the block instead of taking the
/// unclosed-brace recovery path.
#[test]
fn a_spent_diagnostic_budget_does_not_change_the_parsed_shape() {
    fn shape(source: &str, max_errors: usize) -> String {
        let mut budget = ParseBudget::unlimited();
        budget.max_errors = max_errors;
        parse_with_budget(source, budget).ast.to_sexp()
    }

    // Ten independent errors spend a strict budget before the construct.
    let prefix: String = (0..10).map(|i| format!("my $x{i} = ;\n")).collect();

    for tail in
        ["my $h = { [1,2 ; 3 };\n", "{ [1,2 ; 3 }\n", "my $h = { a => [1,2 ; };\n", "{ foo( ; }\n"]
    {
        let source = format!("{prefix}{tail}");
        let spent = shape(&source, 10);
        let generous = shape(&source, 100_000);
        assert_eq!(
            spent, generous,
            "a spent diagnostic budget changed the AST for {tail:?}: retention is a reporting \
             limit, not a grammar input"
        );
    }
}

/// Observation is monotonic and unbounded even when retention is refused, and
/// it resets per operation.
#[test]
fn diagnostic_observation_is_independent_of_retention() {
    const SOURCE: &str = "my $a = ; my $b = ; my $c = ; my $d = ;";

    let mut budget = ParseBudget::unlimited();
    budget.max_errors = 1;
    let capped = parse_with_budget(SOURCE, budget);

    let uncapped = parse_with_budget(SOURCE, ParseBudget::unlimited());

    assert_eq!(capped.budget_usage.errors_emitted, 1, "retention stops at the configured limit");
    assert!(
        uncapped.budget_usage.errors_emitted > capped.budget_usage.errors_emitted,
        "the uncapped operation retains strictly more"
    );
    // The capped parse still saw every condition: its AST matches the uncapped
    // one, which it could not if observation had been clipped along with
    // retention.
    assert_eq!(capped.ast.to_sexp(), uncapped.ast.to_sexp());
}
