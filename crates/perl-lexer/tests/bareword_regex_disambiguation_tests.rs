//! BDD tests for bareword/regex disambiguation via `LocalSymbolTable`.
//!
//! Verifies that the lexer correctly identifies `/` as a regex delimiter (not division)
//! when following a user-declared sub name, and that existing behavior for unknown
//! barewords and builtins is preserved.
//!
//! Issue: #1353 — Parser: Bareword/Regex disambiguation fails without same-file symbol table

use perl_lexer::{LexerConfig, LocalSymbolTable, PerlLexer, TokenType};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn tokens_with_symbol_table(input: &str) -> Vec<TokenType> {
    let symbol_table = LocalSymbolTable::scan_subs(input);
    let config = LexerConfig { symbol_table: Some(symbol_table), ..LexerConfig::default() };
    PerlLexer::with_config(input, config)
        .collect_tokens()
        .into_iter()
        .filter(|t| {
            !matches!(t.token_type, TokenType::Whitespace | TokenType::Newline | TokenType::EOF)
        })
        .map(|t| t.token_type)
        .collect()
}

fn tokens_default(input: &str) -> Vec<TokenType> {
    PerlLexer::new(input)
        .collect_tokens()
        .into_iter()
        .filter(|t| {
            !matches!(t.token_type, TokenType::Whitespace | TokenType::Newline | TokenType::EOF)
        })
        .map(|t| t.token_type)
        .collect()
}

fn has_regex_match(toks: &[TokenType]) -> bool {
    toks.iter().any(|t| matches!(t, TokenType::RegexMatch))
}

fn has_division(toks: &[TokenType]) -> bool {
    toks.iter().any(|t| matches!(t, TokenType::Division))
}

// ---------------------------------------------------------------------------
// Core fix: known sub + `/` → regex, not division
// ---------------------------------------------------------------------------

/// The fundamental case from issue #1353: `sub builder; builder /foo|bar/;`
#[test]
fn known_sub_followed_by_slash_is_regex() {
    let input = "sub my_builder;\nmy_builder /foo|bar/;";
    let toks = tokens_with_symbol_table(input);
    assert!(
        has_regex_match(&toks),
        "slash after a declared sub must be tokenized as regex, got: {:?}",
        toks
    );
    assert!(!has_division(&toks), "must not produce a division token");
}

#[test]
fn known_sub_followed_by_slash_is_regex_definition_style() {
    // `sub` with a body (not just forward decl) should also be collected
    let input = "sub my_filter { return 1; }\nmy_filter /pattern/;";
    let toks = tokens_with_symbol_table(input);
    assert!(has_regex_match(&toks), "slash after defined sub must be regex, got: {:?}", toks);
}

// ---------------------------------------------------------------------------
// Safety check: unknown bareword → division (safe default preserved)
// ---------------------------------------------------------------------------

#[test]
fn unknown_bareword_followed_by_slash_is_division() {
    // `undeclared_func` is not in the symbol table, so `/` is division
    let input = "undeclared_func /foo/;";
    let toks = tokens_with_symbol_table(input);
    assert!(
        has_division(&toks),
        "slash after unknown bareword must remain division, got: {:?}",
        toks
    );
    assert!(!has_regex_match(&toks), "must not produce a regex token");
}

#[test]
fn without_symbol_table_slash_after_bareword_is_division() {
    // When no symbol table is configured, the default behavior is unchanged
    let input = "my_func /foo/;";
    let toks = tokens_default(input);
    assert!(has_division(&toks), "default mode must treat slash as division, got: {:?}", toks);
}

// ---------------------------------------------------------------------------
// Builtin priority: builtins still work regardless of symbol table
// ---------------------------------------------------------------------------

#[test]
fn builtin_function_slash_is_still_regex_with_symbol_table() {
    // `print /regex/` should always be regex — builtin handling must not be broken
    let input = "print /hello/;";
    let toks = tokens_with_symbol_table(input);
    assert!(has_regex_match(&toks), "builtins must still produce regex, got: {:?}", toks);
}

#[test]
fn builtin_takes_precedence_no_symbol_table_needed() {
    let input = "say /pattern/;";
    let toks = tokens_default(input);
    assert!(has_regex_match(&toks), "builtin 'say' must produce regex, got: {:?}", toks);
}

// ---------------------------------------------------------------------------
// Division still works correctly
// ---------------------------------------------------------------------------

#[test]
fn numeric_division_is_preserved_with_symbol_table() {
    let input = "sub foo;\nmy $x = 10 / 2;";
    let toks = tokens_with_symbol_table(input);
    assert!(has_division(&toks), "numeric division must still work, got: {:?}", toks);
}

#[test]
fn variable_followed_by_slash_is_division() {
    // Variable is a term, so `/` after it is division
    let input = "sub foo;\nmy $r = $x / $y;";
    let toks = tokens_with_symbol_table(input);
    assert!(has_division(&toks), "division after variable must work, got: {:?}", toks);
}

// ---------------------------------------------------------------------------
// Forward reference: pre-pass scans whole file so `sub foo` after use is found
// ---------------------------------------------------------------------------

#[test]
fn forward_declared_sub_is_found_by_prescan() {
    // `sub foo` appears AFTER its use — pre-pass must still find it
    let input = "foo /regex/;\nsub foo;";
    let symbol_table = LocalSymbolTable::scan_subs(input);
    assert!(
        symbol_table.is_known_sub("foo"),
        "pre-scan must find forward-declared subs: {:?}",
        symbol_table
    );
    // The lexer will correctly disambiguate because the symbol table has 'foo'
    let toks = tokens_with_symbol_table(input);
    assert!(has_regex_match(&toks), "forward-declared sub must enable regex mode, got: {:?}", toks);
}

// ---------------------------------------------------------------------------
// Sigil stripping: `substr` must not match as `sub` + `str`
// ---------------------------------------------------------------------------

#[test]
fn substr_not_confused_with_sub_keyword() {
    let input = "substr($s, 0, 3);\nsub real;";
    let symbol_table = LocalSymbolTable::scan_subs(input);
    assert!(!symbol_table.is_known_sub("str"), "substr must not register 'str' as a known sub");
    assert!(symbol_table.is_known_sub("real"));
}

// ---------------------------------------------------------------------------
// Multiple subs: all declared subs in one file get the mode benefit
// ---------------------------------------------------------------------------

#[test]
fn multiple_known_subs_all_get_regex_mode() {
    let input = "sub alpha;\nsub beta;\nalpha /x/;\nbeta /y/;";
    let toks = tokens_with_symbol_table(input);
    let regex_count = toks.iter().filter(|t| matches!(t, TokenType::RegexMatch)).count();
    assert_eq!(
        regex_count, 2,
        "both alpha and beta calls should yield regex tokens, got: {:?}",
        toks
    );
}

// ---------------------------------------------------------------------------
// No state corruption: tokenization remains correct after sub lookup
// ---------------------------------------------------------------------------

#[test]
fn remaining_expression_correct_after_regex_disambiguation() {
    // After tokenizing `my_fn /pat/`, the semicolon should follow cleanly
    let input = "sub my_fn;\nmy_fn /pat/;";
    let toks: Vec<_> = PerlLexer::with_config(
        input,
        LexerConfig {
            symbol_table: Some(LocalSymbolTable::scan_subs(input)),
            ..LexerConfig::default()
        },
    )
    .collect_tokens()
    .into_iter()
    .filter(|t| !matches!(t.token_type, TokenType::Whitespace | TokenType::Newline))
    .collect();

    let last_sig = toks.iter().rev().find(|t| !matches!(t.token_type, TokenType::EOF));
    assert!(
        matches!(last_sig.map(|t| &t.token_type), Some(TokenType::Semicolon)),
        "last significant token must be semicolon, got: {:?}",
        last_sig
    );
}
