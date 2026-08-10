//! Tests for issues #5065 and #5067 — scope analyzer edge cases:
//!   #5065: `my $x` in postfix conditionals/statement modifiers must be
//!          hoisted (visible to the statement).  Previously the statement
//!          was analyzed before the condition, producing a false-positive
//!          UndeclaredVariable.
//!   #5067: `$:` (format line-break character) and other punctuation special
//!          variables were missing from the builtin list, producing
//!          false-positive UndeclaredVariable under `use strict`.

use perl_semantic_analyzer::Parser;
use perl_semantic_analyzer::analysis::scope_analyzer::{IssueKind, ScopeAnalyzer, ScopeIssue};
use perl_semantic_analyzer::pragma_tracker::PragmaTracker;
use perl_tdd_support::must;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn scope_issues_strict(code: &str) -> Vec<ScopeIssue> {
    let mut parser = Parser::new(code);
    let ast = must(parser.parse());
    let pragma_map = PragmaTracker::build(&ast);
    let analyzer = ScopeAnalyzer::new();
    analyzer.analyze(&ast, code, &pragma_map)
}

fn has_undeclared(issues: &[ScopeIssue], var_name: &str) -> bool {
    issues.iter().any(|i| i.kind == IssueKind::UndeclaredVariable && i.variable_name == var_name)
}

// ---------------------------------------------------------------------------
// #5065 — StatementModifier `my` hoisting
// ---------------------------------------------------------------------------

/// `print $x if my $x = 1;` — the `my` in the `if` modifier is hoisted to
/// the enclosing block, so `$x` in `print` must be visible.
#[test]
fn postfix_if_my_hoisted_no_false_positive() {
    let code = "use strict; use warnings;\nprint $x if my $x = 1;\n";
    let issues = scope_issues_strict(code);
    assert!(
        !has_undeclared(&issues, "$x"),
        "`print $x if my $x = 1;` should NOT produce UndeclaredVariable for $x (hoisted), got: {:?}",
        issues
    );
}

/// `foo() while my $x = bar();` — the `my` in the `while` modifier is hoisted.
#[test]
fn postfix_while_my_hoisted_no_false_positive() {
    let code = "use strict; use warnings;\nfoo() while my $x = bar();\n";
    let issues = scope_issues_strict(code);
    assert!(
        !has_undeclared(&issues, "$x"),
        "`foo() while my $x = bar();` should NOT produce UndeclaredVariable for $x (hoisted), got: {:?}",
        issues
    );
}

/// `die $err unless my $err = compute();` — `unless` modifier also hoists.
#[test]
fn postfix_unless_my_hoisted_no_false_positive() {
    let code = "use strict; use warnings;\ndie $err unless my $err = compute();\n";
    let issues = scope_issues_strict(code);
    assert!(
        !has_undeclared(&issues, "$err"),
        "`die $err unless my $err = compute();` should NOT produce UndeclaredVariable (hoisted), got: {:?}",
        issues
    );
}

/// Without strict vars the issue is moot, but verify no spurious diagnostic.
#[test]
fn postfix_if_my_no_strict() {
    let code = "print $x if my $x = 1;\n";
    let issues = scope_issues_strict(code);
    assert!(
        !has_undeclared(&issues, "$x"),
        "without strict vars, no UndeclaredVariable expected, got: {:?}",
        issues
    );
}

// ---------------------------------------------------------------------------
// #5067 — `$:` format line-break variable
// ---------------------------------------------------------------------------

/// `$:` is the format line-break characters variable — it must be recognized
/// as a builtin and not flagged as undeclared under strict vars.
#[test]
fn dollar_colon_recognized_as_builtin() {
    let code = "use strict; use warnings;\nlocal $:;\n";
    let issues = scope_issues_strict(code);
    assert!(
        !has_undeclared(&issues, "$:"),
        "`$:` should be a builtin (format line-break chars), got: {:?}",
        issues
    );
}

/// `$:` used in a print under strict should not be undeclared.
#[test]
fn dollar_colon_use_under_strict() {
    let code = "use strict; use warnings;\nprint $:;\n";
    let issues = scope_issues_strict(code);
    assert!(
        !has_undeclared(&issues, "$:"),
        "`$:` under strict should not be undeclared, got: {:?}",
        issues
    );
}

// ---------------------------------------------------------------------------
// Additional modifier coverage (#5094 review)
// ---------------------------------------------------------------------------

/// `print $x until my $x = 0;` — the `until` modifier also hoists.
#[test]
fn postfix_until_my_hoisted_no_false_positive() {
    let code = "use strict; use warnings;\nprint $x until my $x = 0;\n";
    let issues = scope_issues_strict(code);
    assert!(
        !has_undeclared(&issues, "$x"),
        "`print $x until my $x = 0;` should NOT produce UndeclaredVariable (hoisted), got: {:?}",
        issues
    );
}

/// `for` modifier don't declare via `my` in the condition, so this just
/// verifies no spurious diagnostic.
#[test]
fn postfix_for_no_false_positive() {
    let code = "use strict; use warnings;\nfoo() for (1, 2, 3);\n";
    let issues = scope_issues_strict(code);
    assert!(
        !has_undeclared(&issues, "$x"),
        "for modifier without my should not produce spurious diagnostics, got: {:?}",
        issues
    );
}
