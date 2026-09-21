//! Qualified loop-control label rejection tests (#16296).
//!
//! Real Perl rejects package-qualified loop labels outright: labels are
//! plain identifiers. The parse must fail rather than attach `FOO::BAR`
//! as the label.

use perl_parser_core::{ParseError, Parser};

#[test]
fn test_qualified_loop_control_label_is_hard_error() -> Result<(), String> {
    let mut parser = Parser::new("while (1) { last FOO::BAR; }");
    match parser.parse() {
        Err(ParseError::QualifiedLoopControlLabel { .. }) => Ok(()),
        Err(other) => Err(format!("expected QualifiedLoopControlLabel, got {other:?}")),
        Ok(ast) => Err(format!("qualified loop label must fail to parse, got: {}", ast.to_sexp())),
    }
}

#[test]
fn test_qualified_label_survives_brace_fallback() -> Result<(), String> {
    // In `my $x = { ... }` the braces are hash-or-block ambiguous: the
    // parser first tries the contents as an expression, then falls back to a
    // block. The qualified-label error must propagate as the dedicated
    // terminal error rather than being swallowed into a recovered block.
    let mut parser = Parser::new("my $x = { last FOO::BAR; };");
    match parser.parse() {
        Err(ParseError::QualifiedLoopControlLabel { .. }) => Ok(()),
        Err(other) => Err(format!("expected QualifiedLoopControlLabel, got {other:?}")),
        Ok(ast) => Err(format!(
            "qualified loop label in ambiguous braces must fail to parse, errors={:?} sexp={}",
            parser.errors(),
            ast.to_sexp()
        )),
    }
}
