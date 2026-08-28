mod cpan_test_helpers;

use cpan_test_helpers::*;
use perl_parser_core::{Node, NodeKind};

fn collect_indirect_calls<'a>(node: &'a Node, calls: &mut Vec<(&'a str, &'a Node, &'a [Node])>) {
    if let NodeKind::IndirectCall { method, object, args } = &node.kind {
        calls.push((method.as_str(), object.as_ref(), args.as_slice()));
    }

    for child in node.children() {
        collect_indirect_calls(child, calls);
    }
}

fn assert_numeric_scalar_filehandle(source: &str) -> Result<(), String> {
    assert_clean_parse(source);
    assert_no_blocking_diagnostics(source);

    let ast = parse(source);
    let mut calls = Vec::new();
    collect_indirect_calls(&ast, &mut calls);

    let Some((method, object, args)) = calls.first().copied() else {
        return Err(format!("expected an indirect print call, got {}", ast.to_sexp()));
    };

    if calls.len() != 1 {
        return Err(format!("expected one indirect call, got {calls:?}"));
    }
    if method != "print" {
        return Err(format!("expected print method, got {method}"));
    }

    match &object.kind {
        NodeKind::Variable { sigil, name } if sigil == "$" && name == "fh" => {}
        other => return Err(format!("expected $fh filehandle object, got {other:?}")),
    }

    if args.len() != 1 {
        return Err(format!("expected one print argument, got {args:?}"));
    }
    let Some(argument) = args.first() else {
        return Err("expected one print argument".to_string());
    };
    match &argument.kind {
        NodeKind::Number { value } if value == "1" => Ok(()),
        other => Err(format!("expected numeric argument 1, got {other:?}")),
    }
}

fn assert_not_indirect(source: &str) -> Result<(), String> {
    assert_clean_parse(source);
    assert_no_blocking_diagnostics(source);

    let ast = parse(source);
    let mut calls = Vec::new();
    collect_indirect_calls(&ast, &mut calls);
    if calls.is_empty() {
        Ok(())
    } else {
        Err(format!("expected regular print operands, got {calls:?}: {}", ast.to_sexp()))
    }
}

#[test]
fn statement_context_preserves_numeric_scalar_filehandle() -> Result<(), String> {
    assert_numeric_scalar_filehandle("print $fh 1;")
}

#[test]
fn expression_context_preserves_numeric_scalar_filehandle() -> Result<(), String> {
    assert_numeric_scalar_filehandle("my $ok = print $fh 1;")
}

#[test]
fn comma_control_stays_a_regular_print_list() -> Result<(), String> {
    assert_not_indirect("print $fh, 1;")
}

#[test]
fn subscript_controls_stay_regular_print_operands() -> Result<(), String> {
    for source in ["print $hash{key};", "print $array[0];"] {
        assert_not_indirect(source)?;
    }
    Ok(())
}
