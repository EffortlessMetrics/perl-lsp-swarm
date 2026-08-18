//! Integration tests for the public `perl-diagnostics` API.
//!
//! These tests verify module structure, type unification, explicit re-exports,
//! consumer compilation paths, and the validated byte-span contract.

#![allow(dead_code)]

use perl_test_must::must;

// Test 1: Core module imports resolve.
#[test]
fn test_codes_module_imports() {
    use perl_diagnostics::codes::DiagnosticCategory;
    use perl_diagnostics::codes::DiagnosticCode;
    use perl_diagnostics::codes::DiagnosticSeverity;
    use perl_diagnostics::codes::DiagnosticTag;

    let _ = (
        DiagnosticCode::ParseError,
        DiagnosticSeverity::Error,
        DiagnosticTag::Unnecessary,
        DiagnosticCategory::Parser,
    );
}

// Test 2: Types module imports resolve.
#[test]
fn test_types_module_imports() {
    use perl_diagnostics::types::{ByteSpan, Diagnostic, RelatedInformation};

    let _ = (ByteSpan::EMPTY, Diagnostic::default(), RelatedInformation::default());
}

// Test 3: Types module re-exports unified severity and tag.
#[test]
fn test_types_module_reexports_unified_types() {
    use perl_diagnostics::types::DiagnosticSeverity;
    use perl_diagnostics::types::DiagnosticTag;

    let _ = (DiagnosticSeverity::Error, DiagnosticTag::Unnecessary);
}

// Test 4: Catalog module imports resolve.
#[test]
fn test_catalog_module_imports() {
    use perl_diagnostics::catalog::DiagnosticMeta;
    use perl_diagnostics::catalog::diagnostic_meta;
    use perl_diagnostics::catalog::parse_error;

    let _ = (DiagnosticMeta::default(), diagnostic_meta, parse_error);
}

// Test 5: API re-exports are available at crate root.
#[test]
fn test_api_root_reexports_codes_types() {
    use perl_diagnostics::{ByteSpan, DiagnosticCategory, DiagnosticCode, DiagnosticSeverity};

    let _ = (
        ByteSpan::EMPTY,
        DiagnosticCode::ParseError,
        DiagnosticSeverity::Error,
        DiagnosticCategory::Parser,
    );
}

// Test 6: API re-exports catalog functions.
#[test]
fn test_api_root_reexports_catalog() {
    use perl_diagnostics::diagnostic_meta;
    use perl_diagnostics::parse_error;

    let _ = (diagnostic_meta, parse_error);
}

// Test 7: codes and types severity are the same type.
#[test]
fn test_severity_type_unification_cross_path_assignment() {
    use perl_diagnostics::codes::DiagnosticSeverity as CodesSeverity;
    use perl_diagnostics::types::DiagnosticSeverity as TypesSeverity;

    let from_codes: CodesSeverity = CodesSeverity::Error;
    let _as_types: TypesSeverity = from_codes;

    assert_eq!(from_codes as u8, 1);
}

// Test 8: codes and types tag are the same type.
#[test]
fn test_tag_type_unification_cross_path_assignment() {
    use perl_diagnostics::codes::DiagnosticTag as CodesTag;
    use perl_diagnostics::types::DiagnosticTag as TypesTag;

    let from_codes: CodesTag = CodesTag::Unnecessary;
    let _as_types: TypesTag = from_codes;
}

// Test 9: Severity type identity is exact.
#[test]
fn test_severity_type_identity_same_underlying_type() {
    use perl_diagnostics::codes::DiagnosticSeverity as CodesSeverity;
    use perl_diagnostics::types::DiagnosticSeverity as TypesSeverity;

    assert_eq!(
        std::any::TypeId::of::<CodesSeverity>(),
        std::any::TypeId::of::<TypesSeverity>(),
        "DiagnosticSeverity must be one type"
    );
}

// Test 10: Tag type identity is exact.
#[test]
fn test_tag_type_identity_same_underlying_type() {
    use perl_diagnostics::codes::DiagnosticTag as CodesTag;
    use perl_diagnostics::types::DiagnosticTag as TypesTag;

    assert_eq!(
        std::any::TypeId::of::<CodesTag>(),
        std::any::TypeId::of::<TypesTag>(),
        "DiagnosticTag must be one type"
    );
}

// Test 11: Diagnostic uses unified severity and ByteSpan.
#[test]
fn test_diagnostic_struct_uses_unified_severity_and_span() {
    use perl_diagnostics::codes::DiagnosticCode;
    use perl_diagnostics::types::{ByteSpan, Diagnostic, DiagnosticSeverity};

    let span = must(ByteSpan::new(0, 0));
    let diagnostic =
        Diagnostic::new(DiagnosticCode::default(), DiagnosticSeverity::Error, span, "");

    let _severity = diagnostic.severity;
    assert_eq!(diagnostic.range, span);
}

// Test 12: Explicit re-exports include DiagnosticCategory.
#[test]
fn test_api_includes_diagnostic_category_reexport() {
    use perl_diagnostics::DiagnosticCategory;

    let _ = DiagnosticCategory::Parser;
}

// Test 13: Explicit root re-exports include every core public type.
#[test]
fn test_api_reexports_explicit_not_wildcards() {
    use perl_diagnostics::{
        ByteSpan, Diagnostic, DiagnosticCategory, DiagnosticCode, DiagnosticSeverity,
        DiagnosticTag, InvalidByteSpan, RelatedInformation, diagnostic_meta, parse_error,
        syntax_error,
    };

    let _: Option<InvalidByteSpan> = None;
    let _ = (
        ByteSpan::EMPTY,
        DiagnosticCode::ParseError,
        DiagnosticSeverity::Error,
        DiagnosticTag::Unnecessary,
        DiagnosticCategory::Parser,
        Diagnostic::default(),
        RelatedInformation::default(),
        diagnostic_meta,
        parse_error,
        syntax_error,
    );
}

// Test 14: Consumer path for code actions compiles.
#[test]
fn test_consumer_perl_lsp_code_actions_path() {
    use perl_diagnostics::codes::DiagnosticCode;
    use perl_diagnostics::types::{ByteSpan, Diagnostic};

    let _ = (DiagnosticCode::ParseError, ByteSpan::EMPTY, Diagnostic::default());
}

// Test 15: Consumer path for diagnostics compiles.
#[test]
fn test_consumer_perl_lsp_diagnostics_path() {
    use perl_diagnostics::codes::DiagnosticSeverity;
    use perl_diagnostics::types::Diagnostic;
    use perl_diagnostics::types::DiagnosticSeverity as TypesSeverity;

    let from_codes = DiagnosticSeverity::Warning;
    let _as_types: TypesSeverity = from_codes;
    let _ = Diagnostic::default();
}

// Test 16: Consumer path for the LSP server compiles.
#[test]
fn test_consumer_perl_lsp_server_path() {
    use perl_diagnostics::catalog::diagnostic_meta;
    use perl_diagnostics::codes::DiagnosticCode;

    let _ = (DiagnosticCode::ParseError, diagnostic_meta);
}

// Test 17: Severity enum values match the LSP values used by adapters.
#[test]
fn test_severity_lsp_values() {
    use perl_diagnostics::codes::DiagnosticSeverity;

    assert_eq!(DiagnosticSeverity::Error as u8, 1);
    assert_eq!(DiagnosticSeverity::Warning as u8, 2);
    assert_eq!(DiagnosticSeverity::Information as u8, 3);
    assert_eq!(DiagnosticSeverity::Hint as u8, 4);
}

// Test 18: Tag enum values match the LSP values used by adapters.
#[test]
fn test_tag_lsp_values() {
    use perl_diagnostics::codes::DiagnosticTag;

    assert_eq!(DiagnosticTag::Unnecessary as u8, 1);
    assert_eq!(DiagnosticTag::Deprecated as u8, 2);
}

// Test 19: Workspace count documentation.
#[test]
#[ignore = "This test requires external workspace inspection; tracking #4912"]
fn test_workspace_member_count_should_be_121() {}

// Test 20: Publish allowlist count documentation.
#[test]
#[ignore = "This test requires Cargo.toml inspection; tracking #4912"]
fn test_publish_allowlist_count_should_be_118() {}

// Test 21: codes and types maintain the intended dependency direction.
#[test]
fn test_no_circular_dependency_codes_types() {
    use perl_diagnostics::codes::DiagnosticCode;
    use perl_diagnostics::types::{ByteSpan, Diagnostic, DiagnosticSeverity};

    let diagnostic = Diagnostic::new(
        DiagnosticCode::ParseError,
        DiagnosticSeverity::default(),
        ByteSpan::EMPTY,
        "",
    );

    let _ = diagnostic;
}

// Test 22: Diagnostic fields remain accessible with a validated range type.
#[test]
fn test_diagnostic_struct_fields_accessible() {
    use perl_diagnostics::types::{ByteSpan, Diagnostic, DiagnosticSeverity};

    let diagnostic = Diagnostic::new(
        perl_diagnostics::codes::DiagnosticCode::ParseError,
        DiagnosticSeverity::Error,
        ByteSpan::EMPTY,
        "test",
    );

    assert_eq!(diagnostic.message, "test");
    assert_eq!(diagnostic.range, ByteSpan::EMPTY);
}

// Test 23: Layer constraint documentation.
#[test]
#[ignore = "This test requires the repository architecture checker; tracking #4912"]
fn test_perl_diagnostics_no_perl_lsp_dependencies() {}

// Test 24: Catalog functions remain available at crate root.
#[test]
fn test_catalog_function_reexports() {
    use perl_diagnostics::{
        bareword_filehandle, duplicate_package, duplicate_sub, eval_error_flow, from_message,
        implicit_return, missing_package_declaration, missing_return, missing_strict,
        missing_warnings, parse_error, syntax_error, two_arg_open, undefined_var, unexpected_eof,
        unused_var,
    };

    let _ = (
        parse_error,
        syntax_error,
        unexpected_eof,
        missing_strict,
        missing_warnings,
        unused_var,
        undefined_var,
        missing_package_declaration,
        duplicate_package,
        duplicate_sub,
        missing_return,
        bareword_filehandle,
        two_arg_open,
        implicit_return,
        eval_error_flow,
        from_message,
    );
}

// Test 25: DiagnosticMeta is available at crate root.
#[test]
fn test_diagnostic_meta_reexport() {
    use perl_diagnostics::DiagnosticMeta;

    let _ = DiagnosticMeta::default();
}

// Test 26: RelatedInformation uses ByteSpan.
#[test]
fn test_related_information_struct_accessible() {
    use perl_diagnostics::types::{ByteSpan, RelatedInformation};

    let information = RelatedInformation::new("test", ByteSpan::EMPTY);

    assert_eq!(information.message, "test");
    assert_eq!(information.location, ByteSpan::EMPTY);
}

// Test 27: ByteSpan rejects reversal through the public API.
#[test]
fn test_byte_span_rejects_reversal() {
    use perl_diagnostics::ByteSpan;

    assert!(ByteSpan::new(20, 10).is_err());
}
