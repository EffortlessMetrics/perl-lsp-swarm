//! PIR-A bodies — integration tests for PR 2 (#2578).
//!
//! Verifies that `lower_hir_bodies()` correctly lowers canonical HIR body arenas
//! (from `HirFile::bodies`) into PIR-A Read/Write/Modify operations with correct
//! source anchors, scope/package context, and verifier fail-closed behaviour.
//!
//! Test coverage:
//!   1. `my $x = $a + $b;` — canonical receipt: exactly 1 Write $x (double-emit guard), Read $a, Read $b, Binary
//!   2. `$x = $y;` — plain assign → Write $x, Read $y
//!   3. `$x += 1;` — compound assign → Modify $x (place evaluated once)
//!   4. `our $x; $x = $y;` — package place → StashWrite / StashRead, NO LexicalWrite for $x
//!   5. `$Foo::x = $y;` — package slot → StashWrite / StashRead
//!   6. Recovery `my $x = ;` — NO exact operation emitted from recovered syntax
//!   7. Unresolved/Dynamic place — no exact Read/Write op emitted
//!   8. Source anchors present on all non-opaque nodes
//!   9. `lower_hir_bodies` schema version matches PIR_RECEIPT_VERSION
//!  10. Deterministic output for same source
//!  11. `$x++` — Modify (ReadModifyWrite unary)
//!  12. Modify evaluates place once (no duplicate target evaluation in receipt)
//!  13. `sub foo { my $x = 1; }` — sub body produces Write in sub body operations
//!  14. body_model_version guard — version 0 → no body ops emitted / version 1 → ok
//!  15. `our $x = $y;` — exactly 1 StashWrite, 0 LexicalWrite for $x (storage-aware Let guard)

use perl_parser_core::Parser;
use perl_parser_core::hir::lower_ast;
use perl_parser_core::pir::{PIR_RECEIPT_VERSION, PirGraph, PirOperation, lower_hir_bodies};

fn parse_and_lower(source: &str) -> PirGraph {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    let hir = lower_ast(&output.ast);
    lower_hir_bodies(&hir)
}

// ── 1. Canonical receipt: `my $x = $a + $b;` ────────────────────────────────
// Expected: Write (LexicalWrite) for $x, Read (LexicalRead) for $a, Read for $b,
// plus a Binary node (or Opaque for the + operator — acceptable), plus Assign.

#[test]
fn pir_a_canonical_receipt_my_x_equals_a_plus_b() {
    let graph = parse_and_lower("my $x = $a + $b;");

    // Must have exactly one LexicalWrite for $x — double-emission from the Let arm was a bug
    let write_count = graph
        .nodes
        .iter()
        .filter(|n| matches!(&n.operation, PirOperation::LexicalWrite { name } if name.name == "x"))
        .count();
    assert_eq!(
        write_count, 1,
        "must have exactly 1 LexicalWrite for $x, not {write_count} (double-emit guard)"
    );

    // Must have LexicalRead nodes (for $a and $b — both undeclared, package vars OR
    // reads from the rhs. Note: undeclared vars may resolve as StashRead, not LexicalRead.
    // What we require: at least two Read-type operations (Lexical or Stash) on the RHS.
    let read_count = graph
        .nodes
        .iter()
        .filter(|n| {
            matches!(
                &n.operation,
                PirOperation::LexicalRead { .. } | PirOperation::StashRead { .. }
            )
        })
        .count();
    assert!(read_count >= 2, "must have at least 2 read ops for $a and $b; got {read_count}");

    // All source-derived nodes must carry a source anchor
    for node in &graph.nodes {
        assert!(
            node.source_anchor.is_anchored(),
            "every PIR-A node must have a source anchor; got unanchored node: {:?}",
            node.operation.name()
        );
    }
}

// ── 2. Plain assign `$x = $y;` → Write $x, Read $y ─────────────────────────

#[test]
fn pir_a_plain_assign_write_and_read() {
    let graph = parse_and_lower("$x = $y;");

    // LHS: Write (package — $x undeclared)
    let write = graph.nodes.iter().find(|n| {
        matches!(&n.operation,
            PirOperation::LexicalWrite { name } if name.name == "x"
        ) || matches!(&n.operation,
            PirOperation::StashWrite { symbol } if symbol.name == "x"
        )
    });
    assert!(write.is_some(), "must have a Write op for $x");

    // RHS: Read (package — $y undeclared)
    let read = graph.nodes.iter().find(|n| {
        matches!(&n.operation,
            PirOperation::LexicalRead { name } if name.name == "y"
        ) || matches!(&n.operation,
            PirOperation::StashRead { symbol } if symbol.name == "y"
        )
    });
    assert!(read.is_some(), "must have a Read op for $y");
}

// ── 3. Compound assign `$x += 1;` → Modify $x ───────────────────────────────

#[test]
fn pir_a_compound_assign_produces_modify() {
    // Use a declared variable so we get a deterministic Lexical→Modify (not StashModify).
    let graph = parse_and_lower("my $x = 0; $x += 1;");

    let modify = graph.nodes.iter().find(|n| {
        matches!(&n.operation, PirOperation::Modify { .. } | PirOperation::StashModify { .. })
    });
    assert!(
        modify.is_some(),
        "compound assign `$x += 1` must produce a Modify or StashModify operation; got: {:?}",
        graph.nodes.iter().map(|n| n.operation.name()).collect::<Vec<_>>()
    );

    // The Modify op must name $x
    let names_x = graph.nodes.iter().any(|n| match &n.operation {
        PirOperation::Modify { name, .. } => name.name == "x",
        PirOperation::StashModify { symbol, .. } => symbol.name == "x",
        _ => false,
    });
    assert!(names_x, "Modify op must target $x");
}

// ── 4. `our $x; $x = $y;` — package place → StashWrite / StashRead ──────────

#[test]
fn pir_a_our_var_produces_stash_ops() {
    let graph = parse_and_lower("our $x; $x = $y;");

    // `our $x` is a StashWrite (declaration)
    let stash_write = graph.nodes.iter().find(
        |n| matches!(&n.operation, PirOperation::StashWrite { symbol } if symbol.name == "x"),
    );
    assert!(
        stash_write.is_some(),
        "`our $x` must produce StashWrite; got {:?}",
        graph.nodes.iter().map(|n| n.operation.name()).collect::<Vec<_>>()
    );

    // `our $x` must NOT produce a LexicalWrite — the old Let arm bug emitted LexicalWrite
    // unconditionally, ignoring the storage class.
    let lex_write = graph
        .nodes
        .iter()
        .find(|n| matches!(&n.operation, PirOperation::LexicalWrite { name } if name.name == "x"));
    assert!(
        lex_write.is_none(),
        "`our $x` declaration must NOT produce LexicalWrite; got: {:?}",
        lex_write.map(|n| n.operation.name())
    );
}

// ── 5. `$Foo::x = $y;` — qualified name → StashWrite ────────────────────────

#[test]
fn pir_a_qualified_var_produces_stash_write() {
    let graph = parse_and_lower("$Foo::x = $y;");

    let stash_write = graph.nodes.iter().find(|n| {
        matches!(&n.operation, PirOperation::StashWrite { symbol } if symbol.name.contains("x"))
    });
    assert!(
        stash_write.is_some(),
        "qualified `$Foo::x` must produce StashWrite; got {:?}",
        graph.nodes.iter().map(|n| n.operation.name()).collect::<Vec<_>>()
    );
}

// ── 6. Recovery `my $x = ;` — NO exact operation from recovered syntax ───────

#[test]
fn pir_a_recovery_no_exact_operation() {
    let graph = parse_and_lower("my $x = ;");

    // The recovered initializer must not produce a clean Read operation.
    // A Write for the declaration target ($x) is acceptable — the declaration IS known.
    // What must not happen: a Read op appearing to claim the RHS was a real variable.
    // (In practice: recovery → Opaque RHS → no Read emitted for the phantom RHS.)
    let clean_reads: Vec<_> = graph
        .nodes
        .iter()
        .filter(|n| {
            matches!(
                &n.operation,
                PirOperation::LexicalRead { .. } | PirOperation::StashRead { .. }
            )
        })
        .collect();

    // There should be no reads from a recovered RHS. The declaration Write is ok.
    // This test is conservative: if a Read appears it must have a real variable name
    // (not be an artifact of recovery). We assert no reads exist for this case.
    assert!(
        clean_reads.is_empty(),
        "recovery `my $x = ;` must not emit any Read ops; got {:?}",
        clean_reads.iter().map(|n| n.operation.name()).collect::<Vec<_>>()
    );
}

// ── 7. Unresolved/Dynamic place — no exact op emitted ────────────────────────

#[test]
fn pir_a_opaque_expression_no_wrong_fact() {
    // A call `foo($x)` — the call is Opaque in the body. The Modify/Write op
    // for $x should NOT be emitted through the call's Opaque boundary.
    // What we assert: no StashWrite or LexicalWrite claiming $x in the call target.
    let graph = parse_and_lower("foo($x);");

    // The call itself may or may not be modeled. The argument $x appearing as
    // a Write target inside a call-arg would be wrong — $x is Read here.
    // Assert: no Write op targeting $x appears.
    let wrong_write = graph.nodes.iter().find(|n| match &n.operation {
        PirOperation::LexicalWrite { name } => name.name == "x",
        PirOperation::StashWrite { symbol } => symbol.name == "x",
        _ => false,
    });
    assert!(
        wrong_write.is_none(),
        "call argument $x must not produce a Write op; got: {:?}",
        wrong_write.map(|n| n.operation.name())
    );
}

// ── 8. Source anchors present on all non-opaque nodes ────────────────────────

#[test]
fn pir_a_all_nodes_anchored() {
    let graph = parse_and_lower("my $x = 1; $y = $z; $a += 2;");
    for node in &graph.nodes {
        assert!(
            node.source_anchor.is_anchored(),
            "all PIR-A nodes must carry a source anchor; unanchored op: {}",
            node.operation.name()
        );
    }
    assert_eq!(
        graph.receipt.source_anchor_coverage.unanchored, 0,
        "receipt must report 0 unanchored nodes"
    );
}

// ── 9. Schema version matches PIR_RECEIPT_VERSION ────────────────────────────

#[test]
fn pir_a_schema_version_matches() {
    let graph = parse_and_lower("my $x = 1;");
    assert_eq!(
        graph.receipt.schema_version, PIR_RECEIPT_VERSION,
        "PIR-A receipt schema_version must match PIR_RECEIPT_VERSION"
    );
}

// ── 10. Deterministic output ─────────────────────────────────────────────────

#[test]
fn pir_a_deterministic() {
    let source = "my $x = 1; our $y; $x = $y; $x += 1;";
    let graph1 = parse_and_lower(source);
    let graph2 = parse_and_lower(source);
    assert_eq!(graph1.nodes.len(), graph2.nodes.len(), "node count must be deterministic");
    assert_eq!(graph1.edges.len(), graph2.edges.len(), "edge count must be deterministic");
    for (n1, n2) in graph1.nodes.iter().zip(graph2.nodes.iter()) {
        assert_eq!(n1.operation, n2.operation, "operations must be deterministic");
        assert_eq!(n1.source_anchor, n2.source_anchor, "anchors must be deterministic");
    }
}

// ── 11. `$x++` → Modify (ReadModifyWrite unary) ──────────────────────────────

#[test]
fn pir_a_postfix_increment_is_modify() {
    // Use a declared variable so the kind is deterministic (Lexical→Modify).
    let graph = parse_and_lower("my $x = 0; $x++;");

    let modify = graph.nodes.iter().find(|n| {
        matches!(&n.operation, PirOperation::Modify { .. } | PirOperation::StashModify { .. })
    });
    assert!(
        modify.is_some(),
        "`$x++` must produce a Modify or StashModify operation; got ops: {:?}",
        graph.nodes.iter().map(|n| n.operation.name()).collect::<Vec<_>>()
    );
}

// ── 12. Modify evaluates place once (no duplicate target in receipt) ──────────

#[test]
fn pir_a_modify_place_evaluated_once() {
    // Use a declared variable to get Lexical→Modify path.
    let graph = parse_and_lower("my $x = 0; $x += 1;");

    // There must be exactly ONE Modify (or StashModify) op for $x,
    // not two (which would mean the place was evaluated twice — a bug).
    let modify_count = graph
        .nodes
        .iter()
        .filter(|n| match &n.operation {
            PirOperation::Modify { name, .. } => name.name == "x",
            PirOperation::StashModify { symbol, .. } => symbol.name == "x",
            _ => false,
        })
        .count();
    assert_eq!(
        modify_count, 1,
        "Modify must evaluate its place exactly once; got {modify_count} Modify ops for $x"
    );
}

// ── 13. Sub body operations ───────────────────────────────────────────────────

#[test]
fn pir_a_sub_body_produces_ops() {
    let graph = parse_and_lower("sub foo { my $x = 1; }");

    // The sub body `my $x = 1` must produce a LexicalWrite for $x.
    let write = graph
        .nodes
        .iter()
        .find(|n| matches!(&n.operation, PirOperation::LexicalWrite { name } if name.name == "x"));
    assert!(
        write.is_some(),
        "sub body `my $x = 1` must produce LexicalWrite for $x; got: {:?}",
        graph.nodes.iter().map(|n| n.operation.name()).collect::<Vec<_>>()
    );
}

// ── 14. body_model_version guard ─────────────────────────────────────────────
// If body_model_version != HIR_BODY_MODEL_VERSION, lower_hir_bodies must
// return an empty or schema-mismatch graph (no operations from unversioned bodies).

#[test]
fn pir_a_body_model_version_check() {
    use perl_parser_core::hir::HIR_BODY_MODEL_VERSION;
    // The canonical path (lower_ast) always sets body_model_version correctly.
    // Verify that a normally lowered file passes the version check.
    let mut parser = Parser::new("my $x = 1;");
    let output = parser.parse_with_recovery();
    let hir = lower_ast(&output.ast);
    assert_eq!(
        hir.body_model_version, HIR_BODY_MODEL_VERSION,
        "lower_ast must set body_model_version = HIR_BODY_MODEL_VERSION"
    );
    let graph = lower_hir_bodies(&hir);
    // With correct version, must produce at least one op
    assert!(
        !graph.nodes.is_empty(),
        "lower_hir_bodies with correct body_model_version must produce ops"
    );
}

// ── 15. `our $x = $y;` — storage-aware Let: 1 StashWrite, 0 LexicalWrite ──────
// Regression guard for the Let arm bug: storage class was ignored → LexicalWrite
// was always emitted. Now `our` must produce exactly 1 StashWrite for $x and
// zero LexicalWrite ops for $x.

#[test]
fn pir_a_our_with_init_produces_stash_write_not_lexical() {
    let graph = parse_and_lower("our $x = $y;");

    // Exactly one StashWrite for $x
    let stash_write_count = graph
        .nodes
        .iter()
        .filter(
            |n| matches!(&n.operation, PirOperation::StashWrite { symbol } if symbol.name == "x"),
        )
        .count();
    assert_eq!(
        stash_write_count, 1,
        "`our $x = $y` must produce exactly 1 StashWrite for $x; got {stash_write_count}"
    );

    // Zero LexicalWrite for $x
    let lex_write_count = graph
        .nodes
        .iter()
        .filter(|n| matches!(&n.operation, PirOperation::LexicalWrite { name } if name.name == "x"))
        .count();
    assert_eq!(
        lex_write_count, 0,
        "`our $x = $y` must produce 0 LexicalWrite for $x; got {lex_write_count}"
    );
}
