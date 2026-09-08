//! Integration tests for the tree-sitter-compat adapter.

use perl_parser_core::{ParseError, Parser};
use perl_test_must::{must, must_err_with};
use perl_tree_sitter_compat::{TreeError, highlights, parse_to_tree, to_sexp};

#[test]
fn sexp_round_trips_a_realistic_snippet() {
    let tree = must(parse_to_tree("package App;\nuse strict;\nsub run { return 42; }\n1;\n"));
    let sexp = to_sexp(&tree);
    assert!(sexp.starts_with("(program"), "root program: {sexp}");
    assert!(sexp.contains("(package"), "package node present");
    assert!(sexp.contains("(use"), "use node present");
    assert!(sexp.contains("(subroutine"), "subroutine node present");
}

#[test]
fn nodes_carry_byte_and_point_ranges() {
    let tree = must(parse_to_tree("package App;\n"));
    assert_eq!(tree.start_byte, 0);
    assert_eq!(tree.start_point.row, 0);
    assert_eq!(tree.start_point.column, 0);
    // Every node's end is >= its start.
    assert!(all_ranges_valid(&tree));
}

#[test]
fn highlights_cover_keywords_variables_and_literals() {
    let tree = must(parse_to_tree("use strict;\nmy $count = 3;\nmy $name = \"x\";\n"));
    let hl = highlights(&tree);
    assert!(hl.iter().any(|h| h.capture == "keyword"));
    assert!(hl.iter().any(|h| h.capture == "variable"));
    assert!(hl.iter().any(|h| h.capture == "number"));
    assert!(hl.iter().any(|h| h.capture == "string"));
}

#[test]
fn serializes_to_json() {
    let tree = must(parse_to_tree("1;\n"));
    let json = must(serde_json::to_string(&tree));
    let back: perl_tree_sitter_compat::TsNode = must(serde_json::from_str(&json));
    assert_eq!(tree, back);
}

#[test]
fn deterministic_across_parses() {
    let src = "package App;\nsub a { 1 }\nsub b { 2 }\n";
    assert_eq!(to_sexp(&must(parse_to_tree(src))), to_sexp(&must(parse_to_tree(src))));
}

/// Two catastrophic sources that fail at different sites must not collapse to
/// one indistinguishable `ParseFailed`. The native parser is the oracle for
/// offset/kind; a mapping that still discards the diagnostic fails here.
#[test]
fn parse_failures_at_different_offsets_differ_by_native_diagnostic() {
    let early = "not ".repeat(200) + "1";
    let late = format!("my $ok = 1;\n{}", "{".repeat(600));

    let early_err =
        must_err_with(parse_to_tree(&early), "deep `not` chain must fail to produce a tree");
    let late_err =
        must_err_with(parse_to_tree(&late), "prefixed brace nest must fail to produce a tree");

    let (early_offset, early_kind) = parse_failed_parts(early_err);
    let (late_offset, late_kind) = parse_failed_parts(late_err);
    assert_ne!(
        (early_offset, early_kind.as_str()),
        (late_offset, late_kind.as_str()),
        "failures at different source sites must carry distinct native offset/kind"
    );

    let early_native =
        must_err_with(Parser::new(&early).parse(), "native parse must fail the `not` chain");
    let late_native =
        must_err_with(Parser::new(&late).parse(), "native parse must fail the brace nest");
    assert_eq!(early_offset, early_native.location());
    assert_eq!(late_offset, late_native.location());
    assert_eq!(early_kind, native_error_kind(&early_native));
    assert_eq!(late_kind, native_error_kind(&late_native));
    assert_ne!(early_kind, "unknown", "early fixture must be a known native kind");
    assert_ne!(late_kind, "unknown", "late fixture must be a known native kind");
}

#[test]
fn recoverable_syntax_still_produces_a_tree() {
    assert!(parse_to_tree("if (").is_ok(), "recovered syntax must not become ParseFailed");
}

fn parse_failed_parts(error: TreeError) -> (Option<usize>, String) {
    match error {
        TreeError::ParseFailed { offset, kind } => (offset, kind),
    }
}

/// Independent of the adapter's mapping helper: names only the live `Err`-path
/// variants these fixtures must produce so a dummy constant kind cannot pass.
fn native_error_kind(error: &ParseError) -> &'static str {
    match error {
        ParseError::RecursionDepthExhausted { .. } => "recursion_depth_exhausted",
        ParseError::NestingTooDeep { .. } => "nesting_too_deep",
        _ => "unknown",
    }
}

fn all_ranges_valid(node: &perl_tree_sitter_compat::TsNode) -> bool {
    node.end_byte >= node.start_byte && node.children.iter().all(all_ranges_valid)
}
