//! Contract tests for #15334 tokmd cockpit-gate artifact producers.

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use serde_yaml_ng::Value;

fn project_root() -> Result<PathBuf> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| anyhow::anyhow!("CARGO_MANIFEST_DIR has no parent"))?
        .to_path_buf())
}

fn read_workflow() -> Result<(String, Value)> {
    let root = project_root()?;
    let path = root.join(".github/workflows/tokmd.yml");
    let content = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let yaml: Value = serde_yaml_ng::from_str(&content)?;
    Ok((content, yaml))
}

fn policy_step_run(yaml: &Value) -> Result<&str> {
    yaml.get("jobs")
        .and_then(|jobs| jobs.get("analyze"))
        .and_then(|job| job.get("steps"))
        .and_then(Value::as_sequence)
        .and_then(|steps| {
            steps.iter().find(|step| {
                step.get("name").and_then(Value::as_str)
                    == Some("Run advisory cockpit policy from trusted base")
            })
        })
        .and_then(|step| step.get("run"))
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("tokmd policy step run block is missing"))
}

#[test]
fn tokmd_skipped_cockpit_gate_uses_distinct_enveloped_artifact() -> Result<()> {
    let (content, yaml) = read_workflow()?;
    let run = policy_step_run(&yaml)?;

    ensure!(
        !run.contains("echo '{\"status\":\"skipped\"}' > .tokmd/gate/cockpit-gate.json"),
        "skipped branch must not reuse cockpit-gate.json with a bare status object"
    );
    ensure!(
        run.contains("cockpit-gate-skipped.json"),
        "skipped branch must write a distinct skipped receipt path"
    );
    ensure!(
        run.contains("cockpit_gate.v1") && run.contains("cockpit_gate_skipped"),
        "skipped receipt must carry a versioned envelope kind"
    );
    ensure!(
        run.contains("tokmd gate .tokmd/cockpit/cockpit.json") && run.contains("cockpit-gate.json"),
        "real gate output must remain on cockpit-gate.json"
    );

    ensure!(
        content.contains("cockpit-gate-skipped.json"),
        "workflow surfaces must mention the skipped receipt path"
    );

    Ok(())
}
