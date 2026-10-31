//! Statement-form packages inside blocks must not leak their scope frame
//! past the block end (#18221).
//!
//! The first-pass lowerer pushes a scope for a statement-form package that
//! intentionally outlives its own visit; the frame ends with the enclosing
//! block. Before the repair, the block's single exit popped the package
//! frame instead of its own, enclosing exits then popped the wrong frames,
//! and a lexical declared after the block was recorded under the leaked
//! package so its read resolved to `None`.

use perl_parser_core::Parser;
use perl_parser_core::hir::{AccessMode, HirExpr, HirExprId, HirFile, VariableKind, lower_ast};
use perl_tdd_support::must_some;

fn lower(source: &str) -> HirFile {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    lower_ast(&output.ast)
}

/// Assert the lexical declared after the block resolves and is not owned by
/// the statement-form package `Q`.
fn assert_restored(source: &str) -> HirFile {
    let file = lower(source);
    let declaration = must_some(file.scope_graph.bindings.iter().find(|b| b.name == "late"));
    let read_start = must_some(source.rfind("$late"));
    let reference =
        must_some(file.scope_graph.references.iter().find(|r| r.range.start == read_start));
    assert_eq!(reference.resolved_binding, Some(declaration.id));
    // Both the declaration and its read live in the file scope again.
    let file_scope = file.scope_graph.scopes[0].id;
    assert_eq!(declaration.scope_id, file_scope);
    assert_eq!(reference.scope_id, file_scope);
    let owner = must_some(file.scope_graph.scopes.get(declaration.scope_id.index() as usize));
    assert_ne!(owner.package_context.as_deref(), Some("Q"));
    file
}

#[test]
fn block_package_with_statement_package_restores_later_declaration() {
    let source = "package P { package Q; }\nmy $late = 1;\nprint $late;\n";
    let file = assert_restored(source);
    let declaration = must_some(file.scope_graph.bindings.iter().find(|b| b.name == "late"));
    let read_start = must_some(source.rfind("$late"));

    let mut body_reads = Vec::new();
    for body in &file.bodies {
        for index in 0..body.source_map.expr_ranges.len() {
            let id = HirExprId(index as u32);
            if let Some(HirExpr::Variable(variable)) = body.expr(id)
                && variable.name == "late"
                && variable.access == AccessMode::Read
                && must_some(body.source_map.expr_range(id)).start == read_start
            {
                body_reads.push((variable.binding, variable.kind));
            }
        }
    }
    assert_eq!(body_reads, vec![(Some(declaration.id), VariableKind::Lexical)]);
}

/// The same restore holds for a bare block at file scope (the root-guard
/// boundary: the block frame sits directly above the file frame).
#[test]
fn bare_block_statement_package_restores_later_declaration() {
    assert_restored("{ package Q; }\nmy $late = 1;\nprint $late;\n");
}

/// And for a statement-form package in a loop body.
#[test]
fn loop_body_statement_package_restores_later_declaration() {
    assert_restored("for my $i (1) { package Q; }\nmy $late = 1;\nprint $late;\n");
}

/// Control: a statement-form package at file scope still applies to the
/// rest of the file, so a later package variable keeps package attribution.
#[test]
fn file_scope_statement_package_still_applies() {
    let source = "package Q;\nour $g = 1;\nprint $g;\n";
    let file = lower(source);
    let declaration = must_some(file.scope_graph.bindings.iter().find(|b| b.name == "g"));
    let owner = must_some(file.scope_graph.scopes.get(declaration.scope_id.index() as usize));
    assert_eq!(owner.package_context.as_deref(), Some("Q"));
}
