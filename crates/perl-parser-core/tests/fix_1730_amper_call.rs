// Acceptance and regression tests for issue #1730:
// Ampersand-sigil function calls must produce AmperCall nodes, not FunctionCall.
//
// Before this fix, `&foo(1, 2)` and `foo(1, 2)` produced identical
// FunctionCall nodes. AmperCall preserves the & context so downstream
// consumers (linter, debugger, goto-definition) can distinguish the two forms.

mod cpan_test_helpers;
use cpan_test_helpers::*;

use perl_parser_core::NodeKind;

type TestResult = Result<(), String>;

// ── helpers ─────────────────────────────────────────────────────────────────

fn first_statement_expr(source: &str) -> Result<perl_parser_core::Node, String> {
    let ast = parse(source);
    let kind_name = ast.kind.kind_name();
    let NodeKind::Program { statements } = ast.into_parts().0 else {
        return Err(format!("expected Program, got {kind_name:?}"));
    };
    let Some(stmt) = statements.into_iter().next() else {
        return Err("expected at least one statement".to_string());
    };
    match stmt.into_parts().0 {
        NodeKind::ExpressionStatement { expression } => Ok(*expression),
        other => Err(format!("expected ExpressionStatement, got {other:?}")),
    }
}

// ── Acceptance: &foo produces AmperCall ─────────────────────────────────────

#[test]
fn amper_call_with_args_produces_amper_call_node() -> TestResult {
    let expr = first_statement_expr("&foo(1, 2);")?;
    assert!(
        matches!(expr.kind, NodeKind::AmperCall { .. }),
        "&foo(1, 2) must produce AmperCall, got: {:?}",
        expr.kind.kind_name()
    );
    Ok(())
}

#[test]
fn amper_call_location_includes_parenthesized_arguments() -> TestResult {
    let source = "&foo(1, 2);";
    let expr = first_statement_expr(source)?;
    assert_eq!(expr.location.start, 0);
    assert_eq!(
        expr.location.end,
        source.find(';').ok_or("expected statement terminator")?,
        "AmperCall location must include its arguments and closing parenthesis"
    );
    Ok(())
}

#[test]
fn amper_call_without_parens_produces_amper_call_node() -> TestResult {
    // &foo with no parens: forwards caller's @_ verbatim in Perl
    let expr = first_statement_expr("&foo;")?;
    assert!(
        matches!(expr.kind, NodeKind::AmperCall { .. }),
        "&foo (no parens) must produce AmperCall, got: {:?}",
        expr.kind.kind_name()
    );
    Ok(())
}

#[test]
fn amper_call_preserves_function_name() -> TestResult {
    let expr = first_statement_expr("&bar(1);")?;
    let NodeKind::AmperCall { name, .. } = expr.into_parts().0 else {
        return Err("expected AmperCall for &bar(1)".to_string());
    };
    assert_eq!(name, "bar", "&bar(1) must capture name 'bar'");
    Ok(())
}

#[test]
fn amper_call_qualified_name() -> TestResult {
    let expr = first_statement_expr("&Package::sub(42);")?;
    let NodeKind::AmperCall { name, args } = expr.into_parts().0 else {
        return Err("expected AmperCall for &Package::sub(42)".to_string());
    };
    assert_eq!(name, "Package::sub");
    assert_eq!(args.len(), 1, "expected 1 argument");
    Ok(())
}

#[test]
fn amper_call_empty_args_when_no_parens() -> TestResult {
    let expr = first_statement_expr("&helper;")?;
    let NodeKind::AmperCall { name, args } = expr.into_parts().0 else {
        return Err("expected AmperCall for &helper".to_string());
    };
    assert_eq!(name, "helper");
    assert!(args.is_empty(), "no-paren form must have empty args");
    Ok(())
}

fn collect_kinds(node: &perl_parser_core::Node, out: &mut Vec<&'static str>) {
    out.push(node.kind.kind_name());
    for child in node.children() {
        collect_kinds(child, out);
    }
}

#[test]
fn amper_call_dynamic_coderef_with_args_produces_amper_call_node() {
    let ast = parse("sub wrapper { my $callback = sub {}; &$callback(1); }");
    let mut found = Vec::new();
    collect_kinds(&ast, &mut found);
    assert!(
        found.contains(&"AmperCall"),
        "&$callback(1) must include AmperCall node, got kinds: {found:?}"
    );
}

#[test]
fn goto_amper_dynamic_coderef_target_is_amper_call() -> TestResult {
    let ast = parse("goto &$callback;");
    let NodeKind::Program { statements } = ast.into_parts().0 else {
        return Err("expected Program".to_string());
    };
    let Some(stmt) = statements.into_iter().next() else {
        return Err("expected statement".to_string());
    };
    let stmt_kind_name = stmt.kind.kind_name();
    let NodeKind::Goto { target, .. } = stmt.into_parts().0 else {
        return Err(format!("expected Goto node, got: {stmt_kind_name:?}"));
    };
    assert!(
        matches!(target.kind, NodeKind::AmperCall { .. }),
        "goto &$callback target must be AmperCall, got: {:?}",
        target.kind.kind_name()
    );
    Ok(())
}

#[test]
fn amper_call_parses_cleanly() {
    assert_clean_parse("&foo(1, 2, 3);");
    assert_clean_parse("&Package::bar();");
    assert_clean_parse("&helper;");
    assert_clean_parse("my $r = &compute($x);");
}

// ── Goto &sub: target is AmperCall ──────────────────────────────────────────

#[test]
fn goto_amper_sub_target_is_amper_call() -> TestResult {
    // `goto &sub` is Perl's tail-call idiom; the target should now be AmperCall
    let ast = parse("goto &helper;");
    let NodeKind::Program { statements } = ast.into_parts().0 else {
        return Err("expected Program".to_string());
    };
    let Some(stmt) = statements.into_iter().next() else {
        return Err("expected statement".to_string());
    };
    // goto is parsed as a statement-level Goto node, not wrapped in ExpressionStatement
    let stmt_kind_name = stmt.kind.kind_name();
    let NodeKind::Goto { target, .. } = stmt.into_parts().0 else {
        return Err(format!("expected Goto node, got: {stmt_kind_name:?}"));
    };
    assert!(
        matches!(target.kind, NodeKind::AmperCall { .. }),
        "goto &helper target must be AmperCall, got: {:?}",
        target.kind.kind_name()
    );
    Ok(())
}

#[test]
fn goto_amper_sub_parses_cleanly() {
    assert_clean_parse("goto &helper;");
    assert_clean_parse("sub wrapper { goto &handler; }");
}

// ── Regression: plain foo() stays FunctionCall ──────────────────────────────

#[test]
fn plain_function_call_stays_function_call() -> TestResult {
    let expr = first_statement_expr("foo(1, 2);")?;
    assert!(
        matches!(expr.kind, NodeKind::FunctionCall { .. }),
        "foo(1, 2) must stay FunctionCall (no sigil), got: {:?}",
        expr.kind.kind_name()
    );
    Ok(())
}

#[test]
fn method_call_unaffected() -> TestResult {
    let expr = first_statement_expr("$obj->method(1);")?;
    assert!(
        matches!(expr.kind, NodeKind::MethodCall { .. }),
        "$obj->method(1) must stay MethodCall, got: {:?}",
        expr.kind.kind_name()
    );
    Ok(())
}

#[test]
fn plain_call_name_unaffected() -> TestResult {
    let expr = first_statement_expr("say('hello');")?;
    let NodeKind::FunctionCall { name, .. } = expr.into_parts().0 else {
        return Err("expected FunctionCall for say()".to_string());
    };
    assert_eq!(name, "say");
    Ok(())
}

// ── Regression: other sigil variables unaffected ────────────────────────────

#[test]
fn dollar_variable_unaffected() {
    assert_clean_parse("my $x = 1;");
    assert_clean_parse("print $x;");
}

#[test]
fn array_variable_unaffected() {
    assert_clean_parse("my @arr = (1, 2, 3);");
}

// ── Sexp output ─────────────────────────────────────────────────────────────

#[test]
fn amper_call_sexp_starts_with_amper_call() -> TestResult {
    let expr = first_statement_expr("&foo(1);")?;
    let sexp = expr.to_sexp();
    assert!(
        sexp.starts_with("(amper_call"),
        "&foo(1) sexp must start with '(amper_call', got: {sexp}"
    );
    Ok(())
}

#[test]
fn amper_call_no_parens_sexp() -> TestResult {
    let expr = first_statement_expr("&foo;")?;
    let sexp = expr.to_sexp();
    assert!(sexp.contains("amper_call"), "&foo sexp must contain 'amper_call', got: {sexp}");
    Ok(())
}
