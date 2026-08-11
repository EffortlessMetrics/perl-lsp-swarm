//! Lexer-owned matrix for supported Perl quote-like operators.
//!
//! This suite owns recognition, whole-token geometry, context suppression, and
//! malformed delimited forms. Parser AST structure and regex semantics remain
//! separate contracts under #6692 and #2075. Whitespace-separated `s` forms are
//! deliberately excluded until the production lexer admits them under #6723.
//! Bare operator words without delimiters are also excluded: production still
//! falls back to identifiers where Perl commits to a malformed quote-like form.

use perl_lexer::{PerlLexer, Token, TokenType};

type R<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[derive(Debug, Clone, Copy)]
enum ExpectedKind {
    QuoteSingle,
    QuoteDouble,
    QuoteWords,
    QuoteCommand,
    QuoteRegex,
    RegexMatch,
    Substitution,
    Transliteration,
}

impl ExpectedKind {
    fn matches(self, token_type: &TokenType) -> bool {
        match self {
            Self::QuoteSingle => matches!(token_type, TokenType::QuoteSingle),
            Self::QuoteDouble => matches!(token_type, TokenType::QuoteDouble),
            Self::QuoteWords => matches!(token_type, TokenType::QuoteWords),
            Self::QuoteCommand => matches!(token_type, TokenType::QuoteCommand),
            Self::QuoteRegex => matches!(token_type, TokenType::QuoteRegex),
            Self::RegexMatch => matches!(token_type, TokenType::RegexMatch),
            Self::Substitution => matches!(token_type, TokenType::Substitution),
            Self::Transliteration => matches!(token_type, TokenType::Transliteration),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum SuppressedKind {
    Identifier,
    Keyword,
}

fn missing(message: impl Into<String>) -> Box<dyn std::error::Error> {
    std::io::Error::other(message.into()).into()
}

fn next(lexer: &mut PerlLexer<'_>, message: &'static str) -> R<Token> {
    lexer.next_token().ok_or_else(|| missing(message))
}

fn collect(input: &str) -> Vec<Token> {
    PerlLexer::new(input).collect_tokens()
}

fn is_quote_like_family(token_type: &TokenType) -> bool {
    matches!(
        token_type,
        TokenType::QuoteSingle
            | TokenType::QuoteDouble
            | TokenType::QuoteWords
            | TokenType::QuoteCommand
            | TokenType::QuoteRegex
            | TokenType::RegexMatch
            | TokenType::Substitution
            | TokenType::Transliteration
    )
}

fn assert_suppressed_token(
    source: &str,
    tokens: &[Token],
    text: &str,
    expected: SuppressedKind,
) -> R {
    let matches = tokens.iter().filter(|token| token.text.as_ref() == text).collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err(missing(format!(
            "expected one suppressed {text:?} token in {source:?}, got {}",
            matches.len()
        )));
    }
    let token = matches[0];
    match expected {
        SuppressedKind::Identifier => assert!(
            matches!(&token.token_type, TokenType::Identifier(name) if name.as_ref() == text),
            "suppressed {text:?} had type {:?}",
            token.token_type
        ),
        SuppressedKind::Keyword => assert!(
            matches!(&token.token_type, TokenType::Keyword(name) if name.as_ref() == text),
            "suppressed {text:?} had type {:?}",
            token.token_type
        ),
    }
    assert_eq!(source.get(token.start..token.end), Some(text));
    assert!(
        tokens.iter().all(|candidate| {
            !is_quote_like_family(&candidate.token_type)
                && !candidate.token_type.is_recovery_token()
        }),
        "suppressed context emitted a quote-like or recovery token for {source:?}: {tokens:?}"
    );
    assert!(matches!(tokens.last().map(|token| &token.token_type), Some(TokenType::EOF)));
    Ok(())
}

#[test]
fn every_operator_family_owns_exact_whole_token_geometry_and_resumes() -> R {
    let cases = [
        ("q{literal}", ExpectedKind::QuoteSingle),
        (r"q|literal\|tail|", ExpectedKind::QuoteSingle),
        ("q#literal#", ExpectedKind::QuoteSingle),
        ("qq[hello $name]", ExpectedKind::QuoteDouble),
        ("qw(foo bar)", ExpectedKind::QuoteWords),
        ("qx<echo hi>", ExpectedKind::QuoteCommand),
        ("qr/pat+/imsx", ExpectedKind::QuoteRegex),
        ("m|pat|", ExpectedKind::RegexMatch),
        ("s/a/b/g", ExpectedKind::Substitution),
        ("s'foo'bar'", ExpectedKind::Substitution),
        ("s[old][new]", ExpectedKind::Substitution),
        ("tr/a-z/A-Z/cd", ExpectedKind::Transliteration),
        ("tr{a-z}{A-Z}", ExpectedKind::Transliteration),
        ("y<a-z><A-Z>", ExpectedKind::Transliteration),
        ("s{a{b}}{c{d}}g", ExpectedKind::Substitution),
        ("q {spaced}", ExpectedKind::QuoteSingle),
        ("m /spaced/", ExpectedKind::RegexMatch),
        ("tr [a-z] [A-Z]", ExpectedKind::Transliteration),
    ];

    for (lexeme, expected) in cases {
        let source = format!("{lexeme}; after");
        let mut lexer = PerlLexer::new(&source);
        let token = next(&mut lexer, "missing quote-like token")?;

        assert!(
            expected.matches(&token.token_type),
            "{lexeme:?} expected {expected:?}, got {:?}",
            token.token_type
        );
        assert_eq!(token.text.as_ref(), lexeme, "whole-token text for {lexeme:?}");
        assert_eq!((token.start, token.end), (0, lexeme.len()));
        assert!(source.is_char_boundary(token.end));

        let separator = next(&mut lexer, "missing token after quote-like operator")?;
        assert!(matches!(&separator.token_type, TokenType::Semicolon));
        assert_eq!((separator.start, separator.end), (lexeme.len(), lexeme.len() + 1));

        let resumed = next(&mut lexer, "missing identifier after quote-like operator")?;
        assert!(
            matches!(&resumed.token_type, TokenType::Identifier(name) if name.as_ref() == "after"),
            "{lexeme:?} left stale state before the following identifier: {:?}",
            resumed.token_type
        );
        assert_eq!(
            (resumed.start, resumed.end),
            (lexeme.len() + 2, lexeme.len() + 2 + "after".len())
        );
        assert_eq!(source.get(resumed.start..resumed.end), Some("after"));

        let eof = next(&mut lexer, "missing EOF after resumed identifier")?;
        assert!(matches!(&eof.token_type, TokenType::EOF));
        assert_eq!((eof.start, eof.end), (source.len(), source.len()));
        assert!(lexer.next_token().is_none());
    }
    Ok(())
}

#[test]
fn hash_keys_methods_fat_arrows_and_file_tests_have_exact_suppressed_identity() -> R {
    let hash_source = "$h{q} + $h{qq} + $h{m} + $h{s} + $h{tr} + $h{y};";
    let hash_tokens = collect(hash_source);
    for name in ["q", "qq"] {
        assert_suppressed_token(hash_source, &hash_tokens, name, SuppressedKind::Identifier)?;
    }
    for name in ["m", "s", "tr", "y"] {
        assert_suppressed_token(hash_source, &hash_tokens, name, SuppressedKind::Keyword)?;
    }

    for (source, name) in [
        ("$obj->q();", "q"),
        ("$obj->qq();", "qq"),
        ("$obj->qw();", "qw"),
        ("$obj->qx();", "qx"),
        ("$obj->qr();", "qr"),
        ("$obj->m();", "m"),
        ("$obj->s();", "s"),
        ("$obj->tr();", "tr"),
        ("$obj->y();", "y"),
    ] {
        assert_suppressed_token(source, &collect(source), name, SuppressedKind::Keyword)?;
    }

    for (source, name) in [
        ("q => 1;", "q"),
        ("qq => 1;", "qq"),
        ("qw => 1;", "qw"),
        ("qx => 1;", "qx"),
        ("qr => 1;", "qr"),
        ("m => 1;", "m"),
        ("s => 1;", "s"),
        ("tr => 1;", "tr"),
        ("y => 1;", "y"),
    ] {
        assert_suppressed_token(source, &collect(source), name, SuppressedKind::Identifier)?;
    }

    let file_test = "-s 'path';";
    assert_suppressed_token(file_test, &collect(file_test), "s", SuppressedKind::Identifier)?;
    Ok(())
}

#[test]
fn quote_words_remains_enabled_inside_a_hash_slice() -> R {
    let source = "@h{qw/a b c/};";
    let tokens = collect(source);
    let quote = tokens
        .iter()
        .find(|token| token.text.as_ref() == "qw/a b c/")
        .ok_or_else(|| missing("missing quote-words token inside hash slice"))?;
    assert!(matches!(&quote.token_type, TokenType::QuoteWords));
    assert_eq!(source.get(quote.start..quote.end), Some("qw/a b c/"));
    assert!(!tokens.iter().any(|token| matches!(&token.token_type, TokenType::Division)));
    Ok(())
}

#[test]
fn malformed_delimited_forms_emit_one_source_anchored_error_then_eof() -> R {
    for source in [
        "q{unterminated",
        "qq[unterminated",
        "m/pattern",
        "s/a/",
        "s{a}{unterminated",
        "tr/a-z/",
        "y[a-z][A-Z",
    ] {
        let mut lexer = PerlLexer::new(source);
        let token = next(&mut lexer, "missing malformed quote-like token")?;
        assert!(
            matches!(&token.token_type, TokenType::Error(_)),
            "malformed {source:?} must emit Error, got {:?}",
            token.token_type
        );
        assert_eq!(token.text.as_ref(), source);
        assert_eq!((token.start, token.end), (0, source.len()));

        let eof = next(&mut lexer, "missing EOF after malformed quote-like token")?;
        assert!(matches!(&eof.token_type, TokenType::EOF));
        assert_eq!((eof.start, eof.end), (source.len(), source.len()));
        assert!(lexer.next_token().is_none());
    }
    Ok(())
}

#[test]
fn multiline_unicode_and_all_line_endings_preserve_byte_extent() -> R {
    for lexeme in ["qq{\n café $name\n}", "qq{\r\n café $name\r\n}", "qq{\r café $name\r}"] {
        let mut lexer = PerlLexer::new(lexeme);
        let token = next(&mut lexer, "missing multiline quote token")?;
        assert!(matches!(&token.token_type, TokenType::QuoteDouble));
        assert_eq!(token.text.as_ref(), lexeme);
        assert_eq!((token.start, token.end), (0, lexeme.len()));
        assert!(lexeme.is_char_boundary(token.end));
    }
    Ok(())
}
