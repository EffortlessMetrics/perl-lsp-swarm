//! Tests for fix #2380: correct `]` handling at the start of a regex character class.
//!
//! Per perlre, a `]` as the very first character of a character class (e.g. `[]]`)
//! is a literal `]` and not the class terminator. The class is closed by the
//! following `]`.
//!
//! Before this fix, the lexer exited the class on the first `]` it saw,
//! causing `/[]]./` to be tokenized incorrectly (the `.` and trailing `/` ended up
//! outside the expected regex boundary).

use perl_lexer::{PerlLexer, TokenType};

type TestResult = Result<(), Box<dyn std::error::Error>>;

// ── Acceptance tests (new behaviour) ─────────────────────────────────────────

/// `/[]]./` — character class containing a literal `]`, followed by `.`
/// The whole literal must arrive as one RegexMatch token spanning the full input.
#[test]
fn regex_bracket_close_as_first_class_char_is_literal() -> TestResult {
    let code = "/[]]./";
    let mut lexer = PerlLexer::new(code);
    let tok = lexer.next_token().ok_or("expected regex token")?;
    assert_eq!(tok.token_type, TokenType::RegexMatch, "input: {code}");
    assert_eq!(tok.text.as_ref(), code, "token must span entire regex");
    Ok(())
}

/// `/[]]$/` — class with literal `]`, anchored end.
#[test]
fn regex_bracket_close_as_first_class_char_with_anchor() -> TestResult {
    let code = "/[]]$/";
    let mut lexer = PerlLexer::new(code);
    let tok = lexer.next_token().ok_or("expected regex token")?;
    assert_eq!(tok.token_type, TokenType::RegexMatch, "input: {code}");
    assert_eq!(tok.text.as_ref(), code);
    Ok(())
}

/// `/[]abc]/` — class opening with literal `]` followed by more members.
#[test]
fn regex_bracket_close_followed_by_other_class_members() -> TestResult {
    let code = "/[]abc]/";
    let mut lexer = PerlLexer::new(code);
    let tok = lexer.next_token().ok_or("expected regex token")?;
    assert_eq!(tok.token_type, TokenType::RegexMatch, "input: {code}");
    assert_eq!(tok.text.as_ref(), code);
    Ok(())
}

/// `/[]]$/i` — regex with a modifier after the pattern.
#[test]
fn regex_bracket_close_as_first_class_char_with_modifier() -> TestResult {
    let code = "/[]]$/i";
    let mut lexer = PerlLexer::new(code);
    let tok = lexer.next_token().ok_or("expected regex token")?;
    assert_eq!(tok.token_type, TokenType::RegexMatch, "input: {code}");
    assert_eq!(tok.text.as_ref(), code);
    Ok(())
}

// ── Regression tests (existing behaviour must not change) ────────────────────

/// `/[abc]/` — ordinary character class still produces RegexMatch.
#[test]
fn regex_ordinary_char_class_unchanged() -> TestResult {
    let code = "/[abc]/";
    let mut lexer = PerlLexer::new(code);
    let tok = lexer.next_token().ok_or("expected regex token")?;
    assert_eq!(tok.token_type, TokenType::RegexMatch, "input: {code}");
    assert_eq!(tok.text.as_ref(), code);
    Ok(())
}

/// `/[^abc]/` — negated ordinary class unchanged.
#[test]
fn regex_negated_char_class_unchanged() -> TestResult {
    let code = "/[^abc]/";
    let mut lexer = PerlLexer::new(code);
    let tok = lexer.next_token().ok_or("expected regex token")?;
    assert_eq!(tok.token_type, TokenType::RegexMatch, "input: {code}");
    assert_eq!(tok.text.as_ref(), code);
    Ok(())
}

/// `/[a-z]/` — character range unchanged.
#[test]
fn regex_char_range_unchanged() -> TestResult {
    let code = "/[a-z]/";
    let mut lexer = PerlLexer::new(code);
    let tok = lexer.next_token().ok_or("expected regex token")?;
    assert_eq!(tok.token_type, TokenType::RegexMatch, "input: {code}");
    assert_eq!(tok.text.as_ref(), code);
    Ok(())
}

/// Escaped brackets (`[\[\]]`) still work.
#[test]
fn regex_escaped_brackets_in_class_unchanged() -> TestResult {
    let code = r"/[\[\]]/";
    let mut lexer = PerlLexer::new(code);
    let tok = lexer.next_token().ok_or("expected regex token")?;
    assert_eq!(tok.token_type, TokenType::RegexMatch, "input: {code}");
    assert_eq!(tok.text.as_ref(), code);
    Ok(())
}

/// `/[A-Z0-9]/` — alphanumeric range unchanged.
#[test]
fn regex_alphanumeric_range_unchanged() -> TestResult {
    let code = "/[A-Z0-9]/";
    let mut lexer = PerlLexer::new(code);
    let tok = lexer.next_token().ok_or("expected regex token")?;
    assert_eq!(tok.token_type, TokenType::RegexMatch, "input: {code}");
    assert_eq!(tok.text.as_ref(), code);
    Ok(())
}

/// A regex WITHOUT a character class still terminates at the first `/`.
#[test]
fn regex_without_char_class_unchanged() -> TestResult {
    let code = "/hello/";
    let mut lexer = PerlLexer::new(code);
    let tok = lexer.next_token().ok_or("expected regex token")?;
    assert_eq!(tok.token_type, TokenType::RegexMatch, "input: {code}");
    assert_eq!(tok.text.as_ref(), code);
    Ok(())
}
