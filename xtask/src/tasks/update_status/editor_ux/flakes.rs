//! UX flake-ledger loading and known-blocker rendering for the editor UX receipt.

use std::fs;
use std::path::Path;

use color_eyre::eyre::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct UxFlakeLedger {
    entries: Vec<UxFlakeEntry>,
}

#[derive(Debug, Deserialize)]
struct UxFlakeEntry {
    test: String,
    state: String,
    #[serde(default)]
    disposition: Option<String>,
    #[serde(default)]
    failure_class: Option<String>,
    #[serde(default)]
    component: Option<String>,
    #[serde(default)]
    route: Option<String>,
    #[serde(default)]
    issue: Option<u64>,
    #[serde(default)]
    owner: Option<String>,
}

fn load_flake_ledger(root: &Path) -> Result<UxFlakeLedger> {
    let path = root.join(".ci/ux-flakes.json");
    if !path.exists() {
        return Ok(UxFlakeLedger { entries: Vec::new() });
    }
    let raw = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("parsing {}", path.display()))
}

pub(super) fn load_active_known_blockers(root: &Path) -> Result<Vec<serde_json::Value>> {
    let ledger = load_flake_ledger(root)?;
    let blockers = ledger
        .entries
        .into_iter()
        .filter(|entry| entry.state == "active")
        .map(|entry| {
            let route =
                entry.route.or_else(|| route_for_failure_class(entry.failure_class.as_deref()));
            serde_json::json!({
                "test_name": entry.test,
                "state": entry.state,
                "disposition": entry.disposition,
                "failure_class": entry.failure_class,
                "component": entry.component,
                "route": route,
                "issue": entry.issue,
                "owner": entry.owner,
            })
        })
        .collect();
    Ok(blockers)
}

fn route_for_failure_class(failure_class: Option<&str>) -> Option<String> {
    let route = match failure_class? {
        "provider_regression" => "provider_fix",
        "server_crash" => "crash_fix",
        "timeout" => "timeout_triage",
        "infra" => "ci_investigation",
        "matrix_drift" => "fixture_update",
        "baseline_drift" => "baseline_update",
        "test_race" | "new_test_bug" => "test_fix",
        // Sentinel: the literal class "unknown" is a recorded value in the
        // ledger and routes to generic triage. Any *other* unrecognized class
        // falls through to `None` below, so callers can distinguish "the ledger
        // says it does not know" from "this class is not in the routing table".
        "unknown" => "triage",
        _ => return None,
    };
    Some(route.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use color_eyre::eyre::eyre;

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

        // Ten rows carry terminal dispositions with proof runs.
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

        // The FindBin row stays active proof debt: its replacement tolerates
        // the consumer divergence it claims to guard, so it must keep routing
        // to #10015 instead of masquerading as resolved.
        let findbin = scenario_14_entries
            .iter()
            .find(|entry| entry.test.ends_with("scenario_14_findbin_relative"))
            .ok_or_else(|| eyre!("FindBin row missing from ledger"))?;
        assert_eq!(findbin.state, "active");
        assert_eq!(findbin.disposition.as_deref(), Some("not_proven"));
        assert_eq!(findbin.issue, Some(10015));
        assert!(findbin.owner.is_some(), "active FindBin row must name an owner");

        // Scorecard rendering: only the unproven row surfaces as a current
        // blocker; resolved history must not.
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

    #[test]
    fn known_failure_classes_resolve_to_routes() {
        assert_eq!(route_for_failure_class(Some("timeout")).as_deref(), Some("timeout_triage"));
        assert_eq!(route_for_failure_class(Some("test_race")).as_deref(), Some("test_fix"));
        // The "unknown" sentinel is a known class, not an absent one: it must
        // resolve to a route rather than falling through to the wildcard.
        assert_eq!(route_for_failure_class(Some("unknown")).as_deref(), Some("triage"));
    }

    #[test]
    fn absent_or_unrecognized_failure_class_has_no_route() {
        assert_eq!(route_for_failure_class(None), None);
        assert_eq!(route_for_failure_class(Some("not_a_known_class")), None);
        // Guards the sentinel's boundary: near-misses must not reach "triage".
        assert_eq!(route_for_failure_class(Some("Unknown")), None);
        assert_eq!(route_for_failure_class(Some("")), None);
    }
}
