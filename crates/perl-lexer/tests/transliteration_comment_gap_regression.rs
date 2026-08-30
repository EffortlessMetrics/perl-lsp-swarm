//! Regression contract for #13915: `tr`/`y` comment gaps must not consume following code.

use perl_lexer::{PerlLexer, TokenType};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn next_non_trivia(lexer: &mut PerlLexer<'_>) -> Option<perl_lexer::Token> {
    loop {
        let token = lexer.next_token()?;
        if !matches!(token.token_type, TokenType::Whitespace | TokenType::Newline) {
            return Some(token);
        }
    }
}

fn assert_transliteration_then_after(source: &str, expected_text: &str) -> TestResult {
    let mut lexer = PerlLexer::new(source);

    let transliteration = next_non_trivia(&mut lexer).ok_or("expected transliteration token")?;
    assert_eq!(
        transliteration.token_type,
        TokenType::Transliteration,
        "wrong first token for {source:?}"
    );
    assert_eq!(
        transliteration.text.as_ref(),
        expected_text,
        "wrong transliteration text for {source:?}"
    );
    assert_eq!(transliteration.start, 0, "wrong start for {source:?}");
    assert_eq!(transliteration.end, expected_text.len(), "wrong end for {source:?}");

    let semicolon =
        next_non_trivia(&mut lexer).ok_or("expected semicolon after transliteration")?;
    assert_eq!(
        semicolon.token_type,
        TokenType::Semicolon,
        "missing semicolon boundary for {source:?}"
    );
    assert_eq!(semicolon.text.as_ref(), ";");
    assert_eq!(semicolon.start, expected_text.len());
    assert_eq!(semicolon.end, expected_text.len() + 1);

    let following = next_non_trivia(&mut lexer).ok_or("expected following identifier")?;
    assert!(
        matches!(
            &following.token_type,
            TokenType::Identifier(identifier) if identifier.as_ref() == "after"
        ),
        "following source was reframed for {source:?}: {:?}",
        following.token_type
    );
    assert_eq!(following.text.as_ref(), "after");
    assert_eq!(following.start, source.len() - "after".len());
    assert_eq!(following.end, source.len());

    let eof = next_non_trivia(&mut lexer).ok_or("expected EOF after following identifier")?;
    assert_eq!(eof.token_type, TokenType::EOF);
    assert_eq!(eof.start, source.len());
    assert_eq!(eof.end, source.len());

    Ok(())
}

#[test]
fn comment_gap_before_first_paired_body_is_accepted_for_tr_and_y() -> TestResult {
    for (source, expected_text) in [
        ("tr # comment\n {a} {b}; after", "tr # comment\n {a} {b}"),
        ("y # comment\n {a} {b}; after", "y # comment\n {a} {b}"),
    ] {
        assert_transliteration_then_after(source, expected_text)?;
    }

    Ok(())
}

#[test]
fn comment_gap_between_paired_bodies_is_accepted_for_tr_and_y() -> TestResult {
    for (source, expected_text) in [
        ("tr{a} # comment\n {b}; after", "tr{a} # comment\n {b}"),
        ("y{a} # comment\n {b}; after", "y{a} # comment\n {b}"),
    ] {
        assert_transliteration_then_after(source, expected_text)?;
    }

    Ok(())
}

#[test]
fn immediate_hash_remains_a_transliteration_delimiter() -> TestResult {
    for (source, expected_text) in [("tr#a#b#; after", "tr#a#b#"), ("y#a#b#; after", "y#a#b#")] {
        assert_transliteration_then_after(source, expected_text)?;
    }

    Ok(())
}

#[test]
fn unicode_whitespace_does_not_create_a_comment_gap() -> TestResult {
    for whitespace in ['\u{a0}', '\u{2003}'] {
        for operator in ["tr", "y"] {
            let source = format!("{operator}{whitespace}# comment\n {{a}} {{b}}; after");
            let mut lexer = PerlLexer::new(&source);
            let token = next_non_trivia(&mut lexer).ok_or("expected token")?;
            assert!(
                matches!(token.token_type, TokenType::Error(_)),
                "Unicode whitespace must not admit a comment gap: {source:?}"
            );

            let source = format!("{operator}{{a}}{whitespace}# comment\n {{b}}; after");
            let mut lexer = PerlLexer::new(&source);
            let token = next_non_trivia(&mut lexer).ok_or("expected second-boundary token")?;
            assert!(
                matches!(token.token_type, TokenType::Error(_)),
                "Unicode whitespace must not admit a second-body comment gap: {source:?}"
            );

            let source = format!(
                "{operator}{{a}} {unicode_whitespace}# comment\n {{b}}; after",
                unicode_whitespace = '\u{a0}'
            );
            let mut lexer = PerlLexer::new(&source);
            let token =
                next_non_trivia(&mut lexer).ok_or("expected mixed second-boundary token")?;
            assert!(
                matches!(token.token_type, TokenType::Error(_)),
                "mixed ASCII/Unicode whitespace must not admit a second-body comment gap: {source:?}"
            );
        }
    }

    Ok(())
}
