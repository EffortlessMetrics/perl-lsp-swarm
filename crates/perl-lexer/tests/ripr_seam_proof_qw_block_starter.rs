//! Public behavior proof for unclosed parenthesized `qw(` recovery at block-form
//! and parenthesized statement starters (#4491).
//!
//! These call-observation tests drive the public `perl_lexer` API so the
//! `qw_block_statement_boundary_at` recovery seam is exercised and observed at
//! the crate boundary. The unclosed `qw(` emits a single `Error` token whose
//! text is exactly the span the recovery consumed, so that span witnesses
//! whether a following block starter became a synchronization boundary.

use perl_lexer::{LexerConfig, PerlLexer, TokenType};

/// Text of the first unclosed-`qw(` `Error` token — the source span the recovery
/// boundary allowed the quote-word token to consume.
fn qw_recovery_span(input: &str) -> Result<String, String> {
    qw_recovery_span_with_config(input, LexerConfig::default())
}

fn qw_recovery_span_with_config(input: &str, config: LexerConfig) -> Result<String, String> {
    let mut lexer = PerlLexer::with_config(input, config);
    while let Some(token) = lexer.next_token() {
        if matches!(token.token_type, TokenType::Error(_)) && token.text.starts_with("qw(") {
            return Ok(token.text.to_string());
        }
    }
    Err(format!("no unclosed-qw error token was produced for {input:?}"))
}

#[test]
fn block_form_starters_end_the_qw_token() -> Result<(), String> {
    // Each shape-valid block/parenthesized starter is a recovery boundary, so the
    // qw token stops at it rather than swallowing it as quote-word text.
    for (label, input, starter) in [
        ("sub block", "my @a = qw(word\nsub run { print 1; }", "sub"),
        ("sub leading-qualified block", "my @a = qw(word\nsub ::PCDATA { 1; }", "sub"),
        ("sub multiline block", "my @a = qw(word\nsub run\n{ print 1; }", "sub"),
        ("sub multiline prototype block", "my @a = qw(word\nsub run\n($) { print 1; }", "sub"),
        (
            "sub multiline prototype string block",
            "my @a = qw(word\nsub run\n($x = \")\") { print 1; }",
            "sub",
        ),
        ("package block", "my @a = qw(word\npackage Foo { 1; }", "package"),
        ("package leading-qualified block", "my @a = qw(word\npackage ::My::App { 1; }", "package"),
        ("package multiline block", "my @a = qw(word\npackage Foo\n{ 1; }", "package"),
        ("package version block", "my @a = qw(word\npackage Foo 1.23 { 1; }", "package"),
        ("package semi", "my @a = qw(word\npackage Foo;", "package"),
        ("class block", "my @a = qw(word\nclass Foo { 1; }", "class"),
        ("BEGIN block", "my @a = qw(word\nBEGIN { 1; }", "BEGIN"),
        ("END block", "my @a = qw(word\nEND { 1; }", "END"),
        ("BEGIN attribute block", "my @a = qw(word\nBEGIN :lvalue { 1; }", "BEGIN"),
        ("BEGIN qualified attribute block", "my @a = qw(word\nBEGIN :Foo::bar { 1; }", "BEGIN"),
        ("INIT block", "my @a = qw(word\nINIT { 1; }", "INIT"),
        ("CHECK block", "my @a = qw(word\nCHECK { 1; }", "CHECK"),
        ("UNITCHECK block", "my @a = qw(word\nUNITCHECK { 1; }", "UNITCHECK"),
        ("paren print", "my @a = qw(word\nprint(1);", "print"),
    ] {
        let span = qw_recovery_span(input)?;
        if span != "qw(word\n" {
            return Err(format!("[{label}] qw did not stop at `{starter}`: {span:?}"));
        }
    }
    Ok(())
}

#[test]
fn qualified_phaser_attributes_survive_zero_lookahead() -> Result<(), String> {
    let input = "my @a = qw(word\nBEGIN :Foo::bar { 1; }";
    let config = LexerConfig { max_lookahead: 0, ..LexerConfig::default() };
    let span = qw_recovery_span_with_config(input, config)?;
    if span != "qw(word\n" {
        return Err(format!("qualified attribute did not end qw: {span:?}"));
    }
    Ok(())
}

#[test]
fn shapeless_block_word_is_not_a_boundary() -> Result<(), String> {
    // A `sub`/`package`-shaped word with neither a block nor a terminating `;` is
    // ordinary quote-word content: the qw token consumes it.
    for (label, input) in [
        ("bare sub", "my @a = qw(word\nsub run more"),
        ("bare package", "my @a = qw(word\npackage more words"),
    ] {
        let span = qw_recovery_span(input)?;
        if !span.contains("sub") && !span.contains("package") {
            return Err(format!("[{label}] boundary decision lost the words: {span:?}"));
        }
        if span == "qw(word\n" {
            return Err(format!("[{label}] shapeless word wrongly became a boundary: {span:?}"));
        }
    }
    Ok(())
}

#[test]
fn keyword_word_does_not_borrow_a_later_lines_delimiter() -> Result<(), String> {
    // A starter-shaped word must not borrow the `{`/`;` of an unrelated statement
    // on a *later* line: the header-on-one-line guard keeps the whole tail inside
    // the qw span rather than synchronizing on the bare keyword word.
    for (label, input) in [
        ("sub then return", "my @a = qw(word\nsub\nreturn { a => 1 };"),
        ("package then return", "my @a = qw(word\npackage\nreturn 5;"),
    ] {
        let span = qw_recovery_span(input)?;
        if span == "qw(word\n" {
            return Err(format!("[{label}] keyword word borrowed a later delimiter: {span:?}"));
        }
    }
    Ok(())
}

#[test]
fn extra_named_header_words_are_not_boundaries() -> Result<(), String> {
    for (label, input) in [
        ("sub extra header", "my @a = qw(word\nsub run extra { 1; }"),
        ("package extra header", "my @a = qw(word\npackage Foo extra { 1; }"),
    ] {
        let span = qw_recovery_span(input)?;
        if span == "qw(word\n" {
            return Err(format!("[{label}] invalid declaration header became a boundary"));
        }
    }
    Ok(())
}
