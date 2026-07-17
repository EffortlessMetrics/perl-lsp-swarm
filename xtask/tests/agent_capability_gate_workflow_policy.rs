//! Contract tests for the M4b agent-capability workflow routing.

use std::{fs, path::PathBuf};

use anyhow::{Result, anyhow, ensure};
use serde_yaml_ng::Value;

#[test]
fn agent_capability_gate_preserves_trust_and_failure_boundaries() -> Result<()> {
    let (content, workflow) = workflow()?;
    let jobs = mapping_value(&workflow, "jobs")?;
    let router = mapping_value(jobs, "route-agent-capability-gate")?;
    let self_hosted = mapping_value(jobs, "agent-capability-gate-self-hosted")?;
    let hosted = mapping_value(jobs, "agent-capability-gate-hosted")?;

    ensure!(
        scalar_string(mapping_value(router, "runs-on")?)? == "ubuntu-24.04",
        "router must stay on the fixed hosted runner"
    );
    ensure!(
        scalar_string(mapping_value(router, "if")?)?
            .contains("github.event_name == 'pull_request'"),
        "router must explicitly cover the pull-request trigger"
    );
    ensure!(
        sequence_strings(mapping_value(mapping_value(self_hosted, "runs-on")?, "labels")?)?
            == ["self-hosted", "linux", "x64", "em-ci", "trusted-pr", "workflow-nano"],
        "self-hosted job labels changed"
    );
    ensure!(
        scalar_string(mapping_value(hosted, "runs-on")?)? == "ubuntu-24.04",
        "fallback must stay on the pinned hosted image"
    );
    ensure!(
        scalar_string(mapping_value(mapping_value(self_hosted, "runs-on")?, "group")?)?
            == "em-ci-nano",
        "self-hosted job must stay in the workflow-nano runner group"
    );
    for (job, target) in [(self_hosted, "self_hosted"), (hosted, "github")] {
        let condition = scalar_string(mapping_value(job, "if")?)?;
        ensure!(
            condition.contains(&format!("outputs.target == '{target}'")),
            "execution job has no static route guard for {target}"
        );
        ensure!(
            scalar_string(mapping_value(job, "needs")?)? == "route-agent-capability-gate",
            "execution job must depend on the router"
        );
    }

    for required in [
        "pull_request:",
        "'.claude/agents/**'",
        "merge_group:",
        "push:",
        "      - main",
        "      - master",
        "contents: read",
        "cancel-in-progress: ${{ github.event_name == 'pull_request' }}",
        "github.event.pull_request.head.repo.full_name != github.repository",
        "bot_pr_github_hosted",
        "runner_token_missing",
        "runner_group_api_failed",
        "runner_group_missing",
        "runner_api_failed",
        "no_idle_runner",
        "workflow_nano_idle",
        "runner-groups?per_page=100",
        "em-ci-nano",
        "runner_group_id",
        "emit \"github\" \"fork_pr\" \"false\" \"true\"",
        "emit \"github\" \"bot_pr_github_hosted\" \"false\" \"true\"",
        "emit \"github\" \"runner_token_missing\" \"true\" \"true\"",
        "cargo xtask check-agent-capabilities",
        "uses: actions/checkout@9c091bb21b7c1c1d1991bb908d89e4e9dddfe3e0",
        "uses: dtolnay/rust-toolchain@fa04a1451ff1842e2626ccb99004d0195b455a88",
    ] {
        ensure!(content.contains(required), "workflow contract missing `{required}`");
    }
    ensure!(
        content.matches("cargo xtask check-agent-capabilities").count() == 2,
        "both execution paths must run the capability checker"
    );
    ensure!(
        content.matches("fallback_allowed").count() >= 6,
        "router outputs and summaries must preserve fallback evidence"
    );

    Ok(())
}

fn workflow() -> Result<(String, Value)> {
    let path = repo_root()?.join(".github/workflows/agent-capability-gate.yml");
    let content = fs::read_to_string(path)?;
    let workflow = serde_yaml_ng::from_str(&content)?;
    Ok((content, workflow))
}

fn repo_root() -> Result<PathBuf> {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("xtask must live under the repository root"))
}

fn mapping_value<'a>(value: &'a Value, key: &str) -> Result<&'a Value> {
    value
        .as_mapping()
        .ok_or_else(|| anyhow!("expected YAML mapping while looking for `{key}`"))?
        .get(Value::String(key.to_string()))
        .ok_or_else(|| anyhow!("missing YAML key `{key}`"))
}

fn scalar_string(value: &Value) -> Result<&str> {
    value.as_str().ok_or_else(|| anyhow!("expected YAML string scalar"))
}

fn sequence_strings(value: &Value) -> Result<Vec<&str>> {
    value
        .as_sequence()
        .ok_or_else(|| anyhow!("expected YAML sequence"))?
        .iter()
        .map(scalar_string)
        .collect()
}
