//! Quality subsystem status generator.
//!
//! Owns per-crate mutation and quality rendering; consumes the shared lib-test inventory
//! to `editor_ux` and flaky-test tracking to `flaky`.

// LazyLock<Regex> initializers use .expect() for known-good patterns — permitted by coding standards.
#![allow(clippy::expect_used)]

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::LazyLock;

use color_eyre::eyre::Result;
use regex::Regex;

use super::editor_ux::count_ux_scenarios;
use super::flaky::{collect_flaky_test_summary, format_flaky_tests_section};
use super::replace_block;
use super::test_inventory::PerCrateTestCounts;

static DIAGNOSTICS_P50_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\|\s*diagnostics\s*\|\s*([0-9]+(?:\.[0-9]+)?)\s*\|")
        .expect("diagnostics-p50 regex is valid")
});

// ---------------------------------------------------------------------------
// Metric collectors
// ---------------------------------------------------------------------------

/// Read `mutants.out/mutants.json` and group mutations by crate package name.
pub(super) fn collect_per_crate_mutation(root: &Path) -> BTreeMap<String, usize> {
    let path = root.join("mutants.out").join("mutants.json");
    let Ok(raw) = fs::read_to_string(&path) else {
        return BTreeMap::new();
    };
    let Ok(entries) = serde_json::from_str::<Vec<serde_json::Value>>(&raw) else {
        return BTreeMap::new();
    };
    let mut by_crate: BTreeMap<String, usize> = BTreeMap::new();
    for entry in entries {
        if let Some(pkg) = entry.get("package").and_then(|v| v.as_str())
            && !pkg.trim().is_empty()
        {
            *by_crate.entry(pkg.to_string()).or_default() += 1;
        }
    }
    by_crate
}

/// Read `docs/project/status/editor_ux.md` and return the diagnostics p50 latency in ms,
/// or `None` when the receipt file is absent or the table row is not found.
pub(super) fn read_diagnostics_p50_ms(root: &Path) -> Option<f64> {
    let path = root.join("docs/project/status/editor_ux.md");
    let text = fs::read_to_string(&path).ok()?;
    for line in text.lines() {
        if let Some(caps) = DIAGNOSTICS_P50_RE.captures(line) {
            return caps[1].parse::<f64>().ok();
        }
    }
    None
}

/// Read `docs/project/status/parser_performance_scorecard.json` and return the
/// `(incremental_multiple_edits median_ns, incremental_small_edit median_ns)` pair,
/// or `None` when the file is absent or the fields are missing.
pub(super) fn read_incremental_parse_range_ns(root: &Path) -> Option<(u64, u64)> {
    let path = root.join("docs/project/status/parser_performance_scorecard.json");
    let text = fs::read_to_string(&path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let metrics = v.get("metrics")?;
    let small = metrics.get("incremental_small_edit")?.get("median_ns")?.as_u64()?;
    let multi = metrics.get("incremental_multiple_edits")?.get("median_ns")?.as_u64()?;
    // Return (lower, upper) — multiple_edits median is typically lower than small_edit.
    let lower = multi.min(small);
    let upper = multi.max(small);
    Some((lower, upper))
}

/// Build the Quality Metrics bullet string from receipts, falling back to
/// "unmeasured" text for any value not found in the receipt files.
///
/// Sources:
/// - Diagnostics p50: `docs/project/status/editor_ux.md` latency table
/// - Incremental parse: `docs/project/status/parser_performance_scorecard.json`
pub(super) fn format_quality_metrics_bullet(root: &Path) -> String {
    let diag_p50 = read_diagnostics_p50_ms(root);
    let parse_range = read_incremental_parse_range_ns(root);

    match (diag_p50, parse_range) {
        (Some(p50), Some((lower_ns, upper_ns))) => {
            // Round ns to µs (nearest), matching the receipt-backed values in PR #1192:
            // 36733 ns → 37 µs, 73307 ns → 73 µs.
            let lower_us = (lower_ns + 500) / 1_000;
            let upper_us = (upper_ns + 500) / 1_000;
            format!(
                "diagnostics p50 = {p50:.0} ms (receipt: `editor_ux.md`); \
                 incremental parse median = {lower_us}–{upper_us} µs \
                 (receipt: `parser_performance_scorecard.json`)"
            )
        }
        (Some(p50), None) => {
            format!(
                "diagnostics p50 = {p50:.0} ms (receipt: `editor_ux.md`); \
                 incremental parse median = unmeasured"
            )
        }
        (None, Some((lower_ns, upper_ns))) => {
            let lower_us = (lower_ns + 500) / 1_000;
            let upper_us = (upper_ns + 500) / 1_000;
            format!(
                "diagnostics p50 = unmeasured; \
                 incremental parse median = {lower_us}–{upper_us} µs \
                 (receipt: `parser_performance_scorecard.json`)"
            )
        }
        (None, None) => "performance metrics unmeasured — run `just ux-tests` and \
                         `cargo bench -p perl-parser` to populate receipts"
            .to_string(),
    }
}

/// Format a per-crate markdown table showing mutation count and test count.
fn format_crate_quality_table(
    mutation: &BTreeMap<String, usize>,
    tests: &PerCrateTestCounts,
) -> String {
    let mut crates: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for k in mutation.keys() {
        crates.insert(k.as_str());
    }
    for k in tests.by_crate.keys() {
        crates.insert(k.as_str());
    }
    if crates.is_empty() {
        let mut table = "| Crate | Mutants listed | Tests (lib) |\n\
                |-------|---------------|-------------|\n\
                | — | no data yet | no data yet |"
            .to_string();
        if tests.unattributed > 0 {
            table.push_str(&format!(
                "\n\n> Note: {} discovered test(s) had no crate attribution and are excluded from the per-crate table.",
                tests.unattributed
            ));
        }
        return table;
    }
    let mut lines = vec![
        "| Crate | Mutants listed | Tests (lib) |".to_string(),
        "|-------|---------------|-------------|".to_string(),
    ];
    for c in crates {
        let m = mutation.get(c).map_or_else(|| "—".to_string(), |n| n.to_string());
        let t = tests.by_crate.get(c).map_or_else(|| "—".to_string(), |n| n.to_string());
        lines.push(format!("| {c} | {m} | {t} |"));
    }
    let mut table = lines.join("\n");
    if tests.unattributed > 0 {
        table.push_str(&format!(
            "\n\n> Note: {} discovered test(s) had no crate attribution and are excluded from the per-crate table.",
            tests.unattributed
        ));
    }
    table
}

// ---------------------------------------------------------------------------
// Generators
// ---------------------------------------------------------------------------

pub(super) fn generate_quality_status(
    root: &Path,
    original: &str,
    tests_by_crate: &PerCrateTestCounts,
) -> Result<String> {
    let mutation_by_crate = collect_per_crate_mutation(root);
    let ux_scenarios = count_ux_scenarios(root);
    let flaky = collect_flaky_test_summary(root);

    let mutation_note = if mutation_by_crate.is_empty() {
        "mutation data pending first nightly CI run — run `just mutation-subset` locally to populate"
    } else {
        "per-crate data from `mutants.out/mutants.json` (written by nightly CI `cargo mutants` run)"
    };

    let quality_metrics = format_quality_metrics_bullet(root);
    let bullets = format!(
        "- **Quality Metrics**: {quality_metrics}\n\
         - **UX workflow harness**: {ux_scenarios} scenario files in `perl-lsp-ux-tests`; \
           `just ux-tests` runs the default release-confidence lane and `just ux-tests-full` adds \
           the integration-only 10k-line large-file case; confidence signals (manual smoke, \
           first-5-minutes coverage, issue-burndown regression guards) are tracked in \
           `docs/project/status/editor_ux.json`\n\
         - **Mutation testing**: {mutation_note}\n\
         - **Lexer performance scorecard**: `cargo bench -p perl-lexer --bench lexer_benchmarks` writes `benchmarks/results/lexer_scorecard.json` for trend comparisons
\
         - **Production Status**: LSP server public beta (`just ci-gate` passing)"
    );

    let crate_table = format_crate_quality_table(&mutation_by_crate, tests_by_crate);
    let flaky_section = format_flaky_tests_section(&flaky);

    let mut text = original.to_string();
    text = replace_block(
        &text,
        "<!-- BEGIN: QUALITY_METRICS_BULLETS -->",
        "<!-- END: QUALITY_METRICS_BULLETS -->",
        &bullets,
    )?;
    text = replace_block(
        &text,
        "<!-- BEGIN: QUALITY_CRATE_TABLE -->",
        "<!-- END: QUALITY_CRATE_TABLE -->",
        &crate_table,
    )?;
    text = replace_block(
        &text,
        "<!-- BEGIN: FLAKY_TESTS_SUMMARY -->",
        "<!-- END: FLAKY_TESTS_SUMMARY -->",
        &flaky_section,
    )?;
    Ok(text)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests;
