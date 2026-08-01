// Regression tests for issue #5042 Slice 2 — legacy octal semantics.
//
// Perl's `perlnumber` rules treat `0755` as octal (493 decimal), not decimal
// 755. A leading-zero literal containing `8` or `9` is illegal in Perl — it
// fails compilation with `Illegal octal digit`, and is not a decimal fallback.
// This file proves that `parse_perl_integer` correctly classifies each literal
// form so value-aware consumers (critic, hover, constant folding) do not need
// to re-derive the rules ad-hoc.

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

// ── Illegal octal digits (8 or 9 after a leading 0) ──────────────────────────
//
// Perl does NOT fall back to decimal here. Both of these abort compilation:
//
//     $ perl -e 'my $x = 08;'
//     Illegal octal digit '8' at -e line 1, at end of line
//     $ perl -e 'my $x = 018;'
//     Illegal octal digit '8' at -e line 1, at end of line
//
// Verified against perl v5.38.2. Returning `Some((8, Decimal))` here would
// manufacture a value for a program that cannot compile, which is precisely
// what a canonical helper must not do — every consumer (native critic
// `octal_literal_to_decimal`, hover constant-folding) would then report a
// confident, wrong number for source Perl rejects outright.

#[test]
fn zero_eight_is_not_a_legal_literal() {
    assert_eq!(
        parse_perl_integer("08"),
        None,
        "08 is `Illegal octal digit` in Perl, not decimal 8"
    );
}

#[test]
fn zero_one_eight_is_not_a_legal_literal() {
    assert_eq!(
        parse_perl_integer("018"),
        None,
        "018 is `Illegal octal digit` in Perl, not decimal 18"
    );
}

#[test]
fn illegal_octal_digit_rejected_regardless_of_position() {
    // The offending digit may sit anywhere in the literal, and `9` is as
    // illegal as `8`. A wrong implementation that only checked the first digit
    // after the zero would pass the two tests above and fail these.
    assert_eq!(parse_perl_integer("0189"), None, "trailing 9 is still illegal");
    assert_eq!(parse_perl_integer("0781"), None, "8 in the middle is still illegal");
    assert_eq!(parse_perl_integer("09"), None, "9 is illegal too");
    assert_eq!(parse_perl_integer("0_18"), None, "underscores do not launder an illegal digit");
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

#[test]
fn overflowing_literal_returns_none() {
    // The helper returns u64; anything wider has no representable value, and a
    // silent wrap would be worse than declining to answer.
    assert_eq!(
        parse_perl_integer("18446744073709551616"),
        None,
        "u64::MAX + 1 must return None, not wrap"
    );
    assert_eq!(parse_perl_integer("0x1_0000_0000_0000_0000"), None, "65-bit hex must return None");
    assert_eq!(parse_perl_integer("02000000000000000000000"), None, "65-bit octal must be None");
}

#[test]
fn signed_token_text_returns_none() {
    // `from_str_radix` and `str::parse` both accept a leading sign, but the
    // lexer emits `-` as a separate unary operator — a signed number token is
    // never legitimate input, so accepting it would mask a caller bug.
    assert_eq!(parse_perl_integer("-42"), None);
    assert_eq!(parse_perl_integer("+42"), None);
    assert_eq!(parse_perl_integer("0x+1F"), None);
}

// ── Underscore placement is unconstrained in Perl ────────────────────────────
//
// Review on this PR proposed rejecting "malformed" separator placement
// (`1__000`, `1000_`, `0x_1F`). Perl accepts all three:
//
//     $ perl -e 'print 1__000, " ", 1000_, " ", 0x_1F'
//     1000 1000 31
//
// Verified against perl v5.38.2. Stripping every underscore is therefore the
// behaviour that matches the language, and these cases pin it so a later
// "validation" change cannot quietly diverge from Perl.

#[test]
fn underscores_may_appear_anywhere_inside_a_literal() -> R {
    assert_eq!(parse_perl_integer("1__000"), Some((1000, NumericBase::Decimal)));
    assert_eq!(parse_perl_integer("1000_"), Some((1000, NumericBase::Decimal)));
    assert_eq!(parse_perl_integer("0x_1F"), Some((31, NumericBase::Hexadecimal)));
    assert_eq!(parse_perl_integer("0x1_F"), Some((31, NumericBase::Hexadecimal)));
    assert_eq!(parse_perl_integer("0b1_01"), Some((5, NumericBase::Binary)));
    assert_eq!(parse_perl_integer("0o1_7"), Some((15, NumericBase::ExplicitOctal)));
    Ok(())
}
