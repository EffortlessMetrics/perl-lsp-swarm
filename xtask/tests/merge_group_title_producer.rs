//! #18255: the merge queue must receive `validate-title` on the actual G.
//!
//! The queue path resolves the group's constituent PRs from the commit graph,
//! re-reads each current title, and applies the same policy as the PR path. It
//! must not check out candidate contents or mutate PR state.
//!
//! The required context has exactly one static producer: a single
//! unconditionally reachable job whose event split lives inside the script. A
//! second job emitting the same name would violate the gate-enforcement
//! contract (`duplicate_emitted_context`) and a job-level `if` would violate
//! `always-or-scoped-noop` reachability.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use serde_yaml_ng::Value;

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("CARGO_MANIFEST_DIR has a parent")
        .to_path_buf()
}

fn workflow_raw() -> String {
    let path = project_root().join(".github/workflows/pr-title-check.yml");
    fs::read_to_string(&path).expect("read pr-title-check.yml")
}

fn advisory_workflow_raw() -> String {
    let path = project_root().join(".github/workflows/pr-title-advisory.yml");
    fs::read_to_string(&path).expect("read pr-title-advisory.yml")
}

fn workflow() -> Value {
    serde_yaml_ng::from_str(&workflow_raw()).expect("parse pr-title-check.yml")
}

fn advisory_workflow() -> Value {
    serde_yaml_ng::from_str(&advisory_workflow_raw()).expect("parse pr-title-advisory.yml")
}

fn job<'a>(workflow: &'a Value, id: &str) -> &'a Value {
    workflow.get("jobs").and_then(|jobs| jobs.get(id)).unwrap_or_else(|| panic!("job {id} missing"))
}

/// The queue branch of the single script, from its guard to the PR-path marker.
fn queue_branch() -> String {
    let raw = workflow_raw();
    let start = raw.find("if (context.eventName === 'merge_group')").expect("queue branch guard");
    let end = raw.find("// PR and manual-dispatch path.").expect("PR path marker");
    assert!(start < end, "queue branch must precede the PR path");
    raw[start..end].to_string()
}

/// The executable policy shared by required validation and advisory writes.
/// Comments outside this block may differ; these bytes may not.
fn title_policy(raw: &str) -> &str {
    let start = raw.find("const RELEASE_STAMP =").expect("title policy start");
    let terminator = "return { ok: true, kind: 'issue', reason: `issue #${issueNumber}`, issueNumber };\n            };";
    let end = raw[start..]
        .find(terminator)
        .map(|offset| start + offset + terminator.len())
        .expect("title policy end");
    &raw[start..end]
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
fn exactly_one_static_producer_owns_the_context() {
    let workflow = workflow();
    let jobs = workflow.get("jobs").and_then(Value::as_mapping).expect("jobs");
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
}

#[test]
fn producer_job_is_unconditionally_reachable() {
    let workflow = workflow();
    let job = job(&workflow, "validate-title");
    assert!(
        job.get("if").is_none(),
        "an always-or-scoped-noop required job must not carry a job-level `if`"
    );
}

#[test]
fn job_never_checks_out_candidate_contents() {
    for raw in [workflow_raw(), advisory_workflow_raw()] {
        assert!(
            !raw.contains("actions/checkout"),
            "neither workflow may check out anything; the policy is defined inline"
        );
    }
}

#[test]
fn policy_is_defined_once_in_the_workflow() {
    let raw = workflow_raw();
    let advisory = advisory_workflow_raw();
    for policy_owner in [&raw, &advisory] {
        assert_eq!(
            policy_owner.matches("const validateTitle =").count(),
            1,
            "each consumer must contain one title-policy definition"
        );
        for needle in [
            "dependabot",
            "skip-title-check",
            "release stamp",
            "placeholder issue reference",
            "no issue reference",
        ] {
            assert!(policy_owner.contains(needle), "the inline policy must contain `{needle}`");
        }
    }
    assert_eq!(
        title_policy(&raw),
        title_policy(&advisory),
        "required validation and advisory writes must execute the identical title policy"
    );
}

#[test]
fn queue_branch_resolves_members_from_the_commit_graph_and_fails_closed() {
    let queue = queue_branch();
    for needle in [
        "merge_group?.head_sha",
        "merge_group?.base_sha",
        "compareCommitsWithBasehead",
        "listPullRequestsAssociatedWithCommit",
        "pulls.get",
        "no constituent pull requests resolved",
        "merge group title validation failed",
    ] {
        assert!(queue.contains(needle), "queue branch must contain `{needle}`");
    }
}

#[test]
fn queue_branch_never_mutates_pr_state() {
    let queue = queue_branch();
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
}

#[test]
fn required_validation_is_read_only_and_advisory_writes_stay_trusted_sourced() {
    let workflow = workflow();
    let perms = workflow.get("permissions").expect("validation permissions");
    assert_eq!(perms.get("pull-requests").and_then(Value::as_str), Some("read"));
    assert_eq!(perms.get("contents").and_then(Value::as_str), Some("read"));
    assert_eq!(perms.get("checks").and_then(Value::as_str), Some("write"));
    for forbidden in [
        "createComment",
        "updateComment",
        "addLabels",
        "removeLabel",
        "issues.update",
        "pulls.update",
    ] {
        assert!(
            !workflow_raw().contains(forbidden),
            "required validation must not call `{forbidden}`"
        );
    }

    let advisory = advisory_workflow();
    let on = advisory.get("on").expect("advisory triggers");
    assert!(on.get("pull_request_target").is_some());
    assert!(on.get("merge_group").is_none());
    assert!(on.get("workflow_dispatch").is_none());
    let perms = advisory.get("permissions").expect("advisory permissions");
    assert_eq!(perms.get("pull-requests").and_then(Value::as_str), Some("write"));
    let jobs = advisory.get("jobs").and_then(Value::as_mapping).expect("advisory jobs");
    assert_eq!(jobs.len(), 1, "advisory writes must have one bounded owner");
    assert!(
        jobs.values().all(|job| job.get("name").and_then(Value::as_str) != Some("validate-title")),
        "the advisory workflow must not duplicate the required context"
    );
}

#[test]
fn policy_records_merge_group_as_a_real_producer() {
    let path = project_root().join(".ci/policies/required-checks.toml");
    let raw = fs::read_to_string(&path).expect("read required-checks.toml");
    let policy: toml::Value = toml::from_str(&raw).expect("parse required-checks.toml");

    let checks = policy.get("checks").and_then(toml::Value::as_array).expect("checks");
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
    assert!(events.contains(&"merge_group"), "validate-title events must include merge_group");

    let check_rows = policy.get("check").and_then(toml::Value::as_array).expect("check");
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

#[test]
fn queue_membership_executes_actual_workflow_behavior() {
    let root = project_root();
    let output = Command::new("node")
        .arg(root.join("scripts/ci/test_merge_group_title_workflow.js"))
        .current_dir(root)
        .output()
        .expect("run the merge-group title workflow behavior test");
    assert!(
        output.status.success(),
        "workflow behavior failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
