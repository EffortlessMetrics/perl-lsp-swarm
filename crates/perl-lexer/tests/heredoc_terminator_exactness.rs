//! Perl conformance for exact heredoc terminator matching.

use perl_lexer::{PerlLexer, Token, TokenType};

type R<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn missing(message: impl Into<String>) -> Box<dyn std::error::Error> {
    std::io::Error::other(message.into()).into()
}

fn require(condition: bool, message: impl Into<String>) -> R {
    if condition { Ok(()) } else { Err(missing(message)) }
}

fn require_eq<T>(actual: &T, expected: &T, context: impl Into<String>) -> R
where
    T: std::fmt::Debug + PartialEq + ?Sized,
{
    if actual == expected {
        Ok(())
    } else {
        Err(missing(format!("{}: expected {expected:?}, got {actual:?}", context.into())))
    }
}

fn body_slice<'a>(source: &'a str, tokens: &[Token]) -> R<&'a str> {
    let body = tokens
        .iter()
        .find(|token| matches!(&token.token_type, TokenType::HeredocBody(_)))
        .ok_or_else(|| missing("missing heredoc body token"))?;
    source
        .get(body.start..body.end)
        .ok_or_else(|| missing("heredoc body token has invalid source geometry"))
}

fn require_clean_continuation(source: &str, tokens: &[Token], marker: &str) -> R {
    let marker_start = source.find(marker).ok_or_else(|| missing("missing continuation marker"))?;
    require(
        tokens.iter().any(|token| {
            token.start == marker_start
                && matches!(&token.token_type, TokenType::Keyword(keyword) if keyword.as_ref() == "my")
        }),
        "source after the exact terminator was not tokenized as Perl code",
    )?;
    require(
        tokens.iter().all(|token| !token.token_type.is_recovery_token()),
        "exactly terminated heredoc emitted a recovery token",
    )
}

fn require_unterminated_payload(source: &str, tokens: &[Token], expected: &str) -> R {
    let unknown = tokens
        .iter()
        .find(|token| matches!(&token.token_type, TokenType::UnknownRest))
        .ok_or_else(|| missing("expected unterminated heredoc recovery"))?;
    let payload = source
        .get(unknown.start..unknown.end)
        .ok_or_else(|| missing("unterminated heredoc recovery has invalid source geometry"))?;

    require_eq(payload, expected, "unterminated heredoc recovery payload")
}

#[test]
fn ordinary_heredoc_rejects_trailing_whitespace_near_miss() -> R {
    let source = "<<'END'\nbody\nEND   \nEND\nmy $x = 1;\n";
    let tokens = PerlLexer::with_body_tokens(source).collect_tokens();

    require_eq(body_slice(source, &tokens)?, "body\nEND   \n", "ordinary heredoc body")?;
    require_clean_continuation(source, &tokens, "my $x = 1;")
}

#[test]
fn indented_heredoc_allows_leading_but_not_trailing_whitespace() -> R {
    let source = "<<~'END'\n  body\n  END \t\n  END\nmy $x = 1;\n";
    let tokens = PerlLexer::with_body_tokens(source).collect_tokens();

    require_eq(body_slice(source, &tokens)?, "  body\n  END \t\n", "indented heredoc body")?;
    require_clean_continuation(source, &tokens, "my $x = 1;")
}

#[test]
fn trailing_whitespace_near_miss_without_exact_label_is_unterminated() -> R {
    let source = "<<END\nbody\nEND \t";
    let tokens = PerlLexer::new(source).collect_tokens();
    require_unterminated_payload(source, &tokens, "body\nEND \t")
}

#[test]
fn exact_terminator_accepts_crlf_and_rejects_trailing_space() -> R {
    let source = "<<'END'\r\nbody\r\nEND   \r\nEND\r\nmy $x = 1;\r\n";
    let tokens = PerlLexer::with_body_tokens(source).collect_tokens();

    require_eq(body_slice(source, &tokens)?, "body\r\nEND   \r\n", "CRLF heredoc body")?;
    require_clean_continuation(source, &tokens, "my $x = 1;")
}

#[test]
fn exact_terminator_accepts_bare_cr_and_rejects_trailing_tab() -> R {
    let source = "<<'END'\rbody\rEND \t\rEND\rmy $x = 1;\r";
    let tokens = PerlLexer::with_body_tokens(source).collect_tokens();

    require_eq(body_slice(source, &tokens)?, "body\rEND \t\r", "bare-CR heredoc body")?;
    require_clean_continuation(source, &tokens, "my $x = 1;")
}

#[test]
fn prefix_suffix_comment_and_non_line_whitespace_are_near_misses() -> R {
    for (case, near_miss) in [
        ("leading space", " END\n"),
        ("prefix", "XEND\n"),
        ("suffix", "ENDX\n"),
        ("comment", "END#comment\n"),
        ("vertical tab", "END\u{000b}\n"),
        ("form feed", "END\u{000c}\n"),
    ] {
        let source = format!("<<'END'\nbody\n{near_miss}END\nmy $x = 1;\n");
        let tokens = PerlLexer::with_body_tokens(&source).collect_tokens();
        let expected_body = format!("body\n{near_miss}");

        require_eq(
            body_slice(&source, &tokens)?,
            expected_body.as_str(),
            format!("{case} must remain heredoc body"),
        )?;
        require_clean_continuation(&source, &tokens, "my $x = 1;")?;
    }
    Ok(())
}

#[test]
fn exact_label_at_eof_is_unterminated() -> R {
    // Current perlop documents a terminator as the label immediately followed
    // by a newline; an exact label at EOF is therefore intentionally recovery.
    let source = "<<END\nbody\nEND";
    let tokens = PerlLexer::new(source).collect_tokens();

    require_unterminated_payload(source, &tokens, "body\nEND")
}
