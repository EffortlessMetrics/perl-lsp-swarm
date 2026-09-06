use super::AntiPatternDetector;
use crate::heredoc_anti_patterns::model::{
    AntiPattern, DetectionStatus, DetectorFailureReason, DetectorId, DetectorState,
    HeredocDelimiter,
};
use regex::Regex;

#[test]
fn test_format_heredoc_detection() {
    let detector = AntiPatternDetector::new();
    let code = r#"
format REPORT =
<<'END'
Name: @<<<<<<<<<<<<
$name
END
.
"#;

    let diagnostics = detector.detect_all(code);
    // Note: DynamicDelimiterDetector might also flag the << inside the format body as a false positive.
    // But FormatHeredoc should appear first because it starts at 'format'.
    // So diagnostics[0] should be FormatHeredoc.
    assert!(!diagnostics.is_empty());
    assert!(matches!(diagnostics[0].pattern, AntiPattern::FormatHeredoc { .. }));

    if let AntiPattern::FormatHeredoc { heredoc_delimiter, .. } = &diagnostics[0].pattern {
        assert_eq!(*heredoc_delimiter, HeredocDelimiter::Extracted("END".to_string()));
    }
}

#[test]
fn test_begin_heredoc_detection() {
    let detector = AntiPatternDetector::new();
    let code = r###"
BEGIN {
    $config = <<'END';
    server = localhost
END
}
"###;

    let diagnostics = detector.detect_all(code);
    assert_eq!(diagnostics.len(), 1);
    assert!(matches!(diagnostics[0].pattern, AntiPattern::BeginTimeHeredoc { .. }));
}

#[test]
fn test_begin_heredoc_detection_with_nested_braces() {
    let detector = AntiPatternDetector::new();
    let code = r###"
BEGIN {
    if ($ENV{DEV}) {
        $config = <<'END';
        server = localhost
END
    }
}
"###;

    let diagnostics = detector.detect_all(code);
    let begin_count = diagnostics
        .iter()
        .filter(|diag| matches!(diag.pattern, AntiPattern::BeginTimeHeredoc { .. }))
        .count();
    assert_eq!(begin_count, 1);
}

#[test]
fn test_begin_heredoc_ignores_braces_in_comments() {
    let detector = AntiPatternDetector::new();
    let code = r###"
BEGIN {
    # comment with } brace
    $config = <<'END';
    server = localhost
END
}
"###;

    let diagnostics = detector.detect_all(code);
    let begin_count = diagnostics
        .iter()
        .filter(|diag| matches!(diag.pattern, AntiPattern::BeginTimeHeredoc { .. }))
        .count();
    assert_eq!(begin_count, 1);
}

#[test]
fn test_dynamic_delimiter_detection() {
    let detector = AntiPatternDetector::new();
    let code = r###"
my $delimiter = "EOF";
my $content = <<$delimiter;
This is dynamic
EOF
"###;

    let diagnostics = detector.detect_all(code);
    assert_eq!(diagnostics.len(), 1);
    assert!(matches!(diagnostics[0].pattern, AntiPattern::DynamicHeredocDelimiter { .. }));
}

#[test]
fn test_source_filter_detection() {
    let detector = AntiPatternDetector::new();
    let code = r###"
use Filter::Simple;
print <<EOF;
Filtered content
EOF
"###;
    let diagnostics = detector.detect_all(code);
    assert_eq!(diagnostics.len(), 1);
    assert!(matches!(diagnostics[0].pattern, AntiPattern::SourceFilterHeredoc { .. }));
}

#[test]
fn test_regex_heredoc_detection() {
    let detector = AntiPatternDetector::new();
    // Single-line case: (?{ and << on the same line — detected by the bounded pattern.
    // Multi-line cases ((?{ on one line, << on the next) are not detected after #1756;
    // that tradeoff is explicit: line-boundary anchoring prevents ReDoS.
    let code = "m/a(?{b<<'X'})c/";
    let diagnostics = detector.detect_all(code);
    assert_eq!(diagnostics.len(), 1);
    assert!(matches!(diagnostics[0].pattern, AntiPattern::RegexCodeBlockHeredoc { .. }));
}

#[test]
fn test_eval_heredoc_detection() {
    let detector = AntiPatternDetector::new();
    // Single-line case: eval and << on the same line — detected by the bounded pattern.
    // Multi-line cases (closing quote on a later line) are not detected after #1756;
    // that tradeoff is explicit: line-boundary anchoring prevents ReDoS.
    let code = "eval 'print <<EOF;'";
    let diagnostics = detector.detect_all(code);
    assert_eq!(diagnostics.len(), 1);
    assert!(matches!(diagnostics[0].pattern, AntiPattern::EvalStringHeredoc { .. }));
}

#[test]
fn test_tied_handle_detection() {
    let detector = AntiPatternDetector::new();
    let code = r###"
tie *FH, 'Tie::Handle';
print FH <<'DATA';
Tied output
DATA
"###;
    let diagnostics = detector.detect_all(code);
    assert_eq!(diagnostics.len(), 1);
    assert!(matches!(diagnostics[0].pattern, AntiPattern::TiedHandleHeredoc { .. }));
}

#[test]
fn test_tied_scalar_handle_detection() {
    let detector = AntiPatternDetector::new();
    let code = r###"
tie $fh, 'Tie::Handle';
print $fh <<'DATA';
Tied output
DATA
"###;
    let diagnostics = detector.detect_all(code);
    assert_eq!(diagnostics.len(), 1);
    assert!(matches!(diagnostics[0].pattern, AntiPattern::TiedHandleHeredoc { .. }));
}

#[test]
fn test_tied_handle_reports_multiple_writes() {
    let detector = AntiPatternDetector::new();
    let code = r###"
tie *FH, 'Tie::Handle';
print FH <<'FIRST';
One
FIRST
print FH <<'SECOND';
Two
SECOND
"###;

    let diagnostics = detector.detect_all(code);
    let tied_handle_count = diagnostics
        .iter()
        .filter(|diag| matches!(diag.pattern, AntiPattern::TiedHandleHeredoc { .. }))
        .count();
    assert_eq!(tied_handle_count, 2);
}

#[test]
fn test_tied_handle_does_not_report_other_handles() {
    // Regression: PRINT_HEREDOC_PATTERN must only flag handles in the tied set.
    // Writing a heredoc to an *untied* handle (OTHER) must not produce a diagnostic.
    let detector = AntiPatternDetector::new();
    let code = r###"
tie *FH, 'Tie::Handle';
print OTHER <<'DATA';
Not tied
DATA
"###;

    let diagnostics = detector.detect_all(code);
    let tied_handle_count = diagnostics
        .iter()
        .filter(|diag| matches!(diag.pattern, AntiPattern::TiedHandleHeredoc { .. }))
        .count();
    assert_eq!(tied_handle_count, 0);
}

#[test]
fn test_location_column_is_zero_based_for_new_line_matches() {
    let detector = AntiPatternDetector::new();
    let code = "my $x = 1;\nuse Filter::Simple;\n";

    let diagnostics = detector.detect_all(code);
    assert_eq!(diagnostics.len(), 1);

    assert!(
        matches!(diagnostics[0].pattern, AntiPattern::SourceFilterHeredoc { .. }),
        "expected SourceFilterHeredoc pattern, got: {:?}",
        diagnostics[0].pattern
    );
    let AntiPattern::SourceFilterHeredoc { location, .. } = &diagnostics[0].pattern else {
        return;
    };

    assert_eq!(location.line, 1);
    assert_eq!(location.column, 0);
    assert_eq!(location.offset, 11);
}

#[test]
fn test_location_first_byte_is_line_zero_column_zero() {
    // A match at byte offset 0 must report line=0, column=0.
    let detector = AntiPatternDetector::new();
    let code = "use Filter::Simple;\n";

    let diagnostics = detector.detect_all(code);
    assert_eq!(diagnostics.len(), 1);
    let AntiPattern::SourceFilterHeredoc { location, .. } = &diagnostics[0].pattern else {
        unreachable!("expected SourceFilterHeredoc");
    };
    assert_eq!(location.line, 0, "first-byte match must be on line 0");
    assert_eq!(location.column, 0, "first-byte match must be at column 0");
    assert_eq!(location.offset, 0);
}

#[test]
fn test_location_third_line_accurate() {
    // Three-line file — match on line 2, column 0.
    let detector = AntiPatternDetector::new();
    // Line 0: "my $a = 1;\n"  (11 bytes, \n at index 10)
    // Line 1: "my $b = 2;\n"  (11 bytes, \n at index 21)
    // Line 2: "use Filter::Simple;\n"
    let code = "my $a = 1;\nmy $b = 2;\nuse Filter::Simple;\n";

    let diagnostics = detector.detect_all(code);
    assert_eq!(diagnostics.len(), 1);
    let AntiPattern::SourceFilterHeredoc { location, .. } = &diagnostics[0].pattern else {
        unreachable!("expected SourceFilterHeredoc");
    };
    assert_eq!(location.line, 2, "match on third line must report line 2");
    assert_eq!(location.column, 0, "match at start of line must report column 0");
    assert_eq!(location.offset, 22, "byte offset of third-line start");
}

#[test]
fn test_location_mid_line_column_nonzero() {
    // Match that does not start at column 0 must report the correct column.
    // Line 0: "# comment\n"      (10 bytes, \n at index 9)
    // Line 1: "    use Filter::Simple;\n"  — 4 leading spaces, match at column 4
    let detector = AntiPatternDetector::new();
    let code = "# comment\n    use Filter::Simple;\n";

    let diagnostics = detector.detect_all(code);
    // The comment is masked; only SourceFilterHeredoc on line 1 should fire.
    assert_eq!(diagnostics.len(), 1);
    let AntiPattern::SourceFilterHeredoc { location, .. } = &diagnostics[0].pattern else {
        unreachable!("expected SourceFilterHeredoc");
    };
    assert_eq!(location.line, 1);
    assert_eq!(location.column, 4, "mid-line match must report correct column");
    assert_eq!(location.offset, 14, "byte offset = 10 (first line) + 4 spaces");
}

#[test]
fn test_source_filter_detection_ignores_comments_and_strings() {
    let detector = AntiPatternDetector::new();
    let code = r#"
# use Filter::Simple;
my $s = "use Filter::Simple";
"#;

    let diagnostics = detector.detect_all(code);
    assert!(diagnostics.is_empty());
}

#[test]
fn test_begin_detection_ignores_comments_and_strings() {
    let detector = AntiPatternDetector::new();
    let code = r#"
# BEGIN { my $x = <<'END'; END }
my $s = "BEGIN { my $x = <<'END'; END }";
"#;

    let diagnostics = detector.detect_all(code);
    assert!(diagnostics.is_empty());
}

#[test]
fn test_format_detection_handles_utf8_in_masked_regions() {
    let detector = AntiPatternDetector::new();
    let code = r#"# comment with emoji 😀
format REPORT =
<<'END'
Body
END
.
"#;

    let diagnostics = detector.detect_all(code);
    assert!(
        diagnostics.iter().any(|diag| matches!(diag.pattern, AntiPattern::FormatHeredoc { .. }))
    );
}

#[test]
fn test_find_matching_brace_skips_braces_inside_quoted_strings() {
    let code = r#"BEGIN { my $text = "not a } brace"; { 1 } }"#;
    let Some(opening) = code.find('{') else {
        unreachable!("opening brace exists");
    };

    let closing = super::find_matching_brace(code, opening);

    assert_eq!(closing, code.rfind('}'));
}

#[test]
fn test_find_matching_brace_returns_none_for_unclosed_block() {
    let code = "BEGIN { my $text = '{ still open';";
    let Some(opening) = code.find('{') else {
        unreachable!("opening brace exists");
    };

    let closing = super::find_matching_brace(code, opening);

    assert!(closing.is_none());
}

#[test]
fn production_catalog_compiles_and_empty_source_is_complete_clean() {
    let detector = AntiPatternDetector::new();
    let report = detector.detect_all_report("");

    assert_eq!(report.status, DetectionStatus::Complete);
    assert!(report.detectors.iter().all(|obs| matches!(obs.state, DetectorState::Complete)));
    assert_eq!(report.detectors.len(), ALL_DETECTOR_IDS.len());
    assert!(report.is_complete_clean());
}

fn forced_reason() -> DetectorFailureReason {
    DetectorFailureReason::PatternUnavailable { pattern_ids: vec!["TEST_FORCED"] }
}

fn detector_forcing_unavailable(ids: &[DetectorId]) -> AntiPatternDetector {
    let reason = forced_reason();
    let patterns = super::production_pattern_detectors()
        .into_iter()
        .map(|live| {
            let id = live.id();
            if ids.contains(&id) {
                Box::new(super::ForcedUnavailableDetector { id, reason: reason.clone() })
                    as Box<dyn super::PatternDetector>
            } else {
                live
            }
        })
        .collect();
    AntiPatternDetector::from_pattern_detectors(patterns)
}

const ALL_DETECTOR_IDS: [DetectorId; 7] = [
    DetectorId::FormatHeredoc,
    DetectorId::BeginTimeHeredoc,
    DetectorId::DynamicDelimiter,
    DetectorId::SourceFilter,
    DetectorId::RegexCodeBlock,
    DetectorId::EvalString,
    DetectorId::TiedHandle,
];

#[test]
fn forced_unavailable_detector_does_not_panic_and_leaves_others_running() {
    let detector = detector_forcing_unavailable(&[DetectorId::FormatHeredoc]);
    let code = "use Filter::Simple;\n";
    let report = detector.detect_all_report(code);

    assert_eq!(report.status, DetectionStatus::Partial);
    assert_eq!(report.diagnostics.len(), 1);
    assert!(matches!(report.diagnostics[0].pattern, AntiPattern::SourceFilterHeredoc { .. }));
    let format = report
        .detectors
        .iter()
        .find(|obs| obs.id == DetectorId::FormatHeredoc)
        .expect("format detector observation");
    assert!(matches!(format.state, DetectorState::Unavailable { .. }));
}

#[test]
fn partial_empty_scan_is_not_complete_clean() {
    let detector = detector_forcing_unavailable(&[DetectorId::SourceFilter]);
    let code = "my $x = 1;\n";
    let report = detector.detect_all_report(code);

    assert_eq!(report.status, DetectionStatus::Partial);
    assert!(report.diagnostics.is_empty());
    assert!(!report.is_complete_clean());
    assert!(detector.detect_all(code).is_empty());

    let formatted = detector.format_detection_report(&report);
    assert!(formatted.contains("Status: partial"));
    assert!(
        !formatted.contains("No problematic patterns detected."),
        "partial-empty must not masquerade as complete-clean"
    );
    assert!(
        detector.format_report(&report.diagnostics).contains("No problematic patterns detected."),
        "diagnostics-only projection remains lossy and must not be the completeness authority"
    );
}

#[test]
fn all_detectors_unavailable_is_unavailable_not_clean() {
    let detector = detector_forcing_unavailable(&ALL_DETECTOR_IDS);
    let code = "use Filter::Simple;\nprint <<$x;\n";
    let report = detector.detect_all_report(code);

    assert_eq!(report.status, DetectionStatus::Unavailable);
    assert!(report.diagnostics.is_empty());
    assert!(!report.is_complete_clean());
    assert!(
        detector
            .format_detection_report(&report)
            .contains("Analysis unavailable: no detector completed.")
    );
}

#[test]
fn tied_handle_missing_required_pattern_emits_no_findings() {
    let code = "tie *FH, 'Tie::Handle';\nprint FH <<'DATA';\nTied\nDATA\n";
    let line_starts = crate::heredoc_anti_patterns::utils::build_line_starts(code);
    let print = Regex::new(r"print\s+([*$]?\w+)\s+<<").expect("test print pattern");

    let findings = super::detect_tied_handle(code, 0, &line_starts, None, Some(&print));
    assert!(findings.is_empty(), "missing TIE_PATTERN must not fabricate tied-handle findings");

    let state = super::required_state(&[("TIE_PATTERN", false), ("PRINT_HEREDOC_PATTERN", true)]);
    assert_eq!(
        state,
        DetectorState::Unavailable {
            reason: DetectorFailureReason::PatternUnavailable { pattern_ids: vec!["TIE_PATTERN"] },
        }
    );
}

#[test]
fn delimiter_unknown_is_not_pattern_unavailable() {
    let pattern =
        Regex::new(r#"<<\s*['"`]?([A-Za-z_][A-Za-z0-9_]*)['"`]?"#).expect("test delimiter pattern");

    assert_eq!(
        super::extract_heredoc_delimiter_with(Some(&pattern), "<<'END'"),
        HeredocDelimiter::Extracted("END".to_string())
    );
    assert_eq!(
        super::extract_heredoc_delimiter_with(Some(&pattern), "no delimiter here"),
        HeredocDelimiter::Unknown
    );
    assert_eq!(
        super::extract_heredoc_delimiter_with(None, "<<'END'"),
        HeredocDelimiter::Unavailable
    );
}

#[test]
fn shared_delimiter_failure_does_not_fabricate_or_suppress_unrelated_findings() {
    let patterns = super::production_pattern_detectors()
        .into_iter()
        .map(|live| {
            if live.id() == DetectorId::FormatHeredoc {
                Box::new(super::ForcedLimitedFormatDetector) as Box<dyn super::PatternDetector>
            } else {
                live
            }
        })
        .collect();
    let detector = AntiPatternDetector::from_pattern_detectors(patterns);
    let code = r#"
format REPORT =
<<'END'
Name: @<<<<<<<<<<<<
$name
END
.
use Filter::Simple;
"#;

    let report = detector.detect_all_report(code);
    assert_eq!(report.status, DetectionStatus::Partial);
    assert!(!report.is_complete_clean());

    let format = report
        .diagnostics
        .iter()
        .find(|diag| matches!(diag.pattern, AntiPattern::FormatHeredoc { .. }))
        .expect("format finding must survive delimiter unavailability");
    let AntiPattern::FormatHeredoc { heredoc_delimiter, .. } = &format.pattern else {
        unreachable!("format finding");
    };
    assert_eq!(*heredoc_delimiter, HeredocDelimiter::Unavailable);

    assert!(
        report
            .diagnostics
            .iter()
            .any(|diag| matches!(diag.pattern, AntiPattern::SourceFilterHeredoc { .. }))
    );

    let format_state = report
        .detectors
        .iter()
        .find(|obs| obs.id == DetectorId::FormatHeredoc)
        .expect("format observation");
    assert!(matches!(format_state.state, DetectorState::Limited { .. }));
}

#[test]
fn shuffled_catalog_still_emits_stable_observation_order() {
    let mut patterns = super::production_pattern_detectors();
    patterns.reverse();
    let detector = AntiPatternDetector::from_pattern_detectors(patterns);
    let report = detector.detect_all_report("use Filter::Simple;\n");

    let ids: Vec<DetectorId> = report.detectors.iter().map(|obs| obs.id).collect();
    assert_eq!(ids, ALL_DETECTOR_IDS);
    assert_eq!(report.status, DetectionStatus::Complete);
    assert_eq!(report.diagnostics.len(), 1);
    assert!(matches!(report.diagnostics[0].pattern, AntiPattern::SourceFilterHeredoc { .. }));
}

#[test]
fn healthy_source_filter_fixture_retains_kind_location_message_and_order() {
    let detector = AntiPatternDetector::new();
    let code = "my $x = 1;\nuse Filter::Simple;\n";
    let first = detector.detect_all_report(code);
    let second = detector.detect_all_report(code);

    assert_eq!(first, second);
    assert_eq!(first.status, DetectionStatus::Complete);
    assert_eq!(first.diagnostics.len(), 1);
    assert_eq!(first.diagnostics[0].message, "Source filter detected: Filter::Simple");
    let AntiPattern::SourceFilterHeredoc { location, .. } = &first.diagnostics[0].pattern else {
        unreachable!("expected SourceFilterHeredoc");
    };
    assert_eq!(location.line, 1);
    assert_eq!(location.column, 0);
    assert_eq!(location.offset, 11);
}

#[test]
fn format_availability_distinguishes_required_pattern_from_helper() {
    assert_eq!(super::format_availability(true, true), DetectorState::Complete);
    assert!(matches!(super::format_availability(true, false), DetectorState::Limited { .. }));
    assert!(matches!(super::format_availability(false, true), DetectorState::Unavailable { .. }));
}

#[test]
fn empty_catalog_is_unavailable_not_complete_clean() {
    let detector = AntiPatternDetector::from_pattern_detectors(Vec::new());
    let report = detector.detect_all_report("use Filter::Simple;\n");
    assert_eq!(report.status, DetectionStatus::Unavailable);
    assert!(report.diagnostics.is_empty());
    assert!(!report.is_complete_clean());
}
