//! Containment guards for the withdrawn auto-import quick-fix
//! (issues #790 planning basis, #10690 containment).
//!
//! The PL109 diagnostic route previously resolved bareword symbols against a
//! static symbol-to-module map and offered an enabled "Import `<Module>`"
//! quick fix inserting `use Module;`. Hard-coded name affinity is not
//! candidate identity and not edit authorization, so every such route is
//! withdrawn until #790/#8948 land exact candidate planning.
//!
//! Guards:
//!   - table-mapped barewords → no import action on any provider path
//!   - PL109's legitimate quote/filehandle fixes remain available
//!   - builtins/unresolvable symbols → no crash, no action regression

use perl_lsp_rs_core::providers::code_actions::{CodeAction, CodeActionKind, CodeActionsProvider};
use perl_lsp_rs_core::providers::diagnostics::{Diagnostic, DiagnosticSeverity};
use perl_parser::Parser;
use perl_tdd_support::must;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn bareword_diag(start: usize, end: usize, name: &str) -> Diagnostic {
    Diagnostic {
        range: (start, end),
        severity: DiagnosticSeverity::Error,
        code: Some("PL109".to_string()),
        message: format!(
            "Bareword '{}' is not allowed under 'use strict' -- quote it as '{}' or use it as a subroutine call",
            name, name
        ),
        related_information: Vec::new(),
        tags: Vec::new(),
        fixable: false,
        suggestion: None,
    }
}

fn actions_for(source: &str, diags: &[Diagnostic]) -> Vec<CodeAction> {
    let mut parser = Parser::new(source);
    let ast = must(parser.parse());
    let provider = CodeActionsProvider::new(source.to_string());
    provider.get_code_actions(&ast, (0, source.len()), diags)
}

fn find_import_action(actions: &[CodeAction]) -> Option<&CodeAction> {
    actions.iter().find(|a| a.title.starts_with("Import '") || a.title == "Add missing imports")
}

fn assert_no_import_action(actions: &[CodeAction]) {
    assert!(
        find_import_action(actions).is_none(),
        "hard-coded affinity must not authorize import edits (#10690); got: {actions:?}"
    );
    for action in actions {
        for edit in &action.edit.changes {
            let inserts_use_line =
                edit.new_text.lines().any(|line| line.trim_start().starts_with("use "));
            // The missing-pragma quick fix legitimately inserts exactly these
            // pragma texts; any other `use` insertion is affinity-derived.
            let legitimate_pragma_insertion = matches!(
                edit.new_text.as_str(),
                "use strict;\n"
                    | "use warnings;\n"
                    | "use strict;\nuse warnings;\n"
                    | "use strict;\nuse warnings;\n\n"
            );
            assert!(
                !inserts_use_line || legitimate_pragma_insertion,
                "action {:?} carries an import-insertion edit ({:?}); hard-coded affinity must not authorize edits (#10690)",
                action.title,
                edit.new_text
            );
        }
    }
}

// ===========================================================================
// Withdrawal: resolvable-by-table symbols → no import action
// ===========================================================================

#[test]
fn decode_json_bareword_offers_no_import_action() -> Result<(), Box<dyn std::error::Error>> {
    // GIVEN a file calling decode_json without importing JSON
    let source = "use strict;\nuse warnings;\nmy $data = decode_json($text);\n";
    let call_start = source.find("decode_json").ok_or("marker not found")?;
    let call_end = call_start + "decode_json".len();

    let diag = bareword_diag(call_start, call_end, "decode_json");

    // WHEN code actions are requested
    let actions = actions_for(source, &[diag]);

    // THEN no import action is offered; the quote fix remains.
    assert_no_import_action(&actions);
    assert!(
        actions.iter().any(|a| a.kind == CodeActionKind::QuickFix),
        "PL109 quick fixes must remain available; got: {actions:?}"
    );

    Ok(())
}

#[test]
fn basename_bareword_offers_no_import_action() -> Result<(), Box<dyn std::error::Error>> {
    // GIVEN a file calling basename without importing File::Basename
    let source = "use strict;\nmy $name = basename($path);\n";
    let call_start = source.find("basename").ok_or("marker not found")?;
    let call_end = call_start + "basename".len();

    let diag = bareword_diag(call_start, call_end, "basename");
    let actions = actions_for(source, &[diag]);

    assert_no_import_action(&actions);
    assert!(
        actions.iter().any(|a| a.title == "Quote 'basename' with single quotes"),
        "PL109 single-quote fix must remain available; got: {actions:?}"
    );

    Ok(())
}

#[test]
fn dumper_bareword_in_package_first_file_offers_no_import_action()
-> Result<(), Box<dyn std::error::Error>> {
    // The withdrawn route inserted before `package`, importing into main.
    let source = "package App;\ndumper($value);\n1;\n";
    let call_start = source.find("dumper").ok_or("marker not found")?;
    let call_end = call_start + "dumper".len();

    let diag = bareword_diag(call_start, call_end, "dumper");
    let actions = actions_for(source, &[diag]);

    assert_no_import_action(&actions);

    Ok(())
}

#[test]
fn locally_defined_same_name_bareword_offers_no_import_action()
-> Result<(), Box<dyn std::error::Error>> {
    // An identically named local callable made the table actively wrong.
    let source = "use strict;\nsub basename { return 1; }\nmy $name = basename($path);\n";
    let call_start = source.rfind("basename").ok_or("marker not found")?;
    let call_end = call_start + "basename".len();

    let diag = bareword_diag(call_start, call_end, "basename");
    let actions = actions_for(source, &[diag]);

    assert_no_import_action(&actions);

    Ok(())
}

#[test]
fn explicit_empty_import_form_still_offers_no_import_action()
-> Result<(), Box<dyn std::error::Error>> {
    // `use Encode qw();` does not make decode() visible; the withdrawn route
    // must not come back as a "fix" for that either (#10690).
    let source = "use Encode qw();\nuse strict;\nmy $text = decode($bytes);\n";
    let call_start = source.find("decode").ok_or("marker not found")?;
    let call_end = call_start + "decode".len();

    let diag = bareword_diag(call_start, call_end, "decode");
    let actions = actions_for(source, &[diag]);

    assert_no_import_action(&actions);

    Ok(())
}

// ===========================================================================
// Regression: builtins → no import action
// ===========================================================================

#[test]
fn builtin_print_bareword_no_import_action() -> Result<(), Box<dyn std::error::Error>> {
    // GIVEN a file that uses a builtin (which wouldn't get PL109 in practice,
    // but we guard the routing against generating a spurious import action)
    let source = "use strict;\nprint 'hello';\n";
    let call_start = source.find("print").ok_or("marker not found")?;
    let call_end = call_start + "print".len();

    let diag = bareword_diag(call_start, call_end, "print");
    let actions = actions_for(source, &[diag]);

    assert_no_import_action(&actions);

    Ok(())
}

#[test]
fn chomp_builtin_bareword_no_import_action() -> Result<(), Box<dyn std::error::Error>> {
    let source = "use strict;\nchomp($line);\n";
    let call_start = source.find("chomp").ok_or("marker not found")?;
    let call_end = call_start + "chomp".len();

    let diag = bareword_diag(call_start, call_end, "chomp");
    let actions = actions_for(source, &[diag]);

    assert_no_import_action(&actions);

    Ok(())
}

// ===========================================================================
// Regression: unresolvable symbol → no action (no crash)
// ===========================================================================

#[test]
fn unresolvable_symbol_no_import_action() -> Result<(), Box<dyn std::error::Error>> {
    // GIVEN a file calling an unknown function not in any map
    let source = "use strict;\nmy $result = frobnicate(1);\n";
    let call_start = source.find("frobnicate").ok_or("marker not found")?;
    let call_end = call_start + "frobnicate".len();

    let diag = bareword_diag(call_start, call_end, "frobnicate");
    let actions = actions_for(source, &[diag]);

    assert_no_import_action(&actions);

    Ok(())
}

#[test]
fn unresolvable_symbol_does_not_crash() -> Result<(), Box<dyn std::error::Error>> {
    // GIVEN edge cases: empty-ish sources, very long name, unicode name
    let long_name = "a".repeat(300);
    let sources_and_names: Vec<(String, String)> = vec![
        ("use strict;\nnot_a_known_func();\n".to_string(), "not_a_known_func".to_string()),
        ("use strict;\nsome_other_thing();\n".to_string(), "some_other_thing".to_string()),
        (format!("use strict;\n{long_name}();\n"), long_name),
        ("use strict;\ncafé_thing();\n".to_string(), "café_thing".to_string()),
    ];

    for (source, name) in &sources_and_names {
        let Some(call_start) = source.find(name) else {
            continue;
        };
        let call_end = call_start + name.len();
        let diag = bareword_diag(call_start, call_end, name);
        let _actions = actions_for(source, &[diag]);
        // Must not panic
    }

    Ok(())
}
