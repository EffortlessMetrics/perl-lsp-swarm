//! Handlers for call-shaped and structural recursion node kinds in scope analysis.

use super::{
    AnalysisContext, IssueKind, Scope, ScopeAnalyzer, ScopeIssue,
    builtin_declaration_arg_positions, feature_for_keyword, is_topic_defaulting_builtin,
    is_topic_modifying_builtin,
};
use crate::ast::Node;
use crate::pragma_tracker::PragmaState;
use std::rc::Rc;

/// Handle `NodeKind::FunctionCall`.
#[allow(clippy::too_many_arguments)]
pub(super) fn handle_function_call<'a>(
    analyzer: &ScopeAnalyzer,
    node: &'a Node,
    name: &str,
    args: &'a [Node],
    scope: &Rc<Scope>,
    ancestors: &mut Vec<&'a Node>,
    issues: &mut Vec<ScopeIssue>,
    context: &AnalysisContext<'a>,
    pragma_state: &PragmaState,
    strict_vars_mode: bool,
) {
    if let Some((sigil, var_name)) = analyzer.extract_name_like_variable(name) {
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

    // Feature-gated barewords (e.g. `say`) are only valid when the enabling
    // `feature` is active at this offset — via `use feature '...'` or a version
    // bundle (`use v5.10`/`use v5.36`), both resolved by `has_feature` (#2584).
    // A method call (`$o->say`) and an autoquoted hash key (`say => 1`) parse as
    // different node kinds and never reach here, so no extra guard is needed for
    // them; an explicitly imported symbol (`use Foo qw(say)`) or a user-defined
    // sub of the same name suppresses the gate.
    //
    // When the file declares a version pragma (`use vX.Y`), the `version_compat`
    // lint (`PL900`) owns this diagnostic with a version-specific message, so the
    // gate stands down there to avoid a duplicate warning on the same `say`; the
    // bare-`say`-with-no-version case (which `version_compat` skips) stays ours.
    if let Some(feature) = feature_for_keyword(name) {
        if !pragma_state.has_feature(feature)
            && !context.has_declared_version()
            && !context.has_imported_bareword(name)
            && !context.has_defined_sub(name)
        {
            issues.push(ScopeIssue {
                kind: IssueKind::FeatureNotEnabled,
                variable_name: name.to_string(),
                line: context.get_line(node.location.start),
                range: (node.location.start, node.location.end),
                description: format!(
                    "'{name}' requires `use feature '{feature}'` (or a `use vX.Y` bundle that enables it)"
                ),
            });
        }
    }

    // Builtins that default to $_ when called with zero arguments implicitly
    // read (and in some cases modify) $_. Mark it as used so that any lexically-
    // scoped `my $_` in scope is not reported as unused or uninitialized.
    if args.is_empty() && is_topic_defaulting_builtin(name) {
        if is_topic_modifying_builtin(name) {
            let _ = scope.initialize_and_use_variable_parts("$", "_");
        } else {
            let _ = scope.use_variable_parts("$", "_");
        }
    }
    ancestors.push(node);
    let declaration_arg_positions = builtin_declaration_arg_positions(name);
    for (arg_index, arg) in args.iter().enumerate() {
        analyzer.analyze_node(arg, scope, ancestors, issues, context);
        if declaration_arg_positions.contains(&arg_index) {
            analyzer.mark_builtin_declaration_arg_consumed(arg, scope, context);
        }
    }
    ancestors.pop();
}

/// Handle `NodeKind::MethodCall`.
#[allow(clippy::too_many_arguments)]
pub(super) fn handle_method_call<'a>(
    analyzer: &ScopeAnalyzer,
    node: &'a Node,
    object: &'a Node,
    method: &str,
    args: &'a [Node],
    scope: &Rc<Scope>,
    ancestors: &mut Vec<&'a Node>,
    issues: &mut Vec<ScopeIssue>,
    context: &AnalysisContext<'a>,
    strict_vars_mode: bool,
) {
    ancestors.push(node);
    analyzer.analyze_node(object, scope, ancestors, issues, context);
    if let Some((sigil, var_name)) = analyzer.extract_method_name_variable(method) {
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
    for arg in args {
        analyzer.analyze_node(arg, scope, ancestors, issues, context);
    }
    ancestors.pop();
}

/// Handle `NodeKind::Unary`.
pub(super) fn handle_unary<'a>(
    analyzer: &ScopeAnalyzer,
    node: &'a Node,
    operand: &'a Node,
    scope: &Rc<Scope>,
    ancestors: &mut Vec<&'a Node>,
    issues: &mut Vec<ScopeIssue>,
    context: &AnalysisContext<'a>,
) {
    ancestors.push(node);
    analyzer.analyze_node(operand, scope, ancestors, issues, context);
    ancestors.pop();
}

/// Handle `NodeKind::Binary`.
pub(super) fn handle_binary<'a>(
    analyzer: &ScopeAnalyzer,
    node: &'a Node,
    left: &'a Node,
    right: &'a Node,
    scope: &Rc<Scope>,
    ancestors: &mut Vec<&'a Node>,
    issues: &mut Vec<ScopeIssue>,
    context: &AnalysisContext<'a>,
) {
    ancestors.push(node);
    analyzer.analyze_node(left, scope, ancestors, issues, context);
    analyzer.analyze_node(right, scope, ancestors, issues, context);
    ancestors.pop();
}

/// Handle `NodeKind::ArrayLiteral`.
pub(super) fn handle_array_literal<'a>(
    analyzer: &ScopeAnalyzer,
    node: &'a Node,
    elements: &'a [Node],
    scope: &Rc<Scope>,
    ancestors: &mut Vec<&'a Node>,
    issues: &mut Vec<ScopeIssue>,
    context: &AnalysisContext<'a>,
) {
    ancestors.push(node);
    for element in elements {
        analyzer.analyze_node(element, scope, ancestors, issues, context);
    }
    ancestors.pop();
}
