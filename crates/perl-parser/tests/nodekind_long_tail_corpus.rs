//! Exact native-parser proof for the remaining ordinary corpus NodeKind gaps (#13655).
//!
//! The broad corpus audit is reachability evidence, not parser-accuracy gold. This
//! focused test prevents the new fixture from satisfying that reachability count
//! through a different nearby node shape.

use perl_parser::{Node, NodeKind, Parser};
use std::fs;
use std::path::{Path, PathBuf};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn long_tail_fixture_is_discovered_and_emits_exact_nodekinds() -> TestResult {
    let fixture_path = workspace_root().join("test_corpus/nodekind_long_tail.pl");
    let source = fs::read_to_string(&fixture_path)?;

    assert!(
        perl_corpus::get_test_files().iter().any(|path| path.ends_with("nodekind_long_tail.pl")),
        "the long-tail fixture must be part of the governed project-corpus population"
    );

    let mut parser = Parser::new(&source);
    let output = parser.parse_with_recovery();
    assert!(
        output.diagnostics.is_empty(),
        "the long-tail fixture must parse cleanly; observed {} diagnostic(s)",
        output.diagnostics.len()
    );

    assert!(
        contains_key_value_slice(&output.ast, &source),
        "the fixture must emit KeyValueSlice for `%pairs{{qw(alpha beta)}}`"
    );
    assert!(
        contains_vstring(&output.ast, &source),
        "the fixture must emit VString for `v1.2.3`"
    );

    Ok(())
}

fn contains_key_value_slice(node: &Node, source: &str) -> bool {
    if let NodeKind::KeyValueSlice { target, .. } = &node.kind
        && matches!(
            &target.kind,
            NodeKind::Variable { sigil, name } if sigil == "%" && name == "pairs"
        )
        && node_source(node, source) == Some("%pairs{qw(alpha beta)}")
    {
        return true;
    }

    any_child(node, |child| contains_key_value_slice(child, source))
}

fn contains_vstring(node: &Node, source: &str) -> bool {
    if matches!(&node.kind, NodeKind::VString { value } if value == "v1.2.3")
        && node_source(node, source) == Some("v1.2.3")
    {
        return true;
    }

    any_child(node, |child| contains_vstring(child, source))
}

fn any_child(node: &Node, mut predicate: impl FnMut(&Node) -> bool) -> bool {
    let mut found = false;
    node.for_each_child(|child| {
        if !found {
            found = predicate(child);
        }
    });
    found
}

fn node_source<'a>(node: &Node, source: &'a str) -> Option<&'a str> {
    source.get(node.location.start..node.location.end)
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}
