//! Public behavior proof for unclosed parenthesized `qw(` statement-boundary
//! recovery (#4494).
//!
//! These call-observation tests drive the public `perl_lexer` API directly so
//! the recovery seam — `qw_statement_boundary_at` / `qw_statement_probe` /
//! `qw_probe_span_hides_statement` — is exercised and observed at the crate
//! boundary, not only transitively through `perl-parser-core`. The unclosed
//! `qw(` emits a single `Error` token whose text is exactly the span the
//! recovery consumed, so that span is a direct, public witness of which
//! `QwStatementProbe` outcome fired for each input.

use perl_lexer::{PerlLexer, TokenType};

/// Text of the first `Error` token produced for an unclosed `qw(` — i.e. the
/// exact source span the recovery boundary allowed the quote-word token to
/// consume before synchronizing (or refusing to).
fn qw_recovery_span(input: &str) -> Result<String, String> {
    let mut lexer = PerlLexer::new(input);
    while let Some(token) = lexer.next_token() {
        if matches!(token.token_type, TokenType::Error(_)) && token.text.starts_with("qw(") {
            return Ok(token.text.to_string());
        }
    }
    Err(format!("no unclosed-qw error token was produced for {input:?}"))
}

#[test]
fn eof_trailing_statement_synchronizes_the_qw_token() -> Result<(), String> {
    // Eof outcome: a trailing semicolonless declaration at EOF ends the qw token
    // at the statement boundary rather than being swallowed as quote-word text.
    let span = qw_recovery_span("my @items = qw(word\nmy $x = 42")?;
    if span != "qw(word\n" {
        return Err(format!("qw recovery did not stop at the trailing declaration: {span:?}"));
    }
    Ok(())
}

#[test]
fn semicolon_terminated_statement_synchronizes_the_qw_token() -> Result<(), String> {
    // Terminated outcome: a semicolon-terminated following statement ends the qw
    // token at the same boundary.
    let span = qw_recovery_span("my @items = qw(word\nmy $x = 42;\nprint $x;")?;
    if span.contains("my") || span.contains("42") {
        return Err(format!("qw recovery swallowed the terminated statement: {span:?}"));
    }
    Ok(())
}

#[test]
fn nested_unclosed_candidate_defers_synchronization() -> Result<(), String> {
    // Interrupted outcome, driven through qw_probe_span_hides_statement: the
    // candidate `my @nested = qw(inner` itself swallows to EOF and hides an
    // interior `print`, so the outer qw must NOT synchronize on it — it consumes
    // past `@nested` and stops only at the cleaner `print` boundary.
    let span = qw_recovery_span("my @items = qw(word\nmy @nested = qw(inner\nprint 1;")?;
    if !span.contains("@nested") || !span.contains("qw(inner") || span.contains("print") {
        return Err(format!("nested unclosed candidate changed synchronization: {span:?}"));
    }
    Ok(())
}

#[test]
fn eof_swallowing_quote_rhs_still_synchronizes() -> Result<(), String> {
    // Eof outcome where the trailing declaration's own RHS is an unclosed quote
    // that runs to EOF but hides no further statement: qw_probe_span_hides_statement
    // returns false, so the outer qw still synchronizes at the declaration.
    let span = qw_recovery_span("my @items = qw(word\nmy $x = qq(unterminated")?;
    if span != "qw(word\n" {
        return Err(format!("unterminated-quote RHS was swallowed by the outer qw: {span:?}"));
    }
    Ok(())
}

#[test]
fn plain_trailing_words_are_not_a_boundary() -> Result<(), String> {
    // No-recovery baseline: ordinary trailing words never synchronize, so the qw
    // token consumes all of them.
    let span = qw_recovery_span("my @items = qw(alpha beta gamma")?;
    if !span.contains("alpha") || !span.contains("beta") || !span.contains("gamma") {
        return Err(format!("plain trailing words were not all consumed: {span:?}"));
    }
    Ok(())
}
