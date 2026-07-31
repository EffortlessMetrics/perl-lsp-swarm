// Regression tests for issue #5042 Slice 2 — legacy octal semantics.
//
// Perl's `perlnumber` rules treat `0755` as octal (493 decimal), not decimal
// 755. Any digit 8 or 9 after a leading `0` falls back to decimal. This file
// proves that `parse_perl_integer` correctly classifies each literal form so
// value-aware consumers (critic, hover, constant folding) do not need to
// re-derive the rules ad-hoc.

use perl_lexer::numeric::{NumericBase, parse_perl_integer};

type R = Result<(), Box<dyn std::error::Error>>;

// ── Legacy octal ─────────────────────────────────────────────────────────────

#[test]
fn octal_017_is_decimal_15() -> R {
    let (value, base) = parse_perl_integer("017").ok_or("expected Some")?;
    assert_eq!(base, NumericBase::LegacyOctal, "017 must be LegacyOctal");
    assert_eq!(value, 15, "017 octal == 15 decimal");
    Ok(())
}

#[test]
fn octal_0777_is_decimal_511() -> R {
    let (value, base) = parse_perl_integer("0777").ok_or("expected Some")?;
    assert_eq!(base, NumericBase::LegacyOctal, "0777 must be LegacyOctal");
    assert_eq!(value, 511, "0777 octal == 511 decimal");
    Ok(())
}

#[test]
fn octal_0755_is_decimal_493() -> R {
    let (value, base) = parse_perl_integer("0755").ok_or("expected Some")?;
    assert_eq!(base, NumericBase::LegacyOctal, "0755 must be LegacyOctal");
    assert_eq!(value, 493, "0755 octal == 493 decimal (chmod idiom)");
    Ok(())
}

#[test]
fn octal_with_underscore_separator() -> R {
    // 0_17 == 017 == 15 decimal; underscore is just a visual separator.
    let (value, base) = parse_perl_integer("0_17").ok_or("expected Some")?;
    assert_eq!(base, NumericBase::LegacyOctal, "0_17 must be LegacyOctal");
    assert_eq!(value, 15, "0_17 octal == 15 decimal");
    Ok(())
}

#[test]
fn octal_0_alone_is_decimal_zero() -> R {
    // A bare `0` is decimal zero, not a degenerate octal.
    let (value, base) = parse_perl_integer("0").ok_or("expected Some")?;
    assert_eq!(base, NumericBase::Decimal, "0 alone must be Decimal");
    assert_eq!(value, 0);
    Ok(())
}

// ── Perl decimal fall-through (8 or 9 after leading 0) ───────────────────────

#[test]
fn zero_eight_is_decimal_eight() -> R {
    // Perl rule: once 8 or 9 appears, the whole literal is decimal.
    let (value, base) = parse_perl_integer("08").ok_or("expected Some")?;
    assert_eq!(base, NumericBase::Decimal, "08 must be Decimal");
    assert_eq!(value, 8);
    Ok(())
}

#[test]
fn zero_one_eight_is_decimal_eighteen() -> R {
    let (value, base) = parse_perl_integer("018").ok_or("expected Some")?;
    assert_eq!(base, NumericBase::Decimal, "018 must be Decimal");
    assert_eq!(value, 18);
    Ok(())
}

// ── Plain decimal ─────────────────────────────────────────────────────────────

#[test]
fn decimal_42() -> R {
    let (value, base) = parse_perl_integer("42").ok_or("expected Some")?;
    assert_eq!(base, NumericBase::Decimal);
    assert_eq!(value, 42);
    Ok(())
}

#[test]
fn decimal_with_underscores() -> R {
    let (value, base) = parse_perl_integer("1_000").ok_or("expected Some")?;
    assert_eq!(base, NumericBase::Decimal);
    assert_eq!(value, 1000);
    Ok(())
}

// ── Hexadecimal ───────────────────────────────────────────────────────────────

#[test]
fn hex_0x1f_is_31() -> R {
    let (value, base) = parse_perl_integer("0x1F").ok_or("expected Some")?;
    assert_eq!(base, NumericBase::Hexadecimal);
    assert_eq!(value, 31);
    Ok(())
}

#[test]
fn hex_uppercase_prefix() -> R {
    let (value, base) = parse_perl_integer("0XFF").ok_or("expected Some")?;
    assert_eq!(base, NumericBase::Hexadecimal);
    assert_eq!(value, 255);
    Ok(())
}

// ── Binary ────────────────────────────────────────────────────────────────────

#[test]
fn binary_0b101_is_5() -> R {
    let (value, base) = parse_perl_integer("0b101").ok_or("expected Some")?;
    assert_eq!(base, NumericBase::Binary);
    assert_eq!(value, 5);
    Ok(())
}

#[test]
fn binary_uppercase_prefix() -> R {
    let (value, base) = parse_perl_integer("0B1111").ok_or("expected Some")?;
    assert_eq!(base, NumericBase::Binary);
    assert_eq!(value, 15);
    Ok(())
}

// ── Explicit octal (0o prefix, Perl 5.34+) ───────────────────────────────────

#[test]
fn explicit_octal_0o17_is_15() -> R {
    let (value, base) = parse_perl_integer("0o17").ok_or("expected Some")?;
    assert_eq!(base, NumericBase::ExplicitOctal);
    assert_eq!(value, 15);
    Ok(())
}

#[test]
fn explicit_octal_uppercase_prefix() -> R {
    let (value, base) = parse_perl_integer("0O777").ok_or("expected Some")?;
    assert_eq!(base, NumericBase::ExplicitOctal);
    assert_eq!(value, 511);
    Ok(())
}

// ── Non-integer inputs return None ───────────────────────────────────────────

#[test]
fn float_literal_returns_none() {
    assert_eq!(parse_perl_integer("1.5"), None, "float must return None");
    assert_eq!(parse_perl_integer("0.5"), None);
}

#[test]
fn empty_string_returns_none() {
    assert_eq!(parse_perl_integer(""), None);
}

#[test]
fn non_numeric_string_returns_none() {
    assert_eq!(parse_perl_integer("abc"), None);
    assert_eq!(parse_perl_integer("0x"), None, "0x with no digits must return None");
    assert_eq!(parse_perl_integer("0b"), None, "0b with no digits must return None");
    assert_eq!(parse_perl_integer("0o"), None, "0o with no digits must return None");
}
