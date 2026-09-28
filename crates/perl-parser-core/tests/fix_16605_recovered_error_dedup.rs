//! Issue #16605: a failed statement must produce ONE recorded error, not a
//! duplicate pair, and retained errors must render in source order.
//!
//! Before the fix, each statement-recovery loop recorded the original error
//! and then `recover_from_error` unconditionally recorded a synthetic
//! `ParseError::unexpected("statement", …)` for the same token, so `--check`
//! and the LSP shipped an `expected expression…` / `expected statement…`
//! pair anchored at one position where real `perl` reports a single error.
//! Recovery also recorded in discovery order, so a recovered statement could
//! anchor its error *after* higher-positioned errors and batched output
//! printed out of source order (fixture 18 printed its line-7 "Unclosed
//! block" error after the line-8 errors).
//!
//! The chosen dedup rule is site-scoped: the recovery loops reuse the
//! already-recorded error and no longer synthesize a second one. No global
//! dedup exists in `errors()`/`get_error_contexts`, so genuinely distinct
//! errors at one position (e.g. different `expected` sets) both survive —
//! pinned by `distinct_errors_at_one_position_both_survive_contexts` below.

use perl_parser_core::error::{ParseError, get_error_contexts};
use perl_parser_core::{NodeKind, Parser};

/// `13a_regex_unbalanced.pl` from the typo corpus: an unterminated `m/…`
/// regex fails the statement at line 5, column 15 (byte 56).
const REGEX_UNBALANCED: &str = concat!(
    "use strict;\n",
    "use warnings;\n",
    "\n",
    "my $s = \"abc\";\n",
    "my $m = $s =~ m/abc;\n",
    "print \"$m\n",
    "\";\n",
);

/// The same unterminated-regex failure inside a `sub` block, exercising the
/// `parse_block` recovery loop (the second duplicate site).
const BLOCK_REGEX_UNBALANCED: &str =
    "sub f {\n    my $m = $s =~ m/abc;\n}\n";

/// The same failure inside a `given` block, exercising the
/// `parse_given_block` recovery loop (the third duplicate site).
const GIVEN_BLOCK_REGEX_UNBALANCED: &str =
    "given ($x) {\n    my $m = $s =~ m/abc;\n}\n";

/// `18_multi_errors.pl` from the typo corpus: two missing semicolons, a
/// missing comma in a list, and an unclosed block.
const MULTI_ERRORS: &str = concat!(
    "use strict;\n",
    "use warnings;\n",
    "\n",
    "my $x = 1\n",
    "my $y = 2\n",
    "my @list = (1, 2 3);\n",
    "sub f {\n",
    "    my $s = \"unclosed\n",
    "    print $s;\n",
);

#[test]
fn recovered_statement_error_is_recorded_once() {
    let mut parser = Parser::new(REGEX_UNBALANCED);
    let ast = parser.parse().expect("recovery should still return an AST");
    let errors = parser.errors();

    let blocking: Vec<&ParseError> = errors.iter().filter(|e| e.blocks_clean_parse()).collect();
    assert_eq!(
        blocking.len(),
        1,
        "the failed statement must record one error, got: {errors:?}"
    );

    // The surviving record is the ORIGINAL expression error, not the
    // synthetic `expected statement` recovery error.
    let rendered = blocking[0].to_string();
    assert!(
        rendered.starts_with("expected expression"),
        "original error must survive, got: {rendered}"
    );
    assert!(
        !errors.iter().any(|e| e.to_string().starts_with("expected statement")),
        "the synthetic statement-recovery duplicate must be gone: {errors:?}"
    );
    assert_eq!(blocking[0].location(), Some(56), "anchor stays on `m/abc`");

    // Recovery behavior itself is unchanged: the parse continues past the
    // failed statement instead of aborting the program.
    match &ast.kind {
        NodeKind::Program { statements } => {
            assert!(
                statements.len() >= 3,
                "parse must still recover past the failed statement, got {} statements",
                statements.len()
            );
        }
        other => panic!("expected Program node, got {other:?}"),
    }
}

#[test]
fn recovered_block_statement_error_is_recorded_once() {
    let mut parser = Parser::new(BLOCK_REGEX_UNBALANCED);
    let _ast = parser.parse().expect("recovery should still return an AST");
    let errors = parser.errors();

    // The unterminated regex swallows the block's closing `}`, so an
    // "Unclosed block" error is legitimate here. What must NOT happen is the
    // synthetic `expected statement` pair for the failed statement itself.
    assert!(
        !errors.iter().any(|e| e.to_string().starts_with("expected statement")),
        "block-level recovery must not synthesize a statement duplicate: {errors:?}"
    );
    assert!(
        errors.iter().any(|e| e.to_string().starts_with("expected expression")),
        "the original expression error must survive: {errors:?}"
    );
}

#[test]
fn recovered_given_block_statement_error_is_recorded_once() {
    let mut parser = Parser::new(GIVEN_BLOCK_REGEX_UNBALANCED);
    let _ast = parser.parse().expect("recovery should still return an AST");
    let errors = parser.errors();

    assert!(
        !errors.iter().any(|e| e.to_string().starts_with("expected statement")),
        "given-block recovery must not synthesize a statement duplicate: {errors:?}"
    );
    assert!(
        errors.iter().any(|e| e.to_string().starts_with("expected expression")),
        "the original expression error must survive: {errors:?}"
    );
}

#[test]
fn batched_errors_render_in_source_order() {
    let mut parser = Parser::new(MULTI_ERRORS);
    let _ast = parser.parse().expect("recovery should still return an AST");

    // Anchored (located) errors are non-decreasing by anchor; unanchored
    // diagnostics (`Unclosed block` carries an anchor, but budget/limit
    // stops do not) sort last. Discovery order is preserved within a tie.
    let mut last_located: Option<usize> = None;
    let mut saw_unanchored = false;
    for error in parser.errors() {
        match error.location() {
            Some(loc) => {
                assert!(!saw_unanchored, "located error after an unanchored one");
                if let Some(prev) = last_located {
                    assert!(loc >= prev, "errors out of source order: {prev} then {loc}");
                }
                last_located = Some(loc);
            }
            None => saw_unanchored = true,
        }
    }

    // The discriminating case: the line-7 "Unclosed block" error (anchor 74)
    // must now render BEFORE the line-8 errors (anchor 88) that discovery
    // order previously put first.
    let anchors: Vec<usize> = parser.errors().iter().filter_map(|e| e.location()).collect();
    let unclosed_block = anchors.iter().position(|&a| a == 74);
    let unclosed_string = anchors.iter().position(|&a| a == 88);
    assert!(unclosed_block.is_some(), "expected the block error at anchor 74: {anchors:?}");
    assert!(unclosed_string.is_some(), "expected the string error at anchor 88: {anchors:?}");
    assert!(
        unclosed_block.unwrap() < unclosed_string.unwrap(),
        "line-7 error must precede line-8 errors, anchors: {anchors:?}"
    );
}

#[test]
fn distinct_errors_at_one_position_both_survive_contexts() {
    // Pins the chosen rule: no dedup at the storage/contexts layer. Two
    // genuinely distinct errors (different `expected` sets) anchored at one
    // position must both reach `get_error_contexts` output.
    let source = "my @nums = (1 2 3);\n";
    let first = ParseError::unexpected("',' or ')'", "number", 18);
    let second = ParseError::unexpected("statement", "number", 18);

    let contexts = get_error_contexts(&[first, second], source);
    assert_eq!(contexts.len(), 2, "distinct errors at one position must both survive");

    assert_eq!(contexts[0].line, contexts[1].line, "both anchor the same line");
    assert_eq!(contexts[0].column, contexts[1].column, "both anchor the same column");
    assert_ne!(
        contexts[0].error.to_string(),
        contexts[1].error.to_string(),
        "the two errors must remain distinct"
    );
}
