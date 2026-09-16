#[cfg(feature = "semantic-overlay")]
use crate::SemanticOverlay;
use crate::parser::IncrementalState;
use crate::support::ast_has_error;
use crate::{IncrementalMetrics, InputEdit, Node, ParseDiagnostic, ReparseMode, TreeCursor};
use perl_ast::Node as AstNode;
use std::ops::Range;

/// The result of a successful parse: an owned syntax tree and the source text.
///
/// Use [`root_node`][Tree::root_node] to begin traversal.
#[derive(Debug, Clone)]
pub struct Tree {
    pub(crate) root: AstNode,
    pub(crate) source: String,
    /// Pending edits recorded via [`Tree::edit`].
    pub(crate) pending_edits: Vec<InputEdit>,
    pub(crate) diagnostics: Vec<ParseDiagnostic>,
    pub(crate) incremental_state: Option<IncrementalState>,
    pub(crate) reparse_mode: Option<ReparseMode>,
}

impl PartialEq for Tree {
    fn eq(&self, other: &Self) -> bool {
        self.root == other.root
            && self.source == other.source
            && self.pending_edits == other.pending_edits
            && self.diagnostics == other.diagnostics
    }
}

pub(crate) fn tree_from_parts(
    root: AstNode,
    source: &str,
    diagnostics: Vec<ParseDiagnostic>,
) -> Tree {
    Tree {
        root,
        source: source.to_string(),
        pending_edits: Vec::new(),
        incremental_state: None,
        diagnostics,
        reparse_mode: None,
    }
}

impl Tree {
    /// Returns the root node of the syntax tree.
    pub fn root_node(&self) -> Node<'_> {
        Node { inner: &self.root, tree_source: &self.source }
    }

    /// Returns the source text this tree was built from.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Returns the diagnostics collected while building this tree.
    pub fn diagnostics(&self) -> &[ParseDiagnostic] {
        &self.diagnostics
    }

    /// Returns the operation used to produce this tree from an old tree.
    pub fn reparse_mode(&self) -> Option<ReparseMode> {
        self.reparse_mode
    }

    /// Returns measurements for the most recent replay or fallback operation.
    ///
    /// Initial parses and unchanged-source reuse return `None`, so telemetry from
    /// a previous operation is never exposed as a no-op result.
    pub fn incremental_metrics(&self) -> Option<&IncrementalMetrics> {
        match self.reparse_mode {
            Some(ReparseMode::TokenReplay | ReparseMode::FullParseFallback(_)) => {
                self.incremental_state.as_ref().map(IncrementalState::metrics)
            }
            Some(ReparseMode::Unchanged) | None => None,
        }
    }

    /// Returns the source range reprocessed by the most recent operation.
    ///
    /// This reports lexer work, not a structural tree difference. Initial parses
    /// and unchanged-source reuse return an empty vector.
    pub fn reprocessed_ranges(&self) -> Vec<Range<usize>> {
        self.incremental_metrics()
            .map(|metrics| vec![metrics.changed_range.clone()])
            .unwrap_or_default()
    }

    /// Returns `true` when parsing produced diagnostics or an explicit error node.
    pub fn has_error(&self) -> bool {
        self.diagnostics.iter().any(ParseDiagnostic::blocks_clean_parse)
            || ast_has_error(&self.root)
    }

    /// Records a source edit on this tree, invalidating affected byte ranges.
    ///
    /// After calling `edit()`, pass this tree and the new source to
    /// [`crate::Parser::parse_with_old_tree`] to re-parse efficiently.
    ///
    /// In the current implementation this stores the edit for API compatibility;
    /// true incremental re-parsing (skipping unchanged regions) is a planned
    /// optimization.
    pub fn edit(&mut self, edit: &InputEdit) {
        self.pending_edits.push(edit.clone());
    }

    /// Returns a cursor positioned at the root node for stateful tree traversal.
    ///
    /// This mirrors `tree_sitter::Tree::walk()` and is equivalent to
    /// `tree.root_node().walk()`.
    pub fn walk(&self) -> TreeCursor<'_> {
        self.root_node().walk()
    }

    /// Returns the experimental semantic overlay query handle for this tree.
    #[cfg(feature = "semantic-overlay")]
    pub fn semantic_overlay(&self) -> SemanticOverlay<'_> {
        SemanticOverlay { tree: self }
    }
}
