//! Regression coverage for declaration-order-aware lexical lookup (#1772).

use perl_semantic_analyzer::Parser;
use perl_semantic_analyzer::analysis::scope_analyzer::{IssueKind, ScopeAnalyzer, ScopeIssue};
use perl_semantic_analyzer::pragma_tracker::PragmaTracker;
use std::error::Error;

fn scope_issues(code: &str) -> Result<Vec<ScopeIssue>, Box<dyn Error>> {
    let mut parser = Parser::new(code);
    let ast = parser.parse()?;
    let pragma_map = PragmaTracker::build(&ast);
    Ok(ScopeAnalyzer::new().analyze(&ast, code, &pragma_map))
}

fn has_undeclared(issues: &[ScopeIssue], name: &str) -> bool {
    issues
        .iter()
        .any(|issue| issue.kind == IssueKind::UndeclaredVariable && issue.variable_name == name)
}

fn require_undeclared(issues: &[ScopeIssue], name: &str) -> Result<(), Box<dyn Error>> {
    if has_undeclared(issues, name) {
        Ok(())
    } else {
        Err(format!("expected UndeclaredVariable for {name}; got {issues:?}").into())
    }
}

fn require_no_undeclared(issues: &[ScopeIssue], name: &str) -> Result<(), Box<dyn Error>> {
    if has_undeclared(issues, name) {
        Err(format!("unexpected UndeclaredVariable for {name}; got {issues:?}").into())
    } else {
        Ok(())
    }
}

#[test]
fn strict_forward_use_at_file_scope_is_not_hidden_by_later_declaration()
-> Result<(), Box<dyn Error>> {
    let issues = scope_issues("use strict; print $x; my $x = 1;\n")?;
    require_undeclared(&issues, "$x")
}

#[test]
fn strict_forward_use_in_block_is_reported() -> Result<(), Box<dyn Error>> {
    let issues = scope_issues("use strict; { print $x; my $x = 1; }\n")?;
    require_undeclared(&issues, "$x")
}

#[test]
fn strict_forward_use_in_subroutine_is_reported() -> Result<(), Box<dyn Error>> {
    let issues = scope_issues("use strict; sub f { print $x; my $x = 1; }\n")?;
    require_undeclared(&issues, "$x")
}

#[test]
fn strict_forward_use_in_closure_is_reported() -> Result<(), Box<dyn Error>> {
    let issues = scope_issues("use strict; my $f = sub { print $x; my $x = 1; };\n")?;
    require_undeclared(&issues, "$x")
}

#[test]
fn forward_use_without_strict_resolves_as_package_global() -> Result<(), Box<dyn Error>> {
    let issues = scope_issues("print $x; my $x = 1;\n")?;
    require_no_undeclared(&issues, "$x")
}

#[test]
fn strict_closure_capture_of_prior_outer_declaration_is_allowed() -> Result<(), Box<dyn Error>> {
    let issues = scope_issues("use strict; my $outer = 1; my $f = sub { $outer };\n")?;
    require_no_undeclared(&issues, "$outer")
}

#[test]
fn strict_self_initializer_still_uses_outer_scope_order() -> Result<(), Box<dyn Error>> {
    let issues = scope_issues("use strict; my $x = $x;\n")?;
    require_undeclared(&issues, "$x")
}

#[test]
fn statement_modifier_hoisting_allows_prior_statement_use() -> Result<(), Box<dyn Error>> {
    let issues = scope_issues("use strict; print $x if my $x = 1;\n")?;
    require_no_undeclared(&issues, "$x")
}
