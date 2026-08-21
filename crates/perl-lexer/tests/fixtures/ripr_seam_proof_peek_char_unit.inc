use crate::LexerConfig;

#[test]
fn peek_char_uses_character_offsets_for_ascii_and_utf8() -> Result<(), String> {
    let ascii = PerlLexer::new("abc");
    if ascii.peek_char(0) != Some('a') {
        return Err("ASCII offset 0 returned an unexpected character".to_string());
    }
    if ascii.peek_char(2) != Some('c') {
        return Err("ASCII offset 2 returned an unexpected character".to_string());
    }
    if ascii.peek_char(3).is_some() {
        return Err("ASCII EOF returned a character".to_string());
    }

    let bmp = PerlLexer::new("éxy");
    if bmp.peek_char(0) != Some('é') {
        return Err("BMP offset 0 returned an unexpected character".to_string());
    }
    if bmp.peek_char(1) != Some('x') {
        return Err("BMP offset 1 returned an unexpected character".to_string());
    }
    if bmp.peek_char(2) != Some('y') {
        return Err("BMP offset 2 returned an unexpected character".to_string());
    }

    let non_bmp = PerlLexer::new("😀xy");
    if non_bmp.peek_char(0) != Some('😀') {
        return Err("non-BMP offset 0 returned an unexpected character".to_string());
    }
    if non_bmp.peek_char(1) != Some('x') {
        return Err("non-BMP offset 1 returned an unexpected character".to_string());
    }
    if non_bmp.peek_char(2) != Some('y') {
        return Err("non-BMP offset 2 returned an unexpected character".to_string());
    }

    Ok(())
}

#[test]
fn peek_char_preserves_eof_and_max_lookahead_bounds() -> Result<(), String> {
    let config = LexerConfig { max_lookahead: 1, ..LexerConfig::default() };
    let lexer = PerlLexer::with_config("éxy", config);

    if lexer.peek_char(1) != Some('x') {
        return Err("lookahead offset 1 returned an unexpected character".to_string());
    }
    if lexer.peek_char(2).is_some() {
        return Err("bounded lookahead returned a character".to_string());
    }

    let eof = PerlLexer::new("éx");
    if eof.peek_char(2).is_some() {
        return Err("EOF returned a character".to_string());
    }

    Ok(())
}

#[test]
fn peek_char_handles_nonzero_positions_on_ascii_and_utf8_paths() -> Result<(), String> {
    let mut ascii = PerlLexer::new("zabc");
    ascii.position = 1;
    if ascii.peek_char(2) != Some('c') {
        return Err("nonzero ASCII offset 2 returned an unexpected character".to_string());
    }

    let mut utf8 = PerlLexer::new("zéxy");
    utf8.position = 1;
    if utf8.peek_char(2) != Some('y') {
        return Err("nonzero UTF-8 offset 2 returned an unexpected character".to_string());
    }

    Ok(())
}
