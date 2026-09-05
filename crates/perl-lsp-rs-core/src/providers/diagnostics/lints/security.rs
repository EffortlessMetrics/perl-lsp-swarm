//! Security-focused lint checks
//!
//! This module provides lint checks that detect common security anti-patterns
//! in Perl code. These are patterns that `perl -c` and PPI cannot catch because
//! they require AST-level analysis.
//!
//! # Diagnostic codes
//!
//! | Code | Severity | Description |
//! |------|----------|-------------|
//! | `security-two-arg-open` | Warning | `open(FH, ">file")` -- use 3-arg open for safety |
//! | `security-string-eval` | Warning | `eval "$string"` -- string eval is a security risk |
//! | `security-backtick-exec` | Warning | Backtick/qx command execution detected |
//! | `security-signal-handler` | Warning | Global `$SIG{__DIE__}` / `$SIG{__WARN__}` assignment |
//! | `PL603` | Warning | `system()` call executes shell commands |
//! | `PL604` | Warning | `exec()` call replaces the current process |
//! | `PL605` | Warning | Pipe-open executes shell commands |
//! | `PL606` | Warning | `readpipe()` executes shell commands (equivalent to qx//) |
//! | `PL607` | Warning | Interpolated/concatenated SQL text in `->prepare()` / `->do()` (#5035) |
//! | `PL608` | Warning | `s/pat/repl/e` evaluates the substitution replacement as Perl code (#9818) |
//! | `PL609` | Warning | Embedded `(?{ ... })` or `(??{ ... })` code executes inside regex patterns (#9818) |

use std::collections::HashMap;

use perl_diagnostics::codes::DiagnosticCode;
use perl_parser_core::ast::{Node, NodeKind};

use super::super::internal_types::{Diagnostic, RelatedInformation};
use crate::tooling::perl_critic::{
    BuiltInCriticObservation, Severity, is_backtick_string, is_qx_string,
};
use perl_diagnostics::codes::DiagnosticSeverity;

/// Check for security anti-patterns
///
/// This function walks the AST looking for:
/// - Two-argument `open` calls (should use 3-arg form)
/// - String `eval` (security risk vs. block `eval`)
/// - Backtick/qx command execution (ensure input is sanitized)
/// - Global signal-handler assignment to `$SIG{__DIE__}` / `$SIG{__WARN__}`
/// - Interpolated or concatenated SQL text passed to DBI statement-taking
///   methods (`prepare`/`prepare_cached`/`do`) (#5035)
/// - Substitutions evaluating their replacement as Perl code (`s///e`,
///   `s///ee`) and embedded immediate/deferred code blocks (`(?{ ... })`,
///   `(??{ ... })`) in regex patterns (`m//`, `qr//`, bare literals) (#9818)
pub fn check_security(node: &Node, diagnostics: &mut Vec<Diagnostic>) {
    walk_security_node(node, diagnostics, false);
    check_sql_injection(node, diagnostics);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SignalTableAccess {
    Bare,
    MainQualified,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SignalHandlerTarget {
    access: SignalTableAccess,
    signal_name: String,
}

fn walk_security_node(
    node: &Node,
    diagnostics: &mut Vec<Diagnostic>,
    signal_shadowed: bool,
) -> bool {
    match &node.kind {
        NodeKind::Program { statements } => {
            let mut current_shadowed = signal_shadowed;
            for stmt in statements {
                current_shadowed = walk_security_node(stmt, diagnostics, current_shadowed);
            }
            current_shadowed
        }
        NodeKind::Block { statements } => {
            let mut block_shadowed = signal_shadowed;
            for stmt in statements {
                block_shadowed = walk_security_node(stmt, diagnostics, block_shadowed);
            }
            signal_shadowed
        }
        NodeKind::ExpressionStatement { expression } => {
            walk_security_node(expression, diagnostics, signal_shadowed);
            signal_shadowed
        }
        NodeKind::Assignment { lhs, rhs, .. } => {
            check_global_signal_handler_assignment(lhs, node, diagnostics, signal_shadowed);
            walk_security_node(lhs, diagnostics, signal_shadowed);
            walk_security_node(rhs, diagnostics, signal_shadowed);
            signal_shadowed
        }
        NodeKind::VariableDeclaration { declarator, variable, initializer, .. } => {
            if let Some(init) = initializer {
                walk_security_node(init, diagnostics, signal_shadowed);
            }

            let mut updated_shadowed = signal_shadowed;
            if matches!(declarator.as_str(), "my" | "state") && shadows_signal_table(variable) {
                updated_shadowed = true;
            }

            if declarator != "local" {
                walk_security_node(variable, diagnostics, signal_shadowed);
            }
            updated_shadowed
        }
        NodeKind::VariableListDeclaration { declarator, variables, initializer, .. } => {
            if let Some(init) = initializer {
                walk_security_node(init, diagnostics, signal_shadowed);
            }

            if declarator != "local" {
                for variable in variables {
                    walk_security_node(variable, diagnostics, signal_shadowed);
                }
            }

            if matches!(declarator.as_str(), "my" | "state")
                && variables.iter().any(shadows_signal_table)
            {
                true
            } else {
                signal_shadowed
            }
        }
        NodeKind::NestedVariableList { items } => {
            // Recurse into nested variable list items for security analysis.
            for item in items {
                walk_security_node(item, diagnostics, signal_shadowed);
            }
            signal_shadowed
        }
        NodeKind::If { condition, then_branch, elsif_branches, else_branch, .. } => {
            walk_security_node(condition, diagnostics, signal_shadowed);
            walk_security_node(then_branch, diagnostics, signal_shadowed);
            for (condition, branch) in elsif_branches {
                walk_security_node(condition, diagnostics, signal_shadowed);
                walk_security_node(branch, diagnostics, signal_shadowed);
            }
            if let Some(branch) = else_branch {
                walk_security_node(branch, diagnostics, signal_shadowed);
            }
            signal_shadowed
        }
        NodeKind::While { condition, body, .. } => {
            walk_security_node(condition, diagnostics, signal_shadowed);
            walk_security_node(body, diagnostics, signal_shadowed);
            signal_shadowed
        }
        NodeKind::For { init, condition, update, body, continue_block } => {
            let mut loop_shadowed = signal_shadowed;
            if let Some(init) = init {
                loop_shadowed = walk_security_node(init, diagnostics, loop_shadowed);
            }
            if let Some(condition) = condition {
                walk_security_node(condition, diagnostics, loop_shadowed);
            }
            if let Some(update) = update {
                walk_security_node(update, diagnostics, loop_shadowed);
            }
            walk_security_node(body, diagnostics, loop_shadowed);
            if let Some(continue_block) = continue_block {
                walk_security_node(continue_block, diagnostics, loop_shadowed);
            }
            signal_shadowed
        }
        NodeKind::Foreach { variable, list, body, continue_block } => {
            let mut loop_shadowed = walk_security_node(variable, diagnostics, signal_shadowed);
            if shadows_signal_table(variable) {
                loop_shadowed = true;
            }
            walk_security_node(list, diagnostics, signal_shadowed);
            walk_security_node(body, diagnostics, loop_shadowed);
            if let Some(continue_block) = continue_block {
                walk_security_node(continue_block, diagnostics, loop_shadowed);
            }
            signal_shadowed
        }
        NodeKind::Given { expr, body } => {
            walk_security_node(expr, diagnostics, signal_shadowed);
            walk_security_node(body, diagnostics, signal_shadowed);
            signal_shadowed
        }
        NodeKind::When { condition, body } => {
            walk_security_node(condition, diagnostics, signal_shadowed);
            walk_security_node(body, diagnostics, signal_shadowed);
            signal_shadowed
        }
        NodeKind::Default { body } => {
            walk_security_node(body, diagnostics, signal_shadowed);
            signal_shadowed
        }
        NodeKind::StatementModifier { statement, condition, .. } => {
            walk_security_node(statement, diagnostics, signal_shadowed);
            walk_security_node(condition, diagnostics, signal_shadowed);
            signal_shadowed
        }
        NodeKind::Subroutine { signature, body, .. } => {
            let mut sub_shadowed = signal_shadowed;
            if let Some(signature) = signature {
                sub_shadowed = walk_security_node(signature, diagnostics, sub_shadowed);
            }
            walk_security_node(body, diagnostics, sub_shadowed);
            signal_shadowed
        }
        NodeKind::Method { signature, body, .. } => {
            let mut method_shadowed = signal_shadowed;
            if let Some(signature) = signature {
                method_shadowed = walk_security_node(signature, diagnostics, method_shadowed);
            }
            walk_security_node(body, diagnostics, method_shadowed);
            signal_shadowed
        }
        NodeKind::Signature { parameters } => {
            let mut signature_shadowed = signal_shadowed;
            for parameter in parameters {
                signature_shadowed = walk_security_node(parameter, diagnostics, signature_shadowed);
            }
            signature_shadowed
        }
        NodeKind::MandatoryParameter { variable }
        | NodeKind::SlurpyParameter { variable }
        | NodeKind::NamedParameter { variable, .. } => {
            let updated_shadowed =
                if shadows_signal_table(variable) { true } else { signal_shadowed };
            walk_security_node(variable, diagnostics, signal_shadowed);
            updated_shadowed
        }
        NodeKind::OptionalParameter { variable, default_value } => {
            walk_security_node(default_value, diagnostics, signal_shadowed);
            let updated_shadowed =
                if shadows_signal_table(variable) { true } else { signal_shadowed };
            walk_security_node(variable, diagnostics, signal_shadowed);
            updated_shadowed
        }
        NodeKind::Package { block: Some(block), .. }
        | NodeKind::PhaseBlock { block, .. }
        | NodeKind::Class { body: block, .. } => {
            walk_security_node(block, diagnostics, signal_shadowed);
            signal_shadowed
        }
        NodeKind::Try { body, catch_blocks, finally_block } => {
            walk_security_node(body, diagnostics, signal_shadowed);
            for (_, catch_body) in catch_blocks {
                walk_security_node(catch_body, diagnostics, signal_shadowed);
            }
            if let Some(finally_block) = finally_block {
                walk_security_node(finally_block, diagnostics, signal_shadowed);
            }
            signal_shadowed
        }
        NodeKind::Binary { left, right, .. } => {
            walk_security_node(left, diagnostics, signal_shadowed);
            walk_security_node(right, diagnostics, signal_shadowed);
            signal_shadowed
        }
        NodeKind::ArraySlice { target, indices } => {
            walk_security_node(target, diagnostics, signal_shadowed);
            walk_security_node(indices, diagnostics, signal_shadowed);
            signal_shadowed
        }
        NodeKind::HashSlice { target, keys } | NodeKind::KeyValueSlice { target, keys } => {
            walk_security_node(target, diagnostics, signal_shadowed);
            walk_security_node(keys, diagnostics, signal_shadowed);
            signal_shadowed
        }
        NodeKind::ChainedComparison { operands, .. } => {
            for operand in operands {
                walk_security_node(operand, diagnostics, signal_shadowed);
            }
            signal_shadowed
        }
        NodeKind::Ternary { condition, then_expr, else_expr } => {
            walk_security_node(condition, diagnostics, signal_shadowed);
            walk_security_node(then_expr, diagnostics, signal_shadowed);
            walk_security_node(else_expr, diagnostics, signal_shadowed);
            signal_shadowed
        }
        NodeKind::Unary { operand, .. } => {
            walk_security_node(operand, diagnostics, signal_shadowed);
            signal_shadowed
        }
        NodeKind::VariableWithAttributes { variable, .. } => {
            walk_security_node(variable, diagnostics, signal_shadowed);
            signal_shadowed
        }
        NodeKind::FunctionCall { name, args } | NodeKind::AmperCall { name, args } => {
            check_two_arg_open(name, args, node, diagnostics);
            check_string_eval(name, args, node, diagnostics);
            check_system_call(name, node, diagnostics);
            check_exec_call(name, node, diagnostics);
            check_pipe_open(name, args, node, diagnostics);
            check_readpipe(name, node, diagnostics);
            for arg in args {
                walk_security_node(arg, diagnostics, signal_shadowed);
            }
            signal_shadowed
        }
        NodeKind::IndirectCall { object, args, .. } | NodeKind::MethodCall { object, args, .. } => {
            walk_security_node(object, diagnostics, signal_shadowed);
            for arg in args {
                walk_security_node(arg, diagnostics, signal_shadowed);
            }
            signal_shadowed
        }
        NodeKind::Eval { block } => {
            check_eval_node(block, node, diagnostics);
            walk_security_node(block, diagnostics, signal_shadowed);
            signal_shadowed
        }
        NodeKind::Defer { block } => {
            walk_security_node(block, diagnostics, signal_shadowed);
            signal_shadowed
        }
        // Backtick strings: the parser stores `cmd` as
        // String { value: "`cmd`", interpolated: true }. The emitter declares
        // the reviewed PL601 backtick shape at this branch (#11918).
        NodeKind::String { value, interpolated: true } if is_backtick_string(value) => {
            push_command_execution_diagnostic(
                node,
                |severity, byte_range, message, explanation| {
                    BuiltInCriticObservation::pl601_backtick(
                        severity,
                        byte_range,
                        message,
                        explanation,
                    )
                },
                diagnostics,
            );
            signal_shadowed
        }
        // qx(cmd): the parser keeps the raw `qx(...)` spelling in the string
        // value. Same PL601 code, different reviewed shape — the emitter
        // chooses the exact shape at the syntax branch that observed it, so
        // a qx finding can only merge with the native qx alias, never with
        // the backtick alias (#11918).
        NodeKind::String { value, interpolated: true } if is_qx_string(value) => {
            push_command_execution_diagnostic(
                node,
                |severity, byte_range, message, explanation| {
                    BuiltInCriticObservation::pl601_qx(severity, byte_range, message, explanation)
                },
                diagnostics,
            );
            signal_shadowed
        }
        NodeKind::Return { value: Some(value) } => {
            walk_security_node(value, diagnostics, signal_shadowed);
            signal_shadowed
        }
        NodeKind::Regex { has_embedded_code: true, .. } => {
            // Code-executing regex family (#9818): the parser computes
            // `has_embedded_code` for immediate/deferred code blocks, but the flag had
            // no diagnostic consumer here. A bare regex literal has no bound
            // expression to traverse.
            push_embedded_pattern_code_diagnostic(node, diagnostics);
            signal_shadowed
        }
        NodeKind::Match { expr, has_embedded_code, .. } => {
            if *has_embedded_code {
                push_embedded_pattern_code_diagnostic(node, diagnostics);
            }
            // #9821: the expression bound via =~ receives the same checks it
            // would get in any other position.
            walk_security_node(expr, diagnostics, signal_shadowed)
        }
        NodeKind::Substitution { expr, modifiers, has_embedded_code, .. } => {
            // One parser flag conflates both execution causes (#975): when the
            // /e modifier is present the evaluated replacement names the
            // finding (PL608); a remaining flag can then only come from an
            // embedded (?{ ... }) pattern (PL609).
            if modifiers.contains('e') {
                push_substitution_eval_diagnostic(node, diagnostics);
            } else if *has_embedded_code {
                push_embedded_pattern_code_diagnostic(node, diagnostics);
            }
            // #9821: same traversal obligation as Match above.
            walk_security_node(expr, diagnostics, signal_shadowed)
        }
        NodeKind::Return { value: None } => signal_shadowed,
        NodeKind::LabeledStatement { statement, .. } => {
            walk_security_node(statement, diagnostics, signal_shadowed);
            signal_shadowed
        }
        NodeKind::Error { partial: Some(partial), .. } => {
            walk_security_node(partial, diagnostics, signal_shadowed);
            signal_shadowed
        }
        NodeKind::Heredoc { .. }
        | NodeKind::Tie { .. }
        | NodeKind::Untie { .. }
        | NodeKind::Format { .. } => signal_shadowed,
        NodeKind::Package { block: None, .. }
        | NodeKind::Use { .. }
        | NodeKind::No { .. }
        | NodeKind::DataSection { .. }
        | NodeKind::Number { .. }
        | NodeKind::String { .. }
        | NodeKind::VString { .. }
        | NodeKind::Transliteration { .. }
        | NodeKind::Identifier { .. }
        | NodeKind::Variable { .. }
        | NodeKind::Typeglob { .. }
        | NodeKind::Diamond
        | NodeKind::Ellipsis
        | NodeKind::Undef
        | NodeKind::Readline { .. }
        | NodeKind::Glob { .. }
        | NodeKind::ArrayLiteral { .. }
        | NodeKind::HashLiteral { .. }
        | NodeKind::Do { .. }
        | NodeKind::LoopControl { .. }
        | NodeKind::Goto { .. }
        | NodeKind::Prototype { .. }
        | NodeKind::MissingExpression
        | NodeKind::MissingStatement
        | NodeKind::MissingIdentifier
        | NodeKind::MissingBlock
        | NodeKind::Error { .. }
        | NodeKind::UnknownRest => signal_shadowed,
        _ => {
            let mut current_shadowed = signal_shadowed;
            node.for_each_child(|child| {
                current_shadowed = walk_security_node(child, diagnostics, current_shadowed);
            });
            current_shadowed
        }
    }
}

/// Detect a global assignment to `$SIG{__DIE__}` or `$SIG{__WARN__}`.
fn check_global_signal_handler_assignment(
    lhs: &Node,
    node: &Node,
    diagnostics: &mut Vec<Diagnostic>,
    signal_shadowed: bool,
) {
    let Some(signal_handler) = signal_handler_name(lhs) else {
        return;
    };

    if signal_handler.access == SignalTableAccess::Bare && signal_shadowed {
        return;
    }

    diagnostics.push(Diagnostic {
        range: (node.location.start, node.location.end),
        severity: DiagnosticSeverity::Warning,
        code: Some(DiagnosticCode::SecuritySignalHandler.as_str().to_string()),
        message: format!(
            "Global assignment to {}{{{}}} changes process-wide behavior. Use local $SIG{{...}} to scope the handler.",
            signal_table_display(&signal_handler.access),
            signal_handler.signal_name
        ),
        related_information: vec![RelatedInformation {
            location: (node.location.start, node.location.end),
            message: "Localized signal handlers avoid leaking exception or warning hooks across unrelated code.".to_string(),
        }],
        tags: Vec::new(),
        fixable: false,
        critic_observation: None,
        suggestion: Some(format!(
            "Use `local $SIG{{{}}} = ...` if the handler should be scoped",
            signal_handler.signal_name
        )),
    });
}

fn signal_table_display(access: &SignalTableAccess) -> &'static str {
    match access {
        SignalTableAccess::Bare => "$SIG",
        SignalTableAccess::MainQualified => "$main::SIG",
    }
}

/// Extract the signal-handler key if the node targets `$SIG{__DIE__}` or `$SIG{__WARN__}`.
fn signal_handler_name(node: &Node) -> Option<SignalHandlerTarget> {
    match &node.kind {
        NodeKind::Binary { op, left, right } if op == "{}" => {
            signal_handler_from_hash_and_key(left, right)
        }
        NodeKind::HashSlice { target, keys } | NodeKind::KeyValueSlice { target, keys } => {
            signal_handler_from_hash_and_key(target, keys)
        }
        _ => None,
    }
}

fn signal_handler_from_hash_and_key(
    hash_expr: &Node,
    key_expr: &Node,
) -> Option<SignalHandlerTarget> {
    let access = match &hash_expr.kind {
        NodeKind::Variable { sigil, name } if (sigil == "$" || sigil == "%") && name == "SIG" => {
            SignalTableAccess::Bare
        }
        NodeKind::Variable { sigil, name }
            if (sigil == "$" || sigil == "%") && (name == "main::SIG" || name == "::SIG") =>
        {
            SignalTableAccess::MainQualified
        }
        _ => return None,
    };

    signal_name_from_key(key_expr).map(|signal_name| SignalHandlerTarget { access, signal_name })
}

fn signal_name_from_key(node: &Node) -> Option<String> {
    match &node.kind {
        NodeKind::Identifier { name } if name == "__DIE__" || name == "__WARN__" => {
            Some(name.clone())
        }
        NodeKind::String { value, .. } => {
            let trimmed = value.trim_matches(['"', '\'']);
            if trimmed == "__DIE__" || trimmed == "__WARN__" {
                Some(trimmed.to_string())
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Detect string `eval` from `NodeKind::Eval` nodes.
///
/// The parser produces `Eval { block }` for both `eval { ... }` and
/// `eval "string"`. Block evals (`eval { ... }`) are safe exception handling;
/// string/variable evals are a security risk.
fn check_eval_node(block: &Node, eval_node: &Node, diagnostics: &mut Vec<Diagnostic>) {
    let is_string_eval = matches!(&block.kind, NodeKind::String { .. } | NodeKind::Variable { .. })
        || matches!(&block.kind, NodeKind::Binary { op, .. } if op == ".");

    if !is_string_eval {
        return;
    }

    diagnostics.push(Diagnostic {
        range: (eval_node.location.start, eval_node.location.end),
        severity: DiagnosticSeverity::Warning,
        code: Some(DiagnosticCode::SecurityStringEval.as_str().to_string()),
        message: "String eval is a security risk. Consider eval { } for exception handling."
            .to_string(),
        related_information: vec![RelatedInformation {
            location: (eval_node.location.start, eval_node.location.end),
            message: "String eval executes arbitrary Perl code at runtime. If the string contains user input, this allows code injection.".to_string(),
        }],
        tags: Vec::new(),
        fixable: false,
        critic_observation: None,
        suggestion: Some(
            "Use eval { } for exception handling, or consider safer alternatives like Try::Tiny"
                .to_string(),
        ),
    });
}

/// Detect two-argument `open` calls.
///
/// `open(FH, ">file")` is unsafe because the mode and filename are combined,
/// allowing shell injection if the filename comes from user input.
///
/// The parser may represent `open(FH, ">file")` args as either:
/// - Flat `args`: `[fh, mode_str]` (unit-test-constructed ASTs)
/// - Wrapped: `[ArrayLiteral { elements: [fh, mode_str] }]` (real parser output
///   for parenthesized calls) — this must be unwrapped or PL401 never fires
///   for the common `open(FH, MODE)` syntax. Mirrors `check_pipe_open` below.
fn check_two_arg_open(name: &str, args: &[Node], node: &Node, diagnostics: &mut Vec<Diagnostic>) {
    if name != "open" {
        return;
    }

    let effective_args: &[Node] = if args.len() == 1 {
        if let NodeKind::ArrayLiteral { elements } = &args[0].kind {
            elements.as_slice()
        } else {
            args
        }
    } else {
        args
    };

    if effective_args.len() != 2 {
        return;
    }

    diagnostics.push(Diagnostic {
        range: (node.location.start, node.location.end),
        severity: DiagnosticSeverity::Warning,
        code: Some(DiagnosticCode::TwoArgOpen.as_str().to_string()),
        message: "Use 3-argument open for safety: open(my $fh, '>', 'file')".to_string(),
        related_information: vec![RelatedInformation {
            location: (node.location.start, node.location.end),
            message: "Two-argument open combines mode and filename, which can allow shell injection if the filename is derived from user input".to_string(),
        }],
        tags: Vec::new(),
        fixable: false,
        critic_observation: None,
        suggestion: Some("Replace with 3-arg form: open(my $fh, '>', $file)".to_string()),
    });
}

/// Detect string `eval` calls.
///
/// `eval "code"` executes arbitrary Perl code at runtime, which is a security
/// risk when the string contains user input. Block eval (`eval { ... }`) is
/// safe and used for exception handling.
fn check_string_eval(name: &str, args: &[Node], node: &Node, diagnostics: &mut Vec<Diagnostic>) {
    if name != "eval" {
        return;
    }

    // Check that the first argument is a string (not a block/other expression).
    // eval { ... } is parsed as NodeKind::Eval, not FunctionCall, so reaching
    // here already means this is the function-call form. But we still check
    // the arg is a string to avoid false positives on eval($coderef).
    let is_string_arg = args.first().is_some_and(|arg| match &arg.kind {
        NodeKind::String { .. } | NodeKind::Variable { .. } => true,
        NodeKind::Binary { op, .. } if op == "." => true,
        _ => false,
    });

    if !is_string_arg && !args.is_empty() {
        return;
    }

    diagnostics.push(Diagnostic {
        range: (node.location.start, node.location.end),
        severity: DiagnosticSeverity::Warning,
        code: Some(DiagnosticCode::SecurityStringEval.as_str().to_string()),
        message: "String eval is a security risk. Consider eval { } for exception handling."
            .to_string(),
        related_information: vec![RelatedInformation {
            location: (node.location.start, node.location.end),
            message: "String eval executes arbitrary Perl code at runtime. If the string contains user input, this allows code injection.".to_string(),
        }],
        tags: Vec::new(),
        fixable: false,
        critic_observation: None,
        suggestion: Some(
            "Use eval { } for exception handling, or consider safer alternatives like Try::Tiny"
                .to_string(),
        ),
    });
}

/// Detect `system()` calls.
///
/// `system("cmd")` or `system("cmd", @args)` executes a shell command.
/// The list form `system($cmd, @args)` is safer (avoids shell injection),
/// but we flag all uses to prompt developers to consider the security context.
///
/// The emitter also declares the reviewed critic identity (`PL603`, system
/// shape) with its own critic-scale severity while it owns the proposition
/// (#11918): the ordinary diagnostic keeps its LSP severity; the observation
/// is what merges with `native.security.system_exec` in the normalized seam.
fn check_system_call(name: &str, node: &Node, diagnostics: &mut Vec<Diagnostic>) {
    if name != "system" {
        return;
    }

    let range = (node.location.start, node.location.end);
    let message = "system() executes a shell command. Ensure input is sanitized.".to_string();
    let explanation =
        "Use the list form system($cmd, @args) to avoid shell injection when arguments come from user input".to_string();
    const SYSTEM_LIST_FORM_SUGGESTION: &str = "Use the list form: system($cmd, @args) instead of system(\"$cmd @args\") to avoid shell injection";
    diagnostics.push(Diagnostic {
        range,
        severity: DiagnosticSeverity::Warning,
        code: Some(DiagnosticCode::SecuritySystemCall.as_str().to_string()),
        message: message.clone(),
        related_information: vec![RelatedInformation {
            location: range,
            message: explanation.clone(),
        }],
        tags: Vec::new(),
        fixable: false,
        critic_observation: Some(
            BuiltInCriticObservation::pl603_system(
                Severity::Harsh,
                range,
                message,
                Some(explanation.clone()),
            )
            // #12004: the observation carries the ordinary row's exact
            // user-visible remediation so retirement cannot drop it. The
            // binding keeps the two copies from drifting apart.
            .with_suggestion(SYSTEM_LIST_FORM_SUGGESTION)
            .with_related_information(range, explanation),
        ),
        suggestion: Some(SYSTEM_LIST_FORM_SUGGESTION.to_string()),
    });
}

/// Detect `exec()` calls.
///
/// `exec("cmd")` replaces the current process with a shell command.
/// The list form `exec($cmd, @args)` is safer (avoids shell injection),
/// but we flag all uses to prompt developers to consider the security context.
///
/// The emitter also declares the reviewed critic identity (`PL604`, exec
/// shape) with its own critic-scale severity (#11918).
fn check_exec_call(name: &str, node: &Node, diagnostics: &mut Vec<Diagnostic>) {
    if name != "exec" {
        return;
    }

    let range = (node.location.start, node.location.end);
    let message =
        "exec() replaces the current process with a shell command. Ensure input is sanitized."
            .to_string();
    let explanation =
        "Use the list form exec($cmd, @args) to avoid shell injection when arguments come from user input".to_string();
    const EXEC_LIST_FORM_SUGGESTION: &str = "Use the list form: exec($cmd, @args) instead of exec(\"$cmd @args\") to avoid shell injection";
    diagnostics.push(Diagnostic {
        range,
        severity: DiagnosticSeverity::Warning,
        code: Some(DiagnosticCode::SecurityExecCall.as_str().to_string()),
        message: message.clone(),
        related_information: vec![RelatedInformation {
            location: range,
            message: explanation.clone(),
        }],
        tags: Vec::new(),
        fixable: false,
        critic_observation: Some(
            BuiltInCriticObservation::pl604_exec(
                Severity::Harsh,
                range,
                message,
                Some(explanation.clone()),
            )
            .with_suggestion(EXEC_LIST_FORM_SUGGESTION)
            .with_related_information(range, explanation),
        ),
        suggestion: Some(EXEC_LIST_FORM_SUGGESTION.to_string()),
    });
}

/// Detect pipe-open patterns.
///
/// Both 2-arg `open(FH, "|cmd")` and 3-arg `open(FH, "|-", "cmd")` /
/// `open(FH, "-|", "cmd")` forms execute shell commands via pipes.
/// These are distinct from the two-arg-open security check (PL401),
/// which covers all 2-arg open calls regardless of pipe status.
///
/// The parser may represent `open(FH, "|cmd", ...)` args as either:
/// - Flat `args`: `[fh, mode_str, ...]` (unit-test-constructed ASTs)
/// - Wrapped: `[ArrayLiteral { elements: [fh, mode_str, ...] }]` (real parser output)
fn check_pipe_open(name: &str, args: &[Node], node: &Node, diagnostics: &mut Vec<Diagnostic>) {
    if name != "open" {
        return;
    }

    // Resolve the effective argument list: the parser may wrap args in a
    // single ArrayLiteral node (e.g. `open(FH, "|-", "cmd")` becomes
    // `FunctionCall { args: [ArrayLiteral { elements: [fh, "|-", "cmd"] }] }`).
    let effective_args: &[Node] = if args.len() == 1 {
        if let NodeKind::ArrayLiteral { elements } = &args[0].kind {
            elements.as_slice()
        } else {
            args
        }
    } else {
        args
    };

    let is_pipe = match effective_args.len() {
        // 3+ arg form: open(FH, "|-", "cmd") or open(FH, "-|", "cmd")
        n if n >= 3 => {
            let mode_node = &effective_args[1];
            is_pipe_mode_string(mode_node)
        }
        // 2-arg form: open(FH, "|cmd") — mode string starts with "|"
        2 => {
            let mode_node = &effective_args[1];
            is_pipe_two_arg_string(mode_node)
        }
        _ => false,
    };

    if !is_pipe {
        return;
    }

    diagnostics.push(Diagnostic {
        range: (node.location.start, node.location.end),
        severity: DiagnosticSeverity::Warning,
        code: Some(DiagnosticCode::SecurityPipeOpen.as_str().to_string()),
        message: "Pipe-open executes a shell command. Ensure input is sanitized.".to_string(),
        related_information: vec![RelatedInformation {
            location: (node.location.start, node.location.end),
            message: "Use the list form open(my $fh, '-|', $cmd, @args) to avoid shell injection when arguments come from user input".to_string(),
        }],
        tags: Vec::new(),
        fixable: false,
        critic_observation: None,
        suggestion: Some(
            "Use the list form: open(my $fh, '-|', $cmd, @args) for safer command execution"
                .to_string(),
        ),
    });
}

/// Returns true if the node is a string literal representing a pipe mode:
/// `"|-"` (write pipe) or `"-|"` (read pipe).
fn is_pipe_mode_string(node: &Node) -> bool {
    match &node.kind {
        NodeKind::String { value, .. } => {
            let trimmed = value.trim_matches(['"', '\'']);
            trimmed == "|-" || trimmed == "-|"
        }
        _ => false,
    }
}

/// Returns true if the node is a 2-arg open mode string that starts with `|`,
/// indicating a write-pipe: `"|cmd"`.
fn is_pipe_two_arg_string(node: &Node) -> bool {
    match &node.kind {
        NodeKind::String { value, .. } => {
            let trimmed = value.trim_matches(['"', '\'']);
            trimmed.starts_with('|')
        }
        _ => false,
    }
}

/// Shared remediation text for the command-execution family (PL601/PL606):
/// one binding keeps the ordinary diagnostic and the critic observation
/// from drifting apart (#12004).
const OPEN_LIST_FORM_SUGGESTION: &str =
    "Use open(my $fh, '-|', @cmd) or IPC::Run for safer command execution";

/// Detect `readpipe()` function calls.
///
/// `readpipe("cmd")` is functionally identical to backticks/qx//,
/// executing a shell command. Backtick strings are already caught via
/// the `NodeKind::String` branch (PL601); this check covers the explicit
/// function call form.
///
/// The emitter also declares the reviewed critic identity (`PL606`,
/// readpipe shape) with its own critic-scale severity (#11918).
fn check_readpipe(name: &str, node: &Node, diagnostics: &mut Vec<Diagnostic>) {
    if name != "readpipe" {
        return;
    }

    let range = (node.location.start, node.location.end);
    let message =
        "readpipe() executes a shell command (equivalent to qx//). Ensure input is sanitized."
            .to_string();
    let explanation =
        "Use open(my $fh, '-|', $cmd, @args) or IPC::Run for safer command execution with proper input validation".to_string();
    diagnostics.push(Diagnostic {
        range,
        severity: DiagnosticSeverity::Warning,
        code: Some(DiagnosticCode::SecurityReadpipe.as_str().to_string()),
        message: message.clone(),
        related_information: vec![RelatedInformation {
            location: range,
            message: explanation.clone(),
        }],
        tags: Vec::new(),
        fixable: false,
        critic_observation: Some(
            BuiltInCriticObservation::pl606_readpipe(
                Severity::Harsh,
                range,
                message,
                Some(explanation.clone()),
            )
            .with_suggestion(OPEN_LIST_FORM_SUGGESTION)
            .with_related_information(range, explanation),
        ),
        suggestion: Some(OPEN_LIST_FORM_SUGGESTION.to_string()),
    });
}

/// Emit one PL601 command-execution diagnostic for a backtick or `qx`
/// string form, declaring the exact reviewed shape through the supplied
/// observation constructor (#11918).
fn push_command_execution_diagnostic(
    node: &Node,
    observe: impl Fn(Severity, (usize, usize), String, Option<String>) -> BuiltInCriticObservation,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let range = (node.location.start, node.location.end);
    let message = "Command execution detected. Ensure input is sanitized.".to_string();
    let explanation =
        "Consider using open() with a pipe, or IPC::Run for safer command execution with proper input validation".to_string();
    diagnostics.push(Diagnostic {
        range,
        severity: DiagnosticSeverity::Warning,
        code: Some(DiagnosticCode::SecurityBacktickExec.as_str().to_string()),
        message: message.clone(),
        related_information: vec![RelatedInformation {
            location: range,
            message: explanation.clone(),
        }],
        tags: Vec::new(),
        fixable: false,
        critic_observation: Some(
            observe(Severity::Harsh, range, message, Some(explanation.clone()))
                .with_suggestion(OPEN_LIST_FORM_SUGGESTION)
                .with_related_information(range, explanation),
        ),
        suggestion: Some(OPEN_LIST_FORM_SUGGESTION.to_string()),
    });
}

/// Emit one PL608 diagnostic for a substitution whose replacement is
/// evaluated as Perl code by the `e`/`ee` modifier (#9818).
///
/// No native Perl::Critic alias exists for this construct class, so the
/// ordinary row carries no overlap observation: the #11918 observation
/// constructors only admit the reviewed cohorts.
fn push_substitution_eval_diagnostic(node: &Node, diagnostics: &mut Vec<Diagnostic>) {
    let range = (node.location.start, node.location.end);
    diagnostics.push(Diagnostic {
        range,
        severity: DiagnosticSeverity::Warning,
        code: Some(DiagnosticCode::SecuritySubstitutionEval.as_str().to_string()),
        message: "The /e flag evaluates this substitution's replacement as Perl code.".to_string(),
        related_information: vec![RelatedInformation {
            location: range,
            message: "When the substitution runs, its replacement is evaluated like string eval. Untrusted input reaching the replacement allows code injection.".to_string(),
        }],
        tags: Vec::new(),
        fixable: false,
        critic_observation: None,
        suggestion: Some(
            "Compute the replacement without /e, or keep untrusted input out of the evaluated expression"
                .to_string(),
        ),
    });
}

/// Emit one PL609 diagnostic for an embedded immediate `(?{ ... })` or
/// deferred `(??{ ... })` executable code block in a pattern (`m//`, `qr//`,
/// a bare literal, or a substitution pattern) (#9818).
///
/// Same no-native-alias boundary as [`push_substitution_eval_diagnostic`].
fn push_embedded_pattern_code_diagnostic(node: &Node, diagnostics: &mut Vec<Diagnostic>) {
    let range = (node.location.start, node.location.end);
    diagnostics.push(Diagnostic {
        range,
        severity: DiagnosticSeverity::Warning,
        code: Some(DiagnosticCode::SecurityEmbeddedRegexCode.as_str().to_string()),
        message: "Embedded (?{ ... }) or (??{ ... }) code executes Perl code while this pattern is evaluated."
            .to_string(),
        related_information: vec![RelatedInformation {
            location: range,
            message: "An embedded code block runs Perl code during pattern matching or deferred-pattern construction; untrusted patterns allow code injection.".to_string(),
        }],
        tags: Vec::new(),
        fixable: false,
        critic_observation: None,
        suggestion: Some("Remove the embedded executable code block from the pattern".to_string()),
    });
}

/// DBI statement-taking methods whose first argument is SQL text (#5035).
///
/// `execute(@bind_values)` is deliberately absent: its arguments are bind
/// values for an already-prepared statement, not SQL text, so it is never a
/// SQL-text sink (issue #5035 research, DBI 1.651 semantics).
const DBI_STATEMENT_METHODS: [&str; 3] = ["prepare", "prepare_cached", "do"];

/// Classification of the SQL statement argument at a DBI sink (#5035).
///
/// The producer only warns on proven dynamic assembly into the SQL text.
/// A computed statement (variable, call result) is a typed dynamic boundary:
/// the walker cannot distinguish safe from unsafe assembly, so it never
/// guesses and stays silent — mirroring the diagnostics family's
/// dynamic-boundary suppression precedent
/// (`providers/diagnostics/diagnostics_shadow.rs`: a reference inside a
/// dynamic boundary scope is suppressed, never guessed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SqlTextEvidence {
    /// A `$`/`@` sigil variable is interpolated directly into the SQL literal.
    Interpolated,
    /// A variable is concatenated into the SQL literal.
    Concatenated,
    /// Literal-only SQL (with or without `?` placeholders): proven safe shape.
    Static,
    /// The SQL text is computed and indistinguishable at AST level.
    DynamicBoundary,
}

/// Detect SQL assembled from variables and passed to a DBI statement-taking
/// method (#5035).
///
/// Honesty boundaries, each pinned by tests below:
/// - Only reviewed DBI statement-taking sinks (`prepare`, `prepare_cached`,
///   `do`) whose receiver carries same-file `DBI->connect(...)` evidence warn
///   (the receiver-classification precedent lives in
///   `collect_receiver_assignments`). A name qualifies only when every
///   same-file assignment before the sink comes from `DBI->connect`; a method
///   spelled `prepare` on an unproven, shadowed, rebound, or later-connected
///   receiver is a receiver-ambiguity boundary and stays silent — a security
///   warning never guesses DB-ness.
/// - Placeholders (`?`) with bind values are the negative control.
/// - A computed statement argument is a typed dynamic boundary and never
///   warns.
fn check_sql_injection(node: &Node, diagnostics: &mut Vec<Diagnostic>) {
    let mut index = ReceiverAssignmentIndex::new();
    collect_receiver_assignments(node, &mut index);
    let index = index.finish();
    walk_sql_injection_sinks(node, &index, diagnostics);
}

/// One same-file scalar assignment observed for a receiver name (#5035).
struct ReceiverAssignment {
    /// Byte offset of the assignment site, for source-order qualification.
    offset: usize,
    /// Whether the assigned value is a `DBI->connect(...)` call.
    is_connect: bool,
}

/// Collect same-file scalar assignments per receiver name.
///
/// This is the AST-provable form of the repository's DBI receiver-classification
/// precedent (`providers/completion/completion/methods.rs`,
/// `infer_receiver_type`: "check if variable was assigned from DBI->connect"):
/// the canonical `my $dbh = DBI->connect(...)` (or plain assignment) idiom.
/// Completion hints may fall back to name heuristics (`$dbh`), but a security
/// warning may not guess, so qualification is structural: a name whose
/// pre-sink assignments are not all `DBI->connect` calls (shadowed inner
/// `my $dbh = Engine->new`, rebinding, connect introduced after the sink) is
/// unproven and stays silent. Handles reached through aliases, parameters, or
/// DBD-specific class names stay unproven likewise; the binding-precise
/// statement-handle identity model is owned by #7471.
fn collect_receiver_assignments(node: &Node, index: &mut ReceiverAssignmentIndex) {
    let connect_assigned = |value: &Node| match &value.kind {
        NodeKind::MethodCall { object, method, .. } => {
            matches!(&object.kind, NodeKind::Identifier { name } if name == "DBI")
                && method == "connect"
        }
        _ => false,
    };

    match &node.kind {
        // Declarations without an initializer are neutral: they carry no
        // evidence about the receiver's origin either way.
        NodeKind::VariableDeclaration { variable, initializer: Some(init), .. } => {
            if let Some(name) = scalar_variable_name(variable) {
                index.record(name, node.location.start, connect_assigned(init));
            }
        }
        NodeKind::Assignment { lhs, rhs, .. } => {
            if let Some(name) = scalar_variable_name(lhs) {
                index.record(name, node.location.start, connect_assigned(rhs));
            }
        }
        _ => {}
    }

    node.for_each_child(|child| collect_receiver_assignments(child, index));
}

/// Pre-indexed receiver-assignment evidence, keyed by receiver name (#5035).
///
/// One pass over the document builds this index; every SQL sink then resolves
/// its receiver in O(1) by name plus a source-ordered prefix scan of only that
/// receiver's assignments. A document with A assignments and S sinks costs
/// O(A + S log A) per diagnostic pass instead of O(A x S), so a crafted
/// document cannot multiply sinks against assignments to exhaust CPU through
/// an open-document diagnostic request (#5035 review).
struct ReceiverAssignmentIndex {
    by_name: HashMap<String, Vec<ReceiverAssignment>>,
}

impl ReceiverAssignmentIndex {
    fn new() -> Self {
        Self { by_name: HashMap::new() }
    }

    fn record(&mut self, name: String, offset: usize, is_connect: bool) {
        self.by_name.entry(name).or_default().push(ReceiverAssignment { offset, is_connect });
    }

    /// Freeze the index after collection, putting every receiver's
    /// assignments in source order. Pre-order AST traversal is source-ordered
    /// in practice; sorting each bucket by offset makes that a guarantee
    /// instead of an assumption, so the prefix-qualification semantics below
    /// stay identical to the file-wide scan it replaces.
    fn finish(mut self) -> Self {
        for bucket in self.by_name.values_mut() {
            bucket.sort_by_key(|assignment| assignment.offset);
        }
        self
    }

    /// Whether `name` is a proven DBI handle at a sink starting at
    /// `sink_offset` (#5035): at least one same-file assignment before the
    /// sink must exist, and every such assignment must come from
    /// `DBI->connect(...)`. Assignments after the sink cannot describe the
    /// receiver at call time under name-based analysis; mixed pre-sink
    /// evidence is an ambiguity boundary.
    fn is_proven_dbh(&self, name: &str, sink_offset: usize) -> bool {
        let Some(bucket) = self.by_name.get(name) else {
            return false;
        };
        let prior = bucket.partition_point(|assignment| assignment.offset < sink_offset);
        prior > 0 && bucket[..prior].iter().all(|assignment| assignment.is_connect)
    }
}

/// The bare name of a scalar variable node (`$dbh` -> `dbh`), if this node is
/// one.
fn scalar_variable_name(node: &Node) -> Option<String> {
    match &node.kind {
        NodeKind::Variable { sigil, name } if sigil == "$" => Some(name.clone()),
        _ => None,
    }
}

fn walk_sql_injection_sinks(
    node: &Node,
    index: &ReceiverAssignmentIndex,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let NodeKind::MethodCall { object, method, args } = &node.kind {
        check_sql_injection_method_call(object, method, args, node, index, diagnostics);
    }
    node.for_each_child(|child| walk_sql_injection_sinks(child, index, diagnostics));
}

/// Check one method call against the DBI SQL-injection sink set.
fn check_sql_injection_method_call(
    object: &Node,
    method: &str,
    args: &[Node],
    node: &Node,
    index: &ReceiverAssignmentIndex,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if !DBI_STATEMENT_METHODS.contains(&method) {
        return;
    }

    // Receiver ambiguity boundary: only a receiver whose same-file pre-sink
    // assignments all come from `DBI->connect` is a proven DBI handle.
    // Anything else (unassigned `$dbh`, shadowed or rebound names, another
    // class's `prepare`, a later connect) stays silent.
    let receiver_is_dbh = scalar_variable_name(object)
        .is_some_and(|name| index.is_proven_dbh(&name, node.location.start));
    if !receiver_is_dbh {
        return;
    }

    let Some(statement_arg) = sql_statement_argument(args) else {
        return;
    };

    match classify_sql_text(statement_arg) {
        SqlTextEvidence::Interpolated | SqlTextEvidence::Concatenated => {}
        SqlTextEvidence::Static | SqlTextEvidence::DynamicBoundary => return,
    }

    let range = (node.location.start, node.location.end);
    let message = format!(
        "Interpolated SQL passed to ->{method}() is a SQL injection risk. Use placeholders (?) and bind values."
    );
    let explanation = "Values interpolated or concatenated into the SQL text can change the statement's meaning when input is crafted. Placeholders with bind values keep the SQL text static.".to_string();
    diagnostics.push(Diagnostic {
        range,
        severity: DiagnosticSeverity::Warning,
        code: Some(DiagnosticCode::SecuritySqlInjection.as_str().to_string()),
        message: message.clone(),
        related_information: vec![RelatedInformation {
            location: range,
            message: explanation.clone(),
        }],
        tags: Vec::new(),
        fixable: false,
        critic_observation: None,
        suggestion: Some(
            "Use placeholders: $dbh->prepare('... WHERE id = ?')->execute($user_id)".to_string(),
        ),
    });
}

/// The SQL statement argument of a DBI sink call.
///
/// Mirrors `check_two_arg_open`/`check_pipe_open`: the parser may represent
/// parenthesized call args as a flat `args` list or as a single wrapped
/// `ArrayLiteral`, so both shapes resolve to the effective argument list.
fn sql_statement_argument(args: &[Node]) -> Option<&Node> {
    let effective_args: &[Node] = if args.len() == 1 {
        if let NodeKind::ArrayLiteral { elements } = &args[0].kind {
            elements.as_slice()
        } else {
            args
        }
    } else {
        args
    };
    effective_args.first()
}

/// Classify the SQL text expression of a DBI sink call.
fn classify_sql_text(node: &Node) -> SqlTextEvidence {
    match &node.kind {
        NodeKind::String { value, interpolated } => {
            if *interpolated && string_contains_interpolation(value) {
                SqlTextEvidence::Interpolated
            } else {
                // Single-quoted literal, or a double-quoted literal with no
                // sigil in the text: static SQL, placeholders included.
                SqlTextEvidence::Static
            }
        }
        // Heredocs are string literals: an interpolating heredoc (`<<SQL`,
        // `<<"SQL"`) whose body contains a sigil is source-proven assembly,
        // exactly like a double-quoted string; a literal heredoc
        // (`<<'SQL'`) is static (#5035 review).
        NodeKind::Heredoc { content, interpolated, .. } => {
            if *interpolated && string_contains_interpolation(content) {
                SqlTextEvidence::Interpolated
            } else {
                SqlTextEvidence::Static
            }
        }
        NodeKind::Binary { op, .. } if op == "." => classify_concatenation(node),
        // A bare variable, call result, or any other expression: the SQL text
        // is computed and indistinguishable at AST level.
        _ => SqlTextEvidence::DynamicBoundary,
    }
}

/// Classify a `.` concatenation chain by combining operand evidence: any
/// unsafe operand makes the chain unsafe; otherwise any computed operand
/// makes the whole statement a dynamic boundary; literal-only chains are
/// static.
fn classify_concatenation(node: &Node) -> SqlTextEvidence {
    let NodeKind::Binary { left, right, .. } = &node.kind else {
        return SqlTextEvidence::DynamicBoundary;
    };

    let mut combined = SqlTextEvidence::Static;
    for operand in [left, right] {
        let evidence = match &operand.kind {
            // Nested `.` chain: fold recursively.
            NodeKind::Binary { op, .. } if op == "." => classify_concatenation(operand),
            // A variable operand is a proven dynamic value in the SQL text.
            NodeKind::Variable { .. } => SqlTextEvidence::Concatenated,
            // String and heredoc operands reuse the interpolation classifier.
            NodeKind::String { .. } | NodeKind::Heredoc { .. } => classify_sql_text(operand),
            // Any other operand (call, method, conditional) computes text we
            // cannot distinguish.
            _ => SqlTextEvidence::DynamicBoundary,
        };
        combined = match (combined, evidence) {
            // A proven variable in the SQL text dominates: the injection
            // vector is source-proven even when another operand is computed.
            (SqlTextEvidence::Interpolated, _) | (_, SqlTextEvidence::Interpolated) => {
                SqlTextEvidence::Interpolated
            }
            (SqlTextEvidence::Concatenated, _) | (_, SqlTextEvidence::Concatenated) => {
                SqlTextEvidence::Concatenated
            }
            (SqlTextEvidence::DynamicBoundary, _) | (_, SqlTextEvidence::DynamicBoundary) => {
                SqlTextEvidence::DynamicBoundary
            }
            (combined, _) => combined,
        };
    }
    combined
}

/// Whether an interpolating string's text interpolates a variable: a `$` or
/// `@` sigil followed by an identifier character, `{`, or — under the scalar
/// sigil only — one of the punctuation match-variable sigils, and not
/// escaped.
///
/// Escaping follows Perl backslash parity: only an odd-length run of
/// preceding backslashes escapes the sigil. An even-length run escapes the
/// backslash itself, so the sigil still interpolates (`"\\$id"` interpolates
/// `$id` after emitting a literal backslash) (#5035 review).
///
/// The punctuation successors `$&` (match text), `` $` `` (pre-match), `$'`
/// (post-match), and `$+` (highest capture group) name the special match
/// variables: like any scalar they interpolate in double-quoted strings and
/// interpolating heredocs, so a match over attacker-controlled input feeds
/// that text into the SQL exactly like `$id` and must not classify the
/// statement as static (#5035 review).
fn string_contains_interpolation(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.iter().enumerate().any(|(index, byte)| {
        let sigil = match *byte {
            b'$' | b'@' => *byte,
            _ => return false,
        };
        if escaped_by_backslash_run(bytes, index) {
            return false;
        }
        bytes.get(index + 1).is_some_and(|next| is_interpolation_successor(sigil, *next))
    })
}

/// Whether the byte after a `$`/`@` sigil starts an interpolation: an
/// identifier character, a block `${...}`/`@{...}` opener, or — for the
/// scalar sigil only — one of the punctuation match variables
/// (`$&`, `` $` ``, `$'`, `$+`) (#5035 review).
fn is_interpolation_successor(sigil: u8, next: u8) -> bool {
    next.is_ascii_alphanumeric()
        || next == b'_'
        || next == b'{'
        || (sigil == b'$' && matches!(next, b'&' | b'`' | b'\'' | b'+'))
}

/// Whether the byte at `index` is escaped by an odd-length run of contiguous
/// preceding backslashes.
fn escaped_by_backslash_run(bytes: &[u8], index: usize) -> bool {
    let mut run = 0;
    let mut cursor = index;
    while cursor > 0 && bytes[cursor - 1] == b'\\' {
        run += 1;
        cursor -= 1;
    }
    run % 2 == 1
}

fn shadows_signal_table(node: &Node) -> bool {
    match &node.kind {
        NodeKind::Variable { sigil, name } => sigil == "%" && name == "SIG",
        NodeKind::VariableWithAttributes { variable, .. } => shadows_signal_table(variable),
        NodeKind::VariableDeclaration { declarator, variable, .. } => {
            matches!(declarator.as_str(), "my" | "state") && shadows_signal_table(variable)
        }
        NodeKind::VariableListDeclaration { declarator, variables, .. } => {
            matches!(declarator.as_str(), "my" | "state")
                && variables.iter().any(shadows_signal_table)
        }
        NodeKind::MandatoryParameter { variable }
        | NodeKind::SlurpyParameter { variable }
        | NodeKind::NamedParameter { variable, .. } => shadows_signal_table(variable),
        NodeKind::OptionalParameter { variable, .. } => shadows_signal_table(variable),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_parser::Parser;
    use perl_tdd_support::{must, must_some, must_some_with};

    fn security_diags(source: &str) -> Vec<Diagnostic> {
        let ast = must(Parser::new(source).parse());
        let mut diags = vec![];
        check_security(&ast, &mut diags);
        diags
    }

    #[test]
    fn global_sig_warn_handler_is_flagged() {
        let diags = security_diags("$SIG{__WARN__} = sub { };");
        assert!(
            diags.iter().any(|d| d.code.as_deref() == Some("PL602")),
            "global __WARN__ handler should be flagged: {diags:?}"
        );
    }

    #[test]
    fn quoted_global_sig_warn_handler_is_flagged() {
        let diags = security_diags("$SIG{'__WARN__'} = sub { };");
        assert!(
            diags.iter().any(|d| d.code.as_deref() == Some("PL602")),
            "quoted __WARN__ handler should be flagged: {diags:?}"
        );
    }

    #[test]
    fn global_sig_die_handler_is_flagged() {
        let diags = security_diags("%SIG{__DIE__} = sub { };");
        assert!(
            diags.iter().any(|d| d.code.as_deref() == Some("PL602")),
            "global __DIE__ handler should be flagged: {diags:?}"
        );
    }

    #[test]
    fn main_qualified_sig_warn_handler_is_flagged() {
        let diags = security_diags("$main::SIG{__WARN__} = sub { };");
        assert!(
            diags.iter().any(|d| d.code.as_deref() == Some("PL602")),
            "main-qualified __WARN__ handler should be flagged: {diags:?}"
        );
    }

    #[test]
    fn lexical_sig_shadow_is_not_flagged() {
        let diags = security_diags("my %SIG; $SIG{__WARN__} = sub { };");
        assert!(
            diags.iter().all(|d| d.code.as_deref() != Some("PL602")),
            "lexical %SIG shadow should not be flagged: {diags:?}"
        );
    }

    #[test]
    fn local_sig_handlers_are_not_flagged() {
        let warn_diags = security_diags("local $SIG{__WARN__} = sub { };");
        let die_diags = security_diags("local $SIG{__DIE__} = sub { };");

        assert!(
            warn_diags.iter().all(|d| d.code.as_deref() != Some("PL602")),
            "localized __WARN__ handler should not be flagged: {warn_diags:?}"
        );
        assert!(
            die_diags.iter().all(|d| d.code.as_deref() != Some("PL602")),
            "localized __DIE__ handler should not be flagged: {die_diags:?}"
        );
    }

    // --- system() tests ---

    #[test]
    fn system_call_is_flagged() {
        let diags = security_diags(r#"system("ls -la");"#);
        assert!(
            diags.iter().any(|d| d.code.as_deref() == Some("PL603")),
            "system() should be flagged as PL603: {diags:?}"
        );
    }

    #[test]
    fn system_call_list_form_is_flagged() {
        let diags = security_diags(r#"system("ls", "-la");"#);
        assert!(
            diags.iter().any(|d| d.code.as_deref() == Some("PL603")),
            "system() list form should be flagged as PL603: {diags:?}"
        );
    }

    // --- exec() tests ---

    #[test]
    fn exec_call_is_flagged() {
        let diags = security_diags(r#"exec("ls -la");"#);
        assert!(
            diags.iter().any(|d| d.code.as_deref() == Some("PL604")),
            "exec() should be flagged as PL604: {diags:?}"
        );
    }

    #[test]
    fn exec_call_list_form_is_flagged() {
        let diags = security_diags(r#"exec("ls", "-la");"#);
        assert!(
            diags.iter().any(|d| d.code.as_deref() == Some("PL604")),
            "exec() list form should be flagged as PL604: {diags:?}"
        );
    }

    // --- pipe-open tests ---

    #[test]
    fn pipe_write_open_is_flagged() {
        // open(my $fh, "|-", "cmd") — write pipe
        let diags = security_diags(r#"open(my $fh, "|-", "ls");"#);
        assert!(
            diags.iter().any(|d| d.code.as_deref() == Some("PL605")),
            "write pipe-open should be flagged as PL605: {diags:?}"
        );
    }

    #[test]
    fn pipe_read_open_is_flagged() {
        // open(my $fh, "-|", "cmd") — read pipe
        let diags = security_diags(r#"open(my $fh, "-|", "ls");"#);
        assert!(
            diags.iter().any(|d| d.code.as_deref() == Some("PL605")),
            "read pipe-open should be flagged as PL605: {diags:?}"
        );
    }

    #[test]
    fn two_arg_pipe_open_is_flagged_as_pipe_open() {
        // open(FH, "|cmd") — 2-arg pipe-open (also a pipe, covered by PL605 not just PL401)
        let diags = security_diags(r#"open(FH, "|cmd");"#);
        assert!(
            diags.iter().any(|d| d.code.as_deref() == Some("PL605")),
            "two-arg pipe-open should be flagged as PL605: {diags:?}"
        );
    }

    #[test]
    fn normal_three_arg_open_is_not_pipe_flagged() {
        // open(my $fh, "<", "file") — safe, not a pipe
        let diags = security_diags(r#"open(my $fh, "<", "file.txt");"#);
        assert!(
            diags.iter().all(|d| d.code.as_deref() != Some("PL605")),
            "normal 3-arg open should not be flagged as PL605: {diags:?}"
        );
    }

    // --- two-arg open (PL401) tests ---

    #[test]
    fn parenthesized_two_arg_open_is_flagged() {
        // open(FH, ">file") — real parser wraps parenthesized call args in a
        // single ArrayLiteral node, so effective_args must be unwrapped for
        // PL401 to fire on this (the common) syntax.
        let diags = security_diags(r#"open(FH, ">file");"#);
        assert!(
            diags.iter().any(|d| d.code.as_deref() == Some("PL401")),
            "parenthesized 2-arg open should be flagged as PL401: {diags:?}"
        );
    }

    #[test]
    fn parenthesized_two_arg_open_with_lexical_fh_is_flagged() {
        // open(my $fh, $path) — lexical filehandle, still 2-arg and unsafe.
        let diags = security_diags(r#"open(my $fh, $path);"#);
        assert!(
            diags.iter().any(|d| d.code.as_deref() == Some("PL401")),
            "parenthesized 2-arg open with lexical fh should be flagged as PL401: {diags:?}"
        );
    }

    #[test]
    fn three_arg_open_is_not_flagged_as_two_arg_open() {
        // open($fh, "<", $path) — the safe 3-arg form must not trigger PL401.
        let diags = security_diags(r#"open(my $fh, "<", $path);"#);
        assert!(
            diags.iter().all(|d| d.code.as_deref() != Some("PL401")),
            "3-arg open should not be flagged as PL401: {diags:?}"
        );
    }

    // --- readpipe() tests ---

    #[test]
    fn readpipe_call_is_flagged() {
        let diags = security_diags(r#"my $out = readpipe("ls -la");"#);
        assert!(
            diags.iter().any(|d| d.code.as_deref() == Some("PL606")),
            "readpipe() should be flagged as PL606: {diags:?}"
        );
    }

    #[test]
    fn nested_variable_list_traversal_in_security_walker() {
        // Covers lines 113-118: NestedVariableList arm in walk_security_node.
        // A my-declaration with a nested paren group produces a NestedVariableList node
        // in the AST; the security walker must recurse into it without flagging it.
        let diags = security_diags("my ($a, ($b, $c)) = (1, (2, 3));");
        // No security diagnostics expected for a plain lexical nested-list declaration.
        assert!(
            diags.is_empty(),
            "nested variable list declaration should not produce security diagnostics: {diags:?}"
        );
    }

    #[test]
    fn nested_variable_list_deep_traversal_in_security_walker() {
        // Exercises recursive descent through deeply nested NestedVariableList nodes.
        let diags = security_diags("my ($x, ($y, ($z, $w))) = (1, (2, (3, 4)));");
        assert!(
            diags.is_empty(),
            "deeply nested variable list should not produce security diagnostics: {diags:?}"
        );
    }

    // --- producer-owned critic overlap observations (#11918) ---

    use crate::tooling::perl_critic::{CriticFindingOrigin, CriticFindingShape};

    fn observation_of<'a>(
        diags: &'a [Diagnostic],
        code: &str,
    ) -> Option<&'a crate::tooling::perl_critic::BuiltInCriticObservation> {
        diags
            .iter()
            .find(|d| d.code.as_deref() == Some(code))
            .and_then(|d| d.critic_observation.as_ref())
    }

    #[test]
    fn command_execution_emitters_declare_reviewed_critic_identities() {
        for (source, code, shape) in [
            (r#"system("ls");"#, "PL603", CriticFindingShape::SystemCall),
            (r#"exec("ls");"#, "PL604", CriticFindingShape::ExecCall),
            (r#"my $out = readpipe("ls");"#, "PL606", CriticFindingShape::Readpipe),
            ("my $out = `ls`;", "PL601", CriticFindingShape::Backtick),
            ("my $out = qx(ls);", "PL601", CriticFindingShape::Qx),
        ] {
            let diags = security_diags(source);
            assert!(
                observation_of(&diags, code).is_some(),
                "{code} must carry a critic observation: {diags:?}"
            );
            let observation = must_some(observation_of(&diags, code));

            assert_eq!(observation.identity().origin(), CriticFindingOrigin::BuiltInDiagnostic);
            assert_eq!(observation.identity().code(), code);
            assert_eq!(observation.identity().shape(), shape);
            assert_eq!(observation.severity(), crate::tooling::perl_critic::Severity::Harsh);
            assert!(
                observation.message().contains("input is sanitized"),
                "producer message travels with the observation"
            );
            assert!(observation.explanation().is_some());
        }
    }

    /// #12004: the observation's remediation copy must stay identical to the
    /// ordinary diagnostic fields it mirrors, or merged rows silently serve
    /// stale text after the ordinary row retires.
    #[test]
    fn observation_remediation_copies_match_the_ordinary_diagnostic_fields() {
        for (source, code) in [
            (r#"system("ls");"#, "PL603"),
            (r#"exec("ls");"#, "PL604"),
            (r#"my $out = readpipe("ls");"#, "PL606"),
            ("my $out = `ls`;", "PL601"),
            ("my $out = qx(ls);", "PL601"),
        ] {
            let diags = security_diags(source);
            let diagnostic = must_some_with(
                diags.iter().find(|d| d.code.as_deref() == Some(code)),
                format_args!("{code} must be emitted for {source}"),
            );
            let suggestion = must_some_with(
                diagnostic.suggestion.as_deref(),
                format_args!("{code} must carry an ordinary suggestion"),
            );
            let observation = must_some_with(
                observation_of(&diags, code),
                format_args!("{code} must carry a critic observation: {diags:?}"),
            );

            assert_eq!(
                observation.suggestion(),
                Some(suggestion),
                "{code}: observation suggestion drifted from the ordinary diagnostic"
            );

            let ordinary_related: Vec<_> =
                diagnostic.related_information.iter().map(|r| r.message.as_str()).collect();
            let observation_related: Vec<_> = observation
                .related_information()
                .iter()
                .map(|(_, message)| message.as_str())
                .collect();
            assert_eq!(
                observation_related, ordinary_related,
                "{code}: observation related information drifted from the ordinary diagnostic"
            );
        }
    }

    #[test]
    fn qx_form_fires_pl601_and_single_quoted_qx_text_does_not() {
        let diags = security_diags("my $date = qx(date);");
        assert!(
            diags.iter().any(|d| d.code.as_deref() == Some("PL601")),
            "qx command execution is the reviewed second PL601 shape: {diags:?}"
        );

        let quoted = security_diags("my $text = 'qx(date)';");
        assert!(
            quoted.iter().all(|d| d.code.as_deref() != Some("PL601")),
            "an ordinary single-quoted string is not command execution: {quoted:?}"
        );
    }

    #[test]
    fn command_execution_observations_cover_exact_emitter_ranges() {
        let source = "my $out = `ls`;";
        let diags = security_diags(source);
        assert!(
            observation_of(&diags, "PL601").is_some(),
            "backtick must carry an observation: {diags:?}"
        );
        let observation = must_some(observation_of(&diags, "PL601"));
        let (start, end) = observation.byte_range();
        assert_eq!(&source[start..end], "`ls`", "byte range is the exact observed syntax");
    }

    #[test]
    fn unrelated_security_diagnostics_carry_no_observation() {
        let diags = security_diags(r#"open(FH, "<", "file.txt");"#);
        assert!(
            diags.iter().all(|d| d.critic_observation.is_none()),
            "only the reviewed overlap cohort declares observations: {diags:?}"
        );
    }

    // --- SQL injection (PL607) tests (#5035) ---

    fn sql_diags(source: &str) -> Vec<Diagnostic> {
        security_diags(source)
    }

    fn dbh_connect() -> &'static str {
        r#"my $dbh = DBI->connect("dbi:Pg:dbname=x", "u", "p");"#
    }

    fn pl607(diags: &[Diagnostic]) -> Option<&Diagnostic> {
        diags.iter().find(|d| d.code.as_deref() == Some("PL607"))
    }

    #[test]
    fn interpolated_prepare_is_flagged_with_exact_range() {
        let source = format!(
            "{}\nmy $user_id = 42;\nmy $sth = $dbh->prepare(\"SELECT * FROM users WHERE id = $user_id\");\n",
            dbh_connect()
        );
        let diags = sql_diags(&source);
        let diagnostic = must_some_with(
            pl607(&diags),
            format_args!("interpolated prepare must be flagged as PL607: {diags:?}"),
        );
        let (start, end) = diagnostic.range;
        assert_eq!(
            &source[start..end],
            r#"$dbh->prepare("SELECT * FROM users WHERE id = $user_id")"#,
            "PL607 byte range must cover the exact prepare call"
        );
        assert_eq!(diagnostic.severity, DiagnosticSeverity::Warning);
        assert!(diagnostic.critic_observation.is_none());
        assert!(diagnostic.suggestion.is_some());
    }

    #[test]
    fn placeholder_prepare_is_silent() {
        let source = format!(
            "{}\nmy $sth = $dbh->prepare('SELECT * FROM users WHERE id = ?');\n$sth->execute(42);\n",
            dbh_connect()
        );
        let diags = sql_diags(&source);
        assert!(
            pl607(&diags).is_none(),
            "placeholders with bind values are the safe control: {diags:?}"
        );
    }

    #[test]
    fn concatenated_variable_sql_is_flagged() {
        let source = format!(
            "{}\nmy $user_input = <STDIN>;\nmy $sth = $dbh->prepare('SELECT * FROM users WHERE id = ' . $user_input);\n",
            dbh_connect()
        );
        let diags = sql_diags(&source);
        assert!(
            pl607(&diags).is_some(),
            "concatenated variable SQL must be flagged as PL607: {diags:?}"
        );
    }

    #[test]
    fn concatenated_literal_only_sql_is_silent() {
        let source = format!(
            "{}\nmy $sth = $dbh->prepare('SELECT ' . '*' . ' FROM users');\n",
            dbh_connect()
        );
        let diags = sql_diags(&source);
        assert!(
            pl607(&diags).is_none(),
            "literal-only concatenation carries no variable: {diags:?}"
        );
    }

    #[test]
    fn interpolated_do_is_flagged_and_placeholder_do_is_silent() {
        let flagged = format!(
            "{}\nmy $name = <STDIN>;\n$dbh->do(\"DELETE FROM users WHERE name = $name\");\n",
            dbh_connect()
        );
        let diags = sql_diags(&flagged);
        assert!(pl607(&diags).is_some(), "interpolated do() must be flagged as PL607: {diags:?}");

        let safe = format!(
            "{}\n$dbh->do('DELETE FROM users WHERE name = ?', undef, 'bob');\n",
            dbh_connect()
        );
        let diags = sql_diags(&safe);
        assert!(
            pl607(&diags).is_none(),
            "placeholder do() with bind values must stay silent: {diags:?}"
        );
    }

    #[test]
    fn prepare_cached_is_a_statement_sink() {
        let source = format!(
            "{}\nmy $id = <STDIN>;\nmy $sth = $dbh->prepare_cached(\"SELECT * FROM t WHERE id = $id\");\n",
            dbh_connect()
        );
        let diags = sql_diags(&source);
        assert!(
            pl607(&diags).is_some(),
            "prepare_cached takes SQL text exactly like prepare: {diags:?}"
        );
    }

    #[test]
    fn execute_bind_values_are_never_a_sql_text_sink() {
        // #5035 research: execute(@bind_values) receives bind values for an
        // already-prepared statement, not SQL text — even an interpolated
        // argument is a computed bind value, not statement assembly.
        let source = format!(
            "{}\nmy $sth = $dbh->prepare('SELECT * FROM users WHERE id = ?');\nmy $id = <STDIN>;\n$sth->execute(\"$id\");\n",
            dbh_connect()
        );
        let diags = sql_diags(&source);
        assert!(
            pl607(&diags).is_none(),
            "execute() arguments are bind values, never SQL text: {diags:?}"
        );
    }

    #[test]
    fn non_dbi_receiver_prepare_stays_silent() {
        // Receiver ambiguity boundary (#5035 research: "receiver ambiguity
        // cannot produce a DBI warning"): a `prepare` method on an object
        // without same-file DBI->connect evidence is not a proven DBI sink.
        let source = concat!(
            "package Engine;\n",
            "sub new { return bless {}, shift }\n",
            "sub prepare { return 1 }\n",
            "package main;\n",
            "my $engine = Engine->new;\n",
            "my $name = <STDIN>;\n",
            "my $q = $engine->prepare(\"SELECT $name\");\n",
        );
        let diags = sql_diags(source);
        assert!(
            pl607(&diags).is_none(),
            "non-DBI receiver prepare must not produce a DBI warning: {diags:?}"
        );
    }

    #[test]
    fn unassigned_receiver_prepare_stays_silent() {
        // Same ambiguity boundary: `$dbh` with no visible connect evidence is
        // unproven, so even a spelled-identically variable stays silent.
        let source = "my $sth = $dbh->prepare(\"SELECT * FROM users WHERE id = $user_id\");\n";
        let diags = sql_diags(source);
        assert!(
            pl607(&diags).is_none(),
            "unproven receiver must not produce a DBI warning: {diags:?}"
        );
    }

    #[test]
    fn computed_sql_variable_is_a_dynamic_boundary_and_stays_silent() {
        let source = format!(
            "{}\nmy $sql = build_query();\nmy $sth = $dbh->prepare($sql);\n",
            dbh_connect()
        );
        let diags = sql_diags(&source);
        assert!(
            pl607(&diags).is_none(),
            "a computed SQL string is a typed dynamic boundary, never a guess: {diags:?}"
        );
    }

    #[test]
    fn mixed_placeholder_and_interpolation_still_fires() {
        // Placeholders elsewhere in the statement do not sanitize a variable
        // interpolated into another part of the SQL text.
        let source = format!(
            "{}\nmy $col = <STDIN>;\nmy $sth = $dbh->prepare(\"SELECT * FROM t WHERE a = ? AND b = $col\");\n",
            dbh_connect()
        );
        let diags = sql_diags(&source);
        assert!(
            pl607(&diags).is_some(),
            "a placeholder does not sanitize an interpolated variable in the same statement: {diags:?}"
        );
    }

    #[test]
    fn escaped_sigil_in_sql_string_stays_silent() {
        let source =
            format!("{}\nmy $sth = $dbh->prepare(\"SELECT '5\\$' WHERE x = ?\");\n", dbh_connect());
        let diags = sql_diags(&source);
        assert!(pl607(&diags).is_none(), "an escaped sigil is not an interpolation: {diags:?}");
    }

    #[test]
    fn assignment_form_receiver_evidence_is_accepted() {
        let source = concat!(
            "my $dbh;\n",
            "$dbh = DBI->connect('dbi:SQLite:dbname=x');\n",
            "my $id = <STDIN>;\n",
            "$dbh->do(\"DELETE FROM t WHERE id = $id\");\n",
        );
        let diags = sql_diags(source);
        assert!(
            pl607(&diags).is_some(),
            "plain assignment from DBI->connect is valid receiver evidence: {diags:?}"
        );
    }

    #[test]
    fn sql_diagnostic_wire_identity_follows_the_storyboarded_contract() {
        // The storyboarded wire format (lsp_critical_user_stories.rs, TEST 4)
        // pins the security.sql_injection codeDescription to the OWASP SQL
        // injection reference; the registered PL607 code carries that href
        // through documentation_url on both push and pull wire paths.
        assert_eq!(
            DiagnosticCode::SecuritySqlInjection.documentation_url(),
            Some("https://owasp.org/www-community/attacks/SQL_Injection")
        );
    }

    // --- #5035 review repairs ---

    #[test]
    fn even_backslash_run_still_interpolates_and_is_flagged() {
        // Review finding: Perl escapes the backslash itself for an even-length
        // run, so `"\\$id"` emits one literal backslash AND interpolates $id.
        // The producer must flag it, not classify it as static SQL.
        let statement = r#"my $sth = $dbh->prepare("SELECT * FROM t WHERE x = \\$id");"#;
        let source = format!("{}\nmy $id = <STDIN>;\n{}\n", dbh_connect(), statement);
        let diags = sql_diags(&source);
        assert!(
            pl607(&diags).is_some(),
            "an even backslash run does not escape the sigil: {diags:?}"
        );
    }

    #[test]
    fn odd_backslash_run_escapes_the_sigil_and_stays_silent() {
        // Review finding mirror control: `"\$id"` keeps the sigil literal, so
        // the SQL text is static.
        let statement = r#"my $sth = $dbh->prepare("SELECT * FROM t WHERE x = \$id");"#;
        let source = format!("{}\n{}\n", dbh_connect(), statement);
        let diags = sql_diags(&source);
        assert!(pl607(&diags).is_none(), "an odd backslash run escapes the sigil: {diags:?}");
    }

    #[test]
    fn shadowed_non_dbi_rebinding_suppresses_the_warning() {
        // Review finding: an inner `my $dbh = Engine->new` shadows the outer
        // DBI->connect binding. Name-level pre-sink evidence is mixed, so the
        // receiver is an ambiguity boundary and stays silent — the warning
        // never guesses which binding the sink sees.
        let source = concat!(
            "my $dbh = DBI->connect('dbi:SQLite:dbname=x');\n",
            "{\n",
            "my $dbh = Engine->new;\n",
            "my $id = <STDIN>;\n",
            "my $q = $dbh->prepare(\"SELECT * FROM t WHERE id = $id\");\n",
            "}\n",
        );
        let diags = sql_diags(source);
        assert!(
            pl607(&diags).is_none(),
            "a shadowed non-DBI rebinding must not receive PL607: {diags:?}"
        );
    }

    #[test]
    fn connect_introduced_after_the_sink_stays_silent() {
        // Review finding: a connect assignment after the call cannot describe
        // the receiver at call time; the sink has no pre-sink connect
        // evidence and stays silent.
        let source = concat!(
            "my $id = <STDIN>;\n",
            "my $sth = $dbh->prepare(\"SELECT * FROM t WHERE id = $id\");\n",
            "my $dbh = DBI->connect('dbi:SQLite:dbname=x');\n",
        );
        let diags = sql_diags(source);
        assert!(
            pl607(&diags).is_none(),
            "a connect after the sink must not retroactively classify it: {diags:?}"
        );
    }

    #[test]
    fn interpolating_heredoc_sql_is_flagged() {
        // Review finding: `<<END_SQL` interpolates exactly like a
        // double-quoted string, so a sigil in the body is source-proven SQL
        // assembly.
        let source = format!(
            "{}\nmy $id = <STDIN>;\nmy $sth = $dbh->prepare(<<END_SQL);\nSELECT * FROM t WHERE id = $id\nEND_SQL\n",
            dbh_connect()
        );
        let diags = sql_diags(&source);
        assert!(
            pl607(&diags).is_some(),
            "an interpolating heredoc with a sigil must be flagged: {diags:?}"
        );
    }

    #[test]
    fn literal_heredoc_sql_is_silent() {
        // Mirror control: `<<'END_SQL'` never interpolates, so even `$id`
        // text in the body is a static SQL literal.
        let source = format!(
            "{}\nmy $sth = $dbh->prepare(<<'END_SQL');\nSELECT * FROM t WHERE id = $id\nEND_SQL\n",
            dbh_connect()
        );
        let diags = sql_diags(&source);
        assert!(pl607(&diags).is_none(), "a literal heredoc is static SQL: {diags:?}");
    }

    // --- #5035 review repairs: punctuation match variables (P1) ---

    #[test]
    fn ampersand_match_variable_sql_is_flagged_with_exact_range() {
        // `$&` (the match text) interpolates like any scalar: after a match
        // over attacker-controlled input it injects that text into the SQL,
        // so the sink must classify as interpolated, not static.
        let source = format!(
            "{}\nmy $raw = <STDIN>;\n$raw =~ /(\\w+)/;\nmy $sth = $dbh->prepare(\"SELECT * FROM t WHERE name = $&\");\n",
            dbh_connect()
        );
        let diags = sql_diags(&source);
        let diagnostic = must_some_with(
            pl607(&diags),
            format_args!("`$&` match text in SQL must be flagged as PL607: {diags:?}"),
        );
        let (start, end) = diagnostic.range;
        assert_eq!(
            &source[start..end],
            r#"$dbh->prepare("SELECT * FROM t WHERE name = $&")"#,
            "PL607 byte range must cover the exact prepare call"
        );
    }

    #[test]
    fn pre_match_variable_sql_is_flagged_with_exact_range() {
        // `` $` `` (the pre-match text) interpolates like any scalar after a
        // match over attacker-controlled input.
        let source = format!(
            "{}\nmy $raw = <STDIN>;\n$raw =~ /(\\w+)/;\nmy $sth = $dbh->prepare(\"SELECT * FROM t WHERE name = $`\");\n",
            dbh_connect()
        );
        let diags = sql_diags(&source);
        let diagnostic = must_some_with(
            pl607(&diags),
            format_args!("`` $` `` pre-match text in SQL must be flagged as PL607: {diags:?}"),
        );
        let (start, end) = diagnostic.range;
        assert_eq!(
            &source[start..end],
            r#"$dbh->prepare("SELECT * FROM t WHERE name = $`")"#,
            "PL607 byte range must cover the exact prepare call"
        );
    }

    #[test]
    fn post_match_variable_sql_is_flagged_with_exact_range() {
        // `$'` (the post-match text) interpolates like any scalar after a
        // match over attacker-controlled input.
        let source = format!(
            "{}\nmy $raw = <STDIN>;\n$raw =~ /(\\w+)/;\nmy $sth = $dbh->prepare(\"SELECT * FROM t WHERE name = $'\");\n",
            dbh_connect()
        );
        let diags = sql_diags(&source);
        let diagnostic = must_some_with(
            pl607(&diags),
            format_args!("`$'` post-match text in SQL must be flagged as PL607: {diags:?}"),
        );
        let (start, end) = diagnostic.range;
        assert_eq!(
            &source[start..end],
            r#"$dbh->prepare("SELECT * FROM t WHERE name = $'")"#,
            "PL607 byte range must cover the exact prepare call"
        );
    }

    #[test]
    fn highest_capture_group_variable_sql_is_flagged_with_exact_range() {
        // `$+` (the highest-numbered capture group of the last successful
        // match) is a punctuation match variable exactly like `$&`: captured
        // attacker text reaches the SQL through it.
        let source = format!(
            "{}\nmy $raw = <STDIN>;\n$raw =~ /(\\w+)/;\nmy $sth = $dbh->prepare(\"SELECT * FROM t WHERE name = $+\");\n",
            dbh_connect()
        );
        let diags = sql_diags(&source);
        let diagnostic = must_some_with(
            pl607(&diags),
            format_args!("`$+` capture text in SQL must be flagged as PL607: {diags:?}"),
        );
        let (start, end) = diagnostic.range;
        assert_eq!(
            &source[start..end],
            r#"$dbh->prepare("SELECT * FROM t WHERE name = $+")"#,
            "PL607 byte range must cover the exact prepare call"
        );
    }

    #[test]
    fn escaped_punctuation_match_variable_stays_silent() {
        // The parity rule composes with the new successors: an odd backslash
        // run escapes the sigil, so `\$&` is literal `$&` text and the SQL
        // stays static.
        let source = format!(
            "{}\nmy $sth = $dbh->prepare(\"SELECT * FROM t WHERE x = '\\$&' AND y = ?\");\n",
            dbh_connect()
        );
        let diags = sql_diags(&source);
        assert!(
            pl607(&diags).is_none(),
            "an escaped punctuation match variable is not an interpolation: {diags:?}"
        );
    }

    // --- #5035 review repairs: single-pass receiver evidence index (P2) ---

    #[test]
    fn receiver_evidence_index_keeps_classification_identical_across_sink_counts() {
        // Sinks resolve receivers through the name-keyed index instead of
        // rescanning the whole assignment list per sink (the quadratic
        // O(assignments x sinks) pass). This pins the classification matrix
        // at several document sizes: each proven receiver fires exactly once,
        // rebound and unproven receivers stay silent, a shared name with many
        // connect assignments keeps every sink proven, and a mid-document
        // rebinding silences exactly the post-rebinding sinks.
        let pl607_count = |diags: &[Diagnostic]| {
            diags.iter().filter(|d| d.code.as_deref() == Some("PL607")).count()
        };

        for sinks in [1usize, 10, 100, 500] {
            // (a) one connect and one interpolated sink per distinct receiver.
            let mut proven = String::new();
            let mut rebound_silent = String::new();
            for i in 0..sinks {
                proven.push_str(&format!(
                    "my $dbh{i} = DBI->connect('dbi:SQLite:dbname=x');\nmy $id{i} = <STDIN>;\n$dbh{i}->do(\"DELETE FROM t{i} WHERE id = $id{i}\");\n"
                ));
                rebound_silent.push_str(&format!(
                    "my $eng{i} = Engine->new;\nmy $id{i} = <STDIN>;\n$eng{i}->do(\"DELETE FROM t{i} WHERE id = $id{i}\");\n"
                ));
            }
            assert_eq!(
                pl607_count(&sql_diags(&proven)),
                sinks,
                "every proven receiver fires once at {sinks} sinks"
            );
            assert_eq!(
                pl607_count(&sql_diags(&rebound_silent)),
                0,
                "non-connect receivers stay silent at {sinks} sinks"
            );

            // (b) the crafted quadratic shape: one shared name with many
            // assignments, all connects, feeding many sinks.
            let mut shared = String::new();
            for _ in 0..sinks {
                shared.push_str("$dbh = DBI->connect('dbi:SQLite:dbname=x');\n");
            }
            shared.push_str("my $id = <STDIN>;\n");
            for _ in 0..sinks {
                shared.push_str("$dbh->do(\"DELETE FROM t WHERE id = $id\");\n");
            }
            assert_eq!(
                pl607_count(&sql_diags(&shared)),
                sinks,
                "all-connect shared name keeps every sink proven at {sinks} assignments/sinks"
            );

            // (c) mid-document rebinding of the shared name: only the sinks
            // before the non-connect rebinding carry proven evidence.
            let mut rebound = String::new();
            rebound.push_str("my $dbh = DBI->connect('dbi:SQLite:dbname=x');\nmy $id = <STDIN>;\n");
            for _ in 0..sinks {
                rebound.push_str("$dbh->do(\"DELETE FROM a WHERE id = $id\");\n");
            }
            rebound.push_str("$dbh = Engine->new;\n");
            for _ in 0..sinks {
                rebound.push_str("$dbh->do(\"DELETE FROM b WHERE id = $id\");\n");
            }
            assert_eq!(
                pl607_count(&sql_diags(&rebound)),
                sinks,
                "only pre-rebinding sinks fire at {sinks} sinks"
            );
        }
    }

    // --- embedded regex code (#9818) ---

    fn has_code(diags: &[Diagnostic], expected: &str) -> bool {
        diags.iter().any(|d| d.code.as_deref() == Some(expected))
    }

    #[test]
    fn e_modifier_substitution_is_flagged() {
        let diags = security_diags(r#"$s =~ s/(\w+)/uc($1)/e;"#);
        assert!(
            has_code(&diags, "PL608"),
            "s///e should publish the stable substitution-eval code PL608: {diags:?}"
        );
    }

    #[test]
    fn ee_modifier_substitution_is_flagged() {
        let diags = security_diags(r#"$t =~ s/\$(\w+)/$$1/ee;"#);
        assert!(
            has_code(&diags, "PL608"),
            "s///ee should publish the stable substitution-eval code PL608: {diags:?}"
        );
    }

    #[test]
    fn standalone_e_modifier_substitution_is_flagged() {
        let diags = security_diags(r#"s/version (\d+)/$1 + 1/e;"#);
        assert!(
            has_code(&diags, "PL608"),
            "bare s///e should publish PL608 even without a =~ binding: {diags:?}"
        );
    }

    #[test]
    fn embedded_code_block_in_qr_is_flagged() {
        let diags = security_diags(r#"my $r = qr/(?{ print "hi" })/;"#);
        assert!(
            has_code(&diags, "PL609"),
            "qr/(?{{...}})/ should publish the stable embedded-code class PL609: {diags:?}"
        );
    }

    #[test]
    fn embedded_code_block_in_explicit_match_is_flagged() {
        let diags = security_diags(r#"$x =~ m/(?{ print "hi" })/;"#);
        assert!(
            has_code(&diags, "PL609"),
            "m/(?{{...}})/ should publish the stable embedded-code class PL609: {diags:?}"
        );
    }

    #[test]
    fn embedded_code_block_in_bare_match_is_flagged() {
        let diags = security_diags(r#"$x =~ /(?{ print "hi" })/;"#);
        assert!(
            has_code(&diags, "PL609"),
            "bare /(?{{...}})/ should publish the same embedded-code class PL609: {diags:?}"
        );
    }

    #[test]
    fn embedded_code_block_in_substitution_pattern_is_flagged() {
        let diags = security_diags(r#"$x =~ s/(?{ print "hi" })/ok/;"#);
        assert!(
            has_code(&diags, "PL609"),
            "(?{{...}}) inside a substitution pattern without /e should publish PL609: {diags:?}"
        );
    }

    #[test]
    fn plain_substitution_is_not_flagged() {
        let diags = security_diags(r#"$s =~ s/a/b/;"#);
        assert!(
            !diags.iter().any(|d| d.code.as_deref().is_some_and(|c| c.starts_with("PL6"))),
            "plain s/// must not publish a security diagnostic: {diags:?}"
        );
    }

    #[test]
    fn qr_without_embedded_code_is_not_flagged() {
        let diags = security_diags(r#"my $re = qr/hello/;"#);
        assert!(
            !diags.iter().any(|d| d.code.as_deref().is_some_and(|c| c.starts_with("PL6"))),
            "plain qr// must not publish a security diagnostic: {diags:?}"
        );
    }

    // --- bound-expression traversal (#9821) ---

    #[test]
    fn backtick_bound_to_match_is_still_flagged() {
        let diags = security_diags("`ls` =~ /x/;");
        assert!(
            has_code(&diags, "PL601"),
            "backtick under Match.expr must publish the same PL601 as elsewhere: {diags:?}"
        );
    }

    #[test]
    fn backtick_bound_to_substitution_is_still_flagged() {
        let diags = security_diags("`ls` =~ s/a/b/;");
        assert!(
            has_code(&diags, "PL601"),
            "backtick under Substitution.expr must publish the same PL601: {diags:?}"
        );
    }

    #[test]
    fn readpipe_bound_to_match_is_still_flagged() {
        let diags = security_diags(r#"readpipe("ls") =~ /x/;"#);
        assert!(
            has_code(&diags, "PL606"),
            "readpipe() under Match.expr must keep its own stable code PL606: {diags:?}"
        );
    }

    #[test]
    fn variable_bound_to_match_is_not_flagged() {
        let diags = security_diags("$s =~ /x/;");
        assert!(
            !diags.iter().any(|d| d.code.as_deref().is_some_and(|c| c.starts_with("PL6"))),
            "ordinary variable binding under =~ must stay silent: {diags:?}"
        );
    }
}
