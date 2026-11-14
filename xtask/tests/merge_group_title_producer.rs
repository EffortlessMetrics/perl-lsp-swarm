//! #18255: the merge queue must receive `validate-title` on the actual G.
//!
//! The queue path resolves the PR named by the merge-group head ref, re-reads
//! its current title and metadata, and applies the same policy as the PR path.
//! It must not use commit association as membership authority, check out
//! candidate contents, or mutate PR state.
//!
//! The required context has one static producer per event: the PR workflow
//! preserves its comment/label writes, while the merge-group-only workflow is
//! read-only and unconditionally reachable. A second producer on the same event
//! violates `duplicate_emitted_context`; a required-job event guard would violate
//! `always-or-scoped-noop` reachability.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use serde_yaml_ng::Value;

type TestResult = Result<(), Box<dyn std::error::Error>>;
type ValueResult<T> = Result<T, Box<dyn std::error::Error>>;

fn project_root() -> ValueResult<PathBuf> {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|parent| parent.to_path_buf())
        .ok_or_else(|| "CARGO_MANIFEST_DIR has a parent".into())
}

fn workflow_path() -> ValueResult<PathBuf> {
    Ok(project_root()?.join(".github/workflows/merge-group-title-check.yml"))
}

fn workflow_raw() -> ValueResult<String> {
    let path = workflow_path()?;
    fs::read_to_string(&path).map_err(|error| format!("read {}: {error}", path.display()).into())
}

fn workflow() -> ValueResult<Value> {
    serde_yaml_ng::from_str(&workflow_raw()?)
        .map_err(|error| format!("parse merge-group-title-check.yml: {error}").into())
}

fn pr_workflow() -> ValueResult<Value> {
    let path = project_root()?.join(".github/workflows/pr-title-check.yml");
    let raw =
        fs::read_to_string(&path).map_err(|error| format!("read {}: {error}", path.display()))?;
    serde_yaml_ng::from_str(&raw)
        .map_err(|error| format!("parse pr-title-check.yml: {error}").into())
}

fn job<'a>(workflow: &'a Value, id: &str) -> ValueResult<&'a Value> {
    workflow
        .get("jobs")
        .and_then(|jobs| jobs.get(id))
        .ok_or_else(|| format!("job {id} missing").into())
}

/// The merge-group-only workflow's queue branch, from its guard to EOF.
fn queue_branch() -> ValueResult<String> {
    let raw = workflow_raw()?;
    let start = raw.find("if (context.eventName === 'merge_group')").ok_or("queue branch guard")?;
    Ok(raw[start..].to_string())
}

#[test]
fn merge_group_trigger_is_present() -> TestResult {
    let workflow = workflow()?;
    let on = workflow.get("on").ok_or("on block")?;
    assert!(
        on.get("merge_group").is_some(),
        "validate-title must subscribe to merge_group (#18255)"
    );
    Ok(())
}

#[test]
fn exactly_one_static_producer_owns_the_context() -> TestResult {
    let workflow = workflow()?;
    let jobs = workflow.get("jobs").and_then(Value::as_mapping).ok_or("jobs")?;
    let producers: Vec<&str> = jobs
        .iter()
        .filter(|(_, job)| job.get("name").and_then(Value::as_str) == Some("validate-title"))
        .filter_map(|(id, _)| id.as_str())
        .collect();
    assert_eq!(
        producers,
        vec!["validate-title"],
        "the required context must have exactly one static producer"
    );
    Ok(())
}

#[test]
fn producer_job_is_unconditionally_reachable() -> TestResult {
    let workflow = workflow()?;
    let job = job(&workflow, "validate-title")?;
    assert!(
        job.get("if").is_none(),
        "an always-or-scoped-noop required job must not carry a job-level `if`"
    );
    Ok(())
}

#[test]
fn job_never_checks_out_candidate_contents() -> TestResult {
    let raw = workflow_raw()?;
    assert!(
        !raw.contains("actions/checkout"),
        "the job must not check out anything; the policy is defined inline"
    );
    Ok(())
}

#[test]
fn policy_is_defined_once_in_the_workflow() -> TestResult {
    let raw = workflow_raw()?;
    assert_eq!(
        raw.matches("const validateTitle =").count(),
        1,
        "the title policy must be defined exactly once so the paths cannot drift"
    );
    for needle in [
        "dependabot",
        "skip-title-check",
        "release stamp",
        "placeholder issue reference",
        "no issue reference",
    ] {
        assert!(raw.contains(needle), "the inline policy must contain `{needle}`");
    }
    Ok(())
}

#[test]
fn queue_branch_resolves_the_ref_named_pr_and_fails_closed() -> TestResult {
    let queue = queue_branch()?;
    for needle in [
        "mergeGroup.head_sha",
        "mergeGroup.base_sha",
        "mergeGroup.head_ref",
        "gh-readonly-queue/",
        "pulls.get",
        "current.number !== member",
        "merge group title validation failed",
    ] {
        assert!(queue.contains(needle), "queue branch must contain `{needle}`");
    }
    for forbidden in ["compareCommitsWithBasehead", "listPullRequestsAssociatedWithCommit"] {
        assert!(
            !queue.contains(forbidden),
            "commit association must not be membership authority: `{forbidden}`"
        );
    }
    Ok(())
}

#[test]
fn queue_branch_never_mutates_pr_state() -> TestResult {
    let queue = queue_branch()?;
    for forbidden in [
        "createComment",
        "updateComment",
        "addLabels",
        "removeLabel",
        "issues.update",
        "pulls.update",
    ] {
        assert!(!queue.contains(forbidden), "the queue branch must not call `{forbidden}`");
    }
    Ok(())
}

#[test]
fn pr_path_keeps_its_write_boundary() -> TestResult {
    let workflow = pr_workflow()?;
    let perms = workflow.get("permissions").ok_or("workflow permissions")?;
    assert_eq!(perms.get("pull-requests").and_then(Value::as_str), Some("write"));
    assert_eq!(perms.get("checks").and_then(Value::as_str), Some("write"));
    Ok(())
}

#[test]
fn merge_group_producer_is_read_only() -> TestResult {
    let workflow = workflow()?;
    let perms = workflow.get("permissions").ok_or("workflow permissions")?;
    for scope in ["checks", "contents", "issues", "pull-requests"] {
        assert_eq!(perms.get(scope).and_then(Value::as_str), Some("read"));
    }
    for forbidden in ["createComment", "updateComment", "addLabels", "removeLabel"] {
        assert!(
            !workflow_raw()?.contains(forbidden),
            "the merge-group producer must not call `{forbidden}`"
        );
    }
    Ok(())
}

#[test]
fn pr_path_does_not_subscribe_to_merge_group() -> TestResult {
    let workflow = pr_workflow()?;
    let on = workflow.get("on").ok_or("on block")?;
    assert!(on.get("merge_group").is_none(), "merge_group has one dedicated producer");
    assert!(on.get("pull_request_target").is_some());
    assert!(on.get("workflow_dispatch").is_some());
    Ok(())
}

#[test]
fn policy_records_merge_group_as_a_real_producer() -> TestResult {
    let path = project_root()?.join(".ci/policies/required-checks.toml");
    let raw =
        fs::read_to_string(&path).map_err(|error| format!("read {}: {error}", path.display()))?;
    let policy: toml::Value =
        toml::from_str(&raw).map_err(|error| format!("parse required-checks.toml: {error}"))?;

    let checks = policy.get("checks").and_then(toml::Value::as_array).ok_or("checks")?;
    let row = checks
        .iter()
        .find(|row| {
            row.get("workflow").and_then(toml::Value::as_str)
                == Some(".github/workflows/merge-group-title-check.yml")
        })
        .ok_or("merge-group validate-title row")?;
    let events: Vec<&str> = row
        .get("events")
        .and_then(toml::Value::as_array)
        .ok_or("events")?
        .iter()
        .filter_map(toml::Value::as_str)
        .collect();
    assert!(events.contains(&"merge_group"), "validate-title events must include merge_group");

    let check_rows = policy.get("check").and_then(toml::Value::as_array).ok_or("check")?;
    let check_row = check_rows
        .iter()
        .find(|row| {
            row.get("workflow").and_then(toml::Value::as_str)
                == Some(".github/workflows/merge-group-title-check.yml")
        })
        .ok_or("merge-group validate-title governance row")?;
    let events: Vec<&str> = check_row
        .get("events")
        .and_then(toml::Value::as_array)
        .ok_or("governance events")?
        .iter()
        .filter_map(toml::Value::as_str)
        .collect();
    assert_eq!(events, vec!["merge_group"], "this governance row must be merge-group-only");
    if let Some(exemptions) = check_row.get("exemption").and_then(toml::Value::as_array) {
        assert!(
            !exemptions.iter().any(|e| {
                e.get("clause").and_then(toml::Value::as_str) == Some("merge-group-trigger")
            }),
            "the merge-group-trigger exemption must be removed now that the producer exists"
        );
    }
    Ok(())
}

#[test]
fn queue_membership_executes_actual_workflow_behavior() -> TestResult {
    let root = project_root()?;
    let output = Command::new("node")
        .arg(root.join("scripts/ci/test_merge_group_title_workflow.js"))
        .current_dir(root)
        .output()?;
    assert!(
        output.status.success(),
        "workflow behavior failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}
