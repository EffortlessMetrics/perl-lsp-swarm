use perl_parser::Parser;

#[test]
fn parser_facade_documentation_example_compiles_against_public_api() {
    let mut parser = Parser::new("my $answer = 42;");
    let ast = parser.parse().expect("the documented parser example should parse");
    assert!(!ast.to_sexp().is_empty());
}
