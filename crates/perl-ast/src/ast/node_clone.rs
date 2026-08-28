//! Iterative [`Node`] clone.
//!
//! Canonical child fields are walked onto an explicit heap stack. Each parent
//! is rebuilt only after its cloned children exist. Payload and shape copy uses
//! a one-level derived [`NodeKind`] clone behind an operation-scoped
//! placeholder flag so child slots do not recurse on the thread stack.
//! The same engine powers [`Node::clone_with_mapped_locations`], keeping
//! position-only tree rewrites exhaustive and depth-safe.

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

impl Node {
    /// Clone the full tree while replacing every node's source location.
    ///
    /// The structural walk is the same iterative canonical traversal used by
    /// [`Clone`]. `map` is called exactly once for every node. Its invocation
    /// order is intentionally unspecified; callers should derive each result
    /// from the supplied location rather than from traversal order.
    ///
    /// This is a full owned duplication, not an in-place edit or a shared view.
    #[must_use]
    pub fn clone_with_mapped_locations<F>(&self, map: F) -> Self
    where
        F: Fn(SourceLocation) -> SourceLocation,
    {
        clone_node_with_location_map(self, &mut (), &map)
    }
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

fn preserve_location(location: SourceLocation) -> SourceLocation {
    location
}

pub(super) fn clone_node<O: CloneObserver>(root: &Node, observer: &mut O) -> Node {
    clone_node_with_location_map(root, observer, &preserve_location)
}

fn clone_node_with_location_map<O, F>(root: &Node, observer: &mut O, map: &F) -> Node
where
    O: CloneObserver,
    F: Fn(SourceLocation) -> SourceLocation,
{
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
                cloned.location = map(source.location);
                install_cloned_children(&mut cloned, cloned_children);
                observer.on_rebuild();
                done.push(cloned);
            }
        }
    }

    match done.pop() {
        Some(cloned) => cloned,
        None => {
            let mut cloned = clone_payload_shell(root);
            cloned.location = map(root.location);
            cloned
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CLONE_PAYLOAD_SHELL, CloneObserver, Node, NodeKind, ShellCloneGuard, SourceLocation,
        clone_node, clone_payload_shell, clone_slot_placeholder, install_cloned_children,
        take_last_n_reversed,
    };
    use std::cell::Cell;

    fn loc(start: usize, end: usize) -> SourceLocation {
        SourceLocation { start, end }
    }

    fn numbered(value: &str, start: usize) -> Node {
        Node::new(NodeKind::Number { value: value.to_string() }, loc(start, start + 1))
    }

    fn program(children: Vec<Node>) -> Node {
        let end = match children.last() {
            Some(child) => child.location.end,
            None => 0,
        };
        Node::new(NodeKind::Program { statements: children }, loc(0, end))
    }

    struct Recording {
        nodes_entered: u64,
        nodes_rebuilt: u64,
        child_edges: u64,
        max_explicit_stack_depth: usize,
    }

    impl CloneObserver for Recording {
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

    #[test]
    fn clone_observer_records_leaf_and_wide_child_work() {
        let leaf = numbered("7", 0);
        let mut leaf_work = Recording {
            nodes_entered: 0,
            nodes_rebuilt: 0,
            child_edges: 0,
            max_explicit_stack_depth: 0,
        };
        let cloned_leaf = clone_node(&leaf, &mut leaf_work);
        assert_eq!(leaf_work.nodes_entered, 1);
        assert_eq!(leaf_work.nodes_rebuilt, 1);
        assert_eq!(leaf_work.child_edges, 0);
        assert!(leaf_work.max_explicit_stack_depth >= 1);
        assert_eq!(cloned_leaf, leaf);

        let wide = program(vec![numbered("0", 0), numbered("1", 1), numbered("2", 2)]);
        let mut wide_work = Recording {
            nodes_entered: 0,
            nodes_rebuilt: 0,
            child_edges: 0,
            max_explicit_stack_depth: 0,
        };
        let cloned_wide = clone_node(&wide, &mut wide_work);
        assert_eq!(wide_work.nodes_entered, 4);
        assert_eq!(wide_work.nodes_rebuilt, 4);
        assert_eq!(wide_work.child_edges, 3);
        assert!(wide_work.max_explicit_stack_depth >= 3);
        assert_eq!(cloned_wide, wide);
        assert_eq!(wide.clone(), cloned_wide);
    }

    #[test]
    fn mapped_location_clone_updates_every_canonical_node()
    -> Result<(), Box<dyn std::error::Error>> {
        let binary = Node::new(
            NodeKind::Binary {
                op: "+".to_string(),
                left: Box::new(numbered("1", 0)),
                right: Box::new(numbered("2", 2)),
            },
            loc(0, 3),
        );
        let source = program(vec![binary]);
        let calls = Cell::new(0_u64);

        let mapped = source.clone_with_mapped_locations(|location| {
            calls.set(calls.get().saturating_add(1));
            loc(location.start.saturating_add(10), location.end.saturating_add(10))
        });

        assert_eq!(calls.get(), 4);
        assert_eq!(source.location, loc(0, 3), "mapping must not mutate the source tree");
        assert_eq!(mapped.location, loc(10, 13));

        let statements = match &mapped.kind {
            NodeKind::Program { statements } => statements,
            other => return Err(format!("expected Program, got {}", other.kind_name()).into()),
        };
        assert_eq!(statements.len(), 1);
        assert_eq!(statements[0].location, loc(10, 13));
        let (op, left, right) = match &statements[0].kind {
            NodeKind::Binary { op, left, right } => (op, left, right),
            other => return Err(format!("expected Binary, got {}", other.kind_name()).into()),
        };
        assert_eq!(op, "+");
        assert_eq!(left.location, loc(10, 11));
        assert_eq!(right.location, loc(12, 13));
        assert!(matches!(&left.kind, NodeKind::Number { value } if value == "1"));
        assert!(matches!(&right.kind, NodeKind::Number { value } if value == "2"));
        Ok(())
    }

    #[test]
    fn shell_clone_guard_saves_and_restores_previous_flag() {
        assert!(!CLONE_PAYLOAD_SHELL.with(Cell::get));
        {
            let _outer = ShellCloneGuard::enter();
            assert!(CLONE_PAYLOAD_SHELL.with(Cell::get));
            {
                let _inner = ShellCloneGuard::enter();
                assert!(CLONE_PAYLOAD_SHELL.with(Cell::get));
            }
            assert!(CLONE_PAYLOAD_SHELL.with(Cell::get));
        }
        assert!(!CLONE_PAYLOAD_SHELL.with(Cell::get));
    }

    #[test]
    fn clone_slot_placeholder_is_childless_ellipsis_at_zero_range() {
        let placeholder = clone_slot_placeholder();
        assert_eq!(placeholder.kind.kind_name(), "Ellipsis");
        assert_eq!(placeholder.location, loc(0, 0));
    }

    #[test]
    fn payload_shell_installs_placeholders_and_restores_tls() {
        let source = program(vec![numbered("1", 0), numbered("2", 2)]);
        let shell = clone_payload_shell(&source);
        match &shell.kind {
            NodeKind::Program { statements } => {
                assert_eq!(statements.len(), 2);
                assert_eq!(statements[0].kind.kind_name(), "Ellipsis");
                assert_eq!(statements[1].kind.kind_name(), "Ellipsis");
                assert_eq!(statements[0].location, loc(0, 0));
                assert_eq!(statements[1].location, loc(0, 0));
            }
            other => assert_eq!(other.kind_name(), "Program"),
        }
        assert!(!CLONE_PAYLOAD_SHELL.with(Cell::get));
        let leaf = numbered("9", 10);
        assert_eq!(leaf.clone(), leaf);
        match &leaf.clone().kind {
            NodeKind::Number { value } => assert_eq!(value, "9"),
            other => assert_eq!(other.kind_name(), "Number"),
        }
    }

    #[test]
    fn take_last_n_reversed_restores_visit_order_and_empty_take() {
        let mut done = vec![numbered("keep", 9), numbered("1", 1), numbered("0", 0)];
        let empty = take_last_n_reversed(&mut done, 0);
        assert!(empty.is_empty());
        assert_eq!(done.len(), 3);

        // Processing order is LIFO (child 1 rebuilt before child 0). Reverse
        // restores canonical visit order (child 0, then child 1).
        let taken = take_last_n_reversed(&mut done, 2);
        assert_eq!(taken.len(), 2);
        match (&taken[0].kind, &taken[1].kind) {
            (NodeKind::Number { value: first }, NodeKind::Number { value: second }) => {
                assert_eq!(first, "0");
                assert_eq!(second, "1");
            }
            (left, _) => assert_eq!(left.kind_name(), "Number"),
        }
        assert_eq!(taken[0].location.start, 0);
        assert_eq!(taken[1].location.start, 1);
        assert_eq!(done.len(), 1);
        match &done[0].kind {
            NodeKind::Number { value } => assert_eq!(value, "keep"),
            other => assert_eq!(other.kind_name(), "Number"),
        }
    }

    #[test]
    fn install_cloned_children_replaces_placeholders_in_order() {
        let source = program(vec![numbered("1", 0), numbered("2", 2), numbered("3", 4)]);
        let mut shell = clone_payload_shell(&source);
        install_cloned_children(
            &mut shell,
            vec![numbered("1", 0), numbered("2", 2), numbered("3", 4)],
        );
        assert_eq!(shell, source);
        match &shell.kind {
            NodeKind::Program { statements } => {
                assert_ne!(statements[0].kind.kind_name(), "Ellipsis");
                match &statements[1].kind {
                    NodeKind::Number { value } => assert_eq!(value, "2"),
                    other => assert_eq!(other.kind_name(), "Number"),
                }
            }
            other => assert_eq!(other.kind_name(), "Program"),
        }
    }

    #[test]
    fn install_cloned_children_keeps_placeholder_when_a_child_is_missing() {
        let source = program(vec![numbered("1", 0), numbered("2", 2)]);
        let mut shell = clone_payload_shell(&source);
        install_cloned_children(&mut shell, vec![numbered("1", 0)]);
        match &shell.kind {
            NodeKind::Program { statements } => {
                assert_eq!(statements.len(), 2);
                match &statements[0].kind {
                    NodeKind::Number { value } => assert_eq!(value, "1"),
                    other => assert_eq!(other.kind_name(), "Number"),
                }
                assert_eq!(statements[1].kind.kind_name(), "Ellipsis");
                assert_eq!(statements[1].location, loc(0, 0));
            }
            other => assert_eq!(other.kind_name(), "Program"),
        }
    }
}
