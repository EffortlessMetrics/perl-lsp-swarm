#![deny(clippy::map_err_ignore)]

//! Public-boundary contract tests for canonical TAP result facts.

use perl_test_facts::{TapAssertionOutcome, TapAssertionStatus, parse_tap};

#[test]
fn skip_all_plan_is_clean_at_the_public_boundary() {
    let report = parse_tap("TAP version 13\n1..0 # SKIP database unavailable\n");

    assert_eq!(report.version, Some(13));
    assert_eq!(
        report
            .plan
            .as_ref()
            .map(|plan| { (plan.start, plan.end, plan.directive.as_deref(), plan.line,) }),
        Some((1, 0, Some("SKIP database unavailable"), 2))
    );
    assert!(report.assertions.is_empty());
    assert!(report.diagnostics.is_empty());
    assert!(report.raw_lines.is_empty());
    assert!(report.is_success());
}

#[test]
fn nested_assertions_do_not_satisfy_the_top_level_plan() {
    let report = parse_tap("    ok 1 - nested\n1..1\n");

    assert_eq!(report.assertions.len(), 1);
    assert_eq!(report.assertions.first().map(|assertion| assertion.depth), Some(1));
    assert_eq!(report.assertions.iter().filter(|assertion| assertion.depth == 0).count(), 0);
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic == "plan on line 2 declares 1 assertions but 0 were parsed"
    }));
    assert!(
        report.is_success(),
        "a structural plan mismatch stays independent from hard assertion success"
    );
}

#[test]
fn directives_do_not_erase_raw_outcomes_or_hard_failure_counts() {
    let report = parse_tap(
        "1..5\n\
         ok 1 - todo passed # TODO remove workaround\n\
         not ok 2 - todo failed # TODO implement later\n\
         ok 3 - skipped pass # SKIP platform\n\
         not ok 4 - skipped failure # SKIP prerequisite\n\
         not ok 5 - hard failure\n",
    );

    assert_eq!(
        report.assertions.iter().map(|assertion| assertion.status).collect::<Vec<_>>(),
        vec![
            TapAssertionStatus::Todo,
            TapAssertionStatus::Todo,
            TapAssertionStatus::Skip,
            TapAssertionStatus::Skip,
            TapAssertionStatus::Fail,
        ]
    );
    assert_eq!(
        report.assertions.iter().map(|assertion| assertion.outcome).collect::<Vec<_>>(),
        vec![
            TapAssertionOutcome::Pass,
            TapAssertionOutcome::Fail,
            TapAssertionOutcome::Pass,
            TapAssertionOutcome::Fail,
            TapAssertionOutcome::Fail,
        ]
    );

    let raw_failures = report
        .assertions
        .iter()
        .filter(|assertion| assertion.outcome == TapAssertionOutcome::Fail)
        .count();
    assert_eq!(raw_failures, 3);
    assert_eq!(report.failed_count(), 1);
    assert_eq!(report.passed_count(), 1);
    assert_eq!(report.todo_count(), 2);
    assert_eq!(report.skipped_count(), 2);
    assert_eq!(report.unknown_count(), 0);
    assert!(report.diagnostics.is_empty());
    assert!(!report.is_success());
}

#[test]
fn malformed_known_records_and_unknown_records_keep_distinct_evidence() {
    let report = parse_tap(
        "TAP version thirteen\n\
         ok 1 - recognized\n\
         1..oops\n\
         future record: opaque\n\
         1..1\n",
    );

    assert_eq!(report.version, None);
    assert_eq!(report.plan.as_ref().map(|plan| (plan.end, plan.line)), Some((1, 5)));
    assert_eq!(report.assertions.first().map(|assertion| assertion.line), Some(2));
    assert_eq!(
        report.diagnostics,
        vec![
            "line 1: invalid TAP version declaration".to_owned(),
            "line 3: invalid TAP plan".to_owned(),
        ]
    );
    assert_eq!(report.raw_lines, vec!["future record: opaque"]);
    assert!(report.is_success());
}
