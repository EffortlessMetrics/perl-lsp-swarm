//! #18255: the merge queue must receive `validate-title` on the actual G.
//!
//! The queue path resolves the group's constituent PRs from an authoritative
//! source, validates every member's title, and fails closed on absent or
//! contradictory membership. It must not check out candidate contents or hold
//! PR comment/label write privileges.

use std::fs;
use std::path::PathBuf;

use serde_yaml_ng::Value;

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("CARGO_MANIFEST_DIR has a parent")
        .to_path_buf()
}

fn workflow() -> Value {
    let path = project_root().join(".github/workflows/pr-title-check.yml");
    let raw = fs::read_to_string(&path).expect("read pr-title-check.yml");
    serde_yaml_ng::from_str(&raw).expect("parse pr-title-check.yml")
}

fn job<'a>(workflow: &'a Value, id: &str) -> &'a Value {
    workflow
        .get("jobs")
        .and_then(|jobs| jobs.get(id))
        .unwrap_or_else(|| panic!("job {id} missing"))
}

#[test]
fn merge_group_trigger_is_present() {
    let workflow = workflow();
    let on = workflow.get("on").expect("on block");
    assert!(
        on.get("merge_group").is_some(),
        "validate-title must subscribe to merge_group (#18255)"
    );
}

#[test]
fn queue_job_owns_the_required_context_name() {
    let workflow = workflow();
    let queue = job(&workflow, "validate-title-merge-group");
    assert_eq!(
        queue.get("name").and_then(Value::as_str),
        Some("validate-title"),
        "the queue job must emit the required `validate-title` context"
    );
    assert_eq!(
        queue.get("if").and_then(Value::as_str),
        Some("github.event_name == 'merge_group'"),
        "the queue job must run only on merge_group"
    );
}

#[test]
fn queue_job_has_no_pr_write_or_checkout() {
    let workflow = workflow();
    let queue = job(&workflow, "validate-title-merge-group");
    let perms = queue.get("permissions").expect("queue permissions");
    assert_eq!(perms.get("contents").and_then(Value::as_str), Some("read"));
    assert_eq!(perms.get("checks").and_then(Value::as_str), Some("write"));
    assert!(
        perms.get("pull-requests").is_none(),
        "the queue job must not inherit PR comment/label write privileges"
    );
    let raw = serde_yaml_ng::to_string(queue).expect("serialize queue job");
    assert!(
        !raw.contains("actions/checkout"),
        "the queue job must not check out candidate contents"
    );
}

#[test]
fn queue_job_resolves_members_authoritatively_and_fails_closed() {
    let workflow = workflow();
    let queue = job(&workflow, "validate-title-merge-group");
    let raw = serde_yaml_ng::to_string(queue).expect("serialize queue job");
    for needle in [
        "merge_group?.head_sha",
        "merge_group?.base_sha",
        "compareCommitsWithBasehead",
        "mergeQueue(branch:$branch)",
        "listPullRequestsAssociatedWithCommit",
        "no constituent pull requests resolved",
        "for (const pr of members)",
        "merge group title validation failed",
    ] {
        assert!(raw.contains(needle), "queue job must contain `{needle}`");
    }
}

#[test]
fn pr_path_keeps_its_write_boundary() {
    let workflow = workflow();
    let pr = job(&workflow, "validate-title");
    let perms = pr.get("permissions").expect("PR permissions");
    assert_eq!(
        perms.get("pull-requests").and_then(Value::as_str),
        Some("write")
    );
    assert_eq!(perms.get("checks").and_then(Value::as_str), Some("write"));
    assert_eq!(
        pr.get("if").and_then(Value::as_str),
        Some("github.event_name != 'merge_group'"),
        "the PR path must not run on merge_group"
    );
}

#[test]
fn policy_records_merge_group_as_a_real_producer() {
    let path = project_root().join(".ci/policies/required-checks.toml");
    let raw = fs::read_to_string(&path).expect("read required-checks.toml");
    let policy: toml::Value = toml::from_str(&raw).expect("parse required-checks.toml");

    let checks = policy
        .get("checks")
        .and_then(toml::Value::as_array)
        .expect("checks");
    let row = checks
        .iter()
        .find(|row| row.get("name").and_then(toml::Value::as_str) == Some("validate-title"))
        .expect("validate-title row");
    let events: Vec<&str> = row
        .get("events")
        .and_then(toml::Value::as_array)
        .expect("events")
        .iter()
        .filter_map(toml::Value::as_str)
        .collect();
    assert!(
        events.contains(&"merge_group"),
        "validate-title events must include merge_group"
    );

    let check_rows = policy
        .get("check")
        .and_then(toml::Value::as_array)
        .expect("check");
    let check_row = check_rows
        .iter()
        .find(|row| row.get("name").and_then(toml::Value::as_str) == Some("validate-title"))
        .expect("validate-title check row");
    if let Some(exemptions) = check_row.get("exemption").and_then(toml::Value::as_array) {
        assert!(
            !exemptions.iter().any(|e| {
                e.get("clause").and_then(toml::Value::as_str) == Some("merge-group-trigger")
            }),
            "the merge-group-trigger exemption must be removed now that the producer exists"
        );
    }
}
