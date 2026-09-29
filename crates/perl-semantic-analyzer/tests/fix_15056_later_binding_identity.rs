#![deny(clippy::map_err_ignore)]
//! #15056 — preserve later same-scope lexical binding identity.
//!
//! When `my $x` is redeclared in the same scope, subsequent lookup, unused
//! reporting, and initialization must use the *later* declaration. Perl
//! compile-only checking accepts both orderings (`perl -c` reports only the
//! "masks earlier declaration" warning). Runtime definite-initialization is
//! not the oracle: `my $x; my $x = 2; print $x;` prints `2`, and
//! `my $x = 2; my $x; print $x;` warns about an uninitialized value.
//!
//! A first-binding-wins mutant (redeclaration diagnostic, no slot replace)
//! fails the unused-offset and later-uninitialized rows. Statement-modifier
//! visibility stays with #1772 / #14840 and is pinned, not reworked.
//!
//! Oracle: Perl 5.38.2 `perl -c` / `perl -we` as recorded next to each row.

use perl_semantic_analyzer::Parser;
use perl_semantic_analyzer::analysis::scope_analyzer::{IssueKind, ScopeAnalyzer, ScopeIssue};
use perl_semantic_analyzer::pragma_tracker::PragmaTracker;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn analyze(code: &str) -> Result<Vec<ScopeIssue>, Box<dyn std::error::Error>> {
    let mut parser = Parser::new(code);
    let ast = parser.parse()?;
    let pragma_map = PragmaTracker::build(&ast);
    Ok(ScopeAnalyzer::new().analyze(&ast, code, &pragma_map))
}

fn has_issue(issues: &[ScopeIssue], kind: IssueKind, var_name: &str) -> bool {
    issues.iter().any(|i| i.kind == kind && i.variable_name == var_name)
}

fn issues_of<'a>(issues: &'a [ScopeIssue], kind: IssueKind, var_name: &str) -> Vec<&'a ScopeIssue> {
    issues.iter().filter(|i| i.kind == kind && i.variable_name == var_name).collect()
}

fn last_offset(code: &str, needle: &str) -> Result<usize, Box<dyn std::error::Error>> {
    code.rfind(needle).ok_or_else(|| format!("needle {needle:?} missing from {code:?}").into())
}

/// Control: redeclaration must still emit `VariableRedeclaration` (#1661).
///
/// Oracle: `perl -c -e 'use strict; use warnings; my $x; my $x=2; print $x;'`
/// → `"my" variable $x masks earlier declaration in same scope` (syntax OK).
#[test]
fn redeclaration_is_still_reported() -> TestResult {
    let issues = analyze("use strict;\nmy $x;\nmy $x = 2;\nprint $x;\n")?;
    assert!(
        has_issue(&issues, IssueKind::VariableRedeclaration, "$x"),
        "redeclaration must still be reported; got: {issues:?}"
    );
    Ok(())
}

/// `my $x; my $x = 2; print $x;` must resolve `$x` to the second (initialized)
/// declaration. A first-binding-wins slot would flag `UninitializedVariable`.
///
/// Oracle: `perl -we` prints `2` after the mask warning; compile-only is syntax OK.
#[test]
fn later_initialized_binding_wins_lookup() -> TestResult {
    let issues = analyze("use strict;\nmy $x;\nmy $x = 2;\nprint $x;\n")?;
    assert!(
        !has_issue(&issues, IssueKind::UninitializedVariable, "$x"),
        "lookup must resolve to the later initialized binding; got: {issues:?}"
    );
    Ok(())
}

/// Symmetric ordering: `my $x = 2; my $x; print $x;` must report
/// `UninitializedVariable` because the second `my $x` is uninitialized.
///
/// Oracle: `perl -we` → `Use of uninitialized value $x in print`.
#[test]
fn later_uninitialized_binding_wins_lookup() -> TestResult {
    let issues = analyze("use strict;\nmy $x = 2;\nmy $x;\nprint $x;\n")?;
    assert!(
        has_issue(&issues, IssueKind::VariableRedeclaration, "$x"),
        "redeclaration must still be reported; got: {issues:?}"
    );
    assert!(
        has_issue(&issues, IssueKind::UninitializedVariable, "$x"),
        "lookup must resolve to the later uninitialized binding; got: {issues:?}"
    );
    Ok(())
}

/// A read before redeclaration still uses the earlier binding. After the
/// redeclaration, lookup switches to the later binding.
#[test]
fn earlier_reference_uses_earlier_binding_later_uses_later() -> TestResult {
    let issues = analyze("use strict;\nmy $x;\nprint $x;\nmy $x = 2;\nprint $x;\n")?;
    let uninit = issues_of(&issues, IssueKind::UninitializedVariable, "$x");
    assert_eq!(
        uninit.len(),
        1,
        "exactly one uninitialized read (the pre-redeclaration one); got: {issues:?}"
    );
    let redecls = issues_of(&issues, IssueKind::VariableRedeclaration, "$x");
    assert_eq!(redecls.len(), 1, "exactly one redeclaration; got: {issues:?}");
    Ok(())
}

/// Nested shadowing is a different seam: inner `my $x` shadows outer, it does
/// not redeclare. Both prints are initialized.
#[test]
fn nested_shadowing_unaffected() -> TestResult {
    let issues = analyze("use strict;\nmy $x = 1;\n{ my $x = 2; print $x; }\nprint $x;\n")?;
    assert!(
        !has_issue(&issues, IssueKind::UndeclaredVariable, "$x"),
        "nested shadowing should remain clean; got: {issues:?}"
    );
    assert!(
        !has_issue(&issues, IssueKind::UninitializedVariable, "$x"),
        "both nested and outer reads are initialized; got: {issues:?}"
    );
    Ok(())
}

/// Inner same-scope redeclaration must not disturb the outer binding: the inner
/// later slot is initialized, the outer print stays initialized, and shadowing
/// plus redeclaration both fire.
///
/// Oracle: `perl -we 'use strict; my $x=1; { my $x; my $x=2; print $x; } print defined($x)?1:0;'`
/// → mask warning, prints `20`.
#[test]
fn nested_shadowing_plus_inner_redeclaration() -> TestResult {
    let issues = analyze("use strict;\nmy $x = 1;\n{ my $x; my $x = 2; print $x; }\nprint $x;\n")?;
    assert!(
        has_issue(&issues, IssueKind::VariableShadowing, "$x"),
        "inner first declaration must still shadow outer; got: {issues:?}"
    );
    assert!(
        has_issue(&issues, IssueKind::VariableRedeclaration, "$x"),
        "inner second declaration must still redeclare; got: {issues:?}"
    );
    assert!(
        !has_issue(&issues, IssueKind::UninitializedVariable, "$x"),
        "inner later binding is initialized and outer stays initialized; got: {issues:?}"
    );
    Ok(())
}

/// `state` is initialized by definition; the latest binding still owns the read.
#[test]
fn state_redeclaration_latest_wins() -> TestResult {
    let issues = analyze("use strict;\nstate $x;\nstate $x = 2;\nprint $x;\n")?;
    assert!(
        !has_issue(&issues, IssueKind::UninitializedVariable, "$x"),
        "state redeclaration lookup must resolve to the later binding; got: {issues:?}"
    );
    Ok(())
}

/// Unused reporting must name the *later* declaration. First-binding-wins keeps
/// the first slot, so `UnusedVariable.range` would point at the first `my $x`
/// (or vanish entirely if that slot was already used).
///
/// `my $x = 1; print $x; my $x;` — first slot is used; only replacing it with
/// the unused later binding produces `UnusedVariable` at the second `my $x`.
#[test]
fn unused_diagnostic_tracks_later_binding() -> TestResult {
    let code = "use strict;\nmy $x = 1;\nprint $x;\nmy $x;\n";
    let issues = analyze(code)?;
    let unused = issues_of(&issues, IssueKind::UnusedVariable, "$x");
    assert_eq!(
        unused.len(),
        1,
        "later unused binding must be reported; a first-binding-wins mutant keeps the used first slot; got: {issues:?}"
    );
    let later_decl = last_offset(code, "$x;")?;
    let unused_issue = unused.first().ok_or("missing unused issue")?;
    assert_eq!(
        unused_issue.range.0, later_decl,
        "unused diagnostic must point at the later declaration ({later_decl}), not the used first slot; got: {issues:?}"
    );
    Ok(())
}

/// Opposite unused-offset control: both declarations unused, so first-wins still
/// reports unused — but at the *first* offset. Later-wins must point at the second.
#[test]
fn unused_offset_is_later_declaration_when_neither_is_read() -> TestResult {
    let code = "use strict;\nmy $x;\nmy $x = 2;\n";
    let issues = analyze(code)?;
    let unused = issues_of(&issues, IssueKind::UnusedVariable, "$x");
    assert_eq!(unused.len(), 1, "exactly one unused report (latest only); got: {issues:?}");
    let later_decl = last_offset(code, "$x")?;
    let unused_issue = unused.first().ok_or("missing unused issue")?;
    assert_eq!(
        unused_issue.range.0, later_decl,
        "unused must be the later declaration, not the masked first; got: {issues:?}"
    );
    Ok(())
}

/// Triple redeclaration: the last uninitialized binding owns the subsequent read.
/// Two `VariableRedeclaration` diagnostics, one uninitialized use.
#[test]
fn triple_redeclaration_latest_uninitialized_wins() -> TestResult {
    let issues = analyze("use strict;\nmy $x;\nmy $x = 1;\nmy $x;\nprint $x;\n")?;
    let redecls = issues_of(&issues, IssueKind::VariableRedeclaration, "$x");
    assert_eq!(redecls.len(), 2, "second and third declarations redeclare; got: {issues:?}");
    assert!(
        has_issue(&issues, IssueKind::UninitializedVariable, "$x"),
        "the last uninitialized binding must own the read; got: {issues:?}"
    );
    Ok(())
}

/// Assignment after both declarations initializes only the latest slot.
#[test]
fn assignment_after_redeclaration_initializes_later_binding() -> TestResult {
    let issues = analyze("use strict;\nmy $x;\nmy $x;\n$x = 1;\nprint $x;\n")?;
    assert!(
        has_issue(&issues, IssueKind::VariableRedeclaration, "$x"),
        "redeclaration must still be reported; got: {issues:?}"
    );
    assert!(
        !has_issue(&issues, IssueKind::UninitializedVariable, "$x"),
        "assignment must initialize the later binding; got: {issues:?}"
    );
    Ok(())
}

/// Array/hash later-wins: same identity rule, different sigil slots.
///
/// Oracle: `perl -we 'use strict; my @x; my @x=(1,2); print @x;'` → mask warning, prints `12`.
#[test]
fn array_and_hash_later_bindings_win() -> TestResult {
    let array_issues = analyze("use strict;\nmy @x;\nmy @x = (1, 2);\nprint @x;\n")?;
    assert!(
        has_issue(&array_issues, IssueKind::VariableRedeclaration, "@x"),
        "array redeclaration must be reported; got: {array_issues:?}"
    );
    assert!(
        !has_issue(&array_issues, IssueKind::UninitializedVariable, "@x"),
        "later array initializer must win; got: {array_issues:?}"
    );

    let hash_issues = analyze("use strict;\nmy %h;\nmy %h = (a => 1);\nprint %h;\n")?;
    assert!(
        has_issue(&hash_issues, IssueKind::VariableRedeclaration, "%h"),
        "hash redeclaration must be reported; got: {hash_issues:?}"
    );
    assert!(
        !has_issue(&hash_issues, IssueKind::UninitializedVariable, "%h"),
        "later hash initializer must win; got: {hash_issues:?}"
    );
    Ok(())
}

/// Opposite array ordering: later uninitialized array owns the read.
#[test]
fn later_uninitialized_array_wins_lookup() -> TestResult {
    let issues = analyze("use strict;\nmy @x = (1);\nmy @x;\nprint @x;\n")?;
    assert!(
        has_issue(&issues, IssueKind::UninitializedVariable, "@x"),
        "later uninitialized array must own the read; got: {issues:?}"
    );
    Ok(())
}

/// Non-goal control: ordinary later-wins must not make a modifier declaration
/// visible to its statement. #1772 / #14840 own that seam.
///
/// Oracle: `perl -c -e 'use strict; print $x if my $x = 1;'`
/// → `Global symbol "$x" requires explicit package name`.
#[test]
fn modifier_declaration_stays_invisible_to_its_statement() -> TestResult {
    let issues = analyze("use strict;\nprint $x if my $x = 1;\n")?;
    assert!(
        has_issue(&issues, IssueKind::UndeclaredVariable, "$x"),
        "statement-modifier visibility must remain undeclared; got: {issues:?}"
    );
    Ok(())
}
