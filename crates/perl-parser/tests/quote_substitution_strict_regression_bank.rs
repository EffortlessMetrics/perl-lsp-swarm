//! Strict-substitution error regression bank.
//!
//! Companion to `quote_substitution_regression_bank`-style coverage: asserts the
//! structured `SubstitutionError` surface of `extract_substitution_parts_strict`,
//! and in particular that `SubstitutionError::InvalidDelimiter` reaches parity
//! with its sibling `TransliterationError::InvalidDelimiter` (issue #2823).
use perl_parser::quote_parser::{SubstitutionError, extract_substitution_parts_strict};

#[test]
fn substitution_regression_bank_valid_cases() {
    let cases = [
        ("s/a/b/", ("a", "b", "")),
        ("s/foo/bar/gi", ("foo", "bar", "gi")),
        ("s{pattern}{replacement}", ("pattern", "replacement", "")),
        ("s[foo]{bar}r", ("foo", "bar", "r")),
        // '#' is a valid non-paired, non-alphanumeric delimiter
        ("s#a#b#", ("a", "b", "")),
        // whitespace between `s` and a paired delimiter is allowed
        ("s {a}{b}", ("a", "b", "")),
    ];

    for (input, expected) in cases {
        let actual = extract_substitution_parts_strict(input);
        assert_eq!(
            actual,
            Ok((expected.0.to_string(), expected.1.to_string(), expected.2.to_string())),
            "unexpected strict parse for {input:?}"
        );
    }
}

#[test]
fn substitution_regression_bank_strict_errors() {
    let cases = [
        ("s/foo/bar/z", SubstitutionError::InvalidModifier('z')),
        // Alphanumeric delimiter after `s` is invalid (Perl forbids it), mirroring
        // `tr a/b/` -> TransliterationError::InvalidDelimiter('a').
        ("sabcXbarX", SubstitutionError::InvalidDelimiter('a')),
        // Paired search delimiter followed by an alphanumeric replacement delimiter
        // is an invalid delimiter, not merely a missing replacement (mirrors
        // `tr{abc}xyz` -> InvalidDelimiter('x')).
        ("s{foo}bar", SubstitutionError::InvalidDelimiter('b')),
        ("s/foo/bar", SubstitutionError::MissingClosingDelimiter),
        ("s{foo}{bar", SubstitutionError::MissingClosingDelimiter),
        ("s", SubstitutionError::MissingDelimiter),
    ];

    for (input, expected) in cases {
        let actual = extract_substitution_parts_strict(input);
        assert_eq!(actual, Err(expected), "expected strict error for {input:?}");
    }
}
