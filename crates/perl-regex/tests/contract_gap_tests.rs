//! Additional public-contract tests for `perl-regex` edge cases.
//!
//! These tests cover interactions between the validator passes and Perl syntax
//! forms that are easy to regress when the scanners are refactored.

use perl_regex::{RegexAnalyzer, RegexError, RegexValidator, validator::RegexValidationConfig};

#[test]
fn validate_reports_embedded_code_before_other_safety_findings()
-> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::new();
    let result = validator.validate(r"(a+)+(?{ die 'unsafe' })\p{L}\p{N}", 100);

    match result {
        Err(RegexError::Syntax { message, offset }) => {
            assert!(
                message.contains("Embedded code execution"),
                "validate should report embedded code before later safety findings: {message}"
            );
            assert_eq!(offset, 105);
        }
        Ok(()) => return Err("expected embedded-code validation failure".into()),
    }

    Ok(())
}

#[test]
fn validate_reports_nested_quantifier_before_complexity_findings()
-> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::with_config(RegexValidationConfig {
        max_nesting: 10,
        max_unicode_properties: 1,
        max_branch_reset_branches: 50,
    });
    let result = validator.validate(r"abc(a+)+\p{L}\p{N}", 200);

    match result {
        Err(RegexError::Syntax { message, offset }) => {
            assert!(
                message.contains("Nested quantifiers"),
                "validate should report nested quantifiers before complexity limits: {message}"
            );
            assert_eq!(offset, 207);
        }
        Ok(()) => return Err("expected nested-quantifier validation failure".into()),
    }

    Ok(())
}

#[test]
fn branch_reset_branch_limit_counts_only_current_group_alternations()
-> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::with_config(RegexValidationConfig {
        max_nesting: 10,
        max_unicode_properties: 50,
        max_branch_reset_branches: 2,
    });

    validator.validate(r"(?|left|(inner|alternate))", 0)?;

    let result = validator.validate(r"(?|left|right|extra)", 10);
    match result {
        Err(RegexError::Syntax { message, offset }) => {
            assert!(message.contains("Too many branches"), "unexpected message: {message}");
            assert_eq!(offset, 23);
        }
        Ok(()) => return Err("expected branch-reset branch-count validation failure".into()),
    }

    Ok(())
}

#[test]
fn unclosed_quoted_literal_suppresses_syntax_like_text_until_end()
-> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::new();
    let pattern = r"prefix\Q(?{ die 'literal' })(a+)+\p{L}(?<name>literal)";

    validator.validate(pattern, 50)?;
    assert!(!validator.detects_code_execution(pattern));
    assert!(!validator.detect_nested_quantifiers(pattern));
    assert!(RegexAnalyzer::extract_named_captures(pattern).is_empty());

    Ok(())
}

#[test]
fn capture_extraction_skips_character_classes_with_fake_named_captures()
-> Result<(), Box<dyn std::error::Error>> {
    let captures =
        RegexAnalyzer::extract_named_captures(r"[(?<fake>)](?<real>[\]])(?'also_real'\w+)");

    assert_eq!(captures.len(), 2);
    assert_eq!(captures[0].name, "real");
    assert_eq!(captures[0].index, 1);
    assert_eq!(captures[0].pattern, r"[\]]");
    assert_eq!(captures[1].name, "also_real");
    assert_eq!(captures[1].index, 2);

    Ok(())
}

#[test]
fn hover_text_preserves_first_seen_order_for_unknown_modifiers()
-> Result<(), Box<dyn std::error::Error>> {
    let hover = RegexAnalyzer::hover_text_for_regex(r"\w+", "z i y z\nq");

    assert!(hover.contains("case-insensitive matching"), "known modifier should be described");
    assert!(
        hover.contains("Unknown modifiers: `zyq`"),
        "unknown modifiers should be de-duplicated in first-seen order: {hover}"
    );

    Ok(())
}
