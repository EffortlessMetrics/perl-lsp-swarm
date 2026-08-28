mod cpan_test_helpers;

use cpan_test_helpers::*;
use perl_parser_core::{Node, NodeKind};

#[derive(Debug)]
enum ExpectedArgument<'a> {
    Number(&'a str),
    Variable { sigil: &'a str, name: &'a str },
}

fn collect_indirect_calls<'a>(node: &'a Node, calls: &mut Vec<(&'a str, &'a Node, &'a [Node])>) {
    if let NodeKind::IndirectCall { method, object, args } = &node.kind {
        calls.push((method.as_str(), object.as_ref(), args.as_slice()));
    }

    for child in node.children() {
        collect_indirect_calls(child, calls);
    }
}

fn assert_scalar_filehandle_call(
    source: &str,
    expected_method: &str,
    expected_argument: ExpectedArgument<'_>,
) -> Result<(), String> {
    assert_clean_parse(source);
    assert_no_blocking_diagnostics(source);

    let ast = parse(source);
    let mut calls = Vec::new();
    collect_indirect_calls(&ast, &mut calls);

    let Some((method, object, args)) = calls.first().copied() else {
        return Err(format!("expected an indirect {expected_method} call, got {}", ast.to_sexp()));
    };

    if calls.len() != 1 {
        return Err(format!("expected one indirect call, got {calls:?}"));
    }
    if method != expected_method {
        return Err(format!("expected {expected_method} method, got {method}"));
    }

    match &object.kind {
        NodeKind::Variable { sigil, name } if sigil == "$" && name == "fh" => {}
        other => return Err(format!("expected $fh filehandle object, got {other:?}")),
    }

    if args.len() != 1 {
        return Err(format!("expected one {expected_method} argument, got {args:?}"));
    }
    let Some(argument) = args.first() else {
        return Err(format!("expected one {expected_method} argument"));
    };

    match (expected_argument, &argument.kind) {
        (ExpectedArgument::Number(expected), NodeKind::Number { value }) if value == expected => {
            Ok(())
        }
        (
            ExpectedArgument::Variable { sigil: expected_sigil, name: expected_name },
            NodeKind::Variable { sigil, name },
        ) if sigil == expected_sigil && name == expected_name => Ok(()),
        (expected, actual) => Err(format!("expected {expected:?} argument, got {actual:?}")),
    }
}

fn assert_no_indirect_call(source: &str) -> Result<(), String> {
    let ast = parse(source);
    let mut calls = Vec::new();
    collect_indirect_calls(&ast, &mut calls);
    if calls.is_empty() {
        Ok(())
    } else {
        Err(format!("expected no indirect call, got {calls:?}: {}", ast.to_sexp()))
    }
}

fn assert_clean_regular_call(source: &str) -> Result<(), String> {
    assert_clean_parse(source);
    assert_no_blocking_diagnostics(source);
    assert_no_indirect_call(source)
}

#[test]
fn supported_builtins_preserve_numeric_scalar_filehandles_at_statement_start() -> Result<(), String> {
    for (method, source) in [
        ("print", "print $fh 1;"),
        ("printf", "printf $fh 1;"),
        ("say", "say $fh 1;"),
    ] {
        assert_scalar_filehandle_call(source, method, ExpectedArgument::Number("1"))?;
    }
    Ok(())
}

#[test]
fn supported_builtins_preserve_numeric_scalar_filehandles_in_expression_context() -> Result<(), String> {
    for (method, source) in [
        ("print", "my $ok = print $fh 1;"),
        ("printf", "my $ok = printf $fh 1;"),
        ("say", "my $ok = say $fh 1;"),
    ] {
        assert_scalar_filehandle_call(source, method, ExpectedArgument::Number("1"))?;
    }
    Ok(())
}

#[test]
fn expression_context_preserves_hash_message_after_scalar_filehandle() -> Result<(), String> {
    assert_scalar_filehandle_call(
        "my $ok = print $fh %hash;",
        "print",
        ExpectedArgument::Variable { sigil: "%", name: "hash" },
    )
}

#[test]
fn numeric_terms_do_not_enable_other_indirect_builtins() -> Result<(), String> {
    for source in ["open $fh 1;", "sysread $fh 1;", "seek $fh 1;"] {
        assert_no_indirect_call(source)?;
    }
    Ok(())
}

#[test]
fn comma_control_stays_a_regular_print_list() -> Result<(), String> {
    assert_clean_regular_call("print $fh, 1;")
}

#[test]
fn subscript_controls_stay_regular_print_operands() -> Result<(), String> {
    for source in ["print $hash{key};", "print $array[0];"] {
        assert_clean_regular_call(source)?;
    }
    Ok(())
}
