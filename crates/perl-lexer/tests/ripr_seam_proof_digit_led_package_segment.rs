//! RIPR call-observation proof for digit-led package segments (#16640).
//!
//! Seams:
//! | File | Line | Expression |
//! |------|------|-----------|
//! | `src/unicode.rs` | 75 | `is_perl_identifier_start(ch) \|\| ch.is_ascii_digit()` |
//! | `src/lib.rs` | 1161 | `quote_op_word_start.is_some_and(is_quote_op_word_prefix)` |
//! | `src/lib.rs` | 1206 | `!is_perl_package_segment_start(ch)` |
//!
//! Each test drives the public `perl_lexer` token stream so a mutation at the
//! named seam flips the observed token class.

use perl_lexer::{PerlLexer, Token, TokenType};

fn significant(source: &str) -> Vec<Token> {
    PerlLexer::new(source)
        .collect_tokens()
        .into_iter()
        .filter(|token| {
            !matches!(token.token_type, TokenType::Whitespace | TokenType::Newline | TokenType::EOF)
        })
        .collect()
}

fn first_kind(source: &str) -> String {
    match significant(source).into_iter().next() {
        Some(token) => match token.token_type {
            TokenType::Identifier(text) => format!("id:{text}"),
            TokenType::Number(text) => format!("num:{text}"),
            TokenType::QuoteSingle => format!("qsingle:{}", token.text),
            TokenType::QuoteDouble(_) => format!("qdouble:{}", token.text),
            TokenType::Substitution => format!("subst:{}", token.text),
            TokenType::Keyword(text) => format!("kw:{text}"),
            TokenType::Operator(text) => format!("op:{text}"),
            other => format!("{other:?}"),
        },
        None => "none".to_string(),
    }
}

/// RIPR `consume_identifier_segment_tail` call-presence observer (`lib.rs:1161`).
///
/// First-segment `q'…'` must still split at the quote-op apostrophe. A
/// trailing `::` segment passes `quote_op_word_start = None`, so the same
/// helper must *not* treat `'` as a quote-op delimiter there.
#[test]
fn consume_identifier_segment_tail_call_presence_observer() {
    assert_eq!(
        first_kind("q'foo'"),
        "qsingle:q'foo'",
        "first-segment quote-op must hit is_quote_op_word_prefix and split at '"
    );
    assert_eq!(
        first_kind("Foo'Bar"),
        "id:Foo'Bar",
        "legacy apostrophe package must keep one identifier when the prefix is not a quote-op"
    );
    assert_eq!(
        first_kind("Foo::2022_KR'Bar"),
        "id:Foo::2022_KR'Bar",
        "after :: the extracted tail helper must not apply quote-op splitting"
    );
}

/// RIPR `!is_perl_package_segment_start(ch)` observer (`lib.rs:1206`).
#[test]
fn consume_trailing_package_segments_rejects_non_start_after_colon_colon() {
    let combining = significant("Foo::\u{0301}Bar");
    let texts: Vec<String> = combining.iter().map(|token| token.text.to_string()).collect();
    assert!(
        texts.iter().all(|text| !text.contains('\u{0301}')),
        "combining mark after :: must take the !segment-start break, got {texts:?}"
    );

    let spaced = significant("Foo:: 2022_KR");
    assert!(
        spaced.iter().any(
            |token| matches!(&token.token_type, TokenType::Number(text) if text.as_ref() == "2022_")
        ),
        "whitespace after :: is not a segment start; digits stay Number"
    );
}

/// RIPR ident-start vs ASCII-digit disjuncts via the public token stream.
#[test]
fn is_perl_package_segment_start_token_stream_observer() {
    assert_eq!(first_kind("Encode::KR::2022_KR"), "id:Encode::KR::2022_KR");
    assert_eq!(first_kind("Foo::1"), "id:Foo::1");
    assert_eq!(first_kind("Foo::Bar"), "id:Foo::Bar");
    assert!(
        first_kind("2022_KR").starts_with("num:"),
        "first-segment digits must miss package-segment start, got {}",
        first_kind("2022_KR")
    );
}
