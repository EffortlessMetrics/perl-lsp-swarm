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

#[test]
fn scenario_14_rows_split_terminal_dispositions_from_active_proof_debt() -> Result<()> {
    let root = crate::utils::project_root()?;
    let ledger = load_flake_ledger(&root)?;
    let scenario_14_entries: Vec<&UxFlakeEntry> = ledger
        .entries
        .iter()
        .filter(|entry| entry.test.starts_with("ux_scenario_14_inc_conformance::"))
        .collect();

    assert_eq!(scenario_14_entries.len(), 11, "expected the 11 historical Scenario 14 rows");
    for entry in &scenario_14_entries {
        assert_ne!(entry.issue, Some(7570), "{} must not route to unrelated #7570", entry.test);
    }

    let resolved: Vec<&&UxFlakeEntry> =
        scenario_14_entries.iter().filter(|entry| entry.state == "resolved").collect();
    assert_eq!(resolved.len(), 10, "ten Scenario 14 rows must stay terminally resolved");
    for entry in resolved {
        assert!(
            matches!(
                entry.disposition.as_deref(),
                Some("stabilized" | "resolved_by_intent" | "folded" | "not_proven")
            ),
            "{} must carry a terminal disposition, got {:?}",
            entry.test,
            entry.disposition
        );
    }

    let findbin = scenario_14_entries
        .iter()
        .find(|entry| entry.test.ends_with("scenario_14_findbin_relative"))
        .ok_or_else(|| eyre!("FindBin row missing from ledger"))?;
    assert_eq!(findbin.state, "active");
    assert_eq!(findbin.disposition.as_deref(), Some("not_proven"));
    assert_eq!(findbin.issue, Some(10015));
    assert!(findbin.owner.is_some(), "active FindBin row must name an owner");

    let blockers = load_active_known_blockers(&root)?;
    let blocker_names: std::collections::BTreeSet<&str> =
        blockers.iter().filter_map(|entry| entry["test_name"].as_str()).collect();
    assert!(
        blocker_names.contains("ux_scenario_14_inc_conformance::scenario_14_findbin_relative"),
        "the unproven FindBin row must render as a current scorecard blocker"
    );
    for entry in &scenario_14_entries {
        if entry.test.ends_with("scenario_14_findbin_relative") {
            continue;
        }
        assert!(
            !blocker_names.contains(entry.test.as_str()),
            "resolved row {} must not render as a current scorecard blocker",
            entry.test
        );
    }
    Ok(())
}
