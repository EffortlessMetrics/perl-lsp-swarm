//! Unreachable code detection (PL406)
//!
//! Identifies statements that cannot execute because they follow an unconditional
//! control-flow exit (`return`, `die`, `exit`, `croak`, `confess`, `last`,
//! `next`, `redo`, `goto`).
//!
//! # Algorithm
//!
//! The lint uses **recursive statement-slice analysis** rather than a flat
//! pre-order AST walk. This is the only correct approach: a pre-order visitor
//! with a `reachable: bool` flag cannot distinguish "visiting a child of this
//! node" from "visiting the next sibling", so a `return` inside a nested
//! subroutine body would incorrectly poison sibling statements in the outer
//! scope.
//!
//! The correct algorithm:
//! 1. `check_unreachable_code` dispatches on the root, then calls `visit_node`
//!    for each statement in top-level lists.
//! 2. `check_statement_list` iterates a `&[Node]` linearly. When an
//!    unconditional exit is found, all subsequent siblings get a PL406
//!    diagnostic. Nested blocks are recursed into freshly.
//! 3. Subroutine and method bodies (`Subroutine`, `Method`) trigger a fresh
//!    call to `visit_node`, so a `return` in an inner sub never affects the
//!    outer statement list.
//! 4. `eval { }` blocks are intentionally **not** recursed into: `die` inside
//!    `eval { }` is caught and does not exit the outer scope.
//!
//! # Scope of detection
//!
//! | Unconditional exit | Detected? |
//! |--------------------|-----------|
//! | `return`           | Yes |
//! | `die "msg"`        | Yes (direct FunctionCall at statement level) |
//! | `exit $code`       | Yes |
//! | `croak "msg"`      | Yes |
//! | `Carp::croak "msg"` | Yes |
//! | `confess "msg"`    | Yes |
//! | `Carp::confess "msg"` | Yes |
//! | `last` in loop body | Yes |
//! | `next` in loop body | Yes |
//! | `redo` in loop body | Yes |
//! | `return if $cond`  | No (conditional via StatementModifier) |
//! | `die` inside `or`  | No (right operand of Binary, not a direct statement) |
//! | `die` inside `eval { }` | No (caught by eval) |

use super::super::internal_types::{Diagnostic, DiagnosticTag};
use perl_diagnostics::codes::DiagnosticCode;
use perl_diagnostics::codes::DiagnosticSeverity;
use perl_parser_core::ast::{GotoTargetForm, Node, NodeKind};

/// Entry point for unreachable code detection.
///
/// Walk the AST and emit `PL406` diagnostics for any statements that cannot
/// be reached due to a preceding unconditional control-flow exit.
pub fn check_unreachable_code(root: &Node, diagnostics: &mut Vec<Diagnostic>) {
    visit_node(root, diagnostics);
}

/// Dispatch on a single node: recurse into any block-like children using fresh
/// reachability state, and process statement lists as slices.
fn visit_node(node: &Node, diagnostics: &mut Vec<Diagnostic>) {
    match &node.kind {
        // Top-level program: walk all top-level statements as a slice
        NodeKind::Program { statements } => {
            check_statement_list(statements, diagnostics);
        }

        // Subroutine body: fresh reachability scope — return here does not
        // affect the outer statement list
        NodeKind::Subroutine { body, .. } | NodeKind::Method { body, .. } => {
            visit_node(body, diagnostics);
        }

        // Plain block: walk its statements as a slice
        NodeKind::Block { statements } => {
            check_statement_list(statements, diagnostics);
        }

        // If/unless: each branch is an independent scope
        NodeKind::If { then_branch, elsif_branches, else_branch, .. } => {
            visit_node(then_branch, diagnostics);
            for (_, branch_body) in elsif_branches {
                visit_node(branch_body, diagnostics);
            }
            if let Some(else_body) = else_branch {
                visit_node(else_body, diagnostics);
            }
        }

        // Loop bodies: each body is an independent scope.
        // For `While`, also check the `continue { }` block independently.
        // Loop-control statements in a continue block still terminate
        // fallthrough to later siblings in that continue statement list.
        NodeKind::While { body, continue_block, .. } => {
            visit_node(body, diagnostics);
            if let Some(cb) = continue_block {
                visit_continue_block(cb, diagnostics);
            }
        }
        NodeKind::For { body, continue_block, .. }
        | NodeKind::Foreach { body, continue_block, .. } => {
            visit_node(body, diagnostics);
            if let Some(cb) = continue_block {
                visit_continue_block(cb, diagnostics);
            }
        }

        // Given/when/default
        NodeKind::Given { body, .. } | NodeKind::When { body, .. } | NodeKind::Default { body } => {
            visit_node(body, diagnostics);
        }

        // PhaseBlock (BEGIN, END, etc.): walk its block
        NodeKind::PhaseBlock { block, .. } => {
            visit_node(block, diagnostics);
        }

        // Class body
        NodeKind::Class { body, .. } => {
            visit_node(body, diagnostics);
        }

        // Do block: fresh scope (do { ... })
        NodeKind::Do { block } | NodeKind::Defer { block } => {
            visit_node(block, diagnostics);
        }

        // Try body and catch blocks: each is an independent scope
        NodeKind::Try { body, catch_blocks, finally_block } => {
            visit_node(body, diagnostics);
            for (_, catch_body) in catch_blocks {
                visit_node(catch_body, diagnostics);
            }
            if let Some(finally) = finally_block {
                visit_node(finally, diagnostics);
            }
        }

        // ExpressionStatement: recurse into the expression to catch nested
        // subroutine literals (e.g., `my $f = sub { return 1; };`)
        NodeKind::ExpressionStatement { expression } => {
            visit_expr(expression, diagnostics);
        }

        // Variable declarations with initializers may contain anonymous subs
        NodeKind::VariableDeclaration { initializer: Some(init), .. }
        | NodeKind::VariableListDeclaration { initializer: Some(init), .. } => {
            visit_expr(init, diagnostics);
        }

        // Eval: intentionally NOT recursed into.
        // die inside eval { } is caught — the outer scope continues normally.
        NodeKind::Eval { .. } => {}

        // LabeledStatement: recurse into the inner statement
        NodeKind::LabeledStatement { statement, .. } => {
            visit_node(statement, diagnostics);
        }

        // All other nodes have no statement-list children
        _ => {}
    }
}

/// Recursively visit expression nodes looking for anonymous subroutine literals
/// (so that `return` inside an anonymous sub does not appear to be a direct
/// child of the outer statement list).
fn visit_expr(expr: &Node, diagnostics: &mut Vec<Diagnostic>) {
    match &expr.kind {
        // Anonymous sub literal: fresh reachability scope
        NodeKind::Subroutine { body, .. } => {
            visit_node(body, diagnostics);
        }

        // Walk children of common expression forms
        NodeKind::Assignment { lhs, rhs, .. } => {
            visit_expr(lhs, diagnostics);
            visit_expr(rhs, diagnostics);
        }
        NodeKind::Binary { left, right, .. } => {
            visit_expr(left, diagnostics);
            visit_expr(right, diagnostics);
        }
        NodeKind::Unary { operand, .. } => {
            visit_expr(operand, diagnostics);
        }
        NodeKind::Ternary { condition, then_expr, else_expr } => {
            visit_expr(condition, diagnostics);
            visit_expr(then_expr, diagnostics);
            visit_expr(else_expr, diagnostics);
        }
        NodeKind::FunctionCall { args, .. } | NodeKind::MethodCall { args, .. } => {
            for arg in args {
                visit_expr(arg, diagnostics);
            }
        }
        NodeKind::ArrayLiteral { elements } => {
            for elem in elements {
                visit_expr(elem, diagnostics);
            }
        }
        NodeKind::HashLiteral { pairs } => {
            for (key, val) in pairs {
                visit_expr(key, diagnostics);
                visit_expr(val, diagnostics);
            }
        }
        // Other expression forms don't contain sub literals; stop recursing
        _ => {}
    }
}

/// Walk a statement slice linearly. When an unconditional exit is found, emit
/// PL406 for all remaining siblings in the same slice.
///
/// The key correctness property: after calling `check_statement_list`, nested
/// blocks are always entered with a *fresh* call to `visit_node`, which starts
/// with `found_exit = false`. This prevents a `return` in an inner sub from
/// poisoning the outer statement list.
fn check_statement_list(stmts: &[Node], diagnostics: &mut Vec<Diagnostic>) {
    let mut found_exit = false;
    let mut pending_goto_label = None;

    for stmt in stmts {
        if found_exit {
            if pending_goto_label.is_some_and(|label| labeled_statement_name(stmt) == Some(label)) {
                // `goto LABEL` can resume at a later label in this same list.
                // That label and its following statements are reachable again.
                found_exit = false;
                pending_goto_label = None;
            } else {
                // Emit PL406 for this unreachable statement
                diagnostics.push(Diagnostic {
                    range: (stmt.location.start, stmt.location.end),
                    severity: DiagnosticSeverity::Hint,
                    code: Some(DiagnosticCode::UnreachableCode.as_str().to_string()),
                    message: "Unreachable code: this statement cannot be executed".to_string(),
                    related_information: vec![],
                    tags: vec![DiagnosticTag::Unnecessary],
                    suggestion: Some("Remove unreachable code".to_string()),
                });
                // Still recurse into the unreachable node: nested subs deserve
                // independent analysis even if their containing block is dead.
                visit_node(stmt, diagnostics);
                continue;
            }
        }

        // Recurse first (to handle nested subs), then check for exit.
        visit_node(stmt, diagnostics);
        if is_unconditional_exit(stmt) {
            found_exit = true;
            pending_goto_label = goto_label_target(stmt);
        }
    }
}

/// Returns true if this AST node represents an unconditional control-flow exit.
///
/// Only nodes that **directly exit** at the statement level qualify. The key
/// restriction is that `die` inside `or` (a binary expression) does NOT count
/// because the `or` branch is only taken when the left side is falsy — the
/// overall statement does not always exit.
///
/// `StatementModifier` is explicitly `false`: `return if $cond` is conditional.
fn is_unconditional_exit(node: &Node) -> bool {
    match &node.kind {
        // `return;` or `return $value;`
        NodeKind::Return { .. } => true,

        // Direct function call at statement level (not wrapped in ExpressionStatement)
        NodeKind::FunctionCall { name, .. } => is_exit_function(name),

        // Method calls that are known unconditional exits (#5062):
        // `$obj->throw`, `$obj->abort`, `$obj->die` — common in Exception::Class,
        // Throwable::Error, etc. These conventionally never return.
        NodeKind::MethodCall { method, .. } => is_exit_method(method),

        // `die "msg";` — the parser wraps bare function calls in ExpressionStatement
        NodeKind::ExpressionStatement { expression } => is_unconditional_exit(expression),

        // `last`, `next`, `redo` — exit the current loop iteration/block
        NodeKind::LoopControl { op, .. } => matches!(op.as_str(), "last" | "next" | "redo"),

        // All goto forms transfer control without returning to the next sibling.
        // `goto LABEL` may resume at a later labeled statement in this same
        // list; check_statement_list handles that target separately.
        NodeKind::Goto { .. } => true,

        // `return if $cond` is CONDITIONAL — StatementModifier is never an unconditional exit
        NodeKind::StatementModifier { .. } => false,

        _ => false,
    }
}

/// Returns true if the method name is conventionally a non-returning exit.
/// These are common in exception libraries (Exception::Class, Throwable,
/// Catalyst::Exception, Ouch, etc.) where the method throws and never returns. (#5062)
fn is_exit_method(method: &str) -> bool {
    matches!(method, "throw" | "abort" | "rethrow" | "fatal")
}

/// Returns true if the function name is one of the known unconditional-exit functions.
fn is_exit_function(name: &str) -> bool {
    // Strip CORE:: prefix for uniform coverage — handles CORE::die, CORE::exit,
    // CORE::exec, CORE::croak, CORE::confess without a parallel list.
    let bare = name.strip_prefix("CORE::").unwrap_or(name);
    matches!(bare, "die" | "exit" | "exec" | "croak" | "Carp::croak" | "confess" | "Carp::confess")
}

/// Visit a `continue { }` block with statement-list-local fallthrough semantics.
///
/// `next`, `last`, and `redo` transfer control away from the following sibling
/// in the continue block. Their eventual destinations differ, but none falls
/// through to the next statement in this list.
fn visit_continue_block(node: &Node, diagnostics: &mut Vec<Diagnostic>) {
    match &node.kind {
        NodeKind::Block { statements } => {
            check_continue_block_statement_list(statements, diagnostics);
        }
        _ => {
            // Non-block continue node: fall back to standard visit
            visit_node(node, diagnostics);
        }
    }
}

/// Walk a continue-block statement slice with continue-block exit semantics.
///
/// Loop-control destinations remain distinct, but all unconditional
/// `next`/`last`/`redo` forms terminate fallthrough to later siblings.
fn check_continue_block_statement_list(stmts: &[Node], diagnostics: &mut Vec<Diagnostic>) {
    let mut found_exit = false;
    let mut pending_goto_label = None;

    for stmt in stmts {
        if found_exit {
            if pending_goto_label.is_some_and(|label| labeled_statement_name(stmt) == Some(label)) {
                found_exit = false;
                pending_goto_label = None;
            } else {
                diagnostics.push(Diagnostic {
                    range: (stmt.location.start, stmt.location.end),
                    severity: DiagnosticSeverity::Hint,
                    code: Some(DiagnosticCode::UnreachableCode.as_str().to_string()),
                    message: "Unreachable code: this statement cannot be executed".to_string(),
                    related_information: vec![],
                    tags: vec![DiagnosticTag::Unnecessary],
                    suggestion: Some("Remove unreachable code".to_string()),
                });
                visit_node(stmt, diagnostics);
                continue;
            }
        }

        visit_node(stmt, diagnostics);
        if is_unconditional_exit_in_continue(stmt) {
            found_exit = true;
            pending_goto_label = goto_label_target(stmt);
        }
    }
}

fn goto_label_target(node: &Node) -> Option<&str> {
    let NodeKind::Goto { target, form: GotoTargetForm::Label } = &node.kind else {
        return None;
    };

    let NodeKind::Identifier { name } = &target.kind else {
        return None;
    };
    Some(name.as_str())
}

fn labeled_statement_name(node: &Node) -> Option<&str> {
    let NodeKind::LabeledStatement { label, .. } = &node.kind else {
        return None;
    };
    Some(label.as_str())
}

/// Returns true if this node terminates fallthrough within a continue block.
///
/// Loop-control destinations differ, but unconditional `next`, `last`, and
/// `redo` all prevent execution of the following sibling statement.
fn is_unconditional_exit_in_continue(node: &Node) -> bool {
    match &node.kind {
        NodeKind::Return { .. } => true,
        NodeKind::FunctionCall { name, .. } => is_exit_function(name),
        NodeKind::ExpressionStatement { expression } => {
            is_unconditional_exit_in_continue(expression)
        }
        NodeKind::LoopControl { op, .. } => matches!(op.as_str(), "last" | "next" | "redo"),
        NodeKind::Goto { .. } => true,
        NodeKind::StatementModifier { .. } => false,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_parser::Parser;
    use perl_tdd_support::{must, must_some};

    fn unreachable_diags(source: &str) -> Vec<Diagnostic> {
        let ast = must(Parser::new(source).parse());
        let mut diags = Vec::new();
        check_unreachable_code(&ast, &mut diags);
        diags
    }

    fn has_pl406(diags: &[Diagnostic]) -> bool {
        diags.iter().any(|d| d.code.as_deref() == Some("PL406"))
    }

    fn count_pl406(diags: &[Diagnostic]) -> usize {
        diags.iter().filter(|d| d.code.as_deref() == Some("PL406")).count()
    }

    // --- return as exit ---

    #[test]
    fn return_then_statement_is_flagged() {
        let diags = unreachable_diags("sub f { return 1; my $x = 2; }");
        assert!(has_pl406(&diags), "statement after return should be flagged as PL406: {diags:?}");
    }

    #[test]
    fn return_at_end_no_flag() {
        let diags = unreachable_diags("sub f { my $x = 1; return $x; }");
        assert!(!has_pl406(&diags), "return at end of sub should not flag anything: {diags:?}");
    }

    #[test]
    fn conditional_return_does_not_flag_subsequent_code() {
        let diags = unreachable_diags("sub f { return if $cond; my $x = 1; }");
        assert!(!has_pl406(&diags), "return if cond should not flag subsequent code: {diags:?}");
    }

    // --- die as exit ---

    #[test]
    fn die_then_statement_is_flagged() {
        let diags = unreachable_diags(r#"sub f { die "error"; print "never"; }"#);
        assert!(has_pl406(&diags), "statement after die should be flagged as PL406: {diags:?}");
    }

    #[test]
    fn croak_then_statement_is_flagged() {
        let diags = unreachable_diags(r#"sub f { croak "err"; print "never"; }"#);
        assert!(has_pl406(&diags), "statement after croak should be flagged as PL406: {diags:?}");
    }

    #[test]
    fn qualified_croak_then_statement_is_flagged() {
        let diags = unreachable_diags(r#"sub f { Carp::croak "err"; print "never"; }"#);
        assert!(
            has_pl406(&diags),
            "statement after Carp::croak should be flagged as PL406: {diags:?}"
        );
    }

    #[test]
    fn exit_then_statement_is_flagged() {
        let diags = unreachable_diags("exit 0; my $x = 1;");
        assert!(has_pl406(&diags), "statement after exit should be flagged as PL406: {diags:?}");
    }

    // --- multiple unreachable statements ---

    #[test]
    fn two_statements_after_return_both_flagged() {
        let diags = unreachable_diags("sub f { return; my $a = 1; my $b = 2; }");
        assert_eq!(
            count_pl406(&diags),
            2,
            "both statements after return should be PL406: {diags:?}"
        );
    }

    // --- nested sub reachability isolation ---

    #[test]
    fn return_in_inner_sub_does_not_poison_outer() {
        let diags = unreachable_diags("sub outer { my $f = sub { return 1; }; my $x = 2; }");
        assert!(
            !has_pl406(&diags),
            "return inside anonymous sub should not flag outer code: {diags:?}"
        );
    }

    #[test]
    fn return_in_inner_sub_flags_inner_unreachable() {
        let diags = unreachable_diags("sub outer { my $f = sub { return 1; my $dead = 99; }; }");
        assert!(
            has_pl406(&diags),
            "unreachable code inside anonymous sub should be flagged: {diags:?}"
        );
    }

    // --- loop control exits ---

    #[test]
    fn last_in_loop_body_flags_subsequent() {
        let diags = unreachable_diags("for my $i (1..5) { last; print $i; }");
        assert!(has_pl406(&diags), "code after 'last' should be flagged: {diags:?}");
    }

    #[test]
    fn next_in_loop_body_flags_subsequent() {
        let diags = unreachable_diags("for my $i (1..5) { next; print $i; }");
        assert!(has_pl406(&diags), "code after 'next' should be flagged: {diags:?}");
    }

    #[test]
    fn redo_in_loop_body_flags_subsequent() {
        let diags = unreachable_diags("while ($ready) { redo; print $ready; }");
        assert!(has_pl406(&diags), "code after 'redo' should be flagged: {diags:?}");
    }

    #[test]
    fn next_in_continue_block_flags_subsequent() {
        let diags =
            unreachable_diags("while ($ready) { work(); } continue { next; print $ready; }");
        assert!(
            has_pl406(&diags),
            "code after 'next' in continue should be flagged: {diags:?}"
        );
    }

    #[test]
    fn last_in_continue_block_flags_subsequent() {
        let diags =
            unreachable_diags("while ($ready) { work(); } continue { last; print $ready; }");
        assert!(
            has_pl406(&diags),
            "code after 'last' in continue should be flagged: {diags:?}"
        );
    }

    #[test]
    fn redo_in_continue_block_flags_subsequent() {
        let diags =
            unreachable_diags("while ($ready) { work(); } continue { redo; print $ready; }");
        assert!(
            has_pl406(&diags),
            "code after 'redo' in continue should be flagged: {diags:?}"
        );
    }

    #[test]
    fn labelled_next_in_continue_block_flags_subsequent() {
        let diags = unreachable_diags(
            "OUTER: while ($ready) { work(); } continue { next OUTER; print $ready; }",
        );
        assert!(
            has_pl406(&diags),
            "code after labelled next in continue should be flagged: {diags:?}"
        );
    }

    #[test]
    fn two_statements_after_next_in_continue_are_flagged() {
        let diags = unreachable_diags(
            "while ($ready) { work(); } continue { next; print 1; print 2; }",
        );
        assert_eq!(
            count_pl406(&diags),
            2,
            "all later continue-block siblings should be flagged: {diags:?}"
        );
    }

    #[test]
    fn postfix_loop_control_in_continue_keeps_fallthrough() {
        for source in [
            "while ($ready) { work(); } continue { next if $skip; print $ready; }",
            "while ($ready) { work(); } continue { last unless $ready; print $ready; }",
            "while ($ready) { work(); } continue { redo while $retry; print $ready; }",
        ] {
            let diags = unreachable_diags(source);
            assert!(
                !has_pl406(&diags),
                "conditional loop control should preserve continue-block fallthrough: {diags:?}"
            );
        }
    }

    #[test]
    fn loop_control_does_not_poison_code_after_loop() {
        for source in [
            "while ($ready) { next; } print 'after';",
            "while ($ready) { redo; } print 'after';",
            "while ($ready) { last; } print 'after';",
        ] {
            let diags = unreachable_diags(source);
            assert!(
                !has_pl406(&diags),
                "loop control should not make code after the loop unreachable: {diags:?}"
            );
        }
    }

    // --- eval block: die is caught, outer code is reachable ---

    #[test]
    fn die_inside_eval_does_not_flag_after_eval() {
        let diags = unreachable_diags(r#"eval { die "oops"; }; my $x = 1;"#);
        assert!(
            !has_pl406(&diags),
            "die inside eval should not flag code after the eval block: {diags:?}"
        );
    }

    // --- confess ---

    #[test]
    fn confess_then_statement_is_flagged() {
        let diags = unreachable_diags(r#"sub f { confess "msg"; print "dead"; }"#);
        assert!(has_pl406(&diags), "statement after confess should be flagged as PL406: {diags:?}");
    }

    #[test]
    fn qualified_confess_then_statement_is_flagged() {
        let diags = unreachable_diags(r#"sub f { Carp::confess "msg"; print "dead"; }"#);
        assert!(
            has_pl406(&diags),
            "statement after Carp::confess should be flagged as PL406: {diags:?}"
        );
    }

    // --- diagnostic quality ---

    #[test]
    fn pl406_has_unnecessary_tag() {
        let diags = unreachable_diags("sub f { return; my $x = 1; }");
        let diag = must_some(diags.iter().find(|d| d.code.as_deref() == Some("PL406")));
        assert!(
            diag.tags.contains(&DiagnosticTag::Unnecessary),
            "PL406 should carry the Unnecessary tag"
        );
    }

    #[test]
    fn pl406_has_suggestion() {
        let diags = unreachable_diags("sub f { return; my $x = 1; }");
        let diag = must_some(diags.iter().find(|d| d.code.as_deref() == Some("PL406")));
        assert!(diag.suggestion.is_some(), "PL406 should carry a suggestion");
    }

    #[test]
    fn clean_sub_no_pl406() {
        let diags = unreachable_diags("sub f { my $x = 1; my $y = 2; return $x + $y; }");
        assert!(!has_pl406(&diags), "clean sub should not trigger PL406: {diags:?}");
    }

    // --- exec / CORE::exit terminators (#5063) ---

    #[test]
    fn exec_then_statement_is_flagged() {
        let diags = unreachable_diags(r#"exec("perl", "-e", "1"); my $x = 1;"#);
        assert!(
            has_pl406(&diags),
            "statement after exec should be flagged as PL406 (exec never returns): {diags:?}"
        );
    }

    #[test]
    fn exec_paren_less_then_statement_is_flagged() {
        let diags = unreachable_diags(r#"exec "perl", "-e", "1"; my $x = 1;"#);
        assert!(
            has_pl406(&diags),
            "statement after paren-less exec should be flagged as PL406: {diags:?}"
        );
    }

    #[test]
    fn core_exit_then_statement_is_flagged() {
        let diags = unreachable_diags("CORE::exit(0); my $x = 1;");
        assert!(
            has_pl406(&diags),
            "statement after CORE::exit should be flagged as PL406: {diags:?}"
        );
    }

    #[test]
    fn core_exec_then_statement_is_flagged() {
        let diags = unreachable_diags(r#"CORE::exec("perl", "-e", "1"); my $x = 1;"#);
        assert!(
            has_pl406(&diags),
            "statement after CORE::exec should be flagged as PL406: {diags:?}"
        );
    }

    #[test]
    fn exec_or_die_does_not_flag_subsequent() {
        let diags = unreachable_diags(r#"exec("perl", "-e", "1") or die; my $x = 1;"#);
        assert!(
            !has_pl406(&diags),
            "exec() or die should NOT flag subsequent code (Binary or is not a terminator): {diags:?}"
        );
    }
}
