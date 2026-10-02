//! Late CI: draft preparation allocates no runner; ready qualification keeps
//! exact-head formatting/conflict checks and all existing integration routes.

use std::fs;
use std::path::PathBuf;

use serde_yaml_ng::Value;

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;
const GUARD: &str =
    "github.event_name != 'pull_request' || github.event.pull_request.draft != true";

fn project_root() -> TestResult<PathBuf> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("CARGO_MANIFEST_DIR has no parent")?
        .to_path_buf())
}

fn expression(value: &Value) -> TestResult<String> {
    let raw = value.as_str().ok_or("job condition must be a string")?;
    let normalized = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    Ok(normalized
        .strip_prefix("${{")
        .and_then(|inner| inner.strip_suffix("}}"))
        .unwrap_or(&normalized)
        .trim()
        .to_owned())
}

fn assert_preallocation_guard(workflow: &Value) -> TestResult<()> {
    let jobs = workflow.get("jobs").and_then(Value::as_mapping).ok_or("jobs missing")?;
    for (name, job) in jobs {
        let condition = expression(job.get("if").ok_or("job has no draft guard")?)?;
        if condition != GUARD
            && !(condition.starts_with(&format!("({GUARD}) && (")) && condition.ends_with(')'))
        {
            return Err(format!("job {name:?} can allocate a draft runner: {condition}").into());
        }
    }
    Ok(())
}

#[test]
fn drafts_wait_for_native_ready_qualification() -> TestResult<()> {
    let workflow: Value = serde_yaml_ng::from_str(&fs::read_to_string(
        project_root()?.join(".github/workflows/ci.yml"),
    )?)?;
    assert_preallocation_guard(&workflow)?;
    let types = workflow
        .get("on")
        .and_then(|on| on.get("pull_request"))
        .and_then(|pr| pr.get("types"))
        .and_then(Value::as_sequence)
        .ok_or("pull_request types missing")?;
    for event in ["opened", "synchronize", "reopened", "ready_for_review", "converted_to_draft"] {
        assert!(types.iter().any(|value| value.as_str() == Some(event)), "missing event {event}");
    }
    let jobs = workflow.get("jobs").ok_or("jobs missing")?;
    let formatter = jobs.get("rust-formatting").ok_or("formatter missing")?;
    assert!(formatter.get("needs").is_none(), "formatter remains independent of scope selection");
    let subject = formatter
        .get("env")
        .and_then(|env| env.get("SUBJECT_SHA"))
        .and_then(Value::as_str)
        .ok_or("formatter subject missing")?;
    assert!(subject.contains("github.event.pull_request.head.sha"));
    let conflict = jobs.get("conflict-markers").ok_or("conflict check missing")?;
    assert!(
        expression(conflict.get("if").ok_or("conflict condition missing")?)?
            .contains("needs.preflight-latest-check.outputs.is_latest == 'true'")
    );
    let checkout = conflict
        .get("steps")
        .and_then(Value::as_sequence)
        .and_then(|steps| {
            steps.iter().find(|step| {
                step.get("uses")
                    .and_then(Value::as_str)
                    .is_some_and(|action| action.starts_with("actions/checkout@"))
            })
        })
        .ok_or("conflict checkout missing")?;
    assert!(
        checkout
            .get("with")
            .and_then(|with| with.get("ref"))
            .and_then(Value::as_str)
            .ok_or("conflict subject missing")?
            .contains("github.event.pull_request.head.sha")
    );
    assert!(
        expression(
            jobs.get("merge-gate")
                .and_then(|job| job.get("if"))
                .ok_or("result condition missing")?
        )?
        .contains("always()"),
        "ready result must still report failures"
    );
    Ok(())
}

#[test]
fn missing_draft_gate_cannot_hide_behind_a_step_or_always() -> TestResult<()> {
    let workflow: Value = serde_yaml_ng::from_str(&fs::read_to_string(
        project_root()?.join(".github/workflows/ci.yml"),
    )?)?;
    for job in ["draft-pr-check", "conflict-markers", "rust-formatting", "merge-gate"] {
        for condition in ["true", "always()"] {
            let mut mutant = workflow.clone();
            let target = mutant
                .get_mut("jobs")
                .and_then(|jobs| jobs.get_mut(job))
                .and_then(Value::as_mapping_mut)
                .ok_or("fixture job missing")?;
            target.insert(Value::String("if".into()), Value::String(condition.into()));
            assert!(assert_preallocation_guard(&mutant).is_err(), "unguarded {job} must fail");
        }
    }
    Ok(())
}
