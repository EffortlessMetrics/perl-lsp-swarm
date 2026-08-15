//! Comprehensive unit tests for `perl_diagnostics::types`.

use perl_diagnostics::types::{
    ByteSpan, Diagnostic, DiagnosticSeverity, DiagnosticTag, RelatedInformation,
};
use perl_test_must::must;

fn span(start: usize, end: usize) -> ByteSpan {
    must(ByteSpan::new(start, end))
}

// ---------------------------------------------------------------------------
// DiagnosticSeverity (re-exported from codes::)
// ---------------------------------------------------------------------------

#[test]
fn severity_discriminant_values() {
    assert_eq!(DiagnosticSeverity::Error as u8, 1);
    assert_eq!(DiagnosticSeverity::Warning as u8, 2);
    assert_eq!(DiagnosticSeverity::Information as u8, 3);
    assert_eq!(DiagnosticSeverity::Hint as u8, 4);
}

#[test]
fn severity_debug_format() {
    assert!(format!("{:?}", DiagnosticSeverity::Error).contains("Error"));
}

#[test]
fn severity_clone_and_copy() {
    let severity = DiagnosticSeverity::Warning;
    let copied = severity;
    assert_eq!(severity, copied);
}

#[test]
fn severity_equality() {
    assert_eq!(DiagnosticSeverity::Error, DiagnosticSeverity::Error);
    assert_ne!(DiagnosticSeverity::Error, DiagnosticSeverity::Warning);
}

#[test]
fn severity_ordering() {
    assert!(DiagnosticSeverity::Error < DiagnosticSeverity::Warning);
    assert!(DiagnosticSeverity::Warning < DiagnosticSeverity::Information);
    assert!(DiagnosticSeverity::Information < DiagnosticSeverity::Hint);
}

#[test]
fn severity_ord_is_total() {
    let mut levels = vec![
        DiagnosticSeverity::Hint,
        DiagnosticSeverity::Error,
        DiagnosticSeverity::Information,
        DiagnosticSeverity::Warning,
    ];
    levels.sort();
    assert_eq!(
        levels,
        vec![
            DiagnosticSeverity::Error,
            DiagnosticSeverity::Warning,
            DiagnosticSeverity::Information,
            DiagnosticSeverity::Hint,
        ]
    );
}

// ---------------------------------------------------------------------------
// DiagnosticTag (re-exported from codes::)
// ---------------------------------------------------------------------------

#[test]
fn tag_debug_format() {
    assert_eq!(format!("{:?}", DiagnosticTag::Unnecessary), "Unnecessary");
    assert_eq!(format!("{:?}", DiagnosticTag::Deprecated), "Deprecated");
}

#[test]
fn tag_clone_and_copy() {
    let tag = DiagnosticTag::Deprecated;
    let copied = tag;
    assert_eq!(tag, copied);
}

#[test]
fn tag_equality() {
    assert_eq!(DiagnosticTag::Unnecessary, DiagnosticTag::Unnecessary);
    assert_ne!(DiagnosticTag::Unnecessary, DiagnosticTag::Deprecated);
}

// ---------------------------------------------------------------------------
// ByteSpan
// ---------------------------------------------------------------------------

#[test]
fn byte_span_construction_and_accessors() {
    let range = span(10, 20);

    assert_eq!(range.start(), 10);
    assert_eq!(range.end(), 20);
    assert_eq!(range.len(), 10);
    assert!(!range.is_empty());
    assert_eq!(range.to_range(), 10..20);
}

#[test]
fn byte_span_zero_width_is_deliberate() {
    let range = span(10, 10);

    assert!(range.is_empty());
    assert_eq!(range.len(), 0);
}

#[test]
fn byte_span_rejects_reversal() {
    assert!(ByteSpan::new(20, 10).is_err());
}

#[test]
fn byte_span_half_open_contains_and_overlap() {
    let outer = span(10, 20);
    let nested = span(12, 18);
    let adjacent = span(20, 25);
    let overlapping = span(18, 25);

    assert!(outer.contains(10));
    assert!(!outer.contains(20));
    assert!(outer.contains_span(nested));
    assert!(!outer.overlaps(adjacent));
    assert!(outer.overlaps(overlapping));
    assert_eq!(outer.intersection(overlapping), Some(span(18, 20)));
}

// ---------------------------------------------------------------------------
// RelatedInformation
// ---------------------------------------------------------------------------

#[test]
fn related_info_construction() {
    let info = RelatedInformation::new("see declaration here", span(10, 20));
    assert_eq!(info.location, span(10, 20));
    assert_eq!(info.message, "see declaration here");
}

#[test]
fn related_info_checked_construction() {
    let info = must(RelatedInformation::try_new("see declaration here", 10, 20));
    assert_eq!(info.location, span(10, 20));
    assert!(RelatedInformation::try_new("invalid", 20, 10).is_err());
}

#[test]
fn related_info_debug_format() {
    let info = RelatedInformation::new("note", span(0, 5));
    let debug = format!("{:?}", info);
    assert!(debug.contains("RelatedInformation"));
    assert!(debug.contains("note"));
}

#[test]
fn related_info_clone() {
    let info = RelatedInformation::new("original", span(1, 2));
    assert_eq!(info, info.clone());
}

#[test]
fn related_info_equality() {
    let first = RelatedInformation::new("msg", span(1, 2));
    let same = RelatedInformation::new("msg", span(1, 2));
    let other = RelatedInformation::new("msg", span(3, 4));
    assert_eq!(first, same);
    assert_ne!(first, other);
}

#[test]
fn related_info_default_uses_deliberate_compatibility_span() {
    let info = RelatedInformation::default();
    assert!(info.message.is_empty());
    assert_eq!(info.location, ByteSpan::EMPTY);
}

// ---------------------------------------------------------------------------
// Diagnostic
// ---------------------------------------------------------------------------

fn make_diagnostic() -> Diagnostic {
    Diagnostic::new(
        perl_diagnostics::codes::DiagnosticCode::ParseError,
        DiagnosticSeverity::Error,
        span(0, 10),
        "syntax error",
    )
}

#[test]
fn diagnostic_basic_construction() {
    let diagnostic = make_diagnostic();
    assert_eq!(diagnostic.range, span(0, 10));
    assert_eq!(diagnostic.severity, DiagnosticSeverity::Error);
    assert_eq!(
        diagnostic.code,
        perl_diagnostics::codes::DiagnosticCode::ParseError
    );
    assert_eq!(diagnostic.message, "syntax error");
    assert!(diagnostic.related_information.is_none());
    assert!(diagnostic.tags.is_none());
}

#[test]
fn diagnostic_checked_construction() {
    let diagnostic = must(Diagnostic::try_new(
        perl_diagnostics::codes::DiagnosticCode::ParseError,
        DiagnosticSeverity::Error,
        2,
        8,
        "syntax error",
    ));

    assert_eq!(diagnostic.range, span(2, 8));
    assert!(Diagnostic::try_new(
        perl_diagnostics::codes::DiagnosticCode::ParseError,
        DiagnosticSeverity::Error,
        8,
        2,
        "syntax error",
    )
    .is_err());
}

#[test]
fn diagnostic_default_retains_compatibility_shape() {
    let diagnostic = Diagnostic::default();
    assert_eq!(diagnostic.severity, DiagnosticSeverity::Error);
    assert_eq!(diagnostic.range, ByteSpan::EMPTY);
}

#[test]
fn diagnostic_with_severity_field() {
    let mut diagnostic = Diagnostic::default();
    diagnostic.severity = DiagnosticSeverity::Warning;
    assert_eq!(diagnostic.severity, DiagnosticSeverity::Warning);
}

#[test]
fn diagnostic_with_tags() {
    let mut diagnostic = Diagnostic::default();
    diagnostic.tags = Some(vec![DiagnosticTag::Unnecessary]);
    assert_eq!(diagnostic.tags.as_ref().map(Vec::len), Some(1));
}

#[test]
fn diagnostic_with_related_information() {
    let mut diagnostic = Diagnostic::default();
    diagnostic.related_information = Some(vec![RelatedInformation::new(
        "did you mean 'foo'?",
        span(100, 120),
    )]);
    assert_eq!(diagnostic.related_information.as_ref().map(Vec::len), Some(1));
}

#[test]
fn diagnostic_debug_format() {
    let diagnostic = make_diagnostic();
    let debug = format!("{:?}", diagnostic);
    assert!(debug.contains("Diagnostic"));
    assert!(debug.contains("syntax error"));
}

#[test]
fn diagnostic_clone() {
    let diagnostic = make_diagnostic();
    assert_eq!(diagnostic, diagnostic.clone());
}

#[test]
fn diagnostic_equality_same() {
    assert_eq!(make_diagnostic(), make_diagnostic());
}

#[test]
fn diagnostic_inequality_different_range() {
    let first = make_diagnostic();
    let mut second = make_diagnostic();
    second.range = span(1, 11);
    assert_ne!(first, second);
}

#[test]
fn diagnostic_inequality_different_severity() {
    let first = make_diagnostic();
    let mut second = make_diagnostic();
    second.severity = DiagnosticSeverity::Hint;
    assert_ne!(first, second);
}

#[test]
fn diagnostic_inequality_different_message() {
    let first = make_diagnostic();
    let mut second = make_diagnostic();
    second.message = "different".to_string();
    assert_ne!(first, second);
}

// ---------------------------------------------------------------------------
// Collection behavior and mutable public fields
// ---------------------------------------------------------------------------

#[test]
fn diagnostics_can_be_collected_in_vec() {
    let diagnostics: Vec<Diagnostic> = (0..3_usize)
        .map(|index| {
            let mut diagnostic = Diagnostic::default();
            diagnostic.range = span(index, index + 10);
            diagnostic.severity = DiagnosticSeverity::Warning;
            diagnostic.message = format!("warning {index}");
            diagnostic
        })
        .collect();
    assert_eq!(diagnostics.len(), 3);
    assert_eq!(diagnostics[2].message, "warning 2");
}

#[test]
fn severity_can_be_used_as_sort_key() {
    let mut diagnostics = [
        {
            let mut diagnostic = Diagnostic::default();
            diagnostic.severity = DiagnosticSeverity::Hint;
            diagnostic
        },
        {
            let mut diagnostic = Diagnostic::default();
            diagnostic.severity = DiagnosticSeverity::Error;
            diagnostic
        },
    ];
    diagnostics.sort_by_key(|diagnostic| diagnostic.severity);
    assert_eq!(diagnostics[0].severity, DiagnosticSeverity::Error);
    assert_eq!(diagnostics[1].severity, DiagnosticSeverity::Hint);
}

#[test]
fn diagnostic_fields_are_mutable_without_permitting_reversed_ranges() {
    let mut diagnostic = make_diagnostic();
    diagnostic.range = span(100, 200);
    diagnostic.severity = DiagnosticSeverity::Hint;
    diagnostic.message = "updated".to_string();
    diagnostic.related_information = Some(vec![RelatedInformation::new("added", ByteSpan::EMPTY)]);
    diagnostic.tags = Some(vec![DiagnosticTag::Deprecated]);

    assert_eq!(diagnostic.range, span(100, 200));
    assert_eq!(diagnostic.severity, DiagnosticSeverity::Hint);
    assert_eq!(diagnostic.message, "updated");
    assert_eq!(diagnostic.related_information.as_ref().map(Vec::len), Some(1));
    assert_eq!(diagnostic.tags.as_ref().map(Vec::len), Some(1));
}
