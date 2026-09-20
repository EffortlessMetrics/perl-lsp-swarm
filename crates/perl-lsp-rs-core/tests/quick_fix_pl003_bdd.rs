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
    CodeActionsProvider::new(source.to_string()).get_code_actions(
        &ast,
        (0, source.len()),
        &[diagnostic(source, message)],
    )
}

fn apply_first(source: &str, action: &CodeAction) -> String {
    let edit = &action.edit.changes[0];
    let mut result = source.to_string();
    result.replace_range(edit.location.start()..edit.location.end(), &edit.new_text);
    result
}

/// The PL003 EOF-fallback family (closing-brace / missing-semicolon fixes).
///
/// This BDD contract covers the bounded PL003 fallback only. Unrelated
/// providers legitimately add their own actions for these minimal fixture
/// sources — pragma additions fire whenever `use strict`/`use warnings` are
/// absent — so assertions must select the fallback family rather than the
/// global action set (#12798).
fn pl003_fallback_actions(actions: &[CodeAction]) -> Vec<&CodeAction> {
    actions
        .iter()
        .filter(|action| {
            action.title.contains("closing brace") || action.title.contains("missing semicolon")
        })
        .collect()
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
    assert_eq!((edit.location.start(), edit.location.end()), (source.len(), source.len()));
    assert_eq!(edit.new_text, "\n}");
}

#[test]
fn pl003_prefers_missing_semicolon_when_message_identifies_it() {
    let source = "my $value = 1";
    let actions = actions_for(source, "Unexpected end of input: missing semicolon");

    let fallback = pl003_fallback_actions(&actions);
    assert_eq!(fallback.len(), 1);
    assert_eq!(fallback[0].title, "Add missing semicolon");
    assert_eq!(apply_first(source, fallback[0]), "my $value = 1;");
}

#[test]
fn pl003_missing_semicolon_fix_preserves_trailing_newline() {
    let source = "my $value = 1   \n";
    let actions = actions_for(source, "missing semicolon at end of input");

    let fallback = pl003_fallback_actions(&actions);
    assert_eq!(fallback.len(), 1);
    assert_eq!(apply_first(source, fallback[0]), "my $value = 1;   \n");
}

#[test]
fn pl003_semicolon_fix_lands_before_a_trailing_comment() {
    // A semicolon appended after a trailing line comment is swallowed by it
    // (`my $value = 1 # why;` never terminates the statement); the fix must
    // land before the comment (#12803).
    let source = "my $value = 1 # why\n";
    let actions = actions_for(source, "missing semicolon at end of input");

    let fallback = pl003_fallback_actions(&actions);
    assert_eq!(fallback.len(), 1);
    assert_eq!(apply_first(source, fallback[0]), "my $value = 1; # why\n");
}

#[test]
fn pl003_semicolon_fix_does_not_misread_last_index_sigil_as_comment() {
    // `$#array` is the last-index sigil, not a comment opener.
    let source = "my $last = $#array";
    let actions = actions_for(source, "missing semicolon at end of input");

    let fallback = pl003_fallback_actions(&actions);
    assert_eq!(fallback.len(), 1);
    assert_eq!(apply_first(source, fallback[0]), "my $last = $#array;");
}

#[test]
fn pl003_does_not_add_a_brace_for_an_unclosed_parenthesis() {
    let source = "if ($ok";
    let actions = actions_for(source, "Unexpected end of input");

    assert!(
        pl003_fallback_actions(&actions).is_empty(),
        "no PL003 fallback may fire for an unclosed parenthesis: {:?}",
        actions.iter().map(|a| &a.title).collect::<Vec<_>>()
    );
}

#[test]
fn pl003_does_not_add_a_brace_inside_an_unterminated_string() {
    let source = "sub greet { my $message = 'hello";
    let actions = actions_for(source, "Unexpected end of input");

    assert!(
        pl003_fallback_actions(&actions).is_empty(),
        "no PL003 fallback may fire inside an unterminated string: {:?}",
        actions.iter().map(|a| &a.title).collect::<Vec<_>>()
    );
}

#[test]
fn pl003_does_not_count_regex_braces_as_block_structure() {
    let source = "my $match = /\\{/; if ($ok";
    let actions = actions_for(source, "Unexpected end of input");

    assert!(
        pl003_fallback_actions(&actions).is_empty(),
        "no PL003 fallback may count regex braces as block structure: {:?}",
        actions.iter().map(|a| &a.title).collect::<Vec<_>>()
    );
}

#[test]
fn pl003_does_not_offer_one_brace_for_nested_unclosed_blocks() {
    let source = "sub outer { if ($ok) {";
    let actions = actions_for(source, "Unexpected end of input");

    assert!(
        pl003_fallback_actions(&actions).is_empty(),
        "no PL003 fallback may fire for nested unclosed blocks: {:?}",
        actions.iter().map(|a| &a.title).collect::<Vec<_>>()
    );
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

    let actions = CodeActionsProvider::new(source.to_string()).get_code_actions(
        &ast,
        (0, source.len()),
        &[diagnostic],
    );
    assert!(
        pl003_fallback_actions(&actions).is_empty(),
        "PL001 must not receive the PL003 EOF fallback: {:?}",
        actions.iter().map(|a| &a.title).collect::<Vec<_>>()
    );
}
