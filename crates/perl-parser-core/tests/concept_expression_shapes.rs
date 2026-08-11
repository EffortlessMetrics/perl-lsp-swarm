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

fn source_text(source: &str, node: &Node) -> Option<String> {
    source
        .get(node.location.start..node.location.end)
        .map(str::to_owned)
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
    let mut left_nested_assignment = false;
    let mut right_nested_ternary = false;
    let mut condition_nested_ternary = false;

    walk(&ast, &mut |node| match &node.kind {
        NodeKind::Assignment { op, lhs, rhs }
            if op == "=" && matches!(&rhs.kind, NodeKind::Assignment { op, .. } if op == "=") =>
        {
            right_nested_assignment = true;
            left_nested_assignment = matches!(&lhs.kind, NodeKind::Assignment { .. });
        }
        NodeKind::Ternary { condition, else_expr, .. }
            if matches!(&else_expr.kind, NodeKind::Ternary { .. }) =>
        {
            right_nested_ternary = true;
            condition_nested_ternary = matches!(&condition.kind, NodeKind::Ternary { .. });
        }
        _ => {}
    });

    assert!(right_nested_assignment, "assignment must nest on the right");
    assert!(!left_nested_assignment, "assignment must not fabricate left nesting");
    assert!(right_nested_ternary, "chained ternary must nest in the else branch");
    assert!(!condition_nested_ternary, "chained ternary must not nest in the condition branch");
    Ok(())
}

#[test]
fn exponentiation_remains_right_associative() -> Result<(), String> {
    let ast = parse_clean("my $value = 2 ** 3 ** 2;")?;
    let mut right_nested_power = false;
    let mut left_nested_power = false;

    walk(&ast, &mut |node| {
        if let NodeKind::Binary { op, left, right } = &node.kind
            && op == "**"
            && matches!(&right.kind, NodeKind::Binary { op, .. } if op == "**")
        {
            right_nested_power = true;
            left_nested_power = matches!(&left.kind, NodeKind::Binary { op, .. } if op == "**");
        }
    });

    assert!(right_nested_power, "2 ** 3 ** 2 must nest on the right");
    assert!(!left_nested_power, "power must not fabricate left nesting");
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
    let mut divisions = Vec::new();
    let mut matches = Vec::new();
    let mut hash_literals = Vec::new();
    let mut map_blocks = Vec::new();

    walk(&ast, &mut |node| match &node.kind {
        NodeKind::Binary { op, .. } if op == "/" => {
            if let Some(text) = source_text(source, node) {
                divisions.push(text);
            }
        }
        NodeKind::Match {
            expr,
            pattern,
            modifiers,
            has_embedded_code,
            negated,
        } => {
            assert!(matches!(
                &expr.kind,
                NodeKind::Variable { sigil, name } if sigil == "$" && name == "value"
            ));
            assert_eq!(pattern, "pattern");
            assert!(modifiers.is_empty());
            assert!(!has_embedded_code);
            assert!(!negated);
            if let Some(text) = source_text(source, node) {
                matches.push(text);
            }
        }
        NodeKind::HashLiteral { .. } => {
            if let Some(text) = source_text(source, node) {
                hash_literals.push(text);
            }
        }
        NodeKind::FunctionCall { name, args }
            if name == "map" && args.iter().any(|arg| matches!(&arg.kind, NodeKind::Block { .. })) =>
        {
            if let Some(text) = source_text(source, node) {
                map_blocks.push(text);
            }
        }
        _ => {}
    });

    assert_eq!(divisions, vec!["$value / 2"]);
    assert_eq!(matches, vec!["$value =~ /pattern/"]);
    assert_eq!(hash_literals, vec!["{ key => 1 }"]);
    assert_eq!(map_blocks, vec!["map { $_ + 1 } @items"]);
    Ok(())
}
