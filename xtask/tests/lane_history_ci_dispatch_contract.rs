//! Generated lane-history publication must not dispatch development CI (#17903).
//! The compiler-free production-step tests exercise native readback and failures.

use serde_yaml_ng::Value;
use std::fs;
use std::path::PathBuf;

#[test]
fn generated_lane_history_retains_exact_readback_without_dispatch()
-> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("workspace root missing")?
        .to_path_buf();
    let source = fs::read_to_string(root.join(".github/workflows/aggregate-ci-lane-history.yml"))?;
    let workflow: Value = serde_yaml_ng::from_str(&source)?;
    let jobs = workflow.get("jobs").and_then(Value::as_mapping).ok_or("jobs missing")?;
    let steps: Vec<&Value> = jobs
        .values()
        .filter_map(|job| job.get("steps").and_then(Value::as_sequence))
        .flatten()
        .collect();
    assert!(steps.iter().any(|step| {
        step.get("uses")
            .and_then(Value::as_str)
            .is_some_and(|action| action.starts_with("peter-evans/create-pull-request@"))
    }));
    let reconcile = steps
        .iter()
        .find(|step| {
            step.get("name").and_then(Value::as_str) == Some("Reconcile generated PR without CI")
        })
        .ok_or("readback missing")?;
    let run = reconcile.get("run").and_then(Value::as_str).ok_or("readback command missing")?;
    assert!(run.contains("--json headRefOid,state"));
    assert!(run.contains("$CREATED_HEAD_SHA"));
    assert!(run.contains("NOT_PROVEN"));
    for step in steps {
        if let Some(command) = step.get("run").and_then(Value::as_str) {
            assert!(!command.contains("gh workflow run"), "publication cannot dispatch any CI");
            assert!(
                !command.contains("/dispatches"),
                "publication cannot dispatch through the API"
            );
        }
    }
    Ok(())
}
