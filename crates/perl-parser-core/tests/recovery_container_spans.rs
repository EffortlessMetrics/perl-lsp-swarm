use perl_parser_core::{Node, NodeKind, Parser};

fn check_statements(node: &Node, source: &str, expected_kind: &str) -> Result<(), String> {
    let statements = match &node.kind {
        NodeKind::Program { statements } | NodeKind::Block { statements } => statements,
        other => return Err(format!("expected a statement container, got {}", other.kind_name())),
    };
    if statements.is_empty() {
        return Err(format!("fixture has no statements: {source:?}"));
    }
    if !statements.iter().any(|statement| statement.kind.kind_name() == expected_kind) {
        return Err(format!("missing {expected_kind} statement in {source:?}"));
    }
    for statement in statements {
        if node.location.start > statement.location.start
            || statement.location.end > node.location.end
        {
            return Err(format!(
                "{source:?}: {} {:?} excludes {} {:?}",
                node.kind.kind_name(),
                node.location,
                statement.kind.kind_name(),
                statement.location
            ));
        }
    }
    Ok(())
}

#[test]
fn program_contains_recovered_statements() -> Result<(), String> {
    for (source, kind) in [
        ("my $v = {a => ;", "Error"),
        ("my $v = q{a ;", "VariableDeclaration"),
        ("my $v = 1;", "VariableDeclaration"),
    ] {
        let ast = Parser::new(source).parse().map_err(|e| e.to_string())?;
        check_statements(&ast, source, kind)?;
        if ast.location.end != source.len() {
            return Err(format!("unexpected program end for {source:?}: {:?}", ast.location));
        }
    }
    Ok(())
}

#[test]
fn block_contains_recovered_statements() -> Result<(), String> {
    for (source, kind) in [
        ("sub f { s/a/ }", "Error"),
        ("sub f { q{a }", "ExpressionStatement"),
        ("sub f { 1; }", "ExpressionStatement"),
    ] {
        let ast = Parser::new(source).parse().map_err(|e| e.to_string())?;
        let mut pending = vec![&ast];
        let mut blocks = 0;
        while let Some(node) = pending.pop() {
            if matches!(node.kind, NodeKind::Block { .. }) {
                check_statements(node, source, kind)?;
                blocks += 1;
            }
            pending.extend(node.children());
        }
        if blocks != 1 {
            return Err(format!("expected one block in {source:?}, found {blocks}"));
        }
    }
    Ok(())
}
