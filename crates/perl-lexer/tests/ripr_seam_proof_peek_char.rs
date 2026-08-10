//! Public behavior proof for UTF-8-safe character lookahead.

use perl_lexer::{LexerConfig, PerlLexer, TokenType};

fn first_token_text(input: &str, config: Option<LexerConfig>) -> Result<String, String> {
    let mut lexer = match config {
        Some(config) => PerlLexer::with_config(input, config),
        None => PerlLexer::new(input),
    };
    let token =
        lexer.next_token().ok_or_else(|| format!("lexer returned no token for {input:?}"))?;
    if !matches!(token.token_type, TokenType::Identifier(_)) {
        return Err(format!("expected identifier for {input:?}, got {:?}", token.token_type));
    }
    Ok(token.text.to_string())
}

#[test]
fn utf8_character_lookahead_preserves_public_identifier_text() -> Result<(), String> {
    for input in ["abc", "éxy", "😀xy"] {
        let actual = first_token_text(input, None)?;
        if actual != input {
            return Err(format!("identifier text changed for {input:?}: {actual:?}"));
        }
    }

    let bounded =
        first_token_text("éxy", Some(LexerConfig { max_lookahead: 1, ..LexerConfig::default() }))?;
    if bounded != "éxy" {
        return Err(format!("bounded UTF-8 identifier changed: {bounded:?}"));
    }

    Ok(())
}
