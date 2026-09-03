//! Legacy subroutine names must not become variable declarations in body PIR.

use std::error::Error;

use perl_parser_core::Parser;
use perl_parser_core::hir::{HirBodyId, StorageClass, lower_ast};
use perl_parser_core::pir::{PirOperation, lower_single_body};

type TestResult = Result<(), Box<dyn Error>>;

#[test]
fn field_call_does_not_publish_a_lexical_binding() -> TestResult {
    let source = "sub field { 1 }\nour $x;\nfield $x = 1;\nsub show { $x }\n";
    let mut parser = Parser::new(source);
    let parsed = parser.parse_with_recovery();
    let file = lower_ast(&parsed.ast);
    // The legacy call itself must not add a synthetic binding. The preceding
    // `our` declaration remains the only source of any package binding.
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
        nodes.iter().any(|node| matches!(node.operation, PirOperation::LexicalWrite { .. })),
        "legacy field calls must preserve writes to an existing lexical: {nodes:?}"
    );
    assert!(
        nodes.iter().any(|node| matches!(node.operation, PirOperation::Assign)),
        "legacy field calls must preserve assignment effects: {nodes:?}"
    );
    Ok(())
}
