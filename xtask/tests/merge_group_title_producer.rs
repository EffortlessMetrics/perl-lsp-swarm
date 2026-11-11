//! The actual-G title leaf has one unconditional, read-only producer.
//!
//! Native observe -> receipt validation -> recheck replaces the superseded
//! commit-association algorithm (#19257). These source-routing tests do not prove
//! that event B is protected: speculative-B executable trust remains an open
//! activation finding, separately from H admission and live native-G proof.

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

fn workflow() -> Value {
    serde_yaml_ng::from_str(&workflow_raw()).expect("parse pr-title-check.yml")
}

fn job<'a>(workflow: &'a Value, id: &str) -> &'a Value {
    workflow.get("jobs").and_then(|jobs| jobs.get(id)).unwrap_or_else(|| panic!("job {id} missing"))
}

#[test]
fn merge_group_is_the_only_title_workflow_trigger() {
    let workflow = workflow();
    let on = workflow.get("on").and_then(Value::as_mapping).expect("on block");
    assert_eq!(on.len(), 1, "Draft/Ready/publication must not launch title Actions");
    let event = workflow.get("on").and_then(|on| on.get("merge_group")).expect("merge_group");
    let types = event.get("types").and_then(Value::as_sequence).expect("event types");
    assert_eq!(types.len(), 1);
    assert_eq!(types[0].as_str(), Some("checks_requested"));
}

#[test]
fn exactly_one_static_producer_owns_the_context() {
    let workflow = workflow();
    let jobs = workflow.get("jobs").and_then(Value::as_mapping).expect("jobs");
    let producers: Vec<&str> = jobs
        .iter()
        .filter_map(|(id, job)| {
            let id = id.as_str()?;
            let emitted = job.get("name").and_then(Value::as_str).unwrap_or(id);
            (emitted == "validate-title").then_some(id)
        })
        .collect();
    assert_eq!(producers, vec!["validate-title"], "one required context producer");
}

#[test]
fn producer_job_is_unconditionally_reachable() {
    let workflow = workflow();
    let job = job(&workflow, "validate-title");
    assert!(job.get("if").is_none(), "required job must not carry a job-level if");
    assert!(job.get("needs").is_none(), "the title producer must not depend on a skipped job");
    assert!(job.get("strategy").is_none(), "one static producer must not become matrix contexts");
    assert!(job.get("continue-on-error").is_none());
    for step in job.get("steps").and_then(Value::as_sequence).expect("steps") {
        assert!(step.get("if").is_none(), "a title proof step must not become a conditional no-op");
        assert!(step.get("continue-on-error").is_none(), "proof failure must remain failure");
    }
}

#[test]
fn source_checkout_is_exact_event_B_without_candidate_checkout_or_saved_credentials() {
    let workflow = workflow();
    let steps =
        job(&workflow, "validate-title").get("steps").and_then(Value::as_sequence).expect("steps");
    let checkouts: Vec<&Value> = steps
        .iter()
        .filter(|step| {
            step.get("uses")
                .and_then(Value::as_str)
                .is_some_and(|uses| uses.starts_with("actions/checkout@"))
        })
        .collect();
    assert_eq!(checkouts.len(), 1);
    let with = checkouts[0].get("with").expect("checkout inputs");
    assert_eq!(
        with.get("ref").and_then(Value::as_str),
        Some("${{ github.event.merge_group.base_sha }}")
    );
    assert_eq!(with.get("path").and_then(Value::as_str), Some("trusted-title-source"));
    assert_eq!(with.get("persist-credentials").and_then(Value::as_bool), Some(false));
    assert_eq!(with.get("fetch-depth").and_then(Value::as_u64), Some(1));
    // This checks the selected source boundary, not whether native B is protected.
}

#[test]
fn observe_validator_and_recheck_are_one_ordered_receipt_pipeline() {
    let workflow = workflow();
    let steps =
        job(&workflow, "validate-title").get("steps").and_then(Value::as_sequence).expect("steps");
    assert_eq!(steps.len(), 4, "checkout, observe, validate, recheck");
    for (index, operation) in [(1, "observe"), (3, "recheck")] {
        let run = steps[index].get("run").and_then(Value::as_str).expect("native operation");
        let expected =
            format!("python3 trusted-title-source/scripts/ci/native_queue_title.py {operation}");
        assert!(run.starts_with(&expected));
        assert!(run.contains("--receipt \"$RUNNER_TEMP/native-queue-title.json\""));
    }
    let script = steps[2]
        .get("with")
        .and_then(|with| with.get("script"))
        .and_then(Value::as_str)
        .expect("actual script");
    assert!(script.contains("scripts/ci/pr_title_validator.js"));
    assert!(script.contains("await validateMembers({github, core, context}, receipt)"));
    assert!(script.contains("native-queue-title.json"));
    assert!(script.contains("JSON.stringify(results)"));
    assert!(!workflow_raw().contains("const validateTitle ="), "do not duplicate policy inline");
}

#[test]
fn superseded_association_algorithm_and_PR_mutations_are_absent_from_workflow() {
    let raw = workflow_raw();
    for forbidden in [
        "compareCommitsWithBasehead",
        "listPullRequestsAssociatedWithCommit",
        "createComment",
        "updateComment",
        "addLabels",
        "removeLabel",
        "issues.update",
        "pulls.update",
    ] {
        assert!(!raw.contains(forbidden), "workflow must not restore {forbidden}");
    }
}

#[test]
fn producer_has_no_write_permissions() {
    let workflow = workflow();
    let permissions =
        workflow.get("permissions").and_then(Value::as_mapping).expect("workflow permissions");
    for value in permissions.values() {
        assert_eq!(value.as_str(), Some("read"), "no workflow write authority");
    }
    for scope in ["contents", "actions", "checks", "pull-requests", "issues"] {
        assert_eq!(
            workflow.get("permissions").and_then(|p| p.get(scope)).and_then(Value::as_str),
            Some("read")
        );
    }
    assert!(
        job(&workflow, "validate-title").get("permissions").is_none(),
        "no job permission override"
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
    assert_eq!(events, vec!["merge_group"], "title context belongs only to actual G");
    let rows = policy.get("check").and_then(toml::Value::as_array).expect("check");
    let row = rows
        .iter()
        .find(|row| row.get("name").and_then(toml::Value::as_str) == Some("validate-title"))
        .expect("validate-title check row");
    if let Some(exemptions) = row.get("exemption").and_then(toml::Value::as_array) {
        assert!(
            !exemptions
                .iter()
                .any(|e| e.get("clause").and_then(toml::Value::as_str)
                    == Some("merge-group-trigger")),
            "no obsolete trigger exemption"
        );
    }
}

#[test]
fn queue_titles_execute_actual_workflow_behavior() {
    let root = project_root();
    let output = Command::new("node")
        .arg(root.join("scripts/ci/test_merge_group_title_workflow.js"))
        .current_dir(root)
        .output()
        .expect("run actual title workflow behavior tests");
    assert!(
        output.status.success(),
        "workflow behavior failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
