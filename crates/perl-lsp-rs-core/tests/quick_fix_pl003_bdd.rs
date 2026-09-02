//! BDD coverage for the bounded PL003 (UnexpectedEof) quick fixes.

use perl_lsp_rs_core::providers::code_actions::{CodeAction, CodeActionsProvider};
use perl_lsp_rs_core::providers::diagnostics::{Diagnostic, DiagnosticSeverity};
use perl_parser::Parser;
use perl_tdd_support::{must, must_some};

fn diagnostic(source: &str, message: &str) -> Diagnostic {
    Diagnostic {
        range: (source.len(), source.len()),
        severity: DiagnosticSeverity::Error,
        code: Some("PL003".to_string()),
        message: message.to_string(),
        related_information: Vec::new(),
        tags: Vec::new(),
        suggestion: None,
        fixable: false,
        critic_observation: None,
    }
}

fn actions_for(source: &str, message: &str) -> Vec<CodeAction> {
    let mut parser = Parser::new(source);
    let ast = must(parser.parse());
    CodeActionsProvider::new(source.to_string())
        .get_diagnostic_quick_fixes(&ast, &[diagnostic(source, message)])
}

fn apply_first(source: &str, action: &CodeAction) -> String {
    let edit = &action.edit.changes[0];
    let mut result = source.to_string();
    result.replace_range(edit.location.start..edit.location.end, &edit.new_text);
    result
}

#[test]
fn pl003_routes_to_a_bounded_closing_brace_fix() {
    let source = "sub greet {\n    print 'hello';\n";
    let actions = actions_for(source, "The file ended unexpectedly");

    let action = must_some(actions.iter().find(|action| action.title.contains("closing brace")));
    assert_eq!(apply_first(source, action), format!("{source}\n}}"));
}

#[test]
fn pl003_inserts_the_brace_at_end_of_source() {
    let source = "if ($ok) { print 'yes'; }\nsub greet {";
    let actions = actions_for(source, "Unexpected end of input");

    let action = must_some(actions.iter().find(|action| action.title.contains("closing brace")));
    let edit = &action.edit.changes[0];
    assert_eq!((edit.location.start, edit.location.end), (source.len(), source.len()));
    assert_eq!(edit.new_text, "\n}");
}

#[test]
fn pl003_prefers_missing_semicolon_when_message_identifies_it() {
    let source = "my $value = 1";
    let actions = actions_for(source, "Unexpected end of input: missing semicolon");

    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].title, "Add missing semicolon");
    assert_eq!(apply_first(source, &actions[0]), "my $value = 1;");
}

#[test]
fn pl003_missing_semicolon_fix_preserves_trailing_newline() {
    let source = "my $value = 1   \n";
    let actions = actions_for(source, "missing semicolon at end of input");

    assert_eq!(actions.len(), 1);
    assert_eq!(apply_first(source, &actions[0]), "my $value = 1;   \n");
}

#[test]
fn pl003_missing_semicolon_skips_whitespace_only_trailing_lines() {
    let source = "my $value = 1\n   \n\t\n";
    let actions = actions_for(source, "missing semicolon at end of input");

    assert_eq!(actions.len(), 1);
    assert_eq!(apply_first(source, &actions[0]), "my $value = 1;\n   \n\t\n");
}

#[test]
fn pl003_missing_semicolon_preserves_crlf_after_last_content_line() {
    let source = "my $value = 1   \r\n   \r\n";
    let actions = actions_for(source, "missing semicolon at end of input");

    assert_eq!(actions.len(), 1);
    assert_eq!(apply_first(source, &actions[0]), "my $value = 1;   \r\n   \r\n");
}

#[test]
fn pl003_missing_semicolon_does_not_fabricate_action_for_whitespace_only_input() {
    let source = " \t\r\n\n";
    assert!(actions_for(source, "missing semicolon at end of input").is_empty());
}

#[test]
fn pl003_does_not_add_a_brace_for_an_unclosed_parenthesis() {
    let source = "if ($ok";
    let actions = actions_for(source, "Unexpected end of input");

    assert!(actions.is_empty());
}

#[test]
fn pl003_does_not_add_a_brace_inside_an_unterminated_string() {
    let source = "sub greet { my $message = 'hello";
    let actions = actions_for(source, "Unexpected end of input");

    assert!(actions.is_empty());
}

#[test]
fn pl003_does_not_count_regex_braces_as_block_structure() {
    let source = "my $match = /\\{/; if ($ok";
    let actions = actions_for(source, "Unexpected end of input");

    assert!(actions.is_empty());
}

#[test]
fn pl003_does_not_offer_one_brace_for_nested_unclosed_blocks() {
    let source = "sub outer { if ($ok) {";
    let actions = actions_for(source, "Unexpected end of input");

    assert!(actions.is_empty());
}

#[test]
fn other_parse_codes_do_not_receive_the_pl003_eof_fallback() {
    let source = "my $value = 1;\n";
    let mut parser = Parser::new(source);
    let ast = must(parser.parse());
    let diagnostic = Diagnostic {
        code: Some("PL001".to_string()),
        message: "Unexpected end of input".to_string(),
        ..diagnostic(source, "Unexpected end of input")
    };

    let actions = CodeActionsProvider::new(source.to_string())
        .get_diagnostic_quick_fixes(&ast, &[diagnostic]);
    assert!(actions.is_empty());
}
