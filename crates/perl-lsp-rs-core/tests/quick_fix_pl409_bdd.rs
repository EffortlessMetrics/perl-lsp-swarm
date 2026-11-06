//! PL409 removal is confined to a producer-confirmed standalone goto statement.

use std::sync::Arc;

use perl_lsp_rs_core::providers::code_actions::{CodeAction, CodeActionKind, CodeActionsProvider};
use perl_lsp_rs_core::providers::diagnostics::{
    Diagnostic, DiagnosticSeverity, DiagnosticsProvider,
};
use perl_parser::Parser;
use perl_tdd_support::{must, must_some};

fn diagnostics_for(source: &str) -> Vec<Diagnostic> {
    let output = Parser::new(source).parse_with_recovery();
    DiagnosticsProvider::new().get_diagnostics(
        &Arc::new(output.ast),
        &output.diagnostics,
        source,
        None,
    )
}

fn first_pl409(source: &str) -> Diagnostic {
    must_some(
        diagnostics_for(source).into_iter().find(|diag| diag.code.as_deref() == Some("PL409")),
    )
}

fn actions_for(source: &str, diagnostics: &[Diagnostic]) -> Vec<CodeAction> {
    let ast = must(Parser::new(source).parse());
    CodeActionsProvider::new(source.to_string())
        .get_code_actions(&ast, (0, source.len()), diagnostics)
        .into_iter()
        .filter(|action| action.diagnostics.iter().any(|code| code == "PL409"))
        .collect()
}

fn diagnostic(range: (usize, usize), code: &str) -> Diagnostic {
    Diagnostic {
        range,
        severity: DiagnosticSeverity::Warning,
        code: Some(code.to_string()),
        message: "Untrusted presentation does not authorize deletion".to_string(),
        related_information: Vec::new(),
        tags: Vec::new(),
        suggestion: None,
        fixable: false,
        critic_observation: None,
    }
}

fn edited(source: &str, action: &CodeAction) -> String {
    assert_eq!(action.edit.changes.len(), 1);
    let edit = &action.edit.changes[0];
    let mut result = source.to_string();
    result.replace_range(edit.location.start..edit.location.end, &edit.new_text);
    result
}

#[test]
fn pl409_removes_only_the_standalone_goto_line_and_clears_the_finding() {
    let source = "sub run {\n    goto MISSING;\n    print 42;\n}\n";
    let diag = first_pl409(source);
    let actions = actions_for(source, &[diag]);
    assert_eq!(actions.len(), 1, "current missing target must get one safe edit: {actions:?}");
    assert_eq!(actions[0].title, "Remove goto to undefined label");
    assert_eq!(actions[0].kind, CodeActionKind::QuickFix);
    assert!(actions[0].is_preferred);
    let result = edited(source, &actions[0]);
    assert_eq!(result, "sub run {\n    print 42;\n}\n");
    must(Parser::new(&result).parse());
    assert!(!diagnostics_for(&result).iter().any(|diag| diag.code.as_deref() == Some("PL409")));
}

#[test]
fn pl409_accepts_tabs_crlf_and_end_of_file_without_a_newline() {
    for source in ["\tgoto\tMISSING;\r\n", "goto MISSING;", "goto MISSING ;  \n"] {
        let diag = first_pl409(source);
        let actions = actions_for(source, &[diag]);
        assert_eq!(actions.len(), 1, "source {source:?}: {actions:?}");
        assert_eq!(edited(source, &actions[0]), "");
    }
}

#[test]
fn pl409_diagnostic_prose_is_irrelevant_to_the_source_backed_edit() {
    let source = "goto MISSING;\n";
    let mut diag = first_pl409(source);
    diag.message = "Renamed diagnostic wording with no quoted target".to_string();
    let actions = actions_for(source, &[diag]);
    assert_eq!(actions.len(), 1);
    assert_eq!(edited(source, &actions[0]), "");
}

#[test]
fn pl409_does_not_delete_suffix_statements_comments_or_modifiers() {
    for source in [
        "goto MISSING; print 42;\n",
        "goto MISSING; # preserve this comment\n",
        "goto MISSING if $condition;\n",
        "sub run {\n    goto MISSING; }\n",
    ] {
        let diag = first_pl409(source);
        assert!(actions_for(source, &[diag]).is_empty(), "unsafe line {source:?}");
    }
}

#[test]
fn pl409_does_not_delete_code_before_the_goto() {
    for source in ["sub run { goto MISSING; }\n", "print 42; goto MISSING;\n"] {
        let diag = first_pl409(source);
        assert!(actions_for(source, &[diag]).is_empty(), "unsafe prefix {source:?}");
    }
}

#[test]
fn pl409_defined_target_rejects_a_stale_finding_at_the_same_range() {
    let source = "goto FOUND;\nFOUND: print 42;\n";
    let start = must_some(source.find("FOUND"));
    let diag = diagnostic((start, start + "FOUND".len()), "PL409");
    assert!(actions_for(source, &[diag]).is_empty());
}

#[test]
fn pl409_dynamic_goto_and_source_lookalikes_have_no_removal_authority() {
    for (source, needle) in [
        ("my $target = 'MISSING';\ngoto $target;\n", "$target;"),
        ("sub target { 1 }\ngoto &target;\n", "target;"),
        ("my $text = 'goto MISSING;';\n", "MISSING"),
        ("my $text = '\n    goto MISSING;\n';\n", "MISSING"),
        ("# goto MISSING;\nmy $x = 1;\n", "MISSING"),
    ] {
        let start = must_some(source.find(needle));
        let end = start + needle.trim_end_matches(';').len();
        assert!(actions_for(source, &[diagnostic((start, end), "PL409")]).is_empty(), "{source:?}");
    }
}

#[test]
fn pl409_wrong_or_malformed_range_and_other_code_have_no_edit() {
    let source = "goto MISSING;\nmy $text = \"é\";\n";
    let current = first_pl409(source);
    let unicode_start = must_some(source.find('é'));
    for range in [
        (current.range.0 + 1, current.range.1),
        (current.range.0, current.range.0),
        (0, "goto MISSING;".len()),
        (source.len() + 1, source.len() + 2),
        (unicode_start + 1, unicode_start + 2),
    ] {
        assert!(actions_for(source, &[diagnostic(range, "PL409")]).is_empty(), "range {range:?}");
    }
    assert!(actions_for(source, &[diagnostic(current.range, "PL410")]).is_empty());
}
