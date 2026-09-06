//! Checked contract for the hosted `main` history-event detector workflow.
//!
//! #10306 requires the hosted surface to stay read-only, to delegate every
//! ancestry judgement to one repository command, and to treat a missing receipt
//! as a failure. Those properties are invisible to the Rust type system and are
//! exactly what a well-meaning later edit erodes, so they are asserted directly
//! against the workflow source.

use anyhow::{Context, Result, anyhow};
use serde_yaml_ng::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

fn workflow_path() -> Result<PathBuf> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest
        .parent()
        .with_context(|| format!("{} has no repository-root parent", manifest.display()))?;
    Ok(root.join(".github/workflows/main-history-event.yml"))
}

fn workflow_source() -> Result<String> {
    let path = workflow_path()?;
    fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))
}

/// Parse the workflow so structural properties are asserted against the document
/// GitHub will execute rather than against its current formatting.
fn workflow_document() -> Result<Value> {
    Ok(serde_yaml_ng::from_str(&workflow_source()?)?)
}

fn mapping_value<'a>(value: &'a Value, key: &str) -> Result<&'a Value> {
    value
        .as_mapping()
        .ok_or_else(|| anyhow!("expected YAML mapping while looking for `{key}`"))?
        .get(Value::String(key.to_string()))
        .ok_or_else(|| anyhow!("missing YAML key `{key}`"))
}

fn string_map(value: &Value) -> Result<BTreeMap<String, String>> {
    value
        .as_mapping()
        .ok_or_else(|| anyhow!("expected YAML mapping"))?
        .iter()
        .map(|(key, value)| {
            let key = key.as_str().ok_or_else(|| anyhow!("expected string key"))?.to_string();
            let value = value.as_str().ok_or_else(|| anyhow!("expected string value"))?.to_string();
            Ok((key, value))
        })
        .collect()
}

/// The detector's single job, by its declared id.
fn classify_job() -> Result<Value> {
    Ok(mapping_value(mapping_value(&workflow_document()?, "jobs")?, "classify")?.clone())
}

fn named_step(job: &Value, name: &str) -> Result<Value> {
    mapping_value(job, "steps")?
        .as_sequence()
        .ok_or_else(|| anyhow!("expected a step sequence"))?
        .iter()
        .find(|step| mapping_value(step, "name").ok().and_then(Value::as_str) == Some(name))
        .cloned()
        .ok_or_else(|| anyhow!("missing workflow step `{name}`"))
}

/// The detector must never gain mutation authority: the whole point of keeping
/// it out of `post-merge-status.yml` was to deny it a writer's trust boundary.
///
/// Asserted as an exact permission set rather than as the absence of a few
/// known-bad spellings. A denylist would admit every scope it forgot to name —
/// `id-token: write` alone would let the job mint an OIDC token for external
/// auth, which is nobody's idea of read-only.
#[test]
fn workflow_has_no_write_authority() -> Result<()> {
    let document = workflow_document()?;
    let expected = BTreeMap::from([("contents".to_string(), "read".to_string())]);

    let workflow_permissions = string_map(mapping_value(&document, "permissions")?)?;
    assert_eq!(
        workflow_permissions, expected,
        "workflow-level permissions must be exactly {{contents: read}}"
    );

    let job = classify_job()?;
    let job_permissions = string_map(mapping_value(&job, "permissions")?)?;
    assert_eq!(
        job_permissions, expected,
        "job-level permissions must be exactly {{contents: read}}"
    );

    let checkout = named_step(&job, "Checkout the exact event commit")?;
    let with = mapping_value(&checkout, "with")?;
    assert_eq!(
        mapping_value(with, "persist-credentials")?,
        &Value::Bool(false),
        "the checkout must not persist credentials"
    );
    Ok(())
}

/// Negative control: the workflow must not reacquire a private ancestry
/// interpretation. Reading `git fetch` prose or running a second merge-base
/// classifier in YAML is the exact failure that produced the false August 15
/// re-root diagnosis.
#[test]
fn workflow_delegates_every_ancestry_judgement_to_the_repository_command() -> Result<()> {
    let source = workflow_source()?;

    assert!(
        source.contains("--bin main-history-event"),
        "the workflow must invoke the repository command"
    );
    assert!(
        !source.contains("merge-base"),
        "ancestry interpretation belongs to xtask::git_ancestry, not to workflow YAML"
    );
    assert!(!source.contains("rev-list"), "the workflow must not compute its own history relation");
    assert!(
        !source.contains("git fetch"),
        "the workflow must not parse fetch output to decide history movement"
    );
    assert!(
        !source.contains("is-shallow-repository"),
        "shallow interpretation belongs to the shared ancestry authority"
    );
    Ok(())
}

/// A missing receipt is a failed observation. `warn` would let the detector go
/// green while publishing nothing.
#[test]
fn missing_receipt_upload_is_an_error_not_a_warning() -> Result<()> {
    let source = workflow_source()?;

    assert!(
        source.contains("if-no-files-found: error"),
        "a missing history-event receipt must fail the run"
    );
    assert!(
        !source.contains("if-no-files-found: warn"),
        "the detector must not downgrade a missing receipt to a warning"
    );
    assert!(
        !source.contains("if-no-files-found: ignore"),
        "the detector must not ignore a missing receipt"
    );
    Ok(())
}

/// The receipt has to survive the blocking verdict it explains, so the upload
/// must not be skipped when the classifier reports a rewrite.
/// Scoped to the upload step's own `if:` expression. A raw suffix search would
/// be satisfied by the *next* step's `always()` guard, so weakening the upload's
/// condition would silently stop publishing receipts for exactly the blocking
/// verdicts they explain, while this test stayed green.
#[test]
fn receipt_is_published_even_when_the_verdict_blocks() -> Result<()> {
    let job = classify_job()?;

    let upload = named_step(&job, "Publish the history-event receipt")?;
    let upload_condition = mapping_value(&upload, "if")?
        .as_str()
        .ok_or_else(|| anyhow!("the upload step's `if:` must be an expression string"))?
        .to_string();
    assert!(
        upload_condition.contains("always()"),
        "the receipt upload must run even after a blocking verdict, but its condition is \
         {upload_condition:?}"
    );

    let enforce = named_step(&job, "Enforce the detector verdict")?;
    let enforce_condition = mapping_value(&enforce, "if")?
        .as_str()
        .ok_or_else(|| anyhow!("the enforce step's `if:` must be an expression string"))?
        .to_string();
    assert!(
        enforce_condition.contains("always()"),
        "a blocking verdict must still fail the run after the receipt is published"
    );
    Ok(())
}

/// Each push carries an irreplaceable before/after subject, so no run may cancel
/// or displace another.
///
/// Any concurrency group is unsafe here, which is why the workflow declares
/// none. A group admits one running plus one pending run and a newly queued run
/// displaces the pending one, so `cancel-in-progress: false` does not save it.
/// Keying by `github.sha` is not sufficient either: `main` returning to the same
/// commit produces distinct push events sharing one SHA — and that repeated-reset
/// shape is exactly the force-push behaviour this lane exists to observe, so the
/// hole would open precisely when the evidence matters most.
#[test]
fn no_push_receipt_can_be_displaced_by_a_later_push() -> Result<()> {
    let source = workflow_source()?;

    // Match the YAML key itself, at any nesting depth, rather than any mention:
    // the surrounding comment explains why the key is absent and must not be
    // mistaken for the key.
    let declared: Vec<&str> = source
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            !trimmed.starts_with('#') && trimmed.starts_with("concurrency:")
        })
        .collect();

    assert!(
        declared.is_empty(),
        "any concurrency group can displace a pending run and silently drop that push's receipt, \
         but the workflow declares: {declared:?}"
    );
    Ok(())
}

/// The hosted check must describe the blocking class it actually got. Exit 3
/// (unprovable graph) and exit 5 (force reported over a *proven* fast-forward)
/// are opposite situations, so a shared branch would state one of them wrongly.
#[test]
fn every_blocking_exit_class_has_its_own_hosted_message() -> Result<()> {
    let job = classify_job()?;
    let enforce = named_step(&job, "Enforce the detector verdict")?;
    let script = mapping_value(&enforce, "run")?
        .as_str()
        .ok_or_else(|| anyhow!("the enforce step must carry a shell script"))?
        .to_string();

    for (class, expected) in
        [("2", "destructively"), ("3", "could NOT be verified"), ("5", "force push")]
    {
        assert!(
            script.contains(&format!("{class})")),
            "the enforce step must handle blocking exit class {class}"
        );
        assert!(
            script.contains(expected),
            "exit class {class} must be described as {expected:?}, not folded into another class"
        );
    }
    Ok(())
}

/// A `workflow_dispatch` run carries no push payload, so every manual run would
/// classify as `invalid_event` and fail. The detector is push-bound by nature.
#[test]
fn workflow_is_push_triggered_only() -> Result<()> {
    let source = workflow_source()?;

    assert!(
        !source.contains("workflow_dispatch:"),
        "a dispatch run has no push payload and would always fail as invalid_event"
    );
    assert!(
        source.contains("on:\n  push:\n    branches:\n      - main\n"),
        "the detector must observe pushes to main"
    );
    Ok(())
}

/// A shallow checkout cannot decide movement, so the detector must fetch the
/// complete graph and pin the immutable event commit rather than a moving tip.
#[test]
fn workflow_checks_out_the_complete_graph_at_the_exact_event_commit() -> Result<()> {
    let source = workflow_source()?;

    assert!(source.contains("fetch-depth: 0"), "the detector needs the complete graph");
    assert!(
        source.contains("ref: ${{ github.sha }}"),
        "the detector must pin the immutable event commit"
    );
    Ok(())
}

/// The exact platform subjects must reach the command, and at step scope rather
/// than interpolated into the script body.
#[test]
fn exact_event_subjects_are_forwarded_at_step_scope() -> Result<()> {
    let source = workflow_source()?;

    for subject in [
        "EVENT_BEFORE: ${{ github.event.before }}",
        // The delivered payload subject, not `github.sha`: on a ref deletion
        // `github.sha` reverts to the default branch tip, which would put an
        // unrelated commit in the receipt.
        "EVENT_AFTER: ${{ github.event.after }}",
        "EVENT_REF: ${{ github.ref }}",
        "EVENT_FORCED: ${{ github.event.forced }}",
        "EVENT_CREATED: ${{ github.event.created }}",
        "EVENT_DELETED: ${{ github.event.deleted }}",
    ] {
        assert!(source.contains(subject), "the workflow must forward {subject}");
    }
    assert!(
        !source.contains("EVENT_AFTER: ${{ github.sha }}"),
        "`github.sha` is not the delivered after-subject on a ref deletion"
    );
    Ok(())
}
