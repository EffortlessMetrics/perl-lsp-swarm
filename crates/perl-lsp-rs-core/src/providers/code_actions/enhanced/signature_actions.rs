//! Signature refactoring code actions.
//!
//! Provides the "Add parameter to signature" code action for Perl 5.20+
//! subroutine signatures. Finds all in-file call sites and generates
//! a workspace edit that updates both the signature and every call.
//!
//! Until canonical callable edit plans land (#8932 et al.), this action fails
//! closed on any shape whose safety cannot be proven from parser facts alone.

use super::super::types::{CodeAction, CodeActionEdit, CodeActionKind};
use crate::providers::rename::TextEdit;
use perl_parser_core::ast::{Node, NodeKind, SourceLocation};

/// Attempt to generate an "Add parameter to signature" code action.
///
/// Returns `None` when the action is not applicable or the edit shape is not
/// proven safe (fail closed).
pub fn add_parameter_action(source: &str, node: &Node, ast: &Node) -> Option<CodeAction> {
    let (sub_name, signature) = match &node.kind {
        NodeKind::Subroutine { name: Some(n), signature: Some(sig), .. } => (n.clone(), sig),
        _ => return None,
    };

    if !signature_admits_optional_positional_append(signature) {
        return None;
    }

    let package = package_for_subroutine(ast, node);
    let callable_identity = qualified_callable_name(&package, &sub_name);

    let sig_close_paren = find_signature_close_paren(source, node, signature)?;

    let new_param_text = ", $options = {}";
    let call_default_text = ", {}";

    let call_sites = collect_call_sites(source, ast, &callable_identity)?;

    let mut changes = Vec::with_capacity(1 + call_sites.len());

    changes.push(TextEdit {
        location: SourceLocation { start: sig_close_paren, end: sig_close_paren },
        new_text: new_param_text.to_string(),
    });

    for call_close_paren in call_sites {
        changes.push(TextEdit {
            location: SourceLocation { start: call_close_paren, end: call_close_paren },
            new_text: call_default_text.to_string(),
        });
    }

    Some(CodeAction {
        title: "Add parameter to signature".to_string(),
        kind: CodeActionKind::RefactorRewrite,
        diagnostics: Vec::new(),
        edit: CodeActionEdit { changes },
        is_preferred: false,
    })
}

/// Only positional signatures with at least one positional parameter and no
/// trailing slurpy admit the fixed optional positional append.
fn signature_admits_optional_positional_append(signature: &Node) -> bool {
    let NodeKind::Signature { parameters } = &signature.kind else {
        return false;
    };
    if parameters.is_empty() {
        return false;
    }
    if parameters.iter().any(|p| matches!(p.kind, NodeKind::NamedParameter { .. })) {
        return false;
    }
    if parameters.last().is_some_and(|p| matches!(p.kind, NodeKind::SlurpyParameter { .. })) {
        return false;
    }
    true
}

fn qualified_callable_name(package: &str, sub_name: &str) -> String {
    if sub_name.contains("::") {
        return sub_name.to_string();
    }
    if package == "main" { sub_name.to_string() } else { format!("{package}::{sub_name}") }
}

fn package_for_subroutine(ast: &Node, target: &Node) -> String {
    let mut current = "main".to_string();
    let mut found = "main".to_string();
    let target_start = target.location.start;
    walk_for_package(ast, &mut current, target_start, &mut found);
    found
}

fn walk_for_package(node: &Node, current: &mut String, target_start: usize, found: &mut String) {
    if node.location.start == target_start && matches!(node.kind, NodeKind::Subroutine { .. }) {
        *found = current.clone();
    }

    if let NodeKind::Package { name, block, .. } = &node.kind {
        if let Some(blk) = block {
            let saved = current.clone();
            *current = name.clone();
            walk_for_package(blk, current, target_start, found);
            *current = saved;
            return;
        }
        *current = name.clone();
    }

    visit_children(node, |child| walk_for_package(child, current, target_start, found));
}

fn find_signature_close_paren(
    source: &str,
    subroutine_node: &Node,
    signature: &Node,
) -> Option<usize> {
    let sub_start = subroutine_node.location.start;
    let sub_end = subroutine_node.location.end.min(source.len());

    let body_open = source[sub_start..sub_end].find('{').map(|p| sub_start + p)?;

    let search_from = signature.location.start.max(sub_start);
    let sig_open = find_next_significant_byte(source, search_from, body_open, b'(')?;

    find_matching_close_paren(source, sig_open, body_open)
}

fn collect_call_sites(source: &str, ast: &Node, callable_identity: &str) -> Option<Vec<usize>> {
    let mut current = "main".to_string();
    let mut sites = Vec::new();
    if !collect_calls_in_scope(source, ast, &mut current, callable_identity, &mut sites) {
        return None;
    }
    Some(sites)
}

fn collect_calls_in_scope(
    source: &str,
    node: &Node,
    current_package: &mut String,
    callable_identity: &str,
    out: &mut Vec<usize>,
) -> bool {
    if let NodeKind::FunctionCall { name, args } = &node.kind
        && call_matches_callable(name, current_package, callable_identity)
    {
        if call_has_named_arguments(args) || call_source_has_named_pair(source, node) {
            return false;
        }
        let Some(close) = find_call_close_paren(source, node) else {
            return false;
        };
        out.push(close);
    }

    if let NodeKind::Package { name, block, .. } = &node.kind {
        if let Some(blk) = block {
            let saved = current_package.clone();
            *current_package = name.clone();
            let ok = collect_calls_in_scope(source, blk, current_package, callable_identity, out);
            *current_package = saved;
            return ok;
        }
        *current_package = name.clone();
    }

    let mut ok = true;
    visit_children(node, |child| {
        ok &= collect_calls_in_scope(source, child, current_package, callable_identity, out);
    });
    ok
}

fn call_matches_callable(call_name: &str, call_package: &str, callable_identity: &str) -> bool {
    if call_name == callable_identity {
        return true;
    }
    if call_name.contains("::") {
        return false;
    }
    qualified_callable_name(call_package, call_name) == callable_identity
}

fn call_source_has_named_pair(source: &str, call_node: &Node) -> bool {
    let start = call_node.location.start;
    let end = call_node.location.end.min(source.len());
    let Some(open) = find_next_significant_byte(source, start, end, b'(') else {
        return true;
    };
    let Some(close) = find_matching_close_paren(source, open, end) else {
        return true;
    };
    region_has_fat_arrow_outside_literals(source, open + 1, close)
}

fn region_has_fat_arrow_outside_literals(source: &str, from: usize, to: usize) -> bool {
    let mut i = from;
    while i < to {
        i = skip_insignificant(source, i, to).unwrap_or(to);
        if i >= to {
            break;
        }
        if source.as_bytes()[i] == b'=' && i + 1 < to && source.as_bytes()[i + 1] == b'>' {
            return true;
        }
        i += 1;
    }
    false
}

fn call_has_named_arguments(args: &[Node]) -> bool {
    args.iter().any(expression_has_fat_arrow_pair)
}

fn expression_has_fat_arrow_pair(node: &Node) -> bool {
    if let NodeKind::Binary { op, .. } = &node.kind
        && op == "=>"
    {
        return true;
    }
    match &node.kind {
        NodeKind::Binary { left, right, .. } => {
            expression_has_fat_arrow_pair(left) || expression_has_fat_arrow_pair(right)
        }
        NodeKind::Unary { operand, .. } => expression_has_fat_arrow_pair(operand),
        NodeKind::FunctionCall { args, .. } => args.iter().any(expression_has_fat_arrow_pair),
        _ => false,
    }
}

fn find_call_close_paren(source: &str, call_node: &Node) -> Option<usize> {
    let start = call_node.location.start;
    let end = call_node.location.end.min(source.len());
    let open = find_next_significant_byte(source, start, end, b'(')?;
    find_matching_close_paren(source, open, end)
}

fn find_next_significant_byte(source: &str, from: usize, limit: usize, want: u8) -> Option<usize> {
    let mut i = from;
    while i < limit {
        i = skip_insignificant(source, i, limit)?;
        if i >= limit {
            break;
        }
        if source.as_bytes()[i] == want {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn find_matching_close_paren(source: &str, open: usize, limit: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    if open >= limit || bytes[open] != b'(' {
        return None;
    }
    let mut depth = 0usize;
    let mut i = open;
    while i < limit {
        i = skip_insignificant(source, i, limit)?;
        if i >= limit {
            break;
        }
        match bytes[i] {
            b'(' => depth += 1,
            b')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn skip_insignificant(source: &str, mut i: usize, limit: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    while i < limit {
        match bytes[i] {
            b' ' | b'\t' | b'\r' | b'\n' => {
                i += 1;
            }
            b'#' => {
                while i < limit && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'\'' => {
                i += 1;
                while i < limit && bytes[i] != b'\'' {
                    i += 1;
                }
                if i < limit {
                    i += 1;
                }
            }
            b'"' => {
                i += 1;
                while i < limit && bytes[i] != b'"' {
                    if bytes[i] == b'\\' && i + 1 < limit {
                        i += 1;
                    }
                    i += 1;
                }
                if i < limit {
                    i += 1;
                }
            }
            _ => return Some(i),
        }
    }
    Some(i)
}

fn visit_children<F>(node: &Node, mut f: F)
where
    F: FnMut(&Node),
{
    match &node.kind {
        NodeKind::Program { statements } => {
            for s in statements {
                f(s);
            }
        }
        NodeKind::Block { statements } => {
            for s in statements {
                f(s);
            }
        }
        NodeKind::Package { .. } => {}
        NodeKind::ExpressionStatement { expression } => f(expression),
        NodeKind::VariableDeclaration { variable, initializer, .. } => {
            f(variable);
            if let Some(init) = initializer {
                f(init);
            }
        }
        NodeKind::Assignment { lhs, rhs, .. } => {
            f(lhs);
            f(rhs);
        }
        NodeKind::Binary { left, right, .. } => {
            f(left);
            f(right);
        }
        NodeKind::Unary { operand, .. } => f(operand),
        NodeKind::FunctionCall { args, .. } => {
            for a in args {
                f(a);
            }
        }
        NodeKind::MethodCall { object, args, .. } => {
            f(object);
            for a in args {
                f(a);
            }
        }
        NodeKind::Return { value } => {
            value.as_deref().into_iter().for_each(&mut f);
        }
        NodeKind::If { condition, then_branch, elsif_branches, else_branch, .. } => {
            f(condition);
            f(then_branch);
            for (cond, branch) in elsif_branches {
                f(cond);
                f(branch);
            }
            if let Some(b) = else_branch {
                f(b);
            }
        }
        NodeKind::While { condition, body, .. } => {
            f(condition);
            f(body);
        }
        NodeKind::For { init, condition, update, body, .. } => {
            if let Some(init) = init {
                f(init);
            }
            if let Some(cond) = condition {
                f(cond);
            }
            if let Some(upd) = update {
                f(upd);
            }
            f(body);
        }
        NodeKind::Foreach { variable, list, body, continue_block } => {
            f(variable);
            f(list);
            f(body);
            if let Some(cb) = continue_block {
                f(cb);
            }
        }
        NodeKind::Subroutine { body, .. } => {
            f(body);
        }
        NodeKind::Ternary { condition, then_expr, else_expr } => {
            f(condition);
            f(then_expr);
            f(else_expr);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_parser_core::Parser;
    use perl_test_must::{must, must_some_with};

    fn parse(source: &str) -> Node {
        let mut parser = Parser::new(source);
        must(parser.parse())
    }

    fn loc(start: usize, end: usize) -> SourceLocation {
        SourceLocation { start, end }
    }

    fn ident(name: &str, start: usize) -> Node {
        Node::new(NodeKind::Identifier { name: name.to_string() }, loc(start, start + name.len()))
    }

    fn first_sub_start(ast: &Node, name: &str) -> Option<usize> {
        let mut found = None;
        walk_all(ast, &mut |node| {
            if found.is_none()
                && matches!(&node.kind, NodeKind::Subroutine { name: Some(n), .. } if n == name)
            {
                found = Some(node.location.start);
            }
        });
        found
    }

    fn with_first_sub_named(source: &str, name: &str, mut test: impl FnMut(&Node, &Node)) {
        let ast = parse(source);
        let start = must_some_with(
            first_sub_start(&ast, name),
            format!("expected a subroutine named {name}"),
        );
        walk_all(&ast, &mut |node| {
            if node.location.start == start && matches!(node.kind, NodeKind::Subroutine { .. }) {
                test(node, &ast);
            }
        });
    }

    fn walk_all(node: &Node, visit: &mut impl FnMut(&Node)) {
        visit(node);
        visit_children(node, |child| walk_all(child, visit));
    }

    fn edit_insertions(action: &CodeAction) -> Vec<(usize, String)> {
        action
            .edit
            .changes
            .iter()
            .map(|edit| (edit.location.start, edit.new_text.clone()))
            .collect()
    }

    #[test]
    fn visit_children_walks_if_branches_with_keyword_metadata() {
        let node = Node::new(
            NodeKind::If {
                condition: Box::new(ident("cond", 1)),
                then_branch: Box::new(ident("then_branch", 7)),
                elsif_branches: vec![(
                    Box::new(ident("elsif_cond", 20)),
                    Box::new(ident("elsif_branch", 32)),
                )],
                else_branch: Some(Box::new(ident("else_branch", 46))),
                keyword: Some("unless".to_string()),
            },
            loc(0, 57),
        );
        let mut names = Vec::new();

        visit_children(&node, |child| {
            if let NodeKind::Identifier { name } = &child.kind {
                names.push(name.clone());
            }
        });

        assert_eq!(names, vec!["cond", "then_branch", "elsif_cond", "elsif_branch", "else_branch"]);
    }

    #[test]
    fn refuses_empty_signature_and_call() {
        let source = "sub f () { }\nf();\n";
        with_first_sub_named(source, "f", |sub, ast| {
            assert!(add_parameter_action(source, sub, ast).is_none());
        });
    }

    #[test]
    fn refuses_named_signature_parameters() {
        let source = "sub f (:$x) { }\nf(x => 1);\n";
        with_first_sub_named(source, "f", |sub, ast| {
            assert!(add_parameter_action(source, sub, ast).is_none());
        });
    }

    #[test]
    fn refuses_named_call_arguments() {
        let source = "sub f ($x) { }\nf(x => 1);\n";
        with_first_sub_named(source, "f", |sub, ast| {
            assert!(add_parameter_action(source, sub, ast).is_none());
        });
    }

    #[test]
    fn qualified_homonyms_only_update_exact_callable() {
        let source = concat!(
            "package A;\n",
            "sub save ($x) { }\n",
            "package B;\n",
            "sub save ($x) { }\n",
            "A::save(1);\n",
            "B::save(2);\n",
        );
        with_first_sub_named(source, "save", |sub_a, ast| {
            let action = must_some_with(
                add_parameter_action(source, sub_a, ast),
                "safe positional save in A",
            );
            let edits = edit_insertions(&action);
            assert_eq!(edits.len(), 2, "signature plus one matching call site");
            let updated = apply_edits(source, &edits);
            assert!(updated.contains("A::save(1, {})"), "expected A call update: {updated}");
            assert!(updated.contains("B::save(2)"), "must not touch homonym: {updated}");
        });
    }

    #[test]
    fn signature_close_paren_skips_string_and_comment_parens() {
        let source = concat!(
            "sub tricky ($x) { my $s = \")\"; # ) comment\n",
            "    return 1; }\n",
            "tricky(1);\n",
        );
        with_first_sub_named(source, "tricky", |sub, ast| {
            let action =
                must_some_with(add_parameter_action(source, sub, ast), "positional signature");
            let updated = apply_edits(source, &edit_insertions(&action));
            assert!(
                updated.contains("sub tricky ($x, $options = {})"),
                "signature edit must target real close paren: {updated}"
            );
        });
    }

    #[test]
    fn refuses_trailing_slurpy_signature() {
        let source = "sub f ($x, @rest) { }\nf(1, 2);\n";
        with_first_sub_named(source, "f", |sub, ast| {
            assert!(add_parameter_action(source, sub, ast).is_none());
        });
    }

    #[test]
    fn package_local_bare_calls_receive_matching_argument_insert() {
        let source = "package A;\nsub greet ($name) { }\ngreet('hi');\n";
        with_first_sub_named(source, "greet", |sub, ast| {
            let action =
                must_some_with(add_parameter_action(source, sub, ast), "package-local greet");
            let updated = apply_edits(source, &edit_insertions(&action));
            assert!(updated.contains("greet('hi', {})"), "bare call must update: {updated}");
        });
    }

    #[test]
    fn package_block_scope_does_not_leak_past_closing_brace() {
        let source = "package Outer { package Inner; }\nsub f ($x) { }\nf(1);\n";
        with_first_sub_named(source, "f", |sub, ast| {
            let action = must_some_with(add_parameter_action(source, sub, ast), "main package f");
            let updated = apply_edits(source, &edit_insertions(&action));
            assert!(updated.contains("f(1, {})"), "main-package call must update: {updated}");
        });
    }

    #[test]
    fn admits_optional_positional_append_for_proven_positional_shape() {
        let source = "sub greet ($name) { }\ngreet('hi');\n";
        with_first_sub_named(source, "greet", |sub, ast| {
            let action = must_some_with(add_parameter_action(source, sub, ast), "positional greet");
            let updated = apply_edits(source, &edit_insertions(&action));
            assert_eq!(updated, "sub greet ($name, $options = {}) { }\ngreet('hi', {});\n");
        });
    }

    fn apply_edits(source: &str, edits: &[(usize, String)]) -> String {
        let mut ordered = edits.to_vec();
        ordered.sort_by_key(|(offset, _)| *offset);
        let mut out = String::new();
        let mut cursor = 0usize;
        for (offset, text) in ordered {
            out.push_str(&source[cursor..offset]);
            out.push_str(&text);
            cursor = offset;
        }
        out.push_str(&source[cursor..]);
        out
    }
}
