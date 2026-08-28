//! Regression contract for #7279: `tr`/`y` comment gaps must not consume following code.

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
    assert_eq!(
        transliteration.end,
        expected_text.len(),
        "wrong end for {source:?}"
    );

    let semicolon = next_non_trivia(&mut lexer).ok_or("expected semicolon after transliteration")?;
    assert_eq!(
        semicolon.token_type,
        TokenType::Semicolon,
        "missing semicolon boundary for {source:?}"
    );
    assert_eq!(semicolon.text.as_ref(), ";");

    let following = next_non_trivia(&mut lexer).ok_or("expected following identifier")?;
    assert_eq!(
        following.token_type,
        TokenType::Identifier,
        "following source was reframed for {source:?}"
    );
    assert_eq!(following.text.as_ref(), "after");
    assert_eq!(following.end, source.len());

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
    for (source, expected_text) in [
        ("tr#a#b#; after", "tr#a#b#"),
        ("y#a#b#; after", "y#a#b#"),
    ] {
        assert_transliteration_then_after(source, expected_text)?;
    }

    Ok(())
}
