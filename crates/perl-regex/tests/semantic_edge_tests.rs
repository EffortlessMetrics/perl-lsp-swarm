//! Additional semantic edge coverage for the public `perl-regex` API.
//!
//! These tests focus on interactions between scanners: escaped syntax, character
//! classes, validation priority, branch-reset branch accounting, and hover output
//! ordering.

use perl_regex::{RegexAnalyzer, RegexError, RegexValidator, validator::RegexValidationConfig};

#[test]
fn code_execution_scanner_ignores_escaped_and_char_class_constructs()
-> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::new();
    let pattern = r"\(?{ literal }[abc(?{ still literal })]";

    assert!(
        !validator.detects_code_execution(pattern),
        "escaped open parens and character classes must not be reported as executable code"
    );
    assert_eq!(validator.find_code_execution(pattern, 17), None);
    validator.validate(pattern, 17)?;
    Ok(())
}

#[test]
fn validation_reports_code_execution_before_earlier_nested_quantifier()
-> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::new();
    let pattern = r"(a+)+(?{ run })";
    let start_pos = 200;

    match validator.validate(pattern, start_pos) {
        Err(RegexError::Syntax { message, offset }) => {
            assert!(
                message.contains("Embedded code execution"),
                "validation should prefer code-execution diagnostics: {message}"
            );
            assert_eq!(offset, start_pos + 5);
        }
        Ok(()) => return Err("expected code-execution validation error".into()),
    }

    Ok(())
}

#[test]
fn branch_reset_limit_counts_only_current_branch_reset_group_alternations()
-> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::with_config(RegexValidationConfig {
        max_nesting: 10,
        max_unicode_properties: 50,
        max_branch_reset_branches: 2,
    });

    validator.validate(r"(?|(a|b|c)|z)", 0)?;
    let error = match validator.validate(r"(?|a|b|c)", 40) {
        Err(err) => err,
        Ok(()) => return Err("expected branch-reset branch-count error".into()),
    };

    match error {
        RegexError::Syntax { message, offset } => {
            assert!(message.contains("Too many branches in branch reset group"));
            assert_eq!(offset, 46);
        }
    }

    Ok(())
}

#[test]
fn unicode_property_limit_ignores_escaped_literal_p_without_braces()
-> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::with_config(RegexValidationConfig {
        max_nesting: 10,
        max_unicode_properties: 1,
        max_branch_reset_branches: 50,
    });

    validator.validate(r"\p literal \P literal", 0)?;
    validator.validate(r"\p{L}", 0)?;
    let error = match validator.validate(r"\p{L}\P{N}", 9) {
        Err(err) => err,
        Ok(()) => return Err("expected unicode-property limit error".into()),
    };

    match error {
        RegexError::Syntax { message, offset } => {
            assert!(message.contains("Too many Unicode properties"));
            assert_eq!(offset, 14);
        }
    }

    Ok(())
}

#[test]
fn named_capture_extraction_ignores_escaped_and_char_class_parens()
-> Result<(), Box<dyn std::error::Error>> {
    let captures =
        RegexAnalyzer::extract_named_captures(r"\(?<escaped>no)[(?<class>no)](?<real>(?:yes)[)])");

    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].name, "real");
    assert_eq!(captures[0].index, 1);
    assert_eq!(captures[0].pattern, r"(?:yes)[)]");
    Ok(())
}

#[test]
fn named_capture_indexes_skip_lookaround_and_non_capturing_groups()
-> Result<(), Box<dyn std::error::Error>> {
    let captures = RegexAnalyzer::extract_named_captures(
        r"(?<=prefix)(?:skip)(plain)(?<first>\w+)(?!suffix)(?<second>\d+)",
    );

    assert_eq!(captures.len(), 2);
    assert_eq!(captures[0].name, "first");
    assert_eq!(captures[0].index, 2);
    assert_eq!(captures[1].name, "second");
    assert_eq!(captures[1].index, 3);
    Ok(())
}

#[test]
fn hover_text_keeps_first_seen_unknown_modifier_order_after_deduplication()
-> Result<(), Box<dyn std::error::Error>> {
    let hover = RegexAnalyzer::hover_text_for_regex(r"\w+", "z y z q y");

    assert!(hover.contains("Unknown modifiers: `zyq`"), "unexpected hover text: {hover}");
    assert_eq!(hover.matches("Unknown modifiers").count(), 1);
    Ok(())
}
