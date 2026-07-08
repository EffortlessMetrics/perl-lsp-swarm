/// Guardrail tests for bounded-repetition regex quantifiers in heredoc anti-pattern detectors.
///
/// Issue #1756: The detector regexes for dynamic delimiters, regex code block heredocs,
/// and eval string heredocs used unbounded `[^X]+` / `[^X]*` quantifiers. These are
/// replaced with `{0,N}` / `{1,N}` bounds as a defense-in-depth measure.
///
/// Note: the `regex` crate uses NFA-based matching with guaranteed O(m*n) worst-case,
/// so catastrophic backtracking is not possible. These bounds still matter because they
/// cap per-match scan cost on enormous inputs, ensuring file-level throughput stays
/// proportional.
///
/// These tests verify two things:
/// 1. Bounded patterns still detect the anti-patterns they were designed to find.
/// 2. Pathological inputs (long unclosed delimiters) complete in bounded time.
use perl_parser::heredoc_anti_patterns::{AntiPattern, AntiPatternDetector};

// ── helper ────────────────────────────────────────────────────────────────────

fn first_pattern(code: &str) -> Option<AntiPattern> {
    AntiPatternDetector::new().detect_all(code).into_iter().map(|d| d.pattern).next()
}

fn all_patterns(code: &str) -> Vec<AntiPattern> {
    AntiPatternDetector::new().detect_all(code).into_iter().map(|d| d.pattern).collect()
}

// ── 1. Detection still works after bounds are applied ─────────────────────────

#[test]
fn dynamic_delimiter_scalar_still_detected() {
    let code = "my $t = <<$delim;\ntext\n$delim\n";
    let pat = first_pattern(code);
    assert!(
        matches!(pat, Some(AntiPattern::DynamicHeredocDelimiter { .. })),
        "dynamic scalar delimiter must still be detected: {pat:?}"
    );
}

#[test]
fn dynamic_delimiter_braced_still_detected() {
    let code = "my $t = <<${delim};\ntext\n${delim}\n";
    let pat = first_pattern(code);
    assert!(
        matches!(pat, Some(AntiPattern::DynamicHeredocDelimiter { .. })),
        "dynamic braced delimiter must still be detected: {pat:?}"
    );
}

#[test]
fn dynamic_delimiter_command_still_detected() {
    let code = "my $t = <<`echo end`;\n";
    let pat = first_pattern(code);
    assert!(
        matches!(pat, Some(AntiPattern::DynamicHeredocDelimiter { .. })),
        "dynamic backtick delimiter must still be detected: {pat:?}"
    );
}

#[test]
fn regex_heredoc_multiline_still_detected() {
    // The regex code block spans two lines — the bound must not prevent detection.
    let code = "m/pattern(?{\n    print <<'MATCH';\n    Match text\nMATCH\n})/\n";
    let pats = all_patterns(code);
    assert!(
        pats.iter().any(|p| matches!(p, AntiPattern::RegexCodeBlockHeredoc { .. })),
        "regex code block heredoc must still be detected in multi-line input"
    );
}

#[test]
fn eval_heredoc_single_quote_still_detected() {
    let code = "eval 'print <<\"EVAL\";\nEval content\nEVAL';\n";
    let pats = all_patterns(code);
    assert!(
        pats.iter().any(|p| matches!(p, AntiPattern::EvalStringHeredoc { .. })),
        "eval string heredoc (single-quote form) must still be detected"
    );
}

// ── 2. Pathological inputs complete in bounded time ───────────────────────────

#[test]
fn dynamic_delimiter_pathological_input_completes() {
    // 5 KB of characters that look like a dynamic delimiter start but never close.
    let payload = format!("<<${{{}}}", "x".repeat(5_000));
    let start = std::time::Instant::now();
    let _ = all_patterns(&payload);
    assert!(
        start.elapsed().as_secs() < 2,
        "dynamic delimiter detector must complete in < 2s on 5 KB pathological input"
    );
}

#[test]
fn regex_heredoc_pathological_input_completes() {
    // 5 KB inside a (?{ block with no closing }
    let payload = format!("(?{{{}{}", "a<<b".repeat(500), "c".repeat(2_000));
    let start = std::time::Instant::now();
    let _ = all_patterns(&payload);
    assert!(
        start.elapsed().as_secs() < 2,
        "regex heredoc detector must complete in < 2s on 5 KB pathological input"
    );
}

#[test]
fn eval_heredoc_pathological_input_completes() {
    // 5 KB single-quoted string containing << but no closing '
    let payload = format!("eval '{}<<END", "x".repeat(5_000));
    let start = std::time::Instant::now();
    let _ = all_patterns(&payload);
    assert!(
        start.elapsed().as_secs() < 2,
        "eval heredoc detector must complete in < 2s on 5 KB pathological input"
    );
}

#[test]
fn dynamic_delimiter_backtick_pathological_input_completes() {
    // 5 KB inside a backtick dynamic delimiter with no closing `
    let payload = format!("<<`{}", "x".repeat(5_000));
    let start = std::time::Instant::now();
    let _ = all_patterns(&payload);
    assert!(
        start.elapsed().as_secs() < 2,
        "dynamic backtick delimiter detector must complete in < 2s on 5 KB pathological input"
    );
}
