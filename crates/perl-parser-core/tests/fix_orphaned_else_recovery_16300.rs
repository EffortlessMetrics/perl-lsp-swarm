//! Orphaned `else` / `elsif` recovery tests (#16300).
//!
//! An `else` or `elsif` without a preceding `if`/`unless` is a syntax error
//! in real Perl, but the parser recovers: it records the diagnostic,
//! consumes the block, and wraps it in an `If` with a synthetic condition so
//! the LSP still sees the block contents.

use perl_parser_core::{NodeKind, Parser};

fn parse_recovered(src: &str) -> (perl_parser_core::Node, Vec<String>) {
    let mut parser = Parser::new(src);
    let ast =
        parser.parse().unwrap_or_else(|e| panic!("`{src}` must recover, got hard error: {e:?}"));
    let errors = parser.errors().iter().map(|e| format!("{e}")).collect();
    (ast, errors)
}

fn assert_recovered_if(src: &str, marker: &str) {
    let (ast, errors) = parse_recovered(src);
    let NodeKind::Program { statements } = &ast.kind else {
        panic!("expected Program, got {:?}", ast.kind);
    };
    assert_eq!(statements.len(), 1, "Should have 1 recovered statement for `{src}`");
    assert!(
        matches!(statements[0].kind, NodeKind::If { .. }),
        "Expected recovered If node for `{src}`, got: {:?}",
        statements[0].kind
    );
    assert!(
        errors.iter().any(|e| e.contains(marker)),
        "Should record the orphaned-branch diagnostic for `{src}`, got: {errors:?}"
    );
}

#[test]
fn test_recovery_orphaned_else_wraps_in_if() {
    assert_recovered_if("else { print \"hi\"; }", "without preceding");
}

#[test]
fn test_recovery_orphaned_elsif_chain_survives() {
    assert_recovered_if("elsif ($x) { print 1; } else { print 2; }", "without preceding");
}
