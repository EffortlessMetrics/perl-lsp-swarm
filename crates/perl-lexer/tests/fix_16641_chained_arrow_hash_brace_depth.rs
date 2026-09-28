//! Lexer proof for #16641: `->{a}{y}` is a chained hash subscript, not `y}...}`.
//!
//! `->{` must open hash-subscript brace depth so the closing `}` can arm
//! `after_var_subscript` for a following `{y}` / `{s}` / `{m}` / `{tr}` key.
//! Direct `$h{a}{y}` and first-key `$h->{y}` are controls. Real `y///` after
//! the subscript must stay transliteration. This is not angle/heredoc work.

use perl_lexer::{PerlLexer, TokenType};

fn significant(input: &str) -> Vec<perl_lexer::Token> {
    PerlLexer::new(input)
        .collect_tokens()
        .into_iter()
        .filter(|t| {
            !matches!(t.token_type, TokenType::Whitespace | TokenType::Newline | TokenType::EOF)
        })
        .collect()
}

fn has_transliteration(input: &str) -> bool {
    significant(input).iter().any(|t| matches!(t.token_type, TokenType::Transliteration))
}

fn has_substitution(input: &str) -> bool {
    significant(input).iter().any(|t| matches!(t.token_type, TokenType::Substitution))
}

fn has_regex_match(input: &str) -> bool {
    significant(input).iter().any(|t| matches!(t.token_type, TokenType::RegexMatch))
}

#[test]
fn chained_arrow_hash_key_y_is_not_transliteration() {
    let toks = significant("$h->{a}{y}");
    assert!(
        !has_transliteration("$h->{a}{y}"),
        "chained {{y}} after -> must stay a hash key, got {toks:?}"
    );
    assert!(toks.iter().any(|t| t.text.as_ref() == "y"), "expected identifier y in {toks:?}");
}

#[test]
fn chained_arrow_hash_key_s_is_not_substitution() {
    assert!(
        !has_substitution("$h->{a}{s}"),
        "chained {{s}} after -> must stay a hash key, got {:?}",
        significant("$h->{a}{s}")
    );
}

#[test]
fn chained_arrow_hash_key_m_is_not_match() {
    assert!(
        !has_regex_match("$h->{a}{m}"),
        "chained {{m}} after -> must stay a hash key, got {:?}",
        significant("$h->{a}{m}")
    );
}

#[test]
fn first_arrow_hash_key_y_control_is_not_transliteration() {
    assert!(
        !has_transliteration("$h->{y}"),
        "first-key ->{{y}} is already after_arrow-protected, got {:?}",
        significant("$h->{y}")
    );
}

#[test]
fn direct_chained_hash_key_y_control_is_not_transliteration() {
    assert!(
        !has_transliteration("$h{a}{y}"),
        "direct $h{{a}}{{y}} already tracks brace depth, got {:?}",
        significant("$h{a}{y}")
    );
}

#[test]
fn real_transliteration_after_chained_subscript_still_lexes() {
    assert!(
        has_transliteration("$h->{a}{b}; y/a/b/;"),
        "statement-level `y///` after a completed subscript must remain transliteration, got {:?}",
        significant("$h->{a}{b}; y/a/b/;")
    );
}

#[test]
fn real_substitution_after_chained_subscript_still_lexes() {
    assert!(
        has_substitution("$h->{a}{b}; s/a/b/;"),
        "statement-level `s///` after a completed subscript must remain substitution, got {:?}",
        significant("$h->{a}{b}; s/a/b/;")
    );
}

#[test]
fn corpus_plus_line_chained_y_is_not_transliteration() {
    let src = r#"return $params->{named}{x} + $params->{named}{y};"#;
    assert!(
        !has_transliteration(src),
        "corpus line 80 chained {{y}} must be a hash key, got {:?}",
        significant(src)
    );
}
