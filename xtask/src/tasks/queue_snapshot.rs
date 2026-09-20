use crate::utils::project_root;
use color_eyre::eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueSnapshot {
    pub snapshot_id: String,
    pub captured_at: String,
    pub repository: String,
    pub default_branch: String,
    /// Historical field name retained for schema compatibility. The value is
    /// the captured SHA of the repository's current default integration branch.
    pub master_sha: String,
    #[serde(default)]
    pub ruleset_summary: serde_json::Value,
    pub prs: Vec<PullRequestSnapshot>,
    #[serde(default)]
    pub buckets: DerivedBuckets,
    #[serde(default)]
    pub leases: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequestSnapshot {
    pub number: u64,
    pub title: String,
    pub head_sha: String,
    pub base_sha: String,
    pub is_draft: bool,
    #[serde(default)]
    pub mergeability: Option<String>,
    pub merge_state_status: Option<String>,
    pub labels: Vec<String>,
    pub status_check_rollup: Vec<StatusCheck>,
    pub updated_at: String,
    pub author: String,
    pub review_decision: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusCheck {
    pub name: String,
    pub state: String,
}

/// Derived observations for queue navigation.
///
/// These buckets are intentionally not mutually exclusive: CI state,
/// mergeability, and draft state are independent observations.
/// They are navigation/projected state, not merge authorization.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct DerivedBuckets {
    pub mergeable_clean: Vec<u64>,
    pub ci_green: Vec<u64>,
    pub needs_ci_fix: Vec<u64>,
    /// GitHub reports an actual textual conflict (`DIRTY`/`CONFLICTING`).
    pub conflicting: Vec<u64>,
    /// GitHub did not establish mergeability (`UNKNOWN` or missing state).
    pub unknown_not_proven: Vec<u64>,
    /// Checks are neither terminal-failing nor all non-blocking, while
    /// mergeability is known and non-conflicting (for example `UNSTABLE`).
    pub pending_or_unclassified: Vec<u64>,
    pub draft: Vec<u64>,
}

pub fn run_snapshot(out: PathBuf, fixture: Option<PathBuf>) -> Result<()> {
    let snapshot = if let Some(fixture_path) = fixture {
        let fixture_text = fs::read_to_string(&fixture_path)
            .with_context(|| format!("failed to read fixture {}", fixture_path.display()))?;
        serde_json::from_str::<QueueSnapshot>(&fixture_text)
            .with_context(|| format!("failed to parse fixture {}", fixture_path.display()))?
    } else {
        snapshot_from_gh_cli()?
    };

    let mut with_buckets = snapshot;
    with_buckets.buckets = derive_buckets(&with_buckets.prs);

    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create output directory {}", parent.display()))?;
    }

    let payload = serde_json::to_string_pretty(&with_buckets)?;
    fs::write(&out, payload).with_context(|| format!("failed to write {}", out.display()))?;
    println!("wrote queue snapshot to {}", out.display());
    Ok(())
}

fn snapshot_from_gh_cli() -> Result<QueueSnapshot> {
    let root = project_root()?;

    // Fetch repository name (nameWithOwner).
    let repo_output = Command::new("gh")
        .current_dir(&root)
        .args(["repo", "view", "--json", "nameWithOwner", "--jq", ".nameWithOwner"])
        .output()
        .context("failed to execute gh repo view")?;
    let repository = if repo_output.status.success() {
        String::from_utf8_lossy(&repo_output.stdout).trim().to_string()
    } else {
        "unknown".to_string()
    };

    // Fetch current main SHA via git. The serialized `master_sha` field name is
    // retained for backward compatibility with existing snapshot consumers.
    let sha_output = Command::new("git")
        .current_dir(&root)
        .args(["rev-parse", "origin/main"])
        .output()
        .context("failed to execute git rev-parse")?;
    let master_sha = if sha_output.status.success() {
        String::from_utf8_lossy(&sha_output.stdout).trim().to_string()
    } else {
        "unknown".to_string()
    };

    let output = Command::new("gh")
        .current_dir(&root)
        .args([
            "pr",
            "list",
            "--state",
            "open",
            "--limit",
            "200",
            "--json",
            "number,title,isDraft,headRefOid,baseRefOid,mergeable,mergeStateStatus,labels,statusCheckRollup,updatedAt,author,reviewDecision",
        ])
        .output()
        .context("failed to execute gh pr list")?;

    if !output.status.success() {
        bail!("gh pr list failed");
    }

    let raw = String::from_utf8_lossy(&output.stdout);
    let prs_json: Vec<serde_json::Value> = serde_json::from_str(&raw)?;
    let prs = prs_json
        .into_iter()
        .map(|pr| PullRequestSnapshot {
            number: pr.get("number").and_then(serde_json::Value::as_u64).unwrap_or_default(),
            title: pr
                .get("title")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string(),
            head_sha: pr
                .get("headRefOid")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string(),
            base_sha: pr
                .get("baseRefOid")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string(),
            is_draft: pr.get("isDraft").and_then(serde_json::Value::as_bool).unwrap_or(false),
            mergeability: pr
                .get("mergeable")
                .and_then(serde_json::Value::as_str)
                .map(ToString::to_string),
            merge_state_status: pr
                .get("mergeStateStatus")
                .and_then(serde_json::Value::as_str)
                .map(ToString::to_string),
            labels: pr
                .get("labels")
                .and_then(serde_json::Value::as_array)
                .map(|labels| {
                    labels
                        .iter()
                        .filter_map(|label| {
                            label
                                .get("name")
                                .and_then(serde_json::Value::as_str)
                                .map(ToString::to_string)
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
            status_check_rollup: pr
                .get("statusCheckRollup")
                .and_then(serde_json::Value::as_array)
                .map(|checks| {
                    checks
                        .iter()
                        .map(|check| StatusCheck {
                            name: check
                                .get("name")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or("unknown")
                                .to_string(),
                            state: check
                                .get("conclusion")
                                .or_else(|| check.get("state"))
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or("UNKNOWN")
                                .to_string(),
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
            updated_at: pr
                .get("updatedAt")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string(),
            author: pr
                .get("author")
                .and_then(|v| v.get("login"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown")
                .to_string(),
            review_decision: pr
                .get("reviewDecision")
                .and_then(serde_json::Value::as_str)
                .map(ToString::to_string),
        })
        .collect::<Vec<_>>();

    let now = chrono::Utc::now();
    let snapshot_id = format!("gh-snapshot-{}", now.to_rfc3339());
    Ok(QueueSnapshot {
        snapshot_id,
        captured_at: now.to_rfc3339(),
        repository,
        default_branch: "main".to_string(),
        master_sha,
        ruleset_summary: serde_json::json!({"source":"gh-cli"}),
        buckets: derive_buckets(&prs),
        prs,
        leases: Vec::new(),
    })
}

fn is_terminal_check_failure(state: &str) -> bool {
    matches!(
        state.to_ascii_uppercase().as_str(),
        "ACTION_REQUIRED"
            | "CANCELLED"
            | "ERROR"
            | "FAILURE"
            | "STALE"
            | "STARTUP_FAILURE"
            | "TIMED_OUT"
    )
}

pub fn derive_buckets(prs: &[PullRequestSnapshot]) -> DerivedBuckets {
    let mut buckets = DerivedBuckets::default();
    for pr in prs {
        let has_failing =
            pr.status_check_rollup.iter().any(|check| is_terminal_check_failure(&check.state));
        let all_green = !pr.status_check_rollup.is_empty()
            && pr.status_check_rollup.iter().all(|check| {
                check.state.eq_ignore_ascii_case("success")
                    || check.state.eq_ignore_ascii_case("neutral")
                    || check.state.eq_ignore_ascii_case("skipped")
            });
        let merge_state = pr.merge_state_status.as_deref().map(str::to_ascii_uppercase);
        let mergeability = pr.mergeability.as_deref().map(str::to_ascii_uppercase);
        // Prefer GitHub's native `mergeable` observation when it is present.
        // `mergeStateStatus` is the compatibility fallback for older or
        // incomplete snapshots; it must not override an explicit native state.
        let is_conflicting = match mergeability.as_deref() {
            Some("CONFLICTING") => true,
            Some(_) => false,
            None => matches!(merge_state.as_deref(), Some("DIRTY") | Some("CONFLICTING")),
        };
        let is_unknown = match mergeability.as_deref() {
            Some("UNKNOWN") => true,
            Some(_) => false,
            None => matches!(merge_state.as_deref(), None | Some("UNKNOWN")),
        };

        if pr.is_draft {
            buckets.draft.push(pr.number);
        }
        if mergeability.as_deref() == Some("MERGEABLE") && merge_state.as_deref() == Some("CLEAN") {
            buckets.mergeable_clean.push(pr.number);
        }

        if is_conflicting {
            buckets.conflicting.push(pr.number);
        }
        if is_unknown {
            buckets.unknown_not_proven.push(pr.number);
        }

        if has_failing {
            buckets.needs_ci_fix.push(pr.number);
        } else if all_green {
            buckets.ci_green.push(pr.number);
        } else if !is_conflicting && !is_unknown {
            buckets.pending_or_unclassified.push(pr.number);
        }
    }
    buckets
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_pr(number: u64, labels: Vec<&str>, checks: Vec<(&str, &str)>) -> PullRequestSnapshot {
        make_pr_with_state(number, "CLEAN", labels, checks)
    }

    fn make_pr_with_state(
        number: u64,
        merge_state: &str,
        labels: Vec<&str>,
        checks: Vec<(&str, &str)>,
    ) -> PullRequestSnapshot {
        PullRequestSnapshot {
            number,
            title: format!("PR {number}"),
            head_sha: "abc".to_string(),
            base_sha: "def".to_string(),
            is_draft: false,
            mergeability: Some(
                match merge_state {
                    "DIRTY" | "CONFLICTING" => "CONFLICTING",
                    "UNKNOWN" => "UNKNOWN",
                    _ => "MERGEABLE",
                }
                .to_string(),
            ),
            merge_state_status: Some(merge_state.to_string()),
            labels: labels.into_iter().map(ToString::to_string).collect(),
            status_check_rollup: checks
                .into_iter()
                .map(|(name, state)| StatusCheck {
                    name: name.to_string(),
                    state: state.to_string(),
                })
                .collect(),
            updated_at: "2026-04-26T00:00:00Z".to_string(),
            author: "bot".to_string(),
            review_decision: None,
        }
    }

    #[test]
    fn cancelled_check_routes_to_needs_ci_fix() {
        let prs = vec![make_pr(1, vec![], vec![("ci", "CANCELLED")])];
        let buckets = derive_buckets(&prs);
        assert!(buckets.needs_ci_fix.contains(&1), "CANCELLED must route to needs_ci_fix");
        assert!(!buckets.ci_green.contains(&1));
    }

    #[test]
    fn lifecycle_labels_do_not_create_merge_bucket() {
        let prs = vec![make_pr(
            15,
            vec!["merge-ready", "needs-builder-fix", "needs-diff-fix", "diff-audited"],
            vec![("ci", "SUCCESS")],
        )];
        let buckets = derive_buckets(&prs);
        assert!(
            buckets.mergeable_clean.contains(&15),
            "lifecycle labels must not create a merge bucket; native clean state should do so"
        );
    }

    #[test]
    fn timed_out_check_routes_to_needs_ci_fix() {
        let prs = vec![make_pr(2, vec![], vec![("ci", "TIMED_OUT")])];
        let buckets = derive_buckets(&prs);
        assert!(buckets.needs_ci_fix.contains(&2), "TIMED_OUT must route to needs_ci_fix");
    }

    #[test]
    fn action_required_routes_to_needs_ci_fix() {
        let prs = vec![make_pr(3, vec![], vec![("ci", "ACTION_REQUIRED")])];
        let buckets = derive_buckets(&prs);
        assert!(buckets.needs_ci_fix.contains(&3), "ACTION_REQUIRED must route to needs_ci_fix");
    }

    #[test]
    fn success_routes_to_ci_green() {
        let prs = vec![make_pr(4, vec![], vec![("ci", "success")])];
        let buckets = derive_buckets(&prs);
        assert!(buckets.ci_green.contains(&4));
        assert!(!buckets.needs_ci_fix.contains(&4));
    }

    #[test]
    fn failure_routes_to_needs_ci_fix() {
        let prs = vec![make_pr(5, vec![], vec![("ci", "failure")])];
        let buckets = derive_buckets(&prs);
        assert!(buckets.needs_ci_fix.contains(&5));
    }

    #[test]
    fn every_terminal_github_failure_routes_to_needs_ci_fix() {
        for (number, state) in [(12, "ERROR"), (13, "STARTUP_FAILURE"), (14, "STALE")] {
            let buckets = derive_buckets(&[make_pr(number, vec![], vec![("ci", state)])]);
            assert!(buckets.needs_ci_fix.contains(&number), "{state} must route to needs_ci_fix");
            assert!(
                !buckets.pending_or_unclassified.contains(&number),
                "{state} must not look pending"
            );
        }
    }

    #[test]
    fn dirty_routes_to_conflicting_not_unknown() {
        let prs = vec![make_pr_with_state(6, "DIRTY", vec![], vec![("ci", "IN_PROGRESS")])];
        let buckets = derive_buckets(&prs);
        assert!(buckets.conflicting.contains(&6));
        assert!(!buckets.unknown_not_proven.contains(&6));
        assert!(!buckets.pending_or_unclassified.contains(&6));
    }

    #[test]
    fn conflicting_alias_routes_to_conflicting() {
        let prs = vec![make_pr_with_state(7, "CONFLICTING", vec![], vec![("ci", "IN_PROGRESS")])];
        let buckets = derive_buckets(&prs);
        assert!(buckets.conflicting.contains(&7));
    }

    #[test]
    fn unknown_routes_to_not_proven_not_conflicting() {
        let prs = vec![make_pr_with_state(8, "UNKNOWN", vec![], vec![("ci", "IN_PROGRESS")])];
        let buckets = derive_buckets(&prs);
        assert!(buckets.unknown_not_proven.contains(&8));
        assert!(!buckets.conflicting.contains(&8));
        assert!(!buckets.pending_or_unclassified.contains(&8));
    }

    #[test]
    fn native_mergeability_overrides_stale_merge_state() {
        let mut conflicting = make_pr_with_state(16, "BEHIND", vec![], vec![("ci", "IN_PROGRESS")]);
        conflicting.mergeability = Some("CONFLICTING".to_string());
        let mut unknown = make_pr_with_state(17, "CLEAN", vec![], vec![("ci", "IN_PROGRESS")]);
        unknown.mergeability = Some("UNKNOWN".to_string());

        let buckets = derive_buckets(&[conflicting, unknown]);
        assert!(
            buckets.conflicting.contains(&16),
            "native CONFLICTING must win over stale BEHIND merge_state_status"
        );
        assert!(
            !buckets.unknown_not_proven.contains(&16),
            "native CONFLICTING must not be routed to unknown_not_proven"
        );
        assert!(
            buckets.unknown_not_proven.contains(&17),
            "native UNKNOWN must route to unknown_not_proven"
        );
        assert!(
            !buckets.pending_or_unclassified.contains(&17),
            "native UNKNOWN must not be routed to pending_or_unclassified"
        );
    }

    #[test]
    fn merge_state_is_fallback_when_native_mergeability_is_missing() {
        let mut pr = make_pr_with_state(18, "DIRTY", vec![], vec![("ci", "IN_PROGRESS")]);
        pr.mergeability = None;
        let buckets = derive_buckets(&[pr]);
        assert!(
            buckets.conflicting.contains(&18),
            "DIRTY fallback must route to conflicting when native mergeability is absent"
        );
        assert!(
            !buckets.unknown_not_proven.contains(&18),
            "DIRTY fallback must not route to unknown_not_proven"
        );
    }

    #[test]
    fn missing_merge_state_routes_to_unknown_not_proven() {
        let mut pr = make_pr(9, vec![], vec![("ci", "IN_PROGRESS")]);
        pr.merge_state_status = None;
        pr.mergeability = None;
        let buckets = derive_buckets(&[pr]);
        assert!(buckets.unknown_not_proven.contains(&9));
    }

    #[test]
    fn clean_pending_routes_to_pending_or_unclassified() {
        let prs = vec![make_pr_with_state(10, "CLEAN", vec![], vec![("ci", "IN_PROGRESS")])];
        let buckets = derive_buckets(&prs);
        assert!(buckets.pending_or_unclassified.contains(&10));
        assert!(!buckets.conflicting.contains(&10));
        assert!(!buckets.unknown_not_proven.contains(&10));
    }

    #[test]
    fn conflict_and_ci_failure_remain_visible_together() {
        let prs = vec![make_pr_with_state(11, "DIRTY", vec![], vec![("ci", "FAILURE")])];
        let buckets = derive_buckets(&prs);
        assert!(buckets.conflicting.contains(&11));
        assert!(buckets.needs_ci_fix.contains(&11));
    }
}
