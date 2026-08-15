//! Edge cases and regression coverage for `perl-diagnostics`.

use perl_diagnostics::catalog;
use perl_diagnostics::codes::{DiagnosticCategory, DiagnosticCode, DiagnosticTag};
use perl_diagnostics::types::{ByteSpan, Diagnostic, DiagnosticSeverity, RelatedInformation};
use perl_test_must::{must, must_some};

fn span(start: usize, end: usize) -> ByteSpan {
    must(ByteSpan::new(start, end))
}

#[test]
fn edge_case_diagnostic_default_is_valid_compatibility_state() {
    let diagnostic = Diagnostic::default();

    assert_eq!(diagnostic.message, "");
    assert_eq!(diagnostic.code, DiagnosticCode::ParseError);
    assert_eq!(diagnostic.severity, DiagnosticSeverity::Error);
    assert_eq!(diagnostic.range, ByteSpan::EMPTY);
    assert!(diagnostic.related_information.is_none());
    assert!(diagnostic.tags.is_none());
}

#[test]
fn edge_case_related_information_default_is_valid() {
    let information = RelatedInformation::default();

    assert_eq!(information.message, "");
    assert_eq!(information.location, ByteSpan::EMPTY);
}

#[test]
fn edge_case_diagnostic_with_all_fields_populated() {
    let information = RelatedInformation::new("related info", span(10, 20));
    let mut diagnostic = Diagnostic::new(
        DiagnosticCode::SyntaxError,
        DiagnosticSeverity::Warning,
        span(1, 50),
        "Test diagnostic",
    );
    diagnostic.related_information = Some(vec![information]);
    diagnostic.tags = Some(vec![DiagnosticTag::Deprecated]);

    assert_eq!(diagnostic.message, "Test diagnostic");
    assert_eq!(diagnostic.code, DiagnosticCode::SyntaxError);
    assert_eq!(diagnostic.severity, DiagnosticSeverity::Warning);
    assert!(diagnostic.related_information.is_some());
    assert!(diagnostic.tags.is_some());

    let tags = must_some(diagnostic.tags);
    assert_eq!(tags, vec![DiagnosticTag::Deprecated]);
}

#[test]
fn regression_reversed_spans_are_rejected_instead_of_swapped() {
    assert!(ByteSpan::new(50, 1).is_err());
    assert!(Diagnostic::try_new(
        DiagnosticCode::SyntaxError,
        DiagnosticSeverity::Warning,
        50,
        1,
        "Test diagnostic",
    )
    .is_err());
    assert!(RelatedInformation::try_new("related", 20, 10).is_err());
}

#[test]
fn regression_severity_discriminants_match_lsp_values() {
    assert_eq!(DiagnosticSeverity::Error as i32, 1);
    assert_eq!(DiagnosticSeverity::Warning as i32, 2);
    assert_eq!(DiagnosticSeverity::Information as i32, 3);
    assert_eq!(DiagnosticSeverity::Hint as i32, 4);
}

#[test]
fn regression_tag_discriminants_match_lsp_values() {
    assert_eq!(DiagnosticTag::Unnecessary as i32, 1);
    assert_eq!(DiagnosticTag::Deprecated as i32, 2);
}

#[test]
fn edge_case_diagnostic_category_all_variants_accessible() {
    let categories = [
        DiagnosticCategory::Parser,
        DiagnosticCategory::StrictWarnings,
        DiagnosticCategory::PackageModule,
        DiagnosticCategory::Subroutine,
        DiagnosticCategory::BestPractices,
        DiagnosticCategory::Deprecated,
        DiagnosticCategory::Security,
        DiagnosticCategory::Import,
        DiagnosticCategory::Heredoc,
    ];

    assert!(categories.contains(&DiagnosticCategory::Parser));
    assert_ne!(DiagnosticCategory::Parser, DiagnosticCategory::StrictWarnings);
}

#[test]
fn edge_case_diagnostic_code_expected_variants_exist() {
    let codes = [
        DiagnosticCode::ParseError,
        DiagnosticCode::SyntaxError,
        DiagnosticCode::UnexpectedEof,
        DiagnosticCode::MissingStrict,
        DiagnosticCode::MissingWarnings,
        DiagnosticCode::UndefinedVariable,
        DiagnosticCode::UnusedVariable,
        DiagnosticCode::MissingPackageDeclaration,
        DiagnosticCode::DuplicatePackage,
        DiagnosticCode::DuplicateSubroutine,
        DiagnosticCode::MissingReturn,
        DiagnosticCode::BarewordFilehandle,
        DiagnosticCode::TwoArgOpen,
        DiagnosticCode::ImplicitReturn,
        DiagnosticCode::EvalErrorFlow,
    ];

    assert!(codes.contains(&DiagnosticCode::ParseError));
}

#[test]
fn regression_catalog_functions_callable() {
    let _ = (
        catalog::parse_error(),
        catalog::syntax_error(),
        catalog::unexpected_eof(),
        catalog::missing_strict(),
        catalog::missing_warnings(),
        catalog::undefined_var(),
        catalog::unused_var(),
        catalog::duplicate_package(),
        catalog::missing_return(),
    );
}

#[test]
fn regression_diagnostic_meta_dispatcher_function() {
    let metadata = catalog::diagnostic_meta(DiagnosticCode::ParseError);
    assert!(!metadata.code.is_null());
}

#[test]
fn edge_case_severity_copy_semantics() {
    let first = DiagnosticSeverity::Error;
    let second = first;
    assert_eq!(first, second);
}

#[test]
fn edge_case_tag_copy_semantics() {
    let first = DiagnosticTag::Deprecated;
    let second = first;
    assert_eq!(first, second);
}

#[test]
fn edge_case_severity_ordering_complete() {
    let mut severities = [
        DiagnosticSeverity::Hint,
        DiagnosticSeverity::Error,
        DiagnosticSeverity::Information,
        DiagnosticSeverity::Warning,
    ];
    severities.sort();

    assert_eq!(
        severities,
        [
            DiagnosticSeverity::Error,
            DiagnosticSeverity::Warning,
            DiagnosticSeverity::Information,
            DiagnosticSeverity::Hint,
        ]
    );
}

#[test]
fn edge_case_diagnostic_message_various_lengths() {
    let mut empty = Diagnostic::default();
    empty.message = String::new();
    assert_eq!(empty.message, "");

    let long_message = "a".repeat(10_000);
    let mut long = Diagnostic::default();
    long.message = long_message.clone();
    assert_eq!(long.message, long_message);

    let unicode_message = "Error in ñoño → 🚀 ⚠️".to_string();
    let mut unicode = Diagnostic::default();
    unicode.message = unicode_message.clone();
    assert_eq!(unicode.message, unicode_message);
}

#[test]
fn edge_case_diagnostic_multiple_related_information() {
    let first = RelatedInformation::new("First related", span(0, 10));
    let second = RelatedInformation::new("Second related", span(20, 30));
    let third = RelatedInformation::new("Third related", span(40, 50));

    let mut diagnostic = Diagnostic::default();
    diagnostic.related_information = Some(vec![first, second, third]);

    let information = must_some(diagnostic.related_information);
    assert_eq!(information.len(), 3);
    assert_eq!(information[0].message, "First related");
    assert_eq!(information[1].message, "Second related");
    assert_eq!(information[2].message, "Third related");
}

#[test]
fn edge_case_diagnostic_multiple_tags() {
    let mut diagnostic = Diagnostic::default();
    diagnostic.tags = Some(vec![
        DiagnosticTag::Unnecessary,
        DiagnosticTag::Deprecated,
        DiagnosticTag::Unnecessary,
    ]);

    let tags = must_some(diagnostic.tags);
    assert_eq!(tags.len(), 3);
    assert_eq!(tags[0], DiagnosticTag::Unnecessary);
    assert_eq!(tags[1], DiagnosticTag::Deprecated);
    assert_eq!(tags[2], DiagnosticTag::Unnecessary);
}

#[test]
fn regression_note_old_imports_must_fail() {
    let _severity = perl_diagnostics::types::DiagnosticSeverity::Error;
    let _code = perl_diagnostics::codes::DiagnosticCode::ParseError;
    let _metadata = perl_diagnostics::catalog::diagnostic_meta(
        perl_diagnostics::codes::DiagnosticCode::ParseError,
    );
}

#[test]
fn regression_diagnostic_code_copy_semantics() {
    let first = DiagnosticCode::ParseError;
    let second = first;
    assert_eq!(first, second);
}

#[test]
fn edge_case_severity_round_trip_lsp_value() {
    assert_eq!(DiagnosticSeverity::Warning.to_lsp_value(), 2);
}

#[test]
fn edge_case_tag_round_trip_lsp_value() {
    assert_eq!(DiagnosticTag::Deprecated.to_lsp_value(), 2);
}

#[test]
fn regression_severity_partial_ord_chains() {
    let error = DiagnosticSeverity::Error;
    let warning = DiagnosticSeverity::Warning;
    let information = DiagnosticSeverity::Information;

    assert!(error < warning);
    assert!(warning < information);
    assert!(error < information);
    assert!(warning >= error);
    assert!(information >= warning);
}

#[test]
fn regression_diagnostic_category_copy() {
    let first = DiagnosticCategory::Parser;
    let second = first;
    assert_eq!(first, second);
}

#[test]
fn edge_case_diagnostic_debug_representation() {
    let mut diagnostic = Diagnostic::default();
    diagnostic.message = "Debug test".to_string();

    let debug = format!("{:?}", diagnostic);
    assert!(debug.contains("Debug test"));
}

#[test]
fn edge_case_diagnostic_code_debug_representation() {
    assert!(!format!("{:?}", DiagnosticCode::ParseError).is_empty());
}

#[test]
fn edge_case_diagnostic_code_as_str_stable() {
    let first = DiagnosticCode::ParseError.as_str();
    let second = DiagnosticCode::ParseError.as_str();
    assert_eq!(first, second);
    assert_ne!(first, DiagnosticCode::SyntaxError.as_str());
}

#[test]
fn edge_case_diagnostic_code_severity_consistent() {
    let code = DiagnosticCode::ParseError;
    assert_eq!(code.severity(), code.severity());
}

#[test]
fn edge_case_diagnostic_code_category_consistent() {
    assert_eq!(DiagnosticCode::ParseError.category(), DiagnosticCategory::Parser);
}

#[test]
fn edge_case_diagnostic_code_tags_consistent() {
    let code = DiagnosticCode::ParseError;
    assert_eq!(code.tags(), code.tags());
}

#[test]
fn edge_case_diagnostic_clone_and_equality() {
    let first = Diagnostic::new(
        DiagnosticCode::SyntaxError,
        DiagnosticSeverity::Warning,
        span(10, 20),
        "Test",
    );
    assert_eq!(first, first.clone());
}

#[test]
fn edge_case_diagnostic_large_range_values() {
    let mut diagnostic = Diagnostic::default();
    diagnostic.range = span(1_000_000, 2_000_000);

    assert_eq!(diagnostic.range.start(), 1_000_000);
    assert_eq!(diagnostic.range.end(), 2_000_000);
}

#[test]
fn regression_diagnostic_in_collections() {
    let mut first = Diagnostic::default();
    first.message = "First".to_string();
    let mut second = Diagnostic::default();
    second.message = "Second".to_string();

    let diagnostics = vec![first, second];
    assert_eq!(diagnostics.len(), 2);
    assert_eq!(diagnostics[0].message, "First");
    assert_eq!(diagnostics[1].message, "Second");
}

#[test]
fn regression_diagnostic_tag_copy() {
    let first = DiagnosticTag::Unnecessary;
    let second = first;
    assert_eq!(first, second);
}

#[test]
fn edge_case_from_message_phase_scoped_takes_priority_over_use_strict() {
    let strict = "use strict inside a begin block does not enable strict at file scope";
    assert_eq!(
        DiagnosticCode::from_message(strict),
        Some(DiagnosticCode::PhaseScopedStrictPragma)
    );

    let warnings = "use warnings inside a phase block does not enable warnings at file scope";
    assert_eq!(
        DiagnosticCode::from_message(warnings),
        Some(DiagnosticCode::PhaseScopedWarningsPragma)
    );
}

#[test]
fn edge_case_from_message_empty_and_whitespace() {
    assert_eq!(DiagnosticCode::from_message(""), None);
    assert_eq!(DiagnosticCode::from_message("   "), None);
    assert_eq!(DiagnosticCode::from_message("\t\n"), None);
}

#[test]
fn edge_case_parse_code_case_sensitivity() {
    assert_eq!(DiagnosticCode::parse_code("PL001"), Some(DiagnosticCode::ParseError));
    assert_eq!(DiagnosticCode::parse_code("pl001"), None);
    assert_eq!(DiagnosticCode::parse_code("Pl001"), None);
    assert_eq!(DiagnosticCode::parse_code(""), None);
    assert_eq!(DiagnosticCode::parse_code("PL999"), None);
    assert_eq!(DiagnosticCode::parse_code("PC999"), None);
    assert_eq!(DiagnosticCode::parse_code("PL 001"), None);
}

#[test]
fn regression_severity_hash_in_map() {
    use std::collections::HashMap;

    let mut map = HashMap::new();
    map.insert(DiagnosticSeverity::Error, 1);
    map.insert(DiagnosticSeverity::Warning, 2);

    assert_eq!(map.get(&DiagnosticSeverity::Error), Some(&1));
    assert_eq!(map.get(&DiagnosticSeverity::Warning), Some(&2));
}

#[test]
fn regression_tag_hash_in_map() {
    use std::collections::HashMap;

    let mut map = HashMap::new();
    map.insert(DiagnosticTag::Unnecessary, "unnecessary");
    map.insert(DiagnosticTag::Deprecated, "deprecated");

    assert_eq!(map.get(&DiagnosticTag::Unnecessary), Some(&"unnecessary"));
    assert_eq!(map.get(&DiagnosticTag::Deprecated), Some(&"deprecated"));
}
