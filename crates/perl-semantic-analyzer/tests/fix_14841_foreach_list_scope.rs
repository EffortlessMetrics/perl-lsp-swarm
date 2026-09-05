#![deny(clippy::map_err_ignore)]
// Cohort C1 activation (#12598): all production rows exact-excepted; new findings move the crate back to non-C1.
//! Tests for issue #14841 — a lexical `foreach` iterator is unavailable while
//! the list is analyzed, so `for my $x ($x)` is undeclared under strict.
//! An `our` iterator is a compile-time package alias: Perl accepts
//! `for our $x ($x)` under strict, and the analyzer must stay quiet.
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
//!
//! $ perl -c -e 'use strict; for our $x ($x) { }'
//! -e syntax OK
//!
//! $ perl -c -e 'use strict; foreach our $x ($x) { }'
//! -e syntax OK
//!
//! $ perl -c -e 'use strict; for our $x (1) { } print $x;'
//! Variable "$x" is not imported ...
//!
//! $ perl -c -e 'use strict; use feature "state"; for state $x ($x) { }'
//! Global symbol "$x" requires explicit package name ...
//! ```
//!
//! The analyzer previously analyzed the list inside `loop_scope` *after*
//! declaring the loop variable, so a `my` self-reference produced no
//! `UndeclaredVariable`. A `my` in the list remains loop-scoped. Listing
//! `our` after the iterator would reintroduce that `my` hole; listing
//! `our` in the enclosing scope would leak it after the loop.

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

/// `use strict; for our $x ($x) { }` — `our` is a compile-time package alias.
/// perl 5.38.2: `-e syntax OK`. A list-before-iterator walk reports this
/// undeclared (the live false positive after the `my` reorder).
#[test]
fn foreach_our_list_sees_iterator_under_strict() {
    let code = "use strict; for our $x ($x) { }";
    let issues = scope_issues(code);
    assert!(
        !has_undeclared(&issues, "$x"),
        "`use strict; for our $x ($x) {{ }}` must not report UndeclaredVariable; got: {issues:?}"
    );
}

/// Same compile-time alias through the `foreach` keyword.
#[test]
fn foreach_keyword_our_list_sees_iterator_under_strict() {
    let code = "use strict; foreach our $x ($x) { }";
    let issues = scope_issues(code);
    assert!(
        !has_undeclared(&issues, "$x"),
        "`use strict; foreach our $x ($x) {{ }}` must not report UndeclaredVariable; got: {issues:?}"
    );
}

/// `for our $x (1) { } print $x;` — the `our` alias is still loop-scoped.
/// Declaring it in the enclosing scope would silence this, which perl rejects.
#[test]
fn foreach_our_does_not_leak_after_loop() {
    let code = "use strict; for our $x (1) { } print $x;";
    let issues = scope_issues(code);
    assert!(
        has_undeclared(&issues, "$x"),
        "`for our $x (1) {{ }} print $x;` must report UndeclaredVariable for $x; got: {issues:?}"
    );
}

/// `state` is a lexical like `my`: the list must not see the iterator.
#[test]
fn foreach_state_list_does_not_see_loop_variable_under_strict() {
    let code = "use strict; use feature \"state\"; for state $x ($x) { }";
    let issues = scope_issues(code);
    let list_x = code.find("($x)").map(|i| i + 1);
    assert!(
        issues.iter().any(|i| {
            i.kind == IssueKind::UndeclaredVariable
                && i.variable_name == "$x"
                && list_x.is_some_and(|offset| i.range.0 == offset)
        }),
        "`for state $x ($x)` under strict must report UndeclaredVariable for the list $x; got: {issues:?}"
    );
}

fn has_unused(issues: &[ScopeIssue], var_name: &str) -> bool {
    issues.iter().any(|i| i.kind == IssueKind::UnusedVariable && i.variable_name == var_name)
}

/// `my $x; for $x (my $x = 1) { print $x; }` — perl aliases the iterator to
/// the outer pad (`enteriter[$x:outer]`) even though the body reads the list
/// lexical. The outer binding is therefore used.
#[test]
fn foreach_bare_iterator_keeps_outer_binding_used() {
    let code = "use strict; my $x; for $x (my $x = 1) { print $x; }";
    let issues = scope_issues(code);
    assert!(
        !has_unused(&issues, "$x") && !has_undeclared(&issues, "$x"),
        "`my $x; for $x (my $x = 1)` must not report UnusedVariable or UndeclaredVariable for $x; got: {issues:?}"
    );
}

/// Same outer-iterator use through the `foreach` keyword.
#[test]
fn foreach_keyword_bare_iterator_keeps_outer_binding_used() {
    let code = "use strict; my $x; foreach $x (my $x = 1) { print $x; }";
    let issues = scope_issues(code);
    assert!(
        !has_unused(&issues, "$x") && !has_undeclared(&issues, "$x"),
        "`my $x; foreach $x (my $x = 1)` must not report UnusedVariable or UndeclaredVariable for $x; got: {issues:?}"
    );
}
