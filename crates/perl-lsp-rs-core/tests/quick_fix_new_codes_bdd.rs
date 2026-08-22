//! BDD tests for quick-fix handlers added in this release:
//!   PL700 - Unused import (WITHDRAWN, #11079 — no import-removal action may
//!           return; the diagnostic stays a non-fixable advisory)
//!   PL501 - Deprecated `$[` array base (`fix_deprecated_array_base`)
//!   PL602 - Global signal handler (`fix_security_signal_handler`)
//!
//! Each scenario follows the pattern:
//!   GIVEN  source with a specific anti-pattern
//!   WHEN   diagnostics are produced and code actions are requested
//!   THEN   exactly the expected action(s) are returned with correct edits

use perl_lsp_rs_core::providers::code_actions::{CodeAction, CodeActionKind, CodeActionsProvider};
use perl_lsp_rs_core::providers::diagnostics::{Diagnostic, DiagnosticSeverity};
use perl_parser::Parser;
use perl_tdd_support::must;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_diag(start: usize, end: usize, code: &str, message: &str) -> Diagnostic {
    Diagnostic {
        range: (start, end),
        severity: DiagnosticSeverity::Warning,
        code: Some(code.to_string()),
        message: message.to_string(),
        related_information: Vec::new(),
        tags: Vec::new(),
        suggestion: None,
    }
}

fn actions_for(source: &str, diags: &[Diagnostic]) -> Vec<CodeAction> {
    let mut parser = Parser::new(source);
    let ast = must(parser.parse());
    let provider = CodeActionsProvider::new(source.to_string());
    provider.get_code_actions(&ast, (0, source.len()), diags)
}

/// Apply the first matching edit from an action and return the resulting source.
fn edited(source: &str, action: &CodeAction) -> String {
    let mut edits = action.edit.changes.clone();
    edits.sort_by_key(|edit| std::cmp::Reverse(edit.location.start));
    let mut out = source.to_string();
    for edit in edits {
        out.replace_range(edit.location.start..edit.location.end, &edit.new_text);
    }
    out
}

/// Find the first action whose title matches the predicate.
fn find_action(actions: &[CodeAction], pred: impl Fn(&str) -> bool) -> Option<&CodeAction> {
    actions.iter().find(|a| pred(&a.title))
}

// ===========================================================================
// PL700 - Unused import: WITHDRAWN (#11079)
//
// The prose-driven whole-line removal edit is withdrawn until the exact
// replacement trains land (#1719 explicit-symbol removal, #8322 complete
// module-load assessment). A PL700 diagnostic must produce no import-removal
// action at all — no enabled edit, no empty/no-op stand-in.
// ===========================================================================

fn assert_no_unused_import_action(
    actions: &[CodeAction],
) -> Result<(), Box<dyn std::error::Error>> {
    let offending = actions
        .iter()
        .filter(|action| {
            action.diagnostics.iter().any(|code| code == "PL700")
                || action.title.contains("Remove unused 'use")
                || action.title.contains("Remove unused import")
        })
        .collect::<Vec<_>>();
    assert!(
        offending.is_empty(),
        "the PL700 import-removal action is withdrawn (#11079); got: {offending:?}"
    );
    Ok(())
}

#[test]
fn withdrawn_pl700_produces_no_remove_action() -> Result<(), Box<dyn std::error::Error>> {
    // GIVEN a file that imports a module but never uses it
    let source = "use strict;\nuse List::Util;\nmy $x = 1;\n";

    let use_start = source.find("use List::Util;").ok_or("marker not found")?;
    let use_end = use_start + "use List::Util;".len();

    let diag = make_diag(use_start, use_end, "PL700", "Module 'List::Util' appears to be unused");

    // WHEN code actions are requested for the diagnostic range
    let actions = actions_for(source, &[diag]);

    // THEN no action offers removal of the import
    assert_no_unused_import_action(&actions)?;

    Ok(())
}

#[test]
fn withdrawn_pl700_cannot_delete_entire_line() -> Result<(), Box<dyn std::error::Error>> {
    // GIVEN a source where the previously-removable import sits between others
    let source = "use strict;\nuse POSIX;\nuse warnings;\n";
    let use_start = source.find("use POSIX;").ok_or("marker not found")?;
    let use_end = use_start + "use POSIX;".len();

    let diag = make_diag(use_start, use_end, "PL700", "Module 'POSIX' appears to be unused");
    let actions = actions_for(source, &[diag]);

    assert_no_unused_import_action(&actions)?;
    // No quick fix may delete or blank the diagnosed import line. (The
    // separate organize-imports family is #8305's concern, not this
    // containment.)
    for action in &actions {
        if action.kind != CodeActionKind::QuickFix {
            continue;
        }
        for change in &action.edit.changes {
            let covered = &source[change.location.start..change.location.end];
            let deletes_import = covered.contains("use POSIX;");
            assert!(
                !(deletes_import && change.new_text.is_empty()),
                "a quick fix still deletes the diagnosed import line: {action:?}"
            );
        }
    }

    Ok(())
}

#[test]
fn withdrawn_pl700_at_start_of_file_produces_no_edit() -> Result<(), Box<dyn std::error::Error>> {
    let source = "use LWP::UserAgent;\nuse strict;\n";
    let use_end = "use LWP::UserAgent;".len();

    let diag = make_diag(0, use_end, "PL700", "Module 'LWP::UserAgent' appears to be unused");
    let actions = actions_for(source, &[diag]);

    assert_no_unused_import_action(&actions)?;

    Ok(())
}

#[test]
fn withdrawn_pl700_message_prose_grants_no_authority() -> Result<(), Box<dyn std::error::Error>> {
    let source = "use Foo::Bar::Baz;\n";
    let use_end = "use Foo::Bar::Baz;".len();
    let diag = make_diag(0, use_end, "PL700", "Module 'Foo::Bar::Baz' appears to be unused");
    let actions = actions_for(source, &[diag]);

    assert_no_unused_import_action(&actions)?;

    Ok(())
}

#[test]
fn unused_import_non_use_range_is_ignored() -> Result<(), Box<dyn std::error::Error>> {
    let source = "my $x = 1;\n";
    let diag = make_diag(3, 5, "PL700", "Module 'POSIX' appears to be unused");
    let actions = actions_for(source, &[diag]);

    assert!(
        !actions.iter().any(|action| action.title.contains("Remove unused")),
        "expected no unused-import action for non-use range, got: {actions:?}"
    );

    Ok(())
}

#[test]
fn unused_import_non_char_boundary_range_is_ignored() -> Result<(), Box<dyn std::error::Error>> {
    let source = "use strict;\nmy $name = \"\u{e9}\";\n";
    let char_start = source.find('\u{e9}').ok_or("marker not found")?;
    let diag =
        make_diag(char_start + 1, char_start + 2, "PL700", "Module 'POSIX' appears to be unused");
    let actions = actions_for(source, &[diag]);

    assert!(
        !actions.iter().any(|action| action.title.contains("Remove unused")),
        "expected no unused-import action for non-char-boundary range, got: {actions:?}"
    );

    Ok(())
}

// ===========================================================================
// PL501 - Deprecated `$[` array base variable
// ===========================================================================

#[test]
fn deprecated_array_base_produces_remove_action() -> Result<(), Box<dyn std::error::Error>> {
    // GIVEN code that assigns to the deprecated $[ variable
    let source = "use strict;\n$[ = 0;\nmy @arr = (1, 2, 3);\n";
    let dollar_start = source.find("$[").ok_or("marker not found")?;
    let dollar_end = dollar_start + 2; // "$[" is two bytes

    let diag = make_diag(
        dollar_start,
        dollar_end,
        "PL501",
        "Use of '$[' is deprecated and will be removed",
    );

    // WHEN code actions are requested
    let actions = actions_for(source, &[diag]);

    // THEN there is an action offering removal
    let action = find_action(&actions, |t| t.contains("$['"))
        .ok_or_else(|| format!("no '$[' action in: {:?}", actions))?;

    assert_eq!(action.title, "Remove deprecated '$[' array base assignment");
    assert_eq!(action.kind, CodeActionKind::QuickFix);
    assert!(action.is_preferred);

    Ok(())
}

#[test]
fn deprecated_array_base_edit_removes_entire_statement_line()
-> Result<(), Box<dyn std::error::Error>> {
    // GIVEN a standalone `$[ = 0;` line between two other statements
    let source = "use strict;\n$[ = 0;\nmy @arr = ();\n";
    let dollar_start = source.find("$[").ok_or("marker not found")?;
    let dollar_end = dollar_start + 2;

    let diag = make_diag(dollar_start, dollar_end, "PL501", "Use of '$[' is deprecated");
    let actions = actions_for(source, &[diag]);

    let action = find_action(&actions, |t| t.contains("$['"))
        .ok_or_else(|| format!("no '$[' action in: {:?}", actions))?;
    let result = edited(source, action);

    // THEN the `$[ = 0;\n` line is gone; surrounding code is intact
    assert_eq!(result, "use strict;\nmy @arr = ();\n");

    Ok(())
}

#[test]
fn deprecated_array_base_nonzero_assignment_is_not_auto_fixed()
-> Result<(), Box<dyn std::error::Error>> {
    // GIVEN `$[ = 1;` (non-default, requires manual renumbering)
    let source = "use strict;\n$[ = 1;\nprint $arr[1];\n";
    let dollar_start = source.find("$[").ok_or("marker not found")?;
    let dollar_end = dollar_start + 2;

    let diag = make_diag(dollar_start, dollar_end, "PL501", "Use of '$[' is deprecated");
    let actions = actions_for(source, &[diag]);

    // THEN we do not offer a destructive edit for non-zero array bases.
    assert!(
        !actions.iter().any(|action| action.title.contains("$['")),
        "expected no destructive PL501 action for non-zero assignment, got: {actions:?}"
    );

    Ok(())
}

#[test]
fn deprecated_array_base_rhs_use_does_not_offer_line_delete()
-> Result<(), Box<dyn std::error::Error>> {
    // GIVEN `$[` is used inside a larger statement, where deleting the whole
    // line would remove unrelated code.
    let source = "my $base = $[;\n";
    let dollar_start = source.find("$[").ok_or("marker not found")?;
    let dollar_end = dollar_start + 2;

    let diag = make_diag(dollar_start, dollar_end, "PL501", "Use of '$[' is deprecated");
    let actions = actions_for(source, &[diag]);

    assert!(
        !actions.iter().any(|action| action.title.contains("$['")),
        "expected no destructive PL501 action for RHS usage, got: {actions:?}"
    );

    Ok(())
}

#[test]
fn deprecated_array_base_non_char_boundary_range_is_ignored()
-> Result<(), Box<dyn std::error::Error>> {
    let source = "$[ = 0;\nmy $name = \"\u{e9}\";\n";
    let char_start = source.find('\u{e9}').ok_or("marker not found")?;
    let diag = make_diag(char_start + 1, char_start + 2, "PL501", "Use of '$[' is deprecated");
    let actions = actions_for(source, &[diag]);

    assert!(
        !actions.iter().any(|action| action.title.contains("$['")),
        "expected no PL501 action for non-char-boundary range, got: {actions:?}"
    );

    Ok(())
}

// ===========================================================================
// PL602 - Global signal handler assignment
// ===========================================================================

#[test]
fn signal_handler_produces_local_action() -> Result<(), Box<dyn std::error::Error>> {
    // GIVEN a bare global $SIG{__DIE__} assignment at file scope
    let source = "use strict;\n$SIG{__DIE__} = sub { die @_ };\n";
    let sig_start = source.find("$SIG{__DIE__}").ok_or("marker not found")?;
    let stmt_end = source[sig_start..].find(";\n").ok_or("semicolon not found")? + sig_start;

    let diag = make_diag(
        sig_start,
        stmt_end,
        "PL602",
        "Global assignment to $SIG{__DIE__} changes process-wide behavior.",
    );

    // WHEN code actions are requested
    let actions = actions_for(source, &[diag]);

    // THEN there is an action offering to add `local`
    let action = find_action(&actions, |t| t.contains("local"))
        .ok_or_else(|| format!("no 'local' action in: {:?}", actions))?;

    assert_eq!(action.title, "Add 'local' to scope signal handler to the current block");
    assert_eq!(action.kind, CodeActionKind::QuickFix);
    assert!(action.is_preferred);

    Ok(())
}

#[test]
fn signal_handler_edit_prepends_local_before_sig() -> Result<(), Box<dyn std::error::Error>> {
    let source = "$SIG{__WARN__} = sub { warn @_ };\n";
    let sig_start = 0usize;
    let stmt_end = source.find(";\n").ok_or("semicolon not found")?;

    let diag = make_diag(sig_start, stmt_end, "PL602", "Global $SIG{__WARN__} assignment");
    let actions = actions_for(source, &[diag]);

    let action = find_action(&actions, |t| t.contains("local"))
        .ok_or_else(|| format!("no 'local' action in: {:?}", actions))?;
    let result = edited(source, action);

    // THEN `local ` is prepended to the $SIG reference
    assert!(
        result.starts_with("local $SIG{__WARN__}"),
        "expected 'local $SIG{{__WARN__}}' prefix, got: {result:?}"
    );

    Ok(())
}

#[test]
fn signal_handler_main_qualified_sig_gets_local() -> Result<(), Box<dyn std::error::Error>> {
    let source = "$main::SIG{__DIE__} = sub { };\n";
    let sig_start = 0usize;
    let stmt_end = source.find(";\n").ok_or("semicolon not found")?;

    let diag = make_diag(sig_start, stmt_end, "PL602", "Global $main::SIG assignment");
    let actions = actions_for(source, &[diag]);

    let action = find_action(&actions, |t| t.contains("local"))
        .ok_or_else(|| format!("no 'local' action in: {:?}", actions))?;
    let result = edited(source, action);
    assert!(result.starts_with("local $main::SIG"), "expected local prefix, got: {result:?}");

    Ok(())
}

#[test]
fn signal_handler_guard_suppresses_action_for_non_sig_range()
-> Result<(), Box<dyn std::error::Error>> {
    // GIVEN a PL602 diagnostic whose range points to something that is NOT $SIG
    // (this simulates a misrouted or synthetic diagnostic)
    let source = "my $x = 1;\n";
    let diag = make_diag(3, 5, "PL602", "Spurious signal handler diagnostic");
    let actions = actions_for(source, &[diag]);

    // THEN no signal-handler-specific "local" action is produced
    let has_local_action = actions.iter().any(|a| a.title.contains("local"));
    assert!(
        !has_local_action,
        "expected no 'local' action for misaligned range, got: {:?}",
        actions
    );

    Ok(())
}

#[test]
fn signal_handler_non_char_boundary_range_is_ignored() -> Result<(), Box<dyn std::error::Error>> {
    let source = "$SIG{__DIE__} = sub { };\nmy $name = \"\u{e9}\";\n";
    let char_start = source.find('\u{e9}').ok_or("marker not found")?;
    let diag = make_diag(char_start + 1, char_start + 2, "PL602", "Global $SIG assignment");
    let actions = actions_for(source, &[diag]);

    assert!(
        !actions.iter().any(|action| action.title.contains("local")),
        "expected no signal-handler action for non-char-boundary range, got: {actions:?}"
    );

    Ok(())
}

// ===========================================================================
// Cross-cutting: dispatch-table smoke test
// ===========================================================================

#[test]
fn surviving_new_codes_reach_handlers_while_pl700_stays_withdrawn()
-> Result<(), Box<dyn std::error::Error>> {
    // Dispatch-table smoke test: the surviving codes still reach their
    // handlers, and the withdrawn PL700 family produces no import action in
    // the same response.

    let source = "use strict;\nuse Foo;\n$[ = 0;\n$SIG{__DIE__} = sub {};\n";
    let mut parser = Parser::new(source);
    let ast = must(parser.parse());
    let provider = CodeActionsProvider::new(source.to_string());

    let foo_start = source.find("use Foo;").ok_or("use Foo not found")?;
    let dollar_start = source.find("$[").ok_or("$[ not found")?;
    let sig_start = source.find("$SIG{__DIE__}").ok_or("$SIG not found")?;
    let sig_stmt_end = source[sig_start..].find(";\n").ok_or("sig stmt end not found")? + sig_start;

    let diags = vec![
        // PL700 - unused import (withdrawn, #11079)
        make_diag(
            foo_start,
            foo_start + "use Foo;".len(),
            "PL700",
            "Module 'Foo' appears to be unused",
        ),
        // PL501 - deprecated $[
        make_diag(dollar_start, dollar_start + 2, "PL501", "Use of '$[' is deprecated"),
        // PL602 - global signal handler
        make_diag(sig_start, sig_stmt_end, "PL602", "Global $SIG assignment"),
    ];

    let actions = provider.get_code_actions(&ast, (0, source.len()), &diags);

    assert_no_unused_import_action(&actions)?;
    let has_use_foo_deletion = actions.iter().any(|a| {
        a.kind == CodeActionKind::QuickFix
            && a.edit.changes.iter().any(|change| {
                (&source[change.location.start..change.location.end]).contains("use Foo;")
                    && change.new_text.is_empty()
            })
    });
    let has_pl501 = actions.iter().any(|a| a.title.contains("$['"));
    let has_pl602 = actions.iter().any(|a| a.title.contains("local"));

    assert!(
        !has_use_foo_deletion,
        "a quick fix still deletes the diagnosed import line; actions: {actions:?}"
    );
    assert!(has_pl501, "PL501 route not producing action; actions: {:?}", actions);
    assert!(has_pl602, "PL602 route not producing action; actions: {:?}", actions);

    Ok(())
}
