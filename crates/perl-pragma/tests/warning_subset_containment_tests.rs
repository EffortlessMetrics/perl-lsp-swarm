//! Warning-subset containment (#8450): positive subsets must not imply global/all warnings.

#![deny(clippy::map_err_ignore)]

use perl_ast::SourceLocation;
use perl_ast::ast::{Node, NodeKind};
use perl_pragma::{PragmaSnapshot, PragmaTracker};

fn loc(start: usize, end: usize) -> SourceLocation {
    SourceLocation { start, end }
}

fn use_node(module: &str, args: &[&str], start: usize, end: usize) -> Node {
    Node::new(
        NodeKind::Use {
            module: module.to_string(),
            args: args.iter().map(|s| (*s).to_string()).collect(),
            has_filter_risk: false,
        },
        loc(start, end),
    )
}

fn no_node(module: &str, args: &[&str], start: usize, end: usize) -> Node {
    Node::new(
        NodeKind::No {
            module: module.to_string(),
            args: args.iter().map(|s| (*s).to_string()).collect(),
            has_filter_risk: false,
        },
        loc(start, end),
    )
}

fn program(stmts: Vec<Node>) -> Node {
    Node::new(NodeKind::Program { statements: stmts }, loc(0, 500))
}

#[test]
fn positive_warning_subset_does_not_enable_global_or_unrelated_categories() {
    let ast = program(vec![use_node("warnings", &["'uninitialized'"], 0, 32)]);
    let map = PragmaTracker::build(&ast);
    let state = PragmaTracker::state_for_offset(&map, 20);
    let snapshot = PragmaSnapshot::from_state(state.clone());

    assert!(
        !snapshot.warnings_enabled(),
        "positive subset must not set global/all warnings (#8450)"
    );
    assert!(state.is_warning_active("uninitialized"));
    assert!(
        !state.is_warning_active("deprecated"),
        "unrelated categories must not appear active from a subset"
    );
}

#[test]
fn no_warnings_all_disables_global_without_recording_literal_all_category() {
    let ast =
        program(vec![use_node("warnings", &[], 0, 15), no_node("warnings", &["'all'"], 16, 35)]);
    let map = PragmaTracker::build(&ast);
    let state = PragmaTracker::state_for_offset(&map, 30);

    assert!(!state.warnings);
    assert!(
        !state.disabled_warning_categories.iter().any(|c| c == "all"),
        "no warnings 'all' must not record a literal 'all' disable entry"
    );
    assert!(!state.is_warning_active("deprecated"));
    assert!(!state.is_warning_active("uninitialized"));
}

#[test]
fn bare_use_warnings_after_subset_reenables_global_all() {
    let ast = program(vec![
        use_node("warnings", &["'uninitialized'"], 0, 32),
        use_node("warnings", &[], 33, 48),
    ]);
    let map = PragmaTracker::build(&ast);
    let state = PragmaTracker::state_for_offset(&map, 40);

    assert!(state.warnings);
    assert!(state.enabled_warning_categories.is_empty());
    assert!(state.is_warning_active("deprecated"));
}

#[test]
fn unsupported_fatal_shape_does_not_promote_subset_to_global() {
    let ast = program(vec![use_node("warnings", &["FATAL", "=>", "'deprecated'"], 0, 40)]);
    let map = PragmaTracker::build(&ast);
    let state = PragmaTracker::state_for_offset(&map, 20);

    assert!(!state.warnings);
    assert!(!state.is_warning_active("deprecated"));
}
