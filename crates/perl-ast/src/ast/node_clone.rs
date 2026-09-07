//! Iterative [`Node`] clone.
//!
//! Canonical child fields are walked onto an explicit heap stack. Each parent
//! is rebuilt only after its cloned children exist. Payload and shape copy uses
//! a one-level derived [`NodeKind`] clone behind an operation-scoped
//! placeholder flag so child slots do not recurse on the thread stack.
//! The same engine powers [`Node::clone_with_mapped_locations`], keeping
//! position-only tree rewrites exhaustive and depth-safe.

use super::{Node, NodeKind, SourceLocation, Token};
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
    /// Clone the full tree while replacing every source location.
    ///
    /// The structural walk is the same iterative canonical traversal used by
    /// [`Clone`]. `map` is called once for every [`Node::location`], once for
    /// every independent [`SourceLocation`] stored in a [`NodeKind`] payload,
    /// and once for every recovery [`Token`] span. Recovery token text is
    /// immutable, so its mapped start is used while its original byte width is
    /// preserved.
    /// Its invocation order is intentionally unspecified; callers should derive
    /// each result from the supplied location rather than from traversal order.
    ///
    /// This is a full owned duplication, not an in-place edit or a shared view.
    /// Returns `None` when a mapped recovery-token span cannot be represented
    /// without losing the token's validated byte width.
    #[must_use]
    pub fn clone_with_mapped_locations<F>(&self, map: F) -> Option<Self>
    where
        F: Fn(SourceLocation) -> SourceLocation,
    {
        let mut failed = false;
        let cloned = clone_node_with_location_map(self, &mut (), &map, true, &mut failed);
        (!failed).then_some(cloned)
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

fn map_optional_location<F>(location: &mut Option<SourceLocation>, map: &F)
where
    F: Fn(SourceLocation) -> SourceLocation,
{
    if let Some(location) = location {
        *location = map(*location);
    }
}

fn map_token_span<F>(token: &mut Token, map: &F) -> bool
where
    F: Fn(SourceLocation) -> SourceLocation,
{
    let mapped = map(SourceLocation { start: token.start(), end: token.end() });
    let Some(mapped_end) = mapped.start.checked_add(token.len()) else {
        return false;
    };
    let Ok(mapped_token) = token.with_span(mapped.start, mapped_end) else {
        return false;
    };
    *token = mapped_token;
    true
}

/// Map every independent source span stored outside [`Node::location`].
///
/// The no-location arm is intentionally exhaustive and has no wildcard. A new
/// `NodeKind` variant therefore fails to compile here until its payload geometry
/// is classified. Recovery [`Token`] geometry is handled explicitly while
/// preserving the token text's validated byte width.
#[cfg(test)]
fn map_payload_locations<F>(kind: &mut NodeKind, map: &F)
where
    F: Fn(SourceLocation) -> SourceLocation,
{
    let _ = map_payload_locations_with_recovery(kind, map, true);
}

fn map_payload_locations_with_recovery<F>(
    kind: &mut NodeKind,
    map: &F,
    map_recovery_tokens: bool,
) -> bool
where
    F: Fn(SourceLocation) -> SourceLocation,
{
    match kind {
        NodeKind::Heredoc { body_span, .. } => map_optional_location(body_span, map),
        NodeKind::DataSection { marker_span, body_span, .. } => {
            map_optional_location(marker_span, map);
            map_optional_location(body_span, map);
        }
        NodeKind::Try { catch_blocks, .. } => {
            for (catch_variable, _) in catch_blocks {
                if let Some((_, location)) = catch_variable {
                    *location = map(*location);
                }
            }
        }
        NodeKind::Subroutine { name_span, .. }
        | NodeKind::Method { name_span, .. }
        | NodeKind::Class { name_span, .. }
        | NodeKind::Format { name_span, .. } => map_optional_location(name_span, map),
        NodeKind::Package { name_span, .. } => *name_span = map(*name_span),
        NodeKind::PhaseBlock { phase_span, .. } => map_optional_location(phase_span, map),
        NodeKind::Error { found, .. } => {
            if map_recovery_tokens
                && let Some(found) = found
                && !map_token_span(found, map)
            {
                return false;
            }
        }
        NodeKind::Program { .. }
        | NodeKind::ExpressionStatement { .. }
        | NodeKind::VariableDeclaration { .. }
        | NodeKind::VariableListDeclaration { .. }
        | NodeKind::NestedVariableList { .. }
        | NodeKind::Variable { .. }
        | NodeKind::VariableWithAttributes { .. }
        | NodeKind::Assignment { .. }
        | NodeKind::Binary { .. }
        | NodeKind::ArraySlice { .. }
        | NodeKind::HashSlice { .. }
        | NodeKind::KeyValueSlice { .. }
        | NodeKind::ChainedComparison { .. }
        | NodeKind::Ternary { .. }
        | NodeKind::Unary { .. }
        | NodeKind::Diamond
        | NodeKind::Ellipsis
        | NodeKind::Undef
        | NodeKind::Readline { .. }
        | NodeKind::Glob { .. }
        | NodeKind::Typeglob { .. }
        | NodeKind::Number { .. }
        | NodeKind::String { .. }
        | NodeKind::VString { .. }
        | NodeKind::ArrayLiteral { .. }
        | NodeKind::HashLiteral { .. }
        | NodeKind::Block { .. }
        | NodeKind::Eval { .. }
        | NodeKind::Do { .. }
        | NodeKind::Defer { .. }
        | NodeKind::If { .. }
        | NodeKind::LabeledStatement { .. }
        | NodeKind::While { .. }
        | NodeKind::Tie { .. }
        | NodeKind::Untie { .. }
        | NodeKind::For { .. }
        | NodeKind::Foreach { .. }
        | NodeKind::Given { .. }
        | NodeKind::When { .. }
        | NodeKind::Default { .. }
        | NodeKind::StatementModifier { .. }
        | NodeKind::Prototype { .. }
        | NodeKind::Signature { .. }
        | NodeKind::MandatoryParameter { .. }
        | NodeKind::OptionalParameter { .. }
        | NodeKind::SlurpyParameter { .. }
        | NodeKind::NamedParameter { .. }
        | NodeKind::Return { .. }
        | NodeKind::LoopControl { .. }
        | NodeKind::Goto { .. }
        | NodeKind::MethodCall { .. }
        | NodeKind::FunctionCall { .. }
        | NodeKind::AmperCall { .. }
        | NodeKind::IndirectCall { .. }
        | NodeKind::Regex { .. }
        | NodeKind::Match { .. }
        | NodeKind::Substitution { .. }
        | NodeKind::Transliteration { .. }
        | NodeKind::Use { .. }
        | NodeKind::No { .. }
        | NodeKind::Identifier { .. }
        | NodeKind::MissingExpression
        | NodeKind::MissingStatement
        | NodeKind::MissingIdentifier
        | NodeKind::MissingBlock
        | NodeKind::UnknownRest => {}
    }
    true
}

impl NodeKind {
    /// Map every independent source span stored outside [`Node::location`]
    /// in place.
    ///
    /// This is the in-place counterpart of the clone-path mapping engine
    /// behind [`Node::clone_with_mapped_locations`]: incremental position
    /// shifts call it so payload sub-spans move with the shift already
    /// applied to [`Node::location`] instead of staying at their pre-shift
    /// offsets. `map` must derive each result from the supplied location
    /// only; invocation order is unspecified.
    ///
    /// Returns `false` when a recovery [`Token`] span cannot be remapped
    /// without losing the token's validated byte width. The caller must then
    /// discard the mutated tree rather than accept it.
    pub fn map_payload_locations_in_place<F>(&mut self, map: F) -> bool
    where
        F: Fn(SourceLocation) -> SourceLocation,
    {
        map_payload_locations_with_recovery(self, &map, true)
    }
}

fn preserve_location(location: SourceLocation) -> SourceLocation {
    location
}

pub(super) fn clone_node<O: CloneObserver>(root: &Node, observer: &mut O) -> Node {
    let mut failed = false;
    clone_node_with_location_map(root, observer, &preserve_location, false, &mut failed)
}

fn clone_node_with_location_map<O, F>(
    root: &Node,
    observer: &mut O,
    map: &F,
    map_recovery_tokens: bool,
    failed: &mut bool,
) -> Node
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
                if !map_payload_locations_with_recovery(&mut cloned.kind, map, map_recovery_tokens)
                {
                    *failed = true;
                    return clone_slot_placeholder();
                }
                install_cloned_children(&mut cloned, cloned_children);
                observer.on_rebuild();
                done.push(cloned);
            }
        }
    }

    match done.pop() {
        Some(cloned) => cloned,
        None => clone_slot_placeholder(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CLONE_PAYLOAD_SHELL, CloneObserver, Node, NodeKind, ShellCloneGuard, SourceLocation, Token,
        clone_node, clone_payload_shell, clone_slot_placeholder, install_cloned_children,
        map_payload_locations, take_last_n_reversed,
    };
    use perl_token::TokenKind;
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
    fn mapped_location_clone_updates_every_canonical_node() -> Result<(), Box<dyn std::error::Error>>
    {
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

        let mapped = source
            .clone_with_mapped_locations(|location| {
                calls.set(calls.get().saturating_add(1));
                loc(location.start.saturating_add(10), location.end.saturating_add(10))
            })
            .ok_or("location mapping unexpectedly failed")?;

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
    fn mapped_location_clone_updates_recovery_token_span() -> Result<(), Box<dyn std::error::Error>>
    {
        let found = Token::new_checked(TokenKind::Semicolon, ";", 8, 9)?;
        let source = Node::new(
            NodeKind::Error {
                message: "missing expression".to_string(),
                expected: Vec::new(),
                found: Some(found),
                partial: None,
            },
            loc(8, 9),
        );

        let mapped = source
            .clone_with_mapped_locations(|location| {
                loc(location.start.saturating_add(10), location.end.saturating_add(10))
            })
            .ok_or("location mapping unexpectedly failed")?;

        let rejected = source.clone_with_mapped_locations(|location| loc(usize::MAX, location.end));
        assert!(rejected.is_none(), "invalid mapped token geometry must fail closed");

        let source_found = match &source.kind {
            NodeKind::Error { found: Some(found), .. } => found,
            other => return Err(format!("expected Error, got {}", other.kind_name()).into()),
        };
        let mapped_found = match &mapped.kind {
            NodeKind::Error { found: Some(found), .. } => found,
            other => return Err(format!("expected Error, got {}", other.kind_name()).into()),
        };

        assert_eq!(source.location, loc(8, 9));
        assert_eq!(source_found.start(), 8);
        assert_eq!(source_found.end(), 9);
        assert_eq!(mapped.location, loc(18, 19));
        assert_eq!(mapped_found.start(), 18);
        assert_eq!(mapped_found.end(), 19);
        assert_eq!(mapped_found.text.as_ref(), ";");
        Ok(())
    }

    #[test]
    fn payload_location_map_covers_every_source_location_family() {
        let shift = |location: SourceLocation| loc(location.start + 10, location.end + 10);

        let mut heredoc = NodeKind::Heredoc {
            delimiter: "EOF".to_string(),
            content: "body".to_string(),
            interpolated: false,
            indented: false,
            command: false,
            body_span: Some(loc(2, 6)),
        };
        map_payload_locations(&mut heredoc, &shift);
        assert!(
            matches!(heredoc, NodeKind::Heredoc { body_span: Some(span), .. } if span == loc(12, 16))
        );

        let mut try_block = NodeKind::Try {
            body: Box::new(numbered("1", 0)),
            catch_blocks: vec![(Some(("e".to_string(), loc(3, 5))), Box::new(numbered("2", 6)))],
            finally_block: None,
        };
        map_payload_locations(&mut try_block, &shift);
        assert!(matches!(
            try_block,
            NodeKind::Try { catch_blocks, .. }
                if matches!(&catch_blocks[0].0, Some((_, span)) if *span == loc(13, 15))
        ));

        let mut subroutine = NodeKind::Subroutine {
            name: Some("work".to_string()),
            name_span: Some(loc(4, 8)),
            declarator: None,
            prototype: None,
            signature: None,
            attributes: vec![],
            body: Box::new(numbered("3", 9)),
        };
        map_payload_locations(&mut subroutine, &shift);
        assert!(
            matches!(subroutine, NodeKind::Subroutine { name_span: Some(span), .. } if span == loc(14, 18))
        );

        let mut method = NodeKind::Method {
            name: "run".to_string(),
            name_span: Some(loc(5, 8)),
            signature: None,
            attributes: vec![],
            body: Box::new(numbered("4", 9)),
        };
        map_payload_locations(&mut method, &shift);
        assert!(
            matches!(method, NodeKind::Method { name_span: Some(span), .. } if span == loc(15, 18))
        );

        let mut package =
            NodeKind::Package { name: "Pkg".to_string(), name_span: loc(8, 11), block: None };
        map_payload_locations(&mut package, &shift);
        assert!(matches!(package, NodeKind::Package { name_span, .. } if name_span == loc(18, 21)));

        let mut phase = NodeKind::PhaseBlock {
            phase: "BEGIN".to_string(),
            phase_span: Some(loc(0, 5)),
            block: Box::new(numbered("5", 6)),
        };
        map_payload_locations(&mut phase, &shift);
        assert!(
            matches!(phase, NodeKind::PhaseBlock { phase_span: Some(span), .. } if span == loc(10, 15))
        );

        // A data section's marker and payload spans must shift with the node.
        // Leaving them behind would make the exact ranges the HIR shell
        // publishes point at unrelated bytes after any remap.
        let mut data_section = NodeKind::DataSection {
            marker: "__DATA__".to_string(),
            marker_span: Some(loc(0, 8)),
            body: Some("payload\n".to_string()),
            body_span: Some(loc(9, 17)),
        };
        map_payload_locations(&mut data_section, &shift);
        assert!(matches!(
            data_section,
            NodeKind::DataSection { marker_span: Some(m), body_span: Some(b), .. }
                if m == loc(10, 18) && b == loc(19, 27)
        ));

        // A marker with no payload keeps an absent payload span absent.
        let mut data_section_no_body = NodeKind::DataSection {
            marker: "__END__".to_string(),
            marker_span: Some(loc(0, 7)),
            body: None,
            body_span: None,
        };
        map_payload_locations(&mut data_section_no_body, &shift);
        assert!(matches!(
            data_section_no_body,
            NodeKind::DataSection { marker_span: Some(m), body_span: None, .. } if m == loc(10, 17)
        ));

        let mut class = NodeKind::Class {
            name: "Thing".to_string(),
            name_span: Some(loc(6, 11)),
            parents: vec![],
            body: Box::new(numbered("6", 12)),
        };
        map_payload_locations(&mut class, &shift);
        assert!(
            matches!(class, NodeKind::Class { name_span: Some(span), .. } if span == loc(16, 21))
        );

        let mut format = NodeKind::Format {
            name: "STDOUT".to_string(),
            name_span: Some(loc(7, 13)),
            body: String::new(),
        };
        map_payload_locations(&mut format, &shift);
        assert!(
            matches!(format, NodeKind::Format { name_span: Some(span), .. } if span == loc(17, 23))
        );
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
