//! RIPR seam proofs for package-qualified strict-subs validation (#3014).
//!
//! Covers `check_qualified_call` and `collect_defined_packages` introduced to
//! emit `UnresolvedQualifiedCall` when a sub is missing from an in-file package.

use perl_semantic_analyzer::Parser;
use perl_semantic_analyzer::analysis::scope_analyzer::{IssueKind, ScopeAnalyzer, ScopeIssue};
use perl_semantic_analyzer::pragma_tracker::PragmaTracker;
use perl_tdd_support::must;

fn scope_issues_strict(code: &str) -> Vec<ScopeIssue> {
    let mut parser = Parser::new(code);
    let ast = must(parser.parse());
    let pragma_map = PragmaTracker::build(&ast);
    ScopeAnalyzer::new().analyze(&ast, code, &pragma_map)
}

fn has_unresolved_qualified(issues: &[ScopeIssue], name: &str) -> bool {
    issues.iter().any(|issue| {
        issue.kind == IssueKind::UnresolvedQualifiedCall && issue.variable_name == name
    })
}

/// Missing `Foo::baz` in a declared package must surface PL305's issue kind.
#[test]
fn seam_qualified_call_flags_missing_sub_in_defined_package() {
    let code = r#"
use strict 'subs';
package Foo;
sub bar { 1 }
package main;
Foo::bar();
Foo::baz();
"#;
    let issues = scope_issues_strict(code);
    assert!(
        !has_unresolved_qualified(&issues, "Foo::bar"),
        "defined Foo::bar must not be flagged; got: {issues:?}"
    );
    assert!(
        has_unresolved_qualified(&issues, "Foo::baz"),
        "missing Foo::baz in declared package must be flagged; got: {issues:?}"
    );
}

/// External packages must stay opaque to single-file analysis.
#[test]
fn seam_qualified_call_suppresses_external_package() {
    let code = r#"
use strict 'subs';
use Some::External::Module;
Some::External::Module::helper();
"#;
    let issues = scope_issues_strict(code);
    assert!(
        !issues
            .iter()
            .any(|issue| issue.kind == IssueKind::UnresolvedQualifiedCall),
        "external package calls must not be flagged; got: {issues:?}"
    );
}

/// Explicit `sub Foo::bar {}` from another package still suppresses the diagnostic.
#[test]
fn seam_qualified_call_explicit_qualified_sub_definition_suppresses() {
    let code = r#"
use strict 'subs';
package main;
sub Foo::bar { 1 }
Foo::bar();
"#;
    let issues = scope_issues_strict(code);
    assert!(
        !has_unresolved_qualified(&issues, "Foo::bar"),
        "explicit sub Foo::bar must suppress the diagnostic; got: {issues:?}"
    );
}

/// Leading `::` absolute names remain skipped to avoid main-resolution false positives.
#[test]
fn seam_qualified_call_skips_leading_colon_colon_names() {
    let code = r#"
use strict 'subs';
package Foo;
sub bar { 1 }
package main;
::Foo::missing();
"#;
    let issues = scope_issues_strict(code);
    assert!(
        !has_unresolved_qualified(&issues, "::Foo::missing"),
        "leading :: calls are intentionally skipped; got: {issues:?}"
    );
}
