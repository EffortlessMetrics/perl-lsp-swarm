//! Compatibility fixtures for the public facade after the module split.
//!
//! The snapshot files in this package were captured before the implementation was
//! moved out of `lib.rs`.  Keeping this check in an integration-test crate makes
//! the fixture exercise the same public API a downstream crate receives, while
//! the byte assertions protect the source projection independently of the debug
//! S-expression projection.

use tree_sitter_perl_rs::Parser;

struct ParityCase {
    source: &'static str,
    expected_snapshot: &'static str,
    expected_child_spans: &'static [(usize, usize)],
}

const CASES: &[ParityCase] = &[
    ParityCase {
        source: "my $x = 42;",
        expected_snapshot: include_str!("snapshots/snapshots__variable_declaration.snap"),
        expected_child_spans: &[(0, 10)],
    },
    ParityCase {
        source: "sub foo { return $_[0] + 1; }",
        expected_snapshot: include_str!("snapshots/snapshots__subroutine.snap"),
        expected_child_spans: &[(0, 29)],
    },
];

fn parse(source: &str) -> Result<tree_sitter_perl_rs::Tree, Box<dyn std::error::Error>> {
    let mut parser = Parser::new();
    parser.parse(source).ok_or_else(|| "facade parser returned no tree".into())
}

#[test]
fn public_facade_preserves_snapshot_and_source_byte_parity()
-> Result<(), Box<dyn std::error::Error>> {
    for case in CASES {
        let tree = parse(case.source)?;
        let root = tree.root_node();

        assert_eq!(tree.source(), case.source);
        assert_eq!(root.start_byte(), 0);
        assert_eq!(root.end_byte(), case.source.len());
        assert_eq!(root.to_sexp(), snapshot_body(case.expected_snapshot));

        let children: Vec<_> = root.children().collect();
        assert_eq!(children.len(), case.expected_child_spans.len());
        for (child, &(start, end)) in children.iter().zip(case.expected_child_spans) {
            assert_eq!((child.start_byte(), child.end_byte()), (start, end));
            assert_eq!(child.utf8_text(case.source.as_bytes()), Ok(&case.source[start..end]));
        }
    }

    Ok(())
}

#[test]
fn downstream_can_use_only_the_facade_reexports() -> Result<(), Box<dyn std::error::Error>> {
    use tree_sitter_perl_rs::{LANGUAGE, PerlLanguage, PerlNodeKind, language};

    let mut parser = Parser::new();
    let tree = parser.parse("my $x = 42;").ok_or("facade parser returned no tree")?;
    let root = tree.root_node();
    let first = root.child(0).ok_or("facade root has no first child")?;

    if !matches!(root.inner().kind, PerlNodeKind::Program { .. }) {
        return Err("facade root did not expose the native Program kind".into());
    }
    if language() != LANGUAGE || PerlLanguage::default() != LANGUAGE {
        return Err("facade language re-exports diverged".into());
    }
    if first.kind() != "my_declaration" || first.start_byte() != 0 {
        return Err("facade child compatibility contract diverged".into());
    }

    Ok(())
}

fn snapshot_body(snapshot: &str) -> &str {
    snapshot
        .split_once("---\n")
        .and_then(|(_, remainder)| remainder.split_once("---\n"))
        .map_or(snapshot, |(_, body)| body.trim_end())
}
