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
    let router_script = route_script(router)?;

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
        !content.contains("ubuntu-latest"),
        "workflow must not use the floating ubuntu-latest runner"
    );
    ensure!(
        content.contains("IS_FORK_PR") && content.contains("PR_AUTHOR_TYPE"),
        "router must retain fork and bot isolation inputs"
    );

    let self_runs_on = mapping_value(self_hosted, "runs-on")?;
    let self_hosted_labels = if self_runs_on.is_sequence() {
        sequence_strings(self_runs_on)?
    } else {
        sequence_strings(mapping_value(self_runs_on, "labels")?)?
    };
    ensure!(
        self_hosted_labels
            == ["self-hosted", "linux", "x64", "em-ci", "trusted-pr", "workflow-nano"],
        "self-hosted job labels changed"
    );
    ensure!(
        self_hosted_labels.len() == 6,
        "self-hosted job labels must not have missing or extra entries"
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
        let expected_condition = format!(
            "needs.route-agent-capability-gate.result == 'success' && needs.route-agent-capability-gate.outputs.target == '{target}'"
        );
        ensure!(
            condition == expected_condition,
            "execution job routing condition changed for {target}"
        );
        ensure!(
            has_need(mapping_value(job, "needs")?, "route-agent-capability-gate"),
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
        "ORG: ${{ github.repository_owner }}",
        "bot_pr_github_hosted",
        "runner_token_missing",
        "runner_group_api_failed",
        "runner_group_missing",
        "runner_group_response_parse_failed",
        "runner_api_failed",
        "runner_response_parse_failed",
        "tempfile_creation_failed",
        "no_idle_runner",
        "workflow_nano_idle",
        "runner-groups?per_page=100",
        "em-ci-nano",
        "runner_group_id",
        "--paginate",
        "--slurp",
        "emit \"github\" \"fork_pr\" \"false\" \"true\"",
        "emit \"github\" \"bot_pr_github_hosted\" \"false\" \"true\"",
        "emit \"github\" \"runner_token_missing\" \"true\" \"true\"",
        "cargo xtask check-agent-capabilities",
        "uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1",
        "uses: dtolnay/rust-toolchain@6c977a6ca4077a0ceb28ffbe03f59d46e9ac8772",
    ] {
        ensure!(content.contains(required), "workflow contract missing `{required}`");
    }
    for (guard, emit) in [
        (
            "if ! nano_group_id=",
            "emit \"github\" \"runner_group_response_parse_failed\" \"true\" \"true\"",
        ),
        (
            "if ! idle_runner_count_value=",
            "emit \"github\" \"runner_response_parse_failed\" \"true\" \"true\"",
        ),
        ("if ! group_response=", "emit \"github\" \"tempfile_creation_failed\" \"true\" \"true\""),
        ("if ! response=", "emit \"github\" \"tempfile_creation_failed\" \"true\" \"true\""),
    ] {
        ensure!(
            route_branch_contains_emit(router_script, guard, emit),
            "router guard `{guard}` must emit its hosted fallback before exiting"
        );
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

fn route_script(router: &Value) -> Result<&str> {
    mapping_value(router, "steps")?
        .as_sequence()
        .ok_or_else(|| anyhow!("router steps must be a YAML sequence"))?
        .iter()
        .find(|step| {
            mapping_value(step, "id").and_then(scalar_string).is_ok_and(|id| id == "route")
        })
        .ok_or_else(|| anyhow!("router route step is missing"))
        .and_then(|step| scalar_string(mapping_value(step, "run")?))
}

fn route_branch_contains_emit(script: &str, guard: &str, emit: &str) -> bool {
    let Some(start) = script.find(guard) else {
        return false;
    };
    let branch = &script[start..];
    let Some(exit) = branch.find("exit 0") else {
        return false;
    };
    branch[..exit].contains(emit)
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

fn has_need(value: &Value, expected: &str) -> bool {
    value.as_str() == Some(expected)
        || value
            .as_sequence()
            .is_some_and(|values| values.iter().any(|value| value.as_str() == Some(expected)))
}
