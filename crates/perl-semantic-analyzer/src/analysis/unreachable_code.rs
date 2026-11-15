//! Local unreachable-statement analysis shared by diagnostics and native critic.
//!
//! This local control-flow analysis proves whether control can
//! reach the next sibling statement inside one execution unit; it does not
//! make workspace-wide symbol-liveness claims.
//!
//! The implementation uses typed flow summaries rather than a flat
//! `reachable: bool` walk. Each statement reports whether any path falls
//! through and, when it does not, which control-transfer classes were observed.
//! Complete `if`/`elsif`/`else` branches can therefore propagate
//! non-fallthrough to their parent statement list without allowing a transfer
//! inside a nested callable or evaluation scope to poison the outer list.
//!
//! Traversal coverage and evaluation order are governed by the per-variant
//! disposition registry in [`super::unreachable_code_disposition`] (issue
//! #10844): every executable child field recorded there must be visited by
//! this single recursive traversal core in the registered order, and no
//! second structural walker may appear here.

use perl_parser_core::SourceLocation;
use perl_parser_core::ast::{GotoTargetForm, Node, NodeKind};

/// A statement proven unreachable from its preceding local siblings.
///
/// This evidence carries a source byte range only. Diagnostic codes, severity,
/// messages, policy identity and remediation belong to each consumer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnreachableStatement {
    /// Byte range of the unreachable statement in the analyzed AST.
    pub range: SourceLocation,
}

/// One exact local transfer observed by the PL406 flow summarizer.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ControlTransfer {
    Return,
    Raise,
    ProcessTransfer,
    GotoLabel(String),
    DynamicGoto,
    ContinueLoop { label: Option<String> },
    BreakLoop { label: Option<String> },
    RedoLoop { label: Option<String> },
}

/// Whether a statement or block can reach its next sibling, plus the control
/// transfers observed on any path through it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FlowSummary {
    can_fall_through: bool,
    transfers: Vec<ControlTransfer>,
}

impl FlowSummary {
    fn falls_through() -> Self {
        Self { can_fall_through: true, transfers: Vec::new() }
    }

    fn transfer(transfer: ControlTransfer) -> Self {
        Self { can_fall_through: false, transfers: vec![transfer] }
    }

    fn alternatives(branches: Vec<Self>, exhaustive: bool) -> Self {
        let can_fall_through = !exhaustive || branches.iter().any(|branch| branch.can_fall_through);
        let transfers = branches.into_iter().flat_map(|branch| branch.transfers).collect();
        Self { can_fall_through, transfers }
    }

    /// Demote loop-control transfers at a bare-block boundary.
    ///
    /// A bare BLOCK is semantically a one-shot loop (perlsyn), so unlabeled
    /// `last`/`next`, or labeled `last`/`next` whose label names the block
    /// itself, terminate or skip to the end of the block — the parent
    /// statement list keeps a reachable path even when the block's summary
    /// does not fall through. Non-loop transfers (`die`, `return`, `goto`)
    /// always promote unchanged.
    ///
    /// Importantly, two classes of loop control must **not** be demoted:
    ///
    /// * `redo` (with or without label) — `redo` *restarts* the current
    ///   iteration. In a one-shot bare block `redo` creates an infinite loop;
    ///   no execution path falls through to the parent statement list.
    /// * Labeled `last`/`next` whose label names an *outer* construct —
    ///   those transfers escape the bare block entirely and must remain visible
    ///   to the outer summarizer so it can close fallthrough correctly.
    ///
    /// `block_label` is `Some(label)` for a labeled bare block (`LABEL: {
    /// … }`), `None` for an anonymous one.
    fn without_loop_transfers(mut self, block_label: Option<&str>) -> Self {
        let is_demotable = |transfer: &ControlTransfer| match transfer {
            // `last`/`next` that target the block itself exit it normally;
            // the parent statement list regains a fallthrough path.
            ControlTransfer::BreakLoop { label } | ControlTransfer::ContinueLoop { label } => {
                label.is_none() || label.as_deref() == block_label
            }
            // `redo` always restarts the target construct. Even when the label
            // matches the block's own label the effect is an infinite loop —
            // the parent statement list does not regain a fallthrough path.
            ControlTransfer::RedoLoop { .. } => false,
            _ => false,
        };
        let had_demotable = self.transfers.iter().any(&is_demotable);
        if had_demotable {
            self.transfers.retain(|t| !is_demotable(t));
            // Demotable transfers represent paths that exit the block normally,
            // so the parent gains at least one fallthrough path.
            self.can_fall_through = true;
        }
        self
    }

    fn goto_labels(&self) -> Vec<String> {
        self.transfers
            .iter()
            .filter_map(|transfer| match transfer {
                ControlTransfer::GotoLabel(label) => Some(label.clone()),
                _ => None,
            })
            .collect()
    }
}

/// Find statements unreachable from preceding siblings in their local list.
///
/// Results preserve traversal order. Transfers in nested callable or evaluation
/// scopes do not close fallthrough in their containing execution unit.
#[must_use]
pub fn find_unreachable_statements(root: &Node) -> Vec<UnreachableStatement> {
    let mut findings = Vec::new();
    let _ = summarize_node(root, &mut findings);
    findings
}

fn summarize_node(node: &Node, findings: &mut Vec<UnreachableStatement>) -> FlowSummary {
    match &node.kind {
        NodeKind::Program { statements } | NodeKind::Block { statements } => {
            summarize_statement_list(statements, findings)
        }

        // Callable declarations introduce a fresh execution unit. Analyze the
        // body, but the declaration itself falls through in its parent list.
        NodeKind::Subroutine { body, .. } | NodeKind::Method { body, .. } => {
            let _ = summarize_node(body, findings);
            FlowSummary::falls_through()
        }

        NodeKind::If { condition, then_branch, elsif_branches, else_branch, .. } => {
            summarize_if(condition, then_branch, elsif_branches, else_branch.as_deref(), findings)
        }

        // Loops are independent local scopes. Transfers inside their body or
        // continue block do not make the statement after the loop unreachable.
        NodeKind::While { condition, body, continue_block, .. } => {
            let _ = summarize_expression(condition, findings);
            let _ = summarize_node(body, findings);
            summarize_loop_continue(continue_block, findings);
            FlowSummary::falls_through()
        }
        NodeKind::For { init, condition, update, body, continue_block, .. } => {
            // Registry execution order (issue #10844): init, condition, body,
            // continue block, update. The initializer and the initial
            // condition evaluation run exactly once before the first
            // iteration, so a terminal transfer in either means control never
            // reaches this loop or anything after it in the enclosing list:
            // every field still receives local analysis, but the first such
            // transfer becomes this statement's summary.
            let mut entry_gate = None;
            if let Some(init) = init {
                let summary = summarize_expression(init, findings);
                if !summary.can_fall_through {
                    entry_gate = Some(summary);
                }
            }
            if entry_gate.is_none()
                && let Some(condition) = condition
            {
                let summary = summarize_expression(condition, findings);
                if !summary.can_fall_through {
                    entry_gate = Some(summary);
                }
            }
            let _ = summarize_node(body, findings);
            summarize_loop_continue(continue_block, findings);
            if let Some(update) = update {
                let _ = summarize_expression(update, findings);
            }
            entry_gate.unwrap_or_else(FlowSummary::falls_through)
        }
        NodeKind::Foreach { variable, list, body, continue_block } => {
            // The loop variable is a binding alias (analyzed locally, never
            // propagated); the list executes before each body iteration.
            let _ = summarize_expression(variable, findings);
            let _ = summarize_expression(list, findings);
            let _ = summarize_node(body, findings);
            summarize_loop_continue(continue_block, findings);
            FlowSummary::falls_through()
        }

        // These constructs are analyzed locally, but their transfer summaries
        // are deliberately not promoted into the containing execution unit.
        NodeKind::Given { expr, body } => {
            let _ = summarize_expression(expr, findings);
            let _ = summarize_node(body, findings);
            FlowSummary::falls_through()
        }
        NodeKind::When { condition, body } => {
            let _ = summarize_expression(condition, findings);
            let _ = summarize_node(body, findings);
            FlowSummary::falls_through()
        }
        NodeKind::Default { body }
        | NodeKind::PhaseBlock { block: body, .. }
        | NodeKind::Class { body, .. } => {
            let _ = summarize_node(body, findings);
            FlowSummary::falls_through()
        }
        NodeKind::Package { block, .. } => {
            if let Some(block) = block {
                let _ = summarize_node(block, findings);
            }
            FlowSummary::falls_through()
        }
        NodeKind::Eval { block } | NodeKind::Do { block } | NodeKind::Defer { block } => {
            let _ = summarize_node(block, findings);
            FlowSummary::falls_through()
        }
        NodeKind::Try { body, catch_blocks, finally_block } => {
            let _ = summarize_node(body, findings);
            for (_, catch_body) in catch_blocks {
                let _ = summarize_node(catch_body, findings);
            }
            if let Some(finally_block) = finally_block {
                let _ = summarize_node(finally_block, findings);
            }
            FlowSummary::falls_through()
        }

        NodeKind::StatementModifier { statement, condition, .. } => {
            // Perl evaluates the modifier condition before the controlled
            // statement; visit in that registered order (issue #10844).
            let condition_summary = summarize_expression(condition, findings);
            let statement_summary = summarize_node(statement, findings);
            // A terminal condition never reaches the controlled statement and
            // control exits before the next sibling, so the condition's
            // summary is the whole statement's summary (nested findings in
            // the controlled statement were still emitted above).
            if !condition_summary.can_fall_through {
                return condition_summary;
            }
            // Without an accepted constant-value fact, a statement modifier
            // always retains a path that skips the controlled statement. Keep
            // the controlled transfer as an alternative so a later terminal
            // transfer (for example `last if $cond; redo;`) cannot hide a
            // possible exit from a surrounding bare block.
            FlowSummary { can_fall_through: true, transfers: statement_summary.transfers }
        }
        NodeKind::LabeledStatement { label, statement } => {
            let summary = summarize_node(statement, findings);
            // A labeled bare block is still a one-shot loop, so `last LABEL`
            // targets the block itself rather than the enclosing loop.
            if matches!(&statement.kind, NodeKind::Block { .. }) {
                summary.without_loop_transfers(Some(label.as_str()))
            } else {
                summary
            }
        }

        NodeKind::Return { value } => {
            if let Some(value) = value {
                let _ = summarize_expression(value, findings);
            }
            FlowSummary::transfer(ControlTransfer::Return)
        }
        NodeKind::LoopControl { op, label } => summarize_loop_control(op, label),
        NodeKind::Goto { target, form } => {
            // A computed goto target (`goto EXPR`, `goto &sub`) executes at
            // runtime before the transfer happens, so executable children
            // inside it (for example a `do { ... }` block) receive the same
            // local PL406 analysis as any other evaluated expression. Only
            // the plain-label form is a bare name with nothing to execute.
            if !matches!(form, GotoTargetForm::Label) {
                let _ = summarize_expression(target, findings);
            }
            summarize_goto(target, form)
        }
        NodeKind::TargetlessGoto {} => {
            // Honest targetless goto has no target expression to summarize;
            // it transfers control unconditionally with no executed child.
            FlowSummary::transfer(ControlTransfer::DynamicGoto)
        }

        NodeKind::ExpressionStatement { expression } => summarize_expression(expression, findings),
        NodeKind::VariableDeclaration { initializer, .. }
        | NodeKind::VariableListDeclaration { initializer, .. } => {
            // The initializer is the declaration's executable child: an
            // exact transfer inside it (for example `my $x = die("stop");`)
            // prevents the next sibling from running, so its summary
            // determines parent fallthrough exactly like an assignment's rhs.
            match initializer {
                Some(initializer) => {
                    let init_summary = summarize_expression(initializer, findings);
                    if init_summary.can_fall_through {
                        FlowSummary::falls_through()
                    } else {
                        init_summary
                    }
                }
                None => FlowSummary::falls_through(),
            }
        }

        // Recovered syntax is useful for nested findings but cannot provide
        // exact parent non-fallthrough authority.
        NodeKind::Error { partial, .. } => {
            if let Some(partial) = partial {
                let _ = summarize_node(partial, findings);
            }
            FlowSummary::falls_through()
        }

        NodeKind::FunctionCall { name, args } => summarize_function_call(name, args, findings),
        NodeKind::AmperCall { args, .. } => {
            analyze_expression_list(args, findings);
            FlowSummary::falls_through()
        }
        NodeKind::MethodCall { object, args, .. } | NodeKind::IndirectCall { object, args, .. } => {
            let _ = summarize_expression(object, findings);
            analyze_expression_list(args, findings);
            // Method/function spelling alone is not non-returning authority.
            FlowSummary::falls_through()
        }

        _ => summarize_expression(node, findings),
    }
}

fn summarize_if(
    condition: &Node,
    then_branch: &Node,
    elsif_branches: &[(Box<Node>, Box<Node>)],
    else_branch: Option<&Node>,
    findings: &mut Vec<UnreachableStatement>,
) -> FlowSummary {
    let _ = summarize_expression(condition, findings);

    let mut branches = vec![summarize_node(then_branch, findings)];
    for (elsif_condition, elsif_body) in elsif_branches {
        let _ = summarize_expression(elsif_condition, findings);
        branches.push(summarize_node(elsif_body, findings));
    }

    let exhaustive = else_branch.is_some();
    if let Some(else_branch) = else_branch {
        branches.push(summarize_node(else_branch, findings));
    }

    FlowSummary::alternatives(branches, exhaustive)
}

fn summarize_statement_list(
    stmts: &[Node],
    findings: &mut Vec<UnreachableStatement>,
) -> FlowSummary {
    let mut can_fall_through = true;
    let mut terminal_summary = FlowSummary::falls_through();
    let mut pending_goto_labels: Vec<String> = Vec::new();

    for stmt in stmts {
        if !can_fall_through {
            let restores_fallthrough = labeled_statement_name(stmt).is_some_and(|label| {
                pending_goto_labels.iter().any(|target| target.as_str() == label)
            });

            if restores_fallthrough {
                can_fall_through = true;
                // Only the consumed label leaves the pending set: other live
                // goto targets recorded on earlier fallthrough statements
                // stay restorable by their own labeled statements.
                if let Some(consumed) = labeled_statement_name(stmt) {
                    pending_goto_labels.retain(|target| target != consumed);
                }
            } else {
                emit_unreachable(stmt, findings);
                // Nested callables and blocks still deserve their own local
                // analysis even when the containing statement is unreachable.
                let _ = summarize_node(stmt, findings);
                continue;
            }
        }

        let summary = summarize_node(stmt, findings);
        // A bare BLOCK statement is a one-shot loop (perlsyn): loop-control
        // transfers target the block itself and must not close fallthrough in
        // this list. Branch/body blocks of `if`, loops, and callables are not
        // loop targets and keep their verbatim summary. An anonymous bare
        // block carries no label, so only unlabeled loop controls are demoted.
        let summary = if matches!(&stmt.kind, NodeKind::Block { .. }) {
            summary.without_loop_transfers(None)
        } else {
            summary
        };
        // A fallthrough summary may still carry a conditional transfer. Keep
        // it alongside later terminal transfers so bare-block demotion can
        // distinguish `last if $cond; redo;` from an unconditional `redo`.
        terminal_summary.transfers.extend(summary.transfers.iter().cloned());
        // Goto targets observed on any live path stay restorable until a
        // labeled statement consumes them (issue #10844): a conditional
        // `goto NEXT` carried by an earlier fallthrough statement keeps the
        // `NEXT:` sibling reachable even when a later transfer closes the
        // sequential path. Unreachable statements never contribute targets:
        // their summaries are not extended here.
        pending_goto_labels.extend(summary.goto_labels());
        if summary.can_fall_through {
            terminal_summary.can_fall_through = true;
        } else {
            can_fall_through = false;
            terminal_summary.can_fall_through = false;
        }
    }

    if can_fall_through { FlowSummary::falls_through() } else { terminal_summary }
}

fn summarize_expression(node: &Node, findings: &mut Vec<UnreachableStatement>) -> FlowSummary {
    match &node.kind {
        NodeKind::FunctionCall { name, args } => summarize_function_call(name, args, findings),
        NodeKind::AmperCall { args, .. } => {
            analyze_expression_list(args, findings);
            FlowSummary::falls_through()
        }
        NodeKind::MethodCall { object, args, .. } | NodeKind::IndirectCall { object, args, .. } => {
            let _ = summarize_expression(object, findings);
            analyze_expression_list(args, findings);
            FlowSummary::falls_through()
        }
        NodeKind::Subroutine { body, .. } | NodeKind::Method { body, .. } => {
            let _ = summarize_node(body, findings);
            FlowSummary::falls_through()
        }
        NodeKind::Ternary { condition, then_expr, else_expr } => {
            let _ = summarize_expression(condition, findings);
            FlowSummary::alternatives(
                vec![
                    summarize_expression(then_expr, findings),
                    summarize_expression(else_expr, findings),
                ],
                true,
            )
        }
        NodeKind::Assignment { lhs, rhs, .. } => {
            // Perl evaluates the rhs before the lhs. Visit both sides so a
            // transferring lhs cannot skip findings nested in the rhs,
            // then select the terminal summary in evaluation order.
            let rhs_summary = summarize_expression(rhs, findings);
            let lhs_summary = summarize_expression(lhs, findings);
            if !rhs_summary.can_fall_through { rhs_summary } else { lhs_summary }
        }
        NodeKind::Unary { operand, .. } => summarize_expression(operand, findings),
        NodeKind::Binary { left, right, .. } => {
            let _ = summarize_expression(left, findings);
            let _ = summarize_expression(right, findings);
            // Perl's short-circuit and overloaded binary operators require
            // stronger semantic facts before a child transfer can be promoted.
            FlowSummary::falls_through()
        }
        NodeKind::ArrayLiteral { elements } => {
            analyze_expression_list(elements, findings);
            FlowSummary::falls_through()
        }
        NodeKind::HashLiteral { pairs } => {
            for (key, value) in pairs {
                let _ = summarize_expression(key, findings);
                let _ = summarize_expression(value, findings);
            }
            FlowSummary::falls_through()
        }
        NodeKind::Eval { block } | NodeKind::Do { block } | NodeKind::Defer { block } => {
            let _ = summarize_node(block, findings);
            FlowSummary::falls_through()
        }
        // Expression-position blocks (for example map/grep/sort block
        // arguments) are evaluation boundaries: nested statements receive
        // local analysis, but their transfers never promote into the
        // containing execution unit (issue #10844).
        NodeKind::Block { statements } => {
            let _ = summarize_statement_list(statements, findings);
            FlowSummary::falls_through()
        }
        // Remaining executable child fields declared by the disposition
        // registry are visited for nested findings; each container
        // conservatively falls through.
        NodeKind::ArraySlice { target, indices, .. } => {
            let _ = summarize_expression(target, findings);
            let _ = summarize_expression(indices, findings);
            FlowSummary::falls_through()
        }
        NodeKind::HashSlice { target, keys, .. } | NodeKind::KeyValueSlice { target, keys, .. } => {
            let _ = summarize_expression(target, findings);
            let _ = summarize_expression(keys, findings);
            FlowSummary::falls_through()
        }
        NodeKind::ChainedComparison { operands, .. } => {
            analyze_expression_list(operands, findings);
            FlowSummary::falls_through()
        }
        NodeKind::Match { expr, .. }
        | NodeKind::Substitution { expr, .. }
        | NodeKind::Transliteration { expr, .. } => {
            let _ = summarize_expression(expr, findings);
            FlowSummary::falls_through()
        }
        NodeKind::Tie { variable, package, args, .. } => {
            let _ = summarize_expression(variable, findings);
            let _ = summarize_expression(package, findings);
            analyze_expression_list(args, findings);
            FlowSummary::falls_through()
        }
        NodeKind::Untie { variable, .. } => {
            let _ = summarize_expression(variable, findings);
            FlowSummary::falls_through()
        }
        _ => FlowSummary::falls_through(),
    }
}

fn summarize_function_call(
    name: &str,
    args: &[Node],
    findings: &mut Vec<UnreachableStatement>,
) -> FlowSummary {
    analyze_expression_list(args, findings);
    match name {
        "die" | "CORE::die" => FlowSummary::transfer(ControlTransfer::Raise),
        "exit" | "CORE::exit" | "exec" | "CORE::exec" => {
            FlowSummary::transfer(ControlTransfer::ProcessTransfer)
        }
        // `croak`/`confess` are Carp's documented non-returning API and were
        // accepted PL406 terminators under closed issue #5062 ("verified
        // correct — do not regress", fixed by #5151). Framework spellings
        // such as `fatal` or `$object->throw()` remain conservative: without
        // a canonical semantic fact they are not non-returning authority.
        "croak" | "Carp::croak" | "confess" | "Carp::confess" => {
            FlowSummary::transfer(ControlTransfer::Raise)
        }
        _ => FlowSummary::falls_through(),
    }
}

fn analyze_expression_list(nodes: &[Node], findings: &mut Vec<UnreachableStatement>) {
    for node in nodes {
        let _ = summarize_expression(node, findings);
    }
}

fn summarize_loop_continue(
    continue_block: &Option<Box<Node>>,
    findings: &mut Vec<UnreachableStatement>,
) {
    if let Some(continue_block) = continue_block {
        let _ = summarize_node(continue_block, findings);
    }
}

fn summarize_loop_control(op: &str, label: &Option<String>) -> FlowSummary {
    let transfer = match op {
        "next" => ControlTransfer::ContinueLoop { label: label.clone() },
        "last" => ControlTransfer::BreakLoop { label: label.clone() },
        "redo" => ControlTransfer::RedoLoop { label: label.clone() },
        _ => return FlowSummary::falls_through(),
    };
    FlowSummary::transfer(transfer)
}

fn summarize_goto(target: &Node, form: &GotoTargetForm) -> FlowSummary {
    match form {
        GotoTargetForm::Label => {
            if let NodeKind::Identifier { name } = &target.kind {
                FlowSummary::transfer(ControlTransfer::GotoLabel(name.clone()))
            } else {
                FlowSummary::transfer(ControlTransfer::DynamicGoto)
            }
        }
        GotoTargetForm::Sub | GotoTargetForm::Expr => {
            FlowSummary::transfer(ControlTransfer::DynamicGoto)
        }
        _ => FlowSummary::transfer(ControlTransfer::DynamicGoto),
    }
}

fn labeled_statement_name(node: &Node) -> Option<&str> {
    let NodeKind::LabeledStatement { label, .. } = &node.kind else {
        return None;
    };
    Some(label.as_str())
}

fn emit_unreachable(stmt: &Node, findings: &mut Vec<UnreachableStatement>) {
    findings.push(UnreachableStatement { range: stmt.location });
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_parser_core::Parser;
    use perl_tdd_support::{must, must_some};

    fn unreachable_diags(source: &str) -> Vec<UnreachableStatement> {
        let mut parser = Parser::new(source);
        let ast = must(parser.parse());
        assert!(
            parser.errors().is_empty(),
            "test source must parse cleanly; a recovered Error node falls through and \
             would make absence assertions vacuous: {:?}",
            parser.errors()
        );
        find_unreachable_statements(&ast)
    }

    fn has_pl406(diags: &[UnreachableStatement]) -> bool {
        !diags.is_empty()
    }

    fn count_pl406(diags: &[UnreachableStatement]) -> usize {
        diags.len()
    }

    fn assert_pl406_at(source: &str, diags: &[UnreachableStatement], anchor: &str) {
        assert_eq!(
            count_pl406(diags),
            1,
            "the scenario should produce exactly one PL406: {diags:?}"
        );
        let start = must_some(source.find(anchor));
        let pl406 = must_some(diags.first());
        assert_eq!(
            (pl406.range.start, pl406.range.end),
            (start, start + anchor.len()),
            "PL406 must identify the intended unreachable statement {anchor:?}: {diags:?}"
        );
    }

    #[test]
    fn return_then_statement_is_flagged() {
        let source = "sub f { return 1; my $x = 2; }";
        let diags = unreachable_diags(source);
        assert!(has_pl406(&diags), "statement after return should be PL406: {diags:?}");
        let anchor = "my $x = 2";
        let start = must_some(source.find(anchor));
        let pl406 = must_some(diags.first());
        assert_eq!(
            (pl406.range.start, pl406.range.end),
            (start, start + anchor.len()),
            "PL406 must identify the unreachable statement itself, not just exist: {diags:?}"
        );
    }

    #[test]
    fn elsif_body_fallthrough_keeps_following_sibling_reachable() {
        let diags = unreachable_diags(
            r#"sub f { if ($x) { return 1; } elsif ($y) { print "live"; } else { exit 0; } print "also live"; }"#,
        );
        assert!(
            !has_pl406(&diags),
            "a fallthrough elsif body must keep the parent reachable even when \
             then and else both transfer: {diags:?}"
        );
    }

    #[test]
    fn conditional_return_keeps_fallthrough() {
        let diags = unreachable_diags("sub f { return if $cond; my $x = 1; }");
        assert!(!has_pl406(&diags), "conditional return must preserve fallthrough: {diags:?}");
    }

    #[test]
    fn complete_if_else_transfers_make_following_sibling_unreachable() {
        let diags = unreachable_diags(
            r#"sub f { if ($x) { return 1; } else { die "stop"; } print "dead"; }"#,
        );
        assert_eq!(
            count_pl406(&diags),
            1,
            "complete transferring branches should close fallthrough: {diags:?}"
        );
    }

    #[test]
    fn one_fallthrough_branch_keeps_following_sibling_reachable() {
        let diags = unreachable_diags(
            r#"sub f { if ($x) { return 1; } else { print "live"; } print "also live"; }"#,
        );
        assert!(
            !has_pl406(&diags),
            "one fallthrough branch must keep the parent reachable: {diags:?}"
        );
    }

    #[test]
    fn conditional_without_else_keeps_following_sibling_reachable() {
        let diags = unreachable_diags("sub f { if ($x) { return 1; } print 'live'; }");
        assert!(
            !has_pl406(&diags),
            "a missing else leaves an uncovered fallthrough path: {diags:?}"
        );
    }

    #[test]
    fn complete_elsif_chain_transfers() {
        let diags = unreachable_diags(
            r#"sub f { if ($x) { return 1; } elsif ($y) { die "y"; } else { exit 2; } print "dead"; }"#,
        );
        assert_eq!(count_pl406(&diags), 1, "all complete branches transfer: {diags:?}");
    }

    #[test]
    fn nested_sub_transfer_does_not_poison_outer_scope() {
        let diags = unreachable_diags("sub outer { my $f = sub { return 1; }; my $x = 2; }");
        assert!(!has_pl406(&diags), "nested callable transfer must remain local: {diags:?}");
    }

    #[test]
    fn nested_sub_still_reports_its_own_unreachable_sibling() {
        let diags = unreachable_diags("sub outer { my $f = sub { return 1; my $dead = 99; }; }");
        assert_eq!(count_pl406(&diags), 1, "nested callable should retain local PL406: {diags:?}");
    }

    #[test]
    fn eval_reports_local_unreachable_but_outer_scope_falls_through() {
        let diags = unreachable_diags(
            r#"sub f { eval { die "inner"; print "dead"; }; print "outer live"; }"#,
        );
        assert_eq!(count_pl406(&diags), 1, "only the inner eval sibling is unreachable: {diags:?}");
    }

    #[test]
    fn die_then_statement_is_flagged() {
        let diags = unreachable_diags(r#"sub f { die "error"; print "never"; }"#);
        assert!(has_pl406(&diags), "statement after die should be PL406: {diags:?}");
    }

    #[test]
    fn exit_and_exec_are_process_transfers() {
        for source in [
            "sub f { exit 0; print 'dead'; }",
            r#"sub f { exec("perl", "-e", "1"); print "dead"; }"#,
            "sub f { CORE::exit(0); print 'dead'; }",
        ] {
            let diags = unreachable_diags(source);
            assert!(
                has_pl406(&diags),
                "exact process transfer should close fallthrough: {diags:?}"
            );
        }
    }

    #[test]
    fn carp_nonreturning_functions_close_fallthrough() {
        // Accepted PL406 terminator authority under closed issue #5062.
        for source in [
            r#"sub f { croak "stop"; print "dead"; }"#,
            r#"sub f { Carp::croak "stop"; print "dead"; }"#,
            r#"sub f { confess "stop"; print "dead"; }"#,
            r#"sub f { Carp::confess "stop"; print "dead"; }"#,
        ] {
            let diags = unreachable_diags(source);
            assert!(
                has_pl406(&diags),
                "Carp's documented non-returning API must close fallthrough: {diags:?}"
            );
        }
    }

    #[test]
    fn framework_call_spelling_alone_is_not_nonreturning_authority() {
        for source in [
            r#"sub f { fatal(); print "reachable"; }"#,
            r#"sub f { $object->throw(); print "reachable"; }"#,
        ] {
            let diags = unreachable_diags(source);
            assert!(
                !has_pl406(&diags),
                "framework spelling without a semantic fact cannot prove non-return: {diags:?}"
            );
        }
    }

    #[test]
    fn bare_block_loop_control_exits_only_the_block() {
        // A bare BLOCK is a one-shot loop (perlsyn): last/next/redo leave or
        // restart the block, so the following sibling stays reachable.
        for source in [
            r#"sub f { { last; } print "after"; }"#,
            r#"{ next; } print "after";"#,
            r#"while ($c) { { last; } print "still-in-body"; last; }"#,
        ] {
            let diags = unreachable_diags(source);
            assert!(
                !has_pl406(&diags),
                "loop control inside a bare block must not close parent fallthrough: {diags:?}"
            );
        }
    }

    #[test]
    fn labeled_bare_block_last_exits_only_the_block() {
        let diags = unreachable_diags("DONE: { do_it(); last DONE; } print 'after';");
        assert!(
            !has_pl406(&diags),
            "last LABEL targeting a labeled bare block must keep the sibling reachable: {diags:?}"
        );
    }

    #[test]
    fn bare_block_raise_still_closes_fallthrough() {
        let diags = unreachable_diags(r#"{ die "stop"; } print "dead";"#);
        assert_eq!(
            count_pl406(&diags),
            1,
            "non-loop transfers still promote out of a bare block: {diags:?}"
        );
    }

    #[test]
    fn assignment_visits_rhs_even_when_lhs_transfers() {
        // Perl evaluates the rhs of an assignment before the lhs. A lhs that
        // provably transfers must not skip rhs traversal: unreachable
        // statements nested in the rhs would otherwise go unreported.
        let diags =
            unreachable_diags(r#"sub f { ($c ? exit : die) = do { die "x"; print "dead"; }; }"#);
        assert_eq!(
            count_pl406(&diags),
            1,
            "rhs do-block must still report its unreachable statement: {diags:?}"
        );
    }

    #[test]
    fn loop_control_in_if_branches_still_targets_the_enclosing_loop() {
        // if/else branch blocks are NOT loop blocks (perlsyn): a `last` in
        // every complete branch exits the enclosing while, so the sibling
        // after the if inside the loop body is genuinely unreachable.
        let diags =
            unreachable_diags("while (1) { if ($c) { last; } else { last; } print 'dead'; }");
        assert_eq!(
            count_pl406(&diags),
            1,
            "complete if/else last branches must still close loop-body fallthrough: {diags:?}"
        );
    }

    #[test]
    fn binary_short_circuit_does_not_promote_child_transfer() {
        let diags = unreachable_diags(r#"exec("perl", "-e", "1") or die; my $x = 1;"#);
        assert!(
            !has_pl406(&diags),
            "binary expression keeps a conservative fallthrough path: {diags:?}"
        );
    }

    #[test]
    fn loop_controls_flag_later_siblings_inside_loop_body() {
        for source in [
            "for my $i (1..5) { next; print $i; }",
            "for my $i (1..5) { last; print $i; }",
            "while ($ready) { redo; print $ready; }",
        ] {
            let diags = unreachable_diags(source);
            assert!(
                has_pl406(&diags),
                "unconditional loop control closes local fallthrough: {diags:?}"
            );
        }
    }

    #[test]
    fn loop_controls_flag_later_siblings_inside_continue_block() {
        for source in [
            "while ($ready) { work(); } continue { next; print $ready; }",
            "while ($ready) { work(); } continue { last; print $ready; }",
            "while ($ready) { work(); } continue { redo; print $ready; }",
        ] {
            let diags = unreachable_diags(source);
            assert!(
                has_pl406(&diags),
                "continue-block loop control closes sibling fallthrough: {diags:?}"
            );
        }
    }

    #[test]
    fn postfix_loop_control_keeps_fallthrough() {
        for source in [
            "while ($ready) { work(); } continue { next if $skip; print $ready; }",
            "while ($ready) { work(); } continue { last unless $ready; print $ready; }",
            "while ($ready) { work(); } continue { redo while $retry; print $ready; }",
        ] {
            let diags = unreachable_diags(source);
            assert!(!has_pl406(&diags), "conditional loop control retains fallthrough: {diags:?}");
        }
    }

    #[test]
    fn loop_transfer_does_not_poison_code_after_loop() {
        for source in [
            "while ($ready) { next; } print 'after';",
            "while ($ready) { redo; } print 'after';",
            "while ($ready) { last; } print 'after';",
        ] {
            let diags = unreachable_diags(source);
            assert!(
                !has_pl406(&diags),
                "loop transfer remains inside the loop statement: {diags:?}"
            );
        }
    }

    #[test]
    fn goto_forward_label_restores_reachability() {
        let diags = unreachable_diags("goto DONE; print 'dead'; DONE: print 'alive';");
        assert_eq!(
            count_pl406(&diags),
            1,
            "only code before the target label is unreachable: {diags:?}"
        );
    }

    #[test]
    fn dynamic_goto_does_not_fall_through() {
        let diags = unreachable_diags("goto $target; print 'dead';");
        assert_eq!(
            count_pl406(&diags),
            1,
            "dynamic goto transfers without sibling fallthrough: {diags:?}"
        );
    }

    #[test]
    fn targetless_goto_does_not_fall_through() {
        let diags = unreachable_diags("goto; print 'dead';");
        assert_eq!(
            count_pl406(&diags),
            1,
            "targetless goto transfers without sibling fallthrough: {diags:?}"
        );
    }

    #[test]
    fn multiple_statements_after_transfer_are_flagged() {
        let diags = unreachable_diags("sub f { return; my $a = 1; my $b = 2; }");
        assert_eq!(count_pl406(&diags), 2, "all later siblings should be PL406: {diags:?}");
    }

    #[test]
    fn clean_sub_has_no_pl406() {
        let diags = unreachable_diags("sub f { my $x = 1; my $y = 2; return $x + $y; }");
        assert!(!has_pl406(&diags), "ordinary reachable code should remain clean: {diags:?}");
    }

    // ── label-aware demotion (issue #10804) ──────────────────────────────────

    /// `last OUTER` inside a bare block exits the enclosing loop, not the
    /// block. The bare block therefore does not fall through, and the
    /// statement after it inside the loop body is unreachable.
    #[test]
    fn last_outer_label_in_bare_block_closes_loop_body_fallthrough() {
        // last OUTER exits the while, so "dead" never executes.
        let source = "OUTER: while (1) { { last OUTER; } print \"dead\"; }";
        let diags = unreachable_diags(source);
        assert_pl406_at(source, &diags, "print \"dead\"");
    }

    /// `redo LABEL` targeting a labeled bare block restarts the block from the
    /// beginning, creating an infinite loop. The statement after the labeled
    /// block is unreachable.
    #[test]
    fn redo_own_label_bare_block_closes_following_sibling() {
        let source = "R: { redo R; } print \"x\";";
        let diags = unreachable_diags(source);
        assert_pl406_at(source, &diags, "print \"x\"");
    }

    /// `redo` without a label in a bare block also creates an infinite loop
    /// (one-shot loops have no next iteration to `redo` into other than
    /// themselves). The following sibling is unreachable.
    #[test]
    fn redo_unlabeled_bare_block_closes_following_sibling() {
        let source = "{ redo; } print \"y\";";
        let diags = unreachable_diags(source);
        assert_pl406_at(source, &diags, "print \"y\"");
    }

    /// A conditional `last` can become true after `redo` restarts the bare
    /// block. The sibling after the block is therefore reachable.
    #[test]
    fn conditional_last_before_redo_preserves_reachable_fallthrough() {
        let source = r#"my $i = 0; { ++$i; last if $i > 1; redo; } print "reachable";"#;
        let diags = unreachable_diags(source);
        assert_eq!(
            count_pl406(&diags),
            0,
            "a later conditional last can exit after redo; the sibling must remain reachable: {diags:?}"
        );
    }

    /// The repair must not demote an unconditional `redo` merely because
    /// conditional loop-control transfers are now preserved.
    #[test]
    fn unconditional_redo_after_prefix_still_closes_following_sibling() {
        let source = r#"my $i = 0; { ++$i; redo; } print "unreachable";"#;
        let diags = unreachable_diags(source);
        assert_pl406_at(source, &diags, "print \"unreachable\"");
    }

    /// `next OUTER` inside a bare block targets the outer loop's next
    /// iteration, not the bare block itself. The bare block therefore does not
    /// fall through, and the sibling after it inside the loop body is
    /// unreachable.
    #[test]
    fn next_outer_label_in_bare_block_closes_loop_body_fallthrough() {
        let source = "OUTER: while (1) { { next OUTER; } print \"dead\"; }";
        let diags = unreachable_diags(source);
        assert_pl406_at(source, &diags, "print \"dead\"");
    }

    /// `last DONE` inside a labeled bare block `DONE: { … }` exits the block
    /// itself (the label matches). The parent still falls through — this is
    /// the same as unlabeled `last` in an anonymous bare block.
    #[test]
    fn last_matching_label_in_bare_block_keeps_parent_fallthrough() {
        let source = "DONE: { last DONE; } print \"after\";";
        let diags = unreachable_diags(source);
        assert_eq!(
            count_pl406(&diags),
            0,
            "last DONE exits the DONE block itself; no sibling should be PL406: {diags:?}"
        );
    }

    /// `next DONE` inside a labeled bare block `DONE: { … }` skips to the
    /// end of the block (there is no next iteration in a one-shot loop), so
    /// the parent falls through — same as unlabeled `next`.
    #[test]
    fn next_matching_label_in_bare_block_keeps_parent_fallthrough() {
        let source = "DONE: { next DONE; } print \"after\";";
        let diags = unreachable_diags(source);
        assert_eq!(
            count_pl406(&diags),
            0,
            "next DONE exits the DONE block itself; no sibling should be PL406: {diags:?}"
        );
    }
}
