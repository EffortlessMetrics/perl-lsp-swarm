//! HIR lowering proof for `tie`/`untie` dynamic boundaries (issue #14786).
//!
//! `tie` binds a place to a tie class, after which every read, write,
//! iteration, and destruction of that place dispatches to hidden `TIE*`
//! methods. Before this slice, `NodeKind::Tie` and `NodeKind::Untie` had no
//! arm in `crates/perl-parser-core/src/hir/lower.rs`: both fell through to
//! `_ => visit_children` and vanished from flat HIR, so a consumer walking
//! HIR saw a tied `%hash` as ordinary storage.
//!
//! These tests pin that both constructs now emit an explicit
//! `HirKind::DynamicBoundary`, in the same way `Eval`/`Do`/symbolic-deref
//! already do, while still traversing children so the tie class expression
//! and the `TIE*` arguments keep lowering normally.
//!
//! Claim boundary: this proves only that a boundary exists and is anchored
//! to the right construct. The tied place identity, the hidden constructor
//! dispatch, and the tied access classes remain unmodeled and belong to
//! parent issue #6683.

use perl_parser_core::Parser;
use perl_parser_core::hir::{
    DynamicBoundary, DynamicBoundaryKind, HirFile, HirItem, HirKind, disposition, lower_ast,
};
use perl_parser_core::pir::{
    PirCallee, PirEdgeKind, PirGraph, PirId, PirOperation, lower_hir, lower_hir_bodies,
};
use perl_tdd_support::must_some;
use std::collections::{HashMap, HashSet};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn lower_source(source: &str) -> HirFile {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    lower_ast(&output.ast)
}

fn boundaries_of(file: &HirFile, kind: DynamicBoundaryKind) -> Vec<&HirItem> {
    file.items
        .iter()
        .filter(|item| matches!(&item.kind, HirKind::DynamicBoundary(b) if b.kind == kind))
        .collect()
}

fn sole_boundary(
    file: &HirFile,
    kind: DynamicBoundaryKind,
) -> Option<(&HirItem, &DynamicBoundary)> {
    let matches = boundaries_of(file, kind);
    if matches.len() != 1 {
        return None;
    }
    let item = matches[0];
    match &item.kind {
        HirKind::DynamicBoundary(boundary) => Some((item, boundary)),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// tie emits a boundary
// ---------------------------------------------------------------------------

#[test]
fn tie_emits_exactly_one_tied_place_binding_boundary() -> TestResult {
    let file = lower_source("tie %hash, 'Tie::StdHash';\n");
    let (item, boundary) = must_some(sole_boundary(&file, DynamicBoundaryKind::TiedPlaceBinding));
    assert_eq!(
        item.anchor.node_kind, "Tie",
        "the tie boundary must be anchored to the Tie construct, got {:?}",
        item.anchor.node_kind
    );
    assert!(!boundary.reason.trim().is_empty(), "a dynamic boundary must carry a non-empty reason");
    Ok(())
}

#[test]
fn tie_boundary_is_emitted_for_every_tied_sigil_family() -> TestResult {
    for source in [
        "tie $scalar, 'Tie::StdScalar';\n",
        "tie @array, 'Tie::StdArray';\n",
        "tie %hash, 'Tie::StdHash';\n",
        "tie *HANDLE, 'Tie::StdHandle';\n",
    ] {
        let file = lower_source(source);
        assert_eq!(
            boundaries_of(&file, DynamicBoundaryKind::TiedPlaceBinding).len(),
            1,
            "expected exactly one tie boundary for {source:?}"
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// untie emits a distinct boundary
// ---------------------------------------------------------------------------

#[test]
fn untie_emits_exactly_one_tied_place_release_boundary() -> TestResult {
    let file = lower_source("untie %hash;\n");
    let (item, boundary) = must_some(sole_boundary(&file, DynamicBoundaryKind::TiedPlaceRelease));
    assert_eq!(
        item.anchor.node_kind, "Untie",
        "the untie boundary must be anchored to the Untie construct, got {:?}",
        item.anchor.node_kind
    );
    assert!(!boundary.reason.trim().is_empty(), "a dynamic boundary must carry a non-empty reason");
    Ok(())
}

/// `tie` and `untie` are different propositions: binding a place to a class
/// is not the same event as releasing it. A single shared boundary kind would
/// let a consumer confuse the two, so the two kinds must stay distinct.
#[test]
fn tie_and_untie_boundaries_are_independently_classified() -> TestResult {
    let file = lower_source("tie %hash, 'Tie::StdHash';\nuntie %hash;\n");
    assert_eq!(
        boundaries_of(&file, DynamicBoundaryKind::TiedPlaceBinding).len(),
        1,
        "one tie in the source must produce exactly one binding boundary"
    );
    assert_eq!(
        boundaries_of(&file, DynamicBoundaryKind::TiedPlaceRelease).len(),
        1,
        "one untie in the source must produce exactly one release boundary"
    );
    assert_ne!(
        DynamicBoundaryKind::TiedPlaceBinding,
        DynamicBoundaryKind::TiedPlaceRelease,
        "tie and untie must not share a boundary kind"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Children keep lowering
// ---------------------------------------------------------------------------

/// The boundary must not swallow the construct. `tie`'s arguments are ordinary
/// Perl and are evaluated before the hidden constructor runs, so they must
/// still reach HIR.
#[test]
fn tie_boundary_still_traverses_arguments() -> TestResult {
    let file = lower_source("tie %hash, 'Tie::StdHash', build_args();\n");
    assert_eq!(
        boundaries_of(&file, DynamicBoundaryKind::TiedPlaceBinding).len(),
        1,
        "the tie itself must still produce its boundary"
    );
    let has_call = file.items.iter().any(|item| match &item.kind {
        HirKind::CallExpr(call) => call.name == "build_args",
        _ => false,
    });
    assert!(
        has_call,
        "build_args() inside the tie argument list must still lower to a CallExpr; \
         anchored HIR items: {:?}",
        file.items.iter().map(|item| item.anchor.node_kind).collect::<Vec<_>>()
    );
    Ok(())
}

/// Perl evaluates the tie operands and only then dispatches the hidden `TIE*`
/// constructor, so the boundary must come **after** its arguments.
///
/// This is not cosmetic ordering. `pir::lower` turns consecutive flat items into
/// `PirEdgeKind::Fallthrough` edges, so a boundary emitted before its own
/// arguments makes the graph assert that control reaches the hidden dispatch
/// first and falls through into the arguments afterwards — an evaluation order
/// Perl does not have.
#[test]
fn tie_boundary_follows_its_arguments_in_evaluation_order() -> TestResult {
    let file = lower_source("tie %hash, 'Tie::StdHash', build_args();\n");

    let call_index = must_some(file.items.iter().position(|item| match &item.kind {
        HirKind::CallExpr(call) => call.name == "build_args",
        _ => false,
    }));
    let boundary_index = must_some(file.items.iter().position(|item| {
        matches!(&item.kind, HirKind::DynamicBoundary(b) if b.kind == DynamicBoundaryKind::TiedPlaceBinding)
    }));

    assert!(
        call_index < boundary_index,
        "build_args() is evaluated before the hidden TIE* constructor, so its \
         CallExpr must precede the tie boundary; got call at {call_index}, \
         boundary at {boundary_index}, items: {:?}",
        file.items.iter().map(|item| item.anchor.node_kind).collect::<Vec<_>>()
    );
    Ok(())
}

/// Walk the `Fallthrough` chain of a flat PIR graph from its entry node,
/// returning the visited node ids in control-flow order.
///
/// Position in `graph.nodes` is **not** control-flow order: PIR splices operand
/// nodes in beside their expression parent, so a node can be appended to the
/// vector after the operation it feeds while the edges still run operand-first.
/// Any ordering claim therefore has to follow edges, not vector indices.
fn fallthrough_order(graph: &PirGraph) -> Vec<PirId> {
    let mut next: HashMap<PirId, PirId> = HashMap::new();
    let mut has_incoming: HashSet<PirId> = HashSet::new();
    for edge in &graph.edges {
        if edge.kind == PirEdgeKind::Fallthrough
            && let Some(to) = edge.to
        {
            next.insert(edge.from, to);
            has_incoming.insert(to);
        }
    }
    let Some(entry) = graph.nodes.iter().map(|node| node.id).find(|id| !has_incoming.contains(id))
    else {
        return Vec::new();
    };

    let mut order = vec![entry];
    let mut seen: HashSet<PirId> = HashSet::from([entry]);
    let mut cursor = entry;
    while let Some(&following) = next.get(&cursor) {
        if !seen.insert(following) {
            break;
        }
        order.push(following);
        cursor = following;
    }
    order
}

/// The same ordering, observed through the flat PIR graph's actual
/// `Fallthrough` edges rather than through node-vector positions.
#[test]
fn flat_pir_reaches_the_tie_boundary_after_its_arguments() -> TestResult {
    let mut parser = Parser::new("tie %hash, 'Tie::StdHash', build_args();\n");
    let output = parser.parse_with_recovery();
    let hir = lower_ast(&output.ast);
    let graph = lower_hir(&hir);

    let call_id = must_some(graph.nodes.iter().find_map(|node| {
        matches!(&node.operation, PirOperation::Call { callee: PirCallee::Named { name, .. }, .. } if name == "build_args")
            .then_some(node.id)
    }));
    let boundary_id = must_some(graph.nodes.iter().find_map(|node| {
        matches!(&node.operation, PirOperation::DynamicBoundary { .. }).then_some(node.id)
    }));

    let order = fallthrough_order(&graph);
    let call_step = must_some(order.iter().position(|id| *id == call_id));
    let boundary_step = must_some(order.iter().position(|id| *id == boundary_id));

    assert!(
        call_step < boundary_step,
        "control must reach build_args() before the tie boundary along the \
         Fallthrough chain; got call at step {call_step}, boundary at step \
         {boundary_step}, chain {order:?}"
    );
    Ok(())
}

/// Known limitation, pinned deliberately: in a **nested expression** context the
/// flat PIR graph reaches the consuming operation before the tie boundary.
///
/// `my $obj = tie %h, 'C';` is legal Perl and parses cleanly, but flat PIR
/// yields `LexicalWrite($obj) -> Assign -> Literal('C') -> DynamicBoundary`, so
/// the assignment that consumes the tie's result precedes the tie dispatch.
///
/// The cause is not this slice's emission order. `pir::lower` splices ordinary
/// operand nodes back in beside their expression parent through
/// `push_node_maybe_operand`, while `lower_dynamic_boundary` appends through
/// `push_node` — so no dynamic boundary participates in operand splicing. Making
/// them participate would change lowering for every existing boundary kind
/// (`CoderefCall` and `EmbeddedRegexCode` in particular depend on staying
/// adjacent to their owning item, guarded by `debug_assert!`), which is wider
/// than this claim.
///
/// This test exists so the limitation is discoverable and cannot be relied upon
/// by accident. It asserts the boundary is still *present* and records the
/// current ordering; when the shared PIR seam is fixed, this test should fail
/// and be replaced by the correct-order assertion.
#[test]
fn nested_tie_boundary_currently_trails_its_consumer_in_flat_pir() -> TestResult {
    let mut parser = Parser::new("package Main;\nmy $obj = tie %hash, 'Tie::StdHash';\n");
    let output = parser.parse_with_recovery();
    assert_eq!(
        output.diagnostics.len(),
        0,
        "the nested form must parse cleanly, or this limitation is about something else"
    );
    let hir = lower_ast(&output.ast);
    let graph = lower_hir(&hir);

    let boundary_id = must_some(graph.nodes.iter().find_map(|node| {
        matches!(&node.operation, PirOperation::DynamicBoundary { .. }).then_some(node.id)
    }));
    let assign_id = must_some(
        graph
            .nodes
            .iter()
            .find_map(|node| matches!(&node.operation, PirOperation::Assign).then_some(node.id)),
    );

    let order = fallthrough_order(&graph);
    let boundary_step = must_some(order.iter().position(|id| *id == boundary_id));
    let assign_step = must_some(order.iter().position(|id| *id == assign_id));

    assert!(
        assign_step < boundary_step,
        "pinning the known limitation: flat PIR currently reaches the consuming \
         Assign before the tie boundary. If this now fails, the shared \
         dynamic-boundary splicing seam was fixed — replace this test with the \
         correct-order assertion rather than relaxing it. chain {order:?}"
    );
    Ok(())
}

/// Honest claim boundary against the *canonical* PIR-A path.
///
/// `lower_hir` (flat items) is the dormant back-compat path and is the one this
/// slice marks. `lower_hir_bodies` is canonical PIR-A, and it lowers from the
/// body arenas where tie/untie are still opaque calls — so it carries **no**
/// tie boundary. The concept ledger records `pir_a = "absent"` for both rows,
/// and this test is what keeps that row honest: if PIR-A ever does start
/// emitting a tie boundary, this fails and the ledger must be updated with it.
#[test]
fn canonical_pir_a_does_not_yet_carry_a_tie_boundary() -> TestResult {
    let mut parser = Parser::new("tie %hash, 'Tie::StdHash';\nuntie %hash;\n");
    let output = parser.parse_with_recovery();
    let hir = lower_ast(&output.ast);

    let flat = lower_hir(&hir);
    assert!(
        flat.nodes
            .iter()
            .any(|node| matches!(&node.operation, PirOperation::DynamicBoundary { .. })),
        "the flat items path must carry the boundary this slice adds"
    );

    let canonical = lower_hir_bodies(&hir);
    assert!(
        canonical.receipt.dynamic_boundary_counts.is_empty(),
        "canonical PIR-A carries no tie boundary today, so the ledger's \
         pir_a = \"absent\" must stay honest; got {:?}",
        canonical.receipt.dynamic_boundary_counts
    );
    Ok(())
}

/// The boundary belongs to the package and scope the tie *statement* sits in,
/// not to whatever its operands happen to leave behind.
///
/// Traversing children before pushing the boundary (required for evaluation
/// order) exposes this: a no-block `package Foo;` is legal inside a `do` block
/// in an operand, and it mutates the lowerer's `package_context` and pushes a
/// package scope that is never popped. Reading the context after traversal
/// would attribute the tie to the operand's package.
#[test]
fn tie_boundary_keeps_the_tie_site_package_and_scope() -> TestResult {
    let control = lower_source("package Main;\ntie %hash, 'Tie::StdHash', 42;\n");
    let (control_item, _) =
        must_some(sole_boundary(&control, DynamicBoundaryKind::TiedPlaceBinding));
    let expected_package = control_item.package_context.clone();
    let expected_scope = control_item.scope_context;
    assert_eq!(
        expected_package.as_deref(),
        Some("Main"),
        "control: a tie in package Main must be attributed to Main"
    );

    let leaky =
        lower_source("package Main;\ntie %hash, 'Tie::StdHash', do { package Other; 42 };\n");
    let (leaky_item, _) = must_some(sole_boundary(&leaky, DynamicBoundaryKind::TiedPlaceBinding));
    assert_eq!(
        leaky_item.package_context, expected_package,
        "a package declared inside a tie operand must not be attributed to the tie boundary"
    );
    assert_eq!(
        leaky_item.scope_context, expected_scope,
        "a scope opened inside a tie operand must not become the tie boundary's scope"
    );
    Ok(())
}

/// The same site-context guarantee for `untie`, whose target expression can
/// carry the same kind of declaration.
#[test]
fn untie_boundary_keeps_the_untie_site_package_and_scope() -> TestResult {
    let control = lower_source("package Main;\nuntie $hash{key};\n");
    let (control_item, _) =
        must_some(sole_boundary(&control, DynamicBoundaryKind::TiedPlaceRelease));
    assert_eq!(
        control_item.package_context.as_deref(),
        Some("Main"),
        "control: an untie in package Main must be attributed to Main"
    );

    let leaky = lower_source("package Main;\nuntie $hash{ do { package Other; 1 } };\n");
    let (leaky_item, _) = must_some(sole_boundary(&leaky, DynamicBoundaryKind::TiedPlaceRelease));
    assert_eq!(
        leaky_item.package_context, control_item.package_context,
        "a package declared inside an untie operand must not be attributed to the boundary"
    );
    assert_eq!(
        leaky_item.scope_context, control_item.scope_context,
        "a scope opened inside an untie operand must not become the boundary's scope"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Negative controls
// ---------------------------------------------------------------------------

/// Ordinary hash use must not be reported as tied. Without this control the
/// implementation could emit a boundary unconditionally and still pass every
/// positive test above.
#[test]
fn untied_source_emits_no_tie_boundary() -> TestResult {
    let file = lower_source("my %hash = (a => 1);\nmy $value = $hash{a};\nfoo();\n");
    assert!(
        boundaries_of(&file, DynamicBoundaryKind::TiedPlaceBinding).is_empty(),
        "a source with no tie must not emit a tie boundary"
    );
    assert!(
        boundaries_of(&file, DynamicBoundaryKind::TiedPlaceRelease).is_empty(),
        "a source with no untie must not emit an untie boundary"
    );
    Ok(())
}

/// A bareword `tied(%hash)` inspection is a different proposition from `tie`
/// and is not part of this slice; it must not be silently classified as a
/// binding.
#[test]
fn tie_boundary_does_not_fire_for_an_unrelated_call() -> TestResult {
    let file = lower_source("my $obj = tied(%hash);\n");
    assert!(
        boundaries_of(&file, DynamicBoundaryKind::TiedPlaceBinding).is_empty(),
        "tied() is an inspection, not a tie binding"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// The disposition registry must agree with the lowerer
// ---------------------------------------------------------------------------

#[test]
fn tie_and_untie_dispositions_report_dynamic_boundary() -> TestResult {
    for kind_name in ["Tie", "Untie"] {
        let disposition = must_some(disposition::disposition_for(kind_name));
        assert!(
            disposition.may_emit_boundary,
            "{kind_name} must record may_emit_boundary=true now that lower.rs emits one"
        );
        assert!(
            disposition.is_intentional,
            "{kind_name}'s lowering is a deliberate decision, not a fallthrough"
        );
        assert!(
            disposition.traverses_children,
            "{kind_name} must keep traversing children so arguments still lower"
        );
        assert_eq!(
            disposition.legacy_category(),
            disposition::LegacyCategory::DynamicBoundary,
            "{kind_name} must no longer be classified NotYetModeled"
        );
    }
    Ok(())
}
