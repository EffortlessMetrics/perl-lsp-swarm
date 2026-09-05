//! The regex-analysis code block (PL1000-PL1099) introduced for #7024.
//!
//! These codes project the parser-retained canonical regex analysis into stable
//! client-facing identities. The block is the first four-digit range, which makes
//! one property load-bearing: codes must be compared exactly, never by prefix.

use perl_diagnostics::codes::{DiagnosticCategory, DiagnosticCode, DiagnosticSeverity};

const REGEX_ANALYSIS_BLOCK: [(DiagnosticCode, &str, DiagnosticSeverity); 8] = [
    (DiagnosticCode::RegexBacktrackingRisk, "PL1000", DiagnosticSeverity::Warning),
    (DiagnosticCode::RegexAnalysisLimit, "PL1001", DiagnosticSeverity::Information),
    (DiagnosticCode::RegexModifierInvalid, "PL1002", DiagnosticSeverity::Error),
    (DiagnosticCode::RegexModifierNoEffect, "PL1003", DiagnosticSeverity::Warning),
    (DiagnosticCode::RegexModifierUnavailable, "PL1004", DiagnosticSeverity::Warning),
    (DiagnosticCode::RegexCaptureInvalid, "PL1005", DiagnosticSeverity::Error),
    (DiagnosticCode::RegexCaptureUnavailable, "PL1006", DiagnosticSeverity::Warning),
    (DiagnosticCode::RegexAnalysisIncomplete, "PL1007", DiagnosticSeverity::Information),
];

/// A published code is a compatibility surface: clients store it in suppression
/// config, so `as_str` and `parse_code` must agree in both directions forever.
#[test]
fn every_regex_analysis_code_is_stable_and_round_trips() {
    for (code, expected, _) in REGEX_ANALYSIS_BLOCK {
        assert_eq!(code.as_str(), expected, "{code:?} must keep its stable identity");
        assert_eq!(
            DiagnosticCode::parse_code(expected),
            Some(code),
            "{expected} must parse back to {code:?}"
        );
    }
}

/// The catalog owns severity, category, documentation URL, and hint. A code
/// missing any of them reaches the client as an unexplained identifier.
#[test]
fn every_regex_analysis_code_carries_catalog_metadata() {
    for (code, expected, severity) in REGEX_ANALYSIS_BLOCK {
        assert_eq!(code.severity(), severity, "{expected} severity is catalog-owned");
        // The URL is published to clients as `codeDescription.href`, so a code that
        // silently points at another code's page sends users to the wrong
        // explanation. Assert the exact URL, not merely that one exists.
        assert_eq!(
            code.documentation_url(),
            Some(format!("https://docs.perl-lsp.org/errors/{expected}").as_str()),
            "{expected} must resolve to its own documentation page"
        );
        assert_eq!(
            code.category(),
            DiagnosticCategory::RegexAnalysis,
            "{expected} belongs to the regex-analysis category"
        );
        assert!(
            code.documentation_url().is_some(),
            "{expected} must have a documentation URL like every other published code"
        );
        assert!(
            code.context_hint().is_some(),
            "{expected} must explain itself; a code with no hint is a code users cannot act on"
        );
    }
}

/// Executable pattern code deliberately keeps its established security identity
/// rather than moving into this block. Renumbering a shipped code would break
/// existing suppression configuration for no benefit.
#[test]
fn embedded_regex_code_is_not_renumbered_into_the_regex_block() {
    assert_eq!(DiagnosticCode::SecurityEmbeddedRegexCode.as_str(), "PL609");
    assert_eq!(DiagnosticCode::SecurityEmbeddedRegexCode.category(), DiagnosticCategory::Security);
}

/// The known and intentional consequence of widening to four digits: `PL100` is a
/// prefix of `PL1000`, and they are unrelated codes in unrelated categories.
///
/// This test exists so the collision is a recorded decision rather than a surprise.
/// Any consumer classifying codes by prefix is wrong; compare codes exactly.
#[test]
fn four_digit_codes_collide_with_three_digit_prefixes_by_design() {
    let three_digit = DiagnosticCode::MissingStrict;
    let four_digit = DiagnosticCode::RegexBacktrackingRisk;

    assert_eq!(three_digit.as_str(), "PL100");
    assert_eq!(four_digit.as_str(), "PL1000");
    assert!(
        four_digit.as_str().starts_with(three_digit.as_str()),
        "the prefix relationship is real, which is exactly why prefix matching is unsafe"
    );
    assert_ne!(
        three_digit.category(),
        four_digit.category(),
        "and the two codes mean entirely different things"
    );

    // Exact comparison, the supported way, keeps them apart.
    assert_eq!(DiagnosticCode::parse_code("PL100"), Some(three_digit));
    assert_eq!(DiagnosticCode::parse_code("PL1000"), Some(four_digit));
}
