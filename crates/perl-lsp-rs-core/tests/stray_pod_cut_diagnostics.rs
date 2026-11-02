//! Integration tests for PL412 — a lone `=cut` that opens POD and hides the rest
//! of the file.
//!
//! Issue #18771: perl itself accepts a stray `=cut` with no warning, and the
//! server's model silently drops everything after it. The diagnostic must name
//! the `=cut` line.

use std::sync::Arc;

use perl_lsp_rs_core::providers::diagnostics::{Diagnostic, DiagnosticsProvider};
use perl_parser::Parser;
use perl_tdd_support::must_some;

fn diagnostics_for(source: &str) -> Vec<Diagnostic> {
    let output = Parser::new(source).parse_with_recovery();
    let ast = Arc::new(output.ast);
    DiagnosticsProvider::new().get_diagnostics(&ast, &output.diagnostics, source, None)
}

fn pl412(source: &str) -> Vec<Diagnostic> {
    diagnostics_for(source).into_iter().filter(|d| d.code.as_deref() == Some("PL412")).collect()
}

#[test]
fn stray_cut_after_code_emits_pl412() {
    let source = "use strict;\nsub before { 1 }\n=cut\nsub after { 2 }\n";
    let diags = pl412(source);
    assert_eq!(diags.len(), 1, "a lone =cut must emit PL412: {diags:?}");
    let needle = "=cut";
    let at = must_some(source.find(needle));
    let (start, end) = diags[0].range;
    assert!(
        start <= at && end >= at + needle.len(),
        "PL412 range {start}..{end} should cover the stray =cut"
    );
    assert!(diags[0].message.contains("=cut"), "PL412 must mention =cut: {}", diags[0].message);
    assert!(
        diags[0].message.to_ascii_lowercase().contains("pod"),
        "PL412 must explain that the rest of the file becomes POD: {}",
        diags[0].message
    );
}

#[test]
fn matched_pod_cut_is_silent() {
    let source = "use strict;\n=pod\nDocs here.\n=cut\nsub after { 1 }\n";
    assert!(
        pl412(source).is_empty(),
        "a closing =cut after =pod must not warn: {:?}",
        pl412(source)
    );
}

#[test]
fn head1_cut_pair_is_silent() {
    let source = "package Foo;\n=head1 NAME\n\nFoo - a module\n\n=cut\nsub bar { 1 }\n";
    assert!(
        pl412(source).is_empty(),
        "ordinary =head1 ... =cut documentation must not warn: {:?}",
        pl412(source)
    );
}

#[test]
fn cut_continuing_an_assignment_is_silent() {
    let source = "my $x = 1\n=cut\n";
    assert!(
        pl412(source).is_empty(),
        "statement-continuation =cut is an assignment, not POD: {:?}",
        pl412(source)
    );
}

#[test]
fn cutlery_is_not_a_stray_cut() {
    let source = "use strict;\n=cutlery\nstill pod\n=cut\nsub ok { 1 }\n";
    assert!(
        pl412(source).is_empty(),
        "=cutlery is a distinct POD command, not a stray =cut: {:?}",
        pl412(source)
    );
}

#[test]
fn stray_cut_opens_until_the_next_cut() {
    let source = "use strict;\n=cut\nnot code\n=cut\nsub recovered { 1 }\n";
    let diags = pl412(source);
    assert_eq!(diags.len(), 1, "the opening stray =cut warns once; the closer does not: {diags:?}");
}

#[test]
fn stray_cut_at_start_of_file_emits_pl412() {
    let source = "=cut\nsub hidden { 1 }\n";
    assert_eq!(pl412(source).len(), 1, "a file-leading =cut still opens POD: {:?}", pl412(source));
}

#[test]
fn warning_not_error() {
    let source = "sub before { 1 }\n=cut\nsub after { 2 }\n";
    let diag = must_some(pl412(source).into_iter().next());
    assert_eq!(
        diag.severity,
        perl_diagnostics::codes::DiagnosticSeverity::Warning,
        "perl -c accepts the file, so this is a warning"
    );
}
