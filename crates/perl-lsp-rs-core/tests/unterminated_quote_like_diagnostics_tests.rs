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

/// Total published set: exactly one PL1008 terminator plus, at most,
/// PL001 inferred-closer recovery notes. Any other published diagnostic
/// (an expected-'}' / expected-statement cascade at EOF) fails here even
/// when the PL1008 count itself is right.
fn assert_no_cascade(diagnostics: &[Diagnostic]) {
    for diagnostic in diagnostics {
        let is_recovery_note = diagnostic.code.as_deref() == Some("PL001")
            && diagnostic.message.contains("Inferred a missing closing delimiter");
        let is_terminator = diagnostic.code.as_deref() == Some("PL1008");
        assert!(
            is_recovery_note || is_terminator,
            "unexpected published diagnostic beyond terminator + recovery notes: {diagnostic:?}"
        );
        assert!(
            !diagnostic.message.contains("expected '}'")
                && !diagnostic.message.contains("expected statement"),
            "no expected-closer/statement cascade may be published: {diagnostic:?}"
        );
    }
}

#[test]
fn unterminated_m_after_term_publishes_pl1008_at_operator() {
    let source = "my @x = @{['a'm]};";
    let diagnostics = compatibility_diagnostics(source);
    assert_no_cascade(&diagnostics);
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
    assert_no_cascade(&diagnostics);
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
fn missing_s_replacement_after_term_publishes_pl1008_replacement() {
    let source = "my @x = ['a' s{foo}];";
    let diagnostics = compatibility_diagnostics(source);
    assert_no_cascade(&diagnostics);
    let published = with_code(&diagnostics, "PL1008");
    assert_eq!(published.len(), 1, "exactly one PL1008 expected: {diagnostics:#?}");
    assert!(
        published[0].message.contains("Substitution replacement not terminated"),
        "message must name the replacement: {:?}",
        published[0].message
    );
    assert_eq!(published[0].range.0, source.find("s{foo}").unwrap_or(0));
}

#[test]
fn unclosed_tr_replacement_publishes_pl1008_replacement() {
    let source = "my $x = tr{foo}{bar;";
    let diagnostics = compatibility_diagnostics(source);
    assert_no_cascade(&diagnostics);
    let published = with_code(&diagnostics, "PL1008");
    assert_eq!(published.len(), 1, "exactly one PL1008 expected: {diagnostics:#?}");
    assert!(
        published[0].message.contains("Transliteration replacement not terminated"),
        "message must name the replacement: {:?}",
        published[0].message
    );
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
