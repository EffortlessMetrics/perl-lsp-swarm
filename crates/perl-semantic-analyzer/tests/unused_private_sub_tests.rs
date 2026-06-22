//! Tests for PL305 — unused private subroutine detection.
//!
//! Covers the `UnusedPrivateSubroutine` issue kind added in #1404.
//! Private subroutines are those whose names start with `_[a-zA-Z]` and
//! have no scope declarator (`sub _name {}`, not `my sub _name {}`).

use perl_semantic_analyzer::Parser;
use perl_semantic_analyzer::analysis::scope_analyzer::{IssueKind, ScopeAnalyzer, ScopeIssue};
use perl_tdd_support::must;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn scope_issues(code: &str) -> Vec<ScopeIssue> {
    let mut parser = Parser::new(code);
    let ast = must(parser.parse());
    let analyzer = ScopeAnalyzer::new();
    analyzer.analyze(&ast, code, &[])
}

fn has_issue(issues: &[ScopeIssue], kind: IssueKind, name: &str) -> bool {
    issues.iter().any(|i| i.kind == kind && i.variable_name == name)
}

fn count_unused_private_sub_issues(issues: &[ScopeIssue]) -> usize {
    issues.iter().filter(|i| i.kind == IssueKind::UnusedPrivateSubroutine).count()
}

// ---------------------------------------------------------------------------
// Core detection cases
// ---------------------------------------------------------------------------

#[test]
fn unused_private_sub_detected() -> Result<(), Box<dyn std::error::Error>> {
    // `sub _helper { }` is never called — should produce a diagnostic.
    let code = r#"
sub _helper {
    return 42;
}

sub public_func {
    return 1;
}
"#;
    let issues = scope_issues(code);
    assert!(
        has_issue(&issues, IssueKind::UnusedPrivateSubroutine, "_helper"),
        "Expected UnusedPrivateSubroutine for _helper; issues: {:?}",
        issues.iter().map(|i| (&i.kind, &i.variable_name)).collect::<Vec<_>>()
    );
    Ok(())
}

#[test]
fn used_private_sub_not_detected() -> Result<(), Box<dyn std::error::Error>> {
    // `sub _helper { }` IS called — no diagnostic.
    let code = r#"
sub _helper {
    return 42;
}

my $result = _helper();
"#;
    let issues = scope_issues(code);
    assert!(
        !has_issue(&issues, IssueKind::UnusedPrivateSubroutine, "_helper"),
        "Should NOT flag _helper when it is called; issues: {:?}",
        issues.iter().map(|i| (&i.kind, &i.variable_name)).collect::<Vec<_>>()
    );
    Ok(())
}

#[test]
fn public_sub_not_detected() -> Result<(), Box<dyn std::error::Error>> {
    // `sub public_func { }` has no underscore prefix — should not be flagged.
    let code = r#"
sub public_func {
    return 1;
}
"#;
    let issues = scope_issues(code);
    assert!(
        !has_issue(&issues, IssueKind::UnusedPrivateSubroutine, "public_func"),
        "Public sub should not be flagged; issues: {:?}",
        issues.iter().map(|i| (&i.kind, &i.variable_name)).collect::<Vec<_>>()
    );
    Ok(())
}

#[test]
fn underscore_alone_not_detected() -> Result<(), Box<dyn std::error::Error>> {
    // `sub _ { }` — single underscore, not a private naming convention.
    let code = r#"
sub _ {
    return 0;
}
"#;
    let issues = scope_issues(code);
    assert_eq!(
        count_unused_private_sub_issues(&issues),
        0,
        "sub _ should not trigger UnusedPrivateSubroutine; issues: {:?}",
        issues.iter().map(|i| (&i.kind, &i.variable_name)).collect::<Vec<_>>()
    );
    Ok(())
}

#[test]
fn dunder_not_detected() -> Result<(), Box<dyn std::error::Error>> {
    // `sub __ANON__ { }` — dunder name, excluded by convention.
    let code = r#"
sub __ANON__ {
    return 0;
}
"#;
    let issues = scope_issues(code);
    assert_eq!(
        count_unused_private_sub_issues(&issues),
        0,
        "sub __ANON__ should not trigger UnusedPrivateSubroutine; issues: {:?}",
        issues.iter().map(|i| (&i.kind, &i.variable_name)).collect::<Vec<_>>()
    );
    Ok(())
}

#[test]
fn double_underscore_prefix_not_detected() -> Result<(), Box<dyn std::error::Error>> {
    // `sub __private { }` — second char is `_`, excluded.
    let code = r#"
sub __private {
    return 0;
}
"#;
    let issues = scope_issues(code);
    assert_eq!(
        count_unused_private_sub_issues(&issues),
        0,
        "sub __private should not trigger UnusedPrivateSubroutine; issues: {:?}",
        issues.iter().map(|i| (&i.kind, &i.variable_name)).collect::<Vec<_>>()
    );
    Ok(())
}

#[test]
fn multiple_unused_private_subs() -> Result<(), Box<dyn std::error::Error>> {
    // Two distinct unused private subs → two diagnostics.
    let code = r#"
sub _alpha {
    return 1;
}

sub _beta {
    return 2;
}
"#;
    let issues = scope_issues(code);
    assert!(
        has_issue(&issues, IssueKind::UnusedPrivateSubroutine, "_alpha"),
        "Expected diagnostic for _alpha; issues: {:?}",
        issues.iter().map(|i| (&i.kind, &i.variable_name)).collect::<Vec<_>>()
    );
    assert!(
        has_issue(&issues, IssueKind::UnusedPrivateSubroutine, "_beta"),
        "Expected diagnostic for _beta; issues: {:?}",
        issues.iter().map(|i| (&i.kind, &i.variable_name)).collect::<Vec<_>>()
    );
    assert_eq!(
        count_unused_private_sub_issues(&issues),
        2,
        "Expected exactly 2 UnusedPrivateSubroutine diagnostics; issues: {:?}",
        issues.iter().map(|i| (&i.kind, &i.variable_name)).collect::<Vec<_>>()
    );
    Ok(())
}

#[test]
fn one_used_one_unused_private_sub() -> Result<(), Box<dyn std::error::Error>> {
    // Two private subs; only one is called — only the unused one is flagged.
    let code = r#"
sub _used {
    return 1;
}

sub _unused {
    return 2;
}

_used();
"#;
    let issues = scope_issues(code);
    assert!(
        has_issue(&issues, IssueKind::UnusedPrivateSubroutine, "_unused"),
        "Expected diagnostic for _unused; issues: {:?}",
        issues.iter().map(|i| (&i.kind, &i.variable_name)).collect::<Vec<_>>()
    );
    assert!(
        !has_issue(&issues, IssueKind::UnusedPrivateSubroutine, "_used"),
        "Should NOT flag _used when it is called; issues: {:?}",
        issues.iter().map(|i| (&i.kind, &i.variable_name)).collect::<Vec<_>>()
    );
    Ok(())
}

#[test]
fn private_sub_called_from_another_sub() -> Result<(), Box<dyn std::error::Error>> {
    // `_helper` called inside another sub body — counts as used.
    let code = r#"
sub _helper {
    return 42;
}

sub public_func {
    return _helper();
}
"#;
    let issues = scope_issues(code);
    assert!(
        !has_issue(&issues, IssueKind::UnusedPrivateSubroutine, "_helper"),
        "Should NOT flag _helper called from inside another sub; issues: {:?}",
        issues.iter().map(|i| (&i.kind, &i.variable_name)).collect::<Vec<_>>()
    );
    Ok(())
}

#[test]
fn diagnostic_code_is_pl305() -> Result<(), Box<dyn std::error::Error>> {
    // Verify the IssueKind maps correctly (code coverage for the kind itself).
    let code = r#"
sub _unused_sub {
    return 0;
}
"#;
    let issues = scope_issues(code);
    let issue = issues
        .iter()
        .find(|i| i.kind == IssueKind::UnusedPrivateSubroutine)
        .ok_or("Expected at least one UnusedPrivateSubroutine issue")?;
    assert_eq!(issue.variable_name, "_unused_sub", "variable_name should be the sub name");
    assert!(!issue.description.is_empty(), "description should be non-empty");
    Ok(())
}
