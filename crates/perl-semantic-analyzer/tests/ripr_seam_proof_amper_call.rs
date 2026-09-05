#![deny(clippy::map_err_ignore)]
// Cohort C1 activation (#12598): all production rows exact-excepted; new findings move the crate back to non-C1.
//! RIPR seam proofs for `handle_amper_call` (#1730).
//!
//! Covers the scope-analyzer path for `NodeKind::AmperCall` introduced when
//! ampersand-sigil calls stopped sharing `FunctionCall` nodes.

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

fn has_issue(issues: &[ScopeIssue], kind: IssueKind, var_name: &str) -> bool {
    issues.iter().any(|issue| issue.kind == kind && issue.variable_name.contains(var_name))
}

/// `&$coderef` must record a use of the coderef variable under strict.
#[test]
fn seam_amper_call_dynamic_coderef_records_variable_use() {
    let issues = scope_issues_strict("use strict;\nmy $coderef = sub {};\n&$coderef;\n");
    assert!(
        !has_issue(&issues, IssueKind::UndeclaredVariable, "coderef"),
        "declared coderef must not be reported undeclared; got: {issues:?}"
    );
}

/// `&$missing` must surface an undeclared-variable issue under strict.
#[test]
fn seam_amper_call_dynamic_coderef_flags_undeclared() {
    let issues = scope_issues_strict("use strict;\n&$missing;\n");
    assert!(
        has_issue(&issues, IssueKind::UndeclaredVariable, "missing"),
        "dynamic &$missing must report UndeclaredVariable; got: {issues:?}"
    );
}

/// `&foo($bar)` must analyze call arguments, not only the callee name.
#[test]
fn seam_amper_call_with_args_analyzes_arguments() {
    let issues = scope_issues_strict("use strict;\n&foo($bar);\n");
    assert!(
        has_issue(&issues, IssueKind::UndeclaredVariable, "bar"),
        "&foo($bar) must analyze $bar under strict; got: {issues:?}"
    );
}

/// Bare `&helper` (no parens) must not invent false undeclared-variable noise.
#[test]
fn seam_amper_call_without_parens_does_not_flag_helper_name() {
    let issues = scope_issues_strict("use strict;\n&helper;\n");
    assert!(
        !has_issue(&issues, IssueKind::UndeclaredVariable, "helper"),
        "plain &helper must not be treated as a variable use; got: {issues:?}"
    );
}
