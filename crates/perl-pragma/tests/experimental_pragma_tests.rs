//! Tests for `require VERSION` pragma handling (#5106).
#![expect(clippy::panic, reason = "test code")]

use perl_ast::SourceLocation;
use perl_ast::ast::{Node, NodeKind};
use perl_pragma::{PragmaState, PragmaTracker};

fn loc(start: usize, end: usize) -> SourceLocation {
    SourceLocation { start, end }
}

fn require_node(version: &str) -> Node {
    Node::new(
        NodeKind::FunctionCall {
            name: "require".to_string(),
            args: vec![Node::new(NodeKind::Number { value: version.to_string() }, loc(10, 16))],
        },
        loc(0, 17),
    )
}

fn program(stmts: Vec<Node>) -> Node {
    Node::new(NodeKind::Program { statements: stmts }, loc(0, 100))
}

fn last_feature_state(ast: &Node) -> PragmaState {
    let ranges = PragmaTracker::build(ast);
    ranges.last().map(|(_, s)| s.clone()).unwrap_or_default()
}

#[test]
fn require_version_enables_strict_and_features() {
    let ast = program(vec![require_node("5.036")]);
    let state = last_feature_state(&ast);
    assert!(state.strict_vars, "require 5.036 should enable strict vars");
    assert!(state.has_feature("signatures"), "require 5.036 should enable signatures feature");
}

#[test]
fn require_version_enables_warnings() {
    let ast = program(vec![require_node("5.038")]);
    let state = last_feature_state(&ast);
    assert!(state.strict_vars, "require 5.038 should enable strict vars");
    assert!(state.has_feature("say"), "require 5.038 should enable say feature");
}
