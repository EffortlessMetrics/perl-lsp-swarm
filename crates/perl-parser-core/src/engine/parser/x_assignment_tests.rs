#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Regression proof for the contiguous string-repetition assignment operator
//! `x=` (issue #13179).
//!
//! The lexer keeps word-shaped `x` contextual, so `x=` arrives as
//! `Identifier("x")` immediately followed by `Assign`. The parser must fold
//! that adjacent pair into `NodeKind::Assignment` with operator text `x=`
//! through the ordinary assignment-precedence path, while a spaced `x =`
//! keeps its bareword/identifier reading.

use super::*;
use perl_tdd_support::must;

#[test]
fn test_contiguous_x_assign_produces_assignment_node() {
    // $value x= 3; must produce an Assignment node with operator text `x=`.
    let mut parser = Parser::new("$value x= 3;");
    let ast = must(parser.parse());
    let sexp = ast.to_sexp();
    assert!(sexp.contains("(assignment_xassign"), "Expected assignment_xassign in: {sexp}");
    assert!(sexp.contains("(op x=)"), "Expected op `x=` in: {sexp}");
}

#[test]
fn test_x_assign_lhs_and_rhs_shapes() {
    // The LHS is the repeated expression and the RHS is the count.
    let mut parser = Parser::new("$value x= 3;");
    let ast = must(parser.parse());
    let sexp = ast.to_sexp();
    assert!(
        sexp.contains("(lhs (variable (sigil $) (name value)))"),
        "Expected $value LHS in: {sexp}"
    );
    assert!(sexp.contains("(rhs (number (value 3)))"), "Expected number RHS in: {sexp}");
}

#[test]
fn test_x_assign_is_right_associative() {
    // Assignment remains right-associative: `$left x= $right = 2` groups as
    // `$left x= ($right = 2)`.
    let mut parser = Parser::new("$left x= $right = 2;");
    let ast = must(parser.parse());
    let sexp = ast.to_sexp();
    assert!(sexp.contains("(assignment_xassign"), "Expected outer assignment_xassign in: {sexp}");
    // The plain `=` assignment must nest as the direct RHS of `x=`.
    assert!(
        sexp.contains("(rhs (assignment_assign"),
        "Expected plain assignment nested as x= RHS in: {sexp}"
    );
}

#[test]
fn test_spaced_x_equals_is_not_repetition_assignment() {
    // Perl rejects spaced `x =` as the repetition assignment; the source must
    // keep its identifier-assignment reading, never normalize to `x=`.
    let mut parser = Parser::new("$value x = 3;");
    let ast = must(parser.parse());
    let sexp = ast.to_sexp();
    assert!(
        !sexp.contains("assignment_xassign"),
        "Spaced `x =` must not normalize into the operator: {sexp}"
    );
    assert!(
        sexp.contains("(assignment_assign"),
        "Expected ordinary identifier assignment in: {sexp}"
    );
}

#[test]
fn test_tab_between_x_and_equals_is_not_repetition_assignment() {
    // Exact adjacency means any intervening source gap disqualifies the
    // operator pair, not just a single space.
    let mut parser = Parser::new("$value x\t= 3;");
    let ast = must(parser.parse());
    let sexp = ast.to_sexp();
    assert!(
        !sexp.contains("assignment_xassign"),
        "Tab-separated `x\\t=` must not normalize into the operator: {sexp}"
    );
}

#[test]
fn test_comment_between_x_and_equals_is_not_repetition_assignment() {
    // Trivia must not be treated as adjacency: tokens can be stream-adjacent
    // while the source characters are not.
    let mut parser = Parser::new("$value x # gap\n= 3;");
    let ast = must(parser.parse());
    let sexp = ast.to_sexp();
    assert!(
        !sexp.contains("assignment_xassign"),
        "Comment-separated `x` / `=` must not normalize into the operator: {sexp}"
    );
}

#[test]
fn test_binary_x_repetition_still_parses() {
    // The ordinary binary repetition operator is untouched.
    let mut parser = Parser::new("$line = \"-\" x 80;");
    let ast = must(parser.parse());
    let sexp = ast.to_sexp();
    assert!(sexp.contains("binary_x"), "Expected binary_x in: {sexp}");
    assert!(
        !sexp.contains("assignment_xassign"),
        "Binary repetition must not become the operator: {sexp}"
    );
}

#[test]
fn test_identifiers_named_x_still_parse() {
    // A variable named $x and a call named x() keep their meanings.
    let mut parser = Parser::new("$x = 5;");
    let ast = must(parser.parse());
    let sexp = ast.to_sexp();
    assert!(sexp.contains("(assignment_assign"), "Expected ordinary assignment in: {sexp}");
    assert!(
        !sexp.contains("assignment_xassign"),
        "$x assignment must not become the operator: {sexp}"
    );

    let mut parser = Parser::new("x(3);");
    let ast = must(parser.parse());
    let sexp = ast.to_sexp();
    assert!(!sexp.contains("ERROR"), "Call named x() must parse cleanly: {sexp}");
    assert!(
        !sexp.contains("assignment_xassign"),
        "Call named x() must not become the operator: {sexp}"
    );
}

#[test]
fn test_word_not_equal_to_x_is_not_repetition_assignment() {
    // Only the exact word `x` forms the operator: adjacent `xx=` tokenizes as
    // Identifier("xx") + Assign — the same two-token shape — but must stay an
    // identifier assignment of `xx`.
    let mut parser = Parser::new("$handled = 0; xx= 3;");
    let ast = must(parser.parse());
    let sexp = ast.to_sexp();
    assert!(
        !sexp.contains("assignment_xassign"),
        "Identifier `xx` must not form the operator: {sexp}"
    );
}

#[test]
fn test_x_assign_nests_through_assignment_precedence_path() {
    // The operator is recognized through the ordinary assignment-precedence
    // path: a `my` declaration initializer is parsed at assignment level, so
    // `my $c = $v x= 2` carries the x= assignment as the initializer.
    let mut parser = Parser::new("my $c = $v x= 2;");
    let ast = must(parser.parse());
    let sexp = ast.to_sexp();
    assert!(
        sexp.contains("(assignment_xassign"),
        "Expected assignment_xassign in initializer context: {sexp}"
    );
    assert!(
        sexp.contains("(initializer (assignment_xassign"),
        "Expected x= assignment as the declaration initializer: {sexp}"
    );
}

#[test]
fn test_x_assign_after_lvalue_builtin() {
    // Statement-start lvalue builtins route through the specialized
    // assignment tail; contiguous `x=` must be recognized there too.
    let mut parser = Parser::new("substr($s, 0, 1) x= 2;");
    let ast = must(parser.parse());
    let sexp = ast.to_sexp();
    assert!(
        sexp.contains("(assignment_xassign"),
        "Expected assignment_xassign after substr lvalue in: {sexp}"
    );
    assert!(sexp.contains("(op x=)"), "Expected op `x=` in: {sexp}");
}

#[test]
fn test_spaced_x_after_lvalue_builtin_is_not_repetition_assignment() {
    // The adjacency boundary holds on the lvalue-builtin tail as well.
    let mut parser = Parser::new("pos($s) x = 0;");
    let ast = must(parser.parse());
    let sexp = ast.to_sexp();
    assert!(
        !sexp.contains("assignment_xassign"),
        "Spaced `x =` must not normalize into the operator: {sexp}"
    );
    assert!(
        sexp.contains("(assignment_assign"),
        "Expected ordinary assignment on the lvalue builtin in: {sexp}"
    );
}
