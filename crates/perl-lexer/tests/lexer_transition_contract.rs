//! Public transition and invariant contract for the mode-aware lexer.
//!
//! The lexer has a richer state machine than `ExpectTerm`/`ExpectOperator`, but
//! callers observe that state through token identity, source geometry, peek/reset
//! behavior, checkpoint replay, recovery, and terminal behavior. These tests pin
//! those public transitions without exposing private fields.

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
        Self { token_type: token.token_type, text: token.text, start: token.start, end: token.end }
    }
}

fn collect_remaining(lexer: &mut PerlLexer<'_>, input: &str) -> R<Vec<TokenView>> {
    let mut tokens: Vec<TokenView> = Vec::new();
    let max_tokens = input.len().saturating_add(1);
    let mut previous_end = 0usize;
    while let Some(token) = lexer.next_token() {
        if tokens.len() >= max_tokens {
            return Err(missing(format!(
                "lexer exceeded the bounded token budget for {} input bytes",
                input.len()
            )));
        }
        if token.start > token.end
            || !input.is_char_boundary(token.start)
            || !input.is_char_boundary(token.end)
            || token.start < previous_end
        {
            return Err(missing(format!("invalid or overlapping token span: {token:?}")));
        }
        let eof = matches!(&token.token_type, TokenType::EOF);
        if eof {
            if token.start != input.len() || token.end != input.len() {
                return Err(missing(format!("EOF is not at the input end: {token:?}")));
            }
        } else if token.end <= token.start {
            return Err(missing(format!("non-EOF token made no progress: {token:?}")));
        }
        previous_end = token.end;
        tokens.push(token.into());
        if eof {
            break;
        }
    }
    if !matches!(tokens.last().map(|token| &token.token_type), Some(TokenType::EOF)) {
        return Err(missing("lexer ended without emitting EOF"));
    }
    Ok(tokens)
}

fn collect(input: &str) -> R<Vec<TokenView>> {
    collect_remaining(&mut PerlLexer::new(input), input)
}

fn missing(message: impl Into<String>) -> Box<dyn std::error::Error> {
    std::io::Error::other(message.into()).into()
}

fn token_with_text<'a>(tokens: &'a [TokenView], text: &str) -> R<&'a TokenView> {
    tokens
        .iter()
        .find(|token| token.text.as_ref() == text)
        .ok_or_else(|| missing(format!("missing token text {text:?}")))
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
            assert_eq!((token.start, token.end), (input.len(), input.len()));
        } else {
            assert!(token.end > token.start, "non-EOF token made no progress: {token:?}");
            previous_end = token.end;
        }
    }

    assert_eq!(eof_count, 1);
    assert!(lexer.next_token().is_none(), "EOF must remain terminal");
}

#[test]
fn slash_transition_distinguishes_division_regex_and_defined_or() -> R {
    let division = collect("my $x = 10 / 2;")?;
    let division_token = token_with_text(&division, "/")?;
    assert!(matches!(&division_token.token_type, TokenType::Division));
    assert_eq!((division_token.start, division_token.end), (11, 12));
    assert!(!division.iter().any(|token| matches!(&token.token_type, TokenType::RegexMatch)));

    let regex = collect("/answer/")?;
    let regex_token = token_with_text(&regex, "/answer/")?;
    assert!(matches!(&regex_token.token_type, TokenType::RegexMatch));
    assert_eq!((regex_token.start, regex_token.end), (0, 8));
    assert!(!regex.iter().any(|token| matches!(&token.token_type, TokenType::Division)));

    let defined_or = collect("$left // $right;")?;
    let defined_or_token = token_with_text(&defined_or, "//")?;
    assert!(
        matches!(&defined_or_token.token_type, TokenType::Operator(operator) if operator.as_ref() == "//")
    );
    assert_eq!((defined_or_token.start, defined_or_token.end), (6, 8));
    assert!(!defined_or_token.token_type.is_recovery_token());
    assert!(
        !defined_or.iter().any(|token| {
            matches!(&token.token_type, TokenType::Division | TokenType::RegexMatch)
        })
    );
    Ok(())
}

#[test]
fn quote_looking_hash_keys_and_method_names_have_exact_non_operator_identity() -> R {
    let hash_source = "$h{s} + $h{tr} + $h{m};";
    let hash_tokens = collect(hash_source)?;
    for name in ["s", "tr", "m"] {
        let token = token_with_text(&hash_tokens, name)?;
        assert!(
            matches!(&token.token_type, TokenType::Keyword(keyword) if keyword.as_ref() == name),
            "hash key {name:?} had unexpected type {:?}",
            token.token_type
        );
        assert_eq!(hash_source.get(token.start..token.end), Some(name));
    }
    assert!(!hash_tokens.iter().any(|token| {
        matches!(
            &token.token_type,
            TokenType::Substitution | TokenType::Transliteration | TokenType::RegexMatch
        )
    }));

    let method_source = "$obj->m('arg'); $obj->s('arg');";
    let method_tokens = collect(method_source)?;
    for name in ["m", "s"] {
        let token = token_with_text(&method_tokens, name)?;
        assert!(
            matches!(&token.token_type, TokenType::Keyword(keyword) if keyword.as_ref() == name),
            "method name {name:?} had unexpected type {:?}",
            token.token_type
        );
        assert_eq!(method_source.get(token.start..token.end), Some(name));
    }
    assert_eq!(method_tokens.iter().filter(|token| token.text.as_ref() == "->").count(), 2);
    assert!(!method_tokens.iter().any(|token| {
        matches!(
            &token.token_type,
            TokenType::Substitution | TokenType::Transliteration | TokenType::RegexMatch
        )
    }));

    let operators = collect("m/pat/; s/a/b/; tr/a-z/A-Z/;")?;
    assert!(matches!(&token_with_text(&operators, "m/pat/")?.token_type, TokenType::RegexMatch));
    assert!(matches!(&token_with_text(&operators, "s/a/b/")?.token_type, TokenType::Substitution));
    assert!(matches!(
        &token_with_text(&operators, "tr/a-z/A-Z/")?.token_type,
        TokenType::Transliteration
    ));
    Ok(())
}

#[test]
fn quote_operator_and_fat_arrow_take_opposite_transitions() -> R {
    let quote = collect("q{value};")?;
    let quote_token = token_with_text(&quote, "q{value}")?;
    assert!(matches!(&quote_token.token_type, TokenType::QuoteSingle));
    assert_eq!((quote_token.start, quote_token.end), (0, 8));

    let fat_arrow = collect("q => 1;")?;
    let q = token_with_text(&fat_arrow, "q")?;
    assert!(matches!(&q.token_type, TokenType::Identifier(name) if name.as_ref() == "q"));
    let arrow = token_with_text(&fat_arrow, "=>")?;
    assert!(
        matches!(&arrow.token_type, TokenType::Operator(operator) if operator.as_ref() == "=>")
    );
    assert_eq!((arrow.start, arrow.end), (2, 4));
    assert!(!fat_arrow.iter().any(|token| matches!(&token.token_type, TokenType::QuoteSingle)));

    // No gap before `=>`: the whitespace rule cannot decide this one, so the
    // fat-arrow lookahead is the only thing keeping `q` a bareword key here.
    let tight = collect("q=>1;")?;
    let tight_q = token_with_text(&tight, "q")?;
    assert!(matches!(&tight_q.token_type, TokenType::Identifier(name) if name.as_ref() == "q"));
    assert_eq!((tight_q.start, tight_q.end), (0, 1));
    let tight_arrow = token_with_text(&tight, "=>")?;
    assert!(
        matches!(&tight_arrow.token_type, TokenType::Operator(operator) if operator.as_ref() == "=>")
    );
    assert_eq!((tight_arrow.start, tight_arrow.end), (1, 3));
    assert!(!tight.iter().any(|token| matches!(&token.token_type, TokenType::QuoteSingle)));
    Ok(())
}

#[test]
fn heredoc_body_event_precedes_the_resumed_statement() -> R {
    let input = "print <<EOF;\nbody\nEOF\nmy $x = 1;\n";
    let mut lexer = PerlLexer::with_body_tokens(input);
    let tokens = collect_remaining(&mut lexer, input)?;

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
    assert!(body.text.is_empty());
    assert_eq!(input.get(body.start..body.end), Some("body\n"));
    assert!(!tokens.iter().any(|token| token.token_type.is_recovery_token()));
    assert!(matches!(tokens.last().map(|token| &token.token_type), Some(TokenType::EOF)));
    Ok(())
}

#[test]
fn data_section_is_terminal_code_state_with_one_body_and_one_eof() -> R {
    let input = "my $x = 1;\n__DATA__\nsub not_code { 1 }\n";
    let tokens = collect(input)?;

    let marker_index = tokens
        .iter()
        .position(|token| matches!(&token.token_type, TokenType::DataMarker(_)))
        .ok_or_else(|| missing("missing data marker token"))?;
    let body_index = tokens
        .iter()
        .position(|token| matches!(&token.token_type, TokenType::DataBody(_)))
        .ok_or_else(|| missing("missing data body token"))?;

    assert!(marker_index < body_index);
    assert_eq!(input.get(tokens[marker_index].start..tokens[marker_index].end), Some("__DATA__\n"));
    assert_eq!(tokens[body_index].text.as_ref(), "sub not_code { 1 }\n");
    assert_eq!(
        input.get(tokens[body_index].start..tokens[body_index].end),
        Some("sub not_code { 1 }\n")
    );
    assert_eq!(tokens[body_index].start, tokens[marker_index].end);
    assert_eq!(tokens.len(), body_index + 2);
    assert!(matches!(&tokens[body_index + 1].token_type, TokenType::EOF));
    assert!(
        !tokens[body_index + 1..]
            .iter()
            .any(|token| token.text.as_ref() == "sub" || token.text.as_ref() == "not_code")
    );
    Ok(())
}

#[test]
fn peek_and_reset_preserve_the_exact_public_stream() -> R {
    let input = "$obj->m('arg'); q{value};";
    let expected = collect(input)?;
    let mut lexer = PerlLexer::new(input);

    let peeked = lexer.peek_token().ok_or_else(|| missing("missing peeked token"))?;
    let next = lexer.next_token().ok_or_else(|| missing("missing token after peek"))?;
    assert_eq!(TokenView::from(peeked), TokenView::from(next));

    let _ = lexer.next_token();
    lexer.reset();
    assert_eq!(collect_remaining(&mut lexer, input)?, expected);
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
    let first_suffix = collect_remaining(&mut lexer, input)?;

    lexer.restore(&checkpoint);
    let restored_suffix = collect_remaining(&mut lexer, input)?;

    assert_eq!(first_suffix, restored_suffix);
    let method = token_with_text(&restored_suffix, "m")?;
    assert!(matches!(&method.token_type, TokenType::Keyword(name) if name.as_ref() == "m"));
    assert!(
        !restored_suffix.iter().any(|token| matches!(&token.token_type, TokenType::RegexMatch))
    );
    Ok(())
}
