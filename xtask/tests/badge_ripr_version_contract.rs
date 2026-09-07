//! Contract tying badge generation to the reviewed RIPR workflow release.

use std::{collections::BTreeSet, fs, path::PathBuf};

use serde_yaml_ng::Value;

// `seed-cache` (#12563) installs the same reviewed release to warm the rust
// caches; it executes no analysis, but it must stay version-aligned with the
// routed lanes, so the contract covers it too.
const EXPECTED_RIPR_EXECUTION_JOBS: &[&str] =
    &["ripr-cx53", "ripr-cx43", "ripr-github", "ripr-fallback", "seed-cache"];
const VARIABLE_INSTALL_COMMAND: &str = "cargo install ripr --version \"$RIPR_VERSION\" --locked";

fn project_root() -> PathBuf {
    let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    root.pop();
    root
}

fn collect_named_strings(value: &Value, key: &str, output: &mut Vec<String>) -> Result<(), String> {
    match value {
        Value::Mapping(mapping) => {
            for (mapping_key, child) in mapping {
                if mapping_key.as_str() == Some(key) {
                    let text =
                        child.as_str().ok_or_else(|| format!("`{key}` must be a YAML string"))?;
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

/// Extract exactly one reviewed-version token from `text` using `prefix`/`suffix`
/// as literal delimiters.
///
/// Fail-closed by construction: zero matches means the consumer stopped naming a
/// version in the shape this contract knows how to police, and more than one
/// means the file carries two independently editable pins. Both are drift, so
/// both are errors rather than a silently-skipped assertion.
fn sole_pinned_version(
    text: &str,
    prefix: &str,
    suffix: &str,
    label: &str,
) -> Result<String, String> {
    let found: Vec<String> = text
        .match_indices(prefix)
        .filter_map(|(start, _)| {
            let rest = &text[start + prefix.len()..];
            rest.find(suffix).map(|end| rest[..end].to_string())
        })
        .collect();
    match found.len() {
        1 => Ok(found[0].clone()),
        0 => Err(format!(
            "{label} declares no reviewed RIPR version matching `{prefix}…{suffix}`; \
             the version contract can no longer police this consumer"
        )),
        n => Err(format!("{label} declares {n} reviewed RIPR versions: {found:?}")),
    }
}

/// The single reviewed release declared by every routed RIPR execution lane.
fn reviewed_lane_version(ripr_workflow: &Value) -> Result<String, String> {
    let ripr_jobs = ripr_workflow
        .get("jobs")
        .and_then(Value::as_mapping)
        .ok_or("ripr.yml must declare jobs")?;
    let mut versions = BTreeSet::new();
    for job_name in EXPECTED_RIPR_EXECUTION_JOBS {
        let version = ripr_jobs
            .get(*job_name)
            .and_then(|job| job.get("env"))
            .and_then(Value::as_mapping)
            .and_then(|env| env.get("RIPR_VERSION"))
            .and_then(Value::as_str)
            .ok_or_else(|| {
                format!("RIPR execution job `{job_name}` must declare env.RIPR_VERSION")
            })?;
        versions.insert(version.to_string());
    }
    if versions.len() != 1 {
        return Err(format!(
            "routed RIPR execution lanes disagree on the reviewed release: {versions:?}"
        ));
    }
    versions.into_iter().next().ok_or_else(|| "no reviewed release".to_string())
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
-> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let ripr_workflow: Value =
        serde_yaml_ng::from_str(&fs::read_to_string(root.join(".github/workflows/ripr.yml"))?)?;
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

/// The badge *consumer* must accept exactly the release the routed lanes *produce*.
///
/// #13764 moved default-branch badge generation from an independent repository
/// scan to exact-receipt projection: `.github/workflows/ripr.yml` writes
/// `ripr-badge-producer.json` stamped with its own `$RIPR_VERSION`, and
/// `scripts/generate-badges.py` refuses a receipt whose `ripr_version` is not
/// `EXPECTED_RIPR_VERSION`.
///
/// That join was left unpoliced, and it fails *open* in the direction a version
/// migration actually moves: bumping every workflow lane while the Python
/// constant stays behind keeps the pre-existing workflow contract green, while
/// the normal default-branch badge path rejects every produced receipt as
/// unreviewed. The badge silently stops updating instead of failing loudly.
///
/// This is the same class as the recorded
/// `docs/learnings/2026-06-ripr-output-schema-break.md` regression — a producer
/// moved and a consumer kept matching against the old shape — so it is pinned
/// here rather than left to review attention.
#[test]
fn badge_consumer_accepts_exactly_the_reviewed_producer_release()
-> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let ripr_workflow: Value =
        serde_yaml_ng::from_str(&fs::read_to_string(root.join(".github/workflows/ripr.yml"))?)?;
    let reviewed_version = reviewed_lane_version(&ripr_workflow)?;

    let generator = fs::read_to_string(root.join("scripts/generate-badges.py"))?;
    let expected_by_consumer = sole_pinned_version(
        &generator,
        "EXPECTED_RIPR_VERSION = \"",
        "\"",
        "scripts/generate-badges.py",
    )?;

    assert_eq!(
        expected_by_consumer, reviewed_version,
        "scripts/generate-badges.py accepts producer receipts stamped \
         {expected_by_consumer:?} while the routed RIPR lanes stamp {reviewed_version:?}; \
         every default-branch badge receipt would be rejected as unreviewed"
    );

    Ok(())
}

/// Documented install instructions must name the reviewed release too.
///
/// These are the surfaces a contributor copies to reproduce a gate locally, and
/// they rot silently: `docs/agents/SPEC_UPDATE_CHECKLIST.md` was still naming
/// `RIPR_VERSION=0.5.0` while the routed lanes had already moved twice, so a
/// contributor following it would compare local output against a different
/// analyzer than the one deciding the required check.
#[test]
fn documented_ripr_install_instructions_name_the_reviewed_release()
-> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let ripr_workflow: Value =
        serde_yaml_ng::from_str(&fs::read_to_string(root.join(".github/workflows/ripr.yml"))?)?;
    let reviewed_version = reviewed_lane_version(&ripr_workflow)?;

    let ci_doc = fs::read_to_string(root.join("docs/ci/ripr.md"))?;
    let documented_install = sole_pinned_version(
        &ci_doc,
        "cargo install ripr --version ",
        " --locked",
        "docs/ci/ripr.md",
    )?;
    assert_eq!(
        documented_install, reviewed_version,
        "docs/ci/ripr.md documents installing RIPR {documented_install:?} while the routed \
         lanes run {reviewed_version:?}"
    );

    let checklist = fs::read_to_string(root.join("docs/agents/SPEC_UPDATE_CHECKLIST.md"))?;
    let documented_pin = sole_pinned_version(
        &checklist,
        "RIPR pin: `RIPR_VERSION=",
        "`",
        "docs/agents/SPEC_UPDATE_CHECKLIST.md",
    )?;
    assert_eq!(
        documented_pin, reviewed_version,
        "docs/agents/SPEC_UPDATE_CHECKLIST.md records the RIPR pin as {documented_pin:?} \
         while the routed lanes run {reviewed_version:?}"
    );

    Ok(())
}
