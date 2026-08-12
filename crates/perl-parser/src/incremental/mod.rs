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

use anyhow::Result;

pub use perl_line_index::LineIndex;

pub use checkpoint::{LexCheckpoint, ParseCheckpoint, ScopeSnapshot};
pub use diagnostics::ReparseResult;
pub use edit::Edit;
use reparse::{apply_single_edit, apply_text_edit_to_state, full_reparse};
pub use state::IncrementalState;
pub use strategy::MAX_EDIT_SIZE;

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

fn unchanged_result(state: &IncrementalState) -> ReparseResult {
    ReparseResult {
        changed_ranges: Vec::new(),
        parse_output: state.parse_output().clone(),
        diagnostics: Vec::new(),
        reparsed_bytes: 0,
        reused_tokens: state.tokens().len(),
        token_count: state.tokens().len(),
    }
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
        return Ok(unchanged_result(state));
    }

    // Edits use coordinates from the same old source generation. Applying them
    // from the end preserves every earlier coordinate without offset adjustment.
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

        // Work on a candidate generation. A token-path failure may happen after
        // mutating source or token state; no partial generation is published.
        let mut candidate = state.clone();
        let reparse = match apply_single_edit(&mut candidate, edit) {
            Ok(reparse) => reparse,
            Err(_) => return full_reparse_after_edits(state, &sorted_edits),
        };

        // The token fast path does not define a second parser-output contract.
        // Refresh from the same recovery-aware parser entry point used by a
        // fresh parse, then report the complete parser work truthfully.
        candidate.refresh_parse_output();
        let reparsed_bytes = candidate.source().len();
        let result = ReparseResult {
            changed_ranges: vec![reparse.range],
            parse_output: candidate.parse_output().clone(),
            diagnostics: vec![],
            reparsed_bytes,
            reused_tokens: reparse.reused_tokens,
            token_count: reparse.token_count,
        };
        *state = candidate;
        Ok(result)
    } else {
        // Multi-edit batches already finish with a complete parser invocation.
        // Apply the whole validated batch first rather than publishing a prefix
        // when one intermediate token-restart attempt fails.
        full_reparse_after_edits(state, &sorted_edits)
    }
}

#[cfg(test)]
mod tests;
