use perl_parser_core::{Node, NodeKind, Parser};

fn parse_program(source: &str) -> Result<Node, String> {
    let mut parser = Parser::new(source);
    parser.parse().map_err(|error| format!("fixture should parse: {error}"))
}

fn statements(ast: &Node) -> Result<&[Node], String> {
    match &ast.kind {
        NodeKind::Program { statements } => Ok(statements),
        kind => Err(format!("expected program, got {}", kind.kind_name())),
    }
}

fn initializer(statement: &Node) -> Result<&Node, String> {
    match &statement.kind {
        NodeKind::VariableDeclaration { initializer: Some(initializer), .. } => Ok(initializer),
        kind => Err(format!("expected declaration with initializer, got {}", kind.kind_name())),
    }
}

fn source_text<'a>(source: &'a str, start: usize, end: usize) -> Result<&'a str, String> {
    source
        .get(start..end)
        .ok_or_else(|| format!("invalid source span {start}..{end}"))
}

#[test]
fn declaration_span_contains_initializer() -> Result<(), String> {
    let source = "our @ISA = qw(Accuracy::Parent);";
    let ast = parse_program(source)?;
    let statement = statements(&ast)?
        .first()
        .ok_or_else(|| "expected one declaration".to_string())?;
    let initializer = initializer(statement)?;

    if statement.location.start != 0 {
        return Err(format!("declaration starts at {}", statement.location.start));
    }
    if statement.location.end != initializer.location.end {
        return Err(format!(
            "declaration ends at {}, initializer ends at {}",
            statement.location.end, initializer.location.end
        ));
    }
    let text = source_text(source, initializer.location.start, initializer.location.end)?;
    if text != "qw(Accuracy::Parent)" {
        return Err(format!("unexpected initializer text: {text:?}"));
    }
    Ok(())
}

#[test]
fn qw_elements_have_individual_source_spans() -> Result<(), String> {
    let source = "my @names = qw(foo bar);";
    let ast = parse_program(source)?;
    let statement = statements(&ast)?
        .first()
        .ok_or_else(|| "expected one declaration".to_string())?;
    let initializer = initializer(statement)?;
    let NodeKind::ArrayLiteral { elements } = &initializer.kind else {
        return Err(format!("expected qw array, got {}", initializer.kind.kind_name()));
    };
    let [first, second] = elements.as_slice() else {
        return Err(format!("expected two qw elements, got {}", elements.len()));
    };
    let first_text = source_text(source, first.location.start, first.location.end)?;
    let second_text = source_text(source, second.location.start, second.location.end)?;
    if first_text != "foo" || second_text != "bar" {
        return Err(format!("unexpected qw element text: {first_text:?}, {second_text:?}"));
    }
    if first.location.end > second.location.start {
        return Err(format!(
            "qw element spans overlap: {} > {}",
            first.location.end, second.location.start
        ));
    }
    Ok(())
}
