//! Iterative [`Node`] structural equality.
//!
//! Canonical child fields are compared on an explicit heap stack. Non-child
//! payloads, discriminants, and child-slot cardinality reuse derived
//! [`NodeKind`] equality behind an operation-scoped skip so child `Node::eq`
//! calls do not re-enter the walk on the thread stack.

#[cfg(test)]
use super::FieldId;
use super::{Node, NodeKind};
use std::cell::Cell;

thread_local! {
    /// When true, [`Node`]'s [`PartialEq`] implementation treats a child slot
    /// as already handled by the iterative walker.
    ///
    /// Derived [`NodeKind`] equality compares every non-child payload and every
    /// child slot. The iterative [`Node`] walk needs that payload/shape compare
    /// without recursively comparing descendants, so child `Node::eq` calls
    /// made while this flag is set return `true`. Location, variant, payload,
    /// and child content are still compared by the heap walker. The flag is
    /// operation-scoped (saved/restored, including on unwind) and is not a
    /// work counter.
    static EQ_PAYLOAD_SHELL: Cell<bool> = const { Cell::new(false) };
}

/// Compare two owned [`Node`] trees without unbounded stack growth.
///
/// Equality walks canonical child fields iteratively. Non-child payloads,
/// ranges, child order, optional/repeated cardinality, and recovery state
/// follow the ordinary derived [`NodeKind`] equality for those fields. The
/// public [`PartialEq`] contract is unchanged: `left == right` remains exact
/// structural equality, not S-expression, fingerprint, or source-text
/// equality.
///
/// Overflow is proven on a 50,000-node chain with a 256 KiB worker.
impl PartialEq for Node {
    fn eq(&self, other: &Self) -> bool {
        if EQ_PAYLOAD_SHELL.with(Cell::get) {
            return true;
        }
        nodes_eq(self, other, &mut ())
    }
}

/// Operation-local equality work recorded by [`nodes_eq`].
pub(super) trait EqObserver {
    /// Called once per compared node pair.
    fn on_enter(&mut self);
    /// Called whenever the explicit work stack length is observed.
    fn on_stack_depth(&mut self, depth: usize);
}

impl EqObserver for () {
    fn on_enter(&mut self) {}
    fn on_stack_depth(&mut self, _depth: usize) {}
}

struct PayloadEqGuard {
    previous: bool,
}

impl PayloadEqGuard {
    fn enter() -> Self {
        Self { previous: EQ_PAYLOAD_SHELL.with(|flag| flag.replace(true)) }
    }
}

impl Drop for PayloadEqGuard {
    fn drop(&mut self) {
        EQ_PAYLOAD_SHELL.with(|flag| flag.set(self.previous));
    }
}

/// Payload, discriminant, and child-slot cardinality without child *content*.
///
/// Derived [`NodeKind`] equality still compares `Vec` lengths and `Option`
/// presence. Child `Node` values return `true` under the payload-shell flag
/// so this call cannot re-enter iterative [`Node::eq`] on the thread stack.
/// Same-variant wide nodes still visit each child *slot* during that derived
/// walk (the shell skips content, not cardinality), so a first-child mismatch
/// is O(width) in the payload shell. Avoiding that visit requires generated
/// payload slots (#8424), not a third handwritten child-field table.
fn payload_kind_eq(left: &NodeKind, right: &NodeKind) -> bool {
    let _guard = PayloadEqGuard::enter();
    left == right
}

/// Iterative exact structural equality used by [`PartialEq`] and tests.
pub(super) fn nodes_eq<O: EqObserver>(left: &Node, right: &Node, observer: &mut O) -> bool {
    let mut work = vec![(left, right)];
    observer.on_stack_depth(work.len());
    let mut left_scratch = Vec::new();
    let mut right_scratch = Vec::new();

    while let Some((left, right)) = work.pop() {
        observer.on_enter();
        if left.location != right.location {
            return false;
        }
        if !payload_kind_eq(&left.kind, &right.kind) {
            return false;
        }
        left_scratch.clear();
        right_scratch.clear();
        left.for_each_child(|child| left_scratch.push(child));
        right.for_each_child(|child| right_scratch.push(child));
        if left_scratch.len() != right_scratch.len() {
            return false;
        }
        // Reverse so the first canonical child is compared next (short-circuit
        // visits the prefix in source order).
        for (left_child, right_child) in left_scratch.iter().zip(right_scratch.iter()).rev() {
            work.push((*left_child, *right_child));
        }
        observer.on_stack_depth(work.len());
    }

    true
}

#[cfg(test)]
fn collect_children_with_field(node: &Node) -> Vec<(Option<FieldId>, &Node)> {
    let mut children = Vec::new();
    node.for_each_child_with_field(|field, child| children.push((field, child)));
    children
}

/// Why two nodes differed in a diagnostic compare.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DiffReason {
    /// [`Node::location`] differed.
    Location,
    /// [`NodeKind`] variant discriminant differed.
    Variant,
    /// A non-child payload (or pair-record metadata) differed.
    NonChildPayload,
    /// Optional/repeated child slot cardinality differed.
    FieldCardinality,
}

/// Bounded first-difference result. Never overrides public [`PartialEq`].
#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum StructuralCompare {
    /// Trees are exactly equal under current structural semantics.
    Equal {
        /// Nodes popped from the work stack.
        work: u64,
    },
    /// First inequality, with a bounded path and summaries.
    Different {
        /// Canonical field path to the differing node (`""` at the root).
        path: String,
        /// Child field of the parent when the difference is a child slot.
        field: Option<FieldId>,
        /// Classification of the first inequality.
        reason: DiffReason,
        /// Kind name plus range of the left node.
        left_summary: String,
        /// Kind name plus range of the right node.
        right_summary: String,
        /// Nodes compared before returning.
        work: u64,
    },
    /// Diagnostic work limit was hit. Public equality is unaffected.
    Truncated {
        /// Why the diagnostic stopped.
        reason: &'static str,
        /// Nodes compared before truncation.
        work: u64,
    },
}

#[cfg(test)]
struct CompareFrame<'a> {
    left: &'a Node,
    right: &'a Node,
    path: String,
    field: Option<FieldId>,
}

#[cfg(test)]
fn node_summary(node: &Node) -> String {
    format!("{}@{}..{}", node.kind.kind_name(), node.location.start, node.location.end)
}

#[cfg(test)]
fn child_path(parent: &str, field: Option<FieldId>, ordinal: usize) -> String {
    let name = field.map(FieldId::name).unwrap_or("child");
    if parent.is_empty() {
        format!("{name}[{ordinal}]")
    } else {
        format!("{parent}.{name}[{ordinal}]")
    }
}

/// Test-support first-difference comparator.
///
/// `max_work` bounds only this diagnostic. It cannot change [`PartialEq`].
#[cfg(test)]
pub(super) fn compare_structural(
    left: &Node,
    right: &Node,
    max_work: Option<u64>,
) -> StructuralCompare {
    let mut work_stack = vec![CompareFrame { left, right, path: String::new(), field: None }];
    let mut work = 0u64;

    while let Some(frame) = work_stack.pop() {
        work = work.saturating_add(1);
        if let Some(limit) = max_work
            && work > limit
        {
            return StructuralCompare::Truncated { reason: "diagnostic work limit", work };
        }

        if frame.left.location != frame.right.location {
            return StructuralCompare::Different {
                path: frame.path,
                field: frame.field,
                reason: DiffReason::Location,
                left_summary: node_summary(frame.left),
                right_summary: node_summary(frame.right),
                work,
            };
        }

        if frame.left.kind.kind_name() != frame.right.kind.kind_name() {
            return StructuralCompare::Different {
                path: frame.path,
                field: frame.field,
                reason: DiffReason::Variant,
                left_summary: node_summary(frame.left),
                right_summary: node_summary(frame.right),
                work,
            };
        }

        let left_children = collect_children_with_field(frame.left);
        let right_children = collect_children_with_field(frame.right);
        if left_children.len() != right_children.len() {
            return StructuralCompare::Different {
                path: frame.path,
                field: frame.field,
                reason: DiffReason::FieldCardinality,
                left_summary: node_summary(frame.left),
                right_summary: node_summary(frame.right),
                work,
            };
        }

        if !payload_kind_eq(&frame.left.kind, &frame.right.kind) {
            return StructuralCompare::Different {
                path: frame.path,
                field: frame.field,
                reason: DiffReason::NonChildPayload,
                left_summary: node_summary(frame.left),
                right_summary: node_summary(frame.right),
                work,
            };
        }

        let mut child_frames = Vec::with_capacity(left_children.len());
        for (ordinal, ((left_field, left_child), (right_field, right_child))) in
            left_children.into_iter().zip(right_children).enumerate()
        {
            let field = left_field.or(right_field);
            child_frames.push(CompareFrame {
                left: left_child,
                right: right_child,
                path: child_path(&frame.path, field, ordinal),
                field,
            });
        }
        for child in child_frames.into_iter().rev() {
            work_stack.push(child);
        }
    }

    StructuralCompare::Equal { work }
}

#[cfg(test)]
mod tests {
    use super::super::SourceLocation;
    use super::{
        DiffReason, EQ_PAYLOAD_SHELL, EqObserver, Node, NodeKind, PayloadEqGuard,
        StructuralCompare, compare_structural, nodes_eq, payload_kind_eq,
    };
    use perl_token::{Token, TokenKind};
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

    fn wrap_expr(inner: Node) -> Node {
        let location = inner.location;
        Node::new(NodeKind::ExpressionStatement { expression: Box::new(inner) }, location)
    }

    struct Recording {
        nodes_entered: u64,
        max_explicit_stack_depth: usize,
    }

    impl EqObserver for Recording {
        fn on_enter(&mut self) {
            self.nodes_entered = self.nodes_entered.saturating_add(1);
        }

        fn on_stack_depth(&mut self, depth: usize) {
            if depth > self.max_explicit_stack_depth {
                self.max_explicit_stack_depth = depth;
            }
        }
    }

    fn record(left: &Node, right: &Node) -> (bool, Recording) {
        let mut work = Recording { nodes_entered: 0, max_explicit_stack_depth: 0 };
        let equal = nodes_eq(left, right, &mut work);
        (equal, work)
    }

    #[test]
    fn equal_leaves_and_wide_programs_compare() {
        let leaf = numbered("7", 0);
        assert_eq!(leaf, numbered("7", 0));
        assert_ne!(leaf, numbered("8", 0));
        assert_ne!(leaf, numbered("7", 1));

        let wide = program(vec![numbered("0", 0), numbered("1", 1), numbered("2", 2)]);
        assert_eq!(wide, program(vec![numbered("0", 0), numbered("1", 1), numbered("2", 2)]));
        assert_ne!(wide, program(vec![numbered("2", 2), numbered("1", 1), numbered("0", 0)]));
    }

    #[test]
    fn root_mismatch_does_not_visit_descendants() {
        let left = program(vec![numbered("0", 0), numbered("1", 1), numbered("2", 2)]);
        let mut right = program(vec![numbered("0", 0), numbered("1", 1), numbered("2", 2)]);
        right.location = loc(9, 10);
        let (equal, work) = record(&left, &right);
        assert!(!equal);
        assert_eq!(work.nodes_entered, 1, "root location mismatch must not walk children");
    }

    #[test]
    fn first_child_mismatch_does_not_visit_later_siblings() {
        let left = program(vec![numbered("0", 0), numbered("1", 1), numbered("2", 2)]);
        let right = program(vec![numbered("x", 0), numbered("1", 1), numbered("2", 2)]);
        let (equal, work) = record(&left, &right);
        assert!(!equal);
        assert_eq!(work.nodes_entered, 2, "prefix mismatch must not compare later statements");
    }

    #[test]
    fn optional_none_is_not_equal_to_some() {
        let var = |name: &str| {
            Node::new(
                NodeKind::Variable { sigil: "$".to_string(), name: name.to_string() },
                loc(0, 2),
            )
        };
        let none = Node::new(
            NodeKind::VariableDeclaration {
                declarator: "my".to_string(),
                variable: Box::new(var("x")),
                attributes: vec![],
                initializer: None,
            },
            loc(0, 6),
        );
        let some = Node::new(
            NodeKind::VariableDeclaration {
                declarator: "my".to_string(),
                variable: Box::new(var("x")),
                attributes: vec![],
                initializer: Some(Box::new(numbered("1", 5))),
            },
            loc(0, 6),
        );
        assert_ne!(none, some);
        assert_ne!(some, none);
    }

    #[test]
    fn omitted_non_child_payloads_are_material() {
        let left = Node::new(
            NodeKind::VariableDeclaration {
                declarator: "my".to_string(),
                variable: Box::new(Node::new(
                    NodeKind::Variable { sigil: "$".to_string(), name: "x".to_string() },
                    loc(3, 5),
                )),
                attributes: vec![":shared".to_string()],
                initializer: None,
            },
            loc(0, 14),
        );
        let mut right = Node::new(
            NodeKind::VariableDeclaration {
                declarator: "our".to_string(),
                variable: Box::new(Node::new(
                    NodeKind::Variable { sigil: "$".to_string(), name: "x".to_string() },
                    loc(3, 5),
                )),
                attributes: vec![":shared".to_string()],
                initializer: None,
            },
            loc(0, 14),
        );
        assert_ne!(left, right);
        right.kind = NodeKind::VariableDeclaration {
            declarator: "my".to_string(),
            variable: Box::new(Node::new(
                NodeKind::Variable { sigil: "$".to_string(), name: "x".to_string() },
                loc(3, 5),
            )),
            attributes: vec![],
            initializer: None,
        };
        assert_ne!(left, right);
    }

    #[test]
    fn heredoc_body_span_is_material_and_absent_from_sexp() {
        let base = |span: Option<SourceLocation>| {
            Node::new(
                NodeKind::Heredoc {
                    delimiter: "EOF".to_string(),
                    content: "hi".to_string(),
                    interpolated: false,
                    indented: false,
                    command: false,
                    body_span: span,
                },
                loc(0, 3),
            )
        };
        let left = base(Some(loc(4, 6)));
        let right = base(Some(loc(8, 10)));
        assert_eq!(left.to_sexp(), right.to_sexp(), "sexp omits body_span");
        assert_ne!(left, right);
        assert_eq!(left, base(Some(loc(4, 6))));
    }

    #[test]
    fn error_expected_tokens_are_material_and_visible_in_sexp()
    -> Result<(), perl_token::TokenSpanError> {
        let left = Node::new(
            NodeKind::Error {
                message: "oops".to_string(),
                expected: vec![TokenKind::Identifier],
                found: None,
                partial: None,
            },
            loc(0, 1),
        );
        let right = Node::new(
            NodeKind::Error {
                message: "oops".to_string(),
                expected: vec![TokenKind::Number],
                found: None,
                partial: None,
            },
            loc(0, 1),
        );
        assert_ne!(left.to_sexp(), right.to_sexp(), "recovery expected tokens must be visible");
        assert_ne!(left, right);
        let with_found = Node::new(
            NodeKind::Error {
                message: "oops".to_string(),
                expected: vec![TokenKind::Identifier],
                found: Some(Token::new_checked(TokenKind::Identifier, "x", 0, 1)?),
                partial: None,
            },
            loc(0, 1),
        );
        assert_ne!(left.to_sexp(), with_found.to_sexp(), "found token must be visible");
        assert_ne!(left, with_found);
        assert!(left.to_sexp().contains("expected"), "sexp = {}", left.to_sexp());
        assert!(with_found.to_sexp().contains("found"), "sexp = {}", with_found.to_sexp());
        Ok(())
    }

    #[test]
    fn subroutine_name_span_is_material_and_absent_from_sexp() {
        let body = Node::new(NodeKind::Block { statements: vec![] }, loc(10, 12));
        let left = Node::new(
            NodeKind::Subroutine {
                name: Some("foo".to_string()),
                name_span: Some(loc(4, 7)),
                declarator: None,
                prototype: None,
                signature: None,
                attributes: vec![],
                body: Box::new(body.clone()),
            },
            loc(0, 12),
        );
        let span_moved = Node::new(
            NodeKind::Subroutine {
                name: Some("foo".to_string()),
                name_span: Some(loc(5, 8)),
                declarator: None,
                prototype: None,
                signature: None,
                attributes: vec![],
                body: Box::new(body.clone()),
            },
            loc(0, 12),
        );
        let lexical = Node::new(
            NodeKind::Subroutine {
                name: Some("foo".to_string()),
                name_span: Some(loc(4, 7)),
                declarator: Some("my".to_string()),
                prototype: None,
                signature: None,
                attributes: vec![],
                body: Box::new(body),
            },
            loc(0, 12),
        );
        assert_eq!(left.to_sexp(), span_moved.to_sexp(), "sexp omits name_span");
        assert_ne!(left.to_sexp(), lexical.to_sexp(), "declarator is a debug payload");
        assert_ne!(left, span_moved);
        assert_ne!(left, lexical);
        assert!(lexical.to_sexp().contains("declarator"), "sexp = {}", lexical.to_sexp());
    }

    #[test]
    fn catch_variable_payload_is_material() {
        let block = |n: u64| {
            Node::new(NodeKind::Block { statements: vec![numbered(&n.to_string(), 0)] }, loc(0, 1))
        };
        let left = Node::new(
            NodeKind::Try {
                body: Box::new(block(1)),
                catch_blocks: vec![(Some(("err".to_string(), loc(4, 7))), Box::new(block(2)))],
                finally_block: None,
            },
            loc(0, 20),
        );
        let right = Node::new(
            NodeKind::Try {
                body: Box::new(block(1)),
                catch_blocks: vec![(Some(("other".to_string(), loc(4, 7))), Box::new(block(2)))],
                finally_block: None,
            },
            loc(0, 20),
        );
        assert_ne!(left, right);
    }

    #[test]
    fn if_keyword_payload_is_material() {
        let num = |v: &str| Box::new(numbered(v, 0));
        let left = Node::new(
            NodeKind::If {
                condition: num("1"),
                then_branch: num("2"),
                elsif_branches: vec![],
                else_branch: None,
                keyword: None,
            },
            loc(0, 1),
        );
        let unless_kw = Node::new(
            NodeKind::If {
                condition: num("1"),
                then_branch: num("2"),
                elsif_branches: vec![],
                else_branch: None,
                keyword: Some("unless".to_string()),
            },
            loc(0, 1),
        );
        assert_ne!(left, unless_kw);
    }

    #[test]
    fn pair_record_order_is_material() {
        let left = Node::new(
            NodeKind::HashLiteral {
                pairs: vec![
                    (numbered("k", 0), numbered("v", 1)),
                    (numbered("a", 2), numbered("b", 3)),
                ],
            },
            loc(0, 4),
        );
        let swapped = Node::new(
            NodeKind::HashLiteral {
                pairs: vec![
                    (numbered("a", 2), numbered("b", 3)),
                    (numbered("k", 0), numbered("v", 1)),
                ],
            },
            loc(0, 4),
        );
        assert_ne!(left, swapped);
        assert_eq!(
            left,
            Node::new(
                NodeKind::HashLiteral {
                    pairs: vec![
                        (numbered("k", 0), numbered("v", 1)),
                        (numbered("a", 2), numbered("b", 3))
                    ],
                },
                loc(0, 4),
            )
        );
    }

    #[test]
    fn equality_laws_hold_on_representative_trees() {
        let a = wrap_expr(program(vec![numbered("1", 0), numbered("2", 1)]));
        let b = wrap_expr(program(vec![numbered("1", 0), numbered("2", 1)]));
        let c = wrap_expr(program(vec![numbered("1", 0), numbered("2", 1)]));
        assert_eq!(a == b, b == a, "symmetric");
        assert_eq!(a, b);
        assert_eq!(b, c);
        assert_eq!(a, c, "transitive");
        let d = wrap_expr(program(vec![numbered("1", 0), numbered("3", 1)]));
        assert_eq!(a == d, d == a);
        assert_ne!(a, d);
    }

    #[test]
    fn diagnostic_reports_location_variant_payload_and_cardinality() {
        let base = numbered("1", 0);
        match compare_structural(&base, &numbered("1", 9), None) {
            StructuralCompare::Different { reason, path, .. } => {
                assert_eq!(reason, DiffReason::Location);
                assert_eq!(path, "");
            }
            other => assert_eq!(format!("{other:?}"), "location difference"),
        }
        match compare_structural(&base, &Node::new(NodeKind::Ellipsis, loc(0, 1)), None) {
            StructuralCompare::Different { reason, .. } => assert_eq!(reason, DiffReason::Variant),
            other => assert_eq!(format!("{other:?}"), "variant difference"),
        }
        match compare_structural(&base, &numbered("2", 0), None) {
            StructuralCompare::Different { reason, .. } => {
                assert_eq!(reason, DiffReason::NonChildPayload)
            }
            other => assert_eq!(format!("{other:?}"), "payload difference"),
        }
        let left = Node::new(NodeKind::Program { statements: vec![numbered("1", 0)] }, loc(0, 4));
        let right = Node::new(
            NodeKind::Program { statements: vec![numbered("1", 0), numbered("2", 1)] },
            loc(0, 4),
        );
        match compare_structural(&left, &right, None) {
            StructuralCompare::Different { reason, path, .. } => {
                assert_eq!(reason, DiffReason::FieldCardinality);
                assert_eq!(path, "");
            }
            other => assert_eq!(format!("{other:?}"), "cardinality difference"),
        }
        let deep_left = wrap_expr(wrap_expr(numbered("1", 0)));
        let deep_right = wrap_expr(wrap_expr(numbered("9", 0)));
        match compare_structural(&deep_left, &deep_right, None) {
            StructuralCompare::Different { path, reason, .. } => {
                assert_eq!(reason, DiffReason::NonChildPayload);
                assert!(path.contains("expression"), "path={path}");
            }
            other => assert_eq!(format!("{other:?}"), "nested payload difference"),
        }
    }

    #[test]
    fn truncated_diagnostic_does_not_override_partial_eq() {
        let left = wrap_expr(wrap_expr(wrap_expr(numbered("1", 0))));
        let right = wrap_expr(wrap_expr(wrap_expr(numbered("1", 0))));
        assert_eq!(left, right);
        match compare_structural(&left, &right, Some(1)) {
            StructuralCompare::Truncated { work, .. } => assert_eq!(work, 2),
            other => assert_eq!(format!("{other:?}"), "truncation"),
        }
        assert_eq!(left, right, "truncation must not change PartialEq");
        let unequal = wrap_expr(wrap_expr(wrap_expr(numbered("9", 0))));
        assert_ne!(left, unequal);
        match compare_structural(&left, &unequal, Some(1)) {
            StructuralCompare::Truncated { .. } => {}
            other => assert_eq!(format!("{other:?}"), "truncation before the leaf"),
        }
        assert_ne!(left, unequal, "truncated Different-or-Equal must not override");
    }

    #[test]
    fn payload_shell_guard_saves_and_restores_previous_flag() {
        assert!(!EQ_PAYLOAD_SHELL.with(Cell::get));
        {
            let _outer = PayloadEqGuard::enter();
            assert!(EQ_PAYLOAD_SHELL.with(Cell::get));
            {
                let _inner = PayloadEqGuard::enter();
                assert!(EQ_PAYLOAD_SHELL.with(Cell::get));
            }
            assert!(EQ_PAYLOAD_SHELL.with(Cell::get));
        }
        assert!(!EQ_PAYLOAD_SHELL.with(Cell::get));
    }

    #[test]
    fn payload_shell_guard_restores_flag_after_panic() {
        assert!(!EQ_PAYLOAD_SHELL.with(Cell::get));
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = PayloadEqGuard::enter();
            assert!(EQ_PAYLOAD_SHELL.with(Cell::get));
            std::panic::resume_unwind(Box::new("payload-shell unwind"));
        }));
        assert!(panicked.is_err());
        assert!(!EQ_PAYLOAD_SHELL.with(Cell::get), "Drop must restore the flag on unwind");
    }

    #[test]
    fn payload_kind_eq_ignores_child_content_but_not_payloads() {
        let left = program(vec![numbered("1", 0)]);
        let right = program(vec![numbered("9", 0)]);
        assert!(payload_kind_eq(&left.kind, &right.kind), "child content is skipped");
        assert_ne!(left, right, "full equality still compares children");
        let other_card = program(vec![numbered("1", 0), numbered("2", 1)]);
        assert!(!payload_kind_eq(&left.kind, &other_card.kind), "vec length is payload-shape");
        assert!(!EQ_PAYLOAD_SHELL.with(Cell::get));
    }

    #[test]
    fn direct_nodekind_eq_routes_children_through_node_eq() {
        let left = wrap_expr(numbered("1", 0));
        let right = wrap_expr(numbered("1", 0));
        assert_eq!(left.kind, right.kind);
        let moved = wrap_expr(numbered("1", 4));
        assert_ne!(left.kind, moved.kind, "child location is part of NodeKind eq via Node::eq");
        assert_ne!(left, moved);
        let payload = wrap_expr(numbered("2", 0));
        assert_ne!(left.kind, payload.kind);
    }

    #[test]
    fn clone_then_eq_round_trip_on_shaped_trees() {
        let tree = program(vec![
            numbered("1", 0),
            Node::new(
                NodeKind::HashLiteral { pairs: vec![(numbered("k", 1), numbered("v", 2))] },
                loc(1, 3),
            ),
        ]);
        let cloned = tree.clone();
        assert_eq!(tree, cloned);
        assert_eq!(tree.kind, cloned.kind);
    }
}
