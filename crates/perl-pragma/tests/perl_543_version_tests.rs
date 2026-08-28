//! Regression coverage for Perl 5.43 development-release version spellings.

use perl_ast::SourceLocation;
use perl_ast::ast::{Node, NodeKind};
use perl_pragma::{PerlVersion, PragmaTracker, features_enabled_by_version, parse_perl_version};

#[test]
fn perl_543_release_spellings_select_minor_43() {
    for spelling in ["5.043011", "5.043_011", "5.043011_01", "v5.43.11", "5.43.11"] {
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
        ("5.043999", PerlVersion::new(5, 43)),
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
    for spelling in
        ["5.043011_", "5.043011_foo", "5.043011_01_02", "5.043011_01x", "5.043011x", "5.043abc"]
    {
        assert_eq!(parse_perl_version(spelling), None);
    }
}

#[test]
fn numeric_decimal_subminor_suffixes_remain_supported() {
    for (spelling, expected) in
        [("5.043011_01", PerlVersion::new(5, 43)), ("5.012_001", PerlVersion::new(5, 12))]
    {
        assert_eq!(parse_perl_version(spelling), Some(expected), "{spelling}");
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
fn perl_543_decimal_release_updates_the_caller_pragma_state() {
    let version_use = Node::new(
        NodeKind::Use { module: "5.043011".to_string(), args: vec![], has_filter_risk: false },
        SourceLocation { start: 0, end: 9 },
    );
    let program = Node::new(
        NodeKind::Program { statements: vec![version_use] },
        SourceLocation { start: 0, end: 9 },
    );

    let state = PragmaTracker::final_state(&PragmaTracker::build(&program));

    assert!(state.strict_vars);
    assert!(state.strict_subs);
    assert!(state.strict_refs);
    assert!(state.warnings);
    assert!(state.has_feature("signatures"));
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
