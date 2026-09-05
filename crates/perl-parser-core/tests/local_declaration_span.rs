//! `local $x = EXPR` declaration nodes must span the whole assignment.
//!
//! The `local` target is parsed through the assignment parser, which pulls the
//! operator and RHS straight from the token stream without advancing the
//! parser's `previous_position()`. Before this proof, every `local` declaration
//! node stopped at the localized variable, so the HIR statement range (and
//! anything derived from it) silently dropped the assignment.

mod cpan_test_helpers;

use cpan_test_helpers::{assert_clean_parse, parse};
use perl_parser_core::{Node, NodeKind};

fn find_declaration<'a>(node: &'a Node, declarator: &str) -> Option<&'a Node> {
    if matches!(&node.kind, NodeKind::VariableDeclaration { declarator: d, .. } if d == declarator)
    {
        return Some(node);
    }
    node.children().into_iter().find_map(|child| find_declaration(child, declarator))
}

fn assert_declaration_spans(source: &str, declarator: &str, expected: &str) -> Result<(), String> {
    assert_clean_parse(source);
    let ast = parse(source);
    let declaration = find_declaration(&ast, declarator)
        .ok_or_else(|| format!("expected {declarator} declaration in {source:?}"))?;
    let actual = source
        .get(declaration.location.start..declaration.location.end)
        .ok_or_else(|| format!("invalid range {:?} for {source:?}", declaration.location))?;
    if actual != expected {
        return Err(format!(
            "{declarator} declaration in {source:?} spans {actual:?}, expected {expected:?}"
        ));
    }
    Ok(())
}

#[test]
fn local_statement_spans_embedded_assignment() -> Result<(), String> {
    for (source, expected) in [
        ("local $main::z = 'a';", "local $main::z = 'a'"),
        ("local $main::z .= 'q';", "local $main::z .= 'q'"),
        ("local $ENV{PATH} = '/bin';", "local $ENV{PATH} = '/bin'"),
        // A statement modifier is parsed by the outer statement layer; the
        // declaration must stop at the RHS, not swallow the modifier.
        ("local $main::z = 1 if $c;", "local $main::z = 1"),
    ] {
        assert_declaration_spans(source, "local", expected)?;
    }
    Ok(())
}

#[test]
fn local_expression_spans_embedded_assignment() -> Result<(), String> {
    // Expression-position `local` (after `my`-style declaration dispatch and as
    // a call argument) takes different parser paths from the statement form.
    for (source, expected) in [
        ("my $keep = local $main::z = 'a';", "local $main::z = 'a'"),
        ("foo(local $main::z = 'a');", "local $main::z = 'a'"),
    ] {
        assert_declaration_spans(source, "local", expected)?;
    }
    Ok(())
}

#[test]
fn bare_local_and_my_spans_are_unchanged() -> Result<(), String> {
    assert_declaration_spans("local $main::z;", "local", "local $main::z")?;
    assert_declaration_spans("my $q = 'a';", "my", "my $q = 'a'")?;
    assert_declaration_spans("my $q .= 'a';", "my", "my $q .= 'a'")
}
