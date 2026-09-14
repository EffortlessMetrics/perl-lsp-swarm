//! Input-side identity proof for callable summary assembly (#12674, I02).
//!
//! The callable-summary assembler in perl-semantic-analyzer consumes
//! `lower_ast` + `lower_single_body` output as its ONLY input. This test
//! pins the input-stability guarantee that assembly determinism rests on:
//! two independent parse + lower runs over the same source produce
//! identical HIR body owners, PIR node identities, anchors, and operation
//! sequences.
//!
//! Tests return `Result` and use `ok_or`/`?` rather than `expect`/`panic`,
//! per the crate's integration-test lint policy.

use std::error::Error;

use perl_parser_core::Parser;
use perl_parser_core::hir::{HirBodyId, HirFile, lower_ast};
use perl_parser_core::pir::{PirNode, lower_single_body};

mod cpan_test_helpers;
use cpan_test_helpers::assert_clean_parse;

type TestResult = Result<(), Box<dyn Error>>;

const SOURCE: &str = "sub f { a(); b(); my $x = 1; $x += 2; return $x; } sub g { return; }";

fn parse_and_lower(source: &str) -> HirFile {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    // A parse regression must not pass this proof on a recovered tree:
    // assert the parse is clean before lowering (the helper re-parses the
    // same source and fails on any Error/Missing node).
    assert_clean_parse(source);
    lower_ast(&output.ast)
}

/// One body's lowered operation sequence as a comparable shape: operation
/// payload, node id, and anchor range per node, in lowering order. Returns
/// `None` when the body index does not exist — a missing body is an
/// explicit test failure at the call site, never a vacuous empty-vec pass.
fn body_shape(
    file: &HirFile,
    body_idx: usize,
) -> Option<Vec<(String, u32, Option<(usize, usize)>)>> {
    let body = file.bodies.get(body_idx)?;
    Some(
        lower_single_body(body, HirBodyId(body_idx as u32), file)
            .iter()
            .map(|node: &PirNode| {
                (
                    format!("{:?}", node.operation),
                    node.id.index(),
                    node.source_anchor.range.map(|range| (range.start, range.end)),
                )
            })
            .collect(),
    )
}

/// Resolve a named subroutine's body index from the body OWNER, never a
/// hardcoded position: a lowering-order change must surface as an
/// actionable error here, not a silently swapped shape.
fn named_body_index(file: &HirFile, name: &str) -> Result<usize, Box<dyn Error>> {
    file.bodies
        .iter()
        .position(|body| {
            matches!(&body.owner,
                perl_parser_core::hir::BodyOwnerKind::Subroutine { name: Some(owner) }
                    if owner == name)
        })
        .ok_or_else(|| {
            format!(
                "no body owned by subroutine `{name}` — the lowering order or owner mapping changed"
            )
            .into()
        })
}

#[test]
fn callable_summary_input_identity_is_deterministic() -> TestResult {
    let first = parse_and_lower(SOURCE);
    let second = parse_and_lower(SOURCE);

    // Same body owners in the same order.
    let first_owners: Vec<String> =
        first.bodies.iter().map(|body| format!("{:?}", body.owner)).collect();
    let second_owners: Vec<String> =
        second.bodies.iter().map(|body| format!("{:?}", body.owner)).collect();
    assert_eq!(first_owners, second_owners, "body owner sequence must be stable");
    if first_owners.len() != 3 {
        return Err(format!("expected program root + 2 callables, got {first_owners:?}").into());
    }

    // Same item identities: full kind payloads, ranges, and scope contexts
    // in order — a HirKind payload change must be detected, not just a
    // variant-tag change.
    let item_shape = |item: &perl_parser_core::hir::HirItem| {
        format!("{:?}{:?}{:?}{:?}", item.id, item.kind, item.range, item.scope_context)
    };
    let first_items: Vec<String> = first.items.iter().map(&item_shape).collect();
    let second_items: Vec<String> = second.items.iter().map(&item_shape).collect();
    assert_eq!(first_items, second_items, "flat HIR item identities must be stable");

    // Same per-body PIR identities: ids, anchors, and op sequences.
    for body_idx in 0..first.bodies.len() {
        let first_shape =
            body_shape(&first, body_idx).ok_or("first lowering must have every body")?;
        let second_shape =
            body_shape(&second, body_idx).ok_or("second lowering must have every body")?;
        assert_eq!(
            first_shape, second_shape,
            "PIR identity sequence for body {body_idx} must be stable across lowerings"
        );
    }

    // The callable bodies actually lower to operations (the assembler's
    // work law has something to count).
    let f_idx = named_body_index(&first, "f")?;
    let f_shape = body_shape(&first, f_idx).ok_or("sub f body must exist")?;
    assert!(!f_shape.is_empty(), "sub f must lower to a non-empty op sequence");
    Ok(())
}

/// Two subs' bodies stay isolated: lowering one body never leaks nodes or
/// identities from another.
#[test]
fn callable_summary_input_identity_preserves_body_boundaries() -> TestResult {
    let file = parse_and_lower(SOURCE);
    let f_idx = named_body_index(&file, "f")?;
    let g_idx = named_body_index(&file, "g")?;
    let f_shape = body_shape(&file, f_idx).ok_or("sub f body must exist")?;
    let g_shape = body_shape(&file, g_idx).ok_or("sub g body must exist")?;

    // PIR ids restart at zero per body lowering — the id is only meaningful
    // together with the body index.
    let f_first_id = f_shape.first().map(|(_, id, _)| *id).ok_or("sub f has nodes")?;
    let g_first_id = g_shape.first().map(|(_, id, _)| *id).ok_or("sub g has nodes")?;
    assert_eq!(f_first_id, 0);
    assert_eq!(g_first_id, 0);

    // Anchors stay inside each body's own declaration span.
    let f_span = file
        .items
        .iter()
        .find(|item| {
            matches!(&item.kind, perl_parser_core::hir::HirKind::SubDecl(decl)
                if decl.name.as_deref() == Some("f"))
        })
        .map(|item| (item.range.start, item.range.end))
        .ok_or("sub f declaration item exists")?;
    for (_, _, range) in &f_shape {
        let (start, end) = range.ok_or("sub f nodes are source-anchored")?;
        assert!(
            start >= f_span.0 && end <= f_span.1,
            "sub f node anchor escaped its body span: {start}..{end} not within {f_span:?}"
        );
    }
    Ok(())
}
