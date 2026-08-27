//! Regression coverage for punctuation-suffixed typeglobs.
//!
//! These forms appeared in the original `unexpected_token_in_expr` CPAN
//! bucket. They are aliases for punctuation variables and must be accepted as
//! typeglob names rather than routed through generic expression recovery.

mod cpan_test_helpers;
use cpan_test_helpers::assert_clean_parse;
use perl_ast::ast::{Node, NodeKind};
use perl_parser_core::Parser;
use perl_tdd_support::must;

fn collect_typeglob_names(node: &Node, names: &mut Vec<String>) {
    if let NodeKind::Typeglob { name } = &node.kind {
        names.push(name.clone());
    }

    for child in node.children() {
        collect_typeglob_names(child, names);
    }
}

fn contains_error_node(node: &Node) -> bool {
    matches!(node.kind, NodeKind::Error { .. })
        || node.children().into_iter().any(contains_error_node)
}

fn assert_typeglob_rhs(source: &str, expected: &str) {
    let mut parser = Parser::new(source);
    let ast = must(parser.parse());
    let mut names = Vec::new();
    collect_typeglob_names(&ast, &mut names);

    assert!(!contains_error_node(&ast), "typeglob should not produce Error nodes in {source:?}");
    assert!(
        names.iter().any(|name| name == expected),
        "missing RHS typeglob {expected:?} in {source:?}: {names:?}"
    );
}

#[test]
fn typeglob_backtick_name_parses() {
    assert_typeglob_rhs("*STDOUT = *`;", "`");
}

#[test]
fn typeglob_apostrophe_name_parses() {
    assert_typeglob_rhs("*STDOUT = *';", "'");
}

#[test]
fn punctuation_typeglobs_can_be_declared_together() {
    let source = r#"
*STDOUT = *`;
*STDERR = *';
"#;
    assert_clean_parse(source);
    assert_typeglob_rhs(source, "`");
    assert_typeglob_rhs(source, "'");
}

#[test]
fn quote_typeglob_does_not_swallow_the_following_statement() {
    // The lexer used to run unterminated-string recovery on the quote
    // character, silently consuming the rest of the line (same-line form) or
    // producing an Error node (newline form).
    for source in [
        "*STDOUT = *`; my $x = 1;",
        "*STDOUT = *`;\nmy $x = 1;",
        "*STDERR = *';\nmy $x = 1;",
        "*LIST_SEPARATOR = *\";\nmy $x = 1;",
    ] {
        let mut parser = Parser::new(source);
        let ast = must(parser.parse());
        let sexp = format!("{ast:?}");
        assert!(
            sexp.contains("VariableDeclaration"),
            "statement after the quote typeglob was swallowed in {source:?}"
        );
    }
}

#[test]
fn multiplication_by_a_string_is_not_a_typeglob() {
    // `$a * "..."` is multiplication: the glob-sigil rescue must not fire
    // when an operand precedes the star.
    let mut parser = Parser::new("my $b = $a * \"ops\";");
    let ast = must(parser.parse());
    let sexp = format!("{ast:?}");
    assert!(sexp.contains("String"), "string operand lost to the typeglob rescue: {sexp}");
}
