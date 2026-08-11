//! Structural validation for parser-produced ASTs.
//!
//! The oracle in this module is intentionally syntax-structural. It proves that
//! a returned tree is safe to traverse and project into source ranges; it does
//! not claim that the tree represents complete Perl semantics.

use crate::ast::MAX_AST_DEPTH;
use crate::{FieldId, Node, SourceLocation};

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
    /// Traversal reached a depth greater than the configured structural budget.
    DepthLimitExceeded,
}

impl AstInvariantCode {
    /// Return the stable machine token for this finding class.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ReversedRange => "reversed_range",
            Self::RangeOutOfBounds => "range_out_of_bounds",
            Self::NonUtf8Boundary => "non_utf8_boundary",
            Self::UnexpectedEmptyRange => "unexpected_empty_range",
            Self::ChildOutsideParent => "child_outside_parent",
            Self::ChildOrderRegression => "child_order_regression",
            Self::DepthLimitExceeded => "depth_limit_exceeded",
        }
    }
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
    /// Whether direct children must be emitted in nondecreasing start order.
    pub require_child_source_order: bool,
    /// Whether zero-width source ranges are accepted as synthetic/recovery spans.
    pub allow_empty_ranges: bool,
}

impl Default for AstInvariantOptions {
    fn default() -> Self {
        Self {
            max_findings: 64,
            max_depth: MAX_AST_DEPTH,
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
    /// Return `true` when no structural finding was observed.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.findings.is_empty() && !self.truncated
    }
}

struct PendingNode<'a> {
    node: &'a Node,
    path: String,
    depth: usize,
}

fn source_location(start: usize, end: usize) -> SourceLocation {
    SourceLocation { start, end }
}

fn push_finding(
    report: &mut AstInvariantReport,
    finding_limit: usize,
    finding: AstInvariantFinding,
) {
    if report.findings.len() < finding_limit {
        report.findings.push(finding);
    } else {
        report.truncated = true;
    }
}

fn child_path(parent: &str, field: Option<FieldId>, index: usize, child: &Node) -> String {
    let field_name = field.map_or("child", FieldId::name);
    format!("{parent}/{field_name}[{index}]:{}", child.kind.kind_name())
}

/// Validate an AST against the exact source bytes it claims to describe.
///
/// Traversal uses [`Node::for_each_child_with_field`], the canonical exhaustive
/// child iterator. The walk is iterative so deeply nested or adversarial trees
/// cannot overflow the Rust call stack while being checked.
#[must_use]
pub fn validate_ast(
    source: &str,
    root: &Node,
    options: AstInvariantOptions,
) -> AstInvariantReport {
    let finding_limit = options.max_findings.max(1);
    let mut report = AstInvariantReport {
        findings: Vec::new(),
        nodes_visited: 0,
        max_depth_reached: 0,
        truncated: false,
    };
    let mut pending = vec![PendingNode {
        node: root,
        path: format!("root:{}", root.kind.kind_name()),
        depth: 0,
    }];

    while let Some(current) = pending.pop() {
        if report.truncated {
            break;
        }

        report.nodes_visited = report.nodes_visited.saturating_add(1);
        report.max_depth_reached = report.max_depth_reached.max(current.depth);

        if current.depth > options.max_depth {
            push_finding(
                &mut report,
                finding_limit,
                AstInvariantFinding {
                    code: AstInvariantCode::DepthLimitExceeded,
                    node_kind: current.node.kind.kind_name().to_string(),
                    path: current.path,
                    range: current.node.location,
                    related_range: None,
                },
            );
            continue;
        }

        let range = current.node.location;
        if range.start > range.end {
            push_finding(
                &mut report,
                finding_limit,
                AstInvariantFinding {
                    code: AstInvariantCode::ReversedRange,
                    node_kind: current.node.kind.kind_name().to_string(),
                    path: current.path.clone(),
                    range,
                    related_range: None,
                },
            );
        }
        if range.start > source.len() || range.end > source.len() {
            push_finding(
                &mut report,
                finding_limit,
                AstInvariantFinding {
                    code: AstInvariantCode::RangeOutOfBounds,
                    node_kind: current.node.kind.kind_name().to_string(),
                    path: current.path.clone(),
                    range,
                    related_range: None,
                },
            );
        } else if !source.is_char_boundary(range.start) || !source.is_char_boundary(range.end) {
            push_finding(
                &mut report,
                finding_limit,
                AstInvariantFinding {
                    code: AstInvariantCode::NonUtf8Boundary,
                    node_kind: current.node.kind.kind_name().to_string(),
                    path: current.path.clone(),
                    range,
                    related_range: None,
                },
            );
        }
        if !options.allow_empty_ranges && range.start == range.end {
            push_finding(
                &mut report,
                finding_limit,
                AstInvariantFinding {
                    code: AstInvariantCode::UnexpectedEmptyRange,
                    node_kind: current.node.kind.kind_name().to_string(),
                    path: current.path.clone(),
                    range,
                    related_range: None,
                },
            );
        }

        let mut children = Vec::new();
        current
            .node
            .for_each_child_with_field(|field, child| children.push((field, child)));

        let mut previous: Option<SourceLocation> = None;
        for (index, (field, child)) in children.iter().copied().enumerate() {
            let child_range = child.location;
            let path = child_path(&current.path, field, index, child);

            if child_range.start < range.start || child_range.end > range.end {
                push_finding(
                    &mut report,
                    finding_limit,
                    AstInvariantFinding {
                        code: AstInvariantCode::ChildOutsideParent,
                        node_kind: child.kind.kind_name().to_string(),
                        path: path.clone(),
                        range: child_range,
                        related_range: Some(range),
                    },
                );
            }

            if options.require_child_source_order
                && previous.is_some_and(|previous_range| child_range.start < previous_range.start)
            {
                push_finding(
                    &mut report,
                    finding_limit,
                    AstInvariantFinding {
                        code: AstInvariantCode::ChildOrderRegression,
                        node_kind: child.kind.kind_name().to_string(),
                        path: path.clone(),
                        range: child_range,
                        related_range: previous,
                    },
                );
            }
            previous = Some(source_location(child_range.start, child_range.end));
        }

        for (index, (field, child)) in children.into_iter().enumerate().rev() {
            pending.push(PendingNode {
                node: child,
                path: child_path(&current.path, field, index, child),
                depth: current.depth.saturating_add(1),
            });
        }
    }

    report
}
