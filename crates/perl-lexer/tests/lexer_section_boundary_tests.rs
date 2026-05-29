//! Focused lexer coverage for section-marker and format-body boundaries.

use perl_lexer::{PerlLexer, Token, TokenType};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn collect(input: &str) -> Vec<Token> {
    PerlLexer::new(input).collect_tokens()
}

#[test]
fn data_marker_allows_trailing_horizontal_whitespace_and_preserves_body() -> TestResult {
    let input = "say 1;\n__DATA__ \t\r\npayload\n__END__\n";
    let tokens = collect(input);

    let marker = tokens
        .iter()
        .find(|token| matches!(token.token_type, TokenType::DataMarker(_)))
        .ok_or("missing __DATA__ marker")?;
    assert!(
        matches!(&marker.token_type, TokenType::DataMarker(name) if name.as_ref() == "__DATA__")
    );
    assert_eq!(marker.text.as_ref(), "__DATA__");
    assert_eq!(marker.start, input.find("__DATA__").ok_or("missing source marker")?);
    assert_eq!(marker.end, input.find("payload").ok_or("missing payload")?);

    let body = tokens
        .iter()
        .find(|token| matches!(token.token_type, TokenType::DataBody(_)))
        .ok_or("missing data body")?;
    assert!(
        matches!(&body.token_type, TokenType::DataBody(text) if text.as_ref() == "payload\n__END__\n")
    );
    assert_eq!(body.text.as_ref(), "payload\n__END__\n");
    Ok(())
}

#[test]
fn end_marker_allows_trailing_tabs_before_body() -> TestResult {
    let input = "__END__\t\t\ntrailer\n";
    let tokens = collect(input);

    let marker = tokens.first().ok_or("missing first token")?;
    assert!(
        matches!(&marker.token_type, TokenType::DataMarker(name) if name.as_ref() == "__END__")
    );

    let body = tokens.get(1).ok_or("missing data body token")?;
    assert!(matches!(&body.token_type, TokenType::DataBody(text) if text.as_ref() == "trailer\n"));
    Ok(())
}

#[test]
fn data_marker_with_trailing_junk_remains_identifier() -> TestResult {
    let tokens = collect("__DATA__ # not a marker because comment text trails it\nmy $x = 1;");

    assert!(
        !tokens.iter().any(|token| matches!(token.token_type, TokenType::DataMarker(_))),
        "trailing non-whitespace must prevent marker recognition"
    );
    assert!(
        matches!(&tokens.first().ok_or("missing first token")?.token_type, TokenType::Identifier(name) if name.as_ref() == "__DATA__")
    );
    assert!(
        tokens.iter().any(
            |token| matches!(&token.token_type, TokenType::Keyword(kw) if kw.as_ref() == "my")
        ),
        "lexer should continue tokenizing code after a non-marker __DATA__ identifier"
    );
    Ok(())
}

#[test]
fn data_marker_not_at_line_start_remains_identifier() -> TestResult {
    let tokens = collect("print __DATA__\nmy $x = 1;");

    assert!(
        !tokens.iter().any(|token| matches!(token.token_type, TokenType::DataMarker(_))),
        "inline __DATA__ must not switch the lexer into data-section mode"
    );
    assert!(
        tokens.iter().any(|token| matches!(&token.token_type, TokenType::Identifier(name) if name.as_ref() == "__DATA__")),
        "inline __DATA__ should remain a normal identifier token"
    );
    Ok(())
}

#[test]
fn format_body_accepts_dot_terminator_with_trailing_whitespace() -> TestResult {
    let mut lexer = PerlLexer::new("line one\n.\t \r\nmy $x = 1;");
    lexer.enter_format_mode();

    let body = lexer.next_token().ok_or("missing format body")?;
    assert!(
        matches!(&body.token_type, TokenType::FormatBody(text) if text.as_ref() == "line one\n")
    );
    assert_eq!(body.text.as_ref(), "line one\n");

    let next = lexer.next_token().ok_or("missing token after format body")?;
    assert!(matches!(&next.token_type, TokenType::Keyword(kw) if kw.as_ref() == "my"));
    Ok(())
}
