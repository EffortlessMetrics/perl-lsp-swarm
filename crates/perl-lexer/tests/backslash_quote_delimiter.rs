use perl_lexer::{LexerConfig, PerlLexer, StringPart, TokenType};

#[test]
fn backslash_quote_tokens_stop_at_first_closer() -> Result<(), String> {
    for interpolation in [false, true] {
        for quote in [r"q\foo\", r"qq\foo\", r"q\\", r"qq\\", "qw\\a\nmy b\\"] {
            let source = format!("{quote}; my $after = 7;");
            let config = LexerConfig { parse_interpolation: interpolation, ..Default::default() };
            let mut lexer = PerlLexer::with_config(&source, config);
            let token = lexer.next_token().ok_or("missing quote")?;
            let correct_kind = if quote.starts_with("qq") {
                matches!(token.token_type, TokenType::QuoteDouble(_))
            } else if quote.starts_with("qw") {
                token.token_type == TokenType::QuoteWords
            } else {
                token.token_type == TokenType::QuoteSingle
            };
            if token.start != 0
                || token.end != quote.len()
                || &*token.text != quote
                || !correct_kind
            {
                return Err(format!("wrong quote boundary: {token:?}"));
            }
            let separator = lexer.next_token().ok_or("missing semicolon")?;
            if separator.token_type != TokenType::Semicolon || separator.start != quote.len() {
                return Err(format!("swallowed separator: {separator:?}"));
            }
        }
    }
    Ok(())
}

#[test]
fn backslash_qq_keeps_interpolation_and_literal_mode() -> Result<(), String> {
    for enabled in [true, false] {
        let config = LexerConfig { parse_interpolation: enabled, ..Default::default() };
        let mut lexer = PerlLexer::with_config(r"qq\$x\;", config);
        let token = lexer.next_token().ok_or("missing qq")?;
        let TokenType::QuoteDouble(parts) = token.token_type else {
            return Err("qq not classified".into());
        };
        let valid = if enabled {
            matches!(parts.as_slice(), [StringPart::Variable(value)] if &**value == "$x")
        } else {
            matches!(parts.as_slice(), [StringPart::Literal(value)] if &**value == "$x")
        };
        if !valid || token.end != 6 {
            return Err(format!("wrong interpolation or range: {parts:?}, {}", token.end));
        }
    }
    Ok(())
}
