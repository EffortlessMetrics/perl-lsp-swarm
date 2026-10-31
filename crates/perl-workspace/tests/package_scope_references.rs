//! Cross-file references must retain the lexical package of each bare call.

use perl_parser_core::Parser;
use perl_workspace::workspace::workspace_index::{SymKind, SymbolKey, WorkspaceIndex};
use std::sync::Arc;
use url::Url;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn assert_call_lines(index: &WorkspaceIndex, package: &str, lines: &[u32]) {
    let key = SymbolKey {
        pkg: Arc::from(package),
        name: Arc::from("mark"),
        sigil: None,
        kind: SymKind::Sub,
    };
    let actual: Vec<_> = index
        .find_refs(&key)
        .into_iter()
        .map(|location| {
            (
                location.uri,
                location.range.start.line,
                location.range.start.column,
                location.range.end.line,
                location.range.end.column,
            )
        })
        .collect();
    let expected: Vec<_> =
        lines.iter().map(|line| ("file:///caller.pl".to_owned(), *line, 0, *line, 6)).collect();
    assert_eq!(actual, expected, "references to {package}::mark");
}

fn indexed_fixture(caller: &str) -> Result<WorkspaceIndex, Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    for (uri, source) in [
        ("file:///Outer.pm", "package Outer;\nsub mark {}\n"),
        ("file:///Inner.pm", "package Inner;\nsub mark {}\n"),
        ("file:///Third.pm", "package Third;\nsub mark {}\n"),
        ("file:///caller.pl", caller),
    ] {
        // Parser success and synchronous index completion are separate controls.
        let mut parser = Parser::new(source);
        parser.parse()?;
        assert!(
            parser.errors().is_empty(),
            "fixture parser errors in {uri}: {:?}",
            parser.errors()
        );
        index.index_initial_file(Url::parse(uri)?, source.to_owned())?;
    }
    assert_eq!(index.file_count(), 4, "all fixture files must be indexed");
    assert_eq!(index.stale_file_count(), 0, "the fixture index must be current");
    Ok(index)
}

#[test]
fn block_package_restores_enclosing_reference_owner() -> TestResult {
    let caller = "package Outer;\nmark();\npackage Inner {\nmark();\npackage Third {\nmark();\n}\nmark();\n}\nmark();\npackage Third;\nmark();\n";
    let index = indexed_fixture(caller)?;
    assert_call_lines(&index, "Outer", &[1, 9]);
    assert_call_lines(&index, "Inner", &[3, 7]);
    assert_call_lines(&index, "Third", &[5, 11]);
    Ok(())
}

#[test]
fn statement_package_in_block_restores_enclosing_reference_owner() -> TestResult {
    let caller = "package Outer;\nmark();\n{\npackage Inner;\nmark();\n}\nmark();\npackage Third;\nmark();\n";
    let index = indexed_fixture(caller)?;
    assert_call_lines(&index, "Outer", &[1, 6]);
    assert_call_lines(&index, "Inner", &[4]);
    assert_call_lines(&index, "Third", &[8]);
    Ok(())
}
