#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
use perl_lexer::{PerlLexer, TokenType};
use perl_parser_core::tokens::token_stream::{
    ContextualFallbackReason, ContextualOpResult, ContextualTokenOp, TokenStream,
};
use perl_token::{Token, TokenKind};

fn collect_raw_lexer_tokens(source: &str) -> Vec<perl_lexer::Token> {
    let mut lexer = PerlLexer::new(source);
    let mut tokens = Vec::new();

    while let Some(token) = lexer.next_token() {
        if matches!(token.token_type, TokenType::EOF) {
            break;
        }

        tokens.push(token);
    }

    tokens
}

#[test]
fn bdd_given_raw_lexer_tokens_when_converted_then_trivia_is_filtered_and_eof_is_synthesized()
-> Result<(), Box<dyn std::error::Error>> {
    let raw = collect_raw_lexer_tokens("my $x = 1; # trailing comment\n");
    let parser_tokens = TokenStream::lexer_tokens_to_parser_tokens(raw);

    assert_eq!(
        parser_tokens.iter().map(|token| token.kind()).collect::<Vec<_>>(),
        vec![
            TokenKind::My,
            TokenKind::Identifier,
            TokenKind::Assign,
            TokenKind::Number,
            TokenKind::Semicolon,
        ]
    );

    let mut stream = TokenStream::from_vec(parser_tokens);
    let mut kinds = Vec::new();

    loop {
        let token = stream.next()?;
        kinds.push(token.kind());
        if token.kind() == TokenKind::Eof {
            break;
        }
    }

    assert_eq!(
        kinds,
        vec![
            TokenKind::My,
            TokenKind::Identifier,
            TokenKind::Assign,
            TokenKind::Number,
            TokenKind::Semicolon,
            TokenKind::Eof,
        ]
    );
    assert!(matches!(stream.peek(), Ok(token) if token.kind() == TokenKind::Eof));

    Ok(())
}

#[test]
fn bdd_given_buffered_tokens_when_peek_is_invalidated_then_cursor_advances_from_current_position()
-> Result<(), Box<dyn std::error::Error>> {
    let tokens = vec![
        Token::new_checked(TokenKind::My, "my", 0, 2).expect("valid token"),
        Token::new_checked(TokenKind::Identifier, "$x", 3, 5).expect("valid token"),
        Token::new_checked(TokenKind::Semicolon, ";", 5, 6).expect("valid token"),
    ];
    let mut stream = TokenStream::from_vec(tokens);

    assert_eq!(stream.peek()?.kind(), TokenKind::My);

    stream.invalidate_peek();
    assert_eq!(stream.peek()?.kind(), TokenKind::Identifier);

    // Issue #8128: a statement-boundary reset can change token classification,
    // and a buffered stream cannot re-classify. The request is refused with a
    // typed fallback requirement; the in-flight lookahead is preserved so the
    // caller can observe that nothing was applied, and the buffered kinds stay
    // position-bound and consumable.
    let result = stream.apply_contextual(ContextualTokenOp::StatementBoundaryReset);
    assert_eq!(
        result,
        ContextualOpResult::FallbackRequired { reason: ContextualFallbackReason::NoBufferedSource }
    );
    assert_eq!(stream.peek()?.kind(), TokenKind::Identifier);

    assert_eq!(stream.next()?.kind(), TokenKind::Identifier);
    assert_eq!(stream.next()?.kind(), TokenKind::Semicolon);
    assert_eq!(stream.next()?.kind(), TokenKind::Eof);

    Ok(())
}
