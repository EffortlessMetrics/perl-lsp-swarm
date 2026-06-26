//! BDD tests for bareword/regex disambiguation via the pre-pass symbol table.
//!
//! Covers issue #1353: unknown barewords followed by `/` were always lexed as
//! division. When the lexer is created with a symbol table that knows about
//! `sub NAME` declarations, the `/` is correctly lexed as a regex delimiter.

use perl_lexer::{LexerConfig, LocalSymbolTable, PerlLexer, TokenType};
use std::sync::Arc;

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Collect all (non-EOF) token types from the source using a symbol-table-aware
/// lexer built from the same source.
fn tokens_with_symbols(source: &str) -> Vec<TokenType> {
    let mut lexer = PerlLexer::with_source_symbols(source);
    let mut out = Vec::new();
    loop {
        match lexer.next_token() {
            None | Some(perl_lexer::Token { token_type: TokenType::EOF, .. }) => break,
            Some(tok) => out.push(tok.token_type),
        }
    }
    out
}

/// Collect all (non-EOF) token types from the source using the plain lexer
/// (no symbol table).
fn tokens_plain(source: &str) -> Vec<TokenType> {
    let mut lexer = PerlLexer::new(source);
    let mut out = Vec::new();
    loop {
        match lexer.next_token() {
            None | Some(perl_lexer::Token { token_type: TokenType::EOF, .. }) => break,
            Some(tok) => out.push(tok.token_type),
        }
    }
    out
}

/// Find the first slash-like token (Division or RegexMatch) in the stream.
fn first_slash(toks: &[TokenType]) -> Option<&TokenType> {
    toks.iter().find(|t| matches!(t, TokenType::Division | TokenType::RegexMatch))
}

// ── LocalSymbolTable unit tests ───────────────────────────────────────────────

#[test]
fn scan_subs_empty_source() {
    let table = LocalSymbolTable::scan_subs("");
    assert!(!table.is_known_sub("foo"));
}

#[test]
fn scan_subs_single_forward_declaration() {
    let table = LocalSymbolTable::scan_subs("sub foo;");
    assert!(table.is_known_sub("foo"));
    assert!(!table.is_known_sub("bar"));
}

#[test]
fn scan_subs_definition_with_body() {
    let table = LocalSymbolTable::scan_subs("sub bar { return 1 }");
    assert!(table.is_known_sub("bar"));
}

#[test]
fn scan_subs_multiple_declarations() {
    let table = LocalSymbolTable::scan_subs("sub foo;\nsub bar { 1 }\nsub baz;");
    assert!(table.is_known_sub("foo"));
    assert!(table.is_known_sub("bar"));
    assert!(table.is_known_sub("baz"));
    assert!(!table.is_known_sub("qux"));
}

#[test]
fn scan_subs_ignores_sub_in_line_comment() {
    let table = LocalSymbolTable::scan_subs("# sub foo;\nmy $x = 1;");
    assert!(!table.is_known_sub("foo"), "subs in comments must not be collected");
}

#[test]
fn scan_subs_ignores_sub_in_double_quoted_string() {
    let table = LocalSymbolTable::scan_subs(r#"my $s = "sub foo;";"#);
    assert!(!table.is_known_sub("foo"), "subs inside string literals must not be collected");
}

#[test]
fn scan_subs_ignores_sub_in_single_quoted_string() {
    let table = LocalSymbolTable::scan_subs("my $s = 'sub foo;';");
    assert!(!table.is_known_sub("foo"), "subs inside single-quoted strings must not be collected");
}

#[test]
fn scan_subs_does_not_collect_anonymous_sub() {
    // `sub { }` has no name after `sub`, so nothing should be collected.
    let table = LocalSymbolTable::scan_subs("my $cb = sub { 1 };");
    // There should be no named sub in the table — verify a few plausible names.
    assert!(!table.is_known_sub("1"), "anonymous sub body must not be collected");
    assert!(!table.is_known_sub("cb"), "variable name must not be collected");
}

#[test]
fn is_known_sub_case_sensitive() {
    let table = LocalSymbolTable::scan_subs("sub Foo;");
    assert!(table.is_known_sub("Foo"));
    assert!(!table.is_known_sub("foo"), "lookup must be case-sensitive");
}

// ── Disambiguation integration tests ─────────────────────────────────────────

/// Core fix: `sub foo;  foo /regex/;` — `/` must be a regex, not division.
#[test]
fn known_sub_before_slash_yields_regex() {
    let source = "sub my_builder;\nmy_builder /foo|bar/;";
    let toks = tokens_with_symbols(source);
    let slash = first_slash(&toks).expect("should find a slash token");
    assert!(
        matches!(slash, TokenType::RegexMatch),
        "expected RegexMatch after known sub, got {:?}",
        slash
    );
}

/// Safety: unknown bareword followed by `/` must still be division.
#[test]
fn unknown_bareword_before_slash_yields_division() {
    let source = "unknown_fn /2/;";
    let toks = tokens_with_symbols(source);
    let slash = first_slash(&toks).expect("should find a slash token");
    assert!(
        matches!(slash, TokenType::Division),
        "expected Division for unknown bareword, got {:?}",
        slash
    );
}

/// Builtins must continue to set ExpectTerm (already worked before this fix).
#[test]
fn builtin_before_slash_yields_regex() {
    let source = "print /hello/;";
    let toks = tokens_with_symbols(source);
    let slash = first_slash(&toks).expect("should find a slash token");
    assert!(
        matches!(slash, TokenType::RegexMatch),
        "expected RegexMatch after builtin, got {:?}",
        slash
    );
}

/// Division after a number must not be affected.
#[test]
fn number_before_slash_yields_division() {
    let source = "42 / 2;";
    let toks = tokens_with_symbols(source);
    let slash = first_slash(&toks).expect("should find a slash token");
    assert!(
        matches!(slash, TokenType::Division),
        "expected Division after numeric literal, got {:?}",
        slash
    );
}

/// Division after a variable must not be affected.
#[test]
fn variable_before_slash_yields_division() {
    let source = "$x / 2;";
    let toks = tokens_with_symbols(source);
    let slash = first_slash(&toks).expect("should find a slash token");
    assert!(
        matches!(slash, TokenType::Division),
        "expected Division after variable, got {:?}",
        slash
    );
}

/// Forward reference: `foo /x/; sub foo;` — the pre-pass sees `sub foo` even
/// though it comes later, so `/` is lexed as regex.
#[test]
fn forward_ref_is_supported_via_prescan() {
    let source = "foo /x/;\nsub foo;";
    let toks = tokens_with_symbols(source);
    let slash = first_slash(&toks).expect("should find a slash token");
    assert!(
        matches!(slash, TokenType::RegexMatch),
        "expected RegexMatch for forward-declared sub, got {:?}",
        slash
    );
}

/// Non-keyword non-builtin bareword followed by `/` with NO symbol table must
/// remain division (baseline / regression guard).
#[test]
fn without_symbol_table_unknown_bareword_is_division() {
    let source = "sub foo;\nfoo /bar/;";
    let toks = tokens_plain(source);
    let slash = first_slash(&toks).expect("should find a slash token");
    assert!(
        matches!(slash, TokenType::Division),
        "without a symbol table, unknown bareword /bar/ must be Division, got {:?}",
        slash
    );
}

/// Verify that partial matches (e.g. `ifunc`) are not blocked by keyword-like prefix.
#[test]
fn sub_starting_with_keyword_prefix_is_tracked() {
    let source = "sub ifunc;\nifunc /foo/;";
    let toks = tokens_with_symbols(source);
    let slash = first_slash(&toks).expect("should find a slash token");
    assert!(
        matches!(slash, TokenType::RegexMatch),
        "sub name sharing a prefix with keyword must still be tracked, got {:?}",
        slash
    );
}

/// LexerConfig with an explicit Arc<LocalSymbolTable> works the same as
/// `with_source_symbols`.
#[test]
fn explicit_config_symbol_table_works() {
    let source = "sub my_fn;\nmy_fn /pattern/;";
    let table = LocalSymbolTable::scan_subs(source);
    let config = LexerConfig { symbol_table: Some(Arc::new(table)), ..LexerConfig::default() };
    let mut lexer = PerlLexer::with_config(source, config);
    let toks: Vec<_> = std::iter::from_fn(|| lexer.next_token())
        .take_while(|t| !matches!(t.token_type, TokenType::EOF))
        .map(|t| t.token_type)
        .collect();
    let slash = first_slash(&toks).expect("should find slash");
    assert!(
        matches!(slash, TokenType::RegexMatch),
        "explicit config with symbol table must yield RegexMatch, got {:?}",
        slash
    );
}
