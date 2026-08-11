//! Public behavior proof for self-delimited `qw` recovery (#4499).

use perl_lexer::{LexerConfig, LocalSymbolTable, PerlLexer, TokenType};

fn qw_recovery_span(input: &str, config: LexerConfig) -> Result<String, String> {
    let mut lexer = PerlLexer::with_config(input, config);
    while let Some(token) = lexer.next_token() {
        if matches!(token.token_type, TokenType::Error(_)) && token.text.starts_with("qw") {
            return Ok(token.text.to_string());
        }
    }
    Err(format!("no unclosed-qw error token was produced for {input:?}"))
}

#[test]
fn self_delimited_qw_stops_before_warn_and_say() -> Result<(), String> {
    for (label, input) in [
        ("warn", "my @items = qw[word1 word2\nwarn \"bad\";\nprint 1;"),
        ("say", "my @items = qw[word1 word2\nsay \"bad\";\nprint 1;"),
    ] {
        let span = qw_recovery_span(input, LexerConfig::default())?;
        if span != "qw[word1 word2\n" {
            return Err(format!("[{label}] self-delimited qw consumed its follower: {span:?}"));
        }
    }
    Ok(())
}

#[test]
fn self_delimited_qw_stops_before_known_user_bareword() -> Result<(), String> {
    let input = "my @items = qw[word1 word2\nemit \"bad\";\nprint 1;\nsub emit {}";
    let config = LexerConfig {
        symbol_table: Some(LocalSymbolTable::scan_subs(input)),
        ..LexerConfig::default()
    };
    let span = qw_recovery_span(input, config)?;
    if span != "qw[word1 word2\n" {
        return Err(format!("known user bareword was not a recovery boundary: {span:?}"));
    }
    Ok(())
}

#[test]
fn self_delimited_qw_keeps_similar_words_as_content() -> Result<(), String> {
    let input = "my @items = qw[word1 warning words\nprint 1;";
    let span = qw_recovery_span(input, LexerConfig::default())?;
    if span == "qw[word1 warning words\n" {
        return Err("ordinary qw content was treated as a statement boundary".to_string());
    }
    if !span.contains("warning") {
        return Err(format!("ordinary qw content was lost: {span:?}"));
    }
    Ok(())
}
