#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
//! Canonical body-HIR and PIR-A proof for `try` / `catch` / `finally` regions (#15567).
//!
//! Controlling issue: #15567, first PR-sized slice of #6661.
//!
//! # What this file proves
//!
//! Before this slice, `NodeKind::Try` reached canonical body HIR through a
//! call-shaped arm that lowered each region with `lower_expr` rather than
//! `lower_nested_block`. `lower_expr` has no `Block` arm, so every region
//! collapsed into a childless `Opaque { ast_kind: "Block" }` and the construct
//! was emitted as `HirExpr::Call { ast_kind: "Try" }`. Two things followed:
//!
//! 1. every statement inside the try, catch, and finally regions was discarded
//!    before PIR-A ever saw it, and
//! 2. the construct was counted in the PIR receipt under `Call`, so a consumer
//!    could not tell a `try` was present, let alone that three regions were
//!    dropped.
//!
//! The load-bearing assertion is [`try_region_writes_reach_pir`]: the probe
//! source performs four writes to `$x`, one per region plus the declaration.
//! Current `main` emitted exactly **one**. Anything less than four means a
//! region is being dropped again.
//!
//! # Deliberate non-claims
//!
//! PIR v0 models the try regions, not exceptional control flow. Handler and
//! finally entry therefore use `PirEdgeKind::Unknown`, never `Fallthrough`, and
//! the construct records a `TryExceptionalEdges` boundary. The tests below
//! assert that boundary is present precisely so a future change cannot quietly
//! upgrade the claim to a complete exception CFG.

use perl_parser_core::Parser;
use perl_parser_core::hir::{
    AccessMode, BodyOwnerKind, HirBlockId, HirBody, HirCatchHandler, HirExpr, HirFile, HirStmt,
    HirStmtId, HirVariable, Sigil, VariableKind, lower_ast,
};
use perl_parser_core::pir::{PirEdgeKind, PirGraph, PirOperation, lower_hir_bodies};

// ──────────────────────────────────────────────────────────────────────────────
// Helpers
// ──────────────────────────────────────────────────────────────────────────────

/// The probe source from #15567: one write to `$x` in each of the four regions
/// (declaration, try, catch, finally).
const FOUR_WRITE_PROBE: &str = r#"
sub f {
    my $x = 1;
    try {
        $x = 2;
    }
    catch ($e) {
        $x = 3;
    }
    finally {
        $x = 4;
    }
    return $x;
}
"#;

fn lower(source: &str) -> HirFile {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    lower_ast(&output.ast)
}

fn lower_pir(source: &str) -> PirGraph {
    lower_hir_bodies(&lower(source))
}

/// The body owned by `sub f`, i.e. the one that is not the program root.
fn subroutine_body(file: &HirFile) -> &HirBody {
    file.bodies
        .iter()
        .find(|body| matches!(body.owner, BodyOwnerKind::Subroutine { .. }))
        .unwrap_or_else(|| panic!("expected a subroutine body, got {} bodies", file.bodies.len()))
}

/// Return the one `HirExpr::Try` in `body`, or panic with what was found.
fn try_expr(body: &HirBody) -> &HirExpr {
    let mut found = body.exprs.iter().filter(|e| matches!(e, HirExpr::Try { .. }));
    let first = found
        .next()
        .unwrap_or_else(|| panic!("no HirExpr::Try in body; exprs = {:?}", collect_kinds(body)));
    assert!(found.next().is_none(), "expected exactly one HirExpr::Try");
    first
}

fn collect_kinds(body: &HirBody) -> Vec<String> {
    body.exprs.iter().map(|e| format!("{e:?}")).collect()
}

/// The statement ids directly contained in `block`.
fn block_stmts(body: &HirBody, block: HirBlockId) -> &[HirStmtId] {
    body.blocks
        .get(block.0)
        .unwrap_or_else(|| panic!("block {block:?} missing from arena"))
        .stmts
        .as_slice()
}

/// Count `LexicalWrite` PIR nodes naming `name`.
fn lexical_writes(graph: &PirGraph, name: &str) -> usize {
    graph
        .nodes
        .iter()
        .filter(
            |n| matches!(&n.operation, PirOperation::LexicalWrite { name: n } if n.name == name),
        )
        .count()
}

/// The lexical PIR operation names touching `name`, in emission order.
fn lexical_op_names(graph: &PirGraph, name: &str) -> Vec<&'static str> {
    graph
        .nodes
        .iter()
        .filter_map(|n| match &n.operation {
            PirOperation::LexicalWrite { name: n } if n.name == name => Some("LexicalWrite"),
            PirOperation::LexicalRead { name: n } if n.name == name => Some("LexicalRead"),
            _ => None,
        })
        .collect()
}

// ──────────────────────────────────────────────────────────────────────────────
// 1. The load-bearing behavioral claim
// ──────────────────────────────────────────────────────────────────────────────

/// Writes inside try / catch / finally must reach PIR-A.
///
/// This is the regression gate for the whole slice. `main` emitted 1 of 4.
#[test]
fn try_region_writes_reach_pir() {
    let graph = lower_pir(FOUR_WRITE_PROBE);
    let writes = lexical_writes(&graph, "x");
    assert_eq!(
        writes,
        4,
        "expected one LexicalWrite($x) per region (declaration, try, catch, finally); \
         got {writes}. Fewer than 4 means a region's statements were dropped before PIR-A. \
         nodes = {:?}",
        graph.nodes.iter().map(|n| n.operation.name()).collect::<Vec<_>>()
    );
}

/// The construct must no longer be counted as an unsupported `Call`, and must
/// record its real residue instead.
///
/// The `Call` claim is made against a try-free control rather than against an
/// absolute count: the enclosing `sub f { … }` shell itself lowers as
/// `HirExpr::Call { ast_kind: "Subroutine" }`, so the probe legitimately
/// reports one `Call` with or without the try. Comparing the two is what
/// isolates the try's own contribution — on `main` the probe reported `Call: 2`
/// against this control's `Call: 1`.
#[test]
fn try_records_exceptional_edge_boundary_not_a_call() {
    /// Same four writes to `$x`, same `sub` shell, no try construct.
    const TRY_FREE_CONTROL: &str = "sub f { my $x = 1; $x = 2; $x = 3; $x = 4; return $x; }";

    let probe = lower_pir(FOUR_WRITE_PROBE);
    let control = lower_pir(TRY_FREE_CONTROL);

    let probe_unsupported = &probe.receipt.unsupported_construct_counts;
    let control_unsupported = &control.receipt.unsupported_construct_counts;

    assert_eq!(
        probe_unsupported.get("TryExceptionalEdges"),
        Some(&1),
        "the try construct must record exactly one exceptional-edge boundary; \
         unsupported = {probe_unsupported:?}"
    );
    assert_eq!(
        control_unsupported.get("TryExceptionalEdges"),
        None,
        "a body with no try must record no exceptional-edge boundary; \
         unsupported = {control_unsupported:?}"
    );
    assert_eq!(
        probe_unsupported.get("Call"),
        control_unsupported.get("Call"),
        "the try construct must contribute no unsupported Call beyond the `sub` shell the \
         control also has: probe = {probe_unsupported:?}, control = {control_unsupported:?}"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// 2. Region structure in canonical body HIR
// ──────────────────────────────────────────────────────────────────────────────

/// Each region is a distinct block carrying its real statements.
#[test]
fn try_regions_are_distinct_blocks_with_real_statements() {
    let file = lower(FOUR_WRITE_PROBE);
    let body = subroutine_body(&file);

    let HirExpr::Try { body: try_block, catch_handlers, finally_block } = try_expr(body) else {
        unreachable!("try_expr returns only HirExpr::Try")
    };

    assert_eq!(catch_handlers.len(), 1, "one catch handler");
    let finally = finally_block.expect("finally block present");
    let catch_block = catch_handlers[0].block;

    assert_ne!(*try_block, catch_block, "try and catch must be distinct blocks");
    assert_ne!(catch_block, finally, "catch and finally must be distinct blocks");
    assert_ne!(*try_block, finally, "try and finally must be distinct blocks");

    for (label, block) in [("try", *try_block), ("catch", catch_block), ("finally", finally)] {
        let stmts = block_stmts(body, block);
        assert_eq!(
            stmts.len(),
            1,
            "{label} region must carry its one real statement, not a childless Opaque block"
        );
    }
}

/// `catch ($e)` produces a write place with the exact byte range of `$e`, and
/// the sigil is split out of the name.
#[test]
fn catch_binding_is_a_source_exact_write_place() {
    let source = "sub f { try { 1 } catch ($err) { 2 } }";
    let file = lower(source);
    let body = subroutine_body(&file);

    let HirExpr::Try { catch_handlers, .. } = try_expr(body) else { unreachable!() };
    let HirCatchHandler { binding, .. } = &catch_handlers[0];
    let binding = binding.expect("catch ($err) must introduce a binding");

    let Some(HirExpr::Variable(HirVariable { sigil, name, kind, access })) =
        body.exprs.get(binding.0)
    else {
        panic!("catch binding must lower to a Variable place, got {:?}", body.exprs.get(binding.0))
    };

    assert_eq!(*sigil, Sigil::Scalar, "sigil lives in its own field");
    assert_eq!(name, "err", "name must not retain the sigil");
    assert_eq!(*kind, VariableKind::Lexical);
    assert_eq!(*access, AccessMode::Write, "the binding is written, not read");

    // Source-exactness: the range must cover `$err`, not the `catch (...)` header.
    let range = body.source_map.expr_range(binding).expect("binding must be anchored");
    let expected_start = source.find("$err").expect("probe contains $err");
    assert_eq!(
        (range.start, range.end),
        (expected_start, expected_start + "$err".len()),
        "binding must be anchored at the variable token, not the catch header; \
         got {:?}",
        &source[range.start..range.end]
    );
}

/// The bare `catch { }` form introduces no binding.
#[test]
fn bare_catch_has_no_binding() {
    let file = lower("sub f { try { 1 } catch { 2 } }");
    let body = subroutine_body(&file);
    let HirExpr::Try { catch_handlers, .. } = try_expr(body) else { unreachable!() };
    assert_eq!(catch_handlers.len(), 1);
    assert!(catch_handlers[0].binding.is_none(), "bare `catch` introduces no exception binding");
}

/// `try`/`finally` with no catch, and `try`/`catch` with no finally.
#[test]
fn optional_regions_are_independent() {
    let file = lower("sub f { try { 1 } finally { 2 } }");
    let body = subroutine_body(&file);
    let HirExpr::Try { catch_handlers, finally_block, .. } = try_expr(body) else { unreachable!() };
    assert!(catch_handlers.is_empty(), "no catch handlers");
    assert!(finally_block.is_some(), "finally present");

    let file = lower("sub f { try { 1 } catch ($e) { 2 } }");
    let body = subroutine_body(&file);
    let HirExpr::Try { catch_handlers, finally_block, .. } = try_expr(body) else { unreachable!() };
    assert_eq!(catch_handlers.len(), 1, "one catch handler");
    assert!(finally_block.is_none(), "no finally");
}

/// A `try` nested inside a catch handler keeps both regions' effects.
#[test]
fn nested_try_inside_catch_keeps_both_regions() {
    let graph = lower_pir(
        r#"
sub f {
    my $outer = 0;
    try {
        $outer = 1;
    }
    catch ($e) {
        try {
            $outer = 2;
        }
        catch ($inner) {
            $outer = 3;
        }
    }
}
"#,
    );
    assert_eq!(
        lexical_writes(&graph, "outer"),
        4,
        "declaration plus one write per nested region must all reach PIR-A"
    );
    assert_eq!(
        graph.receipt.unsupported_construct_counts.get("TryExceptionalEdges"),
        Some(&2),
        "each try region records its own exceptional-edge boundary"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// 3. Control-flow honesty
// ──────────────────────────────────────────────────────────────────────────────

/// A handler is never an unconditional successor of the try body.
///
/// Negative control for the CFG claim: if handler entry were lowered with
/// `Fallthrough`, PIR would assert that the catch block always runs after the
/// try block, which is wrong for every non-throwing execution.
#[test]
fn handler_and_finally_entry_are_not_fallthrough() {
    let graph = lower_pir(FOUR_WRITE_PROBE);

    // Both regions are reached from the try body's exit by an Unknown edge, and
    // by nothing else. Identify them structurally rather than by node index.
    let unknown: Vec<(u32, u32)> = graph
        .edges
        .iter()
        .filter(|e| e.kind == PirEdgeKind::Unknown)
        .filter_map(|e| e.to.map(|t| (e.from.index(), t.index())))
        .collect();

    assert_eq!(
        unknown.len(),
        2,
        "exactly two conservative region edges are expected — one into the catch handler and \
         one into the finally block; got {unknown:?}"
    );

    let try_exit = unknown[0].0;
    assert!(
        unknown.iter().all(|(from, _)| *from == try_exit),
        "both region edges must leave the same try-body exit node; got {unknown:?}"
    );

    // The catch handler's entry is its `$e` binding write; the finally block has
    // no binding, so its entry is its first statement's write.
    let catch_entry = graph
        .nodes
        .iter()
        .position(
            |n| matches!(&n.operation, PirOperation::LexicalWrite { name } if name.name == "e"),
        )
        .expect("catch binding must emit a write for $e");
    assert!(
        unknown.iter().any(|(_, to)| *to as usize == catch_entry),
        "the catch region must be entered at its binding by an Unknown edge; got {unknown:?}"
    );

    // The decisive negative: nothing may leave the try body by Fallthrough.
    // A Fallthrough here would assert that the catch or finally block always
    // runs after the try body, which is wrong for every non-throwing execution.
    let fallthrough_from_try_exit: Vec<_> = graph
        .edges
        .iter()
        .filter(|e| e.from.index() == try_exit && e.kind == PirEdgeKind::Fallthrough)
        .map(|e| e.to.map(|t| t.index()))
        .collect();
    assert!(
        fallthrough_from_try_exit.is_empty(),
        "the try body must have no unconditional successor: a catch or finally block is not \
         reached by Fallthrough. offending edges to {fallthrough_from_try_exit:?}"
    );
}

/// A statement after a `try` inherits no predecessor — exactly as a statement
/// after an `if`/`else` does not.
///
/// Raised in review: after `HirExpr::Try`, `last_in_scope` is cleared, so the
/// next statement becomes a new graph root even though the non-throwing path
/// and a completed handler can both reach it.
///
/// The mechanism is real, but it is not this construct's invention and not a
/// wrong claim — it is the conservative convention `Branch` already established
/// (`lower.rs`: "The branch has no unconditional successor ... matches pre-#4795
/// behavior"). A missing edge understates reachability; a `Fallthrough` here
/// would overstate it, asserting an unconditional successor that is wrong
/// whenever the try body throws. PIR v0 has no join node to express "whichever
/// region completed", and inventing one is the exceptional-edge taxonomy of
/// #6661, not this slice.
///
/// So this test does not assert the continuation is orphaned as if that were
/// desirable. It pins `try` to the `if` precedent: whatever PIR v0 gives the
/// statement after a branch, it must give the statement after a try. If #6661
/// later teaches `Branch` to join its arms, this test fails and `Try` must be
/// taught the same thing in the same change — which is the real risk worth
/// guarding, since a silently weaker `try` is what a consumer could not see.
#[test]
fn post_try_continuation_matches_the_branch_precedent() {
    // Same shell, same four writes, same trailing statement: the only
    // difference is the construct in the middle.
    const AFTER_IF: &str = r#"
sub f {
    my $x = 0;
    if (c()) { $x = 1; } else { $x = 2; }
    $x = 3;
}
"#;
    const AFTER_TRY: &str = r#"
sub f {
    my $x = 0;
    try { $x = 1; } catch ($e) { $x = 2; }
    $x = 3;
}
"#;

    /// Incoming edge count of the last `LexicalWrite($x)` — the trailing
    /// `$x = 3`, i.e. the continuation after the construct.
    fn continuation_predecessors(source: &str) -> usize {
        let graph = lower_pir(source);
        let continuation = graph
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| {
                matches!(&n.operation, PirOperation::LexicalWrite { name } if name.name == "x")
            })
            .map(|(i, _)| i)
            .next_back()
            .expect("the trailing $x = 3 must reach PIR");
        graph
            .edges
            .iter()
            .filter(|e| e.to.is_some_and(|t| t.index() as usize == continuation))
            .count()
    }

    let after_if = continuation_predecessors(AFTER_IF);
    let after_try = continuation_predecessors(AFTER_TRY);

    assert_eq!(
        after_try, after_if,
        "the statement after a try must have the same number of PIR predecessors as the \
         statement after an if/else ({after_if}); got {after_try}. A try that is treated \
         differently from the established Branch convention is the regression this guards: \
         either both join their regions or neither does."
    );
}

/// A try body that models no PIR node of its own must still leave its handler
/// reachable.
///
/// `try { 1 }` has an opaque literal body that emits no PIR node, so the
/// region's "last modeled node" — the ordinary edge source — does not exist.
/// The handler must then fall back to the node control entered the try from.
/// Without that fallback the handler region carries no incoming edge at all,
/// which a CFG consumer reads as unreachable rather than as conditionally
/// reached, and `PirEdgeKind::Unknown` exists precisely so such an edge is not
/// dropped silently.
#[test]
fn handler_stays_reachable_when_try_body_models_no_node() {
    let graph = lower_pir("sub f { my $x = 0; try { 1 } catch ($e) { $x = 2; } }");

    let handler_entry = graph
        .nodes
        .iter()
        .position(
            |n| matches!(&n.operation, PirOperation::LexicalWrite { name } if name.name == "e"),
        )
        .expect("catch binding must emit a write for $e");

    let incoming: Vec<PirEdgeKind> = graph
        .edges
        .iter()
        .filter(|e| e.to.is_some_and(|t| t.index() as usize == handler_entry))
        .map(|e| e.kind)
        .collect();

    assert!(
        incoming.contains(&PirEdgeKind::Unknown),
        "a handler after a node-less try body must fall back to the try's entry predecessor \
         rather than being orphaned; incoming = {incoming:?}"
    );
    assert!(
        !incoming.contains(&PirEdgeKind::Fallthrough),
        "the fallback must not claim ordinary control flow; incoming = {incoming:?}"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// 3b. The catch binding is a real binding, not a synthesized place
// ──────────────────────────────────────────────────────────────────────────────

/// Reads of the catch variable inside its handler must resolve to the binding.
///
/// The binding is lowered as a *lexical write place*. If the first pass does not
/// register it in the scope graph, reads of the same variable in the same
/// handler resolve against the enclosing scope instead and come back as
/// `StashRead` — a body that writes a lexical and reads a package global under
/// one name. That is an internally inconsistent fact set, and exactly the class
/// of untruth this slice exists to remove.
///
/// The `foreach` iterator binding is the control: it is the precedent this
/// lowering follows, and it is consistent because the parser emits a real child
/// node for `my $i` that the first pass records. The catch variable is tuple
/// metadata on `NodeKind::Try`, so it must be registered deliberately.
#[test]
fn catch_binding_reads_resolve_lexically_like_a_foreach_binding() {
    let catch_ops =
        lexical_op_names(&lower_pir("sub f { try { g() } catch ($e) { h($e); } }"), "e");
    let foreach_ops = lexical_op_names(&lower_pir("sub f { for my $i (1, 2) { h($i); } }"), "i");

    assert_eq!(
        catch_ops,
        ["LexicalWrite", "LexicalRead"],
        "the catch binding and its read must both be lexical; got {catch_ops:?}. \
         A missing LexicalRead means the read resolved as a package access."
    );
    assert_eq!(
        catch_ops, foreach_ops,
        "a catch binding must behave like the foreach iterator binding it is modeled on: \
         catch = {catch_ops:?}, foreach = {foreach_ops:?}"
    );
}

/// `catch ($e)` shadows an outer `my $e` rather than reusing it.
///
/// Negative control for the scope frame: without a handler-scoped frame the
/// read resolves to the outer binding, which is both wrong and invisible in the
/// PIR operation names alone — both spellings produce `LexicalRead`. This
/// asserts the resolved binding identity, not just the operation kind.
#[test]
fn catch_binding_shadows_an_outer_lexical_of_the_same_name() {
    let source = "sub f { my $e = 1; try { g() } catch ($e) { h($e); } }";
    let file = lower(source);
    let graph = &file.scope_graph;

    let outer_start = source.find("my $e").map(|i| i + 3).expect("probe declares my $e");
    let catch_start = source.find("catch ($e)").map(|i| i + 7).expect("probe has catch ($e)");

    let binding_at = |start: usize| {
        graph.bindings.iter().find(|b| b.name == "e" && b.range.start == start).unwrap_or_else(
            || {
                panic!(
                    "no binding for $e at byte {start}; bindings = {:?}",
                    graph.bindings.iter().map(|b| (&b.name, b.range.start)).collect::<Vec<_>>()
                )
            },
        )
    };

    let outer_binding = binding_at(outer_start);
    let catch_binding = binding_at(catch_start);
    assert_ne!(
        outer_binding.id, catch_binding.id,
        "the catch binding must be distinct from the outer declaration"
    );
    assert_ne!(
        outer_binding.scope_id, catch_binding.scope_id,
        "the catch binding must live in its own handler-scoped frame"
    );
    assert_eq!(
        catch_binding.shadows,
        Some(outer_binding.id),
        "the catch binding must record that it shadows the outer $e"
    );

    // The read inside the handler must resolve to the catch binding.
    let read_start = source.find("h($e)").map(|i| i + 2).expect("probe reads $e in the handler");
    let read = graph
        .references
        .iter()
        .find(|r| r.name == "e" && r.range.start == read_start)
        .expect("handler read of $e must be recorded");
    assert_eq!(
        read.resolved_binding,
        Some(catch_binding.id),
        "the handler read must resolve to the catch binding, not the outer $e"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// 4. Support-claim ratchet
// ──────────────────────────────────────────────────────────────────────────────

/// The declaration inside a try region is still a real `Let` statement, proving
/// the region body went through ordinary statement lowering rather than being
/// re-wrapped as an expression argument.
#[test]
fn declarations_inside_try_lower_as_statements() {
    let file = lower("sub f { try { my $inner = 1; } catch ($e) { } }");
    let body = subroutine_body(&file);
    let HirExpr::Try { body: try_block, .. } = try_expr(body) else { unreachable!() };
    let stmts = block_stmts(body, *try_block);
    assert_eq!(stmts.len(), 1, "one statement in the try region");
    let stmt = body.stmts.get(stmts[0].0).expect("statement in arena");
    assert!(
        matches!(stmt, HirStmt::Let { name, .. } if name == "inner"),
        "a declaration inside try must lower as HirStmt::Let, got {stmt:?}"
    );
}
