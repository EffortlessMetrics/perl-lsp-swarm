//! Lexer proof for #16641: `->{a}{y}` is a chained hash subscript, not `y}...}`.
//!
//! `->{` must open hash-subscript brace depth so the closing `}` can arm
//! `after_var_subscript` for a following `{y}` / `{s}` / `{m}` / `{tr}` key.
//! Direct `$h{a}{y}` and first-key `$h->{y}` are controls. Real `y///` after
//! the subscript must stay quote-ops. This is not angle/heredoc work.

use perl_lexer::{PerlLexer, Token, TokenType};

fn significant(input: &str) -> Vec<Token> {
    PerlLexer::new(input)
        .collect_tokens()
        .into_iter()
        .filter(|t| {
            !matches!(t.token_type, TokenType::Whitespace | TokenType::Newline | TokenType::EOF)
        })
        .collect()
}

fn has_transliteration(tokens: &[Token]) -> bool {
    tokens.iter().any(|t| matches!(t.token_type, TokenType::Transliteration))
}

fn has_substitution(tokens: &[Token]) -> bool {
    tokens.iter().any(|t| matches!(t.token_type, TokenType::Substitution))
}

fn has_regex_match(tokens: &[Token]) -> bool {
    tokens.iter().any(|t| matches!(t.token_type, TokenType::RegexMatch))
}

#[test]
fn chained_arrow_hash_key_y_is_not_transliteration() {
    let toks = significant("$h->{a}{y}");
    assert!(
        !has_transliteration(&toks),
        "chained {{y}} after -> must stay a hash key, got {toks:?}"
    );
    assert!(toks.iter().any(|t| t.text.as_ref() == "y"), "expected identifier y in {toks:?}");
}

#[test]
fn chained_arrow_hash_key_s_is_not_substitution() {
    let toks = significant("$h->{a}{s}");
    assert!(!has_substitution(&toks), "chained {{s}} after -> must stay a hash key, got {toks:?}");
}

#[test]
fn chained_arrow_hash_key_m_is_not_match() {
    let toks = significant("$h->{a}{m}");
    assert!(!has_regex_match(&toks), "chained {{m}} after -> must stay a hash key, got {toks:?}");
}

#[test]
fn first_arrow_hash_key_y_control_is_not_transliteration() {
    let toks = significant("$h->{y}");
    assert!(
        !has_transliteration(&toks),
        "first-key ->{{y}} is already after_arrow-protected, got {toks:?}"
    );
}

#[test]
fn direct_chained_hash_key_y_control_is_not_transliteration() {
    let toks = significant("$h{a}{y}");
    assert!(
        !has_transliteration(&toks),
        "direct $h{{a}}{{y}} already tracks brace depth, got {toks:?}"
    );
}

#[test]
fn real_transliteration_after_chained_subscript_still_lexes() {
    let toks = significant("$h->{a}{b}; y/a/b/;");
    assert!(
        has_transliteration(&toks),
        "statement-level `y///` after a completed subscript must remain transliteration, got {toks:?}"
    );
}

#[test]
fn real_substitution_after_chained_subscript_still_lexes() {
    let toks = significant("$h->{a}{b}; s/a/b/;");
    assert!(
        has_substitution(&toks),
        "statement-level `s///` after a completed subscript must remain substitution, got {toks:?}"
    );
}

#[test]
fn corpus_plus_line_chained_y_is_not_transliteration() {
    let src = r#"return $params->{named}{x} + $params->{named}{y};"#;
    let toks = significant(src);
    assert!(
        !has_transliteration(&toks),
        "corpus line 80 chained {{y}} must be a hash key, got {toks:?}"
    );
}
