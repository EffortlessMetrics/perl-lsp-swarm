//! PIR v0 ternary context regression coverage.

use perl_parser_core::Parser;
use perl_parser_core::hir::{HirFile, lower_ast};
use perl_parser_core::pir::{PirContext, PirGraph, PirOperation, lower_hir};
use perl_tdd_support::must_some;

fn lower(source: &str) -> PirGraph {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    let hir: HirFile = lower_ast(&output.ast);
    lower_hir(&hir)
}

fn first_branch_node(graph: &PirGraph) -> &perl_parser_core::pir::PirNode {
    must_some(graph.nodes.iter().find(|n| matches!(n.operation, PirOperation::Branch { .. })))
}

#[test]
fn ternary_lowers_to_unknown_context_on_flat_path() {
    // The flat HIR path receives the same BranchShell as if/unless, but a
    // ternary is a value-producing conditional expression that may participate
    // in an lvalue context. Its enclosing Scalar/List/Lvalue context is not
    // modeled here, so it must stay Unknown rather than Void.
    let graph = lower("my $x = $c ? 1 : 2;");
    let branch = first_branch_node(&graph);

    assert_eq!(branch.context, PirContext::Unknown);
}
