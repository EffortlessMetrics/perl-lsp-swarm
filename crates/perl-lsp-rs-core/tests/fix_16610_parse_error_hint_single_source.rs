//! #16610: LSP diagnostics delegate to the parser-owned hint table.

use perl_lsp_rs_core::providers::diagnostics::{
    build_parse_error_hint, build_parse_error_hint_with_source,
};
use perl_parser_core::syntax::error::ParseError;

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
