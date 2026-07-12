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

    let injection = Query::new(r#"(number) @content (#set! injection.language "comment")"#)?;
    let tree = parse("my $value = 42;\n");
    let mut cursor = QueryCursor::new();
    let matches = cursor.matches(&injection, tree.root_node()).collect::<Vec<_>>();
    let query_match = matches.first().ok_or("injection setting query did not match")?;
    let setting = query_match.settings().first().ok_or("#set! setting was not emitted")?;
    if setting.key != "injection.language" || setting.value != "comment" {
        return Err(format!("unexpected injection setting: {setting:?}").into());
    }

    let unsupported = Query::new(r#"(number) @content (#lua-match? @content "^#!")"#);
    if !matches!(unsupported, Err(QueryError::UnsupportedSyntax { .. })) {
        return Err(format!("unsupported predicate was not rejected: {unsupported:?}").into());
    }
    Ok(())
}

#[test]
fn executes_the_complete_upstream_injections_query() -> TestResult {
    let upstream = include_str!("../../../tree-sitter-perl/queries/injections.scm");
    let query = Query::new(upstream)?;
    if query.pattern_count() != 5 {
        return Err(
            format!("expected five injection patterns, found {}", query.pattern_count()).into()
        );
    }

    // The native AST intentionally omits trivia-only comment and POD nodes,
    // so this fixture proves that the complete upstream file compiles and
    // executes against the facade without pretending those patterns match.
    let source = "my $value = 42;\n";
    let tree = parse(source);
    let mut cursor = QueryCursor::new();
    let _executed_matches = cursor.matches(&query, tree.root_node()).count();
    Ok(())
}
