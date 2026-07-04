/// Guardrail tests for ReDoS-prone heredoc anti-pattern regex patterns (#1756).
///
/// Four patterns in the heredoc anti-pattern detector used unbounded character
/// classes (e.g. `[^}]+`) that could exhibit super-linear matching when given
/// input containing unclosed delimiters.  The fix adds `\n` to each class so
/// the pattern anchors to a single source line, limiting the work per match.
///
/// These tests verify three properties:
///
/// 1. **Performance** — pathological single-line inputs (large buffers of chars
///    with no closing delimiter) complete in well under one second.  This would
///    hang on a genuinely exponential pattern even with small inputs.
///
/// 2. **Valid detection unchanged** — patterns still fire on well-formed Perl
///    anti-patterns (`<<${VAR}`, `(?{...<<...})`, `eval '...<<...'`).
///
/// 3. **Line-boundary correctness** — a delimiter that would only close on a
///    second source line is NOT matched (acceptable tradeoff; heredoc delimiters
///    are inherently single-line in Perl).
use perl_parser::heredoc_anti_patterns::{AntiPattern, AntiPatternDetector};
use std::time::Instant;

// ── performance guardrails ────────────────────────────────────────────────────

/// DYNAMIC_DELIMITER_PATTERN: 5 KB of `a` with no closing `}` must complete
/// well within one second.
#[test]
fn test_antip_no_redos_dynamic_5kb_unclosed() {
    let detector = AntiPatternDetector::new();
    // <<${ followed by 5000 'a's — no closing brace; pathological for [^}]+
    let pathological = format!("<<${{{}}}", "a".repeat(5000));
    let start = Instant::now();
    let _ = detector.detect_all(&pathological);
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_millis() < 1000,
        "DYNAMIC_DELIMITER_PATTERN took {}ms on pathological input (ReDoS?)",
        elapsed.as_millis()
    );
}

/// REGEX_HEREDOC_PATTERN: 5 KB of `a` with no closing `}` must complete
/// well within one second.
#[test]
fn test_antip_no_redos_regex_heredoc_5kb_unclosed() {
    let detector = AntiPatternDetector::new();
    // (?{ followed by 5000 'a's, then <<, still no closing }
    let pathological = format!("(?{{{}<<", "a".repeat(5000));
    let start = Instant::now();
    let _ = detector.detect_all(&pathological);
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_millis() < 1000,
        "REGEX_HEREDOC_PATTERN took {}ms on pathological input (ReDoS?)",
        elapsed.as_millis()
    );
}

/// EVAL_HEREDOC_PATTERN: 5 KB of `a` with no closing quote must complete
/// well within one second.
#[test]
fn test_antip_no_redos_eval_5kb_unclosed() {
    let detector = AntiPatternDetector::new();
    // eval ' followed by 5000 'a's, then <<, still no closing '
    let pathological = format!("eval '{}<<", "a".repeat(5000));
    let start = Instant::now();
    let _ = detector.detect_all(&pathological);
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_millis() < 1000,
        "EVAL_HEREDOC_PATTERN took {}ms on pathological input (ReDoS?)",
        elapsed.as_millis()
    );
}

/// Realistic 1 000-line Perl file must complete in well under 100 ms.
#[test]
fn test_antip_normal_file_performance() {
    let detector = AntiPatternDetector::new();
    let line = "my $x = 'hello world'; print $x;\n";
    let source = line.repeat(1000);
    let start = Instant::now();
    let _ = detector.detect_all(&source);
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_millis() < 100,
        "detect_all on 1000-line file took {}ms (performance regression?)",
        elapsed.as_millis()
    );
}

// ── valid-pattern detection (regression) ─────────────────────────────────────

/// `<<${VAR}` on a single line is a real dynamic-delimiter anti-pattern.
#[test]
fn test_antip_dynamic_delimiter_valid() {
    let detector = AntiPatternDetector::new();
    let source = r#"my $x = <<${DELIM}; print $x;"#;
    let diags = detector.detect_all(source);
    let found =
        diags.iter().any(|d| matches!(d.pattern, AntiPattern::DynamicHeredocDelimiter { .. }));
    assert!(found, "expected DynamicHeredocDelimiter for <<${{DELIM}}");
}

/// `<<$scalar` on a single line is a real dynamic-delimiter anti-pattern.
#[test]
fn test_antip_dynamic_delimiter_scalar() {
    let detector = AntiPatternDetector::new();
    let source = r#"my $delim = "END"; my $x = <<$delim; print $x;"#;
    let diags = detector.detect_all(source);
    let found =
        diags.iter().any(|d| matches!(d.pattern, AntiPattern::DynamicHeredocDelimiter { .. }));
    assert!(found, "expected DynamicHeredocDelimiter for <<$delim");
}

/// `(?{...<<...})` on a single line should still be detected.
#[test]
fn test_antip_regex_heredoc_valid() {
    let detector = AntiPatternDetector::new();
    let source = r#"my $re = qr/(?{my $x = <<END})/"#;
    let diags = detector.detect_all(source);
    let found =
        diags.iter().any(|d| matches!(d.pattern, AntiPattern::RegexCodeBlockHeredoc { .. }));
    assert!(found, "expected RegexCodeBlockHeredoc for (?{{...<<...}})");
}

/// `eval '...<<...'` on a single line should still be detected.
#[test]
fn test_antip_eval_heredoc_valid() {
    let detector = AntiPatternDetector::new();
    let source = r#"eval '<<END; print "hello"'"#;
    let diags = detector.detect_all(source);
    let found = diags.iter().any(|d| matches!(d.pattern, AntiPattern::EvalStringHeredoc { .. }));
    assert!(found, "expected EvalStringHeredoc for eval '...<<...'");
}

// ── line-boundary behaviour ───────────────────────────────────────────────────

/// A `<<${` where the closing `}` only appears on the next line must NOT be
/// matched — heredoc delimiters are single-line in Perl.  This is the
/// accepted accuracy tradeoff in exchange for bounded matching.
#[test]
fn test_antip_multiline_dynamic_delimiter_not_matched() {
    let detector = AntiPatternDetector::new();
    // Opening on line 1, closing } on line 2 — should not be detected.
    let source = "my $x = <<${\naaa\n};\n";
    let diags = detector.detect_all(source);
    let found =
        diags.iter().any(|d| matches!(d.pattern, AntiPattern::DynamicHeredocDelimiter { .. }));
    assert!(
        !found,
        "multiline <<${{...}} spanning a newline should not be detected (line-boundary tradeoff)"
    );
}

/// A `(?{` where the closing `}` is on the next line must NOT be matched.
#[test]
fn test_antip_multiline_regex_heredoc_not_matched() {
    let detector = AntiPatternDetector::new();
    let source = "my $re = qr/(?{my $x = <<END\n})/;\n";
    let diags = detector.detect_all(source);
    let found =
        diags.iter().any(|d| matches!(d.pattern, AntiPattern::RegexCodeBlockHeredoc { .. }));
    assert!(
        !found,
        "multiline (?{{...<<...}}) spanning a newline should not be detected (line-boundary tradeoff)"
    );
}

/// An `eval '...'` where the closing quote is on the next line must NOT match.
#[test]
fn test_antip_multiline_eval_heredoc_not_matched() {
    let detector = AntiPatternDetector::new();
    let source = "eval '<<END\nprint 1\n';\n";
    let diags = detector.detect_all(source);
    let found = diags.iter().any(|d| matches!(d.pattern, AntiPattern::EvalStringHeredoc { .. }));
    assert!(
        !found,
        "multiline eval '...<<...' spanning a newline should not be detected (line-boundary tradeoff)"
    );
}

// ── clean file produces no false positives ────────────────────────────────────

/// A normal Perl file with no heredoc anti-patterns must produce no diagnostics.
#[test]
fn test_antip_no_false_positives_on_clean_code() {
    let detector = AntiPatternDetector::new();
    let source = r#"
package Foo;
use strict;
use warnings;

sub greet {
    my ($name) = @_;
    my $msg = <<END;
Hello, $name!
END
    print $msg;
}

greet("world");
1;
"#;
    let diags = detector.detect_all(source);
    // Regular heredoc with a literal delimiter should not trigger anti-patterns
    let heredoc_antipattern_diags: Vec<_> = diags
        .iter()
        .filter(|d| {
            matches!(
                d.pattern,
                AntiPattern::DynamicHeredocDelimiter { .. }
                    | AntiPattern::RegexCodeBlockHeredoc { .. }
                    | AntiPattern::EvalStringHeredoc { .. }
            )
        })
        .collect();
    assert!(
        heredoc_antipattern_diags.is_empty(),
        "clean code should not trigger heredoc anti-patterns, got: {:?}",
        heredoc_antipattern_diags
    );
}
