//! Integration tests for PL411 — irrelevant `wantarray()` branch detection.

use std::sync::Arc;

use perl_lsp_rs_core::providers::diagnostics::{Diagnostic, DiagnosticsProvider};
use perl_parser::Parser;

fn diagnostics_for(source: &str) -> Vec<Diagnostic> {
    let output = Parser::new(source).parse_with_recovery();
    let ast = Arc::new(output.ast);
    let provider = DiagnosticsProvider::new();
    provider.get_diagnostics(&ast, &output.diagnostics, source, None)
}

fn pl411(source: &str) -> Vec<Diagnostic> {
    diagnostics_for(source).into_iter().filter(|d| d.code.as_deref() == Some("PL411")).collect()
}

#[test]
fn empty_wantarray_arms_in_sub_emits_pl411() {
    let source = r#"use v5.40;
sub get_data {
    if (wantarray()) {
        # list context
    } else {
        # scalar context
    }
    return 42;
}
"#;
    assert_eq!(pl411(source).len(), 1, "issue #2993 minimal reproducer should emit PL411");
}

#[test]
fn return_wantarray_ternary_is_negative_control() {
    let source = r#"use v5.40;
sub ternary_context {
    return wantarray ? (1, 2) : 42;
}
"#;
    assert!(pl411(source).is_empty(), "legitimate context-sensitive return must not emit PL411");
}

#[test]
fn branch_local_return_is_negative_control() {
    let source = r#"use v5.40;
sub get_data {
    if (wantarray()) {
        return (1, 2, 3);
    } else {
        return "scalar";
    }
}
"#;
    assert!(pl411(source).is_empty());
}
