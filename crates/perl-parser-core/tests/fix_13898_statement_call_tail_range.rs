//! Focused regression proof for #13898: a statement-start builtin /
//! named-unary call's remaining expression tail applies Perl's range operator
//! after the symbolic bitwise/logical operators and before ternary, comma, and
//! the low-precedence word operators, so forms like `shift || $b .. $c` keep
//! their `..` tail instead of erroring.
//!
//! The harness is fallible (Option-returning extractors plus `must_some`) so
//! the workspace `clippy::panic` policy keeps covering this file; shape
//! mismatches still report the full AST via `assert!`/`assert_eq!`.

mod cpan_test_helpers;
use cpan_test_helpers::*;
use perl_parser_core::{Node, NodeKind};
use perl_tdd_support::must_some;

#[derive(Clone, Copy)]
enum NestedSide {
    Left,
    Right,
}

/// Return the expression a statement-call-tail test should inspect: the wrapped
/// expression for ordinary expression statements, or the statement itself for
/// the indirect-call route, which returns the operator tail unwrapped
/// (`close FH || $b .. $c;`). `close FH;` alone yields `IndirectCall` and every
/// other route here yields `ExpressionStatement`.
fn expression_statement(ast: &Node) -> Option<&Node> {
    let NodeKind::Program { statements } = &ast.kind else {
        return None;
    };
    let statement = statements.first()?;
    match &statement.kind {
        NodeKind::ExpressionStatement { expression } => Some(expression),
        NodeKind::Binary { .. } => Some(statement),
        _ => None,
    }
}

fn binary_parts(node: &Node) -> Option<(&str, &Node, &Node)> {
    match &node.kind {
        NodeKind::Binary { op, left, right } => Some((op.as_str(), left, right)),
        _ => None,
    }
}

fn function_call_parts(node: &Node) -> Option<(&str, &[Node])> {
    match &node.kind {
        NodeKind::FunctionCall { name, args } => Some((name.as_str(), args.as_slice())),
        _ => None,
    }
}

fn ternary_condition(node: &Node) -> Option<&Node> {
    match &node.kind {
        NodeKind::Ternary { condition, .. } => Some(condition.as_ref()),
        _ => None,
    }
}

fn array_elements(node: &Node) -> Option<&[Node]> {
    match &node.kind {
        NodeKind::ArrayLiteral { elements } => Some(elements.as_slice()),
        _ => None,
    }
}

/// Extract the outer binary operator of the inspected expression. The shape is
/// asserted with the full AST before extraction so mismatches stay debuggable.
fn outer_binary<'n>(ast: &'n Node, source: &str) -> (&'n str, &'n Node, &'n Node) {
    let expression = expression_statement(ast);
    assert!(
        expression.is_some(),
        "expected ExpressionStatement or indirect-call Binary for `{source}`, got:\n{}",
        ast.to_sexp()
    );
    let expression = must_some(expression);
    assert!(
        matches!(&expression.kind, NodeKind::Binary { .. }),
        "expected outer binary for `{source}`, got {:?}:\n{}",
        expression.kind,
        ast.to_sexp()
    );
    must_some(binary_parts(expression))
}

fn assert_function_call(node: &Node, name: &str, args_len: usize, context: &str) {
    assert!(
        matches!(&node.kind, NodeKind::FunctionCall { name: actual, args } if actual == name && args.len() == args_len),
        "expected {name} call with {args_len} argument(s) for {context}, got {:?}",
        node.kind
    );
}

fn assert_variable(node: &Node, name: &str, context: &str) {
    assert!(
        matches!(&node.kind, NodeKind::Variable { sigil, name: actual } if sigil == "$" && actual == name),
        "expected ${name} for {context}, got {:?}",
        node.kind
    );
}

/// Assert `(builtin <nested_op> $b) .. $c` for `NestedSide::Left` and
/// `builtin .. ($b <nested_op> $c)` for `NestedSide::Right`, including the
/// builtin call identity so a parenthesized ordinary-expression substitute
/// cannot pass.
fn assert_range_with_nested_symbolic(
    source: &str,
    builtin: (&str, usize),
    nested_side: NestedSide,
    nested_op: &str,
) {
    assert_clean_parse(source);
    let ast = parse(source);
    let (op, left, right) = outer_binary(&ast, source);
    assert_eq!(op, "..", "expected range outer for `{source}`:\n{}", ast.to_sexp());

    let nested = match nested_side {
        NestedSide::Left => left,
        NestedSide::Right => right,
    };
    assert!(
        matches!(&nested.kind, NodeKind::Binary { .. }),
        "expected nested {nested_op} for `{source}`, got {:?}:\n{}",
        nested.kind,
        ast.to_sexp()
    );
    let (actual_nested, nested_left, _) = must_some(binary_parts(nested));
    assert_eq!(
        actual_nested,
        nested_op,
        "unexpected nested operator for `{source}`:\n{}",
        ast.to_sexp()
    );
    let (name, args_len) = builtin;
    assert_function_call(
        match nested_side {
            NestedSide::Left => nested_left,
            NestedSide::Right => left,
        },
        name,
        args_len,
        source,
    );
}

fn assert_bareword_call_route(source: &str, args_len: usize) {
    assert_clean_parse(source);
    let ast = parse(source);
    let (op, left, right) = outer_binary(&ast, source);
    assert_eq!(op, "..", "expected range outer for `{source}`:\n{}", ast.to_sexp());
    assert_variable(right, "c", source);

    assert!(
        matches!(&left.kind, NodeKind::Binary { .. }),
        "expected nested || for `{source}`, got {:?}:\n{}",
        left.kind,
        ast.to_sexp()
    );
    let (op, call, rhs) = must_some(binary_parts(left));
    assert_eq!(op, "||", "unexpected nested operator for `{source}`:\n{}", ast.to_sexp());
    assert_variable(rhs, "b", source);
    if args_len == 0 {
        assert!(
            matches!(&call.kind, NodeKind::Identifier { name } if name == "foo"),
            "expected `foo` bareword for {source}, got {:?}",
            call.kind
        );
    } else {
        assert_function_call(call, "foo", args_len, source);
    }
}

fn assert_list_operator_route() {
    let source = "print $x || $b .. $c;";
    assert_clean_parse(source);
    let ast = parse(source);
    let expression = expression_statement(&ast);
    assert!(
        expression.is_some_and(|node| matches!(&node.kind, NodeKind::FunctionCall { .. })),
        "expected print call for `{source}`, got:\n{}",
        ast.to_sexp()
    );
    let (name, args) = must_some(function_call_parts(must_some(expression)));
    assert_eq!(name, "print", "unexpected callee for `{source}`:\n{}", ast.to_sexp());
    assert_eq!(args.len(), 1, "expected one print argument for `{source}`, got {args:?}");
    let argument = must_some(args.first());

    assert!(
        matches!(&argument.kind, NodeKind::Binary { .. }),
        "expected outer range for `{source}`, got {:?}:\n{}",
        argument.kind,
        ast.to_sexp()
    );
    let (op, left, right) = must_some(binary_parts(argument));
    assert_eq!(op, "..", "expected range outer for `{source}`:\n{}", ast.to_sexp());
    assert_variable(right, "c", source);

    assert!(
        matches!(&left.kind, NodeKind::Binary { .. }),
        "expected nested || for `{source}`, got {:?}:\n{}",
        left.kind,
        ast.to_sexp()
    );
    let (op, nested_left, rhs) = must_some(binary_parts(left));
    assert_eq!(op, "||", "unexpected nested operator for `{source}`:\n{}", ast.to_sexp());
    assert_variable(nested_left, "x", source);
    assert_variable(rhs, "b", source);
}

fn assert_indirect_call_route() {
    let source = "close FH || $b .. $c;";
    assert_clean_parse(source);
    let ast = parse(source);
    let (op, left, right) = outer_binary(&ast, source);
    assert_eq!(op, "..", "expected range outer for `{source}`:\n{}", ast.to_sexp());
    assert_variable(right, "c", source);

    assert!(
        matches!(&left.kind, NodeKind::Binary { .. }),
        "expected nested || for `{source}`, got {:?}:\n{}",
        left.kind,
        ast.to_sexp()
    );
    let (op, call, rhs) = must_some(binary_parts(left));
    assert_eq!(op, "||", "unexpected nested operator for `{source}`:\n{}", ast.to_sexp());
    assert_variable(rhs, "b", source);
    assert!(
        matches!(
            &call.kind,
            NodeKind::IndirectCall { method, object, args }
                if method == "close"
                    && args.is_empty()
                    && matches!(&object.kind, NodeKind::Identifier { name } if name == "FH")
        ),
        "expected `close FH` indirect call for {source}, got {:?}",
        call.kind
    );
}

#[test]
fn statement_call_tail_range_is_outside_all_symbolic_operators() {
    for (source, nested_op) in [
        ("time & $b .. $c;", "&"),
        ("time ^ $b .. $c;", "^"),
        ("time | $b .. $c;", "|"),
        ("time && $b .. $c;", "&&"),
        ("shift || $b .. $c;", "||"),
        ("time // $b .. $c;", "//"),
    ] {
        let builtin = if nested_op == "||" { "shift" } else { "time" };
        assert_range_with_nested_symbolic(source, (builtin, 0), NestedSide::Left, nested_op);
    }

    for (source, nested_op) in [
        ("time .. $b & $c;", "&"),
        ("time .. $b ^ $c;", "^"),
        ("time .. $b | $c;", "|"),
        ("time .. $b && $c;", "&&"),
        ("time .. $b || $c;", "||"),
        ("time .. $b // $c;", "//"),
    ] {
        assert_range_with_nested_symbolic(source, ("time", 0), NestedSide::Right, nested_op);
    }
}

#[test]
fn statement_call_tail_range_reaches_each_call_site() {
    assert_range_with_nested_symbolic("ref $x || $b .. $c;", ("ref", 1), NestedSide::Left, "||");
    assert_range_with_nested_symbolic("ref $x .. $b && $c;", ("ref", 1), NestedSide::Right, "&&");
    assert_range_with_nested_symbolic("lc $x | $b .. $c;", ("lc", 1), NestedSide::Left, "|");
    assert_indirect_call_route();
}

#[test]
fn statement_call_tail_range_covers_representative_nullary_builtins() {
    for (source, builtin) in [
        ("caller || $b .. $c;", "caller"),
        ("wantarray || $b .. $c;", "wantarray"),
        ("localtime || $b .. $c;", "localtime"),
    ] {
        assert_range_with_nested_symbolic(source, (builtin, 0), NestedSide::Left, "||");
    }
}

/// The unknown-lowercase-bareword and list-operator statement forms reach the same
/// dispatch but do not depend on the range rung's position: each shape below parses
/// identically when `parse_range_with` is moved back after equality, so these are
/// controls on neighboring routes rather than proof of the reorder.
#[test]
fn statement_call_tail_range_neighboring_routes_are_unchanged() {
    assert_bareword_call_route("foo $x || $b .. $c;", 1);
    assert_bareword_call_route("foo || $b .. $c;", 0);
    assert_list_operator_route();
}

#[test]
fn statement_call_tail_range_stays_inside_ternary_condition() {
    let source = "time .. $b ? $c : $d;";
    assert_clean_parse(source);
    let ast = parse(source);
    let expression = expression_statement(&ast);
    assert!(
        expression.is_some_and(|node| matches!(&node.kind, NodeKind::Ternary { .. })),
        "expected outer ternary for `{source}`, got:\n{}",
        ast.to_sexp()
    );
    let condition = must_some(ternary_condition(must_some(expression)));
    assert!(
        matches!(&condition.kind, NodeKind::Binary { op, .. } if op == ".."),
        "expected range in ternary condition for `{source}`:\n{}",
        ast.to_sexp()
    );
}

#[test]
fn statement_call_tail_word_operators_stay_outside_range() {
    for (source, word_op) in [("time .. $b or $c;", "or"), ("time .. $b and $c;", "and")] {
        assert_clean_parse(source);
        let ast = parse(source);
        let (op, left, _) = outer_binary(&ast, source);
        assert_eq!(
            op,
            word_op,
            "unexpected outer word operator for `{source}`:\n{}",
            ast.to_sexp()
        );
        assert!(
            matches!(&left.kind, NodeKind::Binary { op, .. } if op == ".."),
            "expected range in {word_op} operand for `{source}`:\n{}",
            ast.to_sexp()
        );
    }
}

#[test]
fn statement_call_tail_range_preserves_adjacent_boundaries() {
    assert_range_with_nested_symbolic("time == 1 .. 2;", ("time", 0), NestedSide::Left, "==");

    let source = "time .. $b, $c;";
    assert_clean_parse(source);
    let ast = parse(source);
    let expression = expression_statement(&ast);
    assert!(
        expression.is_some_and(|node| matches!(&node.kind, NodeKind::ArrayLiteral { .. })),
        "expected comma array for `{source}`, got:\n{}",
        ast.to_sexp()
    );
    let elements = must_some(array_elements(must_some(expression)));
    assert_eq!(elements.len(), 2, "expected two comma elements for `{source}`:\n{}", ast.to_sexp());
    let first = must_some(elements.first());
    assert!(
        matches!(&first.kind, NodeKind::Binary { op, .. } if op == ".."),
        "expected range in comma operand for `{source}`:\n{}",
        ast.to_sexp()
    );
    let second = must_some(elements.get(1));
    assert!(
        matches!(&second.kind, NodeKind::Variable { sigil, name } if sigil == "$" && name == "c"),
        "expected `$c` after range comma for `{source}`:\n{}",
        ast.to_sexp()
    );

    let source = "time & $b .. $c ^ $d;";
    assert_clean_parse(source);
    let ast = parse(source);
    let (op, left, right) = outer_binary(&ast, source);
    assert_eq!(op, "..", "unexpected outer range for `{source}`:\n{}", ast.to_sexp());
    assert!(
        matches!(&left.kind, NodeKind::Binary { op, .. } if op == "&"),
        "expected bitwise-and on range left for `{source}`:\n{}",
        ast.to_sexp()
    );
    assert!(
        matches!(&right.kind, NodeKind::Binary { op, .. } if op == "^"),
        "expected bitwise-xor on range right for `{source}`:\n{}",
        ast.to_sexp()
    );
}

/// The reordered tail shares the single `parse_range_with` rung with ordinary
/// expressions, so the non-associative range boundary (#13903) applies here
/// too: a second unparenthesized `..` is a syntax error, not a silent
/// left-fold, for every statement-call-tail route.
#[test]
fn statement_call_tail_chained_range_is_rejected() {
    assert_has_blocking_error("time .. $b .. $c;", "range");
    assert_has_blocking_error("shift || $b .. $c .. $d;", "range");
    assert_has_blocking_error("ref $x || $b .. $c .. $d;", "range");
    assert_has_blocking_error("close FH || $b .. $c .. $d;", "range");
}
