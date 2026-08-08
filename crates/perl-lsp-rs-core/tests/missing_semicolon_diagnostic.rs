//! #5474 — the two surfaces must agree about a missing statement terminator.
//!
//! `perllsp --check` and the editor read the same `parser.errors()` list, so a
//! diagnostic the parser records is only useful if it also survives the
//! diagnostics provider: `Recovered` variants are deliberately *not* treated as
//! hard blockers there (they keep the lint/scope stack alive), and a filter
//! change could drop this one silently while `--check` still failed.
//!
//! Drives the real parser rather than a synthetic error, so this fails if the
//! parser stops recording the diagnostic, if its offset moves, or if the
//! provider stops surfacing it.

use std::sync::Arc;

use perl_lsp_rs_core::providers::diagnostics::{
    Diagnostic, DiagnosticSeverity, DiagnosticsProvider,
};
use perl_parser::Parser;

/// The issue's reproduction. `perl -c` rejects it: `syntax error … near "print"`.
const MISSING_SEMICOLON: &str = "my $x = 1\nprint \"hi\";\n";

/// Byte offset of `print` — the token that proves the previous statement was
/// never terminated, and where both surfaces must point.
const PRINT_OFFSET: usize = 10;

fn diagnostics(source: &str) -> Vec<Diagnostic> {
    let output = Parser::new(source).parse_with_recovery();
    let ast = Arc::new(output.ast);
    let provider = DiagnosticsProvider::new();
    provider.get_diagnostics(&ast, &output.diagnostics, source, None)
}

#[test]
fn missing_statement_semicolon_reaches_the_editor_as_an_error() {
    let found: Vec<_> = diagnostics(MISSING_SEMICOLON)
        .into_iter()
        .filter(|diagnostic| diagnostic.message.contains("Missing `;`"))
        .collect();

    assert_eq!(
        found.len(),
        1,
        "exactly one missing-terminator diagnostic expected, got: {found:?}"
    );
    let diagnostic = &found[0];
    assert_eq!(
        diagnostic.range.0, PRINT_OFFSET,
        "diagnostic must anchor at the token that starts the unterminated-from statement"
    );
    assert_eq!(
        diagnostic.severity,
        DiagnosticSeverity::Error,
        "valid-Perl-rejecting syntax must not be surfaced as a warning or hint"
    );
}

/// The control: source Perl accepts must stay quiet on this code path, or the
/// editor would light up every file that omits the final `;`.
#[test]
fn permitted_terminator_omissions_produce_no_diagnostic() {
    for source in [
        "use strict;\nuse warnings;\npackage Foo;\nmy $a = 1;\nmy $b = 2;\nprint $a, $b\n",
        "use strict;\nuse warnings;\npackage Foo;\nsub f {\n    my $y = 2;\n    return $y\n}\n1;\n",
    ] {
        let diagnostics = diagnostics(source);
        assert!(
            diagnostics.is_empty(),
            "valid Perl produced diagnostics:\n{source}\ngot: {diagnostics:?}"
        );
    }
}
