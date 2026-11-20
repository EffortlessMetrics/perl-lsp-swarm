//! Inline variable refactoring: source-backed lexical substitution before edits.
//!
//! Offers `refactor.inline` only when a single `my $name = EXPR` binding can be
//! proven safe to substitute at each in-scope use from the AST and symbol table.

use super::super::types::{CodeAction, CodeActionEdit, CodeActionKind};
use crate::providers::code_actions::enhanced::helpers::Helpers;
use crate::providers::rename::{RenameOptions, RenameProvider, TextEdit};
use perl_parser_core::ast::{Node, NodeKind, SourceLocation};

/// Why inline-variable was not offered (for tests and diagnostics).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InlineVariableRefusal {
    NotOnInitializedLexical,
    UnsupportedDeclarator,
    NonScalarBinding,
    SideEffectingInitializer,
    Reassignment,
    NoInScopeUses,
    SourceBackedSitesUnavailable,
}

/// A proven inline plan: replace each usage with `replacement_text` and delete the
/// declaration statement span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineVariablePlan {
    pub declaration_statement: SourceLocation,
    pub usage_replacements: Vec<(SourceLocation, String)>,
}

/// Prove that inlining is safe and compute replacement edits from source + symbols.
pub fn prove_source_backed_lexical_inline(
    ast: &Node,
    source: &str,
    offset: usize,
) -> Result<InlineVariablePlan, InlineVariableRefusal> {
    let (decl_node, variable, initializer) = find_initialized_lexical_declaration(ast, offset)
        .ok_or(InlineVariableRefusal::NotOnInitializedLexical)?;

    let NodeKind::VariableDeclaration { declarator, .. } = &decl_node.kind else {
        return Err(InlineVariableRefusal::NotOnInitializedLexical);
    };
    if declarator != "my" {
        return Err(InlineVariableRefusal::UnsupportedDeclarator);
    }

    let (_sigil_len, bare_name) =
        scalar_name(variable).ok_or(InlineVariableRefusal::NonScalarBinding)?;

    if initializer_contains_side_effects(initializer) {
        return Err(InlineVariableRefusal::SideEffectingInitializer);
    }

    if has_lexical_reassignment(ast, decl_node.location.end, bare_name) {
        return Err(InlineVariableRefusal::Reassignment);
    }

    let var_offset = variable.location.start.saturating_add(1);
    let rename_provider = RenameProvider::new(ast, source.to_string());
    let rename_result = rename_provider.scoped_rename(
        var_offset,
        bare_name,
        &RenameOptions { validate_new_name: false, ..RenameOptions::default() },
    );
    if !rename_result.is_valid {
        return Err(InlineVariableRefusal::SourceBackedSitesUnavailable);
    }

    let lines: Vec<String> = source.lines().map(str::to_string).collect();
    let helpers = Helpers::new(source, &lines);
    let substitution = format_substitution(source, initializer);
    let stmt_start = helpers.find_statement_start(decl_node.location.start);
    let stmt_end =
        statement_end(source, stmt_start).ok_or(InlineVariableRefusal::NotOnInitializedLexical)?;

    let mut usage_replacements = Vec::new();
    for edit in rename_result.edits {
        if edit.location.start >= stmt_start && edit.location.end <= stmt_end {
            // Skip the declaration site itself; the whole statement is removed.
            continue;
        }
        let mut location = edit.location;
        if location.start > 0 && source.as_bytes().get(location.start - 1) == Some(&b'$') {
            location.start -= 1;
        }
        usage_replacements.push((location, substitution.clone()));
    }

    if usage_replacements.is_empty() {
        return Err(InlineVariableRefusal::NoInScopeUses);
    }

    Ok(InlineVariablePlan {
        declaration_statement: SourceLocation { start: stmt_start, end: stmt_end },
        usage_replacements,
    })
}

/// Build a `refactor.inline` action when the plan proves safe.
pub fn create_inline_variable_action(
    ast: &Node,
    source: &str,
    offset: usize,
) -> Result<CodeAction, InlineVariableRefusal> {
    let plan = prove_source_backed_lexical_inline(ast, source, offset)?;
    let mut changes: Vec<TextEdit> = plan
        .usage_replacements
        .into_iter()
        .map(|(location, new_text)| TextEdit { location, new_text })
        .collect();
    changes.push(TextEdit { location: plan.declaration_statement, new_text: String::new() });
    // Apply from the end of the file backward so offsets stay valid.
    changes.sort_by_key(|edit| std::cmp::Reverse(edit.location.start));

    let title = "Inline variable";
    Ok(CodeAction {
        title: title.to_string(),
        kind: CodeActionKind::RefactorInline,
        diagnostics: Vec::new(),
        edit: CodeActionEdit { changes },
        is_preferred: false,
    })
}

fn find_initialized_lexical_declaration(
    ast: &Node,
    offset: usize,
) -> Option<(&Node, &Node, &Node)> {
    let decl = find_enclosing_initialized_declaration(ast, offset)?;
    let NodeKind::VariableDeclaration { variable, initializer, .. } = &decl.kind else {
        return None;
    };
    let init = initializer.as_deref()?;
    Some((decl, variable, init))
}

fn find_enclosing_initialized_declaration(node: &Node, offset: usize) -> Option<&Node> {
    if offset < node.location.start || offset > node.location.end {
        return None;
    }

    let mut found = None;
    for child in node.children() {
        if let Some(decl) = find_enclosing_initialized_declaration(child, offset) {
            found = Some(decl);
        }
    }

    if let NodeKind::VariableDeclaration { initializer: Some(_), .. } = &node.kind {
        found = Some(node);
    }

    found
}

fn scalar_name(variable: &Node) -> Option<(usize, &str)> {
    match &variable.kind {
        NodeKind::Variable { sigil, name } if sigil == "$" => Some((sigil.len(), name.as_str())),
        _ => None,
    }
}

fn statement_end(source: &str, stmt_start: usize) -> Option<usize> {
    let tail = source.get(stmt_start..)?;
    let semi = tail.find(';')?;
    let mut end = stmt_start + semi + 1;
    if source.get(end..).is_some_and(|rest| rest.starts_with('\n')) {
        end += 1;
    }
    Some(end)
}

fn initializer_contains_side_effects(node: &Node) -> bool {
    match &node.kind {
        NodeKind::FunctionCall { .. }
        | NodeKind::MethodCall { .. }
        | NodeKind::AmperCall { .. }
        | NodeKind::IndirectCall { .. }
        | NodeKind::Eval { .. }
        | NodeKind::Readline { .. }
        | NodeKind::Diamond => true,
        _ => node.children().iter().any(|child| initializer_contains_side_effects(child)),
    }
}

fn has_lexical_reassignment(ast: &Node, after_offset: usize, bare_name: &str) -> bool {
    let mut found = false;
    walk_for_reassignment(ast, after_offset, bare_name, &mut found);
    found
}

fn walk_for_reassignment(node: &Node, after_offset: usize, bare_name: &str, found: &mut bool) {
    if *found {
        return;
    }
    if node.location.start >= after_offset
        && let NodeKind::Assignment { lhs, .. } = &node.kind
        && assignment_targets_scalar(lhs, bare_name)
    {
        *found = true;
        return;
    }
    for child in node.children() {
        walk_for_reassignment(child, after_offset, bare_name, found);
    }
}

fn assignment_targets_scalar(lhs: &Node, bare_name: &str) -> bool {
    match &lhs.kind {
        NodeKind::Variable { sigil, name } if sigil == "$" && name == bare_name => true,
        NodeKind::Binary { op, left, .. } if op == "," => {
            assignment_targets_scalar(left, bare_name)
                || lhs.children().iter().any(|c| assignment_targets_scalar(c, bare_name))
        }
        _ => false,
    }
}

fn format_substitution(source: &str, initializer: &Node) -> String {
    let expr = &source[initializer.location.start..initializer.location.end];
    if substitution_needs_parens(initializer) { format!("({expr})") } else { expr.to_string() }
}

fn substitution_needs_parens(node: &Node) -> bool {
    match &node.kind {
        NodeKind::Binary { .. }
        | NodeKind::Ternary { .. }
        | NodeKind::Unary { .. }
        | NodeKind::Assignment { .. }
        | NodeKind::FunctionCall { .. }
        | NodeKind::MethodCall { .. }
        | NodeKind::AmperCall { .. }
        | NodeKind::IndirectCall { .. } => true,
        _ => node.children().iter().any(|child| substitution_needs_parens(child)),
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

    fn plan_at(source: &str, needle: &str) -> Result<InlineVariablePlan, InlineVariableRefusal> {
        let ast = parse(source);
        let offset = must_some_with(source.find(needle), "needle in fixture");
        prove_source_backed_lexical_inline(&ast, source, offset)
    }

    #[test]
    fn inlines_simple_lexical_at_each_use() {
        let source = "my $extracted = 1 + 2;\nprint $extracted;\nfoo($extracted);\n";
        let plan = must(plan_at(source, "$extracted"));
        assert_eq!(plan.usage_replacements.len(), 2);
        assert!(plan.usage_replacements.iter().all(|(_, text)| text == "(1 + 2)"));
        let applied = apply_plan(source, &plan);
        assert!(applied.contains("print (1 + 2);"));
        assert!(applied.contains("foo((1 + 2));"));
        assert!(!applied.contains("my $extracted"));
    }

    #[test]
    fn refuses_side_effecting_initializer() {
        let source = "my $x = foo();\nprint $x;\n";
        assert_eq!(plan_at(source, "$x"), Err(InlineVariableRefusal::SideEffectingInitializer));
    }

    #[test]
    fn refuses_reassignment() {
        let source = "my $x = 1;\n$x = 2;\nprint $x;\n";
        assert_eq!(plan_at(source, "$x"), Err(InlineVariableRefusal::Reassignment));
    }

    #[test]
    fn refuses_without_in_scope_uses() {
        let source = "my $x = 1;\n";
        assert_eq!(plan_at(source, "$x"), Err(InlineVariableRefusal::NoInScopeUses));
    }

    #[test]
    fn refuses_our_declaration() {
        let source = "our $x = 1;\nprint $x;\n";
        assert_eq!(plan_at(source, "$x"), Err(InlineVariableRefusal::UnsupportedDeclarator));
    }

    #[test]
    fn code_action_carries_refactor_inline_kind() {
        let source = "my $v = 42;\nprint $v;\n";
        let ast = parse(source);
        let offset = must_some_with(source.find("$v"), "needle");
        let action = must(create_inline_variable_action(&ast, source, offset));
        assert_eq!(action.kind, CodeActionKind::RefactorInline);
        assert_eq!(action.edit.changes.len(), 2);
    }

    fn apply_plan(source: &str, plan: &InlineVariablePlan) -> String {
        let mut edits: Vec<(usize, usize, String)> = plan
            .usage_replacements
            .iter()
            .map(|(loc, text)| (loc.start, loc.end, text.clone()))
            .chain(std::iter::once((
                plan.declaration_statement.start,
                plan.declaration_statement.end,
                String::new(),
            )))
            .collect();
        edits.sort_by_key(|edit| std::cmp::Reverse(edit.0));
        let mut out = source.to_string();
        for (start, end, text) in edits {
            out.replace_range(start..end, &text);
        }
        out
    }
}
