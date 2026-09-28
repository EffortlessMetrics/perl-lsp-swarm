//! Extract subroutine code action

use super::super::types::{CodeAction, CodeActionEdit, CodeActionKind};
use crate::providers::rename::TextEdit;
use perl_parser_core::ast::{Node, NodeKind, SourceLocation};
use std::collections::HashSet;

use super::helpers::Helpers;

/// Create extract subroutine action.
///
/// Returns `None` when no trustworthy edit can be produced — see
/// `detect_parameters` for the capture shape this generator cannot express
/// correctly.
pub fn create_extract_subroutine_action(
    node: &Node,
    source: &str,
    helpers: &Helpers<'_>,
) -> Option<CodeAction> {
    let body_text = &source[node.location.start..node.location.end];
    // Strip surrounding block braces to avoid double-brace corruption.
    // A Block node's location spans `{ ... }` inclusive; the generated sub
    // already adds its own `sub NAME {`, so the inner braces must be removed.
    let body_text = body_text.trim_start_matches('{').trim_end_matches('}');
    let sub_name = suggest_subroutine_name(node);
    // `None` when a capture cannot be passed through this calling convention.
    let params = detect_parameters(node)?;
    let returns = detect_return_values(node);

    // Generate function signature
    let signature = if params.is_empty() {
        format!("sub {} {{\n", sub_name)
    } else {
        format!("sub {} {{\n    my ({}) = @_;\n", sub_name, params.join(", "))
    };

    // Find insertion position (before current sub or at end)
    let insert_pos = helpers.find_subroutine_insert_position(node.location.start);

    // Generate function call
    let call = if returns.is_empty() {
        format!("{}({});", sub_name, params.join(", "))
    } else {
        format!("my {} = {}({});", returns.join(", "), sub_name, params.join(", "))
    };

    Some(CodeAction {
        title: "Extract to subroutine".to_string(),
        kind: CodeActionKind::RefactorExtract,
        diagnostics: Vec::new(),
        edit: CodeActionEdit {
            changes: vec![
                // Insert function definition
                TextEdit {
                    location: SourceLocation { start: insert_pos, end: insert_pos },
                    new_text: format!("{}{}\n}}\n\n", signature, body_text),
                },
                // Replace block with function call
                TextEdit { location: node.location, new_text: call },
            ],
        },
        is_preferred: false,
    })
}

/// Suggest a subroutine name
pub fn suggest_subroutine_name(_node: &Node) -> String {
    // Could analyze the code to suggest better names
    "process_data".to_string()
}

/// True when `token` names a plain scalar variable such as `$base`.
fn is_scalar_variable(token: &str) -> bool {
    token.starts_with('$')
}

/// Detect the variables a block captures from the enclosing scope.
///
/// Each capture carries its sigil, because the generator re-emits it verbatim:
/// a bare `base` is a constant item rather than a lexical, so `my (base) = @_;`
/// does not compile and the `$base` it was read from is never declared inside
/// the extracted subroutine. Order is first-use, so the same block always
/// produces the same parameter list.
///
/// Returns `None` when a capture is not a plain scalar. The generated signature
/// binds parameters positionally as scalars, so routing a captured array or
/// hash through it would silently change the program's meaning — `my ($items) =
/// @_` keeps only the first element of a list the caller passed whole. That
/// needs a different calling convention, so the action is withheld rather than
/// offered with an edit that changes semantics.
pub fn detect_parameters(node: &Node) -> Option<Vec<String>> {
    let mut params = Vec::new();
    collect_variables(node, &mut params);
    if params.iter().all(|param| is_scalar_variable(param)) { Some(params) } else { None }
}

/// Detect return values in a block.
///
/// Uses a heuristic: if the last non-empty statement in the block is a bare
/// variable expression (implicit return convention in Perl), and that variable
/// was declared inside the block, treat it as the return value.
///
/// Returns a vec of sigil-bearing variable tokens (e.g. `["$result"]`) so the
/// emitted `my $result = process_data(...);` matches the declaration it came
/// from. Dropping the sigil here reproduces the sibling defect this module
/// already had for parameters: `my x = ...` is a bareword assignment, not a
/// lexical declaration.
pub fn detect_return_values(node: &Node) -> Vec<String> {
    let statements = match &node.kind {
        NodeKind::Block { statements } => statements.as_slice(),
        _ => return Vec::new(),
    };

    // Collect all variables declared inside the block, each with its sigil.
    let mut declared_inside: HashSet<String> = HashSet::new();
    for stmt in statements {
        collect_declared_variables(stmt, &mut declared_inside);
    }

    // Look at the last non-empty statement for implicit or explicit return.
    let last = statements
        .iter()
        .rev()
        .find(|s| !matches!(&s.kind, NodeKind::Block { statements } if statements.is_empty()));

    if let Some(last_stmt) = last
        && let Some(var_name) = extract_last_expression_variable(last_stmt)
        && declared_inside.contains(&var_name)
    {
        return vec![var_name];
    }

    Vec::new()
}

/// Collect the sigil-bearing tokens of all variables declared via `my` in a node.
fn collect_declared_variables(node: &Node, declared: &mut HashSet<String>) {
    match &node.kind {
        NodeKind::VariableDeclaration { variable, .. } => {
            if let NodeKind::Variable { sigil, name } = &variable.kind {
                declared.insert(format!("{sigil}{name}"));
            }
        }
        NodeKind::VariableListDeclaration { variables, .. } => {
            for v in variables {
                if let NodeKind::Variable { sigil, name } = &v.kind {
                    declared.insert(format!("{sigil}{name}"));
                }
            }
        }
        NodeKind::Block { statements } => {
            for stmt in statements {
                collect_declared_variables(stmt, declared);
            }
        }
        NodeKind::ExpressionStatement { expression } => {
            collect_declared_variables(expression, declared);
        }
        _ => {}
    }
}

/// If a statement is a bare variable expression or an explicit return, return the
/// sigil-bearing variable token (e.g. `$result`). Used to detect the implicit
/// return value.
fn extract_last_expression_variable(node: &Node) -> Option<String> {
    match &node.kind {
        NodeKind::ExpressionStatement { expression } => {
            extract_last_expression_variable(expression)
        }
        NodeKind::Variable { sigil, name } => Some(format!("{sigil}{name}")),
        NodeKind::Return { value } => {
            value.as_ref().and_then(|v| extract_last_expression_variable(v))
        }
        _ => None,
    }
}

/// Collect the variables a node reads from outside itself, each as a
/// sigil-bearing token (e.g. `$base`).
///
/// Variables that appear in `VariableDeclaration` nodes inside the block are
/// local to the extracted subroutine and should not be listed as parameters.
///
/// A `Vec` plus a `seen` set is used rather than a `HashSet` because the
/// generated signature must be stable: `HashSet` iteration order varies per run,
/// which would reshuffle the parameter list between identical requests.
pub fn collect_variables(node: &Node, vars: &mut Vec<String>) {
    let mut locals = HashSet::new();
    let mut seen = HashSet::new();
    collect_variables_inner(node, vars, &mut locals, &mut seen);
}

fn collect_variables_inner(
    node: &Node,
    vars: &mut Vec<String>,
    locals: &mut HashSet<String>,
    seen: &mut HashSet<String>,
) {
    match &node.kind {
        NodeKind::Variable { sigil, name } if !locals.contains(name.as_str()) => {
            let token = format!("{sigil}{name}");
            if seen.insert(token.clone()) {
                vars.push(token);
            }
        }
        NodeKind::Block { statements } => {
            for stmt in statements {
                collect_variables_inner(stmt, vars, locals, seen);
            }
        }
        NodeKind::ExpressionStatement { expression } => {
            collect_variables_inner(expression, vars, locals, seen);
        }
        NodeKind::VariableDeclaration { variable, initializer, .. } => {
            // The initializer may reference outer variables — collect those first.
            if let Some(init) = initializer {
                collect_variables_inner(init, vars, locals, seen);
            }
            // The declared variable is local to this block — do not treat it as a parameter.
            if let NodeKind::Variable { name, .. } = &variable.kind {
                locals.insert(name.clone());
            }
        }
        NodeKind::VariableListDeclaration { variables, initializer, .. } => {
            // Initializer may reference outer variables.
            if let Some(init) = initializer {
                collect_variables_inner(init, vars, locals, seen);
            }
            // All declared variables are local to the block.
            for v in variables {
                if let NodeKind::Variable { name, .. } = &v.kind {
                    locals.insert(name.clone());
                }
            }
        }
        NodeKind::Assignment { lhs, rhs, .. } => {
            collect_variables_inner(lhs, vars, locals, seen);
            collect_variables_inner(rhs, vars, locals, seen);
        }
        NodeKind::Binary { left, right, .. } => {
            collect_variables_inner(left, vars, locals, seen);
            collect_variables_inner(right, vars, locals, seen);
        }
        NodeKind::FunctionCall { args, .. } => {
            for arg in args {
                collect_variables_inner(arg, vars, locals, seen);
            }
        }
        NodeKind::MethodCall { object, args, .. } => {
            collect_variables_inner(object, vars, locals, seen);
            for arg in args {
                collect_variables_inner(arg, vars, locals, seen);
            }
        }
        NodeKind::If { condition, then_branch, elsif_branches, else_branch, .. } => {
            collect_variables_inner(condition, vars, locals, seen);
            collect_variables_inner(then_branch, vars, locals, seen);
            for (cond, branch) in elsif_branches {
                collect_variables_inner(cond, vars, locals, seen);
                collect_variables_inner(branch, vars, locals, seen);
            }
            if let Some(branch) = else_branch {
                collect_variables_inner(branch, vars, locals, seen);
            }
        }
        NodeKind::While { condition, body, .. } => {
            collect_variables_inner(condition, vars, locals, seen);
            collect_variables_inner(body, vars, locals, seen);
        }
        NodeKind::For { init, condition, update, body, .. } => {
            if let Some(init) = init {
                collect_variables_inner(init, vars, locals, seen);
            }
            if let Some(condition) = condition {
                collect_variables_inner(condition, vars, locals, seen);
            }
            if let Some(update) = update {
                collect_variables_inner(update, vars, locals, seen);
            }
            collect_variables_inner(body, vars, locals, seen);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::super::EnhancedCodeActionsProvider;
    use super::*;
    use perl_parser_core::Parser;
    use perl_tdd_support::{must, must_some};

    const TITLE: &str = "Extract to subroutine";

    fn loc(start: usize, end: usize) -> SourceLocation {
        SourceLocation { start, end }
    }

    fn parses_as_perl(source: &str) -> bool {
        let mut parser = Parser::new(source);
        parser.parse().is_ok()
    }

    fn var(name: &str, start: usize) -> Node {
        Node::new(
            NodeKind::Variable { sigil: "$".to_string(), name: name.to_string() },
            loc(start, start + name.len() + 1),
        )
    }

    fn block(statements: Vec<Node>, start: usize, end: usize) -> Node {
        Node::new(NodeKind::Block { statements }, loc(start, end))
    }

    /// The single extract-to-subroutine action offered for a whole-file range.
    ///
    /// A `sub` body is a control-flow body and is never offered, so a range over
    /// the whole file yields exactly the standalone bare block.
    fn extract_action(source: &str) -> Option<CodeAction> {
        let mut parser = Parser::new(source);
        let ast = must(parser.parse());
        let provider = EnhancedCodeActionsProvider::new(source.to_string());
        provider
            .get_enhanced_refactoring_actions(&ast, (0, source.len()))
            .into_iter()
            .find(|action| action.title == TITLE)
    }

    /// Apply an action's edits the way an LSP client would: descending by start
    /// offset, so an earlier edit's offsets stay valid while later ones land.
    fn apply(source: &str, action: &CodeAction) -> String {
        let mut changes: Vec<&TextEdit> = action.edit.changes.iter().collect();
        changes.sort_by_key(|change| std::cmp::Reverse(change.location.start));
        let mut out = source.to_string();
        for change in changes {
            out.replace_range(change.location.start..change.location.end, &change.new_text);
        }
        out
    }

    #[test]
    fn collect_variables_visits_if_and_while_children() {
        let if_node = Node::new(
            NodeKind::If {
                condition: Box::new(var("cond", 1)),
                then_branch: Box::new(block(vec![var("then_value", 10)], 9, 25)),
                elsif_branches: vec![(
                    Box::new(var("elsif_cond", 30)),
                    Box::new(block(vec![var("elsif_value", 44)], 43, 60)),
                )],
                else_branch: Some(Box::new(block(vec![var("else_value", 66)], 65, 80))),
                keyword: Some("unless".to_string()),
            },
            loc(0, 81),
        );
        let while_node = Node::new(
            NodeKind::While {
                condition: Box::new(var("loop_cond", 90)),
                body: Box::new(block(vec![var("loop_value", 106)], 105, 122)),
                continue_block: None,
                keyword: Some("until".to_string()),
            },
            loc(89, 123),
        );
        let root = block(vec![if_node, while_node], 0, 123);

        let mut vars = Vec::new();
        collect_variables(&root, &mut vars);

        // Exact order, not set equality: the generated signature depends on this
        // sequence being first-use and stable across runs.
        assert_eq!(
            vars,
            vec![
                "$cond",
                "$then_value",
                "$elsif_cond",
                "$elsif_value",
                "$else_value",
                "$loop_cond",
                "$loop_value",
            ]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>()
        );
    }

    /// The reported defect: capturing `$base` from the enclosing scope emitted
    /// `my (base) = @_;` and `process_data(base);`, neither of which compiles.
    #[test]
    fn captured_parameters_are_emitted_with_their_sigils() {
        let source = "use strict;\n\
                      use warnings;\n\
                      sub worker {\n\
                      \x20   my $base = 10;\n\
                      \x20   {\n\
                      \x20       my $x = $base * 2;\n\
                      \x20       $x + 1;\n\
                      \x20   }\n\
                      }\n\
                      print worker(), \"\\n\";\n";

        let action = must_some(extract_action(source));
        let edited = apply(source, &action);

        assert!(edited.contains("my ($base) = @_;"), "the signature lost its sigil:\n{edited}");
        assert!(edited.contains("process_data($base);"), "the call site lost its sigil:\n{edited}");
        assert!(
            !edited.contains("(base) = @_") && !edited.contains("process_data(base)"),
            "a bare name survived into the generated code:\n{edited}"
        );
        assert!(parses_as_perl(&edited), "the edit did not leave parseable Perl:\n{edited}");
    }

    /// A block whose last statement is a bare declared variable takes the
    /// implicit-return path, where the sigil was also lost (`my x = ...`).
    #[test]
    fn an_implicit_return_variable_keeps_its_sigil() {
        let source = "use strict;\n\
                      use warnings;\n\
                      sub worker {\n\
                      \x20   my $base = 10;\n\
                      \x20   {\n\
                      \x20       my $x = $base * 2;\n\
                      \x20       $x;\n\
                      \x20   }\n\
                      }\n\
                      print worker(), \"\\n\";\n";

        let action = must_some(extract_action(source));
        let edited = apply(source, &action);

        assert!(
            edited.contains("my $x = process_data($base);"),
            "the implicit-return assignment lost a sigil:\n{edited}"
        );
        assert!(parses_as_perl(&edited), "the edit did not leave parseable Perl:\n{edited}");
    }

    /// A captured array cannot travel through the generated `my (...) = @_;`
    /// signature: binding it as a scalar silently keeps only the first element
    /// of a list the caller passed whole. The action is withheld instead.
    #[test]
    fn an_action_is_withheld_when_a_capture_is_not_a_scalar() {
        let source = "use strict;\n\
                      use warnings;\n\
                      sub worker {\n\
                      \x20   my @items = (1, 2, 3);\n\
                      \x20   {\n\
                      \x20       my $n = @items;\n\
                      \x20       $n + 1;\n\
                      \x20   }\n\
                      }\n";

        assert!(
            extract_action(source).is_none(),
            "a non-scalar capture was offered as a refactor that would change semantics"
        );
    }

    /// Why this generator has no parse-based validity gate.
    ///
    /// The obvious defence — parse the generated subroutine and withhold the
    /// action when it fails — does not work here. This crate's own parser
    /// accepts the sigil-less `my (base) = @_;` that `perl -c` rejects, which
    /// is the same blind spot that made `perllsp --check` report the corrupted
    /// file as clean. A parse gate would have published the corrupting edit
    /// unchanged while appearing to validate it.
    ///
    /// The sigil invariant is therefore structural, not validated after the
    /// fact: captured parameters are sigil-bearing tokens, and `detect_parameters`
    /// refuses to produce any that is not a `$` scalar. The assertions above pin
    /// the emitted text directly for that reason. This test pins the parser
    /// limitation so the day it changes, a real gate becomes available again.
    #[test]
    fn the_crate_parser_cannot_serve_as_the_validity_oracle_here() {
        assert!(
            parses_as_perl("sub process_data {\n    my (base) = @_;\n}\n"),
            "the parser now rejects the sigil-less signature; a parse-based gate is viable again"
        );
    }

    /// `HashSet` iteration order varies per instance, so an unordered
    /// parameter list would reshuffle between otherwise identical requests.
    #[test]
    fn parameter_order_is_first_use_and_stable_across_requests() {
        let source = "use strict;\n\
                      use warnings;\n\
                      sub worker {\n\
                      \x20   my ($a, $b, $c) = @_;\n\
                      \x20   {\n\
                      \x20       my $t = $c + $a + $b;\n\
                      \x20       $t + 1;\n\
                      \x20   }\n\
                      }\n";

        let first = apply(source, &must_some(extract_action(source)));
        let second = apply(source, &must_some(extract_action(source)));

        assert_eq!(first, second, "identical requests produced different edits");
        assert!(
            first.contains("my ($c, $a, $b) = @_;"),
            "parameters did not keep first-use order:\n{first}"
        );
        assert!(
            first.contains("process_data($c, $a, $b);"),
            "the call site did not match the signature order:\n{first}"
        );
    }
}
