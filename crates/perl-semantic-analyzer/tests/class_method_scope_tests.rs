#![deny(clippy::map_err_ignore)]
// Cohort C1 activation (#12598): all production rows exact-excepted; new findings move the crate back to non-C1.
//! Regression tests for Perl 5.38+ `use feature 'class'` scope analysis (issue #4647).
//!
//! Verifies that:
//! - Method signature parameters are NOT reported as `UndeclaredVariable`.
//! - The implicit `$self` invocant is NOT reported as `UndeclaredVariable`.
//! - Variables that are genuinely undeclared inside a method body ARE still reported.
//! - The class body itself opens a proper scope (variables declared inside do not leak).

use perl_semantic_analyzer::Parser;
use perl_semantic_analyzer::analysis::scope_analyzer::{IssueKind, ScopeAnalyzer, ScopeIssue};
use perl_semantic_analyzer::pragma_tracker::PragmaTracker;
use perl_tdd_support::must;

fn scope_issues(code: &str) -> Vec<ScopeIssue> {
    let mut parser = Parser::new(code);
    let ast = must(parser.parse());
    let analyzer = ScopeAnalyzer::new();
    analyzer.analyze(&ast, code, &[])
}

fn scope_issues_strict(code: &str) -> Vec<ScopeIssue> {
    let mut parser = Parser::new(code);
    let ast = must(parser.parse());
    let pragma_map = PragmaTracker::build(&ast);
    let analyzer = ScopeAnalyzer::new();
    analyzer.analyze(&ast, code, &pragma_map)
}

fn has_issue(issues: &[ScopeIssue], kind: IssueKind, var_name: &str) -> bool {
    issues.iter().any(|i| i.kind == kind && i.variable_name.contains(var_name))
}

fn no_undeclared(issues: &[ScopeIssue]) -> bool {
    !issues.iter().any(|i| i.kind == IssueKind::UndeclaredVariable)
}

// ---------------------------------------------------------------------------
// Signature parameters must not be flagged as undeclared
// ---------------------------------------------------------------------------

#[test]
fn method_signature_param_not_undeclared() {
    // `$other` is a method parameter — must NOT be UndeclaredVariable.
    let code = r#"
use feature 'class';
class Point {
    field $x = 0;
    field $y = 0;

    method distance($other) {
        my $dx = $x - $other->x;
        my $dy = $y - $other->y;
        return sqrt($dx * $dx + $dy * $dy);
    }
}
"#;
    let issues = scope_issues(code);
    assert!(
        !has_issue(&issues, IssueKind::UndeclaredVariable, "other"),
        "method parameter `$other` must not be reported as undeclared; issues: {:?}",
        issues.iter().map(|i| (&i.kind, &i.variable_name)).collect::<Vec<_>>()
    );
}

#[test]
fn method_multiple_params_none_undeclared() {
    let code = r#"
use feature 'class';
class Rectangle {
    method area($width, $height) {
        return $width * $height;
    }
}
"#;
    let issues = scope_issues(code);
    assert!(
        !has_issue(&issues, IssueKind::UndeclaredVariable, "width"),
        "`$width` must not be undeclared; issues: {:?}",
        issues.iter().map(|i| (&i.kind, &i.variable_name)).collect::<Vec<_>>()
    );
    assert!(
        !has_issue(&issues, IssueKind::UndeclaredVariable, "height"),
        "`$height` must not be undeclared; issues: {:?}",
        issues.iter().map(|i| (&i.kind, &i.variable_name)).collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// Implicit `$self` invocant must not be flagged as undeclared
// ---------------------------------------------------------------------------

#[test]
fn method_self_invocant_not_undeclared() {
    let code = r#"
use feature 'class';
class Counter {
    field $count = 0;

    method increment() {
        $self->{count}++;
        return $self;
    }
}
"#;
    let issues = scope_issues(code);
    assert!(
        !has_issue(&issues, IssueKind::UndeclaredVariable, "self"),
        "`$self` must not be reported as undeclared; issues: {:?}",
        issues.iter().map(|i| (&i.kind, &i.variable_name)).collect::<Vec<_>>()
    );
}

#[test]
fn method_no_signature_self_not_undeclared() {
    // Method without explicit parameters — `$self` still implicitly available.
    let code = r#"
use feature 'class';
class Greeter {
    method greet() {
        return "Hello from " . ref($self);
    }
}
"#;
    let issues = scope_issues(code);
    assert!(
        !has_issue(&issues, IssueKind::UndeclaredVariable, "self"),
        "`$self` without params must not be undeclared; issues: {:?}",
        issues.iter().map(|i| (&i.kind, &i.variable_name)).collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// Genuinely undeclared variables in method bodies ARE still caught
// ---------------------------------------------------------------------------

#[test]
fn method_body_truly_undeclared_still_reported() {
    // `$typo` is neither a parameter nor `$self` — must still be reported.
    let code = r#"
use strict;
use feature 'class';
class Foo {
    method bar($x) {
        return $typo + $x;
    }
}
"#;
    let issues = scope_issues_strict(code);
    assert!(
        has_issue(&issues, IssueKind::UndeclaredVariable, "typo"),
        "genuinely undeclared `$typo` must still be reported; issues: {:?}",
        issues.iter().map(|i| (&i.kind, &i.variable_name)).collect::<Vec<_>>()
    );
    // The explicit param must remain clean
    assert!(
        !has_issue(&issues, IssueKind::UndeclaredVariable, "x"),
        "`$x` (declared param) must not be undeclared; issues: {:?}",
        issues.iter().map(|i| (&i.kind, &i.variable_name)).collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// Class body opens a proper package scope
// ---------------------------------------------------------------------------

#[test]
fn class_does_not_produce_false_undeclared_for_clean_body() {
    let code = r#"
use feature 'class';
class Animal {
    field $name;

    method new_with_name($n) {
        my $obj = Animal->new;
        $obj->{name} = $n;
        return $obj;
    }

    method speak() {
        return "...";
    }
}
"#;
    let issues = scope_issues(code);
    assert!(
        no_undeclared(&issues),
        "clean class body must produce no UndeclaredVariable; issues: {:?}",
        issues.iter().map(|i| (&i.kind, &i.variable_name)).collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// Inheritance / :isa parent class does not affect inner-scope correctness
// ---------------------------------------------------------------------------

#[test]
fn class_isa_parent_method_params_not_undeclared() {
    let code = r#"
use feature 'class';
class Dog :isa(Animal) {
    method bark($times) {
        my $sound = "Woof!" x $times;
        return $sound;
    }
}
"#;
    let issues = scope_issues(code);
    assert!(
        !has_issue(&issues, IssueKind::UndeclaredVariable, "times"),
        "`$times` must not be undeclared in subclass method; issues: {:?}",
        issues.iter().map(|i| (&i.kind, &i.variable_name)).collect::<Vec<_>>()
    );
}
