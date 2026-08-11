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
    let source = "my $value = 1 + 2 * 3;";
    let ast = parse_clean(source)?;
    let mut shapes = Vec::new();

    walk(&ast, &mut |node| {
        if let NodeKind::VariableDeclaration { initializer: Some(initializer), .. } = &node.kind
            && let NodeKind::Binary { op, left, right } = &initializer.kind
            && op == "+"
            && let NodeKind::Binary {
                op: nested_op,
                left: nested_left,
                right: nested_right,
            } = &right.kind
            && nested_op == "*"
        {
            shapes.push((
                source_text(source, initializer),
                source_text(source, left),
                source_text(source, nested_left),
                source_text(source, nested_right),
                matches!(&left.kind, NodeKind::Binary { op, .. } if op == "*"),
            ));
        }
    });

    assert_eq!(
        shapes,
        vec![(
            Some("1 + 2 * 3".to_string()),
            Some("1".to_string()),
            Some("2".to_string()),
            Some("3".to_string()),
            false,
        )],
        "1 + 2 * 3 must parse as the exact 1 + (2 * 3) tree"
    );
    Ok(())
}

#[test]
fn assignment_and_ternary_remain_right_associative() -> Result<(), String> {
    let source = "$a = $b = $c; $a ? $b : $c ? $d : $e;";
    let ast = parse_clean(source)?;
    let mut assignment_shape = None;
    let mut ternary_shape = None;

    walk(&ast, &mut |node| match &node.kind {
        NodeKind::Assignment { op, lhs, rhs } if op == "=" => {
            if let NodeKind::Assignment { op: nested_op, lhs: nested_lhs, rhs: nested_rhs } =
                &rhs.kind
                && nested_op == "="
            {
                assignment_shape = Some((
                    source_text(source, node),
                    source_text(source, lhs),
                    source_text(source, nested_lhs),
                    source_text(source, nested_rhs),
                    matches!(&lhs.kind, NodeKind::Assignment { .. }),
                ));
            }
        }
        NodeKind::Ternary { condition, then_expr, else_expr }
            if source_text(source, node).as_deref() == Some("$a ? $b : $c ? $d : $e") =>
        {
            if let NodeKind::Ternary {
                condition: nested_condition,
                then_expr: nested_then,
                else_expr: nested_else,
            } = &else_expr.kind
            {
                ternary_shape = Some((
                    source_text(source, condition),
                    source_text(source, then_expr),
                    source_text(source, else_expr),
                    source_text(source, nested_condition),
                    source_text(source, nested_then),
                    source_text(source, nested_else),
                    matches!(&condition.kind, NodeKind::Ternary { .. }),
                    matches!(&then_expr.kind, NodeKind::Ternary { .. }),
                ));
            }
        }
        _ => {}
    });

    let (assignment, outer_lhs, inner_lhs, inner_rhs, lhs_nested) = assignment_shape
        .ok_or_else(|| "assignment chain did not retain right nesting".to_string())?;
    assert_eq!(assignment.as_deref(), Some("$a = $b = $c"));
    assert_eq!(outer_lhs.as_deref(), Some("$a"));
    assert_eq!(inner_lhs.as_deref(), Some("$b"));
    assert_eq!(inner_rhs.as_deref(), Some("$c"));
    assert!(!lhs_nested, "assignment must not fabricate left nesting");

    let (
        outer_condition,
        outer_then,
        outer_else,
        inner_condition,
        inner_then,
        inner_else,
        condition_nested,
        then_nested,
    ) = ternary_shape.ok_or_else(|| "ternary chain did not retain right nesting".to_string())?;
    assert_eq!(outer_condition.as_deref(), Some("$a"));
    assert_eq!(outer_then.as_deref(), Some("$b"));
    assert_eq!(outer_else.as_deref(), Some("$c ? $d : $e"));
    assert_eq!(inner_condition.as_deref(), Some("$c"));
    assert_eq!(inner_then.as_deref(), Some("$d"));
    assert_eq!(inner_else.as_deref(), Some("$e"));
    assert!(!condition_nested, "ternary must not nest in the condition branch");
    assert!(!then_nested, "ternary must not nest in the then branch");
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
