//! Iterative borrowed AST reads over the #8424 visit table.
//!
//! Exact whole-tree helpers walk an explicit heap stack and have no ordinary
//! depth-truncation path. Bounded variants report [`AstReadResult::Truncated`]
//! instead of returning a plausible `usize` / `Some` value. Child identity and
//! order come only from [`Node::try_for_each_child_with_field`]; this module
//! does not copy the visit table.

use super::{FieldId, Node};
use std::cmp::Ordering;

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
    /// An exact walk observed truncation even though no caller bound was
    /// installed. This is an internal invariant failure, not a depth guard.
    UnexpectedTruncation,
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
        // Sibling ordinal is canonical visit order. Field name and kind are
        // diagnostic identity: they must participate so Eq and Ord agree for
        // public values that callers may insert into BTreeSet/BTreeMap.
        self.sibling_ordinal
            .cmp(&other.sibling_ordinal)
            .then_with(|| self.field.map(FieldId::name).cmp(&other.field.map(FieldId::name)))
            .then_with(|| self.kind_name.cmp(other.kind_name))
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
    children_loaded: bool,
    children: Vec<(Option<FieldId>, &'a Node)>,
}

impl<'a> Frame<'a> {
    fn new(node: &'a Node, field: Option<FieldId>, sibling_ordinal: usize) -> Self {
        Self {
            node,
            field,
            sibling_ordinal,
            next_child: 0,
            yielded: false,
            children_loaded: false,
            children: Vec::new(),
        }
    }
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

/// Load one node's children through the #8424 visit table once.
///
/// A later `next_child` index is O(1) into this snapshot. Restarting the
/// visit table from child 0 on every sibling would be O(k²) at a `Program`
/// with tens of thousands of statements.
fn load_children(node: &Node) -> Vec<(Option<FieldId>, &Node)> {
    let mut children = Vec::new();
    node.for_each_child_with_field(|field, child| children.push((field, child)));
    children
}

impl<'a> AstReadCursor<'a> {
    fn new(root: &'a Node) -> Self {
        Self { stack: vec![Frame::new(root, None, 0)], work: AstReadWork::default() }
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

            let next = {
                let frame = match self.stack.last_mut() {
                    Some(frame) => frame,
                    None => return Ok(Step::Done),
                };
                if !frame.children_loaded {
                    frame.children = load_children(frame.node);
                    frame.children_loaded = true;
                }
                match frame.children.get(frame.next_child).copied() {
                    None => None,
                    Some((field, child)) => {
                        let ordinal = frame.next_child;
                        frame.next_child = frame
                            .next_child
                            .checked_add(1)
                            .ok_or(AstReadInstrumentCause::WorkCounterOverflow)?;
                        Some((field, child, ordinal))
                    }
                }
            };
            match next {
                None => {
                    self.stack.pop();
                }
                Some((field, child, ordinal)) => {
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
                    self.stack.push(Frame::new(child, field, ordinal));
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

fn finish_match<'a>(
    root: &'a Node,
    best_node: Option<&'a Node>,
    best_depth: usize,
) -> Option<DeepestContainingMatch<'a>> {
    best_node.map(|node| DeepestContainingMatch {
        node,
        depth: best_depth,
        path: path_to(root, node),
    })
}

fn walk_deepest<'a>(
    root: &'a Node,
    offset: usize,
    limits: AstReadLimits,
) -> AstReadResult<Option<DeepestContainingMatch<'a>>> {
    // Preserve the pre-#8867 contract: a child whose span lies outside the
    // walk root cannot match, even if the child itself contains `offset`.
    if !root.contains_offset(offset) {
        return AstReadResult::Complete { value: None, work: AstReadWork::default() };
    }
    let mut cursor = AstReadCursor::new(root);
    let mut best_node: Option<&Node> = None;
    let mut best_depth = 0usize;
    loop {
        match cursor.advance(limits, |child| child.contains_offset(offset)) {
            Ok(Step::Node(node)) => {
                if node.contains_offset(offset) {
                    let depth = cursor.depth();
                    // Visit order is the canonical #8424 sequence, so the first
                    // node at a given depth is the earliest path. Keep it unless
                    // a strictly deeper containing node appears.
                    if best_node.is_none() || depth > best_depth {
                        best_node = Some(node);
                        best_depth = depth;
                    }
                }
            }
            Ok(Step::Truncated(reason)) => {
                return AstReadResult::Truncated {
                    reason,
                    partial: finish_match(root, best_node, best_depth),
                    work: cursor.work,
                };
            }
            Ok(Step::Done) => {
                return AstReadResult::Complete {
                    value: finish_match(root, best_node, best_depth),
                    work: cursor.work,
                };
            }
            Err(cause) => return AstReadResult::InstrumentFailure { cause },
        }
    }
}

/// Reconstruct the canonical visit-table path from `root` to `target`.
///
/// The walk already identified `target`. Materializing the path once at the
/// end keeps a 50k-deep lookup from cloning path steps on every ancestor.
fn path_to<'a>(root: &'a Node, target: &'a Node) -> AstReadPath {
    if std::ptr::eq(root, target) {
        return AstReadPath::default();
    }
    let mut cursor = AstReadCursor::new(root);
    loop {
        match cursor.advance(AstReadLimits::default(), |_| true) {
            Ok(Step::Node(node)) if std::ptr::eq(node, target) => return cursor.path(),
            Ok(Step::Node(_)) => {}
            Ok(Step::Done | Step::Truncated(_)) | Err(_) => return AstReadPath::default(),
        }
    }
}

fn exact_from_result<T>(result: AstReadResult<T>) -> AstReadExact<T> {
    match result {
        AstReadResult::Complete { value, work } => AstReadExact::Complete { value, work },
        AstReadResult::Truncated { .. } => AstReadExact::InstrumentFailure {
            // Exact walks never install a bound. Observing truncation here is
            // an internal invariant failure, not a depth-guard success.
            cause: AstReadInstrumentCause::UnexpectedTruncation,
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
    /// [`AstReadExact::InstrumentFailure`] cannot arise from a finite owned
    /// tree: a `usize` work counter cannot overflow while the tree remains
    /// addressable. This convenience wrapper maps that unreachable arm to `0`
    /// rather than panicking in library code. Call [`Self::count_nodes_exact`]
    /// when the typed failure arm must be distinguished.
    ///
    /// # Examples
    ///
    /// ```
    /// use perl_ast::{Node, NodeKind, SourceLocation};
    ///
    /// let loc = SourceLocation::new(0, 1);
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
    ///     SourceLocation::new(0, 4),
    /// );
    /// let right = Node::new(
    ///     NodeKind::Number { value: "1".to_string() },
    ///     SourceLocation::new(7, 8),
    /// );
    /// let expr = Node::new(
    ///     NodeKind::Binary {
    ///         op: "+".to_string(),
    ///         left: Box::new(left),
    ///         right: Box::new(right),
    ///     },
    ///     SourceLocation::new(0, 8),
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
        SourceLocation::new(0, 1)
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
    fn path_step_ord_agrees_with_eq_when_ordinals_match() {
        use std::collections::BTreeSet;
        let left = AstReadPathStep {
            field: Some(FieldId::EXPRESSION),
            sibling_ordinal: 0,
            kind_name: "Number",
        };
        let right = AstReadPathStep {
            field: Some(FieldId::STATEMENTS),
            sibling_ordinal: 0,
            kind_name: "Identifier",
        };
        assert_ne!(left, right);
        let mut set = BTreeSet::new();
        assert!(set.insert(left));
        assert!(
            set.insert(right),
            "Ord that ignores field/kind would collapse distinct Eq values in BTreeSet"
        );
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn wide_program_loads_children_once_and_counts_exactly() {
        const WIDTH: usize = 4_096;
        let statements: Vec<Node> = (0..WIDTH)
            .map(|i| {
                Node::new(NodeKind::Number { value: "1".into() }, SourceLocation::new(i, i + 1))
            })
            .collect();
        let program = Node::new(NodeKind::Program { statements }, SourceLocation::new(0, WIDTH));
        let expected = WIDTH + 1;
        match program.count_nodes_exact() {
            AstReadExact::Complete { value, work } => {
                assert_eq!(value, expected);
                assert_eq!(work.nodes_visited, expected);
                assert_eq!(work.edges_visited, WIDTH);
            }
            other => {
                assert!(
                    matches!(other, AstReadExact::Complete { .. }),
                    "wide Program exact count must complete, got {other:?}"
                );
            }
        }
        let mut cursor = AstReadCursor::new(&program);
        match cursor.advance(AstReadLimits::default(), |_| true) {
            Ok(Step::Node(node)) => assert!(std::ptr::eq(node, &program)),
            other => {
                assert!(matches!(other, Ok(Step::Node(_))), "expected root yield, got {other:?}");
            }
        }
        let _ = cursor.advance(AstReadLimits::default(), |_| true);
        let frame = cursor.stack.first();
        assert!(frame.is_some(), "root frame remains while children are walked");
        if let Some(frame) = frame {
            assert!(frame.children_loaded);
            assert_eq!(frame.children.len(), WIDTH);
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
