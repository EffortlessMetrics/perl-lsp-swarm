#![deny(clippy::map_err_ignore)]
// Cohort C1 activation (#12598): all production rows exact-excepted; new findings move the crate back to non-C1.
//! #1772 — lexical visibility is decided by declaration order, not by AST traversal order.
//!
//! Perl's `my`/`state` binding takes effect only *after* its declaration.  The scope analyzer
//! carries that rule implicitly: its lookup is existence-based, so a declaration is visible to
//! a use exactly when the traversal reached the declaration first.  Source-ordered traversal
//! therefore gives the right answer — and any arm that reorders children silently breaks it.
//!
//! The `StatementModifier` arm did exactly that, deliberately, on the false premise that Perl
//! hoists a modifier's `my` over the statement it modifies.  It does not.  These rows pin the
//! behaviour so the traversal order cannot drift back.
//!
//! Every expectation below is anchored to perl 5.38.2 as the external oracle, recorded as
//! `perl -c` transcripts.  Cases Perl accepts must stay clean; cases Perl rejects with
//! `Global symbol "..." requires explicit package name` must produce `UndeclaredVariable`.

use perl_semantic_analyzer::Parser;
use perl_semantic_analyzer::analysis::scope_analyzer::{IssueKind, ScopeAnalyzer, ScopeIssue};
use perl_semantic_analyzer::pragma_tracker::PragmaTracker;
use perl_tdd_support::must;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn scope_issues(code: &str) -> Vec<ScopeIssue> {
    let mut parser = Parser::new(code);
    let ast = must(parser.parse());
    let pragma_map = PragmaTracker::build(&ast);
    ScopeAnalyzer::new().analyze(&ast, code, &pragma_map)
}

fn has_undeclared(issues: &[ScopeIssue], var_name: &str) -> bool {
    issues.iter().any(|i| i.kind == IssueKind::UndeclaredVariable && i.variable_name == var_name)
}

/// The regression this suite exists for: a use that Perl rejects must be reported.
fn assert_forward_use_reported(code: &str, var_name: &str) {
    let issues = scope_issues(code);
    assert!(
        has_undeclared(&issues, var_name),
        "real Perl rejects this program with `Global symbol {var_name} requires explicit \
         package name`, so the analyzer must report {var_name} undeclared.\ncode: {code:?}\n\
         issues: {issues:?}"
    );
}

/// The false-positive guard: a use that Perl accepts must stay clean.
fn assert_accepted(code: &str, var_name: &str) {
    let issues = scope_issues(code);
    assert!(
        !has_undeclared(&issues, var_name),
        "real Perl compiles this program (`-e syntax OK`), so {var_name} must not be reported \
         undeclared.\ncode: {code:?}\nissues: {issues:?}"
    );
}

// ---------------------------------------------------------------------------
// The construct this PR actually repairs: statement modifiers
// ---------------------------------------------------------------------------

/// Oracle, perl 5.38.2:
/// ```text
/// $ perl -c -e 'use strict; print $x if my $x = 1;'
/// Global symbol "$x" requires explicit package name (did you forget to declare "my $x"?)
/// ```
///
/// The analyzer previously reordered the `StatementModifier` children so the condition was
/// analyzed first, making the declaration visible to the statement and silencing this.
#[test]
fn statement_modifier_declaration_is_not_visible_to_its_statement() {
    assert_forward_use_reported("use strict;\nprint $x if my $x = 1;\n", "$x");
}

/// The same for `unless`/`until`, so the repair is not keyed to one modifier keyword.
#[test]
fn every_conditional_modifier_keeps_declaration_order() {
    assert_forward_use_reported(
        "use strict;\nsub compute {}\ndie $err unless my $err = compute();\n",
        "$err",
    );
    assert_forward_use_reported("use strict;\nprint $x until my $x = 0;\n", "$x");
}

/// Negative control — without a forward use, a modifier declaration is perfectly legal.
///
/// Oracle: `foo() while my $x = bar();` → `-e syntax OK`.
///
/// Without this row, blanket-flagging every statement-modifier `my` would pass the rows above.
#[test]
fn statement_modifier_declaration_alone_stays_clean() {
    assert_accepted("use strict;\nsub foo {}\nsub bar {}\nfoo() while my $x = bar();\n", "$x");
}

// ---------------------------------------------------------------------------
// Forward use in ordinary scopes — pinned so traversal changes cannot regress them
// ---------------------------------------------------------------------------

/// All four patterns named in #1772.  Oracle: each is
/// `Global symbol "..." requires explicit package name` under `use strict`.
#[test]
fn forward_use_is_reported_in_every_scope_kind() {
    assert_forward_use_reported("use strict;\nprint $x;\nmy $x = 1;\n", "$x");
    assert_forward_use_reported("use strict;\n{ print $x; my $x = 1; }\n", "$x");
    assert_forward_use_reported("use strict;\nsub f { print $x; my $x = 1; }\n", "$x");
    assert_forward_use_reported("use strict;\nif (1) { print $c; my $c = 5; }\n", "$c");
}

/// A closure that captures a name declared later in the enclosing scope.
///
/// Oracle: `use strict; my $f = sub { print $x; }; my $x = 1;` →
/// `Global symbol "$x" requires explicit package name`.
#[test]
fn closure_cannot_capture_a_later_declaration() {
    assert_forward_use_reported("use strict;\nmy $f = sub { print $x; };\nmy $x = 1;\n", "$x");
}

/// `my $x = $x;` — the initializer's `$x` is the *outer* one, which does not exist here.
///
/// Oracle: `use strict; my $x = $x;` → `Global symbol "$x" requires explicit package name`.
/// This depends on the initializer being analyzed before the declaration is recorded
/// (`declarations.rs`), which is the same source-order rule these rows guard.
#[test]
fn self_initialization_does_not_see_its_own_binding() {
    assert_forward_use_reported("use strict;\nmy $x = $x;\n", "$x");
}

// ---------------------------------------------------------------------------
// Negative controls — the shapes an over-eager ordering rule would break
// ---------------------------------------------------------------------------

/// The load-bearing shadowing case: a use before an inner declaration resolves to the
/// *enclosing* binding rather than failing.  This is why an invisible declaration is skipped
/// to the parent scope instead of being reported as absent.
///
/// Oracle: `use strict; my $x = 1; { print $x; my $x = 2; print $x; }` → `-e syntax OK`.
#[test]
fn use_before_an_inner_declaration_resolves_to_the_outer_binding() {
    assert_accepted("use strict;\nmy $x = 1;\n{ print $x; my $x = 2; print $x; }\n", "$x");
}

/// A sub body may reference a binding declared earlier at file scope.
///
/// Oracle: `use strict; my $x = 1; sub f { print $x; }` → `-e syntax OK`.
#[test]
fn sub_body_sees_an_earlier_file_scope_binding() {
    assert_accepted("use strict;\nmy $x = 1;\nsub f { print $x; }\n", "$x");
}

/// Ordinary declare-then-use, loops, and the non-modifier `while (my $l = ...)` form — all
/// accepted by Perl and all must stay clean.
#[test]
fn ordinary_declaration_before_use_stays_clean() {
    assert_accepted("use strict;\nmy $x = 1;\nprint $x;\n", "$x");
    assert_accepted("use strict;\nmy @l = (1, 2);\nfor my $i (@l) { print $i; }\n", "$i");
    assert_accepted("use strict;\nsub f {}\nwhile (my $l = f()) { print $l; }\n", "$l");
}

/// Without `use strict` the name resolves as a package global, so forward use is legal and
/// must not be diagnosed.  Oracle: `print $x; my $x = 1;` → `-e syntax OK` (exit 0).
///
/// This is the control that keeps the repair strict-gated rather than universal.
#[test]
fn forward_use_without_strict_is_not_reported() {
    assert_accepted("print $x;\nmy $x = 1;\n", "$x");
    assert_accepted("{ print $x; my $x = 1; }\n", "$x");
    assert_accepted("print $x if my $x = 1;\n", "$x");
}
