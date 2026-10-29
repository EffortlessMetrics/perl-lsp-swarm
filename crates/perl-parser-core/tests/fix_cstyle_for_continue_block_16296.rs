//! C-style `for` with `continue` block rejection tests (#16296).
//!
//! Real Perl rejects `for (;;) { ... } continue { ... }` outright
//! (`syntax error near "} continue"`): `continue` blocks attach to
//! `while`/`until`/`foreach` loops only, never to C-style `for`. A bare
//! `continue;` statement after the loop stays valid and keeps its route.

use perl_parser_core::{ParseError, Parser};

#[test]
fn c_style_for_continue_block_is_hard_error() -> Result<(), String> {
    let mut parser =
        Parser::new("for (my $i = 0; $i < 3; $i++) { print 1; } continue { print 2; }");
    match parser.parse() {
        Err(ParseError::CStyleForContinueBlock { .. }) => Ok(()),
        Err(other) => Err(format!("expected CStyleForContinueBlock, got {other:?}")),
        Ok(ast) => Err(format!(
            "C-style for with continue block must fail to parse, got: {}",
            ast.to_sexp()
        )),
    }
}

#[test]
fn bare_continue_after_c_style_for_keeps_its_route() -> Result<(), String> {
    // Only the block form is rejected: `for (;;) { ... } continue;` is
    // valid Perl (verified with `perl -c`) and must keep parsing.
    let mut parser = Parser::new("for (my $i = 0; $i < 3; $i++) { print 1; } continue;");
    let ast = parser
        .parse()
        .map_err(|e| format!("bare continue after C-style for must parse, got: {e:?}"))?;
    let sexp = ast.to_sexp();
    if sexp.contains("ERROR") {
        return Err(format!("expected clean parse, got ERROR nodes in: {sexp}"));
    }
    Ok(())
}
