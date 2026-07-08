/// Guardrail tests for bounded-quantifier anti-pattern regex patterns (#1756).
///
/// The anti-pattern patterns use bounded quantifiers (`{0,N}` / `{1,N}`) instead of
/// `\n` newline-exclusion so that:
///
/// 1. **Multi-line detection is preserved** — `[^}]` / `[^']` / `[^"]` already match
///    newlines in the `regex` crate (character-class negation, not `.`). Excluding `\n`
///    would silently drop detection of heredoc anti-patterns whose content spans multiple
///    lines, which is the *common* case for heredocs.
///
/// 2. **Scan depth is capped** — bounded quantifiers limit how many characters the
///    engine will match per attempt. On pathological inputs (e.g., 1 MB of chars with no
///    closing delimiter), this prevents the per-attempt scan from consuming the entire
///    input, even though the `regex` crate is already linear in total input length.
///
/// Tests are organized into two groups:
/// - **Positive** — the detector must still fire on realistic (possibly multiline) inputs.
/// - **Performance** — the detector must complete in reasonable time on large inputs.
use perl_parser::heredoc_anti_patterns::AntiPattern;
use perl_parser::heredoc_anti_patterns::AntiPatternDetector;
use std::time::Instant;

// ─── Multiline detection: REGEX_HEREDOC_PATTERN ────────────────────────────

#[test]
fn test_regex_heredoc_multiline_still_detected() {
    // The heredoc delimiter declaration (`<<`) and the closing `}` of `(?{...})` are on
    // different lines — this is the realistic shape.  The `\n`-exclusion fix in PR #3212
    // would have silently dropped this case.
    let code = r#"
m/pattern(?{
    print <<'MATCH';
    Match text
MATCH
})/
"#;
    let detector = AntiPatternDetector::new();
    let diagnostics = detector.detect_all(code);
    let has_regex_heredoc =
        diagnostics.iter().any(|d| matches!(d.pattern, AntiPattern::RegexCodeBlockHeredoc { .. }));
    assert!(
        has_regex_heredoc,
        "REGEX_HEREDOC_PATTERN must still detect multiline (?{{...<<...}}) constructs"
    );
}

#[test]
fn test_regex_heredoc_same_line_detected() {
    // Single-line case must still work.
    let code = r#"m/(?{print <<"END"; END})/;"#;
    let detector = AntiPatternDetector::new();
    let diagnostics = detector.detect_all(code);
    let has_regex_heredoc =
        diagnostics.iter().any(|d| matches!(d.pattern, AntiPattern::RegexCodeBlockHeredoc { .. }));
    assert!(has_regex_heredoc, "REGEX_HEREDOC_PATTERN must detect single-line (?{{<<}}) construct");
}

// ─── Multiline detection: EVAL_HEREDOC_PATTERN ─────────────────────────────

#[test]
fn test_eval_heredoc_multiline_still_detected() {
    // The heredoc opener and terminator are on separate lines — realistic.
    // The `\n`-exclusion approach would have broken detection of this common shape.
    let code = "eval 'print <<\"EVAL\";\nEval content\nEVAL\n';";
    let detector = AntiPatternDetector::new();
    let diagnostics = detector.detect_all(code);
    let has_eval_heredoc =
        diagnostics.iter().any(|d| matches!(d.pattern, AntiPattern::EvalStringHeredoc { .. }));
    assert!(
        has_eval_heredoc,
        "EVAL_HEREDOC_PATTERN must still detect multiline eval-string heredocs"
    );
}

#[test]
fn test_eval_heredoc_double_quote_variant_multiline() {
    // Double-quoted eval string with multi-line heredoc.
    let code = "eval \"print <<'END';\\ncontent\\nEND\\n\";";
    let detector = AntiPatternDetector::new();
    let diagnostics = detector.detect_all(code);
    let has_eval_heredoc =
        diagnostics.iter().any(|d| matches!(d.pattern, AntiPattern::EvalStringHeredoc { .. }));
    assert!(
        has_eval_heredoc,
        "EVAL_HEREDOC_PATTERN must detect double-quoted eval with multiline heredoc"
    );
}

// ─── Positive: DYNAMIC_DELIMITER_PATTERN ────────────────────────────────────

#[test]
fn test_dynamic_delimiter_curly_brace_detected() {
    let code = "my $x = <<${delimiter};";
    let detector = AntiPatternDetector::new();
    let diagnostics = detector.detect_all(code);
    let has_dynamic = diagnostics
        .iter()
        .any(|d| matches!(d.pattern, AntiPattern::DynamicHeredocDelimiter { .. }));
    assert!(has_dynamic, "DYNAMIC_DELIMITER_PATTERN must detect <<${{expr}} form");
}

#[test]
fn test_dynamic_delimiter_backtick_variant_detected() {
    let code = "my $x = <<`cmd arg`;";
    let detector = AntiPatternDetector::new();
    let diagnostics = detector.detect_all(code);
    let has_dynamic = diagnostics
        .iter()
        .any(|d| matches!(d.pattern, AntiPattern::DynamicHeredocDelimiter { .. }));
    assert!(has_dynamic, "DYNAMIC_DELIMITER_PATTERN must detect <<`cmd` form");
}

// ─── Performance: all patterns complete quickly on large pathological inputs ─

/// Verify that `detect_all` completes in < 5 s on a 1 MB string of unclosed `{`.
///
/// With unbounded `[^}]+` the pattern scans the entire input per attempt (still
/// linear in the `regex` crate, but accumulates quickly with many starting positions).
/// With bounded `{1,500}` / `{0,2000}` the maximum scan per attempt is capped.
///
/// Note: the `regex` crate's NFA engine makes the total time O(n * m) regardless,
/// but capping per-attempt scan depth reduces the constant factor for pathological inputs.
#[test]
fn test_detect_all_completes_fast_on_1mb_unclosed_brace() {
    let large_input: String = std::iter::repeat('{').take(1_000_000).collect();
    let start = Instant::now();
    let detector = AntiPatternDetector::new();
    let _ = detector.detect_all(&large_input);
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs() < 5,
        "detect_all must complete in < 5 s on 1 MB unclosed-brace input, took {:?}",
        elapsed
    );
}

#[test]
fn test_detect_all_completes_fast_on_1mb_unclosed_backtick() {
    let large_input: String = std::iter::repeat('`').take(1_000_000).collect();
    let start = Instant::now();
    let detector = AntiPatternDetector::new();
    let _ = detector.detect_all(&large_input);
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs() < 5,
        "detect_all must complete in < 5 s on 1 MB unclosed-backtick input, took {:?}",
        elapsed
    );
}

#[test]
fn test_detect_all_completes_fast_on_1mb_unclosed_single_quote() {
    // Pathological for EVAL_HEREDOC_PATTERN: eval ' followed by 1MB of text with no closing '.
    let inner: String = std::iter::repeat('x').take(999_990).collect();
    let large_input = format!("eval '{}", inner);
    let start = Instant::now();
    let detector = AntiPatternDetector::new();
    let _ = detector.detect_all(&large_input);
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs() < 5,
        "detect_all must complete in < 5 s on 1 MB unclosed eval-string, took {:?}",
        elapsed
    );
}

// ─── Regression: existing detection must not have regressed ─────────────────

#[test]
fn test_begin_heredoc_still_detected() {
    let code = r#"BEGIN { $x = <<'END'; content END }"#;
    let detector = AntiPatternDetector::new();
    let diagnostics = detector.detect_all(code);
    assert!(
        diagnostics.iter().any(|d| matches!(d.pattern, AntiPattern::BeginTimeHeredoc { .. })),
        "BEGIN heredoc detection must not have regressed"
    );
}
