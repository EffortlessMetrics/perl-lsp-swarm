//! Contract proof for the issue #5432 read-only macOS safe-ICF shadow lane.
//!
//! The lane is manual-only and runs on native macOS runners, so ordinary CI
//! never executes it. That is exactly why its contract has to be executable
//! here: a lane that silently drifts from the measurement instrument produces
//! `not_proven` receipts after two full release builds, or — worse — a
//! confident receipt built from mismatched evidence.
//!
//! Every assertion below is bound to the instrument's own constants rather than
//! restating them, so the workflow and `release_artifact_size` cannot disagree.

#[path = "../src/bin/release_artifact_size/policy.rs"]
mod policy;

use std::{collections::BTreeSet, fs, path::PathBuf};

use anyhow::{Context, Result, anyhow, ensure};
use serde_yaml_ng::Value;

use policy::{
    BINARY_NAMES, GOVERNED_TARGET_RUNNERS, GOVERNED_TARGETS, SAFE_ICF_RUSTFLAGS,
    SHADOW_WORKFLOW_PATH,
};

/// The fixed path `xtask lsp-ux-smoke` always writes to. The lane must retain a
/// per-variant copy; comparing this path directly would let the baseline
/// receipt stand in for the candidate's.
const SHARED_LSP_RECEIPT: &str = "target/receipts/ux/lsp-ux-smoke.json";

/// The workspace root: `xtask/..` without an unwrap on an optional parent.
fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn workflow() -> Result<(String, Value)> {
    let path = project_root().join(SHADOW_WORKFLOW_PATH);
    let content =
        fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let value =
        serde_yaml_ng::from_str(&content).with_context(|| format!("parsing {}", path.display()))?;
    Ok((content, value))
}

/// YAML 1.1 parsers fold a bare `on:` key into the boolean `true`. Mirror the
/// repository lint's accessor so this proof reads the same trigger block the
/// policy checker does.
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

fn steps(workflow: &Value) -> Result<Vec<&Value>> {
    let job = get(get(workflow, "jobs")?, "measure")?;
    Ok(get(job, "steps")?
        .as_sequence()
        .ok_or_else(|| anyhow!("`steps` is not a sequence"))?
        .iter()
        .collect())
}

fn step_named<'a>(steps: &[&'a Value], name: &str) -> Result<(usize, &'a Value)> {
    steps
        .iter()
        .enumerate()
        .find(|(_, step)| text(step, "name").is_ok_and(|value| value == name))
        .map(|(index, step)| (index, *step))
        .ok_or_else(|| anyhow!("no step named `{name}`"))
}

/// A step's `run:` body, or an empty string for `uses:` steps.
fn run_body(step: &Value) -> String {
    text(step, "run").unwrap_or_default().to_string()
}

#[test]
fn shadow_lane_is_manual_only_and_read_only() -> Result<()> {
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
        triggers == BTreeSet::from(["workflow_dispatch".to_string()]),
        "the measurement lane must stay manual-only, found triggers {triggers:?}"
    );

    let permissions = get(&workflow, "permissions")?;
    ensure!(
        text(permissions, "contents")? == "read"
            && permissions.as_mapping().is_some_and(|map| map.len() == 1),
        "workflow permissions must be exactly `contents: read`"
    );
    let job = get(get(&workflow, "jobs")?, "measure")?;
    let job_permissions = get(job, "permissions")?;
    ensure!(
        text(job_permissions, "contents")? == "read"
            && job_permissions.as_mapping().is_some_and(|map| map.len() == 1),
        "the measure job must declare exactly `contents: read`"
    );

    for forbidden in [
        "contents: write",
        "id-token",
        "packages: write",
        "cargo publish",
        "gh release",
        "action-gh-release",
        "secrets.",
    ] {
        ensure!(
            !content.contains(forbidden),
            "a measurement lane must not carry `{forbidden}`; this lane produces evidence only"
        );
    }

    ensure!(
        text(get(&workflow, "concurrency")?, "group").is_ok(),
        "the lane must declare a concurrency group"
    );
    ensure!(
        get(get(&workflow, "concurrency")?, "cancel-in-progress")? == &Value::Bool(false),
        "a measurement in flight must not be cancelled by a later dispatch"
    );

    for step in steps(&workflow)? {
        if let Ok(uses) = text(step, "uses") {
            let reference = uses
                .rsplit_once('@')
                .map(|(_, reference)| reference)
                .ok_or_else(|| anyhow!("`{uses}` is not pinned"))?;
            ensure!(
                reference.len() == 40 && reference.chars().all(|ch| ch.is_ascii_hexdigit()),
                "`{uses}` must be pinned to a full commit SHA"
            );
        }
    }

    Ok(())
}

#[test]
fn shadow_lane_measures_exactly_the_governed_targets_natively() -> Result<()> {
    let (_, workflow) = workflow()?;

    let on = workflow_on(&workflow).ok_or_else(|| anyhow!("workflow declares no triggers"))?;
    let inputs = get(on, "workflow_dispatch")?;
    let options: Vec<String> = get(get(get(inputs, "inputs")?, "target")?, "options")?
        .as_sequence()
        .ok_or_else(|| anyhow!("target options are not a sequence"))?
        .iter()
        .filter_map(Value::as_str)
        .map(ToOwned::to_owned)
        .collect();
    let offered: BTreeSet<&str> = options.iter().map(String::as_str).collect();
    let governed: BTreeSet<&str> = GOVERNED_TARGETS.into_iter().collect();
    ensure!(
        offered == governed,
        "the lane must offer exactly the governed triples, found {offered:?}"
    );

    // `measure.rs` returns `not_proven` when the rustc host is not the measured
    // target, so each triple must select its own native runner image.
    let selected = GOVERNED_TARGET_RUNNERS[1];
    let fallback = GOVERNED_TARGET_RUNNERS[0];
    let expected_runs_on = format!(
        "${{{{ inputs.target == '{}' && '{}' || '{}' }}}}",
        selected.0, selected.1, fallback.1
    );
    let job = get(get(&workflow, "jobs")?, "measure")?;
    ensure!(
        text(job, "runs-on")? == expected_runs_on,
        "runner selection must pair each governed triple with its native image; expected \
         `{expected_runs_on}`, found `{}`",
        text(job, "runs-on")?
    );

    Ok(())
}

#[test]
fn shadow_lane_isolates_the_safe_icf_flags_to_the_candidate() -> Result<()> {
    let (_, workflow) = workflow()?;

    ensure!(
        text(get(&workflow, "env")?, "SAFE_ICF_RUSTFLAGS")? == SAFE_ICF_RUSTFLAGS,
        "the lane's candidate flags must equal the flags the instrument accepts"
    );

    let steps = steps(&workflow)?;
    let (baseline_index, baseline) = step_named(&steps, "Build baseline")?;
    let (candidate_index, candidate) = step_named(&steps, "Build candidate")?;

    ensure!(
        text(get(baseline, "env")?, "RUSTFLAGS")?.is_empty(),
        "the baseline must declare no linker policy; the instrument rejects a dirty baseline"
    );
    ensure!(
        text(get(candidate, "env")?, "RUSTFLAGS")? == "${{ env.SAFE_ICF_RUSTFLAGS }}",
        "the candidate must build with exactly the declared safe-ICF flags"
    );

    // The realistic wrong lane links both variants with ICF and reports a null
    // delta as evidence that ICF does nothing.
    ensure!(
        !run_body(baseline).contains("icf") && !run_body(baseline).contains("rust-lld"),
        "the baseline build step must not reference the candidate link flags"
    );

    for name in BINARY_NAMES {
        for (label, step) in [("baseline", baseline), ("candidate", candidate)] {
            ensure!(
                run_body(step).contains(&format!("-p {name} --bin {name}")),
                "the {label} build must produce `{name}`"
            );
        }
    }

    // `product_identity.rs::embedded_build_input` compiles these values in via
    // `option_env!`. Without them the measured binaries are not the release
    // configuration, so safe ICF folds a different program than the one that
    // ships and an `adopt` would not describe the shipped artifacts. They must
    // be prepared exactly once, before the baseline, so both variants inherit
    // one identical identity and the A/B still isolates the link flags.
    let (identity_index, identity) = step_named(&steps, "Prepare identity-bearing build inputs")?;
    let identity_body = run_body(identity);
    for key in [
        "PERL_LSP_BUILD_REVISION",
        "PERL_LSP_SOURCE_TREE_DIGEST",
        "PERL_LSP_TARGET_TRIPLE",
        "PERL_LSP_BUILD_PROFILE",
        "PERL_LSP_ARTIFACT_ROLE",
        "PERL_LSP_CANDIDATE_ID",
    ] {
        ensure!(
            identity_body.contains(key),
            "the measured binaries must carry the release build input `{key}`"
        );
    }
    ensure!(
        identity_body.contains("$GITHUB_ENV"),
        "the identity must reach both builds through the job environment, not one step"
    );
    ensure!(
        identity_index < baseline_index,
        "identity must be prepared before the baseline, or the two variants differ by more \
         than the link flags"
    );
    for (label, step) in [("baseline", baseline), ("candidate", candidate)] {
        ensure!(
            !run_body(step).contains("PERL_LSP_"),
            "the {label} build must inherit the shared identity rather than declaring its own"
        );
    }

    // A candidate linked from a surviving baseline artifact was never relinked.
    let (discard_index, discard) = step_named(&steps, "Discard the baseline build directory")?;
    ensure!(
        run_body(discard).contains("rm -rf \"target/${TARGET}/release\""),
        "the lane must clear the release build directory between variants"
    );
    ensure!(
        baseline_index < discard_index && discard_index < candidate_index,
        "the build directory must be discarded after the baseline and before the candidate"
    );

    Ok(())
}

#[test]
fn shadow_lane_compares_per_variant_smoke_receipts() -> Result<()> {
    let (content, workflow) = workflow()?;
    let steps = steps(&workflow)?;
    let (_, compare) = step_named(&steps, "Compare baseline and candidate artifacts")?;
    let body = run_body(compare);

    for variant in ["baseline", "candidate"] {
        for receipt in ["lsp-smoke.json", "dap-smoke.json"] {
            let path = format!("target/shadow/{variant}/{receipt}");
            ensure!(
                body.contains(&path),
                "the comparison must consume the retained receipt `{path}`"
            );
        }
    }

    // `xtask lsp-ux-smoke` always writes one fixed path. Comparing it directly
    // would silently reuse the baseline receipt for the candidate.
    ensure!(
        !body.contains(SHARED_LSP_RECEIPT),
        "the comparison must not read the shared LSP receipt path `{SHARED_LSP_RECEIPT}`"
    );

    ensure!(
        body.contains("--baseline-rustflags ''")
            && body.contains("--candidate-rustflags \"$SAFE_ICF_RUSTFLAGS\""),
        "the declared flags passed to the instrument must match what the lane actually built"
    );
    ensure!(
        body.contains("--baseline-source-sha \"$SOURCE_SHA\"")
            && body.contains("--candidate-source-sha \"$SOURCE_SHA\""),
        "both variants must declare the one checkout SHA they were built from"
    );

    // Staging must stay inside the gitignored tree: `subject_complete` requires
    // a clean checkout, so a staging directory in the working tree would make
    // every measurement `not_proven`.
    for argument in [
        "--baseline-dir",
        "--candidate-dir",
        "--baseline-archive",
        "--candidate-archive",
        "--json",
        "--markdown",
    ] {
        let value = body
            .split_whitespace()
            .skip_while(|token| *token != argument)
            .nth(1)
            .ok_or_else(|| anyhow!("the comparison passes no `{argument}`"))?
            .trim_matches('"');
        ensure!(
            value.starts_with("target/shadow/"),
            "`{argument}` must stay under the gitignored staging tree, found `{value}`"
        );
    }

    ensure!(!content.contains("ubuntu-latest"), "the lane must not use a floating runner image");

    Ok(())
}

#[test]
fn shadow_lane_shell_survives_the_runner_bash() -> Result<()> {
    let (content, workflow) = workflow()?;

    // GitHub's macOS images still ship Bash 3.2, where expanding a
    // zero-element array under `set -u` is a fatal "unbound variable". This
    // repository's own CI runs Bash 5, which tolerates it, so the defect is
    // invisible to every local and Linux-hosted check and would only surface
    // on a real dispatch — after both release builds had been paid for.
    ensure!(
        !content.contains("=()"),
        "an empty array literal aborts this lane under the runner's Bash 3.2; build one \
         always-non-empty argv array instead"
    );

    for step in steps(&workflow)? {
        let body = run_body(step);
        if body.is_empty() {
            continue;
        }
        ensure!(
            body.contains("set -euo pipefail"),
            "every shell step must fail closed; a silent step yields a partial measurement"
        );
    }

    Ok(())
}
