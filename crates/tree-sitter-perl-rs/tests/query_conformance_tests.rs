#![cfg(feature = "queries")]

use std::error::Error;

use perl_tdd_support::must_some;
use tree_sitter_perl_rs::{Parser, Query, QueryCursor, QueryError};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

fn parse(source: &str) -> tree_sitter_perl_rs::Tree {
    let mut parser = Parser::new();
    must_some(parser.parse(source))
}

#[test]
fn executes_a_supported_fragment_from_the_upstream_highlights_query() -> TestResult {
    let upstream = include_str!("../../../tree-sitter-perl/queries/highlights.scm");
    let fragment = "(number)";
    if !upstream.contains(fragment) {
        return Err("upstream highlights query no longer contains the conformance fragment".into());
    }

    let source = "my $value = 42;\n";
    let mut parser = Parser::new();
    let tree = must_some(parser.parse(source));
    let query_source = format!("{fragment} @number");
    let query = Query::new(&query_source)?;
    let mut cursor = QueryCursor::new();
    let matches = cursor.matches(&query, tree.root_node()).collect::<Vec<_>>();

    let query_match = must_some(matches.first());
    let capture = must_some(query_match.captures().first());
    assert_eq!(capture.name(), "number");
    assert_eq!(capture.node().kind(), "number");
    assert_eq!(capture.node().utf8_text(source.as_bytes())?, "42");
    Ok(())
}

#[test]
fn executes_the_complete_upstream_injections_query() -> TestResult {
    let upstream = include_str!("../../../tree-sitter-perl/queries/injections.scm");
    let query = Query::new(upstream)?;
    assert_eq!(query.pattern_count(), 5);

    let tree = parse("my $value = 42;\n");
    let mut cursor = QueryCursor::new();
    let _matches = cursor.matches(&query, tree.root_node()).count();
    Ok(())
}

#[test]
fn injection_fixture_predicate_forms_have_typed_support_boundaries() -> TestResult {
    let upstream = include_str!("../../../tree-sitter-perl/queries/injections.scm");
    for predicate in ["#eq?", "#match?", "#not-match?", "#set!"] {
        if !upstream.contains(predicate) {
            return Err(format!("injections.scm no longer contains {predicate}").into());
        }
    }

    let unsupported = Query::new("(number) @content (#lua-match? @content \"^#!\")");
    assert!(matches!(unsupported, Err(QueryError::UnsupportedSyntax { .. })));
    Ok(())
}
