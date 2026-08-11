//! Contract tying badge generation to the reviewed RIPR workflow release.

use std::{collections::BTreeSet, fs, path::PathBuf};

use serde_yaml_ng::Value;

const EXPECTED_RIPR_EXECUTION_JOBS: &[&str] =
    &["ripr-cx53", "ripr-cx43", "ripr-github", "ripr-fallback"];
const VARIABLE_INSTALL_COMMAND: &str =
    "cargo install ripr --version \"$RIPR_VERSION\" --locked";

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

fn job_run_steps(job: &Value) -> Vec<&str> {
    job.get("steps")
        .and_then(Value::as_sequence)
        .into_iter()
        .flatten()
        .filter_map(|step| step.get("run").and_then(Value::as_str))
        .collect()
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
    let expected_jobs: BTreeSet<_> = EXPECTED_RIPR_EXECUTION_JOBS.iter().copied().collect();
    let installer_jobs: BTreeSet<_> = ripr_jobs
        .iter()
        .filter_map(|(job_name, job)| {
            job_run_steps(job)
                .iter()
                .any(|run| run.contains("cargo install ripr --version"))
                .then(|| job_name.as_str())
                .flatten()
        })
        .collect();
    assert_eq!(
        installer_jobs, expected_jobs,
        "the reviewed RIPR execution-lane set changed; update the contract with the workflow"
    );

    let mut lane_versions = Vec::new();
    for job_name in EXPECTED_RIPR_EXECUTION_JOBS {
        let job = ripr_jobs
            .get(*job_name)
            .ok_or_else(|| format!("ripr.yml is missing execution job `{job_name}`"))?;
        let version = job
            .get("env")
            .and_then(Value::as_mapping)
            .and_then(|env| env.get("RIPR_VERSION"))
            .and_then(Value::as_str)
            .ok_or_else(|| {
                format!("RIPR execution job `{job_name}` must declare env.RIPR_VERSION")
            })?;

        let install_steps: Vec<_> = job_run_steps(job)
            .into_iter()
            .filter(|run| run.contains("cargo install ripr --version"))
            .collect();
        assert_eq!(
            install_steps.len(),
            1,
            "RIPR execution job `{job_name}` must have one explicit installer"
        );
        assert!(
            install_steps[0].contains(VARIABLE_INSTALL_COMMAND),
            "RIPR execution job `{job_name}` must install through its canonical RIPR_VERSION"
        );
        lane_versions.push(((*job_name).to_string(), version.to_string()));
    }

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
