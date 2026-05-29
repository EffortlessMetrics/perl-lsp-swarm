use std::error::Error;

use perl_lexer::{LexerConfig, PerlLexer, TokenType};

type TestResult = Result<(), Box<dyn Error>>;

fn collect(input: &str) -> Vec<perl_lexer::Token> {
    PerlLexer::new(input).collect_tokens()
}

#[test]
fn utf8_bom_is_skipped_before_first_token() -> TestResult {
    let mut lexer = PerlLexer::new("\u{FEFF}my $value = 1;");

    let first = lexer.next_token().ok_or("expected first token after BOM")?;

    assert!(matches!(&first.token_type, TokenType::Keyword(word) if word.as_ref() == "my"));
    assert_eq!(first.text.as_ref(), "my");
    assert_eq!(first.start, 3, "UTF-8 BOM should advance the first token byte span");
    assert_eq!(first.end, 5);
    Ok(())
}

#[test]
fn data_marker_consumes_marker_line_then_emits_single_body() -> TestResult {
    let mut lexer = PerlLexer::new("print 1;\n__DATA__   \t\r\npayload\n__END__\nstill data");

    let tokens = lexer.collect_tokens();
    let marker = tokens
        .iter()
        .find(|token| matches!(token.token_type, TokenType::DataMarker(_)))
        .ok_or("expected data marker token")?;
    assert!(
        matches!(&marker.token_type, TokenType::DataMarker(text) if text.as_ref() == "__DATA__")
    );
    assert_eq!(marker.text.as_ref(), "__DATA__");

    let bodies: Vec<_> = tokens
        .iter()
        .filter_map(|token| match &token.token_type {
            TokenType::DataBody(body) => Some(body.as_ref()),
            _ => None,
        })
        .collect();
    assert_eq!(bodies, vec!["payload\n__END__\nstill data"]);
    Ok(())
}

#[test]
fn data_markers_require_line_start_and_trailing_whitespace_only() -> TestResult {
    let not_at_line_start = collect("print 1; __DATA__\ntext");
    assert!(
        !not_at_line_start.iter().any(|token| matches!(
            token.token_type,
            TokenType::DataMarker(_) | TokenType::DataBody(_)
        )),
        "inline __DATA__ must remain normal code tokens"
    );

    let trailing_junk = collect("__END__ extra\ntext");
    assert!(
        !trailing_junk.iter().any(|token| matches!(
            token.token_type,
            TokenType::DataMarker(_) | TokenType::DataBody(_)
        )),
        "marker lines with trailing non-whitespace must not enter data mode"
    );
    Ok(())
}

#[test]
fn format_mode_accepts_dot_terminator_with_horizontal_space_and_crlf() -> TestResult {
    let mut lexer = PerlLexer::new("@<<<\r\nvalue\r\n.  \t\r\nafter");
    lexer.enter_format_mode();

    let token = lexer.next_token().ok_or("expected format body token")?;

    assert!(matches!(token.token_type, TokenType::FormatBody(_)));
    assert_eq!(token.text.as_ref(), "@<<<\r\nvalue\r\n");

    let next = lexer.next_token().ok_or("expected token after format terminator")?;
    assert!(matches!(&next.token_type, TokenType::Identifier(name) if name.as_ref() == "after"));
    Ok(())
}

#[test]
fn format_mode_keeps_dot_lines_that_have_non_whitespace_suffix() -> TestResult {
    let mut lexer = PerlLexer::new(".not terminator\n.\n");
    lexer.enter_format_mode();

    let token = lexer.next_token().ok_or("expected format body token")?;

    assert!(matches!(token.token_type, TokenType::FormatBody(_)));
    assert_eq!(token.text.as_ref(), ".not terminator\n");
    Ok(())
}

#[test]
fn interpolation_config_false_keeps_dollar_text_as_literal_part() -> TestResult {
    let config = LexerConfig { parse_interpolation: false, ..LexerConfig::default() };
    let mut lexer = PerlLexer::with_config(r#""hello $name""#, config);

    let token = lexer.next_token().ok_or("expected string token")?;

    match &token.token_type {
        TokenType::InterpolatedString(parts) => {
            assert_eq!(parts.len(), 1);
            assert!(
                matches!(&parts[0], perl_lexer::StringPart::Literal(text) if text.as_ref() == "hello $name")
            );
        }
        other => return Err(format!("expected uninterpolated literal part, got {other:?}").into()),
    }
    assert_eq!(token.text.as_ref(), r#""hello $name""#);
    Ok(())
}

#[test]
fn reset_allows_body_token_lexer_to_replay_heredoc_body() -> TestResult {
    let input = "print <<EOF;\nbody\nEOF\n";
    let mut lexer = PerlLexer::with_body_tokens(input);
    let first_pass = lexer.collect_tokens();
    assert!(first_pass.iter().any(|token| matches!(token.token_type, TokenType::HeredocBody(_))));

    lexer.reset();
    let second_pass = lexer.collect_tokens();

    assert_eq!(first_pass.len(), second_pass.len());
    assert!(second_pass.iter().any(|token| matches!(token.token_type, TokenType::HeredocBody(_))));
    Ok(())
}
