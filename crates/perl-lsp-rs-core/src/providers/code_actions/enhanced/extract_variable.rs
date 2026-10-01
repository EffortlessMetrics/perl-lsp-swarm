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
/// `visible_names_at`.
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
fn visible_names_at(ast_root: &Node, offset: usize) -> HashSet<String> {
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
            record_scalar_variable(variable, names);
        }
        NodeKind::VariableListDeclaration { variables, .. } => {
            for v in variables {
                record_declared_target(v, names);
            }
        }
        // A nested list target (`my ($x, ($result, $z)) = ...`) binds each
        // item into the same scope; the wrapper itself declares nothing.
        NodeKind::NestedVariableList { items } => {
            for item in items {
                record_declared_target(item, names);
            }
        }
        // Attributes wrap a base variable (`my $x :shared`); the base binds.
        NodeKind::VariableWithAttributes { variable, .. } => {
            record_scalar_variable(variable, names);
        }
        // Signature parameters are bound in the subroutine's scope without a
        // `my`, so they need their own arm. Named and slurpy parameters bind
        // lexicals exactly like positional ones (#16676).
        NodeKind::MandatoryParameter { variable }
        | NodeKind::OptionalParameter { variable, .. }
        | NodeKind::NamedParameter { variable, .. }
        | NodeKind::SlurpyParameter { variable } => {
            record_scalar_variable(variable, names);
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

/// Record the variable only when it is a scalar binding: the action always
/// generates `my $name`, and Perl keeps `@name` / `%name` separate from
/// `$name`, so a same-named array or hash is no collision.
fn record_scalar_variable(variable: &Node, names: &mut HashSet<String>) {
    if let NodeKind::Variable { sigil, name } = &variable.kind
        && sigil == "$"
    {
        names.insert(name.clone());
    }
}

/// A declaration-list item is a variable (possibly carrying attributes) or a
/// further nested list; both bind their scalars into the same scope.
fn record_declared_target(node: &Node, names: &mut HashSet<String>) {
    match &node.kind {
        NodeKind::VariableWithAttributes { variable, .. } => {
            record_scalar_variable(variable, names);
        }
        NodeKind::NestedVariableList { .. } | NodeKind::VariableListDeclaration { .. } => {
            collect_declared_names(node, names);
        }
        _ => record_scalar_variable(node, names),
    }
}

/// Node kinds that introduce a new lexical scope in Perl.
///
/// Every braced body is a scope: an `if`/`while` body is its own `Block`
/// node, and `for`/`foreach` open a scope for their iterator variable, so a
/// `my` declared there is invisible outside the loop.
fn is_scope_boundary(kind: &NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::Program { .. }
            | NodeKind::Block { .. }
            | NodeKind::Subroutine { .. }
            | NodeKind::Package { .. }
            | NodeKind::For { .. }
            | NodeKind::Foreach { .. }
    )
}

/// Return `base` when it is free at the insertion point, otherwise the first
/// free `base2`, `base3`, ... variant.
fn unique_variable_name(base: &str, visible: &HashSet<String>) -> String {
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
    use perl_parser_core::Parser;
    use perl_test_must::{must_some_with, must_with};

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

    fn visible_names_before(source: &str, needle: &str) -> HashSet<String> {
        let mut parser = Parser::new(source);
        let ast = must_with(parser.parse(), "fixture must parse");
        let offset = must_some_with(source.find(needle), "needle present in fixture");
        visible_names_at(&ast, offset)
    }

    /// #16676: a Perl 5.44 named parameter binds a lexical exactly like a
    /// positional one, so an extraction inside that subroutine must treat the
    /// name as taken instead of shadowing the argument.
    #[test]
    fn named_parameter_binding_is_visible() {
        let source = "sub f (:$result) { my $total = 2 + 3; print $result; }";
        let visible = visible_names_before(source, "2 + 3");
        assert!(
            visible.contains("result"),
            "a named scalar parameter must be a taken name, got {visible:?}"
        );
    }

    /// #16676: a same-scope name declared inside a nested list target binds in
    /// the enclosing scope, so extraction must not reuse its spelling.
    #[test]
    fn nested_list_declarations_are_visible() {
        let source = "my ($result, ($result2, $other)) = (1, 2, 3);\nmy $total = 2 + 3;";
        let visible = visible_names_before(source, "2 + 3");
        for taken in ["result", "result2", "other"] {
            assert!(
                visible.contains(taken),
                "nested destructuring binds {taken} in this scope, got {visible:?}"
            );
        }
    }

    /// #16676: a scalar binding declared with attributes is still a taken
    /// scalar name.
    #[test]
    fn attributed_scalar_binding_is_visible() {
        let source = "my $result :shared;\nmy $total = 2 + 3;";
        let visible = visible_names_before(source, "2 + 3");
        assert!(
            visible.contains("result"),
            "an attributed scalar binding must be a taken name, got {visible:?}"
        );
    }

    /// #16676: the generated declaration is always a scalar, and Perl keeps
    /// `@name` / `%name` separate from `$name`, so a same-named array or hash
    /// must not force a scalar rename.
    #[test]
    fn non_scalar_declarations_do_not_collide() {
        let source = "my @result = (1);\nmy %result = (a => 1);\nmy $total = 2 + 3;";
        let visible = visible_names_before(source, "2 + 3");
        assert!(
            !visible.contains("result"),
            "array and hash bindings must not take the scalar name, got {visible:?}"
        );
    }

    /// #16676: a `for my $i (...)` iterator is scoped to the loop. Outside the
    /// loop the name is free; inside it, it must still be treated as taken.
    #[test]
    fn for_iterator_variable_is_scoped_to_the_loop() {
        let source = "for my $i (1 .. 3) { my $inside = $i; }\nmy $total = 2 + 3;";
        let outside = visible_names_before(source, "2 + 3");
        assert!(
            !outside.contains("i") && !outside.contains("inside"),
            "loop-scoped names must be free after the loop, got {outside:?}"
        );

        let inside = visible_names_before(source, "$inside");
        assert!(
            inside.contains("i") && inside.contains("inside"),
            "the loop's own names must be visible inside it, got {inside:?}"
        );
    }
}
