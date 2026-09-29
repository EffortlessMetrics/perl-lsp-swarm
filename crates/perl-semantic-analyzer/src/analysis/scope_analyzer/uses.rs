//! Handlers for variable read/use and assignment tracking node kinds in scope analysis.

use super::{
    AnalysisContext, Scope, ScopeAnalyzer, ScopeIssue, is_builtin_global, is_capture_variable,
    is_known_function, split_variable_name,
};
use crate::ast::{Node, NodeKind};
use crate::pragma_tracker::PragmaState;
use std::rc::Rc;

/// Handle `NodeKind::Variable`.
///
/// Returns `true` if the caller should return early (capture variable already handled).
#[allow(clippy::too_many_arguments)]
pub(super) fn handle_variable<'a>(
    analyzer: &ScopeAnalyzer,
    node: &'a Node,
    sigil: &'a str,
    name: &'a str,
    scope: &Rc<Scope>,
    ancestors: &[&'a Node],
    issues: &mut Vec<ScopeIssue>,
    context: &AnalysisContext<'a>,
    strict_vars_mode: bool,
) -> bool {
    // Capture variables ($1, $2, ...) are built-in globals but require a preceding
    // regex match in scope to be meaningful. Check before the general builtin skip.
    if sigil == "$" && is_capture_variable(name) {
        if !scope.regex_match_in_scope() {
            let full_name = format!("{}{}", sigil, name);
            issues.push(ScopeIssue {
                kind: super::IssueKind::CaptureVarWithoutRegexMatch,
                variable_name: full_name.clone(),
                line: context.get_line(node.location.start),
                range: (node.location.start, node.location.end),
                description: format!(
                    "Capture variable '{}' used without a preceding regex match in scope",
                    full_name
                ),
            });
        }
        return true;
    }

    // Skip built-in global variables — but only when no lexical declaration shadows
    // them.  Variables like $a and $b are sort globals, but `my ($a, $b) = @_`
    // creates a lexical shadow that must be tracked as used.
    if is_builtin_global(sigil, name) && !scope.has_variable_parts(sigil, name) {
        return true;
    }

    // Skip package-qualified variables
    if name.contains("::") {
        return true;
    }

    // Normalize explicit dereference/container syntax before lookup so that
    // `@$ref` resolves to `$ref`, while direct subscripting keeps using the
    // container sigil that the syntax implies.
    let (lookup_sigil, lookup_name) =
        analyzer.resolve_variable_use_target(node, ancestors, context).unwrap_or((sigil, name));
    let (variable_used, is_initialized) =
        analyzer.use_variable_parts_in_context(scope, lookup_sigil, lookup_name, context);

    // Variable not found - check if we should report it
    if !variable_used {
        if strict_vars_mode {
            analyzer.push_undeclared_variable_issue(issues, context, node, sigil, name);
        }
    } else if !is_initialized {
        analyzer.push_uninitialized_variable_issue(issues, context, node, sigil, name);
    }
    false
}

/// Split a braced typeglob body into a single simple variable use — one sigil
/// plus a plain identifier (`$x`, `@a`, `%h`) and nothing else.
///
/// Compound computed bodies (`*{$x . $y}`, `*{foo()}`, `*{$h{key}}`) have no
/// single variable name; recording the raw body text as one variable would
/// fabricate a name like `x . $y` and raise a false undeclared-variable
/// diagnostic under strict mode (#15712). This shortcut only applies when the
/// `Typeglob` node carries no structured body child; with one, the child is
/// walked instead (#15731).
fn split_simple_variable_body(body: &str) -> Option<(&str, &str)> {
    let (sigil, var_name) = split_variable_name(body);
    if sigil.is_empty()
        || var_name.is_empty()
        || !var_name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return None;
    }
    Some((sigil, var_name))
}

/// Handle `NodeKind::Typeglob`.
#[allow(clippy::too_many_arguments)]
pub(super) fn handle_typeglob<'a>(
    analyzer: &ScopeAnalyzer,
    node: &'a Node,
    name: &str,
    body: Option<&'a Node>,
    scope: &Rc<Scope>,
    ancestors: &mut Vec<&'a Node>,
    issues: &mut Vec<ScopeIssue>,
    context: &AnalysisContext<'a>,
    strict_vars_mode: bool,
) {
    // `Typeglob { name }` stores the name without its leading `*`, unlike the
    // equivalent `Variable { sigil: "*", name }` shape.  Reconstruct the
    // sigil here so typeglob aliases participate in the same scope/use ledger.
    // Brace-delimited names remain dynamic and must not become literal symbols.
    if let Some(body) = body {
        // A structured computed body (`*{$x . $y} = ...`) is authoritative:
        // walk it so each used variable is recorded with its own node —
        // declared variables count as used and undeclared variables earn
        // individual strict-mode issues (#15731). Variables are never
        // synthesized from the raw body text.
        ancestors.push(node);
        analyzer.analyze_node(body, scope, ancestors, issues, context);
        ancestors.pop();
        return;
    }

    if let Some(expression) = name.strip_prefix('{').and_then(|name| name.strip_suffix('}')) {
        // Only a body that is exactly one simple variable (`*{$x}`) is a single
        // recorded use; any other computed body must not be recorded as one
        // fabricated variable name (#15712). This string-level shortcut stays
        // for Typeglob nodes parsed without a structured body child.
        if let Some((sigil, var_name)) = split_simple_variable_body(expression) {
            analyzer.record_variable_use(
                scope,
                strict_vars_mode,
                context,
                issues,
                node,
                sigil,
                var_name,
            );
        }
        return;
    }

    let (sigil, var_name) = ("*", name);
    if !sigil.is_empty() && !var_name.is_empty() && !var_name.contains("::") {
        analyzer.record_variable_use(scope, false, context, issues, node, sigil, var_name);
    }
}

/// Handle `NodeKind::Readline`.
pub(super) fn handle_readline(
    analyzer: &ScopeAnalyzer,
    node: &Node,
    filehandle: &str,
    scope: &Rc<Scope>,
    issues: &mut Vec<ScopeIssue>,
    context: &AnalysisContext<'_>,
    strict_vars_mode: bool,
) {
    let (sigil, var_name) = split_variable_name(filehandle);
    if !sigil.is_empty() && !var_name.is_empty() && !var_name.contains("::") {
        analyzer.record_variable_use(
            scope,
            strict_vars_mode,
            context,
            issues,
            node,
            sigil,
            var_name,
        );
    }
}

/// Handle `NodeKind::Assignment`.
///
/// Returns `true` if the caller should return early (fast-path scalar assignment handled).
pub(super) fn handle_assignment<'a>(
    analyzer: &ScopeAnalyzer,
    _node: &'a Node,
    lhs: &'a Node,
    rhs: &'a Node,
    scope: &Rc<Scope>,
    ancestors: &mut Vec<&'a Node>,
    issues: &mut Vec<ScopeIssue>,
    context: &AnalysisContext<'a>,
) -> bool {
    // Handle assignment: LHS variable becomes initialized
    // First analyze RHS (usages)
    analyzer.analyze_node(rhs, scope, ancestors, issues, context);

    // Optimization: Handle simple scalar assignment directly to avoid double lookup
    // (mark_initialized + analyze_node both perform lookups)
    if let NodeKind::Variable { sigil, name } = &lhs.kind
        && !name.contains("::")
        && !is_builtin_global(sigil, name)
        && analyzer.initialize_and_use_variable_parts_in_context(scope, sigil, name, context)
    {
        return true;
    }

    // Then analyze LHS
    // We need to recursively mark variables as initialized in the LHS structure
    // This handles scalars ($x = 1) and lists (($x, $y) = (1, 2))
    analyzer.mark_initialized(lhs, scope, context);

    // Recurse into LHS to trigger UndeclaredVariable checks
    // Note: 'use_variable' marks as used, which is technically correct for assignment too (write usage)
    analyzer.analyze_node(lhs, scope, ancestors, issues, context);
    false
}

/// Handle `NodeKind::Tie`.
#[allow(clippy::too_many_arguments)]
pub(super) fn handle_tie<'a>(
    analyzer: &ScopeAnalyzer,
    node: &'a Node,
    variable: &'a Node,
    package: &'a Node,
    args: &'a [Node],
    scope: &Rc<Scope>,
    ancestors: &mut Vec<&'a Node>,
    issues: &mut Vec<ScopeIssue>,
    context: &AnalysisContext<'a>,
) {
    ancestors.push(node);
    // Analyze arguments first
    analyzer.analyze_node(package, scope, ancestors, issues, context);
    for arg in args {
        analyzer.analyze_node(arg, scope, ancestors, issues, context);
    }

    if let NodeKind::VariableDeclaration { .. } = variable.kind {
        // Must analyze declaration FIRST to declare it, then mark initialized
        analyzer.analyze_node(variable, scope, ancestors, issues, context);
        analyzer.mark_initialized(variable, scope, context);
    } else {
        // For existing variables, mark initialized then analyze (usage)
        analyzer.mark_initialized(variable, scope, context);
        analyzer.analyze_node(variable, scope, ancestors, issues, context);
    }

    ancestors.pop();
}

/// Handle `NodeKind::Untie`.
pub(super) fn handle_untie<'a>(
    analyzer: &ScopeAnalyzer,
    node: &'a Node,
    variable: &'a Node,
    scope: &Rc<Scope>,
    ancestors: &mut Vec<&'a Node>,
    issues: &mut Vec<ScopeIssue>,
    context: &AnalysisContext<'a>,
) {
    ancestors.push(node);
    analyzer.analyze_node(variable, scope, ancestors, issues, context);
    ancestors.pop();
}

/// Handle `NodeKind::Identifier`.
#[allow(clippy::too_many_arguments)]
pub(super) fn handle_identifier(
    analyzer: &ScopeAnalyzer,
    node: &Node,
    name: &str,
    issues: &mut Vec<ScopeIssue>,
    context: &AnalysisContext<'_>,
    ancestors: &[&Node],
    pragma_state: &PragmaState,
    strict_subs_mode: bool,
) {
    // Check for barewords under strict mode, excluding hash keys
    // Hybrid check: Fast path for immediate hash keys (depth 1), then known functions, then deep check
    if strict_subs_mode
        && !analyzer.is_in_hash_key_context(node, ancestors, 1)
        && !is_known_function(name)
        && !pragma_state.has_builtin_import(name)
        && !context.has_imported_bareword(name)
        && !analyzer.is_in_hash_key_context(node, ancestors, 10)
    {
        issues.push(ScopeIssue {
            kind: super::IssueKind::UnquotedBareword,
            variable_name: name.to_string(),
            line: context.get_line(node.location.start),
            range: (node.location.start, node.location.end),
            description: format!("Bareword '{}' not allowed under 'use strict'", name),
        });
    }
}
