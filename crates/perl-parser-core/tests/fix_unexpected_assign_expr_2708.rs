//! Regression for issue #2708 — CPAN `unexpected_assign_expr` bucket.
//!
//! Remaining live subject: `Class::Accessor::Grouped` `$cag_eval`:
//!
//! ```perl
//! wantarray
//!   ? @rv = eval $src
//!   : $rv[0] = eval $src
//! ;
//! ```
//!
//! Perl groups this as `(wantarray ? (@rv = eval $src) : $rv[0]) = eval $src`
//! (`perl -MO=Deparse,-p`). The then-branch assignment is inside the ternary
//! because `:` terminates it; the else-side `=` is ordinary assignment to the
//! ternary lvalue (lower precedence than `?:`).
//!
//! Statement-start nullary builtins (`wantarray`, `shift`, `time`, …) parse
//! through `parse_named_unary_statement_tail`, which climbed from ternary
//! straight to comma and never applied assignment. `$a ? $b = 1 : $c = 2`
//! already worked because that path goes through `parse_assignment`.

mod cpan_test_helpers;
use cpan_test_helpers::*;
use perl_parser_core::{NodeKind, Parser};
use perl_tdd_support::must_some_with;

fn assign_errors(source: &str) -> Vec<String> {
    let mut parser = Parser::new(source);
    let _ = parser.parse();
    parser
        .errors()
        .iter()
        .filter(|error| error.to_string().contains("found '='"))
        .map(ToString::to_string)
        .collect()
}

#[test]
fn identifier_ternary_lvalue_assignment_still_parses() {
    assert_clean_parse("$a ? $b = 1 : $c = 2;");
}

#[test]
fn wantarray_ternary_without_assignment_still_parses() {
    assert_clean_parse("wantarray ? 1 : 0;");
}

#[test]
fn wantarray_ternary_else_assignment_parses_clean() {
    assert_clean_parse("wantarray ? $a = 1 : $b = 2;");
}

#[test]
fn wantarray_grouped_eval_lvalue_shape_parses_clean() {
    assert_clean_parse(
        r#"
        my (@rv, $src);
        wantarray
          ? @rv = eval $src
          : $rv[0] = eval $src
        ;
        "#,
    );
}

#[test]
fn wantarray_ternary_else_assignment_is_outer_assign_not_else_assign() {
    let ast = parse("wantarray ? $a = 1 : $b = 2;");
    assert!(
        matches!(&ast.kind, NodeKind::Program { .. }),
        "expected program, got {}",
        ast.kind.kind_name()
    );
    let NodeKind::Program { statements } = &ast.kind else {
        return;
    };
    let stmt = must_some_with(statements.first(), "statement");
    assert!(
        matches!(&stmt.kind, NodeKind::ExpressionStatement { .. }),
        "expected expression statement, got {}",
        stmt.kind.kind_name()
    );
    let NodeKind::ExpressionStatement { expression } = &stmt.kind else {
        return;
    };
    assert!(
        matches!(&expression.kind, NodeKind::Assignment { .. }),
        "expected outer assignment, got {}",
        expression.kind.kind_name()
    );
    let NodeKind::Assignment { lhs, rhs, op } = &expression.kind else {
        return;
    };
    assert_eq!(op, "=");
    assert!(
        matches!(&rhs.kind, NodeKind::Number { .. }),
        "outer RHS should be the else assignment's RHS, got {}",
        rhs.kind.kind_name()
    );
    assert!(
        matches!(&lhs.kind, NodeKind::Ternary { .. }),
        "outer LHS should be the ternary, got {}",
        lhs.kind.kind_name()
    );
    let NodeKind::Ternary { then_expr, else_expr, .. } = &lhs.kind else {
        return;
    };
    assert!(
        matches!(&then_expr.kind, NodeKind::Assignment { .. }),
        "then-branch keeps its assignment because ':' terminates it, got {}",
        then_expr.kind.kind_name()
    );
    assert!(
        !matches!(&else_expr.kind, NodeKind::Assignment { .. }),
        "else-branch is the ternary lvalue, not an inner assignment; got {}",
        else_expr.kind.kind_name()
    );
}

#[test]
fn other_nullary_builtin_ternary_lvalue_assignment_parses() {
    assert_clean_parse("time ? $a = 1 : $b = 2;");
    assert_clean_parse("shift ? $a = 1 : $b = 2;");
}

#[test]
fn wantarray_ternary_assignment_does_not_swallow_following_word_or() {
    assert_clean_parse("wantarray ? $a = 1 : $b = 2 or die;");
}

#[test]
fn grouped_pm_fixture_has_no_unexpected_assign_expr() {
    let fixture = include_str!("fixtures/class_accessor_grouped.pm");
    let errors = assign_errors(fixture);
    assert!(errors.is_empty(), "unexpected '=' diagnostics: {errors:?}");
}

#[test]
fn named_unary_without_ternary_is_not_forced_assignment_lhs() {
    let ast = parse("length($x) = 5;");
    let NodeKind::Program { statements } = &ast.kind else {
        return;
    };
    for stmt in statements {
        let NodeKind::ExpressionStatement { expression } = &stmt.kind else {
            continue;
        };
        let NodeKind::Assignment { lhs, .. } = &expression.kind else {
            continue;
        };
        let NodeKind::FunctionCall { name, .. } = &lhs.kind else {
            continue;
        };
        assert_ne!(
            name.as_str(),
            "length",
            "length() must not become an assignment target; sexp:\n{}",
            ast.to_sexp()
        );
    }
}
