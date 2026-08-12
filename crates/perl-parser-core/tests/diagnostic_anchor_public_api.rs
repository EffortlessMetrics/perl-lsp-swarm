//! External-crate proof for the parser-owned diagnostic-anchor API.

use perl_parser_core::{
    ParseDiagnosticAnchor, ParseError, ResolvedParseDiagnosticAnchor,
};

#[test]
fn root_exports_support_forward_compatible_location_consumers() {
    let source = "aéz";
    let exact = ParseError::syntax("bad expression", 1);

    assert_eq!(exact.diagnostic_anchor(), ParseDiagnosticAnchor::Exact(1));
    assert_eq!(
        exact.resolved_diagnostic_anchor(source),
        ResolvedParseDiagnosticAnchor::Exact(1)
    );
    assert_eq!(
        ParseError::UnexpectedEof.resolved_diagnostic_anchor(source),
        ResolvedParseDiagnosticAnchor::EndOfInput(source.len())
    );
}

#[test]
fn public_api_preserves_invalid_and_stale_source_states() {
    let middle_of_code_point = ParseError::syntax("inside code point", 2);
    assert_eq!(
        middle_of_code_point.resolved_diagnostic_anchor("aéz"),
        ResolvedParseDiagnosticAnchor::InvalidUtf8Boundary {
            reported: 2,
            source_len: 4,
        }
    );

    let stale = ParseError::syntax("stale", 1);
    assert_eq!(
        stale.resolved_diagnostic_anchor_for_current("abc", "axc"),
        ResolvedParseDiagnosticAnchor::StaleSource {
            parsed_len: 3,
            current_len: 3,
        }
    );
}
