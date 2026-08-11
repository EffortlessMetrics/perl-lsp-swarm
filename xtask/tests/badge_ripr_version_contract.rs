//! Contract tying badge generation to the reviewed RIPR workflow release.

use std::{collections::BTreeSet, fs, path::PathBuf};

use serde_yaml_ng::Value;

fn project_root() -> PathBuf {
    let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    root.pop();
    root
}

fn collect_named_strings(
    value: &Value,
    key: &str,
    output: &mut Vec<String>,
) -> Result<(), String> {
    match value {
        Value::Mapping(mapping) => {
            for (mapping_key, child) in mapping {
                if mapping_key.as_str() == Some(key) {
                    let text = child
                        .as_str()
                        .ok_or_else(|| format!("`{key}` must be a YAML string"))?;
                    output.push(text.to_string());
                }
                collect_named_strings(child, key, output)?;
            }
        }
        Value::Sequence(sequence) => {
            for child in sequence {
                collect_named_strings(child, key, output)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn job_consumes_ripr_version(job: &Value) -> bool {
    job.get("steps").and_then(Value::as_sequence).is_some_and(|steps| {
        steps.iter().any(|step| {
            step.get("run")
                .and_then(Value::as_str)
                .is_some_and(|run| run.contains("RIPR_VERSION"))
        })
    })
}

#[test]
fn badge_installer_matches_the_reviewed_ripr_workflow_release()
    -> Result<(), Box<dyn std::error::Error>>
{
    let root = project_root();
    let ripr_workflow: Value = serde_yaml_ng::from_str(&fs::read_to_string(
        root.join(".github/workflows/ripr.yml"),
    )?)?;
    let badge_workflow: Value = serde_yaml_ng::from_str(&fs::read_to_string(
        root.join(".github/workflows/badge-endpoints.yml"),
    )?)?;

    let ripr_jobs = ripr_workflow
        .get("jobs")
        .and_then(Value::as_mapping)
        .ok_or("ripr.yml must declare jobs")?;
    let mut lane_versions = Vec::new();
    for (job_name, job) in ripr_jobs {
        if !job_consumes_ripr_version(job) {
            continue;
        }
        let job_name = job_name.as_str().ok_or("RIPR job names must be strings")?;
        let version = job
            .get("env")
            .and_then(Value::as_mapping)
            .and_then(|env| env.get("RIPR_VERSION"))
            .and_then(Value::as_str)
            .ok_or_else(|| {
                format!("RIPR execution job `{job_name}` must declare env.RIPR_VERSION")
            })?;
        lane_versions.push((job_name.to_string(), version.to_string()));
    }
    assert!(
        !lane_versions.is_empty(),
        "the routed RIPR workflow must have an execution job consuming its reviewed release"
    );

    let distinct_versions: BTreeSet<_> =
        lane_versions.iter().map(|(_, version)| version.as_str()).collect();
    assert_eq!(
        distinct_versions.len(),
        1,
        "every routed RIPR execution lane must use one reviewed release: {lane_versions:?}"
    );
    let reviewed_version = distinct_versions
        .first()
        .copied()
        .ok_or("the routed RIPR workflow declared no reviewed release")?;

    let mut badge_run_steps = Vec::new();
    collect_named_strings(&badge_workflow, "run", &mut badge_run_steps)?;
    let install_steps: Vec<_> = badge_run_steps
        .iter()
        .map(String::as_str)
        .filter(|run| run.trim().starts_with("cargo install ripr --version "))
        .collect();
    assert_eq!(
        install_steps.len(),
        1,
        "badge generation must have exactly one explicit RIPR installation step"
    );

    let expected_install = format!("cargo install ripr --version {reviewed_version} --locked");
    assert_eq!(
        install_steps[0].trim(),
        expected_install,
        "badge generation must install the reviewed published RIPR release used by routed analysis"
    );

    Ok(())
}
