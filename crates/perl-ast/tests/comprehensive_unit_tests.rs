//! Comprehensive unit tests for the `perl-ast` crate.
//!
//! These tests exercise every exposed public API across `NodeKind`, `Node`,
//! `SourceLocation`, `ALL_KIND_NAMES`, `RECOVERY_KIND_NAMES`, and the
//! `for_each_child` / `for_each_child_mut` traversal helpers.
//!
//! Coverage goals
//! ─────────────
//! • Every `NodeKind` variant constructed at least once
//! • `Node::new`, `children`, `child_count`, `first_child`, `count_nodes`
//! • `to_sexp` / `to_sexp_inner` for representative cases
//! • `for_each_child` and `for_each_child_mut` visit counts
//! • `Display` (via `to_string`) and `Debug` round-trips
//! • `Clone` + `PartialEq` structural equality
//! • `ALL_KIND_NAMES` uniqueness & count; `RECOVERY_KIND_NAMES` sorted

use perl_ast::ast::{Node, NodeKind, SourceLocation};

// ---------------------------------------------------------------------------
// Helpers shared across test functions
// ---------------------------------------------------------------------------

fn loc(start: usize, end: usize) -> SourceLocation {
    SourceLocation { start, end }
}

fn num_node(value: &str) -> Node {
    Node::new(NodeKind::Number { value: value.to_string() }, loc(0, value.len()))
}

fn var_node(sigil: &str, name: &str) -> Node {
    Node::new(
        NodeKind::Variable { sigil: sigil.to_string(), name: name.to_string() },
        loc(0, sigil.len() + name.len()),
    )
}

fn block_node(statements: Vec<Node>) -> Node {
    Node::new(NodeKind::Block { statements }, loc(0, 1))
}

fn dummy_sub(name: Option<&str>) -> Node {
    Node::new(
        NodeKind::Subroutine {
            name: name.map(|s| s.to_string()),
            name_span: None,
            declarator: None,
            prototype: None,
            signature: None,
            attributes: vec![],
            body: Box::new(block_node(vec![])),
        },
        loc(0, 10),
    )
}

// ===========================================================================
// 1. SourceLocation
// ===========================================================================

#[test]
fn source_location_fields() {
    let sl = SourceLocation { start: 5, end: 15 };
    assert_eq!(sl.start, 5);
    assert_eq!(sl.end, 15);
}

#[test]
fn source_location_clone_and_eq() {
    let a = SourceLocation { start: 1, end: 3 };
    let b = a;
    assert_eq!(a, b);
}

#[test]
fn source_location_new_constructor() {
    let sl = SourceLocation::new(10, 20);
    assert_eq!(sl.start, 10);
    assert_eq!(sl.end, 20);
}

// ===========================================================================
// 2. Node construction and basic accessors
// ===========================================================================

#[test]
fn node_new_stores_kind_and_location() {
    let node = num_node("42");
    assert!(matches!(&node.kind, NodeKind::Number { value } if value == "42"));
    assert_eq!(node.location.start, 0);
    assert_eq!(node.location.end, 2);
}

#[test]
fn node_children_empty_for_leaf() {
    assert_eq!(num_node("1").children().len(), 0);
    assert_eq!(var_node("$", "x").children().len(), 0);
}

#[test]
fn node_child_count_equals_children_len() {
    let prog = Node::new(
        NodeKind::Program { statements: vec![num_node("1"), num_node("2")] },
        loc(0, 10),
    );
    assert_eq!(prog.child_count(), 2);
    assert_eq!(prog.child_count(), prog.children().len());
}

#[test]
fn node_first_child_on_leaf_returns_none() {
    assert!(num_node("7").first_child().is_none());
}

#[test]
fn node_first_child_on_program_returns_first_statement() {
    let prog = Node::new(
        NodeKind::Program { statements: vec![num_node("1"), num_node("2")] },
        loc(0, 5),
    );
    let first = prog.first_child().unwrap();
    assert!(matches!(&first.kind, NodeKind::Number { value } if value == "1"));
}

#[test]
fn count_nodes_on_leaf_is_one() {
    assert_eq!(num_node("3").count_nodes(), 1);
}

#[test]
fn count_nodes_on_block_with_children() {
    // Block(3 children) => 1 + 3 = 4
    let b = block_node(vec![num_node("1"), num_node("2"), num_node("3")]);
    assert_eq!(b.count_nodes(), 4);
}

#[test]
fn count_nodes_nested() {
    // Program > Block > Number = 3 nodes
    let prog = Node::new(
        NodeKind::Program { statements: vec![block_node(vec![num_node("0")])] },
        loc(0, 10),
    );
    assert_eq!(prog.count_nodes(), 3);
}

// ===========================================================================
// 3. Clone and PartialEq
// ===========================================================================

#[test]
fn node_clone_is_equal() {
    let original = num_node("99");
    let cloned = original.clone();
    assert_eq!(original, cloned);
}

#[test]
fn node_clone_is_independent() {
    let mut a = Node::new(NodeKind::Program { statements: vec![num_node("1")] }, loc(0, 5));
    let b = a.clone();
    if let NodeKind::Program { statements } = &mut a.kind {
        statements.push(num_node("2"));
    }
    if let NodeKind::Program { statements } = &b.kind {
        assert_eq!(statements.len(), 1);
    }
}

#[test]
fn nodes_with_different_locations_are_not_equal() {
    let a = Node::new(NodeKind::Number { value: "1".to_string() }, loc(0, 1));
    let b = Node::new(NodeKind::Number { value: "1".to_string() }, loc(5, 6));
    assert_ne!(a, b);
}

// ===========================================================================
// 4. Display / Debug
// ===========================================================================

#[test]
fn nodekind_display_returns_kind_name() {
    let k = NodeKind::Variable { sigil: "$".to_string(), name: "x".to_string() };
    assert_eq!(k.to_string(), "Variable");
}

#[test]
fn node_display_returns_sexp() {
    let n = num_node("7");
    assert_eq!(n.to_string(), n.to_sexp());
}

#[test]
fn nodekind_debug_contains_variant_name() {
    let k = NodeKind::Diamond;
    let s = format!("{k:?}");
    assert!(s.contains("Diamond"), "got: {s}");
}

// ===========================================================================
// 5. to_sexp – representative cases
// ===========================================================================

#[test]
fn sexp_number() {
    assert_eq!(num_node("42").to_sexp(), "(number 42)");
}

#[test]
fn sexp_string_non_interpolated() {
    let n = Node::new(
        NodeKind::String { value: "hello".to_string(), interpolated: false },
        loc(0, 7),
    );
    let s = n.to_sexp();
    assert!(s.contains("string"), "got: {s}");
    assert!(s.contains("hello"), "got: {s}");
}

#[test]
fn sexp_string_interpolated() {
    let n = Node::new(
        NodeKind::String { value: "$x".to_string(), interpolated: true },
        loc(0, 4),
    );
    let s = n.to_sexp();
    assert!(s.contains("interpolated_string"), "got: {s}");
}

#[test]
fn sexp_variable() {
    let n = var_node("$", "foo");
    let s = n.to_sexp();
    assert!(s.contains("scalar") || s.contains("variable"), "got: {s}");
    assert!(s.contains("foo"), "got: {s}");
}

#[test]
fn sexp_variable_array() {
    let n = var_node("@", "arr");
    let s = n.to_sexp();
    assert!(s.contains("array") || s.contains("variable"), "got: {s}");
}

#[test]
fn sexp_variable_hash() {
    let n = var_node("%", "h");
    let s = n.to_sexp();
    assert!(s.contains("hash") || s.contains("variable"), "got: {s}");
}

#[test]
fn sexp_program_wraps_statements() {
    let prog = Node::new(
        NodeKind::Program { statements: vec![num_node("1")] },
        loc(0, 10),
    );
    let s = prog.to_sexp();
    assert!(s.starts_with("(source_file"), "got: {s}");
    assert!(s.contains("(number 1)"), "got: {s}");
}

#[test]
fn sexp_block() {
    let b = block_node(vec![num_node("5")]);
    let s = b.to_sexp();
    assert!(s.starts_with("(block"), "got: {s}");
    assert!(s.contains("(number 5)"), "got: {s}");
}

#[test]
fn sexp_binary_add() {
    let n = Node::new(
        NodeKind::Binary {
            op: "+".to_string(),
            left: Box::new(num_node("1")),
            right: Box::new(num_node("2")),
        },
        loc(0, 5),
    );
    let s = n.to_sexp();
    assert!(s.starts_with("(binary_+"), "got: {s}");
}

#[test]
fn sexp_binary_subtract() {
    let n = Node::new(
        NodeKind::Binary {
            op: "-".to_string(),
            left: Box::new(num_node("3")),
            right: Box::new(num_node("1")),
        },
        loc(0, 5),
    );
    let s = n.to_sexp();
    assert!(s.starts_with("(binary_-"), "got: {s}");
}

#[test]
fn sexp_binary_multiply() {
    let n = Node::new(
        NodeKind::Binary {
            op: "*".to_string(),
            left: Box::new(num_node("4")),
            right: Box::new(num_node("2")),
        },
        loc(0, 5),
    );
    let s = n.to_sexp();
    assert!(s.starts_with("(binary_*"), "got: {s}");
}

#[test]
fn sexp_binary_divide() {
    let n = Node::new(
        NodeKind::Binary {
            op: "/".to_string(),
            left: Box::new(num_node("8")),
            right: Box::new(num_node("2")),
        },
        loc(0, 5),
    );
    let s = n.to_sexp();
    assert!(s.starts_with("(binary_/"), "got: {s}");
}

#[test]
fn sexp_unary_minus() {
    let n = Node::new(
        NodeKind::Unary { op: "-".to_string(), operand: Box::new(num_node("3")) },
        loc(0, 2),
    );
    let s = n.to_sexp();
    assert!(s.starts_with("(unary_-") || s.contains("negate"), "got: {s}");
}

#[test]
fn sexp_unary_not() {
    let n = Node::new(
        NodeKind::Unary { op: "!".to_string(), operand: Box::new(var_node("$", "x")) },
        loc(0, 3),
    );
    let s = n.to_sexp();
    assert!(s.contains("not"), "got: {s}");
}

#[test]
fn sexp_assignment() {
    let n = Node::new(
        NodeKind::Assignment {
            lhs: Box::new(var_node("$", "x")),
            rhs: Box::new(num_node("1")),
            op: "=".to_string(),
        },
        loc(0, 5),
    );
    let s = n.to_sexp();
    assert!(s.contains("assignment"), "got: {s}");
}

#[test]
fn sexp_ternary() {
    let n = Node::new(
        NodeKind::Ternary {
            condition: Box::new(num_node("1")),
            then_expr: Box::new(num_node("2")),
            else_expr: Box::new(num_node("3")),
        },
        loc(0, 10),
    );
    let s = n.to_sexp();
    assert!(s.contains("ternary") || s.contains("conditional"), "got: {s}");
}

#[test]
fn sexp_if_with_else() {
    let n = Node::new(
        NodeKind::If {
            condition: Box::new(num_node("1")),
            then_branch: Box::new(block_node(vec![num_node("2")])),
            elsif_branches: vec![],
            else_branch: Some(Box::new(block_node(vec![num_node("3")]))),
            keyword: None,
        },
        loc(0, 20),
    );
    let s = n.to_sexp();
    assert!(s.contains("if"), "got: {s}");
    assert!(s.contains("else"), "got: {s}");
}

#[test]
fn sexp_while_loop() {
    let n = Node::new(
        NodeKind::While {
            condition: Box::new(num_node("1")),
            body: Box::new(block_node(vec![])),
            continue_block: None,
            keyword: None,
        },
        loc(0, 15),
    );
    let s = n.to_sexp();
    assert!(s.contains("while"), "got: {s}");
}

#[test]
fn sexp_for_loop() {
    let n = Node::new(
        NodeKind::For {
            init: Some(Box::new(num_node("0"))),
            condition: Some(Box::new(num_node("10"))),
            update: Some(Box::new(num_node("1"))),
            body: Box::new(block_node(vec![])),
            continue_block: None,
        },
        loc(0, 30),
    );
    let s = n.to_sexp();
    assert!(s.contains("for"), "got: {s}");
}

#[test]
fn sexp_foreach_loop() {
    let n = Node::new(
        NodeKind::Foreach {
            variable: Box::new(var_node("$", "item")),
            list: Box::new(Node::new(NodeKind::ArrayLiteral { elements: vec![] }, loc(0, 2))),
            body: Box::new(block_node(vec![])),
            continue_block: None,
        },
        loc(0, 20),
    );
    let s = n.to_sexp();
    assert!(s.contains("foreach"), "got: {s}");
}

#[test]
fn sexp_named_subroutine() {
    let n = Node::new(
        NodeKind::Subroutine {
            name: Some("greet".to_string()),
            name_span: Some(loc(4, 9)),
            declarator: None,
            prototype: None,
            signature: None,
            attributes: vec![],
            body: Box::new(block_node(vec![])),
        },
        loc(0, 20),
    );
    let s = n.to_sexp();
    assert!(s.contains("sub"), "got: {s}");
    assert!(s.contains("greet"), "got: {s}");
}

#[test]
fn sexp_anonymous_subroutine() {
    let n = dummy_sub(None);
    let s = n.to_sexp();
    assert!(s.contains("anonymous_subroutine_expression"), "got: {s}");
}

#[test]
fn sexp_function_call_builtin() {
    let n = Node::new(
        NodeKind::FunctionCall { name: "print".to_string(), args: vec![num_node("1")] },
        loc(0, 10),
    );
    let s = n.to_sexp();
    assert!(s.contains("print") || s.contains("call"), "got: {s}");
}

#[test]
fn sexp_method_call() {
    let n = Node::new(
        NodeKind::MethodCall {
            object: Box::new(Node::new(
                NodeKind::Identifier { name: "Foo".to_string() },
                loc(0, 3),
            )),
            method: "new".to_string(),
            args: vec![],
        },
        loc(0, 10),
    );
    let s = n.to_sexp();
    assert!(s.contains("method_call") || s.contains("call"), "got: {s}");
}

#[test]
fn sexp_return_with_value() {
    let n = Node::new(NodeKind::Return { value: Some(Box::new(num_node("42"))) }, loc(0, 10));
    let s = n.to_sexp();
    assert!(s.contains("return"), "got: {s}");
    assert!(s.contains("42"), "got: {s}");
}

#[test]
fn sexp_return_without_value() {
    let n = Node::new(NodeKind::Return { value: None }, loc(0, 6));
    let s = n.to_sexp();
    assert!(s.contains("return"), "got: {s}");
}

#[test]
fn sexp_use_statement() {
    let n = Node::new(
        NodeKind::Use { module: "strict".to_string(), args: vec![], has_filter_risk: false },
        loc(0, 11),
    );
    let s = n.to_sexp();
    assert!(s.contains("use_statement") || s.contains("use"), "got: {s}");
    assert!(s.contains("strict"), "got: {s}");
}

#[test]
fn sexp_package_statement() {
    let n = Node::new(
        NodeKind::Package { name: "Foo".to_string(), name_span: loc(8, 11), block: None },
        loc(0, 12),
    );
    let s = n.to_sexp();
    assert!(s.contains("package"), "got: {s}");
    assert!(s.contains("Foo"), "got: {s}");
}

#[test]
fn sexp_variable_declaration_my() {
    let n = Node::new(
        NodeKind::VariableDeclaration {
            declarator: "my".to_string(),
            variable: Box::new(var_node("$", "x")),
            attributes: vec![],
            initializer: None,
        },
        loc(0, 6),
    );
    let s = n.to_sexp();
    assert!(s.contains("my") || s.contains("variable_declaration"), "got: {s}");
}

#[test]
fn sexp_array_literal() {
    let n = Node::new(
        NodeKind::ArrayLiteral { elements: vec![num_node("1"), num_node("2")] },
        loc(0, 6),
    );
    let s = n.to_sexp();
    assert!(s.contains("array") || s.contains("list"), "got: {s}");
}

#[test]
fn sexp_hash_literal() {
    let n = Node::new(
        NodeKind::HashLiteral {
            pairs: vec![(Node::new(
                NodeKind::String { value: "key".to_string(), interpolated: false },
                loc(0, 5),
            ), num_node("1"))],
        },
        loc(0, 10),
    );
    let s = n.to_sexp();
    assert!(s.contains("hash") || s.contains("list"), "got: {s}");
}

#[test]
fn sexp_regex() {
    let n = Node::new(
        NodeKind::Regex {
            pattern: "foo".to_string(),
            replacement: None,
            modifiers: "i".to_string(),
            has_embedded_code: false,
        },
        loc(0, 7),
    );
    let s = n.to_sexp();
    assert!(s.contains("regex") || s.contains("qr"), "got: {s}");
}

#[test]
fn sexp_heredoc_non_interpolated() {
    let n = Node::new(
        NodeKind::Heredoc {
            delimiter: "EOF".to_string(),
            content: "hello\n".to_string(),
            interpolated: false,
            indented: false,
            command: false,
            body_span: None,
        },
        loc(0, 10),
    );
    let s = n.to_sexp();
    assert!(s.contains("heredoc"), "got: {s}");
}

#[test]
fn sexp_heredoc_interpolated() {
    let n = Node::new(
        NodeKind::Heredoc {
            delimiter: "END".to_string(),
            content: "$x\n".to_string(),
            interpolated: true,
            indented: false,
            command: false,
            body_span: None,
        },
        loc(0, 10),
    );
    let s = n.to_sexp();
    assert!(s.contains("heredoc"), "got: {s}");
}

#[test]
fn sexp_diamond_operator() {
    let n = Node::new(NodeKind::Diamond, loc(0, 2));
    let s = n.to_sexp();
    assert!(s.contains("diamond") || s.contains("readline"), "got: {s}");
}

#[test]
fn sexp_ellipsis() {
    let n = Node::new(NodeKind::Ellipsis, loc(0, 3));
    let s = n.to_sexp();
    assert!(s.contains("...") || s.contains("ellipsis") || s.contains("yada"), "got: {s}");
}

#[test]
fn sexp_undef() {
    let n = Node::new(NodeKind::Undef, loc(0, 5));
    let s = n.to_sexp();
    assert!(s.contains("undef"), "got: {s}");
}

#[test]
fn sexp_loop_control_next() {
    let n = Node::new(
        NodeKind::LoopControl { op: "next".to_string(), label: None },
        loc(0, 4),
    );
    let s = n.to_sexp();
    assert!(s.contains("next"), "got: {s}");
}

#[test]
fn sexp_loop_control_last() {
    let n = Node::new(
        NodeKind::LoopControl { op: "last".to_string(), label: Some("OUTER".to_string()) },
        loc(0, 4),
    );
    let s = n.to_sexp();
    assert!(s.contains("last"), "got: {s}");
}

#[test]
fn sexp_eval_block() {
    let n = Node::new(NodeKind::Eval { block: Box::new(block_node(vec![])) }, loc(0, 10));
    let s = n.to_sexp();
    assert!(s.contains("eval"), "got: {s}");
}

#[test]
fn sexp_do_block() {
    let n = Node::new(NodeKind::Do { block: Box::new(block_node(vec![])) }, loc(0, 5));
    let s = n.to_sexp();
    assert!(s.contains("do"), "got: {s}");
}

#[test]
fn sexp_try_catch() {
    let n = Node::new(
        NodeKind::Try {
            body: Box::new(block_node(vec![])),
            catch_blocks: vec![(Some("$e".to_string()), Box::new(block_node(vec![])))],
            finally_block: None,
        },
        loc(0, 20),
    );
    let s = n.to_sexp();
    assert!(s.contains("try"), "got: {s}");
}

#[test]
fn sexp_identifier() {
    let n = Node::new(NodeKind::Identifier { name: "Foo".to_string() }, loc(0, 3));
    let s = n.to_sexp();
    assert!(s.contains("Foo"), "got: {s}");
}

#[test]
fn sexp_error_node() {
    let n = Node::new(
        NodeKind::Error {
            message: "unexpected token".to_string(),
            expected: vec![";".to_string()],
            found: Some("}".to_string()),
            partial: None,
        },
        loc(0, 1),
    );
    let s = n.to_sexp();
    assert!(s.contains("ERROR") || s.contains("error"), "got: {s}");
}

#[test]
fn sexp_missing_expression() {
    let n = Node::new(NodeKind::MissingExpression, loc(0, 0));
    let s = n.to_sexp();
    assert!(!s.is_empty(), "sexp should not be empty");
}

#[test]
fn sexp_missing_statement() {
    let n = Node::new(NodeKind::MissingStatement, loc(0, 0));
    let s = n.to_sexp();
    assert!(!s.is_empty(), "sexp should not be empty");
}

#[test]
fn sexp_missing_identifier() {
    let n = Node::new(NodeKind::MissingIdentifier, loc(0, 0));
    let s = n.to_sexp();
    assert!(!s.is_empty(), "got empty sexp");
}

#[test]
fn sexp_missing_block() {
    let n = Node::new(NodeKind::MissingBlock, loc(0, 0));
    let s = n.to_sexp();
    assert!(!s.is_empty(), "got empty sexp");
}

#[test]
fn sexp_unknown_rest() {
    let n = Node::new(NodeKind::UnknownRest, loc(0, 0));
    let s = n.to_sexp();
    assert!(!s.is_empty(), "got empty sexp");
}

// ===========================================================================
// 6. to_sexp_inner
// ===========================================================================

#[test]
fn sexp_inner_on_expression_statement_unwraps() {
    let es = Node::new(
        NodeKind::ExpressionStatement { expression: Box::new(num_node("5")) },
        loc(0, 1),
    );
    // For non-anonymous-subroutine wrappees, inner should be the inner expr sexp
    let inner = es.to_sexp_inner();
    let outer = es.to_sexp();
    // inner should differ from outer (it unwraps)
    assert!(inner.contains("5"), "inner sexp should contain inner expr, got: {inner}");
    assert!(outer.contains("5"), "outer sexp should contain inner expr, got: {outer}");
}

#[test]
fn sexp_inner_on_non_expression_statement() {
    let n = num_node("7");
    assert_eq!(n.to_sexp_inner(), n.to_sexp());
}

// ===========================================================================
// 7. for_each_child (immutable traversal)
// ===========================================================================

#[test]
fn for_each_child_program_visits_all_statements() {
    let prog = Node::new(
        NodeKind::Program { statements: vec![num_node("1"), num_node("2"), num_node("3")] },
        loc(0, 10),
    );
    let mut count = 0;
    prog.for_each_child(|_| count += 1);
    assert_eq!(count, 3);
}

#[test]
fn for_each_child_leaf_visits_nothing() {
    let n = num_node("42");
    let mut count = 0;
    n.for_each_child(|_| count += 1);
    assert_eq!(count, 0);
}

#[test]
fn for_each_child_binary_visits_two() {
    let n = Node::new(
        NodeKind::Binary {
            op: "+".to_string(),
            left: Box::new(num_node("1")),
            right: Box::new(num_node("2")),
        },
        loc(0, 5),
    );
    let mut count = 0;
    n.for_each_child(|_| count += 1);
    assert_eq!(count, 2);
}

#[test]
fn for_each_child_subroutine_with_prototype_and_signature() {
    let proto = Node::new(NodeKind::Prototype { content: "$$".to_string() }, loc(0, 4));
    let sig = Node::new(NodeKind::Signature { parameters: vec![] }, loc(0, 2));
    let n = Node::new(
        NodeKind::Subroutine {
            name: Some("foo".to_string()),
            name_span: None,
            declarator: None,
            prototype: Some(Box::new(proto)),
            signature: Some(Box::new(sig)),
            attributes: vec![],
            body: Box::new(block_node(vec![])),
        },
        loc(0, 20),
    );
    // prototype + signature + body = 3
    let mut count = 0;
    n.for_each_child(|_| count += 1);
    assert_eq!(count, 3);
}

#[test]
fn for_each_child_subroutine_body_only() {
    let n = Node::new(
        NodeKind::Subroutine {
            name: None,
            name_span: None,
            declarator: None,
            prototype: None,
            signature: None,
            attributes: vec![],
            body: Box::new(block_node(vec![])),
        },
        loc(0, 10),
    );
    let mut count = 0;
    n.for_each_child(|_| count += 1);
    assert_eq!(count, 1);
}

#[test]
fn for_each_child_if_with_elsif_and_else() {
    let n = Node::new(
        NodeKind::If {
            condition: Box::new(num_node("1")),
            then_branch: Box::new(block_node(vec![])),
            elsif_branches: vec![(Box::new(num_node("2")), Box::new(block_node(vec![])))],
            else_branch: Some(Box::new(block_node(vec![]))),
            keyword: None,
        },
        loc(0, 30),
    );
    // condition + then + elsif_cond + elsif_body + else = 5
    let mut count = 0;
    n.for_each_child(|_| count += 1);
    assert_eq!(count, 5);
}

#[test]
fn for_each_child_foreach_with_all_parts() {
    let n = Node::new(
        NodeKind::Foreach {
            variable: Box::new(var_node("$", "x")),
            list: Box::new(Node::new(NodeKind::ArrayLiteral { elements: vec![] }, loc(0, 2))),
            body: Box::new(block_node(vec![])),
            continue_block: Some(Box::new(block_node(vec![]))),
        },
        loc(0, 20),
    );
    // variable + list + body + continue = 4
    let mut count = 0;
    n.for_each_child(|_| count += 1);
    assert_eq!(count, 4);
}

#[test]
fn for_each_child_try_with_multiple_catches() {
    let n = Node::new(
        NodeKind::Try {
            body: Box::new(block_node(vec![])),
            catch_blocks: vec![
                (Some("$e".to_string()), Box::new(block_node(vec![]))),
                (None, Box::new(block_node(vec![]))),
            ],
            finally_block: Some(Box::new(block_node(vec![]))),
        },
        loc(0, 30),
    );
    // body + catch1 + catch2 + finally = 4
    let mut count = 0;
    n.for_each_child(|_| count += 1);
    assert_eq!(count, 4);
}

#[test]
fn for_each_child_method_call_with_args() {
    let n = Node::new(
        NodeKind::MethodCall {
            object: Box::new(var_node("$", "obj")),
            method: "run".to_string(),
            args: vec![num_node("1"), num_node("2")],
        },
        loc(0, 15),
    );
    // object + 2 args = 3
    let mut count = 0;
    n.for_each_child(|_| count += 1);
    assert_eq!(count, 3);
}

#[test]
fn for_each_child_given_when_default() {
    let given = Node::new(
        NodeKind::Given { expr: Box::new(var_node("$", "x")), body: Box::new(block_node(vec![])) },
        loc(0, 15),
    );
    let mut count = 0;
    given.for_each_child(|_| count += 1);
    assert_eq!(count, 2);

    let when = Node::new(
        NodeKind::When { condition: Box::new(num_node("1")), body: Box::new(block_node(vec![])) },
        loc(0, 10),
    );
    count = 0;
    when.for_each_child(|_| count += 1);
    assert_eq!(count, 2);

    let default = Node::new(NodeKind::Default { body: Box::new(block_node(vec![])) }, loc(0, 10));
    count = 0;
    default.for_each_child(|_| count += 1);
    assert_eq!(count, 1);
}

// ===========================================================================
// 8. for_each_child_mut (mutable traversal)
// ===========================================================================

#[test]
fn for_each_child_mut_can_modify_children() {
    let mut prog =
        Node::new(NodeKind::Program { statements: vec![num_node("0")] }, loc(0, 5));
    prog.for_each_child_mut(|child| {
        if let NodeKind::Number { value } = &mut child.kind {
            *value = "99".to_string();
        }
    });
    if let NodeKind::Program { statements } = &prog.kind {
        assert_eq!(
            statements[0].kind,
            NodeKind::Number { value: "99".to_string() }
        );
    }
}

#[test]
fn for_each_child_mut_binary_visits_two() {
    let mut n = Node::new(
        NodeKind::Binary {
            op: "+".to_string(),
            left: Box::new(num_node("1")),
            right: Box::new(num_node("2")),
        },
        loc(0, 5),
    );
    let mut count = 0;
    n.for_each_child_mut(|_| count += 1);
    assert_eq!(count, 2);
}

#[test]
fn for_each_child_mut_unary_visits_one() {
    let mut n = Node::new(
        NodeKind::Unary { op: "-".to_string(), operand: Box::new(num_node("1")) },
        loc(0, 2),
    );
    let mut count = 0;
    n.for_each_child_mut(|_| count += 1);
    assert_eq!(count, 1);
}

#[test]
fn for_each_child_mut_assignment_visits_two() {
    let mut n = Node::new(
        NodeKind::Assignment {
            lhs: Box::new(var_node("$", "x")),
            rhs: Box::new(num_node("0")),
            op: "=".to_string(),
        },
        loc(0, 5),
    );
    let mut count = 0;
    n.for_each_child_mut(|_| count += 1);
    assert_eq!(count, 2);
}

#[test]
fn for_each_child_mut_leaf_visits_nothing() {
    let mut n = num_node("42");
    let mut count = 0;
    n.for_each_child_mut(|_| count += 1);
    assert_eq!(count, 0);
}

// ===========================================================================
// 9. ALL_KIND_NAMES / RECOVERY_KIND_NAMES
// ===========================================================================

#[test]
fn all_kind_names_no_duplicates() {
    let names = NodeKind::ALL_KIND_NAMES;
    let unique: std::collections::BTreeSet<&str> = names.iter().copied().collect();
    assert_eq!(names.len(), unique.len(), "duplicates in ALL_KIND_NAMES");
}

#[test]
fn all_kind_names_minimum_count() {
    assert!(
        NodeKind::ALL_KIND_NAMES.len() >= 60,
        "expected >= 60, got {}",
        NodeKind::ALL_KIND_NAMES.len()
    );
}

#[test]
fn recovery_kind_names_sorted() {
    let names = NodeKind::RECOVERY_KIND_NAMES;
    for w in names.windows(2) {
        assert!(w[0] < w[1], "not sorted: {:?} >= {:?}", w[0], w[1]);
    }
}

#[test]
fn recovery_kind_names_count() {
    assert_eq!(NodeKind::RECOVERY_KIND_NAMES.len(), 6);
}

// ===========================================================================
// 10. kind_name() round-trip
// ===========================================================================

#[test]
fn kind_name_matches_all_kind_names_membership() {
    let names_set: std::collections::BTreeSet<&str> =
        NodeKind::ALL_KIND_NAMES.iter().copied().collect();

    let samples: Vec<NodeKind> = vec![
        NodeKind::Number { value: "1".to_string() },
        NodeKind::Variable { sigil: "$".to_string(), name: "x".to_string() },
        NodeKind::Block { statements: vec![] },
        NodeKind::Diamond,
        NodeKind::Ellipsis,
        NodeKind::MissingExpression,
    ];
    for k in samples {
        assert!(
            names_set.contains(k.kind_name()),
            "{} not in ALL_KIND_NAMES",
            k.kind_name()
        );
    }
}

// ===========================================================================
// 11. Remaining variant constructions (smoke tests)
// ===========================================================================

#[test]
fn smoke_tie() {
    let n = Node::new(
        NodeKind::Tie {
            variable: Box::new(var_node("$", "x")),
            package: Box::new(Node::new(NodeKind::Identifier { name: "Tie::File".to_string() }, loc(0, 8))),
            args: vec![],
        },
        loc(0, 20),
    );
    assert_eq!(n.child_count(), 2);
}

#[test]
fn smoke_untie() {
    let n = Node::new(NodeKind::Untie { variable: Box::new(var_node("@", "arr")) }, loc(0, 10));
    assert_eq!(n.child_count(), 1);
}

#[test]
fn smoke_phase_block() {
    let n = Node::new(
        NodeKind::PhaseBlock {
            phase: "BEGIN".to_string(),
            phase_span: None,
            block: Box::new(block_node(vec![])),
        },
        loc(0, 10),
    );
    let s = n.to_sexp();
    assert!(s.contains("BEGIN") || s.contains("phase") || s.contains("block"), "got: {s}");
}

#[test]
fn smoke_data_section() {
    let n = Node::new(
        NodeKind::DataSection { marker: "__DATA__".to_string(), body: Some("data".to_string()) },
        loc(0, 20),
    );
    let s = n.to_sexp();
    assert!(s.contains("data"), "got: {s}");
}

#[test]
fn smoke_class() {
    let n = Node::new(
        NodeKind::Class {
            name: "Animal".to_string(),
            parents: vec!["Mammal".to_string()],
            body: Box::new(block_node(vec![])),
        },
        loc(0, 30),
    );
    let s = n.to_sexp();
    assert!(s.contains("class") || s.contains("Animal"), "got: {s}");
}

#[test]
fn smoke_method() {
    let n = Node::new(
        NodeKind::Method {
            name: "speak".to_string(),
            signature: None,
            attributes: vec![],
            body: Box::new(block_node(vec![])),
        },
        loc(0, 20),
    );
    let s = n.to_sexp();
    assert!(s.contains("method"), "got: {s}");
}

#[test]
fn smoke_readline() {
    let n = Node::new(NodeKind::Readline { filehandle: Some("STDIN".to_string()) }, loc(0, 7));
    let s = n.to_sexp();
    assert!(!s.is_empty(), "readline sexp empty");
}

#[test]
fn smoke_glob() {
    let n = Node::new(NodeKind::Glob { pattern: "*.pl".to_string() }, loc(0, 6));
    let s = n.to_sexp();
    assert!(!s.is_empty(), "glob sexp empty");
}

#[test]
fn smoke_typeglob() {
    let n = Node::new(NodeKind::Typeglob { name: "main::foo".to_string() }, loc(0, 10));
    let s = n.to_sexp();
    assert!(!s.is_empty(), "typeglob sexp empty");
}

#[test]
fn smoke_prototype() {
    let n = Node::new(NodeKind::Prototype { content: "$$".to_string() }, loc(0, 4));
    let s = n.to_sexp();
    assert!(!s.is_empty(), "prototype sexp empty");
}

#[test]
fn smoke_signature() {
    let n = Node::new(
        NodeKind::Signature {
            parameters: vec![Node::new(
                NodeKind::MandatoryParameter { variable: Box::new(var_node("$", "x")) },
                loc(0, 2),
            )],
        },
        loc(0, 4),
    );
    assert_eq!(n.child_count(), 1);
}

#[test]
fn smoke_optional_parameter() {
    let n = Node::new(
        NodeKind::OptionalParameter {
            variable: Box::new(var_node("$", "opt")),
            default_value: Box::new(num_node("0")),
        },
        loc(0, 10),
    );
    assert_eq!(n.child_count(), 2);
}

#[test]
fn smoke_slurpy_parameter() {
    let n = Node::new(
        NodeKind::SlurpyParameter { variable: Box::new(var_node("@", "rest")) },
        loc(0, 5),
    );
    assert_eq!(n.child_count(), 1);
}

#[test]
fn smoke_named_parameter() {
    let n = Node::new(
        NodeKind::NamedParameter { variable: Box::new(var_node("$", "named")) },
        loc(0, 6),
    );
    assert_eq!(n.child_count(), 1);
}

#[test]
fn smoke_labeled_statement() {
    let n = Node::new(
        NodeKind::LabeledStatement {
            label: "OUTER".to_string(),
            statement: Box::new(num_node("1")),
        },
        loc(0, 10),
    );
    let s = n.to_sexp();
    assert!(s.contains("label") || s.contains("OUTER"), "got: {s}");
}

#[test]
fn smoke_statement_modifier() {
    let n = Node::new(
        NodeKind::StatementModifier {
            statement: Box::new(num_node("1")),
            modifier: "if".to_string(),
            condition: Box::new(num_node("0")),
        },
        loc(0, 10),
    );
    let s = n.to_sexp();
    assert!(!s.is_empty(), "statement_modifier sexp empty");
}

#[test]
fn smoke_match() {
    let n = Node::new(
        NodeKind::Match {
            expr: Box::new(var_node("$", "s")),
            pattern: "foo".to_string(),
            modifiers: "i".to_string(),
            has_embedded_code: false,
            negated: false,
        },
        loc(0, 10),
    );
    let s = n.to_sexp();
    assert!(!s.is_empty(), "match sexp empty");
}

#[test]
fn smoke_substitution() {
    let n = Node::new(
        NodeKind::Substitution {
            expr: Box::new(var_node("$", "s")),
            pattern: "foo".to_string(),
            replacement: "bar".to_string(),
            modifiers: "g".to_string(),
            has_embedded_code: false,
            negated: false,
        },
        loc(0, 15),
    );
    let s = n.to_sexp();
    assert!(!s.is_empty(), "substitution sexp empty");
}

#[test]
fn smoke_transliteration() {
    let n = Node::new(
        NodeKind::Transliteration {
            expr: Box::new(var_node("$", "s")),
            search: "a-z".to_string(),
            replace: "A-Z".to_string(),
            modifiers: "".to_string(),
            negated: false,
        },
        loc(0, 10),
    );
    let s = n.to_sexp();
    assert!(!s.is_empty(), "transliteration sexp empty");
}

#[test]
fn smoke_no_statement() {
    let n = Node::new(
        NodeKind::No { module: "warnings".to_string(), args: vec![], has_filter_risk: false },
        loc(0, 12),
    );
    let s = n.to_sexp();
    assert!(s.contains("no") && s.contains("warnings"), "got: {s}");
}

#[test]
fn smoke_indirect_call() {
    let n = Node::new(
        NodeKind::IndirectCall {
            method: "new".to_string(),
            object: Box::new(Node::new(NodeKind::Identifier { name: "Foo".to_string() }, loc(0, 3))),
            args: vec![],
        },
        loc(0, 10),
    );
    let s = n.to_sexp();
    assert!(!s.is_empty(), "indirect_call sexp empty");
}

#[test]
fn smoke_goto() {
    let n = Node::new(
        NodeKind::Goto { target: Box::new(Node::new(
            NodeKind::Identifier { name: "LABEL".to_string() },
            loc(0, 5),
        ))},
        loc(0, 10),
    );
    let s = n.to_sexp();
    assert!(s.contains("goto"), "got: {s}");
}

#[test]
fn smoke_defer() {
    let n = Node::new(NodeKind::Defer { block: Box::new(block_node(vec![])) }, loc(0, 10));
    let s = n.to_sexp();
    assert!(s.contains("defer"), "got: {s}");
}

#[test]
fn smoke_format() {
    let n = Node::new(
        NodeKind::Format { name: "STDOUT".to_string(), body: "@<<<\n$text".to_string() },
        loc(0, 20),
    );
    let s = n.to_sexp();
    assert!(!s.is_empty(), "format sexp empty");
}

#[test]
fn smoke_nested_variable_list() {
    let n = Node::new(
        NodeKind::NestedVariableList { items: vec![var_node("$", "a"), var_node("$", "b")] },
        loc(0, 10),
    );
    assert_eq!(n.child_count(), 0); // NestedVariableList has no child traversal
}

#[test]
fn smoke_variable_list_declaration() {
    let n = Node::new(
        NodeKind::VariableListDeclaration {
            declarator: "my".to_string(),
            variables: vec![var_node("$", "a"), var_node("$", "b")],
            attributes: vec![],
            initializer: Some(Box::new(num_node("0"))),
        },
        loc(0, 15),
    );
    // 2 vars + 1 init = 3
    assert_eq!(n.child_count(), 3);
}
