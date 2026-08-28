//! Regression coverage for Perl 5.43 development-release version spellings.

use perl_pragma::{PerlVersion, features_enabled_by_version, parse_perl_version};

#[test]
fn perl_543_release_spellings_select_minor_43() {
    for spelling in ["5.043011", "5.043_011", "v5.43.11", "5.43.11"] {
        assert_eq!(
            parse_perl_version(spelling),
            Some(PerlVersion::new(5, 43)),
            "{spelling} must select the Perl 5.43 release line"
        );
    }
}

#[test]
fn decimal_subminor_versions_do_not_inflate_the_minor() {
    for (spelling, expected) in [
        ("5.010001", PerlVersion::new(5, 10)),
        ("5.036001", PerlVersion::new(5, 36)),
        ("5.043011", PerlVersion::new(5, 43)),
    ] {
        assert_eq!(
            parse_perl_version(spelling),
            Some(expected),
            "{spelling} must retain only the governed major/minor projection"
        );
    }
}

#[test]
fn malformed_decimal_subminor_tails_remain_rejected() {
    for spelling in ["5.043011x", "5.043abc"] {
        assert_eq!(parse_perl_version(spelling), None);
    }
}

#[test]
fn perl_543_decimal_release_uses_the_current_5_42_bundle_membership() {
    assert_eq!(
        parse_perl_version("5.043011").map(features_enabled_by_version),
        Some(features_enabled_by_version(PerlVersion::new(5, 42)))
    );
}

#[test]
fn existing_short_and_dotted_forms_keep_their_current_projection() {
    for (spelling, expected) in [
        ("5.036", PerlVersion::new(5, 36)),
        ("5.10", PerlVersion::new(5, 10)),
        ("v5.43.11", PerlVersion::new(5, 43)),
    ] {
        assert_eq!(parse_perl_version(spelling), Some(expected));
    }
}
