//! Bounded syntax-level callable exit summaries.
//!
//! This module inventories explicit `return` statements and the callable's
//! implicit fallthrough exit without inferring result types. It is deliberately
//! conservative: structured control flow is retained as a typed boundary until
//! the canonical control-flow graph can prove reachability and dominance.

use std::collections::BTreeSet;

use crate::ast::{Node, NodeKind};
use crate::SourceLocation;

/// Kind of callable declaration summarized by [`CallableExitSummary`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallableDeclarationKind {
    /// A Perl `sub` declaration, including anonymous subs represented by the
    /// parser's subroutine node.
    Subroutine,
    /// A Perl `method` declaration.
    Method,
}

/// One syntactic exit from a callable body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CallableExitKind {
    /// `return EXPR`.
    ExplicitValue,
    /// Bare `return`.
    ExplicitBare,
    /// A straight-line final expression that may supply Perl's implicit return.
    ImplicitValue,
    /// An empty body with an implicit void result.
    ImplicitVoid,
    /// A fallthrough exit exists, but this bounded pass cannot identify one
    /// exact returned expression.
    ImplicitUnknown,
}

/// Typed reason the syntax inventory cannot claim complete reachable-exit
/// coverage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CallableExitBoundary {
    /// Conditional or statement-modifier control requires CFG reachability.
    ConditionalControl,
    /// Loop control requires bounded fixed-point and exit analysis.
    LoopControl,
    /// Exception or eval control has unmodeled exits.
    ExceptionControl,
    /// `goto` or another dynamic transfer prevents static completeness.
    DynamicControl,
    /// Parser recovery contributed to the callable body.
    RecoveredSyntax,
    /// The callable body or final statement shape is not admitted yet.
    UnsupportedFallthrough,
    /// The traversal exceeded its deterministic node or depth budget.
    TraversalBudget,
}

/// Whether the summary covers every reachable exit for its admitted syntax
/// profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallableExitCompleteness {
    /// Complete for the current straight-line syntax profile.
    Complete,
    /// Useful exit inventory exists, but at least one typed boundary prevents
    /// complete reachability proof.
    Partial,
}

/// Deterministic traversal limits for [`CallableExitSummary::analyze_with_budget`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallableExitBudget {
    /// Maximum AST nodes inspected inside one callable body.
    pub max_nodes: usize,
    /// Maximum nested AST depth inspected inside one callable body.
    pub max_depth: usize,
}

impl Default for CallableExitBudget {
    fn default() -> Self {
        Self {
            max_nodes: 8_192,
            max_depth: 256,
        }
    }
}

/// One explicit or implicit callable exit and its source anchors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallableExit {
    /// Exit class.
    pub kind: CallableExitKind,
    /// Source range of the return statement or final fallthrough statement.
    pub statement_range: SourceLocation,
    /// Source range of the returned expression when one is statically exposed.
    pub value_range: Option<SourceLocation>,
    /// Nesting depth beneath the callable body where the exit was observed.
    pub control_depth: u16,
}

/// Bounded syntax-level inventory of a callable's exit paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallableExitSummary {
    /// Callable declaration kind.
    pub declaration_kind: CallableDeclarationKind,
    /// Declared callable name when present.
    pub callable_name: Option<String>,
    /// Full callable declaration range.
    pub callable_range: SourceLocation,
    /// Callable body range.
    pub body_range: SourceLocation,
    /// Deterministically ordered explicit and implicit exits.
    pub exits: Vec<CallableExit>,
    /// Typed boundaries preventing a complete result.
    pub boundaries: BTreeSet<CallableExitBoundary>,
    /// Whether the exit denominator is complete for the admitted syntax.
    pub completeness: CallableExitCompleteness,
    /// Nested callable declarations deliberately excluded from this summary.
    pub nested_callable_count: usize,
    /// Top-level statements proven unreachable after an unconditional return.
    pub unreachable_tail_count: usize,
    /// Number of AST nodes inspected before completion or widening.
    pub visited_nodes: usize,
}

impl CallableExitSummary {
    /// Analyze a subroutine or method with the default deterministic budget.
    #[must_use]
    pub fn analyze(callable: &Node) -> Option<Self> {
        Self::analyze_with_budget(callable, CallableExitBudget::default())
    }

    /// Analyze a subroutine or method with explicit deterministic limits.
    #[must_use]
    pub fn analyze_with_budget(callable: &Node, budget: CallableExitBudget) -> Option<Self> {
        let (declaration_kind, callable_name, body) = match &callable.kind {
            NodeKind::Subroutine { name, body, .. } => {
                (CallableDeclarationKind::Subroutine, name.clone(), body.as_ref())
            }
            NodeKind::Method { name, body, .. } => (
                CallableDeclarationKind::Method,
                Some(name.clone()),
                body.as_ref(),
            ),
            _ => return None,
        };

        let mut analyzer = ExitAnalyzer::new(budget);
        analyzer.analyze_body(body);
        analyzer.exits.sort_by_key(|exit| {
            (
                exit.statement_range.start,
                exit.statement_range.end,
                exit.kind,
                exit.control_depth,
            )
        });
        analyzer.exits.dedup();

        let completeness = if analyzer.boundaries.is_empty() {
            CallableExitCompleteness::Complete
        } else {
            CallableExitCompleteness::Partial
        };

        Some(Self {
            declaration_kind,
            callable_name,
            callable_range: callable.location,
            body_range: body.location,
            exits: analyzer.exits,
            boundaries: analyzer.boundaries,
            completeness,
            nested_callable_count: analyzer.nested_callable_count,
            unreachable_tail_count: analyzer.unreachable_tail_count,
            visited_nodes: analyzer.visited_nodes,
        })
    }
}

struct ExitAnalyzer {
    budget: CallableExitBudget,
    visited_nodes: usize,
    exits: Vec<CallableExit>,
    boundaries: BTreeSet<CallableExitBoundary>,
    nested_callable_count: usize,
    unreachable_tail_count: usize,
}

impl ExitAnalyzer {
    fn new(budget: CallableExitBudget) -> Self {
        Self {
            budget,
            visited_nodes: 0,
            exits: Vec::new(),
            boundaries: BTreeSet::new(),
            nested_callable_count: 0,
            unreachable_tail_count: 0,
        }
    }

    fn analyze_body(&mut self, body: &Node) {
        let NodeKind::Block { statements } = &body.kind else {
            self.boundaries
                .insert(CallableExitBoundary::UnsupportedFallthrough);
            self.exits.push(CallableExit {
                kind: CallableExitKind::ImplicitUnknown,
                statement_range: body.location,
                value_range: None,
                control_depth: 0,
            });
            return;
        };

        if statements.is_empty() {
            self.exits.push(CallableExit {
                kind: CallableExitKind::ImplicitVoid,
                statement_range: body.location,
                value_range: None,
                control_depth: 0,
            });
            return;
        }

        let mut last_reachable = None;
        let mut terminal_return = false;

        for (index, statement) in statements.iter().enumerate() {
            if !self.reserve_node(0) {
                break;
            }

            if let NodeKind::Return { value } = &statement.kind {
                self.record_return(statement, value.as_deref(), 0);
                self.unreachable_tail_count = statements.len().saturating_sub(index + 1);
                terminal_return = true;
                break;
            }

            self.scan_descendants_without_root(statement, 0);
            last_reachable = Some(statement);
        }

        if terminal_return {
            return;
        }

        if self
            .boundaries
            .contains(&CallableExitBoundary::TraversalBudget)
        {
            self.exits.push(CallableExit {
                kind: CallableExitKind::ImplicitUnknown,
                statement_range: body.location,
                value_range: None,
                control_depth: 0,
            });
            return;
        }

        let Some(last_statement) = last_reachable else {
            self.exits.push(CallableExit {
                kind: CallableExitKind::ImplicitUnknown,
                statement_range: body.location,
                value_range: None,
                control_depth: 0,
            });
            return;
        };

        if let Some(value_range) = implicit_value_range(last_statement) {
            self.exits.push(CallableExit {
                kind: CallableExitKind::ImplicitValue,
                statement_range: last_statement.location,
                value_range: Some(value_range),
                control_depth: 0,
            });
        } else {
            self.boundaries
                .insert(CallableExitBoundary::UnsupportedFallthrough);
            self.exits.push(CallableExit {
                kind: CallableExitKind::ImplicitUnknown,
                statement_range: last_statement.location,
                value_range: None,
                control_depth: 0,
            });
        }
    }

    fn scan_descendants_without_root(&mut self, node: &Node, depth: usize) {
        if matches!(
            &node.kind,
            NodeKind::Subroutine { .. } | NodeKind::Method { .. }
        ) {
            self.nested_callable_count = self.nested_callable_count.saturating_add(1);
            return;
        }

        self.record_boundary(node);
        for child in node.children() {
            self.scan_node(child, depth.saturating_add(1));
            if self
                .boundaries
                .contains(&CallableExitBoundary::TraversalBudget)
            {
                break;
            }
        }
    }

    fn scan_node(&mut self, node: &Node, depth: usize) {
        if !self.reserve_node(depth) {
            return;
        }

        match &node.kind {
            NodeKind::Subroutine { .. } | NodeKind::Method { .. } => {
                self.nested_callable_count = self.nested_callable_count.saturating_add(1);
                return;
            }
            NodeKind::Return { value } => {
                self.record_return(node, value.as_deref(), depth);
                return;
            }
            _ => {}
        }

        self.record_boundary(node);
        for child in node.children() {
            self.scan_node(child, depth.saturating_add(1));
            if self
                .boundaries
                .contains(&CallableExitBoundary::TraversalBudget)
            {
                break;
            }
        }
    }

    fn reserve_node(&mut self, depth: usize) -> bool {
        if self.visited_nodes >= self.budget.max_nodes || depth > self.budget.max_depth {
            self.boundaries
                .insert(CallableExitBoundary::TraversalBudget);
            return false;
        }
        self.visited_nodes = self.visited_nodes.saturating_add(1);
        true
    }

    fn record_return(&mut self, node: &Node, value: Option<&Node>, depth: usize) {
        let control_depth = u16::try_from(depth).unwrap_or(u16::MAX);
        self.exits.push(CallableExit {
            kind: if value.is_some() {
                CallableExitKind::ExplicitValue
            } else {
                CallableExitKind::ExplicitBare
            },
            statement_range: node.location,
            value_range: value.map(|value| value.location),
            control_depth,
        });
    }

    fn record_boundary(&mut self, node: &Node) {
        let boundary = match node.kind.kind_name() {
            "If" | "Unless" | "ConditionalExpression" | "StatementModifier" => {
                Some(CallableExitBoundary::ConditionalControl)
            }
            "While" | "Until" | "For" | "Foreach" | "CStyleFor" | "Continue" => {
                Some(CallableExitBoundary::LoopControl)
            }
            "Try" | "Catch" | "Finally" | "Eval" => {
                Some(CallableExitBoundary::ExceptionControl)
            }
            "Goto" => Some(CallableExitBoundary::DynamicControl),
            "Error" => Some(CallableExitBoundary::RecoveredSyntax),
            _ => None,
        };
        if let Some(boundary) = boundary {
            self.boundaries.insert(boundary);
        }
    }
}

fn implicit_value_range(statement: &Node) -> Option<SourceLocation> {
    match &statement.kind {
        NodeKind::ExpressionStatement { expression } => Some(expression.location),
        NodeKind::VariableDeclaration {
            initializer: Some(initializer),
            ..
        } => Some(initializer.location),
        NodeKind::Assignment { rhs, .. } => Some(rhs.location),
        NodeKind::Return { .. } | NodeKind::Block { .. } | NodeKind::If { .. } => None,
        NodeKind::Error { .. } => None,
        _ => Some(statement.location),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Parser;

    type TestResult = Result<(), String>;

    fn parse_first_callable(source: &str) -> Result<Node, String> {
        let mut parser = Parser::new(source);
        let ast = parser
            .parse()
            .map_err(|errors| format!("fixture should parse: {errors:?}"))?;
        find_first_callable(&ast)
            .cloned()
            .ok_or_else(|| "fixture should contain a callable".to_string())
    }

    fn summarize(source: &str) -> Result<CallableExitSummary, String> {
        let callable = parse_first_callable(source)?;
        CallableExitSummary::analyze(&callable)
            .ok_or_else(|| "callable should produce an exit summary".to_string())
    }

    fn find_first_callable(node: &Node) -> Option<&Node> {
        if matches!(
            &node.kind,
            NodeKind::Subroutine { .. } | NodeKind::Method { .. }
        ) {
            return Some(node);
        }
        node.children()
            .into_iter()
            .find_map(find_first_callable)
    }

    #[test]
    fn straight_line_implicit_value_is_complete() -> TestResult {
        let summary = summarize("sub build { Foo->new; }")?;

        assert_eq!(summary.completeness, CallableExitCompleteness::Complete);
        assert_eq!(summary.exits.len(), 1);
        assert_eq!(summary.exits[0].kind, CallableExitKind::ImplicitValue);
        assert!(summary.exits[0].value_range.is_some());
        assert!(summary.boundaries.is_empty());
        Ok(())
    }

    #[test]
    fn top_level_return_makes_later_statements_unreachable() -> TestResult {
        let summary = summarize("sub build { return 1; 'dead'; }")?;

        assert_eq!(summary.completeness, CallableExitCompleteness::Complete);
        assert_eq!(summary.exits.len(), 1);
        assert_eq!(summary.exits[0].kind, CallableExitKind::ExplicitValue);
        assert_eq!(summary.unreachable_tail_count, 1);
        Ok(())
    }

    #[test]
    fn nested_callable_returns_do_not_leak() -> TestResult {
        let summary = summarize("sub outer { sub inner { return 1; } return 2; }")?;

        assert_eq!(summary.nested_callable_count, 1);
        assert_eq!(summary.exits.len(), 1);
        assert_eq!(summary.exits[0].kind, CallableExitKind::ExplicitValue);
        Ok(())
    }

    #[test]
    fn conditional_returns_are_retained_but_partial() -> TestResult {
        let summary = summarize("sub choose { if ($flag) { return 1; } return 2; }")?;

        assert_eq!(summary.completeness, CallableExitCompleteness::Partial);
        assert!(
            summary
                .boundaries
                .contains(&CallableExitBoundary::ConditionalControl)
        );
        assert_eq!(
            summary
                .exits
                .iter()
                .filter(|exit| exit.kind == CallableExitKind::ExplicitValue)
                .count(),
            2
        );
        Ok(())
    }

    #[test]
    fn empty_body_has_complete_implicit_void_exit() -> TestResult {
        let summary = summarize("sub empty { }")?;

        assert_eq!(summary.completeness, CallableExitCompleteness::Complete);
        assert_eq!(summary.exits.len(), 1);
        assert_eq!(summary.exits[0].kind, CallableExitKind::ImplicitVoid);
        Ok(())
    }

    #[test]
    fn traversal_budget_widens_instead_of_truncating_to_complete() -> TestResult {
        let callable = parse_first_callable("sub build { my $x = Foo->new; $x->prepare; $x; }")?;
        let summary = CallableExitSummary::analyze_with_budget(
            &callable,
            CallableExitBudget {
                max_nodes: 1,
                max_depth: 1,
            },
        )
        .ok_or_else(|| "callable should produce a budgeted exit summary".to_string())?;

        assert_eq!(summary.completeness, CallableExitCompleteness::Partial);
        assert!(
            summary
                .boundaries
                .contains(&CallableExitBoundary::TraversalBudget)
        );
        assert!(
            summary
                .exits
                .iter()
                .any(|exit| exit.kind == CallableExitKind::ImplicitUnknown)
        );
        Ok(())
    }
}
