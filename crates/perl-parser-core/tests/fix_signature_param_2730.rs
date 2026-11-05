//! Regression tests for issue #2730 — CPAN `signature_param` bucket (Test::LectroTest::Property.pm).
//!
//! Root cause: the lexer merges adjacent prototype `&` characters into `TokenKind::And`,
//! so `sub Property(&&@)` was misrouted into signature parsing and emitted
//! "Expected comma or closing parenthesis in signature".
//!
//! Fix: treat `LogicalAnd` as an unambiguous prototype opener and decode it back to `&&`
//! in `parse_prototype`.

mod cpan_test_helpers;
use cpan_test_helpers::*;
use perl_parser_core::{NodeKind, Parser};

#[test]
fn lectrotest_property_prototype_parses_clean() {
    assert_clean_parse("sub Property(&&@) {\n    my ($genspec_fn, $test_fn, @args) = @_;\n}\n");
}

#[test]
fn lectrotest_property_prototype_ast_shape() {
    let source = "sub Property(&&@) { 1 }";
    let ast = parse(source);
    let NodeKind::Program { statements } = &ast.kind else {
        panic!("expected program");
    };
    let first = statements.first().expect("sub statement");
    let NodeKind::Subroutine { prototype, signature, .. } = &first.kind else {
        panic!("expected subroutine");
    };
    assert!(signature.is_none(), "expected prototype, not signature");
    let proto = prototype.as_ref().expect("expected prototype node");
    let NodeKind::Prototype { content } = &proto.kind else {
        panic!("expected prototype node");
    };
    assert_eq!(content.as_str(), "&&@");
}

#[test]
fn lectrotest_property_pm_fixture_parses_without_signature_bucket_errors() {
    let fixture = include_str!("fixtures/lectrotest_property.pm");
    let mut parser = Parser::new(fixture);
    let _ = parser.parse().expect("parse Property.pm fixture");
    let signature_errors: Vec<_> = parser
        .errors()
        .iter()
        .filter(|e| e.to_string().contains("signature"))
        .map(|e| e.to_string())
        .collect();
    assert!(signature_errors.is_empty(), "unexpected signature diagnostics: {signature_errors:?}");
}
