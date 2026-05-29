//! Additional public-contract coverage for validator and analyzer edge cases.

use perl_regex::{RegexAnalyzer, RegexError, RegexValidator, validator::RegexValidationConfig};

fn finding_offset(
    finding: Option<perl_regex::validator::RegexFinding>,
) -> Result<usize, Box<dyn std::error::Error>> {
    match finding {
        Some(finding) => Ok(finding.offset),
        None => Err("expected regex finding".into()),
    }
}

fn syntax_details(
    result: Result<(), RegexError>,
) -> Result<(String, usize), Box<dyn std::error::Error>> {
    match result {
        Err(RegexError::Syntax { message, offset }) => Ok((message, offset)),
        Ok(()) => Err("expected regex syntax error".into()),
    }
}

#[test]
fn validator_exposes_config_used_for_validation() -> Result<(), Box<dyn std::error::Error>> {
    let config = RegexValidationConfig {
        max_nesting: 4,
        max_unicode_properties: 2,
        max_branch_reset_branches: 3,
    };
    let validator = RegexValidator::with_config(config.clone());

    assert_eq!(validator.config(), &config);

    Ok(())
}

#[test]
fn code_execution_finding_reports_start_adjusted_offset_and_message()
-> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::new();

    let finding = match validator.find_code_execution(r"abc(?{danger})", 25) {
        Some(finding) => finding,
        None => return Err("expected immediate code-execution finding".into()),
    };

    assert_eq!(finding.offset, 28);
    assert!(finding.message.contains("Embedded code execution"));
    Ok(())
}

#[test]
fn deferred_code_execution_finding_reports_distinct_message()
-> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::new();

    let finding = match validator.find_code_execution(r"x(??{danger})", 9) {
        Some(finding) => finding,
        None => return Err("expected deferred code-execution finding".into()),
    };

    assert_eq!(finding.offset, 10);
    assert!(finding.message.contains("Deferred embedded code execution"));
    Ok(())
}

#[test]
fn code_execution_scanner_skips_quoted_literals_and_character_classes()
-> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::new();

    assert!(!validator.detects_code_execution(r"\Q(?{literal})\E(?=safe)"));
    assert!(!validator.detects_code_execution(r"[(??{literal})]"));

    Ok(())
}

#[test]
fn nested_quantifier_finding_reports_outer_quantifier_offset()
-> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::new();

    let offset = finding_offset(validator.find_nested_quantifier(r"prefix(a+){2,4}", 300))?;

    assert_eq!(offset, 310);
    Ok(())
}

#[test]
fn nested_quantifier_detection_ignores_possessive_outer_quantifier()
-> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::new();

    assert!(!validator.detect_nested_quantifiers(r"(a+)++"));
    validator.validate(r"(a+)++", 0)?;

    Ok(())
}

#[test]
fn nested_quantifier_detection_ignores_atomic_inner_group() -> Result<(), Box<dyn std::error::Error>>
{
    let validator = RegexValidator::new();

    assert!(!validator.detect_nested_quantifiers(r"(?>a+)+"));
    validator.validate(r"(?>a+)+", 0)?;

    Ok(())
}

#[test]
fn lookbehind_nesting_limit_reports_start_adjusted_offset() -> Result<(), Box<dyn std::error::Error>>
{
    let validator = RegexValidator::with_config(RegexValidationConfig {
        max_nesting: 1,
        max_unicode_properties: 50,
        max_branch_reset_branches: 50,
    });

    let (message, offset) = syntax_details(validator.validate(r"(?<=(?<!a))", 20))?;

    assert!(message.contains("lookbehind nesting"), "unexpected message: {message}");
    assert_eq!(offset, 27);
    Ok(())
}

#[test]
fn unicode_property_limit_counts_uppercase_and_lowercase_properties()
-> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::with_config(RegexValidationConfig {
        max_nesting: 10,
        max_unicode_properties: 1,
        max_branch_reset_branches: 50,
    });

    let (message, offset) = syntax_details(validator.validate(r"\p{L}\P{N}", 5))?;

    assert!(message.contains("Too many Unicode properties"), "unexpected message: {message}");
    assert_eq!(offset, 10);
    Ok(())
}

#[test]
fn named_capture_extraction_supports_all_perl_name_syntaxes()
-> Result<(), Box<dyn std::error::Error>> {
    let captures = RegexAnalyzer::extract_named_captures(r"(?<angle>a)(?'quote'b)(?P<python>c)");

    assert_eq!(captures.len(), 3);
    assert_eq!(captures[0].name, "angle");
    assert_eq!(captures[0].index, 1);
    assert_eq!(captures[1].name, "quote");
    assert_eq!(captures[1].index, 2);
    assert_eq!(captures[2].name, "python");
    assert_eq!(captures[2].index, 3);
    Ok(())
}

#[test]
fn named_capture_extraction_ignores_literals_classes_and_lookbehind()
-> Result<(), Box<dyn std::error::Error>> {
    let captures =
        RegexAnalyzer::extract_named_captures(r"\Q(?<literal>x)\E[(?<class>y)](?<=z)(?<real>w)");

    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].name, "real");
    assert_eq!(captures[0].pattern, "w");
    Ok(())
}

#[test]
fn hover_text_can_report_unknown_modifiers_without_pattern()
-> Result<(), Box<dyn std::error::Error>> {
    let hover = RegexAnalyzer::hover_text_for_regex("", " z z ");

    assert_eq!(hover, "Unknown modifiers: `z`");
    Ok(())
}
