//! Legacy subroutine names must not become variable declarations in body PIR.

use std::error::Error;

use perl_parser_core::Parser;
use perl_parser_core::hir::{HirBodyId, StorageClass, lower_ast};
use perl_parser_core::pir::{PirOperation, lower_hir, lower_single_body};

type TestResult = Result<(), Box<dyn Error>>;

#[test]
fn field_call_does_not_publish_a_lexical_binding() -> TestResult {
    let source = "sub field { 1 }\nour $x;\nfield $x = 1;\nsub show { $x }\n";
    let mut parser = Parser::new(source);
    let parsed = parser.parse_with_recovery();
    let file = lower_ast(&parsed.ast);
    // The legacy call itself must not add a synthetic binding. The preceding
    // `our` declaration remains the only source of any package binding.
    //
    // The negative check alone is satisfied by an empty binding set, so it
    // cannot tell "no synthetic lexical" from "the `our` binding was dropped
    // too". Assert the real declaration survives first, then that nothing
    // lexical joined it.
    assert!(
        file.scope_graph.bindings.iter().any(|binding| {
            binding.name == "x" && matches!(binding.storage, StorageClass::PackageOur)
        }),
        "the `our $x` declaration must still publish its package binding: {:?}",
        file.scope_graph.bindings
    );
    assert!(
        file.scope_graph.bindings.iter().all(|binding| {
            binding.name != "x"
                || !matches!(binding.storage, StorageClass::LexicalMy | StorageClass::LexicalState)
        }),
        "the legacy field target must not gain a lexical scope binding"
    );
    let body = file.bodies.first().ok_or("program body is required")?;
    let nodes = lower_single_body(body, HirBodyId(0), &file);

    assert!(
        nodes.iter().all(|node| !matches!(node.operation, PirOperation::LexicalWrite { .. })),
        "legacy field call must not publish a lexical write: {nodes:?}"
    );
    Ok(())
}

#[test]
fn real_my_declaration_still_publishes_a_lexical_binding() -> TestResult {
    let source = "my $x = 1;\n";
    let mut parser = Parser::new(source);
    let parsed = parser.parse_with_recovery();
    let file = lower_ast(&parsed.ast);
    let body = file.bodies.first().ok_or("program body is required")?;
    let nodes = lower_single_body(body, HirBodyId(0), &file);

    assert!(
        nodes.iter().any(|node| matches!(node.operation, PirOperation::LexicalWrite { .. })),
        "a real my declaration must publish a lexical write: {nodes:?}"
    );
    Ok(())
}

#[test]
fn initialized_field_argument_preserves_package_write_and_assignment() -> TestResult {
    let source = "field $x = 1;\n";
    let mut parser = Parser::new(source);
    let parsed = parser.parse_with_recovery();
    let file = lower_ast(&parsed.ast);
    let body = file.bodies.first().ok_or("program body is required")?;
    let nodes = lower_single_body(body, HirBodyId(0), &file);

    assert!(
        nodes.iter().any(|node| matches!(node.operation, PirOperation::StashWrite { .. })),
        "initialized legacy field argument must retain its package write: {nodes:?}"
    );
    assert!(
        nodes.iter().any(|node| matches!(node.operation, PirOperation::Assign)),
        "initialized legacy field argument must retain its assignment: {nodes:?}"
    );
    assert!(
        nodes.iter().all(|node| !matches!(node.operation, PirOperation::LexicalWrite { .. })),
        "legacy field argument must not publish a lexical write: {nodes:?}"
    );
    Ok(())
}

#[test]
fn bare_field_argument_preserves_package_read() -> TestResult {
    let source = "field $x;\n";
    let mut parser = Parser::new(source);
    let parsed = parser.parse_with_recovery();
    let file = lower_ast(&parsed.ast);
    let body = file.bodies.first().ok_or("program body is required")?;
    let nodes = lower_single_body(body, HirBodyId(0), &file);

    assert!(
        nodes.iter().any(|node| matches!(node.operation, PirOperation::StashRead { .. })),
        "bare legacy field argument must retain its package read: {nodes:?}"
    );
    assert!(
        nodes.iter().all(|node| !matches!(node.operation, PirOperation::LexicalWrite { .. })),
        "bare legacy field argument must not publish a lexical write: {nodes:?}"
    );
    Ok(())
}

#[test]
fn legacy_field_call_reuses_existing_lexical_target() -> TestResult {
    let source = "my $x; field $x = 1;\n";
    let mut parser = Parser::new(source);
    let parsed = parser.parse_with_recovery();
    let file = lower_ast(&parsed.ast);
    let body = file.bodies.first().ok_or("program body is required")?;
    let nodes = lower_single_body(body, HirBodyId(0), &file);

    assert!(
        nodes.iter().any(|node| {
            matches!(node.operation, PirOperation::LexicalWrite { .. })
                && node.source_anchor.range.map(|range| (range.start, range.end)) == Some((13, 15))
        }),
        "legacy field call must write the existing lexical at the field argument anchor: {nodes:?}"
    );
    assert!(
        nodes.iter().any(|node| matches!(node.operation, PirOperation::Assign)),
        "legacy field calls must preserve assignment effects: {nodes:?}"
    );
    Ok(())
}

#[test]
fn bare_field_argument_reuses_existing_lexical_target() -> TestResult {
    // `field $x;` after `my $x` reads that lexical before invoking the callee.
    // Lowering it as an unconditional package read hid the call-site reference
    // from `extract_lexical_facts`, which only collects lexical operations.
    let source = "my $x; field $x;\n";
    let mut parser = Parser::new(source);
    let parsed = parser.parse_with_recovery();
    let file = lower_ast(&parsed.ast);
    let body = file.bodies.first().ok_or("program body is required")?;
    let nodes = lower_single_body(body, HirBodyId(0), &file);

    assert!(
        nodes.iter().any(|node| {
            matches!(node.operation, PirOperation::LexicalRead { .. })
                && node.source_anchor.range.map(|range| (range.start, range.end)) == Some((13, 15))
        }),
        "bare legacy field call must read the existing lexical at the field \
         argument anchor: {nodes:?}"
    );
    assert!(
        nodes.iter().all(|node| !matches!(node.operation, PirOperation::StashRead { .. })),
        "a visible lexical target must not be read as a package symbol: {nodes:?}"
    );
    Ok(())
}

#[test]
fn compound_field_argument_modifies_once_without_an_extra_write() -> TestResult {
    // `field $x += 1` lowers to a complete read-modify-write over `$x`.
    // Wrapping it in the declaration's simple-assignment place published
    // `x = (x += 1)`: one spurious write on top of the modify.
    let source = "my $x; field $x += 1;\n";
    let mut parser = Parser::new(source);
    let parsed = parser.parse_with_recovery();
    let file = lower_ast(&parsed.ast);
    let body = file.bodies.first().ok_or("program body is required")?;
    let nodes = lower_single_body(body, HirBodyId(0), &file);

    let writes: Vec<_> = nodes
        .iter()
        .filter(|node| matches!(node.operation, PirOperation::LexicalWrite { .. }))
        .filter_map(|node| node.source_anchor.range.map(|range| (range.start, range.end)))
        .collect();
    assert_eq!(
        writes,
        vec![(3, 5)],
        "only the `my $x` declaration writes; the legacy call must not add a \
         second write at its own anchor: {nodes:?}"
    );
    assert!(
        nodes.iter().any(|node| matches!(node.operation, PirOperation::Modify { .. })),
        "the compound assignment itself must survive: {nodes:?}"
    );
    Ok(())
}

#[test]
fn flat_lowering_does_not_invent_a_lexical_binding_for_a_legacy_call() -> TestResult {
    // Body PIR stopped publishing the synthetic binding, but the public
    // `lower_hir` path reached the same `VariableDecl` item and still emitted
    // a declaration write, so the retained flat API disagreed with PIR-A.
    for source in ["field $x = 1;\n", "field $x;\n"] {
        let mut parser = Parser::new(source);
        let parsed = parser.parse_with_recovery();
        let file = lower_ast(&parsed.ast);
        let graph = lower_hir(&file);

        assert!(
            graph
                .nodes
                .iter()
                .all(|node| !matches!(node.operation, PirOperation::LexicalWrite { .. })),
            "flat lowering of {source:?} must not publish a lexical write: {:?}",
            graph.nodes
        );
    }
    Ok(())
}

#[test]
fn nested_same_target_assignment_keeps_both_writes() -> TestResult {
    // `field $x = ($x = 1)` writes `$x` twice: the inner assignment, then the
    // outer one storing its result. Skipping the wrapper on target name alone
    // collapsed those into one — the parser-folded `field $x += 1` is told
    // apart by provenance, not by the name it happens to mention.
    let source = "my $x; field $x = ($x = 1);\n";
    let mut parser = Parser::new(source);
    let parsed = parser.parse_with_recovery();
    let file = lower_ast(&parsed.ast);
    let body = file.bodies.first().ok_or("program body is required")?;
    let nodes = lower_single_body(body, HirBodyId(0), &file);

    let writes: Vec<_> = nodes
        .iter()
        .filter(|node| matches!(node.operation, PirOperation::LexicalWrite { .. }))
        .filter_map(|node| node.source_anchor.range.map(|range| (range.start, range.end)))
        .collect();
    assert!(
        writes.contains(&(13, 15)),
        "the legacy call's own write must survive a nested same-target \
         assignment: {nodes:?}"
    );
    assert!(
        writes.contains(&(19, 21)),
        "the nested assignment's write must survive too: {nodes:?}"
    );
    Ok(())
}

#[test]
fn nested_same_target_compound_keeps_the_outer_write() -> TestResult {
    // `field $x = ($x += 1)` is a modify *and* an outer write, unlike the
    // parser-folded `field $x += 1`, which is only the modify.
    let source = "my $x; field $x = ($x += 1);\n";
    let mut parser = Parser::new(source);
    let parsed = parser.parse_with_recovery();
    let file = lower_ast(&parsed.ast);
    let body = file.bodies.first().ok_or("program body is required")?;
    let nodes = lower_single_body(body, HirBodyId(0), &file);

    assert!(
        nodes.iter().any(|node| {
            matches!(node.operation, PirOperation::LexicalWrite { .. })
                && node.source_anchor.range.map(|range| (range.start, range.end)) == Some((13, 15))
        }),
        "the outer write at the field argument must survive: {nodes:?}"
    );
    assert!(
        nodes.iter().any(|node| matches!(node.operation, PirOperation::Modify { .. })),
        "the nested compound modify must survive: {nodes:?}"
    );
    Ok(())
}
