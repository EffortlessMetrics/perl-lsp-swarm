#![deny(clippy::map_err_ignore)]
//! #16670 — a `require` module bareword is not an expression-position
//! unquoted bareword under `use strict`.
//!
//! Oracle: perl 5.38+ accepts `require DBI` / `require Foo::Bar` under
//! `use strict`. The same token text in expression position (`my $x = DBI`)
//! remains a strict-subs error. Classification must follow the AST operand
//! role, not token spelling or a same-line `require` scan.

use perl_semantic_analyzer::Parser;
use perl_semantic_analyzer::analysis::scope_analyzer::{IssueKind, ScopeAnalyzer, ScopeIssue};
use perl_semantic_analyzer::pragma_tracker::PragmaTracker;
use perl_semantic_analyzer::{Node, NodeKind};
use perl_tdd_support::{must, must_some_with};

fn parse_ast(code: &str) -> Node {
    let mut parser = Parser::new(code);
    must(parser.parse())
}

fn parse_recovered(code: &str) -> Node {
    let mut parser = Parser::new(code);
    parser.parse_with_recovery().ast
}

fn scope_issues_strict(code: &str) -> Vec<ScopeIssue> {
    let ast = parse_ast(code);
    let pragma_map = PragmaTracker::build(&ast);
    ScopeAnalyzer::new().analyze(&ast, code, &pragma_map)
}

fn scope_issues_strict_recovered(code: &str) -> Vec<ScopeIssue> {
    let ast = parse_recovered(code);
    let pragma_map = PragmaTracker::build(&ast);
    ScopeAnalyzer::new().analyze(&ast, code, &pragma_map)
}

fn unquoted_names(issues: &[ScopeIssue]) -> Vec<&str> {
    issues
        .iter()
        .filter(|issue| issue.kind == IssueKind::UnquotedBareword)
        .map(|issue| issue.variable_name.as_str())
        .collect()
}

fn has_unquoted(issues: &[ScopeIssue], name: &str) -> bool {
    issues
        .iter()
        .any(|issue| issue.kind == IssueKind::UnquotedBareword && issue.variable_name == name)
}

fn unquoted_span_text<'a>(code: &'a str, issues: &[ScopeIssue], name: &str) -> Option<&'a str> {
    issues.iter().find_map(|issue| {
        if issue.kind == IssueKind::UnquotedBareword && issue.variable_name == name {
            code.get(issue.range.0..issue.range.1)
        } else {
            None
        }
    })
}

/// Direct first argument of `FunctionCall { name: "require" }` when that
/// argument is an Identifier — the syntactic module-request role.
fn require_module_operand_names(node: &Node) -> Vec<&str> {
    let mut names = Vec::new();
    collect_require_module_operand_names(node, &mut names);
    names
}

fn collect_require_module_operand_names<'a>(node: &'a Node, names: &mut Vec<&'a str>) {
    if let NodeKind::FunctionCall { name, args } = &node.kind
        && name == "require"
        && let Some(NodeKind::Identifier { name: module }) = args.first().map(|arg| &arg.kind)
    {
        names.push(module.as_str());
    }
    node.for_each_child(|child| collect_require_module_operand_names(child, names));
}

fn assert_require_operand_role(code: &str, expected: &[&str]) {
    let ast = parse_ast(code);
    let names = require_module_operand_names(&ast);
    assert_eq!(
        names, expected,
        "parser must expose {expected:?} as require-module Identifier operands in {code:?}; got {names:?}"
    );
}

#[test]
fn require_dbi_is_a_module_operand_not_an_expression_bareword() {
    let code = "use strict;\nrequire DBI;\n";
    assert_require_operand_role(code, &["DBI"]);
    let issues = scope_issues_strict(code);
    assert!(
        !has_unquoted(&issues, "DBI"),
        "require DBI is legal Perl under strict; UnquotedBareword must not fire: {:?}",
        unquoted_names(&issues)
    );
}

#[test]
fn require_qualified_module_is_a_module_operand() {
    let code = "use strict;\nrequire Foo::Bar;\n";
    assert_require_operand_role(code, &["Foo::Bar"]);
    let issues = scope_issues_strict(code);
    assert!(
        !has_unquoted(&issues, "Foo::Bar"),
        "require Foo::Bar is a module request: {:?}",
        unquoted_names(&issues)
    );
}

#[test]
fn nested_begin_eval_and_conditional_require_keep_the_module_role() {
    let cases = [
        ("use strict;\nBEGIN { require DBI }\n", "DBI"),
        ("use strict;\neval { require DBI };\n", "DBI"),
        (
            "use strict;\nif ($optional) { require Some::Optional::Module }\n",
            "Some::Optional::Module",
        ),
        ("use strict;\nrequire DBI or die;\n", "DBI"),
        ("use strict;\nrequire DBI if $need;\n", "DBI"),
        ("use strict;\nrequire(DBI);\n", "DBI"),
    ];
    for (code, module) in cases {
        assert_require_operand_role(code, &[module]);
        let issues = scope_issues_strict(code);
        assert!(
            !has_unquoted(&issues, module),
            "nested/parenthesized require of {module} must keep the module-request role in {code:?}: {:?}",
            unquoted_names(&issues)
        );
    }
}

#[test]
fn expression_bareword_with_the_same_spelling_still_flags() {
    let cases = [
        "use strict;\nmy $x = DBI;\n",
        "use strict;\nfoo(DBI);\n",
        "use strict;\nDBI + 1;\n",
        "use strict;\nprint DBI;\n",
    ];
    for code in cases {
        let ast = parse_ast(code);
        assert!(
            require_module_operand_names(&ast).is_empty(),
            "control source must not parse DBI as a require operand: {code:?}"
        );
        let issues = scope_issues_strict(code);
        assert!(
            has_unquoted(&issues, "DBI"),
            "expression-position DBI must still be UnquotedBareword in {code:?}: {:?}",
            unquoted_names(&issues)
        );
        assert_eq!(
            unquoted_span_text(code, &issues, "DBI"),
            Some("DBI"),
            "PL109 range must cover the DBI token in {code:?}"
        );
    }
}

#[test]
fn require_then_same_spelling_in_expression_still_flags_the_expression() {
    let code = "use strict;\nrequire DBI;\nmy $x = DBI;\n";
    assert_require_operand_role(code, &["DBI"]);
    let issues = scope_issues_strict(code);
    let dbi_hits: Vec<_> = issues
        .iter()
        .filter(|issue| issue.kind == IssueKind::UnquotedBareword && issue.variable_name == "DBI")
        .collect();
    assert_eq!(
        dbi_hits.len(),
        1,
        "only the expression-position DBI is illegal: {:?}",
        unquoted_names(&issues)
    );
    let hit = must_some_with(dbi_hits.into_iter().next(), "exactly one DBI UnquotedBareword");
    assert_eq!(code.get(hit.range.0..hit.range.1), Some("DBI"));
    let require_at = must_some_with(code.find("require DBI"), "require DBI present");
    assert!(
        hit.range.0 > require_at,
        "the remaining UnquotedBareword must be the later expression, not the require operand"
    );
}

#[test]
fn same_line_require_does_not_suppress_unrelated_barewords() {
    let code = "use strict;\nprint DBI; require Foo;\n";
    assert_require_operand_role(code, &["Foo"]);
    let issues = scope_issues_strict(code);
    assert!(
        has_unquoted(&issues, "DBI"),
        "DBI is the print operand, not the require operand: {:?}",
        unquoted_names(&issues)
    );
    assert!(
        !has_unquoted(&issues, "Foo"),
        "Foo is the require module operand: {:?}",
        unquoted_names(&issues)
    );
}

#[test]
fn other_require_forms_are_not_reclassified_as_module_barewords() {
    let cases = [
        "use strict;\nrequire \"Foo/Bar.pm\";\nBARE;\n",
        "use strict;\nrequire $path;\nBARE;\n",
        "use strict;\nrequire 5.040;\nBARE;\n",
    ];
    for code in cases {
        let ast = parse_ast(code);
        assert!(
            require_module_operand_names(&ast).is_empty(),
            "string/variable/version require must not expose an Identifier module operand: {code:?}"
        );
        let issues = scope_issues_strict(code);
        assert!(
            has_unquoted(&issues, "BARE"),
            "unrelated expression bareword must still flag after a non-bareword require in {code:?}: {:?}",
            unquoted_names(&issues)
        );
    }
}

#[test]
fn computed_require_argument_does_not_inherit_module_role() {
    let code = "use strict;\nrequire(Foo . \"/Bar.pm\");\n";
    let ast = parse_ast(code);
    assert!(
        require_module_operand_names(&ast).is_empty(),
        "computed require(Foo . ...) must not treat Foo as a module operand"
    );
    let issues = scope_issues_strict(code);
    assert!(
        has_unquoted(&issues, "Foo"),
        "Foo inside a computed require argument is still an expression bareword: {:?}",
        unquoted_names(&issues)
    );
}

#[test]
fn recovered_require_does_not_disable_unquoted_bareword_globally() {
    let code = "use strict;\nrequire DBI\nFOO + 1;\n";
    let issues = scope_issues_strict_recovered(code);
    assert!(
        has_unquoted(&issues, "FOO"),
        "recovery around require DBI must not suppress UnquotedBareword for FOO: {:?}",
        unquoted_names(&issues)
    );
}

#[test]
fn demo_database_begin_eval_require_dbi_is_legal() {
    let code = include_str!("../../../demo_workspace/lib/Database.pm");
    assert_require_operand_role(code, &["DBI"]);
    let issues = scope_issues_strict(code);
    assert!(
        !has_unquoted(&issues, "DBI"),
        "demo Database.pm require DBI must not be UnquotedBareword: {:?}",
        unquoted_names(&issues)
    );
}
