use perl_parser_core::Parser;
use perl_tdd_support::must;

#[test]
fn bareword_filehandles_and_glob_patterns_are_distinct() {
    for (source, expected) in [
        ("<foo>;", "(readline (filehandle foo))"),
        ("<foo_bar1>;", "(readline (filehandle foo_bar1))"),
        ("<FOO1>;", "(readline (filehandle FOO1))"),
        ("<Foo::bar>;", "(readline (filehandle Foo::bar))"),
        ("<STDIN>;", "(readline (filehandle STDIN))"),
        ("<foo.txt>;", "(glob (pattern foo.txt))"),
        ("<foo-bar>;", "(glob (pattern foo-bar))"),
    ] {
        let ast = must(Parser::new(source).parse());
        let sexp = ast.to_sexp();
        assert!(sexp.contains(expected), "{source}: expected {expected}, got {sexp}");
    }
}

#[test]
fn whitespace_in_angle_pattern_does_not_create_a_filehandle() {
    let ast = must(Parser::new("<foo bar>;").parse());
    let sexp = ast.to_sexp();
    assert!(sexp.contains("(glob"), "expected a glob, got {sexp}");
    assert!(!sexp.contains("(readline"), "unexpected readline: {sexp}");
}
