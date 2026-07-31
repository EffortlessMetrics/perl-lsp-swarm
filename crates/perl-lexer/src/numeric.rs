//! Semantic parsing of Perl integer literals.
//!
//! Perl's `perlnumber` rules for integer literals differ from most languages:
//!
//! - `0x…` / `0X…` — hexadecimal
//! - `0b…` / `0B…` — binary
//! - `0o…` / `0O…` — explicit octal (Perl 5.34+)
//! - `0` + only octal digits (0–7) — **legacy octal** (`017` == decimal 15)
//! - `0` + any digit 8 or 9 — treated as **decimal** by Perl
//! - Any other digit sequence — decimal
//!
//! Underscores (`_`) are visual separators and are ignored in all forms.
//!
//! This module exposes [`parse_perl_integer`] so value-aware consumers
//! (native critic, hover, constant folding) can obtain a canonical `(value,
//! base)` pair without re-deriving these rules ad-hoc.

/// The numeric base of a Perl integer literal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumericBase {
    /// Base-10 (`42`, `018`, `08` — Perl falls back to decimal when 8/9 appear).
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
/// `"0x1F"`, `"1_000"`). Returns `None` for non-integer or unparseable input
/// (floating-point, empty string, overflow).
///
/// # Examples
///
/// ```
/// use perl_lexer::numeric::{NumericBase, parse_perl_integer};
///
/// assert_eq!(parse_perl_integer("017"),   Some((15,  NumericBase::LegacyOctal)));
/// assert_eq!(parse_perl_integer("0755"),  Some((493, NumericBase::LegacyOctal)));
/// assert_eq!(parse_perl_integer("018"),   Some((18,  NumericBase::Decimal)));
/// assert_eq!(parse_perl_integer("08"),    Some((8,   NumericBase::Decimal)));
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

    // Strip visual-separator underscores before interpreting digits.
    let stripped: String = s.chars().filter(|&c| c != '_').collect();
    let s = stripped.as_str();

    if s.starts_with("0x") || s.starts_with("0X") {
        let digits = &s[2..];
        if digits.is_empty() {
            return None;
        }
        return u64::from_str_radix(digits, 16).ok().map(|v| (v, NumericBase::Hexadecimal));
    }

    if s.starts_with("0b") || s.starts_with("0B") {
        let digits = &s[2..];
        if digits.is_empty() {
            return None;
        }
        return u64::from_str_radix(digits, 2).ok().map(|v| (v, NumericBase::Binary));
    }

    if s.starts_with("0o") || s.starts_with("0O") {
        let digits = &s[2..];
        if digits.is_empty() {
            return None;
        }
        return u64::from_str_radix(digits, 8).ok().map(|v| (v, NumericBase::ExplicitOctal));
    }

    if s.starts_with('0') && s.len() > 1 {
        let digits = &s[1..];
        if digits.bytes().all(|b| matches!(b, b'0'..=b'7')) {
            // All digits are in the octal range — legacy octal.
            return u64::from_str_radix(digits, 8).ok().map(|v| (v, NumericBase::LegacyOctal));
        }
        // 8 or 9 present — Perl falls back to decimal for this literal.
        return s.parse::<u64>().ok().map(|v| (v, NumericBase::Decimal));
    }

    // Plain decimal (or a non-integer — parse returns Err and we return None).
    s.parse::<u64>().ok().map(|v| (v, NumericBase::Decimal))
}
