use perl_lexer::{PerlLexer, Token, TokenType};

fn recovery_token(input: &str, expected_start: usize) -> Token {
    let tokens = PerlLexer::new(input).collect_tokens();
    match tokens
        .iter()
        .find(|token| token.start == expected_start && token.token_type.is_recovery_token())
        .cloned()
    {
        Some(token) => token,
        None => panic!(
            "expected a recovery token at byte {expected_start} for {input:?}; tokens={tokens:?}"
        ),
    }
}

#[test]
fn malformed_hex_literal_reports_recovery_at_literal_start() {
    let token = recovery_token("0xG", 0);
    assert!(matches!(token.token_type, TokenType::Error(_) | TokenType::UnknownRest));
    assert_eq!(token.start, 0);
    assert_eq!(token.text.as_ref(), "0xG");
}

#[test]
fn malformed_binary_literal_reports_recovery_at_literal_start() {
    let token = recovery_token("0b2", 0);
    assert!(matches!(token.token_type, TokenType::Error(_) | TokenType::UnknownRest));
    assert_eq!(token.start, 0);
    assert_eq!(token.text.as_ref(), "0b2");
}

#[test]
fn unterminated_string_reports_recovery_at_quote_start() {
    let input = r#"my $x = "foo"#;
    let token = recovery_token(input, 8);
    assert!(matches!(token.token_type, TokenType::Error(_) | TokenType::UnknownRest));
    assert_eq!(token.start, 8);
    assert_eq!(token.text.as_ref(), &input[8..]);
}

#[test]
fn unterminated_quote_operator_reports_recovery_at_operator_start() {
    let input = "my $x = q{foo";
    let token = recovery_token(input, 8);
    assert!(matches!(token.token_type, TokenType::Error(_) | TokenType::UnknownRest));
    assert_eq!(token.start, 8);
    assert_eq!(token.text.as_ref(), &input[8..]);
}

#[test]
fn unterminated_heredoc_reports_recovery_at_body_start() {
    let input = "my $h = <<EOF;\nbody\n";
    let tokens = PerlLexer::new(input).collect_tokens();
    let token = tokens
        .iter()
        .find(|token| matches!(token.token_type, TokenType::Error(_) | TokenType::UnknownRest))
        .unwrap_or_else(|| panic!("expected heredoc recovery token for {input:?}; tokens={tokens:?}"));
    assert!(token.start >= input.find("body").expect("body marker"));
    assert!(token.end <= input.len());
    assert_eq!(token.text.as_ref(), &input[token.start..token.end]);
}
