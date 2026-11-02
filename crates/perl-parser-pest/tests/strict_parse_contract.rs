//! Strict parse entry point and legacy `parse()` compatibility contract (#8435).

#![deny(clippy::map_err_ignore)]

use std::error::Error;

use perl_parser_pest::{ParseAttempt, ParseCompleteness, PureRustPerlParser, StrictParseError};

fn expect_complete(ast: &ParseAttempt<perl_parser_pest::AstNode>) -> Result<(), Box<dyn Error>> {
    let ParseAttempt::Outcome(outcome) = ast else {
        return Err(format!("expected complete outcome, got {ast:?}").into());
    };
    if outcome.completeness() != ParseCompleteness::Complete {
        return Err(
            format!("expected Complete completeness, got {:?}", outcome.completeness()).into()
        );
    }
    Ok(())
}

fn expect_rejected(
    attempt: &ParseAttempt<perl_parser_pest::AstNode>,
) -> Result<&StrictParseError, Box<dyn Error>> {
    match attempt {
        ParseAttempt::Rejected(error) => Ok(error),
        other => Err(format!("expected strict rejection, got {other:?}").into()),
    }
}

#[test]
fn strict_valid_program_is_complete() -> Result<(), Box<dyn Error>> {
    let source = include_str!("fixtures/sources/declaration-control-flow/simple-scalar.pl");
    let mut parser = PureRustPerlParser::new();
    let attempt = parser.parse_strict(source);
    expect_complete(&attempt)?;
    Ok(())
}

#[test]
fn strict_empty_source_is_complete_empty_program() -> Result<(), Box<dyn Error>> {
    let mut parser = PureRustPerlParser::new();
    let attempt = parser.parse_strict("");
    expect_complete(&attempt)?;
    Ok(())
}

#[test]
fn strict_trailing_garbage_is_rejected_not_complete() -> Result<(), Box<dyn Error>> {
    let source = "my $x = 1;\n??? trailing\n";
    let mut parser = PureRustPerlParser::new();
    let attempt = parser.parse_strict(source);
    let rejection = expect_rejected(&attempt)?;
    rejection.range().check_over_source(source)?;
    assert!(!rejection.pest_context().is_empty(), "strict rejection must retain pest context");
    Ok(())
}

#[test]
fn strict_valid_invalid_valid_is_rejected_without_partial_ast() -> Result<(), Box<dyn Error>> {
    let source = include_str!("fixtures/sources/recovery/valid-invalid-valid.pl");
    let mut parser = PureRustPerlParser::new();
    let attempt = parser.parse_strict(source);
    expect_rejected(&attempt)?;
    Ok(())
}

#[test]
fn strict_normalized_only_scalar_deref_rejects_without_synthetic_ast() -> Result<(), Box<dyn Error>>
{
    let source = include_str!("fixtures/sources/scalar-deref/double-dollar.pl");
    let mut parser = PureRustPerlParser::new();
    let attempt = parser.parse_strict(source);
    expect_rejected(&attempt)?;
    Ok(())
}

#[test]
fn strict_normalized_only_bitnot_assignment_rejects_without_synthetic_ast()
-> Result<(), Box<dyn Error>> {
    let source = include_str!("fixtures/sources/bitwise-not/spaced-tilde.pl");
    let mut parser = PureRustPerlParser::new();
    let attempt = parser.parse_strict(source);
    expect_rejected(&attempt)?;
    Ok(())
}

#[test]
fn strict_parse_is_deterministic_across_reuse() -> Result<(), Box<dyn Error>> {
    let source = include_str!("fixtures/sources/declaration-control-flow/simple-scalar.pl");
    let mut parser = PureRustPerlParser::new();
    let first = parser.parse_strict(source);
    let second = parser.parse_strict(source);
    if first != second {
        return Err("strict parse must be deterministic across calls on the same parser".into());
    }
    Ok(())
}

#[test]
fn legacy_parse_adapter_preserves_seed_recovery_behavior() -> Result<(), Box<dyn Error>> {
    let mut parser = PureRustPerlParser::new();
    let simple = include_str!("fixtures/sources/declaration-control-flow/simple-scalar.pl");
    if parser.parse(simple).is_err() {
        return Err("legacy parse of simple-scalar.pl must still succeed".into());
    }

    let valid_invalid_valid = include_str!("fixtures/sources/recovery/valid-invalid-valid.pl");
    if parser.parse(valid_invalid_valid).is_err() {
        return Err("legacy parse of valid-invalid-valid.pl must still succeed via recovery".into());
    }

    let deref = include_str!("fixtures/sources/scalar-deref/double-dollar.pl");
    if parser.parse(deref).is_err() {
        return Err("legacy parse of double-dollar.pl must still succeed via normalization".into());
    }
    Ok(())
}

#[test]
fn strict_malformed_source_carries_original_source_range() -> Result<(), Box<dyn Error>> {
    let source = "my = ;\n";
    let mut parser = PureRustPerlParser::new();
    let attempt = parser.parse_strict(source);
    let rejection = expect_rejected(&attempt)?;
    rejection.range().check_over_source(source)?;
    Ok(())
}
