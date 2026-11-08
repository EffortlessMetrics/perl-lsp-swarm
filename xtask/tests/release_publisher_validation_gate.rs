//! Publisher-admission contract for #6231 / #6055.
//!
//! Across every release workflow that can mutate a public channel, no publisher
//! job may start unless the archive, checksum, nonempty-SBOM, VSIX and
//! exact-eligibility validations it depends on succeeded, and publisher
//! credentials stay scoped to the publishing step rather than the job or the
//! workflow.
//!
//! The forced-failure check below is a structural dry run of GitHub's job
//! scheduling: one validation job is forced to fail, every other job that can
//! still be scheduled is assumed to succeed, and any job whose `if:` uses a
//! status function (`always()`, `failure()`, `cancelled()`) or any other
//! condition is conservatively treated as entered. A publisher counts as
//! reached if it is entered. This is source-level proof only; hosted
//! zero-public-mutation evidence remains owned by #9422 / #8576.

use anyhow::{Context, Result, bail, ensure};
use serde_yaml_ng::{Mapping, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

/// Workflow → validation jobs every publisher in it must transitively need.
const GATES: [(&str, &[&str]); 4] = [
    ("release.yml", &["build", "candidate", "publisher-eligibility"]),
    ("publish-crates.yml", &["compute-order"]),
    ("publish-extension.yml", &["resolve-trusted-anchor", "prepare-vsix"]),
    (
        "docker-publish.yml",
        &["verify-release-eligibility", "resolve-trusted-anchor", "build-perl-runtime"],
    ),
];

/// Evidence that each named validation job still performs its validation, so
/// a renamed or emptied job cannot keep the graph check vacuously green.
const VALIDATION_EVIDENCE: [(&str, &str, &[&str]); 7] = [
    (
        "release.yml",
        "candidate",
        &[
            "release artifact-check",
            "SHA256SUMS",
            "test -s candidate/dist/sbom-spdx.json",
            "release_terminal_manifest.py",
            "--check",
        ],
    ),
    ("release.yml", "publisher-eligibility", &["release_terminal_manifest.py", "--check"]),
    ("publish-crates.yml", "compute-order", &["actual_manifest_sha256", ".source_sha"]),
    ("publish-extension.yml", "resolve-trusted-anchor", &["actual_manifest_sha256", ".source_sha"]),
    ("publish-extension.yml", "prepare-vsix", &["verify:marketplace", "Expected one VSIX package"]),
    (
        "docker-publish.yml",
        "verify-release-eligibility",
        &["actual_manifest_sha256", ".source_sha"],
    ),
    ("docker-publish.yml", "build-perl-runtime", &["build-push-action"]),
];

/// Commands that mutate a public channel or start another publisher.
const PUBLISH_COMMANDS: [&str; 7] = [
    "cargo publish",
    "vsce publish",
    "ovsx publish",
    "/dispatches",
    "gh workflow run",
    "action-gh-release",
    "gh release",
];

/// Write scopes that sign provenance rather than publish to a channel.
const PROVENANCE_WRITE_SCOPES: [&str; 3] = ["id-token", "attestations", "artifact-metadata"];

const STATUS_FUNCTIONS: [&str; 3] = ["always()", "failure()", "cancelled()"];

fn workflow(name: &str) -> Result<Value> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .context("xtask must have a repository parent")?
        .join(".github/workflows")
        .join(name);
    let source =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    serde_yaml_ng::from_str(&source).with_context(|| format!("parsing {}", path.display()))
}

fn jobs(document: &Value) -> Result<&Mapping> {
    document.get("jobs").and_then(Value::as_mapping).context("jobs must be a mapping")
}

fn jobs_mut(document: &mut Value) -> Result<&mut Mapping> {
    document.get_mut("jobs").and_then(Value::as_mapping_mut).context("jobs must be a mapping")
}

fn job_mut<'a>(document: &'a mut Value, name: &str) -> Result<&'a mut Mapping> {
    jobs_mut(document)?
        .get_mut(Value::String(name.to_string()))
        .and_then(Value::as_mapping_mut)
        .with_context(|| format!("missing job `{name}`"))
}

fn rendered(value: &Value) -> Result<String> {
    serde_yaml_ng::to_string(value).context("rendering workflow fragment")
}

fn needs(job: &Value) -> Result<Vec<String>> {
    match job.get("needs") {
        None => Ok(Vec::new()),
        Some(Value::String(single)) => Ok(vec![single.clone()]),
        Some(Value::Sequence(items)) => items
            .iter()
            .map(|item| item.as_str().map(str::to_string).context("needs item must be a string"))
            .collect(),
        Some(other) => bail!("needs must be a string or sequence, found {other:?}"),
    }
}

fn is_publisher(job: &Value) -> Result<bool> {
    let text = rendered(job)?;
    let publishing_write =
        job.get("permissions").and_then(Value::as_mapping).is_some_and(|permissions| {
            permissions.iter().any(|(scope, level)| {
                level.as_str() == Some("write")
                    && !scope.as_str().is_some_and(|scope| PROVENANCE_WRITE_SCOPES.contains(&scope))
            })
        });
    Ok(text.contains("secrets.")
        || publishing_write
        || PUBLISH_COMMANDS.iter().any(|command| text.contains(command)))
}

fn publishers(document: &Value) -> Result<BTreeSet<String>> {
    let mut found = BTreeSet::new();
    for (name, job) in jobs(document)? {
        if is_publisher(job)? {
            found.insert(name.as_str().context("job name must be a string")?.to_string());
        }
    }
    Ok(found)
}

fn ancestors(document: &Value, name: &str) -> Result<BTreeSet<String>> {
    let all = jobs(document)?;
    let mut seen = BTreeSet::new();
    let mut frontier = vec![name.to_string()];
    while let Some(current) = frontier.pop() {
        let job = all
            .get(Value::String(current.clone()))
            .with_context(|| format!("`{current}` names a missing job"))?;
        for dependency in needs(job)? {
            if seen.insert(dependency.clone()) {
                frontier.push(dependency);
            }
        }
    }
    Ok(seen)
}

/// Conservative replay of GitHub job scheduling with `failed` forced to fail.
/// Returns the set of jobs that may be entered.
fn simulate(document: &Value, failed: Option<&str>) -> Result<BTreeSet<String>> {
    let all = jobs(document)?;
    let mut outcome: BTreeMap<String, bool> = BTreeMap::new();
    let mut entered = BTreeSet::new();
    loop {
        let mut changed = false;
        for (name, job) in all {
            let name = name.as_str().context("job name must be a string")?;
            if outcome.contains_key(name) {
                continue;
            }
            let dependencies = needs(job)?;
            if !dependencies.iter().all(|dependency| outcome.contains_key(dependency)) {
                continue;
            }
            let condition = job.get("if").map(rendered).transpose()?.unwrap_or_default();
            let status_override =
                STATUS_FUNCTIONS.iter().any(|function| condition.contains(function));
            let needs_succeeded =
                dependencies.iter().all(|dependency| outcome.get(dependency) == Some(&true));
            let runs = status_override || needs_succeeded;
            if runs {
                entered.insert(name.to_string());
            }
            outcome.insert(name.to_string(), runs && Some(name) != failed);
            changed = true;
        }
        if !changed {
            break;
        }
    }
    ensure!(outcome.len() == all.len(), "job graph has a cycle or dangling dependency");
    Ok(entered)
}

/// Every publisher depends on every named validation, every transitive
/// predecessor of a publisher blocks it on failure, and the success path still
/// reaches every publisher.
fn validate_gate(name: &str, document: &Value, gates: &[&str]) -> Result<()> {
    let publishers = publishers(document)?;
    ensure!(!publishers.is_empty(), "{name}: no publisher surface detected");

    let success = simulate(document, None)?;
    for publisher in &publishers {
        ensure!(
            success.contains(publisher),
            "{name}: publisher `{publisher}` is unreachable on the success path, so the \
             failure checks below would be vacuous"
        );
    }

    for publisher in &publishers {
        let predecessors = ancestors(document, publisher)?;
        for gate in gates {
            ensure!(
                predecessors.contains(*gate),
                "{name}: publisher `{publisher}` does not depend on validation `{gate}`"
            );
        }
        for predecessor in &predecessors {
            let entered = simulate(document, Some(predecessor))?;
            ensure!(
                !entered.contains(publisher),
                "{name}: publisher `{publisher}` still starts after `{predecessor}` failed"
            );
        }
    }

    for gate in gates {
        let job = jobs(document)?
            .get(Value::String((*gate).to_string()))
            .with_context(|| format!("{name}: missing validation job `{gate}`"))?;
        ensure!(
            !rendered(job)?.contains("secrets."),
            "{name}: validation job `{gate}` must not receive publisher credentials"
        );
    }
    Ok(())
}

/// `secrets.*` may appear only in a step's `env` or `with`, never in
/// workflow/job `env`, job/step `if`, `container`, `services` or inline `run`.
fn validate_secret_scope(name: &str, document: &Value) -> Result<()> {
    let mut outside_jobs = document.clone();
    if let Some(mapping) = outside_jobs.as_mapping_mut() {
        mapping.remove(Value::String("jobs".to_string()));
    }
    ensure!(
        !rendered(&outside_jobs)?.contains("secrets."),
        "{name}: workflow-level surfaces expose publisher credentials"
    );
    for (job_name, job) in jobs(document)? {
        let job_name = job_name.as_str().context("job name must be a string")?;
        let mapping = job.as_mapping().context("job must be a mapping")?;
        for (key, value) in mapping {
            if key.as_str() == Some("steps") {
                continue;
            }
            ensure!(
                !rendered(value)?.contains("secrets."),
                "{name}: job `{job_name}` exposes credentials at job level via `{key:?}`"
            );
        }
        let steps = job.get("steps").and_then(Value::as_sequence).cloned().unwrap_or_default();
        for step in steps {
            let step = step.as_mapping().context("step must be a mapping")?;
            for (key, value) in step {
                if matches!(key.as_str(), Some("env" | "with")) {
                    continue;
                }
                ensure!(
                    !rendered(value)?.contains("secrets."),
                    "{name}: job `{job_name}` step exposes credentials via `{key:?}`"
                );
            }
        }
    }
    Ok(())
}

fn validate_evidence(
    document: &Value,
    workflow_name: &str,
    job_name: &str,
    required: &[&str],
) -> Result<()> {
    let job = jobs(document)?
        .get(Value::String(job_name.to_string()))
        .with_context(|| format!("{workflow_name}: missing validation job `{job_name}`"))?;
    let text = rendered(job)?;
    for evidence in required {
        ensure!(
            text.contains(evidence),
            "{workflow_name}: validation job `{job_name}` no longer performs `{evidence}`"
        );
    }
    Ok(())
}

/// A dry run of the crates publisher must not receive the registry token.
fn validate_crates_dry_run_token(document: &Value) -> Result<()> {
    let steps = jobs(document)?
        .get(Value::String("publish".to_string()))
        .and_then(|job| job.get("steps"))
        .and_then(Value::as_sequence)
        .context("publish-crates.yml: publish job steps")?;
    let mut bearers = steps.iter().filter_map(|step| {
        step.get("env").and_then(|env| env.get("CARGO_REGISTRY_TOKEN")).and_then(Value::as_str)
    });
    let token = bearers.next().context("publish-crates.yml: no CARGO_REGISTRY_TOKEN step")?;
    ensure!(
        bearers.next().is_none(),
        "publish-crates.yml: registry token mounted in several steps"
    );
    let compact: String = token.chars().filter(|c| !c.is_whitespace()).collect();
    ensure!(
        compact == "${{github.event.inputs.dry_run!='true'&&secrets.CARGO_REGISTRY_TOKEN||''}}",
        "publish-crates.yml: dry runs must not receive CARGO_REGISTRY_TOKEN, found `{token}`"
    );
    Ok(())
}

#[test]
fn every_publisher_waits_for_its_validations() -> Result<()> {
    for (name, gates) in GATES {
        validate_gate(name, &workflow(name)?, gates)?;
    }
    Ok(())
}

#[test]
fn validation_jobs_still_validate() -> Result<()> {
    for (workflow_name, job_name, required) in VALIDATION_EVIDENCE {
        validate_evidence(&workflow(workflow_name)?, workflow_name, job_name, required)?;
    }
    Ok(())
}

#[test]
fn publisher_credentials_are_step_scoped() -> Result<()> {
    for (name, _) in GATES {
        validate_secret_scope(name, &workflow(name)?)?;
    }
    validate_crates_dry_run_token(&workflow("publish-crates.yml")?)
}

#[test]
fn forced_validation_failure_skips_every_publisher() -> Result<()> {
    // The explicit dry run named by the brief: each named validation fails in
    // turn, and the replay reports which publishers would still start.
    for (name, gates) in GATES {
        let document = workflow(name)?;
        let publishers = publishers(&document)?;
        for gate in gates {
            let entered = simulate(&document, Some(gate))?;
            let reached: Vec<_> = publishers.intersection(&entered).collect();
            ensure!(reached.is_empty(), "{name}: `{gate}` failed but {reached:?} still start");
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifiers: each mutation must be rejected, proving the checks discriminate.

fn mutated(name: &str, edit: impl FnOnce(&mut Value) -> Result<()>) -> Result<Value> {
    let mut document = workflow(name)?;
    edit(&mut document)?;
    Ok(document)
}

fn gates_for(name: &str) -> Result<&'static [&'static str]> {
    GATES.iter().find(|(workflow, _)| *workflow == name).map(|(_, gates)| *gates).context("gates")
}

#[test]
fn always_on_a_publisher_is_rejected() -> Result<()> {
    for (name, publisher) in [
        ("publish-extension.yml", "publish-vscode-marketplace"),
        ("release.yml", "dispatch-publishers"),
        ("docker-publish.yml", "publish-dockerhub"),
    ] {
        let document = mutated(name, |document| {
            job_mut(document, publisher)?.insert("if".into(), "always()".into());
            Ok(())
        })?;
        ensure!(
            validate_gate(name, &document, gates_for(name)?).is_err(),
            "{name}: `if: always()` on `{publisher}` was admitted"
        );
    }
    Ok(())
}

#[test]
fn dropping_a_validation_dependency_is_rejected() -> Result<()> {
    for (name, publisher, kept) in [
        ("publish-extension.yml", "publish-open-vsx", vec!["resolve-trusted-anchor"]),
        ("publish-crates.yml", "publish", vec![]),
        ("release.yml", "publish-release", vec!["release-metadata"]),
        (
            "docker-publish.yml",
            "publish-ghcr",
            vec!["init", "resolve-trusted-anchor", "build-perl-runtime"],
        ),
    ] {
        let document = mutated(name, |document| {
            let needs: Vec<Value> = kept.iter().map(|need| Value::from(*need)).collect();
            job_mut(document, publisher)?.insert("needs".into(), Value::Sequence(needs));
            Ok(())
        })?;
        ensure!(
            validate_gate(name, &document, gates_for(name)?).is_err(),
            "{name}: `{publisher}` without its validation dependency was admitted"
        );
    }
    Ok(())
}

#[test]
fn removed_vsix_or_sbom_validation_is_rejected() -> Result<()> {
    for (name, job_name, needle) in [
        ("publish-extension.yml", "prepare-vsix", "verify:marketplace"),
        ("release.yml", "candidate", "test -s candidate/dist/sbom-spdx.json"),
        ("release.yml", "candidate", "release artifact-check"),
    ] {
        let source = rendered(&workflow(name)?)?.replace(needle, "true");
        let document: Value = serde_yaml_ng::from_str(&source)?;
        let (_, _, required) = VALIDATION_EVIDENCE
            .iter()
            .find(|(workflow, job, _)| *workflow == name && *job == job_name)
            .context("evidence row")?;
        ensure!(
            validate_evidence(&document, name, job_name, required).is_err(),
            "{name}: removing `{needle}` from `{job_name}` was admitted"
        );
    }
    Ok(())
}

#[test]
fn job_level_or_inline_credentials_are_rejected() -> Result<()> {
    let job_env = mutated("publish-extension.yml", |document| {
        let mut env = Mapping::new();
        env.insert("VSCE_PAT".into(), "${{ secrets.VSCE_PAT }}".into());
        job_mut(document, "publish-vscode-marketplace")?.insert("env".into(), Value::Mapping(env));
        Ok(())
    })?;
    ensure!(
        validate_secret_scope("publish-extension.yml", &job_env).is_err(),
        "job-level publisher credential was admitted"
    );

    let workflow_env = mutated("publish-crates.yml", |document| {
        let mut env = Mapping::new();
        env.insert("CARGO_REGISTRY_TOKEN".into(), "${{ secrets.CARGO_REGISTRY_TOKEN }}".into());
        document
            .as_mapping_mut()
            .context("workflow mapping")?
            .insert("env".into(), Value::Mapping(env));
        Ok(())
    })?;
    ensure!(
        validate_secret_scope("publish-crates.yml", &workflow_env).is_err(),
        "workflow-level publisher credential was admitted"
    );

    let inline = mutated("publish-extension.yml", |document| {
        let job = job_mut(document, "prepare-vsix")?;
        let steps = job.get_mut("steps").and_then(Value::as_sequence_mut).context("steps")?;
        let mut step = Mapping::new();
        step.insert("run".into(), "echo ${{ secrets.OVSX_PAT }}".into());
        steps.push(Value::Mapping(step));
        Ok(())
    })?;
    ensure!(
        validate_secret_scope("publish-extension.yml", &inline).is_err(),
        "inline run interpolation of a credential was admitted"
    );
    ensure!(
        validate_gate("publish-extension.yml", &inline, gates_for("publish-extension.yml")?)
            .is_err(),
        "a credential inside the VSIX validation job was admitted"
    );
    Ok(())
}

#[test]
fn unconditional_dry_run_token_is_rejected() -> Result<()> {
    let document = mutated("publish-crates.yml", |document| {
        let job = job_mut(document, "publish")?;
        let steps = job.get_mut("steps").and_then(Value::as_sequence_mut).context("steps")?;
        for step in steps {
            if let Some(env) = step.get_mut("env").and_then(Value::as_mapping_mut)
                && env.contains_key("CARGO_REGISTRY_TOKEN")
            {
                env.insert(
                    "CARGO_REGISTRY_TOKEN".into(),
                    "${{ secrets.CARGO_REGISTRY_TOKEN }}".into(),
                );
            }
        }
        Ok(())
    })?;
    ensure!(
        validate_crates_dry_run_token(&document).is_err(),
        "unconditional registry token on dry runs was admitted"
    );
    Ok(())
}
