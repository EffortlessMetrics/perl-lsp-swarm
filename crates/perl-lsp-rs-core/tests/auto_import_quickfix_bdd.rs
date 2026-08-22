//! BDD containment proof for the withdrawn hard-coded missing-import edits
//! (issue #10690).
//!
//! Until #790 lands the exact candidate planner, a hard-coded function→module
//! affinity or a PL109 presentation grants NO import-edit authority:
//!
//! ```text
//! hard-coded spelling / PL109 presentation
//!     != candidate identity
//!     != import edit authorization
//! ```
//!
//! These tests were written against unmodified `main` and proven failing before
//! the production routes were deleted (shift-left). Any mutation that
//! re-couples the static spelling table (`guess_module_for_function`) or the
//! PL109 diagnostic route to an import insertion must fail here.
//!
//! Controls preserved: PL109 quoting/filehandle quick fixes stay available.
//!
//! Exact-process coverage lives in
//! `crates/perllsp/tests/lsp_missing_import_withdrawal_process.rs`; the defect
//! class is production routing, so provider-unit coverage alone is insufficient
//! there.

use perl_lsp_rs_core::providers::code_actions::{CodeAction, CodeActionKind, CodeActionsProvider};
use perl_lsp_rs_core::providers::diagnostics::{Diagnostic, DiagnosticSeverity};
use perl_parser::Parser;
use perl_tdd_support::{must, must_some};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn bareword_diag(source: &str, name: &str) -> Diagnostic {
    let start = must_some(source.find(name));
    Diagnostic {
        range: (start, start + name.len()),
        severity: DiagnosticSeverity::Error,
        code: Some("PL109".to_string()),
        message: format!(
            "Bareword '{name}' is not allowed under 'use strict' -- quote it as '{name}' or use it as a subroutine call"
        ),
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

/// Modules of the withdrawn hard-coded table (#10690).
const WITHDRAWN_TABLE_MODULES: &[&str] =
    &["Data::Dumper", "Encode", "File::Basename", "File::Path", "File::Slurp", "JSON"];

/// Spellings hard-coded in the withdrawn table; none may authorize an edit.
const WITHDRAWN_TABLE_SPELLINGS: &[&str] = &[
    "dumper",
    "encode",
    "decode",
    "basename",
    "dirname",
    "mkpath",
    "rmtree",
    "slurp",
    "decode_json",
    "encode_json",
];

fn is_withdrawn_import_presentation(action: &CodeAction) -> Option<String> {
    if action.title.starts_with("Import '") || action.title.contains("Add missing imports") {
        return Some(format!("withdrawn import presentation: {}", action.title));
    }
    for module in WITHDRAWN_TABLE_MODULES {
        for edit in &action.edit.changes {
            if edit.new_text.contains(&format!("use {module};")) {
                return Some(format!("edit inserts 'use {module};': {}", action.title));
            }
            // Byte-zero-style directive insertion anywhere in the returned
            // changes for a table module is exactly the withdrawn authority.
            if edit.location.start == 0 && edit.new_text.contains(&format!("use {module}")) {
                return Some(format!("byte-zero insert of 'use {module}': {}", action.title));
            }
        }
    }
    None
}

fn reject_withdrawn_import_actions(
    actions: &[CodeAction],
) -> Result<(), Box<dyn std::error::Error>> {
    let offending: Vec<String> =
        actions.iter().filter_map(is_withdrawn_import_presentation).collect();
    assert!(
        offending.is_empty(),
        "hard-coded missing-import edits are withdrawn (#10690); got: {offending:?}"
    );
    Ok(())
}

fn assert_no_enabled_noop_stand_in(
    source: &str,
    actions: &[CodeAction],
) -> Result<(), Box<dyn std::error::Error>> {
    for action in actions {
        // An enabled action whose only effect is rewriting bytes to themselves
        // (or carrying no change at all) must not stand in for refusal.
        let noop = !action.edit.changes.is_empty()
            && action
                .edit
                .changes
                .iter()
                .all(|edit| edit.new_text == &source[edit.location.start..edit.location.end]);
        assert!(!noop, "a no-op rewrite stands in for refusal: {action:?}");
        let empty_enabled =
            action.kind == CodeActionKind::QuickFix
                && action.edit.changes.iter().any(|edit| {
                    edit.new_text.is_empty() && edit.location.start == edit.location.end
                });
        assert!(
            !empty_enabled,
            "an enabled empty-insertion edit stands in for refusal: {action:?}"
        );
    }
    Ok(())
}

fn assert_quoting_controls_survive(
    actions: &[CodeAction],
) -> Result<(), Box<dyn std::error::Error>> {
    assert!(
        actions.iter().any(|action| action.title.contains("with single quotes")
            && action.kind == CodeActionKind::QuickFix),
        "unrelated PL109 quoting fixes must remain available: {:?}",
        actions.iter().map(|action| &action.title).collect::<Vec<_>>()
    );
    Ok(())
}

// ===========================================================================
// Withdrawn table spellings cannot authorize any import edit
// ===========================================================================

#[test]
fn withdrawn_table_spellings_cannot_authorize_import_edits()
-> Result<(), Box<dyn std::error::Error>> {
    for spelling in WITHDRAWN_TABLE_SPELLINGS {
        let source = format!("use strict;\nmy $result = {spelling}($input);\n");
        let diag = bareword_diag(&source, spelling);
        let actions = actions_for(&source, &[diag]);

        reject_withdrawn_import_actions(&actions)?;
        assert_no_enabled_noop_stand_in(&source, &actions)?;
    }
    Ok(())
}

#[test]
fn dumper_cannot_produce_use_data_dumper_from_the_hard_coded_table()
-> Result<(), Box<dyn std::error::Error>> {
    let source = "use strict;\nmy $out = dumper($hash);\n";
    let actions = actions_for(source, &[bareword_diag(source, "dumper")]);

    reject_withdrawn_import_actions(&actions)?;
    assert!(
        !actions.iter().any(|action| action.diagnostics.iter().any(|code| code == "PL109")
            && action
                .edit
                .changes
                .iter()
                .any(|edit| edit.new_text.trim_start().starts_with("use "))),
        "no PL109-keyed action may insert a directive while imports are withdrawn: {actions:?}"
    );
    Ok(())
}

#[test]
fn pl109_presentation_cannot_add_an_import_while_quoting_stays_available()
-> Result<(), Box<dyn std::error::Error>> {
    let source = "use strict;\nmy $name = basename($path);\n";
    let actions = actions_for(source, &[bareword_diag(source, "basename")]);

    reject_withdrawn_import_actions(&actions)?;
    assert_quoting_controls_survive(&actions)?;
    assert_no_enabled_noop_stand_in(source, &actions)
}

#[test]
fn uppercase_filehandle_declaration_survives_alongside_refused_imports()
-> Result<(), Box<dyn std::error::Error>> {
    // Uppercase barewords also get the filehandle declaration option from the
    // surviving PL109 family. It must remain reachable even though no import
    // action may exist for mapped spellings in the same response.
    let source = "use strict;\nprint DUMPFILE $record;\n";
    let actions = actions_for(source, &[bareword_diag(source, "DUMPFILE")]);

    reject_withdrawn_import_actions(&actions)?;
    assert!(
        actions.iter().any(|action| action.title.contains("Declare 'DUMPFILE' as filehandle")),
        "the filehandle declaration control must survive: {:?}",
        actions.iter().map(|action| &action.title).collect::<Vec<_>>()
    );
    Ok(())
}

// ===========================================================================
// Collisions and already-satisfied imports never re-authorize the mapping
// ===========================================================================

#[test]
fn identically_named_local_subroutine_cannot_trigger_the_mapping()
-> Result<(), Box<dyn std::error::Error>> {
    // The call site precedes the definition so the diagnosed range covers the
    // bare spelling itself; affinity alone must never authorize an edit even
    // when a same-named local sub exists.
    let source =
        "use strict;\nmy $b = basename($path);\nsub basename { my ($p) = @_; return $p; }\n";
    // The call site precedes the definition, so this range covers the bare
    // spelling at the call; affinity alone must never authorize an edit even
    // when a same-named local sub exists.
    let actions = actions_for(source, &[bareword_diag(source, "basename")]);

    reject_withdrawn_import_actions(&actions)?;
    assert_no_enabled_noop_stand_in(source, &actions)
}

#[test]
fn already_imported_module_cannot_receive_a_duplicate_directive()
-> Result<(), Box<dyn std::error::Error>> {
    let source = "use strict;\nuse Data::Dumper;\nmy $out = dumper($hash);\n";
    let actions = actions_for(source, &[bareword_diag(source, "dumper")]);

    reject_withdrawn_import_actions(&actions)?;
    assert_no_enabled_noop_stand_in(source, &actions)
}

#[test]
fn unresolvable_symbol_produces_no_action_and_does_not_crash()
-> Result<(), Box<dyn std::error::Error>> {
    let source = "use strict;\nmy $x = frobnicate(1);\n";
    let actions = actions_for(source, &[bareword_diag(source, "frobnicate")]);

    reject_withdrawn_import_actions(&actions)?;
    Ok(())
}

#[test]
fn multi_package_file_receives_no_table_module_insertion_anywhere()
-> Result<(), Box<dyn std::error::Error>> {
    // Insertion geometry was part of the withdrawn authority: byte-zero or
    // wrong-package insertion must not return under any presentation.
    let source = "package A;\nuse strict;\nsub a_tool { }\n1;\npackage B;\nuse warnings;\nmy $x = decode_json($raw);\n1;\n";
    let actions = actions_for(source, &[bareword_diag(source, "decode_json")]);

    reject_withdrawn_import_actions(&actions)?;
    assert_no_enabled_noop_stand_in(source, &actions)
}
