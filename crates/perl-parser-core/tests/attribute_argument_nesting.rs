use perl_parser_core::{NodeKind, Parser};

#[test]
fn dollar_before_closing_parenthesis_in_custom_attributes() -> Result<(), String> {
    for (source, expected) in
        [("sub f :A($) :B { 42 }", "A($)"), ("sub f :A(inner($)) :B { 42 }", "A(inner($))")]
    {
        let mut parser = Parser::new(source);
        let ast = parser.parse().map_err(|error| error.to_string())?;
        if !parser.get_errors().is_empty() {
            return Err(format!("{source}: {:?}", parser.get_errors()));
        }
        let NodeKind::Program { statements } = &ast.kind else {
            return Err(format!("expected program: {ast:?}"));
        };
        let Some(node) = statements.first() else { return Err("missing subroutine".into()) };
        let NodeKind::Subroutine { attributes, body, .. } = &node.kind else {
            return Err(format!("expected subroutine: {node:?}"));
        };
        assert_eq!(attributes, &[expected, "B"]);
        assert!(matches!(body.kind, NodeKind::Block { .. }));
    }
    Ok(())
}
