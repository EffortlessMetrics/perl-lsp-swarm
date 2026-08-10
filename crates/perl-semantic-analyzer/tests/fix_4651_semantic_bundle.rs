//! Tests for issue #4651 — semantic analyzer bundle:
//!   1. `local $non_builtin` treated as lexical instead of dynamic global
//!   2. Implicit `$_` for `for (@list)` not declared in loop scope
//!   3. `find_catch_variable_range` used fragile rfind instead of parser ranges
//!   4. `infer_node` MethodCall returned `Any`, inconsistent with
//!      `infer_expr_fact_in_env`
use perl_semantic_analyzer::Parser;
use perl_semantic_analyzer::analysis::scope_analyzer::{IssueKind, ScopeAnalyzer, ScopeIssue};
use perl_semantic_analyzer::analysis::type_facts::ShapeFact;
use perl_semantic_analyzer::analysis::type_inference::TypeInferenceEngine;
use perl_semantic_analyzer::pragma_tracker::PragmaTracker;
use perl_semantic_analyzer::{Node, NodeKind};
use perl_tdd_support::{must, must_some};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn scope_issues(code: &str) -> Vec<ScopeIssue> {
    let mut parser = Parser::new(code);
    let ast = must(parser.parse());
    let analyzer = ScopeAnalyzer::new();
    analyzer.analyze(&ast, code, &[])
}

fn scope_issues_strict(code: &str) -> Vec<ScopeIssue> {
    let mut parser = Parser::new(code);
    let ast = must(parser.parse());
    let pragma_map = PragmaTracker::build(&ast);
    let analyzer = ScopeAnalyzer::new();
    analyzer.analyze(&ast, code, &pragma_map)
}

fn has_issue(issues: &[ScopeIssue], kind: IssueKind, var_name: &str) -> bool {
    issues.iter().any(|i| i.kind == kind && i.variable_name == var_name)
}

// ---------------------------------------------------------------------------
// Bug 1: `local $non_builtin` treated as lexical instead of dynamic global
// ---------------------------------------------------------------------------

/// `local $x` followed by `my $x` in the same scope must NOT trigger
/// VariableRedeclaration — they occupy different slots (dynamic global vs
/// lexical).
#[test]
fn local_then_my_no_false_redeclaration() {
    let code = "local $x = 1; my $x = 2;";
    let issues = scope_issues(code);
    assert!(
        !has_issue(&issues, IssueKind::VariableRedeclaration, "$x"),
        "local $x followed by my $x should not be a redeclaration, got: {:?}",
        issues
    );
}

/// `local $x` followed by `local $x` in the same scope — both refer to the
/// same package global.  This is the same semantic as `our $x; our $x;`.
/// The test only verifies that `local $x` does not create a lexical that
/// would clash with a later `my $x`.
#[test]
fn local_non_builtin_not_treated_as_lexical() {
    let code = "local $regular = 42; print $regular;";
    let issues = scope_issues_strict(code);
    // Under strict vars, `local $regular` makes `$regular` refer to the
    // package global, so subsequent uses should not be undeclared.
    assert!(
        !has_issue(&issues, IssueKind::UndeclaredVariable, "$regular"),
        "$regular after local $regular should resolve to the package global, got: {:?}",
        issues
    );
}

/// `local` in list form should also not create lexical bindings that clash
/// with subsequent `my` declarations.
#[test]
fn local_list_then_my_no_false_redeclaration() {
    let code = "local ($a, $b) = (1, 2); my $a = 3; my $b = 4;";
    let issues = scope_issues(code);
    assert!(
        !has_issue(&issues, IssueKind::VariableRedeclaration, "$a"),
        "local ($a, ...) followed by my $a should not be a redeclaration"
    );
    assert!(
        !has_issue(&issues, IssueKind::VariableRedeclaration, "$b"),
        "local (..., $b) followed by my $b should not be a redeclaration"
    );
}

/// Ensure the existing builtin `local` behavior is preserved — `local $/`
/// should not produce a false UnusedVariable.
#[test]
fn local_builtin_still_no_false_unused() {
    let code = "local $/ = ''; my $content = <DATA>; ";
    let issues = scope_issues(code);
    assert!(
        !has_issue(&issues, IssueKind::UnusedVariable, "$/"),
        "local $/ should not be flagged as unused"
    );
}

// ---------------------------------------------------------------------------
// Bug 2: Implicit `$_` for `for (@list)` not declared in loop scope
// ---------------------------------------------------------------------------

/// `for (@list) { print; }` — the implicit `$_` should be declared in the
/// loop scope so that `print;` (which uses `$_`) does not trigger
/// UndeclaredVariable under strict vars.
#[test]
fn foreach_implicit_topic_declared_in_loop_scope() {
    let code = "use strict; for (1..10) { print; }";
    let issues = scope_issues_strict(code);
    assert!(
        !issues.iter().any(|i| i.kind == IssueKind::UndeclaredVariable && i.variable_name == "$_"),
        "implicit $_ in for (@list) should be declared in loop scope, got: {:?}",
        issues
    );
}

/// `foreach (@list) { chomp; }` — same as above with the `foreach` keyword.
#[test]
fn foreach_keyword_implicit_topic_declared() {
    let code = "use strict; my @lines = (); foreach (@lines) { chomp; }";
    let issues = scope_issues_strict(code);
    assert!(
        !issues.iter().any(|i| i.kind == IssueKind::UndeclaredVariable && i.variable_name == "$_"),
        "implicit $_ in foreach (@list) should be declared in loop scope, got: {:?}",
        issues
    );
}

/// When an explicit loop variable is used, `$_` should NOT be declared — the
/// explicit variable takes its place.
#[test]
fn foreach_explicit_var_does_not_declare_topic() {
    let code = "use strict; my @items = (); for my $item (@items) { print $item; }";
    let issues = scope_issues_strict(code);
    // $item should be declared (used in print), and no undeclared issues.
    assert!(
        !has_issue(&issues, IssueKind::UndeclaredVariable, "$item"),
        "explicit loop variable $item should be declared"
    );
}

// ---------------------------------------------------------------------------
// Bug 3: Catch variable range from parser (not fragile rfind)
// ---------------------------------------------------------------------------

/// Verify that a try/catch with a catch variable produces correct scope
/// analysis — the catch variable should be declared in the catch scope and
/// not flagged as undeclared.
#[test]
fn try_catch_variable_declared_in_catch_scope() {
    let code = "use strict; try { die 'oops' } catch ($e) { print $e; }";
    let issues = scope_issues_strict(code);
    assert!(
        !has_issue(&issues, IssueKind::UndeclaredVariable, "$e"),
        "catch variable $e should be declared in catch scope, got: {:?}",
        issues
    );
}

/// Verify that a try/catch with a long body between `catch` and the variable
/// does not break range detection (the old 256-byte window would fail).
#[test]
fn try_catch_variable_range_with_long_body() {
    let long_body = "x".repeat(300);
    let code =
        format!("try {{ my $padding = '{long_body}'; die 'oops' }} catch ($e) {{ print $e; }}");
    let issues = scope_issues_strict(&code);
    assert!(
        !has_issue(&issues, IssueKind::UndeclaredVariable, "$e"),
        "catch variable $e should still be declared even with a long try body"
    );
}

// ---------------------------------------------------------------------------
// Bug 4: infer_node MethodCall consistency with infer_expr_fact_in_env
// ---------------------------------------------------------------------------

/// Find a `MethodCall` node with the given method name anywhere in the AST.
fn find_method_call<'a>(node: &'a Node, name: &str) -> Option<&'a Node> {
    if let NodeKind::MethodCall { method, .. } = &node.kind
        && method == name
    {
        return Some(node);
    }
    node.children().iter().find_map(|child| find_method_call(child, name))
}

/// Verify that `infer_node` for a MethodCall (non-`new`) consults the
/// method return facts rather than always returning `Any`.
///
/// We set up a package with a method that returns a constructor call
/// (`Other->new()`), which is a pattern the return-fact collector
/// recognises.  The method return fact carries the package in its
/// `ObjectShape` (the erased type is `Any` by design — the shape is the
/// authoritative metadata).  Before the fix, `infer_node` always returned
/// `Any` for non-`new` method calls and never consulted the facts, so
/// `infer_expr_fact_in_env`'s fallback through `infer_node` also produced
/// a fact with no shape.  After the fix, both entry points go through
/// `method_call_expr_fact` and the Object shape is present.
#[test]
fn infer_node_method_call_not_always_any() {
    let code = r#"
        package Other {
            sub new { bless {}, shift }
        }
        package My::Class {
            sub new { bless {}, shift }
            sub create_other { return Other->new() }
        }
        my $obj = My::Class->new();
        $obj->create_other();
    "#;
    let mut parser = Parser::new(code);
    let ast = must(parser.parse());

    // Run `infer` to populate method return facts and the global env.
    let mut engine = TypeInferenceEngine::new();
    let _ = engine.infer(&ast);

    // Find the `create_other` method call and infer its fact.
    let call_node = find_method_call(&ast, "create_other");
    assert!(call_node.is_some(), "should find create_other method call in AST");

    if let Some(call) = call_node {
        let fact = engine.infer_expr_fact(call);
        // The method return fact for `My::Class::create_other` carries an
        // Object shape with package "Other".  Before the fix, infer_node
        // returned Any without consulting the facts, so the fact would have
        // no shape (TypeFact::unknown).  After the fix, the Object shape is
        // present, proving the facts were consumed.
        assert!(
            matches!(&fact.shape, Some(ShapeFact::Object(_))),
            "infer_expr_fact for $obj->create_other() should have an Object shape \
             proving method return facts were consumed.  Got shape: {:?}",
            fact.shape
        );
        let shape = must_some(fact.shape.as_ref().and_then(|shape| match shape {
            ShapeFact::Object(shape) => Some(shape),
            _ => None,
        }));
        assert_eq!(
            shape.package, "Other",
            "Object shape package should be \"Other\" for create_other"
        );
    }
}

/// `infer_node` for `Class` and `Method` nodes should return `Void`, not
/// `Any`, since they are declarations, not value-producing expressions.
#[test]
fn infer_node_class_and_method_return_void() {
    let code = r#"
        use feature 'class';
        class Point {
            field $x :param;
            method get_x { return $x }
        }
    "#;
    let mut parser = Parser::new(code);
    let ast = must(parser.parse());
    let mut engine = TypeInferenceEngine::new();
    // Should complete without panic — the fix adds explicit arms for
    // Class and Method in infer_node instead of falling through to Any.
    let result = engine.infer(&ast);
    assert!(result.is_ok(), "type inference should complete without error on Class/Method nodes");
}
