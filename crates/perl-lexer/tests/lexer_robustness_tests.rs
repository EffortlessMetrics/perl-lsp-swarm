use perl_lexer::{PerlLexer, Token, TokenType};
use proptest::prelude::*;

fn recovery_token(input: &str, expected_start: usize) -> Option<Token> {
    PerlLexer::new(input)
        .collect_tokens()
        .into_iter()
        .find(|token| token.start == expected_start && token.token_type.is_recovery_token())
}

#[test]
fn malformed_hex_literal_reports_recovery_at_literal_start() {
    let token = recovery_token("0xG", 0);
    assert!(token.is_some(), "expected malformed hex recovery token");
    if let Some(token) = token {
        assert!(matches!(token.token_type, TokenType::Error(_) | TokenType::UnknownRest));
        assert_eq!(token.start, 0);
        assert_eq!(token.text.as_ref(), "0x");
    }
}

#[test]
fn malformed_binary_literal_reports_recovery_at_literal_start() {
    let token = recovery_token("0b2", 0);
    assert!(token.is_some(), "expected malformed binary recovery token");
    if let Some(token) = token {
        assert!(matches!(token.token_type, TokenType::Error(_) | TokenType::UnknownRest));
        assert_eq!(token.start, 0);
        assert_eq!(token.text.as_ref(), "0b");
    }
}

#[test]
fn unterminated_string_reports_recovery_at_quote_start() {
    let input = r#"my $x = "foo"#;
    let token = recovery_token(input, 8);
    assert!(token.is_some(), "expected unterminated string recovery token");
    if let Some(token) = token {
        assert!(matches!(token.token_type, TokenType::Error(_) | TokenType::UnknownRest));
        assert_eq!(token.start, 8);
        assert_eq!(token.text.as_ref(), &input[8..]);
    }
}

#[test]
fn unterminated_quote_operator_reports_recovery_at_operator_start() {
    let input = "my $x = q{foo";
    let token = recovery_token(input, 8);
    assert!(token.is_some(), "expected unterminated quote-operator recovery token");
    if let Some(token) = token {
        assert!(matches!(token.token_type, TokenType::Error(_) | TokenType::UnknownRest));
        assert_eq!(token.start, 8);
        assert_eq!(token.text.as_ref(), &input[8..]);
    }
}

#[test]
fn unterminated_heredoc_reports_recovery_at_body_start() {
    let input = "my $h = <<EOF;\nbody\n";
    let tokens = PerlLexer::new(input).collect_tokens();
    let token = tokens
        .iter()
        .find(|token| matches!(token.token_type, TokenType::Error(_) | TokenType::UnknownRest));
    assert!(token.is_some(), "expected heredoc recovery token");
    if let Some(token) = token {
        let body_start = input.find("body").unwrap_or(input.len());
        assert_eq!(token.start, body_start);
        assert_eq!(token.end, input.len());
        assert_eq!(token.text.as_ref(), &input[body_start..]);
    }

    for input in [
        "1.2.3.4.5",
        r#"my $x = "\z""#,
        r#"my $x = "\u{invalid}""#,
        "my $h = <<EOF;\n",
    ] {
        assert_terminates_with_valid_spans(input);
    }
}

fn assert_terminates_with_valid_spans(input: &str) {
    let mut lexer = PerlLexer::new(input);
    let max_tokens = input.len().saturating_mul(2).saturating_add(100);
    let mut tokens = Vec::new();
    let mut reached_eof = false;

    for _ in 0..max_tokens {
        let Some(token) = lexer.next_token() else {
            break;
        };
        reached_eof = token.token_type == TokenType::EOF;
        tokens.push(token);
        if reached_eof {
            break;
        }
    }

    assert!(reached_eof, "lexer did not terminate for {input:?}");
    let mut previous_end = 0;

    for token in &tokens {
        assert!(token.start <= token.end, "invalid token span: {token:?}");
        assert!(token.start >= previous_end, "overlapping token span: {token:?}");
        assert!(token.end <= input.len(), "token extends past input: {token:?}");
        assert_eq!(token.text.as_ref(), &input[token.start..token.end]);
        previous_end = token.end;
    }

    assert!(
        tokens.iter().any(|token| token.token_type == TokenType::EOF),
        "lexer did not emit EOF for {input:?}"
    );
}

proptest! {
    #[test]
    fn arbitrary_bytes_terminate(bytes in proptest::collection::vec(any::<u8>(), 0..256)) {
        // PerlLexer accepts UTF-8 source, so arbitrary byte fuzzing must cross
        // the same lossy boundary used by callers that receive invalid files.
        let input = String::from_utf8_lossy(&bytes);
        assert_terminates_with_valid_spans(&input);
    }
}
