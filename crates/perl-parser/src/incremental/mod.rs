// Incremental parser internals are being split into private modules; facade
// documentation stays on the exported types and entry points.
#![allow(missing_docs)]

mod checkpoint;
mod diagnostics;
mod edit;
mod lex;
mod reparse;
mod state;
mod strategy;
mod work;

use anyhow::Result;

pub use perl_line_index::LineIndex;

pub use checkpoint::{LexCheckpoint, ParseCheckpoint, ScopeSnapshot};
pub use diagnostics::{LexRestartReport, LexRestartStrategy, ReparseResult};
pub use edit::Edit;
pub use lex::MAX_STORED_LEX_CHECKPOINTS;
use reparse::{apply_single_edit, apply_text_edit_to_state, full_reparse};
pub use state::IncrementalState;
pub use strategy::MAX_EDIT_SIZE;
pub use work::{
    IncrementalStrategy, IncrementalWorkReceipt, IncrementalWorkReceiptError,
};
use work::ParserInvocationReceipt;

pub mod incremental_advanced_reuse;
#[cfg(test)]
mod incremental_boundary_regressions;
pub mod incremental_checkpoint;
pub mod incremental_document;
pub mod incremental_edit;
pub mod incremental_handler_v2;
pub mod incremental_integration;
pub mod incremental_simple;
pub mod incremental_v2;

fn validate_edits(source: &str, edits: &[Edit]) -> Result<usize> {
    let mut by_start = edits.iter().collect::<Vec<_>>();
    by_start.sort_by_key(|edit| (edit.start_byte, edit.old_end_byte));

    let mut previous: Option<&Edit> = None;
    let mut total_changed = 0usize;

    for edit in by_start {
        if edit.start_byte > edit.old_end_byte || edit.old_end_byte > source.len() {
            anyhow::bail!(
                "incremental edit range {}..{} is invalid for source length {}",
                edit.start_byte,
                edit.old_end_byte,
                source.len()
            );
        }
        if !source.is_char_boundary(edit.start_byte)
            || !source.is_char_boundary(edit.old_end_byte)
        {
            anyhow::bail!(
                "incremental edit range {}..{} is not on UTF-8 boundaries",
                edit.start_byte,
                edit.old_end_byte
            );
        }

        let expected_new_end = edit
            .start_byte
            .checked_add(edit.new_text.len())
            .ok_or_else(|| anyhow::anyhow!("incremental edit new range overflows usize"))?;
        if edit.new_end_byte != expected_new_end {
            anyhow::bail!(
                "incremental edit new_end_byte {} does not match replacement end {}",
                edit.new_end_byte,
                expected_new_end
            );
        }

        if let Some(previous) = previous {
            if edit.start_byte < previous.old_end_byte || edit.start_byte == previous.start_byte {
                anyhow::bail!(
                    "incremental edit ranges are overlapping or share an ambiguous start: {}..{} and {}..{}",
                    previous.start_byte,
                    previous.old_end_byte,
                    edit.start_byte,
                    edit.old_end_byte
                );
            }
        }
        previous = Some(edit);

        total_changed = total_changed
            .checked_add(edit.touched_bytes())
            .ok_or_else(|| anyhow::anyhow!("incremental edit byte total overflows usize"))?;
    }

    Ok(total_changed)
}

fn unchanged_result(state: &IncrementalState) -> Result<ReparseResult> {
    let lex_restart = LexRestartReport {
        strategy: LexRestartStrategy::Unchanged,
        restart_byte: state.source().len(),
        old_prefix_bytes_replayed: 0,
        relexed_bytes: 0,
        reused_prefix_tokens: state.tokens().len(),
        reused_suffix_tokens: 0,
        stored_checkpoint_count: state.stored_lex_checkpoint_count(),
    };
    let work = IncrementalWorkReceipt::from_parts(
        IncrementalStrategy::Unchanged,
        ParserInvocationReceipt::default(),
        lex_restart,
        0,
        state.source().len(),
        state.tokens().len(),
        state.parse_output().ast.count_nodes(),
    );
    work.validate()?;
    Ok(ReparseResult {
        changed_ranges: Vec::new(),
        parse_output: state.parse_output().clone(),
        diagnostics: Vec::new(),
        lex_restart,
        work,
        reparsed_bytes: 0,
        reused_tokens: lex_restart.reused_tokens(),
        token_count: state.tokens().len(),
    })
}

fn apply_text_edits(state: &mut IncrementalState, edits_descending: &[Edit]) -> Result<()> {
    for edit in edits_descending {
        apply_text_edit_to_state(state, edit)?;
    }
    Ok(())
}

fn full_reparse_after_edits(
    state: &mut IncrementalState,
    edits_descending: &[Edit],
) -> Result<ReparseResult> {
    let mut candidate = state.clone();
    apply_text_edits(&mut candidate, edits_descending)?;
    let result = full_reparse(&mut candidate)?;
    *state = candidate;
    Ok(result)
}

/// Apply edits incrementally.
pub fn apply_edits(state: &mut IncrementalState, edits: &[Edit]) -> Result<ReparseResult> {
    let total_changed = validate_edits(state.source(), edits)?;
    if edits.is_empty() {
        return unchanged_result(state);
    }

    let mut sorted_edits = edits.to_vec();
    sorted_edits.sort_by_key(|edit| edit.start_byte);
    sorted_edits.reverse();

    if total_changed > MAX_EDIT_SIZE {
        return full_reparse_after_edits(state, &sorted_edits);
    }

    if sorted_edits.len() == 1 {
        let edit = &sorted_edits[0];

        if edit.touched_bytes() > 1024 || edit.new_text.matches('\n').count() > 10 {
            return full_reparse_after_edits(state, &sorted_edits);
        }

        let mut candidate = state.clone();
        let reparse = match apply_single_edit(&mut candidate, edit) {
            Ok(reparse) => reparse,
            Err(_) => return full_reparse_after_edits(state, &sorted_edits),
        };

        let parser_receipt = candidate.refresh_parse_output();
        let reused_tokens = reparse.lex_restart.reused_tokens();
        let final_node_count = candidate.parse_output().ast.count_nodes();
        let work = IncrementalWorkReceipt::from_parts(
            IncrementalStrategy::CheckpointToEofThenFullParse,
            parser_receipt,
            reparse.lex_restart,
            reparse.fresh_tokens_emitted,
            candidate.source().len(),
            reparse.token_count,
            final_node_count,
        );
        work.validate()?;
        let result = ReparseResult {
            changed_ranges: vec![reparse.range],
            parse_output: candidate.parse_output().clone(),
            diagnostics: vec![],
            lex_restart: reparse.lex_restart,
            work,
            reparsed_bytes: candidate.source().len(),
            reused_tokens,
            token_count: reparse.token_count,
        };
        *state = candidate;
        Ok(result)
    } else {
        full_reparse_after_edits(state, &sorted_edits)
    }
}

#[cfg(test)]
mod tests;