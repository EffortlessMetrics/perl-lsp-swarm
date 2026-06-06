//! Branch-reset validation contract tests for `perl-regex`.
//!
//! The validator does not fully parse Perl capture numbering, but it does own
//! safety checks for branch-reset group nesting and branch-count budgets. These
//! tests lock the externally visible behavior around those checks.

use perl_regex::{RegexError, RegexValidator, validator::RegexValidationConfig};

fn syntax_details(
    result: Result<(), RegexError>,
) -> Result<(String, usize), Box<dyn std::error::Error>> {
    match result {
        Err(RegexError::Syntax { message, offset }) => Ok((message, offset)),
        Ok(()) => Err("expected regex syntax error".into()),
    }
}

#[test]
fn simple_branch_reset_group_validates() -> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::new();

    validator.validate(r"(?|(a)(b)|(c)(d))", 0)?;

    Ok(())
}

#[test]
fn nested_branch_reset_within_limit_validates() -> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::new();

    validator.validate(r"(?|(?|(a)(b))(c)(d))", 0)?;

    Ok(())
}

#[test]
fn branch_reset_with_quantified_branches_validates() -> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::new();

    validator.validate(r"(?|(a{2})(b{2})|(c{2})(d{2}))", 0)?;

    Ok(())
}

#[test]
fn branch_reset_with_mixed_named_and_unnamed_captures_validates()
-> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::new();

    validator.validate(r"(?|(?<left>a)(b)|(?<right>c)(d))", 0)?;

    Ok(())
}

#[test]
fn empty_branch_reset_group_validates() -> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::new();

    validator.validate(r"(?|)", 0)?;

    Ok(())
}

#[test]
fn branch_reset_alternation_limit_counts_each_pipe_in_current_group()
-> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::with_config(RegexValidationConfig {
        max_nesting: 10,
        max_unicode_properties: 50,
        max_branch_reset_branches: 3,
    });

    validator.validate(r"(?|one|two|three)", 0)?;
    let (message, offset) = syntax_details(validator.validate(r"(?|one|two|three|four)", 40))?;

    assert!(message.contains("Too many branches"), "unexpected message: {message}");
    assert_eq!(offset, 56);
    Ok(())
}

#[test]
fn nested_branch_reset_alternation_limit_applies_to_inner_group()
-> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::with_config(RegexValidationConfig {
        max_nesting: 10,
        max_unicode_properties: 50,
        max_branch_reset_branches: 2,
    });

    let (message, offset) = syntax_details(validator.validate(r"(?|outer(?|a|b|c)|tail)", 100))?;

    assert!(message.contains("branch reset group"), "unexpected message: {message}");
    assert_eq!(offset, 114);
    Ok(())
}

#[test]
fn branch_reset_nesting_limit_reports_start_offset() -> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::with_config(RegexValidationConfig {
        max_nesting: 2,
        max_unicode_properties: 50,
        max_branch_reset_branches: 50,
    });

    let (message, offset) = syntax_details(validator.validate(r"(?|(?|(?|a)))", 7))?;

    assert!(message.contains("branch reset nesting"), "unexpected message: {message}");
    assert_eq!(offset, 15);
    Ok(())
}

#[test]
fn alternation_outside_branch_reset_does_not_count_toward_branch_reset_limit()
-> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::with_config(RegexValidationConfig {
        max_nesting: 10,
        max_unicode_properties: 50,
        max_branch_reset_branches: 1,
    });

    validator.validate(r"(?:a|b|c)(?|x)", 0)?;

    Ok(())
}
