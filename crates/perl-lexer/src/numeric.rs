//! Semantic parsing of Perl integer literals.
//!
//! Perl's `perlnumber` rules for integer literals differ from most languages:
//!
//! - `0x…` / `0X…` — hexadecimal
//! - `0b…` / `0B…` — binary
//! - `0o…` / `0O…` — explicit octal (Perl 5.34+)
//! - `0` + only octal digits (0–7) — **legacy octal** (`017` == decimal 15)
//! - `0` + any digit 8 or 9 — **not a literal at all**; Perl refuses to
//!   compile the program with `Illegal octal digit`
//! - Any other digit sequence — decimal
//!
//! Underscores (`_`) are visual separators and are ignored in all forms.
//! Perl places no constraint on where they appear inside a literal: `1__000`,
//! `1000_`, and `0x_1F` are all accepted and equal to `1000`, `1000`, and `31`
//! respectively. Verified against perl v5.38.2.
//!
//! This module exposes [`parse_perl_integer`] so value-aware consumers
//! (native critic, hover, constant folding) can obtain a canonical `(value,
//! base)` pair without re-deriving these rules ad-hoc.

/// The numeric base of a Perl integer literal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumericBase {
    /// Base-10 (`42`, `1_000`). A leading `0` never yields decimal: it is
    /// either legacy octal or an illegal literal.
    Decimal,
    /// Legacy octal: leading `0` followed by digits 0–7 only (`017` == 15).
    LegacyOctal,
    /// Explicit octal with `0o` / `0O` prefix (`0o17` == 15, Perl 5.34+).
    ExplicitOctal,
    /// Binary with `0b` / `0B` prefix.
    Binary,
    /// Hexadecimal with `0x` / `0X` prefix.
    Hexadecimal,
}

/// Parse a Perl integer literal and return its value and base.
///
/// The input is the raw token text as emitted by the lexer (e.g. `"017"`,
/// `"0x1F"`, `"1_000"`). Returns `None` for input that is not a valid Perl
/// integer literal: floating-point, empty string, overflow, or a leading-zero
/// literal containing an `8` or `9` (which Perl rejects at compile time with
/// `Illegal octal digit`).
///
/// # Examples
///
/// ```
/// use perl_lexer::numeric::{NumericBase, parse_perl_integer};
///
/// assert_eq!(parse_perl_integer("017"),   Some((15,  NumericBase::LegacyOctal)));
/// assert_eq!(parse_perl_integer("0755"),  Some((493, NumericBase::LegacyOctal)));
/// // `018` / `08` are not valid Perl: `perl -e 'my $x = 018'` is a compile error.
/// assert_eq!(parse_perl_integer("018"),   None);
/// assert_eq!(parse_perl_integer("08"),    None);
/// assert_eq!(parse_perl_integer("0"),     Some((0,   NumericBase::Decimal)));
/// assert_eq!(parse_perl_integer("42"),    Some((42,  NumericBase::Decimal)));
/// assert_eq!(parse_perl_integer("0x1F"),  Some((31,  NumericBase::Hexadecimal)));
/// assert_eq!(parse_perl_integer("0b101"), Some((5,   NumericBase::Binary)));
/// assert_eq!(parse_perl_integer("0o17"),  Some((15,  NumericBase::ExplicitOctal)));
/// assert_eq!(parse_perl_integer("1_000"), Some((1000, NumericBase::Decimal)));
/// assert_eq!(parse_perl_integer("0_17"),  Some((15,  NumericBase::LegacyOctal)));
/// assert_eq!(parse_perl_integer("1.5"),   None);
/// assert_eq!(parse_perl_integer(""),      None);
/// ```
pub fn parse_perl_integer(s: &str) -> Option<(u64, NumericBase)> {
    if s.is_empty() {
        return None;
    }

    // Strip visual-separator underscores before interpreting digits. Perl
    // imposes no placement rule on them, so no validation is needed here — but
    // avoid the allocation for the overwhelmingly common underscore-free case.
    let stripped;
    let s: &str = if s.contains('_') {
        stripped = s.chars().filter(|&c| c != '_').collect::<String>();
        stripped.as_str()
    } else {
        s
    };

    // `from_str_radix` accepts a leading `+`/`-`; Perl literals never carry a
    // sign in the token text (it lexes as a separate unary operator), so reject
    // it rather than silently accepting `0x+1`.
    fn parse_prefixed(digits: &str, radix: u32, base: NumericBase) -> Option<(u64, NumericBase)> {
        if digits.is_empty() || digits.starts_with(['+', '-']) {
            return None;
        }
        u64::from_str_radix(digits, radix).ok().map(|v| (v, base))
    }

    if let Some(digits) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        return parse_prefixed(digits, 16, NumericBase::Hexadecimal);
    }

    if let Some(digits) = s.strip_prefix("0b").or_else(|| s.strip_prefix("0B")) {
        return parse_prefixed(digits, 2, NumericBase::Binary);
    }

    if let Some(digits) = s.strip_prefix("0o").or_else(|| s.strip_prefix("0O")) {
        return parse_prefixed(digits, 8, NumericBase::ExplicitOctal);
    }

    if let Some(digits) = s.strip_prefix('0')
        && !digits.is_empty()
    {
        // A leading zero means octal, and only octal. Perl does not fall back
        // to decimal when an 8 or 9 appears — it refuses to compile the program
        // at all (`Illegal octal digit '8'`, verified on perl v5.38.2). Callers
        // must not be handed a value for a literal that cannot exist.
        return parse_prefixed(digits, 8, NumericBase::LegacyOctal);
    }

    // Plain decimal (or a non-integer — parse returns Err and we return None).
    if s.starts_with(['+', '-']) {
        return None;
    }
    s.parse::<u64>().ok().map(|v| (v, NumericBase::Decimal))
}
