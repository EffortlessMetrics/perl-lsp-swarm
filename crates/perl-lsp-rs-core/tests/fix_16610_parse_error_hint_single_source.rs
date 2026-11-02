//! #16610: LSP diagnostics delegate to the parser-owned hint table.

use std::sync::Arc;

use perl_lsp_rs_core::providers::diagnostics::{
    DiagnosticsProvider, build_parse_error_hint, build_parse_error_hint_with_source,
};
use perl_parser::Parser;
use perl_parser_core::syntax::error::ParseError;
use perl_tdd_support::must;

#[test]
fn lsp_hint_delegates_to_parser_core_table() {
    let error = ParseError::unexpected("expression", "unknown token", 10);
    let source = "my $m = $s =~ m/abc;\n";
    let core = perl_parser_core::syntax::error::build_parse_error_hint_with_source(
        &error,
        "",
        Some(source),
    );
    let lsp = build_parse_error_hint_with_source(&error, "", source);
    assert_eq!(core, lsp);
}

#[test]
fn lsp_hint_without_source_matches_parser_core() {
    let error = ParseError::unexpected(";", "end of input", 5);
    let core = perl_parser_core::syntax::error::build_parse_error_hint(&error, "");
    let lsp = build_parse_error_hint(&error, "");
    assert_eq!(core, lsp);
}

#[test]
fn ast_present_parse_error_hint_uses_source_for_unterminated_regex() {
    let source = "use strict;\nuse warnings;\nmy $s = \"abc\";\nmy $m = $s =~ m/abc;\n";
    let mut parser = Parser::new(source);
    let ast = must(parser.parse());
    let errors = parser.errors().to_vec();
    let diagnostics =
        DiagnosticsProvider::new().get_diagnostics(&Arc::new(ast), &errors, source, None);
    let suggestion = diagnostics
        .iter()
        .flat_map(|diag| diag.related_information.iter())
        .map(|info| info.message.as_str())
        .find(|msg| msg.contains("Suggestion:"))
        .expect("parse diagnostic should carry a suggestion");
    assert!(
        suggestion.contains("Search pattern not terminated"),
        "AST-present LSP path must match --check for unterminated m//: {suggestion}"
    );
}
