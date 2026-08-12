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
use perl_parser_core::ast::{Node, NodeKind, SourceLocation};
use perl_parser_core::parser::Parser;

pub use perl_line_index::LineIndex;

pub use checkpoint::{LexCheckpoint, ParseCheckpoint, ScopeSnapshot};
pub use diagnostics::ReparseResult;
pub use edit::Edit;
use reparse::{apply_single_edit, apply_text_edit_to_state, full_reparse};
pub use state::IncrementalState;
pub use strategy::MAX_EDIT_SIZE;

#[path = "incremental_advanced_reuse.rs"]
mod incremental_advanced_reuse_engine;
#[path = "incremental_advanced_reuse_facade.rs"]
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

impl incremental_v2::IncrementalParserV2 {
    /// Return whether the last parse accepted an incrementally produced tree.
    ///
    /// The legacy `incremental_path_attempted` name is retained for compatibility,
    /// but its value is now recorded only after incremental parsing succeeds.
    pub fn used_incremental_path(&self) -> bool {
        self.incremental_path_attempted()
    }
}

/// Apply edits incrementally
pub fn apply_edits(state: &mut IncrementalState, edits: &[Edit]) -> Result<ReparseResult> {
    let mut sorted_edits = edits.to_vec();
    sorted_edits.sort_by_key(|e| e.start_byte);
    sorted_edits.reverse();

    let total_changed = sorted_edits.iter().map(Edit::touched_bytes).sum::<usize>();

    if total_changed > MAX_EDIT_SIZE {
        return full_reparse(state);
    }

    if sorted_edits.len() == 1 {
        let edit = &sorted_edits[0];

        if edit.touched_bytes() > 1024 || edit.new_text.matches('\n').count() > 10 {
            apply_text_edit_to_state(state, edit)?;
            return full_reparse(state);
        }

        let reparse = match apply_single_edit(state, edit) {
            Ok(reparse) => reparse,
            Err(_) => return full_reparse(state),
        };
        let reparsed_bytes = reparse.range.end - reparse.range.start;

        // Re-parse the AST from the updated source so that state.ast reflects
        // the edit (#5036). apply_single_edit only re-lexes tokens; without
        // this write-back, any consumer reading state.ast after apply_edits
        // gets the pre-edit tree.
        reparse_ast(state);

        Ok(ReparseResult {
            changed_ranges: vec![reparse.range],
            diagnostics: vec![],
            reparsed_bytes,
            reused_tokens: reparse.reused_tokens,
            token_count: reparse.token_count,
        })
    } else {
        for edit in sorted_edits {
            if apply_single_edit(state, &edit).is_err() {
                return full_reparse(state);
            }
        }
        full_reparse(state)
    }
}

/// Re-parse the AST from the current source text without re-lexing.
///
/// This is the AST write-back that `apply_single_edit` was missing (#5036).
/// After `apply_single_edit` updates `state.source` and `state.tokens`, this
/// function re-parses the full source to produce a fresh AST, so consumers
/// reading state.ast after apply_edits get the post-edit tree.
#[expect(
    deprecated,
    reason = "AST write-back is the legacy field's supported refresh boundary (#5036)"
)]
fn reparse_ast(state: &mut IncrementalState) {
    let mut parser = Parser::new(&state.source);
    state.ast = match parser.parse() {
        Ok(ast) => ast,
        Err(e) => Node::new(
            NodeKind::Error {
                message: e.to_string(),
                expected: vec![],
                found: None,
                partial: None,
            },
            SourceLocation { start: 0, end: state.source.len() },
        ),
    };
    state.parse_checkpoints = IncrementalState::create_parse_checkpoints(&state.ast);
}

#[cfg(test)]
mod tests;
