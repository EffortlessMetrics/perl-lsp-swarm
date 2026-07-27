//! Tests for `use experimental` pragma handling (#5091).
#![expect(clippy::panic, reason = "test code")]

use perl_ast::SourceLocation;
use perl_ast::ast::{Node, NodeKind};
use perl_pragma::{PragmaState, PragmaTracker};

fn loc(start: usize, end: usize) -> SourceLocation {
    SourceLocation { start, end }
}

fn use_node(module: &str, args: &[&str]) -> Node {
    Node {
        kind: NodeKind::Use {
            module: module.to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
            has_filter_risk: false,
        },
        location: loc(0, 30),
    }
}

fn no_node(module: &str, args: &[&str]) -> Node {
    Node {
        kind: NodeKind::No {
            module: module.to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
            has_filter_risk: false,
        },
        location: loc(0, 30),
    }
}

fn program(stmts: Vec<Node>) -> Node {
    Node {
        kind: NodeKind::Program { statements: stmts },
        location: loc(0, 100),
    }
}

fn last_feature_state(ast: &Node) -> PragmaState {
    let ranges = PragmaTracker::build(ast);
    ranges.last().map(|(_, s)| s.clone()).unwrap_or_default()
}

#[test]
fn use_experimental_class_enables_class_feature() {
    let ast = program(vec![use_node("experimental", &["'class'"])]);
    let state = last_feature_state(&ast);
    assert!(state.has_feature("class"),
        "use experimental 'class' should enable the 'class' feature");
}

#[test]
fn use_experimental_signatures_enables_signatures() {
    let ast = program(vec![use_node("experimental", &["'signatures'"])]);
    let state = last_feature_state(&ast);
    assert!(state.has_feature("signatures"),
        "use experimental 'signatures' should enable the 'signatures' feature");
}

#[test]
fn no_experimental_disables_feature() {
    let ast = program(vec![
        use_node("experimental", &["'class'"]),
        no_node("experimental", &["'class'"]),
    ]);
    let state = last_feature_state(&ast);
    assert!(!state.has_feature("class"),
        "no experimental 'class' should disable the feature");
}
