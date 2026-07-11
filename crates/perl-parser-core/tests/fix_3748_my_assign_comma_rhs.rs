//! Regression test for issue #3748: `my $a = 1, $b;` mis-parses the
//! initializer RHS of `$a` as the whole comma list `1, $b` instead of
//! stopping at `1`.
//!
//! Ground truth (perl 5.42.2, `-MO=Deparse,-p`):
//!
//!   $ perl -MO=Deparse,-p -e 'my $a = 1, $b;'
//!   ((my($a) = 1), $b);
//!
//!   $ perl -MO=Deparse,-p -e 'my $a = (1, $b);'
//!   (my($a) = ('???', $b));
//!
//! perlop: `=` binds tighter than `,`. So `my $a = 1, $b;` binds as
//! `((my $a = 1), $b)` — the initializer of `$a` is just the scalar literal
//! `1`, and `$b` is a separate trailing comma term at the statement level.
//! Only a PARENTHESIZED RHS (`my $a = (1, $b);`) keeps the list as the
//! initializer, because the parenthesized group is a single primary term.

mod cpan_test_helpers;

use cpan_test_helpers::assert_clean_parse;
use perl_parser_core::{Node, NodeKind, Parser};

fn parse_program(source: &str) -> Result<Node, String> {
    let mut parser = Parser::new(source);
    parser.parse().map_err(|error| error.to_string())
}

fn program_statements(ast: &Node) -> Result<&[Node], String> {
    match &ast.kind {
        NodeKind::Program { statements } => Ok(statements),
        other => Err(format!("expected Program node, got {other:?}")),
    }
}

#[test]
fn unparenthesized_rhs_stops_initializer_at_comma() -> Result<(), String> {
    let source = "my $a = 1, $b;";
    assert_clean_parse(source);

    let ast = parse_program(source)?;
    let statements = program_statements(&ast)?;
    let top = statements.first().ok_or_else(|| "expected one top-level statement".to_string())?;

    // The comma-continuation wraps the declaration + trailing `$b` in an
    // ExpressionStatement { ArrayLiteral { [VariableDeclaration, $b] } }.
    let expr = match &top.kind {
        NodeKind::ExpressionStatement { expression } => expression.as_ref(),
        NodeKind::VariableDeclaration { .. } => top,
        other => {
            return Err(format!(
                "expected ExpressionStatement or VariableDeclaration, got {other:?}"
            ));
        }
    };

    let elements = match &expr.kind {
        NodeKind::ArrayLiteral { elements } => elements.as_slice(),
        NodeKind::VariableDeclaration { .. } => std::slice::from_ref(expr),
        other => {
            return Err(format!("expected ArrayLiteral or VariableDeclaration, got {other:?}"));
        }
    };

    assert_eq!(
        elements.len(),
        2,
        "expected two top-level comma terms: (my $a = 1) and $b; got {elements:?}"
    );

    // First term: the `my $a = 1` declaration, whose initializer must be
    // JUST the scalar literal `1` — not a comma list / ArrayLiteral folding
    // in `$b`.
    let decl = elements.first().ok_or_else(|| "expected first comma term".to_string())?;
    match &decl.kind {
        NodeKind::VariableDeclaration { declarator, initializer, .. } => {
            assert_eq!(declarator, "my");
            let init = initializer
                .as_ref()
                .ok_or_else(|| "expected $a to have an initializer".to_string())?;
            match &init.kind {
                NodeKind::Number { value } => {
                    assert_eq!(value, "1", "initializer must be the scalar literal 1");
                }
                other => {
                    return Err(format!(
                        "initializer must be a scalar Number literal, not a comma list; got {other:?}"
                    ));
                }
            }
        }
        other => {
            return Err(format!("expected VariableDeclaration as first comma term, got {other:?}"));
        }
    }

    // Second term: `$b` as a standalone trailing expression, NOT folded
    // into $a's initializer.
    let second = elements.get(1).ok_or_else(|| "expected second comma term".to_string())?;
    match &second.kind {
        NodeKind::Variable { sigil, name, .. } => {
            assert_eq!(sigil, "$");
            assert_eq!(name, "b");
        }
        other => return Err(format!("expected trailing $b variable, got {other:?}")),
    }

    Ok(())
}

#[test]
fn parenthesized_rhs_keeps_list_as_initializer() -> Result<(), String> {
    // Guard: `my $a = (1, $b);` — the parens make `(1, $b)` a single
    // primary term, so it stays as $a's initializer.
    let source = "my $a = (1, $b);";
    assert_clean_parse(source);

    let ast = parse_program(source)?;
    let statements = program_statements(&ast)?;
    let top = statements.first().ok_or_else(|| "expected one top-level statement".to_string())?;

    let decl = match &top.kind {
        NodeKind::VariableDeclaration { .. } => top,
        NodeKind::ExpressionStatement { expression } => expression.as_ref(),
        other => return Err(format!("expected VariableDeclaration, got {other:?}")),
    };

    match &decl.kind {
        NodeKind::VariableDeclaration { declarator, initializer, .. } => {
            assert_eq!(declarator, "my");
            let init = initializer
                .as_ref()
                .ok_or_else(|| "expected $a to have an initializer".to_string())?;
            match &init.kind {
                NodeKind::ArrayLiteral { elements } => {
                    assert_eq!(
                        elements.len(),
                        2,
                        "parenthesized (1, $b) initializer must keep both elements; got {elements:?}"
                    );
                }
                other => {
                    return Err(format!(
                        "parenthesized RHS must stay a list initializer; got {other:?}"
                    ));
                }
            }
        }
        other => return Err(format!("expected VariableDeclaration, got {other:?}")),
    }

    Ok(())
}
