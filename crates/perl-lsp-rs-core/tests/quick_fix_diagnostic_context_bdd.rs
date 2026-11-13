//! BDD: typed diagnostic context is authoritative for missing-semicolon fixes (#8396).

use perl_diagnostics::codes::DiagnosticCode;
use perl_lsp_rs_core::providers::code_actions::{CodeAction, CodeActionsProvider};
use perl_lsp_rs_core::providers::diagnostics::ParseErrorFixKind;
use perl_lsp_rs_core::providers::diagnostics::{Diagnostic, DiagnosticSeverity};
use perl_parser::Parser;
use perl_tdd_support::must;

fn semicolon_actions(actions: &[CodeAction]) -> Vec<&CodeAction> {
    actions.iter().filter(|action| action.title == "Add missing semicolon").collect()
}

fn actions_for_diagnostic(source: &str, diagnostic: Diagnostic) -> Vec<CodeAction> {
    let mut parser = Parser::new(source);
    let ast = must(parser.parse());
    CodeActionsProvider::new(source.to_string()).get_code_actions(
        &ast,
        (0, source.len()),
        &[diagnostic],
    )
}

#[test]
fn reworded_message_does_not_change_applicability_when_metadata_present() {
    let source = "my $x = 1\n";
    let diagnostic = Diagnostic {
        range: (0, 9),
        severity: DiagnosticSeverity::Error,
        code: Some(DiagnosticCode::ParseError.as_str().to_string()),
        message: "Editor-facing wording only — no semicolon hint".to_string(),
        related_information: Vec::new(),
        tags: Vec::new(),
        suggestion: None,
        fixable: true,
        critic_observation: None,
        parse_error_fix_kind: Some(ParseErrorFixKind::MissingSemicolon),
    };

    let actions = actions_for_diagnostic(source, diagnostic);
    assert_eq!(semicolon_actions(&actions).len(), 1);
}

#[test]
fn near_miss_similar_prose_wrong_code_does_not_authorize_semicolon_fix() {
    let source = "my $x = 1\n";
    let diagnostic = Diagnostic {
        range: (0, 9),
        severity: DiagnosticSeverity::Error,
        code: Some(DiagnosticCode::UnquotedBareword.as_str().to_string()),
        message: "Missing semicolon after statement".to_string(),
        related_information: Vec::new(),
        tags: Vec::new(),
        suggestion: None,
        fixable: true,
        critic_observation: None,
        parse_error_fix_kind: None,
    };

    let actions = actions_for_diagnostic(source, diagnostic);
    assert!(
        semicolon_actions(&actions).is_empty(),
        "prose alone must not authorize edits when code/metadata disagree"
    );
}

#[test]
fn provider_route_uses_stored_parse_error_kind_not_message() {
    let source = "my $x = 1\n";
    let diagnostic = Diagnostic {
        range: (0, 9),
        severity: DiagnosticSeverity::Error,
        code: Some(DiagnosticCode::ParseError.as_str().to_string()),
        message: "Presentation-only".to_string(),
        related_information: Vec::new(),
        tags: Vec::new(),
        suggestion: None,
        fixable: true,
        critic_observation: None,
        parse_error_fix_kind: Some(ParseErrorFixKind::MissingSemicolon),
    };

    let actions = actions_for_diagnostic(source, diagnostic);
    assert_eq!(semicolon_actions(&actions).len(), 1);
}
