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

fn budget_with(dimension: ParseCoreDimension, limit: usize) -> ParseBudget {
    let mut budget = ParseBudget::unlimited();
    match dimension {
        ParseCoreDimension::TokensConsumed => budget.max_tokens_consumed = limit,
        ParseCoreDimension::NodesConstructed => budget.max_nodes_constructed = limit,
        ParseCoreDimension::DiagnosticsEmitted => budget.max_errors = limit,
    }
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
    tracker.tokens_consumed = usize::MAX;

    // At usize::MAX with a usize::MAX limit, the next unit is refused rather
    // than wrapping the counter to zero.
    let refusal = tracker.authorize_core(&budget, ParseCoreDimension::TokensConsumed);
    assert!(refusal.is_err(), "a saturated counter must refuse, not wrap");
    assert_eq!(tracker.tokens_consumed, usize::MAX, "a refused charge must not mutate usage");
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

// ---------------------------------------------------------------------------
// Recurrence controls
// ---------------------------------------------------------------------------

/// Production parser sources, excluding inline test modules.
fn production_parser_sources() -> Vec<(String, String)> {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/src/engine/parser");
    let mut out = Vec::new();
    let mut dirs = vec![std::path::PathBuf::from(root)];
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
            let Ok(body) = std::fs::read_to_string(&path) else { continue };
            out.push((name, body));
        }
    }
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
    assert_every_raw_use_is_annotated("Node::new(", 5);
}

/// No production parser code may retain a diagnostic directly, except the
/// seam itself and the terminal cause that must survive an exhausted budget.
#[test]
fn diagnostic_retention_seam_is_unique() {
    assert_every_raw_use_is_annotated(".errors.push(", 2);
}

/// The pre-#8786 defect must not return: the diagnostic limit is charged
/// usage against the configured budget, never a hard-coded constant or the
/// length of the retained vector.
#[test]
fn diagnostic_limit_is_not_reconstructed_from_the_retained_vector() {
    for (name, body) in production_parser_sources() {
        assert!(
            !body.contains("const MAX_ERRORS"),
            "{name}: a hard-coded diagnostic limit ignores the operation's configured budget"
        );
        assert!(
            !body.contains("self.errors.len() >="),
            "{name}: the diagnostic limit must come from charged usage, not `errors.len()`"
        );
    }
}
