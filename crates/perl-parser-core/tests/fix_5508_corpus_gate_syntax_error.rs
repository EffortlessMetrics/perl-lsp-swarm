/// Regression proof for issue #5508.
///
/// `RecoverySalvageProfile::from_parse` previously classified any file whose
/// diagnostics contained only blocking-severity variants other than
/// `ParseError::Recovered` (e.g. `SyntaxError`) as `Clean`.  The corpus gate
/// therefore could not detect parser changes that emitted `SyntaxError` on
/// valid Perl, silently reporting a green ratchet for a whole class of
/// regression.
///
/// This test file contains discriminating checks that:
/// 1. Confirm a parse producing only `SyntaxError` diagnostics lands in
///    `StructuredRecoveryOnly`, not `Clean`.
/// 2. Confirm blocking diagnostics do not inflate the recovered count.
/// 3. Confirm that a clean parse still returns `Clean`.
/// 4. Confirm that the existing `Recovered` path still yields
///    `StructuredRecoveryOnly`.
use perl_parser_core::syntax::error::{RecoveryKind, RecoverySite};
use perl_parser_core::{ParseError, Parser, RecoverySalvageClass, RecoverySalvageProfile};
use perl_tdd_support::must;

/// Build a salvage profile from a freshly-parsed clean snippet and an
/// explicit diagnostic list injected for testing.  Returns the profile.
fn profile_with_diagnostics(
    source: &str,
    extra_diagnostics: &[ParseError],
) -> RecoverySalvageProfile {
    let mut parser = Parser::new(source);
    let ast = must(parser.parse());
    let mut diagnostics: Vec<ParseError> = parser.errors().to_vec();
    diagnostics.extend_from_slice(extra_diagnostics);
    RecoverySalvageProfile::from_parse(&ast, &diagnostics, false)
}

// ---------------------------------------------------------------------------
// 1. SyntaxError → StructuredRecoveryOnly, not Clean
// ---------------------------------------------------------------------------

#[test]
fn syntax_error_only_is_not_clean() {
    let syntax_err = ParseError::syntax("test injection", 0);
    let profile = profile_with_diagnostics("my $x = 1;", &[syntax_err]);
    assert_ne!(
        profile.class,
        RecoverySalvageClass::Clean,
        "A SyntaxError diagnostic must make the file non-Clean; got {:?}",
        profile.class
    );
    assert_eq!(
        profile.class,
        RecoverySalvageClass::StructuredRecoveryOnly,
        "A SyntaxError-only file must land in StructuredRecoveryOnly; got {:?}",
        profile.class
    );
}

#[test]
fn syntax_error_does_not_count_as_recovered_diagnostic() {
    let syntax_err = ParseError::syntax("test injection", 0);
    let profile = profile_with_diagnostics("my $x = 1;", &[syntax_err]);
    assert_eq!(
        profile.recovered_count, 0,
        "recovered_count must remain 0 when there are no Recovered diagnostics: {:?}",
        profile
    );
}

#[test]
fn multiple_syntax_errors_all_counted() {
    let diagnostics = vec![
        ParseError::syntax("first error", 0),
        ParseError::syntax("second error", 5),
        ParseError::UnexpectedEof,
    ];
    let profile = profile_with_diagnostics("", &diagnostics);
    assert_eq!(
        profile.recovered_count, 0,
        "blocking diagnostics must not be reported as recovered: {:?}",
        profile
    );
    assert_eq!(profile.class, RecoverySalvageClass::StructuredRecoveryOnly);
}

// ---------------------------------------------------------------------------
// 2. Advisory diagnostics do not make a file non-Clean
// ---------------------------------------------------------------------------

#[test]
fn advisory_only_stays_clean() {
    let advisory = ParseError::Advisory { message: "nested quantifier".to_string(), location: 0 };
    let profile = profile_with_diagnostics("my $x = 1;", &[advisory]);
    assert_eq!(
        profile.class,
        RecoverySalvageClass::Clean,
        "Advisory-only files must remain Clean: {:?}",
        profile.class
    );
}

// ---------------------------------------------------------------------------
// 3. Clean parse still returns Clean
// ---------------------------------------------------------------------------

#[test]
fn clean_parse_stays_clean() {
    let profile = profile_with_diagnostics("my $x = 42;", &[]);
    assert_eq!(
        profile.class,
        RecoverySalvageClass::Clean,
        "A parse with no diagnostics must be Clean: {:?}",
        profile.class
    );
    assert_eq!(profile.recovered_count, 0);
}

// ---------------------------------------------------------------------------
// 4. Recovered path still yields StructuredRecoveryOnly
// ---------------------------------------------------------------------------

#[test]
fn recovered_diagnostic_still_yields_structured_recovery_only() {
    let recovered = ParseError::Recovered {
        site: RecoverySite::ArgList,
        kind: RecoveryKind::InsertedCloser,
        location: 0,
    };
    let profile = profile_with_diagnostics("my $x = 1;", &[recovered]);
    assert_eq!(
        profile.class,
        RecoverySalvageClass::StructuredRecoveryOnly,
        "A Recovered diagnostic must still give StructuredRecoveryOnly: {:?}",
        profile.class
    );
    assert_eq!(profile.recovered_count, 1);
}

// ---------------------------------------------------------------------------
// 5. Live parser: unterminated heredoc emits SyntaxError, classified correctly
// ---------------------------------------------------------------------------

#[test]
fn unterminated_heredoc_is_not_clean() {
    // An unterminated heredoc emits ParseError::SyntaxError (not Recovered),
    // so the old gate would have reported it as Clean.  The fix must surface it.
    let src = "my $text = <<END;\nsome content\n";
    let mut parser = Parser::new(src);
    let ast = must(parser.parse());
    let diagnostics = parser.errors().to_vec();
    let profile = RecoverySalvageProfile::from_parse(&ast, &diagnostics, false);

    let has_syntax_error = diagnostics.iter().any(|e| matches!(e, ParseError::SyntaxError { .. }));

    if has_syntax_error {
        // The main regression: was Clean before the fix.
        assert_ne!(
            profile.class,
            RecoverySalvageClass::Clean,
            "Unterminated heredoc with SyntaxError must not be Clean: {:?}",
            profile.class
        );
        assert_eq!(
            profile.recovered_count, 0,
            "a SyntaxError path must not be reported as structured recovery: {:?}",
            profile
        );
    }
    // If the parser emitted a Recovered instead (implementation detail),
    // it should still be non-Clean — both paths land in StructuredRecoveryOnly.
    assert_ne!(
        profile.class,
        RecoverySalvageClass::Clean,
        "An unterminated heredoc must never be classified Clean: {:?}",
        profile.class
    );
}
