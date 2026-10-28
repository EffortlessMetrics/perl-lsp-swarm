#![cfg(feature = "incremental")]
//! Interior edit sequences must agree with fresh construction at each generation.

use perl_parser::incremental::{LexRestartStrategy, ReparseResult};
use perl_parser::{Edit, IncrementalState, apply_edits};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn check_edit(
    state: &mut IncrementalState,
    start: usize,
    end: usize,
    text: &str,
) -> Result<ReparseResult, Box<dyn std::error::Error>> {
    let mut expected_source = state.source().to_string();
    expected_source.replace_range(start..end, text);
    let generation = state.generation().get();
    let result = apply_edits(
        state,
        &[Edit {
            start_byte: start,
            old_end_byte: end,
            new_end_byte: start + text.len(),
            new_text: text.to_string(),
        }],
    )?;
    let fresh = IncrementalState::new(expected_source.clone());
    assert_eq!(state.source(), expected_source);
    assert_eq!(state.generation().get(), generation + 1);
    assert_eq!(state.parse_output().ast, fresh.parse_output().ast);
    assert_eq!(state.tokens().len(), fresh.tokens().len(), "source: {expected_source:?}");
    for (actual, expected) in state.tokens().iter().zip(fresh.tokens()) {
        assert_eq!(actual.token_type, expected.token_type, "source: {expected_source:?}");
        assert_eq!(actual.text, expected.text, "source: {expected_source:?}");
        assert_eq!((actual.start, actual.end), (expected.start, expected.end));
    }
    assert_eq!(result.token_count, fresh.tokens().len());
    assert_eq!(result.lex_restart.strategy, LexRestartStrategy::StoredCheckpointToEof);
    assert_eq!(result.lex_restart.old_prefix_bytes_replayed, 0);
    assert_eq!(result.lex_restart.reused_suffix_tokens, 0);
    assert_eq!(result.reused_tokens, result.lex_restart.reused_prefix_tokens);
    assert_eq!(
        result.lex_restart.relexed_bytes,
        state.source().len() - result.lex_restart.restart_byte
    );
    assert_eq!(result.snapshot.generation(), state.generation());
    assert_eq!(state.snapshot().content_digest(), fresh.snapshot().content_digest());
    state.snapshot().validate_against(state.source())?;
    Ok(result)
}

#[test]
fn extending_an_interior_number_matches_fresh() -> TestResult {
    let mut state = IncrementalState::new("1;".to_string());
    check_edit(&mut state, 1, 1, "2")?;
    check_edit(&mut state, 2, 2, "3")?;
    Ok(())
}

#[test]
fn replacing_a_literal_is_a_control() -> TestResult {
    let mut state = IncrementalState::new("my $first = 1; my $last = 2;".to_string());
    check_edit(&mut state, 12, 13, "3")?;
    Ok(())
}

#[test]
fn extending_a_late_number_still_reuses_the_untouched_prefix() -> TestResult {
    let mut state = IncrementalState::new("my $first = 1; my $last = 2;".to_string());
    let end = state.source().find("2;").ok_or("last literal missing")? + 1;
    let result = check_edit(&mut state, end, end, "3")?;
    assert!(result.lex_restart.restart_byte > 0);
    assert!(result.lex_restart.reused_prefix_tokens > 0);
    check_edit(&mut state, end, end + 1, "")?;
    Ok(())
}

#[test]
fn editing_inside_a_number_does_not_accumulate_restart_anchors() -> TestResult {
    let mut state = IncrementalState::new("12;".to_string());
    for digit in ["3", "4", "5"] {
        check_edit(&mut state, 1, 2, digit)?;
        let fresh = IncrementalState::new(state.source().to_string());
        assert_eq!(state.stored_lex_checkpoint_count(), fresh.stored_lex_checkpoint_count());
    }
    Ok(())
}

#[test]
fn extending_an_interior_unicode_variable_matches_fresh() -> TestResult {
    let mut state = IncrementalState::new("$café;".to_string());
    let end = state.source().find(';').ok_or("semicolon missing")?;
    check_edit(&mut state, end, end, "s")?;
    check_edit(&mut state, end + 1, end + 1, "x")?;
    Ok(())
}

#[test]
fn deleting_a_number_separator_matches_fresh() -> TestResult {
    let mut state = IncrementalState::new("1;2;".to_string());
    check_edit(&mut state, 1, 2, "")?;
    check_edit(&mut state, 1, 1, ";")?;
    Ok(())
}

#[test]
fn replacing_a_number_terminator_matches_fresh() -> TestResult {
    let mut state = IncrementalState::new("12; print 3;".to_string());
    check_edit(&mut state, 2, 3, "3;")?;
    check_edit(&mut state, 2, 3, "")?;
    Ok(())
}
