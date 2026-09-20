use crate::Node;
use crate::support::{ast_child_at, ast_child_count};
use perl_ast::Node as AstNode;

/// Stateful cursor for navigating a subtree.
///
/// The cursor is rooted at the [`Node`] that created it via [`Node::walk`].
/// Calling [`goto_parent`][TreeCursor::goto_parent] at the root returns `false`
/// and keeps the cursor at the root.
pub struct TreeCursor<'tree> {
    pub(crate) root: &'tree AstNode,
    pub(crate) tree_source: &'tree str,
    /// Child indices from `root` to the current node.
    pub(crate) path: Vec<usize>,
}

impl<'tree> TreeCursor<'tree> {
    /// Returns the node currently selected by the cursor.
    pub fn node(&self) -> Node<'tree> {
        Node { inner: self.current_ast_node(), tree_source: self.tree_source }
    }

    /// Moves to the first child of the current node.
    ///
    /// Returns `true` when movement succeeds, `false` when the node has no children.
    pub fn goto_first_child(&mut self) -> bool {
        if self.current_ast_node().first_child().is_none() {
            return false;
        }
        self.path.push(0);
        true
    }

    /// Moves to the last child of the current node.
    ///
    /// Returns `true` when movement succeeds, `false` when the node has no children.
    pub fn goto_last_child(&mut self) -> bool {
        let child_count = self.current_ast_node().children().len();
        if child_count == 0 {
            return false;
        }
        self.path.push(child_count - 1);
        true
    }

    /// Moves to the next sibling of the current node.
    ///
    /// Returns `true` on success. Returns `false` if the cursor is at root or if
    /// there is no next sibling.
    pub fn goto_next_sibling(&mut self) -> bool {
        if self.path.is_empty() {
            return false;
        }

        let parent = self.current_parent_ast_node();
        let sibling_count = ast_child_count(parent);
        let current_index = self.path[self.path.len() - 1];
        let next = current_index + 1;
        if next >= sibling_count {
            return false;
        }

        let last_pos = self.path.len() - 1;
        self.path[last_pos] = next;
        true
    }

    /// Moves to the previous sibling of the current node.
    ///
    /// Returns `true` on success. Returns `false` if the cursor is at root or if
    /// there is no previous sibling.
    pub fn goto_previous_sibling(&mut self) -> bool {
        if self.path.is_empty() {
            return false;
        }

        let current_index = self.path[self.path.len() - 1];
        if current_index == 0 {
            return false;
        }

        let last_pos = self.path.len() - 1;
        self.path[last_pos] = current_index - 1;
        true
    }

    /// Moves to the parent node.
    ///
    /// Returns `true` when movement succeeds, `false` when already at root.
    pub fn goto_parent(&mut self) -> bool {
        self.path.pop().is_some()
    }

    /// Resets the cursor back to its root node.
    pub fn reset(&mut self) {
        self.path.clear();
    }

    fn current_ast_node(&self) -> &'tree AstNode {
        resolve_path(self.root, &self.path)
    }

    fn current_parent_ast_node(&self) -> &'tree AstNode {
        debug_assert!(!self.path.is_empty(), "current_parent_ast_node requires a non-root cursor");
        let parent_path_len = self.path.len() - 1;
        resolve_path(self.root, &self.path[..parent_path_len])
    }
}

// Invariant: TreeCursor path is constructed by traversal methods in this type.
// If a stale/invalid path somehow appears, return the last valid node instead
// of panicking, preserving total API safety guarantees.
fn resolve_path<'tree>(root: &'tree AstNode, path: &[usize]) -> &'tree AstNode {
    let mut current = root;
    for &index in path {
        match ast_child_at(current, index) {
            Some(child) => current = child,
            None => {
                debug_assert!(false, "TreeCursor path must reference a valid child");
                break;
            }
        }
    }
    current
}
