use perl_regex::validator::{
    EmbeddedCodeKind, RegexAnalysisCompleteness, RegexDiagnosticClass, RegexDiagnosticCode,
    RegexValidationConfig,
};
use perl_regex::{RegexError, RegexValidator};

#[test]
fn safe_pattern_returns_complete_clean_analysis() -> Result<(), Box<dyn std::error::Error>> {
    let analysis = RegexValidator::new().analyze(r"^(?<year>\d{4})-(?<month>\d{2})$");
    assert!(analysis.is_clean());
    assert!(analysis.completeness.is_complete());
    assert!(analysis.facts.embedded_code.is_empty());
    assert!(analysis.facts.nested_quantifiers.is_empty());
    Ok(())
}

#[test]
fn typed_analysis_projects_current_scanner_findings() -> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::with_config(RegexValidationConfig {
        max_nesting: 10,
        max_unicode_properties: 1,
        max_branch_reset_branches: 50,
    });
    let analysis = validator.analyze(r"\p{L}\p{N}(a+)+(?{ run })");
    let codes = analysis.diagnostics.iter().map(|diagnostic| diagnostic.code).collect::<Vec<_>>();
    assert!(codes.contains(&RegexDiagnosticCode::UnicodePropertyLimit));
    assert!(codes.contains(&RegexDiagnosticCode::NestedQuantifierRisk));
    assert!(codes.contains(&RegexDiagnosticCode::EmbeddedCodeImmediate));
    assert_eq!(analysis.facts.embedded_code.len(), 1);
    assert_eq!(analysis.facts.embedded_code[0].kind, EmbeddedCodeKind::Immediate);
    assert_eq!(analysis.completeness, RegexAnalysisCompleteness::DynamicAndPolicyLimited);
    Ok(())
}

#[test]
fn diagnostic_identity_is_separate_from_presentation_text() -> Result<(), Box<dyn std::error::Error>>
{
    let validator = RegexValidator::with_config(RegexValidationConfig {
        max_nesting: 10,
        max_unicode_properties: 1,
        max_branch_reset_branches: 50,
    });
    let analysis = validator.analyze(r"\p{L}\p{N}");
    let diagnostic = analysis.diagnostics.first().ok_or("expected policy diagnostic")?;
    assert_eq!(diagnostic.code, RegexDiagnosticCode::UnicodePropertyLimit);
    assert_eq!(diagnostic.code.as_str(), "unicode_property_limit");
    assert_eq!(diagnostic.class, RegexDiagnosticClass::PolicyLimit);
    assert_eq!(diagnostic.limit, Some(1));
    assert!(diagnostic.message().contains("max 1"));
    Ok(())
}

#[test]
fn compatibility_validate_keeps_embedded_code_category_priority()
-> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::new();
    let pattern = r"(a+)+literal(?{ run })";
    let error =
        validator.validate(pattern, 100).err().ok_or("expected compatibility validation error")?;
    match error {
        RegexError::Syntax { message, offset } => {
            assert!(message.contains("Embedded code execution"));
            assert_eq!(offset, 100 + pattern.find("(?{").ok_or("missing opener")?);
        }
    }
    Ok(())
}

#[test]
fn compatibility_finders_remain_absolute_offset_based() -> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::new();
    let code = validator
        .find_code_execution("xx(?{ run })", 20)
        .ok_or("expected embedded-code finding")?;
    assert_eq!(code.offset, 22);
    let nested = validator
        .find_nested_quantifier("abc(a+)+", 20)
        .ok_or("expected nested-quantifier finding")?;
    assert_eq!(nested.offset, 27);
    Ok(())
}

#[test]
fn dynamic_and_policy_completeness_remain_distinct() -> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::with_config(RegexValidationConfig {
        max_nesting: 10,
        max_unicode_properties: 1,
        max_branch_reset_branches: 50,
    });
    let dynamic = validator.analyze("(?{ run })");
    assert_eq!(dynamic.completeness, RegexAnalysisCompleteness::Dynamic);
    let limited = validator.analyze(r"\p{L}\p{N}");
    assert_eq!(limited.completeness, RegexAnalysisCompleteness::PolicyLimited);
    Ok(())
}
