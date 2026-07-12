#![cfg(feature = "queries")]

use std::error::Error;

use perl_tdd_support::must_some;
use tree_sitter_perl_rs::{Parser, Query, QueryCursor, QueryError};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

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
fn upstream_injection_predicates_have_explicit_phase_2b_behavior() -> TestResult {
    let upstream = include_str!("../../../tree-sitter-perl/queries/injections.scm");
    for predicate in ["#eq?", "#match?", "#not-match?", "#set!"] {
        if !upstream.contains(predicate) {
            return Err(format!("upstream injections query no longer contains {predicate}").into());
        }
    }

    let supported =
        Query::new(r#"(identifier) @name (#eq? @name "Inline") (#not-match? @name "^Perl$")"#)?;
    if supported.pattern_count() != 1 {
        return Err("supported predicate fixture did not compile as one pattern".into());
    }

    let unsupported = Query::new(r#"(comment) @content (#set! injection.language "comment")"#);
    if !matches!(
        unsupported,
        Err(QueryError::UnsupportedSyntax { .. }) | Err(QueryError::UnexpectedToken { .. })
    ) {
        return Err(
            format!("#set! was not rejected with a typed syntax error: {unsupported:?}").into()
        );
    }
    Ok(())
}
