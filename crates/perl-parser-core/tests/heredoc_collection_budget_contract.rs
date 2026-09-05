//! Deterministic heredoc collection budget contract (#7291).
//!
//! Parser-core previously abandoned heredoc collection when more than five
//! wall-clock seconds had elapsed since the pending queue became non-empty. The
//! timer spanned the whole enclosing statement — including nested blocks that
//! declare no heredocs — so tracing, a sanitizer, a debugger pause, or a loaded
//! host could drop bodies from valid source and report the loss as a syntax
//! error against the user's code.
//!
//! These tests pin the replacement contract: collection work is charged in
//! source bytes, exhaustion is a typed resource-limit terminal rather than a
//! syntax claim, and identical source plus configuration yields identical AST,
//! diagnostics, and usage on every host.

use perl_ast::NodeKind;
use perl_parser_core::error::{
    ErrorCategory, ErrorClass, ParseBudget, ParseDiagnosticAnchor, ParseStopCause,
};
use perl_parser_core::{ParseError, ParseOutput, Parser, ParserConfigIdentity};
use perl_tdd_support::{must_some, must_some_with};

/// One heredoc-bearing statement. Kept separate so its exact charge can be
/// measured on its own and reused as a boundary threshold below.
const FIRST_STATEMENT: &str = "my $a = <<EOF;\nbody a line one\nbody a line two\nEOF\n";

/// A second, independently drained heredoc statement.
const SECOND_STATEMENT: &str = "my $b = <<EOF2;\nbody b\nEOF2\n";

fn two_heredoc_statements() -> String {
    format!("{FIRST_STATEMENT}{SECOND_STATEMENT}")
}

fn parse_with_budget(source: &str, budget: ParseBudget) -> ParseOutput {
    let config = ParserConfigIdentity::production_default().with_budget(budget);
    let mut parser = Parser::with_production_config(source, config);
    parser.parse_with_recovery()
}

fn unlimited_budget() -> ParseBudget {
    ParseBudget::unlimited()
}

/// Budget that only constrains heredoc scanning, so an exhausted heredoc budget
/// cannot be confused with an error/recovery/depth limit firing instead.
fn heredoc_scan_budget(max_heredoc_scan_bytes: usize) -> ParseBudget {
    let mut budget = ParseBudget::unlimited();
    budget.max_heredoc_scan_bytes = max_heredoc_scan_bytes;
    budget
}

fn heredoc_contents(output: &ParseOutput) -> Vec<String> {
    fn walk(node: &perl_ast::Node, found: &mut Vec<String>) {
        if let NodeKind::Heredoc { content, .. } = &node.kind {
            found.push(content.clone());
        }
        node.for_each_child(|child| walk(child, found));
    }

    let mut found = Vec::new();
    walk(&output.ast, &mut found);
    found
}

// ---------------------------------------------------------------------------
// The wall clock is gone from heredoc source semantics.
// ---------------------------------------------------------------------------

/// Architecture ratchet mirroring `perl-lexer`'s
/// `production_heredoc_scanning_contains_no_wall_clock_cutoff`.
///
/// This is the direct proof of #7291's first acceptance line: no parser-owned
/// wall clock participates in heredoc source semantics. A behavioural test
/// cannot establish this — it would have to wait out a real timeout to observe
/// the branch it is trying to prove absent — so the production source is the
/// oracle, exactly as the lexer proves the same property.
#[test]
fn production_heredoc_parsing_contains_no_wall_clock_cutoff() {
    const HEREDOC: &str = include_str!("../src/engine/parser/heredoc.rs");
    const PARSER_MOD: &str = include_str!("../src/engine/parser/mod.rs");
    const OPERATION: &str = include_str!("../src/engine/parser/operation.rs");

    for (name, source) in [
        ("engine/parser/heredoc.rs", HEREDOC),
        ("engine/parser/mod.rs", PARSER_MOD),
        ("engine/parser/operation.rs", OPERATION),
    ] {
        // Comments in these files discuss the removed timer by name, so the
        // ratchet targets executable spellings rather than the bare words.
        for forbidden in [
            "HEREDOC_TIMEOUT_MS",
            "heredoc_start_time",
            "Instant::now",
            ".elapsed()",
            "std::time::Instant",
        ] {
            assert!(
                !source.contains(forbidden),
                "{name} must not reintroduce wall-clock heredoc control: found `{forbidden}`"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Ordinary source is unaffected and deterministic.
// ---------------------------------------------------------------------------

#[test]
fn ordinary_heredocs_attach_bodies_and_report_no_terminal() {
    let output = parse_with_budget(&two_heredoc_statements(), unlimited_budget());

    assert_eq!(output.stop_cause(), None, "valid heredoc source must not terminate early");
    assert_eq!(
        heredoc_contents(&output),
        vec!["body a line one\nbody a line two".to_string(), "body b".to_string()],
        "both heredoc bodies must attach"
    );
    assert!(
        output.budget_usage.heredoc_scan_bytes > 0,
        "collection work must actually be charged, not silently uncounted"
    );
}

/// The determinism claim: usage is a pure function of source and configuration.
///
/// Under the wall clock this could not hold — charged "usage" was elapsed time,
/// which varies per run. Repeating the parse pins that the replacement carries
/// no host-dependent input.
#[test]
fn repeated_parses_are_byte_identical_in_ast_diagnostics_and_usage() {
    let source = two_heredoc_statements();

    let first = parse_with_budget(&source, unlimited_budget());
    let baseline_contents = heredoc_contents(&first);
    let baseline_usage = first.budget_usage.heredoc_scan_bytes;
    let baseline_diagnostics = format!("{:?}", first.diagnostics);

    for run in 0..16 {
        let repeat = parse_with_budget(&source, unlimited_budget());
        assert_eq!(
            heredoc_contents(&repeat),
            baseline_contents,
            "AST content drifted on run {run}"
        );
        assert_eq!(
            repeat.budget_usage.heredoc_scan_bytes, baseline_usage,
            "charged usage drifted on run {run}"
        );
        assert_eq!(
            format!("{:?}", repeat.diagnostics),
            baseline_diagnostics,
            "diagnostics drifted on run {run}"
        );
        assert_eq!(repeat.stop_cause(), first.stop_cause(), "terminal drifted on run {run}");
    }
}

/// An exhausted operation must leave no residue for a later ordinary parse.
///
/// #7291 requires that exhaustion unwind cleanly rather than poisoning
/// subsequent work through any shared or static route.
#[test]
fn an_exhausted_operation_leaves_no_residue_for_a_later_parse() {
    let source = two_heredoc_statements();

    let exhausted = parse_with_budget(&source, heredoc_scan_budget(0));
    assert!(exhausted.stop_cause().is_some(), "the fixture must actually exhaust first");

    let ordinary = parse_with_budget(&source, unlimited_budget());
    assert_eq!(
        ordinary.stop_cause(),
        None,
        "a later ordinary parse must be unaffected by an earlier exhaustion"
    );
    assert_eq!(
        heredoc_contents(&ordinary),
        vec!["body a line one\nbody a line two".to_string(), "body b".to_string()],
        "a later ordinary parse must attach every body"
    );
    assert!(
        ordinary
            .diagnostics
            .iter()
            .all(|error| !matches!(error, ParseError::HeredocBudgetExhausted { .. })),
        "no budget diagnostic may carry over into a later parse"
    );
}

// ---------------------------------------------------------------------------
// Boundary matrix on the before-work refusal rule.
// ---------------------------------------------------------------------------

#[test]
fn tracker_refuses_only_at_or_above_the_configured_limit() {
    let budget = heredoc_scan_budget(100);
    let mut tracker = perl_parser_core::error::BudgetTracker::new();

    tracker.record_heredoc_scan(99);
    assert!(!tracker.heredoc_scan_exhausted(&budget), "limit - 1 must still admit collection");

    tracker.record_heredoc_scan(1);
    assert!(tracker.heredoc_scan_exhausted(&budget), "limit must refuse further collection");

    tracker.record_heredoc_scan(1);
    assert!(tracker.heredoc_scan_exhausted(&budget), "limit + 1 must remain refused");
}

#[test]
fn zero_budget_refuses_the_first_collection_without_claiming_a_syntax_error() {
    let output = parse_with_budget(&two_heredoc_statements(), heredoc_scan_budget(0));

    let cause = must_some(output.stop_cause());
    assert!(
        matches!(cause, ParseStopCause::HeredocBudgetExhausted { limit: 0, .. }),
        "expected a typed heredoc budget terminal, got {cause:?}"
    );
    assert!(cause.is_budget_exhaustion(), "the terminal must classify as budget exhaustion");
    assert_eq!(cause.as_str(), "heredoc_budget_exhausted");
}

/// The below/at/above triple on the real production drain path.
///
/// The total charge for the source is measured under an unlimited budget and
/// used as the boundary. One byte *below* it the collection genuinely overruns
/// and must report; exactly at it, and one byte above it, the parse completes
/// and must present as ordinary.
///
/// The below case is what makes this discriminating. An earlier revision
/// reported exhaustion at the threshold too, so both remaining cases were
/// "clean" and nothing here could fail — a boundary test with no failing side
/// proves nothing about the boundary.
#[test]
fn total_charge_reports_only_below_the_threshold_and_is_clean_at_and_above_it() {
    let source = two_heredoc_statements();

    let measured = parse_with_budget(&source, unlimited_budget());
    let total_charge = measured.budget_usage.heredoc_scan_bytes;
    assert!(total_charge > 1, "the fixture must charge enough to sit below the boundary");

    // One byte below the total: collection crosses the limit while running, so
    // the after-work check must report it.
    let below_threshold =
        parse_with_budget(&source, heredoc_scan_budget(total_charge.saturating_sub(1)));
    assert!(
        matches!(below_threshold.stop_cause(), Some(ParseStopCause::HeredocBudgetExhausted { .. })),
        "a charge that overruns the limit must report exhaustion, got {:?}",
        below_threshold.stop_cause()
    );

    // Landing exactly on the limit truncated nothing: the drains completed and
    // every body is attached, so the parse is complete and must present as one.
    // An earlier revision reported exhaustion here, which put a blocking
    // resource-limit diagnostic on source that parsed perfectly — the same false
    // claim against valid code that the removed wall clock used to make. The
    // budget is spent, so a *further* collection would be refused; nothing that
    // already finished is retroactively a failure.
    let at_threshold = parse_with_budget(&source, heredoc_scan_budget(total_charge));
    assert_eq!(
        at_threshold.stop_cause(),
        None,
        "landing exactly on the limit with every body collected must not report a terminal"
    );
    assert_eq!(
        heredoc_contents(&at_threshold),
        vec!["body a line one\nbody a line two".to_string(), "body b".to_string()],
        "the at-threshold parse must attach both bodies, exactly as an unlimited one does"
    );
    assert!(
        at_threshold
            .diagnostics
            .iter()
            .all(|error| !matches!(error, ParseError::HeredocBudgetExhausted { .. })),
        "a complete parse must not carry a heredoc-budget diagnostic: {:?}",
        at_threshold.diagnostics
    );

    let above_threshold =
        parse_with_budget(&source, heredoc_scan_budget(total_charge.saturating_add(1)));
    assert_eq!(
        above_threshold.stop_cause(),
        None,
        "one byte above the threshold the parse must be ordinary"
    );
    assert_eq!(
        heredoc_contents(&above_threshold),
        vec!["body a line one\nbody a line two".to_string(), "body b".to_string()],
        "an admitted parse must still attach both bodies"
    );
}

/// A single drain that overruns the limit must still report exhaustion.
///
/// The before-work refusal alone cannot catch this: with one heredoc and no
/// later drain, the pre-check sees zero usage, the collection runs to
/// completion, and nothing would ever be reported — a budget silently spent.
/// This is the falsifier for that hole, so it must use a source with exactly
/// one heredoc and therefore exactly one drain.
#[test]
fn a_single_overrunning_drain_reports_exhaustion_without_a_later_drain() {
    let measured = parse_with_budget(FIRST_STATEMENT, unlimited_budget());
    let charge = measured.budget_usage.heredoc_scan_bytes;
    assert!(charge > 1, "the fixture must charge more than the limit set below");
    assert_eq!(
        heredoc_contents(&measured).len(),
        1,
        "this control is only meaningful with exactly one heredoc, hence one drain"
    );

    // A positive limit, so the before-work check cannot fire: usage is 0 and
    // 0 >= 1 is false. Only the after-work check can catch this.
    let overrun = parse_with_budget(FIRST_STATEMENT, heredoc_scan_budget(1));
    assert!(
        matches!(overrun.stop_cause(), Some(ParseStopCause::HeredocBudgetExhausted { .. })),
        "a single drain that overruns the limit must report exhaustion, got {:?}",
        overrun.stop_cause()
    );
    assert!(
        overrun
            .diagnostics
            .iter()
            .any(|error| matches!(error, ParseError::HeredocBudgetExhausted { .. })),
        "the overrun must also surface a typed diagnostic"
    );
    assert!(
        overrun.budget_usage.heredoc_scan_bytes >= 1,
        "the overrunning work must still be charged"
    );

    // The after-work report anchors through its own expression, so it needs its
    // own assertion: the pre-check's anchor is proven separately in
    // `refusal_anchors_at_the_declaration_it_refused`.
    let declaration =
        must_some_with(FIRST_STATEMENT.find("<<EOF"), "fixture must contain the declaration");
    let reported = must_some_with(
        overrun
            .diagnostics
            .iter()
            .find(|error| matches!(error, ParseError::HeredocBudgetExhausted { .. })),
        "the overrun must surface its typed diagnostic",
    );
    assert_eq!(
        reported.location(),
        Some(declaration),
        "an overrunning drain must anchor at the declaration it was collecting"
    );
    assert!(declaration > 0, "the fixture must not place the declaration at offset 0");
}

// ---------------------------------------------------------------------------
// Negative controls: exhaustion must not be laundered into other outcomes.
// ---------------------------------------------------------------------------

/// Exhaustion is a statement about resources, never about the user's syntax.
///
/// The removed branch pushed `ParseError::syntax("Heredoc parsing timed out")`,
/// which routes to [`ErrorCategory::UserError`] — the parser blamed the source
/// for a host-speed event. This pins the corrected routing.
#[test]
fn exhaustion_is_a_resource_limit_not_a_user_syntax_error() {
    let output = parse_with_budget(&two_heredoc_statements(), heredoc_scan_budget(0));

    let budget_errors: Vec<&ParseError> = output
        .diagnostics
        .iter()
        .filter(|error| matches!(error, ParseError::HeredocBudgetExhausted { .. }))
        .collect();
    assert_eq!(budget_errors.len(), 1, "exactly one typed budget diagnostic must be emitted");

    let error = budget_errors[0];
    assert_eq!(
        error.error_class(),
        ErrorCategory::ResourceLimit,
        "budget exhaustion must not route as a user error"
    );
    assert!(
        !matches!(error, ParseError::SyntaxError { .. }),
        "budget exhaustion must not be spelled as a syntax error"
    );

    for diagnostic in &output.diagnostics {
        let rendered = diagnostic.to_string();
        assert!(
            !rendered.contains("timed out") && !rendered.contains("timeout"),
            "no diagnostic may claim a timeout: {rendered}"
        );
        assert!(
            !rendered.contains("Unterminated heredoc"),
            "a refused collection must not be reported as unterminated source: {rendered}"
        );
    }
}

/// The specific failure the wall clock produced: the queue was cleared and the
/// parse continued, so unresolved placeholders became indistinguishable from
/// heredocs that genuinely declared an empty body.
#[test]
fn refused_collection_cannot_masquerade_as_an_ordinary_empty_body() {
    let refused = parse_with_budget(&two_heredoc_statements(), heredoc_scan_budget(0));

    // Assert the placeholders are actually present before asserting they are
    // empty: `all` is vacuously true on an empty list, so an implementation
    // that dropped the queued heredoc nodes entirely would otherwise pass.
    assert_eq!(
        heredoc_contents(&refused),
        vec![String::new(), String::new()],
        "both placeholders must be retained and unresolved for this control to be meaningful"
    );
    assert!(
        refused.stop_cause().is_some(),
        "empty heredoc content must never be returned as a clean, complete parse"
    );
    assert!(refused.terminated_early(), "a refused collection must project as early termination");

    // A genuinely empty body is a different, ordinary outcome: same empty
    // content, but no terminal. Without this control the assertion above could
    // be satisfied by simply always terminating.
    let genuinely_empty = parse_with_budget("my $x = <<EOF;\nEOF\n", unlimited_budget());
    assert_eq!(
        heredoc_contents(&genuinely_empty),
        vec![String::new()],
        "an empty heredoc body is still empty content"
    );
    assert_eq!(
        genuinely_empty.stop_cause(),
        None,
        "a genuinely empty body is an ordinary complete parse, not a terminal"
    );
}

/// Exhaustion must stay distinct from cooperative cancellation.
#[test]
fn exhaustion_is_distinct_from_cancellation() {
    let output = parse_with_budget(&two_heredoc_statements(), heredoc_scan_budget(0));
    let cause = must_some(output.stop_cause());

    assert!(!cause.is_cancelled(), "budget exhaustion must not be reported as cancellation");
    assert_ne!(cause, ParseStopCause::Cancelled);
}

/// A refused collection must not turn later valid heredocs into syntax errors.
///
/// Refusal deliberately leaves the queued declarations in place so their
/// placeholders stay visibly unresolved. Those entries must not keep occupying
/// the depth-limited admission queue: `push_heredoc_decl` refuses past
/// `MAX_HEREDOC_DEPTH` (100) with `ParseError::syntax("Heredoc depth limit
/// exceeded")`, so a queue that only ever grows after exhaustion would blame
/// the user's source for a resource limit — the exact misclassification this
/// budget replaced. Exhaustion must stay a single typed resource-limit
/// terminal no matter how many declarations follow it.
#[test]
fn exhaustion_does_not_turn_later_heredocs_into_depth_syntax_errors() {
    // Comfortably past MAX_HEREDOC_DEPTH so the guard is reached if the queue
    // is never released after refusal.
    let mut source = String::new();
    for index in 0..150 {
        source.push_str(&format!("my $v{index} = <<EOF{index};\nbody {index}\nEOF{index}\n"));
    }

    let output = parse_with_budget(&source, heredoc_scan_budget(1));

    let depth_errors: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|error| error.to_string().contains("depth limit"))
        .collect();

    assert!(
        depth_errors.is_empty(),
        "a refused heredoc collection must not produce depth-limit syntax errors, got {} of them: {:?}",
        depth_errors.len(),
        depth_errors.iter().map(std::string::ToString::to_string).collect::<Vec<_>>()
    );

    assert!(
        matches!(output.stop_cause(), Some(ParseStopCause::HeredocBudgetExhausted { .. })),
        "the operation must still terminate on the heredoc budget, got {:?}",
        output.stop_cause()
    );

    for error in &output.diagnostics {
        assert_ne!(
            error.error_class(),
            ErrorCategory::UserError,
            "no diagnostic from a resource limit may be classified as a user syntax error: {error}"
        );
    }
}

// ---------------------------------------------------------------------------
// The diagnostic anchors at the declaration whose collection was refused.
// ---------------------------------------------------------------------------

/// The refusal must point at the heredoc it refused, not at the cursor.
///
/// `location` and `diagnostic_anchor` are what make this diagnostic
/// actionable: the removed wall clock anchored at a bare `byte_cursor`, which
/// pointed wherever statement parsing happened to have reached and was useless
/// to a reader. Nothing else in this suite reads the offset, so an
/// implementation that anchored at `0`, at EOF, or at the wrong declaration
/// would pass every other test here.
///
/// The refused declaration is deliberately the *second* one in the source. A
/// budget that admits the first drain and refuses the second cannot be
/// satisfied by anchoring at the start of the file or at the first heredoc.
#[test]
fn refusal_anchors_at_the_declaration_it_refused() {
    let source = two_heredoc_statements();
    let second_declaration =
        must_some_with(source.find("<<EOF2"), "fixture must contain the second declaration");

    // Charge of the first statement alone: setting exactly this as the limit
    // lets the first drain finish (landing on the limit truncates nothing) and
    // refuses the second at its pre-check.
    let first_charge =
        parse_with_budget(FIRST_STATEMENT, unlimited_budget()).budget_usage.heredoc_scan_bytes;

    let output = parse_with_budget(&source, heredoc_scan_budget(first_charge));

    assert!(
        matches!(output.stop_cause(), Some(ParseStopCause::HeredocBudgetExhausted { .. })),
        "the fixture must refuse the second collection, got {:?}",
        output.stop_cause()
    );
    assert_eq!(
        heredoc_contents(&output),
        vec!["body a line one\nbody a line two".to_string(), String::new()],
        "the admitted first body must be attached and only the refused one left unresolved"
    );

    let refusal = must_some_with(
        output
            .diagnostics
            .iter()
            .find(|error| matches!(error, ParseError::HeredocBudgetExhausted { .. })),
        "a refused collection must emit its typed diagnostic",
    );

    assert_eq!(
        refusal.location(),
        Some(second_declaration),
        "the diagnostic must anchor at the refused declaration, not at the parse cursor"
    );
    assert_eq!(
        refusal.diagnostic_anchor(),
        ParseDiagnosticAnchor::Exact(second_declaration),
        "the anchor projection must agree with `location`"
    );

    // Guards against an anchor that is accidentally right: neither end of the
    // source may satisfy the assertions above.
    assert!(second_declaration > 0, "the fixture must not place the refusal at offset 0");
    assert!(second_declaration < source.len(), "the fixture must not place the refusal at EOF");
}
