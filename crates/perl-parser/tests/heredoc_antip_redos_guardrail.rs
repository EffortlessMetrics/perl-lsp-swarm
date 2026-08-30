//! Scan-bound and coverage proof for the heredoc anti-pattern detectors.
//!
//! Every assertion here runs through [`AntiPatternDetector::detect_all`], the
//! same entry point the LSP diagnostics provider calls. An earlier revision of
//! this file re-declared private copies of the detector regexes and asserted
//! against those copies, so it stayed green no matter what the detectors did;
//! it also probed `Regex::captures`, while production consumes the patterns
//! through `captures_iter`. Both gaps are closed here (#3597).
//!
//! Two guardrail dimensions are proved:
//!
//! * **Coverage** — the multi-line constructs that #3568's newline exclusion
//!   silently dropped are detected again, and single-line detection is unchanged.
//! * **Scan bound** — adversarial input stays linear and fast. The absolute
//!   ceilings catch a large-constant regression (notably the `{0,N}`-bounded
//!   candidate rejected in #3597, which is ~6600x slower on the dense shape);
//!   the scaling assertion catches a superlinear regression.

use perl_parser::heredoc_anti_patterns::{AntiPattern, AntiPatternDetector, Diagnostic};
use std::time::{Duration, Instant};

fn detect(code: &str) -> Vec<Diagnostic> {
    AntiPatternDetector::new().detect_all(code)
}

fn has_regex_code_block(code: &str) -> bool {
    detect(code).iter().any(|d| matches!(d.pattern, AntiPattern::RegexCodeBlockHeredoc { .. }))
}

fn has_eval_string(code: &str) -> bool {
    detect(code).iter().any(|d| matches!(d.pattern, AntiPattern::EvalStringHeredoc { .. }))
}

fn has_dynamic_delimiter(code: &str) -> bool {
    detect(code).iter().any(|d| matches!(d.pattern, AntiPattern::DynamicHeredocDelimiter { .. }))
}

/// Median of five `detect_all` runs, to keep wall-clock guardrails stable
/// under noisy shared CI runners.
fn median_detect_time(code: &str) -> Duration {
    let mut samples: Vec<Duration> = (0..5)
        .map(|_| {
            let start = Instant::now();
            let found = detect(code);
            let elapsed = start.elapsed();
            // Keep the work observable so it cannot be optimized away.
            assert!(found.len() < usize::MAX);
            elapsed
        })
        .collect();
    samples.sort_unstable();
    samples[2]
}

// ---------------------------------------------------------------------------
// Coverage: multi-line constructs restored (#3597)
// ---------------------------------------------------------------------------

#[test]
fn antip_detects_multiline_regex_code_block_heredoc() {
    // A heredoc inside a `(?{ ... })` regex code block. The construct is
    // multi-line by necessity: the heredoc body must reach its terminator.
    // #3568's `[^}\n]*` could not cross the newline, so this was undetected.
    let code = "m/pattern(?{\n    print <<'MATCH';\nMatch text\nMATCH\n})/;\n";

    assert!(
        has_regex_code_block(code),
        "multi-line heredoc inside a regex code block must be reported; \
         got {:?}",
        detect(code).iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

#[test]
fn antip_detects_multiline_eval_string_heredoc() {
    // An eval string declaring a heredoc must span newlines to reach the
    // terminator, so `[^\n']*` dropped every real occurrence.
    let code = "eval 'print <<\"EVAL\";\nbody text\nEVAL\n';\n";

    assert!(
        has_eval_string(code),
        "multi-line heredoc inside an eval string must be reported; got {:?}",
        detect(code).iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

#[test]
fn antip_dynamic_delimiter_keeps_its_newline_horizon() {
    // Deliberately NOT widened. A dynamic delimiter has no reason to span
    // newlines, so crossing them would only add false positives on multi-line
    // left shifts (see `antip_multiline_left_shift_is_not_a_dynamic_delimiter`)
    // without recovering any real detection.
    let code = "my $content = <<${\nVARNAME};\n";

    assert!(
        !has_dynamic_delimiter(code),
        "dynamic delimiter must not cross a newline; got {:?}",
        detect(code).iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// Coverage regression guards: single-line detection is unchanged
// ---------------------------------------------------------------------------

#[test]
fn antip_still_detects_single_line_constructs() {
    assert!(has_regex_code_block("m/a(?{b<<'X'})c/;\n"), "single-line regex code block");
    assert!(has_eval_string("eval 'print <<EOF;';\n"), "single-line eval string");
    assert!(has_dynamic_delimiter("my $x = <<${FOO_BAR};\n"), "single-line dynamic delimiter");
    assert!(has_dynamic_delimiter("my $x = <<$delimiter;\n"), "bare scalar delimiter");
}

// ---------------------------------------------------------------------------
// Negative controls
// ---------------------------------------------------------------------------

#[test]
fn antip_reports_nothing_on_ordinary_perl() {
    // Negative control for the whole suite: if this ever fires, the positive
    // assertions above prove nothing.
    let code = "use strict;\nuse warnings;\n\nsub add {\n    my ($a, $b) = @_;\n    return $a + $b;\n}\n\nprint add(1, 2), \"\\n\";\n";

    let found = detect(code);
    assert!(
        found.is_empty(),
        "ordinary Perl must produce no anti-pattern diagnostics; got {:?}",
        found.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

#[test]
fn antip_ignores_constructs_inside_comments_and_strings() {
    // `mask_non_code_regions` blanks these before the scan. Widening the
    // character classes must not let a commented or quoted construct through.
    let code = "# my $x = <<${NAME};\nmy $s = \"a <<${NAME} b\";\n";

    let found = detect(code);
    assert!(
        found.is_empty(),
        "commented and quoted constructs must not be reported; got {:?}",
        found.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

#[test]
fn antip_eval_fragment_in_a_comment_does_not_seed_a_match() {
    // The eval detector must scan raw source, because masking would blank the
    // contents of the very quoted string it needs to look inside. It therefore
    // checks each match origin against the masked view instead. Without that
    // check, an `eval '` fragment in a comment joins unrelated later lines.
    let code = "# eval '\nmy $x = 1 << 2;\nmy $s = 'ok';\n";

    assert!(
        !has_eval_string(code),
        "an eval fragment inside a comment must not be reported; got {:?}",
        detect(code).iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // Same origin check on one line, and inside a string literal.
    assert!(!has_eval_string("# eval 'x<<y';\n"), "commented eval must not be reported");
    assert!(
        !has_eval_string("my $s = \"eval 'x<<y'\";\n"),
        "an eval fragment inside a string literal must not be reported"
    );
}

#[test]
fn antip_widened_branches_are_masked_inside_comments_and_strings() {
    // `antip_ignores_constructs_inside_comments_and_strings` only exercises the
    // dynamic-delimiter pattern, which this PR does not widen. These are the
    // equivalent controls for the two branches that *were* widened, so a
    // regression in either direction cannot ship green.
    for (label, code) in [
        // Eval: a commented `eval '` bridging to a later quoted string.
        ("eval bridging from a comment", "# eval '\nmy $s = 'a <<'B';\n';\n"),
        // Eval: origin inside a string literal rather than a comment.
        ("eval inside a string literal", "my $s = \"eval 'x<<y'\";\n"),
        // Regex code block anchored in a comment, closing on a later line.
        ("regex code block from a comment", "# (?{ some\n<<'X' })c\n"),
    ] {
        let found = detect(code);
        assert!(
            found.is_empty(),
            "{label}: masked region must not seed a diagnostic; got {:?}",
            found.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }
}

#[test]
fn antip_eval_keyword_is_not_matched_as_an_identifier_suffix() {
    // Without a leading `\b`, `myeval '...<<...'` matched at the `eval` suffix
    // and reported PL805 on a valid custom-function call.
    assert!(!has_eval_string("myeval 'print <<EOF;';\n"), "single-line custom eval-like call");
    assert!(
        !has_eval_string("myeval 'print <<\"E\";\nbody\nE\n';\n"),
        "multi-line custom eval-like call"
    );

    // The real keyword still reports, including as a statement-initial token.
    assert!(has_eval_string("eval 'print <<EOF;';\n"), "bare eval must still be reported");
    assert!(
        has_eval_string("my $r = eval 'print <<EOF;';\n"),
        "eval after an assignment must still be reported"
    );
}

#[test]
fn antip_multiline_left_shift_is_not_a_dynamic_delimiter() {
    // `1 << ${...}` is a left shift of a scalar dereference, not a heredoc.
    // Keeping the dynamic-delimiter newline horizon keeps this out.
    let code = "my $x = 1 << ${\nfoo};\n";

    assert!(
        !has_dynamic_delimiter(code),
        "a multi-line left shift must not be reported as a dynamic delimiter; got {:?}",
        detect(code).iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

#[test]
fn antip_unterminated_constructs_do_not_match() {
    // The scan bound comes from each negated class excluding its own
    // terminator, so an unterminated construct must simply not match rather
    // than run to end of file. This is the property that replaces the `\n`
    // horizon; if it regresses, the guardrails below lose their meaning.
    let never_closed = format!("m/x(?{{{}<<{}", "a".repeat(4096), "b".repeat(4096));
    assert!(
        !has_regex_code_block(&never_closed),
        "an unclosed regex code block must not be reported"
    );

    let unclosed_eval = format!("eval '{}<<{}", "a".repeat(4096), "b".repeat(4096));
    assert!(!has_eval_string(&unclosed_eval), "an unclosed eval string must not be reported");
}

// ---------------------------------------------------------------------------
// Scan bound
// ---------------------------------------------------------------------------

/// Dense candidate starts that never close — the shape that forces the engine
/// to consider every start position.
///
/// Measured on current main at 40 KB: ~0.1 ms for the shipped patterns and
/// ~582 ms for the rejected `{0,2000}`-bounded candidate. The 150 ms ceiling
/// sits far above the former and well below the latter, so it discriminates
/// without flaking on a slow runner.
#[test]
fn antip_dense_unclosed_input_stays_bounded() {
    let code = "(?{<<".repeat(8000);
    assert_eq!(code.len(), 40_000);

    let elapsed = median_detect_time(&code);

    assert!(
        elapsed < Duration::from_millis(150),
        "detect_all took {:?} on 40KB of dense unclosed candidates; expected <150ms",
        elapsed
    );
}

/// The same bound where the constructs actually close, so `captures_iter`
/// performs many successive searches — the case the `regex` crate documents as
/// `O(m*n^2)` in the general worst case.
#[test]
fn antip_many_matches_stay_bounded() {
    let code = "m/a(?{b<<'X'})c/;\n".repeat(4000);

    let elapsed = median_detect_time(&code);

    assert!(
        elapsed < Duration::from_millis(150),
        "detect_all took {:?} on 4000 matching constructs; expected <150ms",
        elapsed
    );
}

/// Scan work must stay linear in input size. A superlinear regression shows up
/// here even if the absolute ceilings above are still met.
#[test]
fn antip_scan_work_scales_linearly() {
    let small = "(?{<<".repeat(4000);
    let large = "(?{<<".repeat(16_000);
    assert_eq!(large.len(), small.len() * 4);

    let small_time = median_detect_time(&small).as_nanos().max(1);
    let large_time = median_detect_time(&large).as_nanos().max(1);

    // Quadratic growth over a 4x input would be ~16x. Allow 8x for constant
    // overhead and runner noise; that still separates linear from quadratic.
    let ratio = large_time / small_time;
    assert!(
        ratio <= 8,
        "detect_all grew {}x over a 4x input increase ({}ns -> {}ns); expected roughly linear",
        ratio,
        small_time,
        large_time
    );
}

/// A realistic file must not regress.
#[test]
fn antip_normal_file_performance() {
    let mut code = String::new();
    for i in 0..1000 {
        code.push_str(&format!("sub routine_{i} {{ my $x = {i}; }} # line {i}\n"));
    }

    let elapsed = median_detect_time(&code);

    assert!(
        elapsed < Duration::from_millis(100),
        "detect_all took {:?} on a 1000-line file; expected <100ms",
        elapsed
    );
}
