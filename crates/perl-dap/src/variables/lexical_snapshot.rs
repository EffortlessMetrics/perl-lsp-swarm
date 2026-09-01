//! Bounded stopped-state snapshot model for lexical collections (#7358, PR 1).
//!
//! One reviewed budget policy with cumulative — not per-container — limits, one
//! typed truncation/unavailable vocabulary, and a deterministic pure-model
//! traversal over already-captured value trees. This module introduces the
//! model and its pure falsifiers only; wiring the live inspection boundary is
//! owned by the following PRs of the #7358 sequence.
//!
//! A root collection that was merely clipped at the root is never reported as
//! complete: every node carries its own [`NodeOutcome`], and the snapshot
//! records the first global truncation reason together with the exact work
//! counters that observed it.

/// Cumulative budget for one collection snapshot.
///
/// Every limit applies to the whole snapshot, not independently to each nested
/// container (except `max_items_per_container`, which is inherently local).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotBudget {
    /// Maximum total nodes visited across the entire snapshot.
    pub max_total_nodes: usize,
    /// Maximum children expanded per container node.
    pub max_items_per_container: usize,
    /// Maximum nesting depth from the snapshot root.
    pub max_depth: usize,
    /// Maximum rendered bytes for a single scalar value.
    pub max_scalar_bytes: usize,
    /// Maximum retained bytes across the entire snapshot, including node
    /// names, kind labels, and rendered scalar text.
    pub max_total_bytes: usize,
}

impl SnapshotBudget {
    /// The reviewed default policy (mirrors the 1,024-root-entry clamp with a
    /// global budget on top of it).
    pub const fn defaults() -> Self {
        Self {
            max_total_nodes: 4_096,
            max_items_per_container: 1_024,
            max_depth: 32,
            max_scalar_bytes: 16_384,
            max_total_bytes: 262_144,
        }
    }
}

/// Why a subtree is not fully represented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TruncationReason {
    /// The global node budget was exhausted.
    NodeBudgetExhausted,
    /// The container held more items than the per-container limit.
    ContainerItemLimit,
    /// The subtree exceeded the depth limit.
    DepthLimit,
    /// A scalar rendered larger than the single-scalar byte limit.
    ScalarByteLimit,
    /// The total rendered-byte budget was exhausted.
    TotalByteLimit,
}

/// Why a value could not be inspected without executing debuggee behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnavailableReason {
    /// The array/hash is tied: enumerating it may run `FETCHSIZE`/`FETCH`/
    /// `FIRSTKEY`/`NEXTKEY`.
    Tied,
    /// The value is magical: reading it may run Perl-level callbacks.
    Magical,
    /// Stringifying the value would invoke overloaded methods.
    OverloadedStringify,
    /// Safe inspection could not be proven for another reason.
    InspectionFailed,
}

/// Per-node result inside an accepted snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeOutcome {
    /// The node and all of its children were captured within budget.
    Complete,
    /// The node exists but its representation was cut for this reason.
    Truncated(TruncationReason),
    /// Honest inspection was not possible; the value is rendered as
    /// unavailable rather than being fabricated.
    Unavailable(UnavailableReason),
}

/// Whether a source value would execute debuggee code when inspected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceFlags {
    /// The value is tied (inspection may run FETCH-style methods).
    pub tied: bool,
    /// The value is magical (reading it may run callbacks).
    pub magical: bool,
    /// Stringification would invoke overloaded methods.
    pub overloaded_stringify: bool,
}

impl SourceFlags {
    /// Plain, side-effect-free value.
    pub const PLAIN: Self = Self { tied: false, magical: false, overloaded_stringify: false };

    /// Classify without fabricating: does this flag set forbid enumeration?
    pub const fn forbids_enumeration(self) -> bool {
        self.tied || self.magical
    }

    /// Classify without fabricating: does this flag set forbid rendering text?
    pub const fn forbids_rendering(self) -> bool {
        self.overloaded_stringify
    }
}

/// A node of the accepted snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotNode {
    /// Variable name or container key as written.
    pub name: String,
    /// Honest kind label (for example `array`, `hash`, `scalar`, or an
    /// unavailable marker such as `tied`).
    pub kind_label: String,
    /// Rendered scalar text, when the outcome admits rendering.
    pub rendered: Option<String>,
    /// Captured children, when the outcome admits enumeration.
    pub children: Vec<SnapshotNode>,
    /// This node's own outcome.
    pub outcome: NodeOutcome,
}

/// One accepted snapshot with exact work accounting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionSnapshot {
    /// The snapshot root.
    pub root: SnapshotNode,
    /// Total nodes visited while building the snapshot.
    pub total_nodes_visited: usize,
    /// Total retained bytes accepted into the snapshot, including node names,
    /// kind labels, and rendered scalar text.
    pub total_rendered_bytes: usize,
    /// The first global truncation observed, if any.
    pub truncation: Option<TruncationReason>,
}

/// A static view of a captured source value handed to the model.
#[derive(Debug, Clone, Copy)]
pub struct SourceValue<'a> {
    /// Variable name or container key.
    pub name: &'a str,
    /// Honest kind label from the capture boundary.
    pub kind_label: &'a str,
    /// Pre-captured scalar text, when this is a scalar.
    pub scalar_text: Option<&'a str>,
    /// Side-effect classification of the source value.
    pub flags: SourceFlags,
    /// Captured child values, when this is a container.
    pub children: Option<&'a [SourceValue<'a>]>,
}

impl<'a> SourceValue<'a> {
    /// Build a plain scalar source value.
    pub fn scalar(name: &'a str, text: &'a str) -> Self {
        Self {
            name,
            kind_label: "scalar",
            scalar_text: Some(text),
            flags: SourceFlags::PLAIN,
            children: None,
        }
    }

    /// Build a container source value.
    pub fn container(name: &'a str, kind_label: &'a str, children: &'a [SourceValue<'a>]) -> Self {
        Self {
            name,
            kind_label,
            scalar_text: None,
            flags: SourceFlags::PLAIN,
            children: Some(children),
        }
    }
}

struct BudgetCursor {
    budget: SnapshotBudget,
    visited: usize,
    rendered_bytes: usize,
    truncation: Option<TruncationReason>,
}

impl BudgetCursor {
    fn record_truncation(&mut self, reason: TruncationReason) {
        self.truncation = self.truncation.or(Some(reason));
    }
}

/// Capture one bounded, deterministic snapshot of a captured value tree.
///
/// Traversal order is source order; budget decisions are cumulative and
/// deterministic, so identical inputs always produce identical snapshots.
pub fn capture_snapshot(root: SourceValue<'_>, budget: SnapshotBudget) -> CollectionSnapshot {
    let mut cursor = BudgetCursor { budget, visited: 0, rendered_bytes: 0, truncation: None };
    let root_node = build_node(root, 0, &mut cursor);
    CollectionSnapshot {
        total_nodes_visited: cursor.visited,
        total_rendered_bytes: cursor.rendered_bytes,
        truncation: cursor.truncation,
        root: root_node,
    }
}

fn build_node(value: SourceValue<'_>, depth: usize, cursor: &mut BudgetCursor) -> SnapshotNode {
    if cursor.visited >= cursor.budget.max_total_nodes {
        cursor.record_truncation(TruncationReason::NodeBudgetExhausted);
        return SnapshotNode {
            name: value.name.to_string(),
            kind_label: value.kind_label.to_string(),
            rendered: None,
            children: Vec::new(),
            outcome: NodeOutcome::Truncated(TruncationReason::NodeBudgetExhausted),
        };
    }
    cursor.visited = cursor.visited.saturating_add(1);

    if !charge_bytes(cursor, value.name.len().saturating_add(value.kind_label.len())) {
        return SnapshotNode {
            name: value.name.to_string(),
            kind_label: value.kind_label.to_string(),
            rendered: None,
            children: Vec::new(),
            outcome: NodeOutcome::Truncated(TruncationReason::TotalByteLimit),
        };
    }

    if value.flags.forbids_enumeration() {
        let reason =
            if value.flags.tied { UnavailableReason::Tied } else { UnavailableReason::Magical };
        return SnapshotNode {
            name: value.name.to_string(),
            kind_label: value.kind_label.to_string(),
            rendered: None,
            children: Vec::new(),
            outcome: NodeOutcome::Unavailable(reason),
        };
    }

    let Some(source_children) = value.children else {
        return render_scalar(value, depth, cursor);
    };

    // Container expansion under the depth and per-container limits.
    if depth >= cursor.budget.max_depth {
        cursor.record_truncation(TruncationReason::DepthLimit);
        return SnapshotNode {
            name: value.name.to_string(),
            kind_label: value.kind_label.to_string(),
            rendered: None,
            children: Vec::new(),
            outcome: NodeOutcome::Truncated(TruncationReason::DepthLimit),
        };
    }
    let admitted = source_children.len().min(cursor.budget.max_items_per_container);
    if admitted < source_children.len() {
        cursor.record_truncation(TruncationReason::ContainerItemLimit);
    }

    let mut snapshot_children = Vec::with_capacity(admitted);
    let mut outcome = if admitted < source_children.len() {
        Some(TruncationReason::ContainerItemLimit)
    } else {
        None
    };
    for child in &source_children[..admitted] {
        if cursor.visited >= cursor.budget.max_total_nodes {
            cursor.record_truncation(TruncationReason::NodeBudgetExhausted);
            outcome.get_or_insert(TruncationReason::NodeBudgetExhausted);
            break;
        }
        let node = build_node(*child, depth + 1, cursor);
        if let NodeOutcome::Truncated(reason) = node.outcome {
            outcome.get_or_insert(reason);
        }
        snapshot_children.push(node);
    }

    SnapshotNode {
        name: value.name.to_string(),
        kind_label: value.kind_label.to_string(),
        rendered: None,
        children: snapshot_children,
        outcome: outcome.map_or(NodeOutcome::Complete, NodeOutcome::Truncated),
    }
}

fn render_scalar(value: SourceValue<'_>, _depth: usize, cursor: &mut BudgetCursor) -> SnapshotNode {
    if value.flags.forbids_rendering() {
        return SnapshotNode {
            name: value.name.to_string(),
            kind_label: value.kind_label.to_string(),
            rendered: None,
            children: Vec::new(),
            outcome: NodeOutcome::Unavailable(UnavailableReason::OverloadedStringify),
        };
    }

    let text = value.scalar_text.unwrap_or_default();
    if text.len() > cursor.budget.max_scalar_bytes {
        cursor.record_truncation(TruncationReason::ScalarByteLimit);
        return SnapshotNode {
            name: value.name.to_string(),
            kind_label: value.kind_label.to_string(),
            rendered: None,
            children: Vec::new(),
            outcome: NodeOutcome::Truncated(TruncationReason::ScalarByteLimit),
        };
    }

    if !charge_bytes(cursor, text.len()) {
        return SnapshotNode {
            name: value.name.to_string(),
            kind_label: value.kind_label.to_string(),
            rendered: None,
            children: Vec::new(),
            outcome: NodeOutcome::Truncated(TruncationReason::TotalByteLimit),
        };
    }

    SnapshotNode {
        name: value.name.to_string(),
        kind_label: value.kind_label.to_string(),
        rendered: Some(text.to_string()),
        children: Vec::new(),
        outcome: NodeOutcome::Complete,
    }
}

fn charge_bytes(cursor: &mut BudgetCursor, bytes: usize) -> bool {
    let projected = cursor.rendered_bytes.saturating_add(bytes);
    if projected > cursor.budget.max_total_bytes {
        cursor.record_truncation(TruncationReason::TotalByteLimit);
        false
    } else {
        cursor.rendered_bytes = projected;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{
        NodeOutcome, SnapshotBudget, SourceFlags, SourceValue, TruncationReason, UnavailableReason,
        capture_snapshot,
    };

    fn scalar<'a>(name: &'a str, text: &'a str) -> SourceValue<'a> {
        SourceValue::scalar(name, text)
    }

    #[test]
    fn plain_scalars_capture_complete_within_budget() {
        let kids = [scalar("a", "1"), scalar("b", "2")];
        let root = SourceValue::container("@values", "array", &kids);
        let snapshot = capture_snapshot(root, SnapshotBudget::defaults());

        assert_eq!(snapshot.root.outcome, NodeOutcome::Complete);
        assert_eq!(snapshot.root.children.len(), 2);
        assert_eq!(snapshot.root.children[0].rendered.as_deref(), Some("1"));
        assert_eq!(snapshot.truncation, None);
        assert_eq!(snapshot.total_nodes_visited, 3);
        assert_eq!(snapshot.total_rendered_bytes, 28);
    }

    #[test]
    fn depth_limit_truncates_nested_containers() {
        let leaf = [scalar("x", "1")];
        let mid = [SourceValue::container("%h", "hash", &leaf)];
        let root = [SourceValue::container("@a", "array", &mid)];
        let budget = SnapshotBudget { max_depth: 1, ..SnapshotBudget::defaults() };
        let snapshot = capture_snapshot(root[0], budget);

        assert_eq!(snapshot.truncation, Some(TruncationReason::DepthLimit));
        assert_eq!(snapshot.root.outcome, NodeOutcome::Truncated(TruncationReason::DepthLimit));
        let child = &snapshot.root.children[0];
        assert_eq!(child.outcome, NodeOutcome::Truncated(TruncationReason::DepthLimit));
        assert!(child.children.is_empty());
    }

    #[test]
    fn per_container_item_limit_truncates_wide_containers() {
        let kids: Vec<SourceValue<'_>> = (0..8).map(|_| scalar("s", "v")).collect();
        let root = SourceValue::container("@wide", "array", &kids);
        let budget = SnapshotBudget { max_items_per_container: 3, ..SnapshotBudget::defaults() };
        let snapshot = capture_snapshot(root, budget);

        assert_eq!(snapshot.truncation, Some(TruncationReason::ContainerItemLimit));
        assert_eq!(snapshot.root.children.len(), 3);
        assert_eq!(
            snapshot.root.outcome,
            NodeOutcome::Truncated(TruncationReason::ContainerItemLimit)
        );
    }

    #[test]
    fn node_budget_is_cumulative_across_the_whole_snapshot() {
        // Budget of 2 total nodes: root + first child. Remaining siblings must
        // be omitted once the cumulative counter is exhausted.
        let kids: Vec<SourceValue<'_>> = (0..64).map(|_| scalar("s", "v")).collect();
        let root = SourceValue::container("@v", "array", &kids);
        let budget = SnapshotBudget { max_total_nodes: 2, ..SnapshotBudget::defaults() };
        let snapshot = capture_snapshot(root, budget);

        assert_eq!(snapshot.truncation, Some(TruncationReason::NodeBudgetExhausted));
        assert_eq!(snapshot.root.children[0].outcome, NodeOutcome::Complete);
        assert_eq!(snapshot.root.children.len(), 1);
        assert_eq!(
            snapshot.root.outcome,
            NodeOutcome::Truncated(TruncationReason::NodeBudgetExhausted)
        );
        assert!(snapshot.total_nodes_visited <= budget.max_total_nodes);
    }

    #[test]
    fn total_byte_budget_refuses_the_scalar_that_overflows_it() {
        let kids = [scalar("a", "12345"), scalar("b", "67890")];
        let root = SourceValue::container("@v", "array", &kids);
        let budget = SnapshotBudget { max_total_bytes: 20, ..SnapshotBudget::defaults() };
        let snapshot = capture_snapshot(root, budget);

        assert_eq!(snapshot.truncation, Some(TruncationReason::TotalByteLimit));
        assert_eq!(snapshot.root.children[0].outcome, NodeOutcome::Complete);
        assert_eq!(
            snapshot.root.children[1].outcome,
            NodeOutcome::Truncated(TruncationReason::TotalByteLimit)
        );
        assert_eq!(snapshot.root.children[1].rendered, None);
        assert_eq!(snapshot.root.outcome, NodeOutcome::Truncated(TruncationReason::TotalByteLimit));
    }

    #[test]
    fn oversized_scalars_are_refused_not_clipped_silently() {
        let root = scalar("big", "0123456789");
        let budget = SnapshotBudget { max_scalar_bytes: 4, ..SnapshotBudget::defaults() };
        let snapshot = capture_snapshot(root, budget);

        assert_eq!(
            snapshot.root.outcome,
            NodeOutcome::Truncated(TruncationReason::ScalarByteLimit)
        );
        assert_eq!(snapshot.root.rendered, None);
    }

    #[test]
    fn tied_values_are_unavailable_not_fabricated() {
        let mut tied = SourceValue::scalar("@tied", "");
        tied.flags = SourceFlags { tied: true, ..SourceFlags::PLAIN };
        let snapshot = capture_snapshot(tied, SnapshotBudget::defaults());

        assert_eq!(snapshot.root.outcome, NodeOutcome::Unavailable(UnavailableReason::Tied));
        assert_eq!(snapshot.root.rendered, None);
        assert_eq!(snapshot.truncation, None, "unavailable is honest, not a truncation");
    }

    #[test]
    fn magical_containers_refuse_enumeration() {
        let plain_kids = [scalar("a", "1")];
        let mut magical = SourceValue::container("%env_like", "hash", &plain_kids);
        magical.flags = SourceFlags { magical: true, ..SourceFlags::PLAIN };
        let snapshot = capture_snapshot(magical, SnapshotBudget::defaults());

        assert_eq!(snapshot.root.outcome, NodeOutcome::Unavailable(UnavailableReason::Magical));
        assert!(snapshot.root.children.is_empty());
    }

    #[test]
    fn overloaded_stringification_refuses_rendering() {
        let mut value = SourceValue::scalar("obj", "Object");
        value.flags = SourceFlags { overloaded_stringify: true, ..SourceFlags::PLAIN };
        let snapshot = capture_snapshot(value, SnapshotBudget::defaults());

        assert_eq!(
            snapshot.root.outcome,
            NodeOutcome::Unavailable(UnavailableReason::OverloadedStringify)
        );
    }

    #[test]
    fn empty_containers_remain_containers() {
        let empty: [SourceValue<'_>; 0] = [];
        let root = SourceValue::container("@empty", "array", &empty);
        let snapshot = capture_snapshot(root, SnapshotBudget::defaults());

        assert_eq!(snapshot.root.outcome, NodeOutcome::Complete);
        assert_eq!(snapshot.root.kind_label, "array");
        assert_eq!(snapshot.root.rendered, None);
        assert!(snapshot.root.children.is_empty());
    }

    #[test]
    fn empty_overloaded_containers_do_not_stringify() {
        let empty: [SourceValue<'_>; 0] = [];
        let mut root = SourceValue::container("@empty", "array", &empty);
        root.flags = SourceFlags { overloaded_stringify: true, ..SourceFlags::PLAIN };
        let snapshot = capture_snapshot(root, SnapshotBudget::defaults());

        assert_eq!(snapshot.root.outcome, NodeOutcome::Complete);
        assert_eq!(snapshot.root.rendered, None);
    }

    #[test]
    fn identical_inputs_produce_identical_snapshots() {
        let kids = [scalar("a", "1"), scalar("b", "2")];
        let root = SourceValue::container("@v", "array", &kids);
        let budget = SnapshotBudget::defaults();
        assert_eq!(capture_snapshot(root, budget), capture_snapshot(root, budget));
    }

    #[test]
    fn a_clipped_root_is_never_reported_complete() {
        // Root clipping (fewer top-level entries than exist) is recorded as a
        // truncation on the snapshot, not as a complete capture.
        let kids = [scalar("a", "1")];
        let root = SourceValue::container("@v", "array", &kids);
        let budget = SnapshotBudget { max_items_per_container: 0, ..SnapshotBudget::defaults() };
        let snapshot = capture_snapshot(root, budget);

        assert_eq!(snapshot.truncation, Some(TruncationReason::ContainerItemLimit));
        assert_eq!(
            snapshot.root.outcome,
            NodeOutcome::Truncated(TruncationReason::ContainerItemLimit)
        );
        assert!(snapshot.root.children.is_empty());
    }
}
