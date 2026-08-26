//! Characterization tests for heredoc parsing in `perl-parser-pest`.
//!
//! Issue #3918: the crate had grammar rules for heredocs (`grammar.pest` ~870)
//! and a `Rule::heredoc` builder, but zero test coverage for them. These tests
//! lock in the parser's *current* observable behavior so future changes are
//! intentional rather than accidental.
//!
//! Known limitation being characterized: the pure-rust parser recognizes the
//! heredoc *opener* (`<<MARKER`, plus the `~` indented and `'` quoted flags) but
//! never collects the body — `content` is always empty (see the note at
//! `pure_rust_parser.rs` `Rule::heredoc`, "actual content is handled by the
//! scanner"). The body lines fall through as separate statements.
//!
//! s-expression shape (from `to_sexp`): `(heredoc MARKER FLAGS "CONTENT")`,
//! where FLAGS is `~` when indented, `'` when single-quoted, empty otherwise —
//! so a bare marker renders as `(heredoc EOF  "")` (two spaces around the empty
//! flags field).
//!
//! Deliberate separation of concerns so a future body-capture implementation is
//! not a landmine: the marker-form tests below assert only the marker + flags
//! *prefix* up to the opening content quote (`(heredoc EOF  "`), which stays
//! true regardless of what the content becomes. The single empty-content
//! limitation — that a non-empty body currently yields empty `content` — is
//! pinned by exactly one test,
//! `when_heredoc_has_body_then_content_is_empty_documents_limitation`. If body
//! capture lands, that one test is the only marker test that needs updating.
#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.

use perl_parser_pest::{AstNode, PureRustPerlParser};
use perl_tdd_support::must;

fn parse_to_sexp(source: &str) -> String {
    let mut parser = PureRustPerlParser::new();
    let ast = must(parser.parse(source));
    parser.to_sexp(&ast)
}

fn parse_ast(source: &str) -> AstNode {
    let mut parser = PureRustPerlParser::new();
    must(parser.parse(source))
}

// --- Marker forms ----------------------------------------------------------

#[test]
fn when_bare_heredoc_then_emits_heredoc_node_with_marker() {
    let sexp = parse_to_sexp("my $x = <<EOF;\nhello\nEOF\n");
    assert!(
        sexp.contains("(heredoc EOF  \""),
        "bare `<<EOF` should emit a heredoc node with the EOF marker and no flags; got: {sexp}"
    );
}

#[test]
fn when_indented_heredoc_then_sets_tilde_flag() {
    let sexp = parse_to_sexp("my $x = <<~EOF;\n  indented\n  EOF\n");
    assert!(
        sexp.contains("(heredoc EOF ~ \""),
        "`<<~EOF` should set the indented (`~`) flag; got: {sexp}"
    );
}

#[test]
fn when_single_quoted_heredoc_then_sets_quote_flag() {
    let sexp = parse_to_sexp("my $x = <<'EOF';\nno $interp\nEOF\n");
    assert!(
        sexp.contains("(heredoc EOF ' \""),
        "`<<'EOF'` should set the quoted (`'`) flag; got: {sexp}"
    );
}

#[test]
fn when_double_quoted_heredoc_then_marker_parsed_without_quote_flag() {
    // The builder only sets `quoted` for the single-quote form, so a
    // double-quoted marker parses the bare marker with no quote flag.
    let sexp = parse_to_sexp("my $x = <<\"EOF\";\ninterp $y\nEOF\n");
    assert!(
        sexp.contains("(heredoc EOF  \""),
        "`<<\"EOF\"` should parse the EOF marker with no quote flag; got: {sexp}"
    );
    assert!(
        !sexp.contains("(heredoc EOF '"),
        "double-quoted marker must not set the single-quote flag; got: {sexp}"
    );
}

#[test]
fn when_backtick_heredoc_then_marker_parsed() {
    let sexp = parse_to_sexp("my $x = <<`CMD`;\nls\nCMD\n");
    assert!(
        sexp.contains("(heredoc CMD  \""),
        "`<<`CMD`` should parse the CMD marker; got: {sexp}"
    );
}

#[test]
fn when_escaped_heredoc_then_marker_parsed() {
    let sexp = parse_to_sexp("my $x = <<\\EOF;\nx\nEOF\n");
    assert!(
        sexp.contains("(heredoc EOF  \""),
        "`<<\\EOF` should parse the EOF marker; got: {sexp}"
    );
}

#[test]
fn when_numeric_marker_then_marker_parsed() {
    // `bare_heredoc_delimiter` accepts ASCII alphanumerics, so digits are valid.
    let sexp = parse_to_sexp("my $x = <<123;\nx\n123\n");
    assert!(
        sexp.contains("(heredoc 123  \""),
        "`<<123` should parse the numeric marker; got: {sexp}"
    );
}

#[test]
fn when_heredoc_is_bare_statement_then_parses() {
    let sexp = parse_to_sexp("<<END;\n");
    assert!(
        sexp.contains("(heredoc END  \""),
        "a bare `<<END;` statement should parse as a heredoc primary; got: {sexp}"
    );
}

// --- Body / content limitation --------------------------------------------

#[test]
fn when_heredoc_has_body_then_content_is_empty_documents_limitation() {
    // Characterizes the known gap: the body (`hello world`) is NOT captured as
    // the heredoc's content — it always renders as the empty string. If body
    // capture is implemented, update this test in the same change.
    let sexp = parse_to_sexp("my $x = <<EOF;\nhello world\nEOF\n");
    assert!(
        sexp.contains("(heredoc EOF  \"\")"),
        "heredoc content is currently never captured (always empty); got: {sexp}"
    );
    assert!(
        !sexp.contains("(heredoc EOF  \"hello"),
        "heredoc body should not (yet) appear inside the heredoc node; got: {sexp}"
    );
}

#[test]
fn when_heredoc_body_is_empty_then_still_emits_node() {
    let sexp = parse_to_sexp("my $x = <<EOF;\nEOF\n");
    assert!(
        sexp.contains("(heredoc EOF  \""),
        "an empty-body heredoc should still emit a heredoc node; got: {sexp}"
    );
}

// --- Malformed / recovery (must not panic) ---------------------------------

#[test]
fn when_heredoc_missing_delimiter_then_recovers_without_panic() {
    // `<<` with no delimiter is not a valid heredoc; the parser recovers to a
    // Program rather than panicking, and emits no heredoc node.
    let sexp = parse_to_sexp("my $x = << ;\n");
    assert!(
        !sexp.contains("heredoc"),
        "malformed `<< ;` should not produce a heredoc node; got: {sexp}"
    );
}

#[test]
fn when_heredoc_single_quote_unterminated_then_recovers_without_panic() -> Result<(), String> {
    let ast = parse_ast("my $x = <<'EOF;\nx\n");
    // Recovery returns a Program; the important guarantee is "no panic".
    let AstNode::Program(nodes) = ast else {
        return Err("expected recovery to return a Program".to_string());
    };
    assert!(!nodes.is_empty(), "recovery should preserve at least the leading declaration");
    Ok(())
}

// --- q{} / qq{} __HEREDOC__ placeholder path (slice safety, #3917) ----------
// The issue also flags the slice operations at pure_rust_parser.rs:1303/1318
// that extract heredoc content from `q{__HEREDOC__...__HEREDOC__}` placeholder
// wrappers. These integration tests exercise that path end-to-end.

#[test]
fn when_q_string_has_heredoc_placeholder_then_extracts_inner_content() {
    let sexp = parse_to_sexp("my $x = q{__HEREDOC__body text__HEREDOC__};\n");
    assert!(
        sexp.contains("(string_literal body text)"),
        "q{{}} heredoc placeholder should extract the inner content; got: {sexp}"
    );
}

#[test]
fn when_qq_string_has_heredoc_placeholder_then_extracts_inner_content() {
    let sexp = parse_to_sexp("my $x = qq{__HEREDOC__body text__HEREDOC__};\n");
    assert!(
        sexp.contains("(string_literal body text)"),
        "qq{{}} heredoc placeholder should extract the inner content; got: {sexp}"
    );
}

#[test]
fn when_q_string_has_marker_without_content_then_does_not_panic() {
    // Regression guard for #3917 at the integration level: the placeholder open
    // and close overlap, so the slice guard must reject the inverted range and
    // fall back to the whole literal instead of panicking.
    let sexp = parse_to_sexp("my $x = q{__HEREDOC__};\n");
    assert!(
        sexp.contains("(string_literal q{__HEREDOC__})"),
        "a marker-only q{{}} placeholder should fall back to the whole literal; got: {sexp}"
    );
}
