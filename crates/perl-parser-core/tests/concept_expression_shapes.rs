//! Concept-level parser proofs for expression structure (#6682).
//!
//! These tests pin parser shape and disambiguation only. Runtime value context,
//! lvalue validity, overload, and evaluation remain downstream concerns.

use perl_parser_core::{Node, NodeKind, Parser};

fn parse_clean(source: &str) -> Result<Node, String> {
    let mut parser = Parser::new(source);
    let ast = parser.parse().map_err(|error| format!("parse failed: {error:?}"))?;
    if parser.errors().is_empty() {
        Ok(ast)
    } else {
        Err(format!("expected a clean parse, got diagnostics: {:?}", parser.errors()))
    }
}

fn walk(node: &Node, visit: &mut impl FnMut(&Node)) {
    visit(node);
    for child in node.children() {
        walk(child, visit);
    }
}

#[test]
fn multiplication_binds_inside_addition() -> Result<(), String> {
    let ast = parse_clean("my $value = 1 + 2 * 3;")?;
    let mut proven = false;

    walk(&ast, &mut |node| {
        if let NodeKind::VariableDeclaration { initializer: Some(initializer), .. } = &node.kind
            && let NodeKind::Binary { op, right, .. } = &initializer.kind
            && op == "+"
            && matches!(&right.kind, NodeKind::Binary { op, .. } if op == "*")
        {
            proven = true;
        }
    });

    assert!(proven, "1 + 2 * 3 must parse as addition whose right child is multiplication");
    Ok(())
}

#[test]
fn assignment_and_ternary_remain_right_associative() -> Result<(), String> {
    let ast = parse_clean("$a = $b = $c; $a ? $b : $c ? $d : $e;")?;
    let mut right_nested_assignment = false;
    let mut right_nested_ternary = false;

    walk(&ast, &mut |node| match &node.kind {
        NodeKind::Assignment { op, rhs, .. }
            if op == "=" && matches!(&rhs.kind, NodeKind::Assignment { op, .. } if op == "=") =>
        {
            right_nested_assignment = true;
        }
        NodeKind::Ternary { else_expr, .. }
            if matches!(&else_expr.kind, NodeKind::Ternary { .. }) =>
        {
            right_nested_ternary = true;
        }
        _ => {}
    });

    assert!(right_nested_assignment, "assignment must nest on the right");
    assert!(right_nested_ternary, "chained ternary must nest in the else branch");
    Ok(())
}

#[test]
fn slash_and_brace_ambiguities_keep_distinct_shapes() -> Result<(), String> {
    let source = concat!(
        "$value / 2;\n",
        "$value =~ /pattern/;\n",
        "my $mapping = { key => 1 };\n",
        "map { $_ + 1 } @items;\n",
    );
    let ast = parse_clean(source)?;
    let mut division = 0usize;
    let mut matches = 0usize;
    let mut hash_literals = 0usize;
    let mut map_blocks = 0usize;

    walk(&ast, &mut |node| match &node.kind {
        NodeKind::Binary { op, .. } if op == "/" => division += 1,
        NodeKind::Match { .. } => matches += 1,
        NodeKind::HashLiteral { .. } => hash_literals += 1,
        NodeKind::FunctionCall { name, args }
            if name == "map" && args.iter().any(|arg| matches!(&arg.kind, NodeKind::Block { .. })) =>
        {
            map_blocks += 1;
        }
        _ => {}
    });

    assert_eq!(division, 1, "division must remain a Binary '/' expression");
    assert_eq!(matches, 1, "=~ /.../ must remain a Match expression");
    assert_eq!(hash_literals, 1, "expression braces must retain HashLiteral identity");
    assert_eq!(map_blocks, 1, "map braces must retain block-argument identity");
    Ok(())
}
