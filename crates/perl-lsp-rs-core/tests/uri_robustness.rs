//! Robustness tests for `parse_uri`: empty, whitespace-only, and BOM-only inputs
//! must never return an empty-string URI — they must fall back to `fallback_uri()`.
//!
//! These tests document the bug fixed in #844: `lsp_types::Uri` silently accepts
//! `""` as a valid parse, so the empty-check must happen before the `match`.

use perl_lsp_rs_core::uri::parse_uri;

/// An empty string must return the fallback URI, not `""`.
#[test]
fn parse_uri_empty_string_returns_fallback() {
    let uri = parse_uri("");
    assert!(
        !uri.as_str().is_empty(),
        "parse_uri(\"\") must not return an empty URI; got {:?}",
        uri.as_str()
    );
    assert!(
        uri.as_str().parse::<lsp_types::Uri>().is_ok(),
        "fallback URI must itself be valid; got {:?}",
        uri.as_str()
    );
}

/// A whitespace-only string must return the fallback URI, not `""`.
#[test]
fn parse_uri_whitespace_only_returns_fallback() {
    for ws in ["   ", "\t", "\n", "  \t\n  "] {
        let uri = parse_uri(ws);
        assert!(
            !uri.as_str().is_empty(),
            "parse_uri({ws:?}) must not return an empty URI; got {:?}",
            uri.as_str()
        );
        assert!(
            uri.as_str().parse::<lsp_types::Uri>().is_ok(),
            "fallback URI must itself be valid for input {ws:?}; got {:?}",
            uri.as_str()
        );
    }
}

/// A BOM-only string (no URI content after stripping) must return the fallback URI.
#[test]
fn parse_uri_bom_only_returns_fallback() {
    let bom_only = "\u{feff}";
    let uri = parse_uri(bom_only);
    assert!(
        !uri.as_str().is_empty(),
        "parse_uri(BOM-only) must not return an empty URI; got {:?}",
        uri.as_str()
    );
    assert!(
        uri.as_str().parse::<lsp_types::Uri>().is_ok(),
        "fallback URI must itself be valid for BOM-only input; got {:?}",
        uri.as_str()
    );
}
