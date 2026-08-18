//! Unreachable code detection (PL406)
//!
//! PL406 is a local control-flow diagnostic. It proves whether control can
//! reach the next sibling statement inside one execution unit; it does not
//! make workspace-wide symbol-liveness claims.
//!
//! The implementation uses typed flow summaries rather than a flat
//! `reachable: bool` walk. Each statement reports whether any path falls
//! through and, when it does not, which control-transfer classes were observed.
//! Complete `if`/`elsif`/`else` branches can therefore propagate
//! non-fallthrough to their parent statement list without allowing a transfer
//! inside a nested callable or evaluation scope to poison the outer list.

use super::super::internal_types::{Diagnostic, DiagnosticTag};
use perl_diagnostics::codes::DiagnosticCode;
use perl_diagnostics::codes::DiagnosticSeverity;
use perl_parser_core::ast::{GotoTargetForm, Node, NodeKind};

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

/// Whether a statement or block can reach its next sibling, plus the terminal
/// transfers observed when it cannot.
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

/// Walk the AST and emit PL406 diagnostics for statements that cannot be
/// reached from the preceding sibling in the same local statement list.
pub fn check_unreachable_code(root: &Node, diagnostics: &mut Vec<Diagnostic>) {
    let _ = summarize_node(root, diagnostics);
}

fn summarize_node(node: &Node, diagnostics: &mut Vec<Diagnostic>) -> FlowSummary {
    match &node.kind {
        NodeKind::Program { statements } | NodeKind::Block { statements } => {
            summarize_statement_list(statements, diagnostics)
        }

        // Callable declarations introduce a fresh execution unit. Analyze the
        // body, but the declaration itself falls through in its parent list.
        NodeKind::Subroutine { body, .. } | NodeKind::Method { body, .. } => {
            let _ = summarize_node(body, diagnostics);
            FlowSummary::falls_through()
        }

        NodeKind::If { condition, then_branch, elsif_branches, else_branch, .. } => summarize_if(
            condition,
            then_branch,
            elsif_branches,
            else_branch.as_deref(),
            diagnostics,
        ),

        // Loops are independent local scopes. Transfers inside their body or
        // continue block do not make the statement after the loop unreachable.
        NodeKind::While { condition, body, continue_block, .. } => {
            let _ = summarize_expression(condition, diagnostics);
            let _ = summarize_node(body, diagnostics);
            if let Some(continue_block) = continue_block {
                let _ = summarize_node(continue_block, diagnostics);
            }
            FlowSummary::falls_through()
        }
        NodeKind::For { init, condition, update, body, continue_block, .. } => {
            if let Some(init) = init {
                let _ = summarize_expression(init, diagnostics);
            }
            if let Some(condition) = condition {
                let _ = summarize_expression(condition, diagnostics);
            }
            if let Some(update) = update {
                let _ = summarize_expression(update, diagnostics);
            }
            let _ = summarize_node(body, diagnostics);
            if let Some(continue_block) = continue_block {
                let _ = summarize_node(continue_block, diagnostics);
            }
            FlowSummary::falls_through()
        }
        NodeKind::Foreach { variable, list, body, continue_block } => {
            let _ = summarize_expression(variable, diagnostics);
            let _ = summarize_expression(list, diagnostics);
            let _ = summarize_node(body, diagnostics);
            if let Some(continue_block) = continue_block {
                let _ = summarize_node(continue_block, diagnostics);
            }
            FlowSummary::falls_through()
        }

        // These constructs are analyzed locally, but their transfer summaries
        // are deliberately not promoted into the containing execution unit.
        NodeKind::Given { expr, body } => {
            let _ = summarize_expression(expr, diagnostics);
            let _ = summarize_node(body, diagnostics);
            FlowSummary::falls_through()
        }
        NodeKind::When { condition, body } => {
            let _ = summarize_expression(condition, diagnostics);
            let _ = summarize_node(body, diagnostics);
            FlowSummary::falls_through()
        }
        NodeKind::Default { body }
        | NodeKind::PhaseBlock { block: body, .. }
        | NodeKind::Class { body, .. } => {
            let _ = summarize_node(body, diagnostics);
            FlowSummary::falls_through()
        }
        NodeKind::Package { block, .. } => {
            if let Some(block) = block {
                let _ = summarize_node(block, diagnostics);
            }
            FlowSummary::falls_through()
        }
        NodeKind::Eval { block } | NodeKind::Do { block } | NodeKind::Defer { block } => {
            let _ = summarize_node(block, diagnostics);
            FlowSummary::falls_through()
        }
        NodeKind::Try { body, catch_blocks, finally_block } => {
            let _ = summarize_node(body, diagnostics);
            for (_, catch_body) in catch_blocks {
                let _ = summarize_node(catch_body, diagnostics);
            }
            if let Some(finally_block) = finally_block {
                let _ = summarize_node(finally_block, diagnostics);
            }
            FlowSummary::falls_through()
        }

        NodeKind::StatementModifier { statement, condition, .. } => {
            let _ = summarize_node(statement, diagnostics);
            let _ = summarize_expression(condition, diagnostics);
            // Without an accepted constant-value fact, a statement modifier
            // always retains a path that skips the controlled statement.
            FlowSummary::falls_through()
        }
        NodeKind::LabeledStatement { label, statement } => {
            let summary = summarize_node(statement, diagnostics);
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
                let _ = summarize_expression(value, diagnostics);
            }
            FlowSummary::transfer(ControlTransfer::Return)
        }
        NodeKind::LoopControl { op, label } => summarize_loop_control(op, label),
        NodeKind::Goto { target, form } => summarize_goto(target, form),

        NodeKind::ExpressionStatement { expression } => {
            summarize_expression(expression, diagnostics)
        }
        NodeKind::VariableDeclaration { initializer, .. }
        | NodeKind::VariableListDeclaration { initializer, .. } => {
            if let Some(initializer) = initializer {
                let _ = summarize_expression(initializer, diagnostics);
            }
            FlowSummary::falls_through()
        }

        // Recovered syntax is useful for nested diagnostics but cannot provide
        // exact parent non-fallthrough authority.
        NodeKind::Error { partial, .. } => {
            if let Some(partial) = partial {
                let _ = summarize_node(partial, diagnostics);
            }
            FlowSummary::falls_through()
        }

        NodeKind::FunctionCall { name, args } => summarize_function_call(name, args, diagnostics),
        NodeKind::AmperCall { args, .. } => {
            analyze_expression_list(args, diagnostics);
            FlowSummary::falls_through()
        }
        NodeKind::MethodCall { object, args, .. } | NodeKind::IndirectCall { object, args, .. } => {
            let _ = summarize_expression(object, diagnostics);
            analyze_expression_list(args, diagnostics);
            // Method/function spelling alone is not non-returning authority.
            FlowSummary::falls_through()
        }

        _ => summarize_expression(node, diagnostics),
    }
}

fn summarize_if(
    condition: &Node,
    then_branch: &Node,
    elsif_branches: &[(Box<Node>, Box<Node>)],
    else_branch: Option<&Node>,
    diagnostics: &mut Vec<Diagnostic>,
) -> FlowSummary {
    let _ = summarize_expression(condition, diagnostics);

    let mut branches = vec![summarize_node(then_branch, diagnostics)];
    for (elsif_condition, elsif_body) in elsif_branches {
        let _ = summarize_expression(elsif_condition, diagnostics);
        branches.push(summarize_node(elsif_body, diagnostics));
    }

    let exhaustive = else_branch.is_some();
    if let Some(else_branch) = else_branch {
        branches.push(summarize_node(else_branch, diagnostics));
    }

    FlowSummary::alternatives(branches, exhaustive)
}

fn summarize_statement_list(stmts: &[Node], diagnostics: &mut Vec<Diagnostic>) -> FlowSummary {
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
                pending_goto_labels.clear();
            } else {
                emit_unreachable(stmt, diagnostics);
                // Nested callables and blocks still deserve their own local
                // analysis even when the containing statement is unreachable.
                let _ = summarize_node(stmt, diagnostics);
                continue;
            }
        }

        let summary = summarize_node(stmt, diagnostics);
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
        if summary.can_fall_through {
            terminal_summary = FlowSummary::falls_through();
        } else {
            pending_goto_labels = summary.goto_labels();
            can_fall_through = false;
            terminal_summary = summary;
        }
    }

    if can_fall_through { FlowSummary::falls_through() } else { terminal_summary }
}

fn summarize_expression(node: &Node, diagnostics: &mut Vec<Diagnostic>) -> FlowSummary {
    match &node.kind {
        NodeKind::FunctionCall { name, args } => summarize_function_call(name, args, diagnostics),
        NodeKind::AmperCall { args, .. } => {
            analyze_expression_list(args, diagnostics);
            FlowSummary::falls_through()
        }
        NodeKind::MethodCall { object, args, .. } | NodeKind::IndirectCall { object, args, .. } => {
            let _ = summarize_expression(object, diagnostics);
            analyze_expression_list(args, diagnostics);
            FlowSummary::falls_through()
        }
        NodeKind::Subroutine { body, .. } | NodeKind::Method { body, .. } => {
            let _ = summarize_node(body, diagnostics);
            FlowSummary::falls_through()
        }
        NodeKind::Ternary { condition, then_expr, else_expr } => {
            let _ = summarize_expression(condition, diagnostics);
            FlowSummary::alternatives(
                vec![
                    summarize_expression(then_expr, diagnostics),
                    summarize_expression(else_expr, diagnostics),
                ],
                true,
            )
        }
        NodeKind::Assignment { lhs, rhs, .. } => {
            // Perl evaluates the rhs before the lhs. Visit both sides so a
            // transferring lhs cannot skip diagnostics nested in the rhs,
            // then select the terminal summary in evaluation order.
            let rhs_summary = summarize_expression(rhs, diagnostics);
            let lhs_summary = summarize_expression(lhs, diagnostics);
            if !rhs_summary.can_fall_through { rhs_summary } else { lhs_summary }
        }
        NodeKind::Unary { operand, .. } => summarize_expression(operand, diagnostics),
        NodeKind::Binary { left, right, .. } => {
            let _ = summarize_expression(left, diagnostics);
            let _ = summarize_expression(right, diagnostics);
            // Perl's short-circuit and overloaded binary operators require
            // stronger semantic facts before a child transfer can be promoted.
            FlowSummary::falls_through()
        }
        NodeKind::ArrayLiteral { elements } => {
            analyze_expression_list(elements, diagnostics);
            FlowSummary::falls_through()
        }
        NodeKind::HashLiteral { pairs } => {
            for (key, value) in pairs {
                let _ = summarize_expression(key, diagnostics);
                let _ = summarize_expression(value, diagnostics);
            }
            FlowSummary::falls_through()
        }
        NodeKind::Eval { block } | NodeKind::Do { block } | NodeKind::Defer { block } => {
            let _ = summarize_node(block, diagnostics);
            FlowSummary::falls_through()
        }
        _ => FlowSummary::falls_through(),
    }
}

fn summarize_function_call(
    name: &str,
    args: &[Node],
    diagnostics: &mut Vec<Diagnostic>,
) -> FlowSummary {
    analyze_expression_list(args, diagnostics);
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

fn analyze_expression_list(nodes: &[Node], diagnostics: &mut Vec<Diagnostic>) {
    for node in nodes {
        let _ = summarize_expression(node, diagnostics);
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

fn emit_unreachable(stmt: &Node, diagnostics: &mut Vec<Diagnostic>) {
    diagnostics.push(Diagnostic {
        range: (stmt.location.start, stmt.location.end),
        severity: DiagnosticSeverity::Hint,
        code: Some(DiagnosticCode::UnreachableCode.as_str().to_string()),
        message: "Unreachable code: this statement cannot be executed".to_string(),
        related_information: vec![],
        tags: vec![DiagnosticTag::Unnecessary],
        suggestion: Some("Remove unreachable code".to_string()),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_parser::Parser;
    use perl_tdd_support::{must, must_some};

    fn unreachable_diags(source: &str) -> Vec<Diagnostic> {
        let mut parser = Parser::new(source);
        let ast = must(parser.parse());
        assert!(
            parser.errors().is_empty(),
            "test source must parse cleanly; a recovered Error node falls through and \
             would make absence assertions vacuous: {:?}",
            parser.errors()
        );
        let mut diags = Vec::new();
        check_unreachable_code(&ast, &mut diags);
        diags
    }

    fn has_pl406(diags: &[Diagnostic]) -> bool {
        diags.iter().any(|diagnostic| diagnostic.code.as_deref() == Some("PL406"))
    }

    fn count_pl406(diags: &[Diagnostic]) -> usize {
        diags.iter().filter(|diagnostic| diagnostic.code.as_deref() == Some("PL406")).count()
    }

    #[test]
    fn return_then_statement_is_flagged() {
        let source = "sub f { return 1; my $x = 2; }";
        let diags = unreachable_diags(source);
        assert!(has_pl406(&diags), "statement after return should be PL406: {diags:?}");
        let anchor = "my $x = 2";
        let start = must_some(source.find(anchor));
        let pl406 = must_some(diags.iter().find(|d| d.code.as_deref() == Some("PL406")));
        assert_eq!(
            pl406.range,
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
    fn multiple_statements_after_transfer_are_flagged() {
        let diags = unreachable_diags("sub f { return; my $a = 1; my $b = 2; }");
        assert_eq!(count_pl406(&diags), 2, "all later siblings should be PL406: {diags:?}");
    }

    #[test]
    fn pl406_keeps_canonical_tag_and_suggestion() {
        let diags = unreachable_diags("sub f { return; my $x = 1; }");
        let diagnostic =
            must_some(diags.iter().find(|diagnostic| diagnostic.code.as_deref() == Some("PL406")));
        assert!(diagnostic.tags.contains(&DiagnosticTag::Unnecessary));
        assert!(diagnostic.suggestion.is_some());
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
        let diags = unreachable_diags("OUTER: while (1) { { last OUTER; } print \"dead\"; last; }");
        assert!(
            has_pl406(&diags),
            "last OUTER escapes the bare block; the sibling inside the loop body \
             must be PL406: {diags:?}"
        );
    }

    /// `redo LABEL` targeting a labeled bare block restarts the block from the
    /// beginning, creating an infinite loop. The statement after the labeled
    /// block is unreachable.
    #[test]
    fn redo_own_label_bare_block_closes_following_sibling() {
        let diags = unreachable_diags("R: { redo R; } print \"x\";");
        assert!(
            has_pl406(&diags),
            "redo R restarts the R block forever; the sibling after it must be PL406: {diags:?}"
        );
    }

    /// `redo` without a label in a bare block also creates an infinite loop
    /// (one-shot loops have no next iteration to `redo` into other than
    /// themselves). The following sibling is unreachable.
    #[test]
    fn redo_unlabeled_bare_block_closes_following_sibling() {
        let diags = unreachable_diags("{ redo; } print \"y\";");
        assert!(
            has_pl406(&diags),
            "redo in an unlabeled bare block is an infinite loop; the following \
             sibling must be PL406: {diags:?}"
        );
    }

    /// `next OUTER` inside a bare block targets the outer loop's next
    /// iteration, not the bare block itself. The bare block therefore does not
    /// fall through, and the sibling after it inside the loop body is
    /// unreachable.
    #[test]
    fn next_outer_label_in_bare_block_closes_loop_body_fallthrough() {
        let diags = unreachable_diags("OUTER: while (1) { { next OUTER; } print \"dead\"; last; }");
        assert!(
            has_pl406(&diags),
            "next OUTER escapes the bare block; the sibling inside the loop body \
             must be PL406: {diags:?}"
        );
    }

    /// `last DONE` inside a labeled bare block `DONE: { … }` exits the block
    /// itself (the label matches). The parent still falls through — this is
    /// the same as unlabeled `last` in an anonymous bare block.
    #[test]
    fn last_matching_label_in_bare_block_keeps_parent_fallthrough() {
        let diags = unreachable_diags("DONE: { last DONE; } print \"after\";");
        assert!(
            !has_pl406(&diags),
            "last DONE exits the DONE block itself; the sibling after it must \
             remain reachable: {diags:?}"
        );
    }

    /// `next DONE` inside a labeled bare block `DONE: { … }` skips to the
    /// end of the block (there is no next iteration in a one-shot loop), so
    /// the parent falls through — same as unlabeled `next`.
    #[test]
    fn next_matching_label_in_bare_block_keeps_parent_fallthrough() {
        let diags = unreachable_diags("DONE: { next DONE; } print \"after\";");
        assert!(
            !has_pl406(&diags),
            "next DONE exits the DONE block itself; the sibling after it must \
             remain reachable: {diags:?}"
        );
    }
}
