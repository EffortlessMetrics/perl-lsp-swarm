//! Iterative borrowed AST reads over the #8424 visit table.
//!
//! Exact whole-tree helpers walk an explicit heap stack and have no ordinary
//! depth-truncation path. Bounded variants report [`AstReadResult::Truncated`]
//! instead of returning a plausible `usize` / `Some` value. Child identity and
//! order come only from [`Node::try_for_each_child_with_field`]; this module
//! does not copy the visit table.

use super::{FieldId, Node};
use std::cmp::Ordering;
use std::ops::ControlFlow;

/// Counted nodes and child edges actually walked by one read.
///
/// These counters are incremented as the cursor moves. They are not
/// reconstructed from the final count or from a selected match depth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AstReadWork {
    /// Nodes entered by this walk, including the root.
    pub nodes_visited: usize,
    /// Child edges actually descended. Search pruning and truncation do not
    /// count as descent.
    pub edges_visited: usize,
}

/// Why a bounded walk stopped before exhausting the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AstReadTruncation {
    /// A child would have been entered beyond [`AstReadLimits::max_depth`].
    DepthLimit {
        /// Inclusive maximum root-relative depth that may be entered.
        limit: usize,
    },
    /// Entering another node would exceed [`AstReadLimits::max_nodes`].
    NodeLimit {
        /// Maximum nodes this walk may enter.
        limit: usize,
    },
    /// Descending another child edge would exceed [`AstReadLimits::max_edges`].
    EdgeLimit {
        /// Maximum descended child edges.
        limit: usize,
    },
}

/// Internal arithmetic or cursor invariant failure.
///
/// Pure traversal has no ordinary external instrument. This is not a synonym
/// for a caller-selected bound; bounds are [`AstReadTruncation`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AstReadInstrumentCause {
    /// A checked node, edge, or child-index counter overflowed `usize`.
    WorkCounterOverflow,
}

/// Caller-selected bounds for [`Node::count_nodes_bounded`] and
/// [`Node::find_deepest_containing_offset_bounded`].
///
/// `None` on a field means that dimension is unbounded. Exact helpers ignore
/// this type entirely.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AstReadLimits {
    /// Inclusive maximum root-relative depth (root is 0). A child at
    /// `limit + 1` is not entered.
    pub max_depth: Option<usize>,
    /// Maximum nodes that may be entered, including the root.
    pub max_nodes: Option<usize>,
    /// Maximum child edges that may be descended.
    pub max_edges: Option<usize>,
}

impl AstReadLimits {
    /// Bound only by root-relative depth.
    #[must_use]
    pub const fn max_depth(limit: usize) -> Self {
        Self { max_depth: Some(limit), max_nodes: None, max_edges: None }
    }

    /// Bound only by entered-node count.
    #[must_use]
    pub const fn max_nodes(limit: usize) -> Self {
        Self { max_depth: None, max_nodes: Some(limit), max_edges: None }
    }

    /// Bound only by descended child edges.
    #[must_use]
    pub const fn max_edges(limit: usize) -> Self {
        Self { max_depth: None, max_nodes: None, max_edges: Some(limit) }
    }
}

/// Exact-walk outcome. There is no silent truncation arm.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub enum AstReadExact<T> {
    /// The walk exhausted the tree (or the search-pruned remainder).
    Complete {
        /// Exact product value.
        value: T,
        /// Nodes and edges actually visited.
        work: AstReadWork,
    },
    /// Checked arithmetic or cursor invariant failed.
    InstrumentFailure {
        /// Stable cause of the internal failure.
        cause: AstReadInstrumentCause,
    },
}

impl<T> AstReadExact<T> {
    /// Return the complete value when the walk succeeded.
    #[must_use]
    pub fn complete_value(self) -> Option<T> {
        match self {
            Self::Complete { value, .. } => Some(value),
            Self::InstrumentFailure { .. } => None,
        }
    }
}

/// Bounded-walk outcome. Truncation cannot be mistaken for ordinary success.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub enum AstReadResult<T> {
    /// The walk exhausted the tree (or the search-pruned remainder).
    Complete {
        /// Exact product value.
        value: T,
        /// Nodes and edges actually visited.
        work: AstReadWork,
    },
    /// A caller-selected bound stopped the walk.
    Truncated {
        /// Why the walk stopped.
        reason: AstReadTruncation,
        /// Best known value at the stop. For a count this is the nodes entered
        /// so far; for a lookup this is the best match among entered nodes.
        partial: T,
        /// Nodes and edges actually visited before the stop.
        work: AstReadWork,
    },
    /// Checked arithmetic or cursor invariant failed.
    InstrumentFailure {
        /// Stable cause of the internal failure.
        cause: AstReadInstrumentCause,
    },
}

/// One step of a canonical #8424 path from the walk root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AstReadPathStep {
    /// Structural field of this child, or `None` for the walk root.
    pub field: Option<FieldId>,
    /// Zero-based index among siblings emitted by the visit table.
    pub sibling_ordinal: usize,
    /// [`crate::NodeKind::kind_name`] at this step.
    pub kind_name: &'static str,
}

impl PartialOrd for AstReadPathStep {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for AstReadPathStep {
    fn cmp(&self, other: &Self) -> Ordering {
        // Visit-table sibling order, not FieldId name order: If/HashLiteral
        // interleave reused field names, so name order is not canonical.
        self.sibling_ordinal.cmp(&other.sibling_ordinal)
    }
}

/// Canonical path from the walk root, in visit-table order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AstReadPath {
    /// Root-to-node steps, excluding the root itself.
    pub steps: Vec<AstReadPathStep>,
}

impl PartialOrd for AstReadPath {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for AstReadPath {
    fn cmp(&self, other: &Self) -> Ordering {
        self.steps.cmp(&other.steps)
    }
}

impl AstReadPath {
    /// Render a diagnostic path that retains field identity.
    #[must_use]
    pub fn to_diagnostic_string(&self, root_kind: &str) -> String {
        let mut rendered = format!("root:{root_kind}");
        for step in &self.steps {
            let field = step.field.map_or("child", FieldId::name);
            rendered.push('/');
            rendered.push_str(field);
            rendered.push('[');
            rendered.push_str(&step.sibling_ordinal.to_string());
            rendered.push_str("]:");
            rendered.push_str(step.kind_name);
        }
        rendered
    }
}

/// Exact deepest containing-offset match, with depth and canonical path.
#[derive(Debug, Clone, PartialEq)]
pub struct DeepestContainingMatch<'a> {
    /// The selected node.
    pub node: &'a Node,
    /// Root-relative depth (root is 0).
    pub depth: usize,
    /// Canonical #8424 path from the walk root.
    pub path: AstReadPath,
}

struct Frame<'a> {
    node: &'a Node,
    field: Option<FieldId>,
    sibling_ordinal: usize,
    next_child: usize,
    yielded: bool,
}

/// Internal borrowed DFS cursor. The stack is the canonical path.
struct AstReadCursor<'a> {
    stack: Vec<Frame<'a>>,
    work: AstReadWork,
}

#[derive(Debug)]
enum Step<'a> {
    Node(&'a Node),
    Truncated(AstReadTruncation),
    Done,
}

fn nth_child(node: &Node, index: usize) -> Option<(Option<FieldId>, &Node)> {
    let mut current = 0usize;
    let mut found = None;
    let _ = node.try_for_each_child_with_field(|field, child| {
        if current == index {
            found = Some((field, child));
            ControlFlow::Break(())
        } else {
            match current.checked_add(1) {
                Some(next) => {
                    current = next;
                    ControlFlow::Continue(())
                }
                None => ControlFlow::Break(()),
            }
        }
    });
    found
}

impl<'a> AstReadCursor<'a> {
    fn new(root: &'a Node) -> Self {
        Self {
            stack: vec![Frame {
                node: root,
                field: None,
                sibling_ordinal: 0,
                next_child: 0,
                yielded: false,
            }],
            work: AstReadWork::default(),
        }
    }

    fn depth(&self) -> usize {
        self.stack.len().saturating_sub(1)
    }

    fn path(&self) -> AstReadPath {
        AstReadPath {
            steps: self
                .stack
                .iter()
                .skip(1)
                .map(|frame| AstReadPathStep {
                    field: frame.field,
                    sibling_ordinal: frame.sibling_ordinal,
                    kind_name: frame.node.kind.kind_name(),
                })
                .collect(),
        }
    }

    fn advance(
        &mut self,
        limits: AstReadLimits,
        mut should_descend: impl FnMut(&Node) -> bool,
    ) -> Result<Step<'a>, AstReadInstrumentCause> {
        loop {
            let yielded = match self.stack.last() {
                Some(frame) => frame.yielded,
                None => return Ok(Step::Done),
            };

            if !yielded {
                if let Some(limit) =
                    limits.max_nodes.filter(|&limit| self.work.nodes_visited >= limit)
                {
                    return Ok(Step::Truncated(AstReadTruncation::NodeLimit { limit }));
                }
                if let Some(limit) = limits.max_depth.filter(|&limit| self.depth() > limit) {
                    return Ok(Step::Truncated(AstReadTruncation::DepthLimit { limit }));
                }
                self.work.nodes_visited = self
                    .work
                    .nodes_visited
                    .checked_add(1)
                    .ok_or(AstReadInstrumentCause::WorkCounterOverflow)?;
                let node = match self.stack.last_mut() {
                    Some(frame) => {
                        frame.yielded = true;
                        frame.node
                    }
                    None => return Ok(Step::Done),
                };
                return Ok(Step::Node(node));
            }

            let (parent, next_child) = match self.stack.last() {
                Some(frame) => (frame.node, frame.next_child),
                None => return Ok(Step::Done),
            };
            match nth_child(parent, next_child) {
                None => {
                    self.stack.pop();
                }
                Some((field, child)) => {
                    let incremented = next_child
                        .checked_add(1)
                        .ok_or(AstReadInstrumentCause::WorkCounterOverflow)?;
                    if let Some(frame) = self.stack.last_mut() {
                        frame.next_child = incremented;
                    }
                    if !should_descend(child) {
                        continue;
                    }
                    if let Some(limit) =
                        limits.max_nodes.filter(|&limit| self.work.nodes_visited >= limit)
                    {
                        return Ok(Step::Truncated(AstReadTruncation::NodeLimit { limit }));
                    }
                    if let Some(limit) =
                        limits.max_edges.filter(|&limit| self.work.edges_visited >= limit)
                    {
                        return Ok(Step::Truncated(AstReadTruncation::EdgeLimit { limit }));
                    }
                    // Child depth equals the current stack length (root occupies slot 0).
                    let child_depth = self.stack.len();
                    if let Some(limit) = limits.max_depth.filter(|&limit| child_depth > limit) {
                        return Ok(Step::Truncated(AstReadTruncation::DepthLimit { limit }));
                    }
                    self.work.edges_visited = self
                        .work
                        .edges_visited
                        .checked_add(1)
                        .ok_or(AstReadInstrumentCause::WorkCounterOverflow)?;
                    self.stack.push(Frame {
                        node: child,
                        field,
                        sibling_ordinal: next_child,
                        next_child: 0,
                        yielded: false,
                    });
                }
            }
        }
    }
}

fn walk_count(root: &Node, limits: AstReadLimits) -> AstReadResult<usize> {
    let mut cursor = AstReadCursor::new(root);
    let mut count = 0usize;
    loop {
        match cursor.advance(limits, |_| true) {
            Ok(Step::Node(_)) => {
                count = match count.checked_add(1) {
                    Some(next) => next,
                    None => {
                        return AstReadResult::InstrumentFailure {
                            cause: AstReadInstrumentCause::WorkCounterOverflow,
                        };
                    }
                };
            }
            Ok(Step::Truncated(reason)) => {
                return AstReadResult::Truncated { reason, partial: count, work: cursor.work };
            }
            Ok(Step::Done) => {
                return AstReadResult::Complete { value: count, work: cursor.work };
            }
            Err(cause) => return AstReadResult::InstrumentFailure { cause },
        }
    }
}

fn match_is_better(
    best: &DeepestContainingMatch<'_>,
    candidate: &DeepestContainingMatch<'_>,
) -> bool {
    candidate.depth > best.depth || (candidate.depth == best.depth && candidate.path < best.path)
}

fn walk_deepest<'a>(
    root: &'a Node,
    offset: usize,
    limits: AstReadLimits,
) -> AstReadResult<Option<DeepestContainingMatch<'a>>> {
    let mut cursor = AstReadCursor::new(root);
    let mut best = None;
    loop {
        match cursor.advance(limits, |child| child.contains_offset(offset)) {
            Ok(Step::Node(node)) => {
                if node.contains_offset(offset) {
                    let candidate =
                        DeepestContainingMatch { node, depth: cursor.depth(), path: cursor.path() };
                    if best.as_ref().is_none_or(|current| match_is_better(current, &candidate)) {
                        best = Some(candidate);
                    }
                }
            }
            Ok(Step::Truncated(reason)) => {
                return AstReadResult::Truncated { reason, partial: best, work: cursor.work };
            }
            Ok(Step::Done) => {
                return AstReadResult::Complete { value: best, work: cursor.work };
            }
            Err(cause) => return AstReadResult::InstrumentFailure { cause },
        }
    }
}

fn exact_from_result<T>(result: AstReadResult<T>) -> AstReadExact<T> {
    match result {
        AstReadResult::Complete { value, work } => AstReadExact::Complete { value, work },
        AstReadResult::Truncated { .. } => AstReadExact::InstrumentFailure {
            // Exact walks never install a bound. Observing truncation here is
            // an internal invariant failure, not a depth-guard success.
            cause: AstReadInstrumentCause::WorkCounterOverflow,
        },
        AstReadResult::InstrumentFailure { cause } => AstReadExact::InstrumentFailure { cause },
    }
}

impl Node {
    /// Exact whole-tree node count with no depth ceiling.
    ///
    /// The walk is iterative over [`Self::try_for_each_child_with_field`]. It
    /// cannot return a silently truncated size: callers that need incompleteness
    /// use [`Self::count_nodes_bounded`].
    ///
    /// # Examples
    ///
    /// ```
    /// use perl_ast::{Node, NodeKind, SourceLocation};
    ///
    /// let loc = SourceLocation { start: 0, end: 1 };
    /// let leaf = Node::new(NodeKind::Number { value: "1".to_string() }, loc);
    /// assert_eq!(leaf.count_nodes(), 1);
    ///
    /// let program = Node::new(
    ///     NodeKind::Program { statements: vec![leaf] },
    ///     loc,
    /// );
    /// assert_eq!(program.count_nodes(), 2);
    /// ```
    pub fn count_nodes(&self) -> usize {
        self.count_nodes_exact().complete_value().unwrap_or(0)
    }

    /// Exact whole-tree count with work accounting.
    ///
    /// Never returns [`AstReadResult::Truncated`]. Arithmetic overflow of a
    /// work counter is [`AstReadExact::InstrumentFailure`].
    pub fn count_nodes_exact(&self) -> AstReadExact<usize> {
        exact_from_result(walk_count(self, AstReadLimits::default()))
    }

    /// Bounded whole-tree count.
    ///
    /// Hitting a caller limit returns [`AstReadResult::Truncated`] rather than
    /// an ordinary `usize`.
    pub fn count_nodes_bounded(&self, limits: AstReadLimits) -> AstReadResult<usize> {
        walk_count(self, limits)
    }

    /// Find the deepest node whose half-open span contains `offset`.
    ///
    /// Returns `None` when `offset` is outside this node. Greatest structural
    /// depth wins; equal-depth overlapping matches keep the earliest canonical
    /// #8424 path. The walk is iterative and does not silently stop at
    /// [`super::MAX_AST_DEPTH`].
    ///
    /// The same half-open span semantics as [`Node::contains_offset`] apply:
    /// start is inclusive and end is exclusive. Zero-width nodes therefore do
    /// not contain an ordinary byte offset.
    ///
    /// # Examples
    ///
    /// ```
    /// use perl_ast::{Node, NodeKind, SourceLocation};
    ///
    /// let left = Node::new(
    ///     NodeKind::Identifier { name: "left".to_string() },
    ///     SourceLocation { start: 0, end: 4 },
    /// );
    /// let right = Node::new(
    ///     NodeKind::Number { value: "1".to_string() },
    ///     SourceLocation { start: 7, end: 8 },
    /// );
    /// let expr = Node::new(
    ///     NodeKind::Binary {
    ///         op: "+".to_string(),
    ///         left: Box::new(left),
    ///         right: Box::new(right),
    ///     },
    ///     SourceLocation { start: 0, end: 8 },
    /// );
    ///
    /// assert_eq!(
    ///     expr.find_deepest_containing_offset(7).map(|node| node.kind.kind_name()),
    ///     Some("Number"),
    /// );
    /// assert_eq!(expr.find_deepest_containing_offset(8), None);
    /// ```
    #[inline]
    pub fn find_deepest_containing_offset(&self, offset: usize) -> Option<&Node> {
        match self.find_deepest_containing_offset_exact(offset) {
            AstReadExact::Complete { value, .. } => value.map(|found| found.node),
            AstReadExact::InstrumentFailure { .. } => None,
        }
    }

    /// Exact deepest containing-offset lookup with depth, path, and work.
    pub fn find_deepest_containing_offset_exact(
        &self,
        offset: usize,
    ) -> AstReadExact<Option<DeepestContainingMatch<'_>>> {
        exact_from_result(walk_deepest(self, offset, AstReadLimits::default()))
    }

    /// Bounded deepest containing-offset lookup.
    ///
    /// Hitting a caller limit returns [`AstReadResult::Truncated`] with the
    /// best known match among entered nodes, not a complete `Some(node)`.
    pub fn find_deepest_containing_offset_bounded(
        &self,
        offset: usize,
        limits: AstReadLimits,
    ) -> AstReadResult<Option<DeepestContainingMatch<'_>>> {
        walk_deepest(self, offset, limits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node_kind_fixtures;
    use crate::{NodeKind, SourceLocation};

    fn loc() -> SourceLocation {
        SourceLocation { start: 0, end: 1 }
    }

    fn independent_count(node: &Node) -> usize {
        let mut count = 1usize;
        node.for_each_child(|child| {
            count += independent_count(child);
        });
        count
    }

    fn independent_fields(node: &Node) -> Vec<(Option<FieldId>, &'static str)> {
        let mut fields = Vec::new();
        let mut cursor = AstReadCursor::new(node);
        loop {
            match cursor.advance(AstReadLimits::default(), |_| true) {
                Ok(Step::Node(current)) => {
                    if !std::ptr::eq(current, node) {
                        let frame = cursor.stack.last();
                        assert!(frame.is_some(), "entered node has a frame");
                        if let Some(frame) = frame {
                            fields.push((frame.field, current.kind.kind_name()));
                        }
                    }
                }
                Ok(Step::Done) => break,
                other => {
                    assert!(
                        matches!(other, Ok(Step::Done)),
                        "unbounded representative walk must complete: {other:?}"
                    );
                    break;
                }
            }
        }
        fields
    }

    fn recursive_fields(node: &Node) -> Vec<(Option<FieldId>, &'static str)> {
        let mut fields = Vec::new();
        fn walk(node: &Node, out: &mut Vec<(Option<FieldId>, &'static str)>) {
            node.for_each_child_with_field(|field, child| {
                out.push((field, child.kind.kind_name()));
                walk(child, out);
            });
        }
        walk(node, &mut fields);
        fields
    }

    #[test]
    fn every_populated_fixture_count_matches_visit_table_walk() {
        for fixture in node_kind_fixtures() {
            let expected = independent_count(&fixture.sample);
            assert_eq!(
                fixture.sample.count_nodes(),
                expected,
                "{}: omitting a #8424 child field must change the exact count",
                fixture.sample.kind.kind_name()
            );
            match fixture.sample.count_nodes_exact() {
                AstReadExact::Complete { value, work } => {
                    assert_eq!(value, expected);
                    assert_eq!(work.nodes_visited, expected);
                    if expected == 0 {
                        assert_eq!(work.edges_visited, 0);
                    } else {
                        assert_eq!(work.edges_visited, expected - 1);
                    }
                }
                other => {
                    assert!(
                        matches!(other, AstReadExact::Complete { .. }),
                        "{}: exact count failed: {other:?}",
                        fixture.sample.kind.kind_name()
                    );
                }
            }
        }
    }

    #[test]
    fn cursor_dfs_fields_match_visit_table_without_a_second_match() {
        for fixture in node_kind_fixtures() {
            assert_eq!(
                independent_fields(&fixture.sample),
                recursive_fields(&fixture.sample),
                "{}: read cursor must emit the #8424 visit sequence",
                fixture.sample.kind.kind_name()
            );
        }
    }

    #[test]
    fn optional_initializer_is_counted_when_present() {
        let var = Node::new(NodeKind::Variable { sigil: "$".into(), name: "x".into() }, loc());
        let init = Node::new(NodeKind::Number { value: "1".into() }, loc());
        let decl = Node::new(
            NodeKind::VariableDeclaration {
                declarator: "my".into(),
                variable: Box::new(var),
                attributes: vec![],
                initializer: Some(Box::new(init)),
            },
            loc(),
        );
        assert_eq!(decl.count_nodes(), 3);
        let without = Node::new(
            NodeKind::VariableDeclaration {
                declarator: "my".into(),
                variable: Box::new(Node::new(
                    NodeKind::Variable { sigil: "$".into(), name: "x".into() },
                    loc(),
                )),
                attributes: vec![],
                initializer: None,
            },
            loc(),
        );
        assert_eq!(without.count_nodes(), 2);
    }
}
