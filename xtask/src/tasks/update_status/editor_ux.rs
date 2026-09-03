//! Editor UX scenario counting and receipt generation for quality.md.
//!
//! This module owns the scenario and scorecard loaders and the top-level
//! `generate_editor_ux_receipt`. Receipt rendering is split across two
//! children: `flakes` holds the flake-ledger types, `load_flake_ledger`,
//! `load_active_known_blockers` and `route_for_failure_class`; `rows` holds
//! `top_line_metric_rows`, `workflow_rows` and their row builders.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use color_eyre::eyre::{Context, Result};
use serde::Deserialize;

use crate::tasks::metrics::lsp_stats::MeasuredEditorUxScorecard;

mod flakes;
mod rows;

use flakes::load_active_known_blockers;
use rows::{top_line_metric_rows, workflow_rows};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct EditorUxFixtureMatrix {
    workflows: Vec<EditorUxWorkflow>,
}

#[derive(Debug, Deserialize)]
struct EditorUxWorkflow {
    // ci_tier is present in the JSON but not used for signal counting;
    // the fixture integrity test enforces that tags, not tier, are authoritative.
    #[allow(dead_code)]
    ci_tier: String,
    confidence_signals: Vec<String>,
}

// ---------------------------------------------------------------------------
// Collectors
// ---------------------------------------------------------------------------

pub(super) fn collect_ux_scenario_files(root: &Path) -> Vec<String> {
    let tests_dir = root.join("crates/perl-lsp-ux-tests/tests");
    let Ok(entries) = fs::read_dir(tests_dir) else {
        return Vec::new();
    };

    let mut files: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.starts_with("ux_scenario_") && name.ends_with(".rs"))
        .map(|name| format!("crates/perl-lsp-ux-tests/tests/{name}"))
        .collect();
    files.sort();
    files
}

pub(super) fn count_ux_scenarios(root: &Path) -> usize {
    collect_ux_scenario_files(root).len()
}

pub(super) fn collect_editor_ux_confidence_counts(root: &Path) -> Result<BTreeMap<String, usize>> {
    let matrix_path = root.join("crates/perl-lsp-ux-tests/fixtures/editor_ux_fixture_matrix.json");
    let matrix_raw = fs::read_to_string(&matrix_path)
        .with_context(|| format!("reading {}", matrix_path.display()))?;
    let matrix: EditorUxFixtureMatrix = serde_json::from_str(&matrix_raw)
        .with_context(|| format!("parsing {}", matrix_path.display()))?;

    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for workflow in &matrix.workflows {
        for signal in &workflow.confidence_signals {
            *counts.entry(signal.clone()).or_insert(0) += 1;
        }
    }

    for signal in
        &["first_five_minutes_harness", "manual_editor_smoke", "issue_burndown_regression_guard"]
    {
        counts.entry((*signal).to_string()).or_insert(0);
    }
    Ok(counts)
}

// ---------------------------------------------------------------------------
// Generators
// ---------------------------------------------------------------------------

pub(super) fn generate_editor_ux_receipt(root: &Path) -> Result<String> {
    let scenario_files = collect_ux_scenario_files(root);
    let scenario_count = scenario_files.len();
    let confidence_counts = collect_editor_ux_confidence_counts(root)?;
    let measured_scorecard = load_measured_scorecard(root)?;
    let known_blockers = load_active_known_blockers(root)?;

    let receipt = serde_json::json!({
        "schema_version": 1,
        "receipt_kind": if measured_scorecard.is_some() { "measured_status" } else { "planning_scaffold" },
        "scorecard": "editor_ux",
        "harness": {
            "crate": "crates/perl-lsp-ux-tests",
            "scenario_count": scenario_count,
            "scenario_files": scenario_files,
        },
        "top_line_metrics": top_line_metric_rows(measured_scorecard.as_ref()),
        "workflow_results": workflow_rows(measured_scorecard.as_ref()),
        "known_blockers": known_blockers,
        "confidence_signals": [
            {
                "name": "manual_editor_smoke",
                "state": "tracked",
                "owner": "perl-lsp-ux-tests",
                "workflow_count": confidence_counts.get("manual_editor_smoke").copied().unwrap_or(0),
            },
            {
                "name": "first_five_minutes_harness",
                "state": "tracked",
                "owner": "perl-lsp-ux-tests",
                "workflow_count": confidence_counts.get("first_five_minutes_harness").copied().unwrap_or(0),
            },
            {
                "name": "issue_burndown_regression_guard",
                "state": "tracked",
                "owner": "perl-lsp-ux-tests",
                "workflow_count": confidence_counts
                    .get("issue_burndown_regression_guard")
                    .copied()
                    .unwrap_or(0),
            },
        ],
        "integration_points": {
            "ci_lane": "just ux-tests",
            "release_lane": "just ux-tests-full",
            "status_update": "cargo xtask update-status --only quality",
            "quality_surface": "docs/project/status/quality.md",
        },
    });

    serde_json::to_string_pretty(&receipt).context("serializing editor UX receipt")
}

fn load_measured_scorecard(root: &Path) -> Result<Option<MeasuredEditorUxScorecard>> {
    let path = root.join(".ci/metrics/editor_ux.json");
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let scorecard: MeasuredEditorUxScorecard =
        serde_json::from_str(&raw).with_context(|| format!("parsing {}", path.display()))?;
    Ok(Some(scorecard))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use color_eyre::eyre::eyre;

    #[test]
    fn test_editor_ux_receipt_shape() -> Result<()> {
        let root = crate::utils::project_root()?;
        let receipt_raw = generate_editor_ux_receipt(&root)?;
        let receipt: serde_json::Value = serde_json::from_str(&receipt_raw)?;
        assert_eq!(receipt["schema_version"], 1);
        assert!(
            receipt["receipt_kind"] == "planning_scaffold"
                || receipt["receipt_kind"] == "measured_status"
        );
        assert_eq!(receipt["scorecard"], "editor_ux");
        assert_eq!(receipt["harness"]["crate"], "crates/perl-lsp-ux-tests");
        assert_eq!(
            receipt["harness"]["scenario_count"].as_u64(),
            Some(count_ux_scenarios(&root) as u64)
        );
        let top_line_names = receipt["top_line_metrics"]
            .as_array()
            .ok_or_else(|| eyre!("top_line_metrics must be an array"))?
            .iter()
            .map(|row| row["name"].as_str().ok_or_else(|| eyre!("top_line metric name missing")))
            .collect::<Result<std::collections::BTreeSet<_>>>()?;
        assert_eq!(
            top_line_names,
            std::collections::BTreeSet::from([
                "workflow_pass_rate",
                "workflow_stability_rate",
                "p95_time_to_first_useful_result_ms",
            ])
        );
        assert_eq!(receipt["integration_points"]["ci_lane"], "just ux-tests");
        assert!(receipt["workflow_results"].is_array());
        assert!(receipt["known_blockers"].is_array());
        let confidence_signals = receipt["confidence_signals"]
            .as_array()
            .ok_or_else(|| eyre!("confidence_signals must be an array"))?;
        let confidence_names: std::collections::BTreeSet<&str> = confidence_signals
            .iter()
            .map(|row| row["name"].as_str().ok_or_else(|| eyre!("confidence signal name missing")))
            .collect::<Result<_>>()?;
        assert_eq!(
            confidence_names,
            std::collections::BTreeSet::from([
                "manual_editor_smoke",
                "first_five_minutes_harness",
                "issue_burndown_regression_guard",
            ])
        );
        let live_counts = collect_editor_ux_confidence_counts(&root)?;
        for row in confidence_signals {
            let name = row["name"].as_str().ok_or_else(|| eyre!("name missing"))?;
            let receipt_count = row["workflow_count"]
                .as_u64()
                .ok_or_else(|| eyre!("workflow_count missing for {name}"))?;
            let live_count = *live_counts.get(name).unwrap_or(&0) as u64;
            assert_eq!(
                receipt_count, live_count,
                "receipt workflow_count for `{name}` ({receipt_count}) diverges from \
                 live fixture count ({live_count}) — re-run `cargo xtask update-status` to sync"
            );
            assert!(receipt_count > 0, "signal `{name}` has zero workflow coverage");
        }
        Ok(())
    }
}
