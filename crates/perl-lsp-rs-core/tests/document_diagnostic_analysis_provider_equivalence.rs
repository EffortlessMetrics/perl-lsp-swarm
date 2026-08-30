//! Provider equivalence proof for #7286: `get_diagnostics_*` with a
//! correctly-built prebuilt [`DocumentDiagnosticAnalysis`] must return
//! diagnostics equal to the existing (no-analysis) method, across a fixture
//! matrix covering the realistic shapes production documents take. This is
//! the central behavior-preservation proof for the analysis-sharing seam.

use std::sync::Arc;

use perl_lsp_rs_core::providers::diagnostics::{
    Diagnostic, DiagnosticsProvider, DocumentDiagnosticAnalysis,
};
use perl_parser::Parser;
use perl_parser::error::ParseError;
use perl_parser_core::Node;

fn parse(source: &str) -> (Arc<Node>, Vec<ParseError>) {
    let output = Parser::new(source).parse_with_recovery();
    (Arc::new(output.ast), output.diagnostics)
}

fn assert_provider_equivalent(label: &str, source: &str) {
    let (ast, parse_errors) = parse(source);
    let provider = DiagnosticsProvider::new();

    let without_analysis: Vec<Diagnostic> =
        provider.get_diagnostics_with_path(&ast, &parse_errors, source, None, &[], None);

    let analysis = DocumentDiagnosticAnalysis::build(&ast, source);
    let with_analysis: Vec<Diagnostic> = provider.get_diagnostics_with_path_with_analysis(
        &ast,
        &parse_errors,
        source,
        None,
        &[],
        None,
        Some(&analysis),
    );

    assert_eq!(
        without_analysis, with_analysis,
        "fixture `{label}`: diagnostics with a prebuilt analysis must equal diagnostics without one"
    );
}

#[test]
fn equivalence_clean_code() {
    assert_provider_equivalent("clean_code", "use strict;\nuse warnings;\nmy $x = 1;\nprint $x;\n");
}

#[test]
fn equivalence_scope_issues() {
    assert_provider_equivalent(
        "scope_issues",
        "sub f {\n    my $unused = 1;\n    print $undeclared;\n    my $x = 1;\n    my $x = 2;\n}\n",
    );
}

#[test]
fn equivalence_strict_pragma_regions() {
    assert_provider_equivalent(
        "strict_pragma_regions",
        "use strict;\nmy $x = 1;\n{\n    no strict 'vars';\n    $y = 2;\n}\nprint $x;\n",
    );
}

#[test]
fn equivalence_packages_subs_imports() {
    assert_provider_equivalent(
        "packages_subs_imports",
        "package My::Module;\nuse strict;\nuse warnings;\nuse Scalar::Util qw(blessed);\n\nsub new {\n    my ($class) = @_;\n    return bless {}, $class;\n}\n\nsub greet {\n    my ($self, $name) = @_;\n    return \"hello $name\";\n}\n\n1;\n",
    );
}

#[test]
fn equivalence_heredocs() {
    assert_provider_equivalent(
        "heredocs",
        "my $text = <<\"END\";\nHello, World!\nThis is a heredoc.\nEND\nprint $text;\n",
    );
}

#[test]
fn equivalence_pod() {
    assert_provider_equivalent(
        "pod",
        "package My::Documented;\n\n=head1 NAME\n\nMy::Documented - an example module\n\n=cut\n\nsub run {\n    my ($self) = @_;\n    return 1;\n}\n\n1;\n",
    );
}

#[test]
fn equivalence_data_section() {
    assert_provider_equivalent(
        "data_section",
        "my $x = 1;\nprint $x;\n\n__DATA__\nsome\nraw\ndata\nhere\n",
    );
}

#[test]
fn equivalence_unicode() {
    assert_provider_equivalent(
        "unicode",
        "use utf8;\nmy $greeting = \"héllo wörld \u{1F600}\";\nprint $greeting;\n",
    );
}

#[test]
fn equivalence_lf_line_endings() {
    assert_provider_equivalent("lf_line_endings", "my $x = 1;\nmy $y = 2;\nprint $x + $y;\n");
}

#[test]
fn equivalence_crlf_line_endings() {
    assert_provider_equivalent(
        "crlf_line_endings",
        "my $x = 1;\r\nmy $y = 2;\r\nprint $x + $y;\r\n",
    );
}

// ── negative controls: a mismatched analysis must never leak (#7286) ──

/// A `DocumentDiagnosticAnalysis` built from a DIFFERENT document, passed to
/// the provider, must not leak that other document's facts. The provider
/// must detect the mismatch (via `matches_source`) and rebuild locally,
/// returning the same diagnostics it would without any prebuilt analysis at
/// all.
#[test]
fn mismatched_analysis_does_not_leak_into_provider_result() {
    let source_a = "sub f { my $unused = 1; }\n";
    let source_b = "my $x = 1;\nprint $x;\n";

    let (ast_a, _) = parse(source_a);
    let mismatched_analysis = DocumentDiagnosticAnalysis::build(&ast_a, source_a);
    assert!(
        !mismatched_analysis.matches_source(source_b),
        "fixture invariant: the two fixture sources must not collide"
    );

    let (ast_b, parse_errors_b) = parse(source_b);
    let provider = DiagnosticsProvider::new();
    let correct: Vec<Diagnostic> =
        provider.get_diagnostics_with_path(&ast_b, &parse_errors_b, source_b, None, &[], None);
    let with_mismatched: Vec<Diagnostic> = provider.get_diagnostics_with_path_with_analysis(
        &ast_b,
        &parse_errors_b,
        source_b,
        None,
        &[],
        None,
        Some(&mismatched_analysis),
    );

    assert_eq!(
        correct, with_mismatched,
        "a mismatched analysis from a different document must never leak into the result -- \
         the provider must rebuild locally and return the same diagnostics as with no prebuilt \
         analysis at all"
    );
}

/// Same-length-but-different-content sources must not be conflated by the
/// freshness guard: a mismatched analysis built from same-length different
/// text must still be rejected and never leak into the provider's result.
#[test]
fn same_length_different_content_analysis_is_rejected_by_provider() {
    let source_a = "my $xxxxx = 1;\n";
    let source_b = "my $yyyyy = 2;\n";
    assert_eq!(source_a.len(), source_b.len(), "fixture invariant: lengths must match");

    let (ast_a, _) = parse(source_a);
    let mismatched_analysis = DocumentDiagnosticAnalysis::build(&ast_a, source_a);
    assert!(!mismatched_analysis.matches_source(source_b));

    let (ast_b, parse_errors_b) = parse(source_b);
    let provider = DiagnosticsProvider::new();
    let correct: Vec<Diagnostic> =
        provider.get_diagnostics_with_path(&ast_b, &parse_errors_b, source_b, None, &[], None);
    let with_mismatched: Vec<Diagnostic> = provider.get_diagnostics_with_path_with_analysis(
        &ast_b,
        &parse_errors_b,
        source_b,
        None,
        &[],
        None,
        Some(&mismatched_analysis),
    );

    assert_eq!(
        correct, with_mismatched,
        "a same-length-but-different-content analysis must never leak into the result"
    );
}
