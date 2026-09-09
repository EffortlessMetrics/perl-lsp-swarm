//! Fail-closed publication-ordering contract for #6231.

use anyhow::{Context, Result, bail, ensure};
use serde_yaml_ng::Value;
use std::collections::{BTreeMap, BTreeSet};
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
    let eligibility = job(document, "publisher-eligibility")?;
    let publication = job(document, "publish-release")?;
    let publishers = job(document, "dispatch-publishers")?;

    ensure!(
        needs(publication)?.contains(&"candidate"),
        "GitHub Release can run without candidate success"
    );
    ensure!(
        needs(publication)?.contains(&"publisher-eligibility"),
        "GitHub Release can run without the common publisher eligibility fan-in"
    );
    ensure!(
        needs(publishers)?.contains(&"publish-release"),
        "publisher dispatch can run before GitHub Release success"
    );
    ensure!(
        needs(publishers)?.contains(&"publisher-eligibility"),
        "publisher dispatch can run without the common publisher eligibility fan-in"
    );

    let candidate_text = rendered(candidate)?;
    let producer_text =
        std::fs::read_to_string(root()?.join("scripts/release_terminal_manifest.py"))
            .context("reading terminal manifest producer")?;
    ensure!(
        producer_text
            .contains("output = candidate / \"dist\" / \"release-terminal-manifest.json\""),
        "terminal manifest producer does not declare the canonical output path"
    );
    for required in [
        "release artifact-check",
        "Duplicate archive filename",
        "release_terminal_manifest.py",
        "actions/attest@",
        "release-terminal-candidate",
        "subject-checksums",
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

    let eligibility_text = rendered(eligibility)?;
    for required in [
        "release_terminal_manifest.py",
        "--check",
        "dist/release-terminal-manifest.json",
        "attestation-subjects.sha256",
        "manifest_sha256",
        "candidate_run_id",
    ] {
        ensure!(
            eligibility_text.contains(required),
            "publisher eligibility omits required binding `{required}`"
        );
    }
    ensure!(
        !eligibility_text.contains("secrets."),
        "publisher eligibility must not receive publisher secrets"
    );

    let publication_text = rendered(publication)?;
    ensure!(
        publication_text.contains("release-terminal-candidate"),
        "publication does not consume terminal candidate"
    );
    ensure!(
        publication_text.contains("release_terminal_manifest.py")
            && publication_text.contains("SOURCE_SHA")
            && publication_text.contains("--check"),
        "publication does not bind the terminal manifest to its exact source"
    );
    ensure!(
        publication_text.contains("TAG"),
        "publication does not bind the terminal manifest to its exact tag"
    );
    ensure!(
        publication_text.contains("action-gh-release"),
        "publication job has no GitHub Release boundary"
    );
    for required in ["git/refs", "refs/tags/", "object.sha", "SOURCE_SHA"] {
        ensure!(
            publication_text.contains(required),
            "publication does not atomically bind the tag: `{required}`"
        );
    }

    let publisher_text = rendered(publishers)?;
    for endpoint in PUBLISH_ENDPOINTS {
        ensure!(publisher_text.contains(endpoint), "admitted publisher job omits `{endpoint}`");
    }
    for required in ["EXPECTED_SHA", "CANDIDATE_RUN_ID", "MANIFEST_SHA256"] {
        ensure!(
            publisher_text.contains(required),
            "publisher dispatch does not carry exact eligibility binding `{required}`"
        );
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

fn permission_is_write(job: &Value) -> bool {
    job.get("permissions").and_then(Value::as_mapping).is_some_and(|permissions| {
        permissions.values().any(|value| value.as_str() == Some("write"))
    })
}

fn structurally_simulate_failed_candidate(document: &Value) -> Result<(usize, usize, usize)> {
    let jobs =
        document.get("jobs").and_then(Value::as_mapping).context("jobs must be a mapping")?;
    let mut status = BTreeMap::from([
        ("release-metadata".to_string(), "success"),
        ("build".to_string(), "success"),
        ("candidate".to_string(), "failure"),
    ]);
    let mut entered = BTreeSet::new();
    loop {
        let mut changed = false;
        for (name, value) in jobs {
            let name = name.as_str().context("job name must be a string")?;
            if status.contains_key(name) {
                continue;
            }
            let dependencies = needs(value)?;
            if dependencies.iter().all(|dependency| status.contains_key(*dependency)) {
                let outcome = if dependencies
                    .iter()
                    .all(|dependency| status.get(*dependency) == Some(&"success"))
                {
                    entered.insert(name.to_string());
                    "success"
                } else {
                    "skipped"
                };
                status.insert(name.to_string(), outcome);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    let mut dispatches = 0;
    let mut credential_entries = 0;
    let mut public_mutations = 0;
    for name in entered {
        let value = job(document, &name)?;
        let text = rendered(value)?;
        if permission_is_write(value) {
            credential_entries += 1;
        }
        dispatches += PUBLISH_ENDPOINTS.iter().filter(|endpoint| text.contains(**endpoint)).count();
        if text.contains("action-gh-release") || text.contains("/git/refs") {
            public_mutations += 1;
        }
    }
    Ok((dispatches, credential_entries, public_mutations))
}

#[test]
fn current_release_graph_is_fail_closed() -> Result<()> {
    let document = workflow("release.yml")?;
    let rendered_workflow = rendered(&document)?;
    ensure!(
        rendered_workflow.contains("Hosted zero-public-mutation remains NOT_PROVEN")
            && rendered_workflow.contains("#8576"),
        "workflow overclaims structural reachability as hosted runtime proof"
    );
    validate_release_graph(&document)
}

#[test]
fn orchestration_structurally_has_no_early_publisher_or_public_mutation_surface() -> Result<()> {
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
fn failed_candidate_structural_simulation_reaches_no_publisher_surface() -> Result<()> {
    let observed = structurally_simulate_failed_candidate(&workflow("release.yml")?)?;
    ensure!(
        observed == (0, 0, 0),
        "structural failed-candidate graph reached dispatch/write/public-mutation surfaces: {observed:?}"
    );
    Ok(())
}

#[test]
fn tag_authority_and_currentness_precede_public_release() -> Result<()> {
    let publication = rendered(job(&workflow("release.yml")?, "publish-release")?)?;
    ensure!(
        publication.matches("release_tag_authority.py").count() >= 3,
        "tag authority is not checked before creation and again before publication"
    );
    let currentness = publication
        .find("Revalidate immutable tag currentness before public release")
        .context("missing pre-publication tag currentness step")?;
    let release =
        publication.find("action-gh-release").context("missing public release mutation")?;
    ensure!(currentness < release, "public release precedes tag-currentness proof");
    Ok(())
}

#[test]
fn parent_sha_is_bound_to_child_dispatch_and_child_subject() -> Result<()> {
    let parent = rendered(&workflow("release-orchestration.yml")?)?;
    let child = rendered(&workflow("release.yml")?)?;
    for required in ["subject_sha", "inputs[expected_sha]"] {
        ensure!(parent.contains(required), "parent omits exact-SHA binding `{required}`");
    }
    for required in ["expected_sha", "github.sha", "another source SHA"] {
        ensure!(child.contains(required), "child omits exact-SHA rejection `{required}`");
    }
    Ok(())
}

fn validate_no_release_trigger(document: &Value) -> Result<()> {
    let events = document.get("on").context("publisher must declare events")?;
    let has_release = match events {
        Value::String(name) => name == "release",
        Value::Sequence(names) => names.iter().any(|name| name.as_str() == Some("release")),
        Value::Mapping(mapping) => mapping.contains_key(Value::String("release".into())),
        _ => bail!("publisher event declaration must be a string, sequence or mapping"),
    };
    ensure!(!has_release, "release event can bypass the ordered graph");
    Ok(())
}

#[test]
fn publishers_have_no_release_event_bypass() -> Result<()> {
    for name in ["publish-crates.yml", "publish-extension.yml", "docker-publish.yml"] {
        let document = workflow(name)?;
        validate_no_release_trigger(&document)?;
        ensure!(
            !rendered(&document)?.contains("event.release"),
            "{name} still consumes release event authority"
        );
        for events in ["release", "[workflow_dispatch, release]", "{release: {types: [published]}}"]
        {
            let mut mutant = document.clone();
            mutant["on"] = serde_yaml_ng::from_str(events)?;
            ensure!(
                validate_no_release_trigger(&mutant).is_err(),
                "{name} admitted release-event bypass in {events}"
            );
        }
    }
    Ok(())
}

#[test]
fn downstream_publishers_require_the_exact_eligibility_handoff() -> Result<()> {
    for workflow_name in ["publish-crates.yml", "publish-extension.yml", "docker-publish.yml"] {
        let text = rendered(&workflow(workflow_name)?)?;
        for required in [
            "expected_sha",
            "candidate_run_id",
            "manifest_sha256",
            "release-terminal-candidate",
            "dist/release-terminal-manifest.json",
            "actual_manifest_sha256",
            "gh run view",
            "workflowName",
            "headSha",
            "conclusion",
            ".source_sha",
        ] {
            ensure!(
                text.contains(required),
                "{workflow_name} does not validate the exact eligibility handoff `{required}`"
            );
        }
    }
    Ok(())
}

#[test]
fn occupied_tag_probe_fails_closed_on_transport_error() -> Result<()> {
    let text = rendered(&workflow("release.yml")?)?;
    ensure!(
        text.contains("refs=\"$(git ls-remote --tags origin")
            && !text.contains("git ls-remote --exit-code --tags origin"),
        "occupied-tag probe must distinguish transport failure from an absent tag"
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

// Replay the actual Linux gate scripts with controlled producer observations.
// No workflow dispatch, artifact download, or publication occurs in this proof.
#[cfg(target_os = "linux")]
#[test]
fn publisher_handoff_waits_for_exact_terminal_producer() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    let expected = "a".repeat(40);
    for (workflow_name, job_name) in [
        ("publish-crates.yml", "compute-order"),
        ("publish-extension.yml", "resolve-trusted-anchor"),
        ("docker-publish.yml", "verify-release-eligibility"),
    ] {
        let document = workflow(workflow_name)?;
        let steps = job(&document, job_name)?
            .get("steps")
            .and_then(Value::as_sequence)
            .context("publisher steps")?;
        let script = steps
            .iter()
            .filter_map(|step| step.get("run").and_then(Value::as_str))
            .find(|script| script.contains("gh run view"))
            .context("producer admission script")?;
        let (admission, _) =
            script.split_once("gh run download").context("candidate download boundary")?;
        for (case, states, admitted) in [
            (
                "delayed success",
                vec![("in_progress", "", false), ("completed", "success", false)],
                true,
            ),
            ("failure", vec![("completed", "failure", false)], false),
            (
                "cancellation",
                vec![("in_progress", "", false), ("completed", "cancelled", false)],
                false,
            ),
            ("timeout", vec![("in_progress", "", false)], false),
            ("wrong subject", vec![("completed", "success", true)], false),
        ] {
            let dir = tempfile::tempdir()?;
            let observations: Vec<_> = states.iter().map(|(status, conclusion, wrong)| serde_json::json!({
                "workflowName": "Release", "headSha": if *wrong { "b".repeat(40) } else { expected.clone() },
                "status": status, "conclusion": conclusion, "event": "workflow_dispatch",
            })).collect();
            std::fs::write(dir.path().join("states.json"), serde_json::to_vec(&observations)?)?;
            let fake_gh = dir.path().join("gh");
            std::fs::write(
                &fake_gh,
                r#"#!/usr/bin/env python3
import json, os, pathlib, sys
root = pathlib.Path(os.environ['TEST_STATE_DIR'])
if sys.argv[1:4] != ['run', 'view', '123']:
    sys.exit('unexpected gh command')
count = root / 'count'
n = int(count.read_text()) if count.exists() else 0
states = json.loads((root / 'states.json').read_text())
count.write_text(str(n + 1))
print(json.dumps(states[min(n, len(states) - 1)]))
"#,
            )?;
            std::fs::set_permissions(&fake_gh, std::fs::Permissions::from_mode(0o755))?;
            let fake_sleep = dir.path().join("sleep");
            std::fs::write(&fake_sleep, "#!/bin/sh\nexit 0\n")?;
            std::fs::set_permissions(&fake_sleep, std::fs::Permissions::from_mode(0o755))?;
            let inherited_path =
                std::env::var_os("PATH").context("PATH required for shell proof")?;
            let mut paths = vec![dir.path().to_path_buf()];
            paths.extend(std::env::split_paths(&inherited_path));
            let output = Command::new("bash")
                .arg("-c")
                .arg(admission)
                .env("PATH", std::env::join_paths(paths)?)
                .env("TEST_STATE_DIR", dir.path())
                .env("GITHUB_SHA", &expected)
                .env("EXPECTED_SHA", &expected)
                .env("MANIFEST_SHA256", "c".repeat(64))
                .env("CANDIDATE_RUN_ID", "123")
                .env("GITHUB_REPOSITORY", "fixture/repository")
                .env("GH_TOKEN", "")
                .output()?;
            ensure!(
                output.status.success() == admitted,
                "{workflow_name}: {case}: expected admission={admitted}, stderr={}",
                String::from_utf8_lossy(&output.stderr)
            );
            let calls: usize = std::fs::read_to_string(dir.path().join("count"))?.parse()?;
            if admitted {
                ensure!(calls == 2, "admitted before observing delayed producer success");
            }
            ensure!(calls <= 36, "producer wait exceeded its finite observation bound");
        }
    }
    Ok(())
}
