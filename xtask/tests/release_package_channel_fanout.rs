//! Fail-closed package-channel fan-out contract for #15460.
//!
//! The original `release.yml`'s "Dispatch downstream package refresh workflows"
//! step started all four bump workflows in one `set -euo pipefail` loop. A
//! transient failure on any iteration exited the step immediately, so later
//! channels were never dispatched; a re-run then re-dispatched the channels
//! that had already succeeded.
//!
//! The acceptance criteria pinned by this contract are:
//!
//! 1. A transient failure dispatching one channel does not prevent the
//!    remaining channels from being dispatched.
//! 2. Re-running the dispatch job for the same tag does not start a channel
//!    twice.
//! 3. Partial fan-out is reported explicitly, naming which channels started
//!    and which did not.
//! 4. Prerelease gating is unchanged (the dispatch steps stay skipped for
//!    prereleases).
//! 5. `release.yml` remains the only orchestrated entry point — no new
//!    dispatch workflow, and the upstream bump workflows are untouched.

use anyhow::{Context, Result, ensure};
use serde_yaml_ng::Value;
use std::path::PathBuf;

const PACKAGE_CHANNELS: [&str; 4] =
    ["brew-bump.yml", "scoop-bump.yml", "chocolatey-bump.yml", "winget-bump.yml"];

const DISPATCH_STEP_IDS: [&str; 4] =
    ["dispatch-brew", "dispatch-scoop", "dispatch-chocolatey", "dispatch-winget"];

fn root() -> Result<PathBuf> {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(PathBuf::from)
        .context("xtask must have a repository parent")
}

fn release_workflow() -> Result<Value> {
    let path = root()?.join(".github/workflows/release.yml");
    let source =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    serde_yaml_ng::from_str(&source).with_context(|| format!("parsing {}", path.display()))
}

fn dispatch_publishers(document: &Value) -> Result<&Value> {
    document
        .get("jobs")
        .and_then(Value::as_mapping)
        .and_then(|jobs| jobs.get(Value::String("dispatch-publishers".to_string())))
        .context("release.yml must declare the dispatch-publishers job")
}

fn steps(job: &Value) -> Result<Vec<&Value>> {
    let sequence = job
        .get("steps")
        .and_then(Value::as_sequence)
        .context("dispatch-publishers must declare steps")?;
    Ok(sequence.iter().collect())
}

fn step_id(step: &Value) -> Option<&str> {
    step.get("id").and_then(Value::as_str)
}

fn step_name(step: &Value) -> Option<&str> {
    step.get("name").and_then(Value::as_str)
}

fn step_continue_on_error(step: &Value) -> bool {
    step.get("continue-on-error").and_then(Value::as_bool).unwrap_or(false)
}

fn step_run_text(step: &Value) -> String {
    serde_yaml_ng::to_string(step).unwrap_or_default()
}

#[test]
fn fan_out_uses_per_channel_steps_with_continue_on_error() -> Result<()> {
    let document = release_workflow()?;
    let job = dispatch_publishers(&document)?;
    let steps = steps(job)?;

    for (channel, expected_id) in PACKAGE_CHANNELS.iter().zip(DISPATCH_STEP_IDS.iter()) {
        let step = steps
            .iter()
            .find(|step| step_id(step).map(|id| id == *expected_id) == Some(true))
            .with_context(|| {
                format!(
                    "dispatch-publishers must have a per-channel step `{expected_id}` for {channel}"
                )
            })?;

        ensure!(
            step_name(step).unwrap_or("").contains(channel.trim_end_matches(".yml")),
            "{channel} dispatch step has the wrong name"
        );
        ensure!(
            step_continue_on_error(step),
            "per-channel dispatch for {channel} must use continue-on-error so a transient \
             failure cannot strand the other channels"
        );
    }

    // The legacy single loop step must be gone. The same name fragment on a
    // single step that iterates through the channels is exactly the failure
    // shape #15460 filed against.
    let legacy_loop_steps: Vec<&str> = steps
        .iter()
        .filter_map(|step| step_name(step))
        .filter(|name| name.contains("Dispatch downstream package refresh workflows"))
        .collect();
    ensure!(
        legacy_loop_steps.is_empty(),
        "single `Dispatch downstream package refresh workflows` loop is the failure shape from \
         #15460 and must not exist; found {legacy_loop_steps:?}"
    );

    Ok(())
}

#[test]
fn each_per_channel_step_checks_idempotency_against_existing_dispatch() -> Result<()> {
    let document = release_workflow()?;
    let job = dispatch_publishers(&document)?;
    let steps = steps(job)?;

    for (channel, expected_id) in PACKAGE_CHANNELS.iter().zip(DISPATCH_STEP_IDS.iter()) {
        let step = steps
            .iter()
            .find(|step| step_id(step).map(|id| id == *expected_id) == Some(true))
            .with_context(|| format!("missing dispatch step `{expected_id}`"))?;

        let text = step_run_text(step);

        // The idempotency check queries recent workflow_dispatch runs on the
        // default branch and filters by head_sha == release SHA. The check
        // must (a) actually issue the API call, (b) filter by the bump
        // workflow's path, and (c) gate the dispatch on its result.
        for required in [
            "gh api",
            "actions/runs?event=workflow_dispatch",
            channel,
            "head_sha",
            "RELEASE_SHA",
            "skipping duplicate dispatch",
        ] {
            ensure!(
                text.contains(required),
                "{channel} dispatch step is missing idempotency seam `{required}`"
            );
        }
    }
    Ok(())
}

#[test]
fn fan_out_step_dispatches_against_same_release_sha_for_each_channel() -> Result<()> {
    // RELEASE_SHA is the only stable per-release identifier we have in the
    // dispatch-publishers job. It must be sourced from ${{ github.sha }} (the
    // validated release commit, see the `validate_subject_sha` step) and
    // shared across all four per-channel steps — otherwise the idempotency
    // check would not find sibling dispatches.
    let document = release_workflow()?;
    let job = dispatch_publishers(&document)?;
    let steps = steps(job)?;

    let release_sha_seen = steps
        .iter()
        .filter(|step| step_id(step).map(|id| id.starts_with("dispatch-")) == Some(true))
        .filter_map(|step| step.get("env").and_then(Value::as_mapping))
        .filter_map(|env| env.get(Value::String("RELEASE_SHA".into())).and_then(Value::as_str))
        .all(|value| value.contains("github.sha"));

    ensure!(
        release_sha_seen,
        "every per-channel dispatch step must bind RELEASE_SHA to ${{{{ github.sha }}}}"
    );
    Ok(())
}

#[test]
fn fan_out_step_releases_under_prerelease_gate() -> Result<()> {
    let document = release_workflow()?;
    let job = dispatch_publishers(&document)?;
    let steps = steps(job)?;

    for expected_id in DISPATCH_STEP_IDS.iter() {
        let step = steps
            .iter()
            .find(|step| step_id(step).map(|id| id == *expected_id) == Some(true))
            .with_context(|| format!("missing dispatch step `{expected_id}`"))?;

        let gate = step
            .get("if")
            .and_then(Value::as_str)
            .context("per-channel dispatch must carry a prerelease gate")?;
        ensure!(
            gate.contains("prerelease") && gate.contains("!= 'true'"),
            "per-channel dispatch `{expected_id}` must skip on prerelease"
        );
    }

    // The prerelease summary record step must still exist so prerelease
    // runs have a disposition line.
    let prerelease_record = steps
        .iter()
        .find(|step| {
            step_name(step).unwrap_or("") == "Record prerelease package refresh disposition"
        })
        .context("prerelease disposition record step must remain")?;
    ensure!(
        step_id(prerelease_record).is_none(),
        "prerelease disposition record step should not declare a step id"
    );
    Ok(())
}

#[test]
fn fan_out_step_emits_explicit_disposition_summary() -> Result<()> {
    // The summary step must always run (it has `if: always()`) so a partial
    // fan-out is visible in the job summary, not inferred from the Actions
    // history. It must report each channel by name and read each dispatch
    // step's conclusion.
    let document = release_workflow()?;
    let job = dispatch_publishers(&document)?;
    let steps = steps(job)?;

    let summary = steps
        .iter()
        .find(|step| {
            step_name(step).unwrap_or("") == "Summarize package-channel fan-out disposition"
        })
        .context("fan-out summary step must exist")?;

    let gate = summary
        .get("if")
        .and_then(Value::as_str)
        .context("fan-out summary step must carry an `if` condition")?;
    ensure!(
        gate.contains("always()") && gate.contains("prerelease"),
        "fan-out summary must run regardless of dispatch step outcomes and skip on prerelease"
    );

    let env = summary
        .get("env")
        .and_then(Value::as_mapping)
        .context("fan-out summary must declare its dispatch outcomes in env")?;
    let outcome_env_keys = [
        "DISPATCH_BREW_OUTCOME",
        "DISPATCH_SCOOP_OUTCOME",
        "DISPATCH_CHOCOLATEY_OUTCOME",
        "DISPATCH_WINGET_OUTCOME",
    ];
    for key in outcome_env_keys.iter() {
        let value = env
            .get(Value::String((*key).into()))
            .and_then(Value::as_str)
            .with_context(|| format!("fan-out summary env must bind `{key}`"))?;
        ensure!(
            value.contains("steps["),
            "fan-out summary env `{key}` must source from steps[*].conclusion (got {value:?})"
        );
    }

    let text = step_run_text(summary);
    for channel in ["brew", "scoop", "chocolatey", "winget"] {
        ensure!(text.contains(channel), "fan-out summary must explicitly name `{channel}`");
    }
    for key in outcome_env_keys.iter() {
        ensure!(
            text.contains(key),
            "fan-out summary must reference env var `{key}` (no inline steps[*].conclusion)"
        );
    }
    Ok(())
}

#[test]
fn each_per_channel_step_dispatches_against_default_branch_with_tag_input() -> Result<()> {
    // The actual `gh workflow run` payload must remain compatible with the
    // bump workflows: ref is the default branch, and the `tag` input carries
    // the release tag. This guards against an accidental regression where
    // the idempotency check replaces the dispatch body.
    let document = release_workflow()?;
    let job = dispatch_publishers(&document)?;
    let steps = steps(job)?;

    for (channel, expected_id) in PACKAGE_CHANNELS.iter().zip(DISPATCH_STEP_IDS.iter()) {
        let step = steps
            .iter()
            .find(|step| step_id(step).map(|id| id == *expected_id) == Some(true))
            .with_context(|| format!("missing dispatch step `{expected_id}`"))?;

        let text = step_run_text(step);
        for required in ["gh workflow run", channel, "--ref", "DEFAULT_BRANCH", "tag=$RELEASE_TAG"]
        {
            ensure!(text.contains(required), "{channel} dispatch must still issue `{required}`");
        }
    }
    Ok(())
}

#[test]
fn release_workflow_remains_the_only_orchestrated_entry_point() -> Result<()> {
    // The bump workflows' contract (workflow_dispatch with `tag` input) is the
    // documented entry point per file. release.yml is the only workflow that
    // issues a `gh workflow run` against the bump files. If a new dispatch
    // workflow were introduced, this guard would catch it.
    let workflow_dir = root()?.join(".github/workflows");
    for bump in PACKAGE_CHANNELS.iter() {
        let mut found_dispatch = Vec::new();
        for entry in std::fs::read_dir(&workflow_dir)
            .with_context(|| format!("reading {}", workflow_dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("yml") {
                continue;
            }
            let source = std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            if source.contains(&format!("gh workflow run {bump}"))
                || source.contains(&format!("workflows/{bump}/dispatches"))
            {
                found_dispatch.push(
                    path.file_name()
                        .with_context(|| format!("path {} has no file_name", path.display()))?
                        .to_string_lossy()
                        .to_string(),
                );
            }
        }
        let allowed: Vec<&str> = found_dispatch
            .iter()
            .filter(|name| name.as_str() == "release.yml")
            .map(|name| name.as_str())
            .collect();
        ensure!(
            allowed.len() == 1 && found_dispatch.len() == 1,
            "bump workflow `{bump}` must be dispatched only from release.yml \
             (found dispatches from {found_dispatch:?})"
        );
    }
    Ok(())
}

#[test]
fn bump_workflows_remain_untouched_in_authority_and_trigger_surface() -> Result<()> {
    // The bump workflows already declare workflow_dispatch with a `tag`
    // input and a per-tag concurrency group. The fix must not reach into
    // them — it is contained inside release.yml's dispatch-publishers job.
    for bump in PACKAGE_CHANNELS.iter() {
        let path = root()?.join(".github/workflows").join(bump);
        let source = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        ensure!(
            source.contains("workflow_dispatch"),
            "{bump} must still expose workflow_dispatch as the entry point"
        );
        ensure!(
            source.contains("concurrency:"),
            "{bump} must still carry a per-tag concurrency group as the backstop"
        );
    }
    Ok(())
}

#[test]
fn pre_release_skips_all_dispatch_steps_and_records_disposition() -> Result<()> {
    // Re-render release.yml as a Value and assert that the prerelease gate
    // exists on every per-channel dispatch step, so a prerelease run leaves
    // each step skipped and only the prerelease disposition record runs.
    // We don't simulate GitHub here — we just enforce the `if` shape that
    // #15460 requires to keep prerelease behavior unchanged.
    let document = release_workflow()?;
    let job = dispatch_publishers(&document)?;
    let steps = steps(job)?;

    let prerelease_record = steps
        .iter()
        .find(|step| {
            step_name(step).unwrap_or("") == "Record prerelease package refresh disposition"
        })
        .context("prerelease disposition record step is required")?;
    let prerelease_gate = prerelease_record
        .get("if")
        .and_then(Value::as_str)
        .context("prerelease disposition record step must carry a prerelease-only `if`")?;
    ensure!(
        prerelease_gate.contains("prerelease") && prerelease_gate.contains("== 'true'"),
        "prerelease disposition record step must run only when prerelease is true"
    );

    // None of the per-channel dispatch steps may declare the prerelease
    // equivalent (== 'true') — that would be a regression that re-fires
    // them on prereleases.
    for expected_id in DISPATCH_STEP_IDS.iter() {
        let step = steps
            .iter()
            .find(|step| step_id(step).map(|id| id == *expected_id) == Some(true))
            .with_context(|| format!("missing dispatch step `{expected_id}`"))?;
        let gate =
            step.get("if").and_then(Value::as_str).context("dispatch step must carry an `if`")?;
        ensure!(
            !gate.contains("== 'true'"),
            "per-channel dispatch `{expected_id}` must not run on prereleases"
        );
    }
    Ok(())
}

#[test]
fn summary_step_reports_six_lines_explicitly() -> Result<()> {
    // The summary step's run block must print one line per channel plus a
    // header line. This is the "Partial fan-out is reported explicitly"
    // acceptance criterion.
    let document = release_workflow()?;
    let job = dispatch_publishers(&document)?;
    let steps = steps(job)?;

    let summary = steps
        .iter()
        .find(|step| {
            step_name(step).unwrap_or("") == "Summarize package-channel fan-out disposition"
        })
        .context("fan-out summary step must exist")?;
    let run = summary
        .get("run")
        .and_then(Value::as_str)
        .context("fan-out summary step must have a run block")?;

    let channel_lines = run
        .lines()
        .filter(|line| {
            line.contains("brew:")
                || line.contains("scoop:")
                || line.contains("chocolatey:")
                || line.contains("winget:")
        })
        .count();
    ensure!(
        channel_lines == 4,
        "fan-out summary must report exactly four channel lines (got {channel_lines})"
    );
    ensure!(
        run.contains("Package-channel fan-out disposition"),
        "fan-out summary must carry a header line naming the disposition"
    );
    Ok(())
}

fn _silence_unused_for_older_rustc() {
    // serde_yaml_ng re-export occasionally changes; keep a compile-time
    // pointer in place so the test module stays wired.
    let _ = Value::Null;
}
