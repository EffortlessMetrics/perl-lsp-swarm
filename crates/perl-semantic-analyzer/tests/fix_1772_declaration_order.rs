#![deny(clippy::map_err_ignore)]
// Cohort C1 activation (#12598): all production rows exact-excepted; new findings move the crate back to non-C1.
//! #1772 — lexical declarations in a statement modifier become visible only
//! after the entire statement. Traversal order alone cannot encode this boundary.
//!
//! Perl compile-only checks reject both `print $x if my $x=1` and
//! `my $x=1 if $x` under strict; `state` behaves likewise. Nested blocks finish
//! their own statements independently.
//!
//! Visibility is deferred, but the two children are still visited in *runtime*
//! order (condition, then statement), because capture state and initialization
//! genuinely depend on evaluation order — see
//! `modifier_children_are_analyzed_in_runtime_order`. `our` is not deferred and
//! is therefore reported by its own condition; that pre-existing source-order
//! alias gap is #15048 and is pinned by
//! `our_in_a_modifier_statement_is_a_known_source_order_gap` rather than being
//! papered over by inverting the four runtime facts.
//!
//! The current mirror and boundary controls were checked with Perl 5.42.0;
//! the older forward-use controls below retain their recorded 5.38.2 oracle.

use perl_semantic_analyzer::Parser;
use perl_semantic_analyzer::analysis::scope_analyzer::{IssueKind, ScopeAnalyzer, ScopeIssue};
use perl_semantic_analyzer::pragma_tracker::PragmaTracker;
type TestResult = Result<(), Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn scope_issues(code: &str) -> Result<Vec<ScopeIssue>, Box<dyn std::error::Error>> {
    let mut parser = Parser::new(code);
    let ast = parser.parse()?;
    let pragma_map = PragmaTracker::build(&ast);
    Ok(ScopeAnalyzer::new().analyze(&ast, code, &pragma_map))
}

fn has_undeclared(issues: &[ScopeIssue], var_name: &str) -> bool {
    issues.iter().any(|i| i.kind == IssueKind::UndeclaredVariable && i.variable_name == var_name)
}

fn check_visibility(code: &str, var_name: &str, undeclared: bool) -> TestResult {
    let issues = scope_issues(code)?;
    if has_undeclared(&issues, var_name) != undeclared {
        return Err(format!(
            "expected undeclared={undeclared} for {var_name} in {code:?}; issues: {issues:?}"
        )
        .into());
    }
    Ok(())
}

fn check_forward_use_reported(code: &str, var_name: &str) -> TestResult {
    check_visibility(code, var_name, true)
}

fn check_accepted(code: &str, var_name: &str) -> TestResult {
    check_visibility(code, var_name, false)
}

// ---------------------------------------------------------------------------
// The construct this PR actually repairs: statement modifiers
// ---------------------------------------------------------------------------

/// Oracle, perl 5.38.2:
/// ```text
/// $ perl -c -e 'use strict; print $x if my $x = 1;'
/// Global symbol "$x" requires explicit package name (did you forget to declare "my $x"?)
/// ```
///
/// The analyzer previously reordered the `StatementModifier` children so the condition was
/// analyzed first, making the declaration visible to the statement and silencing this.
#[test]
fn statement_modifier_declaration_is_not_visible_to_its_statement() -> TestResult {
    check_forward_use_reported("use strict;\nprint $x if my $x = 1;\n", "$x")?;
    Ok(())
}

/// The same for `unless`/`until`, so the repair is not keyed to one modifier keyword.
#[test]
fn every_conditional_modifier_keeps_declaration_order() -> TestResult {
    check_forward_use_reported(
        "use strict;\nsub compute {}\ndie $err unless my $err = compute();\n",
        "$err",
    )?;
    check_forward_use_reported("use strict;\nprint $x until my $x = 0;\n", "$x")?;
    Ok(())
}

/// Negative control — without a forward use, a modifier declaration is perfectly legal.
///
/// Oracle: `foo() while my $x = bar();` → `-e syntax OK`.
///
/// Without this row, blanket-flagging every statement-modifier `my` would pass the rows above.
#[test]
fn statement_modifier_declaration_alone_stays_clean() -> TestResult {
    check_accepted("use strict;\nsub foo {}\nsub bar {}\nfoo() while my $x = bar();\n", "$x")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Forward use in ordinary scopes — pinned so traversal changes cannot regress them
// ---------------------------------------------------------------------------

/// All four patterns named in #1772.  Oracle: each is
/// `Global symbol "..." requires explicit package name` under `use strict`.
#[test]
fn forward_use_is_reported_in_every_scope_kind() -> TestResult {
    check_forward_use_reported("use strict;\nprint $x;\nmy $x = 1;\n", "$x")?;
    check_forward_use_reported("use strict;\n{ print $x; my $x = 1; }\n", "$x")?;
    check_forward_use_reported("use strict;\nsub f { print $x; my $x = 1; }\n", "$x")?;
    check_forward_use_reported("use strict;\nif (1) { print $c; my $c = 5; }\n", "$c")?;
    Ok(())
}

/// A closure that captures a name declared later in the enclosing scope.
///
/// Oracle: `use strict; my $f = sub { print $x; }; my $x = 1;` →
/// `Global symbol "$x" requires explicit package name`.
#[test]
fn closure_cannot_capture_a_later_declaration() -> TestResult {
    check_forward_use_reported("use strict;\nmy $f = sub { print $x; };\nmy $x = 1;\n", "$x")?;
    Ok(())
}

/// `my $x = $x;` — the initializer's `$x` is the *outer* one, which does not exist here.
///
/// Oracle: `use strict; my $x = $x;` → `Global symbol "$x" requires explicit package name`.
/// This depends on the initializer being analyzed before the declaration is recorded
/// (`declarations.rs`), which is the same source-order rule these rows guard.
#[test]
fn self_initialization_does_not_see_its_own_binding() -> TestResult {
    check_forward_use_reported("use strict;\nmy $x = $x;\n", "$x")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Negative controls — the shapes an over-eager ordering rule would break
// ---------------------------------------------------------------------------

/// The load-bearing shadowing case: a use before an inner declaration resolves to the
/// *enclosing* binding rather than failing.  This is why an invisible declaration is skipped
/// to the parent scope instead of being reported as absent.
///
/// Oracle: `use strict; my $x = 1; { print $x; my $x = 2; print $x; }` → `-e syntax OK`.
#[test]
fn use_before_an_inner_declaration_resolves_to_the_outer_binding() -> TestResult {
    check_accepted("use strict;\nmy $x = 1;\n{ print $x; my $x = 2; print $x; }\n", "$x")?;
    Ok(())
}

/// A sub body may reference a binding declared earlier at file scope.
///
/// Oracle: `use strict; my $x = 1; sub f { print $x; }` → `-e syntax OK`.
#[test]
fn sub_body_sees_an_earlier_file_scope_binding() -> TestResult {
    check_accepted("use strict;\nmy $x = 1;\nsub f { print $x; }\n", "$x")?;
    Ok(())
}

/// Ordinary declare-then-use, loops, and the non-modifier `while (my $l = ...)` form — all
/// accepted by Perl and all must stay clean.
#[test]
fn ordinary_declaration_before_use_stays_clean() -> TestResult {
    check_accepted("use strict;\nmy $x = 1;\nprint $x;\n", "$x")?;
    check_accepted("use strict;\nmy @l = (1, 2);\nfor my $i (@l) { print $i; }\n", "$i")?;
    check_accepted("use strict;\nsub f {}\nwhile (my $l = f()) { print $l; }\n", "$l")?;
    Ok(())
}

/// Without `use strict` the name resolves as a package global, so forward use is legal and
/// must not be diagnosed.  Oracle: `print $x; my $x = 1;` → `-e syntax OK` (exit 0).
///
/// This is the control that keeps the repair strict-gated rather than universal.
#[test]
fn forward_use_without_strict_is_not_reported() -> TestResult {
    check_accepted("print $x;\nmy $x = 1;\n", "$x")?;
    check_accepted("{ print $x; my $x = 1; }\n", "$x")?;
    check_accepted("print $x if my $x = 1;\n", "$x")?;
    Ok(())
}

#[test]
fn both_modifier_children_wait_for_lexical_visibility() -> TestResult {
    for modifier in ["if", "unless", "while", "until", "for", "foreach"] {
        for declarator in ["my", "state"] {
            for body in [
                format!("print $x {modifier} {declarator} $x = 1;"),
                format!("{declarator} $x = 1 {modifier} $x;"),
                format!("print 1 {modifier} ({declarator} $x = 1, $x);"),
            ] {
                check_visibility(&format!("use strict; use feature 'state'; {body}"), "$x", true)?;
            }
        }
    }
    Ok(())
}

#[test]
fn completed_modifier_installs_lexicals() -> TestResult {
    for declarator in ["my", "state"] {
        check_visibility(
            &format!("use strict; use feature 'state'; {declarator} $x = 1 if 1; print $x;"),
            "$x",
            false,
        )?;
        check_visibility(
            &format!("use strict; use feature 'state'; print 1 if {declarator} $x = 1; print $x;"),
            "$x",
            false,
        )?;
    }
    check_visibility("my $x = 1 if $x;", "$x", false)?;
    Ok(())
}

/// Known gap, pinned so it is visible rather than silent: `our` is not deferred (it aliases a
/// package variable), but the condition is analyzed before the statement, so the alias is not
/// yet in hand when the condition is checked.
///
/// Oracle, perl 5.38.2: `use strict; our $x = 1 if $x;` → `-e syntax OK`, so this diagnostic
/// is a false positive.
///
/// This is a pre-existing source-order alias gap that `origin/main` shares — it is not
/// introduced here, and it is tracked as #15048. It is deliberately NOT fixed by visiting the
/// statement first: doing that inverts four runtime-order facts (both capture-state
/// directions and both initialization directions), which the rows below pin. Trading four
/// regressions for one false positive is the wrong direction.
///
/// When #15048 lands, this row flips to `false` and moves back into the test above.
#[test]
fn our_in_a_modifier_statement_is_a_known_source_order_gap() -> TestResult {
    check_visibility("use strict; our $x = 1 if $x;", "$x", true)?;
    Ok(())
}

/// Runtime-order facts. A modifier's condition really does run before the statement it
/// guards, so these four must follow evaluation order, not source order. Statement-first
/// traversal inverts all four; they are the reason this arm keeps the condition first.
///
/// Oracle, perl 5.38.2 (runtime, `perl -we`):
/// ```text
/// $_="ax"; print "[$1]" if /a(.)/;   -> [x]        the match sets $1 for the statement
/// $_="ax"; /a(.)/ if $1;             -> "Use of uninitialized value $1"
/// ```
#[test]
fn modifier_children_are_analyzed_in_runtime_order() -> TestResult {
    // Capture state, both directions.
    let sets = scope_issues("use strict;\nprint $1 if /a(.)/;\n")?;
    if sets.iter().any(|i| i.kind == IssueKind::CaptureVarWithoutRegexMatch) {
        return Err(format!("a match in the condition sets $1 for the statement: {sets:?}").into());
    }
    let unset = scope_issues("use strict;\n/a(.)/ if $1;\n")?;
    if !unset.iter().any(|i| i.kind == IssueKind::CaptureVarWithoutRegexMatch) {
        return Err(
            format!("the statement's match cannot set $1 for the condition: {unset:?}").into()
        );
    }

    // Initialization, both directions.
    let reads_uninit = scope_issues("use strict;\nmy $x;\n$x = 1 if $x;\n")?;
    if !reads_uninit.iter().any(|i| i.kind == IssueKind::UninitializedVariable) {
        return Err(format!(
            "the condition reads $x before the statement assigns it: {reads_uninit:?}"
        )
        .into());
    }
    let cond_initializes = scope_issues("use strict;\nmy $x;\nprint $x if ($x = 1);\n")?;
    if cond_initializes.iter().any(|i| i.kind == IssueKind::UninitializedVariable) {
        return Err(format!(
            "the condition initializes $x before the statement reads it: {cond_initializes:?}"
        )
        .into());
    }
    Ok(())
}

/// A regex inside an uninvoked `sub` in the condition does not run, so it cannot set `$1`.
///
/// Oracle, perl 5.38.2 (runtime): `$_="ax"; print "[$1]" if sub { /a(.)/ };` warns
/// `Use of uninitialized value $1` — the sub body is never called.
///
/// Ordinary (non-modifier) analysis already gets this right because a sub body owns its own
/// scope; this row pins that the modifier arm does not bypass that boundary.
#[test]
fn an_uninvoked_sub_body_in_the_condition_does_not_set_capture_state() -> TestResult {
    let issues = scope_issues("use strict;\nprint $1 if sub { /a(.)/ };\n")?;
    if !issues.iter().any(|i| i.kind == IssueKind::CaptureVarWithoutRegexMatch) {
        return Err(format!("an uninvoked sub body must not set $1: {issues:?}").into());
    }
    Ok(())
}

#[test]
fn deferred_declarations_keep_existing_and_outer_bindings_available() -> TestResult {
    check_visibility("use strict; my $x = 1; my $x = 2 if $x;", "$x", false)?;
    let code = "use strict; my $x = 1; { my $x = 2 if $x; print $x; }";
    let issues = scope_issues(code)?;
    if has_undeclared(&issues, "$x")
        || issues.iter().any(|i| i.kind == IssueKind::UnusedVariable && i.variable_name == "$x")
        || !issues.iter().any(|i| i.kind == IssueKind::VariableShadowing && i.variable_name == "$x")
    {
        return Err(format!("outer use, inner use and shadowing must survive: {issues:?}").into());
    }
    let code = "use strict; my $x = 1; { my $x = 2 if $x; }";
    let issues = scope_issues(code)?;
    let inner_offset = code.rfind("$x = 2").ok_or("inner declaration absent from fixture")?;
    let unused: Vec<_> = issues.iter().filter(|i| i.kind == IssueKind::UnusedVariable).collect();
    if unused.len() != 1 || unused.first().is_none_or(|i| i.range.0 != inner_offset) {
        return Err(format!("only the deferred inner binding should be unused: {issues:?}").into());
    }
    Ok(())
}

#[test]
fn nested_blocks_complete_only_their_own_declarations() -> TestResult {
    check_visibility("use strict; my $x = 1 if do { my $y = 2; $y };", "$y", false)?;
    check_visibility("use strict; my $x = 1 if do { my $y = 2; $x };", "$x", true)?;
    check_visibility("use strict; my $x = 1 if do { my $y = 2 if 1; $x };", "$x", true)?;
    check_visibility("use strict; my $x = 1 if do { my $y = 2 if 1; $y };", "$y", false)?;
    check_visibility("use strict; my $x = 1 if do { print 1 if 1; $x };", "$x", true)?;
    check_visibility("use strict; print 1 if do { my $x = 1 if 1; print $x; };", "$x", false)?;
    Ok(())
}

#[test]
fn deferred_declaration_metadata_is_retained() -> TestResult {
    let issues = scope_issues("use strict; my ($x, $x) = (1, 2) if 1;")?;
    for kind in [IssueKind::VariableRedeclaration, IssueKind::UnusedVariable] {
        if !issues.iter().any(|i| i.kind == kind && i.variable_name == "$x") {
            return Err(format!("missing {kind:?} for pending declaration: {issues:?}").into());
        }
    }
    let issues = scope_issues("use strict; my $x = 1 if my $x = 2; print $x;")?;
    if !issues.iter().any(|i| i.kind == IssueKind::VariableRedeclaration)
        || issues.iter().any(|i| i.kind == IssueKind::UnusedVariable)
    {
        return Err(
            format!("pending redeclaration and later usage must survive: {issues:?}").into()
        );
    }
    Ok(())
}

#[test]
fn builtin_consumes_its_pending_declaration_without_using_outer_binding() -> TestResult {
    let code = "use strict; my $fh; { open my $fh, '<', 'x' if 1; }";
    let issues = scope_issues(code)?;
    let outer_offset = code.find("$fh;").ok_or("outer declaration absent from fixture")?;
    let unused: Vec<_> = issues.iter().filter(|i| i.kind == IssueKind::UnusedVariable).collect();
    if unused.len() != 1 || unused.first().is_none_or(|i| i.range.0 != outer_offset) {
        return Err(
            format!("builtin must consume only the pending inner declaration: {issues:?}").into()
        );
    }
    let issues =
        scope_issues("use strict; use warnings; read STDIN, my $buf, 1 if 1; print $buf;")?;
    if issues.iter().any(|i| {
        i.variable_name == "$buf"
            && matches!(i.kind, IssueKind::UnusedVariable | IssueKind::UninitializedVariable)
    }) {
        return Err(
            format!("builtin must initialize pending declaration metadata: {issues:?}").into()
        );
    }
    check_visibility("use strict; open my $fh, '<', 'x' if $fh;", "$fh", true)?;
    Ok(())
}

#[test]
fn tie_initializes_its_pending_declaration() -> TestResult {
    let issues = scope_issues("use strict; use warnings; tie my $x, 'Class' if 1; print $x;")?;
    if issues.iter().any(|i| i.variable_name == "$x" && i.kind == IssueKind::UninitializedVariable)
    {
        return Err(format!("tie must initialize pending declaration: {issues:?}").into());
    }
    Ok(())
}

#[test]
fn package_declaration_targets_do_not_consume_pending_lexicals() -> TestResult {
    let code = "use strict; my $x if open our $x, '<', 'x';";
    let issues = scope_issues(code)?;
    let lexical_offset = code.find("$x if").ok_or("lexical declaration absent from fixture")?;
    if !issues.iter().any(|i| i.kind == IssueKind::UnusedVariable && i.range.0 == lexical_offset) {
        return Err(format!("package open must not consume the pending lexical: {issues:?}").into());
    }
    let issues = scope_issues("use strict; use warnings; my $x if tie our $x, 'Class'; print $x;")?;
    if !issues.iter().any(|i| i.kind == IssueKind::UninitializedVariable && i.variable_name == "$x")
    {
        return Err(
            format!("package tie must not initialize the pending lexical: {issues:?}").into()
        );
    }
    Ok(())
}

/// Capture state is a *runtime* effect and survives the deferral change, so it still depends
/// on the order the two halves are visited: the condition runs first, and a match in it sets
/// `$1` for the statement it guards.
///
/// Oracle, perl 5.38.2 — both compile cleanly:
/// ```text
/// $ perl -c -e 'use strict; print $1 if /(foo)/;'                   -e syntax OK
/// $ perl -c -e 'use strict; my $s="a"; print $1 if $s =~ /(foo)/;'  -e syntax OK
/// ```
///
/// Deferring declarations makes *declaration* visibility order-independent, which is what
/// lets the halves be visited in runtime order. Visiting the statement first instead
/// reports a false `CaptureVarWithoutRegexMatch` on a thoroughly ordinary Perl idiom — the
/// regression this row exists to catch.
#[test]
fn a_regex_match_in_the_condition_sets_capture_state_for_the_statement() -> TestResult {
    for code in [
        "use strict;\nprint $1 if /(foo)/;\n",
        "use strict;\nmy $s = 'a';\nprint $1 if $s =~ /(foo)/;\n",
    ] {
        let issues = scope_issues(code)?;
        if issues.iter().any(|i| i.kind == IssueKind::CaptureVarWithoutRegexMatch) {
            return Err(format!(
                "real Perl compiles {code:?}, and the condition runs before the statement, \
                 so $1 must not be reported without a match: {issues:?}"
            )
            .into());
        }
    }
    Ok(())
}

/// When both halves of one modifier declare the same lexical, the binding that survives must
/// be the one Perl resolves — the **textually later** declaration.
///
/// In `EXPR if COND` the condition is always textually after the statement, so analyzing the
/// condition first (see `modifier_children_are_analyzed_in_runtime_order`) makes the
/// first-traversed declaration the textually-later one, and the redeclaration guard keeps it.
/// Statement-first traversal keeps the earlier one instead and attaches later reads to its
/// initialization state, which is the defect reported on this PR.
///
/// Perl leaves the *runtime value* of a conditional `my` undefined, so the pinned property is
/// the static binding identity, observed through the initialization state that follows it.
#[test]
fn a_duplicate_modifier_declaration_keeps_the_textually_later_binding() -> TestResult {
    // Later declaration (the condition) is initialized -> the following read is initialized.
    let later_initialized =
        scope_issues("use strict; use warnings;\nmy $x if my $x = 2;\nprint $x;\n")?;
    if later_initialized.iter().any(|i| i.kind == IssueKind::UninitializedVariable) {
        return Err(format!(
            "the later declaration `my $x = 2` is initialized, so the read is too: \
             {later_initialized:?}"
        )
        .into());
    }

    // Later declaration (the condition) is uninitialized -> the following read is not.
    let later_uninitialized =
        scope_issues("use strict; use warnings;\nmy $x = 1 if my $x;\nprint $x;\n")?;
    if !later_uninitialized.iter().any(|i| i.kind == IssueKind::UninitializedVariable) {
        return Err(format!(
            "the later declaration `my $x` is uninitialized, so the read must be reported: \
             {later_uninitialized:?}"
        )
        .into());
    }

    // The redeclaration itself stays diagnosed in both orderings.
    for code in [
        "use strict; use warnings;\nmy $x if my $x = 2;\n",
        "use strict; use warnings;\nmy $x = 1 if my $x;\n",
    ] {
        let issues = scope_issues(code)?;
        if !issues.iter().any(|i| i.kind == IssueKind::VariableRedeclaration) {
            return Err(format!("redeclaration must stay diagnosed in {code:?}: {issues:?}").into());
        }
    }
    Ok(())
}

/// A declaration *builtin* whose target duplicates a name declared in the same modifier must
/// not consume the retained binding. The builtin's own declaration was rejected as a
/// duplicate, so the slot that survives belongs to the other declaration and keeps its state.
///
/// Oracle, perl 5.38.2 (runtime):
/// ```text
/// $ perl -we 'open my $fh, "<", "/dev/null" if my $fh; print "[$fh]\n";'
/// "my" variable $fh masks earlier declaration in same statement
/// Use of uninitialized value $fh in concatenation (.) or string
/// ```
///
/// The condition is false (the later `my $fh` is undef), so the `open` never runs and the
/// surviving binding stays uninitialized. Looking the pending slot up by name alone marked it
/// initialized and erased that warning, while the equivalent non-builtin form
/// (`my $x = 1 if my $x;`) reported it correctly — the inconsistency this row pins.
#[test]
fn a_builtin_declaration_target_does_not_consume_a_duplicate_binding() -> TestResult {
    for code in [
        "use strict; use warnings;\nopen my $fh, '<', 'f' if my $fh;\nprint $fh;\n",
        "use strict; use warnings;\ntie my $x, 'C' if my $x;\nprint $x;\n",
    ] {
        let issues = scope_issues(code)?;
        if !issues.iter().any(|i| i.kind == IssueKind::UninitializedVariable) {
            return Err(format!(
                "the retained binding is the condition's uninitialized declaration, so the \
                 following read must be reported in {code:?}: {issues:?}"
            )
            .into());
        }
    }

    // Negative control — with no duplicate, the builtin really does initialize its target.
    let no_duplicate =
        scope_issues("use strict; use warnings;\ntie my $x, 'C' if 1;\nprint $x;\n")?;
    if no_duplicate.iter().any(|i| i.kind == IssueKind::UninitializedVariable) {
        return Err(format!(
            "without a duplicate, the builtin initializes its own pending slot: {no_duplicate:?}"
        )
        .into());
    }
    Ok(())
}
