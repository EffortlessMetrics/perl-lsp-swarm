use crate::{Node, Tree};
use perl_ast::{Node as AstNode, NodeKind};
use perl_module::parse_module_import_head;
use perl_pragma::{PragmaState, PragmaTracker};
use perl_semantic_analyzer::semantic::SemanticModel;

/// Experimental semantic overlay query handle.
///
/// This API is intentionally limited while the facade integration is in development.
/// Query capabilities will expand over time.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct SemanticOverlay<'tree> {
    pub(crate) tree: &'tree Tree,
}

/// Symbol definition returned by [`SemanticOverlay`] queries.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct OverlayDefinition {
    /// Symbol name as written in source.
    pub name: String,
    /// Package-qualified symbol name.
    pub qualified_name: String,
    /// Symbol kind label (debug string form).
    pub kind: String,
    /// Definition span start byte (inclusive).
    pub start_byte: usize,
    /// Definition span end byte (exclusive).
    pub end_byte: usize,
}

/// Import statement visible at a specific source offset.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct VisibleImport {
    /// Imported module token (`Foo::Bar`, `strict`, etc.).
    pub module: String,
    /// Statement start byte (inclusive).
    pub statement_start_byte: usize,
    /// Statement end byte (exclusive).
    pub statement_end_byte: usize,
}

impl<'tree> SemanticOverlay<'tree> {
    /// Resolve a symbol definition at a byte offset in the source.
    pub fn definition_at_offset(&self, offset: usize) -> Option<OverlayDefinition> {
        let model = SemanticModel::build(&self.tree.root, self.tree.source());
        model.definition_at(offset).map(|symbol| OverlayDefinition {
            name: symbol.name.clone(),
            qualified_name: symbol.qualified_name.clone(),
            kind: format!("{:?}", symbol.kind),
            start_byte: symbol.location.start,
            end_byte: symbol.location.end,
        })
    }

    /// Resolve a symbol definition for the given node span.
    ///
    /// Uses the node start byte as the query point.
    pub fn definition_for_node(&self, node: &Node<'_>) -> Option<OverlayDefinition> {
        self.definition_at_offset(node.start_byte())
    }

    /// Returns the list of `use`-import modules visible at `offset`.
    ///
    /// Visibility is currently lexical-by-position: this returns `use` statements
    /// with starts less than or equal to `offset`.
    pub fn visible_imports_at_offset(&self, offset: usize) -> Vec<VisibleImport> {
        let mut imports = Vec::new();
        collect_visible_use_imports(&self.tree.root, self.tree.source(), offset, &mut imports);
        let mut deduped = Vec::new();
        for import in imports {
            if !deduped.iter().any(|existing: &VisibleImport| existing.module == import.module) {
                deduped.push(import);
            }
        }
        deduped
    }

    /// Returns the effective pragma state at a byte offset.
    pub fn pragma_state_at_offset(&self, offset: usize) -> PragmaState {
        let pragma_map = PragmaTracker::build(&self.tree.root);
        PragmaTracker::state_for_offset(&pragma_map, offset)
    }
}

fn collect_visible_use_imports(
    node: &AstNode,
    source: &str,
    offset: usize,
    out: &mut Vec<VisibleImport>,
) {
    // Only attempt import extraction on Use AST nodes. Container nodes (Program,
    // Block, Subroutine, etc.) span large source ranges that may accidentally start
    // with a `use` token, producing imports with incorrect statement byte ranges.
    // Use nodes have no children (for_each_child is a no-op), so they are visited
    // exactly once per tree traversal — no inner dedup needed.
    if matches!(node.kind, NodeKind::Use { .. }) && node.location.start <= offset {
        let start = node.location.start.min(source.len());
        let end = node.location.end.min(source.len());
        let statement_text = &source[start..end];
        if let Some(import_head) = parse_module_import_head(statement_text) {
            out.push(VisibleImport {
                module: import_head.token.to_string(),
                statement_start_byte: start,
                statement_end_byte: end,
            });
        }
    }

    node.for_each_child(|child| collect_visible_use_imports(child, source, offset, out));
}
