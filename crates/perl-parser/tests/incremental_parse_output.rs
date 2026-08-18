#![cfg(feature = "incremental")]
//! Differential tests for the incremental native parse-output contract.

use perl_parser::incremental::MAX_EDIT_SIZE;
use perl_parser::{Edit, IncrementalState, ParseOutput, Parser, apply_edits};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn fresh_output(source: &str) -> ParseOutput {
    let mut parser = Parser::new(source);
    parser.parse_with_recovery()
}

fn assert_output_equivalent(actual: &ParseOutput, expected: &ParseOutput) {
    assert_eq!(actual.ast, expected.ast);
    assert_eq!(actual.diagnostics, expected.diagnostics);
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

fn apply_reference_edits(source: &str, edits: &[Edit]) -> String {
    let mut sorted = edits.to_vec();
    sorted.sort_by_key(|edit| edit.start_byte);
    sorted.reverse();
    let mut result = source.to_string();
    for edit in sorted {
        result.replace_range(edit.start_byte..edit.old_end_byte, &edit.new_text);
    }
    result
}

#[test]
fn initial_malformed_state_keeps_the_native_recovered_tree_and_diagnostics() {
    let source = "my $x = ; print 1;";
    let state = IncrementalState::new(source.to_string());
    let fresh = fresh_output(source);
    assert!(!fresh.diagnostics.is_empty());
    assert_output_equivalent(state.parse_output(), &fresh);
}

#[test]
fn empty_edit_batch_preserves_the_current_generation_without_work() -> TestResult {
    let source = "my $x = ; print 1;";
    let mut state = IncrementalState::new(source.to_string());
    let before = state.parse_output().clone();
    let token_count = state.tokens().len();

    let result = apply_edits(&mut state, &[])?;

    assert_eq!(state.source(), source);
    assert!(result.changed_ranges.is_empty());
    assert_eq!(result.reparsed_bytes, 0);
    assert_eq!(result.reused_tokens, token_count);
    assert_eq!(result.token_count, token_count);
    assert_eq!(state.tokens().len(), token_count);
    assert_output_equivalent(state.parse_output(), &before);
    assert_output_equivalent(result.parse_output(), &before);
    Ok(())
}

#[test]
fn empty_edit_batch_preserves_the_current_generation_without_parser_work() -> TestResult {
    let source = "my $x = ; print 1;";
    let mut state = IncrementalState::new(source.to_string());
    let before = state.parse_output().clone();
    let token_count = state.tokens.len();
    assert!(!before.diagnostics.is_empty(), "fixture must preserve recovered output");

    let result = apply_edits(&mut state, &[])?;

    assert_eq!(state.source, source);
    assert!(result.changed_ranges.is_empty());
    assert_eq!(result.reparsed_bytes, 0);
    assert_eq!(result.reused_tokens, token_count);
    assert_eq!(result.token_count, token_count);
    assert_eq!(state.tokens.len(), token_count);
    assert_output_equivalent(state.parse_output(), &before);
    assert_output_equivalent(result.parse_output(), &before);

    Ok(())
}

#[test]
fn clean_to_malformed_edit_returns_the_current_native_parse_output() -> TestResult {
    let source = "my $x = 1; print 2;";
    let start = source.find("= 1").ok_or("initializer missing")? + 2;
    let edit = Edit {
        start_byte: start,
        old_end_byte: start + 1,
        new_end_byte: start,
        new_text: String::new(),
    };
    let final_source = apply_reference_edits(source, std::slice::from_ref(&edit));
    let fresh = fresh_output(&final_source);
    assert!(!fresh.diagnostics.is_empty());

    let mut state = IncrementalState::new(source.to_string());
    let result = apply_edits(&mut state, &[edit])?;

    assert_eq!(state.source(), final_source);
    assert_eq!(result.reparsed_bytes, final_source.len());
    assert_output_equivalent(state.parse_output(), &fresh);
    assert_output_equivalent(result.parse_output(), &fresh);
    Ok(())
}

#[test]
fn malformed_to_clean_edit_removes_recovery_diagnostics_atomically() -> TestResult {
    let source = "my $x = ; print 2;";
    let start = source.find("= ;").ok_or("insertion point missing")? + 2;
    let edit = Edit {
        start_byte: start,
        old_end_byte: start,
        new_end_byte: start + 1,
        new_text: "1".to_string(),
    };
    let final_source = apply_reference_edits(source, std::slice::from_ref(&edit));
    let fresh = fresh_output(&final_source);
    assert!(fresh.diagnostics.is_empty());

    let mut state = IncrementalState::new(source.to_string());
    assert!(!state.parse_output().diagnostics.is_empty());
    let result = apply_edits(&mut state, &[edit])?;

    assert_eq!(state.source(), final_source);
    assert_output_equivalent(state.parse_output(), &fresh);
    assert_output_equivalent(result.parse_output(), &fresh);
    Ok(())
}

#[test]
fn oversized_batch_applies_every_edit_before_full_fallback() -> TestResult {
    let source = "my $left = 1;\nmy $right = 2;\n";
    let second_start = source.find("my $right").ok_or("second statement missing")?;
    let padding = " ".repeat(MAX_EDIT_SIZE / 2 + 1);
    let edits = vec![
        Edit {
            start_byte: 0,
            old_end_byte: 0,
            new_end_byte: padding.len(),
            new_text: padding.clone(),
        },
        Edit {
            start_byte: second_start,
            old_end_byte: second_start,
            new_end_byte: second_start + padding.len(),
            new_text: padding,
        },
    ];
    let final_source = apply_reference_edits(source, &edits);
    let fresh = fresh_output(&final_source);

    let mut state = IncrementalState::new(source.to_string());
    let result = apply_edits(&mut state, &edits)?;

    assert_eq!(state.source(), final_source);
    assert_eq!(result.changed_ranges, vec![0..final_source.len()]);
    assert_eq!(result.reparsed_bytes, final_source.len());
    assert_output_equivalent(state.parse_output(), &fresh);
    assert_output_equivalent(result.parse_output(), &fresh);
    Ok(())
}

#[test]
fn invalid_overlapping_batch_leaves_the_previous_generation_untouched() -> TestResult {
    let source = "my $value = 12;";
    let literal = source.find("12").ok_or("literal missing")?;
    let edits = [
        Edit {
            start_byte: literal,
            old_end_byte: literal + 2,
            new_end_byte: literal + 1,
            new_text: "3".to_string(),
        },
        Edit {
            start_byte: literal + 1,
            old_end_byte: literal + 2,
            new_end_byte: literal + 2,
            new_text: "4".to_string(),
        },
    ];
    let before = fresh_output(source);
    let mut state = IncrementalState::new(source.to_string());

    let Err(error) = apply_edits(&mut state, &edits) else {
        return Err("overlapping edit batch must fail".into());
    };

    assert!(error.to_string().contains("overlapping"));
    assert_eq!(state.source(), source);
    assert_output_equivalent(state.parse_output(), &before);
    Ok(())
}
