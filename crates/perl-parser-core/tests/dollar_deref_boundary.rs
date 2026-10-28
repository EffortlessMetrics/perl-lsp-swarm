use perl_parser_core::{NodeKind, Parser};
use perl_tdd_support::must;

#[test]
fn nested_array_and_hash_dereferences_keep_their_target() {
    for (source, sigil) in [("%$$slice;", "%"), ("@$$slice;", "@")] {
        let mut parser = Parser::new(source);
        let ast = must(parser.parse());
        let NodeKind::Program { statements } = &ast.kind else {
            panic!("expected program for {source}");
        };
        assert_eq!(statements.len(), 1, "{source}: {}", ast.to_sexp());
        assert!(
            ast.to_sexp().contains(&format!("(variable (sigil {sigil}) (name $$slice))")),
            "{source}: {}",
            ast.to_sexp()
        );
    }
}
