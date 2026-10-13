//! Exact native-parser proof for the existing long-tail NodeKind corpus fixture (#13655).
//!
//! The broad corpus audit already proves reachability and parent-context diversity.
//! This focused test binds that evidence to the intended source spans and payloads
//! so a nearby node cannot preserve the aggregate count accidentally.

use perl_parser::{Node, NodeKind, Parser};
use std::fs;
use std::path::{Path, PathBuf};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn existing_long_tail_fixture_is_discovered_and_emits_exact_nodekinds() -> TestResult {
    let workspace_root = fs::canonicalize(workspace_root())?;
    let corpus_paths = perl_corpus::files::CorpusPaths::try_from_root(&workspace_root)?;
    let fixture_path =
        corpus_paths.root_authority().path().join("test_corpus/key_value_slice_and_vstring.pl");
    let source = fs::read_to_string(&fixture_path)?;

    if !perl_corpus::files::get_test_files_from(corpus_paths.as_paths()).contains(&fixture_path) {
        return Err(
            "the checkout fixture must be part of the explicitly rooted project-corpus population"
                .into(),
        );
    }

    let mut parser = Parser::new(&source);
    let output = parser.parse_with_recovery();
    if output.terminated_early() {
        return Err("the long-tail fixture must not terminate parsing early".into());
    }
    if output.stop_cause().is_some() {
        return Err("the long-tail fixture must complete without a stop cause".into());
    }
    if !output.diagnostics.is_empty() {
        return Err(format!(
            "the long-tail fixture must parse cleanly; observed {} diagnostic(s)",
            output.diagnostics.len()
        )
        .into());
    }

    for (expected_span, expected_keys_span, expected_keys) in [
        ("%config{qw(host port)}", "qw(host port)", ["host", "port"].as_slice()),
        ("%config{qw(user)}", "qw(user)", ["user"].as_slice()),
    ] {
        if !contains_key_value_slice(
            &output.ast,
            &source,
            expected_span,
            expected_keys_span,
            expected_keys,
        ) {
            return Err(format!(
                "the fixture must emit the exact KeyValueSlice for `{expected_span}`"
            )
            .into());
        }
    }
    for expected_value in ["v65.66.67", "v76.111.111"] {
        if !contains_vstring(&output.ast, &source, expected_value) {
            return Err(format!("the fixture must emit VString for `{expected_value}`").into());
        }
    }

    Ok(())
}

fn contains_key_value_slice(
    node: &Node,
    source: &str,
    expected_span: &str,
    expected_keys_span: &str,
    expected_keys: &[&str],
) -> bool {
    if let NodeKind::KeyValueSlice { target, keys } = &node.kind
        && matches!(
            &target.kind,
            NodeKind::Variable { sigil, name } if sigil == "%" && name == "config"
        )
        && target.location.start == node.location.start
        && node.location.start.checked_add("%config".len()) == Some(target.location.end)
        && node_source(node, source) == Some(expected_span)
        && node_source(keys, source) == Some(expected_keys_span)
        && key_list_matches(keys, expected_keys)
    {
        return true;
    }

    any_child(node, |child| {
        contains_key_value_slice(child, source, expected_span, expected_keys_span, expected_keys)
    })
}

fn key_list_matches(keys: &Node, expected_keys: &[&str]) -> bool {
    let NodeKind::ArrayLiteral { elements } = &keys.kind else {
        return false;
    };
    elements.len() == expected_keys.len()
        && elements.iter().zip(expected_keys).all(|(element, expected_key)| {
            matches!(
                &element.kind,
                NodeKind::String { value, interpolated: false } if value == expected_key
            )
        })
}

fn contains_vstring(node: &Node, source: &str, expected_value: &str) -> bool {
    if matches!(&node.kind, NodeKind::VString { value } if value == expected_value)
        && node_source(node, source) == Some(expected_value)
    {
        return true;
    }

    any_child(node, |child| contains_vstring(child, source, expected_value))
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
