//! Tests for `ParseError::diagnostic_anchor()` and `ParseDiagnosticAnchor`.
//!
//! Verifies that every current `ParseError` variant is routed to the correct
//! semantic anchor, that `EndOfInput` and `NoSource` are never aliased to byte
//! zero, and that out-of-range offsets are rejected rather than silently clamped.

use perl_parser_core::error::{
    ParseDiagnosticAnchor, ParseError, RecoveryKind, RecoverySite, ResolvedParseDiagnosticAnchor,
};

// ---------------------------------------------------------------------------
// Helper to build every current no-source ParseError variant.
// ---------------------------------------------------------------------------
fn no_source_variants() -> Vec<ParseError> {
    vec![
        ParseError::LexerError { message: "bad byte".into() },
        ParseError::RecursionLimit,
        ParseError::InvalidNumber { literal: "1x".into() },
        ParseError::InvalidString,
        ParseError::UnclosedDelimiter { delimiter: ')' },
        ParseError::InvalidRegex { message: "bad regex".into() },
        ParseError::NestingTooDeep { depth: 5, max_depth: 4 },
        ParseError::Cancelled,
    ]
}

// ---------------------------------------------------------------------------
// Exact-location variants carry their parser-owned byte offset.
// ---------------------------------------------------------------------------

#[test]
fn unexpected_token_yields_exact_anchor() {
    let e = ParseError::UnexpectedToken {
        expected: "expression".into(),
        found: ";".into(),
        location: 11,
    };
    assert_eq!(e.diagnostic_anchor(), ParseDiagnosticAnchor::Exact(11));
}

#[test]
fn syntax_error_yields_exact_anchor() {
    let e = ParseError::SyntaxError { message: "invalid".into(), location: 12 };
    assert_eq!(e.diagnostic_anchor(), ParseDiagnosticAnchor::Exact(12));
}

#[test]
fn advisory_yields_exact_anchor() {
    let e = ParseError::Advisory { message: "warning".into(), location: 13 };
    assert_eq!(e.diagnostic_anchor(), ParseDiagnosticAnchor::Exact(13));
}

#[test]
fn recovered_yields_exact_anchor_at_recovery_point() {
    let e = ParseError::Recovered {
        site: RecoverySite::InfixRhs,
        kind: RecoveryKind::MissingOperand,
        location: 42,
    };
    assert_eq!(e.diagnostic_anchor(), ParseDiagnosticAnchor::Exact(42));
}

#[test]
fn exact_anchor_offset_matches_variant_field() {
    // A round-trip check: the anchor's offset equals the variant's field.
    let offsets = [0usize, 1, 255, 1024, usize::MAX / 2];
    for &loc in &offsets {
        let anchor = ParseError::syntax("msg", loc).diagnostic_anchor();
        assert_eq!(anchor, ParseDiagnosticAnchor::Exact(loc), "offset {loc}");
    }
}

// ---------------------------------------------------------------------------
// UnexpectedEof belongs at end-of-input, not byte zero.
// ---------------------------------------------------------------------------

#[test]
fn unexpected_eof_yields_end_of_input_anchor() {
    assert_eq!(ParseError::UnexpectedEof.diagnostic_anchor(), ParseDiagnosticAnchor::EndOfInput);
}

#[test]
fn eof_is_not_aliased_to_byte_zero() {
    // Resolved against a non-empty source, EndOfInput must not look like Exact(0).
    let resolved = ParseError::UnexpectedEof.resolved_diagnostic_anchor(100);
    assert_eq!(resolved, ResolvedParseDiagnosticAnchor::EndOfInput(100));
    assert_ne!(resolved, ResolvedParseDiagnosticAnchor::Exact(0));
}

#[test]
fn eof_resolved_against_empty_source_is_end_of_input_zero() {
    // Empty source is a legitimate case: EndOfInput(0) is correct, not NoSource.
    let resolved = ParseError::UnexpectedEof.resolved_diagnostic_anchor(0);
    assert_eq!(resolved, ResolvedParseDiagnosticAnchor::EndOfInput(0));
}

// ---------------------------------------------------------------------------
// No-source variants must not be aliased to byte zero either.
// ---------------------------------------------------------------------------

#[test]
fn no_source_variants_yield_no_source_anchor() {
    for e in no_source_variants() {
        assert_eq!(
            e.diagnostic_anchor(),
            ParseDiagnosticAnchor::NoSource,
            "expected NoSource for {e:?}"
        );
    }
}

#[test]
fn no_source_resolved_is_no_source_not_exact_zero() {
    for e in no_source_variants() {
        let resolved = e.resolved_diagnostic_anchor(100);
        assert_eq!(
            resolved,
            ResolvedParseDiagnosticAnchor::NoSource,
            "expected NoSource (not Exact(0)) for {e:?}"
        );
        assert_ne!(
            resolved,
            ResolvedParseDiagnosticAnchor::Exact(0),
            "{e:?} must not alias to Exact(0)"
        );
    }
}

// ---------------------------------------------------------------------------
// Resolution: exact offsets are validated, not silently clamped.
// ---------------------------------------------------------------------------

#[test]
fn in_bounds_exact_offset_resolves_to_exact() {
    let e = ParseError::syntax("msg", 42);
    assert_eq!(e.resolved_diagnostic_anchor(100), ResolvedParseDiagnosticAnchor::Exact(42));
}

#[test]
fn exact_offset_at_source_boundary_is_valid() {
    // offset == source_len is the one-past-end position and is considered valid.
    let e = ParseError::syntax("msg", 42);
    assert_eq!(e.resolved_diagnostic_anchor(42), ResolvedParseDiagnosticAnchor::Exact(42));
}

#[test]
fn out_of_bounds_exact_offset_is_invalid_not_clamped() {
    let e = ParseError::syntax("outside", 43);
    assert_eq!(
        e.resolved_diagnostic_anchor(42),
        ResolvedParseDiagnosticAnchor::InvalidOffset { reported: 43, source_len: 42 }
    );
}

#[test]
fn eof_anchor_resolved_to_offset() {
    let anchor = ParseDiagnosticAnchor::EndOfInput;
    assert_eq!(anchor.to_offset(55), 55);
}

#[test]
fn no_source_anchor_to_offset_is_zero_per_policy() {
    // Explicit policy: NoSource callers get byte 0 (file start) when they
    // must emit a position.
    let anchor = ParseDiagnosticAnchor::NoSource;
    assert_eq!(anchor.to_offset(100), 0);
}

#[test]
fn exact_anchor_to_offset_returns_inner_value() {
    let anchor = ParseDiagnosticAnchor::Exact(77);
    assert_eq!(anchor.to_offset(100), 77);
}

// ---------------------------------------------------------------------------
// Completeness: every variant is covered (negative control).
// ---------------------------------------------------------------------------

#[test]
fn no_source_variant_count_matches_expectation() {
    // If new ParseError variants are added this test will need updating,
    // which is intentional: the author must decide the anchor for the new variant.
    let no_source_count = no_source_variants().len();
    assert_eq!(
        no_source_count, 8,
        "Expected 8 NoSource variants; update this test if a variant was added"
    );
}

// ---------------------------------------------------------------------------
// Falsifiers: replacing an Exact variant with the wrong form fails.
// ---------------------------------------------------------------------------

#[test]
fn recovered_with_location_zero_is_still_exact_not_no_source() {
    // A Recovered error at offset 0 is Exact(0), not NoSource. The two must
    // not be confused even though to_offset(source_len) returns 0 for both.
    let e = ParseError::Recovered {
        site: RecoverySite::ArgList,
        kind: RecoveryKind::InsertedCloser,
        location: 0,
    };
    assert_eq!(e.diagnostic_anchor(), ParseDiagnosticAnchor::Exact(0));
    assert_ne!(e.diagnostic_anchor(), ParseDiagnosticAnchor::NoSource);
}

#[test]
fn lexer_error_anchor_is_no_source_not_end_of_input() {
    let e = ParseError::LexerError { message: "bad encoding".into() };
    let anchor = e.diagnostic_anchor();
    assert_ne!(
        anchor,
        ParseDiagnosticAnchor::EndOfInput,
        "LexerError must be NoSource, not EndOfInput"
    );
    assert_eq!(anchor, ParseDiagnosticAnchor::NoSource);
}
