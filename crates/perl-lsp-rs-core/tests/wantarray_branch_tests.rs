//! Integration tests for PL411 — irrelevant `wantarray()` branch detection.

// Fixture helper below uses `.expect` on fixture-internal needles, matching
// the crate's other integration tests (pre-existing #18698 omission; the
// `-D warnings` gate could not reach this target while CI clippy was dark).
#![allow(clippy::expect_used, clippy::panic)]

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
    let diags = pl411(source);
    assert_eq!(diags.len(), 1, "issue #2993 minimal reproducer should emit PL411");
    let needle = "if (wantarray())";
    let cond_start = source.find(needle).expect("fixture should contain bare wantarray if");
    let cond_end = cond_start + needle.len();
    let (start, end) = (diags[0].range.0, diags[0].range.1);
    assert!(
        start <= cond_start && end >= cond_end,
        "PL411 range {start}..{end} should cover conditional `{needle}`"
    );
    let covered = source.get(start..end).expect("range in bounds");
    assert!(
        covered.contains("if") && covered.contains("wantarray"),
        "diagnostic span should anchor on the wantarray conditional: {covered:?}"
    );
}

#[test]
fn empty_wantarray_arms_in_eval_emits_pl411() {
    let source = r#"use v5.40;
eval {
    if (wantarray()) {
    } else {
    }
    1;
};
"#;
    assert_eq!(pl411(source).len(), 1, "eval blocks use the same PL411 scan path as subs");
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
