//! Focused coverage for public lexer state transitions that are easy to
//! regress when changing the tokenization hot path.

use perl_lexer::{LexerConfig, LexerMode, PerlLexer, StringPart, TokenType};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn next_token(lexer: &mut PerlLexer<'_>, label: &str) -> Result<perl_lexer::Token, String> {
    lexer.next_token().ok_or_else(|| format!("missing token: {label}"))
}

#[test]
fn config_disables_double_quote_interpolation_parts() -> TestResult {
    let config =
        LexerConfig { parse_interpolation: false, track_positions: true, max_lookahead: 8 };
    let mut lexer = PerlLexer::with_config(r#""hello $name ${expr}""#, config);

    let token = next_token(&mut lexer, "double quoted string")?;

    assert!(
        matches!(
            &token.token_type,
            TokenType::InterpolatedString(parts)
                if parts == &vec![StringPart::Literal("hello $name ${expr}".into())]
        ),
        "interpolation-disabled strings should keep dollar forms literal, got {:?}",
        token.token_type
    );
    Ok(())
}

#[test]
fn config_zero_lookahead_stops_package_qualifier_scan() -> TestResult {
    let no_lookahead =
        LexerConfig { parse_interpolation: true, track_positions: true, max_lookahead: 0 };
    let mut lexer = PerlLexer::with_config("Foo::Bar", no_lookahead);

    let first = next_token(&mut lexer, "first identifier")?;

    assert!(
        matches!(&first.token_type, TokenType::Identifier(name) if name.as_ref() == "Foo"),
        "max_lookahead=0 should leave ::Bar for later tokens, got {:?}",
        first.token_type
    );
    assert_eq!(first.text.as_ref(), "Foo");
    Ok(())
}

#[test]
fn peek_preserves_pending_heredoc_body_state() -> TestResult {
    let mut lexer = PerlLexer::with_body_tokens("<<EOF\nbody\nEOF\nprint 1;\n");
    let start = next_token(&mut lexer, "heredoc start")?;
    assert!(matches!(start.token_type, TokenType::HeredocStart));

    let peeked_body = lexer.peek_token().ok_or("missing heredoc body from peek")?;
    let actual_body = next_token(&mut lexer, "heredoc body after peek")?;

    assert!(matches!(peeked_body.token_type, TokenType::HeredocBody(_)));
    assert!(matches!(actual_body.token_type, TokenType::HeredocBody(_)));
    assert_eq!(peeked_body.start, actual_body.start);
    assert_eq!(peeked_body.end, actual_body.end);

    let after_body = next_token(&mut lexer, "token after heredoc body")?;
    assert!(
        matches!(&after_body.token_type, TokenType::Keyword(word) if word.as_ref() == "print"),
        "peek must not drop the token following the heredoc, got {:?}",
        after_body.token_type
    );
    Ok(())
}

#[test]
fn reset_replays_data_section_after_body_consumption() -> TestResult {
    let mut lexer = PerlLexer::new("__DATA__\npayload\n");
    let marker = next_token(&mut lexer, "data marker")?;
    let body = next_token(&mut lexer, "data body")?;

    assert!(matches!(marker.token_type, TokenType::DataMarker(_)));
    assert!(matches!(body.token_type, TokenType::DataBody(_)));

    lexer.reset();

    let replayed_marker = next_token(&mut lexer, "replayed data marker")?;
    let replayed_body = next_token(&mut lexer, "replayed data body")?;

    assert_eq!(marker.text.as_ref(), replayed_marker.text.as_ref());
    assert_eq!(body.text.as_ref(), replayed_body.text.as_ref());
    assert!(matches!(replayed_marker.token_type, TokenType::DataMarker(_)));
    assert!(matches!(replayed_body.token_type, TokenType::DataBody(_)));
    Ok(())
}

#[test]
fn peek_after_consumed_eof_returns_none_until_reset() -> TestResult {
    let mut lexer = PerlLexer::new("1");
    let number = next_token(&mut lexer, "number")?;
    let eof = next_token(&mut lexer, "eof")?;

    assert!(matches!(number.token_type, TokenType::Number(_)));
    assert!(matches!(eof.token_type, TokenType::EOF));
    assert!(lexer.peek_token().is_none(), "peek after consumed EOF should stay exhausted");

    lexer.reset();
    let replayed = next_token(&mut lexer, "number after reset")?;
    assert!(matches!(replayed.token_type, TokenType::Number(_)));
    Ok(())
}

#[test]
fn explicit_expect_delimiter_mode_treats_hash_as_operator() -> TestResult {
    let mut lexer = PerlLexer::new("#not_a_comment");
    lexer.set_mode(LexerMode::ExpectDelimiter);

    let hash = next_token(&mut lexer, "hash delimiter")?;
    let ident = next_token(&mut lexer, "identifier after hash delimiter")?;

    assert!(matches!(&hash.token_type, TokenType::Operator(op) if op.as_ref() == "#"));
    assert!(
        matches!(&ident.token_type, TokenType::Identifier(name) if name.as_ref() == "not_a_comment")
    );
    Ok(())
}

#[test]
fn format_mode_accepts_empty_body_before_dot_terminator() -> TestResult {
    let mut lexer = PerlLexer::new(".\nprint 1;\n");
    lexer.enter_format_mode();

    let format_body = next_token(&mut lexer, "empty format body")?;
    let next = next_token(&mut lexer, "token after format terminator")?;

    assert!(
        matches!(&format_body.token_type, TokenType::FormatBody(body) if body.is_empty()),
        "dot at format-body start should emit an empty body, got {:?}",
        format_body.token_type
    );
    assert!(matches!(&next.token_type, TokenType::Keyword(word) if word.as_ref() == "print"));
    Ok(())
}
