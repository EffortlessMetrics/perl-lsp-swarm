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

#[test]
fn arrow_call_hashref_arg_still_lexes_substitution() {
    // `$cb->({ value => s/foo/bar/r })` is a hash constructor argument, not
    // `->{`. `(` must consume arrow state so `{` does not open subscript depth.
    let toks = significant("$cb->({ value => s/foo/bar/r })");
    assert!(
        has_substitution(&toks),
        "coderef hashref arg must keep s/// as substitution, got {toks:?}"
    );
}

#[test]
fn chained_hash_after_arrow_array_deref_is_not_transliteration() {
    // `->[` consumes arrow state; `]` still arms `after_var_subscript` so
    // `$h->[0]{y}` remains a chained hash key.
    let toks = significant("$h->[0]{y}");
    assert!(
        !has_transliteration(&toks),
        "chained {{y}} after ->[0] must stay a hash key, got {toks:?}"
    );
}

#[test]
fn computed_arrow_key_substitution_matches_direct_subscript() {
    // Direct `$h{scalar s/foo/bar/r}` already uses hash_brace_depth. Arrow
    // `->{...}` now uses the same opener; both should agree.
    let direct = significant("$h{scalar s/foo/bar/r}");
    let arrow = significant("$h->{scalar s/foo/bar/r}");
    assert_eq!(
        has_substitution(&direct),
        has_substitution(&arrow),
        "arrow computed key must match direct `$h{{...}}` quote-op policy; direct={direct:?} arrow={arrow:?}"
    );
}

#[test]
fn nested_do_block_key_then_chained_y_direct_and_arrow() {
    // Nested uncounted block closers stealing hash_brace_depth is a pre-existing
    // `$h{{do {{1}}}}{{y}}` limitation, not unique to `->`.
    let direct = significant("$h{do { 1 }}{y}");
    let arrow = significant("$h->{do { 1 }}{y}");
    assert_eq!(
        has_transliteration(&direct),
        has_transliteration(&arrow),
        "nested do-block key chaining must not be worse for arrow than for `$h`; direct={direct:?} arrow={arrow:?}"
    );
}
