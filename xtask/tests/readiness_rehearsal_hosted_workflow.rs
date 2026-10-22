//! Contract proof for the #16788 hosted no-publish rehearsal matrix.
//!
//! The lane is manual-only. Ordinary PR CI never executes it, so the workflow
//! YAML and routing ledgers have to be executable here: a workflow that
//! reimplements verdicts, drops a cancelled producer, or silently becomes a
//! required gate is a false green.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow, ensure};
use serde_yaml_ng::Value;

const WORKFLOW_PATH: &str = ".github/workflows/readiness-rehearsal-hosted.yml";
const ADMITTED_FREQUENCY: &str = "workflow_dispatch";
const COST_ANALOG_LANE: &str = "vscode_smoke_matrix";

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn workflow() -> Result<(String, Value)> {
    let path = project_root().join(WORKFLOW_PATH);
    let content =
        fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let value =
        serde_yaml_ng::from_str(&content).with_context(|| format!("parsing {}", path.display()))?;
    Ok((content, value))
}

fn workflow_on(workflow: &Value) -> Option<&Value> {
    workflow.as_mapping()?.iter().find_map(|(key, value)| match key {
        Value::String(key) if key == "on" => Some(value),
        Value::Bool(true) => Some(value),
        _ => None,
    })
}

fn get<'a>(value: &'a Value, key: &str) -> Result<&'a Value> {
    value
        .as_mapping()
        .ok_or_else(|| anyhow!("expected a mapping while looking up `{key}`"))?
        .get(Value::String(key.to_string()))
        .ok_or_else(|| anyhow!("missing key `{key}`"))
}

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    get(value, key)?.as_str().ok_or_else(|| anyhow!("`{key}` is not a string"))
}

fn jobs(workflow: &Value) -> Result<&Value> {
    get(workflow, "jobs")
}

fn job<'a>(workflow: &'a Value, name: &str) -> Result<&'a Value> {
    get(jobs(workflow)?, name)
}

fn all_run_bodies(workflow: &Value) -> Result<String> {
    let mut bodies = String::new();
    let jobs = jobs(workflow)?.as_mapping().ok_or_else(|| anyhow!("jobs is not a mapping"))?;
    for (_, job) in jobs {
        if let Ok(steps) = get(job, "steps") {
            if let Some(sequence) = steps.as_sequence() {
                for step in sequence {
                    if let Ok(run) = text(step, "run") {
                        bodies.push_str(run);
                        bodies.push('\n');
                    }
                }
            }
        }
    }
    Ok(bodies)
}

#[test]
fn hosted_lane_is_manual_only_and_read_only() -> Result<()> {
    let (content, workflow) = workflow()?;

    let triggers: BTreeSet<String> = workflow_on(&workflow)
        .ok_or_else(|| anyhow!("workflow declares no triggers"))?
        .as_mapping()
        .ok_or_else(|| anyhow!("`on` is not a mapping"))?
        .keys()
        .filter_map(Value::as_str)
        .map(ToOwned::to_owned)
        .collect();
    ensure!(
        triggers == BTreeSet::from([ADMITTED_FREQUENCY.to_string()]),
        "hosted rehearsal must stay `{ADMITTED_FREQUENCY}`, found {triggers:?}"
    );

    let permissions = get(&workflow, "permissions")?;
    ensure!(
        text(permissions, "contents")? == "read"
            && permissions.as_mapping().is_some_and(|map| map.len() == 1),
        "workflow permissions must be exactly `contents: read`"
    );
    ensure!(!content.contains("id-token:"), "hosted rehearsal must not mint OIDC tokens");
    ensure!(!content.contains("secrets."), "hosted rehearsal must not reference secrets");
    ensure!(
        !content.contains("contents: write"),
        "hosted rehearsal must not request contents: write"
    );
    Ok(())
}

#[test]
fn yaml_invokes_repository_command_and_does_not_reimplement_verdicts() -> Result<()> {
    let (content, workflow) = workflow()?;
    let bodies = all_run_bodies(&workflow)?;

    ensure!(
        bodies.contains("cargo run --locked -p xtask -- readiness-rehearsal hosted-plan"),
        "plan job must invoke hosted-plan"
    );
    ensure!(
        bodies.contains("cargo run --locked -p xtask -- readiness-rehearsal hosted-row"),
        "row job must invoke hosted-row"
    );
    ensure!(
        bodies.contains("--head \"$GITHUB_SHA\""),
        "fan-in must bind rows to the current GITHUB_SHA"
    );
    ensure!(
        bodies.contains("--run-id \"$GITHUB_RUN_ID\""),
        "fan-in must bind rows to the current GITHUB_RUN_ID"
    );
    ensure!(
        bodies.contains("--attempt \"$GITHUB_RUN_ATTEMPT\""),
        "fan-in must bind rows to the current GITHUB_RUN_ATTEMPT"
    );
    ensure!(
        bodies.contains("--matrix-subject \"$MATRIX_SUBJECT\""),
        "row job must bind matrix subject through MATRIX_SUBJECT env"
    );
    ensure!(
        !bodies.contains("${{ matrix.subject }}"),
        "matrix.subject must not be interpolated into interpreter source"
    );
    ensure!(
        content.contains("MATRIX_SUBJECT: ${{ matrix.subject }}"),
        "Bind hosted row must pass matrix.subject through env as data"
    );
    for forbidden in [
        "cargo package",
        "cargo publish",
        "vsce publish",
        "ovsx publish",
        "gh release",
        "docker push",
        "npm publish",
        "row_status",
        "published_channels",
        "release_cut",
    ] {
        ensure!(
            !bodies.contains(forbidden),
            "workflow YAML must not encode `{forbidden}` product semantics"
        );
    }
    ensure!(
        !content.contains("os: [ubuntu-24.04, windows-latest, macos-latest]"),
        "YAML must not hardcode the OS list independently of hosted-plan"
    );
    ensure!(
        content.contains("fromJSON(needs.plan.outputs.matrix)"),
        "row matrix must come from the repository-owned plan output"
    );
    Ok(())
}

#[test]
fn artifact_names_bind_run_attempt_and_subject() -> Result<()> {
    let (content, _) = workflow()?;
    ensure!(
        content.contains(
            "name: readiness-rehearsal-${{ github.run_id }}-${{ github.run_attempt }}-${{ matrix.subject }}"
        ),
        "row artifact names must include run, attempt, and matrix subject"
    );
    ensure!(
        content.contains(
            "pattern: readiness-rehearsal-${{ github.run_id }}-${{ github.run_attempt }}-*"
        ),
        "fan-in must download only this run/attempt's artifacts"
    );
    ensure!(
        content.contains("persist-credentials: false"),
        "checkouts must not persist credentials"
    );
    const UPLOAD_PIN: &str =
        "uses: actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a # v7.0.1";
    ensure!(
        content.matches(UPLOAD_PIN).count() == 3,
        "new/changed upload-artifact pins must use the verified v7.0.1 projection"
    );
    ensure!(
        !content.contains("# v7.0.0"),
        "hosted rehearsal must not carry legacy_debt v7.0.0 pin comments"
    );
    Ok(())
}

#[test]
fn fan_in_runs_when_producers_fail_or_are_cancelled() -> Result<()> {
    let (_, workflow) = workflow()?;
    let fanin = job(&workflow, "fanin")?;
    let condition = text(fanin, "if")?;
    ensure!(
        condition.contains("always()"),
        "fan-in must use always() so cancelled/failed producers still fan in, found `{condition}`"
    );
    ensure!(
        !condition.contains("needs.rehearse.result == 'success'"),
        "fan-in must not skip when the matrix job is non-success"
    );
    let rehearse = job(&workflow, "rehearse")?;
    let strategy = get(rehearse, "strategy")?;
    ensure!(
        get(strategy, "fail-fast")?.as_bool() == Some(false),
        "matrix fail-fast must be false so one row cannot erase the others"
    );
    let steps =
        get(rehearse, "steps")?.as_sequence().ok_or_else(|| anyhow!("rehearse steps missing"))?;
    let upload = steps
        .iter()
        .find(|step| text(step, "name").is_ok_and(|name| name == "Upload row receipt"))
        .ok_or_else(|| anyhow!("missing upload step"))?;
    ensure!(
        text(upload, "if")? == "always()",
        "row upload must run after a failed hosted-row so fan-in sees not_proven instead of missing"
    );

    let fanin_steps =
        get(fanin, "steps")?.as_sequence().ok_or_else(|| anyhow!("fanin steps missing"))?;
    ensure!(
        fanin_steps
            .iter()
            .all(|step| text(step, "name").map_or(true, |name| name != "Download plan")),
        "fan-in must regenerate the admitted plan locally so a failed-job rerun is not blocked by a missing attempt-scoped plan artifact"
    );
    let fanin_run = fanin_steps
        .iter()
        .find(|step| text(step, "name").is_ok_and(|name| name == "Fan-in exact producers"))
        .and_then(|step| text(step, "run").ok())
        .ok_or_else(|| anyhow!("missing fan-in run step"))?;
    ensure!(
        fanin_run.contains("status=$?") && fanin_run.contains("GITHUB_STEP_SUMMARY"),
        "fan-in must keep the step summary when the command exits non-green"
    );
    let admit = fanin_steps
        .iter()
        .find(|step| text(step, "name").is_ok_and(|name| name == "Admit hosted matrix"))
        .and_then(|step| text(step, "run").ok())
        .ok_or_else(|| anyhow!("fan-in must admit the matrix from source"))?;
    ensure!(
        admit.contains("readiness-rehearsal hosted-plan"),
        "fan-in must invoke hosted-plan from the current checkout"
    );
    Ok(())
}

#[test]
fn fork_and_foreign_repository_contexts_are_excluded() -> Result<()> {
    let (_, workflow) = workflow()?;
    for name in ["plan", "rehearse", "fanin"] {
        let condition = text(job(&workflow, name)?, "if")?;
        ensure!(
            condition.contains("github.repository == 'EffortlessMetrics/perl-lsp-swarm'"),
            "{name} must refuse foreign repositories, found `{condition}`"
        );
    }
    Ok(())
}

#[test]
fn routing_ledgers_keep_the_lane_explicit_and_non_required() -> Result<()> {
    let root = project_root();
    let lanes = fs::read_to_string(root.join("policy/ci-lanes.toml"))?;
    ensure!(
        lanes.contains("[lane.readiness_rehearsal_hosted]"),
        "ci-lanes.toml must name the hosted rehearsal lane"
    );
    ensure!(lanes.contains("default_pr = false"), "hosted rehearsal must not be a default PR lane");

    let whitelist = fs::read_to_string(root.join("policy/ci-lane-whitelist.toml"))?;
    let lane_block = whitelist
        .split("[[lane]]")
        .find(|block| block.contains("id = \"readiness_rehearsal_hosted\""))
        .ok_or_else(|| anyhow!("missing readiness_rehearsal_hosted whitelist lane"))?;
    ensure!(
        lane_block.contains(&format!("workflow = \"{WORKFLOW_PATH}\"")),
        "whitelist must point at the hosted rehearsal workflow"
    );
    ensure!(
        lane_block.contains("default_pr = false"),
        "whitelist must not default this lane onto every PR"
    );
    ensure!(lane_block.contains("blocking = false"), "whitelist must not make this lane blocking");
    ensure!(
        lane_block.contains(&format!("allowed_triggers = [\"{ADMITTED_FREQUENCY}\"]")),
        "whitelist triggers must stay manual-only"
    );
    ensure!(
        lane_block.contains(COST_ANALOG_LANE),
        "admission analog {COST_ANALOG_LANE} must remain named in the hosted lane"
    );

    let required = fs::read_to_string(root.join(".ci/policies/required-checks.toml"))?;
    ensure!(
        !required.contains("readiness-rehearsal-hosted"),
        "hosted rehearsal must not enter required-checks.toml"
    );

    let history = fs::read_to_string(root.join(".ci/metrics/ci-lane-history.json"))?;
    ensure!(
        history.contains("\"readiness_rehearsal_hosted\""),
        "lane-history payload must include the hosted rehearsal lane id"
    );

    let economics = fs::read_to_string(root.join("docs/ci/gate-policy-economics.md"))?;
    ensure!(
        economics.contains("`readiness_rehearsal_hosted`"),
        "gate-policy-economics unmapped list must name the hosted rehearsal lane"
    );
    Ok(())
}
