//! Property-based tests for `perl-regex` validation functions.
//!
//! Verifies that all public APIs are panic-free on arbitrary input and that
//! key invariants hold: determinism, source-ordered capture declarations, and
//! conservative code-execution detection.

use perl_regex::{RegexAnalyzer, RegexValidator};
use perl_regex::analyzer::{CaptureLanguageProfile, EffectiveModifiers};
use proptest::prelude::*;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// A generator for printable ASCII strings (no NUL bytes), keeping lengths
/// short so shrinking stays fast.
fn ascii_pattern() -> impl Strategy<Value = String> {
    // bytes are constrained to printable ASCII, so we can build the String
    // directly without any fallible UTF-8 validation step
    prop::collection::vec(0x20u8..0x7fu8, 0..128)
        .prop_map(|bytes| bytes.into_iter().map(|b| b as char).collect())
}

/// A generator for typical regex modifier strings.
fn modifiers() -> impl Strategy<Value = String> {
    prop::collection::vec(prop::sample::select(vec!['i', 'm', 's', 'x', 'g']), 0..4)
        .prop_map(|chars| chars.into_iter().collect::<String>())
}

// ---------------------------------------------------------------------------
// Panic-freedom
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 512,
        ..ProptestConfig::default()
    })]

    /// `validate()` must never panic on arbitrary printable ASCII.
    #[test]
    fn validate_never_panics(s in ascii_pattern(), offset in 0usize..1024) {
        let _ = RegexValidator::new().validate(&s, offset);
    }

    /// `detects_code_execution()` must never panic.
    #[test]
    fn code_execution_never_panics(s in ascii_pattern()) {
        let _ = RegexValidator::new().detects_code_execution(&s);
    }

    /// `detect_nested_quantifiers()` must never panic.
    #[test]
    fn nested_quantifiers_never_panics(s in ascii_pattern()) {
        let _ = RegexValidator::new().detect_nested_quantifiers(&s);
    }

    /// `extract_named_captures()` must never panic.
    #[test]
    fn extract_captures_never_panics(s in ascii_pattern()) {
        let _ = RegexAnalyzer::extract_named_captures(&s);
    }

    /// `analyze_captures()` must never panic.
    #[test]
    fn analyze_captures_never_panics(s in ascii_pattern()) {
        let _ = RegexAnalyzer::analyze_captures(
            &s,
            EffectiveModifiers::default(),
            CaptureLanguageProfile::unknown(),
        );
    }

    /// `hover_text_for_regex()` must never panic on arbitrary pattern + modifiers.
    #[test]
    fn hover_text_never_panics(s in ascii_pattern(), m in modifiers()) {
        let _ = RegexAnalyzer::hover_text_for_regex(&s, &m);
    }
}

// ---------------------------------------------------------------------------
// Determinism
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        ..ProptestConfig::default()
    })]

    /// Calling `validate()` twice on the same input returns the same result.
    #[test]
    fn validate_is_deterministic(s in ascii_pattern(), offset in 0usize..1024) {
        let v = RegexValidator::new();
        let r1 = v.validate(&s, offset);
        let r2 = v.validate(&s, offset);
        prop_assert_eq!(r1, r2);
    }

    /// `detects_code_execution()` is deterministic.
    #[test]
    fn code_execution_is_deterministic(s in ascii_pattern()) {
        let v = RegexValidator::new();
        prop_assert_eq!(v.detects_code_execution(&s), v.detects_code_execution(&s));
    }

    /// `detect_nested_quantifiers()` is deterministic.
    #[test]
    fn nested_quantifiers_is_deterministic(s in ascii_pattern()) {
        let v = RegexValidator::new();
        prop_assert_eq!(v.detect_nested_quantifiers(&s), v.detect_nested_quantifiers(&s));
    }

    /// Capture declarations stay in source order even when branch-reset numbering
    /// intentionally restarts and therefore is not monotonic.
    #[test]
    fn capture_declarations_are_source_ordered(s in ascii_pattern()) {
        let analysis = RegexAnalyzer::analyze_captures(
            &s,
            EffectiveModifiers::default(),
            CaptureLanguageProfile::unknown(),
        );
        for window in analysis.declarations.windows(2) {
            prop_assert!(
                window[1].group_range.start >= window[0].group_range.start,
                "capture declaration ranges not source ordered: {:?} then {:?} in {:?}",
                window[0].group_range,
                window[1].group_range,
                s,
            );
            prop_assert!(window[1].id.index() > window[0].id.index());
        }
    }

    /// All named captures have non-empty names.
    #[test]
    fn capture_names_are_non_empty(s in ascii_pattern()) {
        for cap in RegexAnalyzer::extract_named_captures(&s) {
            prop_assert!(!cap.name.is_empty(), "empty capture name in {:?}", s);
        }
    }

    /// Every statically known capture number is one-based.
    #[test]
    fn capture_numbers_start_at_one(s in ascii_pattern()) {
        let analysis = RegexAnalyzer::analyze_captures(
            &s,
            EffectiveModifiers::default(),
            CaptureLanguageProfile::unknown(),
        );
        for declaration in analysis.declarations {
            if let Some(number) = declaration.number {
                prop_assert!(number >= 1, "capture number < 1 in {:?}", s);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Code-execution detection: conservative invariants
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        ..ProptestConfig::default()
    })]

    /// Patterns that contain neither `(?{` nor `(??{` are never flagged.
    /// This guards against false positives on arbitrary ASCII.
    #[test]
    fn no_code_constructs_means_no_detection(
        // Allow only ASCII printable except the sequence-forming characters
        // by generating from a character set that cannot form (?{ or (??{.
        s in "[A-Za-z0-9 _.,;:'\"!@#%&*\\[\\]<>/|^~`=+-]{0,80}"
    ) {
        // The strategy excludes '(' and '{' so (?{ / (??{ can't appear.
        prop_assert!(!RegexValidator::new().detects_code_execution(&s));
    }

    /// A pattern with `(?{` embedded always triggers detection.
    #[test]
    fn explicit_code_block_always_detected(
        prefix in "[A-Za-z0-9]{0,10}",
        suffix in "[A-Za-z0-9]{0,10}",
        inner in "[A-Za-z0-9 ]{0,20}",
    ) {
        let s = format!("{prefix}(?{{{inner}}}{suffix}");
        prop_assert!(
            RegexValidator::new().detects_code_execution(&s),
            "should detect (?{{...}} in {:?}",
            s
        );
    }

    /// A pattern with `(??{` always triggers detection.
    #[test]
    fn deferred_code_block_always_detected(
        prefix in "[A-Za-z0-9]{0,10}",
        suffix in "[A-Za-z0-9]{0,10}",
        inner in "[A-Za-z0-9 ]{0,20}",
    ) {
        let s = format!("{prefix}(??{{{inner}}}{suffix}");
        prop_assert!(
            RegexValidator::new().detects_code_execution(&s),
            "should detect (??{{...}} in {:?}",
            s
        );
    }

    /// Escaped opener sequences are treated as literals and never trigger detection.
    #[test]
    fn escaped_code_constructs_do_not_trigger_detection(
        prefix in "[A-Za-z0-9]{0,10}",
        suffix in "[A-Za-z0-9]{0,10}",
    ) {
        let escaped_block = format!(r"{prefix}\(?{{danger}}{suffix}");
        prop_assert!(
            !RegexValidator::new().detects_code_execution(&escaped_block),
            "escaped (?{{ should not be detected in {:?}",
            escaped_block
        );

        let escaped_deferred = format!(r"{prefix}\(??{{danger}}{suffix}");
        prop_assert!(
            !RegexValidator::new().detects_code_execution(&escaped_deferred),
            "escaped (??{{ should not be detected in {:?}",
            escaped_deferred
        );
    }

    /// Candidate sequences inside character classes are literals and not executable.
    #[test]
    fn character_class_literals_do_not_trigger_detection(
        prefix in "[A-Za-z0-9]{0,10}",
        suffix in "[A-Za-z0-9]{0,10}",
    ) {
        let class_block = format!("{prefix}[(?{{abc}})]{suffix}");
        prop_assert!(
            !RegexValidator::new().detects_code_execution(&class_block),
            "character class (?{{ literal should not be detected in {:?}",
            class_block
        );

        let class_deferred = format!("{prefix}[(??{{abc}})]{suffix}");
        prop_assert!(
            !RegexValidator::new().detects_code_execution(&class_deferred),
            "character class (??{{ literal should not be detected in {:?}",
            class_deferred
        );
    }
}
