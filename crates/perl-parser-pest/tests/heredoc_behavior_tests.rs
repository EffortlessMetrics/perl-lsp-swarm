//! Behavior-pinning tests for heredoc parsing in `perl-parser-pest` (#3918).
//!
//! The suite previously had no coverage for `Rule::heredoc` or heredoc
//! placeholder handling. These tests pin the parser's current, observable
//! behavior for the common heredoc forms and their edge cases, and guard the
//! q{}/qq{} placeholder path plus error recovery against regressions (cf. the
//! bounds-guarded slice handling flagged in #3917).
//!
//! Scope note: this legacy Pest parser recognizes the heredoc *operator* and
//! preserves its marker/indent/quote flags, but does not slurp the body into
//! the node (the content field is empty). The assertions below deliberately
//! pin only what the parser actually produces today, not idealized semantics.
#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.

use perl_parser_pest::PureRustPerlParser;
use perl_tdd_support::{must, must_err};

fn parse_to_sexp(source: &str) -> String {
    let mut parser = PureRustPerlParser::new();
    let ast = must(parser.parse(source));
    parser.to_sexp(&ast)
}

#[test]
fn when_given_basic_heredoc_then_parser_emits_heredoc_node() {
    let sexp = parse_to_sexp("my $x = <<EOF;\nhello\nEOF\n");
    assert!(
        sexp.contains("(heredoc EOF"),
        "expected a heredoc node with the EOF marker; got: {sexp}"
    );
}

#[test]
fn when_given_indented_heredoc_then_parser_preserves_tilde_marker() {
    // `<<~EOF` is the indented heredoc form; the parser records the `~` flag.
    let sexp = parse_to_sexp("my $x = <<~EOF;\n    hello\n    EOF\n");
    assert!(
        sexp.contains("(heredoc EOF ~"),
        "expected the indented `~` flag to be preserved; got: {sexp}"
    );
}

#[test]
fn when_given_single_quoted_heredoc_then_parser_preserves_quote_marker() {
    // `<<'EOF'` is the non-interpolating form; the parser records the `'` flag.
    let sexp = parse_to_sexp("my $x = <<'EOF';\nno $interp\nEOF\n");
    assert!(
        sexp.contains("(heredoc EOF '"),
        "expected the single-quote flag to be preserved; got: {sexp}"
    );
}

#[test]
fn when_given_empty_heredoc_then_parser_succeeds_with_heredoc_node() {
    let sexp = parse_to_sexp("my $x = <<EOF;\nEOF\n");
    assert!(
        sexp.contains("(heredoc EOF"),
        "empty heredoc should still yield a heredoc node; got: {sexp}"
    );
}

#[test]
fn when_heredoc_marker_appears_inside_qq_string_then_it_is_string_content_not_a_heredoc() {
    // A `<<EOF` token inside `qq{...}` is ordinary string content, not a heredoc.
    let sexp = parse_to_sexp("my $x = qq{<<EOF};\n");
    assert!(
        !sexp.contains("(heredoc"),
        "a marker inside qq{{}} must not be parsed as a heredoc; got: {sexp}"
    );
    assert!(
        sexp.contains("string_literal") && sexp.contains("qq{<<EOF"),
        "expected the qq string to be preserved as a string literal; got: {sexp}"
    );
}

#[test]
fn when_heredoc_marker_appears_inside_q_string_then_it_is_string_content_not_a_heredoc() {
    let sexp = parse_to_sexp("my $x = q{text <<EOF more};\n");
    assert!(
        !sexp.contains("(heredoc"),
        "a marker inside q{{}} must not be parsed as a heredoc; got: {sexp}"
    );
    assert!(
        sexp.contains("string_literal"),
        "expected the q string to be preserved as a string literal; got: {sexp}"
    );
}

#[test]
fn when_given_lone_heredoc_operator_then_parser_returns_error_without_panicking() {
    // `<<` with no marker is malformed; the parser must report an error
    // (Result::Err) rather than panic.
    let mut parser = PureRustPerlParser::new();
    let _err = must_err(parser.parse("<<"));
}

#[test]
fn when_given_heredoc_operator_with_empty_marker_then_parser_recovers_without_panicking() {
    // `<<;` (operator immediately terminated) must parse without panicking and
    // without producing a heredoc node — exercising the recovery path.
    let sexp = parse_to_sexp("my $x = <<;\n");
    assert!(
        !sexp.contains("(heredoc"),
        "malformed `<<;` should not yield a heredoc node; got: {sexp}"
    );
}
