//! Fail-closed publication-ordering contract for #6231.

use anyhow::{Context, Result, bail, ensure};
use serde_yaml_ng::Value;
use std::path::PathBuf;

const PUBLISH_ENDPOINTS: [&str; 3] = [
    "publish-crates.yml/dispatches",
    "publish-extension.yml/dispatches",
    "docker-publish.yml/dispatches",
];

fn root() -> Result<PathBuf> {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(PathBuf::from)
        .context("xtask must have a repository parent")
}

fn workflow(name: &str) -> Result<Value> {
    let path = root()?.join(".github/workflows").join(name);
    let source =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    serde_yaml_ng::from_str(&source).with_context(|| format!("parsing {}", path.display()))
}

fn job<'a>(document: &'a Value, name: &str) -> Result<&'a Value> {
    document
        .get("jobs")
        .and_then(Value::as_mapping)
        .and_then(|jobs| jobs.get(Value::String(name.to_string())))
        .with_context(|| format!("missing workflow job `{name}`"))
}

fn needs(job: &Value) -> Result<Vec<&str>> {
    let value = job.get("needs").context("publisher job must declare needs")?;
    if let Some(single) = value.as_str() {
        return Ok(vec![single]);
    }
    value
        .as_sequence()
        .context("needs must be a string or sequence")?
        .iter()
        .map(|item| item.as_str().context("needs item must be a string"))
        .collect()
}

fn rendered(value: &Value) -> Result<String> {
    serde_yaml_ng::to_string(value).context("rendering workflow fragment")
}

fn validate_release_graph(document: &Value) -> Result<()> {
    let candidate = job(document, "candidate")?;
    let publication = job(document, "publish-release")?;
    let publishers = job(document, "dispatch-publishers")?;

    ensure!(
        needs(publication)?.contains(&"candidate"),
        "GitHub Release can run without candidate success"
    );
    ensure!(
        needs(publishers)?.contains(&"publish-release"),
        "publisher dispatch can run before GitHub Release success"
    );

    let candidate_text = rendered(candidate)?;
    for required in [
        "release artifact-check",
        "Duplicate archive filename",
        "release_terminal_manifest.py",
        "actions/attest@",
        "release-terminal-candidate",
    ] {
        ensure!(
            candidate_text.contains(required),
            "candidate omits required predecessor `{required}`"
        );
    }
    ensure!(
        !candidate_text.contains("secrets."),
        "candidate construction must not receive publisher secrets"
    );
    ensure!(
        !candidate_text.contains("action-gh-release"),
        "candidate job performs a public release mutation"
    );

    let publication_text = rendered(publication)?;
    ensure!(
        publication_text.contains("release-terminal-candidate"),
        "publication does not consume terminal candidate"
    );
    ensure!(
        publication_text.contains("terminal manifest names another source"),
        "publication does not bind the terminal manifest to its exact source"
    );
    ensure!(
        publication_text.contains("terminal manifest names another tag"),
        "publication does not bind the terminal manifest to its exact tag"
    );
    ensure!(
        publication_text.contains("action-gh-release"),
        "publication job has no GitHub Release boundary"
    );

    let publisher_text = rendered(publishers)?;
    for endpoint in PUBLISH_ENDPOINTS {
        ensure!(publisher_text.contains(endpoint), "admitted publisher job omits `{endpoint}`");
    }

    let whole = rendered(document)?;
    for endpoint in PUBLISH_ENDPOINTS {
        ensure!(
            whole.matches(endpoint).count() == 1,
            "publisher endpoint `{endpoint}` exists outside its admitted job"
        );
    }
    Ok(())
}

#[test]
fn current_release_graph_is_fail_closed() -> Result<()> {
    validate_release_graph(&workflow("release.yml")?)
}

#[test]
fn failed_predecessor_has_zero_publisher_dispatch_surface() -> Result<()> {
    let orchestration = workflow("release-orchestration.yml")?;
    let text = rendered(&orchestration)?;
    for forbidden in [
        "git push origin",
        "action-gh-release",
        "publish-crates.yml/dispatches",
        "publish-extension.yml/dispatches",
        "docker-publish.yml/dispatches",
    ] {
        ensure!(
            !text.contains(forbidden),
            "orchestration mutates public state before candidate success: `{forbidden}`"
        );
    }
    ensure!(
        text.matches("release.yml/dispatches").count() == 1,
        "orchestration must dispatch exactly one transaction"
    );
    Ok(())
}

#[test]
fn removing_publication_dependency_is_rejected() -> Result<()> {
    let mut document = workflow("release.yml")?;
    let jobs = document
        .get_mut("jobs")
        .and_then(Value::as_mapping_mut)
        .context("jobs must be a mapping")?;
    let publishers = jobs
        .get_mut(Value::String("dispatch-publishers".to_string()))
        .and_then(Value::as_mapping_mut)
        .context("dispatch-publishers must be a mapping")?;
    publishers.remove(Value::String("needs".to_string()));
    if validate_release_graph(&document).is_ok() {
        bail!("dependency-removal falsifier unexpectedly remained eligible");
    }
    Ok(())
}
