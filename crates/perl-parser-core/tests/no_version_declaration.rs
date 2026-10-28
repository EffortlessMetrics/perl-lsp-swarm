use perl_parser_core::{NodeKind, Parser};
use perl_tdd_support::must;

#[test]
fn no_version_declarations_preserve_the_literal() {
    for version in ["5.010", "5.10.1", "v5.10.1"] {
        let source = format!("no {version};\nmy $x = 1;");
        let mut parser = Parser::new(&source);
        let ast = must(parser.parse());
        assert!(parser.get_errors().is_empty(), "{source}: {:?}", parser.get_errors());
        let NodeKind::Program { statements } = &ast.kind else { panic!("expected program") };
        assert!(
            matches!(statements.first().map(|node| &node.kind),
            Some(NodeKind::No { module, args, .. }) if module == version && args.is_empty()),
            "{source}: {statements:?}"
        );
        assert_eq!(statements.len(), 2, "{source}: {statements:?}");
    }
}
