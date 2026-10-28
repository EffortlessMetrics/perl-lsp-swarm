use perl_lexer::{PerlLexer, TokenType};

fn significant(source: &str) -> Vec<String> {
    PerlLexer::new(source)
        .collect_tokens()
        .into_iter()
        .filter(|token| {
            !matches!(token.token_type, TokenType::Whitespace | TokenType::Newline | TokenType::EOF)
        })
        .map(|token| token.text.to_string())
        .collect()
}

#[test]
fn dereference_does_not_consume_pid_variable() {
    assert_eq!(significant("$$ref;"), ["$", "$ref", ";"]);
    assert_eq!(significant("$$Pkg::ref;"), ["$", "$Pkg::ref", ";"]);
    assert_eq!(significant("$$π;"), ["$", "$π", ";"]);
}

#[test]
fn bare_pid_remains_one_token() {
    assert_eq!(significant("$$;"), ["$$", ";"]);
    assert_eq!(significant("$$ + 1;"), ["$$", "+", "1", ";"]);
    assert_eq!(significant("$!;"), ["$!", ";"]);
}
