//! Unicode 17 XID data/query authority contract (#13326).
//!
//! These tests pin the lower name-classification authority only. They do not
//! tokenize source or parse regex group names (#13327 / #13328).
//!
//! External oracles:
//! - Unicode 17.0 `DerivedCoreProperties.txt` (`XID_Start` / `XID_Continue`)
//! - Perl 5.44 `perl5440delta` (Unicode 17.0; `\w` vs XID enforcement)
//! - `unicode-ident::UNICODE_VERSION` (inspectable table identity)

use perl_lexer::unicode_xid::{
    PerlXidProfile, UnicodeVersion, UnicodeXidApplicability, UnicodeXidDataSource,
    unicode_data_version, unicode_xid_data_source, xid_policy_for_perl,
};
use perl_lexer::unicode_xid::{is_xid_continue, is_xid_start};

/// Sidetic letter N01. Assigned and `XID_Start` in Unicode 17.0; unassigned
/// in Unicode 16.0 (`DerivedCoreProperties-17.0.0.txt`: `10940..10959`).
const UNICODE_17_XID_START: char = '\u{10940}';

/// Tolong Siki letter I. Second Unicode 17.0 script discriminator
/// (`11DB0..11DD8 ; XID_Start`).
const UNICODE_17_XID_START_TOLONG_SIKI: char = '\u{11DB0}';

/// Unassigned Sidetic-block code point in Unicode 17.0 (`U+1095A`).
const UNICODE_17_UNASSIGNED: char = '\u{1095A}';

/// Combining acute accent: `XID_Continue` only.
const XID_CONTINUE_ONLY_MARK: char = '\u{0301}';

/// CIRCLED LATIN CAPITAL LETTER A. Other_Alphabetic, so Unicode `\w` /
/// `char::is_alphanumeric` accept it, but it is not XID. perl5440delta names
/// enclosed Latin letters as the bulk of the 5.44 `\w`-but-not-XID set.
const WORD_BUT_NOT_XID: char = '\u{24B6}';

/// SCRIPT CAPITAL P (`U+2118`). `XID_Start` via Other_ID_Start, but Symbol
/// so not alphanumeric. Proves the oracle is not `is_alphanumeric`.
const XID_BUT_NOT_ALPHANUMERIC: char = '\u{2118}';

#[test]
fn unicode_17_identity_is_exact_and_machine_readable() {
    let version = unicode_data_version();
    assert_eq!(version, UnicodeVersion::UNICODE_17_0);
    assert_eq!(version.as_tuple(), (17, 0, 0));
    assert_eq!(version.to_string(), "17.0.0");
    assert_eq!(unicode_xid_data_source(), UnicodeXidDataSource::UnicodeIdent);
}

#[test]
fn perl_5_44_selects_reviewed_unicode_17_policy() {
    assert_eq!(xid_policy_for_perl(PerlXidProfile::PERL_5_44), UnicodeXidApplicability::Exact);
    assert_eq!(xid_policy_for_perl(PerlXidProfile::new(5, 44)), UnicodeXidApplicability::Exact);
}

#[test]
fn older_and_unknown_profiles_are_unsupported_not_silent_unicode_17() {
    for profile in [
        PerlXidProfile::new(5, 42),
        PerlXidProfile::new(5, 36),
        PerlXidProfile::new(5, 8),
        PerlXidProfile::new(5, 45),
        PerlXidProfile::new(0, 0),
    ] {
        assert_eq!(
            xid_policy_for_perl(profile),
            UnicodeXidApplicability::Unsupported,
            "profile {}.{} must not inherit unreviewed Unicode 17 exactness",
            profile.major,
            profile.minor
        );
    }
}

#[test]
fn unicode_17_sensitive_points_distinguish_the_configured_table() {
    assert!(
        is_xid_start(UNICODE_17_XID_START),
        "U+10940 Sidetic N01 is XID_Start in Unicode 17; a Unicode 16 table rejects it"
    );
    assert!(is_xid_continue(UNICODE_17_XID_START));
    assert!(
        is_xid_start(UNICODE_17_XID_START_TOLONG_SIKI),
        "U+11DB0 Tolong Siki I is XID_Start in Unicode 17"
    );
    assert!(
        !is_xid_start(UNICODE_17_UNASSIGNED) && !is_xid_continue(UNICODE_17_UNASSIGNED),
        "U+1095A is unassigned in Unicode 17.0 and must not be XID"
    );
}

#[test]
fn xid_start_continue_truth_table() {
    let rows: &[(char, bool, bool, &str)] = &[
        ('A', true, true, "ASCII letter"),
        ('z', true, true, "ASCII letter"),
        ('λ', true, true, "non-ASCII letter"),
        ('1', false, true, "ASCII digit is continue-only"),
        ('_', false, true, "underscore is XID_Continue, not XID_Start"),
        (XID_CONTINUE_ONLY_MARK, false, true, "combining mark"),
        ('١', false, true, "ARABIC-INDIC DIGIT ONE continue-only"),
        (UNICODE_17_XID_START, true, true, "Unicode 17 Sidetic"),
        (XID_BUT_NOT_ALPHANUMERIC, true, true, "U+2118 Other_ID_Start"),
        (WORD_BUT_NOT_XID, false, false, "circled A is not XID"),
        (' ', false, false, "space"),
        ('-', false, false, "hyphen"),
        ('\0', false, false, "NUL"),
        ('\n', false, false, "LF"),
        ('\t', false, false, "TAB"),
        (UNICODE_17_UNASSIGNED, false, false, "unassigned Sidetic"),
    ];

    for &(ch, start, cont, label) in rows {
        assert_eq!(is_xid_start(ch), start, "XID_Start {label} U+{:04X}", ch as u32);
        assert_eq!(is_xid_continue(ch), cont, "XID_Continue {label} U+{:04X}", ch as u32);
        if start {
            assert!(cont, "XID_Start implies XID_Continue ({label})");
        }
    }
}

#[test]
fn start_versus_continue_only_discriminator() {
    assert!(!is_xid_start('1') && is_xid_continue('1'));
    assert!(!is_xid_start('_') && is_xid_continue('_'));
    assert!(!is_xid_start(XID_CONTINUE_ONLY_MARK) && is_xid_continue(XID_CONTINUE_ONLY_MARK));
    assert!(is_xid_start('A') && is_xid_continue('A'));
}

#[test]
fn word_and_alphanumeric_predicates_cannot_satisfy_xid() {
    assert!(WORD_BUT_NOT_XID.is_alphanumeric(), "control: circled A is alphanumeric/Word-class");
    assert!(
        !is_xid_start(WORD_BUT_NOT_XID) && !is_xid_continue(WORD_BUT_NOT_XID),
        "replacing XID with is_alphanumeric would accept U+24B6"
    );
    assert!(
        is_xid_start(XID_BUT_NOT_ALPHANUMERIC) && !XID_BUT_NOT_ALPHANUMERIC.is_alphanumeric(),
        "replacing XID with is_alphanumeric would reject U+2118"
    );
    assert!(
        is_xid_continue(XID_CONTINUE_ONLY_MARK) && !XID_CONTINUE_ONLY_MARK.is_alphanumeric(),
        "marks are XID_Continue and not alphanumeric"
    );
}

#[test]
fn ascii_controls_are_not_xid() {
    for ch in ['\0', '\u{01}', '\u{07}', '\t', '\n', '\r', '\u{1B}', '\u{7F}', ' '] {
        assert!(!is_xid_start(ch), "ASCII control/space U+{:04X} is not XID_Start", ch as u32);
        assert!(
            !is_xid_continue(ch),
            "ASCII control/space U+{:04X} is not XID_Continue",
            ch as u32
        );
    }
}

#[test]
fn stale_version_identity_cannot_masquerade_as_unicode_17() {
    let actual = unicode_data_version();
    assert_ne!(
        actual.as_tuple(),
        (16, 0, 0),
        "stale Unicode 16 identity must not be reported as the executable table"
    );
    assert_ne!(actual.as_tuple(), (15, 0, 0));
    assert_ne!(actual.as_tuple(), (18, 0, 0));
    assert_eq!(actual, UnicodeVersion::UNICODE_17_0);
    assert_eq!(
        xid_policy_for_perl(PerlXidProfile::PERL_5_44),
        UnicodeXidApplicability::Exact,
        "Perl 5.44 Exact requires the executable table to be Unicode 17.0.0, not a newer or older table"
    );
}
