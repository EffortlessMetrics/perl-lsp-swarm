//! Lexer-owned matrix for currently supported Perl quote-like operators.
//!
//! This suite owns recognition, whole-token geometry, context suppression, and
//! malformed terminal behavior. Parser AST structure and regex semantics remain
//! separate contracts under #6692 and #2075. Forms without a reviewed Perl
//! version oracle are deliberately not promoted into this matrix.

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

fn missing(message: &'static str) -> Box<dyn std::error::Error> {
    std::io::Error::other(message).into()
}

fn next(lexer: &mut PerlLexer<'_>, message: &'static str) -> R<Token> {
    lexer.next_token().ok_or_else(|| missing(message))
}

fn collect(input: &str) -> Vec<Token> {
    PerlLexer::new(input).collect_tokens()
}

fn is_quote_like(token_type: &TokenType) -> bool {
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

#[test]
fn every_operator_family_owns_exact_whole_token_geometry() -> R {
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
        ("s {old} {new}", ExpectedKind::Substitution),
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
        assert_eq!(token.start, 0, "whole-token start for {lexeme:?}");
        assert_eq!(token.end, lexeme.len(), "whole-token end for {lexeme:?}");
        assert!(source.is_char_boundary(token.end));

        let separator = next(&mut lexer, "missing token after quote-like operator")?;
        assert!(matches!(&separator.token_type, TokenType::Semicolon));
        assert_eq!(separator.start, lexeme.len());
        assert_eq!(separator.end, lexeme.len() + 1);
    }
    Ok(())
}

#[test]
fn bare_operator_names_remain_identifiers_without_a_delimiter() -> R {
    for name in ["q", "qq", "qw", "qx", "qr", "m", "s", "tr", "y"] {
        let mut lexer = PerlLexer::new(name);
        let token = next(&mut lexer, "missing bare operator-name token")?;
        assert!(
            matches!(&token.token_type, TokenType::Identifier(identifier) if identifier.as_ref() == name),
            "bare {name:?} must remain an identifier, got {:?}",
            token.token_type
        );
        assert_eq!(token.start, 0);
        assert_eq!(token.end, name.len());
    }
    Ok(())
}

#[test]
fn hash_keys_methods_fat_arrows_and_file_tests_suppress_quote_operators() {
    let contexts = [
        "$h{q} + $h{qq} + $h{m} + $h{s} + $h{tr} + $h{y};",
        "$obj->q(); $obj->m(); $obj->s(); $obj->tr(); $obj->y();",
        "q => 1; s => 2; tr => 3; y => 4;",
        "-s 'path';",
    ];

    for source in contexts {
        let tokens = collect(source);
        assert!(
            !tokens.iter().any(|token| is_quote_like(&token.token_type)),
            "context must suppress quote-like promotion for {source:?}: {tokens:?}"
        );
    }
}

#[test]
fn quote_words_remains_enabled_inside_a_hash_slice() {
    let tokens = collect("@h{qw/a b c/};");
    assert!(tokens.iter().any(|token| matches!(&token.token_type, TokenType::QuoteWords)));
    assert!(!tokens.iter().any(|token| matches!(&token.token_type, TokenType::Division)));
}

#[test]
fn spaced_slash_is_not_a_substitution_delimiter() -> R {
    let mut lexer = PerlLexer::new("s /old/new/");
    let first = next(&mut lexer, "missing spaced-s token")?;
    assert!(
        matches!(&first.token_type, TokenType::Identifier(identifier) if identifier.as_ref() == "s")
    );
    assert!(!matches!(&first.token_type, TokenType::Substitution));
    Ok(())
}

#[test]
fn malformed_forms_emit_one_source_anchored_error_then_eof() -> R {
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
        assert_eq!(token.start, 0);
        assert_eq!(token.end, source.len());

        let eof = next(&mut lexer, "missing EOF after malformed quote-like token")?;
        assert!(matches!(&eof.token_type, TokenType::EOF));
        assert_eq!(eof.start, source.len());
        assert_eq!(eof.end, source.len());
        assert!(lexer.next_token().is_none());
    }
    Ok(())
}

#[test]
fn multiline_unicode_and_all_line_endings_preserve_byte_extent() -> R {
    for lexeme in [
        "qq{\n café $name\n}",
        "qq{\r\n café $name\r\n}",
        "qq{\r café $name\r}",
    ] {
        let mut lexer = PerlLexer::new(lexeme);
        let token = next(&mut lexer, "missing multiline quote token")?;
        assert!(matches!(&token.token_type, TokenType::QuoteDouble));
        assert_eq!(token.text.as_ref(), lexeme);
        assert_eq!(token.start, 0);
        assert_eq!(token.end, lexeme.len());
        assert!(lexeme.is_char_boundary(token.end));
    }
    Ok(())
}
