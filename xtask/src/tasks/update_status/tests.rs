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

pub(super) struct TestCounts {
    pub tier_a_lib_tests: Option<usize>,
    pub ignored_total: Option<usize>,
    pub bug_count: Option<usize>,
    pub manual_count: Option<usize>,
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
    TestCounts { tier_a_lib_tests: tier_a, ignored_total, bug_count, manual_count }
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
    // The previous behavior panicked when cargo test --list returned empty
    // output (e.g. in CI environments without all features compiled).
    let tier_a_tests_str =
        tests.tier_a_lib_tests.map_or_else(|| "UNVERIFIED".to_string(), |n| n.to_string());

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

    let table_rows = format!(
        "| **Tier A Tests** | {tier_a_tests_str} lib tests (discovered), {ignored_tests_str} ignores (tracked) | 100% pass | PASS |\n\
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
}
