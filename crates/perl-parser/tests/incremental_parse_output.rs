#![cfg(feature = "incremental")]
//! Differential tests for the incremental native parse-output contract.

use perl_parser::{Edit, IncrementalState, ParseOutput, Parser, apply_edits};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn fresh_output(source: &str) -> ParseOutput {
    let mut parser = Parser::new(source);
    parser.parse_with_recovery()
}

fn assert_output_equivalent(actual: &ParseOutput, expected: &ParseOutput) {
    assert_eq!(actual.ast, expected.ast, "AST differs from a fresh recovered parse");
    assert_eq!(
        actual.diagnostics, expected.diagnostics,
        "ordered parser diagnostics differ from a fresh recovered parse"
    );
    assert_eq!(actual.terminated_early, expected.terminated_early);
    assert_eq!(actual.recovered_count, expected.recovered_count);
    assert_eq!(actual.budget_usage.errors_emitted, expected.budget_usage.errors_emitted);
    assert_eq!(actual.budget_usage.current_depth, expected.budget_usage.current_depth);
    assert_eq!(actual.budget_usage.max_depth_reached, expected.budget_usage.max_depth_reached);
    assert_eq!(actual.budget_usage.tokens_skipped, expected.budget_usage.tokens_skipped);
    assert_eq!(
        actual.budget_usage.recoveries_attempted,
        expected.budget_usage.recoveries_attempted
    );
}

fn apply_reference_edit(source: &str, edit: &Edit) -> Result<String, &'static str> {
    if edit.start_byte > edit.old_end_byte || edit.old_end_byte > source.len() {
        return Err("reference edit range is out of bounds");
    }
    if !source.is_char_boundary(edit.start_byte) || !source.is_char_boundary(edit.old_end_byte) {
        return Err("reference edit range is not on UTF-8 boundaries");
    }

    let mut result = source.to_string();
    result.replace_range(edit.start_byte..edit.old_end_byte, &edit.new_text);
    Ok(result)
}

#[test]
fn initial_malformed_state_keeps_the_native_recovered_tree_and_diagnostics() -> TestResult {
    let source = "my $x = ; print 1;";
    let state = IncrementalState::new(source.to_string());
    let fresh = fresh_output(source);

    assert!(!fresh.diagnostics.is_empty(), "fixture must exercise structured recovery");
    assert_output_equivalent(&state.parse_output, &fresh);

    Ok(())
}

#[test]
fn clean_to_malformed_edit_returns_the_current_native_parse_output() -> TestResult {
    let source = "my $x = 1; print 2;";
    let start = source.find("= 1").ok_or("clean fixture lost its initializer")? + 2;
    let edit = Edit {
        start_byte: start,
        old_end_byte: start + 1,
        new_end_byte: start,
        new_text: String::new(),
    };
    let final_source = apply_reference_edit(source, &edit)?;
    let fresh = fresh_output(&final_source);
    assert!(!fresh.diagnostics.is_empty(), "edited fixture must require recovery");

    let mut state = IncrementalState::new(source.to_string());
    let result = apply_edits(&mut state, &[edit])?;

    assert_eq!(state.source, final_source);
    assert_output_equivalent(&state.parse_output, &fresh);
    assert_output_equivalent(&result.parse_output, &fresh);

    Ok(())
}

#[test]
fn malformed_to_clean_edit_removes_recovery_diagnostics_atomically() -> TestResult {
    let source = "my $x = ; print 2;";
    let start = source.find("= ;").ok_or("malformed fixture lost its insertion point")? + 2;
    let edit = Edit {
        start_byte: start,
        old_end_byte: start,
        new_end_byte: start + 1,
        new_text: "1".to_string(),
    };
    let final_source = apply_reference_edit(source, &edit)?;
    let fresh = fresh_output(&final_source);
    assert!(fresh.diagnostics.is_empty(), "repaired fixture should parse cleanly");

    let mut state = IncrementalState::new(source.to_string());
    assert!(!state.parse_output.diagnostics.is_empty());
    let result = apply_edits(&mut state, &[edit])?;

    assert_eq!(state.source, final_source);
    assert_output_equivalent(&state.parse_output, &fresh);
    assert_output_equivalent(&result.parse_output, &fresh);

    Ok(())
}
