use perl_ast_v2::{MissingKind, Node, NodeIdGenerator, NodeKind};
use perl_position_tracking::{Position, Range};

fn range(start: usize, end: usize, start_column: u32, end_column: u32) -> Range {
    Range::new(Position::new(start, 1, start_column), Position::new(end, 1, end_column))
}

fn node(id_gen: &mut NodeIdGenerator, kind: NodeKind) -> Node {
    Node::new(id_gen.next_id(), kind, range(0, 0, 1, 1))
}

#[test]
fn nested_program_sexp_preserves_statement_order() -> Result<(), Box<dyn std::error::Error>> {
    let mut id_gen = NodeIdGenerator::new();
    let first = node(&mut id_gen, NodeKind::Number { value: "1".into() });
    let second = node(&mut id_gen, NodeKind::String { value: "two".into(), interpolated: false });
    let third = node(&mut id_gen, NodeKind::Variable { sigil: "$".into(), name: "three".into() });
    let program = node(&mut id_gen, NodeKind::Program { statements: vec![first, second, third] });

    assert_eq!(program.to_sexp(), r#"(source_file (number 1) (string "two") (variable $ three))"#);
    Ok(())
}

#[test]
fn binary_sexp_recurses_through_nested_operands() -> Result<(), Box<dyn std::error::Error>> {
    let mut id_gen = NodeIdGenerator::new();
    let left = node(&mut id_gen, NodeKind::Number { value: "1".into() });
    let inner_left = node(&mut id_gen, NodeKind::Number { value: "2".into() });
    let inner_right = node(&mut id_gen, NodeKind::Number { value: "3".into() });
    let right = node(
        &mut id_gen,
        NodeKind::Binary {
            op: "*".into(),
            left: Box::new(inner_left),
            right: Box::new(inner_right),
        },
    );
    let expr = node(
        &mut id_gen,
        NodeKind::Binary { op: "+".into(), left: Box::new(left), right: Box::new(right) },
    );

    assert_eq!(expr.to_sexp(), "(binary_+ (number 1) (binary_* (number 2) (number 3)))");
    Ok(())
}

#[test]
fn missing_kind_sexp_covers_all_specific_variants() -> Result<(), Box<dyn std::error::Error>> {
    let cases = [
        (MissingKind::Expression, "(MISSING Expression)"),
        (MissingKind::Statement, "(MISSING Statement)"),
        (MissingKind::Identifier, "(MISSING Identifier)"),
        (MissingKind::Block, "(MISSING Block)"),
        (MissingKind::ClosingDelimiter(')'), "(MISSING ClosingDelimiter(')'))"),
        (MissingKind::Semicolon, "(MISSING Semicolon)"),
        (MissingKind::Condition, "(MISSING Condition)"),
        (MissingKind::Argument, "(MISSING Argument)"),
        (MissingKind::Operator, "(MISSING Operator)"),
    ];

    for (kind, expected) in cases {
        assert_eq!(NodeKind::Missing(kind).to_sexp(), expected);
    }
    Ok(())
}

#[test]
fn error_sexp_uses_message_and_ignores_recovery_payload() -> Result<(), Box<dyn std::error::Error>>
{
    let mut id_gen = NodeIdGenerator::new();
    let partial = node(&mut id_gen, NodeKind::Identifier { name: "partial".into() });
    let error = NodeKind::Error {
        message: "Unexpected keyword".into(),
        expected: vec!["identifier".into(), "sub".into()],
        partial: Some(Box::new(partial)),
    };

    assert_eq!(error.to_sexp(), "(ERROR Unexpected keyword)");
    Ok(())
}

#[test]
fn node_new_preserves_id_kind_and_range() -> Result<(), Box<dyn std::error::Error>> {
    let source_range = range(3, 9, 4, 10);
    let node = Node::new(42, NodeKind::Identifier { name: "answer".into() }, source_range);

    assert_eq!(node.id, 42);
    assert_eq!(node.kind, NodeKind::Identifier { name: "answer".into() });
    assert_eq!(node.range, source_range);
    Ok(())
}

#[test]
fn id_generator_sequences_are_independent() -> Result<(), Box<dyn std::error::Error>> {
    let mut first = NodeIdGenerator::new();
    let mut second = NodeIdGenerator::new();

    assert_eq!(first.next_id(), 0);
    assert_eq!(first.next_id(), 1);
    assert_eq!(second.next_id(), 0);
    assert_eq!(first.next_id(), 2);
    assert_eq!(second.next_id(), 1);
    Ok(())
}

// Regression tests for issue #11734: to_sexp_depth was silently discarding the
// `attributes` field on VariableDeclaration and VariableListDeclaration via `..`.

#[test]
fn variable_declaration_attributes_appear_in_sexp() -> Result<(), Box<dyn std::error::Error>> {
    let mut id_gen = NodeIdGenerator::new();
    let var = node(&mut id_gen, NodeKind::Variable { sigil: "$".into(), name: "count".into() });
    let init = node(&mut id_gen, NodeKind::Number { value: "0".into() });

    // With initializer: attributes must appear between the variable and initializer.
    let decl = node(
        &mut id_gen,
        NodeKind::VariableDeclaration {
            declarator: "my".into(),
            variable: Box::new(var),
            attributes: vec!["shared".into()],
            initializer: Some(Box::new(init)),
        },
    );
    assert_eq!(
        decl.to_sexp(),
        "(variable_declaration my (variable $ count) (attrs shared) (number 0))"
    );
    Ok(())
}

#[test]
fn variable_declaration_attributes_appear_without_initializer()
-> Result<(), Box<dyn std::error::Error>> {
    let mut id_gen = NodeIdGenerator::new();
    let var = node(&mut id_gen, NodeKind::Variable { sigil: "$".into(), name: "x".into() });

    // Without initializer: attributes must still appear after the variable.
    let decl = node(
        &mut id_gen,
        NodeKind::VariableDeclaration {
            declarator: "state".into(),
            variable: Box::new(var),
            attributes: vec!["lvalue".into()],
            initializer: None,
        },
    );
    assert_eq!(decl.to_sexp(), "(variable_declaration state (variable $ x) (attrs lvalue))");
    Ok(())
}

#[test]
fn variable_declaration_multiple_attributes_all_appear_in_sexp()
-> Result<(), Box<dyn std::error::Error>> {
    let mut id_gen = NodeIdGenerator::new();
    let var = node(&mut id_gen, NodeKind::Variable { sigil: "$".into(), name: "y".into() });

    // Multiple attributes must all appear, space-separated inside (attrs ...).
    let decl = node(
        &mut id_gen,
        NodeKind::VariableDeclaration {
            declarator: "our".into(),
            variable: Box::new(var),
            attributes: vec!["shared".into(), "lvalue".into()],
            initializer: None,
        },
    );
    assert_eq!(decl.to_sexp(), "(variable_declaration our (variable $ y) (attrs shared lvalue))");
    Ok(())
}

#[test]
fn variable_declaration_empty_attributes_omits_attrs_group()
-> Result<(), Box<dyn std::error::Error>> {
    let mut id_gen = NodeIdGenerator::new();
    let var = node(&mut id_gen, NodeKind::Variable { sigil: "$".into(), name: "z".into() });

    // Empty attributes must not produce a spurious (attrs) group.
    let decl = node(
        &mut id_gen,
        NodeKind::VariableDeclaration {
            declarator: "my".into(),
            variable: Box::new(var),
            attributes: vec![],
            initializer: None,
        },
    );
    let sexp = decl.to_sexp();
    assert_eq!(sexp, "(variable_declaration my (variable $ z))");
    assert!(!sexp.contains("attrs"), "empty attributes must not emit an (attrs) group: {sexp}");
    Ok(())
}

#[test]
fn variable_list_declaration_attributes_appear_in_sexp() -> Result<(), Box<dyn std::error::Error>> {
    let mut id_gen = NodeIdGenerator::new();
    let var_a = node(&mut id_gen, NodeKind::Variable { sigil: "$".into(), name: "a".into() });
    let var_b = node(&mut id_gen, NodeKind::Variable { sigil: "@".into(), name: "rest".into() });
    let init = node(&mut id_gen, NodeKind::Identifier { name: "source".into() });

    // With initializer: attributes must sit between the variable list and initializer.
    let decl = node(
        &mut id_gen,
        NodeKind::VariableListDeclaration {
            declarator: "my".into(),
            variables: vec![var_a, var_b],
            attributes: vec!["shared".into()],
            initializer: Some(Box::new(init)),
        },
    );
    assert_eq!(
        decl.to_sexp(),
        "(variable_list_declaration my (variable $ a) (variable @ rest) (attrs shared) (identifier source))"
    );
    Ok(())
}

#[test]
fn variable_list_declaration_attributes_appear_without_initializer()
-> Result<(), Box<dyn std::error::Error>> {
    let mut id_gen = NodeIdGenerator::new();
    let var = node(&mut id_gen, NodeKind::Variable { sigil: "$".into(), name: "x".into() });

    let decl = node(
        &mut id_gen,
        NodeKind::VariableListDeclaration {
            declarator: "local".into(),
            variables: vec![var],
            attributes: vec!["lvalue".into()],
            initializer: None,
        },
    );
    assert_eq!(decl.to_sexp(), "(variable_list_declaration local (variable $ x) (attrs lvalue))");
    Ok(())
}

#[test]
fn variable_list_declaration_empty_attributes_omits_attrs_group()
-> Result<(), Box<dyn std::error::Error>> {
    let mut id_gen = NodeIdGenerator::new();
    let var = node(&mut id_gen, NodeKind::Variable { sigil: "$".into(), name: "p".into() });

    let decl = node(
        &mut id_gen,
        NodeKind::VariableListDeclaration {
            declarator: "my".into(),
            variables: vec![var],
            attributes: vec![],
            initializer: None,
        },
    );
    let sexp = decl.to_sexp();
    assert_eq!(sexp, "(variable_list_declaration my (variable $ p))");
    assert!(!sexp.contains("attrs"), "empty attributes must not emit an (attrs) group: {sexp}");
    Ok(())
}
