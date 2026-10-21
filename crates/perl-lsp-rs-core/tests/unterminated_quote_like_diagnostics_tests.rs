//! LSP publication of unterminated quote-like diagnostics (#16608 class 2).

use std::sync::Arc;

use perl_lsp_rs_core::providers::diagnostics::{
    Diagnostic, DiagnosticSeverity, DiagnosticsProvider,
};
use perl_parser::Parser;

fn compatibility_diagnostics(source: &str) -> Vec<Diagnostic> {
    let output = Parser::new(source).parse_with_recovery();
    let ast = Arc::new(output.ast);
    DiagnosticsProvider::new().get_diagnostics(&ast, &output.diagnostics, source, None)
}

fn with_code<'a>(diagnostics: &'a [Diagnostic], code: &str) -> Vec<&'a Diagnostic> {
    diagnostics.iter().filter(|diagnostic| diagnostic.code.as_deref() == Some(code)).collect()
}

#[test]
fn unterminated_m_after_term_publishes_pl1008_at_operator() {
    let source = "my @x = @{['a'm]};";
    let diagnostics = compatibility_diagnostics(source);
    let published = with_code(&diagnostics, "PL1008");
    assert_eq!(published.len(), 1, "exactly one PL1008 expected: {diagnostics:#?}");
    assert_eq!(published[0].severity, DiagnosticSeverity::Error);
    assert!(
        published[0].message.contains("Search pattern not terminated"),
        "message must mirror perl: {:?}",
        published[0].message
    );
    let operator = source.find("m]}").unwrap_or(0);
    assert_eq!(
        published[0].range.0, operator,
        "range must anchor at the operator start: {:?}",
        published[0].range
    );
}

#[test]
fn unterminated_q_after_term_publishes_pl1008_at_operator() {
    let source = "my @x = @{['a'q]};";
    let diagnostics = compatibility_diagnostics(source);
    let published = with_code(&diagnostics, "PL1008");
    assert_eq!(published.len(), 1, "exactly one PL1008 expected: {diagnostics:#?}");
    assert!(
        published[0].message.contains("Can't find string terminator"),
        "message must mirror perl: {:?}",
        published[0].message
    );
    assert_eq!(published[0].range.0, source.find("q]}").unwrap_or(0));
}

#[test]
fn closed_regex_list_element_publishes_no_pl1008() {
    let source = "my @x = ['a', m/foo/];";
    let diagnostics = compatibility_diagnostics(source);
    assert!(
        with_code(&diagnostics, "PL1008").is_empty(),
        "closed regex must stay silent: {diagnostics:#?}"
    );
}
