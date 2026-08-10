//! Byte-offset mapping from `ParseError` to diagnostic range.
//!
//! The diagnostics provider used to derive the position with its own
//! per-variant match whose catch-all arm pinned every unlisted variant —
//! notably `Recovered`, which the parser emits at 15 sites — to byte offset 0,
//! i.e. line 1 column 1. `ParseError::location` is the parser's own authority
//! for which variants carry an offset; these tests pin that every variant that
//! has one reports it, and that the variants which never had one keep their
//! documented anchors.

use std::sync::Arc;

use perl_lsp_rs_core::providers::diagnostics::{Diagnostic, DiagnosticsProvider, parse_error_code};
use perl_parser::Parser;
use perl_parser_core::error::{ParseError, RecoveryKind, RecoverySite};

/// Run the provider over `source` with an explicit `parse_errors` list.
///
/// The AST comes from parsing `source` normally; the errors are supplied by the
/// caller so each variant can be exercised without having to find a Perl
/// snippet that provokes it.
fn diagnostics_for(source: &str, parse_errors: &[ParseError]) -> Vec<Diagnostic> {
    let output = Parser::new(source).parse_with_recovery();
    let ast = Arc::new(output.ast);
    let provider = DiagnosticsProvider::new();
    provider.get_diagnostics(&ast, parse_errors, source, None)
}

/// The range start of the diagnostic produced for `error`, if one was produced.
///
/// The diagnostic is located by the code the provider itself derives for the
/// error, so lint output mixed into the same list cannot satisfy the assertion
/// by accident. `None` means no diagnostic was emitted at all, which fails the
/// caller's `assert_eq!` against a `Some(_)` expectation.
fn parse_diagnostic_start(source: &str, error: ParseError) -> Option<usize> {
    let expected_code = parse_error_code(&error);
    let diags = diagnostics_for(source, std::slice::from_ref(&error));
    diags
        .iter()
        .find(|d| d.code.as_deref() == Some(expected_code.as_str()))
        .map(|found| found.range.0)
}

const SOURCE: &str = "use strict;\nuse warnings;\nmy $x = 1;\nprint $x;\n";

// ---- variants that carry a location ---------------------------------------

#[test]
fn recovered_reports_its_stored_location() -> Result<(), Box<dyn std::error::Error>> {
    let error = ParseError::Recovered {
        site: RecoverySite::ArgList,
        kind: RecoveryKind::InsertedCloser,
        location: 30,
    };
    assert_eq!(
        parse_diagnostic_start(SOURCE, error),
        Some(30),
        "a recovered parse error must land on the recovery point, not offset 0"
    );

    Ok(())
}

#[test]
fn recovered_missing_operand_reports_its_stored_location() -> Result<(), Box<dyn std::error::Error>>
{
    let error = ParseError::Recovered {
        site: RecoverySite::InfixRhs,
        kind: RecoveryKind::MissingOperand,
        location: 41,
    };
    assert_eq!(parse_diagnostic_start(SOURCE, error), Some(41));

    Ok(())
}

#[test]
fn unexpected_token_still_reports_its_location() -> Result<(), Box<dyn std::error::Error>> {
    let error = ParseError::unexpected("semicolon", "}", 25);
    assert_eq!(parse_diagnostic_start(SOURCE, error), Some(25));

    Ok(())
}

#[test]
fn syntax_error_still_reports_its_location() -> Result<(), Box<dyn std::error::Error>> {
    let error = ParseError::syntax("expected expression", 18);
    assert_eq!(parse_diagnostic_start(SOURCE, error), Some(18));

    Ok(())
}

#[test]
fn advisory_still_reports_its_location() -> Result<(), Box<dyn std::error::Error>> {
    let error = ParseError::nested_quantifier_advisory(12);
    assert_eq!(parse_diagnostic_start(SOURCE, error), Some(12));

    Ok(())
}

// ---- variants that carry no location --------------------------------------

#[test]
fn unexpected_eof_stays_anchored_at_end_of_input() -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(
        parse_diagnostic_start(SOURCE, ParseError::UnexpectedEof),
        Some(SOURCE.len()),
        "UnexpectedEof stores no offset and must stay anchored at end-of-input"
    );

    Ok(())
}

#[test]
fn lexer_error_stays_anchored_at_start_of_file() -> Result<(), Box<dyn std::error::Error>> {
    let error = ParseError::LexerError { message: "unterminated string".into() };
    assert_eq!(parse_diagnostic_start(SOURCE, error), Some(0));

    Ok(())
}

#[test]
fn recursion_limit_stays_anchored_at_start_of_file() -> Result<(), Box<dyn std::error::Error>> {
    // `ParseError::RecursionLimit` genuinely carries no position; the fallback
    // to offset 0 is correct for it rather than a symptom of the catch-all.
    assert_eq!(parse_diagnostic_start(SOURCE, ParseError::RecursionLimit), Some(0));

    Ok(())
}

#[test]
fn nesting_too_deep_stays_anchored_at_start_of_file() -> Result<(), Box<dyn std::error::Error>> {
    let error = ParseError::NestingTooDeep { depth: 100, max_depth: 50 };
    assert_eq!(parse_diagnostic_start(SOURCE, error), Some(0));

    Ok(())
}

// ---- location is clamped to the source ------------------------------------

#[test]
fn out_of_range_location_is_clamped_to_the_source_length() -> Result<(), Box<dyn std::error::Error>>
{
    let error = ParseError::syntax("stale offset from a previous revision", SOURCE.len() + 500);
    assert_eq!(
        parse_diagnostic_start(SOURCE, error),
        Some(SOURCE.len()),
        "an offset past end-of-source must clamp rather than produce an invalid range"
    );

    Ok(())
}

// ---- recovery does not suppress the lint stack ----------------------------

#[test]
fn recovered_error_does_not_suppress_scope_and_lint_diagnostics()
-> Result<(), Box<dyn std::error::Error>> {
    let source = "use strict;\nuse warnings;\nmy $unused_here = 1;\n";
    let error = ParseError::Recovered {
        site: RecoverySite::ArgList,
        kind: RecoveryKind::InsertedCloser,
        location: 30,
    };

    let diags = diagnostics_for(source, std::slice::from_ref(&error));

    assert!(
        diags.iter().any(|d| d.code.as_deref() == Some("PL102")),
        "a recovered parse error must not delete the unused-variable lint: {diags:#?}"
    );

    Ok(())
}

#[test]
fn unrecoverable_parse_error_still_suppresses_the_lint_stack()
-> Result<(), Box<dyn std::error::Error>> {
    // Negative case for the same predicate: a hard syntax error means the tree
    // is untrustworthy, so the scope/lint stack must stay suppressed (#5089).
    let source = "use strict;\nuse warnings;\nmy $unused_here = 1;\n";
    let error = ParseError::syntax("expected expression", 20);

    let diags = diagnostics_for(source, std::slice::from_ref(&error));

    assert!(
        !diags.iter().any(|d| d.code.as_deref() == Some("PL102")),
        "a blocking syntax error must still suppress lint output: {diags:#?}"
    );

    Ok(())
}
