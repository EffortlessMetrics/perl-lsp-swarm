//! Additional edge-case coverage for `perl-regex` safety validation.
//!
//! These tests focus on public API behavior at scanner boundaries: offsets,
//! quoting/escaping exclusions, possessive and atomic quantifier exemptions,
//! and configuration-driven complexity limits.

use perl_regex::validator::RegexValidationConfig;
use perl_regex::{RegexError, RegexValidator, validator::RegexFinding};

fn require_finding(
    finding: Option<RegexFinding>,
    label: &str,
) -> Result<RegexFinding, Box<dyn std::error::Error>> {
    finding.ok_or_else(|| format!("expected finding for {label}").into())
}

fn require_error(
    result: Result<(), RegexError>,
    label: &str,
) -> Result<RegexError, Box<dyn std::error::Error>> {
    match result {
        Ok(()) => Err(format!("expected validation error for {label}").into()),
        Err(err) => Ok(err),
    }
}

fn assert_syntax_error(
    err: RegexError,
    expected_message_fragment: &str,
    expected_offset: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    match err {
        RegexError::Syntax { message, offset } => {
            assert!(
                message.contains(expected_message_fragment),
                "expected message containing {expected_message_fragment:?}, got {message:?}",
            );
            assert_eq!(offset, expected_offset);
            Ok(())
        }
    }
}

#[test]
fn find_code_execution_reports_absolute_offset_for_immediate_construct()
-> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::new();
    let finding = require_finding(
        validator.find_code_execution(r"prefix(?{ die 'unsafe' })", 11),
        "immediate code execution",
    )?;

    assert_eq!(finding.offset, 17);
    assert!(finding.message.contains("Embedded code execution"));
    Ok(())
}

#[test]
fn find_code_execution_reports_absolute_offset_for_deferred_construct()
-> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::new();
    let finding = require_finding(
        validator.find_code_execution(r"(?:safe)(??{ build_regex() })", 4),
        "deferred code execution",
    )?;

    assert_eq!(finding.offset, 12);
    assert!(finding.message.contains("Deferred embedded code execution"));
    Ok(())
}

#[test]
fn code_execution_scanner_ignores_escaped_char_class_and_quoted_literals()
-> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::new();

    assert!(!validator.detects_code_execution(r"\(\?\{ literal \}\)"));
    assert!(!validator.detects_code_execution(r"[(?{still literal})]+"));
    assert!(!validator.detects_code_execution(r"\Q(?{ literal })(??{ literal })\E"));
    Ok(())
}

#[test]
fn nested_quantifier_finding_reports_absolute_offset_for_brace_quantifier()
-> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::new();
    let finding = require_finding(
        validator.find_nested_quantifier(r"prefix(a{2,3}){4}", 20),
        "brace nested quantifier",
    )?;

    assert_eq!(finding.offset, 34);
    assert!(finding.message.contains("Nested quantifiers"));
    Ok(())
}

#[test]
fn nested_quantifier_scanner_ignores_possessive_inner_quantifier()
-> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::new();

    assert!(!validator.detect_nested_quantifiers(r"(a++)+"));
    assert!(!validator.detect_nested_quantifiers(r"(a*+)+"));
    assert!(!validator.detect_nested_quantifiers(r"(a{2,3}+)+"));
    Ok(())
}

#[test]
fn nested_quantifier_scanner_ignores_possessive_outer_quantifier()
-> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::new();

    assert!(!validator.detect_nested_quantifiers(r"(a+){2,3}+"));
    assert!(!validator.detect_nested_quantifiers(r"(a*)++"));
    Ok(())
}

#[test]
fn nested_quantifier_scanner_ignores_atomic_groups() -> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::new();

    assert!(!validator.detect_nested_quantifiers(r"(?>a+)+"));
    assert!(!validator.detect_nested_quantifiers(r"((?>a+))+"));
    Ok(())
}

#[test]
fn nested_quantifier_scanner_ignores_invalid_brace_literals()
-> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::new();

    assert!(!validator.detect_nested_quantifiers(r"(a{,})+"));
    assert!(!validator.detect_nested_quantifiers(r"(a{word})+"));
    assert!(!validator.detect_nested_quantifiers(r"(a{2,,3})+"));
    Ok(())
}

#[test]
fn validate_enforces_configured_branch_reset_branch_limit() -> Result<(), Box<dyn std::error::Error>>
{
    let config = RegexValidationConfig {
        max_nesting: 10,
        max_unicode_properties: 50,
        max_branch_reset_branches: 2,
    };
    let validator = RegexValidator::with_config(config);

    let err =
        require_error(validator.validate(r"(?|alpha|beta|gamma)", 100), "branch reset branches")?;
    assert_syntax_error(err, "Too many branches in branch reset group", 113)
}

#[test]
fn validate_enforces_configured_branch_reset_nesting_limit()
-> Result<(), Box<dyn std::error::Error>> {
    let config = RegexValidationConfig {
        max_nesting: 1,
        max_unicode_properties: 50,
        max_branch_reset_branches: 50,
    };
    let validator = RegexValidator::with_config(config);

    let err = require_error(validator.validate(r"(?|a(?|b|c))", 30), "branch reset nesting")?;
    assert_syntax_error(err, "Regex branch reset nesting too deep", 36)
}

#[test]
fn validate_enforces_configured_lookbehind_nesting_limit() -> Result<(), Box<dyn std::error::Error>>
{
    let config = RegexValidationConfig {
        max_nesting: 1,
        max_unicode_properties: 50,
        max_branch_reset_branches: 50,
    };
    let validator = RegexValidator::with_config(config);

    let err = require_error(validator.validate(r"(?<=(?<=a)b)c", 7), "lookbehind nesting")?;
    assert_syntax_error(err, "Regex lookbehind nesting too deep", 14)
}

#[test]
fn validate_does_not_count_unicode_property_text_inside_ignored_regions()
-> Result<(), Box<dyn std::error::Error>> {
    let config = RegexValidationConfig {
        max_nesting: 10,
        max_unicode_properties: 0,
        max_branch_reset_branches: 50,
    };
    let validator = RegexValidator::with_config(config);

    validator.validate(r"[\p{Letter}]", 0)?;
    validator.validate(r"\Q\p{Letter}\E", 0)?;
    Ok(())
}

#[test]
fn validate_reports_unicode_property_offset_after_start_position()
-> Result<(), Box<dyn std::error::Error>> {
    let config = RegexValidationConfig {
        max_nesting: 10,
        max_unicode_properties: 1,
        max_branch_reset_branches: 50,
    };
    let validator = RegexValidator::with_config(config);

    let err =
        require_error(validator.validate(r"\p{Letter}x\P{Number}", 9), "unicode property limit")?;
    assert_syntax_error(err, "Too many Unicode properties", 20)
}
