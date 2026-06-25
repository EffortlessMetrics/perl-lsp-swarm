//! Integration tests for bareword/regex disambiguation via LocalSymbolTable.
//!
//! These tests verify that when a [`LocalSymbolTable`] is wired into [`LexerConfig`],
//! the lexer correctly classifies `/` after user-declared subroutines as a regex
//! delimiter (not division), while preserving all existing division behaviour for
//! unknown barewords.
//!
//! Issue: #1353 — Bareword/Regex disambiguation fails without same-file symbol table

use std::sync::Arc;

use perl_lexer::{LexerConfig, LocalSymbolTable, PerlLexer, TokenType};

type TestResult = Result<(), Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a lexer that has pre-scanned `source` for sub declarations.
fn lexer_with_symbol_table(source: &'static str) -> PerlLexer<'static> {
    let table = LocalSymbolTable::scan_subs(source);
    let config = LexerConfig { symbol_table: Some(Arc::new(table)), ..LexerConfig::default() };
    PerlLexer::with_config(source, config)
}

/// Collect all token types from a lexer (stops at EOF).
fn collect_token_types(mut lexer: PerlLexer<'_>) -> Vec<TokenType> {
    let mut types = Vec::new();
    while let Some(tok) = lexer.next_token() {
        let is_eof = matches!(tok.token_type, TokenType::EOF);
        types.push(tok.token_type);
        if is_eof {
            break;
        }
    }
    types
}

/// Return `true` if `types` contains a regex match token.
fn has_regex(types: &[TokenType]) -> bool {
    types.iter().any(|t| matches!(t, TokenType::RegexMatch))
}

/// Return `true` if `types` contains a division token.
fn has_division(types: &[TokenType]) -> bool {
    types.iter().any(|t| matches!(t, TokenType::Division))
}

// ---------------------------------------------------------------------------
// Core fix: known sub → regex
// ---------------------------------------------------------------------------

/// The primary issue reproduction case from #1353.
///
/// `sub my_regex_builder; my_regex_builder /foo|bar/;`
/// Previously: `/` → Division (wrong)
/// After fix : `/` → RegexMatch (correct)
#[test]
fn bareword_followed_by_slash_is_regex_when_sub_declared() -> TestResult {
    let source = "sub my_regex_builder;\nmy_regex_builder /foo|bar/;";
    let lexer = lexer_with_symbol_table(source);
    let types = collect_token_types(lexer);

    assert!(
        has_regex(&types),
        "Expected a RegexMatch token after known sub bareword, got: {types:?}"
    );
    assert!(
        !has_division(&types),
        "Must not produce a Division token when known sub precedes /…/, got: {types:?}"
    );
    Ok(())
}

/// Variant: sub with block body (not forward-declaration).
#[test]
fn bareword_followed_by_slash_is_regex_when_sub_has_body() -> TestResult {
    let source = "sub builder { return 1; }\nbuilder /test/;";
    let lexer = lexer_with_symbol_table(source);
    let types = collect_token_types(lexer);

    assert!(has_regex(&types), "Expected RegexMatch after sub with body, got: {types:?}");
    assert!(!has_division(&types), "Must not produce Division, got: {types:?}");
    Ok(())
}

// ---------------------------------------------------------------------------
// Safe default: unknown bareword → division (must not regress)
// ---------------------------------------------------------------------------

/// Unknown bareword with no symbol table entry must still produce Division.
#[test]
fn unknown_bareword_followed_by_slash_is_division() -> TestResult {
    // No `sub unknown_func;` declaration.
    let source = "unknown_func /foo/;";
    let lexer = lexer_with_symbol_table(source);
    let types = collect_token_types(lexer);

    assert!(
        has_division(&types),
        "Expected Division after undeclared bareword (safe default), got: {types:?}"
    );
    Ok(())
}

/// Without a symbol table at all, behaviour is unchanged (safe backward compat).
#[test]
fn without_symbol_table_unknown_bareword_is_division() -> TestResult {
    let source = "my_func /foo/;";
    let config = LexerConfig { symbol_table: None, ..LexerConfig::default() };
    let lexer = PerlLexer::with_config(source, config);
    let types = collect_token_types(lexer);

    assert!(
        has_division(&types),
        "Without symbol table, unknown bareword must still give Division, got: {types:?}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Builtins still work (no regression)
// ---------------------------------------------------------------------------

/// Built-in functions must still be treated as term-introducing regardless of symbol table.
#[test]
fn builtin_function_followed_by_slash_is_regex() -> TestResult {
    let source = "print /foo/;";
    // No symbol table needed; builtin path is independent.
    let lexer = PerlLexer::new(source);
    let types = collect_token_types(lexer);

    assert!(has_regex(&types), "print /…/ must give RegexMatch (builtin rule), got: {types:?}");
    assert!(!has_division(&types), "Must not give Division for builtin, got: {types:?}");
    Ok(())
}

/// grep is a builtin; symbol table must not interfere.
#[test]
fn builtin_grep_followed_by_slash_is_regex() -> TestResult {
    let source = "grep /pattern/, @list;";
    let lexer = PerlLexer::new(source);
    let types = collect_token_types(lexer);

    assert!(has_regex(&types), "grep /…/ must give RegexMatch, got: {types:?}");
    Ok(())
}

// ---------------------------------------------------------------------------
// Division after variables is unaffected
// ---------------------------------------------------------------------------

/// Division after a variable must remain division.
#[test]
fn division_after_variable_is_unaffected() -> TestResult {
    let source = "$x / 2";
    let lexer = PerlLexer::new(source);
    let types = collect_token_types(lexer);

    assert!(has_division(&types), "Division after $x must stay Division, got: {types:?}");
    assert!(!has_regex(&types), "Must not produce RegexMatch for $x / 2, got: {types:?}");
    Ok(())
}

/// Division after a number must remain division.
#[test]
fn division_after_number_is_unaffected() -> TestResult {
    let source = "42 / 7";
    let lexer = PerlLexer::new(source);
    let types = collect_token_types(lexer);

    assert!(has_division(&types), "Division after 42 must stay Division, got: {types:?}");
    Ok(())
}

// ---------------------------------------------------------------------------
// Forward reference: declaration after call site
// ---------------------------------------------------------------------------

/// Pre-pass scans whole file, so a forward reference is still resolved.
#[test]
fn forward_reference_sub_declaration_after_call() -> TestResult {
    let source = "builder /foo/;\nsub builder { 1 }";
    let lexer = lexer_with_symbol_table(source);
    let types = collect_token_types(lexer);

    assert!(
        has_regex(&types),
        "Forward-declared sub must still produce RegexMatch, got: {types:?}"
    );
    assert!(!has_division(&types), "Must not produce Division for forward ref, got: {types:?}");
    Ok(())
}

// ---------------------------------------------------------------------------
// use constant → term-introducing
// ---------------------------------------------------------------------------

/// A constant declared with `use constant` should also be term-introducing.
#[test]
fn use_constant_followed_by_slash_is_regex() -> TestResult {
    let source = "use constant MY_PATTERN;\nMY_PATTERN /foo/;";
    let lexer = lexer_with_symbol_table(source);
    let types = collect_token_types(lexer);

    assert!(
        has_regex(&types),
        "Constant name must be term-introducing → RegexMatch, got: {types:?}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Partial match safety: names that contain sub-keywords
// ---------------------------------------------------------------------------

/// `subscribe` must not be misidentified as `sub scribe`.
#[test]
fn identifier_starting_with_sub_is_not_split() -> TestResult {
    // `subscribe` is not declared; should still be unknown bareword → division.
    let source = "subscribe /foo/;";
    let lexer = lexer_with_symbol_table(source);
    let types = collect_token_types(lexer);

    // `subscribe` is unknown → ExpectOperator → division (not regex)
    assert!(
        !has_regex(&types) || has_division(&types),
        "subscribe must not be parsed as sub+scribe; got: {types:?}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Multiple sub declarations in one file
// ---------------------------------------------------------------------------

/// All declared subs in a file should be term-introducing.
#[test]
fn multiple_subs_all_term_introducing() -> TestResult {
    let source = "sub alpha;\nsub beta;\nalpha /a/;\nbeta /b/;";
    let lexer = lexer_with_symbol_table(source);
    let types = collect_token_types(lexer);

    assert!(
        has_regex(&types),
        "Expected RegexMatch tokens for both alpha and beta, got: {types:?}"
    );
    assert!(!has_division(&types), "Must not produce any Division tokens, got: {types:?}");
    Ok(())
}

// ---------------------------------------------------------------------------
// Non-keyword-name safety: no over-rejection
// ---------------------------------------------------------------------------

/// A bareword that looks like a keyword prefix but is declared as sub still
/// gets RegexMatch.
#[test]
fn sub_named_like_partial_keyword_is_term_introducing() -> TestResult {
    let source = "sub ifunc;\nifunc /pattern/;";
    let lexer = lexer_with_symbol_table(source);
    let types = collect_token_types(lexer);

    // `ifunc` is a declared sub → must be term-introducing
    assert!(has_regex(&types), "ifunc sub must be term-introducing → RegexMatch, got: {types:?}");
    Ok(())
}
