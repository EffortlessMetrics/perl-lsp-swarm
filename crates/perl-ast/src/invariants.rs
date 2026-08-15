//! Structural validation for parser-produced ASTs.
//!
//! The oracle in this module is intentionally syntax-structural. It proves that
//! a returned tree is safe to traverse and project into source ranges; it does
//! not claim that the tree represents complete Perl semantics.

use crate::ast::MAX_AST_DEPTH;
use crate::{FieldId, Node, SourceLocation};
use std::ops::ControlFlow;

/// Stable class of AST structural invariant violation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AstInvariantCode {
    /// A node's start offset is greater than its end offset.
    ReversedRange,
    /// A node range extends beyond the current source bytes.
    RangeOutOfBounds,
    /// A source-backed node range does not begin and end on UTF-8 boundaries.
    NonUtf8Boundary,
    /// A zero-width node was found where the selected policy disallows one.
    UnexpectedEmptyRange,
    /// A direct child range is not contained by its parent range.
    ChildOutsideParent,
    /// Direct children were emitted in decreasing source-start order.
    ChildOrderRegression,
    /// Traversal suppressed a child beyond the configured structural depth budget.
    DepthLimitExceeded,
    /// Traversal discovered more nodes than the configured structural budget.
    NodeLimitExceeded,
}

/// One reproducible AST structural finding.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct AstInvariantFinding {
    /// Stable invariant class.
    pub code: AstInvariantCode,
    /// `NodeKind` name at the failing location.
    pub node_kind: String,
    /// Deterministic structural path from the root.
    pub path: String,
    /// Range owned by the node named in this finding.
    pub range: SourceLocation,
    /// Related parent or preceding-child range when the finding is relational.
    pub related_range: Option<SourceLocation>,
}

/// Policy controlling AST structural validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct AstInvariantOptions {
    /// Maximum retained findings before the report is marked truncated.
    pub max_findings: usize,
    /// Maximum root-relative depth visited by the iterative traversal.
    pub max_depth: usize,
    /// Maximum number of nodes visited, including the root.
    pub max_nodes: usize,
    /// Whether direct children must be emitted in nondecreasing start order.
    pub require_child_source_order: bool,
    /// Whether zero-width source ranges are accepted as synthetic/recovery spans.
    pub allow_empty_ranges: bool,
}

impl AstInvariantOptions {
    /// Set the exact maximum number of retained findings.
    #[must_use]
    pub const fn with_max_findings(mut self, max_findings: usize) -> Self {
        self.max_findings = max_findings;
        self
    }

    /// Set the maximum root-relative traversal depth.
    #[must_use]
    pub const fn with_max_depth(mut self, max_depth: usize) -> Self {
        self.max_depth = max_depth;
        self
    }

    /// Set the maximum number of visited nodes, including the root.
    #[must_use]
    pub const fn with_max_nodes(mut self, max_nodes: usize) -> Self {
        self.max_nodes = max_nodes;
        self
    }

    /// Select whether direct-child source order is required.
    #[must_use]
    pub const fn with_child_source_order(mut self, required: bool) -> Self {
        self.require_child_source_order = required;
        self
    }

    /// Select whether zero-width source ranges are accepted.
    #[must_use]
    pub const fn with_empty_ranges(mut self, allowed: bool) -> Self {
        self.allow_empty_ranges = allowed;
        self
    }
}

impl Default for AstInvariantOptions {
    fn default() -> Self {
        Self {
            max_findings: 64,
            max_depth: MAX_AST_DEPTH,
            max_nodes: 100_000,
            require_child_source_order: true,
            allow_empty_ranges: true,
        }
    }
}

/// Bounded result of validating one AST against one exact source string.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct AstInvariantReport {
    /// Findings retained in deterministic depth-first order.
    pub findings: Vec<AstInvariantFinding>,
    /// Number of nodes visited before completion or truncation.
    pub nodes_visited: usize,
    /// Greatest root-relative depth reached.
    pub max_depth_reached: usize,
    /// Whether additional findings or traversal were suppressed by a bound.
    pub truncated: bool,
}

impl AstInvariantReport {
    /// Return `true` when at least one structural finding was retained.
    #[must_use]
    pub fn has_findings(&self) -> bool {
        !self.findings.is_empty()
    }

    /// Return `true` when traversal and finding retention completed within bounds.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        !self.truncated
    }

    /// Return `true` when no structural finding was observed and traversal completed.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        !self.has_findings() && self.is_complete()
    }
}

struct PendingNode<'a> {
    node: &'a Node,
    path: String,
    depth: usize,
}

fn push_finding(
    report: &mut AstInvariantReport,
    finding_limit: usize,
    finding: AstInvariantFinding,
) -> bool {
    if report.findings.len() < finding_limit {
        report.findings.push(finding);
        true
    } else {
        report.truncated = true;
        false
    }
}

fn child_path(parent: &str, field: Option<FieldId>, index: usize, child: &Node) -> String {
    let field_name = field.map_or("child", FieldId::name);
    format!("{parent}/{field_name}[{index}]:{}", child.kind.kind_name())
}

/// Validate an AST against the exact source bytes it claims to describe.
///
/// Traversal uses [`Node::try_for_each_child_with_field`], the canonical
/// exhaustive child iterator with short-circuiting. The walk is iterative, and
/// both depth and node count are bounded, so deep or wide adversarial trees
/// cannot overflow the call stack or allocate an unbounded child list.
#[must_use]
pub fn validate_ast(source: &str, root: &Node, options: AstInvariantOptions) -> AstInvariantReport {
    let mut report = AstInvariantReport {
        findings: Vec::new(),
        nodes_visited: 0,
        max_depth_reached: 0,
        truncated: false,
    };
    let root_path = format!("root:{}", root.kind.kind_name());

    if options.max_nodes == 0 {
        report.truncated = true;
        let _ = push_finding(
            &mut report,
            options.max_findings,
            AstInvariantFinding {
                code: AstInvariantCode::NodeLimitExceeded,
                node_kind: root.kind.kind_name().to_string(),
                path: root_path,
                range: root.location,
                related_range: None,
            },
        );
        return report;
    }

    let mut pending = vec![PendingNode { node: root, path: root_path, depth: 0 }];
    let mut node_limit_reported = false;

    'walk: while let Some(current) = pending.pop() {
        report.nodes_visited = report.nodes_visited.saturating_add(1);
        report.max_depth_reached = report.max_depth_reached.max(current.depth);

        let range = current.node.location;
        if range.start > range.end
            && !push_finding(
                &mut report,
                options.max_findings,
                AstInvariantFinding {
                    code: AstInvariantCode::ReversedRange,
                    node_kind: current.node.kind.kind_name().to_string(),
                    path: current.path.clone(),
                    range,
                    related_range: None,
                },
            )
        {
            break;
        }
        if range.start > source.len() || range.end > source.len() {
            if !push_finding(
                &mut report,
                options.max_findings,
                AstInvariantFinding {
                    code: AstInvariantCode::RangeOutOfBounds,
                    node_kind: current.node.kind.kind_name().to_string(),
                    path: current.path.clone(),
                    range,
                    related_range: None,
                },
            ) {
                break;
            }
        } else if (!source.is_char_boundary(range.start) || !source.is_char_boundary(range.end))
            && !push_finding(
                &mut report,
                options.max_findings,
                AstInvariantFinding {
                    code: AstInvariantCode::NonUtf8Boundary,
                    node_kind: current.node.kind.kind_name().to_string(),
                    path: current.path.clone(),
                    range,
                    related_range: None,
                },
            )
        {
            break;
        }
        if !options.allow_empty_ranges
            && range.start == range.end
            && !push_finding(
                &mut report,
                options.max_findings,
                AstInvariantFinding {
                    code: AstInvariantCode::UnexpectedEmptyRange,
                    node_kind: current.node.kind.kind_name().to_string(),
                    path: current.path.clone(),
                    range,
                    related_range: None,
                },
            )
        {
            break;
        }

        if current.depth == options.max_depth {
            let mut first_suppressed_child = None;
            let _ = current.node.try_for_each_child_with_field(|field, child| {
                first_suppressed_child = Some((field, child));
                ControlFlow::Break(())
            });

            if let Some((field, child)) = first_suppressed_child {
                report.truncated = true;
                if !push_finding(
                    &mut report,
                    options.max_findings,
                    AstInvariantFinding {
                        code: AstInvariantCode::DepthLimitExceeded,
                        node_kind: child.kind.kind_name().to_string(),
                        path: child_path(&current.path, field, 0, child),
                        range: child.location,
                        related_range: Some(range),
                    },
                ) {
                    break;
                }
            }
            continue;
        }

        let reserved = report.nodes_visited.saturating_add(pending.len());
        let remaining_slots = options.max_nodes.saturating_sub(reserved);
        let mut children = Vec::with_capacity(remaining_slots.min(32));
        let mut child_index = 0usize;
        let mut suppressed_child: Option<(usize, Option<FieldId>, &Node)> = None;
        let _ = current.node.try_for_each_child_with_field(|field, child| {
            let index = child_index;
            child_index = child_index.saturating_add(1);
            if children.len() >= remaining_slots {
                suppressed_child = Some((index, field, child));
                ControlFlow::Break(())
            } else {
                children.push((index, field, child));
                ControlFlow::Continue(())
            }
        });

        if let Some((index, field, child)) = suppressed_child {
            report.truncated = true;
            if !node_limit_reported {
                node_limit_reported = true;
                if !push_finding(
                    &mut report,
                    options.max_findings,
                    AstInvariantFinding {
                        code: AstInvariantCode::NodeLimitExceeded,
                        node_kind: child.kind.kind_name().to_string(),
                        path: child_path(&current.path, field, index, child),
                        range: child.location,
                        related_range: Some(range),
                    },
                ) {
                    break;
                }
            }
        }

        let mut previous: Option<SourceLocation> = None;
        for (index, field, child) in children.iter().copied() {
            let child_range = child.location;
            let path = child_path(&current.path, field, index, child);

            if (child_range.start < range.start || child_range.end > range.end)
                && !push_finding(
                    &mut report,
                    options.max_findings,
                    AstInvariantFinding {
                        code: AstInvariantCode::ChildOutsideParent,
                        node_kind: child.kind.kind_name().to_string(),
                        path: path.clone(),
                        range: child_range,
                        related_range: Some(range),
                    },
                )
            {
                break 'walk;
            }

            if options.require_child_source_order
                && previous.is_some_and(|previous_range| child_range.start < previous_range.start)
                && !push_finding(
                    &mut report,
                    options.max_findings,
                    AstInvariantFinding {
                        code: AstInvariantCode::ChildOrderRegression,
                        node_kind: child.kind.kind_name().to_string(),
                        path: path.clone(),
                        range: child_range,
                        related_range: previous,
                    },
                )
            {
                break 'walk;
            }
            previous = Some(child_range);
        }

        for (index, field, child) in children.into_iter().rev() {
            pending.push(PendingNode {
                node: child,
                path: child_path(&current.path, field, index, child),
                depth: current.depth.saturating_add(1),
            });
        }
    }

    report
}
