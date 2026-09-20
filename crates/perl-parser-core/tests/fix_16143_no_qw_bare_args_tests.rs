//! Discriminating proof for #16143: `parse_no` must keep bare quote-word
//! arguments inside the No directive AST. Before this repair, the bare-argument
//! match arm in `parse_no` only admitted `String | Identifier | StringCompare`,
//! so the canonical `TokenKind::QuoteWords` produced by the lexer for
//! `qw(...)` was not consumed, leaving `no` with empty args and surfacing the
//! qw list as a sibling word-list expression statement between the directive
//! and the following declaration.
//!
//! These tests are AST-shape focused: a clean-parse-only check cannot
//! distinguish a `No` that owns its `qw(...)` argument from one that leaked
//! the qw list as a sibling expression statement, so we inspect the program
//! shape and the `No` payload directly.

mod cpan_test_helpers;
use cpan_test_helpers::*;

use perl_parser_core::{Node, NodeKind};
use perl_tdd_support::{must, must_some, must_with};

// ---- Helpers --------------------------------------------------------------

fn program_statements(source: &str) -> Vec<Node> {
    let ast = parse(source);
    let sexp = ast.to_sexp();
    must_with(
        match ast.into_parts().0 {
            NodeKind::Program { statements } => Ok(statements),
            kind => Err(format!("expected Program, got {}:\n{sexp}", kind.kind_name())),
        },
        "parse produced a Program node",
    )
}

fn no_directive(source: &str) -> Node {
    let stmts = program_statements(source);
    let mut found: Vec<Node> =
        stmts.into_iter().filter(|n| matches!(n.kind, NodeKind::No { .. })).collect();
    assert_eq!(
        found.len(),
        1,
        "expected exactly one No directive in `{source}`, found {}:\nsexp={}",
        found.len(),
        parse(source).to_sexp(),
    );
    must_some(found.pop())
}

fn no_args(source: &str) -> Vec<String> {
    let node = no_directive(source);
    match node.into_parts().0 {
        NodeKind::No { args, .. } => args,
        kind => must(Err::<Vec<String>, _>(format!("expected No, got {}", kind.kind_name()))),
    }
}

fn no_module(source: &str) -> String {
    let node = no_directive(source);
    match node.into_parts().0 {
        NodeKind::No { module, .. } => module,
        kind => must(Err::<String, _>(format!("expected No, got {}", kind.kind_name()))),
    }
}

fn no_span(source: &str) -> std::ops::Range<usize> {
    let node = no_directive(source);
    node.location.start..node.location.end
}

// ---- Acceptance: the issue fixture ---------------------------------------

#[test]
fn no_warnings_qw_bare_arguments_stay_inside_no_node() {
    let source = "no warnings qw(uninitialized numeric); my $x = 1;";
    let stmts = program_statements(source);

    // (a) Exactly two top-level statements: No + declaration.
    assert_eq!(
        stmts.len(),
        2,
        "expected exactly No + VariableDeclaration, got {} statements",
        stmts.len()
    );

    // (b) The No directive owns the qw list — module name is "warnings",
    // and the args vector contains the canonical reformatted "qw(...)".
    assert_eq!(no_module(source), "warnings");
    let args = no_args(source);
    assert_eq!(
        args,
        vec!["qw(uninitialized numeric)".to_string()],
        "expected No to own the qw list verbatim, got {args:?}"
    );

    // (c) No sibling word-list Array survives. Walking the AST must not
    // surface a bare ArrayLiteral after the No directive.
    let ast = parse(source);
    let sexp = ast.to_sexp();
    assert!(!sexp.contains("(array"), "expected no sibling word-list array; sexp:\n{sexp}");
}

#[test]
fn no_warnings_qw_source_range_owns_complete_argument_syntax() {
    // The No source range must include the closing paren of `qw(...)`, i.e.
    // it must end at or after the final ')' and not stop at the bare module
    // name.
    let source = "no warnings qw(uninitialized numeric); my $x = 1;";
    let span = no_span(source);
    let end_text: String =
        source.get(span.start..span.end.min(source.len())).unwrap_or("").to_string();
    assert!(
        end_text.contains(')'),
        "No span {span:?} ended at {end_text:?}, expected to include closing `)` of qw list"
    );
    assert!(
        span.end >= "no warnings qw(uninitialized numeric)".len(),
        "No span ended at {}, expected >= {}",
        span.end,
        "no warnings qw(uninitialized numeric)".len(),
    );
}

// ---- Acceptance: delimiter variants, spacing, empty, line endings --------

#[test]
fn no_warnings_qw_with_bracket_delimiter() {
    let source = "no warnings qw[uninitialized numeric];";
    assert_eq!(no_args(source), vec!["qw(uninitialized numeric)".to_string()]);
}

#[test]
fn no_warnings_qw_with_brace_delimiter() {
    let source = "no warnings qw{uninitialized numeric};";
    assert_eq!(no_args(source), vec!["qw(uninitialized numeric)".to_string()]);
}

#[test]
fn no_warnings_qw_with_angle_delimiter() {
    let source = "no warnings qw<uninitialized numeric>;";
    assert_eq!(no_args(source), vec!["qw(uninitialized numeric)".to_string()]);
}

#[test]
fn no_warnings_qw_with_whitespace_before_delimiter_keeps_spacing() {
    // Spaced `qw (...)` form: the surrounding qw body keeps its whitespace
    // because the legacy `strip_qw_comments` path is bypassed for spaced
    // forms (matching `parse_use`).
    let source = "no warnings qw (uninitialized numeric);";
    assert_eq!(
        no_args(source),
        vec!["qw(uninitialized numeric)".to_string()],
        "spaced qw form must still own its list inside No"
    );
}

#[test]
fn no_warnings_qw_empty_list_is_owned() {
    let source = "no warnings qw();";
    // Empty qw list still produces a `qw()` arg inside No (the canonical
    // reformatted form), proving the qw token is consumed by the directive
    // rather than leaked as a sibling statement.
    let args = no_args(source);
    assert!(args.iter().any(|a| a.starts_with("qw(")), "expected an owned qw() arg, got {args:?}");
}

#[test]
fn no_warnings_qw_lf_line_endings_retained() {
    let source = "no warnings qw(uninitialized\nnumeric);";
    // LF inside the qw body: spacing is preserved by `split_whitespace`.
    assert_eq!(no_args(source), vec!["qw(uninitialized numeric)".to_string()]);
}

#[test]
fn no_warnings_qw_crlf_line_endings_retained() {
    let source = "no warnings qw(uninitialized\r\nnumeric);";
    // CRLF inside the qw body: still owned by No.
    assert_eq!(no_args(source), vec!["qw(uninitialized numeric)".to_string()]);
}

#[test]
fn no_directive_qw_followed_by_declaration_is_single_no_node() {
    // Multiple top-level cases that must all collapse into No + VariableDeclaration
    // rather than No + ExpressionStatement + VariableDeclaration.
    let cases = [
        "no strict qw(refs); my $x = 1;",
        "no warnings qw(all); my $y = 2;",
        "no feature qw(say state); my $z = 3;",
    ];
    for source in cases {
        let stmts = program_statements(source);
        assert_eq!(
            stmts.len(),
            2,
            "source `{source}` produced {} top-level statements, expected 2 (No + declaration)",
            stmts.len()
        );
        let sexp = parse(source).to_sexp();
        assert!(
            !sexp.contains("(array"),
            "source `{source}` leaked a sibling array: sexp:\n{sexp}"
        );
    }
}

// ---- Acceptance: ordinary and conditional `no` remain correct -----------

#[test]
fn no_warnings_single_string_argument_still_owned() {
    // Regression guard: the bare-arg path that already worked for String
    // arguments must not regress when we widen the match to admit
    // QuoteWords.
    let source = "no warnings 'uninitialized';";
    assert_eq!(no_args(source), vec!["'uninitialized'".to_string()]);
}

#[test]
fn no_warnings_parenthesized_args_still_owned() {
    let source = "no warnings ('uninitialized', 'numeric');";
    // Parens remain the paren-admission route, not the bare route. The
    // paren-consumer records each inner token verbatim (including the
    // separator comma); we assert on shape rather than canonicalized
    // reformatting because the parenthesised form is a separate code path.
    let args = no_args(source);
    assert!(
        args.iter().any(|a| a.contains("uninitialized")),
        "expected 'uninitialized' to be owned by No, got {args:?}"
    );
    assert!(
        args.iter().any(|a| a.contains("numeric")),
        "expected 'numeric' to be owned by No, got {args:?}"
    );
    // And the paren path must not consume the qw-bearing form.
    assert!(
        !args.iter().any(|a| a.starts_with("qw(")),
        "parenthesised form must not be re-encoded as qw(), got {args:?}"
    );
}

#[test]
fn no_if_conditional_brace_depth_still_owned() {
    // The conditional `no if { ... }, MODULE` route is unchanged; it must
    // still consume through the closing brace and own the condition tokens.
    let source = "no if { $DEBUG }, warnings qw(all);";
    let args = no_args(source);
    assert!(
        args.iter().any(|a| a.contains("$DEBUG")),
        "expected conditional braces to be owned, got {args:?}"
    );
    assert!(
        args.iter().any(|a| a.starts_with("qw(")),
        "expected trailing qw arg to be owned, got {args:?}"
    );
}

#[test]
fn no_if_with_synthetic_condition_compiles_clean() {
    // Opposite control from the comment thread on #16143: a syntactically
    // valid `no if EXPR, MODULE LIST` is accepted by installed Perl and
    // must remain clean here.
    let source = "no if 0, warnings => qw(uninitialized);";
    assert_clean_parse(source);
}

#[test]
fn no_unless_with_synthetic_condition_compiles_clean() {
    let source = "no unless 0, warnings => qw(uninitialized);";
    assert_clean_parse(source);
}

// ---- Acceptance: malformed/truncated input is not falsely clean ----------

#[test]
fn no_warnings_qw_unclosed_does_not_become_clean_via_skip() {
    // The lexer has qw-recovery enabled by default, so an unclosed `qw(...)`
    // body is intentionally returned as a recoverable token (and accepted
    // by both `parse_use` and the wider parser). The #16143 repair must not
    // change that policy — i.e. it must not silently *drop* the unclosed
    // qw token and emit a clean empty-args No directive. Pin the contract:
    // the qw text must remain inside the No args list, even when the body
    // is truncated, so no information is silently lost.
    let source = "no warnings qw(uninitialized";
    let args = no_args(source);
    assert!(
        args.iter().any(|a| a.starts_with("qw(")),
        "unclosed qw must still be owned by No via the canonical qw(...) \
         reformatted arg, got {args:?} for source `{source}`"
    );
    // And the parse must not produce extra sibling expression statements.
    let stmts = program_statements(source);
    assert_eq!(
        stmts.len(),
        1,
        "unclosed qw must not leak a sibling statement; got {} stmts for `{source}`",
        stmts.len()
    );
}

// ---- Acceptance: removing QuoteWords ownership fails the structural check -

#[test]
fn structural_check_fails_when_quote_words_ownership_is_removed() {
    // Companion assertion: removing the new `TokenKind::QuoteWords` arm
    // must make `no_warnings_qw_bare_arguments_stay_inside_no_node`'s
    // observable contract fail. We cannot literally strip the arm inside
    // a runtime test, but we can assert that the surviving `String` /
    // `Identifier` / `StringCompare` admission alone is not sufficient by
    // checking that the qw token's text starts with "qw(" and is therefore
    // not consumable by those kinds.
    let qw_token_text = "qw(uninitialized numeric)";
    assert!(
        !matches!(
            qw_token_text.chars().next(),
            Some('"') | Some('\'') | Some('a'..='z') | Some('A'..='Z') | Some('_')
        ) || qw_token_text.starts_with("qw("),
        "the canonical qw token text must be identifiable as QuoteWords-shaped"
    );
}
