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
    source.get(node.location.start..node.location.end).map(str::to_owned)
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
            && let NodeKind::Binary { op: nested_op, left: nested_left, right: nested_right } =
                &right.kind
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
fn assignment_and_ternary_remain_exactly_right_associative() -> Result<(), String> {
    let source = "$a = $b = $c; $a ? $b : $c ? $d : $e;";
    let ast = parse_clean(source)?;
    let mut assignment_shapes = Vec::new();
    let mut ternary_shapes = Vec::new();

    walk(&ast, &mut |node| match &node.kind {
        NodeKind::Assignment { op, lhs, rhs }
            if op == "=" && source_text(source, node).as_deref() == Some("$a = $b = $c") =>
        {
            if let NodeKind::Assignment { op: nested_op, lhs: nested_lhs, rhs: nested_rhs } =
                &rhs.kind
                && nested_op == "="
            {
                assignment_shapes.push((
                    source_text(source, node),
                    source_text(source, lhs),
                    source_text(source, rhs),
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
                ternary_shapes.push((
                    source_text(source, node),
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

    assert_eq!(
        assignment_shapes,
        vec![(
            Some("$a = $b = $c".to_string()),
            Some("$a".to_string()),
            Some("$b = $c".to_string()),
            Some("$b".to_string()),
            Some("$c".to_string()),
            false,
        )],
        "assignment must be represented by one exact $a = ($b = $c) tree"
    );
    assert_eq!(
        ternary_shapes,
        vec![(
            Some("$a ? $b : $c ? $d : $e".to_string()),
            Some("$a".to_string()),
            Some("$b".to_string()),
            Some("$c ? $d : $e".to_string()),
            Some("$c".to_string()),
            Some("$d".to_string()),
            Some("$e".to_string()),
            false,
            false,
        )],
        "ternary must be represented by one exact $a ? $b : ($c ? $d : $e) tree"
    );
    Ok(())
}

#[test]
fn exponentiation_remains_exactly_right_associative() -> Result<(), String> {
    let source = "my $value = 2 ** 3 ** 2;";
    let ast = parse_clean(source)?;
    let mut shapes = Vec::new();

    walk(&ast, &mut |node| {
        if let NodeKind::VariableDeclaration { initializer: Some(initializer), .. } = &node.kind
            && let NodeKind::Binary { op, left, right } = &initializer.kind
            && op == "**"
            && let NodeKind::Binary { op: nested_op, left: nested_left, right: nested_right } =
                &right.kind
            && nested_op == "**"
        {
            shapes.push((
                source_text(source, initializer),
                source_text(source, left),
                source_text(source, right),
                source_text(source, nested_left),
                source_text(source, nested_right),
                matches!(&left.kind, NodeKind::Binary { op, .. } if op == "**"),
                matches!(&nested_left.kind, NodeKind::Number { value } if value == "3"),
                matches!(&nested_right.kind, NodeKind::Number { value } if value == "2"),
            ));
        }
    });

    assert_eq!(
        shapes,
        vec![(
            Some("2 ** 3 ** 2".to_string()),
            Some("2".to_string()),
            Some("3 ** 2".to_string()),
            Some("3".to_string()),
            Some("2".to_string()),
            false,
            true,
            true,
        )],
        "2 ** 3 ** 2 must parse as one exact 2 ** (3 ** 2) tree"
    );
    Ok(())
}

#[test]
fn unary_minus_binds_outside_exponentiation() -> Result<(), String> {
    let source = "my $value = -2 ** 2;";
    let ast = parse_clean(source)?;
    let mut shapes = Vec::new();

    walk(&ast, &mut |node| {
        if let NodeKind::VariableDeclaration { initializer: Some(initializer), .. } = &node.kind
            && let NodeKind::Unary { op, operand } = &initializer.kind
            && op == "-"
            && let NodeKind::Binary { op: power, left, right } = &operand.kind
            && power == "**"
        {
            shapes.push((
                source_text(source, initializer),
                source_text(source, left),
                source_text(source, right),
            ));
        }
    });

    assert_eq!(
        shapes,
        vec![(Some("-2 ** 2".to_string()), Some("2".to_string()), Some("2".to_string()))],
        "unary minus must remain outside the exponentiation tree"
    );
    Ok(())
}

#[test]
fn symbolic_unary_forms_bind_outside_exponentiation() -> Result<(), String> {
    for op in ["!", "~", "\\", "+", "-"] {
        let source = format!("my $value = {op}2 ** 2;");
        let ast = parse_clean(&source)?;
        let mut shapes = Vec::new();

        walk(&ast, &mut |node| {
            if let NodeKind::VariableDeclaration { initializer: Some(initializer), .. } = &node.kind
                && let NodeKind::Unary { op: unary_op, operand } = &initializer.kind
                && unary_op == op
                && let NodeKind::Binary { op: power, left, right } = &operand.kind
                && power == "**"
            {
                shapes.push((
                    source_text(&source, initializer),
                    source_text(&source, left),
                    source_text(&source, right),
                ));
            }
        });

        assert_eq!(
            shapes,
            vec![(
                Some(format!("{op}2 ** 2")),
                Some("2".to_string()),
                Some("2".to_string()),
            )],
            "{op} must wrap the complete exponentiation tree"
        );
    }

    let source = "my $value = 2 ** !0;";
    let ast = parse_clean(source)?;
    let mut shapes = Vec::new();
    walk(&ast, &mut |node| {
        if let NodeKind::VariableDeclaration { initializer: Some(initializer), .. } = &node.kind
            && let NodeKind::Binary { op, left, right } = &initializer.kind
            && op == "**"
            && let NodeKind::Unary { op: unary_op, operand } = &right.kind
            && unary_op == "!"
        {
            shapes.push((
                source_text(source, initializer),
                source_text(source, left),
                source_text(source, operand),
            ));
        }
    });
    assert_eq!(
        shapes,
        vec![(
            Some("2 ** !0".to_string()),
            Some("2".to_string()),
            Some("0".to_string()),
        )],
        "the right-hand symbolic unary must remain inside the exponentiation tree"
    );
    Ok(())
}

#[test]
fn slash_and_brace_ambiguities_keep_owned_distinct_shapes() -> Result<(), String> {
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
    let mut map_calls = Vec::new();

    walk(&ast, &mut |node| match &node.kind {
        NodeKind::Binary { op, left, right } if op == "/" => divisions.push((
            source_text(source, node),
            source_text(source, left),
            source_text(source, right),
            matches!(
                &left.kind,
                NodeKind::Variable { sigil, name } if sigil == "$" && name == "value"
            ),
            matches!(&right.kind, NodeKind::Number { value } if value == "2"),
        )),
        NodeKind::Match { expr, pattern, modifiers, has_embedded_code, negated } => matches.push((
            source_text(source, node),
            source_text(source, expr),
            pattern.clone(),
            modifiers.clone(),
            *has_embedded_code,
            *negated,
            matches!(
                &expr.kind,
                NodeKind::Variable { sigil, name } if sigil == "$" && name == "value"
            ),
        )),
        NodeKind::HashLiteral { .. } => {
            if let Some(text) = source_text(source, node) {
                hash_literals.push(text);
            }
        }
        NodeKind::FunctionCall { name, args } if name == "map" => map_calls.push((
            source_text(source, node),
            args.iter().filter_map(|arg| source_text(source, arg)).collect::<Vec<_>>(),
            args.len() == 2 && matches!(&args[0].kind, NodeKind::Block { .. }),
            args.len() == 2
                && matches!(
                    &args[1].kind,
                    NodeKind::Variable { sigil, name } if sigil == "@" && name == "items"
                ),
        )),
        _ => {}
    });

    assert_eq!(
        divisions,
        vec![(
            Some("$value / 2".to_string()),
            Some("$value".to_string()),
            Some("2".to_string()),
            true,
            true,
        )]
    );
    assert_eq!(
        matches,
        vec![(
            Some("$value =~ /pattern/".to_string()),
            Some("$value".to_string()),
            "/pattern/".to_string(),
            String::new(),
            false,
            false,
            true,
        )]
    );
    assert_eq!(hash_literals, vec!["key => 1"]);
    assert_eq!(
        map_calls,
        vec![(
            Some("map { $_ + 1 } @items".to_string()),
            vec!["{ $_ + 1 }".to_string(), "@items".to_string()],
            true,
            true,
        )],
        "map must own the block as its first argument and @items as its second"
    );
    Ok(())
}
