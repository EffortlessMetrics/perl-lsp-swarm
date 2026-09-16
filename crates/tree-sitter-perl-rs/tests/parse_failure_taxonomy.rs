//! Guard for the recursion-vs-nesting classification boundary (#12952).
//!
//! `behavior_spec_tests` accepts either recursion variant, so it cannot catch a
//! repair that classifies expression-recursion exhaustion as *structural*
//! nesting. `perl-parser-core` forbids exactly that relabeling, so the boundary
//! is pinned here instead.
//!
//! Failures propagate rather than panic, per the repository lint policy.

use tree_sitter_perl_rs::{ParseFailure, Parser};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

/// Expression recursion exhaustion classifies as `RecursionLimit`.
///
/// The parser's expression guard emits `RecursionDepthExhausted`. Before the
/// #12952 repair the facade had no arm for it, so a genuine budget exhaustion
/// surfaced as `Other` — an "unclassified catastrophic failure" — which is the
/// one thing this facade is not allowed to be vague about.
#[test]
fn expression_recursion_exhaustion_is_classified_not_other() -> TestResult {
    let mut parser = Parser::new();

    let outcome = parser.parse_detailed(&"(".repeat(600));

    match outcome.failure {
        Some(ParseFailure::RecursionLimit) => {}
        Some(ParseFailure::NestingTooDeep { .. }) => {
            return Err("expression-recursion exhaustion was relabeled as structural nesting; \
                        perl-parser-core's ParseError::RecursionDepthExhausted docs forbid this"
                .into());
        }
        Some(ParseFailure::Other { diagnostic }) => {
            return Err(
                format!("recursion exhaustion left unclassified as Other: {diagnostic:?}").into()
            );
        }
        other => return Err(format!("expected a typed recursion failure, got {other:?}").into()),
    }

    if outcome.tree.is_some() {
        return Err("a terminated parse must not publish a tree".into());
    }
    Ok(())
}

/// An earlier recovered diagnostic must not mask the terminal cause.
///
/// Regression for the defect Devin Review found on #15593: the facade used to
/// scan `diagnostics` front-to-back through a total `from_diagnostic`, so the
/// *first* entry won. With a recoverable error ahead of the recursion
/// exhaustion the failure surfaced as `Other { Recovered { .. } }` — a
/// recovered diagnostic reported as the catastrophic cause. `stop_cause` is the
/// authority precisely because, in `perl-parser-core`'s words, "the diagnostic
/// population never determines the stop cause".
#[test]
fn an_earlier_recovered_diagnostic_does_not_mask_the_terminal_cause() -> TestResult {
    let mut parser = Parser::new();
    let source = format!("my $x = ;\n{}", "(".repeat(600));

    let outcome = parser.parse_detailed(&source);

    if outcome.diagnostics.len() <= 1 {
        return Err(format!(
            "fixture must produce a recovered diagnostic before the terminal one, got {:?}",
            outcome.diagnostics
        )
        .into());
    }
    if !matches!(outcome.failure, Some(ParseFailure::RecursionLimit)) {
        return Err(format!(
            "terminal cause was masked by an earlier diagnostic: {:?} (diagnostics: {:?})",
            outcome.failure, outcome.diagnostics
        )
        .into());
    }
    if outcome.tree.is_some() {
        return Err("a terminated parse must not publish a tree".into());
    }
    Ok(())
}

/// Recovery remains nonterminal and still publishes the recovered tree.
///
/// Opposite-direction control: without it, a change that classified *every*
/// parser diagnostic as `RecursionLimit`, treated recovery as terminal, or
/// withheld trees merely because diagnostics exist would pass the tests above.
#[test]
fn a_recovered_parse_reports_no_terminal_failure() -> TestResult {
    let mut parser = Parser::new();

    let outcome = parser.parse_detailed("my $x = ;\n");

    if outcome.diagnostics.is_empty() {
        return Err("fixture must produce a recovery diagnostic".into());
    }
    if outcome.failure.is_some() {
        return Err(format!(
            "recovered source must not report a terminal failure: {:?}",
            outcome.failure
        )
        .into());
    }
    if outcome.tree.is_none() {
        return Err("recovered source must publish a tree".into());
    }
    if !outcome.is_recovered() {
        return Err("recovered source must be reported as recovered".into());
    }
    Ok(())
}
