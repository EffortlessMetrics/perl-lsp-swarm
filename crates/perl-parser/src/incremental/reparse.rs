use crate::incremental::{
    IncrementalState,
    diagnostics::{LexRestartReport, LexRestartStrategy, ReparseResult},
    edit::Edit,
    lex::{lex_from_live_checkpoint, lex_source_with_checkpoints},
    work::{IncrementalStrategy, IncrementalWorkReceipt},
};
use anyhow::Result;
use std::ops::Range;

pub(crate) struct SingleEditReparse {
    pub(crate) range: Range<usize>,
    pub(crate) lex_restart: LexRestartReport,
    pub(crate) fresh_tokens_emitted: usize,
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
            stored
                .prepare_for_edit(&old_source, edit)
                .map(|live| (stored.summary, live))
        })
        .ok_or_else(|| anyhow::anyhow!("No valid stored lexer checkpoint found"))?;
    let (summary, live_checkpoint) = selected;
    let restart_byte = live_checkpoint.position;
    let reused_prefix_tokens = state
        .tokens()
        .iter()
        .take_while(|token| token.start < restart_byte)
        .count();
    let old_prefix_checkpoints = state
        .stored_lex_checkpoints()
        .iter()
        .take_while(|checkpoint| checkpoint.summary.byte < restart_byte)
        .cloned()
        .collect::<Vec<_>>();

    apply_text_edit_to_state(state, edit)?;
    let lexed = lex_from_live_checkpoint(state.source(), state.line_index(), &live_checkpoint)?;
    let fresh_tokens_emitted = lexed.tokens.len();

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
        fresh_tokens_emitted,
        token_count: state.tokens().len(),
    })
}

pub(crate) fn full_reparse(state: &mut IncrementalState) -> Result<ReparseResult> {
    let parser_receipt = state.refresh_parse_output();
    let source_len = state.source().len();
    let final_node_count = state.parse_output().ast.count_nodes();
    let lexed = lex_source_with_checkpoints(state.source(), state.line_index());
    let fresh_tokens_emitted = lexed.tokens.len();
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
    let work = IncrementalWorkReceipt::from_parts(
        IncrementalStrategy::FullFallback,
        parser_receipt,
        lex_restart,
        fresh_tokens_emitted,
        source_len,
        state.tokens().len(),
        final_node_count,
    );
    work.validate()?;

    Ok(ReparseResult {
        changed_ranges: vec![0..source_len],
        parse_output: state.parse_output().clone(),
        diagnostics: vec![],
        lex_restart,
        work,
        reparsed_bytes: source_len,
        reused_tokens: lex_restart.reused_tokens(),
        token_count: state.tokens().len(),
    })
}