use crate::syntax::event::{
    RegexEvent, RegexEventKind, RegexEventStream, RegexGroupKind, RegexModeState,
    parse_regex_events,
};

use super::RegexRange;
use super::analysis::{RegexDiagnostic, RegexDiagnosticClass, RegexDiagnosticCode};
use super::complexity::{ComplexityWork, scan_complexity};
use super::config::RegexValidationConfig;

fn config(max_nesting: usize, max_branch_reset_branches: usize) -> RegexValidationConfig {
    RegexValidationConfig { max_nesting, max_unicode_properties: 50, max_branch_reset_branches }
}

fn parse(pattern: &str) -> RegexEventStream {
    parse_regex_events(pattern, RegexModeState::default())
}

fn nested_lookbehinds(depth: usize) -> String {
    let mut pattern = String::from("x");
    for _ in 0..depth {
        pattern = format!("(?<={pattern})");
    }
    pattern
}

fn nested_branch_resets(depth: usize) -> String {
    let mut pattern = String::from("x");
    for _ in 0..depth {
        pattern = format!("(?|{pattern})");
    }
    pattern
}

fn of_code(diagnostics: &[RegexDiagnostic], code: RegexDiagnosticCode) -> Vec<&RegexDiagnostic> {
    diagnostics.iter().filter(|diagnostic| diagnostic.code == code).collect()
}

fn synthetic(kinds: &[RegexEventKind]) -> RegexEventStream {
    RegexEventStream {
        events: kinds
            .iter()
            .enumerate()
            .map(|(index, kind)| RegexEvent {
                kind: *kind,
                range: RegexRange { start: index, end: index + 1 },
                mode: RegexModeState::default(),
                depth: 0,
            })
            .collect(),
        exhausted: None,
        malformed: false,
    }
}

fn quadratic_stack_scan_visits(nested_opens: usize) -> usize {
    // Previous algorithm visited 0 + 1 + ... + (n-1) frames across n nested opens.
    nested_opens.saturating_mul(nested_opens.saturating_sub(1)) / 2
}

#[test]
fn lookbehind_limit_identity_range_and_emit_once_are_stable() {
    let stream = parse("(?<=(?<=a))(?<=(?<=b))");
    let scan = scan_complexity(&stream, &config(1, 50));
    let hits = of_code(&scan.diagnostics, RegexDiagnosticCode::LookbehindNestingLimit);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].class, RegexDiagnosticClass::PolicyLimit);
    assert_eq!(hits[0].limit, Some(1));
    assert_eq!(hits[0].range, RegexRange { start: 4, end: 8 });
}

#[test]
fn negative_lookbehind_uses_the_same_lookbehind_depth_counter() {
    let stream = parse("(?<!(?<!x))");
    let scan = scan_complexity(&stream, &config(1, 50));
    let hits = of_code(&scan.diagnostics, RegexDiagnosticCode::LookbehindNestingLimit);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].range, RegexRange { start: 4, end: 8 });
}

#[test]
fn lookahead_nesting_does_not_emit_lookbehind_limits() {
    let stream = parse("(?=(?=x))");
    let scan = scan_complexity(&stream, &config(1, 50));
    assert!(of_code(&scan.diagnostics, RegexDiagnosticCode::LookbehindNestingLimit).is_empty());
}

#[test]
fn sequential_lookbehinds_do_not_accumulate_depth() {
    let stream = parse("(?<=a)(?<=b)(?<=c)");
    let scan = scan_complexity(&stream, &config(1, 50));
    assert!(scan.diagnostics.is_empty());
    assert_eq!(scan.lookbehind_depth, 0);
    assert_eq!(scan.open_frames, 0);
}

#[test]
fn wrapping_other_groups_do_not_inflate_lookbehind_depth() {
    let stream = parse("((?:(?<=x)))");
    let scan = scan_complexity(&stream, &config(1, 50));
    assert!(of_code(&scan.diagnostics, RegexDiagnosticCode::LookbehindNestingLimit).is_empty());
}

#[test]
fn lookbehind_at_max_nesting_is_silent_and_one_past_emits() {
    let at_limit = parse(&nested_lookbehinds(2));
    assert!(scan_complexity(&at_limit, &config(2, 50)).diagnostics.is_empty());

    let over = parse(&nested_lookbehinds(3));
    let scan = scan_complexity(&over, &config(2, 50));
    let hits = of_code(&scan.diagnostics, RegexDiagnosticCode::LookbehindNestingLimit);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].limit, Some(2));
}

#[test]
fn max_nesting_zero_emits_on_the_first_lookbehind() {
    let stream = parse("(?<=x)");
    let scan = scan_complexity(&stream, &config(0, 50));
    let hits = of_code(&scan.diagnostics, RegexDiagnosticCode::LookbehindNestingLimit);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].range, RegexRange { start: 0, end: 4 });
    assert_eq!(hits[0].limit, Some(0));
}

#[test]
fn branch_reset_nesting_identity_range_and_emit_once_are_stable() {
    let stream = parse("(?|(?|a))(?|(?|b))");
    let scan = scan_complexity(&stream, &config(1, 50));
    let hits = of_code(&scan.diagnostics, RegexDiagnosticCode::BranchResetNestingLimit);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].class, RegexDiagnosticClass::PolicyLimit);
    assert_eq!(hits[0].limit, Some(1));
    assert_eq!(hits[0].range, RegexRange { start: 3, end: 6 });
}

#[test]
fn lookbehind_inside_branch_reset_does_not_count_as_branch_reset_nesting() {
    let stream = parse("(?|(?<=x))");
    let scan = scan_complexity(&stream, &config(1, 50));
    assert!(of_code(&scan.diagnostics, RegexDiagnosticCode::BranchResetNestingLimit).is_empty());
    assert!(of_code(&scan.diagnostics, RegexDiagnosticCode::LookbehindNestingLimit).is_empty());
}

#[test]
fn nested_branch_reset_inside_lookbehind_uses_its_own_counter() {
    let stream = parse("(?<=(?|(?|a)))");
    let scan = scan_complexity(&stream, &config(1, 50));
    let hits = of_code(&scan.diagnostics, RegexDiagnosticCode::BranchResetNestingLimit);
    assert_eq!(hits.len(), 1);
    assert!(of_code(&scan.diagnostics, RegexDiagnosticCode::LookbehindNestingLimit).is_empty());
}

#[test]
fn branch_reset_at_max_nesting_is_silent_and_one_past_emits() {
    let at_limit = parse(&nested_branch_resets(2));
    assert!(scan_complexity(&at_limit, &config(2, 50)).diagnostics.is_empty());

    let over = parse(&nested_branch_resets(3));
    let scan = scan_complexity(&over, &config(2, 50));
    assert_eq!(of_code(&scan.diagnostics, RegexDiagnosticCode::BranchResetNestingLimit).len(), 1);
}

#[test]
fn branch_reset_branch_limit_identity_range_and_emit_once_are_stable() {
    let stream = parse("(?|a|b|c)(?|d|e|f)");
    let scan = scan_complexity(&stream, &config(10, 1));
    let hits = of_code(&scan.diagnostics, RegexDiagnosticCode::BranchResetBranchLimit);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].class, RegexDiagnosticClass::PolicyLimit);
    assert_eq!(hits[0].limit, Some(1));
    // First `|` in the first group is the event that crosses 1 → 2.
    assert_eq!(hits[0].range, RegexRange { start: 4, end: 5 });
}

#[test]
fn inner_capturing_alternation_does_not_count_toward_outer_branch_reset() {
    let stream = parse("(?|a|(b|c|d|e))");
    let scan = scan_complexity(&stream, &config(10, 2));
    assert!(of_code(&scan.diagnostics, RegexDiagnosticCode::BranchResetBranchLimit).is_empty());
}

#[test]
fn inner_branch_reset_branches_do_not_inflate_the_outer_count() {
    let stream = parse("(?|a|(?|b|c|d))");
    let scan = scan_complexity(&stream, &config(10, 2));
    assert!(of_code(&scan.diagnostics, RegexDiagnosticCode::BranchResetBranchLimit).is_empty());
}

#[test]
fn alternation_outside_branch_reset_is_ignored() {
    let stream = parse("(?:a|b|c|d)");
    let scan = scan_complexity(&stream, &config(10, 1));
    assert!(scan.diagnostics.is_empty());
}

#[test]
fn branch_reset_open_does_not_emit_branch_limit_without_alternation() {
    let stream = parse("(?|a)");
    let scan = scan_complexity(&stream, &config(10, 0));
    assert!(of_code(&scan.diagnostics, RegexDiagnosticCode::BranchResetBranchLimit).is_empty());
}

#[test]
fn unmatched_group_close_does_not_underflow_counters() {
    let stream = synthetic(&[
        RegexEventKind::GroupClose(RegexGroupKind::Lookbehind),
        RegexEventKind::GroupOpen(RegexGroupKind::Lookbehind),
        RegexEventKind::GroupClose(RegexGroupKind::Lookbehind),
        RegexEventKind::GroupClose(RegexGroupKind::BranchReset),
    ]);
    let scan = scan_complexity(&stream, &config(1, 50));
    assert!(scan.diagnostics.is_empty());
    assert_eq!(scan.lookbehind_depth, 0);
    assert_eq!(scan.branch_reset_depth, 0);
    assert_eq!(scan.open_frames, 0);
}

#[test]
fn unclosed_groups_leave_matching_running_depths() {
    let stream = synthetic(&[
        RegexEventKind::GroupOpen(RegexGroupKind::Capturing),
        RegexEventKind::GroupOpen(RegexGroupKind::Lookbehind),
        RegexEventKind::GroupOpen(RegexGroupKind::BranchReset),
        RegexEventKind::GroupOpen(RegexGroupKind::NegativeLookbehind),
    ]);
    let scan = scan_complexity(&stream, &config(10, 50));
    assert_eq!(scan.lookbehind_depth, 2);
    assert_eq!(scan.branch_reset_depth, 1);
    assert_eq!(scan.open_frames, 4);
}

#[test]
fn unicode_property_limit_is_unchanged_and_still_emits_once() {
    let stream = parse(r"\p{L}\p{N}\p{X}");
    let scan = scan_complexity(
        &stream,
        &RegexValidationConfig {
            max_nesting: 10,
            max_unicode_properties: 1,
            max_branch_reset_branches: 50,
        },
    );
    let hits = of_code(&scan.diagnostics, RegexDiagnosticCode::UnicodePropertyLimit);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].limit, Some(1));
}

#[test]
fn nested_lookbehinds_do_not_pay_quadratic_stack_scans() {
    const DEPTH: usize = 32;
    let old_visits = quadratic_stack_scan_visits(DEPTH);
    assert_eq!(old_visits, 496, "control: the retired scan visited n(n-1)/2 frames");

    let stream = parse(&nested_lookbehinds(DEPTH));
    let scan = scan_complexity(&stream, &config(DEPTH, 50));
    assert!(scan.diagnostics.is_empty());
    assert_linear_work(&scan.work, stream.events.len(), old_visits);
}

#[test]
fn nested_branch_resets_do_not_pay_quadratic_stack_scans() {
    const DEPTH: usize = 32;
    let old_visits = quadratic_stack_scan_visits(DEPTH);
    assert_eq!(old_visits, 496);

    let stream = parse(&nested_branch_resets(DEPTH));
    let scan = scan_complexity(&stream, &config(DEPTH, 50));
    assert!(scan.diagnostics.is_empty());
    assert_linear_work(&scan.work, stream.events.len(), old_visits);
}

#[test]
fn mixed_deep_nesting_stays_linear_in_event_count() {
    const DEPTH: usize = 24;
    let mut pattern = String::from("x");
    for index in 0..DEPTH {
        pattern = if index % 2 == 0 { format!("(?<={pattern})") } else { format!("(?|{pattern})") };
    }
    let stream = parse(&pattern);
    let scan = scan_complexity(&stream, &config(DEPTH, 50));
    assert!(scan.diagnostics.is_empty());
    assert_eq!(scan.lookbehind_depth, 0);
    assert_eq!(scan.branch_reset_depth, 0);
    assert_linear_work(&scan.work, stream.events.len(), quadratic_stack_scan_visits(DEPTH));
}

#[test]
fn complexity_source_does_not_rescan_the_open_group_stack() {
    let source = include_str!("complexity.rs");
    assert!(!source.contains(".iter()"), "complexity walk must not iterate the open-group stack");
    assert!(
        !source.contains(".filter("),
        "complexity walk must not filter the open-group stack to recover depth"
    );
    assert!(
        !source.contains("&self.frames"),
        "complexity walk must not borrow the frame stack for a scan"
    );
}

fn assert_linear_work(work: &ComplexityWork, event_count: usize, old_scan_visits: usize) {
    assert_eq!(work.events, event_count);
    assert_eq!(
        work.depth_scan_visits, 0,
        "running counters must not visit stack frames to compute depth"
    );
    assert!(
        work.frame_ops <= work.events,
        "frame mutations must stay O(1) per event, got {} ops over {} events",
        work.frame_ops,
        work.events
    );
    assert!(
        work.frame_ops + work.depth_scan_visits < old_scan_visits,
        "counted work {} must stay below the retired O(depth) scan of {old_scan_visits}",
        work.frame_ops + work.depth_scan_visits
    );
}
