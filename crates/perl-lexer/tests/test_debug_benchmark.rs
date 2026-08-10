use perl_lexer::{PerlLexer, TokenType};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn simple_statement_tokenizes_and_terminates() {
    let input = "my $x = 42; print $x;";
    let mut lexer = PerlLexer::new(input);

    let mut count = 0;
    while let Some(token) = lexer.next_token() {
        count += 1;

        // Check for EOF
        if token.token_type == TokenType::EOF {
            break;
        }

        // Safety check
        assert!(count <= 100, "Too many tokens - possible infinite loop");
    }

    // `my $x = 42 ; print $x ;` is 9 significant tokens; assert the lexer made
    // real progress rather than bailing out after one or two.
    assert!(count >= 9, "expected at least 9 tokens for {input:?}, got {count}");
}

#[test]
fn test_format_termination() -> TestResult {
    // Test case with terminating dot
    let input = "Some format content\n.\n";
    let mut lexer = PerlLexer::new(input);
    lexer.enter_format_mode();

    let token = lexer.next_token().ok_or("Expected token")?;

    // This input carries the documented `.` terminator, so it must lex as a
    // FormatBody — accepting an Error here would let a regression that rejects
    // every valid terminated format body keep this test green. Error is the
    // expected outcome only in the no-termination test below.
    let TokenType::FormatBody(content) = token.token_type else {
        return Err(format!(
            "terminated format body must lex as FormatBody, got {:?}",
            token.token_type
        )
        .into());
    };
    assert_eq!(
        content.as_ref(),
        "Some format content\n",
        "FormatBody must carry the body text without the terminator"
    );

    Ok(())
}

#[test]
fn test_format_no_termination() -> TestResult {
    // Test case without terminating dot
    let input = "Some format content\nno dot here";
    let mut lexer = PerlLexer::new(input);
    lexer.enter_format_mode();

    let token = lexer.next_token().ok_or("Expected token")?;
    assert!(
        matches!(token.token_type, TokenType::Error(_)),
        "Expected error token, got {:?}",
        token.token_type
    );

    if let TokenType::Error(msg) = token.token_type {
        assert_eq!(msg.as_ref(), "Unterminated format body");
    }

    Ok(())
}
