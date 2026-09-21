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
