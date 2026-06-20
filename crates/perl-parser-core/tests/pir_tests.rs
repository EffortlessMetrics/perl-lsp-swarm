//! PIR v0 lowering tests.
//!
//! These cover the [`PLSP-SPEC-0025`](../../../docs/specs/PLSP-SPEC-0025-pir-v0.md)
//! acceptance surface: source-anchor preservation, dynamic-boundary
//! preservation and links, visible unknown context, the operation families PIR
//! v0 lowers, conservative control-flow edges, and the lowering receipt.

use perl_parser_core::Parser;
use perl_parser_core::hir::{HirFile, lower_ast};
use perl_parser_core::pir::{
    PIR_RECEIPT_VERSION, PirCallee, PirContext, PirDynamicBoundaryKind, PirEdgeKind, PirGraph,
    PirLoweringMode, PirMethod, PirOperation, lower_hir, lower_hir_with_identity,
};
use perl_tdd_support::must_some;

fn lower(source: &str) -> PirGraph {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    let hir: HirFile = lower_ast(&output.ast);
    lower_hir(&hir)
}

fn op_names(graph: &PirGraph) -> Vec<&'static str> {
    graph.nodes.iter().map(|node| node.operation.name()).collect()
}

fn first_op<'a, T>(graph: &'a PirGraph, select: impl Fn(&'a PirOperation) -> Option<T>) -> T {
    must_some(graph.nodes.iter().find_map(|node| select(&node.operation)))
}

#[test]
fn empty_source_yields_empty_graph() {
    let graph = lower("");
    assert!(graph.is_empty());
    assert_eq!(graph.nodes.len(), 0);
    assert_eq!(graph.edges.len(), 0);
    assert_eq!(graph.receipt.node_count, 0);
    assert_eq!(graph.receipt.edge_count, 0);
    assert_eq!(graph.receipt.schema_version, PIR_RECEIPT_VERSION);
    assert_eq!(graph.receipt.lowering_mode, PirLoweringMode::HirV0);
    assert!(!graph.receipt.provider_behavior_changed);
}

#[test]
fn lexical_declaration_writes_lvalue_and_assigns() {
    let graph = lower("my $x = 1;");
    assert_eq!(op_names(&graph), vec!["LexicalWrite", "Assign"]);

    // The declared write target is a known lvalue; the statement-level
    // assignment is void. Neither is silently promoted past what HIR proves.
    assert_eq!(graph.nodes[0].context, PirContext::Lvalue);
    assert_eq!(graph.nodes[1].context, PirContext::Void);

    let name = first_op(&graph, |op| match op {
        PirOperation::LexicalWrite { name } => Some(name.clone()),
        _ => None,
    });
    assert_eq!(name.sigil, "$");
    assert_eq!(name.name, "x");
}

#[test]
fn our_declaration_is_a_stash_write_with_package() {
    let graph = lower("package Acme; our @items = (1, 2);");
    let symbol = first_op(&graph, |op| match op {
        PirOperation::StashWrite { symbol } => Some(symbol.clone()),
        _ => None,
    });
    assert_eq!(symbol.sigil, "@");
    assert_eq!(symbol.name, "items");
    assert_eq!(symbol.package.as_deref(), Some("Acme"));
}

#[test]
fn local_declaration_is_a_stash_write() {
    // `local` dynamically scopes a package/global slot, so it lowers to a stash
    // write, not a lexical write (unlike `my`/`state`).
    let graph = lower("local $x;");
    let symbol = first_op(&graph, |op| match op {
        PirOperation::StashWrite { symbol } => Some(symbol.clone()),
        _ => None,
    });
    assert_eq!(symbol.sigil, "$");
    assert_eq!(symbol.name, "x");
}

#[test]
fn named_call_splits_package_qualifier() {
    let graph = lower("Bar::baz();");
    let callee = first_op(&graph, |op| match op {
        PirOperation::Call { callee, .. } => Some(callee.clone()),
        _ => None,
    });
    assert_eq!(
        callee,
        PirCallee::Named { name: "baz".to_string(), package: Some("Bar".to_string()) }
    );
}

#[test]
fn deep_qualified_call_preserves_full_package_path() {
    // The qualifier split keeps the full package path (`rsplit_once`), so the
    // method name is just the final segment and the package is everything else.
    let graph = lower("A::B::foo();");
    let callee = first_op(&graph, |op| match op {
        PirOperation::Call { callee, .. } => Some(callee.clone()),
        _ => None,
    });
    assert_eq!(
        callee,
        PirCallee::Named { name: "foo".to_string(), package: Some("A::B".to_string()) }
    );
}

#[test]
fn unqualified_call_has_no_package() {
    let graph = lower("foo(1, 2, 3);");
    let (callee, arg_count) = first_op(&graph, |op| match op {
        PirOperation::Call { callee, arg_count } => Some((callee.clone(), *arg_count)),
        _ => None,
    });
    assert_eq!(callee, PirCallee::Named { name: "foo".to_string(), package: None });
    assert_eq!(arg_count, 3);

    // A call's runtime context is not provable from HIR; it stays Unknown.
    let context = must_some(
        graph
            .nodes
            .iter()
            .find(|node| matches!(node.operation, PirOperation::Call { .. }))
            .map(|node| node.context),
    );
    assert_eq!(context, PirContext::Unknown);
}

#[test]
fn method_call_lowers_receiver_and_method() {
    let graph = lower("$obj->frobnicate(1, 2);");
    let (method, arg_count) = first_op(&graph, |op| match op {
        PirOperation::MethodCall { method, arg_count, .. } => Some((method.clone(), *arg_count)),
        _ => None,
    });
    assert_eq!(method, PirMethod::Named("frobnicate".to_string()));
    assert_eq!(arg_count, 2);
}

#[test]
fn coderef_call_links_to_dynamic_boundary() {
    let graph = lower("my $cb; $cb->(1);");

    let call = must_some(graph.nodes.iter().find(|node| {
        matches!(node.operation, PirOperation::Call { callee: PirCallee::Dynamic, .. })
    }));

    // The dynamic call preserves a link to its boundary instead of guessing.
    let boundary_id = must_some(call.dynamic_boundary);
    let boundary = must_some(graph.node(boundary_id));
    let kind = must_some(match &boundary.operation {
        PirOperation::DynamicBoundary { kind, .. } => Some(*kind),
        _ => None,
    });
    assert_eq!(kind, PirDynamicBoundaryKind::DynamicCallee);

    // Exactly one boundary — PIR links HIR's boundary, it does not duplicate it.
    assert_eq!(graph.receipt.dynamic_boundary_counts.get("DynamicCallee"), Some(&1));

    // Control may not return through the boundary; the exit edge is preserved.
    assert!(graph.edges.iter().any(|edge| edge.from == boundary_id
        && edge.to.is_none()
        && edge.kind == PirEdgeKind::DynamicExit));
}

#[test]
fn each_coderef_call_links_its_own_boundary() {
    // Two independent coderef calls and a nested one. Each dynamic Call must
    // link to a distinct DynamicCallee boundary — never a stale one from an
    // earlier or unrelated call.
    let graph = lower("$a->(); $b->($c->());");

    let dynamic_calls: Vec<_> = graph
        .nodes
        .iter()
        .filter(|node| {
            matches!(node.operation, PirOperation::Call { callee: PirCallee::Dynamic, .. })
        })
        .collect();
    assert_eq!(dynamic_calls.len(), 3);

    let mut linked_boundaries = Vec::new();
    for call in dynamic_calls {
        let boundary_id = must_some(call.dynamic_boundary);
        let boundary = must_some(graph.node(boundary_id));
        let kind = must_some(match &boundary.operation {
            PirOperation::DynamicBoundary { kind, .. } => Some(*kind),
            _ => None,
        });
        assert_eq!(kind, PirDynamicBoundaryKind::DynamicCallee);
        linked_boundaries.push(boundary_id);
    }

    // Every link is to a distinct boundary node — no stale reuse.
    linked_boundaries.sort_by_key(|id| id.index());
    linked_boundaries.dedup();
    assert_eq!(linked_boundaries.len(), 3);
    assert_eq!(graph.receipt.dynamic_boundary_counts.get("DynamicCallee"), Some(&3));
}

#[test]
fn eval_string_is_a_dynamic_boundary() {
    let graph = lower(r#"eval "$code";"#);
    assert_eq!(graph.receipt.dynamic_boundary_counts.get("EvalExpression"), Some(&1));
}

#[test]
fn symbolic_reference_is_a_dynamic_boundary() {
    let graph = lower("no strict 'refs'; my $v = ${$name};");
    assert_eq!(graph.receipt.dynamic_boundary_counts.get("SymbolicReference"), Some(&1));
}

#[test]
fn typeglob_assignment_is_a_runtime_stash_mutation_boundary() {
    let graph = lower("*alias = $thing;");
    assert_eq!(graph.receipt.dynamic_boundary_counts.get("RuntimeStashMutation"), Some(&1));
}

#[test]
fn autoload_declaration_is_a_dynamic_boundary() {
    let graph = lower("sub AUTOLOAD { }");
    assert_eq!(graph.receipt.dynamic_boundary_counts.get("Autoload"), Some(&1));
}

#[test]
fn dynamic_boundary_nodes_anchor_to_the_boundary_range() {
    let graph = lower(r#"eval "$code";"#);
    let boundary = must_some(
        graph
            .nodes
            .iter()
            .find(|node| matches!(node.operation, PirOperation::DynamicBoundary { .. })),
    );
    assert_eq!(boundary.source_anchor.kind.name(), "DynamicBoundary");
    assert!(boundary.source_anchor.is_anchored());
}

#[test]
fn every_lowered_node_preserves_a_source_anchor() {
    let graph = lower("package Foo; my $x = bar(); $obj->m(); our $y;");
    assert!(graph.nodes.iter().all(|node| node.source_anchor.is_anchored()));
    assert_eq!(graph.receipt.source_anchor_coverage.unanchored, 0);
    assert_eq!(graph.receipt.source_anchor_coverage.anchored, graph.nodes.len());
    assert_eq!(graph.receipt.source_anchor_coverage.total(), graph.nodes.len());
}

#[test]
fn unlowered_constructs_are_counted_not_dropped() {
    let graph = lower("package Foo; use strict; sub f {}");
    // None of these lower to PIR operations in v0...
    assert!(graph.is_empty());
    // ...but they are visible in the receipt rather than silently dropped.
    let unsupported = &graph.receipt.unsupported_construct_counts;
    assert_eq!(unsupported.get("PackageDecl"), Some(&1));
    assert_eq!(unsupported.get("UseDecl"), Some(&1));
    assert_eq!(unsupported.get("SubDecl"), Some(&1));
}

#[test]
fn receipt_counts_are_consistent_with_nodes() {
    let graph = lower("my $x = 1; foo(); $obj->m(); eval '1';");

    let op_total: usize = graph.receipt.operation_counts.values().sum();
    assert_eq!(op_total, graph.nodes.len());
    assert_eq!(graph.receipt.node_count, graph.nodes.len());

    let ctx_total: usize = graph.receipt.context_counts.values().sum();
    assert_eq!(ctx_total, graph.nodes.len());

    assert_eq!(graph.receipt.edge_count, graph.edges.len());
    assert!(!graph.receipt.provider_behavior_changed);
    assert!(graph.receipt.ambient_inputs.is_empty());
}

#[test]
fn consecutive_nodes_in_a_scope_are_linked_by_fallthrough() {
    let graph = lower("foo(); bar(); baz();");
    let fallthroughs =
        graph.edges.iter().filter(|edge| edge.kind == PirEdgeKind::Fallthrough).count();
    // Three sequential calls in the file scope produce two fallthrough edges.
    assert_eq!(fallthroughs, 2);
}

#[test]
fn lowering_is_deterministic() {
    let mut parser = Parser::new("package Foo; my $x = bar(); $obj->m(); eval '1'; our @z;");
    let output = parser.parse_with_recovery();
    let hir = lower_ast(&output.ast);

    let first = lower_hir(&hir);
    let second = lower_hir(&hir);
    assert_eq!(first, second);
}

#[test]
fn source_identity_is_threaded_into_the_receipt() {
    let mut parser = Parser::new("my $x = 1;");
    let output = parser.parse_with_recovery();
    let hir = lower_ast(&output.ast);

    let graph = lower_hir_with_identity(&hir, Some("fixture://demo.pl".to_string()));
    assert_eq!(graph.receipt.source_identity.as_deref(), Some("fixture://demo.pl"));

    let anonymous = lower_hir(&hir);
    assert!(anonymous.receipt.source_identity.is_none());
}

#[test]
fn node_lookup_round_trips() {
    let graph = lower("my $x = 1;");
    for node in &graph.nodes {
        assert_eq!(graph.node(node.id), Some(node));
    }
    assert!(graph.node(perl_parser_core::pir::PirId::from_index(9999)).is_none());
}
