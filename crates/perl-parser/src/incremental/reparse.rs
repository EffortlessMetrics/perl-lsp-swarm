use crate::incremental::{
    IncrementalState, ParseSnapshotStrategy,
    diagnostics::{LexRestartReport, LexRestartStrategy, ReparseResult},
    edit::Edit,
    lex::{capture_live_checkpoint, lex_from_live_checkpoint, lex_source_with_checkpoints},
};
use anyhow::Result;
use std::ops::Range;

pub(crate) struct SingleEditReparse {
    pub(crate) range: Range<usize>,
    pub(crate) lex_restart: LexRestartReport,
    pub(crate) token_count: usize,
}

pub(crate) fn apply_text_edit_to_state(state: &mut IncrementalState, edit: &Edit) -> Result<()> {
    let old_end = edit.old_end_byte.min(state.source().len());
    let start = edit.start_byte.min(state.source().len());
    if !state.source().is_char_boundary(start) || !state.source().is_char_boundary(old_end) {
        anyhow::bail!("edit range is not on UTF-8 boundaries");
    }

    let mut new_source =
        String::with_capacity(state.source().len() - (old_end - start) + edit.new_text.len());
    new_source.push_str(&state.source()[..start]);
    new_source.push_str(&edit.new_text);
    new_source.push_str(&state.source()[old_end..]);
    state.replace_source_text(new_source);
    Ok(())
}

pub(crate) fn apply_single_edit(
    state: &mut IncrementalState,
    edit: &Edit,
) -> Result<SingleEditReparse> {
    let checkpoint_boundary = state
        .tokens()
        .iter()
        .find(|token| token.end >= edit.start_byte)
        .map_or(edit.start_byte, |token| token.start);
    let Some(summary) = state.find_lex_checkpoint(checkpoint_boundary).copied() else {
        apply_text_edit_to_state(state, edit)?;
        anyhow::bail!("No lexer restart boundary found");
    };
    let Some(mut live_checkpoint) = capture_live_checkpoint(state.source(), summary.byte) else {
        apply_text_edit_to_state(state, edit)?;
        anyhow::bail!("Could not reproduce complete live lexer state at restart boundary");
    };
    let old_len = edit
        .old_end_byte
        .checked_sub(edit.start_byte)
        .ok_or_else(|| anyhow::anyhow!("edit end precedes edit start"))?;
    if !live_checkpoint.try_apply_edit(edit.start_byte, old_len, edit.new_text.len()) {
        apply_text_edit_to_state(state, edit)?;
        anyhow::bail!("Edit invalidated required live lexer state");
    }

    let restart_byte = live_checkpoint.position;
    let reused_prefix_tokens =
        state.tokens().iter().take_while(|token| token.start < restart_byte).count();
    apply_text_edit_to_state(state, edit)?;

    let lexed = lex_from_live_checkpoint(state.source(), state.line_index(), &live_checkpoint)?;

    let mut tokens = state.tokens()[..reused_prefix_tokens].to_vec();
    tokens.extend(lexed.tokens);

    let mut checkpoints = state
        .lex_checkpoints()
        .iter()
        .take_while(|checkpoint| checkpoint.byte < restart_byte)
        .copied()
        .collect::<Vec<_>>();
    checkpoints.extend(lexed.checkpoints);
    state.replace_lex_state(tokens, checkpoints);

    let lex_restart = LexRestartReport {
        strategy: LexRestartStrategy::LiveCheckpointToEof,
        restart_byte,
        relexed_bytes: state.source().len().saturating_sub(restart_byte),
        reused_prefix_tokens,
        reused_suffix_tokens: 0,
    };

    Ok(SingleEditReparse {
        range: restart_byte..state.source().len(),
        lex_restart,
        token_count: state.tokens().len(),
    })
}

pub(crate) fn full_reparse(state: &mut IncrementalState) -> Result<ReparseResult> {
    state.refresh_parse_output(ParseSnapshotStrategy::IncrementalFullFallback)?;
    let source_len = state.source().len();
    let lexed = lex_source_with_checkpoints(state.source(), state.line_index());
    state.replace_lex_state(lexed.tokens, lexed.checkpoints);

    let lex_restart = LexRestartReport {
        strategy: LexRestartStrategy::FullRelex,
        restart_byte: 0,
        relexed_bytes: source_len,
        reused_prefix_tokens: 0,
        reused_suffix_tokens: 0,
    };

    Ok(ReparseResult {
        changed_ranges: vec![0..source_len],
        snapshot: state.snapshot().clone(),
        diagnostics: vec![],
        lex_restart,
        reparsed_bytes: source_len,
        reused_tokens: lex_restart.reused_tokens(),
        token_count: state.tokens().len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_lexer::{PerlLexer, Token, TokenType};

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
    fn equal_width_edit_relexes_to_eof_without_speculative_suffix_reuse() -> Result<()> {
        let source = "my $x = 1; my $y = 2;";
        let start = source.find("= 1").ok_or_else(|| anyhow::anyhow!("literal missing"))? + 2;
        let edit = Edit {
            start_byte: start,
            old_end_byte: start + 1,
            new_end_byte: start + 1,
            new_text: "9".to_string(),
        };
        let mut state = IncrementalState::new(source.to_string());
        let result = apply_single_edit(&mut state, &edit)?;

        assert_eq!(result.lex_restart.strategy, LexRestartStrategy::LiveCheckpointToEof);
        assert_eq!(result.lex_restart.reused_suffix_tokens, 0);
        assert_eq!(result.range.end, state.source.len());
        assert_tokens_equal(&state.tokens, &fresh_tokens(&state.source));
        Ok(())
    }

    #[test]
    fn method_name_edit_restores_after_arrow_state_and_matches_fresh_lex() -> Result<()> {
        let source = "$object->method(); my $x = 1;";
        let start = source.find("method").ok_or_else(|| anyhow::anyhow!("method missing"))?;
        let edit = Edit {
            start_byte: start,
            old_end_byte: start + "method".len(),
            new_end_byte: start + "member".len(),
            new_text: "member".to_string(),
        };
        let mut state = IncrementalState::new(source.to_string());
        let result = apply_single_edit(&mut state, &edit)?;

        assert_eq!(result.lex_restart.reused_suffix_tokens, 0);
        assert_tokens_equal(&state.tokens, &fresh_tokens(&state.source));
        Ok(())
    }

    #[test]
    fn token_boundary_insertion_relexes_adjoining_token_and_matches_fresh_lex() -> Result<()> {
        let source = "my $foo;";
        let token_start = source.find("$foo").ok_or_else(|| anyhow::anyhow!("variable missing"))?;
        let edit_start = token_start + "$foo".len();
        let edit = Edit {
            start_byte: edit_start,
            old_end_byte: edit_start,
            new_end_byte: edit_start + 1,
            new_text: "x".to_string(),
        };
        let mut state = IncrementalState::new(source.to_string());
        let result = apply_single_edit(&mut state, &edit)?;

        assert_eq!(result.lex_restart.strategy, LexRestartStrategy::LiveCheckpointToEof);
        assert_eq!(
            result.lex_restart.restart_byte, 2,
            "restart must begin before the adjoining variable token"
        );
        assert_tokens_equal(&state.tokens, &fresh_tokens(&state.source));
        Ok(())
    }

    #[test]
    fn heredoc_body_edit_fails_closed_then_full_reparse_matches_fresh_lex() -> Result<()> {
        let source = "my $value = <<EOF;\nbody\nEOF\nprint $value;\n";
        let start = source.find("body").ok_or_else(|| anyhow::anyhow!("body missing"))?;
        let edit = Edit {
            start_byte: start,
            old_end_byte: start + "body".len(),
            new_end_byte: start + "changed".len(),
            new_text: "changed".to_string(),
        };
        let mut state = IncrementalState::new(source.to_string());
        let result = apply_single_edit(&mut state, &edit);
        assert!(result.is_err(), "queued-heredoc restart must fail closed");
        let reparsed = full_reparse(&mut state)?;

        assert_eq!(reparsed.lex_restart.strategy, LexRestartStrategy::FullRelex);
        assert_tokens_equal(&state.tokens, &fresh_tokens(&state.source));
        Ok(())
    }

    #[test]
    fn length_changing_edit_keeps_every_token_span_in_current_source() -> Result<()> {
        let source = "my $a = 1;\nmy $b = 2;\nmy $c = 3;\n";
        let delete_len = "my $a = 1;\n".len();
        let edit = Edit {
            start_byte: 0,
            old_end_byte: delete_len,
            new_end_byte: 0,
            new_text: String::new(),
        };
        let mut state = IncrementalState::new(source.to_string());
        let result = apply_single_edit(&mut state, &edit)?;

        assert_eq!(result.lex_restart.restart_byte, 0);
        assert_eq!(result.lex_restart.reused_suffix_tokens, 0);
        assert_tokens_equal(&state.tokens, &fresh_tokens(&state.source));
        for token in &state.tokens {
            assert!(token.start <= token.end);
            assert!(token.end <= state.source.len());
            assert!(state.source.is_char_boundary(token.start));
            assert!(state.source.is_char_boundary(token.end));
        }
        Ok(())
    }
}
