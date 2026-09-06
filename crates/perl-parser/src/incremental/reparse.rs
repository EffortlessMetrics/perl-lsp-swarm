use crate::incremental::{
    IncrementalState, ParseSnapshotStrategy,
    diagnostics::{LexRestartReport, LexRestartStrategy, ReparseResult},
    edit::Edit,
    lex::{capture_live_checkpoint, lex_from_live_checkpoint, lex_source_with_checkpoints},
};
use anyhow::Result;
use perl_lexer::LexerCheckpoint;
use perl_source_identity::ContentDigest;
use std::ops::Range;

pub(crate) struct SingleEditReparse {
    pub(crate) range: Range<usize>,
    pub(crate) lex_restart: LexRestartReport,
    pub(crate) token_count: usize,
}

/// One resolved restart boundary for a single validated edit.
struct SelectedRestart {
    live_checkpoint: LexerCheckpoint,
    /// Old-source prefix bytes replayed only to reconstruct restart state.
    /// The canonical stored-checkpoint path reports zero.
    old_prefix_bytes_replayed: usize,
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

/// Resolve the restart state for one validated edit against the committed
/// generation.
///
/// Canonical path: restore the nearest persisted generation-bound checkpoint at
/// or before the edit start without replaying any old byte. Bounded fallback:
/// reproduce complete live state by replaying the old prefix to the nearest
/// boundary; those replayed bytes are reported honestly in the receipt.
fn select_restart(
    old_source: &str,
    old_digest: &ContentDigest,
    state: &IncrementalState,
    edit: &Edit,
) -> Result<SelectedRestart> {
    if let Some(live_checkpoint) = state
        .stored_lex_checkpoints()
        .iter()
        .rev()
        .filter(|stored| stored.summary.byte <= edit.start_byte)
        .find_map(|stored| stored.prepare_for_edit(old_source, old_digest, edit))
    {
        return Ok(SelectedRestart { live_checkpoint, old_prefix_bytes_replayed: 0 });
    }

    let boundary = state
        .tokens()
        .iter()
        .find(|token| token.end >= edit.start_byte)
        .map_or(edit.start_byte, |token| token.start);
    let Some(summary) = state.find_lex_checkpoint(boundary).copied() else {
        anyhow::bail!("No lexer restart boundary found");
    };
    let Some(mut live_checkpoint) = capture_live_checkpoint(old_source, summary.byte) else {
        anyhow::bail!("Could not reproduce complete live lexer state at restart boundary");
    };
    let old_len = edit
        .old_end_byte
        .checked_sub(edit.start_byte)
        .ok_or_else(|| anyhow::anyhow!("edit end precedes edit start"))?;
    if !live_checkpoint.try_apply_edit(edit.start_byte, old_len, edit.new_text.len()) {
        anyhow::bail!("Edit invalidated required live lexer state");
    }
    Ok(SelectedRestart { old_prefix_bytes_replayed: live_checkpoint.position(), live_checkpoint })
}

pub(crate) fn apply_single_edit(
    state: &mut IncrementalState,
    edit: &Edit,
) -> Result<SingleEditReparse> {
    let old_source = state.source().to_string();
    let old_digest = ContentDigest::of_bytes(old_source.as_bytes());

    let selected = select_restart(&old_source, &old_digest, state, edit)?;
    let restart_byte = selected.live_checkpoint.position();
    let reused_prefix_tokens =
        state.tokens().iter().take_while(|token| token.start < restart_byte).count();

    apply_text_edit_to_state(state, edit)?;

    // Surviving old-generation checkpoints carry forward only when every
    // behavior-bearing offset provably survives the edit. The edited generation
    // identity is computed once for the whole carry-forward set.
    let new_digest = ContentDigest::of_bytes(state.source().as_bytes());
    let lexed =
        lex_from_live_checkpoint(state.source(), state.line_index(), &selected.live_checkpoint)?;
    let mut stored_checkpoints = state
        .stored_lex_checkpoints()
        .iter()
        .filter_map(|checkpoint| {
            checkpoint.transform_for_generation(
                &old_source,
                state.source(),
                &old_digest,
                &new_digest,
                edit,
            )
        })
        .collect::<Vec<_>>();
    stored_checkpoints.extend(lexed.stored_checkpoints);

    let mut tokens = state.tokens()[..reused_prefix_tokens].to_vec();
    tokens.extend(lexed.tokens);

    let mut checkpoints = state
        .lex_checkpoints()
        .iter()
        .take_while(|checkpoint| checkpoint.byte < restart_byte)
        .copied()
        .collect::<Vec<_>>();
    checkpoints.extend(lexed.checkpoints);
    state.replace_lex_state(tokens, checkpoints, stored_checkpoints);

    let strategy = if selected.old_prefix_bytes_replayed == 0 {
        LexRestartStrategy::StoredCheckpointToEof
    } else {
        LexRestartStrategy::LiveCheckpointToEof
    };
    let lex_restart = LexRestartReport {
        strategy,
        restart_byte,
        old_prefix_bytes_replayed: selected.old_prefix_bytes_replayed,
        relexed_bytes: state.source().len().saturating_sub(restart_byte),
        reused_prefix_tokens,
        reused_suffix_tokens: 0,
        stored_checkpoint_count: state.stored_lex_checkpoint_count(),
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
        assert!(result.lex_restart.restart_byte > 0, "restart must reuse proven prefix state");
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
        assert!(
            result.lex_restart.restart_byte <= start,
            "restart must sit at or before the queued heredoc body"
        );
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

    #[test]
    fn missing_stored_checkpoints_fall_back_to_prefix_replay_with_honest_receipt() -> Result<()> {
        let source = "my $x = 1; my $y = 2;";
        let start = source.find("= 2").ok_or_else(|| anyhow::anyhow!("literal missing"))? + 2;
        let edit = Edit {
            start_byte: start,
            old_end_byte: start + 1,
            new_end_byte: start + 1,
            new_text: "9".to_string(),
        };

        // Byte-0 anchor invariant.
        let anchored = IncrementalState::new(source.to_string());
        let origin = anchored
            .stored_lex_checkpoints()
            .first()
            .ok_or_else(|| anyhow::anyhow!("origin checkpoint is missing"))?;
        assert_eq!(origin.summary.byte, 0, "origin checkpoint must anchor byte 0");
        let old_digest = ContentDigest::of_bytes(source.as_bytes());
        assert!(
            origin.prepare_for_edit(source, &old_digest, &edit).is_some(),
            "a stored checkpoint at byte 0 always qualifies for selection"
        );

        // Negative control: a fabricating restart that hardcoded the stored
        // strategy or reported zero replayed bytes must fail here.
        let mut state = IncrementalState::new(source.to_string());
        let tokens = state.tokens().to_vec();
        let summaries = state.lex_checkpoints().to_vec();
        state.replace_lex_state(tokens, summaries, Vec::new());
        assert_eq!(state.stored_lex_checkpoint_count(), 0);

        let result = apply_single_edit(&mut state, &edit)?;

        assert_eq!(result.lex_restart.strategy, LexRestartStrategy::LiveCheckpointToEof);
        assert!(
            result.lex_restart.restart_byte > 0,
            "fallback restart must replay a real old-source prefix"
        );
        assert_eq!(
            result.lex_restart.old_prefix_bytes_replayed, result.lex_restart.restart_byte,
            "fallback receipt must report every replayed prefix byte"
        );
        assert_tokens_equal(state.tokens(), &fresh_tokens(state.source()));
        Ok(())
    }
}
