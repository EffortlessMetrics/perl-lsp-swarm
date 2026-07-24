// Acceptance and regression tests for issue #1730:
// Ampersand-sigil function calls must produce AmperCall nodes, not FunctionCall.
//
// Before this fix, `&foo(1, 2)` and `foo(1, 2)` produced identical
// FunctionCall nodes. AmperCall preserves the & context so downstream
// consumers (linter, debugger, goto-definition) can distinguish the two forms.

mod cpan_test_helpers;
use cpan_test_helpers::*;

use perl_parser_core::NodeKind;

// ── helpers ─────────────────────────────────────────────────────────────────

fn first_statement_expr(source: &str) -> perl_parser_core::Node {
    let ast = parse(source);
    let NodeKind::Program { statements } = ast.kind else {
        panic!("expected Program, got {:?}", ast.kind.kind_name());
    };
    let stmt = statements.into_iter().next().expect("expected at least one statement");
    match stmt.kind {
        NodeKind::ExpressionStatement { expression } => *expression,
        other => panic!("expected ExpressionStatement, got {other:?}"),
    }
}

// ── Acceptance: &foo produces AmperCall ─────────────────────────────────────

#[test]
fn amper_call_with_args_produces_amper_call_node() {
    let expr = first_statement_expr("&foo(1, 2);");
    assert!(
        matches!(expr.kind, NodeKind::AmperCall { .. }),
        "&foo(1, 2) must produce AmperCall, got: {:?}",
        expr.kind.kind_name()
    );
}

#[test]
fn amper_call_without_parens_produces_amper_call_node() {
    // &foo with no parens: forwards caller's @_ verbatim in Perl
    let expr = first_statement_expr("&foo;");
    assert!(
        matches!(expr.kind, NodeKind::AmperCall { .. }),
        "&foo (no parens) must produce AmperCall, got: {:?}",
        expr.kind.kind_name()
    );
}

#[test]
fn amper_call_preserves_function_name() {
    let expr = first_statement_expr("&bar(1);");
    let NodeKind::AmperCall { name, .. } = expr.kind else {
        panic!("expected AmperCall for &bar(1)");
    };
    assert_eq!(name, "bar", "&bar(1) must capture name 'bar'");
}

#[test]
fn amper_call_qualified_name() {
    let expr = first_statement_expr("&Package::sub(42);");
    let NodeKind::AmperCall { name, args } = expr.kind else {
        panic!("expected AmperCall for &Package::sub(42)");
    };
    assert_eq!(name, "Package::sub");
    assert_eq!(args.len(), 1, "expected 1 argument");
}

#[test]
fn amper_call_empty_args_when_no_parens() {
    let expr = first_statement_expr("&helper;");
    let NodeKind::AmperCall { name, args } = expr.kind else {
        panic!("expected AmperCall for &helper");
    };
    assert_eq!(name, "helper");
    assert!(args.is_empty(), "no-paren form must have empty args");
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
fn goto_amper_dynamic_coderef_target_is_amper_call() {
    let ast = parse("goto &$callback;");
    let NodeKind::Program { statements } = ast.kind else {
        panic!("expected Program");
    };
    let stmt = statements.into_iter().next().expect("expected statement");
    let NodeKind::Goto { target, .. } = stmt.kind else {
        panic!("expected Goto node, got: {:?}", stmt.kind.kind_name());
    };
    assert!(
        matches!(target.kind, NodeKind::AmperCall { .. }),
        "goto &$callback target must be AmperCall, got: {:?}",
        target.kind.kind_name()
    );
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
fn goto_amper_sub_target_is_amper_call() {
    // `goto &sub` is Perl's tail-call idiom; the target should now be AmperCall
    let ast = parse("goto &helper;");
    let NodeKind::Program { statements } = ast.kind else {
        panic!("expected Program");
    };
    let stmt = statements.into_iter().next().expect("expected statement");
    // goto is parsed as a statement-level Goto node, not wrapped in ExpressionStatement
    let NodeKind::Goto { target, .. } = stmt.kind else {
        panic!("expected Goto node, got: {:?}", stmt.kind.kind_name());
    };
    assert!(
        matches!(target.kind, NodeKind::AmperCall { .. }),
        "goto &helper target must be AmperCall, got: {:?}",
        target.kind.kind_name()
    );
}

#[test]
fn goto_amper_sub_parses_cleanly() {
    assert_clean_parse("goto &helper;");
    assert_clean_parse("sub wrapper { goto &handler; }");
}

// ── Regression: plain foo() stays FunctionCall ──────────────────────────────

#[test]
fn plain_function_call_stays_function_call() {
    let expr = first_statement_expr("foo(1, 2);");
    assert!(
        matches!(expr.kind, NodeKind::FunctionCall { .. }),
        "foo(1, 2) must stay FunctionCall (no sigil), got: {:?}",
        expr.kind.kind_name()
    );
}

#[test]
fn method_call_unaffected() {
    let expr = first_statement_expr("$obj->method(1);");
    assert!(
        matches!(expr.kind, NodeKind::MethodCall { .. }),
        "$obj->method(1) must stay MethodCall, got: {:?}",
        expr.kind.kind_name()
    );
}

#[test]
fn plain_call_name_unaffected() {
    let expr = first_statement_expr("say('hello');");
    let NodeKind::FunctionCall { name, .. } = expr.kind else {
        panic!("expected FunctionCall for say()");
    };
    assert_eq!(name, "say");
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
fn amper_call_sexp_starts_with_amper_call() {
    let expr = first_statement_expr("&foo(1);");
    let sexp = expr.to_sexp();
    assert!(
        sexp.starts_with("(amper_call"),
        "&foo(1) sexp must start with '(amper_call', got: {sexp}"
    );
}

#[test]
fn amper_call_no_parens_sexp() {
    let expr = first_statement_expr("&foo;");
    let sexp = expr.to_sexp();
    assert!(sexp.contains("amper_call"), "&foo sexp must contain 'amper_call', got: {sexp}");
}
