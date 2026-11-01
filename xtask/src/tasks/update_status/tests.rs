//! Tests subsystem status generator.
//!
//! Owns test count collection, missing-docs tracking, and tests.md generation.

use std::fs;
use std::path::Path;
use std::time::Duration;

use super::test_inventory::PerCrateTestCounts;
use super::{replace_block, run_cmd};
use color_eyre::eyre::Result;

// ---------------------------------------------------------------------------
// Test counts struct
// ---------------------------------------------------------------------------

/// Per-identity execution result. Mirrors the generated-status contract
/// `Outcome` model so a pass verdict can only be minted from execution
/// evidence, never from `cargo test -- --list` discovery (#14298).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "policy:14298: production count_tests is discovery-only; tests construct variants"
    )
)]
pub(super) enum ExecutionOutcome {
    Pass,
    Fail,
    Skip,
    NotRun,
}

pub(super) struct TestCounts {
    pub tier_a_lib_tests: Option<usize>,
    pub ignored_total: Option<usize>,
    pub bug_count: Option<usize>,
    pub manual_count: Option<usize>,
    /// When `None`, the writer has only discovery inventory and must not
    /// claim PASS. A supplied bundle is execution-backed evidence.
    pub execution_outcomes: Option<Vec<ExecutionOutcome>>,
}

pub(super) fn count_ignored_tracked(root: &Path) -> (Option<usize>, Option<usize>, Option<usize>) {
    let Ok(counts) = super::super::ignored_tests::compute_category_counts(root) else {
        return (None, None, None);
    };

    let ignored_total = counts.values().sum::<usize>();
    let bug_count = counts.get("bug").copied().unwrap_or(0);
    let manual_count = counts.get("manual").copied().unwrap_or(0);

    (Some(ignored_total), Some(bug_count), Some(manual_count))
}

pub(super) fn count_tests(root: &Path, test_inventory: Option<&PerCrateTestCounts>) -> TestCounts {
    let tier_a = test_inventory.map(PerCrateTestCounts::total);
    let (ignored_total, bug_count, manual_count) = count_ignored_tracked(root);
    TestCounts {
        tier_a_lib_tests: tier_a,
        ignored_total,
        bug_count,
        manual_count,
        execution_outcomes: None,
    }
}

/// Result and verdict cells for the Tier A row.
///
/// Discovery-only input is `not executed | UNVERIFIED`. PASS is reserved for
/// a non-empty execution bundle whose every outcome is `Pass`.
fn tier_a_result_and_verdict(
    outcomes: Option<&[ExecutionOutcome]>,
) -> (&'static str, &'static str) {
    let Some(outcomes) = outcomes.filter(|items| !items.is_empty()) else {
        return ("not executed", "UNVERIFIED");
    };
    let mut all_pass = true;
    let mut saw_fail = false;
    for outcome in outcomes {
        match outcome {
            ExecutionOutcome::Pass => {}
            ExecutionOutcome::Fail => {
                all_pass = false;
                saw_fail = true;
            }
            ExecutionOutcome::Skip | ExecutionOutcome::NotRun => all_pass = false,
        }
    }
    if all_pass {
        ("100% pass", "PASS")
    } else if saw_fail {
        ("failed", "FAIL")
    } else {
        ("not executed", "UNVERIFIED")
    }
}

pub(super) fn count_missing_docs_perl_parser(root: &Path) -> Option<usize> {
    let output = run_cmd(
        root,
        &["cargo", "check", "-p", "perl-parser", "--tests", "--message-format=json"],
        Duration::from_mins(5),
    );
    if output.is_empty() {
        return None;
    }

    let mut count: usize = 0;
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let obj: serde_json::Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if obj.get("reason").and_then(|v| v.as_str()) != Some("compiler-message") {
            continue;
        }
        let pkg_id = obj.get("package_id").and_then(|v| v.as_str()).unwrap_or("");
        if !pkg_id.starts_with("perl-parser ") {
            continue;
        }
        let msg = match obj.get("message") {
            Some(m) if m.is_object() => m,
            _ => continue,
        };
        let level = msg.get("level").and_then(|v| v.as_str()).unwrap_or("");
        let code =
            msg.get("code").and_then(|v| v.get("code")).and_then(|v| v.as_str()).unwrap_or("");
        if level == "warning" && code == "missing_docs" {
            count += 1;
        }
    }
    Some(count)
}

pub(super) fn read_missing_docs_baseline(root: &Path) -> Option<usize> {
    let path = root.join("ci/missing_docs_baseline.txt");
    let raw = fs::read_to_string(path).ok()?;
    raw.trim().parse::<usize>().ok()
}

// ---------------------------------------------------------------------------
// Generator
// ---------------------------------------------------------------------------

pub(super) fn generate_tests_status(
    tests: &TestCounts,
    missing_docs_current: Option<usize>,
    missing_docs_baseline: Option<usize>,
    original: &str,
) -> Result<String> {
    // Allow zero or None — render as UNVERIFIED instead of bailing (#5275).
    // Discovery (`cargo test -- --list`) is not execution evidence; PASS is
    // minted only from a supplied all-pass execution bundle (#14298).
    let ignored_tests_str =
        tests.ignored_total.map_or_else(|| "UNVERIFIED".to_string(), |n| n.to_string());

    let (tracked_debt_str, bug_count_str, manual_count_str) =
        match (tests.bug_count, tests.manual_count) {
            (Some(b), Some(m)) => ((b + m).to_string(), b.to_string(), m.to_string()),
            _ => ("UNVERIFIED".to_string(), "UNVERIFIED".to_string(), "UNVERIFIED".to_string()),
        };

    let missing_docs_str =
        missing_docs_current.map_or_else(|| "UNVERIFIED".to_string(), |n| n.to_string());

    let baseline_suffix = match (missing_docs_baseline, missing_docs_current) {
        (Some(bl), Some(_)) => format!(" (baseline {bl})"),
        _ => String::new(),
    };

    let (tier_a_result, tier_a_verdict) =
        tier_a_result_and_verdict(tests.execution_outcomes.as_deref());
    let executed_all_pass = matches!((tier_a_result, tier_a_verdict), ("100% pass", "PASS"));
    let (tier_a_tests_str, source_label) = if executed_all_pass {
        let executed = tests.execution_outcomes.as_ref().map_or(0, Vec::len);
        (executed.to_string(), "executed")
    } else {
        (
            tests.tier_a_lib_tests.map_or_else(|| "UNVERIFIED".to_string(), |n| n.to_string()),
            "discovered",
        )
    };
    let table_rows = format!(
        "| **Tier A Tests** | {tier_a_tests_str} lib tests ({source_label}), {ignored_tests_str} ignores (tracked) | {tier_a_result} | {tier_a_verdict} |\n\
         | **Tracked Test Debt** | {tracked_debt_str} ({bug_count_str} bug, {manual_count_str} manual) | 0 | Near-zero |"
    );

    let bullets_content = format!(
        "- **Test Status**: {tier_a_tests_str} lib tests (Tier A), {ignored_tests_str} ignores tracked ({tracked_debt_str} total tracked debt: {bug_count_str} bug, {manual_count_str} manual)\n\
         - **Docs (perl-parser)**: missing_docs warnings = {missing_docs_str}{baseline_suffix}"
    );

    let mut text = original.to_string();
    text = replace_block(
        &text,
        "<!-- BEGIN: TESTS_TABLE_ROWS -->",
        "<!-- END: TESTS_TABLE_ROWS -->",
        &table_rows,
    )?;
    text = replace_block(
        &text,
        "<!-- BEGIN: TESTS_METRICS_BULLETS -->",
        "<!-- END: TESTS_METRICS_BULLETS -->",
        &bullets_content,
    )?;
    Ok(text)
}

#[cfg(test)]
mod fail_closed_tests {
    use super::*;

    const STATUS_TEMPLATE: &str = "<!-- BEGIN: TESTS_TABLE_ROWS -->\nold\n<!-- END: TESTS_TABLE_ROWS -->\n\
<!-- BEGIN: TESTS_METRICS_BULLETS -->\nold\n<!-- END: TESTS_METRICS_BULLETS -->\n";

    #[test]
    fn count_tests_reuses_the_shared_inventory_total() -> Result<()> {
        let dir = tempfile::tempdir()?;
        fs::create_dir_all(dir.path().join("crates"))?;
        let inventory = PerCrateTestCounts {
            by_crate: std::collections::BTreeMap::from([
                ("perl-parser".to_string(), 3),
                ("perl-lsp-rs".to_string(), 4),
            ]),
            unattributed: 2,
        };

        let counts = count_tests(dir.path(), Some(&inventory));
        assert_eq!(counts.tier_a_lib_tests, Some(9));
        color_eyre::eyre::ensure!(
            counts.execution_outcomes.is_none(),
            "shared inventory discovery must not mint execution evidence"
        );
        Ok(())
    }

    #[test]
    fn generate_tests_status_handles_zero_discovery_gracefully() -> Result<()> {
        // #5275: zero discovery should render as "0" not bail/panic
        let counts = TestCounts {
            tier_a_lib_tests: Some(0),
            ignored_total: Some(0),
            bug_count: Some(0),
            manual_count: Some(0),
            execution_outcomes: None,
        };
        let result = generate_tests_status(&counts, Some(0), Some(0), STATUS_TEMPLATE);
        color_eyre::eyre::ensure!(
            result.is_ok(),
            "zero discovery should produce UNVERIFIED, not error"
        );
        let output = result.unwrap();
        color_eyre::eyre::ensure!(output.contains("0 lib tests"), "should show 0 tests: {output}");
        Ok(())
    }

    #[test]
    fn generate_tests_status_handles_missing_discovery_gracefully() -> Result<()> {
        // #5275: None discovery should render as "UNVERIFIED" not bail/panic
        let counts = TestCounts {
            tier_a_lib_tests: None,
            ignored_total: Some(0),
            bug_count: Some(0),
            manual_count: Some(0),
            execution_outcomes: None,
        };
        let result = generate_tests_status(&counts, Some(0), Some(0), STATUS_TEMPLATE);
        color_eyre::eyre::ensure!(
            result.is_ok(),
            "None discovery should produce UNVERIFIED, not error"
        );
        let output = result.unwrap();
        color_eyre::eyre::ensure!(
            output.contains("UNVERIFIED"),
            "should show UNVERIFIED for tier_a: {output}"
        );
        Ok(())
    }

    fn tier_a_row(output: &str) -> Result<&str> {
        output
            .lines()
            .find(|line| line.contains("Tier A Tests"))
            .ok_or_else(|| color_eyre::eyre::eyre!("Tier A row must be rendered, got: {output}"))
    }

    #[test]
    fn generate_tests_status_discovery_only_does_not_claim_pass() -> Result<()> {
        // #14298: cargo test -- --list is enumeration, not execution. A
        // discovery-only inventory must not mint 100% pass / PASS.
        let counts = TestCounts {
            tier_a_lib_tests: Some(12_269),
            ignored_total: Some(15),
            bug_count: Some(0),
            manual_count: Some(0),
            execution_outcomes: None,
        };
        let output = generate_tests_status(&counts, Some(0), Some(0), STATUS_TEMPLATE)?;
        let row = tier_a_row(&output)?;
        color_eyre::eyre::ensure!(
            !row.contains("100% pass"),
            "discovery-only row must not claim 100% pass: {row}"
        );
        color_eyre::eyre::ensure!(
            !row.contains("PASS"),
            "discovery-only row must not claim PASS: {row}"
        );
        color_eyre::eyre::ensure!(
            row.contains("not executed") && row.contains("UNVERIFIED"),
            "discovery-only row must be execution-honest: {row}"
        );
        Ok(())
    }

    #[test]
    fn generate_tests_status_all_pass_execution_bundle_renders_pass() -> Result<()> {
        let counts = TestCounts {
            tier_a_lib_tests: Some(3),
            ignored_total: Some(0),
            bug_count: Some(0),
            manual_count: Some(0),
            execution_outcomes: Some(vec![
                ExecutionOutcome::Pass,
                ExecutionOutcome::Pass,
                ExecutionOutcome::Pass,
            ]),
        };
        let output = generate_tests_status(&counts, Some(0), Some(0), STATUS_TEMPLATE)?;
        let row = tier_a_row(&output)?;
        color_eyre::eyre::ensure!(
            row.contains("100% pass") && row.contains("PASS"),
            "all-pass execution evidence must render PASS: {row}"
        );
        color_eyre::eyre::ensure!(
            row.contains("3 lib tests (executed)") && !row.contains("12269"),
            "PASS count must bind to the executed population, not discovery: {row}"
        );
        Ok(())
    }

    #[test]
    fn generate_tests_status_fail_or_skip_execution_bundle_does_not_render_pass() -> Result<()> {
        for outcomes in [
            vec![ExecutionOutcome::Pass, ExecutionOutcome::Fail],
            vec![ExecutionOutcome::Pass, ExecutionOutcome::Skip],
            vec![ExecutionOutcome::NotRun],
            vec![ExecutionOutcome::Skip],
        ] {
            let counts = TestCounts {
                tier_a_lib_tests: Some(outcomes.len()),
                ignored_total: Some(0),
                bug_count: Some(0),
                manual_count: Some(0),
                execution_outcomes: Some(outcomes.clone()),
            };
            let output = generate_tests_status(&counts, Some(0), Some(0), STATUS_TEMPLATE)?;
            let row = tier_a_row(&output)?;
            color_eyre::eyre::ensure!(
                !row.contains("100% pass") && !row.contains("PASS"),
                "fail/skip/not-run execution evidence must not render PASS: {row} ({outcomes:?})"
            );
        }
        Ok(())
    }
}
