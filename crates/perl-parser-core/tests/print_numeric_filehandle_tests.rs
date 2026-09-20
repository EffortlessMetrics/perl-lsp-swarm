mod cpan_test_helpers;

use cpan_test_helpers::*;
use perl_parser_core::hir::{HirExpr, HirExprId, HirKind, lower_ast};
use perl_parser_core::pir::{PirMethod, PirOperation, lower_hir, lower_hir_bodies};
use perl_parser_core::{Node, NodeKind, Parser, SourceLocation};

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

fn first_indirect_call_range(node: &Node) -> Option<SourceLocation> {
    if matches!(node.kind, NodeKind::IndirectCall { .. }) {
        return Some(node.location);
    }
    node.children().into_iter().find_map(first_indirect_call_range)
}

fn lower_source(source: &str) -> perl_parser_core::hir::HirFile {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    lower_ast(&output.ast)
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
fn supported_builtins_preserve_numeric_scalar_filehandles_at_statement_start() -> Result<(), String>
{
    for (method, source) in
        [("print", "print $fh 1;"), ("printf", "printf $fh 1;"), ("say", "say $fh 1;")]
    {
        assert_scalar_filehandle_call(source, method, ExpectedArgument::Number("1"))?;
    }
    Ok(())
}

#[test]
fn supported_builtins_preserve_numeric_scalar_filehandles_in_expression_context()
-> Result<(), String> {
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
    for source in ["print $fh, 1;", "my $ok = print $fh, 1;"] {
        assert_clean_regular_call(source)?;
    }
    Ok(())
}

#[test]
fn subscript_controls_stay_regular_print_operands() -> Result<(), String> {
    for source in [
        "print $hash{key};",
        "print $array[0];",
        "my $ok = print $hash{key};",
        "my $ok = print $array[0];",
    ] {
        assert_clean_regular_call(source)?;
    }
    Ok(())
}

#[test]
fn numeric_scalar_filehandle_reaches_ast_hir_and_pir_with_one_range() -> Result<(), String> {
    let source = "print $fh 0x10;";
    assert_clean_parse(source);
    assert_no_blocking_diagnostics(source);

    let ast = parse(source);
    let ast_range = first_indirect_call_range(&ast)
        .ok_or_else(|| "AST did not retain an IndirectCall node".to_string())?;
    let ast_text = source
        .get(ast_range.start()..ast_range.end())
        .ok_or_else(|| format!("AST range is outside source: {ast_range:?}"))?;
    if !ast_text.starts_with("print $fh 0x10") {
        return Err(format!("AST range lost the numeric scalar-filehandle form: {ast_text:?}"));
    }

    let hir = lower_source(source);
    let item = hir
        .items
        .iter()
        .find(|item| matches!(item.kind, HirKind::IndirectCallExpr(_)))
        .ok_or_else(|| "flat HIR did not retain IndirectCallExpr".to_string())?;
    if item.range != ast_range {
        return Err(format!("flat HIR range {:?} differs from AST {:?}", item.range, ast_range));
    }

    let flat_pir = lower_hir(&hir);
    let pir_node = flat_pir
        .nodes
        .iter()
        .find(|node| {
            matches!(
                &node.operation,
                PirOperation::MethodCall {
                    method: PirMethod::Named(method),
                    arg_count: 1,
                    ..
                } if method == "print"
            )
        })
        .ok_or_else(|| "flat PIR did not retain the indirect print operation".to_string())?;
    if pir_node.source_anchor.range != Some(ast_range) {
        return Err(format!(
            "flat PIR anchor {:?} differs from AST {:?}",
            pir_node.source_anchor.range, ast_range
        ));
    }

    let body = hir.root_body().ok_or_else(|| "canonical HIR root body is missing".to_string())?;
    let (body_call_id, body_call_args, body_call_range) = (0..body.exprs.len())
        .find_map(|index| {
            let id = HirExprId(index as u32);
            match body.expr(id) {
                Some(HirExpr::Call { ast_kind, args, .. }) if ast_kind == "IndirectCall" => {
                    Some((id, (args.clone(), body.source_map.expr_range(id))))
                }
                _ => None,
            }
        })
        .and_then(|(id, (args, range))| range.map(|range| (id, args, range)))
        .ok_or_else(|| "canonical HIR body did not retain IndirectCall".to_string())?;
    if body_call_range != ast_range {
        return Err(format!(
            "canonical HIR call range {:?} differs from AST {:?}",
            body_call_range, ast_range
        ));
    }
    let body_call = body
        .expr(body_call_id)
        .ok_or_else(|| "canonical HIR call disappeared from its arena".to_string())?;
    let HirExpr::Call { args, .. } = body_call else {
        return Err("canonical HIR call changed shape while checking its range".to_string());
    };
    if args != &body_call_args
        || args.len() != 2
        || !matches!(body.expr(args[0]), Some(HirExpr::Variable(variable)) if variable.name == "fh")
    {
        return Err(format!("canonical HIR lost the filehandle operand: {body_call:?}"));
    }

    let body_pir = lower_hir_bodies(&hir);
    if body_pir.receipt.unsupported_construct_counts.get("Call") != Some(&1) {
        return Err(format!(
            "canonical PIR did not record the reached indirect call shell: {:?}",
            body_pir.receipt.unsupported_construct_counts
        ));
    }
    if !body_pir.nodes.iter().any(|node| {
        matches!(
            &node.operation,
            PirOperation::StashRead { symbol } if symbol.name == "fh"
        ) && node.source_anchor.range == Some(SourceLocation::new(6, 9))
    }) {
        return Err(format!(
            "canonical PIR did not preserve the $fh operand range: {:?}",
            body_pir.nodes
        ));
    }
    Ok(())
}

#[test]
fn numeric_literal_matrix_stays_an_indirect_print_argument() -> Result<(), String> {
    for literal in ["1", "0x10", "0x1F", "1_000", "1.5"] {
        let source = format!("print $fh {literal};");
        assert_scalar_filehandle_call(&source, "print", ExpectedArgument::Number(literal))?;
    }
    Ok(())
}

#[test]
fn comma_list_keeps_the_full_indirect_argument_shape() -> Result<(), String> {
    for source in ["print $fh 1, 2, 3;", "my $ok = print $fh 1, 2, 3;"] {
        assert_clean_parse(source);
        assert_no_blocking_diagnostics(source);

        let ast = parse(source);
        let mut calls = Vec::new();
        collect_indirect_calls(&ast, &mut calls);

        if calls.len() != 1 {
            return Err(format!("expected one indirect call, got {calls:?}"));
        }
        let Some((method, object, args)) = calls.first().copied() else {
            return Err(format!("expected an indirect print call, got {}", ast.to_sexp()));
        };
        if method != "print" {
            return Err(format!("expected print method, got {method}"));
        }
        match &object.kind {
            NodeKind::Variable { sigil, name } if sigil == "$" && name == "fh" => {}
            other => return Err(format!("expected $fh filehandle object, got {other:?}")),
        }
        let numbers: Vec<&str> = args
            .iter()
            .filter_map(|arg| match &arg.kind {
                NodeKind::Number { value } => Some(value.as_str()),
                _ => None,
            })
            .collect();
        if numbers != ["1", "2", "3"] {
            return Err(format!("expected numeric argument list [1, 2, 3], got {args:?}"));
        }
    }
    Ok(())
}

#[test]
fn negative_literal_stays_a_regular_print_list() -> Result<(), String> {
    // `-1` lexes as `Minus Number`, not a single numeric term, so it must not
    // open the indirect scalar-filehandle route: `print $fh -1;` stays an
    // ordinary print list over `$fh` and `-1`.
    for source in ["print $fh -1;", "my $ok = print $fh -1;"] {
        assert_clean_parse(source);
        assert_no_blocking_diagnostics(source);
        assert_no_indirect_call(source)?;
    }
    Ok(())
}

#[test]
fn malformed_numeric_filehandle_forms_never_claim_an_indirect_call() -> Result<(), String> {
    for source in [
        "print $fh 1 2;",
        "print $fh 1 \"x\";",
        "print $fh 1 $x;",
        "print $fh 1, 2 3;",
        "print $fh {;",
        "print $fh %;",
        "my $ok = print $fh 1 2;",
        "my $ok = print $fh 1 \"x\";",
        "my $ok = print $fh 1 $x;",
        "my $ok = print $fh 1, 2 3;",
        "my $ok = print $fh %;",
    ] {
        assert_has_error(source, "");
        assert_no_indirect_call(source)?;
    }
    Ok(())
}
