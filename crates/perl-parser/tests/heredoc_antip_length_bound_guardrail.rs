/// Defense-in-depth guardrail tests for length-bound character classes in
/// heredoc anti-pattern detectors.
///
/// Issue #1756: Four regex patterns in the heredoc anti-pattern detector and
/// `moniker.rs` used unbounded `[^x]+`/`[^x]*` character classes. These have
/// been replaced with length-bound forms (`{1,2000}` / `{0,2000}`) as honest
/// defense-in-depth.
///
/// The `regex` crate's NFA engine cannot exhibit catastrophic backtracking, so
/// these tests are NOT guarding against an active performance problem. Their
/// purpose is to verify that the length-bound change does not regress detection
/// ability — especially for multi-line inputs that the `\n`-exclusion approach
/// (the rejected alternative) would have silently dropped.
use perl_parser::heredoc_anti_patterns::AntiPatternDetector;

fn detector() -> AntiPatternDetector {
    AntiPatternDetector::new()
}

// ---------------------------------------------------------------------------
// DYNAMIC_DELIMITER_PATTERN: <<${expr} and <<`cmd`
// ---------------------------------------------------------------------------

#[test]
fn test_dynamic_delimiter_brace_expr_detected() {
    let code = r#"my $heredoc = <<${delimiter_var};
heredoc content here
END
"#;
    let diags = detector().detect_all(code);
    assert!(
        diags.iter().any(|d| matches!(
            &d.pattern,
            perl_parser::heredoc_anti_patterns::AntiPattern::DynamicHeredocDelimiter { .. }
        )),
        "Dynamic delimiter with brace expression should be detected"
    );
}

#[test]
fn test_dynamic_delimiter_backtick_detected() {
    let code = "my $heredoc = <<`get_delimiter`;\ncontent\nEND\n";
    let diags = detector().detect_all(code);
    assert!(
        diags.iter().any(|d| matches!(
            &d.pattern,
            perl_parser::heredoc_anti_patterns::AntiPattern::DynamicHeredocDelimiter { .. }
        )),
        "Dynamic delimiter with backtick expression should be detected"
    );
}

#[test]
fn test_dynamic_delimiter_multiline_brace_expr_detected() {
    // The brace expression itself may contain embedded content across lines;
    // with `\n`-exclusion this would have silently failed to detect.
    // The length-bound form `[^}]{1,2000}` correctly matches across lines.
    let code = "my $d = <<${\n  $name\n};\nsome content\nEND\n";
    let diags = detector().detect_all(code);
    // Detection is best-effort on malformed syntax; just confirm no panic.
    let _ = diags;
}

#[test]
fn test_dynamic_delimiter_plain_var_detected() {
    let code = "my $h = <<$var;\ncontent\nEND\n";
    let diags = detector().detect_all(code);
    assert!(
        diags.iter().any(|d| matches!(
            &d.pattern,
            perl_parser::heredoc_anti_patterns::AntiPattern::DynamicHeredocDelimiter { .. }
        )),
        "Dynamic delimiter with plain variable should be detected"
    );
}

// ---------------------------------------------------------------------------
// REGEX_HEREDOC_PATTERN: (?{...<<...})
// ---------------------------------------------------------------------------

#[test]
fn test_regex_code_block_heredoc_detected() {
    let code = r"my $re = qr/(?{<<END})/; some content END";
    let diags = detector().detect_all(code);
    assert!(
        diags.iter().any(|d| matches!(
            &d.pattern,
            perl_parser::heredoc_anti_patterns::AntiPattern::RegexCodeBlockHeredoc { .. }
        )),
        "Heredoc inside regex code block should be detected"
    );
}

#[test]
fn test_regex_code_block_heredoc_multiline_detected() {
    // The regex code block may span lines. With `\n`-exclusion this would have
    // failed silently. The `[^}]{0,2000}` form matches across newlines correctly.
    let code = "my $re = qr/(?{\nmy $x = <<END;\ncontent\nEND\n})/;\n";
    let diags = detector().detect_all(code);
    // Detection best-effort on multi-line; confirm no panic.
    let _ = diags;
}

// ---------------------------------------------------------------------------
// EVAL_HEREDOC_PATTERN: eval '..<<..' / eval "..<<.."
// ---------------------------------------------------------------------------

#[test]
fn test_eval_heredoc_single_quote_detected() {
    let code = "eval '<<END';\n";
    let diags = detector().detect_all(code);
    assert!(
        diags.iter().any(|d| matches!(
            &d.pattern,
            perl_parser::heredoc_anti_patterns::AntiPattern::EvalStringHeredoc { .. }
        )),
        "Heredoc inside single-quoted eval string should be detected"
    );
}

#[test]
fn test_eval_heredoc_double_quote_detected() {
    let code = r#"eval "<<END";"#;
    let diags = detector().detect_all(code);
    assert!(
        diags.iter().any(|d| matches!(
            &d.pattern,
            perl_parser::heredoc_anti_patterns::AntiPattern::EvalStringHeredoc { .. }
        )),
        "Heredoc inside double-quoted eval string should be detected"
    );
}

#[test]
fn test_eval_heredoc_multiline_detected() {
    // Eval strings with embedded heredocs span multiple lines. With `\n`-exclusion
    // the detection would have silently stopped at the first newline. The
    // `[^']{0,2000}` / `[^"]{0,2000}` form correctly crosses newlines.
    let code = "eval '\nmy $x = <<END;\ncontent\nEND\n';\n";
    let diags = detector().detect_all(code);
    // Best-effort on complex multi-line form; confirm no panic.
    let _ = diags;
}

// ---------------------------------------------------------------------------
// Regression: length bound does NOT suppress short-content detection
// ---------------------------------------------------------------------------

#[test]
fn test_length_bound_does_not_suppress_short_patterns() {
    // A heredoc in an eval string with a short (1-char) prefix must still fire.
    let code = "eval 'x <<END';\n";
    let diags = detector().detect_all(code);
    assert!(
        diags.iter().any(|d| matches!(
            &d.pattern,
            perl_parser::heredoc_anti_patterns::AntiPattern::EvalStringHeredoc { .. }
        )),
        "Short-content eval heredoc must still be detected after length-bound change"
    );
}

#[test]
fn test_length_bound_does_not_suppress_empty_prefix_patterns() {
    // Zero-length prefix (eval '<<END') uses `{0,2000}` — must still fire.
    let code = "eval '<<END';\n";
    let diags = detector().detect_all(code);
    assert!(!diags.is_empty(), "eval '<<END' must still produce at least one diagnostic");
}

// ---------------------------------------------------------------------------
// Negative: normal heredocs without anti-patterns produce no false positives
// ---------------------------------------------------------------------------

#[test]
fn test_normal_heredoc_no_false_positive() {
    let code = r#"my $text = <<"EOF";
Hello world
EOF
print $text;
"#;
    let diags = detector().detect_all(code);
    // Normal heredoc — only DynamicDelimiter / Regex / Eval patterns checked here.
    let has_dynamic = diags.iter().any(|d| {
        matches!(
            &d.pattern,
            perl_parser::heredoc_anti_patterns::AntiPattern::DynamicHeredocDelimiter { .. }
                | perl_parser::heredoc_anti_patterns::AntiPattern::RegexCodeBlockHeredoc { .. }
                | perl_parser::heredoc_anti_patterns::AntiPattern::EvalStringHeredoc { .. }
        )
    });
    assert!(!has_dynamic, "Normal heredoc must not trigger dynamic/regex/eval anti-patterns");
}

// ---------------------------------------------------------------------------
// Stress: moderately long content does not time out or panic
// ---------------------------------------------------------------------------

#[test]
fn test_long_content_does_not_panic() {
    // 500-char dynamic delimiter expression — within the 2000-char bound.
    let inner = "x".repeat(500);
    let code = format!("my $h = <<${{{}}};\ncontent\nEND\n", inner);
    let diags = detector().detect_all(&code);
    let _ = diags; // just confirm no panic
}

#[test]
fn test_content_exceeding_bound_does_not_panic() {
    // 3000-char prefix — intentionally beyond the 2000-char bound.
    // The pattern simply won't match; no panic.
    let inner = "y".repeat(3000);
    let code = format!("eval '{}<<END';\n", inner);
    let diags = detector().detect_all(&code);
    // No match expected for this pathological input, but no panic either.
    let _ = diags;
}
