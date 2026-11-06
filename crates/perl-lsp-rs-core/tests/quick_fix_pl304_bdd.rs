//! Source-backed PL304 edits: current producer and exact subroutine geometry.

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

fn first_pl304(source: &str) -> Diagnostic {
    must_some(
        diagnostics_for(source).into_iter().find(|diag| diag.code.as_deref() == Some("PL304")),
    )
}

fn actions_for(source: &str, diagnostics: &[Diagnostic]) -> Vec<CodeAction> {
    let ast = must(Parser::new(source).parse());
    CodeActionsProvider::new(source.to_string())
        .get_code_actions(&ast, (0, source.len()), diagnostics)
        .into_iter()
        .filter(|action| action.diagnostics.iter().any(|code| code == "PL304"))
        .collect()
}

fn diagnostic(range: (usize, usize), code: &str) -> Diagnostic {
    Diagnostic {
        range,
        severity: DiagnosticSeverity::Hint,
        code: Some(code.to_string()),
        message: "Untrusted presentation text 'not_the_sub_name'".to_string(),
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

const MODULE: &str = "package M;\nuse Exporter 'import';\nour @EXPORT = qw(run);\nsub run { 42 }\n";

#[test]
fn pl304_current_exported_sub_gets_pod_and_edit_clears_the_finding() {
    let diag = first_pl304(MODULE);
    let actions = actions_for(MODULE, &[diag]);
    assert_eq!(actions.len(), 1, "current exported sub must receive one edit: {actions:?}");
    let action = &actions[0];
    assert_eq!(action.kind, CodeActionKind::QuickFix);
    assert!(action.is_preferred);
    assert_eq!(action.title, "Add '=head2 run' POD documentation stub");
    let result = edited(MODULE, action);
    assert!(result.contains("\n\n=head2 run\n\nDescription.\n\n=cut\n\nsub run { 42 }\n"));
    must(Parser::new(&result).parse());
    assert!(
        !diagnostics_for(&result).iter().any(|diag| diag.code.as_deref() == Some("PL304")),
        "the applied edit must satisfy the same producer that authorized it"
    );
}

#[test]
fn pl304_uses_the_current_ast_name_instead_of_diagnostic_prose() {
    let mut diag = first_pl304(MODULE);
    diag.message = "Wording changed entirely; 'injected\n=cut\ncode' is presentation".to_string();
    let actions = actions_for(MODULE, &[diag]);
    assert_eq!(actions.len(), 1);
    assert!(edited(MODULE, &actions[0]).contains("=head2 run\n"));
    assert!(!edited(MODULE, &actions[0]).contains("injected"));
}

#[test]
fn pl304_indented_standalone_sub_gets_column_one_pod_without_moving_code() {
    let source = MODULE.replace("sub run", "  sub run");
    let diag = first_pl304(&source);
    let actions = actions_for(&source, &[diag]);
    assert_eq!(actions.len(), 1);
    let result = edited(&source, &actions[0]);
    assert!(result.contains("\n=head2 run\n"));
    assert!(result.contains("=cut\n\n  sub run { 42 }\n"));
}

#[test]
fn pl304_preserves_crlf_in_the_inserted_stub() {
    let source = MODULE.replace('\n', "\r\n");
    let diag = first_pl304(&source);
    let actions = actions_for(&source, &[diag]);
    assert_eq!(actions.len(), 1);
    let result = edited(&source, &actions[0]);
    assert!(result.contains("\r\n=head2 run\r\n\r\nDescription.\r\n\r\n=cut\r\n\r\n"));
    assert!(!result.replace("\r\n", "").contains('\n'));
}

#[test]
fn pl304_wrong_or_malformed_ranges_have_no_edit() {
    let source = format!("{MODULE}my $text = \"é\";\n");
    let valid = first_pl304(&source);
    let unicode_start = must_some(source.find('é'));
    for range in [
        (valid.range.0 + 1, valid.range.1),
        (valid.range.0, valid.range.0),
        (valid.range.1, valid.range.0),
        (source.len() + 1, source.len() + 2),
        (unicode_start + 1, unicode_start + 2),
    ] {
        assert!(actions_for(&source, &[diagnostic(range, "PL304")]).is_empty(), "range {range:?}");
    }
}

#[test]
fn pl304_unexported_or_already_documented_sub_rejects_a_stale_finding() {
    let stale = first_pl304(MODULE);
    let unexported = MODULE.replace("qw(run)", "qw(noo)");
    assert!(actions_for(&unexported, &[stale]).is_empty());

    let documented = format!("=head2 run\n\nExisting documentation.\n\n=cut\n\n{MODULE}");
    let start = must_some(documented.find("sub run"));
    let end = start + "sub run { 42 }".len();
    assert!(actions_for(&documented, &[diagnostic((start, end), "PL304")]).is_empty());
}

#[test]
fn pl304_export_without_local_sub_and_inline_declaration_are_refused() {
    let no_sub = "package M;\nuse Exporter 'import';\nour @EXPORT = qw(run);\n";
    let diag = first_pl304(no_sub);
    assert!(actions_for(no_sub, &[diag]).is_empty(), "export range is not a sub insertion anchor");

    let inline = MODULE.replace(";\nsub run", "; sub run");
    let diag = first_pl304(&inline);
    assert!(
        actions_for(&inline, &[diag]).is_empty(),
        "POD cannot be inserted inside a statement line"
    );
}

#[test]
fn pl304_action_requires_its_own_diagnostic_code() {
    let mut diag = first_pl304(MODULE);
    diag.code = Some("PL409".to_string());
    assert!(actions_for(MODULE, &[diag]).is_empty());
}

#[test]
fn pl304_subroutine_shaped_string_content_is_not_an_insertion_anchor() {
    let source = "package M;\nuse Exporter 'import';\nour @EXPORT = qw(run);\nmy $text = '\nsub run { 42 }\n';\n";
    let start = must_some(source.find("sub run"));
    let end = start + "sub run { 42 }".len();
    assert!(actions_for(source, &[diagnostic((start, end), "PL304")]).is_empty());
}
