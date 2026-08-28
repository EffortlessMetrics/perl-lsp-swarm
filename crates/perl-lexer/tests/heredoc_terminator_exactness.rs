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
    let unknown = tokens
        .iter()
        .find(|token| matches!(&token.token_type, TokenType::UnknownRest))
        .ok_or_else(|| missing("whitespace-suffixed near miss terminated the heredoc"))?;
    let payload = source
        .get(unknown.start..unknown.end)
        .ok_or_else(|| missing("unterminated heredoc recovery has invalid source geometry"))?;

    require_eq(payload, "body\nEND \t", "unterminated heredoc recovery payload")
}
