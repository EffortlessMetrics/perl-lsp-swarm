//! Guard for the recursion-vs-nesting classification boundary (#12952).
//!
//! `behavior_spec_tests` accepts either recursion variant, so it cannot catch a
//! repair that classifies expression-recursion exhaustion as *structural*
//! nesting. `perl-parser-core` forbids exactly that relabeling, so the boundary
//! is pinned here instead.

use tree_sitter_perl_rs::{ParseFailure, Parser};

/// Expression recursion exhaustion classifies as `RecursionLimit`.
///
/// The parser's expression guard emits `RecursionDepthExhausted`. Before the
/// #12952 repair the facade had no arm for it, so a genuine budget exhaustion
/// surfaced as `Other` — an "unclassified catastrophic failure" — which is the
/// one thing this facade is not allowed to be vague about.
#[test]
fn expression_recursion_exhaustion_is_classified_not_other() {
    let mut parser = Parser::new();

    let outcome = parser.parse_detailed(&"(".repeat(600));

    match outcome.failure {
        Some(ParseFailure::RecursionLimit) => {}
        Some(ParseFailure::NestingTooDeep { .. }) => panic!(
            "expression-recursion exhaustion was relabeled as structural nesting; \
             perl-parser-core's ParseError::RecursionDepthExhausted docs forbid this"
        ),
        Some(ParseFailure::Other { diagnostic }) => {
            panic!("recursion exhaustion left unclassified as Other: {diagnostic:?}")
        }
        other => panic!("expected a typed recursion failure, got {other:?}"),
    }

    assert!(outcome.tree.is_none(), "a terminated parse must not publish a tree");
}

/// A recovered parse still reports no catastrophic failure.
///
/// Opposite-direction control: without it, a change that classified *every*
/// diagnostic as `RecursionLimit` would pass the test above.
#[test]
fn an_ordinary_parse_reports_no_failure() {
    let mut parser = Parser::new();

    let outcome = parser.parse_detailed("my $x = 42;\n");

    assert!(outcome.failure.is_none(), "clean source must not report a failure");
    assert!(outcome.tree.is_some(), "clean source must publish a tree");
}
