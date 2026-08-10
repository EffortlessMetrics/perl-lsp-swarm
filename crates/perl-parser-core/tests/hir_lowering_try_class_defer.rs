//! HIR lowering tests for `try`/`catch`/`finally`, `class`, and `defer`.
//!
//! Pins the item-level HIR shells for `NodeKind::Try`, `Class`, and `Defer`
//! (issue #2192, sibling of #2195's regex-ops HIR shells). Each construct
//! lowers to exactly one typed HIR shell (`TryExpr`, `ClassDecl`,
//! `DeferExpr`); the body/catch/finally/class-body/deferred-block children
//! are still traversed via `visit_children`, mirroring how `Eval`/`Do`/
//! `Match` traverse their operands.
//!
//! The implementation lives in `crates/perl-parser-core/src/hir/lower.rs`
//! (the `NodeKind::Try`/`Class`/`Defer` arms).

use perl_parser_core::Parser;
use perl_parser_core::hir::{HirFile, HirKind, lower_ast};
use perl_tdd_support::must_some;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn lower_source(source: &str) -> HirFile {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    lower_ast(&output.ast)
}

fn has_call_named(file: &HirFile, name: &str) -> bool {
    file.items.iter().any(|item| matches!(&item.kind, HirKind::CallExpr(c) if c.name == name))
}

// ---------------------------------------------------------------------------
// try/catch (no finally)
// ---------------------------------------------------------------------------

#[test]
fn try_catch_lowers_to_try_expr_shell() -> TestResult {
    let file = lower_source("try { 1; } catch ($e) { 2; }\n");
    let try_expr = must_some(file.items.iter().find_map(|item| match &item.kind {
        HirKind::TryExpr(t) => Some(t),
        _ => None,
    }));
    assert_eq!(try_expr.catch_count, 1, "one catch clause must be recorded");
    assert!(!try_expr.has_finally, "no finally block present");
    Ok(())
}

// ---------------------------------------------------------------------------
// try/catch/finally
// ---------------------------------------------------------------------------

#[test]
fn try_catch_finally_lowers_to_try_expr_shell() -> TestResult {
    let file = lower_source("try { 1; } catch ($e) { 2; } finally { 3; }\n");
    let try_expr = must_some(file.items.iter().find_map(|item| match &item.kind {
        HirKind::TryExpr(t) => Some(t),
        _ => None,
    }));
    assert_eq!(try_expr.catch_count, 1, "one catch clause must be recorded");
    assert!(try_expr.has_finally, "finally block must be recorded as present");
    Ok(())
}

// ---------------------------------------------------------------------------
// try/catch without a bound exception variable
// ---------------------------------------------------------------------------

#[test]
fn try_catch_without_bound_variable_still_counts_catch() -> TestResult {
    let file = lower_source("try { 1; } catch { 2; }\n");
    let try_expr = must_some(file.items.iter().find_map(|item| match &item.kind {
        HirKind::TryExpr(t) => Some(t),
        _ => None,
    }));
    assert_eq!(try_expr.catch_count, 1, "bare `catch {{ ... }}` still counts as one clause");
    Ok(())
}

// ---------------------------------------------------------------------------
// try body/catch/finally are traversed: calls inside each still lower
// ---------------------------------------------------------------------------

#[test]
fn try_traverses_body_catch_and_finally() -> TestResult {
    // Guard the `visit_children` traversal in the Try arm: statements inside
    // the try body, the catch handler, and the finally block must all still
    // lower to their own HIR items — the TryExpr shell must not swallow them.
    let file = lower_source("try { foo(); } catch ($e) { bar(); } finally { baz(); }\n");

    let has_try = file.items.iter().any(|item| matches!(&item.kind, HirKind::TryExpr(_)));
    assert!(has_try, "expected a TryExpr HIR item");

    assert!(has_call_named(&file, "foo"), "try body call `foo()` must still lower to CallExpr");
    assert!(has_call_named(&file, "bar"), "catch body call `bar()` must still lower to CallExpr");
    assert!(has_call_named(&file, "baz"), "finally body call `baz()` must still lower to CallExpr");
    Ok(())
}

// ---------------------------------------------------------------------------
// class with a name (no parents)
// ---------------------------------------------------------------------------

#[test]
fn class_with_name_lowers_to_class_decl_shell() -> TestResult {
    let file = lower_source("class Dog {}\n");
    let class = must_some(file.items.iter().find_map(|item| match &item.kind {
        HirKind::ClassDecl(c) => Some(c),
        _ => None,
    }));
    assert_eq!(class.name, "Dog");
    assert!(class.parents.is_empty(), "no `:isa(...)` attribute means no parents");
    Ok(())
}

// ---------------------------------------------------------------------------
// class with a parent via `:isa(Parent)`
// ---------------------------------------------------------------------------

#[test]
fn class_with_parent_records_parent_list() -> TestResult {
    let file = lower_source("class Dog :isa(Animal) {}\n");
    let class = must_some(file.items.iter().find_map(|item| match &item.kind {
        HirKind::ClassDecl(c) => Some(c),
        _ => None,
    }));
    assert_eq!(class.name, "Dog");
    assert_eq!(class.parents, vec!["Animal".to_string()], "parent class name must be recorded");
    Ok(())
}

// ---------------------------------------------------------------------------
// class body is traversed: a method inside still lowers to its own item
// ---------------------------------------------------------------------------

#[test]
fn class_traverses_body_method() -> TestResult {
    // Guard the `visit_children` traversal in the Class arm: a `method`
    // declaration (and the call inside it) must still lower to its own HIR
    // items — the ClassDecl shell must not swallow the class body.
    let file = lower_source("class Dog { method bark { woof(); } }\n");

    let has_class = file.items.iter().any(|item| matches!(&item.kind, HirKind::ClassDecl(_)));
    assert!(has_class, "expected a ClassDecl HIR item");

    let has_method = file.items.iter().any(|item| match &item.kind {
        HirKind::MethodDecl(m) => m.name == "bark",
        _ => false,
    });
    assert!(
        has_method,
        "expected the class body's `method bark` to lower to its own MethodDecl item.\n\
         HIR item count: {}",
        file.items.len()
    );
    assert!(has_call_named(&file, "woof"), "call inside the method body must still lower");
    Ok(())
}

// ---------------------------------------------------------------------------
// defer block
// ---------------------------------------------------------------------------

#[test]
fn defer_block_lowers_to_defer_expr_shell() -> TestResult {
    let file = lower_source("defer { 1; }\n");
    let has_defer = file.items.iter().any(|item| matches!(&item.kind, HirKind::DeferExpr(_)));
    assert!(has_defer, "expected a DeferExpr HIR item");
    Ok(())
}

// ---------------------------------------------------------------------------
// defer block is traversed: a call inside it still lowers
// ---------------------------------------------------------------------------

#[test]
fn defer_traverses_block_operand() -> TestResult {
    // Guard the `visit_children` traversal in the Defer arm: the deferred
    // block's own statements (here, the call `cleanup()`) must still lower
    // to their own HIR items — the DeferExpr shell must not swallow them.
    let file = lower_source("defer { cleanup(); }\n");

    let has_defer = file.items.iter().any(|item| matches!(&item.kind, HirKind::DeferExpr(_)));
    assert!(has_defer, "expected a DeferExpr HIR item");

    assert!(
        has_call_named(&file, "cleanup"),
        "expected the deferred block's `cleanup()` call to lower to its own CallExpr item.\n\
         HIR item count: {}",
        file.items.len()
    );

    // The deferred block is a plain `Block` node, so it must also emit its
    // own `BlockShell` exactly like any other block.
    let has_block_shell =
        file.items.iter().any(|item| matches!(&item.kind, HirKind::BlockShell(_)));
    assert!(has_block_shell, "expected the deferred block to also emit its own BlockShell");
    Ok(())
}
