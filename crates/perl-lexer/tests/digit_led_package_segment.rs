//! Digit-led package segments after `::` (#16640).
//!
//! Perl admits `Encode::KR::2022_KR` as one qualified name. The first
//! segment of a bareword still cannot start with a digit, and a number
//! after whitespace remains a number (including `package` VERSION).
//! Oracle: perl 5.38+ `-ce` on this host.

use perl_lexer::{PerlLexer, Token, TokenType};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn significant_tokens(source: &str) -> Vec<Token> {
    PerlLexer::new(source)
        .collect_tokens()
        .into_iter()
        .filter(|token| {
            !matches!(token.token_type, TokenType::Whitespace | TokenType::Newline | TokenType::EOF)
        })
        .collect()
}

fn identifier_texts(source: &str) -> Vec<String> {
    significant_tokens(source)
        .into_iter()
        .filter_map(|token| match token.token_type {
            TokenType::Identifier(text) => Some(text.to_string()),
            _ => None,
        })
        .collect()
}

fn has_number_text(source: &str, expected: &str) -> bool {
    significant_tokens(source).iter().any(
        |token| matches!(&token.token_type, TokenType::Number(text) if text.as_ref() == expected),
    )
}

#[test]
fn encode_kr_2022_kr_is_one_qualified_identifier() -> TestResult {
    let texts = identifier_texts("Encode::KR::2022_KR");
    assert_eq!(
        texts,
        vec!["Encode::KR::2022_KR".to_string()],
        "digit-led segment after :: must fold into the qualified identifier, got {texts:?}"
    );
    assert!(
        !has_number_text("Encode::KR::2022_KR", "2022_"),
        "2022_KR after :: must not be lexed as Number(\"2022_\")"
    );
    Ok(())
}

#[test]
fn package_statement_keeps_digit_led_name_out_of_the_number_class() -> TestResult {
    let source = "package Encode::KR::2022_KR;";
    let texts = identifier_texts(source);
    assert!(
        texts.iter().any(|text| text == "Encode::KR::2022_KR"),
        "package name must stay one identifier, got {texts:?}"
    );
    assert!(
        !significant_tokens(source)
            .iter()
            .any(|token| matches!(token.token_type, TokenType::Number(_))),
        "package Encode::KR::2022_KR must not emit a Number token"
    );
    Ok(())
}

#[test]
fn pure_digit_segment_after_colon_colon_is_an_identifier() -> TestResult {
    assert_eq!(identifier_texts("Foo::1"), vec!["Foo::1".to_string()]);
    assert_eq!(identifier_texts("Foo::2022_KR::Bar"), vec!["Foo::2022_KR::Bar".to_string()]);
    Ok(())
}

#[test]
fn leading_colon_colon_digit_segment_is_an_identifier() -> TestResult {
    let tokens = significant_tokens("package ::2022_KR;");
    let kinds: Vec<String> = tokens
        .iter()
        .map(|token| match &token.token_type {
            TokenType::Keyword(text) => format!("kw:{text}"),
            TokenType::Operator(text) => format!("op:{text}"),
            TokenType::Identifier(text) => format!("id:{text}"),
            TokenType::Number(text) => format!("num:{text}"),
            TokenType::Semicolon => "semi".to_string(),
            other => format!("{other:?}"),
        })
        .collect();
    assert_eq!(
        kinds,
        vec![
            "kw:package".to_string(),
            "op:::".to_string(),
            "id:2022_KR".to_string(),
            "semi".to_string()
        ],
        "leading :: must leave a digit-led identifier, got {kinds:?}"
    );
    Ok(())
}

#[test]
fn dotted_tail_after_digit_segment_is_not_swallowed() -> TestResult {
    // perl -ce 'package Foo::1.2;' is a syntax error: name is Foo::1, then
    // `.2` is an invalid VERSION. The lexer must stop the segment at the dot.
    let tokens = significant_tokens("Foo::1.2");
    let texts: Vec<String> = tokens.iter().map(|token| token.text.to_string()).collect();
    assert_eq!(
        texts,
        vec!["Foo::1".to_string(), ".".to_string(), "2".to_string()],
        "dot after a digit-led segment must remain a separate token, got {texts:?}"
    );
    Ok(())
}

#[test]
fn digit_led_word_without_colon_colon_stays_a_number() -> TestResult {
    // Opposite-direction control: `2022_KR` is not a bare identifier.
    // The number scanner consumes the digit/underscore run, then `KR`.
    let tokens = significant_tokens("2022_KR");
    assert!(
        matches!(&tokens[0].token_type, TokenType::Number(_)),
        "a leading-digit word must stay a number, got {:?}",
        tokens[0].token_type
    );
    assert!(
        !identifier_texts("2022_KR").iter().any(|text| text == "2022_KR"),
        "must not invent a first-segment digit-led identifier"
    );
    Ok(())
}

#[test]
fn package_version_after_letter_name_stays_a_number() -> TestResult {
    let source = "package Foo::Bar 1.23;";
    assert!(
        identifier_texts(source).iter().any(|text| text == "Foo::Bar"),
        "ordinary qualified name must still merge"
    );
    assert!(has_number_text(source, "1.23"), "VERSION after a completed name must remain a Number");
    Ok(())
}

#[test]
fn whitespace_after_colon_colon_does_not_promote_a_number() -> TestResult {
    // perl -ce 'package Foo:: 2022_KR;' is a syntax error: the digits are
    // VERSION, not a name segment. Adjacent-`::` promotion must not cross
    // whitespace.
    let source = "package Foo:: 2022_KR;";
    assert!(
        has_number_text(source, "2022_") || has_number_text(source, "2022"),
        "digits after `::` + space must stay numeric, tokens={:?}",
        significant_tokens(source).iter().map(|token| token.text.to_string()).collect::<Vec<_>>()
    );
    assert!(
        !identifier_texts(source).iter().any(|text| text.contains("2022_KR")),
        "must not fold a spaced digit run into the package name"
    );
    Ok(())
}
