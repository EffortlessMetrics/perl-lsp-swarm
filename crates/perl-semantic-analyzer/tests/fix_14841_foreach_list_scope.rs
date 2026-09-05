#![deny(clippy::map_err_ignore)]
// Cohort C1 activation (#12598): all production rows exact-excepted; new findings move the crate back to non-C1.
//! Tests for issue #14841 — a `foreach` list is analyzed before the loop
//! variable is declared, so the list cannot see the iterator.
//!
//! Real Perl (5.38.2 `perl -c`):
//!
//! ```text
//! $ perl -c -e 'use strict; for my $x ($x) { }'
//! Global symbol "$x" requires explicit package name (did you forget to declare "my $x"?) at -e line 1.
//! -e had compilation errors.
//!
//! $ perl -c -e 'for my $x ($x) { }'
//! -e syntax OK
//!
//! $ perl -c -e 'use strict; my $x = 1; for my $x ($x) { }'
//! -e syntax OK
//!
//! $ perl -c -e 'use strict; for my $x (my $y = 1) { print $y; }'
//! -e syntax OK
//!
//! $ perl -c -e 'use strict; for my $x (my $y = 1) { } print $y;'
//! Global symbol "$y" requires explicit package name ...
//! ```
//!
//! The analyzer previously analyzed the list inside `loop_scope` *after*
//! declaring the loop variable, so the strict self-reference produced no
//! `UndeclaredVariable`. A `my` in the list remains loop-scoped.

use perl_semantic_analyzer::Parser;
use perl_semantic_analyzer::analysis::scope_analyzer::{IssueKind, ScopeAnalyzer, ScopeIssue};
use perl_semantic_analyzer::pragma_tracker::PragmaTracker;
use perl_tdd_support::must;

fn scope_issues(code: &str) -> Vec<ScopeIssue> {
    let mut parser = Parser::new(code);
    let ast = must(parser.parse());
    let pragma_map = PragmaTracker::build(&ast);
    ScopeAnalyzer::new().analyze(&ast, code, &pragma_map)
}

fn has_undeclared(issues: &[ScopeIssue], var_name: &str) -> bool {
    issues.iter().any(|i| i.kind == IssueKind::UndeclaredVariable && i.variable_name == var_name)
}

/// `use strict; for my $x ($x) { }` — the list's `$x` is analyzed before the
/// loop variable is declared, so it does not resolve to the iterator.
#[test]
fn foreach_list_does_not_see_loop_variable_under_strict() {
    let code = "use strict; for my $x ($x) { }";
    let issues = scope_issues(code);
    let list_x = code.find("($x)").map(|i| i + 1);
    assert!(
        issues.iter().any(|i| {
            i.kind == IssueKind::UndeclaredVariable
                && i.variable_name == "$x"
                && list_x.is_some_and(|offset| i.range.0 == offset)
        }),
        "`use strict; for my $x ($x) {{ }}` must report UndeclaredVariable for the list $x, not the loop variable; got: {issues:?}"
    );
}

/// Without `use strict`, the list's `$x` is a package-global access — Perl
/// accepts it, and the analyzer must stay quiet.
#[test]
fn foreach_list_self_ref_without_strict_is_not_reported() {
    let code = "for my $x ($x) { }";
    let issues = scope_issues(code);
    assert!(
        !has_undeclared(&issues, "$x"),
        "`for my $x ($x) {{ }}` without strict must not report UndeclaredVariable; got: {issues:?}"
    );
}

/// Ordinary `foreach` with a declared list and a used loop variable stays clean.
#[test]
fn ordinary_foreach_with_declared_list_stays_clean() {
    let code = "use strict; my @l = (1, 2); for my $i (@l) { print $i; }";
    let issues = scope_issues(code);
    assert!(
        !has_undeclared(&issues, "$i") && !has_undeclared(&issues, "@l"),
        "ordinary `for my $i (@l)` must not report UndeclaredVariable; got: {issues:?}"
    );
}

/// Implicit-topic `for (@l) { print; }` still declares `$_` in the loop scope
/// so the body's implicit topic is not reported undeclared.
#[test]
fn implicit_topic_foreach_body_stays_resolved() {
    let code = "use strict; my @l = (1); for (@l) { print; }";
    let issues = scope_issues(code);
    assert!(
        !has_undeclared(&issues, "$_"),
        "implicit-topic `for (@l) {{ print; }}` must not report UndeclaredVariable for $_; got: {issues:?}"
    );
}

/// `use strict; my $x = 1; for my $x ($x) { }` — the list's `$x` resolves to
/// the outer binding. Legal Perl; must stay clean of UndeclaredVariable.
#[test]
fn foreach_list_resolves_outer_binding_when_loop_var_shadows() {
    let code = "use strict; my $x = 1; for my $x ($x) { }";
    let issues = scope_issues(code);
    assert!(
        !has_undeclared(&issues, "$x"),
        "`my $x = 1; for my $x ($x)` must resolve the list to the outer $x; got: {issues:?}"
    );
}

/// `for my $x (my $y = 1) { print $y; }` — a `my` in the list is loop-scoped
/// and must be visible in the body. perl 5.38.2: `-e syntax OK`.
#[test]
fn foreach_list_my_is_visible_in_body() {
    let code = "use strict; for my $x (my $y = 1) { print $y; }";
    let issues = scope_issues(code);
    assert!(
        !has_undeclared(&issues, "$y"),
        "`for my $x (my $y = 1) {{ print $y; }}` must not report UndeclaredVariable for $y; got: {issues:?}"
    );
}

/// `for my $x (my $y = 1) { } print $y;` — the list `my` must not leak into
/// the enclosing scope. perl 5.38.2 rejects `$y` after the loop.
#[test]
fn foreach_list_my_does_not_leak_after_loop() {
    let code = "use strict; for my $x (my $y = 1) { } print $y;";
    let issues = scope_issues(code);
    assert!(
        has_undeclared(&issues, "$y"),
        "`for my $x (my $y = 1) {{ }} print $y;` must report UndeclaredVariable for $y; got: {issues:?}"
    );
}
