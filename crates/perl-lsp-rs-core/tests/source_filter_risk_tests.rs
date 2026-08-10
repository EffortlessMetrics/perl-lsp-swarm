//! Integration tests for PL702 — source-filter module warning.
//!
//! These tests drive `DiagnosticsProvider::get_diagnostics` end-to-end to
//! ensure the lint is registered and surfaces the parser's precomputed
//! `has_filter_risk` flag with a stable, filterable code.

use std::sync::Arc;

use perl_lsp_rs_core::providers::diagnostics::{Diagnostic, DiagnosticsProvider};
use perl_parser::Parser;

fn diagnostics_for(source: &str) -> Vec<Diagnostic> {
    let output = Parser::new(source).parse_with_recovery();
    let ast = Arc::new(output.ast);
    let provider = DiagnosticsProvider::new();
    provider.get_diagnostics(&ast, &output.diagnostics, source, None)
}

fn pl702(source: &str) -> Vec<Diagnostic> {
    diagnostics_for(source).into_iter().filter(|d| d.code.as_deref() == Some("PL702")).collect()
}

#[test]
fn use_filter_simple_warns() {
    let diags = pl702("use Filter::Simple;\n");
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("Filter::Simple")
                && d.message.contains("source filter module")),
        "expected PL702 for `use Filter::Simple;`, got: {diags:?}"
    );
}

#[test]
fn use_filter_util_call_warns() {
    assert!(
        !pl702("use Filter::Util::Call;\n").is_empty(),
        "expected PL702 for `use Filter::Util::Call;`"
    );
}

#[test]
fn ordinary_modules_do_not_warn() {
    let diags = pl702("use strict;\nuse warnings;\nuse List::Util qw(sum);\n");
    assert!(diags.is_empty(), "ordinary modules must not raise PL702, got: {diags:?}");
}

#[test]
fn filter_lookalike_name_does_not_warn() {
    // `Filter::Simpleton` is not on the known-filter list; must not fire.
    let diags = pl702("use Filter::Simpleton;\n");
    assert!(diags.is_empty(), "unknown Filter::* module must not raise PL702, got: {diags:?}");
}
