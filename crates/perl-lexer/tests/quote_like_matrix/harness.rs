//! Public lexer and parser TokenStream observation harness.

use super::schema::{ExpectedKind, MatrixRow, NextOrdinary, TerminalStateClass, TokenSpec};
use perl_lexer::{Checkpointable, LexerMode, PerlLexer, Token, TokenType};
use perl_parser_core::TokenStream;
use perl_token::TokenKind;

pub fn observe_and_assert(row: &MatrixRow) -> Result<(), String> {
    let lexer_tokens = lex(row.source);
    assert_lexer_tokens(row, &lexer_tokens)?;
    assert_next_ordinary(row, &lexer_tokens)?;
    assert_terminal_state(row)?;
    assert_token_stream_agrees(row, &lexer_tokens)?;
    super::oracle::check_expectation(row.source, row.oracle).map_err(|error| prefix(row, error))?;
    Ok(())
}

pub fn lex(source: &str) -> Vec<Token> {
    PerlLexer::new(source).collect_tokens()
}

fn assert_lexer_tokens(row: &MatrixRow, tokens: &[Token]) -> Result<(), String> {
    let expected = locate_tokens(row.source, row.expected)?;
    if tokens.len() != expected.len() {
        return Err(prefix(
            row,
            format!(
                "token count {} != {}, got {}",
                tokens.len(),
                expected.len(),
                format_tokens(tokens)
            ),
        ));
    }
    for (index, (got, want)) in tokens.iter().zip(expected.iter()).enumerate() {
        if !want.kind.matches(&got.token_type) {
            return Err(prefix(
                row,
                format!(
                    "token {index} kind {:?} != {:?}: {:?}",
                    got.token_type, want.kind, got.text
                ),
            ));
        }
        if got.text.as_ref() != want.text {
            return Err(prefix(
                row,
                format!("token {index} text {:?} != {:?}", got.text.as_ref(), want.text),
            ));
        }
        if (got.start, got.end) != (want.start, want.end) {
            return Err(prefix(
                row,
                format!(
                    "token {index} span {}..{} != {}..{}",
                    got.start, got.end, want.start, want.end
                ),
            ));
        }
        if row.source.get(got.start..got.end) != Some(want.text)
            && !matches!(want.kind, ExpectedKind::Eof)
        {
            return Err(prefix(row, format!("token {index} span is not source-anchored")));
        }
        if !row.source.is_char_boundary(got.end) {
            return Err(prefix(row, format!("token {index} end is not a UTF-8 boundary")));
        }
    }
    Ok(())
}

fn assert_next_ordinary(row: &MatrixRow, tokens: &[Token]) -> Result<(), String> {
    match row.next_ordinary {
        NextOrdinary::Present { kind, text } => {
            let found = tokens
                .iter()
                .find(|token| token.text.as_ref() == text)
                .ok_or_else(|| prefix(row, format!("missing next ordinary token {text:?}")))?;
            if !kind.matches(&found.token_type) {
                return Err(prefix(
                    row,
                    format!("next ordinary {text:?} had {:?}", found.token_type),
                ));
            }
            Ok(())
        }
        NextOrdinary::EatenByError => {
            if !tokens.iter().any(|token| matches!(token.token_type, TokenType::Error(_))) {
                return Err(prefix(row, "expected following code to be eaten by Error"));
            }
            if tokens.iter().any(|token| token.text.as_ref() == "after") {
                return Err(prefix(row, "following code `after` leaked past Error"));
            }
            Ok(())
        }
        NextOrdinary::EatenByComment => {
            if tokens.iter().any(|token| token.text.as_ref() == "after") {
                return Err(prefix(row, "following code `after` leaked past comment"));
            }
            Ok(())
        }
        NextOrdinary::NoneAtEof => Ok(()),
    }
}

fn assert_terminal_state(row: &MatrixRow) -> Result<(), String> {
    let mut lexer = PerlLexer::new(row.source);
    let _ = lexer.collect_tokens();
    let checkpoint = lexer.checkpoint();
    if !row.terminal.matches(checkpoint.mode) {
        return Err(prefix(
            row,
            format!("terminal mode {:?} != {:?}", checkpoint.mode, row.terminal),
        ));
    }
    if checkpoint.current_quote_op.is_some() {
        return Err(prefix(row, "stale current_quote_op after observation"));
    }
    if !checkpoint.delimiter_stack.is_empty() {
        return Err(prefix(row, "stale delimiter_stack after observation"));
    }
    if !checkpoint.eof_emitted {
        return Err(prefix(row, "EOF was not emitted"));
    }
    match row.terminal {
        TerminalStateClass::ExpectOperator | TerminalStateClass::ExpectTerm => {
            if matches!(checkpoint.mode, LexerMode::ExpectDelimiter | LexerMode::InFormatBody) {
                return Err(prefix(row, "terminal mode remained inside a quote-like class"));
            }
        }
    }
    Ok(())
}

fn assert_token_stream_agrees(row: &MatrixRow, lexer_tokens: &[Token]) -> Result<(), String> {
    let converted = TokenStream::lexer_tokens_to_parser_tokens(lexer_tokens.to_vec());
    let streamed = drain_token_stream(row.source)?;
    let Some((stream_eof, stream_tokens)) = streamed.split_last() else {
        return Err(prefix(row, "TokenStream produced no tokens"));
    };
    if stream_eof.kind() != TokenKind::Eof {
        return Err(prefix(row, "TokenStream did not end at EOF"));
    }
    if stream_eof.start() != row.source.len() || stream_eof.end() != row.source.len() {
        return Err(prefix(
            row,
            format!(
                "TokenStream EOF span {}..{} != {}",
                stream_eof.start(),
                stream_eof.end(),
                row.source.len()
            ),
        ));
    }
    if converted.len() != stream_tokens.len() {
        return Err(prefix(
            row,
            format!(
                "TokenStream count {} != converted lexer count {}",
                stream_tokens.len(),
                converted.len()
            ),
        ));
    }
    for (index, (stream_token, converted_token)) in
        stream_tokens.iter().zip(converted.iter()).enumerate()
    {
        if stream_token.kind() != converted_token.kind()
            || stream_token.text.as_ref() != converted_token.text.as_ref()
            || stream_token.start() != converted_token.start()
            || stream_token.end() != converted_token.end()
        {
            return Err(prefix(
                row,
                format!(
                    "TokenStream token {index} {:?} {:?} {}..{} != converted {:?} {:?} {}..{}",
                    stream_token.kind(),
                    stream_token.text,
                    stream_token.start(),
                    stream_token.end(),
                    converted_token.kind(),
                    converted_token.text,
                    converted_token.start(),
                    converted_token.end()
                ),
            ));
        }
    }
    Ok(())
}

fn drain_token_stream(source: &str) -> Result<Vec<perl_token::Token>, String> {
    let mut stream = TokenStream::new(source);
    let mut tokens = Vec::new();
    loop {
        let token = stream.next().map_err(|error| format!("TokenStream: {error}"))?;
        let is_eof = token.kind() == TokenKind::Eof;
        tokens.push(token);
        if is_eof {
            break;
        }
        if tokens.len() > 256 {
            return Err("TokenStream produced too many tokens".to_string());
        }
    }
    Ok(tokens)
}

struct LocatedToken {
    kind: ExpectedKind,
    text: &'static str,
    start: usize,
    end: usize,
}

fn locate_tokens(source: &str, specs: &[TokenSpec]) -> Result<Vec<LocatedToken>, String> {
    let mut pos = 0;
    let mut located = Vec::with_capacity(specs.len());
    for spec in specs {
        if matches!(spec.kind, ExpectedKind::Eof) {
            located.push(LocatedToken {
                kind: spec.kind,
                text: "",
                start: source.len(),
                end: source.len(),
            });
            continue;
        }
        let remainder =
            source.get(pos..).ok_or_else(|| format!("span {pos} is not a char boundary"))?;
        let rel = remainder
            .find(spec.text)
            .ok_or_else(|| format!("expected token text {:?} after byte {pos}", spec.text))?;
        let start = pos + rel;
        let end = start + spec.text.len();
        located.push(LocatedToken { kind: spec.kind, text: spec.text, start, end });
        pos = end;
    }
    Ok(located)
}

fn format_tokens(tokens: &[Token]) -> String {
    tokens
        .iter()
        .map(|token| format!("{:?} {:?}", token.token_type, token.text.as_ref()))
        .collect::<Vec<_>>()
        .join(" | ")
}

fn prefix(row: &MatrixRow, message: impl Into<String>) -> String {
    format!("{}: {}", row.id, message.into())
}
