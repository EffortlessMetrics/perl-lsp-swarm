use perl_ast_v2::{MissingKind, Node, NodeIdGenerator, NodeKind};
use perl_position_tracking::{Position, Range};

fn zero_range() -> Range {
    Range::new(Position::new(0, 1, 1), Position::new(0, 1, 1))
}

fn make_node(id_gen: &mut NodeIdGenerator, kind: NodeKind) -> Node {
    Node::new(id_gen.next_id(), kind, zero_range())
}

// ---- NodeIdGenerator -------------------------------------------------------

#[test]
fn test_id_generator_sequential() {
    let mut id_gen = NodeIdGenerator::new();
    assert_eq!(id_gen.next_id(), 0);
    assert_eq!(id_gen.next_id(), 1);
    assert_eq!(id_gen.next_id(), 2);
}

#[test]
fn test_id_generator_default() {
    let mut id_gen = NodeIdGenerator::default();
    assert_eq!(id_gen.next_id(), 0);
}

// ---- Node construction & sexp ----------------------------------------------

#[test]
fn test_variable_sexp() {
    let mut id_gen = NodeIdGenerator::new();
    let node = make_node(&mut id_gen, NodeKind::Variable { sigil: "$".into(), name: "x".into() });
    assert_eq!(node.to_sexp(), "(variable $ x)");
}

#[test]
fn test_number_sexp() {
    let mut id_gen = NodeIdGenerator::new();
    let node = make_node(&mut id_gen, NodeKind::Number { value: "3.14".into() });
    assert_eq!(node.to_sexp(), "(number 3.14)");
}

#[test]
fn test_string_non_interpolated_sexp() {
    let mut id_gen = NodeIdGenerator::new();
    let node =
        make_node(&mut id_gen, NodeKind::String { value: "hello".into(), interpolated: false });
    assert_eq!(node.to_sexp(), r#"(string "hello")"#);
}

#[test]
fn test_string_interpolated_sexp() {
    let mut id_gen = NodeIdGenerator::new();
    let node = make_node(&mut id_gen, NodeKind::String { value: "hi".into(), interpolated: true });
    assert_eq!(node.to_sexp(), r#"(string_interpolated "hi")"#);
}

#[test]
fn test_error_ref_sexp() {
    let mut id_gen = NodeIdGenerator::new();
    let node = make_node(&mut id_gen, NodeKind::ErrorRef { diag_id: 7 });
    assert_eq!(node.to_sexp(), "(ERROR_REF #7)");
}

#[test]
fn test_missing_expression_sexp() {
    let mut id_gen = NodeIdGenerator::new();
    let node = make_node(&mut id_gen, NodeKind::MissingExpression);
    assert_eq!(node.to_sexp(), "(MISSING_EXPRESSION)");
}

#[test]
fn test_missing_statement_sexp() {
    let mut id_gen = NodeIdGenerator::new();
    let node = make_node(&mut id_gen, NodeKind::MissingStatement);
    assert_eq!(node.to_sexp(), "(MISSING_STATEMENT)");
}

#[test]
fn test_missing_identifier_sexp() {
    let mut id_gen = NodeIdGenerator::new();
    let node = make_node(&mut id_gen, NodeKind::MissingIdentifier);
    assert_eq!(node.to_sexp(), "(MISSING_IDENTIFIER)");
}

#[test]
fn test_missing_block_sexp() {
    let mut id_gen = NodeIdGenerator::new();
    let node = make_node(&mut id_gen, NodeKind::MissingBlock);
    assert_eq!(node.to_sexp(), "(MISSING_BLOCK)");
}

#[test]
fn test_missing_kind_variant_sexp() {
    let mut id_gen = NodeIdGenerator::new();
    let node = make_node(&mut id_gen, NodeKind::Missing(MissingKind::Semicolon));
    let sexp = node.to_sexp();
    assert!(sexp.contains("MISSING"), "expected MISSING in {sexp}");
    assert!(sexp.contains("Semicolon"), "expected Semicolon variant in {sexp}");
}

#[test]
fn test_program_sexp_empty() {
    let mut id_gen = NodeIdGenerator::new();
    let node = make_node(&mut id_gen, NodeKind::Program { statements: vec![] });
    assert_eq!(node.to_sexp(), "(source_file )");
}

#[test]
fn test_program_with_child_sexp() {
    let mut id_gen = NodeIdGenerator::new();
    let child = make_node(&mut id_gen, NodeKind::Number { value: "1".into() });
    let node = make_node(&mut id_gen, NodeKind::Program { statements: vec![child] });
    assert_eq!(node.to_sexp(), "(source_file (number 1))");
}

#[test]
fn test_block_sexp() {
    let mut id_gen = NodeIdGenerator::new();
    let inner = make_node(&mut id_gen, NodeKind::Number { value: "0".into() });
    let node = make_node(&mut id_gen, NodeKind::Block { statements: vec![inner] });
    assert_eq!(node.to_sexp(), "(block (number 0))");
}

#[test]
fn test_block_sexp_empty() {
    let mut id_gen = NodeIdGenerator::new();
    let node = make_node(&mut id_gen, NodeKind::Block { statements: vec![] });
    assert_eq!(node.to_sexp(), "(block )");
}

#[test]
fn test_binary_sexp() {
    let mut id_gen = NodeIdGenerator::new();
    let left = make_node(&mut id_gen, NodeKind::Number { value: "1".into() });
    let right = make_node(&mut id_gen, NodeKind::Number { value: "2".into() });
    let node = make_node(
        &mut id_gen,
        NodeKind::Binary { op: "+".into(), left: Box::new(left), right: Box::new(right) },
    );
    assert_eq!(node.to_sexp(), "(binary_+ (number 1) (number 2))");
}

#[test]
fn test_node_equality() {
    let mut id_gen = NodeIdGenerator::new();
    let r = zero_range();
    let a = Node::new(id_gen.next_id(), NodeKind::Number { value: "5".into() }, r);
    let b = Node::new(id_gen.next_id(), NodeKind::Number { value: "5".into() }, r);
    // Same kind/range but different ids — kinds are equal even if ids differ
    assert_eq!(a.kind, b.kind);
    assert_ne!(a.id, b.id);
    // Full node equality considers id — nodes with different ids are not equal
    assert_ne!(a, b);
}

// ---- No variant falls back to Debug rendering -----------------------------
//
// This section used to test a wildcard arm, `_ => format!("({:?})", self)`,
// that produced the Debug representation wrapped in parens. That arm is gone:
// `to_sexp_depth` now has an explicit arm for every `NodeKind` variant, each
// rendering a lowercase tree-sitter-style form.
//
// The tests that exercised the wildcard are replaced by the ratchet below.
// Their assertions required the Debug variant name (`sexp.contains("If")` and
// similar), which contradicted the in-crate assertion in `src/lib.rs` that
// `to_sexp` output must not contain Debug struct syntax. Nothing caught the
// contradiction because no gate ran this file (#11694, #11708).
//
// Per-variant exact forms are covered individually above. What none of those
// cover, and what this section now owns, is the cross-variant guarantee that
// no variant regresses to Debug rendering.

#[test]
fn no_node_kind_variant_renders_debug_syntax() {
    let mut id_gen = NodeIdGenerator::new();
    let variable =
        make_node(&mut id_gen, NodeKind::Variable { sigil: "$".into(), name: "x".into() });
    let number = make_node(&mut id_gen, NodeKind::Number { value: "1".into() });
    let block = make_node(&mut id_gen, NodeKind::Block { statements: vec![] });
    let ident = make_node(&mut id_gen, NodeKind::Identifier { name: "foo".into() });

    // One representative node per `NodeKind` variant, in declaration order.
    let cases: Vec<(&str, NodeKind)> = vec![
        ("Program", NodeKind::Program { statements: vec![] }),
        ("Block", NodeKind::Block { statements: vec![] }),
        (
            "VariableDeclaration",
            NodeKind::VariableDeclaration {
                declarator: "my".into(),
                variable: Box::new(variable.clone()),
                attributes: vec!["shared".into()],
                initializer: Some(Box::new(number.clone())),
            },
        ),
        (
            "VariableListDeclaration",
            NodeKind::VariableListDeclaration {
                declarator: "my".into(),
                variables: vec![variable.clone()],
                attributes: vec!["shared".into()],
                initializer: None,
            },
        ),
        ("Variable", NodeKind::Variable { sigil: "$".into(), name: "x".into() }),
        (
            "Error",
            NodeKind::Error {
                message: "Unexpected token".into(),
                expected: vec!["identifier".into()],
                partial: Some(Box::new(ident.clone())),
            },
        ),
        ("ErrorRef", NodeKind::ErrorRef { diag_id: 7 }),
        ("MissingExpression", NodeKind::MissingExpression),
        ("MissingStatement", NodeKind::MissingStatement),
        ("MissingIdentifier", NodeKind::MissingIdentifier),
        ("MissingBlock", NodeKind::MissingBlock),
        ("Missing", NodeKind::Missing(MissingKind::Semicolon)),
        (
            "Binary",
            NodeKind::Binary {
                op: "+".into(),
                left: Box::new(number.clone()),
                right: Box::new(number.clone()),
            },
        ),
        ("Unary", NodeKind::Unary { op: "-".into(), operand: Box::new(number.clone()) }),
        (
            "If",
            NodeKind::If {
                condition: Box::new(variable.clone()),
                then_branch: Box::new(block.clone()),
                elsif_branches: vec![],
                else_branch: None,
            },
        ),
        ("Number", NodeKind::Number { value: "1".into() }),
        ("String", NodeKind::String { value: "s".into(), interpolated: false }),
        ("Identifier", NodeKind::Identifier { name: "foo".into() }),
    ];

    for (label, kind) in cases {
        let sexp = make_node(&mut id_gen, kind).to_sexp();
        assert!(sexp.starts_with('('), "{label}: sexp must start with '(', got: {sexp}");
        assert!(sexp.ends_with(')'), "{label}: sexp must end with ')', got: {sexp}");
        assert!(
            !sexp.contains('{') && !sexp.contains('}'),
            "{label}: sexp must not contain Debug struct syntax, got: {sexp}"
        );
    }
}

#[test]
fn test_all_missing_kind_variants_have_distinct_debug_sexp() {
    let cases = [
        (MissingKind::Expression, "Expression"),
        (MissingKind::Statement, "Statement"),
        (MissingKind::Identifier, "Identifier"),
        (MissingKind::Block, "Block"),
        (MissingKind::ClosingDelimiter('}'), "ClosingDelimiter('}')"),
        (MissingKind::Semicolon, "Semicolon"),
        (MissingKind::Condition, "Condition"),
        (MissingKind::Argument, "Argument"),
        (MissingKind::Operator, "Operator"),
    ];

    let mut id_gen = NodeIdGenerator::new();
    for (kind, expected_debug) in cases {
        let node = make_node(&mut id_gen, NodeKind::Missing(kind));
        let sexp = node.to_sexp();
        assert_eq!(sexp, format!("(MISSING {expected_debug})"));
    }
}

#[test]
fn test_error_sexp_ignores_recovery_metadata_but_equality_keeps_it() {
    let mut id_gen = NodeIdGenerator::new();
    let partial = make_node(&mut id_gen, NodeKind::Identifier { name: "partial".into() });
    let with_partial = make_node(
        &mut id_gen,
        NodeKind::Error {
            message: "Unexpected token".into(),
            expected: vec!["identifier".into(), "term".into()],
            partial: Some(Box::new(partial)),
        },
    );
    let without_partial = make_node(
        &mut id_gen,
        NodeKind::Error {
            message: "Unexpected token".into(),
            expected: vec!["identifier".into()],
            partial: None,
        },
    );

    assert_eq!(with_partial.to_sexp(), "(ERROR Unexpected token)");
    assert_eq!(without_partial.to_sexp(), "(ERROR Unexpected token)");
    assert_ne!(with_partial.kind, without_partial.kind);
}

#[test]
fn variable_declaration_sexp_includes_declarator_and_initializer() {
    let mut id_gen = NodeIdGenerator::new();
    let var =
        make_node(&mut id_gen, NodeKind::Variable { sigil: "$".into(), name: "count".into() });
    let initializer = make_node(&mut id_gen, NodeKind::Number { value: "1".into() });
    let node = make_node(
        &mut id_gen,
        NodeKind::VariableDeclaration {
            declarator: "state".into(),
            variable: Box::new(var),
            attributes: vec!["shared".into()],
            initializer: Some(Box::new(initializer)),
        },
    );

    assert_eq!(node.to_sexp(), "(variable_declaration state (variable $ count) (number 1))");

    // NOTE: `attributes` is deliberately absent above, and that is what the
    // code does today -- the `VariableDeclaration` and `VariableListDeclaration`
    // arms of `to_sexp_depth` both destructure with `..`, discarding it. This
    // test previously asserted `sexp.contains("shared")` and could not have
    // passed; it never ran, so the omission was never surfaced. Whether the
    // s-expression should carry attributes is an open question recorded in
    // #11708, not something this test decides.
}

#[test]
fn if_sexp_includes_elsif_and_else_branches() {
    let mut id_gen = NodeIdGenerator::new();
    let condition =
        make_node(&mut id_gen, NodeKind::Variable { sigil: "$".into(), name: "a".into() });
    let then_branch = make_node(&mut id_gen, NodeKind::Block { statements: vec![] });
    let elsif_condition =
        make_node(&mut id_gen, NodeKind::Variable { sigil: "$".into(), name: "b".into() });
    let elsif_branch = make_node(&mut id_gen, NodeKind::Block { statements: vec![] });
    let else_branch = make_node(&mut id_gen, NodeKind::Block { statements: vec![] });
    let node = make_node(
        &mut id_gen,
        NodeKind::If {
            condition: Box::new(condition),
            then_branch: Box::new(then_branch),
            elsif_branches: vec![(elsif_condition, elsif_branch)],
            else_branch: Some(Box::new(else_branch)),
        },
    );

    // Both optional branches are rendered, and the elsif condition survives
    // rather than being elided with the branch.
    assert_eq!(
        node.to_sexp(),
        "(if (variable $ a) (block ) (elsif (variable $ b) (block )) (else (block )))"
    );
}
