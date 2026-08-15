use crate::incremental::{
    IncrementalState,
    diagnostics::{LexRestartReport, LexRestartStrategy, ReparseResult},
    edit::Edit,
    lex::{lex_from_live_checkpoint, lex_source_with_checkpoints},
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
    let old_source = state.source().to_string();
    let selected = state
        .stored_lex_checkpoints()
        .iter()
        .rev()
        .filter(|stored| stored.summary.byte <= edit.start_byte)
        .find_map(|stored| {
            stored.prepare_for_edit(&old_source, edit).map(|live| (stored.summary, live))
        })
        .ok_or_else(|| anyhow::anyhow!("No valid stored lexer checkpoint found"))?;
    let (summary, live_checkpoint) = selected;
    let restart_byte = live_checkpoint.position;
    let reused_prefix_tokens =
        state.tokens().iter().take_while(|token| token.start < restart_byte).count();
    let old_prefix_checkpoints = state
        .stored_lex_checkpoints()
        .iter()
        .take_while(|checkpoint| checkpoint.summary.byte < restart_byte)
        .cloned()
        .collect::<Vec<_>>();

    apply_text_edit_to_state(state, edit)?;
    let lexed = lex_from_live_checkpoint(state.source(), state.line_index(), &live_checkpoint)?;

    let mut tokens = state.tokens()[..reused_prefix_tokens].to_vec();
    tokens.extend(lexed.tokens);

    let mut checkpoint_summaries = state
        .lex_checkpoints()
        .iter()
        .take_while(|checkpoint| checkpoint.byte < restart_byte)
        .copied()
        .collect::<Vec<_>>();
    checkpoint_summaries.extend(lexed.checkpoints);

    let mut stored_checkpoints = old_prefix_checkpoints
        .iter()
        .filter_map(|checkpoint| {
            checkpoint.transform_for_generation(&old_source, state.source(), edit)
        })
        .collect::<Vec<_>>();
    stored_checkpoints.extend(lexed.stored_checkpoints);

    state.replace_lex_state(tokens, checkpoint_summaries, stored_checkpoints);

    let lex_restart = LexRestartReport {
        strategy: LexRestartStrategy::StoredCheckpointToEof,
        restart_byte,
        old_prefix_bytes_replayed: 0,
        relexed_bytes: state.source().len().saturating_sub(restart_byte),
        reused_prefix_tokens,
        reused_suffix_tokens: 0,
        stored_checkpoint_count: state.stored_lex_checkpoint_count(),
    };

    debug_assert_eq!(summary.byte, restart_byte);
    Ok(SingleEditReparse {
        range: restart_byte..state.source().len(),
        lex_restart,
        token_count: state.tokens().len(),
    })
}

pub(crate) fn full_reparse(state: &mut IncrementalState) -> Result<ReparseResult> {
    state.refresh_parse_output();
    let source_len = state.source().len();
    let lexed = lex_source_with_checkpoints(state.source(), state.line_index());
    state.replace_lex_state(lexed.tokens, lexed.checkpoints, lexed.stored_checkpoints);

    let lex_restart = LexRestartReport {
        strategy: LexRestartStrategy::FullRelex,
        restart_byte: 0,
        old_prefix_bytes_replayed: 0,
        relexed_bytes: source_len,
        reused_prefix_tokens: 0,
        reused_suffix_tokens: 0,
        stored_checkpoint_count: state.stored_lex_checkpoint_count(),
    };

    Ok(ReparseResult {
        changed_ranges: vec![0..source_len],
        parse_output: state.parse_output().clone(),
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
    fn equal_width_edit_restores_stored_state_without_replaying_old_bytes() -> Result<()> {
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

        assert_eq!(result.lex_restart.strategy, LexRestartStrategy::StoredCheckpointToEof);
        assert_eq!(result.lex_restart.old_prefix_bytes_replayed, 0);
        assert_eq!(result.lex_restart.reused_suffix_tokens, 0);
        assert_eq!(result.range.end, state.source().len());
        assert_tokens_equal(state.tokens(), &fresh_tokens(state.source()));
        Ok(())
    }

    #[test]
    fn method_name_edit_matches_fresh_lex_from_stored_state() -> Result<()> {
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

        assert_eq!(result.lex_restart.old_prefix_bytes_replayed, 0);
        assert_tokens_equal(state.tokens(), &fresh_tokens(state.source()));
        Ok(())
    }

    #[test]
    fn heredoc_body_edit_uses_an_earlier_safe_stored_checkpoint() -> Result<()> {
        let source = "my $value = <<EOF;\nbody\nEOF\nprint $value;\n";
        let start = source.find("body").ok_or_else(|| anyhow::anyhow!("body missing"))?;
        let edit = Edit {
            start_byte: start,
            old_end_byte: start + "body".len(),
            new_end_byte: start + "changed".len(),
            new_text: "changed".to_string(),
        };
        let mut state = IncrementalState::new(source.to_string());
        let result = apply_single_edit(&mut state, &edit)?;

        assert_eq!(result.lex_restart.strategy, LexRestartStrategy::StoredCheckpointToEof);
        assert_eq!(result.lex_restart.old_prefix_bytes_replayed, 0);
        assert_eq!(result.lex_restart.reused_suffix_tokens, 0);
        assert_tokens_equal(state.tokens(), &fresh_tokens(state.source()));
        Ok(())
    }

    #[test]
    fn sequential_edits_regenerate_current_generation_checkpoints() -> Result<()> {
        let source = "my $a = 1; my $b = 2; my $c = 3;";
        let mut state = IncrementalState::new(source.to_string());
        let first_start =
            source.find("= 2").ok_or_else(|| anyhow::anyhow!("first edit missing"))? + 2;
        let first = Edit {
            start_byte: first_start,
            old_end_byte: first_start + 1,
            new_end_byte: first_start + 1,
            new_text: "8".to_string(),
        };
        apply_single_edit(&mut state, &first)?;

        let second_start =
            state.source().find("= 3").ok_or_else(|| anyhow::anyhow!("second edit missing"))? + 2;
        let second = Edit {
            start_byte: second_start,
            old_end_byte: second_start + 1,
            new_end_byte: second_start + 1,
            new_text: "9".to_string(),
        };
        let result = apply_single_edit(&mut state, &second)?;

        assert_eq!(result.lex_restart.strategy, LexRestartStrategy::StoredCheckpointToEof);
        assert_eq!(result.lex_restart.old_prefix_bytes_replayed, 0);
        assert_tokens_equal(state.tokens(), &fresh_tokens(state.source()));
        Ok(())
    }
}
