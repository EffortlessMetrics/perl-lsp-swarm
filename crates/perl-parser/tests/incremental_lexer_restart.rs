#![cfg(feature = "incremental")]
//! Public-contract tests for correctness-first live lexer restart.

use perl_lexer::{PerlLexer, Token, TokenType};
use perl_parser::{
    Edit, IncrementalState, LexRestartStrategy, apply_edits,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn fresh_tokens(source: &str) -> Vec<Token> {
    let mut lexer = PerlLexer::new(source);
    let mut tokens = Vec::new();
    while let Some(token) = lexer.next_token() {
        if token.token_type == TokenType::EOF {
            break;
        }
        tokens.push(token);
    }
    tokens
}

fn assert_tokens_equal(actual: &[Token], expected: &[Token]) {
    assert_eq!(actual.len(), expected.len(), "token count diverged");
    for (index, (actual, expected)) in actual.iter().zip(expected).enumerate() {
        assert_eq!(actual.token_type, expected.token_type, "token kind {index}");
        assert_eq!(actual.text, expected.text, "token payload {index}");
        assert_eq!(actual.start, expected.start, "token start {index}");
        assert_eq!(actual.end, expected.end, "token end {index}");
    }
}

#[test]
fn late_equal_width_edit_retains_prefix_and_relexes_the_complete_suffix() -> TestResult {
    let source = "my $before = 1; my $target = 2; my $after = 3;";
    let start = source.find("= 2").ok_or("target literal is missing")? + 2;
    let edit = Edit {
        start_byte: start,
        old_end_byte: start + 1,
        new_end_byte: start + 1,
        new_text: "9".to_string(),
    };
    let mut state = IncrementalState::new(source.to_string());
    let result = apply_edits(&mut state, &[edit])?;

    assert_eq!(result.lex_restart.strategy, LexRestartStrategy::LiveCheckpointToEof);
    assert!(result.lex_restart.restart_byte > 0);
    assert!(result.lex_restart.reused_prefix_tokens > 0);
    assert_eq!(result.lex_restart.reused_suffix_tokens, 0);
    assert_eq!(
        result.lex_restart.relexed_bytes,
        state.source.len() - result.lex_restart.restart_byte
    );
    assert_eq!(result.reused_tokens, result.lex_restart.reused_tokens());
    assert_tokens_equal(&state.tokens, &fresh_tokens(&state.source));
    Ok(())
}

#[test]
fn method_context_edit_matches_fresh_lexing_after_complete_state_restore() -> TestResult {
    let source = "my $value = $object->method(); my $after = 1;";
    let start = source.find("method").ok_or("method name is missing")?;
    let edit = Edit {
        start_byte: start,
        old_end_byte: start + "method".len(),
        new_end_byte: start + "member".len(),
        new_text: "member".to_string(),
    };
    let mut state = IncrementalState::new(source.to_string());
    let result = apply_edits(&mut state, &[edit])?;

    assert_eq!(result.lex_restart.strategy, LexRestartStrategy::LiveCheckpointToEof);
    assert_eq!(result.lex_restart.reused_suffix_tokens, 0);
    assert_tokens_equal(&state.tokens, &fresh_tokens(&state.source));
    Ok(())
}

#[test]
fn large_edit_reports_full_relex_instead_of_checkpoint_reuse() -> TestResult {
    let source = "my $value = 1;";
    let replacement = "my $value = 2;\n".repeat(80);
    let edit = Edit {
        start_byte: 0,
        old_end_byte: source.len(),
        new_end_byte: replacement.len(),
        new_text: replacement,
    };
    let mut state = IncrementalState::new(source.to_string());
    let result = apply_edits(&mut state, &[edit])?;

    assert_eq!(result.lex_restart.strategy, LexRestartStrategy::FullRelex);
    assert_eq!(result.lex_restart.restart_byte, 0);
    assert_eq!(result.lex_restart.reused_prefix_tokens, 0);
    assert_eq!(result.lex_restart.reused_suffix_tokens, 0);
    assert_eq!(result.lex_restart.relexed_bytes, state.source.len());
    assert_tokens_equal(&state.tokens, &fresh_tokens(&state.source));
    Ok(())
}
