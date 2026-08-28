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

    for input in ["1.2.3.4.5", r#"my $x = "\z""#, r#"my $x = "\u{invalid}""#, "my $h = <<EOF;\n"] {
        assert_terminates_with_valid_spans(input);
    }
}

/// Bare `/` in ExpectTerm is a regex opener. Current-main `parse_regex` scanned to
/// EOF and returned `None`, so `next_token` ended the stream without the `EOF`
/// token its contract requires (#12504).
#[test]
fn unterminated_expect_term_slash_regex_emits_recovery_then_eof() {
    // (source, byte offset of the regex `/`)
    // `"/\0"` is the issue's observed shape. `"\0\0/"` is the shrink from
    // `arbitrary_bytes_terminate` on origin/main @ 826c66225.
    let cases = [("/", 0usize), ("/\0", 0), ("/[", 0), ("/\\", 0), ("\0/", 1), ("\0\0/", 2)];

    for (input, slash_at) in cases {
        let tokens = PerlLexer::new(input).collect_tokens();
        assert_terminates_with_valid_spans(input);
        assert_eq!(
            tokens.last().map(|token| &token.token_type),
            Some(&TokenType::EOF),
            "collect_tokens must end with EOF for {input:?}; tokens={tokens:?}"
        );

        let recovery = tokens
            .iter()
            .find(|token| token.start == slash_at && token.token_type.is_recovery_token());
        assert!(
            recovery.is_some(),
            "expected recovery covering unterminated `/' for {input:?}; tokens={tokens:?}"
        );
        if let Some(recovery) = recovery {
            assert_eq!(
                recovery.end,
                input.len(),
                "recovery must consume through EOF for {input:?}"
            );
            assert_eq!(recovery.text.as_ref(), &input[slash_at..]);
        }
        assert!(
            !tokens.iter().any(|token| {
                token.start == slash_at
                    && matches!(token.token_type, TokenType::Division | TokenType::RegexMatch)
            }),
            "unterminated ExpectTerm `/` must not be division or a closed regex for {input:?}; tokens={tokens:?}"
        );
    }
}

#[test]
fn lossy_invalid_utf8_after_slash_emits_recovery_then_eof() {
    let input = String::from_utf8_lossy(&[b'/', 0xFF, 0x00]);
    assert_terminates_with_valid_spans(&input);
    let tokens = PerlLexer::new(&input).collect_tokens();
    let recovery = tokens.iter().find(|token| token.token_type.is_recovery_token());
    assert!(recovery.is_some(), "expected recovery for {input:?}; tokens={tokens:?}");
    if let Some(recovery) = recovery {
        assert_eq!(recovery.start, 0);
        assert_eq!(recovery.end, input.len());
        assert_eq!(tokens.last().map(|token| &token.token_type), Some(&TokenType::EOF));
    }
}

/// Opposite-direction: a closed `/…/` that contains NUL is a regex, not recovery.
#[test]
fn closed_slash_regex_with_nul_is_regex_match_then_eof() {
    let input = "/\0/";
    assert_terminates_with_valid_spans(input);
    let tokens = PerlLexer::new(input).collect_tokens();
    assert!(
        matches!(tokens.first().map(|token| &token.token_type), Some(TokenType::RegexMatch)),
        "closed `/\\0/` must stay RegexMatch; tokens={tokens:?}"
    );
    assert_eq!(tokens[0].text.as_ref(), input);
    assert_eq!(tokens.last().map(|token| &token.token_type), Some(&TokenType::EOF));
}

/// Opposite-direction: after a term, `/` is division even if a NUL follows.
#[test]
fn division_after_number_with_nul_still_emits_eof() {
    let input = "1/\0";
    assert_terminates_with_valid_spans(input);
    let tokens = PerlLexer::new(input).collect_tokens();
    assert!(matches!(tokens.first().map(|token| &token.token_type), Some(TokenType::Number(_))));
    assert_eq!(
        tokens.get(1).map(|token| &token.token_type),
        Some(&TokenType::Division),
        "ExpectOperator `/` must remain division; tokens={tokens:?}"
    );
    assert_eq!(tokens.last().map(|token| &token.token_type), Some(&TokenType::EOF));
}

/// Parser-stack fixture: a broken `/…` must not swallow the next line (#12504).
/// Same line-bounded recovery as unterminated strings (#5090).
#[test]
fn unterminated_regex_is_line_bounded_so_followup_statement_lexes() {
    let input = "if ($text =~ /abc) { print 1; }\nmy $ok = 1;";
    let slash_at = input.find('/');
    assert!(slash_at.is_some(), "fixture contains `/`");
    let newline_at = input.find('\n');
    assert!(newline_at.is_some(), "fixture contains a newline");

    if let (Some(slash_at), Some(newline_at)) = (slash_at, newline_at) {
        assert_terminates_with_valid_spans(input);
        let tokens = PerlLexer::new(input).collect_tokens();
        let recovery = tokens
            .iter()
            .find(|token| token.start == slash_at && token.token_type.is_recovery_token());
        assert!(recovery.is_some(), "expected unterminated-regex recovery; tokens={tokens:?}");
        if let Some(recovery) = recovery {
            assert_eq!(recovery.end, newline_at);
            assert_eq!(recovery.text.as_ref(), &input[slash_at..newline_at]);
            assert!(
                tokens
                    .iter()
                    .any(|token| token.start > newline_at && token.text.as_ref().contains("ok")),
                "follow-up `my $ok` must still be tokenized; tokens={tokens:?}"
            );
            assert_eq!(tokens.last().map(|token| &token.token_type), Some(&TokenType::EOF));
        }
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
