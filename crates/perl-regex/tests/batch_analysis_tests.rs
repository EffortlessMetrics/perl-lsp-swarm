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
fn one_batch_preserves_dynamic_advisory_and_policy_findings()
-> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::with_config(RegexValidationConfig {
        max_nesting: 10,
        max_unicode_properties: 1,
        max_branch_reset_branches: 50,
    });
    let pattern = r"\p{L}\p{N}(a+)+(?{ run })(??{ later })";

    let analysis = validator.analyze(pattern);
    let codes = analysis.diagnostics.iter().map(|diagnostic| diagnostic.code).collect::<Vec<_>>();

    assert_eq!(
        codes,
        vec![
            RegexDiagnosticCode::UnicodePropertyLimit,
            RegexDiagnosticCode::NestedQuantifierRisk,
            RegexDiagnosticCode::EmbeddedCodeImmediate,
            RegexDiagnosticCode::EmbeddedCodeDeferred,
        ]
    );
    assert_eq!(analysis.completeness, RegexAnalysisCompleteness::DynamicAndPolicyLimited);
    assert_eq!(analysis.facts.nested_quantifiers.len(), 1);
    assert_eq!(analysis.facts.nested_quantifiers[0].start, 14);
    assert_eq!(analysis.facts.embedded_code.len(), 2);
    assert_eq!(analysis.facts.embedded_code[0].kind, EmbeddedCodeKind::Immediate);
    assert_eq!(analysis.facts.embedded_code[0].range.start, 15);
    assert_eq!(analysis.facts.embedded_code[1].kind, EmbeddedCodeKind::Deferred);
    assert_eq!(analysis.facts.embedded_code[1].range.start, 25);
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
    assert_eq!(diagnostic.class.as_str(), "policy_limit");
    assert_eq!(diagnostic.limit, Some(1));
    assert!(diagnostic.message().contains("max 1"));
    Ok(())
}

#[test]
fn diagnostics_are_deterministic_and_source_ordered() -> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::with_config(RegexValidationConfig {
        max_nesting: 10,
        max_unicode_properties: 1,
        max_branch_reset_branches: 50,
    });
    let pattern = r"(?{ first })\p{L}\p{N}(a+)+(?{ second })";
    let first = validator.analyze(pattern);
    let second = validator.analyze(pattern);

    assert_eq!(first, second);
    for pair in first.diagnostics.windows(2) {
        assert!(pair[0].range.start <= pair[1].range.start);
    }
    Ok(())
}

#[test]
fn compatibility_validate_keeps_embedded_code_category_priority()
-> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::new();
    let pattern = r"(a+)+literal(?{ run })";
    let analysis = validator.analyze(pattern);

    assert_eq!(analysis.diagnostics[0].code, RegexDiagnosticCode::NestedQuantifierRisk);
    assert_eq!(analysis.diagnostics[1].code, RegexDiagnosticCode::EmbeddedCodeImmediate);

    let error =
        validator.validate(pattern, 100).err().ok_or("expected compatibility validation error")?;
    match error {
        RegexError::Syntax { message, offset } => {
            assert!(message.contains("Embedded code execution"));
            assert_eq!(offset, 112);
        }
    }
    Ok(())
}

#[test]
fn compatibility_finders_project_relative_batch_ranges_to_absolute_offsets()
-> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::new();

    let code_analysis = validator.analyze("xx(?{ run })");
    assert_eq!(code_analysis.facts.embedded_code[0].range.start, 2);
    let code = validator
        .find_code_execution("xx(?{ run })", 20)
        .ok_or("expected embedded-code compatibility finding")?;
    assert_eq!(code.offset, 22);

    let nested_analysis = validator.analyze("abc(a+)+");
    assert_eq!(nested_analysis.facts.nested_quantifiers[0].start, 7);
    let nested = validator
        .find_nested_quantifier("abc(a+)+", 20)
        .ok_or("expected nested-quantifier compatibility finding")?;
    assert_eq!(nested.offset, 27);

    assert_eq!(
        validator.detects_code_execution("xx(?{ run })"),
        !code_analysis.facts.embedded_code.is_empty()
    );
    assert_eq!(
        validator.detect_nested_quantifiers("abc(a+)+"),
        !nested_analysis.facts.nested_quantifiers.is_empty()
    );
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
    assert!(dynamic.completeness.has_dynamic_boundary());
    assert!(!dynamic.completeness.is_policy_limited());

    let limited = validator.analyze(r"\p{L}\p{N}");
    assert_eq!(limited.completeness, RegexAnalysisCompleteness::PolicyLimited);
    assert!(!limited.completeness.has_dynamic_boundary());
    assert!(limited.completeness.is_policy_limited());
    assert!(
        limited
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.class != RegexDiagnosticClass::Syntax)
    );
    Ok(())
}
