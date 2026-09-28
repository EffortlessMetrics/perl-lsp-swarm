//! Extract subroutine code action

use super::super::types::{CodeAction, CodeActionEdit, CodeActionKind};
use crate::providers::rename::TextEdit;
use perl_parser_core::Parser;
use perl_parser_core::ast::{Node, NodeKind, SourceLocation};
use std::collections::{HashMap, HashSet};

use super::helpers::Helpers;

/// A variable reference carrying its sigil, bare name, and first-use position.
///
/// The sigil must survive detection so the generator can emit `$name` / `@name`
/// / `%name` at both the signature line and the call site; dropping it produced
/// invalid Perl such as `my (base) = @_;` (#16642).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VariableRef {
    /// Sigil of the variable (`$`, `@`, or `%`).
    pub sigil: String,
    /// Bare name without the sigil (e.g. `base` for `$base`).
    pub name: String,
    /// Byte offset of the variable's first observed use in the source.
    pub position: usize,
}

impl VariableRef {
    /// Render the variable as it is spelled in source (sigil + name).
    pub fn spelled(&self) -> String {
        format!("{}{}", self.sigil, self.name)
    }
}

/// Create extract subroutine action.
///
/// Returns `None` when the generated edit would not parse: a refactoring whose
/// result is invalid Perl must never be surfaced as an offer (#16642 validity
/// gate).
pub fn create_extract_subroutine_action(
    node: &Node,
    source: &str,
    helpers: &Helpers<'_>,
) -> Option<CodeAction> {
    let body_text = &source[node.location.start..node.location.end];
    // Strip exactly the block's own surrounding braces to avoid double-brace
    // corruption. A Block node's location spans `{ ... }` inclusive; the
    // generated sub already adds its own `sub NAME {`, so the outer pair must
    // be removed. Only one brace is stripped per side — repeated stripping
    // would also unwrap an inner leading/trailing block and silently change
    // the extracted semantics.
    let body_text = body_text.strip_prefix('{').unwrap_or(body_text);
    let body_text = body_text.strip_suffix('}').unwrap_or(body_text);
    let sub_name = suggest_subroutine_name(node);
    let params = detect_parameters(node);
    let returns = detect_return_values(node);

    let param_list = join_spelled(&params);

    // Generate function signature
    let signature = if params.is_empty() {
        format!("sub {} {{\n", sub_name)
    } else {
        format!("sub {} {{\n    my ({}) = @_;\n", sub_name, param_list)
    };

    // Find insertion position (before current sub or at end)
    let insert_pos = helpers.find_subroutine_insert_position(node.location.start);

    // Generate function call
    let call = if returns.is_empty() {
        format!("{}({});", sub_name, param_list)
    } else {
        format!("my {} = {}({});", join_spelled(&returns), sub_name, param_list)
    };

    let insert_text = format!("{}{}\n}}\n\n", signature, body_text);

    // Validity gate: apply both edits to the source and require the result to
    // parse cleanly before the action is offered at all.
    let generated =
        splice_generated_source(source, node.location, insert_pos, &insert_text, &call)?;
    if !parses_cleanly(&generated) {
        return None;
    }

    Some(CodeAction {
        title: "Extract to subroutine".to_string(),
        kind: CodeActionKind::RefactorExtract,
        diagnostics: Vec::new(),
        edit: CodeActionEdit {
            changes: vec![
                // Insert function definition
                TextEdit {
                    location: SourceLocation { start: insert_pos, end: insert_pos },
                    new_text: insert_text,
                },
                // Replace block with function call
                TextEdit { location: node.location, new_text: call },
            ],
        },
        is_preferred: false,
    })
}

/// Splice the extract-subroutine edit pair into `source`, producing the program
/// text the editor would end up with. Returns `None` when the edit geometry is
/// out of bounds or lands on a non-char-boundary.
fn splice_generated_source(
    source: &str,
    replace: SourceLocation,
    insert_pos: usize,
    insert_text: &str,
    call: &str,
) -> Option<String> {
    if replace.start > replace.end || replace.end > source.len() || insert_pos > source.len() {
        return None;
    }
    let mut out = String::with_capacity(source.len() + insert_text.len() + call.len());
    if insert_pos >= replace.end {
        // Replacement site comes first in the source; splice from the back.
        out.push_str(source.get(..replace.start)?);
        out.push_str(call);
        out.push_str(source.get(replace.end..insert_pos)?);
        out.push_str(insert_text);
        out.push_str(source.get(insert_pos..)?);
    } else {
        out.push_str(source.get(..insert_pos)?);
        out.push_str(insert_text);
        out.push_str(source.get(insert_pos..replace.start)?);
        out.push_str(call);
        out.push_str(source.get(replace.end..)?);
    }
    Some(out)
}

/// Parse `source` with the crate parser and report whether it is free of
/// syntax errors. Recovery parsing returns `Ok` even for erroneous input, so
/// the retained error list — not just the `Result` — is the validity predicate.
fn parses_cleanly(source: &str) -> bool {
    let mut parser = Parser::new(source);
    parser.parse().is_ok() && parser.errors().is_empty()
}

/// Join variables as their source spellings (`$a, @b, %c`).
fn join_spelled(vars: &[VariableRef]) -> String {
    vars.iter().map(VariableRef::spelled).collect::<Vec<_>>().join(", ")
}

/// Suggest a subroutine name
pub fn suggest_subroutine_name(_node: &Node) -> String {
    // Could analyze the code to suggest better names
    "process_data".to_string()
}

/// Detect the free variables used by a block, in first-use source order.
///
/// Variables declared inside the block are local to the extracted subroutine
/// and are not parameters. Sigils are preserved so the generator can emit
/// `$name` / `@name` / `%name` correctly (#16642).
pub fn detect_parameters(node: &Node) -> Vec<VariableRef> {
    let mut params: Vec<VariableRef> = Vec::new();
    collect_variables(node, &mut params);
    // Deterministic emission order: first use in the source, with the bare
    // name as a stable tiebreak — never HashSet iteration order.
    params.sort_by(|a, b| a.position.cmp(&b.position).then_with(|| a.name.cmp(&b.name)));
    params
}

/// Detect return values in a block.
///
/// Uses a heuristic: if the last non-empty statement in the block is a bare
/// variable expression (implicit return convention in Perl), and that variable
/// was declared inside the block, treat it as the return value.
///
/// Returns the return variables with their sigils (e.g. `$result`), so the
/// generated `my $result = ...;` call site is valid Perl (#16642).
pub fn detect_return_values(node: &Node) -> Vec<VariableRef> {
    let statements = match &node.kind {
        NodeKind::Block { statements } => statements.as_slice(),
        _ => return Vec::new(),
    };

    // Collect the spelled names (sigil + name) of all variables declared
    // inside the block.
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
        && let Some(var) = extract_last_expression_variable(last_stmt)
        && declared_inside.contains(&var.spelled())
    {
        return vec![var];
    }

    Vec::new()
}

/// Collect the spelled names (sigil + name) of all variables declared via a
/// declarator in a node.
fn collect_declared_variables(node: &Node, declared: &mut HashSet<String>) {
    match &node.kind {
        NodeKind::VariableDeclaration { variable, .. } => {
            if let NodeKind::Variable { sigil, name } = &variable.kind {
                declared.insert(format!("{}{}", sigil, name));
            }
        }
        NodeKind::VariableListDeclaration { variables, .. } => {
            for v in variables {
                if let NodeKind::Variable { sigil, name } = &v.kind {
                    declared.insert(format!("{}{}", sigil, name));
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

/// If a statement is a bare variable expression or an explicit return, return
/// that variable with its sigil. Used to detect the implicit return value.
fn extract_last_expression_variable(node: &Node) -> Option<VariableRef> {
    match &node.kind {
        NodeKind::ExpressionStatement { expression } => {
            extract_last_expression_variable(expression)
        }
        NodeKind::Variable { sigil, name } => Some(VariableRef {
            sigil: sigil.clone(),
            name: name.clone(),
            position: node.location.start,
        }),
        NodeKind::Return { value } => {
            value.as_ref().and_then(|v| extract_last_expression_variable(v))
        }
        _ => None,
    }
}

/// Collect the variables used in a node, excluding those declared within it.
///
/// Each distinct (sigil, name) pair is appended once, at its first observed
/// use; callers order the result deterministically. Variables declared inside
/// the node are locals of the extracted code, not parameters.
pub fn collect_variables(node: &Node, vars: &mut Vec<VariableRef>) {
    let mut seen: HashMap<String, usize> = HashMap::new();
    collect_variables_inner(node, vars, &mut HashSet::new(), &mut seen);
}

fn collect_variables_inner(
    node: &Node,
    vars: &mut Vec<VariableRef>,
    locals: &mut HashSet<String>,
    seen: &mut HashMap<String, usize>,
) {
    match &node.kind {
        NodeKind::Variable { sigil, name } => {
            let spelled = format!("{}{}", sigil, name);
            if locals.contains(&spelled) || seen.contains_key(&spelled) {
                return;
            }
            seen.insert(spelled, vars.len());
            vars.push(VariableRef {
                sigil: sigil.clone(),
                name: name.clone(),
                position: node.location.start,
            });
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
            if let NodeKind::Variable { sigil, name } = &variable.kind {
                locals.insert(format!("{}{}", sigil, name));
            }
        }
        NodeKind::VariableListDeclaration { variables, initializer, .. } => {
            // Initializer may reference outer variables.
            if let Some(init) = initializer {
                collect_variables_inner(init, vars, locals, seen);
            }
            // All declared variables are local to the block.
            for v in variables {
                if let NodeKind::Variable { sigil, name } = &v.kind {
                    locals.insert(format!("{}{}", sigil, name));
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

    fn loc(start: usize, end: usize) -> SourceLocation {
        SourceLocation { start, end }
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

    fn source_lines(source: &str) -> Vec<String> {
        source.lines().map(str::to_string).collect()
    }

    /// Apply an action's edits to the source, back-to-front so earlier
    /// offsets stay valid. Test-only mirror of what an editor does.
    fn apply_changes(source: &str, changes: &[TextEdit]) -> String {
        let mut edits: Vec<&TextEdit> = changes.iter().collect();
        edits.sort_by(|a, b| {
            b.location
                .start
                .cmp(&a.location.start)
                .then_with(|| b.location.end.cmp(&a.location.end))
        });
        let mut out = source.to_string();
        for edit in edits {
            out.replace_range(edit.location.start..edit.location.end, &edit.new_text);
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

        let mut names: Vec<String> = vars.iter().map(VariableRef::spelled).collect();
        names.sort();
        let mut expected = [
            "$cond",
            "$then_value",
            "$elsif_cond",
            "$elsif_value",
            "$else_value",
            "$loop_cond",
            "$loop_value",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
        expected.sort();
        assert_eq!(names, expected);
    }

    /// #16642 researcher scenario (em_a): a bare block using the outer lexical
    /// `$base` must generate a signature line and call site that carry the
    /// sigil, and the applied result must parse under the crate parser.
    #[test]
    fn extract_bare_block_emits_sigiled_params_and_parses() {
        let source = concat!(
            "use strict;\n",
            "use warnings;\n",
            "sub worker {\n",
            "    my $base = 10;\n",
            "    {\n",
            "        my $x = $base * 2;\n",
            "        $x + 1;\n",
            "    }\n",
            "}\n",
            "print worker(), \"\\n\";\n",
        );
        let mut parser = Parser::new(source);
        let ast = must(parser.parse());

        let provider = EnhancedCodeActionsProvider::new(source.to_string());
        // Select the interior of the bare block; the extract action must be
        // offered for the block node overlapping the range.
        let actions = provider.get_enhanced_refactoring_actions(&ast, (66, 90));

        let action = must_some(actions.iter().find(|a| a.title == "Extract to subroutine"));
        assert_eq!(action.edit.changes.len(), 2, "insert + replace edits expected");

        let edited = apply_changes(source, &action.edit.changes);
        assert!(
            edited.contains("my ($base) = @_;"),
            "signature line must carry the sigil; got:\n{edited}"
        );
        assert!(
            edited.contains("process_data($base);"),
            "call site must carry the sigil; got:\n{edited}"
        );
        assert!(
            parses_cleanly(&edited),
            "the applied edit must parse cleanly under the crate parser; got:\n{edited}"
        );
    }

    /// #16642 researcher scenario (em_d): an implicit-return variable must
    /// produce a sigiled `my $x = process_data(...);` call site. Before the
    /// fix this emitted `my x = process_data(base);`, which even the crate
    /// parser rejects.
    #[test]
    fn extract_implicit_return_emits_sigiled_return_and_parses() {
        let source = concat!(
            "use strict;\n",
            "use warnings;\n",
            "sub calc {\n",
            "    my $base = 10;\n",
            "    {\n",
            "        my $x = $base * 2;\n",
            "        $x;\n",
            "    }\n",
            "}\n",
            "print calc(), \"\\n\";\n",
        );
        let mut parser = Parser::new(source);
        let ast = must(parser.parse());

        let provider = EnhancedCodeActionsProvider::new(source.to_string());
        let actions = provider.get_enhanced_refactoring_actions(&ast, (65, 86));

        let action = must_some(actions.iter().find(|a| a.title == "Extract to subroutine"));

        let edited = apply_changes(source, &action.edit.changes);
        assert!(
            edited.contains("my $x = process_data($base);"),
            "implicit return must be captured as a sigiled assignment; got:\n{edited}"
        );
        assert!(
            parses_cleanly(&edited),
            "the applied edit must parse cleanly under the crate parser; got:\n{edited}"
        );
    }

    /// The no-parameter implicit-return form: the generated call site is
    /// `my $x = process_data();` with an empty signature line.
    #[test]
    fn extract_no_param_implicit_return_parses() {
        let source = concat!(
            "use strict;\n",
            "use warnings;\n",
            "{\n",
            "    my $x = 42;\n",
            "    $x;\n",
            "}\n",
        );
        let mut parser = Parser::new(source);
        let ast = must(parser.parse());

        let provider = EnhancedCodeActionsProvider::new(source.to_string());
        let actions = provider.get_enhanced_refactoring_actions(&ast, (20, 45));

        let action = must_some(actions.iter().find(|a| a.title == "Extract to subroutine"));

        let edited = apply_changes(source, &action.edit.changes);
        assert!(
            edited.contains("sub process_data {\n"),
            "no parameters means no signature line; got:\n{edited}"
        );
        assert!(
            edited.contains("my $x = process_data();"),
            "implicit return with no params must call with an empty argument list; got:\n{edited}"
        );
        assert!(
            parses_cleanly(&edited),
            "the applied edit must parse cleanly under the crate parser; got:\n{edited}"
        );
    }

    /// Parameter order is deterministic and follows first use in the source —
    /// not alphabetical, not HashSet iteration order.
    #[test]
    fn parameter_order_is_source_position_order_and_stable() {
        let source = concat!(
            "use strict;\n",
            "use warnings;\n",
            "{\n",
            "    my $sum = $beta + $alpha;\n",
            "    $alpha * $sum;\n",
            "}\n",
        );

        let emitted = |source: &str| {
            let mut parser = Parser::new(source);
            let ast = must(parser.parse());
            let provider = EnhancedCodeActionsProvider::new(source.to_string());
            let actions = provider.get_enhanced_refactoring_actions(&ast, (22, 52));
            let action = must_some(actions.iter().find(|a| a.title == "Extract to subroutine"));
            action.edit.changes[1].new_text.clone()
        };

        let first = emitted(source);
        let second = emitted(source);
        assert_eq!(first, second, "two runs must emit identical call sites");
        assert_eq!(
            first, "process_data($beta, $alpha);",
            "parameters follow first-use source order ($beta before $alpha), got: {first}"
        );
    }

    /// The validity gate: the em_d corruption shape (`my x = ...`) must fail
    /// the crate-parse predicate while the repaired spelling passes.
    #[test]
    fn validity_gate_rejects_unparseable_generated_source() {
        assert!(
            !parses_cleanly("use strict;\nmy x = process_data(base);\n"),
            "the sigil-less `my x = ...` corruption must fail the gate"
        );
        assert!(
            parses_cleanly("use strict;\nmy $x = process_data($base);\n"),
            "the sigiled spelling must pass the gate"
        );
    }

    /// End-to-end suppression: when the generated call site would not parse,
    /// the action is not offered at all. A synthetic variable with an empty
    /// sigil reproduces the dropped-sigil emission (`my x = process_data();`)
    /// that the real parser can no longer produce.
    #[test]
    fn action_suppressed_when_generated_source_would_not_parse() {
        let source = "my $unused = 0;\n";
        let lines = source_lines(source);
        let helpers = Helpers::new(source, &lines);

        let decl_var = Node::new(
            NodeKind::Variable { sigil: String::new(), name: "x".to_string() },
            loc(4, 6),
        );
        let decl = Node::new(
            NodeKind::VariableDeclaration {
                declarator: "my".to_string(),
                variable: Box::new(decl_var),
                attributes: vec![],
                initializer: None,
            },
            loc(1, 6),
        );
        let last_use = Node::new(
            NodeKind::Variable { sigil: String::new(), name: "x".to_string() },
            loc(11, 13),
        );
        let last_stmt = Node::new(
            NodeKind::ExpressionStatement { expression: Box::new(last_use) },
            loc(11, 14),
        );
        let block_node = block(vec![decl, last_stmt], 0, 15);

        let action = create_extract_subroutine_action(&block_node, source, &helpers);
        assert!(
            action.is_none(),
            "an extraction whose generated source cannot parse must not be offered"
        );
    }

    /// Only the block's own braces are stripped: an inner leading block must
    /// survive extraction with its braces intact (repeated-match stripping
    /// used to unwrap it, silently changing the extracted semantics).
    #[test]
    fn brace_stripping_removes_only_the_outer_pair() {
        let source = concat!(
            "use strict;\n",
            "use warnings;\n",
            "{\n",
            "    {\n",
            "        my $x = 1;\n",
            "        $x;\n",
            "    }\n",
            "}\n",
        );
        let mut parser = Parser::new(source);
        let ast = must(parser.parse());

        let provider = EnhancedCodeActionsProvider::new(source.to_string());
        // Select the inner block's interior; the extract action is emitted for
        // the outer block node whose span covers the selection.
        let actions = provider.get_enhanced_refactoring_actions(&ast, (35, 50));

        let outer_action = actions
            .iter()
            .find(|a| a.title == "Extract to subroutine" && a.edit.changes[1].location.start == 26);
        let action = must_some(outer_action);

        let insert_text = &action.edit.changes[0].new_text;
        assert!(
            insert_text.contains("{\n        my $x = 1;"),
            "inner block braces must be retained in the extracted body; got:\n{insert_text}"
        );

        let edited = apply_changes(source, &action.edit.changes);
        assert!(
            parses_cleanly(&edited),
            "the applied edit must parse cleanly under the crate parser; got:\n{edited}"
        );
    }
}
