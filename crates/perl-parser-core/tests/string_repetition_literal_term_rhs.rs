//! Ordinary binary `x` remaining literal/keyword term classes (#13929).
//!
//! Discriminates already-tokenized, `parse_primary`-supported RHS identities that
//! the post-#13102 operand-start whitelist still rejected. The classifier under
//! test is private to ordinary binary repetition; these fixtures do not grant a
//! global expression-starter predicate.

mod cpan_test_helpers;
use cpan_test_helpers::*;
use perl_parser_core::{Node, NodeKind, Parser, TokenKind, TokenStream};
use perl_tdd_support::must;

fn collect_string_repetitions<'a>(node: &'a Node, repetitions: &mut Vec<&'a Node>) {
    if matches!(&node.kind, NodeKind::Binary { op, .. } if op == "x") {
        repetitions.push(node);
    }
    for child in node.children() {
        collect_string_repetitions(child, repetitions);
    }
}

fn parse_with_errors(source: &str) -> (Node, Vec<String>) {
    let mut parser = Parser::new(source);
    let ast = must(parser.parse());
    let errors = parser.get_errors().iter().map(|error| format!("{error:?}")).collect();
    (ast, errors)
}

fn repetitions_of(ast: &Node) -> Vec<&Node> {
    let mut repetitions = Vec::new();
    collect_string_repetitions(ast, &mut repetitions);
    repetitions
}

fn token_kinds(source: &str) -> Vec<(TokenKind, String)> {
    let mut stream = TokenStream::new(source);
    let mut kinds = Vec::new();
    while let Ok(token) = stream.next() {
        if token.kind() == TokenKind::Eof {
            break;
        }
        kinds.push((token.kind(), token.text.to_string()));
    }
    kinds
}

fn rhs_kind_after_x(source: &str) -> Option<TokenKind> {
    let kinds = token_kinds(source);
    kinds.windows(2).find_map(|pair| {
        let [(left_kind, left_text), (right_kind, _)] = pair else {
            return None;
        };
        (*left_kind == TokenKind::Identifier && left_text == "x").then_some(*right_kind)
    })
}

fn assert_string_repetition(
    source: &str,
    term: &str,
    expected_rhs: impl Fn(&NodeKind) -> bool,
    expected_rhs_desc: &str,
) {
    let (ast, errors) = parse_with_errors(source);
    assert!(
        errors.is_empty(),
        "expected no diagnostics for source:\n{source}\n\nsexp:\n{}\n\ndiagnostics:\n{errors:#?}",
        ast.to_sexp(),
    );
    assert_clean_parse(source);

    let repetitions = repetitions_of(&ast);
    assert_eq!(
        repetitions.len(),
        1,
        "expected exactly one binary string-repetition node for source:\n{source}\n\nsexp:\n{}",
        ast.to_sexp(),
    );

    let Some(repetition) = repetitions.first() else {
        return;
    };
    assert!(
        matches!(&repetition.kind, NodeKind::Binary { .. }),
        "repetition node was not NodeKind::Binary for source:\n{source}\n\nsexp:\n{}",
        ast.to_sexp(),
    );
    let NodeKind::Binary { left, right, .. } = &repetition.kind else {
        return;
    };
    assert!(
        matches!(&left.kind, NodeKind::String { .. }),
        "the repetition LHS must be a string literal for source:\n{source}\n\nsexp:\n{}",
        ast.to_sexp(),
    );
    assert!(
        expected_rhs(&right.kind),
        "the repetition RHS must be {expected_rhs_desc} for source:\n{source}\n\nsexp:\n{}",
        ast.to_sexp(),
    );
    assert!(
        !matches!(&right.kind, NodeKind::Binary { op, .. } if op == "x"),
        "the RHS must not be parsed as another repetition for source:\n{source}\n\nsexp:\n{}",
        ast.to_sexp(),
    );

    assert!(source.contains(term), "term {term:?} missing from source:\n{source}");
    let Some(term_start) = source.find(term) else {
        return;
    };
    let term_end = term_start + term.len();
    assert!(
        right.location.start <= term_start && right.location.end >= term_end,
        "RHS span {}..{} must cover term {term:?} at {term_start}..{term_end} for source:\n{source}\n\nsexp:\n{}",
        right.location.start,
        right.location.end,
        ast.to_sexp(),
    );
}

fn assert_not_repetition(source: &str) {
    let (ast, _errors) = parse_with_errors(source);
    let repetitions = repetitions_of(&ast);
    assert!(
        repetitions.is_empty(),
        "source must remain non-repetition:\n{source}\n\nsexp:\n{}",
        ast.to_sexp(),
    );
}

#[test]
fn remaining_term_classes_are_already_tokenized_as_parse_primary_identities() {
    let cases = [
        (r#"my $value = "x" x v1.2.3;"#, TokenKind::VString),
        (r#"my $value = "x" x qw(3 4);"#, TokenKind::QuoteWords),
        (r#"my $value = "x" x qx{printf 3};"#, TokenKind::QuoteCommand),
        // `/3/` is Slash at this lookahead; unary term reclassification turns it
        // into Regex after `x` is accepted as the operator.
        (r#"my $value = "x" x /3/;"#, TokenKind::Slash),
        (r#"my $value = "x" x m{3};"#, TokenKind::Regex),
        (r#"my $value = "x" x qr{3};"#, TokenKind::Regex),
        (r#"my $value = "x" x s/a/b/;"#, TokenKind::Substitution),
        (r#"my $value = "x" x tr/a/b/;"#, TokenKind::Transliteration),
        (r#"my $value = "x" x eval { 3 };"#, TokenKind::Eval),
        (r#"my $value = "x" x eval "3";"#, TokenKind::Eval),
        ("my $value = \"x\" x <<'END';\n3\nEND\n", TokenKind::HeredocStart),
    ];

    for (source, expected) in cases {
        let got = rhs_kind_after_x(source);
        assert_eq!(
            got,
            Some(expected),
            "lexer/parser token identity after binary `x` for {source:?}\ntokens: {:?}",
            token_kinds(source),
        );
    }
}

#[test]
fn repetition_accepts_vstring_rhs() {
    assert_string_repetition(
        r#"my $value = "x" x v1.2.3;"#,
        "v1.2.3",
        |kind| matches!(kind, NodeKind::VString { .. }),
        "a v-string",
    );
}

#[test]
fn repetition_accepts_quote_words_rhs() {
    assert_string_repetition(
        r#"my $value = "x" x qw(3 4);"#,
        "qw(3 4)",
        |kind| match kind {
            NodeKind::ArrayLiteral { elements } => {
                elements.len() == 2
                    && matches!(&elements[0].kind, NodeKind::String { value, .. } if value == "3")
                    && matches!(&elements[1].kind, NodeKind::String { value, .. } if value == "4")
            }
            _ => false,
        },
        "a quote-words list",
    );
}

#[test]
fn repetition_accepts_quote_command_rhs() {
    assert_string_repetition(
        r#"my $value = "x" x qx{printf 3};"#,
        "qx{printf 3}",
        |kind| matches!(kind, NodeKind::String { interpolated: true, .. }),
        "a quote-command string",
    );
}

#[test]
fn repetition_accepts_match_regex_rhs() {
    assert_string_repetition(
        r#"my $value = "x" x /3/;"#,
        "/3/",
        |kind| matches!(kind, NodeKind::Regex { .. }),
        "a match regex term",
    );
}

#[test]
fn repetition_accepts_m_regex_rhs() {
    assert_string_repetition(
        r#"my $value = "x" x m{3};"#,
        "m{3}",
        |kind| matches!(kind, NodeKind::Regex { .. }),
        "an m{} regex term",
    );
}

#[test]
fn repetition_accepts_qr_regex_rhs() {
    assert_string_repetition(
        r#"my $value = "x" x qr{3};"#,
        "qr{3}",
        |kind| matches!(kind, NodeKind::Regex { .. }),
        "a qr{} regex term",
    );
}

#[test]
fn repetition_accepts_substitution_rhs() {
    assert_string_repetition(
        r#"my $value = "x" x s/a/b/;"#,
        "s/a/b/",
        |kind| matches!(kind, NodeKind::Substitution { .. }),
        "a substitution term",
    );
}

#[test]
fn repetition_accepts_transliteration_rhs() {
    assert_string_repetition(
        r#"my $value = "x" x tr/a/b/;"#,
        "tr/a/b/",
        |kind| matches!(kind, NodeKind::Transliteration { .. }),
        "a transliteration term",
    );
}

#[test]
fn repetition_accepts_eval_block_rhs() {
    assert_string_repetition(
        r#"my $value = "x" x eval { 3 };"#,
        "eval { 3 }",
        |kind| matches!(kind, NodeKind::Eval { .. }),
        "an eval block",
    );
}

#[test]
fn repetition_accepts_eval_string_rhs() {
    assert_string_repetition(
        r#"my $value = "x" x eval "3";"#,
        r#"eval "3""#,
        |kind| matches!(kind, NodeKind::Eval { .. }),
        "an eval string",
    );
}

#[test]
fn repetition_accepts_heredoc_rhs() {
    assert_string_repetition(
        "my $value = \"x\" x <<'END';\n3\nEND\n",
        "<<'END'",
        |kind| matches!(kind, NodeKind::Heredoc { delimiter, .. } if delimiter == "END"),
        "a heredoc term",
    );
}

#[test]
fn fat_arrow_hash_method_package_subscript_and_direct_call_remain_non_repetition() {
    for source in [
        r#"sub configure; configure x => 3;"#,
        r#"(x => 3)"#,
        r#"$obj->x(3)"#,
        r#"Pkg::x(3)"#,
        r#"$h{x}"#,
        r#"x()"#,
    ] {
        assert_not_repetition(source);
    }
}

#[test]
fn division_after_a_string_is_not_binary_repetition() {
    // Slash is admitted only as the token after operator `x`. A following
    // `/` that is the multiplicative operator itself must stay division.
    let source = r#"my $value = "x" / 3;"#;
    assert_not_repetition(source);
    let (ast, errors) = parse_with_errors(source);
    assert!(errors.is_empty(), "unexpected diagnostics for {source}: {errors:#?}");
    assert!(ast.to_sexp().contains("binary_/"), "expected division, got:\n{}", ast.to_sexp());
}

#[test]
fn word_not_and_angle_bracket_terms_remain_outside_this_classifier() {
    for source in
        [r#"my $value = "x" x not 1;"#, r#"my $value = "x" x <>;"#, r#"my $value = "x" x <STDIN>;"#]
    {
        assert_not_repetition(source);
    }
}

#[test]
fn missing_rhs_does_not_fabricate_a_binary_x_node() {
    // `"x" x;` currently splits into a string statement plus a bare `x`
    // statement rather than synthesizing `Binary { op: "x", right: Missing }`.
    // This claim keeps that non-repetition recovery; it does not add a new
    // missing-operand diagnostic.
    let source = r#""x" x;"#;
    assert_not_repetition(source);
}

#[test]
fn yada_yada_ellipsis_is_not_an_expression_rhs_for_binary_x() {
    // Perl 5.38.2 rejects `...` outside statement position (`my $x = ...` and
    // `"x" x ...` are both syntax errors). parse_primary still has an Ellipsis
    // arm, but admitting it here would invent a binary-x node for input the
    // language oracle does not accept. Word `not` and magic constants stay with
    // their own leaves.
    assert_not_repetition(r#"my $value = "x" x ...;"#);
}
