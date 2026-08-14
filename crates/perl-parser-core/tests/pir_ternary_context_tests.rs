//! PIR v0 ternary context regression coverage.
//!
//! Guards the flat-path context classification landed for #6122
//! (`crates/perl-parser-core/src/pir/lower.rs`, `lower_branch`): an
//! `if`/`unless` statement lowers to `PirContext::Void`, a ternary to
//! `PirContext::Unknown`.
//!
//! The shapes below are the ones where a keyword-blind `Void` would be
//! observably wrong. Two of them pin invariants that currently hold for
//! reasons no other test states — see
//! `nested_ternary_keeps_every_arm_branch_unknown` and
//! `statement_level_ternary_stays_unknown_not_void`.

use perl_parser_core::Parser;
use perl_parser_core::hir::{HirFile, lower_ast};
use perl_parser_core::pir::{PirContext, PirGraph, PirNode, PirOperation, lower_hir};
use perl_tdd_support::must_some;

fn lower(source: &str) -> PirGraph {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();

    // Every source below must parse cleanly. Without this guard a parser
    // regression could hand these tests a recovery AST that still happens to
    // carry a BranchShell, and the context assertions would keep passing while
    // proving something else. All seven sources currently yield zero
    // diagnostics.
    assert!(
        output.diagnostics.is_empty(),
        "source must parse without diagnostics, got {:?} for {source:?}",
        output.diagnostics
    );

    let hir: HirFile = lower_ast(&output.ast);
    lower_hir(&hir)
}

fn first_branch_node(graph: &PirGraph) -> &PirNode {
    must_some(graph.nodes.iter().find(|n| matches!(n.operation, PirOperation::Branch { .. })))
}

fn branch_contexts(graph: &PirGraph) -> Vec<PirContext> {
    graph
        .nodes
        .iter()
        .filter(|n| matches!(n.operation, PirOperation::Branch { .. }))
        .map(|n| n.context)
        .collect()
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

#[test]
fn nested_ternary_keeps_every_arm_branch_unknown() {
    // WHY THIS TEST EXISTS: nested ternaries are classified correctly today for
    // a non-obvious reason, and nothing else asserts it.
    //
    // `lower_branch` reads `BranchShell.keyword`, so it only classifies each
    // conditional correctly as long as each conditional actually gets its own
    // shell. It does: the flat HIR lowerer pushes one BranchShell per
    // `NodeKind::Ternary` and then recurses through `visit_children`
    // (`crates/perl-parser-core/src/hir/lower.rs`, the `NodeKind::Ternary` arm),
    // so an inner ternary is never folded into its enclosing conditional and
    // never inherits the outer node's classification.
    //
    // That is an accident of the current shell-per-node shape, not a stated
    // contract. A future arm-edge or condition-lowering slice that folds arms
    // into their parent shell would silently drop the inner Branch node or
    // reclassify it, and this assertion is what catches that.
    let graph = lower("my $x = $a ? ($b ? 1 : 2) : 3;");
    let contexts = branch_contexts(&graph);

    assert_eq!(contexts.len(), 2, "nested ternary should produce two Branch nodes: {contexts:?}");
    assert!(
        contexts.iter().all(|c| *c == PirContext::Unknown),
        "every nested ternary Branch must stay Unknown: {contexts:?}"
    );
}

#[test]
fn lvalue_ternary_stays_unknown_rather_than_void() {
    // `($c ? $a : $b) = 9` mutates the selected variable, so the ternary is an
    // assignment target. The flat path cannot prove Lvalue, but claiming Void
    // here would assert the node yields nothing — the opposite of the truth.
    let graph = lower("($c ? $a : $b) = 9;");
    let branch = first_branch_node(&graph);

    assert_eq!(branch.context, PirContext::Unknown);
}

#[test]
fn list_context_ternary_stays_unknown() {
    // In list context both arms flatten into the assigned list. `Unknown` is
    // the fail-closed answer; a later refinement may prove `List`, but nothing
    // in the flat path may narrow it today.
    let graph = lower("my @x = $c ? (1, 2) : (3, 4);");
    let branch = first_branch_node(&graph);

    assert_eq!(branch.context, PirContext::Unknown);
}

#[test]
fn ternary_with_differing_arm_contexts_stays_unknown() {
    // Context propagates downward into whichever arm is selected, and the arms
    // consume it differently (`@list` yields the enclosing list context,
    // `scalar(@list)` forces scalar). The Branch node describes the
    // conditional itself, not a merge of its arms, so it stays Unknown rather
    // than picking one arm's context.
    let graph = lower("my @x = $c ? @list : scalar(@list);");
    let branch = first_branch_node(&graph);

    assert_eq!(branch.context, PirContext::Unknown);
}

#[test]
fn statement_level_ternary_stays_unknown_not_void() {
    // DELIBERATE UNDER-CLAIM — do not "fix" this to Void.
    //
    // A bare ternary statement really is evaluated in void context in Perl, so
    // `Void` was accidentally right for this one shape before #6122. It is
    // still wrong to assert here: void-ness is a property of the *enclosing
    // statement*, which the flat path does not model, not of the BranchShell.
    // The node cannot prove it.
    //
    // So this is an accepted trade, not an oversight: `Unknown` under-claims
    // for this single shape in exchange for not over-claiming `Void` for every
    // value-producing ternary (the #6122 defect). Narrowing it back to `Void`
    // requires modeling the enclosing statement's context first — at which
    // point this assertion should be updated deliberately, with that modeling
    // in the same change, rather than flipped to make a keyword-blind
    // classification look correct again.
    let graph = lower("$c ? foo() : bar();");
    let branch = first_branch_node(&graph);

    assert_eq!(branch.context, PirContext::Unknown);
}

#[test]
fn statement_branch_and_ternary_keep_distinct_contexts_in_one_file() {
    // Opposite-direction control: the keyword match must discriminate within a
    // single lowering, not merely flip every Branch node to Unknown.
    let graph = lower("if ($p) { my $x = $q ? 1 : 2; }");
    let contexts = branch_contexts(&graph);

    assert!(
        contexts.contains(&PirContext::Void),
        "the `if` statement must still lower to Void: {contexts:?}"
    );
    assert!(
        contexts.contains(&PirContext::Unknown),
        "the nested ternary must lower to Unknown: {contexts:?}"
    );
}
