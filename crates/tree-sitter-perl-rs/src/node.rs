use crate::support::{
    ast_child_at, ast_child_count, ast_child_field, ast_children, ast_has_error, byte_to_point,
};
use crate::{FieldId, Point, TreeCursor};
use perl_ast::{Node as AstNode, NodeKind};
use std::ops::ControlFlow;

/// A borrowed reference to a node in the syntax tree.
///
/// Mirrors the tree-sitter `Node` API surface. Lifetime `'tree` is tied to the
/// owning [`crate::Tree`].
#[derive(Clone, Copy)]
pub struct Node<'tree> {
    pub(crate) inner: &'tree AstNode,
    pub(crate) tree_source: &'tree str,
}

impl<'tree> Node<'tree> {
    /// Returns the tree-sitter grammar-canonical node kind name.
    ///
    /// This matches tree-sitter expectations for `Node::kind()`, for example the
    /// root node kind is `"source_file"`. Use [`native_kind`][Node::native_kind]
    /// for the v3 parser's internal PascalCase kind name.
    pub fn kind(&self) -> String {
        self.grammar_kind()
    }

    /// Returns the v3 parser's internal node kind name (e.g. `"Program"`).
    pub fn native_kind(&self) -> &'static str {
        self.inner.kind.kind_name()
    }

    /// Returns the tree-sitter grammar-canonical node kind name.
    ///
    /// Alias of [`kind`][Node::kind] kept for compatibility.
    ///
    /// This method returns the grammar name
    /// used in S-expressions (e.g., `"source_file"`, `"sub"`).
    /// This matches the kind strings returned by `tree-sitter-perl-c` and the
    /// upstream tree-sitter Perl grammar.
    /// Error nodes use `"ERROR"` (uppercase), matching tree-sitter convention.
    ///
    /// For most nodes the grammar name is an allocation-free static mapping. For
    /// nodes whose name depends on runtime data (e.g., operator-named `Binary`
    /// or dynamic `VariableDeclaration`), only that node's fields are inspected.
    ///
    /// # Example
    ///
    /// ```rust
    /// use tree_sitter_perl_rs::Parser;
    ///
    /// let mut parser = Parser::new();
    /// let tree = parser.parse("my $x = 42;");
    /// assert!(tree.is_some());
    /// assert_eq!(tree.unwrap().root_node().grammar_kind(), "source_file");
    /// ```
    pub fn grammar_kind(&self) -> String {
        self.inner.kind.grammar_kind_name()
    }

    /// Returns `true` when this node is an explicit parser error node.
    pub fn is_error(&self) -> bool {
        matches!(self.inner.kind, NodeKind::Error { .. })
    }

    /// Returns `true` when this node or one of its descendants is an error node.
    pub fn has_error(&self) -> bool {
        ast_has_error(self.inner)
    }

    /// Returns the native debug S-expression for this node and its subtree.
    ///
    /// Delegates to [`perl_ast::Node::to_sexp`]. This is a non-normative debug
    /// projection, not a Tree-sitter compatibility CST. Compatibility serialization
    /// is tracked on issue 8047 (`perl-tree-sitter-compat`).
    pub fn to_sexp(&self) -> String {
        self.inner.to_sexp()
    }

    /// Returns the number of direct children.
    pub fn child_count(&self) -> usize {
        ast_child_count(self.inner)
    }

    /// Returns the `i`-th direct child, or `None` if out of range.
    pub fn child(&self, i: usize) -> Option<Node<'tree>> {
        ast_child_at(self.inner, i)
            .map(|child| Node { inner: child, tree_source: self.tree_source })
    }

    /// Returns the first direct child carrying the given named field.
    ///
    /// Unknown field names and fields absent from this node return `None`.
    pub fn child_by_field_name(&self, name: &str) -> Option<Node<'tree>> {
        let field = FieldId::from_name(name)?;
        let mut found = None;
        let _ = self.inner.try_for_each_child_with_field(|candidate, child| {
            if candidate == Some(field) {
                found = Some(Node { inner: child, tree_source: self.tree_source });
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        });
        found
    }

    /// Returns all direct children carrying the given named field, in source order.
    pub fn children_by_field_name(&self, name: &str) -> impl Iterator<Item = Node<'tree>> + '_ {
        let field = FieldId::from_name(name);
        let mut children = Vec::new();
        if let Some(field) = field {
            self.inner.for_each_child_with_field(|candidate, child| {
                if candidate == Some(field) {
                    children.push(Node { inner: child, tree_source: self.tree_source });
                }
            });
        }
        children.into_iter()
    }

    /// Returns the named field associated with the `index`-th direct child.
    pub fn field_name_for_child(&self, index: usize) -> Option<&'static str> {
        ast_child_field(self.inner, index).map(FieldId::name)
    }

    /// Returns an iterator over direct children.
    ///
    /// The iterator yields [`Node`] values sharing the same `'tree` lifetime as `self`.
    pub fn children(&self) -> impl Iterator<Item = Node<'tree>> + '_ {
        // Collect into a Vec so we can own the references. The lifetimes are valid
        // because all child nodes are part of the same owned tree (Tree::root).
        let kids = ast_children(self.inner);
        kids.into_iter().map(move |child| Node { inner: child, tree_source: self.tree_source })
    }

    /// Returns the start byte offset in the source text (inclusive).
    pub fn start_byte(&self) -> usize {
        self.inner.location.start
    }

    /// Returns the end byte offset in the source text (exclusive).
    pub fn end_byte(&self) -> usize {
        self.inner.location.end.min(self.tree_source.len())
    }

    /// Returns the start position as a tree-sitter-compatible [`Point`].
    ///
    /// `row`/`column` are zero-based and `column` is measured in bytes.
    pub fn start_position(&self) -> Point {
        byte_to_point(self.tree_source, self.start_byte())
    }

    /// Returns the end position as a tree-sitter-compatible [`Point`].
    ///
    /// `row`/`column` are zero-based and `column` is measured in bytes.
    pub fn end_position(&self) -> Point {
        byte_to_point(self.tree_source, self.end_byte())
    }

    /// Extracts the source text slice covered by this node.
    ///
    /// Returns `Err` only when the byte range contains invalid UTF-8, which is unlikely
    /// for content produced from a valid Rust `&str`.
    ///
    /// If the node's byte offsets extend beyond `source`, the result is clamped to
    /// the available range rather than panicking. This can happen when `source` is a
    /// different buffer than the one used to build the tree.
    pub fn utf8_text<'a>(&self, source: &'a [u8]) -> Result<&'a str, std::str::Utf8Error> {
        let start = self.inner.location.start.min(source.len());
        let end = self.inner.location.end.min(source.len());
        std::str::from_utf8(&source[start..end])
    }

    /// Returns `true` if this node has no children (is a leaf node).
    pub fn is_leaf(&self) -> bool {
        self.inner.first_child().is_none()
    }

    /// Returns the source text that was provided when creating the owning [`crate::Tree`].
    pub fn tree_source(&self) -> &'tree str {
        self.tree_source
    }

    /// Returns the inner `perl_ast::Node` for direct access to the v3 AST.
    ///
    /// This escape hatch lets callers use capabilities that go beyond the tree-sitter
    /// surface (e.g., match on [`crate::PerlNodeKind`] variants).
    pub fn inner(&self) -> &'tree AstNode {
        self.inner
    }

    /// Returns a cursor positioned at this node for stateful tree traversal.
    ///
    /// Mirrors `tree_sitter::TreeCursor` style navigation with a lightweight,
    /// allocation-free path stack.
    pub fn walk(&self) -> TreeCursor<'tree> {
        TreeCursor { root: self.inner, tree_source: self.tree_source, path: Vec::new() }
    }
}
