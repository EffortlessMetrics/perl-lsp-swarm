//! Proof tests for #14798 (unify declaration-shaped call classification).
//!
//! Parser-core integration parts live here; classifier-level unit tests
//! (including the synthetic-future-form T4/T5 cases the parser cannot
//! produce) live in `hir/body.rs::declaration_verdict_tests`; the bridge
//! part extends
//! `crates/perl-semantic-analyzer/tests/callable_semantic_summary.rs`.
//! Each test is VALIDATED against real behavior at build time.
//!
//! Coverage map to issue Tests:
//! - T1 (field $x / = 1 / += 1 outside class): `legacy_forms_*` below.
//! - T2 (my/our/local/state stay declarations): `real_declarators_*`.
//! - T3 (class field stays Field): pointer to fix_13844 tests + one PIR check.
//! - T4/T5 (synthetic future declarator): classifier-level tests at the
//!   bottom; need the real `classify_declaration` signature at build time.
//!   Parser cannot produce such nodes, so these MUST be unit tests on the
//!   classifier (or synthetic AST construction if a builder exists).
//! - T6 (all passes agree): `passes_agree_*`.
//! - T7 (mutation controls): to be written against the final shape; the
//!   classifier-level tests double as the first mutation kill-set.
//!
//! IMPLEMENTATION STATUS: all tests below pass against the unified
//! classifier (`classify_declaration` in hir/body.rs) consumed by
//! first-pass binding/stash collection, body lowering, and both PIR
//! lowerings. The classifier-level contract (T4/T5/T7) lives as unit tests
//! in `hir/body.rs::declaration_verdict_tests`; the per-body receipt bridge
//! (R2) is proved at both ends by
//! `pir/lower.rs::single_body_with_unsupported_carries_nodes_plus_call_boundary`
//! and `callable_semantic_summary.rs::legacy_field_call_body_cannot_report_complete_facets`.

use std::error::Error;

use perl_parser_core::Parser;
use perl_parser_core::hir::{DeclStorageClass, StorageClass, lower_ast};
use perl_parser_core::pir::{PirOperation, lower_hir, lower_hir_bodies};

type TestResult = Result<(), Box<dyn Error>>;

fn parse(source: &str) -> perl_parser_core::hir::HirFile {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    lower_ast(&output.ast)
}

// ── T1: legacy `field` forms outside a class ─────────────────────────────

#[test]
fn legacy_forms_create_no_binding() -> TestResult {
    for source in ["field $x;\n", "field $x = 1;\n", "field $x += 1;\n"] {
        let file = parse(source);
        assert!(
            file.scope_graph.bindings.iter().all(|b| {
                b.name != "x"
                    || !matches!(b.storage, StorageClass::LexicalMy | StorageClass::LexicalState)
            }),
            "legacy `{source}` must not bind a lexical"
        );
    }
    Ok(())
}

#[test]
fn legacy_forms_publish_no_lexical_write_in_either_pir_path() -> TestResult {
    for source in ["field $x;\n", "field $x = 1;\n", "field $x += 1;\n"] {
        let file = parse(source);
        for nodes in [lower_hir(&file).nodes, lower_hir_bodies(&file).nodes] {
            assert!(
                nodes.iter().all(|n| !matches!(n.operation, PirOperation::LexicalWrite { .. })),
                "legacy `{source}` must not publish a lexical write"
            );
        }
    }
    Ok(())
}

#[test]
fn legacy_forms_carry_a_stable_unsupported_identity_in_both_receipts() -> TestResult {
    // Reviewed shape (R1): each path renders the most specific key its
    // evidence supports. Flat lowering sees the declarator, so the legacy
    // call keeps `LegacyFieldCall`; the body arena carries no declarator, so
    // it renders the generic `DeclarationShapedCall`. A future form can never
    // inherit the field label on either path (see unit tests in hir/body.rs).
    for source in ["field $x;\n", "field $x = 1;\n", "field $x += 1;\n"] {
        let file = parse(source);
        let flat = lower_hir(&file);
        let body = lower_hir_bodies(&file);
        assert!(
            flat.receipt
                .unsupported_construct_counts
                .get("LegacyFieldCall")
                .is_some_and(|c| *c >= 1),
            "flat PIR must identify the legacy call in `{source}`"
        );
        assert!(
            body.receipt
                .unsupported_construct_counts
                .get("DeclarationShapedCall")
                .is_some_and(|c| *c >= 1),
            "body PIR must identify the declaration-shaped call in `{source}`"
        );
        assert!(
            body.receipt.unsupported_construct_counts.get("LegacyFieldCall").is_none(),
            "body PIR must not reuse the field-only label in `{source}`"
        );
    }
    Ok(())
}

// ── T2: real declarators are untouched ────────────────────────────────────

#[test]
fn real_declarators_still_declare() -> TestResult {
    let file = parse("my $a = 1;\nour $b = 2;\nstate $c = 3;\nlocal $d = 4;\n");
    for (name, storage) in [
        ("a", DeclStorageClass::My),
        ("b", DeclStorageClass::Our),
        ("c", DeclStorageClass::State),
        ("d", DeclStorageClass::Local),
    ] {
        let mut found = file.bodies.iter().flat_map(|b| {
            b.stmts.iter().filter_map(|s| match s {
                perl_parser_core::hir::HirStmt::Let { name: n, storage: st, .. } if n == name => {
                    Some(st.clone())
                }
                _ => None,
            })
        });
        assert!(found.any(|st| st == storage), "`{name}` must keep {storage:?} storage");
    }
    Ok(())
}

// ── T5: qualified legacy effects in flat PIR ─────────────────────────────

#[test]
fn qualified_legacy_call_keeps_stash_effects_in_flat_pir() -> TestResult {
    // Flat PIR has no scope cursor, but a qualified name is an
    // unambiguous package slot, so these facts need no cursor.
    let file = parse("field $Foo::bar = 1;\n");
    let flat = lower_hir(&file);
    assert!(
        flat.nodes.iter().any(|n| matches!(n.operation, PirOperation::StashWrite { .. })),
        "flat PIR must keep the qualified StashWrite: {:?}",
        flat.nodes
    );
    let file = parse("field $Foo::bar;\n");
    let flat = lower_hir(&file);
    assert!(
        flat.nodes.iter().any(|n| matches!(n.operation, PirOperation::StashRead { .. })),
        "flat PIR must keep the qualified StashRead: {:?}",
        flat.nodes
    );
    Ok(())
}

// ── T5b: qualified compound targets keep the read-modify-write ──────────

#[test]
fn qualified_compound_legacy_call_keeps_modify_in_flat_pir() -> TestResult {
    // `has_initializer` alone cannot tell `=` from `+=`: flat PIR must keep
    // the read-modify-write the body path emits instead of degrading the
    // target to a plain write. The boundary receipt still fires exactly once.
    let file = parse("field $Foo::bar += 2;\n");
    let flat = lower_hir(&file);
    assert!(
        flat.nodes.iter().any(|n| matches!(
            &n.operation,
            PirOperation::StashModify { symbol, op }
                if symbol.name == "Foo::bar"
                    && symbol.package.as_deref() == Some("Foo")
                    && op == "+="
        )),
        "flat PIR must keep the qualified StashModify: {:?}",
        flat.nodes
    );
    assert_eq!(
        flat.receipt.unsupported_construct_counts.get("LegacyFieldCall"),
        Some(&1),
        "the compound call keeps its legacy-call receipt"
    );
    // A nested operator belongs to the other place: `field $x = ($y += 1)`
    // is a simple write to `$x`, never a modify of it.
    let file = parse("field $Foo::bar = ($y += 1);\n");
    let flat = lower_hir(&file);
    assert!(
        flat.nodes.iter().all(|n| !matches!(&n.operation, PirOperation::StashModify { .. })),
        "a nested compound must not become the target's modify: {:?}",
        flat.nodes
    );
    Ok(())
}

// ── T6: every pass agrees ────────────────────────────────────────────────

#[test]
fn passes_agree_on_legacy_and_clean_files() -> TestResult {
    // Legacy file: first-pass (no lexical binding), body HIR (Unknown Let),
    // flat + body PIR receipts (same unsupported count).
    let file = parse("field $x = 1;\n");
    assert!(
        file.scope_graph.bindings.iter().all(|b| b.name != "x"),
        "first pass must not bind the legacy target"
    );
    let unknown_lets = file
        .bodies
        .iter()
        .flat_map(|b| b.stmts.iter())
        .filter(|s| {
            matches!(
                s,
                perl_parser_core::hir::HirStmt::Let { storage: DeclStorageClass::Unknown, .. }
            )
        })
        .count();
    assert_eq!(unknown_lets, 1, "body HIR must hold exactly one Unknown Let");
    let flat = lower_hir(&file);
    let body = lower_hir_bodies(&file);
    let flat_count: usize = flat.receipt.unsupported_construct_counts.values().sum();
    let body_count: usize = body.receipt.unsupported_construct_counts.values().sum();
    assert_eq!(flat_count, body_count, "flat and body PIR must agree on unsupported counts");
    // Clean file: zero everywhere.
    let clean = parse("my $x = 1;\n");
    assert_eq!(lower_hir(&clean).receipt.unsupported_construct_counts.values().sum::<usize>(), 0);
    assert_eq!(
        lower_hir_bodies(&clean).receipt.unsupported_construct_counts.values().sum::<usize>(),
        0
    );
    Ok(())
}

// ── T4/T5/T7: classifier-level contract ───────────────────────────────────
//
// Implemented as unit tests in `hir/body.rs::declaration_verdict_tests`
// (the parser cannot produce synthetic future forms, so integration tests
// cannot reach them):
//   - ("my"|"our"|"local"|"state", _) => RealDeclaration (any context)
//   - ("field", true)  => RealDeclaration(Field)
//   - ("field", false) => DeclarationShapedCall { declarator: "field" }
//   - ("frobnicate", _) => DeclarationShapedCall { declarator: "frobnicate" }
//     (the future-form case: never a binding, never the field label)
// Mutation kill-set: flipping any arm must fail one of those or T1/T6 here.
