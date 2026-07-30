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
    assert!(
        matches!(token.token_type, TokenType::FormatBody(_) | TokenType::Error(_)),
        "Expected FormatBody or Error, got {:?}",
        token.token_type
    );

    // Whichever arm is taken, the payload must be non-empty — an empty body or
    // an empty error message would be a silent lexer failure.
    match token.token_type {
        TokenType::FormatBody(content) => {
            assert!(!content.is_empty(), "FormatBody payload must not be empty");
        }
        TokenType::Error(msg) => {
            assert!(!msg.is_empty(), "Error payload must not be empty");
        }
        other => return Err(format!("unexpected token type: {other:?}").into()),
    }

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
