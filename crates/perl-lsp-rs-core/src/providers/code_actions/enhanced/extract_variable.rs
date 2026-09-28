//! Extract variable code action

use std::collections::HashSet;

use super::super::types::{CodeAction, CodeActionEdit, CodeActionKind};
use crate::providers::rename::TextEdit;
use perl_parser_core::ast::{Node, NodeKind, SourceLocation};

use super::helpers::Helpers;

/// Create extract variable action with smart naming.
///
/// `ast_root` must be the full program AST so the suggested name can be checked
/// against the declarations visible at the insertion point; see
/// [`visible_names_at`].
pub fn create_extract_variable_action(
    node: &Node,
    source: &str,
    helpers: &Helpers<'_>,
    ast_root: &Node,
) -> CodeAction {
    let expr_text = &source[node.location.start..node.location.end];

    // Find the best insertion point
    let stmt_start = helpers.find_statement_start(node.location.start);
    let indent = helpers.get_indent_at(stmt_start);

    // The declaration is inserted at `stmt_start`, so any name already bound in
    // the enclosing scope chain would either be redeclared (same scope, which
    // `perl -c` reports as a masking warning) or silently shadowed (outer
    // scope, which changes program behaviour without any diagnostic).
    let visible = visible_names_at(ast_root, stmt_start);
    let var_name = unique_variable_name(&suggest_variable_name(node), &visible);

    CodeAction {
        title: format!("Extract '{}' to variable", helpers.truncate_expr(expr_text, 30)),
        kind: CodeActionKind::RefactorExtract,
        diagnostics: Vec::new(),
        edit: CodeActionEdit {
            changes: vec![
                // Insert variable declaration
                TextEdit {
                    location: SourceLocation { start: stmt_start, end: stmt_start },
                    new_text: format!("{}my ${} = {};\n", indent, var_name, expr_text),
                },
                // Replace expression with variable
                TextEdit { location: node.location, new_text: format!("${}", var_name) },
            ],
        },
        is_preferred: false,
    }
}

/// Suggest a variable name based on the expression
pub fn suggest_variable_name(node: &Node) -> String {
    match &node.kind {
        NodeKind::FunctionCall { name, .. } => {
            let func_name = name.as_str();
            match func_name {
                "length" | "size" => "len",
                "split" => "parts",
                "join" => "joined",
                "sort" => "sorted",
                "reverse" => "reversed",
                "grep" | "filter" => "filtered",
                "map" => "mapped",
                _ => "result",
            }
        }
        NodeKind::MethodCall { method, .. } => {
            let method_name = method.as_str();
            match method_name {
                "new" => "instance",
                "clone" | "copy" => "copy",
                "get" | "fetch" => "value",
                "find" | "search" | "lookup" => "found",
                "count" | "size" | "length" => "count",
                "name" | "get_name" => "name",
                "type" | "get_type" => "type_name",
                "to_string" | "stringify" | "as_string" => "str",
                "is_valid" | "validate" | "check" => "is_valid",
                _ => "result",
            }
        }
        NodeKind::Binary { op, .. } => match op.as_str() {
            "+" | "-" | "*" | "/" | "%" => "result",
            "." | "x" => "str",
            "&&" | "||" | "and" | "or" => "condition",
            "==" | "!=" | "<" | ">" | "<=" | ">=" => "is_valid",
            "{}" => "val",
            "[]" => "elem",
            _ => "value",
        },
        _ => "extracted",
    }
    .to_string()
}

/// Collect the variable names that are already bound at `offset`.
///
/// Walks only the nodes whose span contains `offset`, so the cost is bounded by
/// the enclosing scope chain rather than by the size of the file. Declarations
/// inside *nested* scopes are deliberately skipped: a `my` in an inner block is
/// not visible at `offset` and reusing its name cannot shadow anything there.
pub fn visible_names_at(ast_root: &Node, offset: usize) -> HashSet<String> {
    let mut names = HashSet::new();
    collect_enclosing_scope_names(ast_root, offset, &mut names);
    names
}

/// Recurse into the child that encloses `offset`, recording the declarations of
/// every scope-introducing node on the way down.
fn collect_enclosing_scope_names(node: &Node, offset: usize, names: &mut HashSet<String>) {
    if !node.contains_offset(offset) {
        return;
    }

    if is_scope_boundary(&node.kind) {
        for child in node.children() {
            collect_declared_names(child, names);
        }
    }

    for child in node.children() {
        collect_enclosing_scope_names(child, offset, names);
    }
}

/// Record the names declared anywhere in a single scope, without descending
/// into an inner scope.
fn collect_declared_names(node: &Node, names: &mut HashSet<String>) {
    match &node.kind {
        NodeKind::VariableDeclaration { variable, .. } => {
            if let NodeKind::Variable { name, .. } = &variable.kind {
                names.insert(name.clone());
            }
        }
        NodeKind::VariableListDeclaration { variables, .. } => {
            for v in variables {
                if let NodeKind::Variable { name, .. } = &v.kind {
                    names.insert(name.clone());
                }
            }
        }
        // Signature parameters are bound in the subroutine's scope without a
        // `my`, so they need their own arm.
        NodeKind::MandatoryParameter { variable }
        | NodeKind::OptionalParameter { variable, .. } => {
            if let NodeKind::Variable { name, .. } = &variable.kind {
                names.insert(name.clone());
            }
        }
        _ => {}
    }

    if is_scope_boundary(&node.kind) {
        return;
    }

    for child in node.children() {
        collect_declared_names(child, names);
    }
}

/// Node kinds that introduce a new lexical scope in Perl.
///
/// A `while`/`if` body is deliberately absent: those bodies do not open a new
/// lexical scope, so a `my` in one is still visible to the enclosing block and
/// must be treated as a collision.
fn is_scope_boundary(kind: &NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::Program { .. }
            | NodeKind::Block { .. }
            | NodeKind::Subroutine { .. }
            | NodeKind::Package { .. }
    )
}

/// Return `base` when it is free at the insertion point, otherwise the first
/// free `base2`, `base3`, ... variant.
pub fn unique_variable_name(base: &str, visible: &HashSet<String>) -> String {
    if !visible.contains(base) {
        return base.to_string();
    }
    let mut suffix = 2;
    loop {
        let candidate = format!("{}{}", base, suffix);
        if !visible.contains(&candidate) {
            return candidate;
        }
        suffix += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(names: &[&str]) -> HashSet<String> {
        names.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn unique_name_is_the_base_when_free() {
        assert_eq!(unique_variable_name("len", &set(&["other"])), "len");
        assert_eq!(unique_variable_name("len", &set(&[])), "len");
    }

    #[test]
    fn unique_name_suffixes_when_base_is_bound() {
        assert_eq!(unique_variable_name("len", &set(&["len"])), "len2");
    }

    /// The suffix must skip an already-taken candidate rather than collide with
    /// it, which is the property that makes the loop terminate on the first
    /// genuinely free name.
    #[test]
    fn unique_name_skips_a_taken_suffixed_candidate() {
        assert_eq!(unique_variable_name("len", &set(&["len", "len2"])), "len3");
        assert_eq!(unique_variable_name("len", &set(&["len", "len2", "len3"])), "len4");
    }
}
