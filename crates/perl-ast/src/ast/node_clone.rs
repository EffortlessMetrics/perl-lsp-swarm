//! Iterative [`Node`] clone.
//!
//! Canonical child fields are walked onto an explicit heap stack. Each parent
//! is rebuilt only after its cloned children exist. Payload and shape copy uses
//! a one-level derived [`NodeKind`] clone behind an operation-scoped
//! placeholder flag so child slots do not recurse on the thread stack.

use super::{Node, NodeKind, SourceLocation};
use std::cell::Cell;

thread_local! {
    /// When true, [`Node`]'s [`Clone`] implementation returns a childless placeholder.
    ///
    /// Derived [`NodeKind`] clone copies every non-child payload and every child
    /// slot. The iterative [`Node`] clone needs that payload/shape copy without
    /// recursively cloning descendants, so child `Node::clone` calls made while
    /// this flag is set become placeholders that `for_each_child_mut` then
    /// replaces with already-cloned children. The flag is operation-scoped
    /// (saved/restored, including on unwind) and is not a work counter.
    static CLONE_PAYLOAD_SHELL: Cell<bool> = const { Cell::new(false) };
}

/// Duplicate an owned [`Node`] tree without unbounded stack growth.
///
/// Cloning walks canonical child fields iteratively and rebuilds each parent
/// only after its cloned children are available. Non-child payloads, ranges,
/// child order, optional/repeated cardinality, and recovery state follow the
/// ordinary derived [`NodeKind`] clone. The public [`Clone`] contract is
/// unchanged: `node.clone()` still returns an independent owned tree.
///
/// Cloning is a full structural duplication, not a cheap shared projection.
/// Overflow is proven on a 50,000-node chain with a 256 KiB worker.
impl Clone for Node {
    fn clone(&self) -> Self {
        if CLONE_PAYLOAD_SHELL.with(Cell::get) {
            return clone_slot_placeholder();
        }
        clone_node(self, &mut ())
    }
}

/// Operation-local clone work recorded by [`clone_node`].
///
/// Counts are the clone operations actually performed for one call, not the
/// depth-bounded [`Node::count_nodes`] population of the result.
#[cfg(test)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct CloneWork {
    pub(super) nodes_entered: u64,
    pub(super) nodes_rebuilt: u64,
    pub(super) child_edges: u64,
    pub(super) max_explicit_stack_depth: usize,
}

pub(super) trait CloneObserver {
    fn on_enter(&mut self, child_count: usize);
    fn on_rebuild(&mut self);
    fn on_stack_depth(&mut self, depth: usize);
}

impl CloneObserver for () {
    fn on_enter(&mut self, _child_count: usize) {}
    fn on_rebuild(&mut self) {}
    fn on_stack_depth(&mut self, _depth: usize) {}
}

#[cfg(test)]
impl CloneObserver for CloneWork {
    fn on_enter(&mut self, child_count: usize) {
        self.nodes_entered = self.nodes_entered.saturating_add(1);
        self.child_edges = self.child_edges.saturating_add(child_count as u64);
    }

    fn on_rebuild(&mut self) {
        self.nodes_rebuilt = self.nodes_rebuilt.saturating_add(1);
    }

    fn on_stack_depth(&mut self, depth: usize) {
        if depth > self.max_explicit_stack_depth {
            self.max_explicit_stack_depth = depth;
        }
    }
}

struct ShellCloneGuard {
    previous: bool,
}

impl ShellCloneGuard {
    fn enter() -> Self {
        Self { previous: CLONE_PAYLOAD_SHELL.with(|flag| flag.replace(true)) }
    }
}

impl Drop for ShellCloneGuard {
    fn drop(&mut self) {
        CLONE_PAYLOAD_SHELL.with(|flag| flag.set(self.previous));
    }
}

fn clone_slot_placeholder() -> Node {
    // Same constructor Drop uses for detached slots so clone-created nodes
    // participate in `drop_audit` rather than destroying without a construct.
    Node::new(NodeKind::Ellipsis, SourceLocation { start: 0, end: 0 })
}

fn clone_payload_shell(source: &Node) -> Node {
    let _guard = ShellCloneGuard::enter();
    Node::new(source.kind.clone(), source.location)
}

fn take_last_n_reversed(done: &mut Vec<Node>, n: usize) -> Vec<Node> {
    let start = done.len().saturating_sub(n);
    let mut children = done.split_off(start);
    children.reverse();
    children
}

fn install_cloned_children(shell: &mut Node, children: Vec<Node>) {
    let mut next = children.into_iter();
    shell.for_each_child_mut(|slot| {
        if let Some(child) = next.next() {
            *slot = child;
        }
    });
}

pub(super) fn clone_node<O: CloneObserver>(root: &Node, observer: &mut O) -> Node {
    enum Work<'a> {
        Enter(&'a Node),
        Assemble { source: &'a Node, child_count: usize },
    }

    let mut work = vec![Work::Enter(root)];
    let mut done: Vec<Node> = Vec::new();
    observer.on_stack_depth(work.len());

    while let Some(item) = work.pop() {
        match item {
            Work::Enter(source) => {
                // One child walk: push child frames, then insert Assemble
                // underneath them so reconstruction stays postorder.
                let child_start = work.len();
                source.for_each_child(|child| work.push(Work::Enter(child)));
                let child_count = work.len().saturating_sub(child_start);
                work.insert(child_start, Work::Assemble { source, child_count });
                observer.on_enter(child_count);
                observer.on_stack_depth(work.len());
            }
            Work::Assemble { source, child_count } => {
                let cloned_children = take_last_n_reversed(&mut done, child_count);
                let mut cloned = clone_payload_shell(source);
                install_cloned_children(&mut cloned, cloned_children);
                observer.on_rebuild();
                done.push(cloned);
            }
        }
    }

    match done.pop() {
        Some(cloned) => cloned,
        None => clone_payload_shell(root),
    }
}
