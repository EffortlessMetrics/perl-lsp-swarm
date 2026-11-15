//! Production-path proof that parser operations emit typed completion.
//!
//! These tests read [`ParseOutput::completion`] from `parse_with_recovery`.
//! Diagnostic contents cannot choose the class.

use perl_parser_core::{ParseCompletion, ParseCompletionClass, ParseStopCause, Parser};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

#[test]
fn clean_parse_completes_clean() {
    let mut parser = Parser::new("my $x = 1;");
    let output = parser.parse_with_recovery();
    assert_eq!(output.completion(), ParseCompletion::CompletedClean);
    assert_eq!(output.completion().class(), ParseCompletionClass::Clean);
    assert!(output.completion().validate_against_output(&output).is_ok());
}

#[test]
fn recovered_parse_completes_with_recovery() {
    let mut parser = Parser::new("my $x = ;");
    let output = parser.parse_with_recovery();
    assert!(
        matches!(output.completion(), ParseCompletion::CompletedWithRecovery { recovered_count } if recovered_count > 0)
    );
    assert_eq!(output.completion().class(), ParseCompletionClass::Recovered);
    assert!(output.stop_cause().is_none());
}

#[test]
fn cooperative_cancellation_is_typed_regardless_of_other_diagnostics() {
    let flag = Arc::new(AtomicBool::new(true));
    let mut parser = Parser::new_with_cancellation("my $x = 1; my $y = 2;", flag);
    let output = parser.parse_with_recovery();
    assert_eq!(output.stop_cause(), Some(ParseStopCause::Cancelled));
    assert_eq!(output.completion().class(), ParseCompletionClass::Cancelled);
    assert!(output.completion().validate_against_output(&output).is_ok());
}

#[test]
fn repeated_identical_source_is_deterministic() {
    let source = "sub f { my $x = ; return 1; }";
    let first = Parser::new(source).parse_with_recovery().completion();
    let second = Parser::new(source).parse_with_recovery().completion();
    assert_eq!(first, second);
}
