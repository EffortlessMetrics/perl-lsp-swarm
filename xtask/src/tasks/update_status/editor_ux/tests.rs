use super::*;
use color_eyre::eyre::eyre;

#[test]
fn test_editor_ux_receipt_shape() -> Result<()> {
    let root = crate::utils::project_root()?;
    let receipt_raw = generate_editor_ux_receipt(&root)?;
    let receipt: serde_json::Value = serde_json::from_str(&receipt_raw)?;
    assert_eq!(receipt["schema_version"], "editor_ux.v1");
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
fn measured_scorecard_consumer_accepts_current_producer_envelope() -> Result<()> {
    let root = crate::utils::project_root()?;
    let scorecard = load_measured_scorecard(&root)?
        .expect("tracked .ci/metrics/editor_ux.json must exist and be accepted");
    assert_eq!(
        scorecard.schema_version, SUPPORTED_EDITOR_UX_SCHEMA_VERSION,
        "committed producer output must carry the supported schema_version"
    );
    Ok(())
}

/// Write a mutated scorecard envelope into a temp root and return the root.
fn temp_root_with_envelope(envelope: &serde_json::Value) -> Result<tempfile::TempDir> {
    let dir = tempfile::tempdir()?;
    let metrics_dir = dir.path().join(".ci").join("metrics");
    fs::create_dir_all(&metrics_dir)?;
    fs::write(metrics_dir.join("editor_ux.json"), serde_json::to_string_pretty(envelope)?)?;
    Ok(dir)
}

fn tracked_scorecard_envelope() -> Result<serde_json::Value> {
    let root = crate::utils::project_root()?;
    let raw = fs::read_to_string(root.join(".ci").join("metrics").join("editor_ux.json"))?;
    Ok(serde_json::from_str(&raw)?)
}

#[test]
fn measured_scorecard_consumer_refuses_future_schema_version() -> Result<()> {
    let mut envelope = tracked_scorecard_envelope()?;
    envelope["schema_version"] = serde_json::json!(SUPPORTED_EDITOR_UX_SCHEMA_VERSION + 1);
    let dir = temp_root_with_envelope(&envelope)?;

    let err = load_measured_scorecard(dir.path())
        .expect_err("a future schema_version must fail closed, not silently render")
        .to_string();
    assert!(
        err.contains("expected 1") && err.contains("found 2"),
        "error must name expected and found versions, got: {err}"
    );
    Ok(())
}

#[test]
fn measured_scorecard_consumer_refuses_missing_schema_version() -> Result<()> {
    let mut envelope = tracked_scorecard_envelope()?;
    envelope.as_object_mut().expect("scorecard envelope is a JSON object").remove("schema_version");
    let dir = temp_root_with_envelope(&envelope)?;

    let err =
        load_measured_scorecard(dir.path()).expect_err("a missing schema_version must fail closed");
    let chain = format!("{err:?}");
    assert!(
        chain.contains("schema_version"),
        "error chain must identify schema_version as the problem, got: {chain}"
    );
    Ok(())
}

#[test]
fn scenario_14_rows_fully_resolved_with_empty_active_projection() -> Result<()> {
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
    assert_eq!(
        resolved.len(),
        11,
        "every Scenario 14 row must stay terminally resolved — including the FindBin row \
         whose #10015 proof debt was discharged in #17042"
    );
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
        assert!(
            entry.issue.is_none() && entry.owner.is_none(),
            "resolved row {} must not retain active-row bookkeeping",
            entry.test
        );
    }

    // The FindBin proof debt was discharged, not deleted: the row keeps its
    // stabilized disposition and the #17042 evidence binding (re-verified by
    // the quarantine registry contract on the perl-lsp-ux-tests side).
    let findbin = scenario_14_entries
        .iter()
        .find(|entry| entry.test.ends_with("scenario_14_findbin_relative"))
        .ok_or_else(|| eyre!("FindBin row missing from ledger"))?;
    assert_eq!(findbin.state, "resolved");
    assert_eq!(findbin.disposition.as_deref(), Some("stabilized"));

    // With zero active rows the scorecard projects zero blockers; the active
    // rendering path itself stays exercised by the fixture-ledger tests below.
    let blockers = load_active_known_blockers(&root)?;
    assert!(
        blockers.is_empty(),
        "a fully resolved ledger must project zero active scorecard blockers, got {blockers:?}"
    );
    Ok(())
}

/// Write a minimal flake ledger into a temp root and project its active
/// blockers through `load_active_known_blockers`.
fn temp_root_with_ledger(entries: &serde_json::Value) -> Result<tempfile::TempDir> {
    let dir = tempfile::tempdir()?;
    let ci_dir = dir.path().join(".ci");
    fs::create_dir_all(&ci_dir)?;
    fs::write(
        ci_dir.join("ux-flakes.json"),
        serde_json::json!({ "schema_version": 2, "entries": entries }).to_string(),
    )?;
    Ok(dir)
}

/// The active-blocker projection must keep rendering issue-owned rows with
/// their triage route even once the live ledger reaches zero active rows —
/// otherwise the projection path only ever gets exercised by the live
/// registry's current shape and silently rots (issue #9879 A06 residue).
#[test]
fn active_blocker_projection_renders_bounded_rows_from_fixture_ledger() -> Result<()> {
    let active_row = serde_json::json!({
        "test": "ux_scenario_14_inc_conformance::synthetic_active_row",
        "state": "active",
        "disposition": "not_proven",
        "failure_class": "provider_regression",
        "component": "module_resolution",
        "issue": 10015,
        "owner": "@maintainer",
        "expires_after_days": 30
    });
    let resolved_row = serde_json::json!({
        "test": "ux_scenario_14_inc_conformance::synthetic_resolved_row",
        "state": "resolved",
        "disposition": "stabilized"
    });
    let dir = temp_root_with_ledger(&serde_json::json!([active_row, resolved_row]))?;

    let blockers = load_active_known_blockers(dir.path())?;
    assert_eq!(blockers.len(), 1, "only the active row must project as a blocker");
    let blocker = &blockers[0];
    assert_eq!(blocker["test_name"], "ux_scenario_14_inc_conformance::synthetic_active_row");
    assert_eq!(blocker["state"], "active");
    assert_eq!(blocker["disposition"], "not_proven");
    assert_eq!(blocker["failure_class"], "provider_regression");
    assert_eq!(blocker["component"], "module_resolution");
    // The triage route is derived from the failure class when the row carries
    // no explicit route.
    assert_eq!(blocker["route"], "provider_fix");
    assert_eq!(blocker["issue"], 10015);
    assert_eq!(blocker["owner"], "@maintainer");
    // The bounded review window must survive parsing and render with the
    // blocker — a dropped bound would make the projection claim a row is
    // bounded while showing no bound (#9879 review).
    assert_eq!(blocker["expires_after_days"], 30);
    Ok(())
}

/// An explicit per-row `route` must win over the failure-class default, and an
/// empty ledger must project zero blockers rather than erroring.
#[test]
fn active_blocker_projection_honors_explicit_route_and_empty_ledger() -> Result<()> {
    let routed_row = serde_json::json!({
        "test": "ux_scenario_14_inc_conformance::synthetic_routed_row",
        "state": "active",
        "disposition": "still_failing",
        "failure_class": "timeout",
        "route": "custom_triage_lane",
        "issue": 10015,
        "owner": "@maintainer",
        "expires_after_days": 14
    });
    let dir = temp_root_with_ledger(&serde_json::json!([routed_row]))?;
    let blockers = load_active_known_blockers(dir.path())?;
    assert_eq!(blockers.len(), 1);
    assert_eq!(blockers[0]["route"], "custom_triage_lane");
    assert_eq!(blockers[0]["expires_after_days"], 14);

    let empty_dir = temp_root_with_ledger(&serde_json::json!([]))?;
    assert!(
        load_active_known_blockers(empty_dir.path())?.is_empty(),
        "a ledger with zero active rows must project zero blockers"
    );
    Ok(())
}
