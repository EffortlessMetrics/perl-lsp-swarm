use perl_parser_core::{
    NodeKind, Parser,
    trivia::{Trivia, TriviaToken},
    trivia_parser::{TriviaParseOutput, TriviaPreservingParser, source_with_trivia},
};

#[test]
fn trivia_whitespace_variant() {
    let trivia = Trivia::Whitespace("  ".to_string());
    assert_eq!(trivia.as_str(), "  ");
    assert_eq!(trivia.kind_name(), "whitespace");
}

#[test]
fn trivia_comment_variant() {
    let trivia = Trivia::LineComment("# hello".to_string());
    assert_eq!(trivia.as_str(), "# hello");
    assert_eq!(trivia.kind_name(), "comment");
}

#[test]
fn trivia_newline_variant() {
    let trivia = Trivia::Newline;
    assert_eq!(trivia.as_str(), "\n");
    assert_eq!(trivia.kind_name(), "newline");
}

#[test]
fn trivia_token_construction() {
    let range = perl_position_tracking::Range::new(
        perl_position_tracking::Position::new(0, 1, 1),
        perl_position_tracking::Position::new(2, 1, 3),
    );
    let token = TriviaToken::new(Trivia::Whitespace("  ".to_string()), range);
    assert_eq!(token.trivia.as_str(), "  ");
}

#[test]
fn trivia_preserving_parser_returns_canonical_ast() {
    let source = "  # comment\nmy $x;".to_string();
    let result: TriviaParseOutput = TriviaPreservingParser::new(source.clone()).parse();
    let mut canonical = Parser::new(&source);
    let canonical_output = canonical.parse_with_recovery();

    assert!(matches!(&result.parse.ast.kind, NodeKind::Program { .. }));
    assert_eq!(result.parse.ast.to_sexp(), canonical_output.ast.to_sexp());
    assert_eq!(result.parse.diagnostics, canonical_output.diagnostics);
    assert!(
        result
            .trivia
            .iter()
            .any(|token| matches!(&token.trivia, Trivia::LineComment(text) if text == "# comment"))
    );
}

#[test]
fn source_projection_returns_exact_valid_perl() {
    let source = "  # comment\nmy $x;\n".to_string();
    let result = TriviaPreservingParser::new(source.clone()).parse();

    assert_eq!(source_with_trivia(&result), source);
    assert!(!source_with_trivia(&result).contains("Program {"));
}

#[test]
fn unknown_syntax_is_owned_by_canonical_recovery_not_silently_skipped() {
    let source = "if (".to_string();
    let result = TriviaPreservingParser::new(source.clone()).parse();
    let mut canonical = Parser::new(&source);
    let canonical_output = canonical.parse_with_recovery();

    assert_eq!(result.parse.ast.to_sexp(), canonical_output.ast.to_sexp());
    assert_eq!(result.parse.diagnostics, canonical_output.diagnostics);
    assert!(!result.parse.diagnostics.is_empty());
}
