//! Mutation-proof boundary tests for the pure TAP facts parser.
//!
//! Each test pins one parser decision through the public `parse_tap` API.
//! These tests are intentionally separate from the unit tests so RIPR can
//! identify the public behavior that must remain exposed when parser branches
//! change.

use perl_test_facts::{TapAssertionStatus, parse_tap};

// ── SEAM A: indentation belongs to nested TAP streams ───────────────────────

/// A nested assertion is retained with its depth, while the top-level plan
/// counts only the top-level assertion.
///
/// Mutating the indentation calculation or removing the depth filter from
/// plan validation changes either the recorded depth or the diagnostics.
#[test]
fn seam_nested_tap_depth_is_preserved_and_excluded_from_top_level_plan() {
    let report = parse_tap("TAP version 13\n    ok 1 - inner\n    1..1\nok 1 - outer\n1..1\n");

    assert_eq!(report.assertions.len(), 2);
    assert_eq!(report.assertions[0].depth, 1);
    assert_eq!(report.assertions[0].status, TapAssertionStatus::Pass);
    assert_eq!(report.assertions[1].depth, 0);
    assert_eq!(report.plan.as_ref().map(|plan| plan.end), Some(1));
    assert!(report.diagnostics.is_empty(), "nested plan must not mismatch the top-level plan");
}

// ── SEAM B: only recognized directives split assertion descriptions ──────────

/// A literal hash in an assertion name is not a directive unless the marker
/// is TODO or SKIP.
///
/// Mutating the directive classifier back to "split at every hash" changes
/// the exact name and introduces a false unknown directive.
#[test]
fn seam_literal_hash_remains_part_of_assertion_name() {
    let report = parse_tap("ok 1 - check # FLAKY\n1..1\n");

    assert_eq!(report.assertions[0].name.as_deref(), Some("check # FLAKY"));
    assert_eq!(report.assertions[0].directive, None);
    assert_eq!(report.unknown_count(), 0);
    assert!(report.is_success());
}

// ── SEAM C: unknown protocol lines are retained as raw evidence ──────────────

/// An unrecognized non-comment line remains observable without becoming a
/// structural diagnostic or changing hard-result success.
///
/// Mutating the fallback from `raw_lines` to `diagnostics` changes both the
/// evidence location and the result contract.
#[test]
fn seam_unknown_non_comment_line_is_non_fatal_raw_evidence() {
    let report = parse_tap("ok 1 - check\nfuture TAP extension\n1..1\n");

    assert_eq!(report.raw_lines, vec!["future TAP extension"]);
    assert!(report.diagnostics.is_empty());
    assert!(report.is_success());
}

// ── SEAM D: hard result is independent from structural validity ──────────────

/// A plan mismatch is reported separately while a run with no hard assertion
/// failure or bailout remains a successful hard result.
///
/// Mutating `is_success` to require a plan or empty diagnostics changes this
/// distinction and makes the accepted ADR semantics unobservable.
#[test]
fn seam_plan_mismatch_does_not_change_hard_result_success() {
    let report = parse_tap("1..2\nok 1 - only assertion\n");

    assert!(
        report.diagnostics.iter().any(|diagnostic| diagnostic.contains("declares 2 assertions")),
        "the plan mismatch must remain visible as a diagnostic"
    );
    assert!(report.is_success());
}

// ── SEAM E: bailout is terminal and case-insensitive ────────────────────────

/// Once a bailout is observed, later TAP records remain raw evidence and do
/// not create a plan or assertions. The marker is case-insensitive per TAP.
#[test]
fn seam_bailout_terminates_semantic_parsing() {
    let report = parse_tap("ok 1 - starts\nbAIL OUT! stopped\nok 2 - after\n1..2\n");

    assert_eq!(report.bail_out.as_deref(), Some("stopped"));
    assert_eq!(report.assertions.len(), 1);
    assert_eq!(report.plan, None);
    assert_eq!(report.raw_lines, vec!["ok 2 - after", "1..2"]);
    assert!(!report.is_success());
}

// ── SEAM F: malformed YAML cannot swallow a later plan ──────────────────────

/// A plan before a YAML terminator interrupts the pending block instead of
/// attaching its following text to the previous assertion.
#[test]
fn seam_yaml_block_does_not_swallow_a_later_plan() {
    let report = parse_tap("not ok 1 - broken\n  ---\n1..1\n  message: raw\n  ...\n");

    assert_eq!(report.assertions.len(), 1);
    assert!(report.assertions[0].diagnostics.is_empty());
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains("interrupted before terminator"))
    );
}

// ── SEAM G: plan order and assertion numbering are structural facts ──────────

/// Plans between top-level assertions and duplicate assertion numbers are
/// diagnosed even when the count and range superficially match.
#[test]
fn seam_plan_order_and_duplicate_numbers_are_diagnosed() {
    let report = parse_tap("ok 1 - first\n1..2\nok 1 - duplicate\n");

    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains("between top-level assertions"))
    );
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains("duplicate top-level assertion number 1"))
    );
}

// ── SEAM H: malformed indentation is not coerced to depth zero ──────────────

/// One-to-three leading spaces are retained as raw evidence rather than
/// becoming a top-level assertion through integer division.
#[test]
fn seam_partial_indentation_is_not_a_top_level_assertion() {
    let report = parse_tap("  ok 1 - malformed\nok 1 - valid\n1..1\n");

    assert_eq!(report.assertions.len(), 1);
    assert_eq!(report.assertions[0].name.as_deref(), Some("valid"));
    assert_eq!(report.raw_lines, vec!["  ok 1 - malformed"]);
    assert!(report.diagnostics.is_empty());
}

// ── SEAM I: line-ending normalization preserves stream records ───────────────

/// TAP streams using lone carriage returns still produce separate records and
/// preserve the same line-oriented facts as LF input.
#[test]
fn seam_lone_carriage_returns_are_normalized() {
    let report = parse_tap("ok 1 - lone CR\r1..1\r");

    assert_eq!(report.assertions.len(), 1);
    assert_eq!(report.assertions[0].name.as_deref(), Some("lone CR"));
    assert_eq!(report.plan.as_ref().map(|plan| plan.end), Some(1));
}

// ── SEAM J: YAML content is delimited by indentation ────────────────────────

/// YAML scalar text that resembles TAP remains attached to the YAML block.
#[test]
fn seam_yaml_tap_looking_scalar_is_not_an_assertion() {
    let report = parse_tap("not ok 1 - broken\n  ---\n  ok 2\n  ...\n1..1\n");

    assert_eq!(report.assertions.len(), 1);
    assert_eq!(report.assertions[0].diagnostics[1], "  ok 2");
    assert!(report.diagnostics.is_empty());
}

// ── SEAM K: adjacency and bailout grammar are explicit ──────────────────────

/// Blank/comment lines do not break YAML eligibility, and a non-delimited
/// bailout prefix remains raw evidence rather than terminating the report.
#[test]
fn seam_yaml_comment_gap_and_bailout_delimiter_are_checked() {
    let separated = parse_tap("not ok 1 - broken\n\n# separated\n  ---\n  message: raw\n  ...\n");
    assert_eq!(separated.diagnostics, Vec::<String>::new());
    assert_eq!(separated.assertions[0].diagnostics[1], "  message: raw");

    let prefixed = parse_tap("Bail out!ish text\nok 1 - valid\n1..1\n");
    assert_eq!(prefixed.bail_out, None);
    assert_eq!(prefixed.assertions.len(), 1);
    assert_eq!(prefixed.raw_lines, vec!["Bail out!ish text"]);
}
