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

/// Apply one edit through the shared lower-tier kernel when the legacy state
/// can be initialized safely.
///
/// The public `IncrementalState` still carries its historical caches, but
/// those caches are refreshed after the core operation so downstream callers
/// observing the old fields continue to see the current document.
fn try_apply_core_edit(state: &mut IncrementalState, edit: &Edit) -> Result<Option<ReparseResult>> {
    let start = edit.start_byte;
    let old_end = edit.old_end_byte;
    if start > old_end
        || old_end > state.source.len()
        || !state.source.is_char_boundary(start)
        || !state.source.is_char_boundary(old_end)
    {
        return Ok(None);
    }

    let remaining = state.source.len().checked_sub(old_end - start);
    let Some(capacity) = remaining.and_then(|value| value.checked_add(edit.new_text.len())) else {
        return Ok(None);
    };
    let mut new_source = String::with_capacity(capacity);
    new_source.push_str(&state.source[..start]);
    new_source.push_str(&edit.new_text);
    new_source.push_str(&state.source[old_end..]);

    let core_edit = crate::incremental_core::IncrementalEdit::new(start, old_end, &edit.new_text);
    let result = {
        let Some(core_state) = state.core_state.as_mut() else {
            return Ok(None);
        };
        match core_state.apply_edit(&new_source, &core_edit) {
            Ok(result) => result,
            Err(_) => return Ok(None),
        }
    };

    state.source = new_source;
    state.ast = result.ast;
    state.refresh_derived_state();

    Ok(Some(ReparseResult {
        changed_ranges: vec![result.metrics.changed_range],
        diagnostics: Vec::new(),
        reparsed_bytes: result.metrics.reparsed_bytes,
        reused_tokens: result.metrics.tokens_reused,
        token_count: state.tokens.len(),
    }))
}

/// Apply edits incrementally
pub fn apply_edits(state: &mut IncrementalState, edits: &[Edit]) -> Result<ReparseResult> {
    let mut sorted_edits = edits.to_vec();
    sorted_edits.sort_by_key(|e| e.start_byte);
    sorted_edits.reverse();

    let total_changed = sorted_edits.iter().map(Edit::touched_bytes).sum::<usize>();

    if sorted_edits.len() == 1 {
        if let Some(result) = try_apply_core_edit(state, &sorted_edits[0])? {
            return Ok(result);
        }
    }

    let result = if total_changed > MAX_EDIT_SIZE {
        full_reparse(state)?
    } else if sorted_edits.len() == 1 {
        let edit = &sorted_edits[0];

        if edit.touched_bytes() > 1024 || edit.new_text.matches('\n').count() > 10 {
            apply_text_edit_to_state(state, edit)?;
            full_reparse(state)?
        } else {
            match apply_single_edit(state, edit) {
                Ok(reparse) => {
                    let reparsed_bytes = reparse.range.end - reparse.range.start;
                    ReparseResult {
                        changed_ranges: vec![reparse.range],
                        diagnostics: vec![],
                        reparsed_bytes,
                        reused_tokens: reparse.reused_tokens,
                        token_count: reparse.token_count,
                    }
                }
                Err(_) => full_reparse(state)?,
            }
        }
    } else {
        for edit in sorted_edits {
            if apply_single_edit(state, &edit).is_err() {
                break;
            }
        }
        full_reparse(state)?
    };

    state.rebuild_core_state();
    Ok(result)
}

#[cfg(test)]
mod tests;
