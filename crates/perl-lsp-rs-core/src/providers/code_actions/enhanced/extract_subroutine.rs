//! Extract subroutine code action

use super::super::types::{CodeAction, CodeActionEdit, CodeActionKind};
use crate::providers::rename::TextEdit;
use perl_parser_core::ast::{Node, NodeKind, SourceLocation};
use std::collections::HashSet;

use super::helpers::Helpers;

/// Create extract subroutine action.
///
/// Returns `None` unless the block, scalar captures, lexical declarations,
/// insertion context, and emitted name fit this generator's narrow calling
/// convention. Unsupported control flow, writes, pragmas, and scope changes
/// are refused before an edit is offered.
pub fn create_extract_subroutine_action(
    node: &Node,
    source: &str,
    helpers: &Helpers<'_>,
) -> Option<CodeAction> {
    let body_text = source.get(node.location.start..node.location.end)?;
    if !admissible_block(node, true) {
        return None;
    }
    let insert_pos = helpers.find_subroutine_insert_position(node.location.start);
    let first_sub = source.find("sub ").unwrap_or(node.location.start);
    let strict_context = source
        .get(..node.location.start.min(insert_pos).min(first_sub))?
        .lines()
        .any(|line| line.trim() == "use strict;");
    // The text-only insertion helper cannot distinguish package boundaries or
    // trailing data from Perl source. Refuse those files until insertion has an
    // AST-owned position.
    if !strict_context
        || has_unsafe_scope_directive(source, node.location.start.min(insert_pos))
        || source.lines().any(|line| {
            let line = line.trim_start();
            line.starts_with("__DATA__") || line.starts_with("__END__")
        })
    {
        return None;
    }
    // Strip surrounding block braces to avoid double-brace corruption.
    // A Block node's location spans `{ ... }` inclusive; the generated sub
    // already adds its own `sub NAME {`, so the inner braces must be removed.
    let body_text = body_text.strip_prefix('{')?.strip_suffix('}')?;
    let sub_name = suggest_subroutine_name(source)?;
    // `None` when a capture cannot be passed through this calling convention.
    let params = detect_parameters(node)?;
    if params.iter().any(|param| {
        !has_simple_lexical_declaration(source, insert_pos, node.location.start, param)
    }) {
        return None;
    }
    let newline = if source.contains("\r\n") { "\r\n" } else { "\n" };

    // Generate function signature
    let signature = if params.is_empty() {
        format!("sub {sub_name} {{{newline}")
    } else {
        format!("sub {sub_name} {{{newline}    my ({}) = @_;{newline}", params.join(", "))
    };

    // Find insertion position (before current sub or at end)
    if insert_pos != source.len() {
        let line_start = source[..insert_pos].rfind('\n').map_or(0, |idx| idx + 1);
        if !source[line_start..insert_pos].trim().is_empty()
            || !source[insert_pos..].starts_with("sub ")
        {
            return None;
        }
    }

    // Generate function call
    // Preserve the original lexical scope. A caller-side `my $x = ...` can
    // shadow an enclosing `$x`, while a bare call cannot.
    let call = format!("{{{newline}    {sub_name}({});{newline}}}", params.join(", "));

    Some(CodeAction {
        title: "Extract to subroutine".to_string(),
        kind: CodeActionKind::RefactorExtract,
        diagnostics: Vec::new(),
        edit: CodeActionEdit {
            changes: vec![
                // Insert function definition
                TextEdit {
                    location: SourceLocation { start: insert_pos, end: insert_pos },
                    new_text: format!("{signature}{body_text}{newline}}}{newline}{newline}"),
                },
                // Replace block with function call
                TextEdit { location: node.location, new_text: call },
            ],
        },
        is_preferred: false,
    })
}

fn has_unsafe_scope_directive(source: &str, allowed_end: usize) -> bool {
    let Some(allowed_prefix) = source.get(..allowed_end) else { return true };
    let Some(remainder) = source.get(allowed_end..) else { return true };
    // Only pragmas preceding the insertion point apply equally to the new
    // subroutine and the original block. A later `use warnings;`, for example,
    // changes the original block's lexical behavior but not the extracted sub.
    let remaining = format!(
        "{}{}",
        allowed_prefix.replace("use strict;", "").replace("use warnings;", ""),
        remainder
    );
    ["use", "no", "package", "our", "state", "local"].into_iter().any(|keyword| {
        remaining.match_indices(keyword).any(|(start, _)| {
            let before = remaining[..start].chars().next_back();
            let after = remaining[start + keyword.len()..].chars().next();
            before.is_none_or(|ch| !ch.is_ascii_alphanumeric() && ch != '_')
                && after.is_some_and(char::is_whitespace)
        })
    })
}

/// Suggest a subroutine name
pub fn suggest_subroutine_name(source: &str) -> Option<String> {
    for suffix in 1..=1000 {
        let name =
            if suffix == 1 { "process_data".to_string() } else { format!("process_data_{suffix}") };
        if !source.match_indices(&name).any(|(start, _)| {
            let before = source[..start].chars().next_back();
            let after = source[start + name.len()..].chars().next();
            before.is_none_or(|ch| !ch.is_ascii_alphanumeric() && ch != '_')
                && after.is_none_or(|ch| !ch.is_ascii_alphanumeric() && ch != '_')
        }) {
            return Some(name);
        }
    }
    None
}

/// True when `token` names a plain scalar variable such as `$base`.
fn is_scalar_variable(token: &str) -> bool {
    token.strip_prefix('$').is_some_and(|name| {
        let mut chars = name.chars();
        chars.next().is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
            && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    })
}

fn has_simple_lexical_declaration(
    source: &str,
    scope_start: usize,
    before: usize,
    token: &str,
) -> bool {
    let Some(prefix) = source.get(scope_start..before) else { return false };
    let needle = format!("my {token}");
    prefix.match_indices(&needle).any(|(start, _)| {
        let preceding = prefix[..start].chars().next_back();
        let line_start = prefix[..start].rfind('\n').map_or(0, |idx| idx + 1);
        let in_comment = prefix[line_start..start].contains('#');
        !in_comment
            && preceding.is_none_or(|ch| !ch.is_ascii_alphanumeric() && ch != '_')
            && prefix[start + needle.len()..]
                .chars()
                .next()
                .is_none_or(|ch| !ch.is_ascii_alphanumeric() && ch != '_')
    })
}

/// Admit only syntax for which the capture walk and call-site replacement have
/// a known meaning. In particular, assignment, control transfer, implicit
/// caller bindings, and forms with hidden child reads cannot be moved safely.
fn admissible_block(node: &Node, root: bool) -> bool {
    match &node.kind {
        NodeKind::Block { .. } if root => {}
        NodeKind::ExpressionStatement { .. } => {}
        NodeKind::VariableDeclaration { declarator, attributes, .. }
            if declarator == "my" && attributes.is_empty() => {}
        NodeKind::Variable { sigil, name } => {
            if !is_scalar_variable(&format!("{sigil}{name}")) {
                return false;
            }
        }
        NodeKind::Binary { op, .. }
            if matches!(
                op.as_str(),
                "+" | "-" | "*" | "/" | "." | "==" | "!=" | "<" | ">" | "<=" | ">="
            ) => {}
        NodeKind::Number { .. } | NodeKind::Undef => {}
        NodeKind::String { value, .. } if !value.contains('$') && !value.contains('@') => {}
        _ => return false,
    }
    node.children().into_iter().all(|child| admissible_block(child, false))
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
        NodeKind::Variable { sigil, name } if !locals.contains(&format!("{sigil}{name}")) => {
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
            if let NodeKind::Variable { sigil, name } = &variable.kind {
                locals.insert(format!("{sigil}{name}"));
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
                    locals.insert(format!("{sigil}{name}"));
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
    use std::io::Write;
    use std::process::{Command, Stdio};

    const TITLE: &str = "Extract to subroutine";

    fn loc(start: usize, end: usize) -> SourceLocation {
        SourceLocation { start, end }
    }

    fn parses_as_perl(source: &str) -> bool {
        let mut parser = Parser::new(source);
        parser.parse().is_ok()
    }

    fn perl_compiles(source: &str) -> bool {
        let mut child = must(
            Command::new("perl")
                .args(["-c", "-"])
                .stdin(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn(),
        );
        if let Some(mut stdin) = child.stdin.take() {
            must(stdin.write_all(source.as_bytes()));
        }
        must(child.wait_with_output()).status.success()
    }

    fn perl_stdout(source: &str) -> Vec<u8> {
        let mut child = must(
            Command::new("perl")
                .arg("-")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn(),
        );
        if let Some(mut stdin) = child.stdin.take() {
            must(stdin.write_all(source.as_bytes()));
        }
        let output = must(child.wait_with_output());
        assert!(
            output.status.success(),
            "Perl execution failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
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
        assert!(perl_compiles(&edited), "the applied edit did not compile with Perl:\n{edited}");
    }

    /// The call remains inside the original braces, and the selected block's
    /// final value still reaches the enclosing subroutine's return context.
    #[test]
    fn block_scope_and_final_value_are_preserved() {
        let source = "use strict;\n\
                      use warnings;\n\
                      sub worker {\n\
                      \x20   my $base = 10;\n\
                      \x20   my $x = 7;\n\
                      \x20   {\n\
                      \x20       my $x = $base * 2;\n\
                      \x20       $x;\n\
                      \x20   }\n\
                      }\n\
                      print worker(), \"\\n\";\n";

        let action = must_some(extract_action(source));
        let edited = apply(source, &action);

        assert!(edited.contains("process_data($base);"), "the call lost a sigil:\n{edited}");
        assert!(!edited.contains("my $x = process_data"), "a local escaped its block:\n{edited}");
        assert!(perl_compiles(&edited), "the applied edit did not compile with Perl:\n{edited}");
        let before = perl_stdout(source);
        let after = perl_stdout(&edited);
        assert_eq!(String::from_utf8_lossy(&before).trim(), "20");
        assert_eq!(after, before, "the edit changed the final value");
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
        assert!(!perl_compiles("sub process_data {\n    my (base) = @_;\n}\n"));
    }

    #[test]
    fn action_is_withheld_for_unsupported_capture_and_control_flow() {
        for body in [
            "print $Foo::bar;",
            "print $_;",
            "print $1;",
            "my $items = 3; print scalar @items;",
            "$base = $base + 1;",
            "return $base;",
            "my $x = $base[0];",
            "my $x = -$base; $x;",
        ] {
            let source = format!(
                "use strict;\nuse warnings;\nsub worker {{ my $base = 1; my @items = (1, 2); {{ {body} }} }}\n"
            );
            assert!(extract_action(&source).is_none(), "unsafe body was offered: {body}");
        }
    }

    #[test]
    fn name_collision_uses_stable_unused_suffix() {
        let source = "use strict;\nsub process_data { 1 }\nsub worker { my $base = 2; { my $x = $base + 1; $x; } }\n";
        let edited = apply(source, &must_some(extract_action(source)));
        assert!(edited.contains("sub process_data_2 {"), "name collision survived:\n{edited}");
        assert!(perl_compiles(&edited), "renamed action did not compile:\n{edited}");
    }

    #[test]
    fn action_is_withheld_without_simple_lexical_ownership_or_matching_pragmas() {
        for source in [
            "use strict;\nour $base = 2;\nsub worker { { my $x = $base + 1; $x; } }\n",
            "use strict;\nsub worker { our $base = 2; { my $x = $base + 1; $x; } }\n",
            "use strict;\nmy $base = 2;\nour\t$base;\nsub worker { { my $x = $base + 1; $x; } }\n",
            "sub worker { my $base = 2; { my $x = $base + 1; $x; } }\nuse strict;\n",
            "sub earlier {\n  use strict;\n  1;\n}\nsub worker { my $base = 2; { my $x = $base + 1; $x; } }\n",
            "use strict;\nsub unrelated { my $base = 2; }\nsub worker { { my $x = $base + 1; $x; } }\n",
            "use strict;\nsub worker { use integer; my $base = 5; { my $x = $base / 2; $x; } }\n",
            "use strict;\nsub worker {\n  use\tinteger;\n  my $base = 5;\n  { my $x = $base / 2; $x; }\n}\nprint worker(), \"\\n\";\n",
            "use strict;\nsub worker { use warnings; my $base; { my $x = $base + 1; $x; } }\nprint worker(), \"\\n\";\n",
            "use strict;\nsub worker {\n  package\tFoo;\n  my $base = 5;\n  { my $x = $base / 2; $x; }\n}\n",
            "use strict;\nsub worker { my $base = 2; no strict; { my $x = $base + 1; $x; } }\n",
        ] {
            assert!(
                extract_action(source).is_none(),
                "ambiguous lexical context was offered: {source}"
            );
        }
        assert!(has_unsafe_scope_directive("use\ninteger;", 0));
        assert!(has_unsafe_scope_directive("package\tFoo;", 0));
        assert!(has_unsafe_scope_directive("our\t$base;", 0));
    }

    /// `HashSet` iteration order varies per instance, so an unordered
    /// parameter list would reshuffle between otherwise identical requests.
    #[test]
    fn parameter_order_is_first_use_and_stable_across_requests() {
        let source = "use strict;\n\
                      use warnings;\n\
                      sub worker {\n\
                      \x20   my $a = 1;\n\
                      \x20   my $b = 2;\n\
                      \x20   my $c = 3;\n\
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
