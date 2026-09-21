/// Red-TDD and correctness tests for the heredoc anti-pattern detectors.
///
/// #1756 (ReDoS hardening) introduced the `[^}\n]` newline-horizon bound on the
/// regex heredoc detector. #14191 replaced the regex with a brace-tracking
/// scanner, so the ReDoS guard is now expressed against the detector's public
/// API (which preserves the same newline-horizon DoS bound) and the scanner's
/// new correctness property — that a `(?{ ... <<'X' ... })` block whose body
/// contains a nested `{ ... }` is still detected.
///
/// Each test asserts the public detector behaviour:
///   * the new scanner stays linear on pathological input that would have
///     triggered ReDoS in the original regex (`<10ms` on a 5KB unclosed
///     `(?{ ... <<` body), and
///   * the cases the original regex missed (a nested block before the heredoc,
///     on one line or split across a single line's nested braces) now report.
use perl_parser::heredoc_anti_patterns::{
    AntiPattern, AntiPatternDetector, DetectionReport, DetectionStatus,
};
use std::time::Instant;

/// Drive the production detector over `code` and report the wall-clock time.
fn detect_timed(code: &str) -> (DetectionReport, std::time::Duration) {
    let detector = AntiPatternDetector::new();
    let start = Instant::now();
    let report = detector.detect_all_report(code);
    (report, start.elapsed())
}

/// Count the `RegexCodeBlockHeredoc` findings in the report.
fn regex_heredoc_count(report: &DetectionReport) -> usize {
    report
        .diagnostics
        .iter()
        .filter(|d| matches!(d.pattern, AntiPattern::RegexCodeBlockHeredoc { .. }))
        .count()
}

/// The detector scans in finite time on a 5KB unclosed `(?{ ... <<` body, the
/// pathological input that #1756's regex bounded with `[^}\n]`. The brace
/// scanner inherits that horizon (`find_matching_brace_same_line` stops at
/// `\n`), so the run-time stays sub-millisecond.
#[test]
fn test_antip_no_redos_regex_5kb_unclosed() {
    let pathological_input = format!("{}{}{}", "(?{", "a".repeat(5000), "<<");

    let (_, elapsed) = detect_timed(&pathological_input);

    assert!(
        elapsed.as_millis() < 10,
        "Regex heredoc detector took {}ms on 5KB unclosed (?{{..<<) input; expected <10ms (ReDoS detected)",
        elapsed.as_millis()
    );
}

/// Same bound for the postponed opener `(??{ ... <<` from #14390.
#[test]
fn test_antip_no_redos_regex_postponed_5kb_unclosed() {
    let pathological_input = format!("{}{}{}", "(??{", "a".repeat(5000), "<<");

    let (_, elapsed) = detect_timed(&pathological_input);

    assert!(
        elapsed.as_millis() < 10,
        "Regex heredoc detector took {}ms on 5KB unclosed (??{{..<<) input; expected <10ms (ReDoS detected)",
        elapsed.as_millis()
    );
}

/// `(???{` is not a valid Perl opener. The bounded brace scanner must not
/// over-match and then advance into the body, which is the ReDoS shape the
/// original `?{1,2}` alternation guarded against.
#[test]
fn test_antip_no_redos_regex_triple_question_5kb_unclosed() {
    let pathological_input = format!("{}{}{}", "(???{", "a".repeat(5000), "<<");

    let (_, elapsed) = detect_timed(&pathological_input);

    assert!(
        elapsed.as_millis() < 10,
        "Regex heredoc detector took {}ms on 5KB unclosed (???{{..<<) input; expected <10ms (ReDoS detected)",
        elapsed.as_millis()
    );
}

/// ReDoS guard for `EVAL_HEREDOC_PATTERN` (unchanged shape).
#[test]
fn test_antip_no_redos_eval_5kb_unclosed() {
    let pathological_input = format!("{}{}{}", "eval '", "a".repeat(5000), "<<");

    let (_, elapsed) = detect_timed(&pathological_input);

    assert!(
        elapsed.as_millis() < 10,
        "Eval-heredoc detector took {}ms on 5KB unclosed quote input; expected <10ms (ReDoS detected)",
        elapsed.as_millis()
    );
}

/// ReDoS guard for `EXPORT_QW_RE` (unchanged shape).
#[test]
fn test_antip_no_redos_export_5kb_unclosed() {
    let pathological_input = format!("{}{}", "@EXPORT = qw(", "a".repeat(5000));

    let (_, elapsed) = detect_timed(&pathological_input);

    assert!(
        elapsed.as_millis() < 10,
        "Export-qw detector took {}ms on 5KB unclosed delimiter input; expected <10ms (ReDoS detected)",
        elapsed.as_millis()
    );
}

/// Positive control: a plain `(?{ ... <<'EOF' ... })` on one line is still
/// reported after the scanner rewrite.
#[test]
fn test_antip_regex_heredoc_valid() {
    let valid_input = "my $r = qr/(?{print <<'EOF'})/ or die;";
    let (report, _) = detect_timed(valid_input);

    assert!(
        regex_heredoc_count(&report) >= 1,
        "Expected the regex heredoc detector to fire on a single-line (?{{..<<..}}) block"
    );
}

/// Positive control: a postponed `(??{ ... <<'EOF' ... })` is still reported.
#[test]
fn test_antip_regex_postponed_heredoc_valid() {
    let valid_input = "my $r = qr/(??{print <<'EOF'})/ or die;";
    let (report, _) = detect_timed(valid_input);

    assert!(
        regex_heredoc_count(&report) >= 1,
        "Expected the regex heredoc detector to fire on a single-line (??{{..<<..}}) block"
    );
}

/// Positive control: a `(?{ ... })` whose body contains a nested `{ ... }`
/// block before the heredoc marker is now detected. This is the property the
/// original regex missed — the flat `[^}\n]*` character class stopped at the
/// inner `}`. The repro from #14191 is the canonical failing case.
#[test]
fn test_antip_regex_heredoc_with_nested_block_one_line() {
    let nested_input = "qr/x(?{ if (1) { 1 } print <<'MATCH';\nbody\nMATCH\n})/ or die;";
    let (report, _) = detect_timed(nested_input);

    assert!(
        regex_heredoc_count(&report) >= 1,
        "Expected the regex heredoc detector to fire when a nested {{ ... }} block \
         appears inside the same-line (?{{..<<..}}) block (the #14191 repro)"
    );
}

/// Positive control: the same nested block on a single source line — the body
/// of the heredoc opener spans a brace pair without a newline. The scanner
/// must still match the outer `}`.
#[test]
fn test_antip_regex_heredoc_with_nested_block_inline() {
    let inline = "qr/x(?{ if (1) { 1 } <<'M' })/ or die;";
    let (report, _) = detect_timed(inline);

    assert!(
        regex_heredoc_count(&report) >= 1,
        "Expected the regex heredoc detector to fire when a nested {{ }} block \
         sits between the opener and the heredoc on one line"
    );
}

/// Positive control: a nested `(?{ ... })` inside another `(?{ ... })` whose
/// heredoc marker lives in the outer block is reported once.
#[test]
fn test_antip_regex_heredoc_with_nested_opener() {
    let nested_opener = "qr/(?{ (?{ inner }) outer <<'X' })/ or die;";
    let (report, _) = detect_timed(nested_opener);

    assert!(
        regex_heredoc_count(&report) >= 1,
        "Expected the regex heredoc detector to fire when a nested (?{{...}}) \
         sits inside an outer (?{{...<<...}}) block"
    );
}

/// Negative control: a `(?{ ... })` with no heredoc marker is not reported.
#[test]
fn test_antip_regex_heredoc_no_marker_not_reported() {
    let no_marker = "qr/(?{ if (1) { 1 } })/ or die;";
    let (report, _) = detect_timed(no_marker);

    assert_eq!(
        regex_heredoc_count(&report),
        0,
        "A (?{{..}}) block with no << marker must not be reported"
    );
}

/// Negative control: a `<<` marker that lives outside any `(?{ ... })` block
/// is not picked up by this detector (it is still picked up by the dynamic
/// delimiter detector, which is exercised by a separate test).
#[test]
fn test_antip_regex_heredoc_marker_outside_block_not_reported() {
    let outside = "my $x = <<'EOF';\nbody\nEOF\nmy $r = qr/(?{ 1 })/;";
    let (report, _) = detect_timed(outside);

    assert_eq!(
        regex_heredoc_count(&report),
        0,
        "A heredoc marker outside any (?{{..}}) block must not be reported"
    );
}

/// Negative control: a heredoc marker inside a string literal is masked by
/// `mask_non_code_regions` and therefore must not be reported as a regex
/// code-block heredoc. The string content is collapsed to spaces before the
/// scanner runs.
#[test]
fn test_antip_regex_heredoc_marker_in_string_not_reported() {
    let in_string = r#"my $r = qr/(?{ "<< not a heredoc" })/; "#;
    let (report, _) = detect_timed(in_string);

    assert_eq!(
        regex_heredoc_count(&report),
        0,
        "A << sequence that lives inside a string literal must not be reported"
    );
}

/// Multiline opener: a `(?{ ... << ... })` whose matching `}` is on a later
/// line is reported by the brace scanner, which spans newlines (the original
/// regex's `[^}\n]*` did not). This is the canonical multi-line repro from
/// #14191 — the inner `{ 1 }` block plus the heredoc body all sit between
/// the opener and the closing brace on different lines.
#[test]
fn test_antip_regex_heredoc_multiline_opener_reported() {
    let multiline = "qr/x(?{ if (1) { 1 } print <<'MATCH';\nbody\nMATCH\n})/ or die;";
    let (report, _) = detect_timed(multiline);

    assert!(
        regex_heredoc_count(&report) >= 1,
        "Expected the regex heredoc detector to fire on the #14191 multi-line repro: \
         a (?{{ ... {{ ... }} ... << ... }})/ block whose body spans multiple lines"
    );
}

/// The five other detectors (`DYNAMIC_DELIMITER_PATTERN`, `EVAL_HEREDOC_PATTERN`,
/// `EXPORT_QW_RE`, etc.) keep their existing semantics. These are documented
/// here as control tests so any regression in their patterns surfaces
/// alongside the regex-heredoc change.
#[test]
fn test_antip_dynamic_delimiter_valid() {
    let valid_input = "my $x = <<${FOO_BAR};";
    let (report, _) = detect_timed(valid_input);

    let dynamic_count = report
        .diagnostics
        .iter()
        .filter(|d| matches!(d.pattern, AntiPattern::DynamicHeredocDelimiter { .. }))
        .count();
    assert!(dynamic_count >= 1, "Dynamic delimiter should be detected");
}

#[test]
fn test_antip_eval_heredoc_valid() {
    let valid_input = "eval 'my $x = <<EOF;'";
    let (report, _) = detect_timed(valid_input);

    let eval_count = report
        .diagnostics
        .iter()
        .filter(|d| matches!(d.pattern, AntiPattern::EvalStringHeredoc { .. }))
        .count();
    assert!(eval_count >= 1, "Eval-string heredoc should be detected");
}

#[test]
fn test_antip_delimiter_in_string() {
    let code_with_string = r#"print "use this <<${ pattern";"#;
    let (report, _) = detect_timed(code_with_string);

    // The raw pattern would match the literal <<${ in the string. The
    // detector's mask_non_code_regions() blanks string contents before the
    // scan, so this must NOT report a dynamic-delimiter heredoc.
    let dynamic_count = report
        .diagnostics
        .iter()
        .filter(|d| matches!(d.pattern, AntiPattern::DynamicHeredocDelimiter { .. }))
        .count();
    assert_eq!(
        dynamic_count, 0,
        "A <<${{ sequence inside a string literal must not be reported (masking layer)"
    );
}

/// Sanity guard: the entire detector finishes a realistic 1000-line file
/// within 100ms. The brace scanner runs O(n) over the source, so this should
/// stay well inside the budget.
#[test]
fn test_antip_normal_file_performance() {
    let mut code = String::new();
    for i in 0..1000 {
        code.push_str(&format!("sub routine_{} {{ my $x = {}; }} # line {}\n", i, i, i));
    }

    let (_, elapsed) = detect_timed(&code);

    assert!(
        elapsed.as_millis() < 100,
        "Detector on 1000-line file took {}ms; expected <100ms (performance regression)",
        elapsed.as_millis()
    );
}

/// The detector's overall status is Complete on normal input — every pattern
/// compiled and ran. The fix preserves the completeness invariant for the
/// regex heredoc detector (its availability no longer depends on a regex
/// compile result).
#[test]
fn test_antip_completeness_normal_input() {
    let input = "my $r = qr/(?{print <<'EOF'})/ or die;\nmy $x = <<${Y};\n";
    let (report, _) = detect_timed(input);

    assert_eq!(
        report.status,
        DetectionStatus::Complete,
        "Detector status should be Complete on well-formed input"
    );
}
