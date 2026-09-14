#![deny(clippy::map_err_ignore)]
// Cohort C1 activation (#12598): all production rows exact-excepted; new findings move the crate back to non-C1.
//! Tests for issues #5065 and #5067 — scope analyzer edge cases:
//!   #5065: `my $x` in a postfix conditional / statement modifier.  These rows were
//!          originally written asserting that such a declaration is *hoisted* and so
//!          visible to the statement it modifies.  That premise is false: Perl does not
//!          hoist it, and the statement cannot see the binding.  Corrected under #1772
//!          against perl 5.38.2 as the oracle (see the per-test transcripts below).
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
// #5065 — StatementModifier `my` is NOT hoisted over the statement (#1772)
// ---------------------------------------------------------------------------

/// `print $x if my $x = 1;` — the `my` in the `if` modifier is **not** hoisted; the
/// statement it modifies cannot see the binding.
///
/// Oracle, perl 5.38.2:
/// ```text
/// $ perl -c -e 'use strict; use warnings;
/// print $x if my $x = 1;'
/// Global symbol "$x" requires explicit package name (did you forget to declare "my $x"?)
/// -e had compilation errors.
/// ```
#[test]
fn postfix_if_my_is_not_hoisted_over_statement() {
    let code = "use strict; use warnings;\nprint $x if my $x = 1;\n";
    let issues = scope_issues_strict(code);
    assert!(
        has_undeclared(&issues, "$x"),
        "`print $x if my $x = 1;` is a strict error in real Perl, so $x must be reported \
         undeclared, got: {:?}",
        issues
    );
}

/// Negative control for the corrected rows in this file: the statement does not *use* the
/// modifier's variable, so declaring it there is legal and must stay undiagnosed.  This is
/// what keeps those rows from being satisfied by blanket-flagging every modifier declaration.
///
/// Oracle, perl 5.38.2: `foo() while my $x = bar();` → `-e syntax OK` (exit 0).
#[test]
fn postfix_while_my_declaration_alone_is_not_flagged() {
    let code = "use strict; use warnings;\nfoo() while my $x = bar();\n";
    let issues = scope_issues_strict(code);
    assert!(
        !has_undeclared(&issues, "$x"),
        "`foo() while my $x = bar();` should NOT produce UndeclaredVariable for $x (hoisted), got: {:?}",
        issues
    );
}

/// `die $err unless my $err = compute();` — the `unless` modifier does not hoist either.
///
/// Oracle, perl 5.38.2:
/// ```text
/// Global symbol "$err" requires explicit package name (did you forget to declare "my $err"?)
/// ```
#[test]
fn postfix_unless_my_is_not_hoisted_over_statement() {
    let code = "use strict; use warnings;\ndie $err unless my $err = compute();\n";
    let issues = scope_issues_strict(code);
    assert!(
        has_undeclared(&issues, "$err"),
        "`die $err unless my $err = compute();` is a strict error in real Perl, so $err must \
         be reported undeclared, got: {:?}",
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

/// `print $x until my $x = 0;` — the `until` modifier does not hoist either.
///
/// Oracle, perl 5.38.2:
/// ```text
/// Global symbol "$x" requires explicit package name (did you forget to declare "my $x"?)
/// ```
#[test]
fn postfix_until_my_is_not_hoisted_over_statement() {
    let code = "use strict; use warnings;\nprint $x until my $x = 0;\n";
    let issues = scope_issues_strict(code);
    assert!(
        has_undeclared(&issues, "$x"),
        "`print $x until my $x = 0;` is a strict error in real Perl, so $x must be reported \
         undeclared, got: {:?}",
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
