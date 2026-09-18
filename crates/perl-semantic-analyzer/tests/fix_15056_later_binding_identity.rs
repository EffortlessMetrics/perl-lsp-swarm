#![deny(clippy::map_err_ignore)]
//! #15056 — preserve later same-scope lexical binding identity.
//!
//! When `my $x` is redeclared in the same scope (e.g. `my $x; my $x = 2;`),
//! the ordinary visible-variable map must continue to reflect the *latest*
//! declaration so subsequent reads resolve to the new binding. Previously,
//! `declare_variable_parts` emitted `VariableRedeclaration` and returned
//! before installing the later declaration, so a later `print $x` resolved
//! against the earlier (uninitialized) binding.
//!
//! This file is the failing proof for the repair; the matching fix lives
//! in `src/analysis/scope_analyzer/mod.rs`.
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

/// Control: redeclaration must still emit `VariableRedeclaration` (#1661).
#[test]
fn redeclaration_is_still_reported() -> TestResult {
    let issues = analyze("use strict;\nmy $x;\nmy $x = 2;\nprint $x;\n")?;
    assert!(
        has_issue(&issues, IssueKind::VariableRedeclaration, "$x"),
        "redeclaration must still be reported; got: {:?}",
        issues
    );
    Ok(())
}

/// #15056 — `my $x; my $x = 2; print $x;` must resolve `$x` to the second
/// (initialized) declaration, not the first. Previously this incorrectly
/// reported `UninitializedVariable` because the first binding was still the
/// active slot.
#[test]
fn later_initialized_binding_wins_lookup() -> TestResult {
    let issues = analyze("use strict;\nmy $x;\nmy $x = 2;\nprint $x;\n")?;
    assert!(
        !has_issue(&issues, IssueKind::UninitializedVariable, "$x"),
        "lookup must resolve to the later initialized binding; got: {:?}",
        issues
    );
    Ok(())
}

/// #15056 — symmetric ordering: `my $x = 2; my $x; print $x;` must report
/// `UninitializedVariable` (and `VariableRedeclaration`) because the second
/// `my $x` is uninitialized and becomes the latest binding.
#[test]
fn later_uninitialized_binding_wins_lookup() -> TestResult {
    let issues = analyze("use strict;\nmy $x = 2;\nmy $x;\nprint $x;\n")?;
    assert!(
        has_issue(&issues, IssueKind::VariableRedeclaration, "$x"),
        "redeclaration must still be reported; got: {:?}",
        issues
    );
    assert!(
        has_issue(&issues, IssueKind::UninitializedVariable, "$x"),
        "lookup must resolve to the later uninitialized binding; got: {:?}",
        issues
    );
    Ok(())
}

/// #15056 — earlier reference before redeclaration still resolves to the
/// earlier binding (and is therefore uninitialized). After the redeclaration
/// the lookup switches to the later binding.
#[test]
fn earlier_reference_uses_earlier_binding_later_uses_later() -> TestResult {
    let issues = analyze("use strict;\nmy $x;\nprint $x;\nmy $x = 2;\nprint $x;\n")?;
    let uninit: Vec<_> = issues
        .iter()
        .filter(|i| i.kind == IssueKind::UninitializedVariable && i.variable_name == "$x")
        .collect();
    assert_eq!(
        uninit.len(),
        1,
        "exactly one uninitialized read (the pre-redeclaration one); got: {:?}",
        issues
    );
    let redecls: Vec<_> = issues
        .iter()
        .filter(|i| i.kind == IssueKind::VariableRedeclaration && i.variable_name == "$x")
        .collect();
    assert_eq!(redecls.len(), 1, "exactly one redeclaration; got: {:?}", issues);
    Ok(())
}

/// #15056 — nested shadowing: outer `my $x` is preserved, inner `my $x`
/// shadows it (existing semantics). Inside the inner block, lookups hit the
/// inner binding; outside, they hit the outer.
#[test]
fn nested_shadowing_unaffected() -> TestResult {
    let issues = analyze("use strict;\nmy $x = 1;\n{ my $x = 2; print $x; }\nprint $x;\n")?;
    assert!(
        !has_issue(&issues, IssueKind::UndeclaredVariable, "$x"),
        "nested shadowing should remain clean; got: {:?}",
        issues
    );
    Ok(())
}

/// #15056 — `state` redeclaration stays initialized by definition, but the
/// latest binding is still what survives a subsequent read; this is a
/// regression pin for the broader fix.
#[test]
fn state_redeclaration_latest_wins() -> TestResult {
    let issues = analyze("use strict;\nstate $x;\nstate $x = 2;\nprint $x;\n")?;
    assert!(
        !has_issue(&issues, IssueKind::UninitializedVariable, "$x"),
        "state redeclaration lookup must resolve to the later binding; got: {:?}",
        issues
    );
    Ok(())
}

/// #15056 — false-green mutant control: a first-binding-wins mutant must
/// fail this test. If the visible-variable map is reverted to keep the
/// first binding on redeclaration, this test reports no
/// `UninitializedVariable` for the post-redeclaration read.
#[test]
fn first_binding_wins_mutant_fails() -> TestResult {
    let issues = analyze("use strict;\nmy $x = 2;\nmy $x;\nprint $x;\n")?;
    assert!(
        has_issue(&issues, IssueKind::UninitializedVariable, "$x"),
        "first-binding-wins mutant would make this read resolve to the uninitialized redeclaration; got: {:?}",
        issues
    );
    Ok(())
}
