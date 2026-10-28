//! Caller-level RIPR seam proofs for shared quote delimiter scanning.

use perl_lexer::{PerlLexer, TokenType};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn next_non_trivia(lexer: &mut PerlLexer<'_>) -> Option<perl_lexer::Token> {
    loop {
        let token = lexer.next_token()?;
        if !matches!(token.token_type, TokenType::Whitespace | TokenType::Newline) {
            return Some(token);
        }
    }
}

/// Observe the shared scanner through an ordinary quote-like production path.
#[test]
fn q_operator_observes_nested_and_escaped_delimiters() -> TestResult {
    let input = r"q(foo (nested) \) tail)";
    let mut lexer = PerlLexer::new(input);
    let token = next_non_trivia(&mut lexer).ok_or("expected q token")?;

    assert_eq!(token.token_type, TokenType::QuoteSingle);
    assert_eq!(token.text.as_ref(), input);
    assert_eq!(token.start, 0);
    assert_eq!(token.end, input.len());
    Ok(())
}

/// Observe the recovery callback through the lexer/parser-facing `qw` path.
#[test]
fn unclosed_qw_observes_recovery_before_following_statement() -> TestResult {
    let input = "qw(word\nmy $x = 1;\n";
    let mut lexer = PerlLexer::new(input);
    let recovered = next_non_trivia(&mut lexer).ok_or("expected recovered qw token")?;

    assert!(matches!(
        &recovered.token_type,
        TokenType::Error(message) if message.contains("unclosed qw")
    ));
    assert_eq!(recovered.text.as_ref(), "qw(word\n");

    let following = next_non_trivia(&mut lexer).ok_or("expected following statement")?;
    assert_eq!(following.text.as_ref(), "my");
    Ok(())
}

#[test]
fn qq_interpolation_preserves_outer_delimiter_nesting() -> TestResult {
    use perl_lexer::LexerConfig;

    for input in [r#"qq[${\("[")}]];"#, r#"qq[${\("\[")}];"#] {
        for enabled in [false, true] {
            let config = LexerConfig { parse_interpolation: enabled, ..LexerConfig::default() };
            let mut lexer = PerlLexer::with_config(input, config);
            let token = next_non_trivia(&mut lexer).ok_or("expected qq token")?;
            assert!(matches!(token.token_type, TokenType::QuoteDouble(_)), "{input}: {token:?}");
            assert_eq!(
                token.text.as_ref(),
                &input[..input.len() - 1],
                "{input}, interpolation={enabled}"
            );
            assert_eq!(next_non_trivia(&mut lexer).ok_or("expected semicolon")?.text.as_ref(), ";");
        }
    }
    Ok(())
}

#[test]
fn qq_unclosed_nested_delimiter_is_an_error_in_both_modes() -> TestResult {
    use perl_lexer::LexerConfig;

    let input = r#"qq[${\("[")}];"#;
    for enabled in [false, true] {
        let config = LexerConfig { parse_interpolation: enabled, ..LexerConfig::default() };
        let mut lexer = PerlLexer::with_config(input, config);
        let token = next_non_trivia(&mut lexer).ok_or("expected error token")?;
        assert!(matches!(token.token_type, TokenType::Error(_)), "{token:?}");
        assert_eq!(token.text.as_ref(), input);
    }
    Ok(())
}
