use perl_parser_core::{Node, NodeKind, Parser};

fn parse_program(source: &str) -> Node {
    let mut parser = Parser::new(source);
    parser.parse().expect("fixture should parse")
}

#[test]
fn declaration_span_contains_initializer() {
    let source = "our @ISA = qw(Accuracy::Parent);";
    let ast = parse_program(source);
    let NodeKind::Program { statements } = ast.kind else { panic!("expected program") };
    let NodeKind::VariableDeclaration { initializer: Some(initializer), .. } = &statements[0].kind else {
        panic!("expected declaration: {}", statements[0].to_sexp());
    };
    assert_eq!(statements[0].location.start, 0);
    assert_eq!(statements[0].location.end, initializer.location.end);
    assert_eq!(&source[initializer.location.start..initializer.location.end], "qw(Accuracy::Parent)");
}

#[test]
fn qw_elements_have_individual_source_spans() {
    let source = "my @names = qw(foo bar);";
    let ast = parse_program(source);
    let NodeKind::Program { statements } = ast.kind else { panic!("expected program") };
    let NodeKind::VariableDeclaration { initializer: Some(initializer), .. } = &statements[0].kind else {
        panic!("expected declaration: {}", statements[0].to_sexp());
    };
    let NodeKind::ArrayLiteral { elements } = &initializer.kind else {
        panic!("expected qw array: {}", initializer.to_sexp());
    };
    assert_eq!(elements.len(), 2);
    assert_eq!(&source[elements[0].location.start..elements[0].location.end], "foo");
    assert_eq!(&source[elements[1].location.start..elements[1].location.end], "bar");
    assert!(elements[0].location.end <= elements[1].location.start);
}
