//! Public transition and invariant contract for the mode-aware lexer.
//!
//! The lexer has a richer state machine than `ExpectTerm`/`ExpectOperator`, but
//! callers observe that state through token classification, source geometry,
//! checkpoint replay, recovery, and terminal behavior. These tests lock those
//! observable transitions without exposing private implementation fields.

use std::sync::Arc;

use perl_lexer::{Checkpointable, PerlLexer, Token, TokenType};

type R<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[derive(Debug, Clone, PartialEq)]
struct TokenView {
    token_type: TokenType,
    text: Arc<str>,
    start: usize,
    end: usize,
}

impl From<Token> for TokenView {
    fn from(token: Token) -> Self {
        Self {
            token_type: token.token_type,
            text: token.text,
            start: token.start,
            end: token.end,
        }
    }
}

fn collect_remaining(lexer: &mut PerlLexer<'_>) -> Vec<TokenView> {
    let mut tokens = Vec::new();
    while let Some(token) = lexer.next_token() {
        let eof = matches!(&token.token_type, TokenType::EOF);
        tokens.push(token.into());
        if eof {
            break;
        }
    }
    tokens
}

fn collect(input: &str) -> Vec<TokenView> {
    collect_remaining(&mut PerlLexer::new(input))
}

fn missing(message: &'static str) -> Box<dyn std::error::Error> {
    std::io::Error::other(message).into()
}

#[test]
fn eof_is_emitted_once_and_every_ordinary_token_advances() {
    let input = "my $café = q{value}; $café =~ /value/;";
    let mut lexer = PerlLexer::new(input);
    let mut previous_end = 0usize;
    let mut eof_count = 0usize;
    let mut token_count = 0usize;

    while let Some(token) = lexer.next_token() {
        token_count += 1;
        assert!(token_count < 100, "lexer did not terminate for a short valid input");
        assert!(token.start <= token.end, "reversed token span: {token:?}");
        assert!(input.is_char_boundary(token.start));
        assert!(input.is_char_boundary(token.end));
        assert!(token.start >= previous_end, "overlapping token span: {token:?}");

        if matches!(&token.token_type, TokenType::EOF) {
            eof_count += 1;
            assert_eq!(token.start, input.len());
            assert_eq!(token.end, input.len());
        } else {
            assert!(token.end > token.start, "non-EOF token made no progress: {token:?}");
            previous_end = token.end;
        }
    }

    assert_eq!(eof_count, 1);
    assert!(lexer.next_token().is_none(), "EOF must remain terminal");
}

#[test]
fn slash_transition_distinguishes_division_regex_and_defined_or() {
    let division = collect("my $x = 10 / 2;");
    assert!(division.iter().any(|token| matches!(&token.token_type, TokenType::Division)));
    assert!(!division.iter().any(|token| matches!(&token.token_type, TokenType::RegexMatch)));

    let regex = collect("/answer/;");
    assert!(regex.iter().any(|token| matches!(&token.token_type, TokenType::RegexMatch)));
    assert!(!regex.iter().any(|token| matches!(&token.token_type, TokenType::Division)));

    let defined_or = collect("$left // $right;");
    assert!(defined_or.iter().any(|token| token.text.as_ref() == "//"));
    assert!(!defined_or.iter().any(|token| matches!(&token.token_type, TokenType::RegexMatch)));
}

#[test]
fn quote_looking_hash_keys_and_method_names_do_not_leak_operator_state() {
    let hash_key = collect("$h{s} + $h{tr} + $h{m};");
    assert!(hash_key.iter().any(|token| token.text.as_ref() == "s"));
    assert!(!hash_key.iter().any(|token| {
        matches!(
            &token.token_type,
            TokenType::Substitution | TokenType::Transliteration | TokenType::RegexMatch
        )
    }));

    let method = collect("$obj->m('arg'); $obj->s('arg');");
    assert!(method.iter().any(|token| token.text.as_ref() == "->"));
    assert!(method.iter().any(|token| token.text.as_ref() == "m"));
    assert!(method.iter().any(|token| token.text.as_ref() == "s"));
    assert!(!method.iter().any(|token| {
        matches!(
            &token.token_type,
            TokenType::Substitution | TokenType::Transliteration | TokenType::RegexMatch
        )
    }));

    let operators = collect("m/pat/; s/a/b/; tr/a-z/A-Z/;");
    assert!(operators.iter().any(|token| matches!(&token.token_type, TokenType::RegexMatch)));
    assert!(operators.iter().any(|token| matches!(&token.token_type, TokenType::Substitution)));
    assert!(operators.iter().any(|token| matches!(&token.token_type, TokenType::Transliteration)));
}

#[test]
fn quote_operator_and_fat_arrow_take_opposite_transitions() {
    let quote = collect("q{value};");
    assert!(quote.iter().any(|token| matches!(&token.token_type, TokenType::QuoteSingle)));

    let fat_arrow = collect("q => 1;");
    assert!(fat_arrow.iter().any(|token| {
        matches!(&token.token_type, TokenType::Identifier(name) if name.as_ref() == "q")
    }));
    assert!(fat_arrow.iter().any(|token| matches!(&token.token_type, TokenType::FatComma)));
    assert!(!fat_arrow.iter().any(|token| matches!(&token.token_type, TokenType::QuoteSingle)));
}

#[test]
fn heredoc_body_event_precedes_the_resumed_statement() -> R {
    let input = "print <<EOF;\nbody\nEOF\nmy $x = 1;\n";
    let mut lexer = PerlLexer::with_body_tokens(input);
    let tokens = collect_remaining(&mut lexer);

    let start_index = tokens
        .iter()
        .position(|token| matches!(&token.token_type, TokenType::HeredocStart))
        .ok_or_else(|| missing("missing heredoc opener token"))?;
    let body_index = tokens
        .iter()
        .position(|token| matches!(&token.token_type, TokenType::HeredocBody(_)))
        .ok_or_else(|| missing("missing heredoc body token"))?;
    let resumed_index = tokens
        .iter()
        .position(|token| token.text.as_ref() == "my")
        .ok_or_else(|| missing("missing statement following heredoc"))?;

    assert!(start_index < body_index);
    assert!(body_index < resumed_index);
    let body = &tokens[body_index];
    assert_eq!(input.get(body.start..body.end), Some("body\n"));
    assert!(matches!(tokens.last().map(|token| &token.token_type), Some(TokenType::EOF)));
    Ok(())
}

#[test]
fn data_section_is_terminal_code_state_with_a_separate_body() -> R {
    let input = "my $x = 1;\n__DATA__\nsub not_code { 1 }\n";
    let tokens = collect(input);

    let marker_index = tokens
        .iter()
        .position(|token| matches!(&token.token_type, TokenType::DataMarker(_)))
        .ok_or_else(|| missing("missing data marker token"))?;
    let body_index = tokens
        .iter()
        .position(|token| matches!(&token.token_type, TokenType::DataBody(_)))
        .ok_or_else(|| missing("missing data body token"))?;

    assert!(marker_index < body_index);
    assert_eq!(tokens[body_index].text.as_ref(), "sub not_code { 1 }\n");
    assert!(tokens[body_index + 1..]
        .iter()
        .all(|token| matches!(&token.token_type, TokenType::EOF)));
    Ok(())
}

#[test]
fn checkpoint_after_arrow_replays_the_exact_method_suffix() -> R {
    let input = "$obj->m('arg')->s('next');";
    let mut lexer = PerlLexer::new(input);

    loop {
        let token = lexer.next_token().ok_or_else(|| missing("arrow before EOF"))?;
        if token.text.as_ref() == "->" {
            break;
        }
        if matches!(&token.token_type, TokenType::EOF) {
            return Err(missing("arrow before EOF"));
        }
    }

    let checkpoint = lexer.checkpoint();
    assert!(lexer.can_restore(&checkpoint));
    let first_suffix = collect_remaining(&mut lexer);

    lexer.restore(&checkpoint);
    let restored_suffix = collect_remaining(&mut lexer);

    assert_eq!(first_suffix, restored_suffix);
    assert!(restored_suffix.iter().any(|token| token.text.as_ref() == "m"));
    assert!(!restored_suffix.iter().any(|token| matches!(&token.token_type, TokenType::RegexMatch)));
    Ok(())
}
